use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tale::local::process::{
    Cancellation, LocalCommand, LocalOperation, LocalProcessError, decode_utf8, run,
};

#[test]
fn command_debug_does_not_include_arguments_or_output() {
    let command = LocalCommand::new(
        OsString::from("/tmp/tailscale with spaces"),
        LocalOperation::DnsQuery,
        vec![
            OsString::from("name with spaces"),
            OsString::from("$(touch pwned)"),
        ],
    );
    let debug = format!("{command:?}");
    assert!(!debug.contains("tailscale with spaces"));
    assert!(!debug.contains("name with spaces"));
    assert!(!debug.contains("touch pwned"));
}

#[test]
fn exact_local_observer_command_vectors_are_typed() {
    let path = PathBuf::from("/tmp/tailscale with spaces");
    let ping =
        tale::local::diagnostics::ping_command(&path, Duration::from_secs(5), "name;$(not shell)");
    assert_eq!(
        ping.args,
        vec![
            OsString::from("ping"),
            OsString::from("--c=10"),
            OsString::from("--timeout=5s"),
            OsString::from("--until-direct=true"),
            OsString::from("name;$(not shell)"),
        ]
    );
    let one_shot =
        tale::local::diagnostics::netcheck_command(&path, Some(Duration::from_secs(5)), false);
    assert_eq!(
        one_shot.args,
        vec![OsString::from("netcheck"), OsString::from("--format=json")]
    );
    let live = tale::local::diagnostics::netcheck_command(&path, None, true);
    assert_eq!(
        live.args,
        vec![
            OsString::from("netcheck"),
            OsString::from("--format=json-line"),
            OsString::from("--every=2s"),
        ]
    );
    let query = tale::local::diagnostics::dns_query_command(
        &path,
        Duration::from_secs(5),
        "name with spaces;$(not shell)",
        tale::local::diagnostics::DnsRecordType::Aaaa,
    );
    assert_eq!(
        query.args,
        vec![
            OsString::from("dns"),
            OsString::from("query"),
            OsString::from("--json"),
            OsString::from("name with spaces;$(not shell)"),
            OsString::from("AAAA"),
        ]
    );
    let whois = tale::local::diagnostics::whois_command(
        &path,
        Duration::from_secs(5),
        "[fd7a::1]:41641",
        Some(tale::local::diagnostics::WhoisProtocol::Udp),
    );
    assert_eq!(
        whois.args,
        vec![
            OsString::from("whois"),
            OsString::from("--json"),
            OsString::from("--proto=udp"),
            OsString::from("[fd7a::1]:41641"),
        ]
    );
}

