use std::fs;
use std::path::PathBuf;
use std::process::Command;

use clap::{CommandFactory, Parser};

use tale::cli::{Cli, Command as TaleCommand};

fn parse(arguments: &[&str]) -> Option<Cli> {
    Cli::try_parse_from(arguments).ok()
}

#[test]
fn every_flag_and_subcommand_parses() {
    let cli = parse(&[
        "tale",
        "--profile",
        "ops",
        "--config",
        "/tmp/tale-config.toml",
        "--view",
        "devices",
        "--read-only",
        "--no-local",
        "--tailscale-path",
        "/fictional/tailscale",
        "--tailscale-socket",
        "/fictional/tailscaled.sock",
        "--mock",
    ]);
    assert!(cli.is_some());
    if let Some(cli) = cli {
        assert!(cli.profile.is_some());
        assert!(cli.config.is_some());
        assert_eq!(cli.view.as_deref(), Some("devices"));
        assert!(cli.read_only);
        assert!(cli.no_local);
        assert!(cli.tailscale_path.is_some());
        assert_eq!(
            cli.tailscale_socket.as_deref(),
            Some(std::path::Path::new("/fictional/tailscaled.sock"))
        );
        assert!(cli.mock);
    }

    let cli = parse(&[
        "tale",
        "config",
        "path",
        "--config",
        "/tmp/tale-config.toml",
    ]);
    assert!(matches!(
        cli.map(|value| value.command),
        Some(Some(TaleCommand::Config { .. }))
    ));

    let cli = parse(&[
        "tale",
        "config",
        "check",
        "--config",
        "/tmp/tale-config.toml",
    ]);
    assert!(matches!(
        cli.map(|value| value.command),
        Some(Some(TaleCommand::Config { .. }))
    ));

    let cli = parse(&[
        "tale",
        "doctor",
        "--config",
        "/tmp/tale-config.toml",
        "--mock",
    ]);
    assert!(matches!(
        cli.map(|value| value.command),
        Some(Some(TaleCommand::Doctor(_)))
    ));

    let cli = parse(&["tale", "gen-completions", "--shell", "/bin/zsh"]);
    assert!(matches!(
        cli.map(|value| value.command),
        Some(Some(TaleCommand::GenCompletions(_)))
    ));
}

#[test]
fn invalid_arguments_are_rejected_before_the_tui() {
    let unknown = Cli::try_parse_from(["tale", "--not-a-real-flag"]);
    assert!(unknown.is_err());

    let profile = Cli::try_parse_from(["tale", "--mock", "--profile", "ops"]);
    assert!(profile.is_ok());
}

#[test]
fn help_does_not_expose_tui_routes_as_subcommands() {
    let help = Cli::command().render_help().to_string();
    assert!(help.contains("config"));
    assert!(help.contains("doctor"));
    assert!(!help.contains("users"));
    assert!(!help.contains("credentials"));
    assert!(!help.contains("services"));
}

#[test]
fn non_interactive_binary_commands_do_not_enter_an_alternate_screen() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_file = std::env::temp_dir().join(format!("tale-cli-{}.toml", std::process::id()));
    let _ = fs::remove_file(&config_file);
    let output = Command::new(env!("CARGO_BIN_EXE_tale"))
        .current_dir(root)
        .args(["config", "path", "--config"])
        .arg(&config_file)
        .output();
    assert!(output.is_ok());
    if let Ok(output) = output {
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("config"));
        assert!(!stdout.contains("\u{1b}[?1049h"));
    }
    assert!(!config_file.exists());
}

#[test]
fn generated_completions_are_available_from_the_binary() {
    for shell in ["bash", "zsh", "fish"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tale"))
            .args(["gen-completions", "--shell", shell])
            .output();
        assert!(output.is_ok());
        if let Ok(output) = output {
            assert!(output.status.success(), "shell: {shell}");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("tale"), "shell: {shell}");
            assert!(!stdout.contains("--mock"), "shell: {shell}");
            assert!(!stdout.contains("\u{1b}[?1049h"), "shell: {shell}");
        }
    }
}

#[test]
fn invalid_route_and_mock_profile_have_exit_code_two() {
    for arguments in [
        vec!["--view", "future-route"],
        vec!["--mock", "--profile", "ops"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tale"))
            .args(arguments)
            .output();
        assert!(output.is_ok());
        if let Ok(output) = output {
            assert_eq!(output.status.code(), Some(2));
        }
    }
}

/// `auth add` is the only writer to the credential store and the sole recovery path
/// after `auth remove` empties a config, so it has to work without a controlling
/// terminal. `rpassword` reads `/dev/tty` directly, which a pipe cannot satisfy.
#[test]
fn auth_add_accepts_a_secret_without_a_controlling_terminal() {
    use std::io::Write;
    use std::process::Stdio;

    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    let Ok(directory) = directory else { return };

    let child = Command::new(env!("CARGO_BIN_EXE_tale"))
        .env("XDG_CONFIG_HOME", directory.path())
        .args([
            "auth",
            "add",
            "scripted",
            "--tailnet",
            "TLFIXTURE",
            "--kind",
            "access-token",
            "--secret-stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    assert!(child.is_ok());
    let Ok(mut child) = child else { return };
    if let Some(mut stdin) = child.stdin.take() {
        assert!(stdin.write_all(b"").is_ok());
    }
    let output = child.wait_with_output();
    assert!(output.is_ok());
    if let Ok(output) = output {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // An empty pipe must be reported as an empty secret. Reaching this message
        // proves stdin was the source; the old prompt-only path failed earlier with
        // "credential input was cancelled" because it could not open a tty at all.
        assert!(
            stderr.contains("access token cannot be empty"),
            "unexpected failure: {stderr}"
        );
    }
}

#[test]
fn scripted_auth_add_reports_the_missing_flag_instead_of_blocking_on_a_prompt() {
    use std::process::Stdio;

    let directory = tempfile::tempdir();
    assert!(directory.is_ok());
    let Ok(directory) = directory else { return };

    for (arguments, expected) in [
        (vec!["auth", "add", "p", "--secret-stdin"], "--tailnet"),
        (
            vec!["auth", "add", "p", "--tailnet", "T", "--secret-stdin"],
            "--kind",
        ),
        (
            vec![
                "auth",
                "add",
                "p",
                "--tailnet",
                "T",
                "--kind",
                "oauth-client",
                "--secret-stdin",
            ],
            "--client-id",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tale"))
            .env("XDG_CONFIG_HOME", directory.path())
            .args(&arguments)
            .stdin(Stdio::null())
            .output();
        assert!(output.is_ok());
        if let Ok(output) = output {
            assert_eq!(output.status.code(), Some(2), "arguments: {arguments:?}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains(expected),
                "arguments: {arguments:?}: {stderr}"
            );
        }
    }
}
