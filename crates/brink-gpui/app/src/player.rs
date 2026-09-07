//! The Player: a transcript of the running story and its live choices, in a
//! centre tab beside the documents.
//!
//! The runtime lives on the worker (`brink_gpui_model::play`); this view
//! sends commands and folds the plain-data steps that come back into a
//! transcript. A line that knows where it was written is a link back into
//! the editor. The prompt — the live choices — sits under the transcript
//! rather than in it, so it never scrolls out of reach.

use std::ops::Range;

use brink_gpui_model::play::{PlayChoice, PlayCommand, PlayError, PlayOutcome, PlayStep};
use brink_gpui_model::query::Location;
use brink_gpui_shell::tool_window::{TabSlot, select_tab};
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ListAlignment, ListState, Render, SharedString, Subscription, WeakEntity, Window, div, list,
    px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, PanelId, TabGroup};
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};

/// Clicking a line with a known source opens it.
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Navigate {
        path: String,
        span: Range<usize>,
    },
    /// Follow-in-editor: reveal this line's source WITHOUT taking focus.
    /// Distinct from `Navigate`, which is a click and means "take me
    /// there" — following must leave the keyboard on the Player, or the
    /// number keys stop picking choices halfway through a story.
    Follow {
        path: String,
        span: Range<usize>,
    },
    /// Something worth keeping outside the transcript — a compile failure
    /// or a runtime error. Restart clears the transcript; the Output log
    /// (`crate::output_log`) keeps the record.
    Log {
        level: crate::output_log::Level,
        text: SharedString,
    },
}

/// One row of the transcript.
#[derive(Debug, Clone)]
enum Entry {
    Line {
        text: SharedString,
        tags: Vec<SharedString>,
        source: Option<Location>,
    },
    /// The choice the player took, echoed the way it was written.
    Chosen {
        text: SharedString,
        sticky: bool,
    },
    /// A turn boundary or a runtime warning.
    Notice(SharedString),
    Error(SharedString),
}

