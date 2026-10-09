//! The Player: a transcript of the running story and its live choices, in a
//! centre tab beside the documents.
//!
//! The runtime lives on the worker (`brink_gpui_model::play`); this view
//! sends commands and folds the plain-data steps that come back into a
//! transcript. A line that knows where it was written is a link back into
//! the editor. The prompt — the live choices — sits under the transcript
//! rather than in it, so it never scrolls out of reach.

use std::ops::Range;

use brink_gpui_model::play::{
    Fault, PlayChoice, PlayCommand, PlayError, PlayOutcome, PlayStep, StopKind,
};
use brink_gpui_model::query::Location;
use brink_gpui_shell::icons::BrinkIcon;
use brink_gpui_shell::tool_window::{TabSlot, select_tab};
use gpui::AnimationExt as _;
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla,
    IntoElement, ListAlignment, ListState, Render, SharedString, Subscription, WeakEntity, Window,
    div, list, px,
};
use gpui_component::dock::{BasePanel, Panel, PanelEvent, PanelId, TabGroup};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, h_flex, v_flex};

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
    /// A debug verb came to rest on a source line — reveal it, so the
    /// author is looking at where the story is.
    Stopped {
        path: String,
        line: u32,
    },
    /// The header's ⋯: open Settings at the Player section — where the
    /// Autoplay pace and the transcript's look live.
    OpenSettings,
    /// Write mode's close: the Player is a pane there, and its header is
    /// the pane's.
    Close,
    /// The pointer is on a row or choice: show its source on the NOW line,
    /// scrolling only (decision log 2026-10-09).
    Peek {
        path: String,
        span: Range<usize>,
    },
    /// The pointer left the Player: back to NOW — the line the story is
    /// on, or the choices it is offering.
    PeekEnd {
        now: Option<(String, Range<usize>)>,
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
    /// The reader's pick, and where the choice was written (a choice echo
    /// links back the way a line does — ruled 2026-09-02).
    Chosen {
        text: SharedString,
        sticky: bool,
        source: Option<Location>,
        /// The rest of the offer, passed by — the manuscript dims them.
        /// Kept on the echo, so a rewind past it takes them back too.
        passed: Vec<Location>,
    },
    /// A turn boundary or a runtime warning.
    Notice(SharedString),
    /// A failure. `at` is the site a RUNTIME fault resolved, which makes
    /// the row a link — the studio jumps there once when the fault lands,
    /// and this is how you get back to it afterwards. A compile failure
    /// carries none: its positions are Problems' business.
    Error {
        text: SharedString,
        at: Option<(String, u32)>,
    },
}

