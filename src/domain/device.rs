use std::cmp::Ordering;

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
    NoPath,
}

impl ConnectionPath {
    pub fn label(&self) -> &str {
        match self {
            Self::Direct { .. } => "direct",
            Self::Derp { .. } => "derp",
            Self::PeerRelay { .. } => "peer-relay",
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
    pub tags: Vec<String>,
    pub last_seen: Option<Timestamp>,
    pub created_at: Timestamp,
    pub capabilities: DeviceCapabilities,
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
        SortField::Owner => compare_optional_text(left.owner.as_deref(), right.owner.as_deref()),
        SortField::Os => left.os.label().cmp(right.os.label()),
        SortField::Path => left.path.label().cmp(right.path.label()),
        SortField::LastSeen => compare_optional(left.last_seen, right.last_seen),
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
