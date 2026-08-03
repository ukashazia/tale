use sha2::{Digest, Sha256};

use crate::admin::client::PolicyBody;
use crate::domain::Timestamp;
use crate::domain::policy::PolicySnapshot;

pub fn decode_policy(body: PolicyBody, fetched_at: Timestamp) -> PolicySnapshot {
    let mut hasher = Sha256::new();
    hasher.update(&body.source_bytes);
    let content_hash = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    PolicySnapshot {
        source_bytes: body.source_bytes,
        content_type: body.content_type,
        fetched_at,
        content_hash,
        etag: body.etag,
    }
}
