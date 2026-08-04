use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;
use tale::admin::auth::{
    AccessTokenRecord, CredentialRecord, CredentialStore, MemoryCredentialStore, SecretValue,
    TokenManager, encode_record,
};
use tale::admin::client::{AdminClient, Endpoint};
use tale::admin::log_streaming::LogStreamReplacement;
use tale::domain::access_explorer::{AccessDecision, AccessQuestion, PolicySource};
use tale::domain::export::{ExportCollection, ExportDocument, ExportMetadata, ExportRow};
use tale::domain::flow::{
    AggregateDimension, FlowConnection, FlowFilter, FlowMessage, FlowMode, FlowSnapshot,
    FlowWindow, MAX_FLOW_MESSAGES, aggregate_checked, aggregate_checked_cancellable,
};
use tale::domain::health::{
    ApprovalState, HealthDevice, HealthResource, HealthRoute, HealthSnapshot,
    KEY_EXPIRY_WARNING_WINDOW, MAX_AFFECTED_RESOURCE_IDS, RelaySample, Severity,
    SourceFailureClass,
};
use tale::domain::log_stream::LogType;
use tale::domain::policy_workflow::PolicyDocument;
use tale::domain::saved_view::{
    FilterClause, FilterOperator, FilterValue, SavedView, SavedViewStore, SortDirection, SortTerm,
    ViewRegistry,
};
use tale::domain::secret_result::SecretBuffer;
use tale::domain::webhook::{DestinationType, SubscriptionSet, WebhookDraft, WebhookMutation};
use tale::export::{ExportFormat, ExportWriteError, write_atomic};
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn fake_response(
    status: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<(url::Url, Arc<tokio::sync::Mutex<String>>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let captured = Arc::new(tokio::sync::Mutex::new(String::new()));
    let captured_for_task = Arc::clone(&captured);
    let status = status.to_owned();
    let content_type = content_type.to_owned();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut request = vec![0_u8; 64 * 1024];
            let Ok(count) = stream.read(&mut request).await else {
                return;
            };
            if let Ok(mut value) = captured_for_task.try_lock() {
                *value = String::from_utf8_lossy(&request[..count]).to_string();
            }
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes()).await;
            let _ = stream.write_all(&body).await;
        }
    });
    let base =
        url::Url::parse(&format!("http://{address}/api/v2")).map_err(|error| error.to_string())?;
    Ok((base, captured))
}

async fn client_with_token(
    base_url: url::Url,
) -> Result<(AdminClient, tale::admin::auth::AccessToken), String> {
    let store = Arc::new(MemoryCredentialStore::default());
    let record = CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new("phase-eight-fixture-token"),
    });
    let encoded = encode_record(&record).map_err(|error| error.to_string())?;
    store
        .set("phase-eight", &encoded)
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::new(store, None);
    let token = manager
        .access_token("phase-eight", "phase-eight")
        .await
        .map_err(|error| error.to_string())?;
    let client = AdminClient::with_base_url(base_url, Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    Ok((client, token))
}

fn fixture_export_document(schema: ExportCollection, rows: Vec<ExportRow>) -> ExportDocument {
    let route = match schema {
        ExportCollection::Devices => "devices",
        ExportCollection::Users => "users",
        ExportCollection::Routes => "routes",
        ExportCollection::Dns => "dns",
        ExportCollection::CredentialMetadata => "credentials",
        ExportCollection::Audit => "activity",
        ExportCollection::HealthFindings => "overview",
        ExportCollection::FlowLogs => "activity",
    };
    ExportDocument {
        metadata: ExportMetadata {
            schema,
            schema_version: 1,
            tale_version: "tale/test".to_owned(),
            sources: vec![tale::domain::export::ExportSource {
                id: "fixture".to_owned(),
                observed_at: 1_754_275_200,
            }],
            observed_at: 1_754_275_200,
            route: route.to_owned(),
            active_filter: "none".to_owned(),
            active_sort: "stable_key".to_owned(),
            truncated: false,
            complete: true,
            export_timestamp: None,
        },
        rows,
    }
}

