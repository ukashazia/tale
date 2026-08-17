//! Typed one-shot operational requests.
//!
//! This enum groups payloads that share confirmation and dispatch plumbing. It
//! intentionally has no lifecycle of its own: local saved-view/export work is
//! synchronous, while remote operations are tracked by their effect result.

use std::path::PathBuf;
use std::sync::Arc;

use super::export::ExportCollection;
use super::log_stream::{LogType, SecretAction};
use super::saved_view::SavedView;
use super::secret_result::SecretBuffer;
use super::webhook::WebhookMutation;

/// The non-generic, typed payloads used by operational forms.
///
/// Secret values are held only by an ephemeral reference-counted buffer while a
/// confirmation or request is alive. They are never part of a preview or task
/// description.
#[derive(Clone)]
pub struct LogStreamMutationDraft {
    pub log_type: LogType,
    pub destination_type: String,
    pub url: String,
    pub user: Option<String>,
    pub upload_period_minutes: Option<u64>,
    pub compression_format: Option<String>,
    pub token: Option<Arc<SecretBuffer>>,
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub s3_key_prefix: Option<String>,
    pub s3_authentication_type: Option<String>,
    pub s3_access_key_id: Option<String>,
    pub s3_role_arn: Option<String>,
    pub gcs_bucket: Option<String>,
    pub gcs_key_prefix: Option<String>,
    pub gcs_scopes: Vec<String>,
    pub gcs_credentials: Option<Arc<SecretBuffer>>,
    pub secret_action: SecretAction,
}

impl std::fmt::Debug for LogStreamMutationDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LogStreamMutationDraft")
            .field(&self.preview())
            .finish()
    }
}

impl LogStreamMutationDraft {
    pub fn preview(&self) -> String {
        let private_note = if self.url.starts_with("http://") {
            " private-endpoint prerequisites remain server-managed (device_invites and policy_file); Tale does not edit policy"
        } else {
            ""
        };
        format!(
            "Replace {} log stream: destination={} identity={} user={} period={} compression={} s3_bucket={} s3_region={} s3_auth={} s3_access_key={} s3_role={} gcs_bucket={} gcs_prefix={} gcs_scopes={} secret={}{}",
            self.log_type.wire_value(),
            self.destination_type,
            redact_identity(&self.url),
            self.user.as_deref().map_or("unchanged", |_| "provided"),
            self.upload_period_minutes
                .map_or_else(|| "unchanged".to_owned(), |value| value.to_string()),
            self.compression_format
                .as_deref()
                .map_or("unchanged", |value| value),
            self.s3_bucket.as_deref().map_or("unchanged", |value| value),
            self.s3_region.as_deref().map_or("unchanged", |value| value),
            self.s3_authentication_type
                .as_deref()
                .map_or("unchanged", |value| value),
            self.s3_access_key_id
                .as_deref()
                .map_or("unchanged", |value| value),
            self.s3_role_arn
                .as_deref()
                .map_or("unchanged", |value| value),
            self.gcs_bucket
                .as_deref()
                .map_or("unchanged", |value| value),
            self.gcs_key_prefix
                .as_deref()
                .map_or("unchanged", |value| value),
            if self.gcs_scopes.is_empty() {
                "unchanged".to_owned()
            } else {
                self.gcs_scopes.join(",")
            },
            self.secret_action.label(),
            private_note,
        )
    }
}

#[derive(Debug, Clone)]
pub enum OperationalMutation {
    Webhook(WebhookMutation),
    LogStreamReplace(LogStreamMutationDraft),
    LogStreamDelete(LogType),
    NetworkLogSetting { enabled: bool },
    SavedView(SavedViewMutation),
    Export(ExportRequest),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SavedViewMutation {
    Create(SavedView),
    Replace { name: String, view: SavedView },
    Rename { name: String, replacement: String },
    Delete { name: String },
    Apply { name: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExportRequest {
    pub collection: ExportCollection,
    pub format: String,
    pub path: PathBuf,
}

impl PartialEq for OperationalMutation {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Webhook(left), Self::Webhook(right)) => left == right,
            (Self::LogStreamReplace(left), Self::LogStreamReplace(right)) => {
                left.log_type == right.log_type
                    && left.destination_type == right.destination_type
                    && left.url == right.url
                    && left.user == right.user
                    && left.upload_period_minutes == right.upload_period_minutes
                    && left.compression_format == right.compression_format
                    && left.s3_bucket == right.s3_bucket
                    && left.s3_region == right.s3_region
                    && left.s3_key_prefix == right.s3_key_prefix
                    && left.s3_authentication_type == right.s3_authentication_type
                    && left.s3_access_key_id == right.s3_access_key_id
                    && left.s3_role_arn == right.s3_role_arn
                    && left.gcs_bucket == right.gcs_bucket
                    && left.gcs_key_prefix == right.gcs_key_prefix
                    && left.gcs_scopes == right.gcs_scopes
                    && left.secret_action == right.secret_action
                    && secret_equal(left.token.as_ref(), right.token.as_ref())
                    && secret_equal(
                        left.gcs_credentials.as_ref(),
                        right.gcs_credentials.as_ref(),
                    )
            }
            (Self::LogStreamDelete(left), Self::LogStreamDelete(right)) => left == right,
            (
                Self::NetworkLogSetting { enabled: left },
                Self::NetworkLogSetting { enabled: right },
            ) => left == right,
            (Self::SavedView(left), Self::SavedView(right)) => left == right,
            (Self::Export(left), Self::Export(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for OperationalMutation {}

fn secret_equal(left: Option<&Arc<SecretBuffer>>, right: Option<&Arc<SecretBuffer>>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.as_bytes() == right.as_bytes(),
        _ => false,
    }
}

impl OperationalMutation {
    pub fn preview(&self) -> String {
        match self {
            Self::Webhook(mutation) => mutation.preview(),
            Self::LogStreamReplace(draft) => draft.preview(),
            Self::LogStreamDelete(log_type) => {
                format!(
                    "Delete the {} log-stream configuration",
                    log_type.wire_value()
                )
            }
            Self::NetworkLogSetting { enabled } => format!(
                "Set documented network-flow collection setting to {}",
                if *enabled { "enabled" } else { "disabled" }
            ),
            Self::SavedView(mutation) => match mutation {
                SavedViewMutation::Create(view) => {
                    format!("Create saved view {} for route {}", view.name, view.route)
                }
                SavedViewMutation::Replace { name, view } => {
                    format!("Replace saved view {name} with route {}", view.route)
                }
                SavedViewMutation::Rename { name, replacement } => {
                    format!("Rename saved view {name} to {replacement}")
                }
                SavedViewMutation::Delete { name } => format!("Delete saved view {name}"),
                SavedViewMutation::Apply { name } => format!("Apply saved view {name}"),
            },
            Self::Export(request) => format!(
                "Export {} as {} to {}",
                request.collection.schema_name(),
                request.format,
                request.path.display()
            ),
        }
    }
}

fn redact_identity(value: &str) -> String {
    super::redaction::redact_destination_url(value)
}
