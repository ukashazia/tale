use std::cmp::Ordering;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;

use crate::domain::device::{ConnectionPath, DeviceId};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RouteError {
    Empty,
    MissingPrefix(String),
    InvalidAddress(String),
    InvalidPrefix(String),
    InvalidEndpoint(String),
    InvalidPort(String),
}

impl fmt::Display for RouteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("route cannot be empty"),
            Self::MissingPrefix(value) => write!(formatter, "route has no prefix: {value}"),
            Self::InvalidAddress(value) => write!(formatter, "invalid IP address: {value}"),
            Self::InvalidPrefix(value) => write!(formatter, "invalid prefix length: {value}"),
            Self::InvalidEndpoint(value) => write!(formatter, "invalid static endpoint: {value}"),
            Self::InvalidPort(value) => write!(formatter, "invalid port: {value}"),
        }
    }
}

impl std::error::Error for RouteError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct IpNet {
    address: IpAddr,
    prefix: u8,
}

impl IpNet {
    pub fn new(address: IpAddr, prefix: u8) -> Result<Self, RouteError> {
        let max = match address {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix > max {
            return Err(RouteError::InvalidPrefix(prefix.to_string()));
        }
        Ok(Self {
            address: mask_address(address, prefix),
            prefix,
        })
    }

    pub const fn address(self) -> IpAddr {
        self.address
    }

    pub const fn prefix(self) -> u8 {
        self.prefix
    }

    pub fn contains(self, address: IpAddr) -> bool {
        match (self.address, address) {
            (IpAddr::V4(network), IpAddr::V4(value)) => {
                mask_address(IpAddr::V4(value), self.prefix) == IpAddr::V4(network)
            }
            (IpAddr::V6(network), IpAddr::V6(value)) => {
                mask_address(IpAddr::V6(value), self.prefix) == IpAddr::V6(network)
            }
            _ => false,
        }
    }

    pub fn overlaps(self, other: Self) -> bool {
        match (self.address, other.address) {
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {
                self.contains(other.address) || other.contains(self.address)
            }
            _ => false,
        }
    }
}

impl FromStr for IpNet {
    type Err = RouteError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err(RouteError::Empty);
        }
        let (address, prefix) = value
            .split_once('/')
            .ok_or_else(|| RouteError::MissingPrefix(value.to_owned()))?;
        let address = IpAddr::from_str(address)
            .map_err(|_| RouteError::InvalidAddress(address.to_owned()))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| RouteError::InvalidPrefix(prefix.to_owned()))?;
        Self::new(address, prefix)
    }
}

impl fmt::Display for IpNet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.address, self.prefix)
    }
}

impl Ord for IpNet {
    fn cmp(&self, other: &Self) -> Ordering {
        route_family(self.address)
            .cmp(&route_family(other.address))
            .then_with(|| address_bytes(self.address).cmp(&address_bytes(other.address)))
            .then_with(|| self.prefix.cmp(&other.prefix))
    }
}

impl PartialOrd for IpNet {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn parse_route_set(value: &str) -> Result<Vec<IpNet>, RouteError> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut routes = value
        .split(',')
        .map(str::trim)
        .map(IpNet::from_str)
        .collect::<Result<Vec<_>, _>>()?;
    routes.sort();
    routes.dedup();
    Ok(routes)
}

