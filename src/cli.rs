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

    /// Select a configured tailnet profile for this session.
    #[arg(long, global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Read configuration from PATH instead of the default config location.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Open ROUTE when the terminal interface starts.
    #[arg(long, global = true, value_name = "ROUTE")]
    pub view: Option<String>,

    /// Disable every mutation for this session.
    #[arg(long, global = true)]
    pub read_only: bool,

    /// Do not connect to the local Tailscale client or daemon.
    #[arg(long, global = true)]
    pub no_local: bool,

    /// Use PATH as the local Tailscale executable.
    #[arg(long, global = true, value_name = "PATH")]
    pub tailscale_path: Option<PathBuf>,

    /// Connect to the local Tailscale daemon at PATH.
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
    /// Add, inspect, or remove tailnet credentials.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Inspect and validate Tale configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print a redacted, non-mutating diagnostic report.
    Doctor(DoctorArgs),
}

#[derive(Debug, Clone, Copy, Args)]
pub struct GenCompletionsArgs {
    /// Shell to generate completions for: bash, zsh, or fish.
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
    /// Create or update a credential profile.
    Add(AuthAddArgs),
    /// Delete a credential profile and its stored secret.
    Remove(AuthProfileArgs),
    /// Show the selected profile's credential status.
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
    /// Name for the profile. Use letters, digits, '_' and '-'.
    pub profile: String,

    /// Tailnet ID or '-'; prompts when omitted.
    #[arg(long, value_name = "ID")]
    pub tailnet: Option<String>,

    /// Credential type: oauth-client or access-token; prompts when omitted.
    #[arg(long, value_name = "KIND")]
    pub kind: Option<CredentialKindArg>,

    /// Read the secret from standard input instead of prompting. Selects the access
    /// token, or the client secret when the kind is `oauth_client`.
    #[arg(long)]
    pub secret_stdin: bool,

    /// OAuth client ID; required with --secret-stdin for oauth-client credentials.
    #[arg(long, value_name = "ID")]
    pub client_id: Option<String>,

    /// Comma-separated OAuth scopes; prompts when omitted for oauth-client credentials.
    #[arg(long, value_name = "SCOPES")]
    pub scopes: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct AuthProfileArgs {
    /// Name of the profile.
    pub profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct AuthStatusArgs {
    /// Profile to inspect; defaults to --profile.
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Print configuration, credential, state, and cache locations.
    Path,
    /// Validate the configuration without opening the terminal interface.
    Check,
    /// Every resolved value and what decided it.
    Show,
}

#[derive(Debug, Clone, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    #[arg(hide = true)]
    pub mock: bool,

    /// Write the report to PATH instead of standard output.
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
}