#[tokio::test]
async fn flow_contract_is_windowed_and_preserves_server_metadata() -> Result<(), String> {
    let body = include_bytes!("fixtures/admin/flows/network-flow.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", body).await?;
    let (client, token) = client_with_token(base_url).await?;
    let now = OffsetDateTime::parse(
        "2026-08-04T00:30:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|error| error.to_string())?;
    let window = FlowWindow::new(now - TimeDuration::hours(1), now, now)
        .map_err(|error| error.to_string())?;
    let response = client
        .get_network_flow_logs(&token, "example test/tailnet", &window)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(response.value.len(), 1);
    assert_eq!(response.value[0].node_id, "node-flow-a");
    assert_eq!(response.value[0].virtual_traffic[0].tx_bytes, 1_000);
    let request = capture.lock().await.clone();
    assert!(request.starts_with(
        "GET /api/v2/tailnet/example%20test%2Ftailnet/logging/network?start=2026-08-03T23%3A30%3A00Z&end=2026-08-04T00%3A30%3A00Z HTTP/1.1"
    ));
    Ok(())
}

#[tokio::test]
async fn webhook_inventory_redacts_secrets_and_create_returns_view_once_material()
-> Result<(), String> {
    let body = include_bytes!("fixtures/admin/webhooks/inventory.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", body).await?;
    let (client, token) = client_with_token(base_url).await?;
    let response = client
        .list_webhooks(&token, "example.test")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(response.value.len(), 1);
    assert_eq!(
        response.value[0].subscriptions.wire_events(),
        vec!["futureEventFromServer", "nodeCreated"]
    );
    assert!(!capture.lock().await.contains("futureEventFromServer"));

    let created = include_bytes!("fixtures/admin/webhooks/created.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", created).await?;
    let (client, token) = client_with_token(base_url).await?;
    let subscriptions = tale::domain::webhook::SubscriptionSet::from_wire(
        Vec::new(),
        vec!["nodeApproved".to_owned()],
    )
    .map_err(|error| error.to_string())?;
    let result = client
        .create_webhook(
            &token,
            "example.test",
            "https://hooks.example.test/new",
            &tale::domain::webhook::DestinationType::Discord,
            &subscriptions,
        )
        .await
        .map_err(|error| error.to_string())?;
    assert!(result.secret.is_some());
    let request = capture.lock().await.clone();
    let body = request.split_once("\r\n\r\n").map_or("", |parts| parts.1);
    let json = serde_json::from_str::<Value>(body).map_err(|error| error.to_string())?;
    assert_eq!(json["endpointUrl"], "https://hooks.example.test/new");
    assert_eq!(json["providerType"], "discord");
    assert_eq!(json["subscriptions"], serde_json::json!(["nodeApproved"]));
    assert!(!body.contains("fixture-secret"));
    Ok(())
}

#[tokio::test]
async fn log_stream_contract_uses_independent_reads_and_typed_replacement() -> Result<(), String> {
    let config = include_bytes!("fixtures/admin/log_streaming/configuration.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", config).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .get_log_stream_configuration(&token, "example.test", LogType::Configuration)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        result.value.destination.identity,
        "https://logs.example.test/ingest"
    );
    assert!(
        capture
            .lock()
            .await
            .starts_with("GET /api/v2/tailnet/example.test/logging/configuration/stream HTTP/1.1")
    );

    let (base_url, capture) = fake_response("200 OK", "application/json", Vec::new()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let replacement = LogStreamReplacement {
        log_type: LogType::Network,
        destination_type: "splunk".to_owned(),
        url: "https://logs.example.test/network".to_owned(),
        user: Some("fixture-user".to_owned()),
        upload_period_minutes: Some(5),
        compression_format: Some("zstd".to_owned()),
        token: None,
        s3_bucket: None,
        s3_region: None,
        s3_key_prefix: None,
        s3_authentication_type: None,
        s3_access_key_id: None,
        s3_role_arn: None,
        gcs_bucket: None,
        gcs_key_prefix: None,
        gcs_scopes: Vec::new(),
        gcs_credentials: None,
    };
    let result = client
        .replace_log_stream_configuration(&token, "example.test", &replacement)
        .await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(
        request.starts_with("PUT /api/v2/tailnet/example.test/logging/network/stream HTTP/1.1")
    );
    let body = request.split_once("\r\n\r\n").map_or("", |parts| parts.1);
    let json = serde_json::from_str::<Value>(body).map_err(|error| error.to_string())?;
    assert_eq!(json["destinationType"], "splunk");
    assert_eq!(json["url"], "https://logs.example.test/network");
    assert!(json.get("token").is_none());
    Ok(())
}

#[tokio::test]
async fn status_and_network_setting_contracts_preserve_independent_reads_and_partial_patch()
-> Result<(), String> {
    let status = include_bytes!("fixtures/admin/log_streaming/status.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", status).await?;
    let (client, token) = client_with_token(base_url).await?;
    let response = client
        .get_log_stream_status(&token, "example.test", LogType::Configuration)
        .await
        .map_err(|error| error.to_string())?;
    let expected = OffsetDateTime::parse(
        "2026-08-04T00:03:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        response.value.last_observation,
        u64::try_from(expected.unix_timestamp()).ok()
    );
    assert!(
        capture
            .lock()
            .await
            .starts_with("GET /api/v2/tailnet/example.test/logging/configuration/status HTTP/1.1")
    );

    let (base_url, capture) = fake_response(
        "200 OK",
        "application/json",
        br#"{"networkFlowLoggingOn":true}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let response = client
        .get_network_log_setting(&token, "example.test")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(response.value.network_flow_logging_on, Some(true));
    assert!(
        capture
            .lock()
            .await
            .starts_with("GET /api/v2/tailnet/example.test/settings HTTP/1.1")
    );

    let (base_url, capture) = fake_response(
        "200 OK",
        "application/json",
        br#"{"networkFlowLoggingOn":true}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let response = client
        .set_network_log_setting(&token, "example.test", true)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(response.value.network_flow_logging_on, Some(true));
    let request = capture.lock().await.clone();
    assert!(request.starts_with("PATCH /api/v2/tailnet/example.test/settings HTTP/1.1"));
    let body = request.split_once("\r\n\r\n").map_or("", |parts| parts.1);
    assert_eq!(body, r#"{"networkFlowLoggingOn":true}"#);
    Ok(())
}

#[tokio::test]
async fn phase_eight_contracts_reject_write_only_reads_and_preserve_unknown_edits()
-> Result<(), String> {
    let (base_url, _) = fake_response(
        "200 OK",
        "application/json",
        br#"{"logType":"network","destinationType":"gcs","gcsBucket":"fixture-bucket","gcsCredentials":"secret-must-not-be-read"}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .get_log_stream_configuration(&token, "example.test", LogType::Network)
        .await;
    assert!(result.is_err());
    if let Err(error) = result {
        assert!(!error.to_string().contains("secret-must-not-be-read"));
    }

    let (base_url, capture) = fake_response(
        "200 OK",
        "application/json",
        br#"{"endpointId":"webhook-fixture-1","endpointUrl":"https://hooks.example.test/tale","providerType":"slack","subscriptions":["nodeCreated","futureEventFromServer"]}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let subscriptions = SubscriptionSet::from_wire(
        vec!["device".to_owned()],
        vec![
            "futureEventFromServer".to_owned(),
            "nodeApproved".to_owned(),
        ],
    )
    .map_err(|error| error.to_string())?;
    client
        .edit_webhook_subscriptions(&token, "webhook-fixture-1", &subscriptions)
        .await
        .map_err(|error| error.to_string())?;
    let request = capture.lock().await.clone();
    assert!(request.starts_with("PATCH /api/v2/webhooks/webhook-fixture-1 HTTP/1.1"));
    let body = request.split_once("\r\n\r\n").map_or("", |parts| parts.1);
    let json = serde_json::from_str::<Value>(body).map_err(|error| error.to_string())?;
    assert_eq!(
        json["subscriptions"],
        serde_json::json!(["device", "futureEventFromServer", "nodeApproved"])
    );
    Ok(())
}

#[tokio::test]
async fn webhook_test_accepts_async_acknowledgement_without_delivery_claim() -> Result<(), String> {
    let (base_url, capture) = fake_response("202 Accepted", "application/json", Vec::new()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let response = client
        .test_webhook(&token, "webhook-fixture-1")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(response.meta.status, 202);
    assert!(
        capture
            .lock()
            .await
            .starts_with("POST /api/v2/webhooks/webhook-fixture-1/test HTTP/1.1")
    );
    Ok(())
}

#[test]
fn health_maximum_fixture_is_capped_and_deterministic() {
    let devices = (0..5_000)
        .map(|index| HealthDevice {
            stable_id: format!("device-{index:04}"),
            source_id: "fixture".to_owned(),
            key_expires_at: None,
            approval: ApprovalState::Approved,
            client_version: Some(if index % 2 == 0 {
                "1.0.0".to_owned()
            } else {
                "1.3.0".to_owned()
            }),
            posture_read_succeeded: false,
            posture_attributes_present: None,
        })
        .collect();
    let snapshot = HealthSnapshot {
        now: 1_000,
        devices,
        users: Vec::new(),
        resources: Vec::new(),
        routes: Vec::new(),
        posture_integration_enabled: false,
        relay_samples: Vec::new(),
    };
    let findings = snapshot.findings();
    let skew = findings
        .iter()
        .find(|finding| finding.rule_id == "client-version-skew");
    assert!(skew.is_some());
    if let Some(skew) = skew {
        assert_eq!(skew.affected_resource_ids.len(), MAX_AFFECTED_RESOURCE_IDS);
        assert_eq!(skew.truncated_affected_resource_count, 4_000);
        assert!(skew.derived);
    }
    assert_eq!(KEY_EXPIRY_WARNING_WINDOW, 604_800);
}

#[test]
fn health_rules_use_authoritative_thresholds_and_failure_classes() {
    let now = 100_000;
    let devices = vec![
        HealthDevice {
            stable_id: "expired".to_owned(),
            source_id: "devices".to_owned(),
            key_expires_at: Some(now),
            approval: ApprovalState::Pending,
            client_version: Some("1.0.0".to_owned()),
            posture_read_succeeded: true,
            posture_attributes_present: Some(false),
        },
        HealthDevice {
            stable_id: "expiring".to_owned(),
            source_id: "devices".to_owned(),
            key_expires_at: Some(now + KEY_EXPIRY_WARNING_WINDOW),
            approval: ApprovalState::Approved,
            client_version: Some("1.3.0".to_owned()),
            posture_read_succeeded: true,
            posture_attributes_present: Some(true),
        },
        HealthDevice {
            stable_id: "unparseable-version".to_owned(),
            source_id: "devices".to_owned(),
            key_expires_at: Some(now + KEY_EXPIRY_WARNING_WINDOW + 1),
            approval: ApprovalState::NotReturned,
            client_version: Some("vendor-current".to_owned()),
            posture_read_succeeded: false,
            posture_attributes_present: None,
        },
        HealthDevice {
            stable_id: "skewed".to_owned(),
            source_id: "devices".to_owned(),
            key_expires_at: None,
            approval: ApprovalState::Approved,
            client_version: Some("1.4.0".to_owned()),
            posture_read_succeeded: false,
            posture_attributes_present: None,
        },
    ];
    let snapshot = HealthSnapshot {
        now,
        devices,
        users: vec![tale::domain::health::HealthUser {
            stable_id: "user-pending".to_owned(),
            source_id: "users".to_owned(),
            approval: ApprovalState::Pending,
        }],
        resources: vec![
            HealthResource {
                stable_id: "source-warning".to_owned(),
                source_id: "warning".to_owned(),
                observed_at: now - 301,
                refresh_interval: 100,
                current: true,
                refresh_failures: 0,
                failure_class: None,
            },
            HealthResource {
                stable_id: "source-exact-three".to_owned(),
                source_id: "exact".to_owned(),
                observed_at: now - 300,
                refresh_interval: 100,
                current: true,
                refresh_failures: 0,
                failure_class: None,
            },
            HealthResource {
                stable_id: "source-critical".to_owned(),
                source_id: "critical".to_owned(),
                observed_at: now - 1_001,
                refresh_interval: 100,
                current: true,
                refresh_failures: 1,
                failure_class: Some(SourceFailureClass::Failed),
            },
            HealthResource {
                stable_id: "source-forbidden".to_owned(),
                source_id: "forbidden".to_owned(),
                observed_at: now - 10_001,
                refresh_interval: 100,
                current: true,
                refresh_failures: 1,
                failure_class: Some(SourceFailureClass::Forbidden),
            },
        ],
        routes: vec![
            HealthRoute {
                stable_id: "route-a".to_owned(),
                source_id: "routes".to_owned(),
                cidr: "10.0.0.0/24".to_owned(),
                advertiser_id: "device-a".to_owned(),
                approval: ApprovalState::Approved,
            },
            HealthRoute {
                stable_id: "route-b".to_owned(),
                source_id: "routes".to_owned(),
                cidr: "10.0.0.128/25".to_owned(),
                advertiser_id: "device-b".to_owned(),
                approval: ApprovalState::Pending,
            },
            HealthRoute {
                stable_id: "route-c".to_owned(),
                source_id: "routes".to_owned(),
                cidr: "10.0.1.0/24".to_owned(),
                advertiser_id: "device-a".to_owned(),
                approval: ApprovalState::Approved,
            },
        ],
        posture_integration_enabled: true,
        relay_samples: (0..5)
            .map(|index| RelaySample {
                source_id: "old-session".to_owned(),
                peer_id: "peer-old".to_owned(),
                relay: true,
                observed_at: index,
            })
            .chain((0..5).map(|index| RelaySample {
                source_id: "current-session".to_owned(),
                peer_id: "peer-current".to_owned(),
                relay: index != 4,
                observed_at: 100 + index,
            }))
            .collect(),
    };
    let findings = snapshot.findings();
    let rules = findings
        .iter()
        .map(|finding| finding.rule_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for rule in [
        "device-key-expired",
        "device-key-expiring",
        "device-approval-pending",
        "user-approval-pending",
        "source-stale",
        "source-failed",
        "route-overlap-review",
        "client-version-skew",
        "posture-observation-missing",
        "relay-heavy-local-peer",
    ] {
        assert!(rules.contains(rule), "missing rule {rule}");
    }
    assert!(!findings.iter().any(|finding| {
        finding.rule_id == "source-stale"
            && finding.affected_resource_ids == ["source-forbidden".to_owned()]
    }));
    assert!(findings.iter().any(|finding| {
        finding.rule_id == "source-failed"
            && finding
                .observed_facts
                .iter()
                .any(|fact| fact.label == "failure_class" && fact.value == "forbidden")
    }));
    assert!(findings.iter().all(|finding| finding.derived));
    let severity_rank = |severity: Severity| match severity {
        Severity::Critical => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    };
    assert!(findings.windows(2).all(|pair| {
        severity_rank(pair[0].severity) < severity_rank(pair[1].severity)
            || (severity_rank(pair[0].severity) == severity_rank(pair[1].severity)
                && (pair[0].rule_id.as_str(), &pair[0].affected_resource_ids)
                    <= (pair[1].rule_id.as_str(), &pair[1].affected_resource_ids))
    }));
}

#[test]
fn flow_message_boundary_and_cancellation_are_explicit() -> Result<(), String> {
    let now = OffsetDateTime::parse(
        "2026-08-04T00:30:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|error| error.to_string())?;
    let window = FlowWindow::new(now - TimeDuration::hours(1), now, now)
        .map_err(|error| error.to_string())?;
    let message = |index: usize| FlowMessage {
        node_id: format!("node-{index}"),
        reporting_node_name: None,
        logged: "2026-08-04T00:02:00Z".to_owned(),
        start: "2026-08-04T00:00:00Z".to_owned(),
        end: "2026-08-04T00:01:00Z".to_owned(),
        source_node: None,
        destination_nodes: Vec::new(),
        virtual_traffic: Vec::new(),
        subnet_traffic: Vec::new(),
        exit_traffic: Vec::new(),
        physical_traffic: Vec::new(),
    };
    let at_limit = (0..MAX_FLOW_MESSAGES).map(message).collect::<Vec<_>>();
    assert!(FlowSnapshot::from_messages(window.clone(), at_limit, FlowMode::Raw, 1).is_ok());
    let over_limit = (0..=MAX_FLOW_MESSAGES).map(message).collect::<Vec<_>>();
    assert_eq!(
        FlowSnapshot::from_messages(window, over_limit, FlowMode::Raw, 1),
        Err(tale::domain::flow::FlowError::MessageLimit)
    );
    let cancelled = AtomicBool::new(true);
    let result = aggregate_checked_cancellable(
        &[message(1)],
        &FlowFilter::default(),
        &[AggregateDimension::Protocol],
        Some(&cancelled),
    );
    assert_eq!(result, Err(tale::domain::flow::FlowError::Cancelled));
    Ok(())
}

#[test]
fn export_allowlist_handles_fifty_thousand_rows_deterministically() -> Result<(), String> {
    let rows = (0..50_000)
        .rev()
        .map(|index| ExportRow::Device {
            id: format!("device-{index:05}"),
            name: format!("fixture-{index:05}"),
            addresses: vec![format!("100.64.{}.{}", index / 256, index % 256)],
            source: "fixture".to_owned(),
            observed_at: 1_754_275_200,
        })
        .collect();
    let mut document = ExportDocument {
        metadata: ExportMetadata {
            schema: ExportCollection::Devices,
            schema_version: 1,
            tale_version: "tale/test".to_owned(),
            sources: Vec::new(),
            observed_at: 1_754_275_200,
            route: "devices".to_owned(),
            active_filter: "none".to_owned(),
            active_sort: "id:ascending".to_owned(),
            truncated: false,
            complete: true,
            export_timestamp: None,
        },
        rows,
    };
    document.sort_rows();
    let first = document.json_bytes().map_err(|error| error.to_string())?;
    let second = document.json_bytes().map_err(|error| error.to_string())?;
    assert_eq!(first, second);
    assert!(
        String::from_utf8(first)
            .map_err(|error| error.to_string())?
            .contains("device-49999")
    );
    Ok(())
}

#[tokio::test]
async fn access_explorer_preserves_authoritative_indeterminate_results() -> Result<(), String> {
    let (base_url, _) = fake_response(
        "200 OK",
        "application/json",
        br#"{"matches":[],"type":"user","previewFor":"alice@example.test"}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let question = AccessQuestion {
        source_selector: "alice@example.test".to_owned(),
        destination_selector: "100.64.0.2".to_owned(),
        protocol_or_port: None,
        ssh_user: None,
        application_capability: None,
        policy_source: PolicySource::CurrentRemote,
    };
    let policy =
        PolicyDocument::from_bytes(br#"{}"#.to_vec(), 1).map_err(|error| error.to_string())?;
    let result = client
        .ask_access(&token, "example.test", &question, &policy, 2)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(result.decision, AccessDecision::Indeterminate);
    assert!(!result.limitations.is_empty());
    Ok(())
}

#[test]
fn webhook_and_log_stream_previews_are_complete_but_secret_safe() {
    let subscriptions = SubscriptionSet::from_wire(
        vec!["device".to_owned(), "future-category".to_owned()],
        vec!["nodeCreated".to_owned(), "future-event".to_owned()],
    );
    assert!(subscriptions.is_ok());
    if let Ok(subscriptions) = subscriptions {
        let draft = WebhookDraft {
            endpoint_url: "https://hooks.example.test/hook?tenant=fixture".to_owned(),
            destination_type: DestinationType::Slack,
            subscriptions: subscriptions.clone(),
        };
        assert!(draft.validate().is_ok());
        let preview = WebhookMutation::Create(draft).preview();
        assert!(preview.contains("tenant=fixture"));
        assert!(preview.contains("future-event"));
        assert!(!preview.contains("fixture-secret"));
    }
    let replacement = LogStreamReplacement {
        log_type: LogType::Network,
        destination_type: "gcs".to_owned(),
        url: String::new(),
        user: None,
        upload_period_minutes: None,
        compression_format: None,
        token: None,
        s3_bucket: None,
        s3_region: None,
        s3_key_prefix: None,
        s3_authentication_type: None,
        s3_access_key_id: None,
        s3_role_arn: None,
        gcs_bucket: Some("fixture-bucket".to_owned()),
        gcs_key_prefix: None,
        gcs_scopes: Vec::new(),
        gcs_credentials: Some(SecretBuffer::new("fixture-secret")),
    };
    assert!(format!("{replacement:?}").contains("<redacted>"));
    assert!(!format!("{replacement:?}").contains("fixture-secret"));
}

#[test]
fn flow_aggregation_detects_counter_overflow() {
    let message = FlowMessage {
        node_id: "node".to_owned(),
        reporting_node_name: None,
        logged: "2026-08-04T00:00:00Z".to_owned(),
        start: "2026-08-04T00:00:00Z".to_owned(),
        end: "2026-08-04T00:01:00Z".to_owned(),
        source_node: None,
        destination_nodes: Vec::new(),
        virtual_traffic: vec![
            FlowConnection {
                proto: "tcp".to_owned(),
                src: "100.64.0.1".to_owned(),
                dst: "100.64.0.2".to_owned(),
                src_port: None,
                dst_port: None,
                tx_packets: u64::MAX,
                tx_bytes: 0,
                rx_packets: 0,
                rx_bytes: 0,
            },
            FlowConnection {
                proto: "tcp".to_owned(),
                src: "100.64.0.1".to_owned(),
                dst: "100.64.0.2".to_owned(),
                src_port: None,
                dst_port: None,
                tx_packets: 1,
                tx_bytes: 0,
                rx_packets: 0,
                rx_bytes: 0,
            },
        ],
        subnet_traffic: Vec::new(),
        exit_traffic: Vec::new(),
        physical_traffic: Vec::new(),
    };
    let result = aggregate_checked(
        &[message],
        &FlowFilter::default(),
        &[AggregateDimension::Protocol],
    );
    assert!(result.is_err());
}

#[test]
fn saved_view_file_rejects_unknown_schema_fields_without_migration() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("saved-views.toml");
    std::fs::write(&path, "version = 1\nviews = []\nobsolete_alias = true\n")
        .map_err(|error| error.to_string())?;
    let result = SavedViewStore::load(&path, &ViewRegistry::default());
    assert!(result.is_err());
    Ok(())
}

#[test]
fn export_and_action_registries_are_explicit() -> Result<(), String> {
    let actions = tale::action::all_actions();
    for id in [
        "overview.health.open_resource",
        "activity.flows.select_window",
        "admin.webhook.rotate_secret",
        "admin.log_stream.replace",
        "saved_view.apply",
        "collection.export",
        "access_explorer.ask",
    ] {
        assert!(actions.iter().any(|action| action.id.as_str() == id));
    }
    let document = tale::domain::export::ExportDocument {
        metadata: tale::domain::export::ExportMetadata {
            schema: tale::domain::export::ExportCollection::Devices,
            schema_version: 1,
            tale_version: "tale/test".to_owned(),
            sources: Vec::new(),
            observed_at: 1_754_275_200,
            route: "devices".to_owned(),
            active_filter: "none".to_owned(),
            active_sort: "id".to_owned(),
            truncated: false,
            complete: true,
            export_timestamp: None,
        },
        rows: Vec::new(),
    };
    let first = document.json_bytes().map_err(|error| error.to_string())?;
    let second = document.json_bytes().map_err(|error| error.to_string())?;
    assert_eq!(first, second);
    let text = String::from_utf8(document.csv_bytes().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    assert!(text.starts_with(
        "_row_kind,_schema,_observed_at,_sources,_filter,_sort,id,name,addresses,source,observed_at"
    ));
    Ok(())
}

#[test]
fn export_csv_escapes_special_values_and_writes_atomically() -> Result<(), String> {
    let document = fixture_export_document(
        ExportCollection::Devices,
        vec![ExportRow::Device {
            id: "device-quoted".to_owned(),
            name: "name,\"quoted\"\nline".to_owned(),
            addresses: vec!["198.51.100.1".to_owned()],
            source: "fixture".to_owned(),
            observed_at: 1_754_275_200,
        }],
    );
    let json_first = document.json_bytes().map_err(|error| error.to_string())?;
    let json_second = document.json_bytes().map_err(|error| error.to_string())?;
    assert_eq!(json_first, json_second);
    let csv = String::from_utf8(document.csv_bytes().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    assert!(csv.contains("\"name,\"\"quoted\"\"\nline\""));
    assert!(csv.lines().next().is_some_and(|line| {
        line.starts_with("_row_kind,_schema,_observed_at,_sources,_filter,_sort,")
    }));

    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("devices.csv");
    write_atomic(&document, &path, ExportFormat::Csv, false).map_err(|error| error.to_string())?;
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    assert_eq!(
        bytes,
        document.csv_bytes().map_err(|error| error.to_string())?
    );
    assert_eq!(
        write_atomic(&document, &path, ExportFormat::Csv, false),
        Err(ExportWriteError::OverwriteNotConfirmed)
    );
    std::fs::write(&path, b"old export").map_err(|error| error.to_string())?;
    write_atomic(&document, &path, ExportFormat::Csv, true).map_err(|error| error.to_string())?;
    assert_eq!(
        std::fs::read(&path).map_err(|error| error.to_string())?,
        document.csv_bytes().map_err(|error| error.to_string())?
    );
    let temporary_files = std::fs::read_dir(directory.path())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".devices.csv."))
        })
        .count();
    assert_eq!(temporary_files, 0);
    assert_eq!(
        write_atomic(
            &document,
            &directory.path().join("missing").join("devices.csv"),
            ExportFormat::Json,
            false,
        ),
        Err(ExportWriteError::MissingParent)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    Ok(())
}

#[test]
fn export_serialization_failure_does_not_replace_the_target() -> Result<(), String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("invalid.json");
    std::fs::write(&path, b"preserve this").map_err(|error| error.to_string())?;
    let mut document = fixture_export_document(ExportCollection::Devices, Vec::new());
    document.metadata.observed_at = u64::MAX;
    let result = write_atomic(&document, &path, ExportFormat::Json, true);
    assert!(matches!(
        result,
        Err(ExportWriteError::Serialization(
            tale::domain::export::ExportError::InvalidTimestamp
        ))
    ));
    assert_eq!(
        std::fs::read(&path).map_err(|error| error.to_string())?,
        b"preserve this"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn export_and_saved_view_state_reject_symlink_targets() -> Result<(), String> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let document = fixture_export_document(ExportCollection::Devices, Vec::new());
    let target = directory.path().join("real.json");
    let link = directory.path().join("export.json");
    std::fs::write(&target, b"target").map_err(|error| error.to_string())?;
    symlink(&target, &link).map_err(|error| error.to_string())?;
    assert!(matches!(
        write_atomic(&document, &link, ExportFormat::Json, true),
        Err(ExportWriteError::NotWritable(_))
    ));

    let saved_target = directory.path().join("saved-real.toml");
    let saved_link = directory.path().join("saved-views.toml");
    std::fs::write(&saved_target, "version = 1\nviews = []\n")
        .map_err(|error| error.to_string())?;
    symlink(&saved_target, &saved_link).map_err(|error| error.to_string())?;
    assert!(SavedViewStore::load(&saved_link, &ViewRegistry::default()).is_err());
    Ok(())
}

#[test]
fn saved_view_storage_contains_only_registered_query_and_presentation_state() -> Result<(), String>
{
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = directory.path().join("saved-views.toml");
    let mut store =
        SavedViewStore::load(&path, &ViewRegistry::default()).map_err(|error| error.to_string())?;
    let view = SavedView {
        name: "production-linux".to_owned(),
        route: "devices".to_owned(),
        wide_columns: false,
        columns: vec!["name".to_owned(), "owner".to_owned()],
        filters: vec![FilterClause {
            field: "os".to_owned(),
            operator: FilterOperator::Equals,
            value: FilterValue::Text("linux".to_owned()),
        }],
        sort: vec![SortTerm {
            field: "last_seen".to_owned(),
            direction: SortDirection::Descending,
        }],
    };
    store
        .create(view, &ViewRegistry::default())
        .map_err(|error| error.to_string())?;
    let contents = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    assert!(!contents.contains("credential"));
    assert!(!contents.contains("selected"));
    assert!(!contents.contains("snapshot"));
    assert!(contents.contains("route = \"devices\""));
    Ok(())
}

#[test]
fn health_can_render_fifty_thousand_deterministic_findings() {
    let devices = (0..50_000)
        .map(|index| HealthDevice {
            stable_id: format!("pending-{index:05}"),
            source_id: "fixture-health".to_owned(),
            key_expires_at: None,
            approval: ApprovalState::Pending,
            client_version: None,
            posture_read_succeeded: false,
            posture_attributes_present: None,
        })
        .collect();
    let snapshot = HealthSnapshot {
        now: 1_754_275_200,
        devices,
        users: Vec::new(),
        resources: Vec::new(),
        routes: Vec::new(),
        posture_integration_enabled: false,
        relay_samples: Vec::new(),
    };
    let findings = snapshot.findings();
    assert_eq!(findings.len(), 50_000);
    assert!(findings.iter().all(|finding| finding.derived));
    assert!(findings.windows(2).all(|pair| {
        (pair[0].rule_id.as_str(), &pair[0].affected_resource_ids)
            <= (pair[1].rule_id.as_str(), &pair[1].affected_resource_ids)
    }));
}

#[test]
fn phase_eight_endpoint_scopes_are_explicit() {
    assert_eq!(
        Endpoint::NetworkFlowLogs.required_scope(),
        "logs:network:read"
    );
    assert_eq!(Endpoint::Webhooks.required_scope(), "webhooks:read");
    assert_eq!(Endpoint::Webhook.required_scope(), "webhooks:read");
    assert_eq!(Endpoint::WebhookCreate.required_scope(), "webhooks");
    assert_eq!(Endpoint::WebhookEdit.required_scope(), "webhooks");
    assert_eq!(Endpoint::WebhookTest.required_scope(), "webhooks");
    assert_eq!(Endpoint::WebhookRotate.required_scope(), "webhooks");
    assert_eq!(Endpoint::WebhookDelete.required_scope(), "webhooks");
    assert_eq!(
        Endpoint::LogStreamConfiguration.required_scope(),
        "log_streaming:read"
    );
    assert_eq!(
        Endpoint::LogStreamStatus.required_scope(),
        "log_streaming:read"
    );
    assert_eq!(
        Endpoint::LogStreamConfigurationReplace.required_scope(),
        "log_streaming"
    );
    assert_eq!(
        Endpoint::LogStreamConfigurationDelete.required_scope(),
        "log_streaming"
    );
    assert_eq!(
        Endpoint::NetworkLogSettings.required_scope(),
        "logs:network:read"
    );
    assert_eq!(
        Endpoint::NetworkLogSettingsUpdate.required_scope(),
        "logs:network"
    );
}
