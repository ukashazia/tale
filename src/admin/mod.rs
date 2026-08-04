pub mod access_explorer;
pub mod audit;
pub mod auth;
pub mod client;
pub mod credentials;
pub mod device_mutations;
pub mod devices;
pub mod dns;
pub mod dns_mutations;
pub mod dto;
pub mod flow_logs;
pub mod key_mutations;
pub mod log_streaming;
pub mod mutation;
pub mod policy;
pub mod policy_mutations;
pub mod route_mutations;
pub mod routes;
pub mod user_mutations;
pub mod users;
pub mod webhooks;

use std::collections::{BTreeMap, BTreeSet};

use crate::admin::client::AdminError;
use crate::admin::dto::{ContactDto, ContactsResponse, SettingsDto};
use crate::domain::activity::AuditSnapshot;
use crate::domain::credential::CredentialSnapshot;
use crate::domain::device::AdminDevice;
use crate::domain::dns::{AdminDnsPreferences, AdminNameservers, AdminSearchPaths, AdminSplitDns};
use crate::domain::flow::{FlowSnapshot, FlowWindow};
use crate::domain::log_stream::{LogStreamConfiguration, LogStreamStatus, LogType};
use crate::domain::policy::PolicySnapshot;
use crate::domain::user::AdminUser;
use crate::domain::{SourceHealth, Timestamp};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AdminResourceState {
    Idle,
    Loading,
    Ready,
    Stale,
    Forbidden,
    PlanRestricted,
    Unsupported,
    Unauthenticated,
    Failed,
}

impl AdminResourceState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "fresh",
            Self::Stale => "stale",
            Self::Forbidden => "forbidden",
            Self::PlanRestricted => "plan restricted",
            Self::Unsupported => "unsupported",
            Self::Unauthenticated => "unauthenticated",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdminResource<T> {
    pub profile: Option<String>,
    pub generation: u64,
    pub state: AdminResourceState,
    pub last_failure: Option<AdminResourceState>,
    pub snapshot: Option<T>,
    pub observed_at: Option<Timestamp>,
    pub error: Option<String>,
}

impl<T> AdminResource<T> {
    pub fn new(profile: Option<String>) -> Self {
        Self {
            profile,
            generation: 0,
            state: AdminResourceState::Idle,
            last_failure: None,
            snapshot: None,
            observed_at: None,
            error: None,
        }
    }

    pub fn begin(&mut self, generation: u64) {
        self.generation = generation;
        self.state = AdminResourceState::Loading;
        self.error = None;
    }

    pub fn succeed(&mut self, generation: u64, snapshot: T, observed_at: Timestamp) {
        if generation != self.generation {
            return;
        }
        self.snapshot = Some(snapshot);
        self.observed_at = Some(observed_at);
        self.state = AdminResourceState::Ready;
        self.last_failure = None;
        self.error = None;
    }

    pub fn fail(&mut self, generation: u64, state: AdminResourceState, detail: String) {
        if generation != self.generation {
            return;
        }
        self.last_failure = Some(state);
        if self.snapshot.is_some() {
            self.state = AdminResourceState::Stale;
        } else {
            self.state = state;
        }
        self.error = Some(detail);
    }

