use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::Timestamp;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DeviceId(pub String);

impl DeviceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Liveness {
    Online,
    Offline,
    Unknown,
}

impl Liveness {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Unknown => "unknown",
        }
    }

    pub const fn marker(self) -> &'static str {
        match self {
            Self::Online => "*",
            Self::Offline => "o",
            Self::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ConnectionPath {
    Direct { latency_ms: Option<u16> },
    Derp { region: String },
    PeerRelay { peer: String },
    Idle,
    Unknown(String),
    NoPath,
}

impl ConnectionPath {
    pub fn label(&self) -> &str {
        match self {
            Self::Direct { .. } => "direct",
            Self::Derp { .. } => "derp",
            Self::PeerRelay { .. } => "peer-relay",
            Self::Idle => "idle",
            Self::Unknown(_) => "unknown",
            Self::NoPath => "no-path",
        }
    }

    /// What is carrying the traffic, rather than what kind of path it is: a
    /// direct connection has no relay to name, so it names itself. `label` is
    /// what the `path:` filter still matches on.
    pub fn relay_label(&self) -> &str {
        match self {
            Self::Direct { .. } => "direct",
            Self::Derp { region } => region,
            Self::PeerRelay { peer } => peer,
            Self::Idle | Self::Unknown(_) | Self::NoPath => "-",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum OperatingSystem {
    Linux,
    MacOS,
    Windows,
    IOS,
    Android,
    Unknown(String),
}

impl OperatingSystem {
    pub fn label(&self) -> &str {
        match self {
            Self::Linux => "linux",
            Self::MacOS => "macos",
            Self::Windows => "windows",
            Self::IOS => "ios",
            Self::Android => "android",
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeviceCapabilities {
    pub exit_node: bool,
    pub exit_node_option: bool,
    pub subnet_router: bool,
    pub ssh: bool,
    pub funnel: bool,
    pub shared: bool,
    pub expired: bool,
    pub approved: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Device {
    pub id: DeviceId,
    pub display_name: String,
    pub hostname: String,
    pub owner: Option<String>,
    pub owner_label: Option<String>,
    pub os: OperatingSystem,
    pub version: Option<String>,
    pub liveness: Liveness,
    pub path: ConnectionPath,
    pub addresses: Vec<String>,
    pub advertised_routes: Vec<String>,
    pub tags: Vec<String>,
    pub last_seen: Option<Timestamp>,
    pub created_at: Option<Timestamp>,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    pub capabilities: DeviceCapabilities,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalDevice {
    pub id: DeviceId,
    pub public_key: Option<String>,
    pub display_name: String,
    pub hostname: String,
    pub dns_name: Option<String>,
    pub os: OperatingSystem,
    pub version: Option<String>,
    pub owner_label: Option<String>,
    pub user_id: Option<String>,
    pub tags: Vec<String>,
    pub tailscale_ips: Vec<String>,
    pub advertised_routes: Vec<String>,
    pub current_endpoint: Option<String>,
    pub relay_region: Option<String>,
    pub path: ConnectionPath,
    pub online: Option<bool>,
    pub active: bool,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    pub created_at: Option<Timestamp>,
    pub last_seen: Option<Timestamp>,
    pub last_handshake: Option<Timestamp>,
    pub exit_node: bool,
    pub exit_node_option: bool,
    pub ssh_host_keys_present: bool,
    pub shared: bool,
    pub capabilities: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminDevice {
    pub stable_id: String,
    pub legacy_id: Option<String>,
    pub node_id: Option<String>,
    pub addresses: Vec<String>,
    pub user_id: Option<String>,
    pub name: Option<String>,
    pub hostname: Option<String>,
    pub client_version: Option<String>,
    pub update_available: Option<bool>,
    pub os: Option<OperatingSystem>,
    pub created_at: Option<Timestamp>,
    pub connected_to_control: Option<bool>,
    pub last_seen: Option<Timestamp>,
    pub key_expiry_disabled: Option<bool>,
    pub expires_at: Option<Timestamp>,
    pub authorized: Option<bool>,
    pub is_external: Option<bool>,
    pub multiple_connections: Option<bool>,
    pub advertised_routes_returned: bool,
    pub advertised_routes: Vec<String>,
    pub enabled_routes_returned: bool,
    pub enabled_routes: Vec<String>,
    pub tags: Vec<String>,
    pub is_ephemeral: Option<bool>,
    pub ssh_enabled: Option<bool>,
    pub posture_present: Option<bool>,
    pub source_observed_at: Timestamp,
}

impl AdminDevice {
    pub fn display_name(&self) -> &str {
        match (self.name.as_deref(), self.hostname.as_deref()) {
            (Some(name), _) => magic_dns_device_name(name),
            (None, Some(hostname)) => hostname,
            (None, None) => &self.stable_id,
        }
    }

    pub fn exact_node_id(&self) -> Option<&str> {
        self.node_id.as_deref().or(self.legacy_id.as_deref())
    }

    /// The tailnet this device's name places it in. A node shared in from
    /// another tailnet carries that tailnet's suffix, so it is excluded rather
    /// than allowed to speak for the tailnet being read.
    pub fn tailnet_suffix(&self) -> Option<&str> {
        if self.is_external == Some(true) {
            return None;
        }
        magic_dns_suffix(self.name.as_deref()?)
    }

    pub fn to_display_device(&self) -> Device {
        let os = match self.os.clone() {
            Some(os) => os,
            None => OperatingSystem::Unknown("not returned".to_owned()),
        };
        Device {
            id: DeviceId::new(self.stable_id.clone()),
            display_name: self.display_name().to_owned(),
            hostname: match self.hostname.clone() {
                Some(hostname) => hostname,
                None => self.display_name().to_owned(),
            },
            owner: self.user_id.clone(),
            owner_label: self.user_id.clone(),
            os,
            version: self.client_version.clone(),
            liveness: match self.connected_to_control {
                Some(true) => Liveness::Online,
                Some(false) => Liveness::Offline,
                None => Liveness::Unknown,
            },
            path: ConnectionPath::Unknown("admin observation".to_owned()),
            addresses: self.addresses.clone(),
            advertised_routes: self.advertised_routes.clone(),
            tags: self.tags.clone(),
            last_seen: self.last_seen,
            created_at: self.created_at,
            rx_bytes: None,
            tx_bytes: None,
            capabilities: DeviceCapabilities {
                exit_node: self
                    .advertised_routes
                    .iter()
                    .any(|route| route == "0.0.0.0/0" || route == "::/0"),
                exit_node_option: self
                    .enabled_routes
                    .iter()
                    .any(|route| route == "0.0.0.0/0" || route == "::/0"),
                subnet_router: self.advertised_routes_returned
                    && !self.advertised_routes.is_empty(),
                ssh: self.ssh_enabled.is_some_and(|value| value),
                funnel: false,
                shared: self.is_external.is_some_and(|value| value),
                expired: self
                    .expires_at
                    .is_some_and(|expiry| expiry <= self.source_observed_at),
                approved: self.authorized.is_none_or(|value| value),
            },
        }
    }
}

/// The device name carried by a full MagicDNS name. This is intentionally not
/// the OS hostname: the two can differ after a device has been renamed.
fn magic_dns_device_name(name: &str) -> &str {
    name.split_once('.')
        .map_or(name, |(device_name, _)| device_name)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ComposedDevice {
    pub id: String,
    pub local: Option<LocalDevice>,
    pub admin: Option<AdminDevice>,
}

/// Everything after the first label of a fully-qualified device name, which is
/// the tailnet's MagicDNS suffix. `None` when the name carries no suffix to
/// compare, so an unqualified name never reads as agreement.
pub fn magic_dns_suffix(name: &str) -> Option<&str> {
    let suffix = name.trim_end_matches('.').split_once('.')?.1;
    (!suffix.is_empty()).then_some(suffix)
}

/// Whether two MagicDNS suffixes name the same tailnet. DNS is case-insensitive
/// and the trailing root dot is optional, so neither may decide the answer.
pub fn same_tailnet(left: &str, right: &str) -> bool {
    let normalize = |value: &str| value.trim_end_matches('.').to_ascii_lowercase();
    !left.is_empty() && normalize(left) == normalize(right)
}

pub fn compose_exact_id(local: &[LocalDevice], admin: &[AdminDevice]) -> Vec<ComposedDevice> {
    let mut composed = Vec::with_capacity(local.len().saturating_add(admin.len()));
    let mut used_admin = std::collections::BTreeSet::new();
    for local_device in local {
        let local_id = local_device.id.0.as_str();
        let matching = admin.iter().enumerate().find(|(index, device)| {
            !used_admin.contains(index) && device.exact_node_id() == Some(local_id)
        });
        if let Some((index, device)) = matching {
            used_admin.insert(index);
            composed.push(ComposedDevice {
                id: local_id.to_owned(),
                local: Some(local_device.clone()),
                admin: Some(device.clone()),
            });
        } else {
            composed.push(ComposedDevice {
                id: local_id.to_owned(),
                local: Some(local_device.clone()),
                admin: None,
            });
        }
    }
    for (index, device) in admin.iter().enumerate() {
        if !used_admin.contains(&index) {
            composed.push(ComposedDevice {
                id: device.stable_id.clone(),
                local: None,
                admin: Some(device.clone()),
            });
        }
    }
    composed
}

impl LocalDevice {
    pub fn liveness(&self) -> Liveness {
        match self.online {
            Some(true) => Liveness::Online,
            Some(false) => Liveness::Offline,
            None => Liveness::Unknown,
        }
    }

    pub fn preferred_target(&self) -> Option<&str> {
        self.dns_name
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| self.tailscale_ips.first().map(String::as_str))
    }

    pub fn to_display_device(&self) -> Device {
        let capabilities = DeviceCapabilities {
            exit_node: self.exit_node,
            exit_node_option: self.exit_node_option,
            subnet_router: !self.advertised_routes.is_empty(),
            ssh: self.ssh_host_keys_present || capability_is_true(&self.capabilities, "ssh"),
            funnel: capability_is_true(&self.capabilities, "funnel"),
            shared: self.shared,
            expired: capability_is_true(&self.capabilities, "expired"),
            approved: !matches!(self.capabilities.get("approved"), Some(false)),
        };
        Device {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            hostname: self.hostname.clone(),
            owner: None,
            owner_label: self.owner_label.clone(),
            os: self.os.clone(),
            version: self.version.clone(),
            liveness: self.liveness(),
            path: self.path.clone(),
            addresses: self.tailscale_ips.clone(),
            advertised_routes: self.advertised_routes.clone(),
            tags: self.tags.clone(),
            last_seen: self.last_seen,
            created_at: self.created_at,
            rx_bytes: self.rx_bytes,
            tx_bytes: self.tx_bytes,
            capabilities,
        }
    }
}

fn capability_is_true(capabilities: &BTreeMap<String, bool>, name: &str) -> bool {
    capabilities.get(name).copied().is_some_and(|value| value)
}

impl Device {
    /// Identity fields a bare filter word searches, kept separate so a loose
    /// match cannot span two unrelated values.
    pub fn search_fields(&self) -> Vec<&str> {
        let mut fields = vec![self.display_name.as_str(), self.hostname.as_str()];
        if let Some(owner) = self.owner.as_deref() {
            fields.push(owner);
        }
        if let Some(owner_label) = self.owner_label.as_deref() {
            fields.push(owner_label);
        }
        fields.extend(self.tags.iter().map(String::as_str));
        fields.extend(self.addresses.iter().map(String::as_str));
        fields
    }

    pub fn property_matches(&self, value: &str) -> bool {
        match value.to_ascii_lowercase().as_str() {
            "exit-node" => self.capabilities.exit_node,
            "exit-node-option" => self.capabilities.exit_node_option,
            "subnet-router" => self.capabilities.subnet_router,
            "ssh" => self.capabilities.ssh,
            "shared" => self.capabilities.shared,
            _ => false,
        }
    }

    pub fn age_at(&self, now: Timestamp) -> Option<u64> {
        self.last_seen
            .map(|last_seen| now.saturating_sub(last_seen))
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SortField {
    Name,
    Liveness,
    Owner,
    Os,
    Path,
    LastSeen,
    Rx,
    Tx,
    DeviceId,
    Version,
}

impl SortField {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Liveness => "state",
            Self::Owner => "owner",
            Self::Os => "os",
            Self::Path => "path",
            Self::LastSeen => "lastSeen",
            Self::Rx => "rx",
            Self::Tx => "tx",
            Self::DeviceId => "id",
            Self::Version => "version",
        }
    }

    /// Wording for the interface. `label` stays the stored spelling used by
    /// saved views and exports.
    pub const fn display_label(self) -> &'static str {
        match self {
            Self::LastSeen => "last seen",
            Self::Rx => "received",
            Self::Tx => "transmitted",
            other => other.label(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ascending => "asc",
            Self::Descending => "desc",
        }
    }

    pub const fn is_ascending(self) -> bool {
        matches!(self, Self::Ascending)
    }

    pub const fn reverse(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::LastSeen,
            direction: SortDirection::Descending,
        }
    }
}

pub fn compare_devices(left: &Device, right: &Device, sort: SortSpec, now: Timestamp) -> Ordering {
    compare_devices_by_specs(left, right, &[sort], now)
}

pub fn compare_devices_by_specs(
    left: &Device,
    right: &Device,
    sorts: &[SortSpec],
    now: Timestamp,
) -> Ordering {
    for sort in sorts {
        let primary = compare_device_field(left, right, sort.field, now);
        let directed = match sort.direction {
            SortDirection::Ascending => primary,
            SortDirection::Descending => primary.reverse(),
        };
        if directed != Ordering::Equal {
            return directed;
        }
    }
    left.id.cmp(&right.id)
}

fn compare_device_field(
    left: &Device,
    right: &Device,
    field: SortField,
    now: Timestamp,
) -> Ordering {
    match field {
        SortField::Name => left
            .display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase()),
        SortField::Liveness => liveness_rank(left.liveness).cmp(&liveness_rank(right.liveness)),
        SortField::Owner => compare_optional_text(
            left.owner.as_deref().or(left.owner_label.as_deref()),
            right.owner.as_deref().or(right.owner_label.as_deref()),
        ),
        SortField::Os => left.os.label().cmp(right.os.label()),
        SortField::Path => left.path.label().cmp(right.path.label()),
        SortField::LastSeen => compare_optional(left.age_at(now), right.age_at(now)),
        SortField::Rx => compare_optional(left.rx_bytes, right.rx_bytes),
        SortField::Tx => compare_optional(left.tx_bytes, right.tx_bytes),
        SortField::DeviceId => left.id.cmp(&right.id),
        SortField::Version => left.version.cmp(&right.version),
    }
}

fn compare_optional_text(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.to_lowercase().cmp(&right.to_lowercase()),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional<T: Ord>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

const fn liveness_rank(liveness: Liveness) -> u8 {
    match liveness {
        Liveness::Online => 0,
        Liveness::Offline => 1,
        Liveness::Unknown => 2,
    }
}
