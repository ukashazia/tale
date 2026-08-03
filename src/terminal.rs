use std::io::{self, Stdout};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
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

pub struct RealTerminal {
    pub session: TerminalSession<CrosstermControl>,
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl RealTerminal {
    pub fn enter() -> Result<Self, TaleError> {
        let session = TerminalSession::new(CrosstermControl::new())
            .map_err(|error| TaleError::Terminal(error.to_string()))?;
        let backend = CrosstermBackend::new(io::stdout());
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
