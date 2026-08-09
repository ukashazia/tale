use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::{Cli, Command, ConfigCommand};
use crate::paths::{self, PathEnvironment, PathError, Paths};
use crate::secrets::CredentialBackend;
use thiserror::Error;

use crate::ui::theme::{ColorCapability, ThemeId};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ValueSource {
    Cli,
    Environment,
    File,
    Default,
}

impl ValueSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Environment => "environment",
            Self::File => "file",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnvironmentValues {
    pub config_file: Option<PathBuf>,
    pub tailscale_path: Option<String>,
    pub tailscale_socket: Option<PathBuf>,
    pub no_color: bool,
}

impl EnvironmentValues {
    pub fn from_process() -> Self {
        Self {
            config_file: std::env::var_os("TALE_CONFIG_FILE").map(PathBuf::from),
            tailscale_path: std::env::var("TALE_TAILSCALE_PATH").ok(),
            tailscale_socket: std::env::var_os("TALE_TAILSCALE_SOCKET").map(PathBuf::from),
            no_color: std::env::var_os("NO_COLOR").is_some(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub paths: Paths,
    pub mock: bool,
    pub read_only: bool,
    pub no_local: bool,
    pub profile: Option<String>,
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub read_only_source: ValueSource,
    pub local: LocalConfig,
    pub admin: AdminConfig,
    pub ui: UiConfig,
    pub history: HistoryConfig,
}

#[derive(Debug, Clone)]
pub struct LocalConfig {
    pub enabled: bool,
    pub tailscale_path: String,
    pub socket_path: PathBuf,
    pub reconcile_interval: Duration,
    pub command_timeout: Duration,
    pub enabled_source: ValueSource,
    pub tailscale_path_source: ValueSource,
    pub socket_path_source: ValueSource,
    pub reconcile_interval_source: ValueSource,
    pub command_timeout_source: ValueSource,
}

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub refresh_interval: Duration,
    pub request_timeout: Duration,
    pub refresh_interval_source: ValueSource,
    pub request_timeout_source: ValueSource,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ColorMode {
    Auto,
    None,
    Ansi16,
    Ansi256,
    TrueColor,
}

impl ColorMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
            Self::Ansi16 => "ansi16",
            Self::Ansi256 => "ansi256",
            Self::TrueColor => "truecolor",
        }
    }

    pub const fn capability(self) -> ColorCapability {
        match self {
            Self::Auto | Self::TrueColor => ColorCapability::TrueColor,
            Self::Ansi256 => ColorCapability::Ansi256,
            Self::Ansi16 => ColorCapability::Ansi16,
            Self::None => ColorCapability::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SymbolsMode {
    Auto,
    Ascii,
    Unicode,
}

impl SymbolsMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ascii => "ascii",
            Self::Unicode => "unicode",
        }
    }

