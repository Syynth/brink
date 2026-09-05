//! The GPUI-native brink studio — `docs/gpui-studio-spec.md`.
//!
//! Tier 3: the features, and the wiring. This file is the one place that
//! knows a Binder is a thing that goes in the left rail and that the three
//! editor views are Code, Single File and the manuscript — the shell does
//! not, and must not.

mod binder;
mod code_view;
mod continuous;
mod document;
mod icons;
mod problems;
mod project;
mod search;
mod single_view;

use std::ops::Range;
use std::path::PathBuf;

use brink_gpui_shell::editor_view::EditorView;
use brink_gpui_shell::region::RailSlot;
use brink_gpui_shell::tool_window::ToolWindowSpec;
use brink_gpui_shell::workspace::{StatusCell, Workspace};
use gpui::{
    AppContext as _, Application, Bounds, Context, Entity, Focusable as _, IntoElement, Render,
    Subscription, Window, WindowBounds, WindowOptions, actions, prelude::*, px, size,
};
use gpui_component::{Root, TitleBar};

use crate::binder::{Binder, BinderEvent};
use crate::code_view::CodeView;
use crate::continuous::ContinuousView;
use crate::problems::{OpenProblem, Problems};
use crate::project::{Project, ProjectEvent};
use crate::search::{SearchEvent, SearchView};
use crate::single_view::SingleFileView;

actions!(
    brink,
    [
        Save,
        /// `search.focus`: show the Search window and put the caret in it.
        SearchFocus,
    ]
);

/// The application root: it owns the model and the features, and hands the
/// shell its panels and views.
struct Studio {
    project: Entity<Project>,
    workspace: Entity<Workspace>,
    /// Code view — and with it the open documents. Opening a file always
    /// lands here, whichever view is showing: Single File shows this view's
    /// active document, and the manuscript reveals the file in place.
    code: Entity<CodeView>,
    /// Continuous view — the whole project as one scroller.
    manuscript: Entity<ContinuousView>,
    search: Entity<SearchView>,
    _subscriptions: Vec<Subscription>,
}

impl Studio {
    fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = cx.new(Project::new);
        let workspace = cx.new(|cx| Workspace::new(window, cx));

        let binder = cx.new(|cx| Binder::new(project.clone(), window, cx));
        let problems = cx.new(|cx| Problems::new(project.clone(), window, cx));
        let search = cx.new(|cx| SearchView::new(project.clone(), window, cx));
        let code = cx.new(|cx| CodeView::new(project.clone(), window, cx));
        let single = cx.new(|cx| SingleFileView::new(code.clone(), cx));
        let manuscript = cx.new(|cx| ContinuousView::new(project.clone(), window, cx));

        workspace.update(cx, |workspace, cx| {
            workspace.add_tool_window(
                ToolWindowSpec::new("binder", "Binder", RailSlot::LEFT_UPPER)
                    .icon(icons::FOLDER)
                    .size(px(260.))
                    .open(),
                binder.clone(),
                window,
                cx,
            );
            // Beside the Binder in the left dock — the second tab there,
            // which is what made the rail tab-aware.
            workspace.add_tool_window(
                ToolWindowSpec::new("search", "Search", RailSlot::LEFT_UPPER)
                    .icon(icons::SEARCH)
                    .size(px(320.)),
                search.clone(),
                window,
                cx,
            );
            workspace.add_tool_window(
                // Lower-left: with no bottom rail, this is what addresses
                // the bottom dock (`docs/gpui-studio-spec.md` §4.1).
                ToolWindowSpec::new("problems", "Problems", RailSlot::LEFT_LOWER)
                    .icon(icons::WARNING_MARK)
                    .size(px(160.))
                    .open(),
                problems.clone(),
                window,
                cx,
            );
            // The three views (decision log 2026-08-26). Registered before
            // the project opens so the manuscript is subscribed when the
            // files land.
            let code_focus = code.read(cx).focus_handle(cx);
            let single_focus = single.read(cx).focus_handle(cx);
            let manuscript_focus = manuscript.read(cx).focus_handle(cx);
            workspace.set_view_occupant(EditorView::Code, code.clone().into(), code_focus, cx);
            workspace.set_view_occupant(EditorView::Single, single.into(), single_focus, cx);
            workspace.set_view_occupant(
                EditorView::Continuous,
                manuscript.clone().into(),
                manuscript_focus,
                cx,
            );
            // The app's own commands go through the same registry as the
            // shell's, so the palette and the menu list them.
            workspace.register_command("File", "Save", Save, Some("cmd-s"), cx);
            // Studio: "Search: Find in Files", Mod-Shift-F (VS Code precedent).
            workspace.register_command(
                "Search",
                "Find in Files",
                SearchFocus,
                Some("cmd-shift-f"),
                cx,
            );
        });

