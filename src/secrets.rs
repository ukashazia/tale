//! Secret material and the store that holds it.
//!
//! The store is expressed as a trait over [`CredentialRecord`] rather than over an
//! opaque encoded string, so each backend chooses its own representation: the file
//! backend writes readable TOML, and a future backend is free to serialise however it
//! likes without the record type or its callers changing.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

const RECORD_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialKind {
    OAuthClient,
    AccessToken,
}

impl CredentialKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OAuthClient => "oauth_client",
            Self::AccessToken => "access_token",
        }
    }
}

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("the credential store could not be read")]
    ReadFailure,
    #[error("the credential store could not be written")]
    WriteFailure,
    #[error("the credential store is not valid TOML")]
    Malformed,
    #[error("the credential record is not valid")]
    InvalidRecord,
    #[error("the credential store has unsafe permissions: {0}")]
    UnsafePermissions(String),
}

pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[derive(Debug)]
pub struct OAuthClientRecord {
    pub version: u8,
    pub client_id: SecretValue,
    pub client_secret: SecretValue,
    pub requested_scopes: Vec<String>,
}

#[derive(Debug)]
pub struct AccessTokenRecord {
    pub version: u8,
    pub access_token: SecretValue,
}

#[derive(Debug)]
pub enum CredentialRecord {
    OAuthClient(OAuthClientRecord),
    AccessToken(AccessTokenRecord),
}

impl CredentialRecord {
    pub fn kind(&self) -> CredentialKind {
        match self {
            Self::OAuthClient(_) => CredentialKind::OAuthClient,
            Self::AccessToken(_) => CredentialKind::AccessToken,
        }
    }

    pub fn requested_scopes(&self) -> Vec<String> {
        match self {
            Self::OAuthClient(record) => record.requested_scopes.clone(),
            Self::AccessToken(_) => Vec::new(),
        }
    }
}

/// Which backend holds a profile's credential, and whatever that backend needs to find
/// it. Recorded per profile so a configuration states where its secret lives rather than
/// leaving it implied by the build, and so a second backend can be added without
/// reinterpreting existing profiles.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CredentialBackend {
    File { path: PathBuf },
}

impl CredentialBackend {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
        }
    }

    /// The backend's location as it appears in configuration.
    pub fn location(&self) -> &Path {
        match self {
            Self::File { path } => path,
        }
    }

    pub fn open(&self) -> Arc<dyn CredentialStore> {
        match self {
            Self::File { path } => Arc::new(FileCredentialStore::new(path.clone())),
        }
    }
}

/// Where credentials live. Implementations must treat the reference as an opaque
/// account name and must not return a record they cannot fully decode.
pub trait CredentialStore: Send + Sync {
    fn get(&self, reference: &str) -> Result<Option<CredentialRecord>, SecretsError>;
    fn set(&self, reference: &str, record: &CredentialRecord) -> Result<(), SecretsError>;
    fn delete(&self, reference: &str) -> Result<bool, SecretsError>;
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredRecord {
    OAuthClient {
        version: u8,
        client_id: String,
        client_secret: String,
        requested_scopes: Vec<String>,
    },
    AccessToken {
        version: u8,
        access_token: String,
    },
}

#[derive(Serialize, Deserialize, Default)]
struct StoredFile {
    #[serde(default)]
    credentials: BTreeMap<String, StoredRecord>,
}

impl From<&CredentialRecord> for StoredRecord {
    fn from(record: &CredentialRecord) -> Self {
        match record {
            CredentialRecord::OAuthClient(record) => Self::OAuthClient {
                version: record.version,
                client_id: record.client_id.as_str().to_owned(),
                client_secret: record.client_secret.as_str().to_owned(),
                requested_scopes: record.requested_scopes.clone(),
            },
            CredentialRecord::AccessToken(record) => Self::AccessToken {
                version: record.version,
                access_token: record.access_token.as_str().to_owned(),
            },
        }
    }
}

impl TryFrom<StoredRecord> for CredentialRecord {
    type Error = SecretsError;