    /// Whether to draw the Unicode glyph. `Auto` says yes: the frame already
    /// uses box-drawing borders, `·` separators, and `…` ellipses, so a
    /// terminal that cannot render those is already broken, and ASCII markers
    /// beside Unicode borders only look inconsistent. `ascii` is the opt-out.
    pub const fn unicode(self) -> bool {
        matches!(self, Self::Auto | Self::Unicode)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DetailPaneMode {
    Auto,
    Always,
    Never,
}

impl DetailPaneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TimeZoneMode {
    Local,
    Utc,
}

impl TimeZoneMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Utc => "utc",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiConfig {
    pub theme: ThemeId,
    pub color: ColorMode,
    pub symbols: SymbolsMode,
    pub mouse: bool,
    pub detail_pane: DetailPaneMode,
    pub time_zone: TimeZoneMode,
    pub relative_times: bool,
    pub show_footer: bool,
    pub theme_source: ValueSource,
    pub color_source: ValueSource,
    pub symbols_source: ValueSource,
    pub mouse_source: ValueSource,
    pub detail_pane_source: ValueSource,
    pub time_zone_source: ValueSource,
    pub relative_times_source: ValueSource,
    pub show_footer_source: ValueSource,
}

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub persist_tasks: bool,
    pub max_tasks: usize,
    pub persist_tasks_source: ValueSource,
    pub max_tasks_source: ValueSource,
}

#[derive(Debug, Clone)]
pub struct ProfileConfig {
    pub tailnet: String,
    pub read_only: bool,
    pub credential: String,
    /// Which store holds this profile's credential, and where. Stated per profile so the
    /// configuration is self-describing rather than depending on a process-wide default.
    pub credential_backend: CredentialBackend,
}

#[derive(Debug, Clone)]
pub struct SettingDisplay {
    pub name: &'static str,
    pub value: String,
    pub source: ValueSource,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("unknown field {0}")]
    UnknownField(String),
    #[error("field {field} must be {expected}")]
    InvalidField {
        field: String,
        expected: &'static str,
    },
    #[error("field {field} is outside the allowed range {allowed}")]
    OutOfRange {
        field: String,
        allowed: &'static str,
    },
    #[error("field {field} is required")]
    MissingField { field: String },
    #[error("invalid TOML syntax")]
    InvalidToml,
    #[error("configuration file could not be read")]
    ReadFailure,
    #[error("configuration file could not be written")]
    WriteFailure,
    #[error("profile does not exist: {0}")]
    UnknownProfile(String),
    #[error("--mock cannot be combined with a profile")]
    MockConflict,
}

#[derive(Debug, Clone)]
struct FileConfig {
    read_only: Option<bool>,
    local: FileLocal,
    admin: FileAdmin,
    ui: FileUi,
    history: FileHistory,
    profiles: BTreeMap<String, ProfileConfig>,
}

