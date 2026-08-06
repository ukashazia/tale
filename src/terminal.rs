use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use thiserror::Error;

use crate::error::TaleError;

pub trait TerminalControl {
    fn enable_raw(&mut self) -> Result<(), TerminalError>;
    fn disable_raw(&mut self) -> Result<(), TerminalError>;
    fn enter_alternate(&mut self) -> Result<(), TerminalError>;
    fn leave_alternate(&mut self) -> Result<(), TerminalError>;
    fn enable_paste(&mut self) -> Result<(), TerminalError>;
    fn disable_paste(&mut self) -> Result<(), TerminalError>;
    fn enable_mouse(&mut self) -> Result<(), TerminalError>;
    fn disable_mouse(&mut self) -> Result<(), TerminalError>;
    fn hide_cursor(&mut self) -> Result<(), TerminalError>;
    fn show_cursor(&mut self) -> Result<(), TerminalError>;
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum TerminalError {
    #[error("terminal operation failed: {0}")]
    Operation(String),
    #[error("terminal cleanup failed: {0}")]
    Cleanup(String),
}

#[derive(Clone, Eq, PartialEq)]
pub struct EditorCommand {
    executable: PathBuf,
    arguments: Vec<String>,
}

impl std::fmt::Debug for EditorCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EditorCommand")
            .field("executable", &self.executable)
            .field("arguments", &self.arguments)
            .finish()
    }
}

impl EditorCommand {
    pub fn parse(value: &str) -> Result<Self, EditorError> {
        if value.trim().is_empty() || contains_shell_syntax(value) {
            return Err(EditorError::InvalidCommand);
        }
        let parts = shlex::split(value).ok_or(EditorError::InvalidCommand)?;
        let mut parts = parts.into_iter();
        let executable = parts.next().ok_or(EditorError::InvalidCommand)?;
        if executable.is_empty() || is_shell_executable(&executable) {
            return Err(EditorError::InvalidCommand);
        }
        Ok(Self {
            executable: PathBuf::from(executable),
            arguments: parts.collect(),
        })
    }

    pub fn from_environment() -> Result<Self, EditorError> {
        let visual = std::env::var("VISUAL").ok();
        let editor = std::env::var("EDITOR").ok();
        visual
            .filter(|value| !value.trim().is_empty())
            .or(editor.filter(|value| !value.trim().is_empty()))
            .ok_or(EditorError::NotConfigured)
            .and_then(|value| Self::parse(&value))
    }

    pub fn executable(&self) -> &Path {
        self.executable.as_path()
    }

    pub fn arguments(&self) -> &[String] {
        self.arguments.as_slice()
    }

