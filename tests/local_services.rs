use std::path::Path;
use std::time::Duration;

use tale::action::{ActionId, Risk};
use tale::domain::certificate::CertificateRequest;
use tale::domain::service::{
    Backend, Exposure, Listener, PathMount, Port, ProxyProtocol, ServiceActionRequest,
};
use tale::domain::transfer::TaildropConflict;
use tale::local::certificates::certificate_command;
use tale::local::services::{
    bugreport_command, funnel_status_command, mapping_command, parse_bugreport_identifier,
    parse_funnel_status, parse_serve_status, redacted_metrics, serve_status_command,
};
use tale::local::transfers::{
    drive_list_command, drive_rename_command, drive_share_command, drive_unshare_command,
    parse_drive_list, parse_taildrop_progress, parse_taildrop_targets, taildrop_receive_command,
    taildrop_send_command,
};

fn valid_port(value: u16) -> Option<Port> {
    Port::new(value).ok()
}

#[test]
fn serve_fixtures_decode_unknown_fields_and_reject_incomplete_documents() {
    let empty = include_str!("fixtures/local/services/1.98.9/linux/serve-empty.json");
    let https = include_str!("fixtures/local/services/1.98.9/linux/serve-https.json");
    let files = include_str!("fixtures/local/services/1.98.9/linux/serve-filesystem.json");
    let tcp = include_str!("fixtures/local/services/1.98.9/linux/serve-tcp.json");
    let unix = include_str!("fixtures/local/services/1.98.9/linux/serve-unix.json");
    let malformed = include_str!("fixtures/local/services/1.98.9/linux/serve-malformed.json");
    let incomplete = include_str!("fixtures/local/services/1.98.9/linux/serve-incomplete.json");
    let live = include_str!("fixtures/local/services/1.98.9/linux/serve-live.json");

    assert!(parse_serve_status(empty).is_ok());
    let https_status = parse_serve_status(https);
    assert_eq!(
        https_status.as_ref().map(|status| status.mappings.len()),
        Ok(1)
    );
    if let Ok(status) = https_status {
        let Some(port) = valid_port(443) else {
            return;
        };
        assert_eq!(status.mappings[0].listener, Listener::Https(port));
        assert_eq!(status.mappings[0].mount, PathMount::Root);
    }
    let files_status = parse_serve_status(files);
    assert_eq!(
        files_status
            .as_ref()
            .ok()
            .and_then(|status| status.mappings.first())
            .map(|mapping| mapping.backend.argument()),
        Some("/srv/tale files".to_owned())
    );
    let tcp_status = parse_serve_status(tcp);
    assert_eq!(
        tcp_status
            .as_ref()
            .ok()
            .and_then(|status| status.mappings.first())
            .map(|mapping| mapping.proxy_protocol),
        Some(ProxyProtocol::Version1)
    );
    assert!(parse_serve_status(unix).is_ok());
    let unix_status = parse_serve_status(unix);
    assert_eq!(
        unix_status
            .as_ref()
            .ok()
            .and_then(|status| status.mappings.first())
            .map(|mapping| mapping.backend.argument()),
        Some("unix:/tmp/tale.sock".to_owned())
    );
    assert!(parse_serve_status(live).is_ok());
    assert_eq!(
        parse_serve_status(live).map(|status| status.mappings.len()),
        Ok(0)
    );
    assert_eq!(
        parse_funnel_status(live).map(|status| status.mappings.len()),
        Ok(1)
    );
    assert!(parse_serve_status(malformed).is_err());
    assert!(parse_serve_status(incomplete).is_err());
}

#[test]
fn funnel_is_decoded_separately_and_http_is_not_a_public_listener() {
    let public = include_str!("fixtures/local/services/1.98.9/linux/funnel-public.json");
    let status = parse_funnel_status(public);
    assert_eq!(status.as_ref().map(|value| value.mappings.len()), Ok(1));
    if let Ok(status) = status {
        assert_eq!(status.mappings[0].exposure, Exposure::Public);
        let request = ServiceActionRequest::Funnel {
            mapping: status.mappings[0].clone(),
            edit: false,
        };
        assert_eq!(request.risk(), Risk::Disruptive);
        assert_eq!(request.action_id(), ActionId::ServicesFunnelCreate);
    }
    let http = ServiceActionRequest::Funnel {
        mapping: tale::domain::service::ServiceMapping {
            exposure: Exposure::Public,
            listener: Listener::Http({
                let Some(port) = valid_port(80) else {
                    return;
                };
                port
            }),
            mount: PathMount::Root,
            backend: Backend::Port({
                let Some(port) = valid_port(8080) else {
                    return;
                };
                port
            }),
            proxy_protocol: ProxyProtocol::None,
            hostname: None,
        },
        edit: false,
    };
    assert!(
        mapping_command(
            Path::new("tailscale"),
            Duration::from_secs(1),
            match &http {
                ServiceActionRequest::Funnel { mapping, .. } => mapping,
                _ => return,
            },
            true
        )
        .is_err()
    );
}

