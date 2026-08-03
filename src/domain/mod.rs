pub mod account;
pub mod device;
pub mod diagnostic;
pub mod filter;
pub mod mutation;
pub mod preference;
pub mod redaction;
pub mod route;
pub mod source;

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
