use std::fs;
use std::path::Path;

#[test]
fn committed_fixture_manifests_are_complete_and_redaction_reviewed() {
    for directory in [
        "tests/fixtures/tailscale/1.98.9/linux",
        "tests/fixtures/local/services/1.98.9/linux",
        "tests/fixtures/local/transfers/1.98.9/linux",
    ] {
        let manifest = Path::new(directory).join("manifest.toml");
        let bytes = fs::read_to_string(&manifest);
        assert!(
            bytes.is_ok(),
            "missing fixture manifest: {}",
            manifest.display()
        );
        if let Ok(bytes) = bytes {
            for required in [
                "tailscale_version = \"1.98.9\"",
                "platform = \"linux\"",
                "command = ",
                "arguments = ",
                "exit_code = ",
                "stdout_file = ",
                "stderr_file = ",
                "captured_at = ",
                "redaction_reviewed = true",
                "[[fixtures]]",
            ] {
                assert!(bytes.contains(required), "manifest missing {required}");
            }
        }
    }
}

#[test]
fn unsupported_client_output_is_a_scoped_compatibility_result() {
    let error = tale::local::client::ClientError::UnsupportedOutput {
        operation: "version".to_owned(),
        detail: "required field missing".to_owned(),
    };
    let state = error.state("9.9.9");
    assert!(matches!(
        state,
        tale::domain::source::LocalState::UnsupportedClient { .. }
    ));
}
