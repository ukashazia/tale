use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tale::domain::source::{ExecutableSource, LocalState};
use tale::local::client::{ClientError, LocalCliClient, ResolvedExecutable};
use tale::local::process::Cancellation;

#[test]
fn version_output_errors_are_classified_as_unsupported_client() {
    let missing = ClientError::UnsupportedOutput {
        operation: "version".to_owned(),
        detail: "required CLI version was not returned".to_owned(),
    };
    assert!(matches!(
        missing.state("unknown"),
        LocalState::UnsupportedClient { .. }
    ));
    let unknown_flag = ClientError::NonZero {
        operation: "version".to_owned(),
        status: Some(2),
        detail: "unknown flag: --json".to_owned(),
    };
    assert!(matches!(
        unknown_flag.state("1.0.0"),
        LocalState::UnsupportedClient { .. }
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn discovery_probes_each_feature_once_and_keeps_unavailable_features_local() {
    use std::os::unix::fs::PermissionsExt;

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let number = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "tale-local-client-{}-{number}.sh",
        std::process::id()
    ));
    let body = r#"
case "$1" in
  --socket)
    if [ "$2" != "/fictional/tailscaled.sock" ]; then exit 3; fi
    shift 2
    ;;
esac
case "$1" in
  version)
    if [ "$2" != "--json" ] || [ "$3" != "--daemon" ]; then exit 2; fi
    printf '%s\n' '{"version":"1.98.9","unknown":true}'
    ;;
  status)
    if [ "$2" = "--help" ]; then exit 0; fi
    if [ "$2" = "--json" ]; then printf '%s\n' '{"Self":{"ID":"nodekey:self","HostName":"self"}}'; exit 0; fi
    exit 2
    ;;
  ping|netcheck)
    exit 0
    ;;
  dns)
    exit 0
    ;;
  whois)
    exit 1
    ;;
  *)
    exit 2
    ;;
esac
"#;
    assert!(fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}")).is_ok());
    let mut permissions = match fs::metadata(&path) {
        Ok(metadata) => metadata.permissions(),
        Err(_) => return,
    };
    permissions.set_mode(0o755);
    assert!(fs::set_permissions(&path, permissions).is_ok());

    let discovered = LocalCliClient::discover(
        ResolvedExecutable {
            path: path.clone(),
            socket_path: Some(PathBuf::from("/fictional/tailscaled.sock")),
            source: ExecutableSource::Path,
        },
        Duration::from_secs(2),
        &Cancellation::new(),
    )
    .await;
    assert!(discovered.is_ok());
    if let Ok(executable) = discovered {
        assert_eq!(executable.version, "1.98.9");
        assert_eq!(
            executable.socket_path,
            Some(PathBuf::from("/fictional/tailscaled.sock"))
        );
        assert!(executable.capabilities.ping);
        assert!(executable.capabilities.netcheck_json);
        assert!(executable.capabilities.netcheck_json_line);
        assert!(executable.capabilities.dns_status_json);
        assert!(executable.capabilities.dns_query_json);
        assert!(!executable.capabilities.whois_json);
    }
    let _ = fs::remove_file(path);
}

#[test]
fn resolved_executable_keeps_the_invoked_path_without_canonicalization() {
    let path = PathBuf::from("/tmp/path with spaces/tailscale");
    let resolved = ResolvedExecutable {
        path: path.clone(),
        socket_path: None,
        source: ExecutableSource::Cli,
    };
    assert_eq!(resolved.path, path);
}
