use std::fmt;

use super::Timestamp;

#[derive(Clone, Eq, PartialEq)]
pub struct PolicySnapshot {
    pub source_bytes: Vec<u8>,
    pub content_type: String,
    pub fetched_at: Timestamp,
    pub content_hash: String,
    pub etag: Option<String>,
}

impl fmt::Debug for PolicySnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PolicySnapshot")
            .field(
                "source_bytes",
                &format_args!("<{} bytes>", self.source_bytes.len()),
            )
            .field("content_type", &self.content_type)
            .field("fetched_at", &self.fetched_at)
            .field("content_hash", &self.content_hash)
            .field("etag", &self.etag)
            .finish()
    }
}

impl PolicySnapshot {
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.source_bytes).ok()
    }
}