#[derive(Debug, Clone, Default)]
struct FileLocal {
    enabled: Option<bool>,
    tailscale_path: Option<String>,
    tailscale_socket: Option<PathBuf>,
    reconcile_interval: Option<Duration>,
    command_timeout: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
struct FileAdmin {
    refresh_interval: Option<Duration>,
    request_timeout: Option<Duration>,
}

#[derive(Debug, Clone, Default)]
struct FileUi {
    theme: Option<ThemeId>,
    color: Option<ColorMode>,
    symbols: Option<SymbolsMode>,
    mouse: Option<bool>,
    detail_pane: Option<DetailPaneMode>,
    time_zone: Option<TimeZoneMode>,
    relative_times: Option<bool>,
    show_footer: Option<bool>,
}

#[derive(Debug, Clone, Default)]
struct FileHistory {
    persist_tasks: Option<bool>,
    max_tasks: Option<usize>,
}

pub fn resolve(
    cli: &Cli,
    environment: &EnvironmentValues,
    path_environment: &PathEnvironment,
) -> Result<ResolvedConfig, ConfigError> {
    if cli.mock && cli.profile.is_some() {
        return Err(ConfigError::MockConflict);
    }
    let mut paths = paths::resolve_paths(path_environment).map_err(path_error)?;
    let config_path = cli.config.as_deref().or(environment.config_file.as_deref());
    if let Some(config_path) = config_path {
        paths = paths::with_config_file(paths, config_path, &path_environment.current_dir);
    }

    let file = read_file_config(&paths.config_file)?;
    // A session starts on the local client. An admin profile is a deliberate
    // act, on the command line or on `:profiles`, never a leftover in a file.
    let selected_profile = cli.profile.clone();
    if let Some(profile) = selected_profile.as_deref()
        && !file.profiles.contains_key(profile)
    {
        return Err(ConfigError::UnknownProfile(profile.to_owned()));
    }
    if cli.mock && selected_profile.is_some() {
        return Err(ConfigError::MockConflict);
    }
    let activate_profile = !matches!(
        cli.command,
        Some(Command::Config {
            command: ConfigCommand::Check,
        })
    );

    let read_only = choose_bool(file.read_only, false, ValueSource::File, cli.read_only);
    let read_only_source = if cli.read_only {
        ValueSource::Cli
    } else if file.read_only.is_some() {
        ValueSource::File
    } else {
        ValueSource::Default
    };
    let local_enabled = if cli.no_local {
        false
    } else {
        file.local.enabled.is_none_or(|value| value)
    };
    let local_enabled_source = if cli.no_local {
        ValueSource::Cli
    } else if file.local.enabled.is_some() {
        ValueSource::File
    } else {
        ValueSource::Default
    };
    let tailscale_path = if let Some(path) = cli.tailscale_path.as_deref() {
        path.to_string_lossy().into_owned()
    } else if let Some(path) = environment.tailscale_path.as_deref() {
        path.to_owned()
    } else {
        match file.local.tailscale_path.clone() {
            Some(value) => value,
            None => "tailscale".to_owned(),
        }
    };
    let tailscale_path_source = if cli.tailscale_path.is_some() {
        ValueSource::Cli
    } else if environment.tailscale_path.is_some() {
        ValueSource::Environment
    } else if file.local.tailscale_path.is_some() {
        ValueSource::File
    } else {
        ValueSource::Default
    };

    let socket_path = match cli
        .tailscale_socket
        .clone()
        .or_else(|| environment.tailscale_socket.clone())
        .or_else(|| file.local.tailscale_socket.clone())
    {
        Some(path) => path,
        None => crate::local::daemon::documented_socket_path(),
    };
    let socket_path_source = if cli.tailscale_socket.is_some() {
        ValueSource::Cli
    } else if environment.tailscale_socket.is_some() {
        ValueSource::Environment
    } else if file.local.tailscale_socket.is_some() {
        ValueSource::File
    } else {
        ValueSource::Default
    };

    let color = if environment.no_color {
        ColorMode::None
    } else {
        file.ui.color.map_or(ColorMode::Auto, |value| value)
    };
    let color_source = if environment.no_color {
        ValueSource::Environment
    } else if file.ui.color.is_some() {
        ValueSource::File
    } else {
        ValueSource::Default
    };

    Ok(ResolvedConfig {
        paths,
        mock: cli.mock,
        read_only,
        no_local: cli.no_local,
        profile: if activate_profile {
            selected_profile
        } else {
            None
        },
        profiles: file.profiles,
        read_only_source,
        local: LocalConfig {
            enabled: local_enabled,
            tailscale_path,
            socket_path,
            reconcile_interval: file
                .local
                .reconcile_interval
                .map_or(Duration::from_secs(30), |value| value),
            command_timeout: file
                .local
                .command_timeout
                .map_or(Duration::from_secs(10), |value| value),
            enabled_source: local_enabled_source,
            tailscale_path_source,
            socket_path_source,
            reconcile_interval_source: source_for(file.local.reconcile_interval.is_some()),
            command_timeout_source: source_for(file.local.command_timeout.is_some()),
        },
        admin: AdminConfig {
            refresh_interval: file
                .admin
                .refresh_interval
                .map_or(Duration::from_secs(30), |value| value),
            request_timeout: file
                .admin
                .request_timeout
                .map_or(Duration::from_secs(15), |value| value),
            refresh_interval_source: source_for(file.admin.refresh_interval.is_some()),
            request_timeout_source: source_for(file.admin.request_timeout.is_some()),
        },
        ui: UiConfig {
            theme: file.ui.theme.unwrap_or(ThemeId::TailscaleDark),
            color,
            symbols: file.ui.symbols.map_or(SymbolsMode::Auto, |value| value),
            mouse: file.ui.mouse.is_some_and(|value| value),
            detail_pane: file
                .ui
                .detail_pane
                .map_or(DetailPaneMode::Auto, |value| value),
            time_zone: file.ui.time_zone.map_or(TimeZoneMode::Local, |value| value),
            relative_times: file.ui.relative_times.is_none_or(|value| value),
            show_footer: file.ui.show_footer.is_none_or(|value| value),
            theme_source: source_for(file.ui.theme.is_some()),
            color_source,
            symbols_source: source_for(file.ui.symbols.is_some()),
            mouse_source: source_for(file.ui.mouse.is_some()),
            detail_pane_source: source_for(file.ui.detail_pane.is_some()),
            time_zone_source: source_for(file.ui.time_zone.is_some()),
            relative_times_source: source_for(file.ui.relative_times.is_some()),
            show_footer_source: source_for(file.ui.show_footer.is_some()),
        },
        history: HistoryConfig {
            persist_tasks: file.history.persist_tasks.is_some_and(|value| value),
            max_tasks: file.history.max_tasks.map_or(200, |value| value),
            persist_tasks_source: source_for(file.history.persist_tasks.is_some()),
            max_tasks_source: source_for(file.history.max_tasks.is_some()),
        },
    })
}

pub fn resolve_paths_for_cli(
    cli: &Cli,
    environment: &EnvironmentValues,
    path_environment: &PathEnvironment,
) -> Result<Paths, ConfigError> {
    let paths = paths::resolve_paths(path_environment).map_err(path_error)?;
    Ok(cli
        .config
        .as_deref()
        .or(environment.config_file.as_deref())
        .map_or(paths.clone(), |config_path| {
            paths::with_config_file(paths, config_path, &path_environment.current_dir)
        }))
}

pub fn is_valid_profile_name(value: &str) -> bool {
    valid_profile_name(value)
}

pub fn write_profile_atomic(
    path: &Path,
    profile_name: &str,
    profile: &ProfileConfig,
) -> Result<(), ConfigError> {
    if !valid_profile_name(profile_name) {
        return Err(ConfigError::InvalidField {
            field: "profile".to_owned(),
            expected: "an ASCII profile name containing letters, digits, '_' or '-'",
        });
    }
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err(ConfigError::ReadFailure),
    };
    if !contents.is_empty() {
        let _ = parse_file_config(&contents)?;
    }
    let mut root = if contents.is_empty() {
        toml::Table::new()
    } else {
        toml::from_str::<toml::Table>(&contents).map_err(|_| ConfigError::InvalidToml)?
    };
    let profiles = root
        .entry("profiles".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or(ConfigError::InvalidField {
            field: "profiles".to_owned(),
            expected: "a TOML table",
        })?;
    let mut profile_table = toml::Table::new();
    profile_table.insert(
        "tailnet".to_owned(),
        toml::Value::String(profile.tailnet.clone()),
    );
    profile_table.insert(
        "read_only".to_owned(),
        toml::Value::Boolean(profile.read_only),
    );
    profile_table.insert(
        "credential".to_owned(),
        toml::Value::String(profile.credential.clone()),
    );
    profile_table.insert(
        "credential_backend".to_owned(),
        toml::Value::String(profile.credential_backend.label().to_owned()),
    );
    match &profile.credential_backend {
        CredentialBackend::File { path } => {
            profile_table.insert(
                "credential_file".to_owned(),
                toml::Value::String(path.display().to_string()),
            );
        }
    }
    profiles.insert(profile_name.to_owned(), toml::Value::Table(profile_table));
    let serialized =
        toml::to_string_pretty(&toml::Value::Table(root)).map_err(|_| ConfigError::WriteFailure)?;
    atomic_write(path, serialized.as_bytes())
}

pub fn remove_profile_atomic(path: &Path, profile_name: &str) -> Result<bool, ConfigError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(ConfigError::ReadFailure),
    };
    let _ = parse_file_config(&contents)?;
    let mut root =
        toml::from_str::<toml::Table>(&contents).map_err(|_| ConfigError::InvalidToml)?;
    let mut removed = false;
    if let Some(profiles) = root.get_mut("profiles").and_then(toml::Value::as_table_mut) {
        removed = profiles.remove(profile_name).is_some();
        if profiles.is_empty() {
            root.remove("profiles");
        }
    }
    if removed {
        let serialized = toml::to_string_pretty(&toml::Value::Table(root))
            .map_err(|_| ConfigError::WriteFailure)?;
        atomic_write(path, serialized.as_bytes())?;
    }
    Ok(removed)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| ConfigError::WriteFailure)?;
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ConfigError::WriteFailure)?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| ConfigError::WriteFailure)?;
    let write_result = file.write_all(bytes).and_then(|_| file.sync_all());
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(ConfigError::WriteFailure);
    }
    fs::rename(&temporary, path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        ConfigError::WriteFailure
    })
}

