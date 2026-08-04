use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Command, CommandFactory};
use clap_complete::generate;
use clap_complete::shells::{Bash, Fish, Zsh};
use clap_mangen::Man;

use tale::cli::Cli;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_dir()?;
    let completions_dir = output_dir.join("completions");
    let man_dir = output_dir.join("docs/cli");
    fs::create_dir_all(&completions_dir)?;
    fs::create_dir_all(&man_dir)?;

    write_completion(&completions_dir.join("tale.bash"), Bash)?;
    write_completion(&completions_dir.join("_tale"), Zsh)?;
    write_completion(&completions_dir.join("tale.fish"), Fish)?;

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

fn write_completion<S>(path: &Path, shell: S) -> Result<(), io::Error>
where
    S: clap_complete::Generator,
{
    let mut command = Cli::command();
    let mut generated = Vec::new();
    generate(shell, &mut command, "tale", &mut generated);
    let generated = String::from_utf8(generated)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "completion was not UTF-8"))?;
    let mut file = File::create(path)?;
    file.write_all(sanitize_completion(&generated).as_bytes())?;
    Ok(())
}

fn artifact_command() -> Command {
    Cli::command()
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
