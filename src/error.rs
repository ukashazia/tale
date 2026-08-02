use std::fmt::Display;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaleError {
    #[error("invalid command-line arguments: {0}")]
    InvalidArguments(String),
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("could not read configuration: {0}")]
    ConfigurationIo(String),
    #[error("runtime initialization failed: {0}")]
    RuntimeInitialization(String),
    #[error("terminal error: {0}")]
    Terminal(String),
    #[error("application error: {0}")]
    Application(String),
}

impl TaleError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::InvalidArguments(_) | Self::InvalidConfiguration(_) => 2,
            Self::ConfigurationIo(_)
            | Self::RuntimeInitialization(_)
            | Self::Terminal(_)
            | Self::Application(_) => 1,
        }
    }

    pub fn from_message<E>(error: E, kind: fn(String) -> Self) -> Self
    where
        E: Display,
    {
        kind(error.to_string())
    }
}