fn choose_bool(file: Option<bool>, default: bool, _file_source: ValueSource, cli: bool) -> bool {
    if cli {
        true
    } else {
        file.map_or(default, |value| value)
    }
}

fn source_for(from_file: bool) -> ValueSource {
    if from_file {
        ValueSource::File
    } else {
        ValueSource::Default
    }
}

fn path_error(error: PathError) -> ConfigError {
    ConfigError::InvalidField {
        field: "paths".to_owned(),
        expected: match error {
            PathError::CurrentDirectory(_) => "a usable current directory",
            PathError::MissingEnvironment(_) => "the required platform environment path",
        },
    }
}

fn read_file_config(path: &Path) -> Result<FileConfig, ConfigError> {
    match fs::read_to_string(path) {
        Ok(contents) => parse_file_config(&contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileConfig {
            read_only: None,
            local: FileLocal::default(),
            admin: FileAdmin::default(),
            ui: FileUi::default(),
            history: FileHistory::default(),
            profiles: BTreeMap::new(),
        }),
        Err(_) => Err(ConfigError::ReadFailure),
    }
}

fn parse_file_config(contents: &str) -> Result<FileConfig, ConfigError> {
    let root = toml::from_str::<toml::Table>(contents).map_err(|_| ConfigError::InvalidToml)?;
    check_unknown(
        &root,
        "",
        &["read_only", "local", "admin", "ui", "history", "profiles"],
    )?;

    let read_only = optional_bool(&root, "read_only", "read_only")?;
    let local_table = optional_table(&root, "local", "local")?;
    check_unknown(
        local_table,
        "local",
        &[
            "enabled",
            "tailscale_path",
            "socket_path",
            "reconcile_interval",
            "command_timeout",
        ],
    )?;
    let local = FileLocal {
        enabled: optional_bool(local_table, "enabled", "local.enabled")?,
        tailscale_path: optional_string(local_table, "tailscale_path", "local.tailscale_path")?,
        tailscale_socket: optional_path(local_table, "socket_path", "local.socket_path")?,
        reconcile_interval: optional_duration(
            local_table,
            "reconcile_interval",
            "local.reconcile_interval",
            Duration::from_secs(5),
            Duration::from_secs(600),
        )?,
        command_timeout: optional_duration(
            local_table,
            "command_timeout",
            "local.command_timeout",
            Duration::from_secs(1),
            Duration::from_secs(600),
        )?,
    };

    let admin_table = optional_table(&root, "admin", "admin")?;
    check_unknown(
        admin_table,
        "admin",
        &["refresh_interval", "request_timeout"],
    )?;
    let admin = FileAdmin {
        refresh_interval: optional_duration(
            admin_table,
            "refresh_interval",
            "admin.refresh_interval",
            Duration::from_secs(5),
            Duration::from_secs(1800),
        )?,
        request_timeout: optional_duration(
            admin_table,
            "request_timeout",
            "admin.request_timeout",
            Duration::from_secs(1),
            Duration::from_secs(120),
        )?,
    };

    let ui_table = optional_table(&root, "ui", "ui")?;
    check_unknown(
        ui_table,
        "ui",
        &[
            "theme",
            "color",
            "symbols",
            "mouse",
            "detail_pane",
            "time_zone",
            "relative_times",
            "show_footer",
        ],
    )?;
    let ui = FileUi {
        theme: optional_enum(ui_table, "theme", "ui.theme", ThemeId::parse)?,
        color: optional_enum(ui_table, "color", "ui.color", parse_color)?,
        symbols: optional_enum(ui_table, "symbols", "ui.symbols", parse_symbols)?,
        mouse: optional_bool(ui_table, "mouse", "ui.mouse")?,
        detail_pane: optional_enum(ui_table, "detail_pane", "ui.detail_pane", parse_detail_pane)?,
        time_zone: optional_enum(ui_table, "time_zone", "ui.time_zone", parse_time_zone)?,
        relative_times: optional_bool(ui_table, "relative_times", "ui.relative_times")?,
        show_footer: optional_bool(ui_table, "show_footer", "ui.show_footer")?,
    };

    let history_table = optional_table(&root, "history", "history")?;
    check_unknown(history_table, "history", &["persist_tasks", "max_tasks"])?;
    let history = FileHistory {
        persist_tasks: optional_bool(history_table, "persist_tasks", "history.persist_tasks")?,
        max_tasks: optional_integer_range(
            history_table,
            "max_tasks",
            "history.max_tasks",
            20,
            5000,
        )?,
    };

    let profiles_table = optional_table(&root, "profiles", "profiles")?;
    let mut profiles = BTreeMap::new();
    for (name, profile_value) in profiles_table {
        if !valid_profile_name(name) {
            return Err(ConfigError::InvalidField {
                field: format!("profiles.{name}"),
                expected: "an ASCII profile name containing letters, digits, '_' or '-'",
            });
        }
        let profile = profile_value.as_table().ok_or(ConfigError::InvalidField {
            field: format!("profiles.{name}"),
            expected: "a TOML table",
        })?;
        let prefix = format!("profiles.{name}");
        check_unknown(
            profile,
            &prefix,
            &[
                "tailnet",
                "read_only",
                "credential",
                "credential_backend",
                "credential_file",
            ],
        )?;
        let tailnet = required_string(profile, "tailnet", &format!("{prefix}.tailnet"))?;
        let credential = required_string(profile, "credential", &format!("{prefix}.credential"))?;
        let read_only = optional_bool(profile, "read_only", &format!("{prefix}.read_only"))?
            .is_none_or(|value| value);
        let backend_name = required_string(
            profile,
            "credential_backend",
            &format!("{prefix}.credential_backend"),
        )?;
        let credential_backend = match backend_name.as_str() {
            "file" => {
                let path = required_string(
                    profile,
                    "credential_file",
                    &format!("{prefix}.credential_file"),
                )?;
                CredentialBackend::File {
                    path: PathBuf::from(path),
                }
            }
            _ => {
                return Err(ConfigError::InvalidField {
                    field: format!("{prefix}.credential_backend"),
                    expected: "a supported credential backend: file",
                });
            }
        };
        profiles.insert(
            name.clone(),
            ProfileConfig {
                tailnet,
                read_only,
                credential,
                credential_backend,
            },
        );
    }
    Ok(FileConfig {
        read_only,
        local,
        admin,
        ui,
        history,
        profiles,
    })
}

