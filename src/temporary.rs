use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::policy_workflow::MAX_POLICY_BYTES;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum TemporaryFileError {
    #[error("secure policy editing is not supported on this platform")]
    UnsupportedPlatform,
    #[error("secure policy temporary storage is unavailable")]
    Unavailable,
    #[error("the policy temporary file could not be created")]
    Create,
    #[error("the policy temporary file could not be written")]
    Write,
    #[error("the policy temporary file could not be read")]
    Read,
    #[error("the policy temporary file is not a regular file")]
    NotRegular,
    #[error("the policy temporary file was replaced by a link")]
    Link,
    #[error("the policy candidate exceeds the 4 MiB limit")]
    TooLarge,
    #[error(
        "the policy temporary file could not be removed; remediate this path after Tale exits: {path}"
    )]
    Cleanup { path: PathBuf },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
    len: u64,
    modified_nanos: Option<u128>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|value| value.as_nanos()),
            #[cfg(unix)]
            device: std::os::unix::fs::MetadataExt::dev(metadata),
            #[cfg(unix)]
            inode: std::os::unix::fs::MetadataExt::ino(metadata),
        }
    }
}

pub struct TemporaryPolicyFile {
    directory: Option<tempfile::TempDir>,
    path: PathBuf,
    initial_identity: FileIdentity,
    closed: bool,
}

impl std::fmt::Debug for TemporaryPolicyFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TemporaryPolicyFile")
            .field("path", &self.path)
            .field("initial_identity", &self.initial_identity)
            .field("closed", &self.closed)
            .finish()
    }
}

impl TemporaryPolicyFile {
    pub fn create(bytes: &[u8]) -> Result<Self, TemporaryFileError> {
        if !secure_user_storage_available() {
            return Err(TemporaryFileError::UnsupportedPlatform);
        }
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(TemporaryFileError::TooLarge);
        }
        let directory = tempfile::Builder::new()
            .prefix("tale-policy-")
            .tempdir()
            .map_err(|_| TemporaryFileError::Unavailable)?;
        let path = directory.path().join("policy.hujson");
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(&path)
            .map_err(|_| TemporaryFileError::Create)?;
        if file.write_all(bytes).is_err() || file.sync_all().is_err() {
            return Err(TemporaryFileError::Write);
        }
        drop(file);
        let metadata = fs::symlink_metadata(&path).map_err(|_| TemporaryFileError::Create)?;
        if !metadata.file_type().is_file() {
            return Err(TemporaryFileError::NotRegular);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o777 != 0o600 {
                return Err(TemporaryFileError::Create);
            }
        }
        let initial_identity = FileIdentity::from_metadata(&metadata);
        Ok(Self {
            directory: Some(directory),
            path,
            initial_identity,
            closed: false,
        })
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn identity_changed(&self) -> Result<bool, TemporaryFileError> {
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| TemporaryFileError::Read)?;
        if !metadata.file_type().is_file() {
            return Err(TemporaryFileError::NotRegular);
        }
        Ok(FileIdentity::from_metadata(&metadata) != self.initial_identity)
    }

    pub fn read_candidate(&self) -> Result<Vec<u8>, TemporaryFileError> {
        if self.closed {
            return Err(TemporaryFileError::Read);
        }
        read_policy_candidate(&self.path)
    }

    pub fn read_candidate_path(path: &Path) -> Result<Vec<u8>, TemporaryFileError> {
        read_policy_candidate(path)
    }
}

#[cfg(unix)]
pub const fn policy_editing_supported() -> bool {
    true
}

#[cfg(not(unix))]
pub const fn policy_editing_supported() -> bool {
    false
}

#[cfg(unix)]
const fn secure_user_storage_available() -> bool {
    policy_editing_supported()
}

#[cfg(not(unix))]
const fn secure_user_storage_available() -> bool {
    policy_editing_supported()
}

fn read_policy_candidate(path: &Path) -> Result<Vec<u8>, TemporaryFileError> {
    let link_metadata = fs::symlink_metadata(path).map_err(|_| TemporaryFileError::Read)?;
    if link_metadata.file_type().is_symlink() {
        return Err(TemporaryFileError::Link);
    }
    if !link_metadata.file_type().is_file() {
        return Err(TemporaryFileError::NotRegular);
    }
    if link_metadata.len() > MAX_POLICY_BYTES as u64 {
        return Err(TemporaryFileError::TooLarge);
    }
    let file = open_read_only_no_follow(path)?;
    let metadata = file.metadata().map_err(|_| TemporaryFileError::Read)?;
    if !metadata.file_type().is_file() {
        return Err(TemporaryFileError::NotRegular);
    }
    if metadata.len() > MAX_POLICY_BYTES as u64 {
        return Err(TemporaryFileError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_POLICY_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| TemporaryFileError::Read)?;
    if bytes.len() > MAX_POLICY_BYTES {
        return Err(TemporaryFileError::TooLarge);
    }
    Ok(bytes)
}

impl TemporaryPolicyFile {
    pub fn close(&mut self) -> Result<(), TemporaryFileError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let file_result = match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_file() => fs::remove_file(&self.path),
            Ok(_) => Err(io::Error::other("temporary path is not a regular file")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
        let directory_result = self
            .directory
            .take()
            .map_or(Ok(()), |directory| directory.close());
        if file_result.is_ok() && directory_result.is_ok() {
            Ok(())
        } else {
            Err(TemporaryFileError::Cleanup {
                path: self.path.clone(),
            })
        }
    }
}

impl Drop for TemporaryPolicyFile {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn open_read_only_no_follow(path: &Path) -> Result<File, TemporaryFileError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options.open(path).map_err(|error| {
        if error.kind() == io::ErrorKind::InvalidInput {
            TemporaryFileError::Link
        } else {
            TemporaryFileError::Read
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{TemporaryFileError, TemporaryPolicyFile};

    #[test]
    fn preserves_bytes_and_rejects_symlink_replacement() {
        let original = b"{\r\n  // fictional comment\n}\n";
        let file = TemporaryPolicyFile::create(original).map_err(|_| ()).ok();
        assert!(file.is_some());
        if let Some(file) = file {
            assert_eq!(
                file.read_candidate().map_err(|_| ()).ok().as_deref(),
                Some(original.as_slice())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_not_accepted() {
        let original = TemporaryPolicyFile::create(b"base").map_err(|_| ()).ok();
        assert!(original.is_some());
        if let Some(mut original) = original {
            let path = original.path().to_path_buf();
            let replacement = path.with_extension("replacement");
            let moved = std::fs::rename(&path, &replacement).is_ok();
            let linked = moved && std::os::unix::fs::symlink(&replacement, &path).is_ok();
            assert!(linked);
            assert!(matches!(
                original.read_candidate(),
                Err(TemporaryFileError::Link)
            ));
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(&replacement);
            let _ = original.close();
        }
    }
}
