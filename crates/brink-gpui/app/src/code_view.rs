//! **Code view** — decision log 2026-08-26: tabs, groups, splits; a writer
//! working across files.
//!
//! Its own `DockArea`, centre only, wearing the toolkit's skin: a
//! [`Document`] is a dock panel, so tabs, drag-between-groups and splits are
//! the toolkit's. The shell renders this view as one occupant of the editor
//! root (`brink_gpui_shell::editor_view`) and never looks inside it — which
//! is also what keeps the nesting reversible: this pane tree could be the
//! centre itself and nothing here would change.
//!
//! **This view owns the open documents.** The "active document" — the one
//! Single File view shows — is the document most recently opened here or
//! made the displayed tab of its group. That is the one fact the views
//! share (ruled 2026-08-26); nothing else crosses between them.

use std::ops::Range;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Subscription, Window, div,
};
use gpui_component::dock::{DockArea, DockPlacement, DockSkin, PanelStyle, panel_handle};

use crate::compiled_output::CompiledOutputView;
use crate::document::{Document, DocumentEvent};
use crate::player::Player;
use crate::project::Project;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeViewEvent {
    /// The active document changed, or there is none.
    ActiveChanged,
    /// A document's navigation asked to show `span` of `path`. Re-raised
    /// for the studio, which opens tabs.
    Navigate { path: String, span: Range<usize> },
}

pub struct CodeView {
    project: Entity<Project>,
    dock_area: Entity<DockArea>,
    /// Kept so the skin's settings outlive construction; nothing reads it.
    _skin: Rc<DockSkin>,
    /// Open documents, oldest first.
    documents: Vec<Entity<Document>>,
    /// Where each file was scrolled to, by root-relative path — including
    /// files whose tab has since been closed. A document is a view of the
    /// shared buffer and comes and goes; where you were reading in a file
    /// is a fact about the FILE, so it outlives the tab and is restored
    /// when the file is opened again.
    scroll: std::collections::BTreeMap<String, f32>,
    active: Option<Entity<Document>>,
    /// One subscription per open document, dropped with it when it closes.
    subscriptions: Vec<(Entity<Document>, Subscription)>,
}