pub struct Player {
    project: Entity<Project>,
    entries: Vec<Entry>,
    /// The live prompt; empty while the story runs or is over.
    choices: Vec<PlayChoice>,
    list: ListState,
    /// A command is in flight — the prompt is disabled until it answers.
    busy: bool,
    /// Whether a story has been started (and not stopped by an error).
    running: bool,
    /// Where the last start began — `None` is the entry. What Restart
    /// repeats.
    start_at: Option<String>,
    /// Sources changed since the running story was compiled.
    stale: bool,
    /// Follow-in-editor is held off because the author is editing. An
    /// edit means they are reading their own text, not the story's;
    /// Play and Restart resume it. Mirrors the web's `followPaused`.
    follow_paused: bool,
    /// Bumped on every start so a reply from before it is dropped.
    generation: u64,
    focus: FocusHandle,
    tab: TabSlot,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PlayerEvent> for Player {}
impl EventEmitter<PanelEvent> for Player {}

impl Player {
    pub fn new(project: Entity<Project>, cx: &mut Context<Self>) -> Self {
        let on_project = cx.subscribe(&project, |this, _, event: &ProjectEvent, cx| {
            if let ProjectEvent::SourceChanged { .. } = event {
                // Editing pauses following: the author is reading their
                // own text now, and having the editor jump under them
                // mid-sentence is the opposite of help.
                this.follow_paused = true;
                if this.running && !this.stale {
                    this.stale = true;
                    cx.notify();
                }
            }
        });
        // The transcript's size is a setting; a change to it has to reach
        // the panel, not wait for the next line.
        cx.observe_global::<brink_gpui_shell::settings::AppSettings>(|_, cx| cx.notify())
            .detach();
        Self {
            project,
            entries: Vec::new(),
            choices: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(600.)),
            busy: false,
            running: false,
            start_at: None,
            stale: false,
            follow_paused: false,
            generation: 0,
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_project],
        }
    }

    /// Whether the panel currently sits in a dock.
    #[must_use]
    pub fn is_docked(&self) -> bool {
        self.tab.group().is_some()
    }

    /// Make this the shown tab of its group.
    pub fn activate(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        if let Some(group) = this.read(cx).tab.group() {
            select_tab(&group, PanelId::from(this.entity_id()), window, cx);
        }
    }

    /// Compile and start — from the entry, or from a knot/stitch path.
    pub fn start(&mut self, at: Option<String>, cx: &mut Context<Self>) {
        self.generation += 1;
        self.entries.clear();
        self.choices.clear();
        self.list = ListState::new(0, ListAlignment::Top, px(600.));
        self.running = true;
        self.stale = false;
        // Play and Restart are the way back to following, as the web's are.
        self.follow_paused = false;
        if let Some(path) = &at {
            self.push(Entry::Notice(format!("— from {path} —").into()));
        }
        self.start_at = at.clone();
        self.send(PlayCommand::Start { at }, cx);
    }

    /// Start again from where the last start began.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        let at = self.start_at.clone();
        self.start(at, cx);
    }

    /// The numbered choice buttons are numbered for a reason: `1`-`9` take
    /// that choice. Playtesting is a loop of read-then-pick, and reaching
    /// for the mouse on every pick is the friction the numbers already
    /// promised to remove.
    ///
    /// Bound on the panel's own focus rather than as a command: a digit is
    /// only a choice while the Player has focus and is showing choices,
    /// and a global `1` would fight every text field in the studio.
    fn on_key(&mut self, event: &gpui::KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.choices.is_empty() {
            return;
        }
        let key = event.keystroke.key.as_str();
        // Only a bare digit: `cmd-1` is the shell's tool-window toggle.
        if event.keystroke.modifiers.modified() {
            return;
        }
        let Some(n) = key.parse::<usize>().ok().filter(|n| *n >= 1) else {
            return;
        };
        let Some(choice) = self.choices.get(n - 1) else {
            return;
        };
        let index = choice.index;
        self.choose(index, cx);
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(choice) = self.choices.iter().find(|c| c.index == index).cloned() else {
            return;
        };
        self.choices.clear();
        self.push(Entry::Chosen {
            text: choice.text.into(),
            sticky: choice.sticky,
        });
        self.send(PlayCommand::Choose(index), cx);
    }

    fn send(&mut self, command: PlayCommand, cx: &mut Context<Self>) {
        self.busy = true;
        let generation = self.generation;
        let task = self.project.read(cx).play(command, cx);
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.busy = false;
                    match outcome {
                        Ok(outcome) => this.apply(outcome, cx),
                        Err(e) => {
                            this.running = false;
                            let text = SharedString::from(format!("{e:#}"));
                            this.push(Entry::Error(text.clone()));
                            cx.emit(PlayerEvent::Log {
                                level: crate::output_log::Level::Error,
                                text,
                            });
                        }
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn apply(&mut self, outcome: PlayOutcome, cx: &mut Context<Self>) {
        let follow = last_source(&outcome.steps);
        for step in outcome.steps {
            match step {
                PlayStep::Line { text, tags, source } => {
                    let text = text.trim_end_matches('\n').to_owned();
                    self.push(Entry::Line {
                        text: text.into(),
                        tags: tags.into_iter().map(SharedString::from).collect(),
                        source,
                    });
                }
                PlayStep::Choices(choices) => self.choices = choices,
                PlayStep::Done => {
                    self.running = false;
                    self.push(Entry::Notice("— done —".into()));
                }
                PlayStep::End => {
                    self.running = false;
                    self.push(Entry::Notice("— end —".into()));
                }
                PlayStep::Suspended => {
                    self.running = false;
                    self.push(Entry::Notice("— suspended at an await —".into()));
                }
            }
        }
        if let Some(loc) = follow
            && !self.follow_paused
            && brink_gpui_shell::settings::AppSettings::get(cx).follow_in_editor
        {
            cx.emit(PlayerEvent::Follow {
                path: loc.path.clone(),
                span: loc.start as usize..loc.end as usize,
            });
        }
        for warning in outcome.warnings {
            let text = SharedString::from(format!("warning: {warning}"));
            self.push(Entry::Notice(text.clone()));
            cx.emit(PlayerEvent::Log {
                level: crate::output_log::Level::Warning,
                text,
            });
        }
        if let Some(error) = outcome.error {
            self.running = false;
            self.choices.clear();
            let text = SharedString::from(error.to_string());
            self.push(Entry::Error(text.clone()));
            cx.emit(PlayerEvent::Log {
                level: crate::output_log::Level::Error,
                text,
            });
            if let PlayError::Compile(errors) = error {
                for line in errors {
                    let text = SharedString::from(line);
                    self.push(Entry::Error(text.clone()));
                    cx.emit(PlayerEvent::Log {
                        level: crate::output_log::Level::Error,
                        text,
                    });
                }
                self.push(Entry::Notice("Fix them in Problems, then Restart.".into()));
            }
        }
    }

    fn push(&mut self, entry: Entry) {
        let ix = self.entries.len();
        self.entries.push(entry);
        self.list.splice(ix..ix, 1);
        self.list.scroll_to_reveal_item(ix);
    }

    fn render_entry(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (muted, danger, primary) = (theme.muted_foreground, theme.danger, theme.primary);
        let Some(entry) = self.entries.get(ix) else {
            return div().into_any_element();
        };
        match entry {
            Entry::Line { text, tags, source } => {
                let row = h_flex()
                    .id(("play-line", ix))
                    .w_full()
                    .items_baseline()
                    .gap_2()
                    .px_4()
                    .py_1()
                    // `min_w_0`: a flex item's minimum is its content by
                    // default, so a long line refused to shrink and pushed
                    // its own tags off the right edge instead of wrapping.
                    .child(div().flex_1().min_w_0().child(text.clone()))
                    .children(
                        tags.iter()
                            .map(|tag| div().text_xs().text_color(muted).child(format!("# {tag}"))),
                    );
                match source.clone() {
                    Some(loc) => row
                        .cursor_pointer()
                        .hover(|el| el.bg(theme.muted.opacity(0.4)))
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PlayerEvent::Navigate {
                                path: loc.path.clone(),
                                span: loc.start as usize..loc.end as usize,
                            });
                        }))
                        .into_any_element(),
                    None => row.into_any_element(),
                }
            }
            Entry::Chosen { text, sticky } => div()
                .px_4()
                .py_1()
                .text_color(primary)
                .child(format!("{} {text}", if *sticky { "+" } else { "*" }))
                .into_any_element(),
            Entry::Notice(text) => div()
                .px_4()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(text.clone())
                .into_any_element(),
            Entry::Error(text) => div()
                .px_4()
                .py_1()
                .text_xs()
                .text_color(danger)
                .child(text.clone())
                .into_any_element(),
        }
    }

    fn render_prompt(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.choices.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let busy = self.busy;
        Some(
            v_flex()
                .w_full()
                .gap_1()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(theme.border)
                .children(self.choices.iter().enumerate().map(|(n, choice)| {
                    let index = choice.index;
                    Button::new(("play-choice", index))
                        .outline()
                        .small()
                        .disabled(busy)
                        .label(format!("{}. {}", n + 1, choice.text.trim()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.choose(index, cx);
                        }))
                }))
                .into_any_element(),
        )
    }
}

