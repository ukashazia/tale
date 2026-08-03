use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tale::admin::auth::{
    AccessTokenRecord, CredentialRecord, CredentialStore, MemoryCredentialStore, SecretValue,
    TokenManager, encode_record,
};
use tale::admin::client::{AdminClient, AdminError, Endpoint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEVICE_ID: &str = "node-fictional-001";
const USER_ID: &str = "user-fictional-001";
const TAILNET: &str = "example test/fictional";

async fn client_with_token(
    base_url: url::Url,
) -> Result<(AdminClient, tale::admin::auth::AccessToken), String> {
    let store = Arc::new(MemoryCredentialStore::default());
    let record = CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new("canary-token-for-tests"),
    });
    let encoded = encode_record(&record).map_err(|error| error.to_string())?;
    store
        .set("fixture", &encoded)
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::new(store, None);
    let token = manager
        .access_token("fixture", "fixture")
        .await
        .map_err(|error| error.to_string())?;
    let client = AdminClient::with_base_url(base_url, Duration::from_secs(2))
        .map_err(|error| error.to_string())?;
    Ok((client, token))
}

fn header_end(request: &[u8]) -> Option<usize> {
    request.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(request: &[u8]) -> usize {
    let Some(end) = header_end(request) else {
        return 0;
    };
    String::from_utf8_lossy(&request[..end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .map_or(0, |value| value)
}

async fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    loop {
        let mut chunk = [0_u8; 4096];
        let count = match stream.read(&mut chunk).await {
            Ok(count) => count,
            Err(_) => return request,
        };
        if count == 0 {
            return request;
        }
        request.extend_from_slice(&chunk[..count]);
        let Some(end) = header_end(&request) else {
            continue;
        };
        let required = end
            .saturating_add(4)
            .saturating_add(content_length(&request));
        if request.len() >= required {
            return request;
        }
    }
}

async fn response_server(
    status: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Result<(url::Url, Arc<tokio::sync::Mutex<Option<Vec<u8>>>>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let captured_for_task = captured.clone();
    let status = status.to_owned();
    let content_type = content_type.to_owned();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let request = read_request(&mut stream).await;
        *captured_for_task.lock().await = Some(request);
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        if stream.write_all(header.as_bytes()).await.is_err() {
            return;
        }
        let _ = stream.write_all(&body).await;
    });
    let url =
        url::Url::parse(&format!("http://{address}/api/v2")).map_err(|error| error.to_string())?;
    Ok((url, captured))
}

async fn delayed_response_server(
    delay: Duration,
) -> Result<(url::Url, Arc<tokio::sync::Mutex<Option<Vec<u8>>>>), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let captured_for_task = captured.clone();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let request = read_request(&mut stream).await;
        *captured_for_task.lock().await = Some(request);
        tokio::time::sleep(delay).await;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    });
    let url =
        url::Url::parse(&format!("http://{address}/api/v2")).map_err(|error| error.to_string())?;
    Ok((url, captured))
}

async fn captured(value: &Arc<tokio::sync::Mutex<Option<Vec<u8>>>>) -> Result<Vec<u8>, String> {
    for _ in 0..100 {
        if let Some(request) = value.lock().await.clone() {
            return Ok(request);
        }
        tokio::task::yield_now().await;
    }
    Err("fake server did not capture a request".to_owned())
}

fn request_line(request: &[u8]) -> String {
    let text = String::from_utf8_lossy(request);
    text.split_once("\r\n")
        .map_or_else(|| text.to_string(), |parts| parts.0.to_owned())
}

fn request_body(request: &[u8]) -> &[u8] {
    let Some(end) = header_end(request) else {
        return &[];
    };
    request
        .get(end.saturating_add(4)..)
        .map_or(&[], |body| body)
}

fn assert_bearer(request: &[u8]) {
    let text = String::from_utf8_lossy(request);
    assert!(
        text.lines().any(|line| {
            line.eq_ignore_ascii_case("authorization: Bearer canary-token-for-tests")
        }),
        "request did not contain the expected bearer header: {text}"
    );
}