        let on_project = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| match event {
                ProjectEvent::Opened { elapsed_ms } => {
                    eprintln!("project opened in {elapsed_ms:.1} ms");
                    for warning in this.project.read(cx).warnings() {
                        eprintln!("warning: {warning}");
                    }
                    this.open_initial(window, cx);
                    this.refresh_status(cx);
                }
                ProjectEvent::OpenFailed(message) => eprintln!("failed to open: {message}"),
                ProjectEvent::Analyzed => this.refresh_status(cx),
                ProjectEvent::SourceChanged { .. } | ProjectEvent::Saved => {}
            },
        );
        let on_binder = cx.subscribe_in(
            &binder,
            window,
            |this, binder, event: &BinderEvent, window, cx| {
                let BinderEvent::Open { path, offset } = event;
                this.open(path, offset.map(|o| o..o), window, cx);
                // The manuscript's per-file editors do not scroll — its list
                // does — so revealing a file there is a separate move from
                // opening its document.
                this.manuscript.update(cx, |manuscript, cx| {
                    manuscript.reveal(path, cx);
                });
                // Revealing an offset focuses the editor, which would kill
                // the Binder's own arrow-key navigation after the first
                // click. A panel click opens the document but keeps focus in
                // the panel — Zed's project-panel behaviour, and the
                // studio's.
                let handle = binder.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            },
        );
        let on_problem = cx.subscribe_in(
            &problems,
            window,
            |this, _, event: &OpenProblem, window, cx| {
                this.open(&event.path, Some(event.span.clone()), window, cx);
            },
        );
        let on_search = cx.subscribe_in(
            &search,
            window,
            |this, _, event: &SearchEvent, window, cx| {
                let SearchEvent::Reveal { path, span } = event;
                this.open(path, Some(span.clone()), window, cx);
            },
        );

        project.update(cx, |project, _| project.open(root));

        // Keys have somewhere to land from the first frame.
        let workspace_focus = workspace.read(cx).focus_handle(cx);
        window.focus(&workspace_focus, cx);

        Self {
            project,
            workspace,
            code,
            manuscript,
            search,
            _subscriptions: vec![on_project, on_binder, on_problem, on_search],
        }
    }

    /// Open the project's entry, or its first file when it names none.
    fn open_initial(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = {
            let project = self.project.read(cx);
            project
                .entry()
                .map(str::to_owned)
                .or_else(|| project.files().first().cloned())
        };
        if let Some(path) = first {
            self.open(&path, None, window, cx);
        }
    }

    /// Open a file in Code view, or select it if it is already open, and
    /// optionally reveal a span inside it.
    fn open(
        &mut self,
        path: &str,
        span: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code
            .update(cx, |code, cx| code.open(path, span, window, cx));
    }

    /// `search.focus`: show the window (open, never toggle) and focus the
    /// query — the studio's `ensureToolWindowOpen` + `requestSearchFocus`.
    fn search_focus(&mut self, _: &SearchFocus, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.open_tool_window("search", window, cx)
        });
        self.search
            .update(cx, |search, cx| search.focus_query(window, cx));
    }

    /// Save every dirty file — whichever editor it was changed in.
    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.project.update(cx, |project, cx| {
            for (path, err) in project.save_all(cx) {
                eprintln!("failed to save {path}: {err:#}");
            }
        });
    }

    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let cells = {
            let project = self.project.read(cx);
            let (last, worst) = project.timings();
            vec![
                StatusCell::new(project.root().display().to_string()),
                StatusCell::new(format!("{} files", project.files().len())),
                // "N errors — click → Problems" (spec §4 status bar).
                StatusCell::new(format!("{} problems", project.problem_count())).opens("problems"),
                StatusCell::new(format!("analyze {last:.1} ms")),
                StatusCell::new(format!("worst {worst:.1} ms")),
            ]
        };
        self.workspace
            .update(cx, |workspace, cx| workspace.set_status(cells, cx));
    }
}

impl Render for Studio {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        gpui::div()
            .size_full()
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::search_focus))
            .child(self.workspace.clone())
    }
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../tests/tier1-native/conventions-cross-file"));
    let root = root.canonicalize().unwrap_or(root);

    // gpui-pre publishes the core without a platform backend; the macOS/
    // Windows/Linux implementations live in `gpui-pre-platform`.
    Application::with_platform(gpui_platform::current_platform(false)).run(move |cx| {
        gpui_component::init(cx);
        let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..TitleBar::window_options()
        };
        let root = root.clone();
        let opened = cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| Studio::new(root, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        if let Err(err) = opened {
            eprintln!("failed to open window: {err:#}");
            std::process::exit(1);
        }
        cx.activate(true);
    });
}
