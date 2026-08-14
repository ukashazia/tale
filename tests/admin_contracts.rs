use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tale::admin::auth::{
    AccessTokenRecord, CredentialRecord, CredentialStore, MemoryCredentialStore, SecretValue,
    TokenManager,
};
use tale::admin::client::{AdminClient, AdminError};
use tale::admin::key_mutations::AuthKeyCreateRequest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn fake_response(
    status: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<(url::Url, Arc<tokio::sync::Mutex<String>>), String> {
    fake_response_with_headers(status, content_type, body, Vec::new()).await
}

async fn fake_response_with_headers(
    status: &str,
    content_type: &str,
    body: Vec<u8>,
    extra_headers: Vec<(String, String)>,
) -> Result<(url::Url, Arc<tokio::sync::Mutex<String>>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let captured = Arc::new(tokio::sync::Mutex::new(String::new()));
    let captured_for_task = captured.clone();
    let status = status.to_owned();
    let content_type = content_type.to_owned();
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut request = vec![0_u8; 32 * 1024];
            let count = match stream.read(&mut request).await {
                Ok(count) => count,
                Err(_) => return,
            };
            let text = String::from_utf8_lossy(&request[..count]).to_string();
            if let Ok(mut captured) = captured_for_task.try_lock() {
                *captured = text;
            }
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                body.len(),
                extra_headers
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}\r\n"))
                    .collect::<String>()
            );
            let _ = stream.write_all(header.as_bytes()).await;
            let _ = stream.write_all(&body).await;
        }
    });
    let url =
        url::Url::parse(&format!("http://{address}/api/v2")).map_err(|error| error.to_string())?;
    Ok((url, captured))
}

async fn client_with_token(
    base_url: url::Url,
) -> Result<(AdminClient, tale::admin::auth::AccessToken), String> {
    client_with_token_timeout(base_url, Duration::from_secs(2)).await
}

async fn client_with_token_timeout(
    base_url: url::Url,
    timeout: Duration,
) -> Result<(AdminClient, tale::admin::auth::AccessToken), String> {
    let store = Arc::new(MemoryCredentialStore::default());
    let record = CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new("canary-token-for-tests"),
    });
    store
        .set("fixture", &record)
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::new(store);
    let token = manager
        .access_token("fixture", "fixture")
        .await
        .map_err(|error| error.to_string())?;
    let client =
        AdminClient::with_base_url(base_url, timeout).map_err(|error| error.to_string())?;
    Ok((client, token))
}

async fn repeated_response(
    status: &str,
    content_type: &str,
    body: Vec<u8>,
    extra_headers: Vec<(String, String)>,
    responses: usize,
) -> Result<(url::Url, Arc<tokio::sync::Mutex<usize>>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let count = Arc::new(tokio::sync::Mutex::new(0usize));
    let count_for_task = count.clone();
    let status = status.to_owned();
    let content_type = content_type.to_owned();
    tokio::spawn(async move {
        for _ in 0..responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 32 * 1024];
            if stream.read(&mut request).await.is_err() {
                return;
            }
            if let Ok(mut count) = count_for_task.try_lock() {
                *count = count.saturating_add(1);
            }
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                body.len(),
                extra_headers
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}\r\n"))
                    .collect::<String>()
            );
            let _ = stream.write_all(header.as_bytes()).await;
            let _ = stream.write_all(&body).await;
        }
    });
    let url =
        url::Url::parse(&format!("http://{address}/api/v2")).map_err(|error| error.to_string())?;
    Ok((url, count))
}

async fn delayed_response() -> Result<url::Url, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}")
                .await;
        }
    });
    url::Url::parse(&format!("http://{address}/api/v2")).map_err(|error| error.to_string())
}

#[tokio::test]
async fn devices_contract_uses_encoded_path_and_bearer_header() -> Result<(), String> {
    let body = include_bytes!("fixtures/admin/devices.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", body).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_devices(&token, "example test/fictional").await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(
        request.starts_with("GET /api/v2/tailnet/example%20test%2Ffictional/devices HTTP/1.1"),
        "{request}"
    );
    assert!(
        request.contains("authorization: Bearer canary-token-for-tests")
            || request.contains("Authorization: Bearer canary-token-for-tests")
    );
    Ok(())
}

