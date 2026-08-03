use crate::action::Risk;

use super::Timestamp;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PreferenceField {
    AcceptDns,
    AcceptRoutes,
    ShieldsUp,
    Ssh,
    AutomaticUpdate,
    UpdateCheck,
    ReportPosture,
    Hostname,
    Nickname,
    WebClient,
}

impl PreferenceField {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AcceptDns => "accept DNS",
            Self::AcceptRoutes => "accept routes",
            Self::ShieldsUp => "shields up",
            Self::Ssh => "Tailscale SSH",
            Self::AutomaticUpdate => "automatic update",
            Self::UpdateCheck => "update check",
            Self::ReportPosture => "posture reporting",
            Self::Hostname => "hostname",
            Self::Nickname => "nickname",
            Self::WebClient => "web client",
        }
    }

    pub const fn flag(self) -> &'static str {
        match self {
            Self::AcceptDns => "accept-dns",
            Self::AcceptRoutes => "accept-routes",
            Self::ShieldsUp => "shields-up",
            Self::Ssh => "ssh",
            Self::AutomaticUpdate => "auto-update",
            Self::UpdateCheck => "update-check",
            Self::ReportPosture => "report-posture",
            Self::Hostname => "hostname",
            Self::Nickname => "nickname",
            Self::WebClient => "webclient",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PreferenceEditability {
    Editable,
    PolicyManaged,
    PermissionDenied,
    Unsupported,
    Unknown,
}

impl PreferenceEditability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Editable => "editable",
            Self::PolicyManaged => "policy managed",
            Self::PermissionDenied => "permission denied",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }

    pub const fn can_edit(self) -> bool {
        matches!(self, Self::Editable)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PreferenceSource {
    LocalApi,
    Unknown(String),
}

impl PreferenceSource {
    pub const fn label(&self) -> &str {
        match self {
            Self::LocalApi => "LocalAPI GetPrefs",
            Self::Unknown(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ObservedPreference<T> {
    pub value: Option<T>,
    pub editability: PreferenceEditability,
    pub source: PreferenceSource,
    pub observed_at: Timestamp,
}

impl<T> ObservedPreference<T> {
    pub fn known(value: T, observed_at: Timestamp) -> Self {
        Self {
            value: Some(value),
            editability: PreferenceEditability::Editable,
            source: PreferenceSource::LocalApi,
            observed_at,
        }
    }

    pub fn unknown(observed_at: Timestamp) -> Self {
        Self {
            value: None,
            editability: PreferenceEditability::Unknown,
            source: PreferenceSource::LocalApi,
            observed_at,
        }
    }

    pub fn unavailable(observed_at: Timestamp) -> Self {
        Self {
            value: None,
            editability: PreferenceEditability::Unsupported,
            source: PreferenceSource::LocalApi,
            observed_at,
        }
    }

    pub fn with_editability(mut self, editability: PreferenceEditability) -> Self {
        self.editability = editability;
        self
    }

    pub fn can_edit(&self) -> bool {
        self.value.is_some() && self.editability.can_edit()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalPreferences {
    pub want_running: ObservedPreference<bool>,
    pub logged_out: ObservedPreference<bool>,
    pub accept_dns: ObservedPreference<bool>,
    pub accept_routes: ObservedPreference<bool>,
    pub shields_up: ObservedPreference<bool>,
    pub ssh: ObservedPreference<bool>,
    pub update_check: ObservedPreference<bool>,
    pub automatic_update: ObservedPreference<bool>,
    pub report_posture: ObservedPreference<bool>,
    pub hostname: ObservedPreference<String>,
    pub nickname: ObservedPreference<String>,
    pub web_client: ObservedPreference<bool>,
    pub exit_node_id: ObservedPreference<String>,
    pub exit_node_ip: ObservedPreference<String>,
    pub auto_exit_node: ObservedPreference<bool>,
    pub exit_node_allow_lan_access: ObservedPreference<bool>,
    pub advertised_routes: ObservedPreference<Vec<String>>,
    pub advertised_exit_node: ObservedPreference<bool>,
    pub app_connector: ObservedPreference<bool>,
    pub relay_server_port: ObservedPreference<u16>,
    pub relay_server_port_disabled: ObservedPreference<bool>,
    pub relay_server_static_endpoints: ObservedPreference<Vec<String>>,
}

impl LocalPreferences {
    pub fn empty(observed_at: Timestamp) -> Self {
        Self::with_editability(observed_at, PreferenceEditability::Unknown)
    }

    pub fn unavailable(observed_at: Timestamp) -> Self {
        Self::with_editability(observed_at, PreferenceEditability::Unsupported)
    }

    pub fn permission_denied(observed_at: Timestamp) -> Self {
        Self::with_editability(observed_at, PreferenceEditability::PermissionDenied)
    }

    fn with_editability(observed_at: Timestamp, editability: PreferenceEditability) -> Self {
        Self {
            want_running: ObservedPreference::unknown(observed_at).with_editability(editability),
            logged_out: ObservedPreference::unknown(observed_at).with_editability(editability),
            accept_dns: ObservedPreference::unknown(observed_at).with_editability(editability),
            accept_routes: ObservedPreference::unknown(observed_at).with_editability(editability),
            shields_up: ObservedPreference::unknown(observed_at).with_editability(editability),
            ssh: ObservedPreference::unknown(observed_at).with_editability(editability),
            update_check: ObservedPreference::unknown(observed_at).with_editability(editability),
            automatic_update: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
            report_posture: ObservedPreference::unknown(observed_at).with_editability(editability),
            hostname: ObservedPreference::unknown(observed_at).with_editability(editability),
            nickname: ObservedPreference::unknown(observed_at).with_editability(editability),
            web_client: ObservedPreference::unknown(observed_at).with_editability(editability),
            exit_node_id: ObservedPreference::unknown(observed_at).with_editability(editability),
            exit_node_ip: ObservedPreference::unknown(observed_at).with_editability(editability),
            auto_exit_node: ObservedPreference::unknown(observed_at).with_editability(editability),
            exit_node_allow_lan_access: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
            advertised_routes: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
            advertised_exit_node: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
            app_connector: ObservedPreference::unknown(observed_at).with_editability(editability),
            relay_server_port: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
            relay_server_port_disabled: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
            relay_server_static_endpoints: ObservedPreference::unknown(observed_at)
                .with_editability(editability),
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.want_running.value {
            Some(true) => "running intent",
            Some(false) => "stopped intent",
            None => "not returned",
        }
    }

    pub fn selected_exit_label(&self) -> String {
        if self.auto_exit_node.value == Some(true) {
            return "automatic".to_owned();
        }
        let selected = self
            .exit_node_id
            .value
            .as_deref()
            .filter(|value| !value.is_empty())
            .or(self.exit_node_ip.value.as_deref())
            .filter(|value| !value.is_empty());
        if let Some(value) = selected {
            return value.to_owned();
        }
        if self.auto_exit_node.value == Some(false)
            && self
                .exit_node_id
                .value
                .as_deref()
                .is_some_and(str::is_empty)
            && self
                .exit_node_ip
                .value
                .as_deref()
                .is_some_and(str::is_empty)
        {
            return "none".to_owned();
        }
        "not returned".to_owned()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct PreferenceRequest {
    pub accept_dns: Option<bool>,
    pub accept_routes: Option<bool>,
    pub shields_up: Option<bool>,
    pub ssh: Option<bool>,
    pub automatic_update: Option<bool>,
    pub update_check: Option<bool>,
    pub report_posture: Option<bool>,
    pub hostname: Option<String>,
    pub nickname: Option<String>,
    pub web_client: Option<bool>,
}

impl PreferenceRequest {
    pub fn is_empty(&self) -> bool {
        self.changed_fields().is_empty()
    }

    pub fn changed_fields(&self) -> Vec<PreferenceField> {
        let mut fields = Vec::new();
        if self.accept_dns.is_some() {
            fields.push(PreferenceField::AcceptDns);
        }
        if self.accept_routes.is_some() {
            fields.push(PreferenceField::AcceptRoutes);
        }
        if self.shields_up.is_some() {
            fields.push(PreferenceField::ShieldsUp);
        }
        if self.ssh.is_some() {
            fields.push(PreferenceField::Ssh);
        }
        if self.automatic_update.is_some() {
            fields.push(PreferenceField::AutomaticUpdate);
        }
        if self.update_check.is_some() {
            fields.push(PreferenceField::UpdateCheck);
        }
        if self.report_posture.is_some() {
            fields.push(PreferenceField::ReportPosture);
        }
        if self.hostname.is_some() {
            fields.push(PreferenceField::Hostname);
        }
        if self.nickname.is_some() {
            fields.push(PreferenceField::Nickname);
        }
        if self.web_client.is_some() {
            fields.push(PreferenceField::WebClient);
        }
        fields
    }

    pub fn risk(&self) -> Risk {
        Risk::Reversible
    }
}
