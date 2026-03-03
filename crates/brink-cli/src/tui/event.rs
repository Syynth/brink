use std::io;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

/// Abstracted input events for the TUI.
#[derive(Clone, Copy)]
pub enum Input {
    Quit,
    Skip,
    Up,
    Down,
    Confirm,
    Tab,
    None,
}

/// Poll for a single input event, waiting up to `timeout`.
pub fn poll_input(timeout: Duration) -> io::Result<Input> {
    if event::poll(timeout)?
        && let Event::Key(key) = event::read()?
    {
        if key.kind != KeyEventKind::Press {
            return Ok(Input::None);
        }
        return Ok(match key.code {
            KeyCode::Char('q') => Input::Quit,
            KeyCode::Char(' ') => Input::Skip,
            KeyCode::Up => Input::Up,
            KeyCode::Down => Input::Down,
            KeyCode::Enter => Input::Confirm,
            KeyCode::Tab | KeyCode::BackTab => Input::Tab,
            _ => Input::None,
        });
    }
    Ok(Input::None)
}