#[tokio::test]
async fn policy_contract_preserves_bytes_and_requests_hujson() -> Result<(), String> {
    let source = b"{\n  // preserved\n  \"fictional\": true\n}\n".to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/hujson", source.clone()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.get_policy(&token, "example.test").await;
    assert!(result.is_ok());
    if let Ok(response) = result {
        assert_eq!(response.value.source_bytes, source);
        assert_eq!(response.value.content_type, "application/hujson");
    }
    let request = capture.lock().await.clone();
    assert!(
        request.contains("accept: application/hujson")
            || request.contains("Accept: application/hujson")
    );
    Ok(())
}

fn captured_body(request: &str) -> &[u8] {
    request
        .split_once("\r\n\r\n")
        .map_or("", |parts| parts.1)
        .as_bytes()
}

#[tokio::test]
async fn policy_validation_preview_and_save_use_exact_contracts() -> Result<(), String> {
    let candidate = include_bytes!("fixtures/admin/policy/valid.hujson").to_vec();
    let (base_url, capture) =
        fake_response("200 OK", "application/json", br#"{}"#.to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .validate_policy(&token, "example test/fictional", &candidate)
        .await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(
        request
            .starts_with("POST /api/v2/tailnet/example%20test%2Ffictional/acl/validate HTTP/1.1")
    );
    assert!(request.contains("accept: application/json"));
    assert!(request.contains("content-type: application/hujson"));
    assert_eq!(captured_body(&request), candidate.as_slice());

    let preview = br#"{"matches":[],"type":"user","previewFor":"user-fictional-001"}"#;
    let (base_url, capture) = fake_response("200 OK", "application/json", preview.to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .preview_policy(
            &token,
            "example.test",
            tale::domain::policy_workflow::PolicySelectorType::User,
            "user-fictional-001",
            &candidate,
        )
        .await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(request.starts_with(
        "POST /api/v2/tailnet/example.test/acl/preview?type=user&previewFor=user-fictional-001 HTTP/1.1"
    ));
    assert!(request.contains("accept: application/json"));
    assert!(request.contains("content-type: application/hujson"));
    assert_eq!(captured_body(&request), candidate.as_slice());

    let (base_url, capture) =
        fake_response("200 OK", "application/hujson", candidate.clone()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .save_policy(&token, "example.test", &candidate, "\"policy-version-7\"")
        .await;
    assert!(result.is_ok());
    if let Ok(result) = result {
        assert_eq!(result.value.source_bytes, candidate);
    }
    let request = capture.lock().await.clone();
    assert!(request.starts_with("POST /api/v2/tailnet/example.test/acl HTTP/1.1"));
    assert!(request.contains("accept: application/hujson"));
    assert!(request.contains("content-type: application/hujson"));
    assert!(request.contains("if-match: \"policy-version-7\""));
    assert_eq!(captured_body(&request), candidate.as_slice());
    Ok(())
}

#[tokio::test]
async fn credential_list_detail_create_and_revoke_use_exact_contracts() -> Result<(), String> {
    let (base_url, capture) =
        fake_response("200 OK", "application/json", br#"{"keys":[]}"#.to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_keys(&token, "example test/fictional").await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(
        request
            .starts_with("GET /api/v2/tailnet/example%20test%2Ffictional/keys?all=false HTTP/1.1")
    );
    assert!(request.contains("accept: application/json"));

    let detail = include_bytes!("fixtures/admin/credentials/auth-key-no-secret.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", detail).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .get_key(&token, "example.test", "key fictional/001")
        .await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(
        request.starts_with("GET /api/v2/tailnet/example.test/keys/key%20fictional%2F001 HTTP/1.1")
    );
    assert!(request.contains("accept: application/json"));

    let create_body = include_bytes!("fixtures/admin/credentials/auth-key-success.json").to_vec();
    let (base_url, capture) = fake_response("200 OK", "application/json", create_body).await?;
    let (client, token) = client_with_token(base_url).await?;
    let create = AuthKeyCreateRequest {
        description: Some("fictional operator".to_owned()),
        expiry_seconds: 7 * 24 * 60 * 60,
        reusable: false,
        ephemeral: true,
        preauthorized: false,
        tags: vec!["tag:fictional".to_owned()],
    };
    let result = client
        .create_auth_key(&token, "example.test", &create)
        .await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(request.starts_with("POST /api/v2/tailnet/example.test/keys HTTP/1.1"));
    assert!(request.contains("accept: application/json"));
    assert!(request.contains("content-type: application/json"));
    let body = serde_json::from_slice::<Value>(captured_body(&request))
        .map_err(|error| error.to_string())?;
    assert_eq!(body["keyType"], "auth");
    assert_eq!(body["description"], "fictional operator");
    assert_eq!(body["expirySeconds"], 7 * 24 * 60 * 60);
    assert_eq!(body["capabilities"]["devices"]["create"]["reusable"], false);
    assert_eq!(body["capabilities"]["devices"]["create"]["ephemeral"], true);
    assert_eq!(
        body["capabilities"]["devices"]["create"]["preauthorized"],
        false
    );
    assert_eq!(
        body["capabilities"]["devices"]["create"]["tags"][0],
        "tag:fictional"
    );

    let (base_url, capture) = fake_response("200 OK", "application/json", Vec::new()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .revoke_credential(&token, "example.test", "key fictional/001")
        .await;
    assert!(result.is_ok());
    let request = capture.lock().await.clone();
    assert!(
        request
            .starts_with("DELETE /api/v2/tailnet/example.test/keys/key%20fictional%2F001 HTTP/1.1")
    );
    assert!(request.contains("accept: application/json"));
    assert_eq!(captured_body(&request), b"");
    Ok(())
}

#[tokio::test]
async fn forbidden_is_endpoint_scoped_and_errors_do_not_echo_token() -> Result<(), String> {
    let (base_url, _) = fake_response(
        "403 Forbidden",
        "application/json",
        br#"{"error":"forbidden"}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::Forbidden { .. })));
    if let Err(error) = result {
        assert!(!error.to_string().contains("canary-token-for-tests"));
    }
    Ok(())
}

#[tokio::test]
async fn credential_metadata_decoder_rejects_returned_secret_field() -> Result<(), String> {
    let payload = br#"{"keys":[{"id":"fictional-key","key":"unexpected-secret-field"}]}"#;
    let (base_url, _) = fake_response("200 OK", "application/json", payload.to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_keys(&token, "example.test").await;
    assert!(result.is_ok());
    if let Ok(response) = result {
        let decoded = tale::admin::credentials::decode_credentials(response.value.keys, 1);
        assert!(decoded.is_err());
        if let Err(error) = decoded {
            assert!(!error.to_string().contains("unexpected-secret-field"));
        }
    }
    Ok(())
}

#[test]
fn secret_wrapper_and_credential_debug_are_redacted() {
    let record = CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new("canary-token-for-tests"),
    });
    let debug = format!("{record:?}");
    assert!(!debug.contains("canary-token-for-tests"));
    assert!(debug.contains("redacted"));
}

#[tokio::test]
async fn oauth_exchange_is_form_encoded_and_refreshes_are_coalesced() -> Result<(), String> {
    let response =
        br#"{"access_token":"oauth-canary-token","token_type":"Bearer","expires_in":3600}"#;
    let (base_url, capture) =
        fake_response("200 OK", "application/json", response.to_vec()).await?;
    let token_url = format!("{base_url}/oauth/token");
    let store = Arc::new(MemoryCredentialStore::default());
    let record = CredentialRecord::OAuthClient(tale::admin::auth::OAuthClientRecord {
        version: 1,
        client_id: SecretValue::new("fictional-client-id"),
        client_secret: SecretValue::new("fictional-client-secret"),
        requested_scopes: vec!["devices:core:read".to_owned(), "users:read".to_owned()],
    });
    store
        .set("oauth", &record)
        .map_err(|error| error.to_string())?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::with_client(store, http, token_url);
    let (left, right) = tokio::join!(
        manager.access_token("fictional", "oauth"),
        manager.access_token("fictional", "oauth")
    );
    assert!(left.is_ok());
    assert!(right.is_ok());
    let request = capture.lock().await.clone();
    assert!(request.contains("client_id=fictional-client-id"));
    assert!(
        request.contains("content-type: application/x-www-form-urlencoded")
            || request.contains("Content-Type: application/x-www-form-urlencoded")
    );
    assert!(request.contains("grant_type=client_credentials"));
    assert!(request.contains("scope=devices%3Acore%3Aread+users%3Aread"));
    assert!(manager.access_token("fictional", "oauth").await.is_ok());
    Ok(())
}

#[tokio::test]
async fn status_errors_are_typed_without_body_guessing() -> Result<(), String> {
    type ErrorCase = (&'static str, Vec<u8>, fn(&AdminError) -> bool);
    let cases: [ErrorCase; 4] = [
        (
            "401 Unauthorized",
            br#"{"error":"expired"}"#.to_vec(),
            |error: &AdminError| matches!(error, AdminError::Unauthenticated),
        ),
        (
            "403 Forbidden",
            br#"{"code":"plan_restricted","message":"fictional plan"}"#.to_vec(),
            |error: &AdminError| matches!(error, AdminError::PlanRestricted { .. }),
        ),
        (
            "404 Not Found",
            br#"{"error":"missing"}"#.to_vec(),
            |error: &AdminError| matches!(error, AdminError::NotFound { .. }),
        ),
        (
            "412 Precondition Failed",
            br#"{"error":"policy changed"}"#.to_vec(),
            |error: &AdminError| matches!(error, AdminError::Conflict { .. }),
        ),
    ];
    for (status, body, matches_error) in cases {
        let (base_url, _) = fake_response(status, "application/json", body).await?;
        let (client, token) = client_with_token(base_url).await?;
        let result = client.list_users(&token, "example.test").await;
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(matches_error(&error));
        }
    }
    Ok(())
}

#[tokio::test]
async fn rate_limits_retry_twice_and_capture_metadata() -> Result<(), String> {
    let body = br#"{"error":"busy"}"#.to_vec();
    let (base_url, count) = repeated_response(
        "429 Too Many Requests",
        "application/json",
        body,
        vec![("Retry-After".to_owned(), "0".to_owned())],
        3,
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::RateLimited { .. })));
    assert_eq!(*count.lock().await, 3);

    let (base_url, count) = repeated_response(
        "429 Too Many Requests",
        "application/json",
        br#"{"error":"busy"}"#.to_vec(),
        Vec::new(),
        3,
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::RateLimited { .. })));
    assert_eq!(*count.lock().await, 3);
    Ok(())
}

