use std::collections::BTreeSet;
use std::process::Command;

use tale::domain::device::{ConnectionPath, Liveness, OperatingSystem};
use tale::mock::{self, MockLoadScenario};

#[test]
fn deterministic_fixture_covers_the_phase_one_domain_contract() {
    let first = mock::devices();
    let second = mock::devices();
    assert_eq!(first, second);
    assert_eq!(first.len(), 14);

    assert!(
        first
            .iter()
            .any(|device| device.liveness == Liveness::Online)
    );
    assert!(
        first
            .iter()
            .any(|device| device.liveness == Liveness::Offline)
    );
    assert!(
        first
            .iter()
            .any(|device| device.liveness == Liveness::Unknown)
    );
    assert!(
        first
            .iter()
            .any(|device| matches!(device.path, ConnectionPath::Direct { .. }))
    );
    assert!(
        first
            .iter()
            .any(|device| matches!(device.path, ConnectionPath::Derp { .. }))
    );
    assert!(
        first
            .iter()
            .any(|device| matches!(device.path, ConnectionPath::PeerRelay { .. }))
    );
    assert!(
        first
            .iter()
            .any(|device| device.path == ConnectionPath::NoPath)
    );

    let operating_systems: BTreeSet<_> = first.iter().map(|device| device.os.label()).collect();
    for operating_system in ["linux", "macos", "windows", "ios", "android"] {
        assert!(operating_systems.contains(operating_system));
    }
    assert!(
        first
            .iter()
            .any(|device| matches!(device.os, OperatingSystem::Unknown(_)))
    );
    assert!(first.iter().any(|device| device.owner.is_some()));
    assert!(first.iter().any(|device| device.owner.is_none()));
    assert!(first.iter().any(|device| device.addresses.is_empty()));
    assert!(
        first
            .iter()
            .any(|device| device.addresses.iter().any(|address| address.contains(':')))
    );
    assert!(first.iter().any(|device| device.tags.len() > 1));
    assert!(
        first
            .iter()
            .any(|device| device.display_name.chars().count() > 32)
    );
    assert!(first.iter().any(|device| !device.display_name.is_ascii()));
    assert!(first.iter().any(|device| device.capabilities.exit_node));
    assert!(first.iter().any(|device| device.capabilities.subnet_router));
    assert!(first.iter().any(|device| device.capabilities.ssh));
    assert!(first.iter().any(|device| device.capabilities.funnel));
    assert!(first.iter().any(|device| device.capabilities.shared));
    assert!(first.iter().any(|device| device.capabilities.expired));
    assert!(first.iter().any(|device| !device.capabilities.approved));
    assert!(first.iter().all(|device| {
        device.addresses.iter().all(|address| {
            address.starts_with("192.0.2.")
                || address.starts_with("198.51.100.")
                || address.starts_with("203.0.113.")
                || address.starts_with("2001:db8:")
        })
    }));
    assert!(first.iter().all(|device| {
        device
            .owner
            .as_deref()
            .is_none_or(|owner| owner.ends_with("@example.com"))
    }));
}

#[test]
fn scenarios_are_repeatable_and_distinguish_stale_and_failure() {
    let initial = mock::load_devices(MockLoadScenario::Initial);
    let success = mock::load_devices(MockLoadScenario::Success);
    assert_eq!(initial, success);
    if let Ok((devices, observed_at)) = initial {
        assert_eq!(devices, mock::devices());
        assert_eq!(observed_at, mock::MOCK_NOW);
    }

    let stale = mock::load_devices(MockLoadScenario::Stale);
    assert!(stale.is_ok());
    if let Ok((devices, observed_at)) = stale {
        assert_eq!(devices, mock::devices());
        assert_eq!(observed_at, mock::MOCK_NOW.saturating_sub(240));
    }

    let failure = mock::load_devices(MockLoadScenario::Failure);
    assert!(failure.is_err());
    if let Err(detail) = failure {
        assert_eq!(detail, "mock refresh failed: fictional source timeout");
    }
}

#[test]
fn mock_doctor_reports_no_real_adapters() {
    let output = Command::new(env!("CARGO_BIN_EXE_tale"))
        .args(["doctor", "--mock"])
        .output();
    assert!(output.is_ok());
    if let Ok(output) = output {
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"source_mode\": \"mock\""));
        assert!(stdout.contains("doctor does not spawn a local process"));
        assert!(stdout.contains("doctor does not contact the Control API"));
        assert!(stdout.contains("credential store content"));
        assert!(!stdout.contains("TALE_ACCESS_TOKEN"));
    }
}