impl CodeView {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (dock_area, skin) = DockSkin::dock_area("brink-code-view", Some(1), window, cx);
        // An editor's tab bar is always there: a lone file still gets a tab,
        // not a title strip (VS Code, Zed).
        skin.set_panel_style(PanelStyle::TabBar, cx);
        // Nothing to collapse here — this area has no docks.
        skin.set_toggle_button_visible(false, cx);
        Self {
            project,
            dock_area,
            _skin: skin,
            documents: Vec::new(),
            scroll: std::collections::BTreeMap::new(),
            active: None,
            subscriptions: Vec::new(),
        }
    }

    /// Open a file, or select its tab if it is already open, and optionally
    /// reveal a span inside it. Either way it becomes the active document.
    pub fn open(
        &mut self,
        path: &str,
        span: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .documents
            .iter()
            .find(|d| d.read(cx).path().as_ref() == path)
            .cloned();
        let document = match existing {
            Some(document) => {
                Document::activate(&document, window, cx);
                document
            }
            None => {
                let Some(text) = self.project.read(cx).loaded_source(path).map(str::to_owned)
                else {
                    return;
                };
                let project = self.project.clone();
                let key = SharedString::from(path.to_owned());
                let document = cx.new(|cx| Document::new(project, key, text, window, cx));
                self.dock_area.update(cx, |area, cx| {
                    area.add_panel_view(
                        panel_handle(document.clone()),
                        DockPlacement::Center,
                        None,
                        window,
                        cx,
                    );
                });
                if let Some(top) = self.scroll.get(path).copied() {
                    document.update(cx, |doc, cx| doc.set_scroll_top(top, cx));
                }
                Document::activate(&document, window, cx);
                let subscription = cx.subscribe(&document, Self::on_document_event);
                self.subscriptions.push((document.clone(), subscription));
                self.documents.push(document.clone());
                document
            }
        };
        if let Some(span) = span {
            document.update(cx, |document, cx| {
                document.reveal(span, window, cx);
            });
        }
        // Opening is an explicit act of making this the file you are on; the
        // dock will confirm it on the next tick, but the views should not
        // wait a frame to agree.
        self.set_active(Some(document), cx);
    }

    /// Put the Player in the centre dock (once) and select its tab.
    pub fn show_player(
        &mut self,
        player: &Entity<Player>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !player.read(cx).is_docked() {
            self.dock_area.update(cx, |area, cx| {
                area.add_panel_view(
                    panel_handle(player.clone()),
                    DockPlacement::Center,
                    None,
                    window,
                    cx,
                );
            });
        }
        Player::activate(player, window, cx);
    }

    /// Put Compiled Output in the centre dock (once) and select its tab.
    /// Same shape as [`Self::show_player`]: a singleton panel that is not
    /// a file, so it never joins `documents` and never becomes the active
    /// document Single File view would show.
    pub fn show_compiled(
        &mut self,
        compiled: &Entity<CompiledOutputView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !compiled.read(cx).is_docked() {
            self.dock_area.update(cx, |area, cx| {
                area.add_panel_view(
                    panel_handle(compiled.clone()),
                    DockPlacement::Center,
                    None,
                    window,
                    cx,
                );
            });
        }
        CompiledOutputView::activate(compiled, window, cx);
    }

    /// Note where a file was scrolled to. Bounded: a project has as many
    /// entries as it has files, and a path is only ever overwritten.
    fn remember_scroll(&mut self, path: String, top: f32) {
        if top.is_finite() {
            self.scroll.insert(path, top);
        }
    }

    /// Every remembered scroll, with the OPEN documents' current positions
    /// folded in — an open tab's live position is the truth, and the map
    /// only knows what it was told when a tab closed.
    #[must_use]
    pub fn scroll_state(&self, cx: &App) -> std::collections::BTreeMap<String, f32> {
        let mut out = self.scroll.clone();
        for document in &self.documents {
            let document = document.read(cx);
            let top = document.scroll_top(cx);
            if top.is_finite() {
                out.insert(document.path().to_string(), top);
            }
        }
        out
    }

    /// Seed the remembered scrolls — the persisted ones, at startup.
    pub fn set_scroll_state(&mut self, scroll: std::collections::BTreeMap<String, f32>) {
        self.scroll = scroll;
    }

    /// The document Single File view shows.
    #[must_use]
    pub fn active_document(&self) -> Option<&Entity<Document>> {
        self.active.as_ref()
    }

    fn set_active(&mut self, document: Option<Entity<Document>>, cx: &mut Context<Self>) {
        if self.active != document {
            self.active = document;
            cx.emit(CodeViewEvent::ActiveChanged);
            cx.notify();
        }
    }

    fn on_document_event(
        &mut self,
        document: Entity<Document>,
        event: &DocumentEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            DocumentEvent::Activated => self.set_active(Some(document), cx),
            DocumentEvent::Navigate { path, span } => cx.emit(CodeViewEvent::Navigate {
                path: path.clone(),
                span: span.clone(),
            }),
            DocumentEvent::Closed => {
                // Last chance to read it: the entity is about to go.
                let path = document.read(cx).path().to_string();
                let top = document.read(cx).scroll_top(cx);
                self.remember_scroll(path, top);
                self.documents.retain(|d| *d != document);
                self.subscriptions.retain(|(d, _)| *d != document);
                if self.active.as_ref() == Some(&document) {
                    // The dock will activate whichever tab takes the closed
                    // one's place; until it says so, fall back to the most
                    // recently opened.
                    let next = self.documents.last().cloned();
                    self.set_active(next, cx);
                }
            }
        }
    }
}

impl EventEmitter<CodeViewEvent> for CodeView {}

impl Focusable for CodeView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.dock_area.focus_handle(cx)
    }
}

impl Render for CodeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.dock_area.clone())
    }
}