pub fn format_route_set(routes: &[IpNet]) -> String {
    canonical_routes(routes)
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub fn canonical_routes(routes: &[IpNet]) -> Vec<IpNet> {
    let mut canonical = routes.to_vec();
    canonical.sort();
    canonical.dedup();
    canonical
}

pub fn overlapping_routes(routes: &[IpNet]) -> Vec<(IpNet, IpNet)> {
    let canonical = canonical_routes(routes);
    let mut overlaps = Vec::new();
    for (index, left) in canonical.iter().enumerate() {
        for right in canonical.iter().skip(index.saturating_add(1)) {
            if left.overlaps(*right) {
                overlaps.push((*left, *right));
            }
        }
    }
    overlaps
}

pub fn parse_static_endpoints(value: &str) -> Result<Vec<SocketAddr>, RouteError> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut endpoints = value
        .split(',')
        .map(str::trim)
        .map(|value| {
            SocketAddr::from_str(value).map_err(|_| RouteError::InvalidEndpoint(value.to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    endpoints.sort_by_key(ToString::to_string);
    endpoints.dedup();
    Ok(endpoints)
}

pub fn format_static_endpoints(endpoints: &[SocketAddr]) -> String {
    let mut values = endpoints
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values.join(",")
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExitNodeCandidate {
    pub device_id: DeviceId,
    pub display_name: String,
    pub dns_name: Option<String>,
    pub tailscale_ips: Vec<String>,
    pub online: Option<bool>,
    pub path: ConnectionPath,
    pub last_probe_ms: Option<u16>,
    pub selected: bool,
}

impl ExitNodeCandidate {
    pub fn stable_target(&self) -> Option<String> {
        self.dns_name
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| self.tailscale_ips.first().cloned())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ExitNodeSelection {
    None,
    Device { device_id: DeviceId, target: String },
    AutoAny,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ExitNodeRequest {
    pub selection: ExitNodeSelection,
    pub allow_lan_access: bool,
}

impl ExitNodeRequest {
    pub fn target(&self) -> String {
        match &self.selection {
            ExitNodeSelection::None => String::new(),
            ExitNodeSelection::Device { target, .. } => target.clone(),
            ExitNodeSelection::AutoAny => "auto:any".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct AdvertisementRequest {
    pub routes: Option<Vec<IpNet>>,
    pub advertise_exit_node: Option<bool>,
    pub advertise_connector: Option<bool>,
    pub relay_server_port: Option<Option<u16>>,
    pub relay_server_static_endpoints: Option<Vec<SocketAddr>>,
    pub accept_mac_app_connector_risk: bool,
}

impl AdvertisementRequest {
    pub fn canonical_routes(&self) -> Option<Vec<IpNet>> {
        self.routes.as_deref().map(canonical_routes)
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_none()
            && self.advertise_exit_node.is_none()
            && self.advertise_connector.is_none()
            && self.relay_server_port.is_none()
            && self.relay_server_static_endpoints.is_none()
    }
}

fn route_family(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 0,
        IpAddr::V6(_) => 1,
    }
}

fn address_bytes(address: IpAddr) -> Vec<u8> {
    match address {
        IpAddr::V4(value) => value.octets().to_vec(),
        IpAddr::V6(value) => value.octets().to_vec(),
    }
}

fn mask_address(address: IpAddr, prefix: u8) -> IpAddr {
    match address {
        IpAddr::V4(value) => {
            let bits = u32::from(value);
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32_u8.saturating_sub(prefix))
            };
            IpAddr::V4(Ipv4Addr::from(bits & mask))
        }
        IpAddr::V6(value) => {
            let bits = u128::from(value);
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128_u8.saturating_sub(prefix))
            };
            IpAddr::V6(Ipv6Addr::from(bits & mask))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_canonicalized_deduplicated_and_family_sorted() {
        let routes = parse_route_set("2001:db8::1/64,10.1.2.3/8,10.0.0.0/8");
        assert!(routes.is_ok());
        if let Ok(routes) = routes {
            assert_eq!(
                routes.iter().map(ToString::to_string).collect::<Vec<_>>(),
                vec!["10.0.0.0/8", "2001:db8::/64"]
            );
        }
    }

    #[test]
    fn overlaps_report_only_same_family_networks() {
        let routes = parse_route_set("10.0.0.0/8,10.20.0.0/16,2001:db8::/32");
        assert!(routes.is_ok());
        if let Ok(routes) = routes {
            assert_eq!(overlapping_routes(&routes).len(), 1);
        }
    }

    #[test]
    fn static_endpoint_format_brackets_ipv6_and_sorts() {
        let endpoints = parse_static_endpoints("[2001:db8::1]:443,203.0.113.10:80");
        assert!(endpoints.is_ok());
        if let Ok(endpoints) = endpoints {
            assert_eq!(
                format_static_endpoints(&endpoints),
                "203.0.113.10:80,[2001:db8::1]:443"
            );
        }
    }
}
