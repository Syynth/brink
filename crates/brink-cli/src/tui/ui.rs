use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};

use super::app::{App, Focus, Passage, Phase};

/// Muted blue for history text — lower contrast without being invisible.
const HISTORY_COLOR: Color = Color::Rgb(100, 115, 150);
const HISTORY_CHOICE_COLOR: Color = Color::Rgb(120, 130, 160);

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // ── Stable layout: story + choices + status ──
    let choice_height = u16::try_from(app.choice_count().max(1) + 2).unwrap_or(5);
    let chunks = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(choice_height),
        Constraint::Length(1),
    ])
    .split(area);

    // ── Story area ──
    let story_area = chunks[0];
    let story_border = if app.focus == Focus::Story {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let story_block = Block::default()
        .borders(Borders::ALL)
        .border_style(story_border)
        .padding(Padding::horizontal(1));
    let story_inner = story_block.inner(story_area);
    frame.render_widget(story_block, story_area);

    // Build content lines
    let (full_text, revealed_bytes) = app.render_text();
    let mut lines = build_story_lines(&app.history, full_text, revealed_bytes);

    // Center a fixed-width column sized to the widest line in the full
    // content (history + current passage). This prevents horizontal
    // shifting during typewriter reveal.
    let column_width = max_content_width(&app.history, full_text)
        .max(1)
        .min(story_inner.width);
    let centered = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(column_width),
        Constraint::Fill(1),
    ])
    .split(story_inner);
    let text_area = centered[1];

    // Bottom-align: pad with empty lines at the top.
    // Count visual rows (accounting for word-wrap) rather than logical lines.
    let inner_height = text_area.height;
    let total_visual = visual_row_count(&lines, text_area.width);
    let padding = inner_height.saturating_sub(total_visual);
    if padding > 0 {
        lines.splice(0..0, (0..padding).map(|_| Line::from("")));
    }

    // Scroll: start at the bottom, then apply user scrollback.
    let padded_visual = total_visual + padding;
    let natural_scroll = padded_visual.saturating_sub(inner_height);
    let clamped_offset = app.scroll_offset.min(natural_scroll);
    let effective_scroll = natural_scroll.saturating_sub(clamped_offset);

    let story_widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((effective_scroll, 0));

    frame.render_widget(story_widget, text_area);

    // ── Choice box (always present) ──
    let choice_lines = build_choices(app);
    let choice_border = if app.focus == Focus::Choices {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let choice_title = if matches!(app.phase, Phase::Choosing { .. }) {
        " Choices "
    } else {
        ""
    };

    let choices_widget = Paragraph::new(choice_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(choice_border)
            .title(choice_title)
            .padding(Padding::horizontal(1)),
    );

    frame.render_widget(choices_widget, chunks[1]);

    // ── Status bar ──
    let status_text = match (&app.phase, app.focus) {
        (Phase::Typing { .. }, _) => "[Space] skip  [↑/↓] scroll  [q] quit",
        (Phase::Choosing { .. }, Focus::Choices) => {
            "[↑/↓] select  [Enter] confirm  [Tab] scroll  [q] quit"
        }
        (Phase::Choosing { .. }, Focus::Story) => "[↑/↓] scroll  [Tab] choices  [q] quit",
        (Phase::Ended { .. }, _) => "[↑/↓] scroll  [q] quit",
    };
    let status = Paragraph::new(Line::from(Span::styled(
        status_text,
        Style::default().fg(Color::DarkGray),
    )))
    .alignment(Alignment::Center);

    frame.render_widget(status, chunks[2]);
}

/// Build all story lines: dimmed history + typewriter-revealed current passage.
fn build_story_lines<'a>(
    history: &'a [Passage],
    full_text: &'a str,
    revealed_bytes: usize,
) -> Vec<Line<'a>> {
    let mut lines: Vec<Line> = Vec::new();

    let dim = Style::default().fg(HISTORY_COLOR);
    let dim_choice = Style::default()
        .fg(HISTORY_CHOICE_COLOR)
        .add_modifier(Modifier::ITALIC);

    for passage in history {
        for raw_line in passage.text.lines() {
            lines.push(Line::from(Span::styled(raw_line, dim)));
        }
        if let Some(chosen) = &passage.chosen {
            lines.push(Line::from(Span::styled(
                format!("  >> {chosen}"),
                dim_choice,
            )));
        }
        lines.push(Line::from(""));
    }

    // Current passage — iterate the *full* text's lines so vertical space
    // is reserved from the first frame. Only the revealed portion is visible.
    let mut byte_pos: usize = 0;
    for (i, line_str) in full_text.split('\n').enumerate() {
        if i > 0 {
            byte_pos += 1; // '\n' separator
        }
        let line_start = byte_pos;
        let line_end = line_start + line_str.len();

        if line_end <= revealed_bytes {
            lines.push(Line::from(line_str));
        } else if line_start < revealed_bytes {
            let split = revealed_bytes - line_start;
            lines.push(Line::from(line_str[..split].to_owned()));
        } else {
            lines.push(Line::from(""));
        }

        byte_pos = line_end;
    }

    lines
}

/// Widest line across history passages and the full current passage text.
fn max_content_width(history: &[Passage], full_text: &str) -> u16 {
    let history_max = history
        .iter()
        .flat_map(|p| {
            let text_widths = p.text.lines().map(str::len);
            let choice_width = p.chosen.as_ref().map(|c| c.len() + 5);
            text_widths.chain(choice_width)
        })
        .max()
        .unwrap_or(0);

    let current_max = full_text.lines().map(str::len).max().unwrap_or(0);

    u16::try_from(history_max.max(current_max)).unwrap_or(u16::MAX)
}

/// Count visual rows that `lines` will occupy when wrapped at `width`.
fn visual_row_count(lines: &[Line<'_>], width: u16) -> u16 {
    let w = usize::from(width.max(1));
    let total: usize = lines
        .iter()
        .map(|line| {
            let lw = line.width();
            if lw == 0 { 1 } else { lw.div_ceil(w) }
        })
        .sum();
    u16::try_from(total).unwrap_or(u16::MAX)
}

/// Build choice lines with typewriter reveal applied per-choice.
fn build_choices(app: &App) -> Vec<Line<'_>> {
    let Phase::Choosing {
        choices, selected, ..
    } = &app.phase
    else {
        return Vec::new();
    };

    let reveal = app.render_choices();
    let revealed_bytes = reveal.map_or(usize::MAX, |(_, rb)| rb);

    let mut byte_pos: usize = 0;
    choices
        .iter()
        .enumerate()
        .map(|(i, choice)| {
            if i > 0 {
                byte_pos += 1; // '\n' separator in concatenated text
            }
            let line_start = byte_pos;
            let line_end = line_start + choice.text.len();

            let visible = if line_end <= revealed_bytes {
                choice.text.as_str()
            } else if line_start < revealed_bytes {
                &choice.text[..revealed_bytes - line_start]
            } else {
                ""
            };

            byte_pos = line_end;

            let marker = if i == *selected { ">> " } else { "   " };
            let style = if i == *selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            Line::from(Span::styled(format!("{marker}{visible}"), style))
        })
        .collect()
}
