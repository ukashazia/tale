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

    #[arg(long, global = true, value_name = "PATH")]
    pub tailscale_socket: Option<PathBuf>,

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
    Add(AuthAddArgs),
    Remove(AuthProfileArgs),
    Status(AuthStatusArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum CredentialKindArg {
    OauthClient,
    AccessToken,
}

impl CredentialKindArg {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OauthClient => "oauth_client",
            Self::AccessToken => "access_token",
        }
    }
}

/// `auth add` is the only writer to the credential store, so it has to be usable
/// without a terminal: the prompts cannot be reached from a script, a container, or
/// a CI job, and they are the sole recovery path once a profile has been removed.
#[derive(Debug, Clone, Args)]
pub struct AuthAddArgs {
    pub profile: String,

    #[arg(long, value_name = "ID")]
    pub tailnet: Option<String>,

    #[arg(long, value_name = "KIND")]
    pub kind: Option<CredentialKindArg>,

    /// Read the secret from standard input instead of prompting. Selects the access
    /// token, or the client secret when the kind is `oauth_client`.
    #[arg(long)]
    pub secret_stdin: bool,

    #[arg(long, value_name = "ID")]
    pub client_id: Option<String>,

    #[arg(long, value_name = "SCOPES")]
    pub scopes: Option<String>,
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
    /// Every resolved value and what decided it.
    Show,
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    #[arg(hide = true)]
    pub mock: bool,

    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}