    pub fn clear_profile(&mut self, profile: Option<String>) {
        self.profile = profile;
        self.generation = self.generation.saturating_add(1);
        self.state = AdminResourceState::Idle;
        self.last_failure = None;
        self.snapshot = None;
        self.observed_at = None;
        self.error = None;
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminSettings {
    pub acls_externally_managed_on: Option<bool>,
    pub acls_external_link: Option<String>,
    pub devices_approval_on: Option<bool>,
    pub devices_auto_updates_on: Option<bool>,
    pub devices_key_duration_days: Option<i64>,
    pub users_approval_on: Option<bool>,
    pub network_flow_logging_on: Option<bool>,
    pub regional_routing_on: Option<bool>,
    pub posture_identity_collection_on: Option<bool>,
    pub https_enabled: Option<bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminContact {
    pub email: Option<String>,
    pub fallback_email: Option<String>,
    pub needs_verification: Option<bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminContacts {
    pub account: Option<AdminContact>,
    pub support: Option<AdminContact>,
    pub security: Option<AdminContact>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CapabilityState {
    Configured,
    Available,
    Forbidden,
    PlanRestricted,
    Unsupported,
    Unauthenticated,
    Failed,
}

impl CapabilityState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Available => "available",
            Self::Forbidden => "forbidden",
            Self::PlanRestricted => "plan restricted",
            Self::Unsupported => "unsupported",
            Self::Unauthenticated => "unauthenticated",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RouteFinding {
    pub device_id: String,
    pub route: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct OverviewQueues {
    pub devices_awaiting_approval: Vec<String>,
    pub users_awaiting_approval: Vec<String>,
    pub expired_device_keys: Vec<String>,
    pub soon_expiring_device_keys: Vec<String>,
    pub unapproved_routes: Vec<RouteFinding>,
    pub resource_problems: Vec<String>,
    pub client_versions: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct AdminSnapshot {
    pub profile: Option<String>,
    pub tailnet: Option<String>,
    pub profile_read_only: bool,
    pub requested_scopes: Vec<String>,
    pub capabilities: BTreeMap<String, CapabilityState>,
    pub devices: AdminResource<Vec<AdminDevice>>,
    pub users: AdminResource<Vec<AdminUser>>,
    pub routes: AdminResource<Vec<routes::AdminRouteObservation>>,
    pub posture: AdminResource<()>,
    pub nameservers: AdminResource<AdminNameservers>,
    pub dns_preferences: AdminResource<AdminDnsPreferences>,
    pub search_paths: AdminResource<AdminSearchPaths>,
    pub split_dns: AdminResource<AdminSplitDns>,
    pub policy: AdminResource<PolicySnapshot>,
    pub credentials: AdminResource<CredentialSnapshot>,
    pub settings: AdminResource<AdminSettings>,
    pub contacts: AdminResource<AdminContacts>,
    pub activity: AdminResource<AuditSnapshot>,
}

#[derive(Debug, Clone)]
pub struct AdminRefreshReport {
    pub profile: String,
    pub generation: u64,
    pub observed_at: Timestamp,
    pub requested_scopes: Vec<String>,
    pub devices: Result<Vec<AdminDevice>, AdminError>,
    pub users: Result<Vec<AdminUser>, AdminError>,
    pub routes: Option<Result<Vec<routes::AdminRouteObservation>, AdminError>>,
    pub nameservers: Result<AdminNameservers, AdminError>,
    pub dns_preferences: Result<AdminDnsPreferences, AdminError>,
    pub search_paths: Result<AdminSearchPaths, AdminError>,
    pub split_dns: Result<AdminSplitDns, AdminError>,
    pub policy: Result<PolicySnapshot, AdminError>,
    pub credentials: Result<CredentialSnapshot, AdminError>,
    pub settings: Result<AdminSettings, AdminError>,
    pub contacts: Result<AdminContacts, AdminError>,
    pub activity: Result<AuditSnapshot, AdminError>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AdminRefreshResource {
    Devices,
    DeviceRoutes(String),
    Users,
    Nameservers,
    DnsPreferences,
    SearchPaths,
    SplitDns,
    Policy,
    Credentials,
    Settings,
    Contacts,
    Activity,
    FlowLogs(FlowWindow),
    Webhooks,
    LogStreamConfiguration(LogType),
    LogStreamStatus(LogType),
    NetworkLogSettings,
}

#[derive(Debug, Clone)]
pub enum AdminResourceResult {
    Devices(Result<Vec<AdminDevice>, AdminError>),
    DeviceRoutes(Result<routes::AdminRouteObservation, AdminError>),
    Users(Result<Vec<AdminUser>, AdminError>),
    Nameservers(Result<AdminNameservers, AdminError>),
    DnsPreferences(Result<AdminDnsPreferences, AdminError>),
    SearchPaths(Result<AdminSearchPaths, AdminError>),
    SplitDns(Result<AdminSplitDns, AdminError>),
    Policy(Result<PolicySnapshot, AdminError>),
    Credentials(Result<CredentialSnapshot, AdminError>),
    Settings(Result<AdminSettings, AdminError>),
    Contacts(Result<AdminContacts, AdminError>),
    Activity(Result<AuditSnapshot, AdminError>),
    FlowLogs(Box<Result<FlowSnapshot, AdminError>>),
    Webhooks(
        Result<
            (
                Vec<crate::domain::webhook::WebhookEndpoint>,
                crate::admin::client::ResponseMeta,
            ),
            AdminError,
        >,
    ),
    LogStreamConfiguration {
        log_type: LogType,
        result: Result<LogStreamConfiguration, AdminError>,
    },
    LogStreamStatus {
        log_type: LogType,
        result: Result<LogStreamStatus, AdminError>,
    },
    NetworkLogSettings(Result<AdminSettings, AdminError>),
}

#[derive(Debug, Clone)]
pub struct AdminResourceReport {
    pub profile: String,
    pub generation: u64,
    pub observed_at: Timestamp,
    pub requested_scopes: Vec<String>,
    pub resources: Vec<AdminResourceResult>,
}

pub fn decode_settings(dto: SettingsDto) -> AdminSettings {
    AdminSettings {
        acls_externally_managed_on: dto.acls_externally_managed_on,
        acls_external_link: dto.acls_external_link,
        devices_approval_on: dto.devices_approval_on,
        devices_auto_updates_on: dto.devices_auto_updates_on,
        devices_key_duration_days: dto.devices_key_duration_days,
        users_approval_on: dto.users_approval_on,
        network_flow_logging_on: dto.network_flow_logging_on,
        regional_routing_on: dto.regional_routing_on,
        posture_identity_collection_on: dto.posture_identity_collection_on,
        https_enabled: dto.https_enabled,
    }
}

pub fn decode_contacts(dto: ContactsResponse) -> AdminContacts {
    AdminContacts {
        account: dto.account.map(decode_contact),
        support: dto.support.map(decode_contact),
        security: dto.security.map(decode_contact),
    }
}

fn decode_contact(dto: ContactDto) -> AdminContact {
    AdminContact {
        email: dto.email,
        fallback_email: dto.fallback_email,
        needs_verification: dto.needs_verification,
    }
}

impl AdminSnapshot {
    pub fn new(
        profile: Option<String>,
        tailnet: Option<String>,
        profile_read_only: bool,
        requested_scopes: Vec<String>,
    ) -> Self {
        let resource_profile = profile.clone();
        Self {
            profile,
            tailnet,
            profile_read_only,
            requested_scopes,
            capabilities: BTreeMap::new(),
            devices: AdminResource::new(resource_profile.clone()),
            users: AdminResource::new(resource_profile.clone()),
            routes: AdminResource::new(resource_profile.clone()),
            posture: AdminResource::new(resource_profile.clone()),
            nameservers: AdminResource::new(resource_profile.clone()),
            dns_preferences: AdminResource::new(resource_profile.clone()),
            search_paths: AdminResource::new(resource_profile.clone()),
            split_dns: AdminResource::new(resource_profile.clone()),
            policy: AdminResource::new(resource_profile.clone()),
            credentials: AdminResource::new(resource_profile.clone()),
            settings: AdminResource::new(resource_profile.clone()),
            contacts: AdminResource::new(resource_profile.clone()),
            activity: AdminResource::new(resource_profile),
        }
    }

    pub fn clear_profile(&mut self, profile: Option<String>, tailnet: Option<String>) {
        self.profile = profile.clone();
        self.tailnet = tailnet;
        self.capabilities.clear();
        self.devices.clear_profile(profile.clone());
        self.users.clear_profile(profile.clone());
        self.routes.clear_profile(profile.clone());
        self.posture.clear_profile(profile.clone());
        self.nameservers.clear_profile(profile.clone());
        self.dns_preferences.clear_profile(profile.clone());
        self.search_paths.clear_profile(profile.clone());
        self.split_dns.clear_profile(profile.clone());
        self.policy.clear_profile(profile.clone());
        self.credentials.clear_profile(profile.clone());
        self.settings.clear_profile(profile.clone());
        self.contacts.clear_profile(profile.clone());
        self.activity.clear_profile(profile);
    }

    pub fn overview_queues(&self, now: Timestamp) -> OverviewQueues {
        let mut queues = OverviewQueues {
            devices_awaiting_approval: Vec::new(),
            users_awaiting_approval: Vec::new(),
            expired_device_keys: Vec::new(),
            soon_expiring_device_keys: Vec::new(),
            unapproved_routes: Vec::new(),
            resource_problems: Vec::new(),
            client_versions: BTreeMap::new(),
        };
        if let Some(devices) = self.devices.snapshot.as_ref() {
            for device in devices {
                let label = device.display_name().to_owned();
                if device.authorized == Some(false) {
                    queues.devices_awaiting_approval.push(label.clone());
                }
                if let Some(expiry) = device.expires_at {
                    if expiry <= now {
                        queues.expired_device_keys.push(label.clone());
                    } else if expiry <= now.saturating_add(7 * 24 * 60 * 60) {
                        queues.soon_expiring_device_keys.push(label.clone());
                    }
                }
                if let Some(version) = device.client_version.as_ref() {
                    let count = queues.client_versions.entry(version.clone()).or_insert(0);
                    *count = count.saturating_add(1);
                }
            }
        }
        if let Some(users) = self.users.snapshot.as_ref() {
            for user in users {
                if user.status.as_deref().is_some_and(|status| {
                    status.eq_ignore_ascii_case("pending")
                        || status.eq_ignore_ascii_case("needs_approval")
                }) {
                    queues.users_awaiting_approval.push(user.label().to_owned());
                }
            }
        }
        if let Some(routes) = self.routes.snapshot.as_ref() {
            for observation in routes {
                if observation.complete {
                    append_unapproved_routes(
                        &mut queues.unapproved_routes,
                        &observation.device_id,
                        &observation.advertised,
                        &observation.enabled,
                    );
                }
            }
        } else if let Some(devices) = self.devices.snapshot.as_ref() {
            for device in devices {
                if device.advertised_routes_returned && device.enabled_routes_returned {
                    append_unapproved_routes(
                        &mut queues.unapproved_routes,
                        &device.stable_id,
                        &device.advertised_routes,
                        &device.enabled_routes,
                    );
                }
            }
        }
        for (name, resource) in self.resource_entries() {
            if matches!(
                resource,
                AdminResourceState::Forbidden
                    | AdminResourceState::PlanRestricted
                    | AdminResourceState::Unsupported
                    | AdminResourceState::Unauthenticated
                    | AdminResourceState::Failed
                    | AdminResourceState::Stale
            ) {
                queues
                    .resource_problems
                    .push(format!("{name}: {}", resource.label()));
            }
        }
        queues
    }

    pub fn route_observations(&self) -> Vec<routes::AdminRouteObservation> {
        match self.routes.snapshot.as_ref() {
            Some(routes) => routes.clone(),
            None => self
                .devices
                .snapshot
                .as_ref()
                .map_or_else(Vec::new, |devices| {
                    devices.iter().filter_map(routes::from_device).collect()
                }),
        }
    }

    fn resource_entries(&self) -> Vec<(&'static str, AdminResourceState)> {
        vec![
            ("devices", self.devices.state),
            ("users", self.users.state),
            ("routes", self.routes.state),
            ("devices.posture", self.posture.state),
            ("dns.nameservers", self.nameservers.state),
            ("dns.preferences", self.dns_preferences.state),
            ("dns.searchpaths", self.search_paths.state),
            ("dns.split", self.split_dns.state),
            ("access", self.policy.state),
            ("credentials", self.credentials.state),
            ("settings", self.settings.state),
            ("contacts", self.contacts.state),
            ("activity", self.activity.state),
        ]
    }
}

fn append_unapproved_routes(
    findings: &mut Vec<RouteFinding>,
    device_id: &str,
    advertised: &[String],
    enabled: &[String],
) {
    let enabled: BTreeSet<&str> = enabled.iter().map(String::as_str).collect();
    for route in advertised {
        if !enabled.contains(route.as_str()) {
            findings.push(RouteFinding {
                device_id: device_id.to_owned(),
                route: route.clone(),
            });
        }
    }
}

impl SourceHealth {
    pub const fn from_admin_state(state: AdminResourceState) -> Self {
        match state {
            AdminResourceState::Idle => Self::Unavailable,
            AdminResourceState::Loading => Self::Loading,
            AdminResourceState::Ready => Self::Healthy,
            AdminResourceState::Stale => Self::Stale,
            AdminResourceState::Forbidden
            | AdminResourceState::PlanRestricted
            | AdminResourceState::Unsupported
            | AdminResourceState::Unauthenticated
            | AdminResourceState::Failed => Self::Error,
        }
    }
}
