use std::collections::BTreeSet;
use std::fmt;

use thiserror::Error;
use url::Url;

use super::Timestamp;

const KNOWN_CATEGORIES: &[&str] = &[
    "device",
    "user",
    "key",
    "policy",
    "dns",
    "webhook",
    "network",
    "nodeCreated",
    "nodeNeedsApproval",
    "nodeApproved",
    "nodeKeyExpiringInOneDay",
    "nodeKeyExpired",
    "nodeDeleted",
    "nodeSigned",
    "nodeNeedsSignature",
    "policyUpdate",
    "userCreated",
    "userNeedsApproval",
    "userSuspended",
    "userRestored",
    "userDeleted",
    "userApproved",
    "userRoleUpdated",
    "subnetIPForwardingNotEnabled",
    "exitNodeIPForwardingNotEnabled",
];

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum WebhookError {
    #[error("webhook URL must be a valid HTTPS URL")]
    InvalidUrl,
    #[error("webhook URL may use only documented ports 80 or 443")]
    UnsupportedPort,
    #[error("webhook URL must not contain credentials, fragments, or control characters")]
    UnsafeUrl,
    #[error("webhook subscription list contains an empty value")]
    EmptySubscription,
    #[error("webhook destination type is not supported by the documented mutation contract")]
    UnsupportedDestination,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DestinationType {
    None,
    Discord,
    GoogleChat,
    Mattermost,
    Slack,
    Unknown(String),
}

impl DestinationType {
    pub fn from_wire(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "discord" => Self::Discord,
            "googlechat" | "google_chat" => Self::GoogleChat,
            "mattermost" => Self::Mattermost,
            "slack" => Self::Slack,
            _ => Self::Unknown(value.to_owned()),
        }
    }

    pub fn wire_value(&self) -> &str {
        match self {
            Self::None => "none",
            Self::Discord => "discord",
            Self::GoogleChat => "googlechat",
            Self::Mattermost => "mattermost",
            Self::Slack => "slack",
            Self::Unknown(value) => value.as_str(),
        }
    }

    pub const fn supported_for_mutation(&self) -> bool {
        matches!(
            self,
            Self::Discord | Self::GoogleChat | Self::Mattermost | Self::Slack
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SubscriptionSet {
    pub categories: Vec<String>,
    pub events: Vec<String>,
    pub unknown_categories: Vec<String>,
    pub unknown_events: Vec<String>,
}

impl SubscriptionSet {
    pub fn from_wire(categories: Vec<String>, events: Vec<String>) -> Result<Self, WebhookError> {
        if categories
            .iter()
            .chain(events.iter())
            .any(|value| value.trim().is_empty())
        {
            return Err(WebhookError::EmptySubscription);
        }
        let known_categories = categories
            .iter()
            .filter(|value| KNOWN_CATEGORIES.contains(&value.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let unknown_categories = categories
            .into_iter()
            .filter(|value| !KNOWN_CATEGORIES.contains(&value.as_str()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let known_events = events
            .iter()
            .filter(|value| KNOWN_CATEGORIES.contains(&value.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let unknown_events = events
            .into_iter()
            .filter(|value| !KNOWN_CATEGORIES.contains(&value.as_str()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(Self {
            categories: known_categories,
            events: known_events,
            unknown_categories,
            unknown_events,
        })
    }

    pub fn wire_categories(&self) -> Vec<String> {
        merge_sorted(&self.categories, &self.unknown_categories)
    }

    pub fn wire_events(&self) -> Vec<String> {
        merge_sorted(&self.events, &self.unknown_events)
    }

    pub fn wire_subscriptions(&self) -> Vec<String> {
        self.wire_categories()
            .into_iter()
            .chain(self.wire_events())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn edit_known(
        &self,
        categories: Vec<String>,
        events: Vec<String>,
    ) -> Result<Self, WebhookError> {
        let mut replacement = Self::from_wire(categories, events)?;
        replacement
            .unknown_categories
            .extend(self.unknown_categories.iter().cloned());
        replacement
            .unknown_events
            .extend(self.unknown_events.iter().cloned());
        replacement.unknown_categories.sort();
        replacement.unknown_categories.dedup();
        replacement.unknown_events.sort();
        replacement.unknown_events.dedup();
        Ok(replacement)
    }
}

fn merge_sorted(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .chain(right.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Clone, Eq, PartialEq)]
pub struct WebhookEndpoint {
    pub stable_id: String,
    pub endpoint_url: String,
    pub destination_type: DestinationType,
    pub subscriptions: SubscriptionSet,
    pub creator_login_name: Option<String>,
    pub created_at: Option<String>,
    pub last_modified_at: Option<String>,
    pub status: String,
    pub last_result: Option<String>,
    pub observed_at: Timestamp,
    pub source_id: String,
}

impl fmt::Debug for WebhookEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebhookEndpoint")
            .field("stable_id", &self.stable_id)
            .field("endpoint_url", &redact_url(&self.endpoint_url))
            .field("destination_type", &self.destination_type)
            .field("subscriptions", &self.subscriptions)
            .field("creator_login_name", &self.creator_login_name)
            .field("created_at", &self.created_at)
            .field("last_modified_at", &self.last_modified_at)
            .field("status", &self.status)
            .field("last_result", &self.last_result)
            .field("observed_at", &self.observed_at)
            .field("source_id", &self.source_id)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct WebhookDraft {
    pub endpoint_url: String,
    pub destination_type: DestinationType,
    pub subscriptions: SubscriptionSet,
}

impl fmt::Debug for WebhookDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebhookDraft")
            .field("endpoint_url", &redact_url(&self.endpoint_url))
            .field("destination_type", &self.destination_type)
            .field("subscriptions", &self.subscriptions)
            .finish()
    }
}

impl WebhookDraft {
    pub fn validate(&self) -> Result<(), WebhookError> {
        validate_url(&self.endpoint_url)?;
        if !self.destination_type.supported_for_mutation() {
            return Err(WebhookError::UnsupportedDestination);
        }
        if self
            .subscriptions
            .wire_categories()
            .iter()
            .chain(self.subscriptions.wire_events().iter())
            .any(|value| value.trim().is_empty())
        {
            return Err(WebhookError::EmptySubscription);
        }
        Ok(())
    }
}

pub fn validate_url(value: &str) -> Result<(), WebhookError> {
    if value.chars().any(char::is_control) {
        return Err(WebhookError::UnsafeUrl);
    }
    let parsed = Url::parse(value).map_err(|_| WebhookError::InvalidUrl)?;
    if parsed.scheme() != "https"
        || parsed
            .host_str()
            .is_none_or(|host| host.is_empty() || host == ".")
    {
        return Err(WebhookError::InvalidUrl);
    }
    let authority_has_userinfo = value
        .split_once("://")
        .and_then(|(_, remainder)| {
            remainder
                .split(['/', '?', '#'])
                .next()
                .map(|authority| authority.contains('@'))
        })
        .unwrap_or(false);
    if parsed.username() != ""
        || parsed.password().is_some()
        || authority_has_userinfo
        || parsed.fragment().is_some()
    {
        return Err(WebhookError::UnsafeUrl);
    }
    if parsed.port().is_some_and(|port| port != 80 && port != 443) {
        return Err(WebhookError::UnsupportedPort);
    }
    Ok(())
}

#[derive(Clone, Eq, PartialEq)]
pub enum WebhookMutation {
    Create(WebhookDraft),
    EditSubscriptions {
        endpoint_id: String,
        endpoint_url: String,
        destination_type: DestinationType,
        before: SubscriptionSet,
        after: SubscriptionSet,
    },
    Test {
        endpoint_id: String,
    },
    RotateSecret {
        endpoint_id: String,
    },
    Delete {
        endpoint_id: String,
        endpoint_label: String,
    },
}

impl fmt::Debug for WebhookMutation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("WebhookMutation")
            .field(&self.preview())
            .finish()
    }
}

impl WebhookMutation {
    pub fn preview(&self) -> String {
        match self {
            Self::Create(draft) => format!(
                "Create HTTPS webhook to {} ({}) with categories [{}] (category subscriptions include future category events) and events [{}]",
                redact_url(&draft.endpoint_url),
                draft.destination_type.wire_value(),
                draft.subscriptions.wire_categories().join(","),
                draft.subscriptions.wire_events().join(",")
            ),
            Self::EditSubscriptions {
                endpoint_id,
                endpoint_url,
                destination_type,
                before,
                after,
            } => format!(
                "Edit webhook {endpoint_id}: URL {} → {} (unchanged), destination {} → {} (unchanged), categories [{}] → [{}] (category subscriptions include future category events), events [{}] → [{}]",
                redact_url(endpoint_url),
                redact_url(endpoint_url),
                destination_type.wire_value(),
                destination_type.wire_value(),
                before.wire_categories().join(","),
                after.wire_categories().join(","),
                before.wire_events().join(","),
                after.wire_events().join(",")
            ),
            Self::Test { endpoint_id } => format!("Queue a test for webhook {endpoint_id}"),
            Self::RotateSecret { endpoint_id } => {
                format!("Rotate the write-only secret for webhook {endpoint_id}")
            }
            Self::Delete {
                endpoint_id,
                endpoint_label,
            } => format!("Delete webhook {endpoint_label} ({endpoint_id})"),
        }
    }
}

fn redact_url(value: &str) -> String {
    super::redaction::redact_destination_url(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_credentials_and_undocumented_ports() {
        assert_eq!(
            validate_url("https://user:pass@example.test/hook"),
            Err(WebhookError::UnsafeUrl)
        );
        assert_eq!(
            validate_url("https://example.test:8443/hook"),
            Err(WebhookError::UnsupportedPort)
        );
        assert!(validate_url("https://example.test/hook").is_ok());
    }

    #[test]
    fn previews_and_debug_hide_url_paths_and_queries() {
        let subscriptions = SubscriptionSet::from_wire(Vec::new(), vec!["nodeCreated".to_owned()]);
        let Ok(subscriptions) = subscriptions else {
            return;
        };
        let mutation = WebhookMutation::Create(WebhookDraft {
            endpoint_url: "https://hooks.example.test/bearer-secret?token=query-secret".to_owned(),
            destination_type: DestinationType::Slack,
            subscriptions,
        });
        let preview = mutation.preview();
        let debug = format!("{mutation:?}");
        for output in [preview, debug] {
            assert!(output.contains("https://hooks.example.test/<redacted>"));
            assert!(!output.contains("bearer-secret"));
            assert!(!output.contains("query-secret"));
        }
    }

    #[test]
    fn unknown_subscriptions_survive_known_edit() {
        let original_result = SubscriptionSet::from_wire(
            vec!["device".to_owned(), "future-category".to_owned()],
            vec!["future-event".to_owned()],
        );
        assert!(original_result.is_ok());
        let Ok(original) = original_result else {
            return;
        };
        let edited_result = original.edit_known(vec!["user".to_owned()], Vec::new());
        assert!(edited_result.is_ok());
        let Ok(edited) = edited_result else {
            return;
        };
        assert_eq!(edited.wire_categories(), vec!["future-category", "user"]);
        assert_eq!(edited.wire_events(), vec!["future-event"]);
    }
}
