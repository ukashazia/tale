use crate::app::App;

pub fn summary(app: &App) -> String {
    if app.health_findings.is_empty() {
        return "Health · no issues found in the latest tailnet data".to_owned();
    }
    let mut lines = vec![format!(
        "Health · {} derived finding{} · label: Derived by Tale",
        app.health_findings.len(),
        if app.health_findings.len() == 1 {
            ""
        } else {
            "s"
        }
    )];
    for finding in app.health_findings.iter().take(6) {
        let affected = finding.affected_resource_ids.join(", ");
        let facts = finding
            .observed_facts
            .iter()
            .take(3)
            .map(|fact| format!("{}={}", fact.label, fact.value))
            .collect::<Vec<_>>()
            .join(", ");
        let sources = if finding.source_ids.is_empty() {
            "not returned".to_owned()
        } else {
            finding.source_ids.join(", ")
        };
        lines.push(format!(
            "Derived by Tale · {} · {} · {} · [{}] · facts [{}] · source [{}]",
            finding.severity.label(),
            finding.rule_id,
            finding.title,
            affected,
            facts,
            sources,
        ));
    }
    if app.health_findings.len() > 6 {
        lines.push(format!(
            "… {} additional derived finding{}",
            app.health_findings.len().saturating_sub(6),
            if app.health_findings.len() == 7 {
                ""
            } else {
                "s"
            }
        ));
    }
    lines.join("\n")
}