pub struct Player {
    project: Entity<Project>,
    entries: Vec<Entry>,
    /// The source under the pointer — a row or a choice card — for the
    /// manuscript's bracket (decision log 2026-10-09), and whether it is a
    /// choice.
    hovered: Option<(Location, bool)>,
    /// The save slot a Load attached this session to: Save state writes
    /// back there (W14). `None` — a fresh run or a fork — saves anew.
    attached: Option<(crate::saves::Store, String)>,
    /// Both stores' slots as last read, for the idle Player's list.
    saves: Vec<(crate::saves::Store, crate::saves::SlotMeta)>,
    /// Lines rewound past: the manuscript keeps a dashed rail on them
    /// until the story moves on (#3665).
    undone: Vec<Location>,
    /// How many steps back the worker can rewind.
    history: usize,
    /// When the current choices arrived, so their cards slide in once.
    choices_at: Option<std::time::Instant>,
    /// When each entry arrived, parallel to `entries`: a row animates in
    /// only while it is new, so one rebuilt later (scrolled back, the tab
    /// re-shown) never replays its arrival.
    arrivals: Vec<std::time::Instant>,
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
    /// The last outcome left the flow stopped by the debugger.
    paused: bool,
    /// Follow-in-editor is held off because the author is editing. An
    /// edit means they are reading their own text, not the story's;
    /// Play and Restart resume it. Mirrors the web's `followPaused`.
    follow_paused: bool,
    /// Bumped on every start so a reply from before it is dropped.
    generation: u64,
    /// The header's Show tags toggle.
    show_tags: bool,
    /// Where the last debug stop held the story, `(file, line)`.
    held_at: Option<(String, u32)>,
    /// The NOW line, in window coordinates (Write mode): a story line's
    /// top sits here, level with its source in the manuscript.
    now_y: f32,
    /// The spacer under the transcript, so its last row can reach NOW.
    tail: f32,
    /// The spacer above the transcript, so its first rows can reach NOW.
    head: f32,
    /// A story line to bring to NOW, and how many more frames may try: its
    /// real top is measured after each layout and the list scrolled by the
    /// difference, until it sits there.
    to_now: Option<NowMove>,
    /// `>>`: playing on by itself at the Settings pace, until a stop.
    autoplay: bool,
    /// Where autoplay will stop, found by running a copy of the story
    /// ahead when it starts (`PlayCommand::Lookahead`).
    autoplay_stop: Option<brink_gpui_model::play::PlayStop>,
    /// The wait before autoplay's next line; dropping it cancels it.
    autoplay_timer: Option<gpui::Task<()>>,
    /// Where the next line starts, as the last line's stop says: what ▶
    /// plays next, for the manuscript.
    next_at: Option<(String, u32)>,
    /// Why the story is held, for the header: a breakpoint, a write, a
    /// step.
    held_why: Option<String>,
    /// Drawn for Write mode: the transport has no Step there (decision
    /// log 2026-10-08, "line-level breakpoints").
    write_mode: bool,
    /// The project's dialogue dialect, compiled for reading the
    /// transcript as Stage runs, and the reading of the rows as of the
    /// last render.
    reader: Option<crate::player_stage::Reader>,
    looks: Vec<crate::player_stage::Look>,
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
        let saves = all_saves(project.read(cx).root());
        Self {
            project,
            entries: Vec::new(),
            arrivals: Vec::new(),
            choices_at: None,
            hovered: None,
            undone: Vec::new(),
            history: 0,
            attached: None,
            saves,
            choices: Vec::new(),
            list: ListState::new(2, ListAlignment::Top, px(600.)),
            busy: false,
            running: false,
            start_at: None,
            stale: false,
            paused: false,
            follow_paused: false,
            generation: 0,
            show_tags: false,
            held_at: None,
            now_y: 0.,
            tail: 0.,
            head: 0.,
            to_now: None,
            autoplay: false,
            autoplay_timer: None,
            autoplay_stop: None,
            next_at: None,
            held_why: None,
            write_mode: false,
            reader: None,
            looks: Vec::new(),
            focus: cx.focus_handle(),
            tab: TabSlot::default(),
            _subscriptions: vec![on_project],
        }
    }

    /// Whether the session is paused by the debugger — at a breakpoint,
    /// after a step, on a watchpoint — rather than mid-turn or waiting on
    /// the reader. What lets a hover show a frame's locals.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.running && self.paused
    }

    /// Whether a running story is older than the sources — the Player's
    /// own "sources changed" state, which the Program Explorer reports as
    /// a degraded session.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.running && self.stale
    }

    /// What the status bar says about the session.
    #[must_use]
    pub fn state(&self) -> SessionState {
        if self.busy {
            SessionState::Working
        } else if !self.choices.is_empty() {
            SessionState::AwaitingChoice
        } else if self.running {
            SessionState::Running
        } else if self.entries.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Over
        }
    }

    /// A cheap fingerprint of where the session is: it changes whenever
    /// the story could have moved — a start, a line, a choice, a command
    /// going out or coming back. An observer that queries the worker keys
    /// on this rather than on every notify, since the Player is also
    /// notified for reasons that move nothing (a redraw, a setting).
    #[must_use]
    pub fn session_key(&self) -> (u64, usize, usize, bool, bool) {
        (
            self.generation,
            self.entries.len(),
            self.choices.len(),
            self.busy,
            self.running,
        )
    }

    /// Where the last session was started, for the tests.
    #[cfg(test)]
    pub fn started_at(&self) -> Option<&str> {
        self.start_at.as_deref()
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
        self.begin();
        // A fresh run is attached to no save: the next save makes a new one.
        self.attached = None;
        if let Some(path) = &at {
            self.push(Entry::Notice(format!("— from {path} —").into()));
        }
        self.start_at = at.clone();
        self.send(PlayCommand::Start { at }, cx);
    }

    /// A clean session: what Start and a Load both begin from.
    fn begin(&mut self) {
        self.generation += 1;
        self.entries.clear();
        self.arrivals.clear();
        self.undone.clear();
        self.history = 0;
        self.choices.clear();
        // One item before the transcript and one past it — the head and the
        // tail, the room that lets its first and last rows reach NOW.
        self.list = ListState::new(2, ListAlignment::Top, px(600.));
        self.running = true;
        self.stale = false;
        self.halt_autoplay();
        self.next_at = None;
        // Play and Restart are the way back to following, as the web's are.
        self.follow_paused = false;
    }

    /// Read both stores' slots afresh (the idle Player's list).
    pub(crate) fn refresh_saves(&mut self, cx: &mut Context<Self>) {
        self.saves = all_saves(self.project.read(cx).root());
        cx.notify();
    }

    /// The default store for a new save: the app setting.
    fn new_save_store(cx: &App) -> crate::saves::Store {
        if brink_gpui_shell::settings::AppSettings::get(cx).saves_on_this_computer {
            crate::saves::Store::Local
        } else {
            crate::saves::Store::Project
        }
    }

    /// Save state (W14): checkpoint the running story — back into the slot
    /// a Load attached this session to, else a new slot in the default
    /// store.
    pub(crate) fn save_state(&mut self, cx: &mut Context<Self>) {
        if !self.running || self.busy {
            return;
        }
        let root = self.project.read(cx).root().to_path_buf();
        let target = self
            .attached
            .clone()
            .map_or((Self::new_save_store(cx), None), |(store, id)| {
                (store, Some(id))
            });
        let generation = self.generation;
        let task = self.project.read(cx).play(PlayCommand::Save, cx);
        cx.spawn(async move |this, cx| {
            let saved = task.await.ok().and_then(|o| o.saved);
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                let (store, id) = target;
                let written = saved
                    .ok_or_else(|| "nothing to save".to_owned())
                    .and_then(|saved| {
                        let dir = crate::saves::dir(store, &root)
                            .ok_or_else(|| "no place to keep saves".to_owned())?;
                        crate::saves::write(&dir, id.as_deref(), &saved).map_err(|e| e.to_string())
                    });
                match written {
                    Ok(meta) => {
                        this.push(Entry::Notice(
                            format!("— saved: {} ({}) —", meta.name, store.label()).into(),
                        ));
                        this.attached = Some((store, meta.id));
                    }
                    Err(e) => this.push(Entry::Error {
                        text: format!("could not save: {e}").into(),
                        at: None,
                    }),
                }
                this.refresh_saves(cx);
            });
        })
        .detach();
    }

    /// Load a save — attached, so Save state writes back to it — or fork
    /// one: start from a copy, unattached, the checkpoint untouched.
    pub(crate) fn load_save(
        &mut self,
        store: crate::saves::Store,
        id: &str,
        fork: bool,
        cx: &mut Context<Self>,
    ) {
        let root = self.project.read(cx).root().to_path_buf();
        let Some(loaded) = crate::saves::dir(store, &root).and_then(|d| crate::saves::read(&d, id))
        else {
            self.push(Entry::Notice("— that save no longer exists —".into()));
            self.refresh_saves(cx);
            return;
        };
        self.begin();
        self.attached = (!fork).then(|| (store, id.to_owned()));
        self.push(Entry::Notice(
            format!(
                "— {} {} —",
                if fork { "forked from" } else { "loaded" },
                loaded.meta.name
            )
            .into(),
        ));
        self.start_at = loaded.meta.knot_path.clone();
        self.send(
            PlayCommand::Load {
                state: loaded.state,
                transcript: loaded.transcript,
                knot: loaded.meta.knot_path,
            },
            cx,
        );
    }

    /// Delete a save.
    pub(crate) fn delete_save(
        &mut self,
        store: crate::saves::Store,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let root = self.project.read(cx).root().to_path_buf();
        if let Some(dir) = crate::saves::dir(store, &root) {
            let _ = crate::saves::remove(&dir, id);
        }
        if self
            .attached
            .as_ref()
            .is_some_and(|(s, i)| *s == store && i == id)
        {
            self.attached = None;
        }
        self.refresh_saves(cx);
    }

    /// Draw for Write mode (no Step) or Script.
    pub fn set_write_mode(&mut self, write: bool, cx: &mut Context<Self>) {
        if self.write_mode != write {
            self.write_mode = write;
            cx.notify();
        }
    }

    /// End the story: the transport's ■.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.running {
            return;
        }
        self.generation += 1;
        self.running = false;
        self.paused = false;
        self.held_at = None;
        self.next_at = None;
        self.halt_autoplay();
        self.choices.clear();
        self.busy = false;
        self.push(Entry::Notice("— stopped —".into()));
        let task = self.project.read(cx).play(PlayCommand::Stop, cx);
        task.detach();
        cx.notify();
    }

    /// The big round button (decision log 2026-10-09): start when nothing
    /// runs; pause autoplay while it plays; otherwise play the next line —
    /// a held one included.
    pub(crate) fn primary(&mut self, cx: &mut Context<Self>) {
        if !self.running {
            let at = self.start_at.clone();
            self.start(at, cx);
        } else if self.autoplay {
            self.halt_autoplay();
            cx.notify();
        } else if self.can_advance() {
            self.send(PlayCommand::Next, cx);
        }
    }

    /// How many story lines the transcript holds.
    #[cfg(test)]
    pub(crate) fn line_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e, Entry::Line { .. }))
            .count()
    }

    /// Whether a command is in flight.
    #[cfg(test)]
    pub(crate) fn is_busy(&self) -> bool {
        self.busy
    }

    /// Whether any story line so far contains `needle`.
    #[cfg(test)]
    pub(crate) fn has_line(&self, needle: &str) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e, Entry::Line { text, .. } if text.contains(needle)))
    }

    /// Where entry `ix`'s row was laid out, in window coordinates.
    #[cfg(test)]
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.list.bounds_for_item(ix + 1)
    }

    /// Where the current row was laid out, in window coordinates.
    #[cfg(test)]
    pub(crate) fn active_row_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.list.bounds_for_item(self.active_row()? + 1)
    }

    /// The current row's top and the NOW line, in window coordinates.
    #[cfg(test)]
    pub(crate) fn now_gap(&self) -> Option<(f32, f32)> {
        let row = self.list.bounds_for_item(self.active_row()? + 1)?;
        Some((f32::from(row.top()), self.now_y))
    }

    /// The save slots as last read, by id.
    #[cfg(test)]
    pub(crate) fn save_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.saves.iter().map(|(_, m)| m.id.clone()).collect();
        ids.sort();
        ids
    }

    /// What the header's status says.
    #[cfg(test)]
    pub(crate) fn status_label(&self, cx: &App) -> String {
        self.status(cx).0.to_string()
    }

    /// Whether `>>` is playing on.
    #[cfg(test)]
    pub(crate) fn is_autoplaying(&self) -> bool {
        self.autoplay
    }

    /// The line a breakpoint holds the story before, if one does.
    #[cfg(test)]
    pub(crate) fn held(&self) -> Option<&(String, u32)> {
        self.held_at.as_ref().filter(|_| self.paused)
    }

    /// Whether a line can be played now: running, nothing in flight, and
    /// no choice waiting to be made.
    fn can_advance(&self) -> bool {
        self.running && !self.busy && self.choices.is_empty()
    }

    /// `>>`: play on by itself at the Settings pace, until a breakpoint, a
    /// choice or the end; a second press (or ▶) pauses it.
    pub(crate) fn toggle_autoplay(&mut self, cx: &mut Context<Self>) {
        if self.autoplay {
            self.halt_autoplay();
        } else if self.can_advance() {
            self.autoplay = true;
            self.look_ahead(cx);
            self.send(PlayCommand::Next, cx);
        }
        cx.notify();
    }

    /// Ask where autoplay will stop, for the status line. Sent before the
    /// first line it plays, so the worker answers from where it starts.
    fn look_ahead(&mut self, cx: &mut Context<Self>) {
        self.autoplay_stop = None;
        let generation = self.generation;
        let task = self.project.read(cx).play(PlayCommand::Lookahead, cx);
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation && this.autoplay {
                    this.autoplay_stop = outcome.stop;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// `|<`: back to just before the last choice — choose again.
    pub(crate) fn back_to_choice(&mut self, cx: &mut Context<Self>) {
        if self.history > 0 && !self.busy {
            self.halt_autoplay();
            self.send(PlayCommand::BackToChoice, cx);
        }
    }

    /// `<<`: one line back per press (2026-10-09, revised: not a timed
    /// rewind).
    pub(crate) fn back_one_line(&mut self, cx: &mut Context<Self>) {
        if self.history > 0 && !self.busy {
            self.halt_autoplay();
            self.send(PlayCommand::Back, cx);
        }
    }

    /// Keep the transcript up to the `lines`-th story line, and set what
    /// came after aside as undone.
    fn rewind_to(&mut self, lines: usize) {
        let cut = if lines == 0 {
            self.entries
                .iter()
                .position(|e| matches!(e, Entry::Line { .. }))
                .unwrap_or(self.entries.len())
        } else {
            self.entries
                .iter()
                .enumerate()
                .filter(|(_, e)| matches!(e, Entry::Line { .. }))
                .nth(lines - 1)
                .map_or(self.entries.len(), |(ix, _)| ix + 1)
        };
        let old = self.entries.len();
        if cut >= old {
            return;
        }
        let removed: Vec<Entry> = self.entries.drain(cut..).collect();
        self.arrivals.truncate(cut);
        self.list.splice(cut + 1..old + 1, 0);
        self.undone
            .extend(removed.into_iter().filter_map(|e| match e {
                Entry::Line {
                    source: Some(loc), ..
                } => Some(loc),
                _ => None,
            }));
    }

    fn halt_autoplay(&mut self) {
        self.autoplay = false;
        self.autoplay_timer = None;
        self.autoplay_stop = None;
    }

    /// Autoplay's next line, after the pace's wait.
    fn schedule_autoplay(&mut self, cx: &mut Context<Self>) {
        let wait = std::time::Duration::from_secs_f32(
            brink_gpui_shell::settings::AppSettings::get(cx).autoplay_ms / 1000.,
        );
        let generation = self.generation;
        self.autoplay_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |this, cx| {
                if this.autoplay && this.generation == generation && this.can_advance() {
                    this.send(PlayCommand::Next, cx);
                }
            });
        }));
    }

    /// `>|`: straight to the next stop — a breakpoint, a choice, the end.
    pub(crate) fn skip(&mut self, cx: &mut Context<Self>) {
        if self.can_advance() {
            self.halt_autoplay();
            self.send(PlayCommand::Continue, cx);
        }
    }

    /// The manuscript's NOW pill: follow again, and go back to the line the
    /// story is on.
    pub fn back_to_now(&mut self, cx: &mut Context<Self>) {
        self.follow_paused = false;
        if let Some(loc) = self.current_source().cloned() {
            cx.emit(PlayerEvent::Follow {
                path: loc.path,
                span: loc.start as usize..loc.end as usize,
            });
        }
        cx.notify();
    }

    /// Follow: on → off; paused → on (resumed); off → on.
    fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        let on = brink_gpui_shell::settings::AppSettings::get(cx).follow_in_editor;
        if on && self.follow_paused {
            self.follow_paused = false;
        } else {
            brink_gpui_shell::settings::update(cx, |s| s.follow_in_editor = !on);
            self.follow_paused = false;
        }
        cx.notify();
    }

    /// Where the story is now: the newest line with a source.
    fn current_source(&self) -> Option<&Location> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Line { source, .. } => source.as_ref(),
            _ => None,
        })
    }

    /// The choices on offer, as one span of their source: from the first
    /// choice's line to the last's (in the first's file).
    fn choices_span(&self) -> Option<(String, Range<usize>)> {
        let first = self.choices.iter().find_map(|c| c.source.as_ref())?;
        let end = self
            .choices
            .iter()
            .filter_map(|c| c.source.as_ref())
            .filter(|l| l.path == first.path)
            .map(|l| l.end)
            .max()
            .unwrap_or(first.end);
        Some((first.path.clone(), first.start as usize..end as usize))
    }

    /// Where NOW is: the choices on offer, or the line the story is on.
    pub(crate) fn now_target(&self) -> Option<(String, Range<usize>)> {
        self.choices_span().or_else(|| {
            self.current_source()
                .map(|loc| (loc.path.clone(), loc.start as usize..loc.end as usize))
        })
    }

    /// Where the session has been, for the manuscript's gutter: each
    /// played line's source, the active line's, and a held line.
    #[must_use]
    pub fn trail(&self) -> PlayTrail {
        let active = self.active_row();
        PlayTrail {
            played: self
                .entries
                .iter()
                .enumerate()
                .filter(|(ix, _)| Some(*ix) != active)
                .filter_map(|(_, e)| match e {
                    // A taken choice is a played line too: it gets the rail.
                    Entry::Line {
                        source: Some(loc), ..
                    }
                    | Entry::Chosen {
                        source: Some(loc), ..
                    } => Some(loc.clone()),
                    _ => None,
                })
                .collect(),
            active: active.and_then(|ix| match self.entries.get(ix) {
                Some(Entry::Line { source, .. }) => source.clone(),
                _ => None,
            }),
            held: self.held_at.clone().filter(|_| self.paused),
            next: self.next_at.clone().filter(|_| self.running),
            hover: self.hovered.clone(),
            now: self.now_target().map(|(path, span)| Location {
                path,
                start: u32::try_from(span.start).unwrap_or(u32::MAX),
                end: u32::try_from(span.end).unwrap_or(u32::MAX),
            }),
            chosen: self.chosen_sources(),
            not_taken: {
                let chosen = self.chosen_sources();
                self.entries
                    .iter()
                    .filter_map(|e| match e {
                        Entry::Chosen { passed, .. } => Some(passed),
                        _ => None,
                    })
                    .flatten()
                    .filter(|loc| !chosen.contains(loc))
                    .cloned()
                    .collect()
            },
            undone: self.undone.clone(),
        }
    }

    /// Where each choice the reader took was written.
    fn chosen_sources(&self) -> Vec<Location> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Chosen {
                    source: Some(loc), ..
                } => Some(loc.clone()),
                _ => None,
            })
            .collect()
    }

    /// The index of the newest story line — the active row.
    fn active_row(&self) -> Option<usize> {
        if !self.running {
            return None;
        }
        self.entries
            .iter()
            .rposition(|e| matches!(e, Entry::Line { .. }))
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
        if event.keystroke.key == "space" && !event.keystroke.modifiers.modified() {
            if self.choices.is_empty() {
                self.primary(cx);
            }
            return;
        }
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

    pub(crate) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(choice) = self.choices.iter().find(|c| c.index == index).cloned() else {
            return;
        };
        let passed = self
            .choices
            .iter()
            .filter(|c| c.index != index)
            .filter_map(|c| c.source.clone())
            .collect();
        self.choices.clear();
        self.push(Entry::Chosen {
            text: choice.text.into(),
            sticky: choice.sticky,
            source: choice.source.clone(),
            passed,
        });
        self.send(PlayCommand::Choose(index), cx);
    }

    /// Run a debug verb against the live session. Nothing running is not
    /// an error the panel invents: the worker answers `NotStarted` and
    /// the transcript says so, exactly as `Choose` does.
    pub fn debug(&mut self, command: PlayCommand, cx: &mut Context<Self>) {
        self.send(command, cx);
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
                        Ok(outcome) => {
                            // Autoplay goes on only past an ordinary line.
                            let line = outcome.error.is_none()
                                && outcome
                                    .stop
                                    .as_ref()
                                    .is_some_and(|stop| stop.kind == StopKind::Line);
                            this.apply(outcome, cx);
                            if this.autoplay {
                                if line && this.can_advance() {
                                    this.schedule_autoplay(cx);
                                } else {
                                    this.halt_autoplay();
                                }
                            }
                        }
                        Err(e) => {
                            this.running = false;
                            this.halt_autoplay();
                            let text = SharedString::from(format!("{e:#}"));
                            this.push(Entry::Error {
                                text: text.clone(),
                                at: None,
                            });
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
        // Every outcome says afresh whether the flow is held by the
        // debugger — a breakpoint, a watch, a step — rather than resting
        // after a line or at a place the story itself yields.
        if let Some(back) = outcome.rewound {
            self.rewind_to(back.lines);
            self.choices.clear();
            self.running = true;
        }
        if outcome.rewound.is_some() || outcome.history > 0 || outcome.steps.is_empty() {
            self.history = outcome.history;
        }
        let kind = outcome.stop.as_ref().map(|stop| stop.kind);
        self.paused = kind.is_some_and(StopKind::holds);
        let at = outcome.stop.as_ref().and_then(|stop| stop.at.clone());
        self.held_at = if self.paused { at.clone() } else { None };
        self.held_why = outcome
            .stop
            .as_ref()
            .filter(|_| self.paused)
            .map(|stop| match stop.kind {
                StopKind::Breakpoint => "Held at a breakpoint".to_owned(),
                StopKind::Watchpoint => format!("Held at a {}", stop.reason),
                StopKind::Step => "Stepped".to_owned(),
                _ => "Held".to_owned(),
            });
        self.next_at = if kind == Some(StopKind::Line) {
            at
        } else {
            None
        };
        // A Start arms the marked lines against the program it just
        // compiled, and says which of them bound to nothing. The Project
        // owns the marks, so it is told: a mark that can never hit is
        // drawn differently rather than left looking armed.
        if !outcome.unbound.is_empty() {
            let unbound = outcome.unbound.clone();
            self.project
                .update(cx, |project, cx| project.set_unbound(unbound, cx));
        }
        // Where the debugger holds the story — the transcript says so, and
        // the studio reveals it. A line played, a choice point and the end
        // are the story's own rhythm, and say nothing.
        if let Some(stop) = outcome.stop.as_ref().filter(|stop| stop.kind.holds()) {
            let text: SharedString = match &stop.at {
                Some((path, line)) => {
                    format!("— stopped at {path}:{line} ({})", stop.reason).into()
                }
                None => format!("— stopped ({})", stop.reason).into(),
            };
            self.push(Entry::Notice(text));
            if let Some((path, line)) = stop.at.clone() {
                cx.emit(PlayerEvent::Stopped { path, line });
            }
        }
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
                PlayStep::Choices(choices) => {
                    self.choices = choices;
                    self.choices_at = Some(std::time::Instant::now());
                }
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
        // At a choice point NOW is where the choices are offered, not the
        // line before them (decision log 2026-10-09).
        let follow = self
            .choices_span()
            .or_else(|| follow.map(|loc| (loc.path, loc.start as usize..loc.end as usize)));
        if let Some((path, span)) = follow
            && !self.follow_paused
            && brink_gpui_shell::settings::AppSettings::get(cx).follow_in_editor
        {
            cx.emit(PlayerEvent::Follow { path, span });
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
            let at = match &error {
                PlayError::Runtime(Fault { at, .. }) => at.clone(),
                _ => None,
            };
            self.push(Entry::Error {
                text: text.clone(),
                at,
            });
            cx.emit(PlayerEvent::Log {
                level: crate::output_log::Level::Error,
                text,
            });
            // A runtime fault names its site the way a breakpoint stop
            // does, so the studio reveals it the same way: the author
            // lands on the line that faulted instead of reading a
            // message and then going to look for it. ink's own runtime
            // reports `'story.ink' line 7` and this is the parity.
            if let PlayError::Runtime(Fault {
                at: Some((path, line)),
                ..
            }) = &error
            {
                cx.emit(PlayerEvent::Stopped {
                    path: path.clone(),
                    line: *line,
                });
            }
            if let PlayError::Compile(errors) = error {
                for line in errors {
                    let text = SharedString::from(line);
                    self.push(Entry::Error {
                        text: text.clone(),
                        at: None,
                    });
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
        let line = matches!(entry, Entry::Line { .. });
        // The story moving on settles what was rewound past.
        if line {
            self.undone.clear();
        }
        self.entries.push(entry);
        self.arrivals.push(std::time::Instant::now());
        // Item 0 is the head spacer, so entry `ix` is item `ix + 1`.
        self.list.splice(ix + 1..ix + 1, 1);
        // Beside the manuscript a story line's top sits on the NOW line,
        // where the manuscript puts its source (decision log 2026-10-09);
        // otherwise, and for chrome rows, just keep the row in view. The
        // row lays out where it falls — under the one before — and the
        // transcript then slides up until it meets NOW (`render`), measured
        // rather than estimated. (It used to snap the row to the top and
        // then back down: two jumps, one of them seen.)
        if self.write_mode && line {
            self.to_now = Some(NowMove {
                ix,
                slide: None,
                tries: NOW_TRIES,
            });
        } else {
            self.list.scroll_to_reveal_item(ix + 1);
        }
    }

    /// A speaker's colour: a theme colour picked by the name, so it is
    /// the same speaker in the same colour for the whole session.
    fn speaker_colour(speaker: &str, cx: &App) -> Hsla {
        let t = brink_gpui_shell::theme::current(cx).tokens;
        let slots = [
            t.syn_number,
            t.symbol_file,
            t.success,
            t.symbol_knot,
            t.syn_label,
            t.info,
            t.warning,
            t.symbol_stitch,
        ];
        brink_gpui_shell::theme::hsla(slots[crate::player_stage::colour_slot(speaker, slots.len())])
    }

    /// One transcript row on the Stage surface: everything hangs off the
    /// spine; a speaker's run is a rule in their colour with the name
    /// printed once; action is dimmed; the reader's pick is a ring on the
    /// spine with its `*` / `+`.
    fn render_entry(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = self.render_row(ix, cx);
        // A new row fades in and rises into place — the web Player's
        // `player-row-in` (400ms, 6px). Only while it is new.
        let fresh = self
            .arrivals
            .get(ix)
            .is_some_and(|at| at.elapsed() < ROW_IN);
        if !fresh {
            return row;
        }
        div()
            .relative()
            .child(row)
            .with_animation(
                ("player-row-in", self.generation as usize * 100_000 + ix),
                gpui::Animation::new(ROW_IN).with_easing(gpui::ease_out_quint()),
                |el, delta| el.opacity(delta).top(px(ROW_RISE * (1. - delta))),
            )
            .into_any_element()
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        use crate::player_stage::Role;
        let theme = cx.theme();
        let (fg, muted, danger, border, primary, hover) = (
            theme.foreground,
            theme.muted_foreground,
            theme.danger,
            theme.border,
            theme.primary,
            theme.muted.opacity(0.4),
        );
        let knot =
            brink_gpui_shell::theme::hsla(brink_gpui_shell::theme::current(cx).tokens.symbol_knot);
        let Some(entry) = self.entries.get(ix) else {
            // The tail: room under the last row for it to reach NOW.
            return div().h(px(self.tail)).into_any_element();
        };
        let spine = div()
            .absolute()
            .left(px(SPINE_X))
            .top_0()
            .bottom_0()
            .w(px(2.))
            .bg(border.opacity(0.6));
        let row = div().id(("play-row", ix)).relative().w_full().child(spine);
        match entry {
            Entry::Line { text, tags, source } => {
                let (role, shown, direction) = self.looks.get(ix).map_or_else(
                    || (Role::Narration, text.to_string(), None),
                    |look| (look.role.clone(), look.text.clone(), look.direction.clone()),
                );
                let active = self.active_row() == Some(ix);
                let mut body = v_flex().w_full().py(px(5.)).pr_4();
                let mut row = row;
                match &role {
                    Role::Speech { speaker, cue } => {
                        let colour = Self::speaker_colour(speaker, cx);
                        row = row.child(
                            div()
                                .absolute()
                                .left(px(SPINE_X - 1.))
                                .top(px(if *cue { 10. } else { 0. }))
                                .bottom_0()
                                .w(px(4.))
                                .when(*cue, |el| el.rounded_t(px(2.)))
                                .bg(colour),
                        );
                        if *cue {
                            body = body.child(
                                div()
                                    .pl(px(TEXT_X))
                                    .pt(px(6.))
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(colour)
                                    .child(speaker.to_uppercase()),
                            );
                        }
                        if !shown.is_empty() {
                            body = body.child(
                                div()
                                    .pl(px(SPEECH_X))
                                    .text_color(fg)
                                    .child(Self::speech(direction, shown, muted)),
                            );
                        }
                    }
                    Role::Action => {
                        body = body.child(div().pl(px(TEXT_X)).text_color(muted).child(shown));
                    }
                    Role::Narration | Role::Chrome => {
                        body = body.child(div().pl(px(TEXT_X)).text_color(fg).child(shown));
                    }
                }
                if self.show_tags && !tags.is_empty() {
                    body = body.child(h_flex().pl(px(TEXT_X)).pt_1().gap_1().children(
                        tags.iter().map(|tag| {
                            div()
                                .px_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(border)
                                .text_xs()
                                .text_color(muted)
                                .child(format!("#{tag}"))
                        }),
                    ));
                }
                let row = row.child(body).when(active, |el| {
                    el.bg(primary.opacity(0.14)).child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(3.))
                            .bg(primary),
                    )
                });
                let row = row.when(!active, |el| el.hover(move |s| s.bg(hover)));
                self.linked(row, source.as_ref(), false, cx)
            }
            Entry::Chosen {
                text,
                sticky,
                source,
                ..
            } => {
                let row = row
                    .py(px(6.))
                    .child(
                        Self::choice_mark(*sticky, 24., knot)
                            .absolute()
                            .left(px(SPINE_X - 11.))
                            .top(px(5.))
                            .bg(theme.background),
                    )
                    .child(
                        div()
                            .pl(px(TEXT_X))
                            .py(px(2.))
                            .text_color(knot)
                            .child(text.clone()),
                    )
                    .hover(move |s| s.bg(hover));
                self.linked(row, source.as_ref(), true, cx)
            }
            Entry::Notice(text) => row
                .child(
                    div()
                        .pl(px(TEXT_X))
                        .py_1()
                        .text_xs()
                        .text_color(muted)
                        .child(text.clone()),
                )
                .into_any_element(),
            Entry::Error { text, at } => {
                let body = div()
                    .pl(px(TEXT_X))
                    .py_1()
                    .text_xs()
                    .text_color(danger)
                    .child(text.clone());
                match at.clone() {
                    Some((path, line)) => row
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(body)
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(PlayerEvent::Stopped {
                                path: path.clone(),
                                line,
                            });
                        }))
                        .into_any_element(),
                    None => row.child(body).into_any_element(),
                }
            }
        }
    }

    /// A choice's ring, with its `*` (once only) or `+` (sticky) drawn
    /// centred in it — a font's asterisk sits up at cap height, which
    /// left it floating in the ring's top.
    fn choice_mark(sticky: bool, size: f32, colour: Hsla) -> gpui::Div {
        div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .border_2()
            .border_color(colour)
            .flex()
            .items_center()
            .justify_center()
            .child(brink_gpui_shell::icons::icon(
                if sticky {
                    BrinkIcon::ChoiceSticky
                } else {
                    BrinkIcon::ChoiceOnce
                },
                px(size * 0.55),
                colour,
            ))
    }

    /// A speech line's text, with a direction opening it — `(quietly)` —
    /// set apart: dimmed, italic, and a space before the words. One run of
    /// text, so a long line still wraps as prose.
    fn speech(direction: Option<String>, words: String, muted: Hsla) -> gpui::AnyElement {
        let Some(direction) = direction.filter(|d| !d.is_empty()) else {
            return words.into_any_element();
        };
        let set_apart = 0..direction.len();
        let text = if words.is_empty() {
            direction
        } else {
            format!("{direction} {words}")
        };
        gpui::StyledText::new(text)
            .with_highlights([(
                set_apart,
                gpui::HighlightStyle {
                    color: Some(muted),
                    font_style: Some(gpui::FontStyle::Italic),
                    ..Default::default()
                },
            )])
            .into_any_element()
    }

    /// Link a row back to where it was written (#3436, ruled 2026-09-02):
    /// while the row is hovered, a small go-to-source button in its
    /// top-right corner — inside the row, never over its text — with
    /// `file:line` for a tooltip; ⌘-click on the row does the same. A
    /// plain click does nothing, so reading never jumps the manuscript.
    fn linked(
        &self,
        row: gpui::Stateful<gpui::Div>,
        source: Option<&Location>,
        choice: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(loc) = source.cloned() else {
            return row.into_any_element();
        };
        let theme = cx.theme();
        let (muted, panel, border, accent) = (
            theme.muted_foreground,
            theme.background,
            theme.border,
            theme.primary,
        );
        let line = self
            .project
            .read(cx)
            .loaded_source(&loc.path)
            .and_then(|text| text.get(..loc.start as usize))
            .map_or(0, |before| before.matches('\n').count() + 1);
        let name = loc.path.rsplit('/').next().unwrap_or(&loc.path).to_owned();
        let tip = SharedString::from(format!("{name}:{line} · ⌘-click to open"));
        let open = {
            let loc = loc.clone();
            move |cx: &mut Context<Self>| {
                cx.emit(PlayerEvent::Navigate {
                    path: loc.path.clone(),
                    span: loc.start as usize..loc.end as usize,
                });
            }
        };
        let chip = {
            let open = open.clone();
            div()
                .id("player-provenance")
                .absolute()
                .right(px(6.))
                .top(px(4.))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .border_1()
                .border_color(border)
                .bg(panel)
                .shadow_md()
                .cursor_pointer()
                .invisible()
                .group_hover(ROW_GROUP, |s| s.visible())
                .hover(move |s| s.border_color(accent))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .child(brink_gpui_shell::icons::icon(
                    BrinkIcon::GoToSource,
                    px(12.),
                    muted,
                ))
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    open(cx);
                }))
        };
        row.group(ROW_GROUP)
            .child(chip)
            .on_hover(Self::hover_listener(loc.clone(), choice, cx))
            .on_click(cx.listener(move |_, event: &ClickEvent, _, cx| {
                if event.modifiers().platform {
                    open(cx);
                }
            }))
            .into_any_element()
    }

    /// Track the pointer over a row or card with `loc` for its source:
    /// the manuscript brackets it while it is hovered.
    fn hover_listener(
        loc: Location,
        choice: bool,
        cx: &mut Context<Self>,
    ) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        cx.listener(move |this, hovered: &bool, _, cx| {
            if *hovered {
                this.hovered = Some((loc.clone(), choice));
                if brink_gpui_shell::settings::AppSettings::get(cx).follow_in_editor
                    && !this.follow_paused
                {
                    cx.emit(PlayerEvent::Peek {
                        path: loc.path.clone(),
                        span: loc.start as usize..loc.end as usize,
                    });
                }
            } else if this.hovered.as_ref().is_some_and(|(l, _)| *l == loc) {
                this.hovered = None;
            } else {
                return;
            }
            cx.notify();
        })
    }

    /// The live choices as cards above the transport strip, each with its
    /// `*` / `+` and its number (the digit picks it).
    fn render_cards(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.choices.is_empty() {
            return None;
        }
        let theme = cx.theme();
        let (fg, muted, border, card) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.secondary,
        );
        let knot =
            brink_gpui_shell::theme::hsla(brink_gpui_shell::theme::current(cx).tokens.symbol_knot);
        let busy = self.busy;
        Some(
            v_flex()
                .w_full()
                .gap(px(8.))
                .px_4()
                .pt(px(8.))
                .pb(px(10.))
                .children(self.choices.iter().enumerate().map(|(n, choice)| {
                    let index = choice.index;
                    let card = h_flex()
                        .id(("play-choice", index))
                        .w_full()
                        .h(px(40.))
                        .px(px(12.))
                        .gap(px(12.))
                        .items_center()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(border)
                        .bg(card)
                        .when_some(choice.source.clone(), |el, loc| {
                            el.on_hover(Self::hover_listener(loc, true, cx))
                        })
                        .text_color(fg)
                        .when(!busy, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.border_color(knot))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.choose(index, cx);
                                }))
                        })
                        .child(Self::choice_mark(choice.sticky, 22., knot))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(choice.text.trim().to_owned()),
                        )
                        .child(div().text_xs().text_color(muted).child((n + 1).to_string()));
                    // The web Player's `player-choice-in`: each card slides
                    // in from the left, one after another — while new.
                    match self.choices_at.filter(|at| at.elapsed() < cards_in(n)) {
                        None => card.into_any_element(),
                        Some(_) => {
                            let total = cards_in(n);
                            let wait = CARD_STAGGER.as_secs_f32() * n as f32 / total.as_secs_f32();
                            card.with_animation(
                                ("player-card-in", self.generation as usize * 1_000 + n),
                                gpui::Animation::new(total),
                                move |el, delta| {
                                    let t = gpui::ease_out_quint()(
                                        ((delta - wait) / (1. - wait)).clamp(0., 1.),
                                    );
                                    el.opacity(t).left(px(-CARD_SLIDE * (1. - t)))
                                },
                            )
                            .into_any_element()
                        }
                    }
                }))
                .into_any_element(),
        )
    }

    /// A header or strip button: an icon, a tooltip, lit when `on`.
    fn icon_button(
        id: &'static str,
        icon: BrinkIcon,
        tooltip: impl Into<SharedString>,
        on: Option<Hsla>,
        enabled: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let (fg, hover) = (theme.muted_foreground, theme.muted.opacity(0.5));
        div()
            .id(id)
            .size(px(30.))
            .flex_none()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .border_color(gpui::transparent_black())
            .when_some(on, |el, c| {
                el.bg(c.opacity(0.14)).border_color(c.opacity(0.4))
            })
            .when(enabled, |el| {
                el.cursor_pointer().hover(move |s| s.bg(hover))
            })
            .when(!enabled, |el| el.opacity(0.3))
            .tooltip({
                let tooltip: SharedString = tooltip.into();
                move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .child(brink_gpui_shell::icons::icon(
                icon,
                px(16.),
                on.unwrap_or(fg),
            ))
    }

    /// What the header says about the session, and in what colour.
    /// What the header says about the session, in what colour, and
    /// whether its dot is hollow (following paused — not a state of the
    /// story, a state of the editor). The canvas's states, 2026-10-09.
    fn status(&self, cx: &App) -> (SharedString, Hsla, bool) {
        let theme = cx.theme();
        let tokens = brink_gpui_shell::theme::current(cx).tokens;
        let hsla = brink_gpui_shell::theme::hsla;
        let line_of = |path: &str, offset: u32| {
            self.project
                .read(cx)
                .loaded_source(path)
                .and_then(|text| text.get(..offset as usize))
                .map_or(0, |before| before.matches('\n').count() + 1)
        };
        let here = self
            .current_source()
            .map(|loc| format!("{} {}", loc.path, line_of(&loc.path, loc.start)));
        let with_here = |what: &str| match &here {
            Some(at) => format!("{what} · {at}"),
            None => what.to_owned(),
        };
        if self.stale {
            return ("Sources changed — Restart".into(), theme.warning, false);
        }
        // A line in flight is a beat, not a state: only a start says so.
        if self.busy && self.entries.is_empty() {
            return ("Starting\u{2026}".into(), theme.muted_foreground, false);
        }
        if self.paused {
            let why = self.held_why.as_deref().unwrap_or("Held");
            let label = match &self.held_at {
                Some((path, line)) => format!("{why} · {path} {line}"),
                None => why.to_owned(),
            };
            return (label.into(), hsla(tokens.warning), false);
        }
        if !self.choices.is_empty() {
            return ("Choose".into(), hsla(tokens.symbol_knot), false);
        }
        if self.autoplay {
            // Where it will stop, as the lookahead found (the canvas's
            // "stops at ● tower.ink 6"); where it is until that answers.
            let label = match self.autoplay_stop.as_ref() {
                Some(stop) => match (stop.kind, &stop.at) {
                    (StopKind::Breakpoint, Some((path, line))) => {
                        let name = path.rsplit('/').next().unwrap_or(path);
                        format!("Autoplaying · stops at ● {name} {line}")
                    }
                    (StopKind::Watchpoint, _) => {
                        format!("Autoplaying · stops at a {}", stop.reason)
                    }
                    (StopKind::Choices, _) => "Autoplaying · to the next choice".to_owned(),
                    (StopKind::Terminal, _) => "Autoplaying · to the end".to_owned(),
                    _ => with_here("Autoplaying"),
                },
                None => with_here("Autoplaying"),
            };
            return (label.into(), hsla(tokens.info), false);
        }
        if self.running {
            let following = brink_gpui_shell::settings::AppSettings::get(cx).follow_in_editor;
            if following && self.follow_paused {
                return (
                    "Paused following — you\u{2019}re editing".into(),
                    hsla(tokens.warning),
                    true,
                );
            }
            return (with_here("Playing").into(), hsla(tokens.success), false);
        }
        if self.entries.is_empty() {
            ("Ready".into(), theme.muted_foreground, false)
        } else {
            ("Ended".into(), theme.muted_foreground, false)
        }
    }

    /// The session header (decision log 2026-10-09): the status — a click
    /// returns to the current line — then Follow, Show tags, Save state,
    /// and ⋯ for settings.
    fn render_header(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (bar, border, fg, primary) = (
            theme.title_bar,
            theme.border,
            theme.foreground,
            theme.primary,
        );
        let amber =
            brink_gpui_shell::theme::hsla(brink_gpui_shell::theme::current(cx).tokens.warning);
        let (label, colour, hollow) = self.status(cx);
        // The list's width as last laid out — the panel's; 0 before it has
        // laid out, which reads as wide.
        let width = f32::from(self.list.viewport_bounds().size.width);
        let narrow = width > 0. && width < NARROW_HEADER;
        let can_save = self.running && !self.busy;
        let save_tip: SharedString = match self.attached.as_ref().and_then(|(store, id)| {
            self.saves
                .iter()
                .find(|(s, m)| s == store && &m.id == id)
                .map(|(_, m)| m.name.clone())
        }) {
            Some(name) => format!("Save state — writes back to {name}").into(),
            None => "Save state — a new save".into(),
        };
        let follow_on = brink_gpui_shell::settings::AppSettings::get(cx).follow_in_editor;
        let (follow_icon, follow_lit, follow_tip) = match (follow_on, self.follow_paused) {
            (true, true) => (
                BrinkIcon::PlayerFollowPaused,
                Some(amber),
                "Following paused while you edit — click to resume",
            ),
            (true, false) => (
                BrinkIcon::PlayerFollow,
                Some(primary),
                "Following the manuscript",
            ),
            (false, _) => (BrinkIcon::PlayerFollow, None, "Follow the manuscript"),
        };
        h_flex()
            .h(px(36.))
            .flex_none()
            .pl(px(14.))
            .pr(px(6.))
            .gap(px(2.))
            .items_center()
            .bg(bar)
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .id("player-status")
                    .min_w_0()
                    .gap(px(7.))
                    .items_center()
                    .text_xs()
                    .text_color(fg)
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(8.))
                            .flex_none()
                            .rounded_full()
                            .when(hollow, |el| el.border_1().border_color(colour))
                            .when(!hollow, |el| el.bg(colour)),
                    )
                    .child(div().truncate().child(label))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        if let Some(loc) = this.current_source().cloned() {
                            cx.emit(PlayerEvent::Follow {
                                path: loc.path,
                                span: loc.start as usize..loc.end as usize,
                            });
                        }
                    })),
            )
            .child(div().flex_1())
            .child(
                Self::icon_button(
                    "player-follow",
                    follow_icon,
                    follow_tip,
                    follow_lit,
                    true,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_follow(cx))),
            )
            // Narrow, Tags and Save fold into ⋯; Follow stays out, since
            // it is the one toggle used mid-play (canvas, 2026-10-09).
            .when(!narrow, |el| {
                el.child(
                    Self::icon_button(
                        "player-tags",
                        BrinkIcon::PlayerTags,
                        "Show tags",
                        self.show_tags.then_some(primary),
                        true,
                        cx,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_tags(cx))),
                )
                .child(
                    Self::icon_button(
                        "player-save",
                        BrinkIcon::PlayerSave,
                        save_tip.clone(),
                        None,
                        can_save,
                        cx,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_state(cx))),
                )
                .child(
                    Self::icon_button(
                        "player-more",
                        BrinkIcon::Dots,
                        "Player settings",
                        None,
                        true,
                        cx,
                    )
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(PlayerEvent::OpenSettings)),
                    ),
                )
            })
            .child(self.render_saves_menu(cx))
            .when(narrow, |el| el.child(self.render_more_menu(cx)))
            .when(self.write_mode, |el| {
                el.child(
                    Self::icon_button(
                        "player-close",
                        BrinkIcon::PlayerClose,
                        "Close the Player",
                        None,
                        true,
                        cx,
                    )
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(PlayerEvent::Close))),
                )
            })
            .into_any_element()
    }

    /// Show or hide the transcript's tags.
    fn toggle_tags(&mut self, cx: &mut Context<Self>) {
        self.show_tags = !self.show_tags;
        cx.notify();
    }

    /// The header's saves menu (decision log 2026-10-09): both stores'
    /// slots, read when it opens — Load (and save back to it) or Fork
    /// (start from a copy). Deleting lives on the idle Player's list.
    fn render_saves_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui_component::button::{Button, ButtonVariants as _};
        use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
        use gpui_component::{IconName, Sizable as _};
        let me = cx.entity().downgrade();
        let root = self.project.read(cx).root().to_path_buf();
        Button::new("player-saves")
            .ghost()
            .small()
            .icon(IconName::FolderOpen)
            .tooltip("Saves")
            .dropdown_menu(move |menu, _, _| {
                let slots = all_saves(&root);
                if slots.is_empty() {
                    return menu.item(PopupMenuItem::new("No saves yet").disabled(true));
                }
                let pick = |fork: bool| {
                    let me = me.clone();
                    move |store: crate::saves::Store, id: String| {
                        let me = me.clone();
                        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                            let _ = me.update(cx, |this, cx| this.load_save(store, &id, fork, cx));
                        }
                    }
                };
                let (load, fork) = (pick(false), pick(true));
                let mut menu = menu.label("Load — and save back to it");
                for (store, meta) in slots.iter().take(SAVES_IN_MENU) {
                    menu = menu.item(
                        PopupMenuItem::new(slot_label(*store, meta))
                            .on_click(load(*store, meta.id.clone())),
                    );
                }
                menu = menu.separator().label("Fork — start from a copy");
                for (store, meta) in slots.iter().take(SAVES_IN_MENU) {
                    menu = menu.item(
                        PopupMenuItem::new(slot_label(*store, meta))
                            .on_click(fork(*store, meta.id.clone())),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// The idle Player's saves: each with Load, Fork and delete.
    fn render_idle_saves(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, hover) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.muted.opacity(0.5),
        );
        let action = |id: (&'static str, usize), label: &'static str| {
            div()
                .id(id)
                .px_2()
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(5.))
                .border_1()
                .border_color(border)
                .text_xs()
                .text_color(fg)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(label)
        };
        v_flex()
            .pl(px(TEXT_X))
            .pr_4()
            .pt_4()
            .gap(px(6.))
            .child(div().text_xs().text_color(muted).child("Saves"))
            .children(self.saves.iter().enumerate().map(|(n, (store, meta))| {
                let (store, id) = (*store, meta.id.clone());
                let (id_load, id_fork, id_delete) = (id.clone(), id.clone(), id);
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(fg)
                            .child(slot_label(store, meta)),
                    )
                    .child(action(("save-load", n), "Load").on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.load_save(store, &id_load, false, cx)
                        },
                    )))
                    .child(action(("save-fork", n), "Fork").on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            this.load_save(store, &id_fork, true, cx)
                        },
                    )))
                    .child(action(("save-delete", n), "Delete").on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| this.delete_save(store, &id_delete, cx),
                    )))
            }))
            .into_any_element()
    }

    /// The narrow header's ⋯: what folded out of the bar, then settings.
    fn render_more_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui_component::button::{Button, ButtonVariants as _};
        use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
        use gpui_component::{IconName, Sizable as _};
        let me = cx.entity().downgrade();
        let tags = self.show_tags;
        let can_save = self.running && !self.busy;
        Button::new("player-more")
            .ghost()
            .small()
            .icon(IconName::Ellipsis)
            .tooltip("More")
            .dropdown_menu(move |menu, _, _| {
                let toggle_tags = {
                    let me = me.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = me.update(cx, |this, cx| this.toggle_tags(cx));
                    }
                };
                let settings = {
                    let me = me.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = me.update(cx, |_, cx| cx.emit(PlayerEvent::OpenSettings));
                    }
                };
                let save = {
                    let me = me.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = me.update(cx, |this, cx| this.save_state(cx));
                    }
                };
                menu.item(
                    PopupMenuItem::new("Show tags")
                        .checked(tags)
                        .on_click(toggle_tags),
                )
                .item(
                    PopupMenuItem::new("Save state")
                        .disabled(!can_save)
                        .on_click(save),
                )
                .separator()
                .item(PopupMenuItem::new("Player settings").on_click(settings))
            })
            .into_any_element()
    }

    /// The transport strip (decision log 2026-10-09): Restart and Stop at
    /// its edge; centred `|<` · `<<` · ▶ · `>>` · `>|`; in Script mode,
    /// Step and Step Instruction after them; and a hint saying what space
    /// does now. Rewind is #3665; Autoplay, Skip and line-at-a-time
    /// Continue come with the hook-up — drawn, not yet live.
    fn render_strip(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let (bar, border, muted, primary, bg) = (
            theme.title_bar,
            theme.border,
            theme.muted_foreground,
            theme.primary,
            theme.background,
        );
        let started = !self.entries.is_empty();
        let width = f32::from(self.list.viewport_bounds().size.width);
        let roomy = width == 0. || width >= HINT_MIN_WIDTH;
        let can_primary = !self.running || self.autoplay || self.can_advance();
        let can_back = self.history > 0 && !self.busy;
        let advance = self.can_advance();
        let hint: SharedString = if self.autoplay {
            "space · pause".into()
        } else if !self.choices.is_empty() {
            format!("1\u{2013}{} · choose", self.choices.len()).into()
        } else if !self.running {
            "space · play".into()
        } else if self.paused {
            "space · play the held line".into()
        } else {
            "space · continue".into()
        };
        let (primary_icon, primary_tip) = if self.autoplay {
            (BrinkIcon::TransportPause, "Pause (space)")
        } else {
            (BrinkIcon::TransportPlay, "Continue (space)")
        };
        let stepping = !self.write_mode && self.paused && !self.busy;
        h_flex()
            .relative()
            .h(px(58.))
            .flex_none()
            .justify_center()
            .items_center()
            .gap(px(12.))
            .bg(bar)
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .absolute()
                    .left(px(10.))
                    .gap(px(2.))
                    .child(
                        Self::icon_button(
                            "player-restart",
                            BrinkIcon::TransportRestart,
                            "Restart",
                            None,
                            started,
                            cx,
                        )
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.restart(cx))),
                    )
                    .child(
                        Self::icon_button(
                            "player-stop",
                            BrinkIcon::TransportStop,
                            "Stop — end the story",
                            None,
                            self.running,
                            cx,
                        )
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.stop(cx))),
                    ),
            )
            .child(
                Self::icon_button(
                    "player-prev",
                    BrinkIcon::TransportPrev,
                    "Back to just before the last choice",
                    None,
                    can_back,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back_to_choice(cx))),
            )
            .child(
                Self::icon_button(
                    "player-back",
                    BrinkIcon::TransportBack,
                    "Back one line",
                    None,
                    can_back,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back_one_line(cx))),
            )
            .child(
                div()
                    .id("player-primary")
                    .size(px(44.))
                    .flex_none()
                    .rounded_full()
                    .bg(primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(!can_primary, |el| el.opacity(0.3))
                    .when(can_primary, |el| el.cursor_pointer())
                    .tooltip(move |window, cx| Tooltip::new(primary_tip).build(window, cx))
                    .child(brink_gpui_shell::icons::icon(primary_icon, px(18.), bg))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.primary(cx))),
            )
            .child(
                Self::icon_button(
                    "player-auto",
                    BrinkIcon::TransportAuto,
                    "Autoplay — on at the reading pace until a breakpoint, a choice or the end",
                    self.autoplay.then_some(primary),
                    self.autoplay || advance,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_autoplay(cx))),
            )
            .child(
                Self::icon_button(
                    "player-skip",
                    BrinkIcon::TransportSkip,
                    "Skip to the next breakpoint, choice or end",
                    None,
                    advance,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.skip(cx))),
            )
            .when(!self.write_mode, |el| {
                el.child(
                    Self::icon_button(
                        "player-step",
                        BrinkIcon::TransportStep,
                        "Step (F10)",
                        None,
                        stepping,
                        cx,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.debug(PlayCommand::StepLine, cx);
                    })),
                )
                .child(
                    Self::icon_button(
                        "player-step-instruction",
                        BrinkIcon::TransportStepInstruction,
                        "Step Instruction (F11)",
                        None,
                        stepping,
                        cx,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.debug(PlayCommand::StepInstruction, cx);
                    })),
                )
            })
            // The hint gives way before it would run into the transport.
            .when(roomy, |el| {
                el.child(
                    div()
                        .absolute()
                        .right(px(14.))
                        .text_xs()
                        .text_color(muted)
                        .child(hint),
                )
            })
            .into_any_element()
    }
}

