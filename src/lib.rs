pub mod action;
pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
pub mod effect;
pub mod error;
pub mod event;
pub mod mock;
pub mod paths;
pub mod runtime;
pub mod task;
pub mod terminal;
pub mod ui;

use std::io::Write;

use clap::CommandFactory;

use crate::cli::{Cli, Command, ConfigCommand};
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
        Some(Command::Config {
            command: ConfigCommand::Path,
        }) => {
            let paths = config::resolve_paths_for_cli(&cli, &environment, &path_environment)
                .map_err(|error| TaleError::InvalidConfiguration(error.to_string()))?;
            println!("config  {}", paths.config_file.display());
            println!("state   {}", paths.state_dir.display());
            println!("cache   {}", paths.cache_dir.display());
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
        Some(Command::Doctor(args)) => {
            cli.mock = args.mock;
            let config =
                config::resolve(&cli, &environment, &path_environment).map_err(config_error)?;
            doctor(&config);
            Ok(())
        }
        None => {
            let config =
                config::resolve(&cli, &environment, &path_environment).map_err(config_error)?;
            launch_tui(config, cli.view.as_deref())
        }
    }
}

fn doctor(config: &ResolvedConfig) {
    println!("Tale doctor (Phase 1)");
    println!(
        "source: {}",
        if config.mock {
            "mock"
        } else {
            "local unavailable"
        }
    );
    println!("local process adapter: not constructed");
    println!("HTTP adapter: not constructed");
    println!("keyring adapter: not constructed");
    println!("config: {}", config.paths.config_file.display());
    println!("terminal: lifecycle owned by TerminalSession");
    if config.ui.mouse {
        println!("ui.mouse: unsupported in Phase 1");
    }
}

fn launch_tui(config: ResolvedConfig, view: Option<&str>) -> Result<(), TaleError> {
    let mut app = app::App::new(config);
    if let Some(view) = view.and_then(app::Route::parse) {
        app.route_stack = vec![view];
    }
    let mut terminal = terminal::RealTerminal::enter()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| TaleError::RuntimeInitialization(error.to_string()))?;
    let result = runtime.block_on(runtime::run(&mut app, &mut terminal));
    let _ = std::io::stdout().flush();
    result
}

pub fn command_help() -> String {
    let mut command = Cli::command();
    command.render_help().to_string()
}