#[tokio::test]
async fn transient_server_failures_retry_twice() -> Result<(), String> {
    let (base_url, count) = repeated_response(
        "503 Service Unavailable",
        "application/json",
        br#"{"error":"temporary"}"#.to_vec(),
        vec![("Retry-After".to_owned(), "0".to_owned())],
        3,
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::ServerFailure { .. })));
    assert_eq!(*count.lock().await, 3);
    Ok(())
}

#[tokio::test]
async fn malformed_content_and_body_limits_are_rejected() -> Result<(), String> {
    let (base_url, _) = fake_response("200 OK", "application/json", b"not json".to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::DecodeFailed { .. })));

    let (base_url, _) = fake_response("200 OK", "text/plain", br#"{"users":[]}"#.to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::DecodeFailed { .. })));

    let oversized = vec![b'x'; 4 * 1024 * 1024 + 1];
    let (base_url, _) = fake_response("200 OK", "application/json", oversized).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::BodyTooLarge { .. })));
    Ok(())
}

#[tokio::test]
async fn response_metadata_and_success_status_are_explicit() -> Result<(), String> {
    let (base_url, _) = fake_response_with_headers(
        "200 OK",
        "application/json",
        br#"{"users":[]}"#.to_vec(),
        vec![
            (
                "x-tailscale-request-id".to_owned(),
                "request-fictional".to_owned(),
            ),
            ("x-ratelimit-limit".to_owned(), "100".to_owned()),
            ("x-ratelimit-remaining".to_owned(), "99".to_owned()),
            ("x-ratelimit-reset".to_owned(), "1785754800".to_owned()),
        ],
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(result.is_ok());
    if let Ok(response) = result {
        assert_eq!(
            response.meta.request_id.as_deref(),
            Some("request-fictional")
        );
        assert_eq!(response.meta.status, 200);
        assert_eq!(response.meta.page_count, 1);
        assert_eq!(
            response
                .meta
                .rate_limit
                .as_ref()
                .and_then(|value| value.limit),
            Some(100)
        );
    }

    let (base_url, _) = fake_response(
        "201 Created",
        "application/json",
        br#"{"users":[]}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(
        result,
        Err(AdminError::UnexpectedStatus { status: 201, .. })
    ));
    Ok(())
}