/// Where a session has been (`Player::trail`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayTrail {
    pub played: Vec<Location>,
    pub active: Option<Location>,
    pub held: Option<(String, u32)>,
    pub next: Option<(String, u32)>,
    /// The source of the transcript row (or choice card) under the
    /// pointer, and whether it is a choice — the manuscript brackets it.
    pub hover: Option<(Location, bool)>,
    /// Where NOW is: the choices on offer, or the line the story is on.
    pub now: Option<Location>,
    /// The choices taken, and the ones offered beside them and passed by.
    pub chosen: Vec<Location>,
    pub not_taken: Vec<Location>,
    /// Lines rewound past, until the story moves on.
    pub undone: Vec<Location>,
}

/// The story session's state, for the status bar (`docs/studio-shell-spec.md`
/// §7.3: "story state (idle / running / awaiting choice)").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No story has been started in this window.
    Idle,
    /// A command is in flight.
    Working,
    /// Running, and waiting for the reader to pick.
    AwaitingChoice,
    /// Started, mid-turn, nothing to pick yet.
    Running,
    /// The story ended, or an error stopped it.
    Over,
}

impl SessionState {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "running…",
            Self::AwaitingChoice => "awaiting a choice",
            Self::Running => "running",
            Self::Over => "story over",
        }
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

    /// Not the kit's to close: its `…` → Close would be a second way
    /// out beside the tab's own ✕ (`crate::tab_title`), which goes through
    /// the studio like every other close. Removal from the dock does not
    /// ask this.
    fn closable(&self, _cx: &App) -> bool {
        false
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
    fn title(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label =
            brink_gpui_shell::tool_window::tab_title(gpui_component::IconName::Play, "Player");
        crate::tab_title::closable_tab(cx.entity_id(), label, cx)
    }

    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The NOW line, and the room the transcript's end needs to reach it
        // (as the list last laid out).
        self.now_y = now_line(window);
        let viewport = self.list.viewport_bounds();
        let tail = if self.write_mode {
            (f32::from(viewport.bottom()) - self.now_y).max(0.)
        } else {
            0.
        };
        if (tail - self.tail).abs() > 0.5 {
            self.tail = tail;
            let n = self.entries.len();
            self.list.remeasure_items(n + 1..n + 2);
        }
        let head = if self.write_mode {
            (self.now_y - f32::from(viewport.top())).max(0.)
        } else {
            0.
        };
        if (head - self.head).abs() > 0.5 {
            self.head = head;
            self.list.remeasure_items(0..1);
        }
        // Bring the current line to NOW: measure where the last layout
        // really put it, scroll by the difference, and look again next
        // frame — until it is there, or the tries run out.
        if let Some(mut slide) = self.to_now.take() {
            match self.list.bounds_for_item(slide.ix + 1) {
                Some(row) => {
                    // Where it is now; where the slide wants it this frame.
                    let off = f32::from(row.top()) - self.now_y;
                    let (from, started) =
                        *slide.slide.get_or_insert((off, std::time::Instant::now()));
                    let t = (started.elapsed().as_secs_f32() / NOW_SLIDE.as_secs_f32()).min(1.);
                    // Ease-out cubic: settles gently onto NOW rather than
                    // covering most of the way in the first few frames.
                    let want = from * (1. - t).powi(3);
                    let by = off - want;
                    if by.abs() > 0.25 {
                        self.list.scroll_by(px(by));
                    }
                    if t < 1. || want.abs() > 1. {
                        self.to_now = Some(slide);
                    }
                }
                // Not laid out yet: give it a frame or two; past that it is
                // out of reach (scrolled well away), so go to it.
                None if slide.tries > 0 => {
                    slide.tries -= 1;
                    self.to_now = Some(slide);
                }
                None => {
                    self.list.scroll_to(gpui::ListOffset {
                        item_ix: slide.ix + 1,
                        offset_in_item: px(0.),
                    });
                    self.to_now = Some(NowMove {
                        tries: NOW_TRIES,
                        ..slide
                    });
                }
            }
            if self.to_now.is_some() {
                window.request_animation_frame();
            }
        }
        // Read the transcript as Stage runs, through the project's dialect
        // (rebuilt only when the dialect changes).
        let dialect = self.project.read(cx).dialogue().0.cloned();
        match (&dialect, &self.reader) {
            (Some(d), Some(reader)) if reader.is_for(d) => {}
            (Some(d), _) => self.reader = crate::player_stage::Reader::new(d),
            (None, _) => self.reader = None,
        }
        let looks = {
            let rows: Vec<Option<&str>> = self
                .entries
                .iter()
                .map(|e| match e {
                    Entry::Line { text, .. } => Some(text.as_ref()),
                    _ => None,
                })
                .collect();
            let echo: Vec<bool> = self
                .entries
                .iter()
                .map(|e| matches!(e, Entry::Chosen { .. }))
                .collect();
            crate::player_stage::read(&rows, &echo, self.reader.as_ref())
        };
        self.looks = looks;

        let theme = cx.theme();
        let (muted, surface) = (theme.muted_foreground, theme.background);
        let started = !self.entries.is_empty();
        let prose_size = brink_gpui_shell::settings::AppSettings::get(cx).player_size();
        let header = self.render_header(cx);
        let idle_saves = self.render_idle_saves(cx);
        let cards = self.render_cards(cx);
        let strip = self.render_strip(cx);

        v_flex()
            .id("player")
            .track_focus(&self.focus)
            // The pointer leaving the Player ends a hover's peek.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered {
                    cx.emit(PlayerEvent::PeekEnd {
                        now: this.now_target(),
                    });
                }
            }))
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .bg(surface)
            .text_sm()
            .child(header)
            .when(!started, |el| {
                el.child(
                    v_flex()
                        .flex_1()
                        .child(
                            div()
                                .p_4()
                                .pl(px(TEXT_X))
                                .text_xs()
                                .text_color(muted)
                                .child(
                                    "Nothing is running. ▶ plays the story from its entry; \
                                     \"Play from here\" on a knot starts there.",
                                ),
                        )
                        .when(!self.saves.is_empty(), |el| el.child(idle_saves)),
                )
            })
            .when(started, |el| {
                el.child(
                    // The transcript's own size — the reading surface, not
                    // the chrome around it.
                    list(
                        self.list.clone(),
                        cx.processor(|this, ix: usize, _window, cx| match ix.checked_sub(1) {
                            // Item 0: room above the first row to reach NOW.
                            None => div().h(px(this.head)).into_any_element(),
                            Some(entry) => this.render_entry(entry, cx),
                        }),
                    )
                    .flex_1()
                    // No padding: the list measures rows from its own edge,
                    // so padding would set every row that much off NOW (the
                    // head and tail spacers are the room instead).
                    .text_size(px(prose_size)),
                )
            })
            .children(cards)
            .child(strip)
    }
}