fn assert_json_body(request: &[u8], expected: &str) -> Result<(), String> {
    let actual = serde_json::from_slice::<Value>(request_body(request))
        .map_err(|error| error.to_string())?;
    let expected = serde_json::from_str::<Value>(expected).map_err(|error| error.to_string())?;
    assert_eq!(actual, expected);
    let text = String::from_utf8_lossy(request);
    assert!(
        text.lines()
            .any(|line| { line.eq_ignore_ascii_case("content-type: application/json") }),
        "JSON mutation did not declare its content type: {text}"
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum EmptyMutation {
    DeleteDevice,
    ApproveDevice,
    ExpireDevice,
    ConfigureKeyExpiry,
    RenameDevice,
    ReplaceTags,
    ApproveUser,
    ChangeUserRole,
    SuspendUser,
    RestoreUser,
    DeleteUser,
}

async fn call_empty(
    client: &AdminClient,
    token: &tale::admin::auth::AccessToken,
    operation: EmptyMutation,
) -> Result<(), AdminError> {
    match operation {
        EmptyMutation::DeleteDevice => client.delete_device(token, DEVICE_ID).await.map(|_| ()),
        EmptyMutation::ApproveDevice => client
            .set_device_authorized(token, DEVICE_ID, true)
            .await
            .map(|_| ()),
        EmptyMutation::ExpireDevice => client.expire_device_key(token, DEVICE_ID).await.map(|_| ()),
        EmptyMutation::ConfigureKeyExpiry => client
            .set_device_key_expiry(token, DEVICE_ID, false)
            .await
            .map(|_| ()),
        EmptyMutation::RenameDevice => client
            .set_device_name(token, DEVICE_ID, "workstation.example.test")
            .await
            .map(|_| ()),
        EmptyMutation::ReplaceTags => client
            .set_device_tags(
                token,
                DEVICE_ID,
                &["tag:fictional".to_owned(), "tag:operator".to_owned()],
            )
            .await
            .map(|_| ()),
        EmptyMutation::ApproveUser => client.approve_user(token, USER_ID).await.map(|_| ()),
        EmptyMutation::ChangeUserRole => client
            .set_user_role(token, USER_ID, "network-admin")
            .await
            .map(|_| ()),
        EmptyMutation::SuspendUser => client.suspend_user(token, USER_ID).await.map(|_| ()),
        EmptyMutation::RestoreUser => client.restore_user(token, USER_ID).await.map(|_| ()),
        EmptyMutation::DeleteUser => client.delete_user(token, USER_ID).await.map(|_| ()),
    }
}

#[tokio::test]
async fn device_and_user_mutations_use_exact_empty_contracts() -> Result<(), String> {
    let cases = [
        (
            EmptyMutation::DeleteDevice,
            "DELETE /api/v2/device/node-fictional-001 HTTP/1.1",
            None,
        ),
        (
            EmptyMutation::ApproveDevice,
            "POST /api/v2/device/node-fictional-001/authorized HTTP/1.1",
            Some(r#"{"authorized":true}"#),
        ),
        (
            EmptyMutation::ExpireDevice,
            "POST /api/v2/device/node-fictional-001/expire HTTP/1.1",
            None,
        ),
        (
            EmptyMutation::ConfigureKeyExpiry,
            "POST /api/v2/device/node-fictional-001/key HTTP/1.1",
            Some(r#"{"keyExpiryDisabled":false}"#),
        ),
        (
            EmptyMutation::RenameDevice,
            "POST /api/v2/device/node-fictional-001/name HTTP/1.1",
            Some(r#"{"name":"workstation.example.test"}"#),
        ),
        (
            EmptyMutation::ReplaceTags,
            "POST /api/v2/device/node-fictional-001/tags HTTP/1.1",
            Some(r#"{"tags":["tag:fictional","tag:operator"]}"#),
        ),
        (
            EmptyMutation::ApproveUser,
            "POST /api/v2/users/user-fictional-001/approve HTTP/1.1",
            None,
        ),
        (
            EmptyMutation::ChangeUserRole,
            "POST /api/v2/users/user-fictional-001/role HTTP/1.1",
            Some(r#"{"role":"network-admin"}"#),
        ),
        (
            EmptyMutation::SuspendUser,
            "POST /api/v2/users/user-fictional-001/suspend HTTP/1.1",
            None,
        ),
        (
            EmptyMutation::RestoreUser,
            "POST /api/v2/users/user-fictional-001/restore HTTP/1.1",
            None,
        ),
        (
            EmptyMutation::DeleteUser,
            "POST /api/v2/users/user-fictional-001/delete HTTP/1.1",
            None,
        ),
    ];
    for (operation, expected_line, expected_body) in cases {
        let (base_url, capture) = response_server("200 OK", "application/json", Vec::new()).await?;
        let (client, token) = client_with_token(base_url).await?;
        assert!(call_empty(&client, &token, operation).await.is_ok());
        let request = captured(&capture).await?;
        assert_eq!(request_line(&request), expected_line);
        assert_bearer(&request);
        match expected_body {
            Some(body) => assert_json_body(&request, body)?,
            None => assert!(request_body(&request).is_empty()),
        }
    }
    Ok(())
}

#[tokio::test]
async fn route_and_dns_mutations_decode_responses_and_preserve_paths() -> Result<(), String> {
    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/routes-verified.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let routes = vec!["192.0.2.0/24".to_owned(), "203.0.113.0/24".to_owned()];
    let response = client.set_device_routes(&token, DEVICE_ID, &routes).await;
    assert!(response.is_ok());
    if let Ok(response) = response {
        assert_eq!(response.value.enabled_routes, Some(routes.clone()));
    }
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "POST /api/v2/device/node-fictional-001/routes HTTP/1.1"
    );
    assert_json_body(&request, r#"{"routes":["192.0.2.0/24","203.0.113.0/24"]}"#)?;

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/nameservers-verified.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let nameservers = vec!["192.0.2.53".to_owned(), "2001:db8:100::53".to_owned()];
    assert!(
        client
            .set_nameservers(&token, TAILNET, &nameservers)
            .await
            .is_ok()
    );
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "POST /api/v2/tailnet/example%20test%2Ffictional/dns/nameservers HTTP/1.1"
    );
    assert_json_body(&request, r#"{"dns":["192.0.2.53","2001:db8:100::53"]}"#)?;

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        br#"{"magicDNS":false}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    assert!(
        client
            .set_dns_preferences(&token, TAILNET, false)
            .await
            .is_ok()
    );
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "POST /api/v2/tailnet/example%20test%2Ffictional/dns/preferences HTTP/1.1"
    );
    assert_json_body(&request, r#"{"magicDNS":false}"#)?;

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        br#"{"searchPaths":["svc.example.test"]}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let search_paths = vec!["svc.example.test".to_owned()];
    assert!(
        client
            .set_search_paths(&token, TAILNET, &search_paths)
            .await
            .is_ok()
    );
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "POST /api/v2/tailnet/example%20test%2Ffictional/dns/searchpaths HTTP/1.1"
    );
    assert_json_body(&request, r#"{"searchPaths":["svc.example.test"]}"#)?;

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/split-dns-verified.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let split_body = serde_json::json!({"corp.example.test": ["192.0.2.53"]});
    assert!(
        client
            .patch_split_dns(&token, TAILNET, split_body)
            .await
            .is_ok()
    );
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "PATCH /api/v2/tailnet/example%20test%2Ffictional/dns/split-dns HTTP/1.1"
    );
    assert_json_body(&request, r#"{"corp.example.test":["192.0.2.53"]}"#)?;
    Ok(())
}

#[tokio::test]
async fn mutation_success_requires_documented_response_shape() -> Result<(), String> {
    let (base_url, _) = response_server(
        "200 OK",
        "application/json",
        br#"{"unexpected":true}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.delete_device(&token, DEVICE_ID).await;
    assert!(matches!(result, Err(AdminError::DecodeFailed { .. })));

    let (base_url, _) = response_server("200 OK", "text/plain", Vec::new()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.set_device_routes(&token, DEVICE_ID, &[]).await;
    assert!(matches!(result, Err(AdminError::DecodeFailed { .. })));

    let (base_url, _) =
        response_server("200 OK", "application/json", br#"{"dns":[]}"#.to_vec()).await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .set_device_name(&token, DEVICE_ID, "workstation")
        .await;
    assert!(matches!(result, Err(AdminError::DecodeFailed { .. })));
    Ok(())
}

#[tokio::test]
async fn mutations_never_retry_errors_or_timeouts() -> Result<(), String> {
    let (base_url, capture) = response_server(
        "429 Too Many Requests",
        "application/json",
        include_bytes!("fixtures/admin/mutations/error-rate-limit.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client.delete_device(&token, DEVICE_ID).await;
    assert!(matches!(result, Err(AdminError::RateLimited { .. })));
    assert!(captured(&capture).await.is_ok());

    let (base_url, capture) = response_server(
        "500 Internal Server Error",
        "application/json",
        br#"{"message":"fictional server failure"}"#.to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .set_device_name(&token, DEVICE_ID, "workstation")
        .await;
    assert!(matches!(result, Err(AdminError::ServerFailure { .. })));
    assert!(captured(&capture).await.is_ok());

    let (base_url, capture) = delayed_response_server(Duration::from_millis(100)).await?;
    let store = Arc::new(MemoryCredentialStore::default());
    let record = CredentialRecord::AccessToken(AccessTokenRecord {
        version: 1,
        access_token: SecretValue::new("canary-token-for-tests"),
    });
    let encoded = encode_record(&record).map_err(|error| error.to_string())?;
    store
        .set("fixture", &encoded)
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::new(store, None);
    let token = manager
        .access_token("fixture", "fixture")
        .await
        .map_err(|error| error.to_string())?;
    let client = AdminClient::with_base_url(base_url, Duration::from_millis(20))
        .map_err(|error| error.to_string())?;
    let result = client
        .set_device_name(&token, DEVICE_ID, "workstation")
        .await;
    assert!(matches!(result, Err(AdminError::TimedOut { .. })));
    assert!(captured(&capture).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn verification_reads_have_exact_resource_paths() -> Result<(), String> {
    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/device-verified.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    assert!(
        client
            .get_device(&token, "node-fictional/001")
            .await
            .is_ok()
    );
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "GET /api/v2/device/node-fictional%2F001?fields=all HTTP/1.1"
    );

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/routes-verified.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    assert!(client.get_routes(&token, DEVICE_ID).await.is_ok());
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "GET /api/v2/device/node-fictional-001/routes HTTP/1.1"
    );

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/users.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    assert!(client.list_users(&token, "example.test").await.is_ok());
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "GET /api/v2/tailnet/example.test/users HTTP/1.1"
    );

    let (base_url, capture) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/audit-zero.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    assert!(
        client
            .get_audit(
                &token,
                "example.test",
                "2026-08-03T10:00:00Z",
                "2026-08-03T10:01:00Z"
            )
            .await
            .is_ok()
    );
    let request = captured(&capture).await?;
    assert_eq!(
        request_line(&request),
        "GET /api/v2/tailnet/example.test/logging/configuration?start=2026-08-03T10%3A00%3A00Z&end=2026-08-03T10%3A01%3A00Z HTTP/1.1"
    );

    let (base_url, _) = response_server(
        "200 OK",
        "application/json",
        include_bytes!("fixtures/admin/mutations/audit-ambiguous.json").to_vec(),
    )
    .await?;
    let (client, token) = client_with_token(base_url).await?;
    let result = client
        .get_audit(
            &token,
            "example.test",
            "2026-08-03T10:00:00Z",
            "2026-08-03T10:01:00Z",
        )
        .await;
    assert!(result.is_ok());
    Ok(())
}

#[test]
fn phase_six_endpoint_scopes_are_explicit() {
    assert_eq!(Endpoint::DeviceDelete.required_scope(), "devices:core");
    assert_eq!(Endpoint::DeviceAuthorized.required_scope(), "devices:core");
    assert_eq!(Endpoint::DeviceExpire.required_scope(), "devices:core");
    assert_eq!(Endpoint::DeviceKey.required_scope(), "devices:core");
    assert_eq!(Endpoint::DeviceName.required_scope(), "devices:core");
    assert_eq!(Endpoint::DeviceTags.required_scope(), "devices:core");
    assert_eq!(Endpoint::DeviceRoutesSet.required_scope(), "devices:routes");
    assert_eq!(Endpoint::NameserversSet.required_scope(), "dns");
    assert_eq!(Endpoint::DnsPreferencesSet.required_scope(), "dns");
    assert_eq!(Endpoint::SearchPathsSet.required_scope(), "dns");
    assert_eq!(Endpoint::SplitDnsPatch.required_scope(), "dns");
    assert_eq!(Endpoint::UserApprove.required_scope(), "users");
    assert_eq!(Endpoint::UserRole.required_scope(), "users");
    assert_eq!(Endpoint::UserSuspend.required_scope(), "users");
    assert_eq!(Endpoint::UserRestore.required_scope(), "users");
    assert_eq!(Endpoint::UserDelete.required_scope(), "users");
}