#[test]
fn service_commands_keep_values_as_distinct_arguments() {
    let mapping = tale::domain::service::ServiceMapping {
        exposure: Exposure::Tailnet,
        listener: Listener::Https({
            let Some(port) = valid_port(443) else {
                return;
            };
            port
        }),
        mount: PathMount::Path({
            let Ok(path) = tale::domain::service::AbsoluteUrlPath::new("/with spaces") else {
                return;
            };
            path
        }),
        backend: Backend::HttpUrl("http://127.0.0.1:3000".to_owned()),
        proxy_protocol: ProxyProtocol::None,
        hostname: None,
    };
    let command = mapping_command(
        Path::new("/tmp/tailscale"),
        Duration::from_secs(1),
        &mapping,
        true,
    );
    assert!(command.is_ok());
    if let Ok(command) = command {
        let args = command
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"--set-path=/with spaces".to_owned()));
        assert!(args.contains(&"http://127.0.0.1:3000".to_owned()));
        assert!(!args.iter().any(|value| value.contains("$(sh")));
    }
    let status_command = serve_status_command(Path::new("tailscale"), Duration::from_secs(1));
    assert_eq!(
        status_command
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["serve", "status", "--json"]
    );
    let funnel_command = funnel_status_command(Path::new("tailscale"), Duration::from_secs(1));
    assert_eq!(
        funnel_command
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["funnel", "status", "--json"]
    );
}

#[test]
fn bugreport_and_certificate_commands_do_not_expose_key_contents() {
    let command = bugreport_command(
        Path::new("tailscale"),
        Duration::from_secs(1),
        Some("note with spaces"),
        true,
    );
    assert!(command.is_ok());
    if let Ok(command) = command {
        assert_eq!(
            command
                .args
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["bugreport", "--diagnose", "note with spaces"]
        );
    }
    let identifier = parse_bugreport_identifier(include_str!(
        "fixtures/local/services/1.98.9/linux/bugreport.txt"
    ));
    assert!(identifier.is_ok());
    if let Ok(identifier) = identifier {
        assert!(identifier.starts_with("BUG-"));
    }
    let request = CertificateRequest {
        domain: "node.example.ts.net".to_owned(),
        certificate_path: Path::new("/tmp/cert.pem").to_path_buf(),
        key_path: Path::new("/tmp/key.pem").to_path_buf(),
        min_validity: Some("120h".to_owned()),
        overwrites_existing: false,
    };
    let command = certificate_command(Path::new("tailscale"), Duration::from_secs(1), &request);
    assert!(command.is_ok());
    if let Ok(command) = command {
        let args = command
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            args.iter()
                .any(|value| value == "--cert-file=/tmp/cert.pem")
        );
        assert!(args.iter().any(|value| value == "--key-file=/tmp/key.pem"));
        assert!(args.iter().all(|value| !value.contains("PRIVATE KEY")));
        assert!(args.iter().all(|value| value != "--serve-demo"));
    }
}

