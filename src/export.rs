use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::domain::export::{ExportDocument, ExportError};
use crate::private_file::{WriteStage, write_private_atomic};

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
    write_private_atomic(&temporary, path, &bytes).map_err(|error| match error.stage() {
        WriteStage::Open | WriteStage::Permissions => {
            ExportWriteError::NotWritable(error.to_string())
        }
        WriteStage::Write | WriteStage::Rename => ExportWriteError::Write(error.to_string()),
    })?;
    Ok(path.to_path_buf())
}
