use std::time::Duration;

use brink_runtime::{Program, StepResult, Story};

use super::event::Input;
use super::typewriter::TypewriterState;

/// A passage of text that has been fully revealed (history entry).
pub struct Passage {
    pub text: String,
    pub chosen: Option<String>,
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

/// The current phase of the TUI state machine.
pub enum Phase {
    /// Text is being revealed character by character.
    Typing {
        typewriter: TypewriterState,
        pending_choices: Vec<ChoiceEntry>,
        ended: bool,
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

/// Top-level TUI application state.
pub struct App {
    pub phase: Phase,
    pub focus: Focus,
    pub history: Vec<Passage>,
    pub scroll_offset: u16,
    pub should_quit: bool,
    char_delay: Duration,
}

impl App {
    pub fn new(char_delay: Duration) -> Self {
        Self {
            phase: Phase::Ended {
                text: String::new(),
            },
            focus: Focus::Story,
            history: Vec::new(),
            scroll_offset: 0,
            should_quit: false,
            char_delay,
        }
    }

    /// Step the story forward and enter the appropriate phase.
    pub fn advance_story(
        &mut self,
        story: &mut Story,
        program: &Program,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.scroll_offset = 0;
        self.focus = Focus::Story;

        match story.step(program)? {
            StepResult::Done { text } => {
                self.phase = Phase::Typing {
                    typewriter: TypewriterState::new(text, self.char_delay),
                    pending_choices: Vec::new(),
                    ended: false,
                };
            }
            StepResult::Choices { text, choices } => {
                let entries: Vec<ChoiceEntry> = choices
                    .into_iter()
                    .map(|c| ChoiceEntry {
                        text: c.text,
                        index: c.index,
                    })
                    .collect();
                self.phase = Phase::Typing {
                    typewriter: TypewriterState::new(text, self.char_delay),
                    pending_choices: entries,
                    ended: false,
                };
            }
            StepResult::Ended { text } => {
                self.phase = Phase::Typing {
                    typewriter: TypewriterState::new(text, self.char_delay),
                    pending_choices: Vec::new(),
                    ended: true,
                };
            }
        }
        Ok(())
    }

    /// Handle a user input event.
    pub fn handle_input(
        &mut self,
        input: Input,
        story: &mut Story,
        program: &Program,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match input {
            Input::Quit => {
                self.should_quit = true;
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
                if let Phase::Choosing { .. } = &self.phase {
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
                        let choice_index =
                            choices.get(selected).map(|c| c.index).unwrap_or_default();

                        self.history.push(Passage {
                            text,
                            chosen: Some(chosen_text),
                        });

                        story.choose(choice_index)?;
                        self.advance_story(story, program)?;
                    }
                }
            }
            Input::None => {}
        }
        Ok(())
    }

    /// Called each frame to advance typewriter and transition phases.
    pub fn tick(
        &mut self,
        story: &mut Story,
        program: &Program,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let transition = match &mut self.phase {
            Phase::Typing {
                typewriter,
                pending_choices,
                ended,
            } => {
                typewriter.tick();
                if typewriter.is_complete() {
                    if *ended {
                        Some(PhaseTransition::End)
                    } else if pending_choices.is_empty() {
                        Some(PhaseTransition::AutoStep)
                    } else {
                        Some(PhaseTransition::ToChoosing)
                    }
                } else {
                    None
                }
            }
            Phase::Choosing { typewriter, .. } => {
                typewriter.tick();
                None
            }
            Phase::Ended { .. } => None,
        };

        if let Some(transition) = transition {
            match transition {
                PhaseTransition::ToChoosing => {
                    let old = std::mem::replace(
                        &mut self.phase,
                        Phase::Ended {
                            text: String::new(),
                        },
                    );
                    if let Phase::Typing {
                        typewriter: passage_tw,
                        pending_choices,
                        ..
                    } = old
                    {
                        let text = passage_tw.into_text();
                        let choice_text: String = pending_choices
                            .iter()
                            .map(|c| c.text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let typewriter = TypewriterState::new(choice_text, self.char_delay);
                        self.phase = Phase::Choosing {
                            text,
                            choices: pending_choices,
                            selected: 0,
                            typewriter,
                        };
                        self.focus = Focus::Choices;
                    }
                }
                PhaseTransition::End => {
                    let old = std::mem::replace(
                        &mut self.phase,
                        Phase::Ended {
                            text: String::new(),
                        },
                    );
                    if let Phase::Typing { typewriter, .. } = old {
                        let text = typewriter.into_text();
                        self.history.push(Passage {
                            text: text.clone(),
                            chosen: None,
                        });
                        self.phase = Phase::Ended { text };
                    }
                }
                PhaseTransition::AutoStep => {
                    let old = std::mem::replace(
                        &mut self.phase,
                        Phase::Ended {
                            text: String::new(),
                        },
                    );
                    if let Phase::Typing { typewriter, .. } = old {
                        let text = typewriter.into_text();
                        if !text.is_empty() {
                            self.history.push(Passage { text, chosen: None });
                        }
                        self.advance_story(story, program)?;
                    }
                }
            }
        }

        Ok(())
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
                pending_choices, ..
            } => pending_choices.len(),
            Phase::Choosing { choices, .. } => choices.len(),
            Phase::Ended { .. } => 0,
        }
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
}

enum PhaseTransition {
    ToChoosing,
    End,
    AutoStep,
}
