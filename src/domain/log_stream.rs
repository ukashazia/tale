use thiserror::Error;

use super::Timestamp;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum LogType {
    Configuration,
    Network,
}

impl LogType {
    pub const fn wire_value(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Network => "network",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SecretAction {
    KeepExisting,
    Replace,
}

impl SecretAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::KeepExisting => "keep existing",
            Self::Replace => "replace",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LogStreamDestination {
    pub kind: String,
    pub identity: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LogStreamConfiguration {
    pub log_type: LogType,
    pub enabled: bool,
    pub destination: LogStreamDestination,
    pub secret_action: SecretAction,
    pub observed_at: Timestamp,
    pub source_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LogStreamStatus {
    pub log_type: LogType,
    pub configured: bool,
    pub healthy: Option<bool>,
    pub status: String,
    pub last_observation: Option<Timestamp>,
    pub source_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LogStreamForm {
    pub log_type: LogType,
    pub enabled: bool,
    pub destination: LogStreamDestination,
    pub secret_action: SecretAction,
}

impl LogStreamForm {
    pub fn validate(&self) -> Result<(), LogStreamError> {
        if self.destination.kind.trim().is_empty() || self.destination.identity.trim().is_empty() {
            return Err(LogStreamError::MissingDestination);
        }
        if self.destination.identity.chars().any(char::is_control) {
            return Err(LogStreamError::InvalidDestination);
        }
        Ok(())
    }

    pub fn preview(&self) -> String {
        format!(
            "Replace {} log stream: enabled={} destination={} identity={} secret={}",
            self.log_type.wire_value(),
            self.enabled,
            self.destination.kind,
            self.destination.identity,
            self.secret_action.label()
        )
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum LogStreamError {
    #[error("a log-stream destination is required")]
    MissingDestination,
    #[error("the log-stream destination contains a control character")]
    InvalidDestination,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_never_contains_secret_values() {
        let form = LogStreamForm {
            log_type: LogType::Network,
            enabled: true,
            destination: LogStreamDestination {
                kind: "webhook".to_owned(),
                identity: "logs.example.test".to_owned(),
            },
            secret_action: SecretAction::Replace,
        };
        assert!(!form.preview().contains("secret-value"));
        assert!(form.validate().is_ok());
    }
}
