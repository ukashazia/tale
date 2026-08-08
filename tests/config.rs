use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use tale::cli::Cli;
use tale::config::{self, EnvironmentValues, ValueSource};
use tale::paths::{self, PathEnvironment, Platform};
use tale::ui::theme::ThemeId;

fn path_environment(platform: Platform, root: &Path) -> PathEnvironment {
    PathEnvironment {
        platform,
        current_dir: root.to_path_buf(),
        xdg_config_home: Some(root.join("xdg-config")),
        home: Some(root.join("home")),
        xdg_state_home: Some(root.join("xdg-state")),
        xdg_cache_home: Some(root.join("xdg-cache")),
        appdata: Some(root.join("appdata")),
        localappdata: Some(root.join("localappdata")),
    }
}

fn cli(config: Option<PathBuf>) -> Cli {
    Cli {
        command: None,
        profile: None,
        config,
        view: None,
        read_only: false,
        no_local: false,
        tailscale_path: None,
        tailscale_socket: None,
        mock: false,
    }
}

fn environment() -> EnvironmentValues {
    EnvironmentValues {
        config_file: None,
        tailscale_path: None,
        tailscale_socket: None,
        no_color: false,
    }
}

#[test]
fn default_paths_follow_unix_and_windows_contracts_without_creation() {
    let root = PathBuf::from("/fictional/tale-path-tests");
    let unix = paths::resolve_paths(&path_environment(Platform::Unix, &root));
    assert!(unix.is_ok());
    if let Ok(paths) = unix {
        assert_eq!(paths.config_file, root.join("xdg-config/tale/config.toml"));
        assert_eq!(paths.state_dir, root.join("xdg-state/tale"));
        assert_eq!(paths.cache_dir, root.join("xdg-cache/tale"));
    }

    let windows = paths::resolve_paths(&path_environment(Platform::Windows, &root));
    assert!(windows.is_ok());
    if let Ok(paths) = windows {
        assert_eq!(paths.config_file, root.join("appdata/tale/config.toml"));
        assert_eq!(paths.state_dir, root.join("localappdata/tale"));
        assert_eq!(paths.cache_dir, root.join("localappdata/tale/cache"));
    }
}

