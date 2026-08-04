use crate::app::App;

pub fn summary(app: &App) -> String {
    let Some(result) = app.access_explorer_result.as_ref() else {
        return "Access Explorer · server policy preview only · ask a documented user or ip:port question"
            .to_owned();
    };
    let rules = if result.rule_locations.is_empty() {
        "no rule locations returned".to_owned()
    } else {
        format!("rules {}", join_u32(&result.rule_locations))
    };
    let matched_users = if result.matched_users.is_empty() {
        "none".to_owned()
    } else {
        result.matched_users.join(",")
    };
    let matched_ports = if result.matched_ports.is_empty() {
        "none".to_owned()
    } else {
        result.matched_ports.join(",")
    };
    let matched = format!(
        "matched users [{}] ports [{}]",
        matched_users, matched_ports,
    );
    format!(
        "Access Explorer · {} · policy {} · input {} · source {} · requested_at {} · {} · {}\n{}",
        result.decision.label(),
        result.policy_hash,
        result.input,
        result.source.label(),
        result.requested_at,
        rules,
        matched,
        result.limitations.join("; ")
    )
}

fn join_u32(values: &[u32]) -> String {
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
