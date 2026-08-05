#![cfg(unix)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{future::pending, time::Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use tale::local::daemon::{
    LOCAL_API_CAPABILITY, LOCAL_API_HOST, LocalDaemonClient, NewlineJsonDecoder, NotifyWatchMask,
    WatchInvalidation,
};
use tale::local::ipn::{ObserverConfig, ObserverEvent, ReadSerializers};
use tale::local::process::Cancellation;

const STATUS: &str = include_str!("fixtures/tailscale/1.98.9/linux/status.json");
const PREFS: &[u8] = include_bytes!("fixtures/tailscale/1.98.9/linux/prefs.json");

fn socket_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("tale-localapi-{name}-{}", std::process::id()))
}

async fn read_request(stream: &mut UnixStream) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let count = stream.read(&mut chunk).await.ok()?;
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if bytes.len() > 16 * 1024 {
            return None;
        }
    }
    String::from_utf8(bytes).ok()
}

async fn response(mut stream: UnixStream, request: String) -> Result<(), String> {
    let request_lower = request.to_ascii_lowercase();
    assert!(request_lower.contains(&format!("host: {LOCAL_API_HOST}")));
    assert!(request_lower.contains(&format!("tailscale-cap: {LOCAL_API_CAPABILITY}")));
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1));
    match path {
        Some(path) if path.ends_with("/localapi/v0/status") => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTailscale-Version: 1.98.9\r\n\r\n",
                STATUS.len()
            );
            assert!(stream.write_all(header.as_bytes()).await.is_ok());
            assert!(stream.write_all(STATUS.as_bytes()).await.is_ok());
        }
        Some(path) if path.ends_with("/localapi/v0/prefs") => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTailscale-Version: 1.98.9\r\n\r\n",
                PREFS.len()
            );
            assert!(stream.write_all(header.as_bytes()).await.is_ok());
            assert!(stream.write_all(PREFS).await.is_ok());
        }
        Some(path)
            if path.ends_with(&format!(
                "/localapi/v0/watch-ipn-bus?mask={}",
                NotifyWatchMask::tale().value()
            )) =>
        {
            let header = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTailscale-Version: 1.98.9\r\n\r\n";
            assert!(stream.write_all(header.as_bytes()).await.is_ok());
            for part in [
                b"{\"State\"".as_slice(),
                b":\"Running\"}\n{\"Prefs\"".as_slice(),
                b":{}}\n".as_slice(),
            ] {
                let chunk_header = format!("{:X}\r\n", part.len());
                assert!(stream.write_all(chunk_header.as_bytes()).await.is_ok());
                assert!(stream.write_all(part).await.is_ok());
                assert!(stream.write_all(b"\r\n").await.is_ok());
            }
            assert!(stream.write_all(b"0\r\n\r\n").await.is_ok());
        }
        _ => return Err("unexpected LocalAPI request".to_owned()),
    }
    Ok(())
}

async fn observer_response(
    mut stream: UnixStream,
    request: String,
    paths: Arc<Mutex<Vec<String>>>,
) -> Result<(), String> {
    let request_lower = request.to_ascii_lowercase();
    if !request_lower.contains(&format!("host: {LOCAL_API_HOST}"))
        || !request_lower.contains(&format!("tailscale-cap: {LOCAL_API_CAPABILITY}"))
    {
        return Err("observer request omitted the LocalAPI contract headers".to_owned());
    }
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(str::to_owned)
        .ok_or_else(|| "observer request had no request target".to_owned())?;
    if let Ok(mut values) = paths.lock() {
        values.push(path.clone());
    }
    if path.ends_with("/localapi/v0/watch-ipn-bus?mask=4495") {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTailscale-Version: 1.98.9\r\n\r\n",
            )
            .await
            .map_err(|error| error.to_string())?;
        pending::<()>().await;
    } else if path.ends_with("/localapi/v0/status") {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTailscale-Version: 1.98.9\r\n\r\n",
            STATUS.len()
        );
        stream
            .write_all(header.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        stream
            .write_all(STATUS.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
    } else if path.ends_with("/localapi/v0/prefs") {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTailscale-Version: 1.98.9\r\n\r\n",
            PREFS.len()
        );
        stream
            .write_all(header.as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        stream
            .write_all(PREFS)
            .await
            .map_err(|error| error.to_string())?;
    } else {
        return Err(format!("unexpected observer request path: {path}"));
    }
    Ok(())
}