    pub fn argv_with_path(&self, path: &Path) -> Vec<std::ffi::OsString> {
        self.arguments
            .iter()
            .map(std::ffi::OsString::from)
            .chain(std::iter::once(path.as_os_str().to_owned()))
            .collect()
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum EditorError {
    #[error("no usable external editor is configured")]
    NotConfigured,
    #[error("the external editor command is invalid")]
    InvalidCommand,
    #[error("the external editor could not be started")]
    Spawn,
    #[error("the external editor could not be waited for")]
    Wait,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EditorExit {
    pub success: bool,
    pub code: Option<i32>,
    pub elapsed: Duration,
}

impl EditorCommand {
    pub async fn run(&self, path: &Path) -> Result<EditorExit, EditorError> {
        let started = SystemTime::now();
        let mut command = tokio::process::Command::new(self.executable.as_os_str());
        command
            .args(self.argv_with_path(path))
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        command.kill_on_drop(true);
        let status = command.status().await.map_err(|_| EditorError::Spawn)?;
        let elapsed = SystemTime::now()
            .duration_since(started)
            .map_or(Duration::ZERO, |value| value);
        Ok(EditorExit {
            success: status.success(),
            code: status.code(),
            elapsed,
        })
    }
}

fn contains_shell_syntax(value: &str) -> bool {
    value.chars().any(|character| {
        matches!(
            character,
            ';' | '|' | '&' | '>' | '<' | '`' | '$' | '\n' | '\r'
        )
    })
}

fn is_shell_executable(value: &str) -> bool {
    let executable = match value.rsplit(['/', '\\']).next() {
        Some(value) => value,
        None => value,
    }
    .to_ascii_lowercase();
    matches!(
        executable.as_str(),
        "sh" | "sh.exe"
            | "bash"
            | "bash.exe"
            | "zsh"
            | "zsh.exe"
            | "dash"
            | "dash.exe"
            | "fish"
            | "fish.exe"
            | "ksh"
            | "ksh.exe"
            | "cmd"
            | "cmd.exe"
            | "command.com"
            | "powershell"
            | "powershell.exe"
            | "pwsh"
            | "pwsh.exe"
    )
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
struct AcquiredStates {
    raw: bool,
    alternate: bool,
    paste: bool,
    mouse: bool,
    cursor_hidden: bool,
}

pub struct TerminalSession<C: TerminalControl> {
    control: C,
    acquired: AcquiredStates,
    cleaned: bool,
    cleanup_error: Option<TerminalError>,
}

impl<C: TerminalControl> TerminalSession<C> {
    pub fn new(control: C) -> Result<Self, TerminalError> {
        Self::new_with_mouse(control, false)
    }

    pub fn new_with_mouse(control: C, mouse: bool) -> Result<Self, TerminalError> {
        let mut session = Self {
            control,
            acquired: AcquiredStates::default(),
            cleaned: false,
            cleanup_error: None,
        };
        if let Err(error) = session.acquire(mouse) {
            let cleanup = session.cleanup();
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(TerminalError::Operation(format!(
                    "{error}; cleanup also failed: {cleanup_error}"
                ))),
            };
        }
        Ok(session)
    }

    fn acquire(&mut self, mouse: bool) -> Result<(), TerminalError> {
        self.control.enable_raw()?;
        self.acquired.raw = true;
        self.control.enter_alternate()?;
        self.acquired.alternate = true;
        self.control.enable_paste()?;
        self.acquired.paste = true;
        if mouse {
            self.control.enable_mouse()?;
            self.acquired.mouse = true;
        }
        // The cursor shape and blink are never changed, so editors show whatever
        // insertion point the terminal is configured to draw.
        self.control.hide_cursor()?;
        self.acquired.cursor_hidden = true;
        Ok(())
    }

    pub fn suspend(&mut self) -> Result<(), TerminalError> {
        if self.cleaned {
            return Err(TerminalError::Operation(
                "terminal session has already been cleaned up".to_owned(),
            ));
        }
        self.release()
    }

    pub fn resume(&mut self, mouse: bool) -> Result<(), TerminalError> {
        if self.cleaned {
            return Err(TerminalError::Operation(
                "terminal session has already been cleaned up".to_owned(),
            ));
        }
        if self.acquired != AcquiredStates::default() {
            return Ok(());
        }
        match self.acquire(mouse) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = self.release();
                Err(error)
            }
        }
    }

    pub fn cleanup(&mut self) -> Result<(), TerminalError> {
        if self.cleaned {
            return self.cleanup_error.clone().map_or(Ok(()), Err);
        }
        self.cleaned = true;
        self.release()
    }

    fn release(&mut self) -> Result<(), TerminalError> {
        let mut errors = Vec::new();
        if self.acquired.cursor_hidden
            && let Err(error) = self.control.show_cursor()
        {
            errors.push(error.to_string());
        }
        if self.acquired.mouse
            && let Err(error) = self.control.disable_mouse()
        {
            errors.push(error.to_string());
        }
        if self.acquired.paste
            && let Err(error) = self.control.disable_paste()
        {
            errors.push(error.to_string());
        }
        if self.acquired.alternate
            && let Err(error) = self.control.leave_alternate()
        {
            errors.push(error.to_string());
        }
        if self.acquired.raw
            && let Err(error) = self.control.disable_raw()
        {
            errors.push(error.to_string());
        }
        self.acquired = AcquiredStates::default();
        if errors.is_empty() {
            Ok(())
        } else {
            let error = TerminalError::Cleanup(errors.join("; "));
            self.cleanup_error = Some(error.clone());
            Err(error)
        }
    }

    pub fn cleanup_error(&self) -> Option<&TerminalError> {
        self.cleanup_error.as_ref()
    }
}

impl<C: TerminalControl> Drop for TerminalSession<C> {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.cleanup();
        }
    }
}

pub struct CrosstermControl {
    stdout: Stdout,
}

impl CrosstermControl {
    pub fn new() -> Self {
        Self {
            stdout: io::stdout(),
        }
    }

    fn execute_command<T: crossterm::Command>(&mut self, command: T) -> Result<(), TerminalError> {
        execute!(self.stdout, command).map_err(|error| TerminalError::Operation(error.to_string()))
    }
}

impl Default for CrosstermControl {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalControl for CrosstermControl {
    fn enable_raw(&mut self) -> Result<(), TerminalError> {
        enable_raw_mode().map_err(|error| TerminalError::Operation(error.to_string()))
    }

    fn disable_raw(&mut self) -> Result<(), TerminalError> {
        disable_raw_mode().map_err(|error| TerminalError::Operation(error.to_string()))
    }

    fn enter_alternate(&mut self) -> Result<(), TerminalError> {
        self.execute_command(EnterAlternateScreen)
    }

    fn leave_alternate(&mut self) -> Result<(), TerminalError> {
        self.execute_command(LeaveAlternateScreen)
    }

