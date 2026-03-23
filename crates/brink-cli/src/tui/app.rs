use std::time::Duration;

use brink_runtime::{Line, Story};

use super::event::Input;
use super::typewriter::TypewriterState;

/// A passage of text that has been fully revealed (history entry).
pub struct Passage {
    pub text: String,
    pub chosen: Option<ChosenEntry>,
    /// Index into the story transcript where this passage ends.
    /// Used for re-rendering on locale switch.
    pub transcript_end: usize,
}

/// A choice that was selected — stores the display text and an optional
/// fragment reference for re-rendering on locale switch.
pub struct ChosenEntry {
    pub text: String,
    /// If the choice was backed by a fragment, its index for re-resolution.
    pub fragment_idx: Option<u32>,
}

/// A choice the player can select.
pub struct ChoiceEntry {
    pub text: String,
    pub index: usize,
}

/// Which panel currently owns Up/Down input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Story,
    Choices,
}

/// What happens after the current passage finishes typing.
pub(super) enum AfterPassage {
    ShowChoices(Vec<ChoiceEntry>),
    End,
}

/// The current phase of the TUI state machine.
pub enum Phase {
    /// Text is being revealed character by character.
    Typing {
        typewriter: TypewriterState,
        then: AfterPassage,
    },
    /// All text is visible, awaiting player choice. Choice text is revealed
    /// via its own typewriter (concatenated choice texts joined by `\n`).
    Choosing {
        text: String,
        choices: Vec<ChoiceEntry>,
        selected: usize,
        typewriter: TypewriterState,
    },
    /// Story is over.
    Ended { text: String },
}

/// State for the locale-switching modal overlay.
pub struct LocaleModal {
    pub labels: Vec<String>,
    pub selected: usize,
}

/// Top-level TUI application state.
pub struct App {
    pub phase: Phase,
    pub focus: Focus,
    pub history: Vec<Passage>,
    pub scroll_offset: u16,
    pub should_quit: bool,
    pub locale_modal: Option<LocaleModal>,
    pub active_locale: usize,
    locale_labels: Vec<String>,
    char_delay: Duration,
    /// Transcript end index for the current (in-progress) passage.
    current_transcript_end: usize,
}

impl App {
    pub fn new(char_delay: Duration, locale_labels: Vec<String>) -> Self {
        Self {
            phase: Phase::Ended {
                text: String::new(),
            },
            focus: Focus::Story,
            history: Vec::new(),
            scroll_offset: 0,
            should_quit: false,
            locale_modal: None,
            active_locale: 0,
            locale_labels,
            char_delay,
            current_transcript_end: 0,
        }
    }

    /// Step the story forward and enter the appropriate phase.
    pub fn advance_story(&mut self, story: &mut Story) -> Result<(), Box<dyn std::error::Error>> {
        self.scroll_offset = 0;
        self.focus = Focus::Story;

        let lines = story.continue_maximally()?;
        let text: String = lines.iter().map(Line::text).collect();
        self.current_transcript_end = story.transcript_len();
        match lines.last() {
            Some(Line::Choices { choices, .. }) => {
                let entries: Vec<ChoiceEntry> = choices
                    .iter()
                    .map(|c| ChoiceEntry {
                        text: c.text.clone(),
                        index: c.index,
                    })
                    .collect();
                self.phase = Phase::Typing {
                    typewriter: TypewriterState::new(text, self.char_delay),
                    then: AfterPassage::ShowChoices(entries),
                };
            }
            Some(Line::Done { .. } | Line::End { .. }) => {
                self.phase = Phase::Typing {
                    typewriter: TypewriterState::new(text, self.char_delay),
                    then: AfterPassage::End,
                };
            }
            Some(Line::Text { .. }) | None => {
                self.phase = Phase::Ended { text };
            }
        }
        Ok(())
    }