#[cfg(unix)]
mod unix_process_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    static NEXT_SCRIPT: AtomicUsize = AtomicUsize::new(0);

    fn script(body: &str) -> Option<PathBuf> {
        let number = NEXT_SCRIPT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tale-local-process-{}-{number}.sh",
            std::process::id()
        ));
        if fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).is_err() {
            return None;
        }
        let mut permissions = match fs::metadata(&path) {
            Ok(metadata) => metadata.permissions(),
            Err(_) => return None,
        };
        permissions.set_mode(0o755);
        if fs::set_permissions(&path, permissions).is_err() {
            return None;
        }
        Some(path)
    }

    fn remove(path: &PathBuf) {
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn user_values_remain_one_argv_value_without_shell_expansion() {
        let path = script("printf '%s\\n' \"$@\"");
        assert!(path.is_some());
        if let Some(path) = path {
            let command = LocalCommand::new(
                path.as_os_str().to_os_string(),
                LocalOperation::Ping,
                vec![
                    OsString::from("value with spaces"),
                    OsString::from("\"quoted\""),
                    OsString::from("$(touch /tmp/tale-must-not-exist)"),
                    OsString::from("a;b"),
                    OsString::from("-leading"),
                ],
            );
            let result = run(command, &Cancellation::new()).await;
            assert!(result.is_ok());
            if let Ok(result) = result {
                let output = String::from_utf8(result.stdout);
                assert!(output.is_ok());
                if let Ok(output) = output {
                    assert!(output.lines().any(|line| line == "value with spaces"));
                    assert!(output.lines().any(|line| line == "\"quoted\""));
                    assert!(
                        output
                            .lines()
                            .any(|line| line == "$(touch /tmp/tale-must-not-exist)")
                    );
                    assert!(output.lines().any(|line| line == "a;b"));
                    assert!(output.lines().any(|line| line == "-leading"));
                }
            }
            remove(&path);
        }
    }

    #[tokio::test]
    async fn timeout_and_cancellation_return_distinct_results_and_reap_child() {
        let timeout_path = script("while :; do :; done");
        assert!(timeout_path.is_some());
        if let Some(timeout_path) = timeout_path {
            let command = LocalCommand::new(
                timeout_path.as_os_str().to_os_string(),
                LocalOperation::Ping,
                Vec::new(),
            )
            .with_timeout(Duration::from_millis(40));
            let result =
                tokio::time::timeout(Duration::from_secs(1), run(command, &Cancellation::new()))
                    .await;
            assert!(result.is_ok());
            if let Ok(result) = result {
                assert_eq!(result, Err(LocalProcessError::TimedOut));
            }
            remove(&timeout_path);
        }

        let cancel_path = script("while :; do :; done");
        assert!(cancel_path.is_some());
        if let Some(cancel_path) = cancel_path {
            let cancellation = Cancellation::new();
            let task_cancellation = cancellation.clone();
            let command = LocalCommand::new(
                cancel_path.as_os_str().to_os_string(),
                LocalOperation::Ping,
                Vec::new(),
            )
            .without_timeout();
            let task = tokio::spawn(async move { run(command, &task_cancellation).await });
            tokio::time::sleep(Duration::from_millis(40)).await;
            cancellation.cancel();
            let result = tokio::time::timeout(Duration::from_secs(1), task).await;
            assert!(result.is_ok());
            if let Ok(result) = result {
                assert!(result.is_ok());
                if let Ok(result) = result {
                    assert_eq!(result, Err(LocalProcessError::Cancelled));
                }
            }
            remove(&cancel_path);
        }
    }

    #[tokio::test]
    async fn output_caps_and_non_utf8_decoding_are_bounded() {
        let path =
            script("i=0\nwhile [ \"$i\" -lt 10000 ]; do printf 1234567890; i=$((i + 1)); done");
        assert!(path.is_some());
        if let Some(path) = path {
            let command = LocalCommand::new(
                path.as_os_str().to_os_string(),
                LocalOperation::Netcheck,
                Vec::new(),
            )
            .with_limits(32, 32);
            let result = run(command, &Cancellation::new()).await;
            assert!(result.is_ok(), "{result:?}");
            if let Ok(result) = result {
                assert!(result.stdout.len() <= 32);
                assert!(result.truncated_stdout);
            }
            remove(&path);
        }

        let path = script("printf '\\377'");
        assert!(path.is_some());
        if let Some(path) = path {
            let command = LocalCommand::new(
                path.as_os_str().to_os_string(),
                LocalOperation::Netcheck,
                Vec::new(),
            );
            let result = run(command, &Cancellation::new()).await;
            assert!(result.is_ok());
            if let Ok(result) = result {
                let decoded = decode_utf8(&result.stdout);
                assert!(matches!(decoded, Err(LocalProcessError::OutputNotUtf8(_))));
            }
            remove(&path);
        }
    }

    #[test]
    fn non_executable_script_is_not_treated_as_a_runnable_child() {
        let path = script("exit 0");
        assert!(path.is_some());
        if let Some(path) = path {
            let mut permissions = match fs::metadata(&path) {
                Ok(metadata) => metadata.permissions(),
                Err(_) => return,
            };
            permissions.set_mode(0o644);
            let _ = fs::set_permissions(&path, permissions);
            let metadata = fs::metadata(&path);
            assert!(metadata.is_ok());
            if let Ok(metadata) = metadata {
                assert_eq!(metadata.permissions().mode() & 0o111, 0);
            }
            remove(&path);
        }
    }
}
