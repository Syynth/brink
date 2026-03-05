mod app;
mod event;
mod typewriter;
mod ui;

use std::time::Duration;

use brink_runtime::{Program, Story};

/// Configuration for the TUI.
#[derive(Clone, Copy)]
pub struct TuiConfig {
    /// Delay between each character reveal, in milliseconds.
    pub char_delay_ms: u64,
}

/// Run the interactive TUI for playing a story.
pub fn run(
    program: &Program,
    story: &mut Story,
    config: TuiConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = ratatui::init();
    let result = run_inner(&mut terminal, program, story, config);
    ratatui::restore();
    result
}

fn run_inner(
    terminal: &mut ratatui::DefaultTerminal,
    program: &Program,
    story: &mut Story,
    config: TuiConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let char_delay = Duration::from_millis(config.char_delay_ms);
    let mut app = app::App::new(char_delay);
    app.advance_story(story, program)?;

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        let timeout = app.poll_timeout();
        let input = event::poll_input(timeout)?;
        app.handle_input(input, story, program)?;
        app.tick();
    }

    Ok(())
}
