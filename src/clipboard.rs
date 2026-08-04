use std::fmt;

use crate::domain::secret_result::SecretBuffer;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ClipboardError {
    Unavailable,
    CopyFailed,
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("clipboard is unavailable"),
            Self::CopyFailed => formatter.write_str("clipboard copy failed"),
        }
    }
}

impl std::error::Error for ClipboardError {}

pub trait ClipboardSink {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError>;
}

pub struct SystemClipboard {
    clipboard: arboard::Clipboard,
}

impl fmt::Debug for SystemClipboard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SystemClipboard(<connected>)")
    }
}

impl SystemClipboard {
    pub fn new() -> Result<Self, ClipboardError> {
        arboard::Clipboard::new()
            .map(|clipboard| Self { clipboard })
            .map_err(|_| ClipboardError::Unavailable)
    }
}

impl ClipboardSink for SystemClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.clipboard
            .set_text(text.to_owned())
            .map_err(|_| ClipboardError::CopyFailed)
    }
}

pub fn copy_secret<S: ClipboardSink>(
    sink: &mut S,
    secret: &SecretBuffer,
) -> Result<(), ClipboardError> {
    let text = secret.as_str().ok_or(ClipboardError::CopyFailed)?;
    sink.set_text(text)
}

#[cfg(test)]
mod tests {
    use super::{ClipboardError, ClipboardSink, copy_secret};
    use crate::domain::secret_result::SecretBuffer;

    struct FakeClipboard {
        copied: String,
    }

    impl ClipboardSink for FakeClipboard {
        fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.copied.push_str(text);
            Ok(())
        }
    }

    #[test]
    fn copying_is_explicit_and_does_not_clear_existing_clipboard() {
        let mut sink = FakeClipboard {
            copied: "existing".to_owned(),
        };
        let secret = SecretBuffer::new("fictional-secret-canary");
        let copied = copy_secret(&mut sink, &secret);
        assert!(copied.is_ok());
        assert_eq!(sink.copied, "existingfictional-secret-canary");
    }
}