    /// Handle a user input event. Returns `Some(locale_index)` if a locale
    /// switch was confirmed (caller must perform the actual swap).
    pub fn handle_input(
        &mut self,
        input: Input,
        story: &mut Story,
    ) -> Result<Option<usize>, Box<dyn std::error::Error>> {
        // Modal takes priority over all other input.
        if let Some(modal) = &mut self.locale_modal {
            match input {
                Input::Up => {
                    if modal.selected == 0 {
                        modal.selected = modal.labels.len() - 1;
                    } else {
                        modal.selected -= 1;
                    }
                }
                Input::Down => {
                    modal.selected = (modal.selected + 1) % modal.labels.len();
                }
                Input::Confirm => {
                    let chosen = modal.selected;
                    self.locale_modal = None;
                    if chosen != self.active_locale {
                        self.active_locale = chosen;
                        return Ok(Some(chosen));
                    }
                }
                Input::Escape | Input::LocaleSwitch | Input::Quit => {
                    self.locale_modal = None;
                }
                _ => {}
            }
            return Ok(None);
        }

        match input {
            Input::Quit => {
                self.should_quit = true;
            }
            Input::LocaleSwitch => {
                if self.locale_labels.len() > 1 {
                    self.locale_modal = Some(LocaleModal {
                        labels: self.locale_labels.clone(),
                        selected: self.active_locale,
                    });
                }
            }
            Input::Skip => match &mut self.phase {
                Phase::Typing { typewriter, .. } | Phase::Choosing { typewriter, .. } => {
                    typewriter.skip();
                }
                Phase::Ended { .. } => {}
            },
            Input::Tab => {
                if matches!(self.phase, Phase::Choosing { .. }) {
                    self.focus = match self.focus {
                        Focus::Story => Focus::Choices,
                        Focus::Choices => Focus::Story,
                    };
                }
            }
            Input::Up => match self.focus {
                Focus::Choices => {
                    if let Phase::Choosing {
                        selected, choices, ..
                    } = &mut self.phase
                    {
                        if *selected == 0 {
                            *selected = choices.len() - 1;
                        } else {
                            *selected -= 1;
                        }
                    }
                }
                Focus::Story => {
                    self.scroll_offset = self.scroll_offset.saturating_add(1);
                }
            },
            Input::Down => match self.focus {
                Focus::Choices => {
                    if let Phase::Choosing {
                        selected, choices, ..
                    } = &mut self.phase
                    {
                        *selected = (*selected + 1) % choices.len();
                    }
                }
                Focus::Story => {
                    self.scroll_offset = self.scroll_offset.saturating_sub(1);
                }
            },
            Input::Confirm => {
                self.confirm_choice(story)?;
            }
            Input::Escape | Input::None => {}
        }
        Ok(None)
    }

    /// Confirm the currently selected choice and advance the story.
    fn confirm_choice(&mut self, story: &mut Story) -> Result<(), Box<dyn std::error::Error>> {
        if !matches!(self.phase, Phase::Choosing { .. }) {
            return Ok(());
        }

        let old_phase = std::mem::replace(
            &mut self.phase,
            Phase::Ended {
                text: String::new(),
            },
        );
        if let Phase::Choosing {
            text,
            choices,
            selected,
            ..
        } = old_phase
        {
            let chosen_text = choices
                .get(selected)
                .map(|c| c.text.clone())
                .unwrap_or_default();
            let choice_index = choices.get(selected).map(|c| c.index).unwrap_or_default();
            let fragment_idx = story.choice_fragment_idx(choice_index);

            self.history.push(Passage {
                text,
                chosen: Some(ChosenEntry {
                    text: chosen_text,
                    fragment_idx,
                }),
                transcript_end: self.current_transcript_end,
            });

            story.choose(choice_index)?;
            self.advance_story(story)?;
        }
        Ok(())
    }

    /// Called each frame to advance typewriter and transition phases.
    pub fn tick(&mut self) {
        match &mut self.phase {
            Phase::Typing { typewriter, .. } => {
                typewriter.tick();
                if typewriter.is_complete() {
                    self.transition_after_passage();
                }
            }
            Phase::Choosing { typewriter, .. } => {
                typewriter.tick();
            }
            Phase::Ended { .. } => {}
        }
    }

    /// Transition from Typing to the next phase based on `AfterPassage`.
    fn transition_after_passage(&mut self) {
        let old = std::mem::replace(
            &mut self.phase,
            Phase::Ended {
                text: String::new(),
            },
        );
        if let Phase::Typing {
            typewriter, then, ..
        } = old
        {
            let text = typewriter.into_text();
            match then {
                AfterPassage::ShowChoices(entries) => {
                    let choice_text: String = entries
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let typewriter = TypewriterState::new(choice_text, self.char_delay);
                    self.phase = Phase::Choosing {
                        text,
                        choices: entries,
                        selected: 0,
                        typewriter,
                    };
                    self.focus = Focus::Choices;
                }
                AfterPassage::End => {
                    self.history.push(Passage {
                        text: text.clone(),
                        chosen: None,
                        transcript_end: self.current_transcript_end,
                    });
                    self.phase = Phase::Ended { text };
                }
            }
        }
    }