/// A new row's arrival: how long, and how far it rises (the web's
/// `player-row-in`).
const ROW_IN: std::time::Duration = std::time::Duration::from_millis(400);
const ROW_RISE: f32 = 6.;
/// A choice card's slide in (the web's `player-choice-in`), and the
/// stagger between cards.
const CARD_IN: std::time::Duration = std::time::Duration::from_millis(320);
const CARD_STAGGER: std::time::Duration = std::time::Duration::from_millis(60);
const CARD_SLIDE: f32 = 10.;

/// How long card `n`'s arrival runs, its stagger included.
fn cards_in(n: usize) -> std::time::Duration {
    CARD_IN + CARD_STAGGER * u32::try_from(n).unwrap_or(u32::MAX)
}

/// How many frames a new line may take to be laid out before the list is
/// scrolled to it outright.
const NOW_TRIES: u8 = 4;

/// How long the transcript slides up to bring a new line to NOW.
const NOW_SLIDE: std::time::Duration = std::time::Duration::from_millis(220);

/// A story line on its way to NOW: which, and — once it is laid out —
/// where the slide began and when.
#[derive(Clone, Copy)]
struct NowMove {
    ix: usize,
    slide: Option<(f32, std::time::Instant)>,
    tries: u8,
}

/// How many saves each section of the header's saves menu lists.
const SAVES_IN_MENU: usize = 8;

