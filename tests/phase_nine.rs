use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use tale::action::{ActionId, Risk};
use tale::domain::Timestamp;
use tale::domain::admin_mutation::{AdminMutationState, can_transition, transition};
use tale::domain::device::{ConnectionPath, DeviceId, LocalDevice, OperatingSystem};
use tale::domain::flow::{
    AggregateDimension, FlowError, FlowFilter, FlowMessage, FlowWindow,
    aggregate_checked_cancellable,
};
use tale::domain::mutation::{
    Mutation, MutationResult, MutationState, can_transition as can_local_transition,
};
use tale::domain::source::{
    LocalFailure, LocalFailureKind, LocalResource, LocalResourceStatus, LocalSnapshot, LocalState,
};
use tale::mock;
use tale::task::{TaskState, TaskStore, bounded_detail};

const NOW: Timestamp = 1_775_000_000;

fn local_snapshot(version: &str, observed_at: Timestamp) -> LocalSnapshot {
    LocalSnapshot {
        observed_at,
        client_version: version.to_owned(),
        daemon_version: Some(version.to_owned()),
        backend_state: LocalState::Running,
        health_messages: Vec::new(),
        current_tailnet: Some("tailnet.example.test".to_owned()),
        magic_dns_suffix: Some("tailnet.example.test".to_owned()),
        cert_domains: Vec::new(),
        self_node: LocalDevice {
            id: DeviceId::new("node-01"),
            public_key: None,
            display_name: "node-01".to_owned(),
            hostname: "node-01.example.test".to_owned(),
            dns_name: None,
            os: OperatingSystem::Linux,
            version: Some(version.to_owned()),
            owner_label: None,
            user_id: None,
            tags: Vec::new(),
            tailscale_ips: vec!["100.64.0.1".to_owned()],
            advertised_routes: Vec::new(),
            current_endpoint: None,
            relay_region: None,
            path: ConnectionPath::Direct {
                latency_ms: Some(1),
            },
            online: Some(true),
            active: true,
            rx_bytes: None,
            tx_bytes: None,
            created_at: None,
            last_seen: Some(observed_at),
            last_handshake: Some(observed_at),
            exit_node: false,
            exit_node_option: false,
            ssh_host_keys_present: false,
            shared: false,
            capabilities: BTreeMap::new(),
        },
        peers: Vec::new(),
    }
}

#[test]
fn stale_generation_cannot_replace_last_good_snapshot() {
    let mut resource = LocalResource::new();
    resource.begin(1, NOW);
    assert!(resource.succeed(1, local_snapshot("1.98.9", NOW)));
    resource.begin(2, NOW.saturating_add(1));
    assert!(!resource.succeed(1, local_snapshot("obsolete", NOW.saturating_add(2))));
    assert_eq!(
        resource
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.client_version.as_str()),
        Some("1.98.9")
    );
    assert!(resource.fail(
        2,
        LocalFailure::new(
            LocalFailureKind::TimedOut,
            "status",
            "fictional timeout",
            "bounded detail",
            true,
        )
    ));
    assert_eq!(resource.status, LocalResourceStatus::Stale);
    assert_eq!(resource.consecutive_failures, 1);
    assert_eq!(
        resource
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.client_version.as_str()),
        Some("1.98.9")
    );
}