#[tokio::test]
async fn request_timeout_is_distinct_and_debug_is_secret_safe() -> Result<(), String> {
    let base_url = delayed_response().await?;
    let (client, token) = client_with_token_timeout(base_url, Duration::from_millis(20)).await?;
    let result = client.list_users(&token, "example.test").await;
    assert!(matches!(result, Err(AdminError::TimedOut { .. })));

    let debug_client = AdminClient::with_base_url(
        url::Url::parse("http://127.0.0.1:1/api/v2?client_secret=fictional-secret")
            .map_err(|error| error.to_string())?,
        Duration::from_secs(1),
    )
    .map_err(|error| error.to_string())?;
    let debug = format!("{debug_client:?}");
    assert!(!debug.contains("fictional-secret"));
    Ok(())
}

/// Wraps a store and counts reads so a test can assert how often the OS keyring would
/// have been touched. On macOS every read can raise a modal unlock prompt that blocks
/// the calling thread, so the count is a correctness property, not a performance one.
#[derive(Default)]
struct CountingCredentialStore {
    inner: MemoryCredentialStore,
    reads: std::sync::atomic::AtomicUsize,
}

impl CountingCredentialStore {
    fn reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl CredentialStore for CountingCredentialStore {
    fn get(
        &self,
        reference: &str,
    ) -> Result<Option<CredentialRecord>, tale::secrets::SecretsError> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.get(reference)
    }

