use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::ResolvedConfig;

const BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct SupportBundle {
    pub schema_version: u32,
    pub tale: TaleBuild,
    pub operating_system: OperatingSystem,
    pub terminal: TerminalCapabilities,
    pub paths: ResolvedPaths,
    pub configuration: SafeConfiguration,
    pub local: LocalCapability,
    pub admin: AdminCapability,
    pub recent_tasks: RecentTasks,
    pub redaction: RedactionManifest,
}

#[derive(Debug, Serialize)]
pub struct TaleBuild {
    pub version: &'static str,
    pub target: &'static str,
}

#[derive(Debug, Serialize)]
pub struct OperatingSystem {
    pub family: &'static str,
    pub architecture: &'static str,
}

#[derive(Debug, Serialize)]
pub struct TerminalCapabilities {
    pub interactive: bool,
    pub terminal_class: &'static str,
    pub color_class: &'static str,
    pub symbol_class: &'static str,
    pub tmux_present: bool,
    pub mouse_enabled: bool,
    pub signal_class: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ResolvedPaths {
    pub config: String,
    pub state: String,
    pub cache: String,
}

#[derive(Debug, Serialize)]
pub struct SafeConfiguration {
    pub source_mode: &'static str,
    pub read_only: bool,
    pub no_local: bool,
    pub selected_profile: Option<String>,
    pub configured_profiles: Vec<String>,
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct LocalCapability {
    pub status: &'static str,
    pub executable: &'static str,
    pub client_version: Option<&'static str>,
    pub daemon_version: Option<&'static str>,
    pub capability_errors: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct AdminCapability {
    pub status: &'static str,
    pub endpoint_capabilities: Vec<&'static str>,
    pub error_classes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct RecentTasks {
    pub retained: usize,
    pub note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RedactionManifest {
    pub bounded: bool,
    pub excluded: Vec<&'static str>,
    pub pseudonymized: Vec<&'static str>,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum DoctorError {
    #[error("support bundle serialization failed")]
    Serialization,
    #[error("support bundle parent directory does not exist")]
    MissingParent,
    #[error("support bundle output already exists")]
    ExistingOutput,
    #[error("support bundle output must be a regular new path")]
    InvalidOutput,
    #[error("support bundle output could not be written: {0}")]
    Write(String),
}

pub fn build(config: &ResolvedConfig) -> SupportBundle {
    let source_mode = if config.mock {
        "mock"
    } else if config.no_local || !config.local.enabled {
        "local_disabled"
    } else {
        "local"
    };
    let configured_profiles = config
        .profiles
        .keys()
        .map(|value| pseudonymize(value))
        .collect::<Vec<_>>();
    let selected_profile = config.profile.as_deref().map(pseudonymize);
    let values = safe_configuration_values(config);
    SupportBundle {
        schema_version: BUNDLE_SCHEMA_VERSION,
        tale: TaleBuild {
            version: env!("CARGO_PKG_VERSION"),
            target: target_triple(),
        },
        operating_system: OperatingSystem {
            family: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
        },
        terminal: terminal_capabilities(config.ui.mouse),
        paths: ResolvedPaths {
            config: config.paths.config_file.display().to_string(),
            state: config.paths.state_dir.display().to_string(),
            cache: config.paths.cache_dir.display().to_string(),
        },
        configuration: SafeConfiguration {
            source_mode,
            read_only: config.read_only,
            no_local: config.no_local,
            selected_profile,
            configured_profiles,
            values,
        },
        local: LocalCapability {
            status: if config.mock {
                "mock"
            } else if config.no_local || !config.local.enabled {
                "disabled"
            } else {
                "not_probed"
            },
            executable: "not included",
            client_version: None,
            daemon_version: None,
            capability_errors: vec!["doctor does not spawn a local process"],
        },
        admin: AdminCapability {
            status: if config.profile.is_some() {
                "configured_not_probed"
            } else {
                "not_configured"
            },
            endpoint_capabilities: Vec::new(),
            error_classes: vec!["doctor does not contact the Control API"],
        },
        recent_tasks: RecentTasks {
            retained: 0,
            note: "doctor runs outside the TUI task history",
        },
        redaction: RedactionManifest {
            bounded: true,
            excluded: vec![
                "environment values",
                "keyring content",
                "access tokens",
                "client secrets",
                "auth-key and webhook secrets",
                "policy and audit content",
                "flow rows",
                "command stdout and stderr",
                "device, user, address, ID, and domain values",
                "clipboard content",
                "private certificate-key content",
            ],
            pseudonymized: vec!["profile names", "credential references"],
        },
    }
}

pub fn json_bytes(bundle: &SupportBundle) -> Result<Vec<u8>, DoctorError> {
    serde_json::to_vec_pretty(bundle).map_err(|_| DoctorError::Serialization)
}

pub fn text_report(bundle: &SupportBundle) -> Result<String, DoctorError> {
    let bytes = json_bytes(bundle)?;
    String::from_utf8(bytes).map_err(|_| DoctorError::Serialization)
}

pub fn write_new_atomic(bundle: &SupportBundle, path: &Path) -> Result<PathBuf, DoctorError> {
    let parent = path.parent().ok_or(DoctorError::MissingParent)?;
    if !parent.is_dir() {
        return Err(DoctorError::MissingParent);
    }
    match fs::symlink_metadata(path) {
        Ok(_) => return Err(DoctorError::ExistingOutput),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(DoctorError::InvalidOutput),
    }
    let bytes = json_bytes(bundle)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_nanos());
    let temporary = parent.join(format!(
        ".tale-support.{}.{}.tmp",
        std::process::id(),
        nonce
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true).read(true);
        let mut file = options
            .open(&temporary)
            .map_err(|error| DoctorError::Write(error.to_string()))?;
        set_private_permissions(&file)?;
        file.write_all(&bytes)
            .map_err(|error| DoctorError::Write(error.to_string()))?;
        file.flush()
            .map_err(|error| DoctorError::Write(error.to_string()))?;
        file.sync_all()
            .map_err(|error| DoctorError::Write(error.to_string()))?;
        fs::rename(&temporary, path).map_err(|error| DoctorError::Write(error.to_string()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(path.to_path_buf())
}

fn safe_configuration_values(config: &ResolvedConfig) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    values.insert("local.enabled".to_owned(), config.local.enabled.to_string());
    values.insert(
        "local.tailscale_path".to_owned(),
        if config.local.tailscale_path == "tailscale" {
            "default executable".to_owned()
        } else {
            "configured executable".to_owned()
        },
    );
    values.insert(
        "local.socket_path".to_owned(),
        config.local.socket_path.display().to_string(),
    );
    values.insert(
        "local.reconcile_interval".to_owned(),
        format_duration(config.local.reconcile_interval),
    );
    values.insert(
        "local.command_timeout".to_owned(),
        format_duration(config.local.command_timeout),
    );
    values.insert(
        "admin.refresh_interval".to_owned(),
        format_duration(config.admin.refresh_interval),
    );
    values.insert(
        "admin.request_timeout".to_owned(),
        format_duration(config.admin.request_timeout),
    );
    values.insert("ui.theme".to_owned(), config.ui.theme.as_str().to_owned());
    values.insert("ui.color".to_owned(), config.ui.color.as_str().to_owned());
    values.insert(
        "ui.color.resolved".to_owned(),
        format!(
            "{} ({})",
            config.ui.color.capability().as_str(),
            config.ui.color_source.label()
        ),
    );
    values.insert(
        "ui.symbols".to_owned(),
        config.ui.symbols.as_str().to_owned(),
    );
    values.insert("ui.mouse".to_owned(), config.ui.mouse.to_string());
    values.insert(
        "ui.detail_pane".to_owned(),
        config.ui.detail_pane.as_str().to_owned(),
    );
    values.insert(
        "ui.time_zone".to_owned(),
        config.ui.time_zone.as_str().to_owned(),
    );
    values.insert(
        "ui.relative_times".to_owned(),
        config.ui.relative_times.to_string(),
    );
    values.insert(
        "ui.show_footer".to_owned(),
        config.ui.show_footer.to_string(),
    );
    values.insert(
        "history.persist_tasks".to_owned(),
        config.history.persist_tasks.to_string(),
    );
    values.insert(
        "history.max_tasks".to_owned(),
        config.history.max_tasks.to_string(),
    );
    values
}

fn format_duration(value: std::time::Duration) -> String {
    if value.as_millis().is_multiple_of(1000) {
        format!("{}s", value.as_secs())
    } else {
        format!("{}ms", value.as_millis())
    }
}

fn pseudonymize(value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(value.as_bytes());
    let digest = digest.finalize();
    let suffix = digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("label-{suffix}")
}

fn target_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        _ => "unknown-target",
    }
}

fn terminal_capabilities(mouse_enabled: bool) -> TerminalCapabilities {
    let term = std::env::var("TERM").ok();
    let color = if std::env::var_os("NO_COLOR").is_some() {
        "none"
    } else if std::env::var("COLORTERM")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "truecolor" | "24bit"))
    {
        "truecolor"
    } else if term.as_deref() == Some("dumb") {
        "none"
    } else {
        "unknown"
    };
    TerminalCapabilities {
        interactive: std::io::IsTerminal::is_terminal(&std::io::stdout()),
        terminal_class: match term.as_deref() {
            Some("dumb") => "dumb",
            Some(_) => "named",
            None => "absent",
        },
        color_class: color,
        symbol_class: if term.as_deref() == Some("dumb") {
            "ascii"
        } else {
            "unknown"
        },
        tmux_present: std::env::var_os("TMUX").is_some(),
        mouse_enabled,
        signal_class: if cfg!(unix) {
            "unix termination signals"
        } else if cfg!(windows) {
            "windows console signals"
        } else {
            "unsupported"
        },
    }
}

fn set_private_permissions(file: &File) -> Result<(), DoctorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| DoctorError::Write(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build, json_bytes, write_new_atomic};
    use crate::cli::Cli;
    use crate::config::{EnvironmentValues, resolve};
    use crate::paths::PathEnvironment;
    use clap::Parser;
    use std::path::PathBuf;