    /// How long the main loop should wait for the next event.
    pub fn poll_timeout(&self) -> Duration {
        match &self.phase {
            Phase::Typing { typewriter, .. } | Phase::Choosing { typewriter, .. } => {
                typewriter.poll_timeout()
            }
            Phase::Ended { .. } => Duration::from_millis(100),
        }
    }

    /// Number of choices currently known (pending or active).
    pub fn choice_count(&self) -> usize {
        match &self.phase {
            Phase::Typing {
                then: AfterPassage::ShowChoices(c),
                ..
            } => c.len(),
            Phase::Choosing { choices, .. } => choices.len(),
            Phase::Typing {
                then: AfterPassage::End,
                ..
            }
            | Phase::Ended { .. } => 0,
        }
    }

    /// Access the locale labels for status bar display.
    pub fn locale_labels(&self) -> &[String] {
        &self.locale_labels
    }

    /// Returns `(full_text, revealed_byte_count)` for the current passage.
    pub fn render_text(&self) -> (&str, usize) {
        match &self.phase {
            Phase::Typing { typewriter, .. } => {
                (typewriter.full_text(), typewriter.visible_text().len())
            }
            Phase::Choosing { text, .. } | Phase::Ended { text } => (text.as_str(), text.len()),
        }
    }

    /// Returns `(full_choice_text, revealed_byte_count)` for the choice
    /// typewriter, if currently in the Choosing phase.
    pub fn render_choices(&self) -> Option<(&str, usize)> {
        match &self.phase {
            Phase::Choosing { typewriter, .. } => {
                Some((typewriter.full_text(), typewriter.visible_text().len()))
            }
            _ => None,
        }
    }

    /// Re-render all history passages and the current passage against the
    /// story's current line tables. Called after a locale switch.
    pub fn rerender_history(&mut self, story: &Story) {
        let mut prev_end = 0;
        for passage in &mut self.history {
            let lines = story.resolve_transcript_slice(prev_end..passage.transcript_end);
            passage.text = Self::join_resolved_lines(&lines);
            // Re-resolve chosen entry if it has a fragment reference.
            if let Some(ChosenEntry {
                text: chosen_text,
                fragment_idx: Some(idx),
            }) = &mut passage.chosen
            {
                *chosen_text = story.resolve_fragment(*idx);
            }
            prev_end = passage.transcript_end;
        }

        // Also re-render the current passage (in phase).
        let current_text = {
            let lines = story.resolve_transcript_slice(prev_end..self.current_transcript_end);
            Self::join_resolved_lines(&lines)
        };
        // Re-resolve choice entries if we're in the choosing phase.
        let refreshed_choices = story.pending_choices();
        match &mut self.phase {
            Phase::Typing {
                typewriter, then, ..
            } => {
                *typewriter = TypewriterState::new(current_text, self.char_delay);
                typewriter.skip(); // show immediately after locale switch
                // Also refresh pending choice entries in AfterPassage.
                if let AfterPassage::ShowChoices(entries) = then {
                    for entry in entries.iter_mut() {
                        if let Some(fresh) =
                            refreshed_choices.iter().find(|c| c.index == entry.index)
                        {
                            entry.text = fresh.text.clone();
                        }
                    }
                }
            }
            Phase::Choosing {
                text,
                choices,
                typewriter,
                ..
            } => {
                *text = current_text;
                // Update choice text from re-resolved choices.
                for choice in choices.iter_mut() {
                    if let Some(fresh) = refreshed_choices.iter().find(|c| c.index == choice.index)
                    {
                        choice.text = fresh.text.clone();
                    }
                }
                // Rebuild choice typewriter with updated text.
                let mut choice_text = String::new();
                for c in choices.iter() {
                    choice_text.push_str(&c.text);
                    choice_text.push('\n');
                }
                *typewriter = TypewriterState::new(choice_text, self.char_delay);
                typewriter.skip();
            }
            Phase::Ended { text } => {
                *text = current_text;
            }
        }
    }

    fn join_resolved_lines(lines: &[(String, Vec<String>)]) -> String {
        let mut text = String::new();
        for (i, (line_text, _tags)) in lines.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            text.push_str(line_text);
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text
    }
}
