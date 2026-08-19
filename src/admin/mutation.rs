//! Admin resource snapshots, preflight comparisons, and verification adapters.
//!
//! Lifecycle ownership remains in `domain::admin_mutation`; this module maps
//! concrete API resources into that model and does not define another mutation
//! framework.

use std::collections::{BTreeMap, BTreeSet};

use crate::action::{ActionId, Risk};
use crate::domain::Timestamp;
use crate::domain::activity::AuditEvent;
use crate::domain::admin_mutation::{
    AdminChange, AdminMutation, AdminMutationState, AdminResourceLockKey, AuditCorrelation,
    BatchMutation, BatchTarget, FieldConflict, compare_preflight,
};
use crate::domain::device::AdminDevice;
use crate::domain::dns::{AdminDnsPreferences, AdminNameservers, AdminSearchPaths, AdminSplitDns};
use crate::domain::route::IpNet;
use crate::domain::user::AdminUser;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct AdminSnapshotFields {
    pub values: BTreeMap<String, String>,
}

impl AdminSnapshotFields {
    pub fn with(values: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            values: values.into_iter().collect(),
        }
    }
}

pub type AdminMutationRequest = AdminMutation<AdminSnapshotFields, AdminChange>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminBatchConfirmation {
    pub batch: BatchMutation,
    pub requests: Vec<AdminMutationRequest>,
}

