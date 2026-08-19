use crate::app::App;

pub fn summary(app: &App) -> String {
    if app.webhooks.is_empty() {
        return "Webhooks · none configured · secrets are never shown".to_owned();
    }
    let mut lines = vec![format!(
        "Webhooks · {} configured · secrets are never shown",
        app.webhooks.len(),
    )];
    lines.extend(
        app.webhooks
        .iter()
        .map(|webhook| {
            format!(
                "{} · {} · {} · url={} · created by={} · last result={} · created={} · updated={} · subscriptions [{}]",
                webhook.stable_id,
                webhook.destination_type.wire_value(),
                webhook.status,
                redact_endpoint(&webhook.endpoint_url),
                webhook
                    .creator_login_name
                    .as_deref()
                    .map_or("not returned", |value| value),
                webhook
                    .last_result
                    .as_deref()
                    .map_or("not returned", |value| value),
                webhook.created_at.as_deref().map_or("not returned", |value| value),
                webhook
                    .last_modified_at
                    .as_deref()
                    .map_or("not returned", |value| value),
                webhook.subscriptions.wire_subscriptions().join(", ")
            )
        })
        .collect::<Vec<_>>(),
    );
    lines.join("\n")
}

fn redact_endpoint(value: &str) -> String {
    url::Url::parse(value).map_or_else(
        |_| "<invalid endpoint>".to_owned(),
        |url| {
            format!(
                "https://{}{}",
                url.host_str().map_or("host unavailable", |host| host),
                url.path()
            )
        },
    )
}
