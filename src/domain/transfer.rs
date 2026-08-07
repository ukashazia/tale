use std::path::{Path, PathBuf};

use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaildropTarget {
    pub command_target: String,
    pub display_name: String,
    /// The client does not always report a separate device name; when it does
    /// not, there is nothing to show rather than a sentinel to print.
    pub device_name: Option<String>,
    pub online: Option<bool>,
    pub capability_reason: Option<String>,
}

impl TaildropTarget {
    pub fn available(&self) -> bool {
        self.capability_reason.is_none() && self.online != Some(false)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TransferFile {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaildropSendRequest {
    pub files: Vec<TransferFile>,
    pub target: TaildropTarget,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TaildropConflict {
    Skip,
    Overwrite,
    Rename,
}

impl TaildropConflict {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Overwrite => "overwrite",
            Self::Rename => "rename",
        }
    }

    pub const fn is_overwrite(self) -> bool {
        matches!(self, Self::Overwrite)
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "skip" => Some(Self::Skip),
            "overwrite" => Some(Self::Overwrite),
            "rename" => Some(Self::Rename),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaildropReceiveRequest {
    pub directory: PathBuf,
    pub conflict: TaildropConflict,
    pub wait: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TransferProgress {
    pub completed_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub percent: Option<u8>,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TaildriveShare {
    pub name: String,
    pub path: PathBuf,
    pub as_user: Option<String>,
}

pub fn validate_regular_file(path: &Path) -> Result<TransferFile, String> {
    if path.as_os_str() == "-" {
        return Err("standard input '-' is not a file selection".to_owned());
    }
    let metadata = std::fs::metadata(path).map_err(|_| "file does not exist".to_owned())?;
    if !metadata.is_file() {
        return Err("directories and non-regular files are not accepted".to_owned());
    }
    Ok(TransferFile {
        path: path.to_path_buf(),
        size: metadata.len(),
    })
}

pub fn validate_receive_directory(path: &Path) -> Result<(), String> {
    let metadata =
        std::fs::metadata(path).map_err(|_| "destination directory does not exist".to_owned())?;
    if !metadata.is_dir() {
        return Err("destination must be a directory".to_owned());
    }
    let probe = path.join(".tale-write-check");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            Ok(())
        }
        Err(_) => Err("destination directory is not writable".to_owned()),
    }
}

pub fn normalize_share_name(input: &str) -> Result<String, String> {
    let normalized = input.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err("share name becomes empty after normalization".to_owned());
    }
    if !normalized.chars().all(|character| {
        character.is_ascii_lowercase()
            || character == ' '
            || character == '_'
            || character == '('
            || character == ')'
    }) {
        return Err(
            "share names may contain lowercase letters, spaces, underscores, or parentheses"
                .to_owned(),
        );
    }
    Ok(normalized)
}
