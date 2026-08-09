use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::generate;
use clap_complete::shells::{Bash, Fish, Zsh};

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
    /// Print shell completion instructions to standard output.
    GenCompletions(GenCompletionsArgs),
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

#[derive(Debug, Clone, Copy, Args)]
pub struct GenCompletionsArgs {
    #[arg(long, value_parser = parse_completion_shell)]
    pub shell: CompletionShell,
}

#[derive(Debug, Clone, Copy)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

fn parse_completion_shell(value: &str) -> Result<CompletionShell, String> {
    match Path::new(value).file_name().and_then(|name| name.to_str()) {
        Some("bash") => Ok(CompletionShell::Bash),
        Some("zsh") => Ok(CompletionShell::Zsh),
        Some("fish") => Ok(CompletionShell::Fish),
        _ => Err("shell must be bash, zsh, or fish".to_owned()),
    }
}

pub fn completion(shell: CompletionShell) -> Result<String, String> {
    let mut command = Cli::command();
    let mut generated = Vec::new();
    match shell {
        CompletionShell::Bash => generate(Bash, &mut command, "tale", &mut generated),
        CompletionShell::Zsh => generate(Zsh, &mut command, "tale", &mut generated),
        CompletionShell::Fish => generate(Fish, &mut command, "tale", &mut generated),
    }
    let generated = String::from_utf8(generated)
        .map_err(|_| "generated completion was not UTF-8".to_owned())?;
    Ok(sanitize_completion(&generated))
}

fn sanitize_completion(generated: &str) -> String {
    generated
        .lines()
        .filter_map(|line| {
            if line.contains("-l mock") || line.contains("'--mock[]'") {
                None
            } else {
                Some(line.replace(" --mock", "").replace(" mock ", " "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
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
