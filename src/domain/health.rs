use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use sha2::{Digest, Sha256};

use super::Timestamp;

pub const KEY_EXPIRY_WARNING_WINDOW: Timestamp = 7 * 24 * 60 * 60;
pub const MAX_AFFECTED_RESOURCE_IDS: usize = 1_000;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Warning => "Warning",
            Self::Critical => "Critical",
        }
    }

    const fn sort_rank(self) -> u8 {
        match self {
            Self::Critical => 0,
            Self::Warning => 1,
            Self::Info => 2,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ObservedFact {
    pub label: String,
    pub value: String,
    pub source_id: Option<String>,
    pub observed_at: Option<Timestamp>,
}

impl ObservedFact {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            source_id: None,
            observed_at: None,
        }
    }

    pub fn from_source(
        label: impl Into<String>,
        value: impl Into<String>,
        source_id: impl Into<String>,
        observed_at: Timestamp,
    ) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            source_id: Some(source_id.into()),
            observed_at: Some(observed_at),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Finding {
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    pub title: String,
    pub observed_facts: Vec<ObservedFact>,
    pub observed_at: Timestamp,
    pub affected_resource_ids: Vec<String>,
    pub truncated_affected_resource_count: usize,
    pub source_ids: Vec<String>,
    pub explanation: String,
    pub suggested_action_ids: Vec<String>,
    pub derived: bool,
}

impl Finding {
    #[allow(clippy::too_many_arguments)]
    fn new(
        rule_id: &str,
        severity: Severity,
        title: &str,
        observed_at: Timestamp,
        mut affected_resource_ids: Vec<String>,
        mut observed_facts: Vec<ObservedFact>,
        explanation: &str,
        suggested_action_ids: Vec<String>,
    ) -> Self {
        affected_resource_ids.sort();
        affected_resource_ids.dedup();
        let id = finding_id(rule_id, &affected_resource_ids);
        let total = affected_resource_ids.len();
        let truncated_affected_resource_count = total.saturating_sub(MAX_AFFECTED_RESOURCE_IDS);
        affected_resource_ids.truncate(MAX_AFFECTED_RESOURCE_IDS);
        observed_facts.sort();
        observed_facts.dedup();
        let source_ids = observed_facts
            .iter()
            .filter_map(|fact| fact.source_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        Self {
            id,
            rule_id: rule_id.to_owned(),
            severity,
            title: title.to_owned(),
            observed_facts,
            observed_at,
            affected_resource_ids,
            truncated_affected_resource_count,
            source_ids,
            explanation: explanation.to_owned(),
            suggested_action_ids,
            derived: true,
        }
    }
}

fn finding_id(rule_id: &str, stable_ids: &[String]) -> String {
    let mut values = stable_ids.to_vec();
    values.sort();
    let mut hasher = Sha256::new();
    hasher.update(rule_id.as_bytes());
    hasher.update([0]);
    for value in values {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ApprovalState {
    Approved,
    Pending,
    NotReturned,
}

impl ApprovalState {
    const fn label(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Pending => "pending",
            Self::NotReturned => "not returned",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HealthDevice {
    pub stable_id: String,
    pub source_id: String,
    pub key_expires_at: Option<Timestamp>,
    pub approval: ApprovalState,
    pub client_version: Option<String>,
    pub posture_read_succeeded: bool,
    pub posture_attributes_present: Option<bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HealthUser {
    pub stable_id: String,
    pub source_id: String,
    pub approval: ApprovalState,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HealthResource {
    pub stable_id: String,
    pub source_id: String,
    pub observed_at: Timestamp,
    pub refresh_interval: Timestamp,
    pub current: bool,
    pub refresh_failures: u32,
    pub failure_class: Option<SourceFailureClass>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceFailureClass {
    Failed,
    Forbidden,
    PlanRestricted,
}

impl SourceFailureClass {
    const fn label(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Forbidden => "forbidden",
            Self::PlanRestricted => "plan restricted",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HealthRoute {
    pub stable_id: String,
    pub source_id: String,
    pub cidr: String,
    pub advertiser_id: String,
    pub approval: ApprovalState,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RelaySample {
    pub source_id: String,
    pub peer_id: String,
    pub relay: bool,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HealthSnapshot {
    pub now: Timestamp,
    pub devices: Vec<HealthDevice>,
    pub users: Vec<HealthUser>,
    pub resources: Vec<HealthResource>,
    pub routes: Vec<HealthRoute>,
    pub posture_integration_enabled: bool,
    pub relay_samples: Vec<RelaySample>,
}

impl HealthSnapshot {
    pub fn findings(&self) -> Vec<Finding> {
        derive_findings(self)
    }
}

pub fn derive_findings(snapshot: &HealthSnapshot) -> Vec<Finding> {
    let mut findings = Vec::new();
    expired_key_findings(snapshot, &mut findings);
    approval_findings(snapshot, &mut findings);
    source_findings(snapshot, &mut findings);
    route_overlap_findings(snapshot, &mut findings);
    client_version_findings(snapshot, &mut findings);
    posture_findings(snapshot, &mut findings);
    relay_findings(snapshot, &mut findings);
    findings.sort_by(|left, right| {
        left.severity
            .sort_rank()
            .cmp(&right.severity.sort_rank())
            .then_with(|| left.rule_id.cmp(&right.rule_id))
            .then_with(|| left.affected_resource_ids.cmp(&right.affected_resource_ids))
            .then_with(|| left.id.cmp(&right.id))
    });
    findings
}

fn expired_key_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    for device in &snapshot.devices {
        let Some(expires_at) = device.key_expires_at else {
            continue;
        };
        if expires_at <= snapshot.now {
            findings.push(Finding::new(
                "device-key-expired",
                Severity::Critical,
                "Device key is expired",
                snapshot.now,
                vec![device.stable_id.clone()],
                vec![ObservedFact::from_source(
                    "expires_at",
                    expires_at.to_string(),
                    device.source_id.clone(),
                    snapshot.now,
                )],
                "The authoritative device observation reports an expiry at or before the supplied clock.",
                vec!["admin.device.key_expire_now".to_owned()],
            ));
        } else if expires_at.saturating_sub(snapshot.now) <= KEY_EXPIRY_WARNING_WINDOW {
            findings.push(Finding::new(
                "device-key-expiring",
                Severity::Warning,
                "Device key expires soon",
                snapshot.now,
                vec![device.stable_id.clone()],
                vec![ObservedFact::from_source(
                    "expires_at",
                    expires_at.to_string(),
                    device.source_id.clone(),
                    snapshot.now,
                )],
                "The authoritative device observation places key expiry within the fixed seven-day warning window.",
                Vec::new(),
            ));
        }
    }
}

fn approval_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    for device in &snapshot.devices {
        if device.approval == ApprovalState::Pending {
            findings.push(Finding::new(
                "device-approval-pending",
                Severity::Warning,
                "Device approval is pending",
                snapshot.now,
                vec![device.stable_id.clone()],
                vec![ObservedFact::from_source(
                    "approval",
                    device.approval.label(),
                    device.source_id.clone(),
                    snapshot.now,
                )],
                "The server explicitly returned a pending device approval state.",
                vec!["admin.device.approve".to_owned()],
            ));
        }
    }
    for user in &snapshot.users {
        if user.approval == ApprovalState::Pending {
            findings.push(Finding::new(
                "user-approval-pending",
                Severity::Warning,
                "User approval is pending",
                snapshot.now,
                vec![user.stable_id.clone()],
                vec![ObservedFact::from_source(
                    "approval",
                    user.approval.label(),
                    user.source_id.clone(),
                    snapshot.now,
                )],
                "The server explicitly returned a pending user approval state.",
                vec!["admin.user.approve".to_owned()],
            ));
        }
    }
}

fn source_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    for resource in &snapshot.resources {
        if !resource.current {
            continue;
        }
        if let Some(failure_class) = resource.failure_class {
            findings.push(Finding::new(
                "source-failed",
                Severity::Warning,
                "Current source refresh failed",
                snapshot.now,
                vec![resource.stable_id.clone()],
                vec![ObservedFact::from_source(
                    "failure_class",
                    failure_class.label(),
                    resource.source_id.clone(),
                    resource.observed_at,
                )],
                "The current source returned an explicit failure classification; permission and plan failures are not treated as offline infrastructure.",
                Vec::new(),
            ));
        }
        let age = snapshot.now.saturating_sub(resource.observed_at);
        let interval = resource.refresh_interval.max(1);
        let infrastructure_failure = resource
            .failure_class
            .is_none_or(|class| class == SourceFailureClass::Failed);
        if !infrastructure_failure {
            continue;
        }
        if age > interval.saturating_mul(10) && resource.refresh_failures > 0 {
            findings.push(Finding::new(
                "source-stale",
                Severity::Critical,
                "Source observation is critically stale",
                snapshot.now,
                vec![resource.stable_id.clone()],
                vec![
                    ObservedFact::from_source(
                        "age_seconds",
                        age.to_string(),
                        resource.source_id.clone(),
                        resource.observed_at,
                    ),
                    ObservedFact::new("refresh_interval_seconds", interval.to_string()),
                    ObservedFact::new("failed_refreshes", resource.refresh_failures.to_string()),
                ],
                "The source is more than ten refresh intervals old and has at least one recorded failed refresh.",
                Vec::new(),
            ));
        } else if age > interval.saturating_mul(3) {
            findings.push(Finding::new(
                "source-stale",
                Severity::Warning,
                "Source observation is stale",
                snapshot.now,
                vec![resource.stable_id.clone()],
                vec![
                    ObservedFact::from_source(
                        "age_seconds",
                        age.to_string(),
                        resource.source_id.clone(),
                        resource.observed_at,
                    ),
                    ObservedFact::new("refresh_interval_seconds", interval.to_string()),
                ],
                "The source observation is more than three refresh intervals old.",
                Vec::new(),
            ));
        }
    }
}

fn route_overlap_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    // Each CIDR is parsed once and the routes are ordered by address, so the
    // routes that can overlap a given one are the ones immediately after it.
    // Comparing every pair and re-parsing both sides made this quadratic in a
    // tailnet's route count.
    let mut routes = snapshot
        .routes
        .iter()
        .filter_map(|route| route_span(&route.cidr).map(|span| (span, route)))
        .collect::<Vec<_>>();
    routes.sort_by(|(left_span, left), (right_span, right)| {
        left_span
            .cmp(right_span)
            .then_with(|| left.stable_id.cmp(&right.stable_id))
    });
    for (index, (span, first)) in routes.iter().enumerate() {
        for (other_span, second) in routes.iter().skip(index + 1) {
            // Ordered by start address, so the first route that begins after
            // this one ends rules out every route after it as well.
            if other_span.family != span.family || other_span.start > span.end {
                break;
            }
            if first.advertiser_id == second.advertiser_id {
                continue;
            }
            // The pair is reported lowest stable id first, so the finding reads
            // the same however the scan reached it.
            let (left, right) = if first.stable_id <= second.stable_id {
                (first, second)
            } else {
                (second, first)
            };
            findings.push(Finding::new(
                "route-overlap-review",
                Severity::Info,
                "Overlapping routes require review",
                snapshot.now,
                vec![left.stable_id.clone(), right.stable_id.clone()],
                vec![
                    ObservedFact::from_source(
                        "left_route",
                        format!(
                            "cidr={} advertiser={} approval={}",
                            left.cidr,
                            left.advertiser_id,
                            left.approval.label()
                        ),
                        left.source_id.clone(),
                        snapshot.now,
                    ),
                    ObservedFact::from_source(
                        "right_route",
                        format!(
                            "cidr={} advertiser={} approval={}",
                            right.cidr,
                            right.advertiser_id,
                            right.approval.label()
                        ),
                        right.source_id.clone(),
                        snapshot.now,
                    ),
                ],
                "The observed route CIDRs overlap and have different stable advertisers. This is a review signal, not a reachability or conflict claim.",
                vec!["admin.routes.replace_approvals".to_owned()],
            ));
        }
    }
}

/// A route's address span, normalized so that ordering brings every pair that
/// can overlap next to each other. IPv4 and IPv6 never overlap, so the family
/// leads the ordering and separates the two blocks.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct RouteSpan {
    family: u8,
    start: u128,
    end: u128,
}

fn route_span(cidr: &str) -> Option<RouteSpan> {
    let (address, prefix) = cidr.split_once('/')?;
    let prefix = prefix.parse::<u8>().ok()?;
    let (family, bits, width) = match address.parse::<IpAddr>().ok()? {
        IpAddr::V4(value) => (0, u128::from(u32::from(value)), 32),
        IpAddr::V6(value) => (1, u128::from(value), 128),
    };
    if prefix > width {
        return None;
    }
    // Written as a shift of the all-ones value so that a zero-length prefix,
    // whose host part is the whole address, does not shift past the word.
    let host = if prefix == 0 {
        u128::MAX >> (128 - u32::from(width))
    } else {
        (1u128 << (width - prefix)) - 1
    };
    let start = bits & !host;
    Some(RouteSpan {
        family,
        start,
        end: start | host,
    })
}

fn client_version_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    let mut versions: BTreeMap<(u64, u64), Vec<&HealthDevice>> = BTreeMap::new();
    for device in &snapshot.devices {
        let Some(version) = device.client_version.as_deref() else {
            continue;
        };
        let Some((major, minor)) = parse_client_version(version) else {
            continue;
        };
        versions.entry((major, minor)).or_default().push(device);
    }
    let majors = versions
        .keys()
        .map(|(major, _)| *major)
        .collect::<BTreeSet<_>>();
    for major in majors {
        let matching = versions
            .iter()
            .filter(|((version_major, _), _)| *version_major == major)
            .collect::<Vec<_>>();
        let Some(minimum) = matching.iter().map(|((_, minor), _)| *minor).min() else {
            continue;
        };
        let Some(maximum) = matching.iter().map(|((_, minor), _)| *minor).max() else {
            continue;
        };
        if maximum.saturating_sub(minimum) <= 2 {
            continue;
        }
        let devices = matching
            .into_iter()
            .flat_map(|(_, devices)| devices.iter())
            .collect::<Vec<_>>();
        let ids = devices
            .iter()
            .map(|device| device.stable_id.clone())
            .collect::<Vec<_>>();
        let facts = devices
            .iter()
            .filter_map(|device| {
                device.client_version.as_ref().map(|version| {
                    ObservedFact::from_source(
                        "client_version",
                        format!("{}={version}", device.stable_id),
                        device.source_id.clone(),
                        snapshot.now,
                    )
                })
            })
            .collect::<Vec<_>>();
        findings.push(Finding::new(
            "client-version-skew",
            Severity::Info,
            "Observed client versions are spread across minor releases",
            snapshot.now,
            ids,
            facts,
            "Parseable observations under one major version differ by more than two minor versions. This reports only observed skew and makes no support or vulnerability claim.",
            Vec::new(),
        ));
    }
}

fn parse_client_version(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn posture_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    if !snapshot.posture_integration_enabled {
        return;
    }
    for device in &snapshot.devices {
        if device.posture_read_succeeded && device.posture_attributes_present == Some(false) {
            findings.push(Finding::new(
                "posture-observation-missing",
                Severity::Info,
                "Enabled posture integration returned no posture attributes",
                snapshot.now,
                vec![device.stable_id.clone()],
                vec![ObservedFact::from_source(
                    "posture_attributes",
                    "none returned",
                    device.source_id.clone(),
                    snapshot.now,
                )],
                "The enabled posture integration was read successfully and returned no attributes. This is an observation, not a compliance or noncompliance result.",
                Vec::new(),
            ));
        }
    }
}

fn relay_findings(snapshot: &HealthSnapshot, findings: &mut Vec<Finding>) {
    let Some(latest_source) = snapshot
        .relay_samples
        .iter()
        .max_by(|left, right| {
            left.observed_at
                .cmp(&right.observed_at)
                .then_with(|| left.source_id.cmp(&right.source_id))
        })
        .map(|sample| sample.source_id.as_str())
    else {
        return;
    };
    let mut by_peer: BTreeMap<String, Vec<&RelaySample>> = BTreeMap::new();
    for sample in snapshot
        .relay_samples
        .iter()
        .filter(|sample| sample.source_id == latest_source)
    {
        by_peer
            .entry(sample.peer_id.clone())
            .or_default()
            .push(sample);
    }
    for (peer_id, samples) in by_peer {
        if samples.len() < 5 {
            continue;
        }
        let relay_count = samples.iter().filter(|sample| sample.relay).count();
        if relay_count.saturating_mul(100) < samples.len().saturating_mul(80) {
            continue;
        }
        findings.push(Finding::new(
            "relay-heavy-local-peer",
            Severity::Info,
            "Recent local peer samples are relay-heavy",
            snapshot.now,
            vec![peer_id],
            vec![
                ObservedFact::from_source(
                    "relay_samples",
                    samples.len().to_string(),
                    latest_source.to_owned(),
                    snapshot.now,
                ),
                ObservedFact::new("relay_samples_relayed", relay_count.to_string()),
            ],
            "At least five current-session samples were observed for this peer and at least 80% used a relay. Samples are scoped to the latest source identity and are not an offline-health inference.",
            Vec::new(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> HealthSnapshot {
        HealthSnapshot {
            now: 1_000,
            devices: vec![HealthDevice {
                stable_id: "device-a".to_owned(),
                source_id: "fixture".to_owned(),
                key_expires_at: Some(900),
                approval: ApprovalState::Pending,
                client_version: Some("1.0.0".to_owned()),
                posture_read_succeeded: true,
                posture_attributes_present: Some(false),
            }],
            users: vec![HealthUser {
                stable_id: "user-a".to_owned(),
                source_id: "fixture".to_owned(),
                approval: ApprovalState::Pending,
            }],
            resources: Vec::new(),
            routes: Vec::new(),
            posture_integration_enabled: true,
            relay_samples: Vec::new(),
        }
    }

    #[test]
    fn finding_order_and_ids_are_deterministic() {
        let first = snapshot().findings();
        let second = snapshot().findings();
        assert_eq!(first, second);
        assert!(first.iter().all(|finding| finding.derived));
        assert_eq!(first[0].severity, Severity::Critical);
    }

    #[test]
    fn offline_is_not_a_finding_without_authoritative_evidence() {
        let mut value = snapshot();
        value.devices[0].key_expires_at = None;
        value.devices[0].approval = ApprovalState::NotReturned;
        value.devices[0].posture_read_succeeded = false;
        value.posture_integration_enabled = false;
        value.users[0].approval = ApprovalState::NotReturned;
        assert!(value.findings().is_empty());
    }
}