/// Both stores' slots for the project at `root`, newest first within each.
fn all_saves(root: &std::path::Path) -> Vec<(crate::saves::Store, crate::saves::SlotMeta)> {
    [crate::saves::Store::Project, crate::saves::Store::Local]
        .into_iter()
        .flat_map(|store| {
            crate::saves::dir(store, root)
                .map(|dir| crate::saves::list(&dir))
                .unwrap_or_default()
                .into_iter()
                .map(move |meta| (store, meta))
        })
        .collect()
}

/// A slot as a list shows it: its name, where it is, the turn, and which
/// store holds it.
fn slot_label(store: crate::saves::Store, meta: &crate::saves::SlotMeta) -> String {
    let at = meta.knot_path.as_deref().unwrap_or("story");
    format!(
        "{} · {at} · turn {} · {}",
        meta.name,
        meta.turn,
        store.label()
    )
}

/// The hover group of a transcript row, for its go-to-source button.
const ROW_GROUP: &str = "player-row";

/// Below this width the header folds Tags and Save into ⋯.
const NARROW_HEADER: f32 = 380.;
/// Below this width the strip drops its hint: the centred transport needs
/// the room.
const HINT_MIN_WIDTH: f32 = 560.;

/// Where the NOW line sits, as a share of the window's height: the Player
/// and the manuscript both put the current line's top there.
const NOW_FRACTION: f32 = 0.4;

/// The NOW line of the window `handle`, for a test.
#[cfg(test)]
pub(crate) fn now_line_for_test(cx: &mut App, handle: gpui::AnyWindowHandle) -> f32 {
    handle
        .update(cx, |_, window, _| now_line(window))
        .unwrap_or(0.)
}

/// The NOW line in `window`'s coordinates.
pub(crate) fn now_line(window: &Window) -> f32 {
    f32::from(window.viewport_size().height) * NOW_FRACTION
}

/// Where the spine runs, where plain text starts, and where a speaker's
/// lines start (indented under their name, ruled 2026-09-02).
const SPINE_X: f32 = 35.;
const TEXT_X: f32 = 60.;
const SPEECH_X: f32 = 72.;

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
    fn every_session_state_says_something_different() {
        use super::SessionState;
        let all = [
            SessionState::Idle,
            SessionState::Working,
            SessionState::AwaitingChoice,
            SessionState::Running,
            SessionState::Over,
        ];
        let mut labels: Vec<&str> = all.iter().map(|s| s.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            all.len(),
            "two states reading the same is a lie"
        );
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
