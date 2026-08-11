use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const BLOCK_SIZE: usize = 512;

struct Options {
    target: String,
    binary: PathBuf,
    output: PathBuf,
    checksum: PathBuf,
    source_date_epoch: u64,
}

struct Entry {
    archive_name: String,
    source: PathBuf,
    mode: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let entries = entries(&options, root);
    validate_new_path(&options.output)?;
    validate_new_path(&options.checksum)?;
    validate_entries(&entries)?;

    let archive_result = write_archive(&options, &entries);
    if archive_result.is_err() {
        let _ = fs::remove_file(&options.output);
    }
    archive_result?;

    let checksum_result = write_checksum(&options);
    if checksum_result.is_err() {
        let _ = fs::remove_file(&options.output);
        let _ = fs::remove_file(&options.checksum);
    }
    checksum_result?;
    Ok(())
}

fn parse_options() -> Result<Options, io::Error> {
    let mut arguments = env::args_os().skip(1);
    let mut target = None;
    let mut binary = None;
    let mut output = None;
    let mut checksum = None;
    let mut source_date_epoch = None;
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(usage_error)?;
        match flag.to_str() {
            Some("--target") => target = value.to_str().map(str::to_owned),
            Some("--binary") => binary = Some(PathBuf::from(value)),
            Some("--output") => output = Some(PathBuf::from(value)),
            Some("--checksum") => checksum = Some(PathBuf::from(value)),
            Some("--source-date-epoch") => {
                source_date_epoch = value.to_str().and_then(|value| value.parse().ok())
            }
            _ => return Err(usage_error()),
        }
    }
    match (target, binary, output, checksum, source_date_epoch) {
        (Some(target), Some(binary), Some(output), Some(checksum), Some(source_date_epoch))
            if !target.is_empty() =>
        {
            Ok(Options {
                target,
                binary,
                output,
                checksum,
                source_date_epoch,
            })
        }
        _ => Err(usage_error()),
    }
}

fn usage_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: package-artifact --target TARGET --binary PATH --output PATH --checksum PATH --source-date-epoch SECONDS",
    )
}

fn entries(options: &Options, root: &Path) -> Vec<Entry> {
    let prefix = format!("tale-{}", options.target);
    let mut entries = vec![Entry {
        archive_name: format!("{prefix}/tale"),
        source: options.binary.clone(),
        mode: 0o755,
    }];
    for relative in [
        "LICENSE",
        "NOTICE",
        "README.md",
        "docs/cli/tale.1",
        "completions/tale.bash",
        "completions/_tale",
        "completions/tale.fish",
    ] {
        entries.push(Entry {
            archive_name: format!("{prefix}/{relative}"),
            source: root.join(relative),
            mode: 0o644,
        });
    }
    entries
}

fn validate_new_path(path: &Path) -> Result<(), io::Error> {
    let parent = path.parent().map_or_else(|| Path::new("."), |value| value);
    if !parent.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("parent directory does not exist: {}", parent.display()),
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("output already exists: {}", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn validate_entries(entries: &[Entry]) -> Result<(), io::Error> {
    for entry in entries {
        let metadata = fs::symlink_metadata(&entry.source)?;
        if !metadata.file_type().is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "archive source is not a regular file: {}",
                    entry.source.display()
                ),
            ));
        }
    }
    Ok(())
}

fn write_archive(options: &Options, entries: &[Entry]) -> Result<(), io::Error> {
    let mut archive = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&options.output)?;
    for entry in entries {
        let bytes = fs::read(&entry.source)?;
        let header = tar_header(
            &entry.archive_name,
            entry.mode,
            bytes.len() as u64,
            options.source_date_epoch,
        )?;
        archive.write_all(&header)?;
        archive.write_all(&bytes)?;
        let padding = (BLOCK_SIZE - (bytes.len() % BLOCK_SIZE)) % BLOCK_SIZE;
        if padding != 0 {
            archive.write_all(&[0_u8; BLOCK_SIZE][..padding])?;
        }
    }
    archive.write_all(&[0_u8; BLOCK_SIZE * 2])?;
    archive.sync_all()
}

fn tar_header(
    name: &str,
    mode: u32,
    size: u64,
    modified: u64,
) -> Result<[u8; BLOCK_SIZE], io::Error> {
    let name = name.as_bytes();
    if name.len() > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "archive path is too long for the deterministic tar writer",
        ));
    }
    let mut header = [0_u8; BLOCK_SIZE];
    header[..name.len()].copy_from_slice(name);
    write_octal(&mut header[100..108], u64::from(mode), b'\0')?;
    write_octal(&mut header[108..116], 0, b'\0')?;
    write_octal(&mut header[116..124], 0, b'\0')?;
    write_octal(&mut header[124..136], size, b'\0')?;
    write_octal(&mut header[136..148], modified, b'\0')?;
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum();
    write_octal(&mut header[148..156], checksum, b' ')?;
    Ok(header)
}

fn write_octal(field: &mut [u8], value: u64, terminator: u8) -> Result<(), io::Error> {
    let digits = format!("{value:o}");
    if digits.len().saturating_add(1) > field.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "archive numeric field overflow",
        ));
    }
    field.fill(b'0');
    let start = field.len().saturating_sub(digits.len().saturating_add(1));
    field[start..start.saturating_add(digits.len())].copy_from_slice(digits.as_bytes());
    if let Some(last) = field.last_mut() {
        *last = terminator;
    }
    Ok(())
}

fn write_checksum(options: &Options) -> Result<(), io::Error> {
    let archive = fs::read(&options.output)?;
    let digest = Sha256::digest(archive);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let archive_name = options.output.file_name().map_or_else(
        || "tale.tar".to_owned(),
        |value| value.to_string_lossy().into_owned(),
    );
    let contents = format!("{hex}  {archive_name}\n");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&options.checksum)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_headers_are_deterministic() {
        let first = tar_header("tale-target/tale", 0o755, 17, 1_700_000_000);
        let second = tar_header("tale-target/tale", 0o755, 17, 1_700_000_000);
        assert!(first.is_ok());
        assert!(second.is_ok());
        if let (Ok(first), Ok(second)) = (first, second) {
            assert_eq!(first, second);
        }
    }
}