    fn enable_paste(&mut self) -> Result<(), TerminalError> {
        self.execute_command(crossterm::event::EnableBracketedPaste)
    }

    fn disable_paste(&mut self) -> Result<(), TerminalError> {
        self.execute_command(crossterm::event::DisableBracketedPaste)
    }

    fn enable_mouse(&mut self) -> Result<(), TerminalError> {
        self.execute_command(EnableMouseCapture)
    }

    fn disable_mouse(&mut self) -> Result<(), TerminalError> {
        self.execute_command(DisableMouseCapture)
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalError> {
        self.execute_command(Hide)
    }

    fn show_cursor(&mut self) -> Result<(), TerminalError> {
        self.execute_command(Show)
    }
}

/// Keeps the terminal's own cursor steady across repaints.
///
/// Ratatui queues a frame's cell writes but then shows and moves the cursor with
/// immediately flushed commands. That leaves the cursor visible for one flush
/// wherever the last cell landed before it jumps to the prompt, and it restarts
/// the terminal's blink timer on every repaint, however little changed. This
/// backend hides the cursor while cells are written, moves before it shows, and
/// stays silent when a frame leaves the cursor exactly where it already was.
pub struct SteadyCursor<B> {
    inner: B,
    /// `None` once cell writes have left the cursor somewhere unknown.
    position: Option<ratatui::layout::Position>,
    shown: bool,
    wanted: bool,
}

impl<B> SteadyCursor<B> {
    pub const fn new(inner: B) -> Self {
        Self {
            inner,
            position: None,
            shown: false,
            wanted: false,
        }
    }
}

impl<B: Backend> Backend for SteadyCursor<B> {
    type Error = B::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        let mut content = content.peekable();
        if content.peek().is_none() {
            // An empty diff changes nothing, including where the cursor sits.
            return Ok(());
        }
        // Cell writes are queued, not flushed, so they reach the terminal
        // together with the move that follows. Hiding the cursor here would only
        // add a visible off/on cycle to every repaint.
        self.inner.draw(content)?;
        self.position = None;
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.wanted = false;
        if self.shown {
            self.inner.hide_cursor()?;
            self.shown = false;
        }
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        // Deferred until the position is known, so it never appears mid-frame.
        self.wanted = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<ratatui::layout::Position, Self::Error> {
        let position = self.inner.get_cursor_position()?;
        self.position = Some(position);
        Ok(position)
    }

    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        position: P,
    ) -> Result<(), Self::Error> {
        let position = position.into();
        if self.position != Some(position) {
            self.inner.set_cursor_position(position)?;
            self.position = Some(position);
        }
        if self.wanted && !self.shown {
            self.inner.show_cursor()?;
            self.shown = true;
        }
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.position = None;
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ratatui::backend::ClearType) -> Result<(), Self::Error> {
        self.position = None;
        self.inner.clear_region(clear_type)
    }

    fn append_lines(&mut self, lines: u16) -> Result<(), Self::Error> {
        self.position = None;
        self.inner.append_lines(lines)
    }

    fn size(&self) -> Result<ratatui::layout::Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }
}

pub struct RealTerminal {
    pub session: TerminalSession<CrosstermControl>,
    pub terminal: Terminal<SteadyCursor<CrosstermBackend<Stdout>>>,
}

impl RealTerminal {
    pub fn enter() -> Result<Self, TaleError> {
        Self::enter_with_mouse(false)
    }

    pub fn enter_with_mouse(mouse: bool) -> Result<Self, TaleError> {
        let session = TerminalSession::new_with_mouse(CrosstermControl::new(), mouse)
            .map_err(|error| TaleError::Terminal(error.to_string()))?;
        let backend = SteadyCursor::new(CrosstermBackend::new(io::stdout()));
        let terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let mut session = session;
                let cleanup = session.cleanup();
                return match cleanup {
                    Ok(()) => Err(TaleError::Terminal(error.to_string())),
                    Err(cleanup_error) => Err(TaleError::Terminal(format!(
                        "{error}; cleanup also failed: {cleanup_error}"
                    ))),
                };
            }
        };
        Ok(Self { session, terminal })
    }

    pub fn restore(&mut self) -> Result<(), TaleError> {
        self.session
            .cleanup()
            .map_err(|error| TaleError::Terminal(error.to_string()))
    }

    pub fn suspend_for_handoff(&mut self) -> Result<(), TaleError> {
        self.session
            .suspend()
            .map_err(|error| TaleError::Terminal(error.to_string()))
    }

    pub fn resume_after_handoff(&mut self) -> Result<(), TaleError> {
        self.session
            .resume(false)
            .map_err(|error| TaleError::Terminal(error.to_string()))?;
        self.terminal
            .clear()
            .map_err(|error| TaleError::Terminal(error.to_string()))
    }
}
