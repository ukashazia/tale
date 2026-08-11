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

pub fn expand_home_path(path: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    let Ok(relative) = path.strip_prefix("~") else {
        return Ok(path.to_path_buf());
    };
    let home = home.ok_or_else(|| "cannot expand '~' because HOME is not set".to_owned())?;
    Ok(home.join(relative))
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
    // `create_new` on a fixed name reports "not writable" for a directory that
    // is writable, whenever a second check is in flight against it — another
    // Tale, or the same one twice. The name is unique per check so the probe
    // measures the directory rather than the other probe.
    let probe = path.join(format!(
        ".tale-write-check-{}-{}",
        std::process::id(),
        PROBE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
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

static PROBE_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_home_shorthand_without_changing_other_paths() {
        let home = Path::new("/home/alice");
        assert_eq!(
            expand_home_path(Path::new("~/documents/report.pdf"), Some(home)),
            Ok(PathBuf::from("/home/alice/documents/report.pdf"))
        );
        assert_eq!(
            expand_home_path(Path::new("~"), Some(home)),
            Ok(PathBuf::from("/home/alice"))
        );
        assert_eq!(
            expand_home_path(Path::new("/tmp/report.pdf"), Some(home)),
            Ok(PathBuf::from("/tmp/report.pdf"))
        );
        assert_eq!(
            expand_home_path(Path::new("~someone/report.pdf"), Some(home)),
            Ok(PathBuf::from("~someone/report.pdf"))
        );
    }

    #[test]
    fn reports_when_home_is_unavailable_for_shorthand() {
        assert_eq!(
            expand_home_path(Path::new("~/report.pdf"), None),
            Err("cannot expand '~' because HOME is not set".to_owned())
        );
    }

    /// A writable directory has to read as writable however many checks are
    /// running against it. A fixed probe name made the second one fail, which
    /// told the user their own download directory was not writable.
    #[test]
    fn concurrent_checks_of_one_directory_all_succeed() {
        let directory = std::env::temp_dir().join(format!(
            "tale-probe-{}-{}",
            std::process::id(),
            PROBE_SEQUENCE.load(std::sync::atomic::Ordering::Relaxed)
        ));
        assert!(std::fs::create_dir_all(&directory).is_ok());

        let results = std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| scope.spawn(|| validate_receive_directory(&directory)))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.len(), 8);
        for result in results {
            assert!(result.is_ok(), "a concurrent check failed: {result:?}");
        }
        // The probes clean up after themselves rather than accumulating in a
        // directory the user chose for their downloads.
        let leftovers = std::fs::read_dir(&directory)
            .map(|entries| entries.count())
            .unwrap_or(usize::MAX);
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
