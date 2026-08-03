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
    pub version: String,
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
            version: match self.version.clone() {
                Some(value) => value,
                None => "not returned".to_owned(),
            },
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
    pub fn search_text(&self) -> String {
        let mut fields = vec![self.display_name.as_str(), self.hostname.as_str()];
        if let Some(owner) = self.owner.as_deref() {
            fields.push(owner);
        }
        if let Some(owner_label) = self.owner_label.as_deref() {
            fields.push(owner_label);
        }
        fields.extend(self.tags.iter().map(String::as_str));
        fields.extend(self.addresses.iter().map(String::as_str));
        fields.join(" ").to_lowercase()
    }

    pub fn owner_matches(&self, value: &str) -> bool {
        let needle = value.to_lowercase();
        self.owner
            .as_deref()
            .is_some_and(|owner| owner.eq_ignore_ascii_case(&needle))
            || self
                .owner_label
                .as_deref()
                .is_some_and(|owner| owner.eq_ignore_ascii_case(&needle))
    }

    pub fn tag_matches(&self, value: &str) -> bool {
        self.tags.iter().any(|tag| tag.eq_ignore_ascii_case(value))
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

pub fn compare_devices(left: &Device, right: &Device, sort: SortSpec) -> Ordering {
    let primary = match sort.field {
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
        SortField::LastSeen => compare_optional(left.last_seen, right.last_seen),
        SortField::Rx => compare_optional(left.rx_bytes, right.rx_bytes),
        SortField::Tx => compare_optional(left.tx_bytes, right.tx_bytes),
        SortField::DeviceId => left.id.cmp(&right.id),
    };
    let directed = match sort.direction {
        SortDirection::Ascending => primary,
        SortDirection::Descending => primary.reverse(),
    };
    if directed == Ordering::Equal {
        left.id.cmp(&right.id)
    } else {
        directed
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