    fn config() -> Result<crate::config::ResolvedConfig, String> {
        let cli = Cli::try_parse_from(["tale", "--mock"]).map_err(|error| error.to_string())?;
        let mut environment = EnvironmentValues::from_process();
        environment.config_file = Some(PathBuf::from("/tmp/tale-doctor-test-config.toml"));
        environment.profile = None;
        environment.access_token_present = false;
        let paths = PathEnvironment::from_process().map_err(|error| error.to_string())?;
        resolve(&cli, &environment, &paths).map_err(|error| error.to_string())
    }

    #[test]
    fn bundle_is_deterministic_and_secret_free() {
        let config = config();
        assert!(config.is_ok());
        if let Ok(config) = config {
            let first = json_bytes(&build(&config));
            let second = json_bytes(&build(&config));
            assert_eq!(first, second);
            if let Ok(bytes) = first {
                let text = String::from_utf8_lossy(&bytes);
                assert!(!text.contains("TALE_ACCESS_TOKEN"));
                assert!(!text.contains("client_secret"));
                assert!(!text.contains("fictional-secret"));
            }
        }
    }

    #[test]
    fn output_must_be_new_and_symlink_free() {
        let config = config();
        assert!(config.is_ok());
        if let Ok(config) = config {
            let directory = tempfile::tempdir();
            assert!(directory.is_ok());
            if let Ok(directory) = directory {
                let path = directory.path().join("bundle.json");
                let first = write_new_atomic(&build(&config), &path);
                assert!(first.is_ok());
                let second = write_new_atomic(&build(&config), &path);
                assert_eq!(second, Err(super::DoctorError::ExistingOutput));
            }
        }
    }
}
