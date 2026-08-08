pub mod action;
pub mod admin;
pub mod app;
pub mod cli;
pub mod clipboard;
pub mod config;
pub mod doctor;
pub mod domain;
pub mod effect;
pub mod error;
pub mod event;
pub mod export;
pub mod health;
pub mod local;
pub mod mock;
pub mod paths;
pub mod runtime;
pub mod saved_views;
pub mod secrets;
pub mod task;
pub mod temporary;
pub mod terminal;
pub mod ui;

pub const VERSION_USER_AGENT: &str = concat!("tale/", env!("CARGO_PKG_VERSION"));

use std::io;
use std::io::Write;

use clap::CommandFactory;

use crate::admin::auth::{
    AccessTokenRecord, CredentialBackend, CredentialRecord, OAuthClientRecord, SecretValue,
};
use crate::cli::{AuthAddArgs, AuthCommand, Cli, Command, ConfigCommand};
use crate::config::{EnvironmentValues, ResolvedConfig};
use crate::error::TaleError;
use crate::paths::PathEnvironment;

fn config_error(error: config::ConfigError) -> TaleError {
    match error {
        config::ConfigError::ReadFailure => {
            TaleError::ConfigurationIo("configuration file could not be read".to_owned())
        }
        other => TaleError::InvalidConfiguration(other.to_string()),
    }
}

pub fn run(mut cli: Cli) -> Result<(), TaleError> {
    let environment = EnvironmentValues::from_process();
    let path_environment = PathEnvironment::from_process()
        .map_err(|error| TaleError::InvalidConfiguration(error.to_string()))?;

    if let Some(view) = cli.view.as_deref()
        && crate::app::Route::parse(view).is_none()
    {
        return Err(TaleError::InvalidArguments(format!(
            "unknown Phase-1 route: {view}"
        )));
    }

    match cli.command.clone() {
        Some(Command::Auth { command }) => run_auth(command, &cli, &environment, &path_environment),
        Some(Command::Config {
            command: ConfigCommand::Path,
        }) => {
            let paths = config::resolve_paths_for_cli(&cli, &environment, &path_environment)
                .map_err(|error| TaleError::InvalidConfiguration(error.to_string()))?;
            println!("config       {}", paths.config_file.display());
            println!("credentials  {}", paths.credentials_file.display());
            println!("state        {}", paths.state_dir.display());
            println!("cache        {}", paths.cache_dir.display());
            Ok(())
        }
        Some(Command::Config {
            command: ConfigCommand::Check,
        }) => {
            let config =
                config::resolve(&cli, &environment, &path_environment).map_err(config_error)?;
            println!(
                "configuration valid: {}",
                config.paths.config_file.display()
            );
            Ok(())
        }
        Some(Command::Config {
            command: ConfigCommand::Show,
        }) => {
            let config =
                config::resolve(&cli, &environment, &path_environment).map_err(config_error)?;
            let settings = config.settings();
            let width = settings
                .iter()
                .map(|setting| setting.name.chars().count())
                .max()
                .unwrap_or(0);
            for setting in settings {
                println!(
                    "{:<width$}  {}  [{}]",
                    setting.name,
                    setting.value,
                    setting.source.label()
                );
            }
            Ok(())
        }
        Some(Command::Doctor(args)) => {
            cli.mock = args.mock;
            let config =
                config::resolve(&cli, &environment, &path_environment).map_err(config_error)?;
            run_doctor(&config, args.output.as_deref())?;
            Ok(())
        }
        None => {
            let config =
                config::resolve(&cli, &environment, &path_environment).map_err(config_error)?;
            launch_tui(config, cli.view.as_deref())
        }
    }
}

fn run_auth(
    command: AuthCommand,
    cli: &Cli,
    environment: &EnvironmentValues,
    path_environment: &PathEnvironment,
) -> Result<(), TaleError> {
    if cli.mock {
        return Err(TaleError::InvalidArguments(
            "auth commands cannot use --mock".to_owned(),
        ));
    }
    let mut check_cli = cli.clone();
    check_cli.command = Some(Command::Config {
        command: ConfigCommand::Check,
    });
    check_cli.profile = match &command {
        AuthCommand::Add(_) => None,
        AuthCommand::Remove(args) => Some(args.profile.clone()),
        AuthCommand::Status(args) => args.profile.clone(),
    };
    check_cli.mock = false;
    let checked =
        config::resolve(&check_cli, environment, path_environment).map_err(config_error)?;
    match command {
        AuthCommand::Add(args) => auth_add(args, &checked),
        AuthCommand::Status(args) => auth_status(args.profile, &checked),
        AuthCommand::Remove(args) => auth_remove(args.profile, &checked),
    }
}

