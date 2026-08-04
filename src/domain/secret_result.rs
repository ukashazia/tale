use std::fmt;
use std::sync::Arc;

use zeroize::Zeroizing;

use super::Timestamp;

/// An in-memory secret that can only be borrowed by the owning workflow.
///
/// This type intentionally has no `Clone`, `Display`, serialization, or
/// equality implementation. The only copyable handle is an `Arc` owned by an
/// ephemeral result or a clipboard operation.
pub struct SecretBuffer(Zeroizing<Vec<u8>>);

impl SecretBuffer {
    pub fn new(value: impl AsRef<[u8]>) -> Self {
        Self(Zeroizing::new(value.as_ref().to_vec()))
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(self.as_bytes()).ok()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted secret>")
    }
}

/// Editable write-only secret input used by an ephemeral form.
#[derive(Clone, Eq, PartialEq)]
pub struct SecretInput(Zeroizing<String>);

impl SecretInput {
    pub fn new() -> Self {
        Self(Zeroizing::new(String::new()))
    }

    pub fn push(&mut self, value: char) {
        self.0.push(value);
    }

    pub fn push_str(&mut self, value: &str) {
        self.0.push_str(value);
    }

    pub fn pop(&mut self) {
        let _ = self.0.pop();
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Default for SecretInput {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted secret input>")
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SecretMetadata {
    pub result_id: u64,
    pub credential_id: Option<String>,
    pub credential_type: String,
    pub description: Option<String>,
    pub created_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub warning: String,
}

pub struct SecretResult {
    metadata: SecretMetadata,
    secret: Option<Arc<SecretBuffer>>,
    copy_requested: bool,
}

impl fmt::Debug for SecretResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretResult")
            .field("metadata", &self.metadata)
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .field("copy_requested", &self.copy_requested)
            .finish()
    }
}

impl SecretResult {
    pub fn new(metadata: SecretMetadata, secret: SecretBuffer) -> Self {
        Self::from_handle(metadata, Arc::new(secret))
    }

    pub(crate) fn from_handle(metadata: SecretMetadata, secret: Arc<SecretBuffer>) -> Self {
        Self {
            metadata,
            secret: Some(secret),
            copy_requested: false,
        }
    }

    pub fn metadata(&self) -> &SecretMetadata {
        &self.metadata
    }

    pub fn copy_requested(&self) -> bool {
        self.copy_requested
    }

    pub fn mark_copy_requested(&mut self) -> Option<Arc<SecretBuffer>> {
        let secret = self.secret.as_ref().map(Arc::clone)?;
        self.copy_requested = true;
        Some(secret)
    }

    pub fn close(&mut self) {
        self.secret = None;
    }

    pub fn is_closed(&self) -> bool {
        self.secret.is_none()
    }

    pub(crate) fn secret_handle(&self) -> Option<Arc<SecretBuffer>> {
        self.secret.as_ref().map(Arc::clone)
    }
}

impl Drop for SecretResult {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{SecretBuffer, SecretMetadata, SecretResult};

    #[test]
    fn close_drops_the_only_secret_handle_owned_by_result() {
        let result = SecretResult::new(
            SecretMetadata {
                result_id: 1,
                credential_id: Some("key-1".to_owned()),
                credential_type: "auth".to_owned(),
                description: None,
                created_at: 1,
                expires_at: None,
                warning: "view once".to_owned(),
            },
            SecretBuffer::new("fictional-secret-canary"),
        );
        let handle = result.secret_handle();
        assert!(handle.is_some());
        drop(handle);
        let mut result = result;
        result.close();
        assert!(result.is_closed());
    }
}
