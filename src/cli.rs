use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "tale",
    version,
    about = "A terminal-native Tailscale workspace",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, value_name = "ROUTE")]
    pub view: Option<String>,

    #[arg(long, global = true)]
    pub read_only: bool,

    #[arg(long, global = true)]
    pub no_local: bool,

    #[arg(long, global = true, value_name = "PATH")]
    pub tailscale_path: Option<PathBuf>,

    #[arg(long)]
    #[arg(hide = true)]
    pub mock: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Doctor(DoctorArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum AuthCommand {
    Add(AuthProfileArgs),
    Remove(AuthProfileArgs),
    Status(AuthStatusArgs),
}

#[derive(Debug, Clone, Args)]
pub struct AuthProfileArgs {
    pub profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct AuthStatusArgs {
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    Path,
    Check,
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    #[arg(hide = true)]
    pub mock: bool,

    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}