#[test]
fn parser_and_transfer_commands_cover_targets_progress_and_conflicts() {
    let targets = parse_taildrop_targets(include_str!(
        "fixtures/local/transfers/1.98.9/linux/taildrop-targets.tsv"
    ));
    assert_eq!(targets.as_ref().map(|value| value.len()), Ok(2));
    if let Ok(targets) = targets {
        assert!(targets.iter().any(|target| !target.available()));
        assert!(
            targets
                .iter()
                .any(|target| target.command_target == "100.64.0.2")
        );
    }
    let additive = parse_taildrop_targets(include_str!(
        "fixtures/local/transfers/1.98.9/linux/taildrop-targets-additive.tsv"
    ));
    assert_eq!(additive.as_ref().map(|value| value.len()), Ok(1));
    let minimal = parse_taildrop_targets(include_str!(
        "fixtures/local/transfers/1.98.9/linux/taildrop-targets-minimal.tsv"
    ));
    assert_eq!(
        minimal
            .as_ref()
            .ok()
            .and_then(|targets| targets.first())
            .map(|target| target.device_name.as_str()),
        Some("not returned")
    );
    let progress = parse_taildrop_progress("copying file 25%", 100);
    assert_eq!(progress.as_ref().and_then(|value| value.percent), Some(25));
    let bytes = parse_taildrop_progress("1024/4096 bytes", 100);
    assert_eq!(
        bytes.as_ref().and_then(|value| value.completed_bytes),
        Some(1024)
    );
    assert!(parse_taildrop_progress("waiting", 100).is_none());

    let send = taildrop_send_command(
        Path::new("tailscale"),
        Duration::from_secs(1),
        &[
            Path::new("/tmp/a path").to_path_buf(),
            Path::new("/tmp/b").to_path_buf(),
        ],
        "100.64.0.2",
    );
    assert!(send.is_ok());
    if let Ok(send) = send {
        let args = send
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"/tmp/a path".to_owned()));
        assert_eq!(args.last().map(String::as_str), Some("100.64.0.2:"));
    }
    let receive = taildrop_receive_command(
        Path::new("tailscale"),
        Duration::from_secs(1),
        Path::new("/tmp/inbox"),
        TaildropConflict::Overwrite,
        true,
    );
    assert!(receive.is_ok());
    if let Ok(receive) = receive {
        let args = receive
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.contains(&"--conflict=overwrite".to_owned()));
        assert!(args.contains(&"--wait".to_owned()));
        assert!(args.contains(&"/tmp/inbox".to_owned()));
    }
    let drive_command = drive_list_command(Path::new("tailscale"), Duration::from_secs(1));
    assert_eq!(
        drive_command
            .args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec!["drive", "list"]
    );
}

#[test]
fn taildrive_table_parser_preserves_spaces_and_rejects_mismatch() {
    let empty = parse_drive_list(include_str!(
        "fixtures/local/transfers/1.98.9/linux/drive-empty.txt"
    ));
    assert_eq!(empty, Ok(Vec::new()));
    let spaces = parse_drive_list(include_str!(
        "fixtures/local/transfers/1.98.9/linux/drive-spaces.txt"
    ));
    assert!(spaces.is_ok());
    if let Ok(shares) = spaces {
        assert_eq!(
            shares[0].path,
            Path::new("/srv/tale/path with spaces/files")
        );
    }
    let additive = parse_drive_list(include_str!(
        "fixtures/local/transfers/1.98.9/linux/drive-additive.txt"
    ));
    assert!(additive.is_ok());
    let malformed = parse_drive_list(include_str!(
        "fixtures/local/transfers/1.98.9/linux/drive-malformed.txt"
    ));
    assert!(malformed.is_err());
}

#[test]
fn typed_values_normalize_names_and_redact_metrics_without_guessing() {
    let normalized = tale::domain::transfer::normalize_share_name("  Project Docs  ");
    assert_eq!(normalized, Ok("project docs".to_owned()));
    assert!(tale::domain::transfer::normalize_share_name("project/docs").is_err());

    let metrics = redacted_metrics(
        b"safe_metric 1\napi_key=do-not-display\nAuthorization: Bearer do-not-display\nvalue=2",
    );
    assert!(metrics.contains("safe_metric 1"));
    assert!(metrics.contains("[redacted]"));
    assert!(!metrics.contains("do-not-display"));

    assert!(parse_bugreport_identifier("BUG-1 and BUG-2").is_err());
    assert!(parse_bugreport_identifier("no identifier").is_err());
    let note = bugreport_command(
        Path::new("tailscale"),
        Duration::from_secs(1),
        Some("first line\nsecond\tline"),
        false,
    );
    assert!(note.is_ok());
    if let Ok(note) = note {
        assert_eq!(
            note.args
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["bugreport", "first line\nsecond\tline"]
        );
    }
}

#[test]
fn taildrive_mutation_commands_keep_names_and_paths_distinct() {
    let share = drive_share_command(
        Path::new("tailscale"),
        Duration::from_secs(1),
        "project docs",
        Path::new("/srv/tale/project docs"),
    );
    assert!(share.is_ok());
    if let Ok(share) = share {
        assert_eq!(
            share
                .args
                .iter()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["drive", "share", "project docs", "/srv/tale/project docs"]
        );
    }
    let rename = drive_rename_command(
        Path::new("tailscale"),
        Duration::from_secs(1),
        "old share",
        "new share",
    );
    assert!(rename.is_ok());
    let unshare =
        drive_unshare_command(Path::new("tailscale"), Duration::from_secs(1), "new share");
    assert!(unshare.is_ok());
}
