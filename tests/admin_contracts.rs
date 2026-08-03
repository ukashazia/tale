use std::sync::Arc;
use std::time::Duration;

use tale::admin::auth::{
    AccessTokenRecord, CredentialRecord, CredentialStore, MemoryCredentialStore, SecretValue,
    TokenManager, encode_record,
};
use tale::admin::client::{AdminClient, AdminError};
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
    let encoded = encode_record(&record).map_err(|error| error.to_string())?;
    store
        .set("fixture", &encoded)
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::new(store, None);
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
    let encoded = encode_record(&record).map_err(|error| error.to_string())?;
    store
        .set("oauth", &encoded)
        .map_err(|error| error.to_string())?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let manager = TokenManager::with_client(store, None, http, token_url);
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
    let cases: [ErrorCase; 3] = [
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
