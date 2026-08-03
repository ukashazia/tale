use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use crate::domain::Timestamp;
use crate::domain::transfer::{
    TaildriveShare, TaildropConflict, TaildropTarget, TransferProgress, normalize_share_name,
};
use crate::local::process::{LocalCommand, LocalOperation, OutputMode};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TransferParseError {
    pub detail: String,
}

impl TransferParseError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for TransferParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for TransferParseError {}

pub fn taildrop_targets_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildropTargets,
        vec![
            OsString::from("file"),
            OsString::from("cp"),
            OsString::from("--targets"),
        ],
    )
    .with_timeout(timeout)
    .with_modes(OutputMode::Collect, OutputMode::Collect)
}

pub fn taildrop_send_command(
    path: &Path,
    timeout: Duration,
    files: &[std::path::PathBuf],
    target: &str,
) -> Result<LocalCommand, TransferParseError> {
    if files.is_empty() {
        return Err(TransferParseError::new("at least one file is required"));
    }
    if target.is_empty() || target.chars().any(char::is_control) {
        return Err(TransferParseError::new("Taildrop target is invalid"));
    }
    let mut args = vec![
        OsString::from("file"),
        OsString::from("cp"),
        OsString::from("--update-interval=1s"),
    ];
    args.extend(files.iter().map(|file| file.as_os_str().to_os_string()));
    args.push(OsString::from(format!("{target}:")));
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildropSend,
        args,
    )
    .with_timeout(timeout)
    .with_modes(OutputMode::Lines, OutputMode::Lines))
}

pub fn taildrop_receive_command(
    path: &Path,
    timeout: Duration,
    directory: &Path,
    conflict: TaildropConflict,
    wait: bool,
) -> Result<LocalCommand, TransferParseError> {
    if directory.as_os_str().is_empty() || directory.as_os_str() == "-" {
        return Err(TransferParseError::new("receive directory is invalid"));
    }
    let mut args = vec![
        OsString::from("file"),
        OsString::from("get"),
        OsString::from(format!("--conflict={}", conflict.label())),
    ];
    if wait {
        args.push(OsString::from("--wait"));
    }
    args.push(directory.as_os_str().to_os_string());
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildropReceive,
        args,
    )
    .with_timeout(timeout)
    .with_modes(OutputMode::Lines, OutputMode::Lines))
}

pub fn drive_list_command(path: &Path, timeout: Duration) -> LocalCommand {
    LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildriveList,
        vec![OsString::from("drive"), OsString::from("list")],
    )
    .with_timeout(timeout)
}

pub fn drive_share_command(
    path: &Path,
    timeout: Duration,
    name: &str,
    directory: &Path,
) -> Result<LocalCommand, TransferParseError> {
    let normalized = normalize_share_name(name).map_err(TransferParseError::new)?;
    if normalized != name {
        return Err(TransferParseError::new(
            "share command requires the already-normalized name",
        ));
    }
    if directory.as_os_str().is_empty() || directory.as_os_str() == "-" {
        return Err(TransferParseError::new("share directory is invalid"));
    }
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildriveShare,
        vec![
            OsString::from("drive"),
            OsString::from("share"),
            OsString::from(name),
            directory.as_os_str().to_os_string(),
        ],
    )
    .with_timeout(timeout))
}

pub fn drive_rename_command(
    path: &Path,
    timeout: Duration,
    old_name: &str,
    new_name: &str,
) -> Result<LocalCommand, TransferParseError> {
    if normalize_share_name(old_name).map_err(TransferParseError::new)? != old_name
        || normalize_share_name(new_name).map_err(TransferParseError::new)? != new_name
    {
        return Err(TransferParseError::new(
            "rename command requires already-normalized names",
        ));
    }
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildriveRename,
        vec![
            OsString::from("drive"),
            OsString::from("rename"),
            OsString::from(old_name),
            OsString::from(new_name),
        ],
    )
    .with_timeout(timeout))
}

pub fn drive_unshare_command(
    path: &Path,
    timeout: Duration,
    name: &str,
) -> Result<LocalCommand, TransferParseError> {
    if normalize_share_name(name).map_err(TransferParseError::new)? != name {
        return Err(TransferParseError::new(
            "unshare command requires an already-normalized name",
        ));
    }
    Ok(LocalCommand::new(
        path.as_os_str().to_os_string(),
        LocalOperation::TaildriveUnshare,
        vec![
            OsString::from("drive"),
            OsString::from("unshare"),
            OsString::from(name),
        ],
    )
    .with_timeout(timeout))
}