/// Where follow-in-editor should land for a batch of steps: the LAST
/// line in it that has a source.
///
/// A run delivers many lines at once. Revealing each in turn would scroll
/// the editor through all of them to arrive at exactly the same place, and
/// the place is what the author wants to see — where the story is now.
fn last_source(steps: &[PlayStep]) -> Option<Location> {
    steps.iter().rev().find_map(|step| match step {
        PlayStep::Line { source, .. } => source.clone(),
        _ => None,
    })
}

impl Focusable for Player {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BasePanel for Player {
    fn panel_name(&self) -> &'static str {
        "Player"
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tab.added_to(group);
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.tab.removed();
    }
}

impl Panel for Player {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Player")
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for Player {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, warn, border) = (theme.muted_foreground, theme.warning, theme.border);
        let started = !self.entries.is_empty();
        let status: Option<(SharedString, gpui::Hsla)> = if self.stale {
            Some(("sources changed — Restart to pick them up".into(), warn))
        } else if self.busy {
            Some(("running…".into(), muted))
        } else {
            self.start_at
                .as_ref()
                .map(|at| (format!("from {at}").into(), muted))
        };
        let prompt = self.render_prompt(cx);
        let prose_size = brink_gpui_shell::settings::AppSettings::get(cx).player_size();

        v_flex()
            .id("player")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .text_sm()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        Button::new("play-restart")
                            .ghost()
                            .xsmall()
                            .label("Restart")
                            .disabled(!started)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.restart(cx);
                            })),
                    )
                    .child(
                        Button::new("play-from-start")
                            .ghost()
                            .xsmall()
                            .label("From start")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.start(None, cx);
                            })),
                    )
                    .when_some(status, |el, (text, color)| {
                        el.child(div().text_xs().text_color(color).child(text))
                    }),
            )
            .when(!started, |el| {
                el.child(div().p_4().text_xs().text_color(muted).child(
                    "Nothing is running. Play runs the story from its entry; \
                     \"Play from here\" on a knot in the Binder starts there.",
                ))
            })
            .when(started, |el| {
                el.child(
                    // The transcript's own size — the reading surface, not
                    // the chrome around it. Set on the list rather than the
                    // panel so the header, the status and the choice
                    // buttons stay at the studio's scale.
                    list(
                        self.list.clone(),
                        cx.processor(|this, ix, _window, cx| this.render_entry(ix, cx)),
                    )
                    .flex_1()
                    .py_2()
                    .text_size(px(prose_size)),
                )
            })
            .children(prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::{Location, last_source};
    use brink_gpui_model::play::PlayStep;

    fn line(text: &str, source: Option<(&str, u32)>) -> PlayStep {
        PlayStep::Line {
            text: text.to_owned(),
            tags: Vec::new(),
            source: source.map(|(path, start)| Location {
                path: path.to_owned(),
                start,
                end: start + 4,
            }),
        }
    }

    #[test]
    fn following_lands_on_the_last_line_of_the_run() {
        let steps = vec![
            line("first", Some(("story.ink", 10))),
            line("second", Some(("story.ink", 40))),
            PlayStep::Done,
        ];
        assert_eq!(
            last_source(&steps).map(|l| l.start),
            Some(40),
            "where the story is now, not where the run started"
        );
    }

    #[test]
    fn a_line_with_no_source_does_not_cancel_the_one_before_it() {
        // A synthetic line (a glue join, a tag-only line) has no source.
        // Following stays on the last line that HAS one rather than
        // giving up on the batch.
        let steps = vec![
            line("real", Some(("story.ink", 10))),
            line("synthetic", None),
        ];
        assert_eq!(last_source(&steps).map(|l| l.start), Some(10));
    }

    #[test]
    fn a_run_of_nothing_followable_asks_for_no_reveal() {
        assert!(last_source(&[]).is_none());
        assert!(last_source(&[PlayStep::Done]).is_none());
        assert!(last_source(&[line("x", None)]).is_none());
    }
}