#[tokio::test]
async fn fake_localapi_checks_headers_endpoints_and_chunked_watch() {
    let path = socket_path("contract");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path);
    assert!(listener.is_ok());
    let Ok(listener) = listener else {
        return;
    };
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            let accepted = listener.accept().await;
            assert!(accepted.is_ok());
            let Ok((mut stream, _)) = accepted else {
                return;
            };
            let request = read_request(&mut stream).await;
            assert!(request.is_some());
            if let Some(request) = request {
                let result = response(stream, request).await;
                assert!(result.is_ok());
            }
        }
    });
    let client = LocalDaemonClient::new(path.clone(), Duration::from_secs(2));
    let cancellation = Cancellation::new();
    let (status, preferences, watch) = tokio::join!(
        client.status(&cancellation),
        client.preferences(&cancellation),
        client.watch(NotifyWatchMask::tale(), &cancellation),
    );
    assert!(status.is_ok());
    assert!(preferences.is_ok());
    assert!(watch.is_ok());
    if let Ok(mut watch) = watch {
        let first = watch.next(&cancellation).await;
        assert!(matches!(
            first,
            Ok(Some(value)) if value.invalidation == WatchInvalidation::Status
        ));
        let second = watch.next(&cancellation).await;
        assert!(matches!(
            second,
            Ok(Some(value)) if value.invalidation == WatchInvalidation::Preferences
        ));
        assert!(matches!(watch.next(&cancellation).await, Ok(None)));
    }
    assert!(server.await.is_ok());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn observer_accepts_watch_before_bootstrap_reads_and_cancels_idle_stream() {
    let path = socket_path("observer");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path);
    assert!(listener.is_ok());
    let Ok(listener) = listener else {
        return;
    };
    let paths = Arc::new(Mutex::new(Vec::new()));
    let server_paths = Arc::clone(&paths);
    let server = tokio::spawn(async move {
        let mut handlers = Vec::new();
        for _ in 0..3 {
            let accepted = listener.accept().await;
            if let Ok((mut stream, _)) = accepted {
                let request = read_request(&mut stream).await;
                if let Some(request) = request {
                    let paths = Arc::clone(&server_paths);
                    handlers.push(tokio::spawn(async move {
                        observer_response(stream, request, paths).await
                    }));
                }
            }
        }
        for handler in handlers {
            let _ = handler.await;
        }
    });

    let cancellation = Cancellation::new();
    let client = LocalDaemonClient::new(path.clone(), Duration::from_secs(2));
    let (sender, mut receiver) = tokio::sync::mpsc::channel(32);
    let observer = tokio::spawn(tale::local::ipn::run(
        client,
        ObserverConfig {
            reconcile_interval: Duration::from_secs(30),
            generation: 7,
            initial_status_generation: 0,
            initial_preferences_generation: 0,
        },
        cancellation.clone(),
        sender,
        ReadSerializers::new(),
    ));
    let connected = tokio::time::timeout(Duration::from_secs(2), receiver.recv()).await;
    assert!(matches!(
        connected,
        Ok(Some(ObserverEvent::WatcherConnected { generation: 7 }))
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut status_succeeded = false;
    let mut preferences_succeeded = false;
    while !(status_succeeded && preferences_succeeded) && Instant::now() < deadline {
        let event = tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await;
        if let Ok(Some(event)) = event {
            status_succeeded |= matches!(event, ObserverEvent::StatusSucceeded { .. });
            preferences_succeeded |= matches!(event, ObserverEvent::PreferencesSucceeded { .. });
        }
    }
    assert!(status_succeeded);
    assert!(preferences_succeeded);
    let first_path = paths.lock().ok().and_then(|values| values.first().cloned());
    assert!(matches!(
        first_path.as_deref(),
        Some("http://local-tailscaled.sock/localapi/v0/watch-ipn-bus?mask=4495")
    ));
    cancellation.cancel();
    let joined = tokio::time::timeout(Duration::from_secs(1), observer).await;
    assert!(joined.is_ok());
    server.abort();
    let _ = server.await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reconnect_keeps_read_generations_monotonic() {
    let path = socket_path("reconnect-generations");
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path);
    assert!(listener.is_ok());
    let Ok(listener) = listener else {
        return;
    };
    let watches = Arc::new(AtomicUsize::new(0));
    let server_watches = Arc::clone(&watches);
    let server = tokio::spawn(async move {
        loop {
            let accepted = listener.accept().await;
            let Ok((mut stream, _)) = accepted else {
                return;
            };
            let request = read_request(&mut stream).await;
            let Some(request) = request else {
                continue;
            };
            let first_line = request
                .lines()
                .next()
                .map_or_else(String::new, str::to_owned);
            let watches = Arc::clone(&server_watches);
            tokio::spawn(async move {
                if first_line.contains("watch-ipn-bus") {
                    let attempt = watches.fetch_add(1, Ordering::AcqRel);
                    let header = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTailscale-Version: 1.98.9\r\n\r\n";
                    if stream.write_all(header).await.is_err() {
                        return;
                    }
                    if attempt == 0 {
                        let _ = stream.write_all(b"0\r\n\r\n").await;
                    } else {
                        let mut closed = [0_u8; 1];
                        let _ = stream.read(&mut closed).await;
                    }
                } else if first_line.contains("/localapi/v0/status") {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTailscale-Version: 1.98.9\r\n\r\n",
                        STATUS.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(STATUS.as_bytes()).await;
                } else if first_line.contains("/localapi/v0/prefs") {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nTailscale-Version: 1.98.9\r\n\r\n",
                        PREFS.len()
                    );
                    let _ = stream.write_all(header.as_bytes()).await;
                    let _ = stream.write_all(PREFS).await;
                }
            });
        }
    });

    let cancellation = Cancellation::new();
    let client = LocalDaemonClient::new(path.clone(), Duration::from_secs(2));
    let (sender, mut receiver) = tokio::sync::mpsc::channel(64);
    let observer = tokio::spawn(tale::local::ipn::run(
        client,
        ObserverConfig {
            reconcile_interval: Duration::from_secs(30),
            generation: 9,
            initial_status_generation: 10,
            initial_preferences_generation: 20,
        },
        cancellation.clone(),
        sender,
        ReadSerializers::new(),
    ));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let mut status_generations = Vec::new();
        let mut preference_generations = Vec::new();
        while status_generations.len() < 2 || preference_generations.len() < 2 {
            let event = receiver.recv().await?;
            match event {
                ObserverEvent::StatusStarted { generation, .. } => {
                    status_generations.push(generation);
                }
                ObserverEvent::PreferencesStarted { generation, .. } => {
                    preference_generations.push(generation);
                }
                _ => {}
            }
        }
        Some((status_generations, preference_generations))
    })
    .await;
    assert!(matches!(
        result,
        Ok(Some((status, preferences)))
            if status == vec![11, 12] && preferences == vec![21, 22]
    ));
    cancellation.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), observer)
            .await
            .is_ok()
    );
    server.abort();
    let _ = server.await;
    let _ = std::fs::remove_file(path);
}

#[test]
fn newline_decoder_bounds_and_releases_consumed_frames() {
    let mut decoder = NewlineJsonDecoder::new(32);
    let first = decoder.push(br#"{"State":"Running"}"#, "test");
    assert!(first.is_ok());
    assert!(decoder.finish("test").is_err());
    let mut decoder = NewlineJsonDecoder::new(32);
    let first = decoder.push(
        br#"{"State":"Running"}
{"Prefs":{}}
"#,
        "test",
    );
    assert!(first.is_ok());
    if let Ok(values) = first {
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].invalidation, WatchInvalidation::Status);
        assert_eq!(values[1].invalidation, WatchInvalidation::Preferences);
    }
    let mut decoder = NewlineJsonDecoder::new(4);
    assert!(decoder.push(b"12345", "test").is_err());
    assert!(decoder.push(b"{\"State\":null}\r\n", "test").is_err());
    let mut decoder = NewlineJsonDecoder::new(32);
    assert!(decoder.push(b"not-json\n", "test").is_err());
    assert!(
        decoder
            .push(
                br#"{"Prefs":"invalid"}
"#,
                "test"
            )
            .is_err()
    );
}
