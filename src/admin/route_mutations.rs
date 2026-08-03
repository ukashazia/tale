use crate::admin::mutation::requested_route_set;
use crate::admin::routes::AdminRouteObservation;

pub fn canonical_enabled_routes(routes: &[String]) -> Result<Vec<String>, String> {
    requested_route_set(routes)
}

pub fn verify_enabled_routes(
    observation: &AdminRouteObservation,
    requested: &[String],
) -> Result<(), String> {
    let actual = canonical_enabled_routes(&observation.enabled)?;
    let requested = canonical_enabled_routes(requested)?;
    if actual == requested {
        Ok(())
    } else {
        Err(format!(
            "server returned enabled routes [{}], requested [{}]",
            actual.join(", "),
            requested.join(", ")
        ))
    }
}

pub fn newly_enabled_routes(
    advertised: &[String],
    requested: &[String],
) -> Result<Vec<String>, String> {
    let advertised = canonical_enabled_routes(advertised)?;
    let requested = canonical_enabled_routes(requested)?;
    let missing = requested
        .iter()
        .filter(|route| !advertised.contains(route))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(requested)
    } else {
        Err(format!(
            "only advertised routes may be newly approved; missing: {}",
            missing.join(", ")
        ))
    }
}

pub fn validate_replacement(
    advertised: &[String],
    currently_enabled: &[String],
    requested: &[String],
) -> Result<Vec<String>, String> {
    let advertised = canonical_enabled_routes(advertised)?;
    let currently_enabled = canonical_enabled_routes(currently_enabled)?;
    let requested = canonical_enabled_routes(requested)?;
    let missing = requested
        .iter()
        .filter(|route| !advertised.contains(route) && !currently_enabled.contains(route))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(requested)
    } else {
        Err(format!(
            "only advertised routes may be newly approved; missing: {}",
            missing.join(", ")
        ))
    }
}

pub fn route_context(observation: &AdminRouteObservation) -> Vec<String> {
    vec![
        format!("advertised: {}", join_or_none(&observation.advertised)),
        format!("approved: {}", join_or_none(&observation.enabled)),
        format!(
            "exit-node capability: {}",
            if observation.advertised_exit_node() {
                "advertised"
            } else {
                "not advertised"
            }
        ),
        "admin approval does not advertise routes on the device".to_owned(),
    ]
}

fn join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}
