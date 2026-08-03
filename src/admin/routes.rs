use crate::admin::dto::{DeviceRoutesDto, DtoError, route_values};
use crate::domain::Timestamp;
use crate::domain::device::AdminDevice;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdminRouteObservation {
    pub device_id: String,
    pub advertised: Vec<String>,
    pub enabled: Vec<String>,
    pub observed_at: Timestamp,
    pub complete: bool,
}

impl AdminRouteObservation {
    pub fn advertised_exit_node(&self) -> bool {
        self.advertised
            .iter()
            .any(|route| route == "0.0.0.0/0" || route == "::/0")
    }

    pub fn enabled_exit_node(&self) -> bool {
        self.enabled
            .iter()
            .any(|route| route == "0.0.0.0/0" || route == "::/0")
    }
}

pub fn decode_routes(
    device_id: impl Into<String>,
    routes: DeviceRoutesDto,
    observed_at: Timestamp,
) -> Result<AdminRouteObservation, DtoError> {
    let complete = routes.advertised_routes.is_some() && routes.enabled_routes.is_some();
    let advertised = route_values(routes.advertised_routes)?;
    let enabled = route_values(routes.enabled_routes)?;
    Ok(AdminRouteObservation {
        device_id: device_id.into(),
        complete,
        advertised,
        enabled,
        observed_at,
    })
}

pub fn incomplete_routes(
    device_id: impl Into<String>,
    observed_at: Timestamp,
) -> AdminRouteObservation {
    AdminRouteObservation {
        device_id: device_id.into(),
        advertised: Vec::new(),
        enabled: Vec::new(),
        observed_at,
        complete: false,
    }
}

pub fn from_device(device: &AdminDevice) -> Option<AdminRouteObservation> {
    if device.advertised_routes.is_empty() && device.enabled_routes.is_empty() {
        return None;
    }
    Some(AdminRouteObservation {
        device_id: device.stable_id.clone(),
        advertised: device.advertised_routes.clone(),
        enabled: device.enabled_routes.clone(),
        observed_at: device.source_observed_at,
        complete: false,
    })
}