pub fn parse_taildrop_targets(input: &str) -> Result<Vec<TaildropTarget>, TransferParseError> {
    let mut targets = Vec::new();
    for (line_number, line) in input.lines().enumerate() {
        let line = line.trim_end();
        if line.trim().is_empty() || is_target_header(line) || line.starts_with('#') {
            continue;
        }
        let fields = if line.contains('\t') {
            line.split('\t').map(str::trim).collect::<Vec<_>>()
        } else {
            split_target_fixed_line(line)
        };
        if fields.len() < 2 {
            return Err(TransferParseError::new(format!(
                "Taildrop target line {} does not contain stable target and display fields",
                line_number.saturating_add(1)
            )));
        }
        let command_target = fields[0].to_owned();
        if command_target.is_empty() || command_target.chars().any(char::is_control) {
            return Err(TransferParseError::new(
                "Taildrop target is empty or invalid",
            ));
        }
        let display_name = fields[1].to_owned();
        let device_name = fields
            .get(2)
            .filter(|value| !value.is_empty())
            .map_or_else(|| "not returned".to_owned(), |value| (*value).to_owned());
        if display_name.is_empty() {
            return Err(TransferParseError::new(
                "Taildrop target display names must not be empty",
            ));
        }
        let online = fields.get(3).and_then(|value| parse_online(value));
        let capability_reason = fields
            .get(4)
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("available"))
            .map(|value| (*value).to_owned());
        targets.push(TaildropTarget {
            command_target,
            display_name,
            device_name,
            online,
            capability_reason,
        });
    }
    targets.sort_by(|left, right| left.command_target.cmp(&right.command_target));
    if targets
        .windows(2)
        .any(|window| window[0].command_target == window[1].command_target)
    {
        return Err(TransferParseError::new(
            "Taildrop target output contains duplicate command targets",
        ));
    }
    Ok(targets)
}

pub fn parse_drive_list(input: &str) -> Result<Vec<TaildriveShare>, TransferParseError> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let lines = input.lines().collect::<Vec<_>>();
    let header_index = lines
        .iter()
        .position(|line| has_drive_header(line))
        .ok_or_else(|| TransferParseError::new("Taildrive list header was not recognized"))?;
    let header = lines[header_index];
    let starts = column_starts(header);
    if starts.len() < 2 {
        return Err(TransferParseError::new(
            "Taildrive list requires NAME and PATH columns",
        ));
    }
    let mut shares = Vec::new();
    for line in lines.iter().skip(header_index.saturating_add(1)) {
        if line.trim().is_empty() || line.trim_start().starts_with('-') {
            continue;
        }
        let fields = if line.contains('\t') {
            line.split('\t').map(str::trim).collect::<Vec<_>>()
        } else {
            slice_columns(line, &starts)
        };
        if fields.len() < 2 || fields[0].is_empty() || fields[1].is_empty() {
            return Err(TransferParseError::new(
                "Taildrive list row does not contain a name and path",
            ));
        }
        shares.push(TaildriveShare {
            name: fields[0].to_owned(),
            path: std::path::PathBuf::from(fields[1]),
            as_user: fields
                .get(2)
                .filter(|value| !value.is_empty())
                .map(|value| (*value).to_owned()),
        });
    }
    Ok(shares)
}

pub fn parse_taildrop_progress(line: &str, observed_at: Timestamp) -> Option<TransferProgress> {
    let line = line.trim();
    let percent = line
        .split_whitespace()
        .find_map(|token| {
            token
                .strip_suffix('%')
                .and_then(|value| value.parse::<u8>().ok())
        })
        .filter(|value| *value <= 100);
    let bytes = line
        .split_whitespace()
        .find_map(|token| token.split_once('/'))
        .and_then(|(completed, total)| {
            Some((completed.parse::<u64>().ok()?, total.parse::<u64>().ok()?))
        });
    if percent.is_none() && bytes.is_none() {
        return None;
    }
    Some(TransferProgress {
        completed_bytes: bytes.map(|values| values.0),
        total_bytes: bytes.map(|values| values.1),
        percent,
        observed_at,
    })
}

fn is_target_header(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("target") && (lower.contains("device") || lower.contains("name"))
}

fn split_target_fixed_line(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

fn parse_online(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "online" | "yes" | "true" | "available" => Some(true),
        "offline" | "no" | "false" | "unavailable" => Some(false),
        _ => None,
    }
}

fn has_drive_header(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("name") && lower.contains("path")
}

fn column_starts(header: &str) -> Vec<usize> {
    header
        .split_whitespace()
        .filter_map(|value| header.find(value))
        .collect()
}

fn slice_columns<'a>(line: &'a str, starts: &[usize]) -> Vec<&'a str> {
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts
                .get(index.saturating_add(1))
                .copied()
                .unwrap_or(line.len());
            line.get(*start..end).unwrap_or("").trim()
        })
        .collect()
}