    fn try_from(stored: StoredRecord) -> Result<Self, Self::Error> {
        match stored {
            StoredRecord::OAuthClient {
                version,
                client_id,
                client_secret,
                requested_scopes,
            } if version == RECORD_VERSION
                && !client_id.is_empty()
                && !client_secret.is_empty() =>
            {
                Ok(Self::OAuthClient(OAuthClientRecord {
                    version,
                    client_id: SecretValue::new(client_id),
                    client_secret: SecretValue::new(client_secret),
                    requested_scopes,
                }))
            }
            StoredRecord::AccessToken {
                version,
                access_token,
            } if version == RECORD_VERSION && !access_token.is_empty() => {
                Ok(Self::AccessToken(AccessTokenRecord {
                    version,
                    access_token: SecretValue::new(access_token),
                }))
            }
            _ => Err(SecretsError::InvalidRecord),
        }
    }
}

/// Credentials in a single TOML file readable only by its owner.
#[derive(Debug, Clone)]
pub struct FileCredentialStore {
    path: PathBuf,
}

impl FileCredentialStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Result<StoredFile, SecretsError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoredFile::default());
            }
            Err(_) => return Err(SecretsError::ReadFailure),
        };
        // A world- or group-readable secret file is refused rather than quietly used:
        // the whole point of this backend is that the mode is the protection.
        enforce_owner_only(&self.path)?;
        toml::from_str::<StoredFile>(&contents).map_err(|_| SecretsError::Malformed)
    }

    fn store(&self, file: &StoredFile) -> Result<(), SecretsError> {
        let serialized =
            Zeroizing::new(toml::to_string_pretty(file).map_err(|_| SecretsError::WriteFailure)?);
        write_owner_only_atomic(&self.path, serialized.as_bytes())
    }
}

impl CredentialStore for FileCredentialStore {
    fn get(&self, reference: &str) -> Result<Option<CredentialRecord>, SecretsError> {
        self.load()?
            .credentials
            .remove(reference)
            .map(CredentialRecord::try_from)
            .transpose()
    }

    fn set(&self, reference: &str, record: &CredentialRecord) -> Result<(), SecretsError> {
        let mut file = self.load()?;
        file.credentials
            .insert(reference.to_owned(), StoredRecord::from(record));
        self.store(&file)
    }

    fn delete(&self, reference: &str) -> Result<bool, SecretsError> {
        let mut file = self.load()?;
        if file.credentials.remove(reference).is_none() {
            return Ok(false);
        }
        self.store(&file)?;
        Ok(true)
    }
}

#[derive(Default)]
pub struct MemoryCredentialStore {
    values: std::sync::Mutex<BTreeMap<String, StoredRecord>>,
}

impl fmt::Debug for MemoryCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryCredentialStore")
            .field(
                "entries",
                &self.values.lock().map_or(0, |values| values.len()),
            )
            .finish()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, reference: &str) -> Result<Option<CredentialRecord>, SecretsError> {
        let values = self.values.lock().map_err(|_| SecretsError::ReadFailure)?;
        values
            .get(reference)
            .cloned()
            .map(CredentialRecord::try_from)
            .transpose()
    }

    fn set(&self, reference: &str, record: &CredentialRecord) -> Result<(), SecretsError> {
        self.values
            .lock()
            .map_err(|_| SecretsError::WriteFailure)
            .map(|mut values| {
                values.insert(reference.to_owned(), StoredRecord::from(record));
            })
    }

    fn delete(&self, reference: &str) -> Result<bool, SecretsError> {
        self.values
            .lock()
            .map_err(|_| SecretsError::WriteFailure)
            .map(|mut values| values.remove(reference).is_some())
    }
}

#[cfg(unix)]
fn enforce_owner_only(path: &Path) -> Result<(), SecretsError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)
        .map_err(|_| SecretsError::ReadFailure)?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 == 0 {
        return Ok(());
    }
    Err(SecretsError::UnsafePermissions(format!(
        "{} is mode {mode:04o}; run `chmod 600 {}`",
        path.display(),
        path.display()
    )))
}

#[cfg(not(unix))]
fn enforce_owner_only(_path: &Path) -> Result<(), SecretsError> {
    Ok(())
}

/// Writes through a same-directory temporary file created with owner-only permissions,
/// so the secret is never briefly visible at a wider mode.
fn write_owner_only_atomic(path: &Path, bytes: &[u8]) -> Result<(), SecretsError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| SecretsError::WriteFailure)?;
        restrict_directory(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(SecretsError::WriteFailure)?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| SecretsError::WriteFailure)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(SecretsError::WriteFailure);
    }
    drop(file);
    fs::rename(&temporary, path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        SecretsError::WriteFailure
    })
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<(), SecretsError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| SecretsError::WriteFailure)
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<(), SecretsError> {
    Ok(())
}
