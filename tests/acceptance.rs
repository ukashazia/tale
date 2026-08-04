use std::fs;
use std::process::Command;

#[test]
fn generated_cli_artifacts_match_the_typed_command_definition() {
    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    if let Ok(directory) = directory {
        let output = Command::new(env!("CARGO_BIN_EXE_generate-artifacts"))
            .args(["--output-dir", directory.path().to_string_lossy().as_ref()])
            .output();
        assert!(output.is_ok());
        if let Ok(output) = output {
            assert!(output.status.success());
            for relative in [
                "completions/tale.bash",
                "completions/_tale",
                "completions/tale.fish",
                "docs/cli/tale.1",
            ] {
                let generated = fs::read(directory.path().join(relative));
                let committed = fs::read(relative);
                assert!(generated.is_ok());
                assert!(committed.is_ok());
                if let (Ok(generated), Ok(committed)) = (generated, committed) {
                    assert_eq!(generated, committed, "artifact mismatch: {relative}");
                    let artifact = String::from_utf8_lossy(&generated);
                    assert!(
                        !artifact.contains("--mock"),
                        "internal mock flag in {relative}"
                    );
                }
            }
        }
    }
}

#[test]
fn doctor_support_bundle_is_allowlisted_and_deterministic() {
    let first = Command::new(env!("CARGO_BIN_EXE_tale"))
        .args(["doctor", "--mock"])
        .output();
    let second = Command::new(env!("CARGO_BIN_EXE_tale"))
        .args(["doctor", "--mock"])
        .output();
    assert!(first.is_ok());
    assert!(second.is_ok());
    if let (Ok(first), Ok(second)) = (first, second) {
        assert!(first.status.success());
        assert!(second.status.success());
        assert_eq!(first.stdout, second.stdout);
        let text = String::from_utf8_lossy(&first.stdout);
        for forbidden in [
            "fictional-secret-canary",
            "TALE_ACCESS_TOKEN=fictional",
            "Bearer fictional",
            "client-secret-value",
            "access-token-value",
            "webhook-signing-secret-value",
        ] {
            assert!(!text.contains(forbidden), "doctor leaked {forbidden}");
        }
        assert!(text.contains("redaction"));
        assert!(text.contains("schema_version"));
    }
}

#[test]
fn doctor_writes_only_a_new_private_bundle_path() {
    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    if let Ok(directory) = directory {
        let output_path = directory.path().join("bundle.json");
        let output = Command::new(env!("CARGO_BIN_EXE_tale"))
            .args([
                "doctor",
                "--mock",
                "--output",
                output_path.to_string_lossy().as_ref(),
            ])
            .output();
        assert!(output.is_ok());
        if let Ok(output) = output {
            assert!(output.status.success());
            let first = fs::read(&output_path);
            assert!(first.is_ok());
            let second = Command::new(env!("CARGO_BIN_EXE_tale"))
                .args([
                    "doctor",
                    "--mock",
                    "--output",
                    output_path.to_string_lossy().as_ref(),
                ])
                .output();
            assert!(second.is_ok());
            if let (Ok(first), Ok(second)) = (first, second) {
                assert!(second.status.code().is_some_and(|code| code != 0));
                assert_eq!(fs::read(&output_path).ok(), Some(first));
            }
        }
    }
}

#[test]
fn all_fifteen_acceptance_journeys_have_explicit_evidence_status() {
    let matrix = fs::read_to_string("tests/acceptance/journeys.md");
    assert!(matrix.is_ok());
    if let Ok(matrix) = matrix {
        for number in 1..=15 {
            assert!(
                matrix.contains(&format!("| {number} |")),
                "missing acceptance journey {number}"
            );
        }
        assert!(matrix.contains("Real-environment evidence"));
        assert!(matrix.contains("Blocked:"));
    }
}
