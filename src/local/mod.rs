pub mod accounts;
pub mod certificates;
pub mod client;
pub mod daemon;
pub mod diagnostics;
pub mod dto;
pub mod handoff;
pub mod ipn;
pub mod policy;
pub mod process;
pub mod services;
pub mod transfers;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::Timestamp;

pub fn now() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
