pub mod client;
pub mod diagnostics;
pub mod dto;
pub mod process;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::Timestamp;

pub fn now() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