    fn set(
        &self,
        reference: &str,
        record: &CredentialRecord,
    ) -> Result<(), tale::secrets::SecretsError> {
        self.inner.set(reference, record)
    }

    fn delete(&self, reference: &str) -> Result<bool, tale::secrets::SecretsError> {
        self.inner.delete(reference)
    }
}

#[tokio::test]
async fn credential_status_reuses_the_record_the_token_read_already_decoded() -> Result<(), String>
{
    let store = Arc::new(CountingCredentialStore::default());
    let record = CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new("canary-token-for-tests"),
    });
    store
        .set("fixture", &record)
        .map_err(|error| error.to_string())?;

    let manager = TokenManager::new(store.clone());
    manager
        .access_token("fixture", "fixture")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        store.reads(),
        1,
        "the token read is the only unavoidable one"
    );

    for _ in 0..5 {
        let status = manager
            .credential_status("fixture")
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "status was missing".to_owned())?;
        assert_eq!(status.kind.label(), "access_token");
    }
    assert_eq!(
        store.reads(),
        1,
        "every admin refresh calls credential_status; none may reach the keyring again"
    );
    Ok(())
}

#[tokio::test]
async fn credential_status_still_reads_when_nothing_is_cached() -> Result<(), String> {
    let store = Arc::new(CountingCredentialStore::default());
    let record = CredentialRecord::OAuthClient(tale::admin::auth::OAuthClientRecord {
        version: 1,
        client_id: SecretValue::new("fictional-client"),
        client_secret: SecretValue::new("fictional-secret"),
        requested_scopes: vec!["devices:core:read".to_owned()],
    });
    store
        .set("fixture", &record)
        .map_err(|error| error.to_string())?;

    let manager = TokenManager::new(store.clone());
    let status = manager
        .credential_status("fixture")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "status was missing".to_owned())?;
    assert_eq!(
        status.requested_scopes,
        vec!["devices:core:read".to_owned()]
    );
    assert_eq!(store.reads(), 1);

    manager.clear_all().await;
    manager
        .credential_status("fixture")
        .map_err(|error| error.to_string())?;
    assert_eq!(store.reads(), 2, "clearing the cache must force a re-read");
    Ok(())
}
