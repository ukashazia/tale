use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PolicySource {
    CurrentRemote,
    ActiveCandidate,
}

impl PolicySource {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::CurrentRemote => "current remote",
            Self::ActiveCandidate => "active candidate",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AccessQuestion {
    pub source_selector: String,
    pub destination_selector: String,
    pub protocol_or_port: Option<String>,
    pub ssh_user: Option<String>,
    pub application_capability: Option<String>,
    pub policy_source: PolicySource,
}

impl AccessQuestion {
    pub fn supported_preview_type(&self) -> Option<&'static str> {
        if self.ssh_user.is_some() || self.application_capability.is_some() {
            None
        } else if self.protocol_or_port.is_some() {
            Some("ipport")
        } else {
            Some("user")
        }
    }

    pub fn preview_input(&self) -> Option<String> {
        match self.supported_preview_type()? {
            "ipport" => Some(format!(
                "{}:{}",
                self.destination_selector,
                self.protocol_or_port.as_deref()?
            )),
            "user" => Some(self.source_selector.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AccessDecision {
    Allowed,
    Denied,
    Indeterminate,
}

impl AccessDecision {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Allowed => "Allowed",
            Self::Denied => "Denied",
            Self::Indeterminate => "Indeterminate",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AccessResult {
    pub decision: AccessDecision,
    pub policy_hash: String,
    pub input: String,
    pub requested_at: Timestamp,
    pub limitations: Vec<String>,
    pub matched_users: Vec<String>,
    pub matched_ports: Vec<String>,
    pub rule_locations: Vec<u32>,
    pub source: PolicySource,
}

impl AccessResult {
    pub fn indeterminate(
        policy_hash: impl Into<String>,
        input: impl Into<String>,
        requested_at: Timestamp,
        source: PolicySource,
        limitation: impl Into<String>,
    ) -> Self {
        Self {
            decision: AccessDecision::Indeterminate,
            policy_hash: policy_hash.into(),
            input: input.into(),
            requested_at,
            limitations: vec![limitation.into()],
            matched_users: Vec::new(),
            matched_ports: Vec::new(),
            rule_locations: Vec::new(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_dimensions_are_not_locally_evaluated() {
        let question = AccessQuestion {
            source_selector: "alice@example.test".to_owned(),
            destination_selector: "100.64.0.2".to_owned(),
            protocol_or_port: None,
            ssh_user: Some("root".to_owned()),
            application_capability: None,
            policy_source: PolicySource::CurrentRemote,
        };
        assert!(question.supported_preview_type().is_none());
    }
}