fn auth_add(args: AuthAddArgs, checked: &ResolvedConfig) -> Result<(), TaleError> {
    let profile_name = args.profile;
    if !config::is_valid_profile_name(&profile_name) {
        return Err(TaleError::InvalidArguments(
            "profile name contains unsupported characters".to_owned(),
        ));
    }
    // Reading the secret from standard input means no prompt can be answered, so every
    // remaining value has to arrive as a flag rather than silently blocking on a tty.
    let scripted = args.secret_stdin;
    let existing = checked.profiles.get(&profile_name).cloned();
    let tailnet = match args.tailnet {
        Some(tailnet) => tailnet,
        None => match existing.as_ref() {
            Some(profile) => profile.tailnet.clone(),
            None if scripted => {
                return Err(TaleError::InvalidArguments(
                    "--tailnet is required when the secret is read from standard input".to_owned(),
                ));
            }
            None => prompt_line("tailnet ID (or -): ")?,
        },
    };
    if tailnet.is_empty() {
        return Err(TaleError::InvalidArguments(
            "tailnet ID cannot be empty".to_owned(),
        ));
    }
    let kind = match args.kind {
        Some(kind) => kind.label().to_owned(),
        None if scripted => {
            return Err(TaleError::InvalidArguments(
                "--kind is required when the secret is read from standard input".to_owned(),
            ));
        }
        None => prompt_line("credential kind [oauth_client/access_token]: ")?,
    };
    let record = match kind.trim() {
        "oauth_client" => {
            let client_id = match args.client_id {
                Some(client_id) => client_id,
                None if scripted => {
                    return Err(TaleError::InvalidArguments(
                        "--client-id is required for oauth_client when the secret is read \
                         from standard input"
                            .to_owned(),
                    ));
                }
                None => prompt_secret("OAuth client ID: ")?,
            };
            let client_secret = read_secret("OAuth client secret: ", scripted)?;
            let scopes = match args.scopes {
                Some(scopes) => scopes,
                None if scripted => {
                    return Err(TaleError::InvalidArguments(
                        "--scopes is required for oauth_client when the secret is read \
                         from standard input"
                            .to_owned(),
                    ));
                }
                None => prompt_line("requested scopes (space separated): ")?,
            }
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
            if client_id.is_empty() || client_secret.is_empty() || scopes.is_empty() {
                return Err(TaleError::InvalidArguments(
                    "OAuth client ID, secret, and at least one scope are required".to_owned(),
                ));
            }
            crate::admin::auth::validate_requested_scopes(&scopes).map_err(|error| {
                TaleError::InvalidArguments(format!("invalid OAuth scopes: {error}"))
            })?;
            CredentialRecord::OAuthClient(OAuthClientRecord {
                version: 1,
                client_id: SecretValue::new(client_id),
                client_secret: SecretValue::new(client_secret),
                requested_scopes: scopes,
            })
        }
        "access_token" => {
            let access_token = read_secret("access token: ", scripted)?;
            if access_token.is_empty() {
                return Err(TaleError::InvalidArguments(
                    "access token cannot be empty".to_owned(),
                ));
            }
            CredentialRecord::AccessToken(AccessTokenRecord {
                version: 1,
                access_token: SecretValue::new(access_token),
            })
        }
        _ => {
            return Err(TaleError::InvalidArguments(
                "credential kind must be oauth_client or access_token".to_owned(),
            ));
        }
    };
    let tailnet_profile = config::ProfileConfig {
        tailnet,
        read_only: existing.as_ref().is_none_or(|profile| profile.read_only),
        credential: existing.as_ref().map_or_else(
            || profile_name.clone(),
            |profile| profile.credential.clone(),
        ),
        // An existing profile keeps whichever backend it already names, so re-running
        // `auth add` to rotate a secret never silently relocates it.
        credential_backend: existing.as_ref().map_or_else(
            || CredentialBackend::File {
                path: checked.paths.credentials_file.clone(),
            },
            |profile| profile.credential_backend.clone(),
        ),
    };
    let runtime = auth_runtime()?;
    runtime
        .block_on(crate::admin::auth::validate_record(
            &profile_name,
            &tailnet_profile.tailnet,
            &record,
        ))
        .map_err(|error| {
            TaleError::Application(format!("credential validation failed: {error}"))
        })?;
    let store = tailnet_profile.credential_backend.open();
    let previous = store.get(&tailnet_profile.credential).map_err(|error| {
        TaleError::Application(format!("credential could not be read: {error}"))
    })?;
    store
        .set(&tailnet_profile.credential, &record)
        .map_err(|error| {
            TaleError::Application(format!("credential could not be stored: {error}"))
        })?;
    if let Err(error) =
        config::write_profile_atomic(&checked.paths.config_file, &profile_name, &tailnet_profile)
    {
        let rollback = match previous {
            Some(previous) => store.set(&tailnet_profile.credential, &previous),
            None => store.delete(&tailnet_profile.credential).map(|_| ()),
        };
        if rollback.is_err() {
            return Err(TaleError::Application(
                "profile write failed and credential rollback failed".to_owned(),
            ));
        }
        return Err(TaleError::InvalidConfiguration(error.to_string()));
    }
    println!("stored {kind} credential for profile {profile_name}");
    Ok(())
}

