use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::domain::export::{ExportDocument, ExportError};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ExportWriteError {
    #[error("export parent directory does not exist")]
    MissingParent,
    #[error("export path is not writable: {0}")]
    NotWritable(String),
    #[error("export would overwrite an existing file without confirmation")]
    OverwriteNotConfirmed,
    #[error("export serialization failed: {0}")]
    Serialization(ExportError),
    #[error("export write failed: {0}")]
    Write(String),
}

pub fn write_atomic(
    document: &ExportDocument,
    path: &Path,
    format: ExportFormat,
    overwrite_confirmed: bool,
) -> Result<PathBuf, ExportWriteError> {
    let parent = path.parent().ok_or(ExportWriteError::MissingParent)?;
    if !parent.is_dir() {
        return Err(ExportWriteError::MissingParent);
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ExportWriteError::NotWritable(
                    "export target is not a regular file".to_owned(),
                ));
            }
            if !overwrite_confirmed {
                return Err(ExportWriteError::OverwriteNotConfirmed);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(ExportWriteError::NotWritable(error.to_string())),
    }
    let bytes = match format {
        ExportFormat::Json => document.json_bytes_in_order(),
        ExportFormat::Csv => document.csv_bytes_in_order(),
    }
    .map_err(ExportWriteError::Serialization)?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .map_or("export", |value| value),
        std::process::id(),
        suffix
    ));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true).read(true);
        let mut file = options
            .open(&temporary)
            .map_err(|error| ExportWriteError::NotWritable(error.to_string()))?;
        set_private_permissions(&file)?;
        file.write_all(&bytes)
            .map_err(|error| ExportWriteError::Write(error.to_string()))?;
        file.flush()
            .map_err(|error| ExportWriteError::Write(error.to_string()))?;
        file.sync_all()
            .map_err(|error| ExportWriteError::Write(error.to_string()))?;
        fs::rename(&temporary, path).map_err(|error| ExportWriteError::Write(error.to_string()))?;
        Ok::<(), ExportWriteError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(path.to_path_buf())
}

fn set_private_permissions(file: &File) -> Result<(), ExportWriteError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| ExportWriteError::NotWritable(error.to_string()))?;
    }
    Ok(())
}
