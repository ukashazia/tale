use crate::admin::{AdminResource, AdminResourceState, AdminSnapshot};
use crate::domain::health::{
    ApprovalState, HealthDevice, HealthResource, HealthRoute, HealthUser, SourceFailureClass,
};
use crate::domain::health::{Finding, HealthSnapshot};

pub fn snapshot_from_admin(
    admin: &AdminSnapshot,
    now: u64,
    refresh_interval: u64,
) -> HealthSnapshot {
    let source_id = admin.profile.clone().unwrap_or_else(|| "admin".to_owned());
    let devices = admin
        .devices
        .snapshot
        .as_deref()
        .map_or_else(Vec::new, |values| {
            values
                .iter()
                .map(|device| HealthDevice {
                    stable_id: device.stable_id.clone(),
                    source_id: source_id.clone(),
                    key_expires_at: device.expires_at,
                    approval: approval_from_bool(device.authorized),
                    client_version: device.client_version.clone(),
                    posture_read_succeeded: device.posture_present.is_some(),
                    posture_attributes_present: device.posture_present,
                })
                .collect()
        });
    let users = admin
        .users
        .snapshot
        .as_deref()
        .map_or_else(Vec::new, |values| {
            values
                .iter()
                .map(|user| HealthUser {
                    stable_id: user.id.clone(),
                    source_id: source_id.clone(),
                    approval: approval_from_status(user.status.as_deref()),
                })
                .collect()
        });
    let routes = admin
        .route_observations()
        .into_iter()
        .flat_map(|observation| {
            let route_source_id = source_id.clone();
            observation.advertised.into_iter().map(move |cidr| {
                let approved = observation.enabled.iter().any(|value| value == &cidr);
                HealthRoute {
                    stable_id: format!("{}:{cidr}", observation.device_id),
                    source_id: route_source_id.clone(),
                    cidr,
                    advertiser_id: observation.device_id.clone(),
                    approval: if approved {
                        ApprovalState::Approved
                    } else {
                        ApprovalState::Pending
                    },
                }
            })
        })
        .collect();
    let resources = vec![
        health_resource("devices", &admin.devices, now, &source_id, refresh_interval),
        health_resource("users", &admin.users, now, &source_id, refresh_interval),
        health_resource("routes", &admin.routes, now, &source_id, refresh_interval),
        health_resource("access", &admin.policy, now, &source_id, refresh_interval),
        health_resource(
            "settings",
            &admin.settings,
            now,
            &source_id,
            refresh_interval,
        ),
    ];
    HealthSnapshot {
        now,
        devices,
        users,
        resources,
        routes,
        posture_integration_enabled: admin
            .settings
            .snapshot
            .as_ref()
            .and_then(|settings| settings.posture_identity_collection_on)
            .is_some_and(|value| value),
        relay_samples: Vec::new(),
    }
}

fn approval_from_bool(value: Option<bool>) -> ApprovalState {
    match value {
        Some(true) => ApprovalState::Approved,
        Some(false) => ApprovalState::Pending,
        None => ApprovalState::NotReturned,
    }
}

fn approval_from_status(value: Option<&str>) -> ApprovalState {
    match value {
        Some(value)
            if value.eq_ignore_ascii_case("pending")
                || value.eq_ignore_ascii_case("needs_approval") =>
        {
            ApprovalState::Pending
        }
        Some(value)
            if value.eq_ignore_ascii_case("active") || value.eq_ignore_ascii_case("approved") =>
        {
            ApprovalState::Approved
        }
        Some(_) => ApprovalState::NotReturned,
        None => ApprovalState::NotReturned,
    }
}

fn health_resource<T>(
    stable_id: &str,
    resource: &AdminResource<T>,
    now: u64,
    source_id: &str,
    refresh_interval: u64,
) -> HealthResource {
    let failure_state = resource.last_failure.unwrap_or(resource.state);
    let failure_class = match failure_state {
        AdminResourceState::Failed => Some(SourceFailureClass::Failed),
        AdminResourceState::Forbidden => Some(SourceFailureClass::Forbidden),
        AdminResourceState::PlanRestricted => Some(SourceFailureClass::PlanRestricted),
        _ => None,
    };
    HealthResource {
        stable_id: stable_id.to_owned(),
        source_id: source_id.to_owned(),
        observed_at: resource.observed_at.map_or(now, |value| value),
        refresh_interval: refresh_interval.max(1),
        current: true,
        refresh_failures: u32::from(failure_class.is_some()),
        failure_class,
    }
}

#[derive(Debug, Clone, Default)]
pub struct HealthState {
    pub generation: u64,
    pub snapshot: Option<HealthSnapshot>,
    pub findings: Vec<Finding>,
}

impl HealthState {
    pub fn replace(&mut self, snapshot: HealthSnapshot) {
        self.generation = self.generation.saturating_add(1);
        self.findings = snapshot.findings();
        self.snapshot = Some(snapshot);
    }

    pub fn replace_evaluated(&mut self, snapshot: HealthSnapshot, findings: Vec<Finding>) {
        self.generation = self.generation.saturating_add(1);
        self.findings = findings;
        self.snapshot = Some(snapshot);
    }

    pub fn clear(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.snapshot = None;
        self.findings.clear();
    }
}