fn auth_status(
    requested_profile: Option<String>,
    checked: &ResolvedConfig,
) -> Result<(), TaleError> {
    let profile_name = requested_profile
        .or_else(|| checked.profile.clone())
        .ok_or_else(|| {
            TaleError::InvalidArguments("no profile is selected; pass --profile".to_owned())
        })?;
    let profile = checked
        .profiles
        .get(&profile_name)
        .ok_or_else(|| TaleError::InvalidArguments("profile does not exist".to_owned()))?;
    let store = profile.credential_backend.open();
    let status = crate::admin::auth::TokenManager::new(store.clone())
        .credential_status(&profile.credential)
        .map_err(|error| TaleError::Application(format!("credential status failed: {error}")))?;
    println!("profile: {profile_name}");
    println!("tailnet: {}", profile.tailnet);
    println!(
        "backend: {} ({})",
        profile.credential_backend.label(),
        profile.credential_backend.location().display()
    );
    match status {
        None => println!("credential: missing"),
        Some(status) => {
            println!("credential: {}", status.kind.label());
            println!(
                "scopes: {}",
                if status.requested_scopes.is_empty() {
                    "not recorded".to_owned()
                } else {
                    status.requested_scopes.join(" ")
                }
            );
            let runtime = auth_runtime()?;
            let result = runtime.block_on(crate::admin::auth::live_check(
                &profile_name,
                &profile.tailnet,
                &profile.credential,
                store,
                checked.admin.request_timeout,
            ));
            println!(
                "live authentication: {}",
                result.map_or_else(|error| format!("failed ({error})"), |_| "ok".to_owned())
            );
        }
    }
    Ok(())
}

fn auth_remove(profile_name: String, checked: &ResolvedConfig) -> Result<(), TaleError> {
    let profile = checked
        .profiles
        .get(&profile_name)
        .ok_or_else(|| TaleError::InvalidArguments("profile does not exist".to_owned()))?;
    let store = profile.credential_backend.open();
    let removed = store
        .delete(&profile.credential)
        .map_err(|error| TaleError::Application(format!("credential removal failed: {error}")))?;
    if removed {
        println!("removed stored credential {}", profile.credential);
    } else {
        println!("stored credential {} was not present", profile.credential);
    }
    let answer = prompt_line("also remove the profile configuration? [y/N]: ")?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        let paths = &checked.paths;
        let removed = config::remove_profile_atomic(&paths.config_file, &profile_name)
            .map_err(|error| TaleError::InvalidConfiguration(error.to_string()))?;
        if removed {
            println!("removed profile configuration {profile_name}");
        } else {
            println!("profile configuration {profile_name} was not present");
        }
    } else {
        println!("kept profile configuration {profile_name}");
    }
    println!("remote credential was not revoked");
    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String, TaleError> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|_| TaleError::Application("could not write prompt".to_owned()))?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|_| TaleError::Application("could not read prompt".to_owned()))?;
    Ok(value.trim().to_owned())
}

fn prompt_secret(prompt: &str) -> Result<String, TaleError> {
    rpassword::prompt_password(prompt)
        .map_err(|_| TaleError::Application("credential input was cancelled".to_owned()))
}

/// `rpassword` opens `/dev/tty` directly, so prompting fails outright wherever there is
/// no controlling terminal. Reading standard input covers those callers.
fn read_secret(prompt: &str, from_stdin: bool) -> Result<String, TaleError> {
    if !from_stdin {
        return prompt_secret(prompt);
    }
    let mut value = String::new();
    io::Read::read_to_string(&mut io::stdin(), &mut value)
        .map_err(|_| TaleError::Application("could not read the secret from stdin".to_owned()))?;
    Ok(value.trim().to_owned())
}

fn auth_runtime() -> Result<tokio::runtime::Runtime, TaleError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| TaleError::RuntimeInitialization(error.to_string()))
}

fn run_doctor(config: &ResolvedConfig, output: Option<&std::path::Path>) -> Result<(), TaleError> {
    let bundle = crate::doctor::build(config);
    match output {
        Some(path) => {
            crate::doctor::write_new_atomic(&bundle, path)
                .map_err(|error| TaleError::Application(error.to_string()))?;
            println!("wrote redacted support bundle: {}", path.display());
        }
        None => {
            let report = crate::doctor::text_report(&bundle)
                .map_err(|error| TaleError::Application(error.to_string()))?;
            println!("{report}");
        }
    }
    Ok(())
}

fn launch_tui(config: ResolvedConfig, view: Option<&str>) -> Result<(), TaleError> {
    let mouse = config.ui.mouse;
    let mut app = app::App::new(config);
    if let Some(view) = view.and_then(app::Route::parse) {
        app.set_route(view);
    }
    let mut terminal = terminal::RealTerminal::enter_with_mouse(mouse)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| TaleError::RuntimeInitialization(error.to_string()))?;
    let result = runtime.block_on(runtime::run(&mut app, &mut terminal));
    // Dropping the runtime joins its worker threads, which would reintroduce the very
    // stall the bounded shutdown above avoids if one is parked in a blocking call.
    runtime.shutdown_timeout(crate::task::grace_duration());
    let _ = std::io::stdout().flush();
    result
}

pub fn command_help() -> String {
    let mut command = Cli::command();
    command.render_help().to_string()
}