type Table<'a> = toml::map::Map<String, toml::Value>;

fn check_unknown(table: &Table<'_>, prefix: &str, allowed: &[&str]) -> Result<(), ConfigError> {
    for key in table.keys() {
        if !allowed.iter().any(|candidate| candidate == key) {
            let full = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            return Err(ConfigError::UnknownField(full));
        }
    }
    Ok(())
}

fn optional_table<'a>(
    table: &'a Table<'_>,
    key: &str,
    field: &str,
) -> Result<&'a Table<'a>, ConfigError> {
    match table.get(key) {
        None => Ok(empty_table()),
        Some(value) => value.as_table().ok_or(ConfigError::InvalidField {
            field: field.to_owned(),
            expected: "a TOML table",
        }),
    }
}

fn empty_table() -> &'static Table<'static> {
    static EMPTY: std::sync::OnceLock<Table<'static>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Table::new)
}

fn optional_string(
    table: &Table<'_>,
    key: &str,
    field: &str,
) -> Result<Option<String>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::to_owned)
            .ok_or(ConfigError::InvalidField {
                field: field.to_owned(),
                expected: "a string",
            })
            .map(Some),
    }
}

fn optional_path(
    table: &Table<'_>,
    key: &str,
    field: &str,
) -> Result<Option<PathBuf>, ConfigError> {
    optional_string(table, key, field).map(|value| value.map(PathBuf::from))
}

