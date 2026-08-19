use crate::app::App;

pub fn summary(app: &App) -> String {
    if app.log_stream_configurations.is_empty() && app.log_stream_statuses.is_empty() {
        return "Log streams · configuration and publishing status are shown separately; no secret values are shown"
            .to_owned();
    }
    let mut lines = vec!["Log streams · Activity/Settings".to_owned()];
    for log_type in [
        crate::domain::log_stream::LogType::Configuration,
        crate::domain::log_stream::LogType::Network,
    ] {
        let configuration = app.log_stream_configurations.get(&log_type);
        let status = app.log_stream_statuses.get(&log_type);
        lines.push(format!(
            "{} · configured={} · destination={} · status={} · healthy={} · observed={}",
            log_type.wire_value(),
            configuration.is_some(),
            configuration.map_or("not observed", |value| value.destination.identity.as_str()),
            status.map_or("not observed", |value| value.status.as_str()),
            status.map_or("not observed", |value| {
                value
                    .healthy
                    .map_or("unknown", |healthy| if healthy { "yes" } else { "no" })
            }),
            status
                .and_then(|value| value.last_observation)
                .map_or_else(|| "not observed".to_owned(), |value| value.to_string())
        ));
    }
    lines.join("\n")
}
