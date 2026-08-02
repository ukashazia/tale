use std::sync::{Arc, Mutex};

use tale::terminal::{TerminalControl, TerminalError, TerminalSession};

#[derive(Clone)]
struct FakeTerminal {
    calls: Arc<Mutex<Vec<&'static str>>>,
    fail: Option<&'static str>,
}

impl FakeTerminal {
    fn new(fail: Option<&'static str>) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail,
        }
    }

    fn call(&mut self, name: &'static str) -> Result<(), TerminalError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(name);
        }
        if self.fail == Some(name) {
            Err(TerminalError::Operation(name.to_owned()))
        } else {
            Ok(())
        }
    }
}

impl TerminalControl for FakeTerminal {
    fn enable_raw(&mut self) -> Result<(), TerminalError> {
        self.call("enable_raw")
    }
    fn disable_raw(&mut self) -> Result<(), TerminalError> {
        self.call("disable_raw")
    }
    fn enter_alternate(&mut self) -> Result<(), TerminalError> {
        self.call("enter_alternate")
    }
    fn leave_alternate(&mut self) -> Result<(), TerminalError> {
        self.call("leave_alternate")
    }
    fn enable_paste(&mut self) -> Result<(), TerminalError> {
        self.call("enable_paste")
    }
    fn disable_paste(&mut self) -> Result<(), TerminalError> {
        self.call("disable_paste")
    }
    fn enable_mouse(&mut self) -> Result<(), TerminalError> {
        self.call("enable_mouse")
    }
    fn disable_mouse(&mut self) -> Result<(), TerminalError> {
        self.call("disable_mouse")
    }
    fn hide_cursor(&mut self) -> Result<(), TerminalError> {
        self.call("hide_cursor")
    }
    fn show_cursor(&mut self) -> Result<(), TerminalError> {
        self.call("show_cursor")
    }
}

fn calls(fake: &FakeTerminal) -> Vec<&'static str> {
    match fake.calls.lock() {
        Ok(calls) => calls.clone(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn normal_cleanup_restores_every_acquired_state_in_reverse_order_and_is_idempotent() {
    let fake = FakeTerminal::new(None);
    let observed = fake.clone();
    let session = TerminalSession::new_with_mouse(fake, true);
    assert!(session.is_ok());
    if let Ok(mut session) = session {
        let cleaned = session.cleanup();
        assert!(cleaned.is_ok());
        let after_first = calls(&observed);
        let cleaned_again = session.cleanup();
        assert!(cleaned_again.is_ok());
        assert_eq!(calls(&observed), after_first);
    }
    assert_eq!(
        calls(&observed),
        vec![
            "enable_raw",
            "enter_alternate",
            "enable_paste",
            "enable_mouse",
            "hide_cursor",
            "show_cursor",
            "disable_mouse",
            "disable_paste",
            "leave_alternate",
            "disable_raw",
        ]
    );
}

#[test]
fn setup_failure_restores_only_states_that_were_acquired() {
    for failing_operation in [
        "enter_alternate",
        "enable_paste",
        "enable_mouse",
        "hide_cursor",
    ] {
        let fake = FakeTerminal::new(Some(failing_operation));
        let observed = fake.clone();
        let session = TerminalSession::new_with_mouse(fake, true);
        assert!(session.is_err());
        let calls = calls(&observed);
        assert!(calls.contains(&failing_operation));
        if failing_operation == "enter_alternate" {
            assert!(calls.contains(&"disable_raw"));
            assert!(!calls.contains(&"leave_alternate"));
        }
        if failing_operation == "enable_paste" {
            assert!(calls.contains(&"leave_alternate"));
            assert!(calls.contains(&"disable_raw"));
            assert!(!calls.contains(&"disable_paste"));
        }
        if failing_operation == "enable_mouse" {
            assert!(calls.contains(&"disable_paste"));
            assert!(!calls.contains(&"disable_mouse"));
        }
        if failing_operation == "hide_cursor" {
            assert!(calls.contains(&"disable_mouse"));
        }
    }
}

#[test]
fn cleanup_attempts_remaining_states_after_an_injected_restore_failure() {
    let fake = FakeTerminal::new(Some("disable_paste"));
    let observed = fake.clone();
    let session = TerminalSession::new_with_mouse(fake, true);
    assert!(session.is_ok());
    if let Ok(mut session) = session {
        let cleanup = session.cleanup();
        assert!(cleanup.is_err());
    }
    let calls = calls(&observed);
    assert!(calls.ends_with(&[
        "show_cursor",
        "disable_mouse",
        "disable_paste",
        "leave_alternate",
        "disable_raw",
    ]));
}

#[cfg(target_os = "macos")]
#[test]
fn binary_restores_a_real_pty_after_quit() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let log = std::env::temp_dir().join(format!("tale-pty-{}.log", std::process::id()));
    let mut child = Command::new("script")
        .args(["-q"])
        .arg(&log)
        .arg(env!("CARGO_BIN_EXE_tale"))
        .arg("--mock")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    assert!(child.is_ok());
    if let Ok(ref mut child) = child {
        if let Some(mut stdin) = child.stdin.take() {
            let write = stdin.write_all(b"q");
            assert!(write.is_ok());
        }
        let status = child.wait();
        assert!(status.is_ok());
        if let Ok(status) = status {
            assert_eq!(status.code(), Some(0));
        }
    }
    let contents = std::fs::read_to_string(&log);
    assert!(contents.is_ok());
    if let Ok(contents) = contents {
        assert!(contents.contains("?1049l"));
        assert!(contents.contains("?2004l"));
        assert!(contents.contains("?25h"));
    }
    let _ = std::fs::remove_file(log);
}