fn required_string(table: &Table<'_>, key: &str, field: &str) -> Result<String, ConfigError> {
    optional_string(table, key, field)?.ok_or(ConfigError::MissingField {
        field: field.to_owned(),
    })
}

fn optional_bool(table: &Table<'_>, key: &str, field: &str) -> Result<Option<bool>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_bool()
            .ok_or(ConfigError::InvalidField {
                field: field.to_owned(),
                expected: "a boolean",
            })
            .map(Some),
    }
}

fn optional_integer_range(
    table: &Table<'_>,
    key: &str,
    field: &str,
    min: i64,
    max: i64,
) -> Result<Option<usize>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => {
            let integer = value.as_integer().ok_or(ConfigError::InvalidField {
                field: field.to_owned(),
                expected: "an integer",
            })?;
            if !(min..=max).contains(&integer) {
                return Err(ConfigError::OutOfRange {
                    field: field.to_owned(),
                    allowed: "20..=5000",
                });
            }
            usize::try_from(integer)
                .map(Some)
                .map_err(|_| ConfigError::OutOfRange {
                    field: field.to_owned(),
                    allowed: "20..=5000",
                })
        }
    }
}

fn optional_duration(
    table: &Table<'_>,
    key: &str,
    field: &str,
    min: Duration,
    max: Duration,
) -> Result<Option<Duration>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or(ConfigError::InvalidField {
                field: field.to_owned(),
                expected: "a duration such as 2s",
            })?;
            let duration = parse_duration(text).ok_or(ConfigError::InvalidField {
                field: field.to_owned(),
                expected: "a duration such as 2s",
            })?;
            if duration < min || duration > max {
                return Err(ConfigError::OutOfRange {
                    field: field.to_owned(),
                    allowed: "the documented duration range",
                });
            }
            Ok(Some(duration))
        }
    }
}

