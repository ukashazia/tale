use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Command, CommandFactory};
use clap_mangen::Man;

use tale::cli::{self, Cli, CompletionShell};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_dir()?;
    let completions_dir = output_dir.join("completions");
    let man_dir = output_dir.join("docs/cli");
    fs::create_dir_all(&completions_dir)?;
    fs::create_dir_all(&man_dir)?;

    write_completion(&completions_dir.join("tale.bash"), CompletionShell::Bash)?;
    write_completion(&completions_dir.join("_tale"), CompletionShell::Zsh)?;
    write_completion(&completions_dir.join("tale.fish"), CompletionShell::Fish)?;

    let man_path = man_dir.join("tale.1");
    let mut man_file = File::create(man_path)?;
    Man::new(artifact_command()).render(&mut man_file)?;
    Ok(())
}

fn output_dir() -> Result<PathBuf, io::Error> {
    let mut arguments = env::args_os().skip(1);
    match (arguments.next(), arguments.next(), arguments.next()) {
        (Some(flag), Some(path), None) if flag == "--output-dir" => Ok(PathBuf::from(path)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: generate-artifacts --output-dir PATH",
        )),
    }
}

fn write_completion(path: &Path, shell: CompletionShell) -> Result<(), io::Error> {
    let generated = cli::completion(shell)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut file = File::create(path)?;
    file.write_all(generated.as_bytes())?;
    Ok(())
}

fn artifact_command() -> Command {
    Cli::command()
}