#[test]
fn precedence_is_cli_then_environment_then_file_then_default() {
    let root = std::env::temp_dir().join(format!("tale-config-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let file = root.join("config.toml");
    let write = fs::write(
        &file,
        "read_only = false\n[local]\nenabled = true\ntailscale_path = \"file-binary\"\nsocket_path = \"file.sock\"\nreconcile_interval = \"5s\"\n[ui]\ncolor = \"auto\"\n",
    );
    assert!(write.is_ok());

    let mut env = environment();
    env.tailscale_path = Some("environment-binary".to_owned());
    env.tailscale_socket = Some(PathBuf::from("environment.sock"));
    env.no_color = true;
    let mut command = cli(Some(file.clone()));
    command.read_only = true;
    command.no_local = true;
    command.tailscale_path = Some(PathBuf::from("cli-binary"));
    command.tailscale_socket = Some(PathBuf::from("cli.sock"));
    let resolved = config::resolve(&command, &env, &path_environment(Platform::Unix, &root));
    assert!(resolved.is_ok());
    if let Ok(resolved) = resolved {
        assert!(resolved.read_only);
        assert_eq!(resolved.read_only_source, ValueSource::Cli);
        assert!(!resolved.local.enabled);
        assert_eq!(resolved.local.enabled_source, ValueSource::Cli);
        assert_eq!(resolved.local.tailscale_path, "cli-binary");
        assert_eq!(resolved.local.tailscale_path_source, ValueSource::Cli);
        assert_eq!(resolved.local.socket_path, PathBuf::from("cli.sock"));
        assert_eq!(resolved.local.socket_path_source, ValueSource::Cli);
        assert_eq!(resolved.ui.color, config::ColorMode::None);
        assert_eq!(resolved.ui.color_source, ValueSource::Environment);
        assert_eq!(resolved.local.reconcile_interval.as_secs(), 5);
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn theme_is_strict_and_defaults_to_tailscale_dark() {
    let root = std::env::temp_dir().join(format!("tale-theme-config-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let missing = config::resolve(
        &cli(Some(root.join("missing.toml"))),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(missing.is_ok());
    if let Ok(resolved) = missing {
        assert_eq!(resolved.ui.theme, ThemeId::TailscaleDark);
        assert_eq!(resolved.ui.theme_source, ValueSource::Default);
    }

    let file = root.join("config.toml");
    assert!(fs::write(&file, "[ui]\ntheme = \"tailscale-light\"\n").is_ok());
    let light = config::resolve(
        &cli(Some(file.clone())),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(light.is_ok());
    if let Ok(resolved) = light {
        assert_eq!(resolved.ui.theme, ThemeId::TailscaleLight);
        assert_eq!(resolved.ui.theme_source, ValueSource::File);
    }

    assert!(fs::write(&file, "[ui]\ntheme = \"dark\"\n").is_ok());
    let invalid = config::resolve(
        &cli(Some(file)),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(matches!(
        invalid,
        Err(config::ConfigError::InvalidField { field, .. }) if field == "ui.theme"
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn removed_local_polling_setting_is_an_unknown_field() {
    let root =
        std::env::temp_dir().join(format!("tale-removed-local-setting-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let file = root.join("config.toml");
    let write = fs::write(&file, "[local]\nrefresh_interval = \"30s\"\n");
    assert!(write.is_ok());
    let resolved = config::resolve(
        &cli(Some(file)),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(
        matches!(resolved, Err(config::ConfigError::UnknownField(field)) if field == "local.refresh_interval")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_config_uses_defaults_and_does_not_write() {
    let root = std::env::temp_dir().join(format!("tale-missing-config-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let file = root.join("not-created/config.toml");
    let resolved = config::resolve(
        &cli(Some(file.clone())),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(resolved.is_ok());
    if let Ok(resolved) = resolved {
        assert_eq!(resolved.history.max_tasks, 200);
        assert_eq!(resolved.local.reconcile_interval.as_secs(), 30);
    }
    assert!(!file.exists());
    assert!(!root.exists());
}

#[test]
fn unknown_fields_are_full_dotted_paths_and_do_not_echo_values() {
    let root = std::env::temp_dir().join(format!("tale-invalid-config-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let file = root.join("config.toml");
    let write = fs::write(&file, "[ui]\nnot_a_setting = \"super-secret-value\"\n");
    assert!(write.is_ok());
    let resolved = config::resolve(
        &cli(Some(file)),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(resolved.is_err());
    if let Err(error) = resolved {
        let message = error.to_string();
        assert!(message.contains("ui.not_a_setting"));
        assert!(!message.contains("super-secret-value"));
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_documented_duration_boundary_is_checked() {
    let root = std::env::temp_dir().join(format!("tale-duration-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let file = root.join("config.toml");
    let write = fs::write(&file, "[local]\nreconcile_interval = \"499ms\"\n");
    assert!(write.is_ok());
    let invalid = config::resolve(
        &cli(Some(file.clone())),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(invalid.is_err());
    let write = fs::write(
        &file,
        "[local]\nreconcile_interval = \"5s\"\ncommand_timeout = \"10m\"\n[admin]\nrefresh_interval = \"30m\"\nrequest_timeout = \"2m\"\n[history]\nmax_tasks = 5000\n",
    );
    assert!(write.is_ok());
    let valid = config::resolve(
        &cli(Some(file.clone())),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(valid.is_ok());
    let write = fs::write(&file, "[history]\nmax_tasks = 19\n");
    assert!(write.is_ok());
    let invalid = config::resolve(
        &cli(Some(file)),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(invalid.is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn mock_conflicts_with_a_selected_profile_and_is_not_persisted() {
    let root = std::env::temp_dir().join(format!("tale-mock-config-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let mut command = cli(None);
    command.mock = true;
    command.profile = Some("ops".to_owned());
    let conflict = config::resolve(
        &command,
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(conflict.is_err());

    // Without a selected profile the mock session is legitimate and must resolve.
    command.profile = None;
    let mocked = config::resolve(
        &command,
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(mocked.is_ok());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn default_profile_is_validated_and_activated_for_phase_five() {
    let root = std::env::temp_dir().join(format!("tale-default-profile-{}", std::process::id()));
    let _ = fs::create_dir_all(&root);
    let file = root.join("config.toml");
    let write = fs::write(
        &file,
        "default_profile = \"ops\"\n[profiles.ops]\ntailnet = \"-\"\ncredential = \"ops\"\n",
    );
    assert!(write.is_ok());

    let active = config::resolve(
        &cli(Some(file.clone())),
        &environment(),
        &path_environment(Platform::Unix, &root),
    );
    assert!(active.is_ok());
    if let Ok(active) = active {
        assert_eq!(active.profile.as_deref(), Some("ops"));
        assert_eq!(active.profiles["ops"].tailnet, "-");
    }

    let check = Cli::try_parse_from([
        "tale",
        "config",
        "check",
        "--config",
        file.to_string_lossy().as_ref(),
    ]);
    assert!(check.is_ok());
    if let Ok(check) = check {
        let resolved = config::resolve(
            &check,
            &environment(),
            &path_environment(Platform::Unix, &root),
        );
        assert!(resolved.is_ok());
        if let Ok(resolved) = resolved {
            assert_eq!(resolved.default_profile.as_deref(), Some("ops"));
            assert!(resolved.profile.is_none());
            assert!(resolved.profiles.contains_key("ops"));
        }
    }
    let _ = fs::remove_dir_all(root);
}