fn optional_enum<T>(
    table: &Table<'_>,
    key: &str,
    field: &str,
    parser: fn(&str) -> Option<T>,
) -> Result<Option<T>, ConfigError> {
    match table.get(key) {
        None => Ok(None),
        Some(value) => {
            let text = value.as_str().ok_or(ConfigError::InvalidField {
                field: field.to_owned(),
                expected: "one of the documented values",
            })?;
            parser(text)
                .ok_or(ConfigError::InvalidField {
                    field: field.to_owned(),
                    expected: "one of the documented values",
                })
                .map(Some)
        }
    }
}

fn parse_duration(value: &str) -> Option<Duration> {
    let (number, unit) = value.split_at(value.find(|character: char| !character.is_ascii_digit())?);
    if number.is_empty() || unit.is_empty() {
        return None;
    }
    let amount = number.parse::<u64>().ok()?;
    let multiplier = match unit {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        _ => return None,
    };
    Duration::from_millis(amount.checked_mul(multiplier)?).checked_add(Duration::from_nanos(0))
}

fn parse_color(value: &str) -> Option<ColorMode> {
    match value {
        "auto" => Some(ColorMode::Auto),
        "none" => Some(ColorMode::None),
        "ansi16" => Some(ColorMode::Ansi16),
        "ansi256" => Some(ColorMode::Ansi256),
        "truecolor" => Some(ColorMode::TrueColor),
        _ => None,
    }
}

fn parse_symbols(value: &str) -> Option<SymbolsMode> {
    match value {
        "auto" => Some(SymbolsMode::Auto),
        "ascii" => Some(SymbolsMode::Ascii),
        "unicode" => Some(SymbolsMode::Unicode),
        _ => None,
    }
}

fn parse_detail_pane(value: &str) -> Option<DetailPaneMode> {
    match value {
        "auto" => Some(DetailPaneMode::Auto),
        "always" => Some(DetailPaneMode::Always),
        "never" => Some(DetailPaneMode::Never),
        _ => None,
    }
}

