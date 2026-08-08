pub mod access_explorer;
pub mod account;
pub mod activity;
pub mod admin_mutation;
pub mod certificate;
pub mod credential;
pub mod device;
pub mod diagnostic;
pub mod dns;
pub mod export;
pub mod filter;
pub mod flow;
pub mod health;
pub mod log_stream;
pub mod mutation;
pub mod operational;
pub mod policy;
pub mod policy_workflow;
pub mod preference;
pub mod profile;
pub mod redaction;
pub mod route;
pub mod saved_view;
pub mod secret_result;
pub mod service;
pub mod source;
pub mod transfer;
pub mod user;
pub mod webhook;

pub type Timestamp = u64;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceHealth {
    Loading,
    Healthy,
    Stale,
    Error,
    Unavailable,
}

impl SourceHealth {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Healthy => "healthy",
            Self::Stale => "stale",
            Self::Error => "error",
            Self::Unavailable => "unavailable",
        }
    }
}
