use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum WriteStage {
    Open,
    Permissions,
    Write,
    Rename,
}

#[derive(Debug)]
pub(crate) struct WriteError {
    stage: WriteStage,
    source: io::Error,
}

impl WriteError {
    pub(crate) fn stage(&self) -> WriteStage {
        self.stage
    }
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

pub(crate) fn write_private_atomic(
    temporary: &Path,
    destination: &Path,
    bytes: &[u8],
) -> Result<(), WriteError> {
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true).read(true);
        let mut file = options.open(temporary).map_err(|source| WriteError {
            stage: WriteStage::Open,
            source,
        })?;
        set_private_permissions(&file)?;
        file.write_all(bytes)
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all())
            .map_err(|source| WriteError {
                stage: WriteStage::Write,
                source,
            })?;
        fs::rename(temporary, destination).map_err(|source| WriteError {
            stage: WriteStage::Rename,
            source,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
        return result;
    }
    if let Some(parent) = destination.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn set_private_permissions(file: &File) -> Result<(), WriteError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|source| WriteError {
                stage: WriteStage::Permissions,
                source,
            })?;
    }
    Ok(())
}
