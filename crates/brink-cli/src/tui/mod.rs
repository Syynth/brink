mod app;
mod event;
mod typewriter;
mod ui;

use std::path::Path;
use std::time::Duration;

use brink_format::{LineEntry, LocaleData};
use brink_runtime::{LocaleMode, Program, Story, apply_locale};

/// Configuration for the TUI.
#[derive(Clone)]
pub struct TuiConfig {
    /// Delay between each character reveal, in milliseconds.
    pub char_delay_ms: u64,
}

/// A loaded locale that can be applied at runtime.
pub struct LoadedLocale {
    pub label: String,
    pub data: LocaleData,
}

/// Run the interactive TUI for playing a story.
pub fn run(
    program: &Program,
    base_line_tables: &[Vec<LineEntry>],
    locales: &[LoadedLocale],
    config: &TuiConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = ratatui::init();
    let result = run_inner(&mut terminal, program, base_line_tables, locales, config);
    ratatui::restore();
    result
}

fn run_inner(
    terminal: &mut ratatui::DefaultTerminal,
    program: &Program,
    base_line_tables: &[Vec<LineEntry>],
    locales: &[LoadedLocale],
    config: &TuiConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let char_delay = Duration::from_millis(config.char_delay_ms);

    // Build locale labels: "base" + each loaded locale.
    let mut locale_labels: Vec<String> = vec!["base".to_string()];
    locale_labels.extend(locales.iter().map(|l| l.label.clone()));

    let mut app = app::App::new(char_delay, locale_labels);
    let mut story = Story::new(program, base_line_tables.to_vec());
    app.advance_story(&mut story)?;

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        let timeout = app.poll_timeout();
        let input = event::poll_input(timeout)?;

        // Check if the app wants a locale switch before normal input handling.
        if let Some(locale_idx) = app.handle_input(input, &mut story)? {
            // Snapshot story state, swap locale, restore.
            let (snapshot, _old_tables) = story.into_snapshot();
            let new_tables = if locale_idx > 0 {
                apply_locale(
                    program,
                    &locales[locale_idx - 1].data,
                    base_line_tables,
                    LocaleMode::Overlay,
                )?
            } else {
                base_line_tables.to_vec()
            };
            story = Story::from_snapshot(program, snapshot, new_tables);
        }

        app.tick();
    }

    Ok(())
}

/// Load locale files from disk.
pub fn load_locales(paths: &[&Path]) -> Result<Vec<LoadedLocale>, Box<dyn std::error::Error>> {
    let mut locales = Vec::new();
    for path in paths {
        let bytes = std::fs::read(path)?;
        let data = brink_format::read_inkl(&bytes)?;
        let label = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        locales.push(LoadedLocale { label, data });
    }
    Ok(locales)
}
