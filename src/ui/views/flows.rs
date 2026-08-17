use crate::app::App;

pub fn summary(app: &App) -> String {
    let Some(snapshot) = app.flow_snapshot.as_ref() else {
        return "Flow Logs · no bounded window observed · metadata/counters only; packet contents are unavailable"
            .to_owned();
    };
    let mode = match &snapshot.mode {
        crate::domain::flow::FlowMode::Raw => "raw",
        crate::domain::flow::FlowMode::Aggregate(_) => "aggregate",
    };
    let start = snapshot
        .window
        .start
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or("<invalid>".to_owned());
    let end = snapshot
        .window
        .end
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or("<invalid>".to_owned());
    let clock_skew = if snapshot.has_clock_skew() {
        "\nClock-skew caveat: a node-recorded start/end falls outside the queried window while server logged time is inside."
    } else {
        ""
    };
    let sample = match &snapshot.mode {
        crate::domain::flow::FlowMode::Raw => {
            let rows = snapshot
                .messages
                .iter()
                .flat_map(|message| message.records())
                .filter(|record| snapshot.filter.matches(record))
                .take(8)
                .map(|record| {
                    format!(
                        "  {} · {} · {} {} → {} · tx={}B/rx={}B",
                        record.node_id,
                        record.class.label(),
                        record.connection.proto,
                        record.connection.canonical_src(),
                        record.connection.canonical_dst(),
                        record.connection.tx_bytes,
                        record.connection.rx_bytes
                    )
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                "raw sample: no records match the active filters".to_owned()
            } else {
                format!("raw sample:\n{}", rows.join("\n"))
            }
        }
        crate::domain::flow::FlowMode::Aggregate(_) => snapshot
            .aggregates
            .as_ref()
            .map(|aggregates| {
                let rows = aggregates
                    .iter()
                    .take(8)
                    .map(|aggregate| {
                        format!(
                            "  [{}] · records={} · tx={}B/rx={}B",
                            aggregate.key.join(" · "),
                            aggregate.records,
                            aggregate.tx_bytes,
                            aggregate.rx_bytes
                        )
                    })
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    "aggregate sample: no records match the active filters".to_owned()
                } else {
                    format!("aggregate sample:\n{}", rows.join("\n"))
                }
            })
            .unwrap_or_else(|| "aggregate sample: aggregation is not available".to_owned()),
    };
    format!(
        "Flow Logs · {} records from {} messages · {} mode · {} · {} UTC → {} UTC\nReporting node IDs and node timestamps are preserved. Packet contents are not returned by this API.{}{}\n{}",
        snapshot.visible_record_count(),
        snapshot.messages.len(),
        mode,
        if snapshot.complete {
            "complete"
        } else {
            "partial"
        },
        start,
        end,
        clock_skew,
        snapshot
            .limitation
            .as_deref()
            .map_or(String::new(), |value| format!("\nLimit: {value}")),
        sample
    )
}
