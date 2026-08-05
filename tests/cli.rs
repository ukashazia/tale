use std::fs;
use std::path::PathBuf;
use std::process::Command;

use clap::{CommandFactory, Parser};

use tale::cli::{Cli, Command as TaleCommand};

fn parse(arguments: &[&str]) -> Option<Cli> {
    Cli::try_parse_from(arguments).ok()
}

#[test]
fn every_phase_one_flag_and_subcommand_parses() {
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
}

#[test]
fn invalid_arguments_are_rejected_before_the_tui() {
    let unknown = Cli::try_parse_from(["tale", "--not-a-phase-one-flag"]);
    assert!(unknown.is_err());

    let profile = Cli::try_parse_from(["tale", "--mock", "--profile", "ops"]);
    assert!(profile.is_ok());
}

#[test]
fn help_has_no_later_phase_routes() {
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