pub fn batch_target(request: &AdminMutationRequest) -> BatchTarget {
    let requested_change = match &request.change {
        AdminChange::DeviceRoutes { routes } => format!("routes={}", routes.join(",")),
        change => change.audit_action_class().to_owned(),
    };
    BatchTarget {
        target_id: request.target_id.clone(),
        target_label: request.target_id.clone(),
        requested_change,
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminMutationOutcome {
    pub mutation_id: u64,
    pub state: AdminMutationState,
    pub detail: String,
    pub verification: String,
    pub audit: AuditCorrelation,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminPreflightConflict {
    pub fields: Vec<FieldConflict>,
}

pub fn device_fields(device: &AdminDevice) -> AdminSnapshotFields {
    AdminSnapshotFields::with([
        ("name".to_owned(), device.name.clone().unwrap_or_default()),
        (
            "hostname".to_owned(),
            device.hostname.clone().unwrap_or_default(),
        ),
        (
            "owner".to_owned(),
            device.user_id.clone().unwrap_or_default(),
        ),
        ("tags".to_owned(), device.tags.join(",")),
        ("authorized".to_owned(), optional_bool(device.authorized)),
        (
            "keyExpiryDisabled".to_owned(),
            optional_bool(device.key_expiry_disabled),
        ),
        (
            "expires".to_owned(),
            device
                .expires_at
                .map_or_else(String::new, |value| value.to_string()),
        ),
        ("addresses".to_owned(), device.addresses.join(",")),
        (
            "advertisedRoutes".to_owned(),
            device.advertised_routes.join(","),
        ),
        ("enabledRoutes".to_owned(), device.enabled_routes.join(",")),
        (
            "connectedToControl".to_owned(),
            optional_bool(device.connected_to_control),
        ),
    ])
}

pub fn route_fields(advertised: &[String], enabled: &[String]) -> AdminSnapshotFields {
    AdminSnapshotFields::with([
        ("advertisedRoutes".to_owned(), advertised.join(",")),
        ("enabledRoutes".to_owned(), enabled.join(",")),
    ])
}

pub fn user_fields(user: &AdminUser) -> AdminSnapshotFields {
    AdminSnapshotFields::with([
        ("id".to_owned(), user.id.clone()),
        (
            "displayName".to_owned(),
            user.display_name.clone().unwrap_or_default(),
        ),
        (
            "loginName".to_owned(),
            user.login_name.clone().unwrap_or_default(),
        ),
        ("role".to_owned(), user.role.clone().unwrap_or_default()),
        ("status".to_owned(), user.status.clone().unwrap_or_default()),
        (
            "deviceCount".to_owned(),
            user.device_count
                .map_or_else(String::new, |value| value.to_string()),
        ),
    ])
}

pub fn nameserver_fields(value: &AdminNameservers) -> AdminSnapshotFields {
    AdminSnapshotFields::with([("dns".to_owned(), value.values.join(","))])
}

pub fn dns_preferences_fields(value: &AdminDnsPreferences) -> AdminSnapshotFields {
    AdminSnapshotFields::with([("magicDNS".to_owned(), optional_bool(value.magic_dns))])
}

pub fn search_path_fields(value: &AdminSearchPaths) -> AdminSnapshotFields {
    AdminSnapshotFields::with([("searchPaths".to_owned(), value.values.join(","))])
}

pub fn split_dns_fields(value: &AdminSplitDns) -> AdminSnapshotFields {
    let encoded = value
        .entries
        .iter()
        .map(|(domain, resolvers)| {
            format!(
                "{domain}={}",
                resolvers
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |values| values.join("|"))
            )
        })
        .collect::<Vec<_>>();
    let mut fields = BTreeMap::new();
    fields.insert("splitDns".to_owned(), encoded.join(","));
    for (domain, resolvers) in &value.entries {
        fields.insert(
            format!("splitDns:{domain}"),
            resolvers
                .as_ref()
                .map_or_else(|| "null".to_owned(), |values| values.join("|")),
        );
    }
    AdminSnapshotFields { values: fields }
}

pub fn requested_fields(change: &AdminChange) -> AdminSnapshotFields {
    match change {
        AdminChange::DeviceRename { name } => {
            AdminSnapshotFields::with([(String::from("name"), name.clone())])
        }
        AdminChange::DeviceTags { tags } => {
            AdminSnapshotFields::with([(String::from("tags"), tags.join(","))])
        }
        AdminChange::DeviceApproval { authorized } => {
            AdminSnapshotFields::with([(String::from("authorized"), authorized.to_string())])
        }
        AdminChange::DeviceKeyExpiry { disabled } => {
            AdminSnapshotFields::with([(String::from("keyExpiryDisabled"), disabled.to_string())])
        }
        AdminChange::DeviceExpireNow => {
            AdminSnapshotFields::with([(String::from("expireNow"), String::from("true"))])
        }
        AdminChange::DeviceDelete => {
            AdminSnapshotFields::with([(String::from("deleted"), String::from("true"))])
        }
        AdminChange::DeviceRoutes { routes } => {
            AdminSnapshotFields::with([(String::from("enabledRoutes"), routes.join(","))])
        }
        AdminChange::DnsNameservers { values } => {
            AdminSnapshotFields::with([(String::from("dns"), values.join(","))])
        }
        AdminChange::DnsPreferences { magic_dns } => {
            AdminSnapshotFields::with([(String::from("magicDNS"), magic_dns.to_string())])
        }
        AdminChange::DnsSearchPaths { values } => {
            AdminSnapshotFields::with([(String::from("searchPaths"), values.join(","))])
        }
        AdminChange::DnsSplitMapping {
            domain, resolvers, ..
        } => AdminSnapshotFields::with([(
            format!("splitDns:{domain}"),
            format!(
                "{domain}={}",
                resolvers
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |values| values.join("|"))
            ),
        )]),
        AdminChange::UserApproval => {
            AdminSnapshotFields::with([(String::from("status"), String::from("approved"))])
        }
        AdminChange::UserRole { role } => {
            AdminSnapshotFields::with([(String::from("role"), role.clone())])
        }
        AdminChange::UserSuspend => {
            AdminSnapshotFields::with([(String::from("status"), String::from("suspended"))])
        }
        AdminChange::UserRestore => {
            AdminSnapshotFields::with([(String::from("status"), String::from("active"))])
        }
        AdminChange::UserDelete => {
            AdminSnapshotFields::with([(String::from("deleted"), String::from("true"))])
        }
    }
}

pub fn preflight_conflict(
    base: &AdminSnapshotFields,
    fresh: &AdminSnapshotFields,
    change: &AdminChange,
) -> Option<AdminPreflightConflict> {
    let requested = requested_fields(change);
    let dependencies = dependent_fields(change, base, fresh);
    let base = dependencies
        .iter()
        .map(|field| {
            (
                field.clone(),
                base.values.get(field).cloned().unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let fresh = dependencies
        .iter()
        .map(|field| {
            (
                field.clone(),
                fresh.values.get(field).cloned().unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let fields = compare_preflight(&base, &fresh, &requested.values);
    (!fields.is_empty()).then_some(AdminPreflightConflict { fields })
}

fn dependent_fields(
    change: &AdminChange,
    base: &AdminSnapshotFields,
    fresh: &AdminSnapshotFields,
) -> Vec<String> {
    let names: &[&str] = match change {
        AdminChange::DeviceRename { .. } => &["name", "hostname"],
        AdminChange::DeviceTags { .. } => &["tags", "owner"],
        AdminChange::DeviceApproval { .. } => &[
            "authorized",
            "owner",
            "tags",
            "keyExpiryDisabled",
            "advertisedRoutes",
            "enabledRoutes",
        ],
        AdminChange::DeviceKeyExpiry { .. } => &["keyExpiryDisabled", "expires"],
        AdminChange::DeviceExpireNow => &["keyExpiryDisabled", "expires"],
        AdminChange::DeviceDelete => &[],
        AdminChange::DeviceRoutes { .. } => &["advertisedRoutes", "enabledRoutes"],
        AdminChange::DnsNameservers { .. } => &["dns"],
        AdminChange::DnsPreferences { .. } => &["magicDNS"],
        AdminChange::DnsSearchPaths { .. } => &["searchPaths"],
        AdminChange::DnsSplitMapping { domain, .. } => {
            return vec![format!("splitDns:{domain}")];
        }
        AdminChange::UserApproval | AdminChange::UserRestore => &["status", "role", "deviceCount"],
        AdminChange::UserRole { .. } => &["role", "status", "deviceCount"],
        AdminChange::UserSuspend | AdminChange::UserDelete => {
            &["status", "role", "deviceCount", "displayName", "loginName"]
        }
    };
    if names.is_empty() {
        let mut keys = base.values.keys().cloned().collect::<BTreeSet<_>>();
        keys.extend(fresh.values.keys().cloned());
        keys.into_iter().collect()
    } else {
        names.iter().map(|name| (*name).to_owned()).collect()
    }
}

pub fn preview_lines(
    base: &AdminSnapshotFields,
    fresh: &AdminSnapshotFields,
    change: &AdminChange,
) -> Vec<String> {
    let source = if base == fresh { base } else { fresh };
    match change {
        AdminChange::DeviceRename { name } => vec![format!(
            "machine name: {} -> {name}",
            display_field(source, "name")
        )],
        AdminChange::DeviceTags { tags } => list_diff_lines(
            "tags",
            split_list(source.values.get("tags")),
            tags.iter().map(String::as_str).collect(),
        ),
        AdminChange::DeviceApproval { authorized } => {
            let current = match source.values.get("authorized").map(String::as_str) {
                Some("true") => "approved",
                Some("false") => "not approved",
                _ => "unknown",
            };
            let requested = if *authorized { "approved" } else { "revoked" };
            vec![format!("Approval: {current} -> {requested}")]
        }
        AdminChange::DeviceKeyExpiry { disabled } => vec![
            format!(
                "key expiry disabled: {} -> {}",
                display_field(source, "keyExpiryDisabled"),
                disabled
            ),
            "enabling an already expired key requires device reauthentication".to_owned(),
        ],
        AdminChange::DeviceExpireNow => vec![
            "current device key: active -> expired".to_owned(),
            "the device may disconnect and must reauthenticate; Tale will not reauthenticate it"
                .to_owned(),
        ],
        AdminChange::DeviceDelete => vec![
            "device: present -> absent".to_owned(),
            "owned user records, other advertisers, local profiles, and keyring records are not deleted"
                .to_owned(),
        ],
        AdminChange::DeviceRoutes { routes } => list_diff_lines(
            "enabled routes",
            split_list(source.values.get("enabledRoutes")),
            routes.iter().map(String::as_str).collect(),
        ),
        AdminChange::DnsNameservers { values } => ordered_replacement_lines(
            "nameservers",
            split_list(source.values.get("dns")),
            values,
        ),
        AdminChange::DnsPreferences { magic_dns } => vec![format!(
            "MagicDNS: {} -> {}",
            display_field(source, "magicDNS"),
            magic_dns
        )],
        AdminChange::DnsSearchPaths { values } => ordered_replacement_lines(
            "search paths",
            split_list(source.values.get("searchPaths")),
            values,
        ),
        AdminChange::DnsSplitMapping {
            domain, resolvers, ..
        } => vec![format!(
            "split DNS {domain}: {} -> {}",
            split_mapping_value(source.values.get("splitDns"), domain),
            resolvers
                .as_ref()
                .map_or_else(|| "removed".to_owned(), |values| values.join(", "))
        )],
        AdminChange::UserApproval => vec![format!(
            "user status: {} -> approved",
            display_field(source, "status")
        )],
        AdminChange::UserRole { role } => vec![format!(
            "role: {} -> {role}",
            display_field(source, "role")
        )],
        AdminChange::UserSuspend => vec![
            format!("user status: {} -> suspended", display_field(source, "status")),
            "owned-device sessions and secondary credentials are not predicted from audit data"
                .to_owned(),
        ],
        AdminChange::UserRestore => vec![format!(
            "user status: {} -> active",
            display_field(source, "status")
        )],
        AdminChange::UserDelete => vec![
            "user: present -> absent".to_owned(),
            "local Tale profiles and keyring records remain unchanged".to_owned(),
        ],
    }
}

pub fn lock_keys(
    profile: &str,
    target_id: &str,
    change: &AdminChange,
) -> Vec<AdminResourceLockKey> {
    change.lock_keys(profile, target_id)
}

pub fn required_scope(change: &AdminChange) -> &'static str {
    match change {
        AdminChange::DeviceRoutes { .. } => "devices:routes",
        AdminChange::DnsNameservers { .. }
        | AdminChange::DnsPreferences { .. }
        | AdminChange::DnsSearchPaths { .. }
        | AdminChange::DnsSplitMapping { .. } => "dns",
        AdminChange::UserApproval
        | AdminChange::UserRole { .. }
        | AdminChange::UserSuspend
        | AdminChange::UserRestore
        | AdminChange::UserDelete => "users",
        AdminChange::DeviceRename { .. }
        | AdminChange::DeviceTags { .. }
        | AdminChange::DeviceApproval { .. }
        | AdminChange::DeviceKeyExpiry { .. }
        | AdminChange::DeviceExpireNow
        | AdminChange::DeviceDelete => "devices:core",
    }
}

pub fn action_id(change: &AdminChange) -> ActionId {
    change.action_id()
}

pub fn risk(change: &AdminChange) -> Risk {
    change.risk()
}

pub fn correlate_audit(
    events: &[AuditEvent],
    target_id: &str,
    action_class: &str,
    actor_identity: Option<&str>,
    dispatched_at: Timestamp,
    observed_until: Timestamp,
) -> AuditCorrelation {
    let lower_action = action_class.to_ascii_lowercase();
    let start = dispatched_at.saturating_sub(5);
    let mut candidate_event_ids = events
        .iter()
        .filter(|event| event.event_time >= start && event.event_time <= observed_until)
        .filter(|event| {
            event
                .target
                .as_ref()
                .and_then(|target| target.id.as_deref())
                .is_some_and(|id| id == target_id)
        })
        .filter(|event| {
            event
                .action
                .as_deref()
                .is_some_and(|action| action.to_ascii_lowercase().contains(lower_action.as_str()))
        })
        .filter(|event| {
            actor_identity.is_none_or(|actor| {
                event.actor.as_ref().is_some_and(|principal| {
                    principal.id.as_deref() == Some(actor)
                        || principal.display.as_deref() == Some(actor)
                })
            })
        })
        .filter_map(|event| event.event_group_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    candidate_event_ids.sort();
    AuditCorrelation {
        candidate_event_ids,
        polling_stopped: observed_until >= dispatched_at.saturating_add(120),
    }
}

fn optional_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn display_field(fields: &AdminSnapshotFields, field: &str) -> String {
    fields
        .values
        .get(field)
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| "unknown".to_owned())
}

fn split_list(value: Option<&String>) -> Vec<&str> {
    value
        .filter(|value| !value.is_empty())
        .map_or_else(Vec::new, |value| value.split(',').collect())
}

fn list_diff_lines(field: &str, old: Vec<&str>, new: Vec<&str>) -> Vec<String> {
    let old_set = old.iter().copied().collect::<BTreeSet<_>>();
    let new_set = new.iter().copied().collect::<BTreeSet<_>>();
    let added = new_set.difference(&old_set).copied().collect::<Vec<_>>();
    let removed = old_set.difference(&new_set).copied().collect::<Vec<_>>();
    let retained = new_set.intersection(&old_set).copied().collect::<Vec<_>>();
    vec![
        format!("{field} added: {}", joined_or_none(&added)),
        format!("{field} removed: {}", joined_or_none(&removed)),
        format!("{field} retained: {}", joined_or_none(&retained)),
    ]
}

fn ordered_replacement_lines(field: &str, old: Vec<&str>, new: &[String]) -> Vec<String> {
    vec![
        format!("{field} old (ordered): {}", joined_or_none(&old)),
        format!(
            "{field} new (ordered): {}",
            joined_or_none(&new.iter().map(String::as_str).collect::<Vec<_>>())
        ),
    ]
}

fn joined_or_none(values: &[&str]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}

fn split_mapping_value(value: Option<&String>, domain: &str) -> String {
    value
        .and_then(|value| {
            value.split(',').find_map(|entry| {
                let (entry_domain, resolvers) = entry.split_once('=')?;
                (entry_domain == domain).then_some(resolvers.to_owned())
            })
        })
        .unwrap_or_else(|| "not returned".to_owned())
}

pub fn canonical_route_strings(routes: &[IpNet]) -> Vec<String> {
    let mut values = routes.iter().map(ToString::to_string).collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub fn requested_route_set(routes: &[String]) -> Result<Vec<String>, String> {
    let mut parsed = routes
        .iter()
        .map(|route| route.parse::<IpNet>().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    parsed.sort();
    parsed.dedup();
    Ok(canonical_route_strings(&parsed))
}

pub fn collect_target_ids(targets: &[String]) -> Vec<String> {
    let mut ids = targets.to_vec();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::admin_mutation::AdminChange;

    #[test]
    fn previews_show_complete_tag_delta_and_dns_order() {
        let base = AdminSnapshotFields::with([
            (String::from("tags"), String::from("tag:old,tag:keep")),
            (
                String::from("dns"),
                String::from("203.0.113.1,2001:db8::53"),
            ),
        ]);
        let tags = preview_lines(
            &base,
            &base,
            &AdminChange::DeviceTags {
                tags: vec![String::from("tag:keep"), String::from("tag:new")],
            },
        );
        assert!(tags.iter().any(|line| line.contains("tag:new")));
        let dns = preview_lines(
            &base,
            &base,
            &AdminChange::DnsNameservers {
                values: vec![String::from("1.1.1.1")],
            },
        );
        assert!(dns.iter().any(|line| line.contains("2001:db8::53")));
        assert!(dns.iter().any(|line| line.contains("1.1.1.1")));
    }

    #[test]
    fn route_request_is_canonical_and_deduplicated() {
        let result = requested_route_set(&[
            String::from("2001:db8::1/64"),
            String::from("10.0.1.2/8"),
            String::from("10.0.0.0/8"),
        ]);
        assert_eq!(
            result,
            Ok(vec![
                String::from("10.0.0.0/8"),
                String::from("2001:db8::/64")
            ])
        );
    }
}
