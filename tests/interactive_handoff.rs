use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tale::local::handoff::{
    HandoffError, bounded_handoff_duration, login_command, nc_command, run, ssh_command,
};
use tale::terminal::{TerminalControl, TerminalError, TerminalSession};

#[test]
fn handoff_arguments_remain_typed_and_restricted() {
    let ssh = ssh_command(Path::new("tailscale"), Some("alice"), "node.example");
    assert!(ssh.is_ok());
    if let Ok(ssh) = ssh {
        assert_eq!(
            ssh.args(),
            vec![OsString::from("ssh"), OsString::from("alice@node.example")]
        );
    }
    let nc = nc_command(Path::new("tailscale"), "100.64.0.10", "443");
    assert!(nc.is_ok());
    if let Ok(nc) = nc {
        assert_eq!(
            nc.args(),
            vec![
                OsString::from("nc"),
                OsString::from("100.64.0.10"),
                OsString::from("443"),
            ]
        );
    }
    assert!(matches!(
        ssh_command(Path::new("tailscale"), Some("alice@evil"), "node"),
        Err(HandoffError::InvalidArgument(_))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn direct_children_report_success_and_non_zero_exit_without_output_capture() {
    let success = run(login_command(Path::new("/usr/bin/true"))).await;
    assert!(success.is_ok());
    if let Ok(success) = success {
        assert_eq!(success.exit_status, Some(0));
        assert!(bounded_handoff_duration(&success) <= std::time::Duration::from_secs(2));
    }

    let failure = run(login_command(Path::new("/usr/bin/false"))).await;
    assert!(failure.is_ok());
    if let Ok(failure) = failure {
        assert_ne!(failure.exit_status, Some(0));
    }
}

#[tokio::test]
async fn failed_spawn_is_distinct_from_a_child_exit() {
    let result = run(login_command(Path::new(
        "/path/that/does/not/exist/tailscale",
    )))
    .await;
    assert!(matches!(result, Err(HandoffError::Spawn(_))));
}

#[derive(Clone)]
struct FakeTerminal {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl FakeTerminal {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn record(&mut self, name: &'static str) -> Result<(), TerminalError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(name);
        }
        Ok(())
    }
}

impl TerminalControl for FakeTerminal {
    fn enable_raw(&mut self) -> Result<(), TerminalError> {
        self.record("enable_raw")
    }

    fn disable_raw(&mut self) -> Result<(), TerminalError> {
        self.record("disable_raw")
    }

    fn enter_alternate(&mut self) -> Result<(), TerminalError> {
        self.record("enter_alternate")
    }

    fn leave_alternate(&mut self) -> Result<(), TerminalError> {
        self.record("leave_alternate")
    }

    fn enable_paste(&mut self) -> Result<(), TerminalError> {
        self.record("enable_paste")
    }

    fn disable_paste(&mut self) -> Result<(), TerminalError> {
        self.record("disable_paste")
    }

    fn enable_mouse(&mut self) -> Result<(), TerminalError> {
        self.record("enable_mouse")
    }

    fn disable_mouse(&mut self) -> Result<(), TerminalError> {
        self.record("disable_mouse")
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalError> {
        self.record("hide_cursor")
    }

    fn show_cursor(&mut self) -> Result<(), TerminalError> {
        self.record("show_cursor")
    }
}

#[test]
fn terminal_is_released_for_handoff_and_reacquired_once() {
    let fake = FakeTerminal::new();
    let observed = fake.clone();
    let session = TerminalSession::new(fake);
    assert!(session.is_ok());
    if let Ok(mut session) = session {
        assert!(session.suspend().is_ok());
        assert!(session.resume(false).is_ok());
        assert!(session.cleanup().is_ok());
    }
    let calls = match observed.calls.lock() {
        Ok(calls) => calls.clone(),
        Err(_) => Vec::new(),
    };
    assert_eq!(
        calls,
        vec![
            "enable_raw",
            "enter_alternate",
            "enable_paste",
            "hide_cursor",
            "show_cursor",
            "disable_paste",
            "leave_alternate",
            "disable_raw",
            "enable_raw",
            "enter_alternate",
            "enable_paste",
            "hide_cursor",
            "show_cursor",
            "disable_paste",
            "leave_alternate",
            "disable_raw",
        ]
    );
}
