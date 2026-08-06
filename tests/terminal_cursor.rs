use std::sync::{Arc, Mutex};

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use tale::terminal::SteadyCursor;

/// Records the escape-sequence level operations a backend would emit.
#[derive(Clone, Default)]
struct Recorder {
    operations: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn record(&self, operation: String) {
        if let Ok(mut operations) = self.operations.lock() {
            operations.push(operation);
        }
    }

    fn take(&self) -> Vec<String> {
        match self.operations.lock() {
            Ok(mut operations) => std::mem::take(&mut *operations),
            Err(_) => Vec::new(),
        }
    }
}

impl Backend for Recorder {
    type Error = std::io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let count = content.count();
        if count > 0 {
            self.record(format!("draw({count})"));
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.record("hide".to_owned());
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.record("show".to_owned());
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(Position::new(0, 0))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let position = position.into();
        self.record(format!("move({},{})", position.x, position.y));
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.record("clear".to_owned());
        Ok(())
    }

    fn clear_region(&mut self, _clear_type: ClearType) -> Result<(), Self::Error> {
        self.record("clear_region".to_owned());
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size::new(80, 24))
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: Size::new(80, 24),
            pixels: Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The sequence ratatui runs for one frame that wants a cursor.
fn frame(backend: &mut SteadyCursor<Recorder>, cells: usize, position: Position) {
    let cell = Cell::default();
    let content = (0..cells).map(|index| {
        let index = u16::try_from(index).map_or(u16::MAX, |value| value);
        (index, 0, &cell)
    });
    assert!(backend.draw(content).is_ok());
    assert!(backend.show_cursor().is_ok());
    assert!(backend.set_cursor_position(position).is_ok());
    assert!(backend.flush().is_ok());
}

#[test]
fn the_cursor_is_moved_before_it_is_shown_and_never_toggles_afterwards() {
    let recorder = Recorder::default();
    let observed = recorder.clone();
    let mut backend = SteadyCursor::new(recorder);

    // First frame: nothing to hide yet, and the cursor only appears once it has
    // been moved onto the prompt.
    frame(&mut backend, 3, Position::new(10, 20));
    assert_eq!(observed.take(), vec!["draw(3)", "move(10,20)", "show"]);

    // Later repaints only move it. Cell writes are queued and reach the
    // terminal together with the move, so there is nothing to hide from, and an
    // off/on cycle here would restart the blink on every keystroke.
    frame(&mut backend, 5, Position::new(10, 20));
    assert_eq!(observed.take(), vec!["draw(5)", "move(10,20)"]);

    frame(&mut backend, 2, Position::new(11, 20));
    assert_eq!(observed.take(), vec!["draw(2)", "move(11,20)"]);
}

#[test]
fn a_frame_that_changes_nothing_leaves_the_cursor_completely_alone() {
    let recorder = Recorder::default();
    let observed = recorder.clone();
    let mut backend = SteadyCursor::new(recorder);
    frame(&mut backend, 2, Position::new(4, 9));
    let _ = observed.take();

    // No cells and the same position: the terminal hears nothing at all, so its
    // blink phase is never restarted.
    for _ in 0..3 {
        frame(&mut backend, 0, Position::new(4, 9));
        assert!(observed.take().is_empty());
    }

    // Moving the caret is a real change and does reach the terminal.
    frame(&mut backend, 0, Position::new(5, 9));
    assert_eq!(observed.take(), vec!["move(5,9)"]);
}

#[test]
fn hiding_is_issued_once_and_only_when_the_cursor_was_visible() {
    let recorder = Recorder::default();
    let observed = recorder.clone();
    let mut backend = SteadyCursor::new(recorder);

    // Frames with no cursor request never emit a redundant hide.
    assert!(backend.hide_cursor().is_ok());
    assert!(backend.hide_cursor().is_ok());
    assert!(observed.take().is_empty());

    frame(&mut backend, 1, Position::new(0, 0));
    assert_eq!(observed.take(), vec!["draw(1)", "move(0,0)", "show"]);

    assert!(backend.hide_cursor().is_ok());
    assert!(backend.hide_cursor().is_ok());
    assert_eq!(observed.take(), vec!["hide"]);

    // And it comes back exactly once when an editor is opened again.
    frame(&mut backend, 1, Position::new(2, 2));
    assert_eq!(observed.take(), vec!["draw(1)", "move(2,2)", "show"]);
    frame(&mut backend, 1, Position::new(3, 2));
    assert_eq!(observed.take(), vec!["draw(1)", "move(3,2)"]);
}

#[test]
fn clearing_forces_the_next_frame_to_reposition_the_cursor() {
    let recorder = Recorder::default();
    let observed = recorder.clone();
    let mut backend = SteadyCursor::new(recorder);
    frame(&mut backend, 1, Position::new(7, 7));
    let _ = observed.take();

    // A clear moves the real cursor, so the cached position cannot be trusted
    // and the next frame has to put it back even though nothing else changed.
    assert!(backend.clear().is_ok());
    frame(&mut backend, 0, Position::new(7, 7));
    assert_eq!(observed.take(), vec!["clear", "move(7,7)"]);
}
