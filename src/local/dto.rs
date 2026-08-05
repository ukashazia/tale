use serde_json::Value;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VersionDto {
    pub cli_version: String,
    pub daemon_version: Option<String>,
    pub build: Option<String>,
    pub raw: Value,
}

pub fn decode_version(input: &str) -> Result<super::client::VersionInfo, String> {
    let value: Value =
        serde_json::from_str(input).map_err(|error| format!("invalid JSON: {error}"))?;
    let cli_version = first_string(&value, &["version", "short", "full", "Version"])
        .ok_or_else(|| "required CLI version was not returned".to_owned())?;
    let daemon_version = first_string(&value, &["daemonVersion", "DaemonVersion"]);
    let build = first_string(&value, &["gitCommit", "GitCommit", "commit", "full"]);
    Ok(super::client::VersionInfo {
        version: cli_version,
        daemon_version,
        build,
    })
}

fn first_string(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}