fn parse_time_zone(value: &str) -> Option<TimeZoneMode> {
    match value {
        "local" => Some(TimeZoneMode::Local),
        "utc" => Some(TimeZoneMode::Utc),
        _ => None,
    }
}

fn valid_profile_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

impl ResolvedConfig {
    pub fn settings(&self) -> Vec<SettingDisplay> {
        vec![
            SettingDisplay {
                name: "paths.config",
                value: self.paths.config_file.display().to_string(),
                source: ValueSource::Default,
            },
            SettingDisplay {
                name: "paths.state",
                value: self.paths.state_dir.display().to_string(),
                source: ValueSource::Default,
            },
            SettingDisplay {
                name: "paths.cache",
                value: self.paths.cache_dir.display().to_string(),
                source: ValueSource::Default,
            },
            SettingDisplay {
                name: "source",
                value: if self.mock {
                    "mock"
                } else if self.no_local || !self.local.enabled {
                    "local disabled"
                } else {
                    "local"
                }
                .to_owned(),
                source: ValueSource::Cli,
            },
            SettingDisplay {
                name: "read_only",
                value: self.read_only.to_string(),
                source: self.read_only_source,
            },
            SettingDisplay {
                name: "local.enabled",
                value: self.local.enabled.to_string(),
                source: self.local.enabled_source,
            },
            SettingDisplay {
                name: "local.tailscale_path",
                value: self.local.tailscale_path.clone(),
                source: self.local.tailscale_path_source,
            },
            SettingDisplay {
                name: "local.socket_path",
                value: self.local.socket_path.display().to_string(),
                source: self.local.socket_path_source,
            },
            SettingDisplay {
                name: "local.reconcile_interval",
                value: format_duration(self.local.reconcile_interval),
                source: self.local.reconcile_interval_source,
            },
            SettingDisplay {
                name: "local.command_timeout",
                value: format_duration(self.local.command_timeout),
                source: self.local.command_timeout_source,
            },
            SettingDisplay {
                name: "admin.refresh_interval",
                value: format_duration(self.admin.refresh_interval),
                source: self.admin.refresh_interval_source,
            },
            SettingDisplay {
                name: "admin.request_timeout",
                value: format_duration(self.admin.request_timeout),
                source: self.admin.request_timeout_source,
            },
            SettingDisplay {
                name: "ui.theme",
                value: self.ui.theme.as_str().to_owned(),
                source: self.ui.theme_source,
            },
            SettingDisplay {
                name: "ui.color",
                value: self.ui.color.as_str().to_owned(),
                source: self.ui.color_source,
            },
            SettingDisplay {
                name: "ui.symbols",
                value: self.ui.symbols.as_str().to_owned(),
                source: self.ui.symbols_source,
            },
            SettingDisplay {
                name: "ui.mouse",
                value: format!("{} (opt-in)", self.ui.mouse),
                source: self.ui.mouse_source,
            },
            SettingDisplay {
                name: "ui.detail_pane",
                value: self.ui.detail_pane.as_str().to_owned(),
                source: self.ui.detail_pane_source,
            },
            SettingDisplay {
                name: "ui.time_zone",
                value: self.ui.time_zone.as_str().to_owned(),
                source: self.ui.time_zone_source,
            },
            SettingDisplay {
                name: "ui.relative_times",
                value: self.ui.relative_times.to_string(),
                source: self.ui.relative_times_source,
            },
            SettingDisplay {
                name: "ui.show_footer",
                value: self.ui.show_footer.to_string(),
                source: self.ui.show_footer_source,
            },
            SettingDisplay {
                name: "history.persist_tasks",
                value: self.history.persist_tasks.to_string(),
                source: self.history.persist_tasks_source,
            },
            SettingDisplay {
                name: "history.max_tasks",
                value: self.history.max_tasks.to_string(),
                source: self.history.max_tasks_source,
            },
        ]
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis().is_multiple_of(1000) {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}