#[test]
fn local_and_admin_mutation_timeout_stages_are_terminal_without_retry() {
    assert!(can_local_transition(
        MutationState::AwaitingConfirmation,
        MutationState::CancelledBeforeDispatch
    ));
    assert!(!can_local_transition(
        MutationState::CancelledBeforeDispatch,
        MutationState::Running
    ));

    let mut local = Mutation::new(
        1,
        ActionId::LocalConnect,
        "fictional-node".to_owned(),
        (),
        Risk::Reversible,
    );
    assert!(local.transition(MutationState::Preview).is_ok());
    assert!(
        local
            .transition(MutationState::AwaitingConfirmation)
            .is_ok()
    );
    assert!(
        local
            .transition(MutationState::CancelledBeforeDispatch)
            .is_ok()
    );
    assert!(local.state.terminal());
    assert!(!can_local_transition(local.state, MutationState::Running));

    for result in [
        MutationResult::OutcomeUnknown {
            summary: "request outcome unknown".to_owned(),
            detail: "fictional timeout".to_owned(),
            exit_status: None,
        },
        MutationResult::OutcomeUnknown {
            summary: "server apply outcome unknown".to_owned(),
            detail: "response was lost".to_owned(),
            exit_status: None,
        },
    ] {
        assert!(!result.is_success());
        assert_eq!(result.exit_status(), None);
    }

    for start in [
        AdminMutationState::Dispatching,
        AdminMutationState::Verifying,
        AdminMutationState::CorrelatingAudit,
    ] {
        let mut state = start;
        assert!(transition(&mut state, AdminMutationState::OutcomeUnknown).is_ok());
        assert!(state.terminal());
        assert!(!can_transition(state, AdminMutationState::Dispatching));
    }
}

#[test]
fn bounded_history_and_detail_do_not_retain_unbounded_task_state() {
    let mut tasks = TaskStore::new();
    let max_tasks = 32;
    for cycle in 0..10_u64 {
        for index in 0..64_u64 {
            let id = tasks.create(
                ActionId::MockSuccess,
                format!("fictional-task-{cycle}-{index}"),
                NOW,
                false,
            );
            assert!(tasks.start(id));
            assert!(tasks.succeed(id, NOW.saturating_add(1), "done", "bounded"));
        }
        tasks.evict_completed(max_tasks);
        assert!(tasks.all().len() <= max_tasks);
        assert!(
            tasks
                .all()
                .iter()
                .all(|task| task.state == TaskState::Succeeded)
        );
    }
    let bounded = bounded_detail(&"x".repeat(300_000), 256 * 1024);
    assert!(bounded.len() <= 256 * 1024);
    assert!(bounded.contains("output truncated"));
}

#[test]
fn flow_cpu_work_is_bounded_cancellable_and_window_checked() {
    let mut messages = Vec::new();
    for index in 0..1_000_usize {
        messages.push(FlowMessage {
            node_id: format!("node-{index}"),
            reporting_node_name: Some("reporter.example.test".to_owned()),
            logged: "2026-08-05T00:00:00Z".to_owned(),
            start: "2026-08-05T00:00:00Z".to_owned(),
            end: "2026-08-05T00:00:01Z".to_owned(),
            source_node: None,
            destination_nodes: Vec::new(),
            virtual_traffic: Vec::new(),
            subnet_traffic: Vec::new(),
            exit_traffic: Vec::new(),
            physical_traffic: Vec::new(),
        });
    }
    let cancellation = AtomicBool::new(true);
    let result = aggregate_checked_cancellable(
        &messages,
        &FlowFilter::default(),
        &[AggregateDimension::ReportingNode],
        Some(&cancellation),
    );
    assert_eq!(result, Err(FlowError::Cancelled));
    assert!(cancellation.load(Ordering::Relaxed));

    let now = time::OffsetDateTime::from_unix_timestamp(NOW as i64);
    assert!(now.is_ok());
    if let Ok(now) = now {
        let window = FlowWindow::previous_hour(now);
        assert!(window.query_values().is_ok());
        assert!(FlowWindow::new(now - time::Duration::hours(25), now, now).is_err());
    }
}

#[test]
fn fixture_generators_are_deterministic_and_use_fictional_data() {
    let first = mock::devices();
    let second = mock::devices();
    assert_eq!(first, second);
    assert!(first.iter().all(|device| {
        device.addresses.iter().all(|address| {
            address.starts_with("192.0.2.")
                || address.starts_with("198.51.100.")
                || address.starts_with("203.0.113.")
                || address.starts_with("2001:db8:")
        })
    }));
    assert!(
        first
            .iter()
            .all(|device| device.hostname.ends_with(".example.com"))
    );
}
