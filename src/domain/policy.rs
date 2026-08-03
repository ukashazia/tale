use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PolicySnapshot {
    pub source_bytes: Vec<u8>,
    pub content_type: String,
    pub fetched_at: Timestamp,
    pub content_hash: String,
    pub etag: Option<String>,
}

impl PolicySnapshot {
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.source_bytes).ok()
    }
}
