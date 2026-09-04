//! The GPUI-native brink studio — `docs/gpui-studio-spec.md`.
//!
//! Tier 3: the features, and the wiring. This file is the one place that
//! knows a Binder is a thing that goes in the left rail and a Document is a
//! thing that goes in the centre — the shell does not, and must not.

mod binder;
mod document;
mod icons;
mod problems;
mod project;

use std::path::PathBuf;

use brink_gpui_shell::region::RailSlot;
use brink_gpui_shell::tool_window::ToolWindowSpec;
use brink_gpui_shell::workspace::Workspace;
use gpui::{
    AppContext as _, Application, Bounds, Context, Entity, Focusable as _, IntoElement, Render,
    SharedString, Subscription, Window, WindowBounds, WindowOptions, actions, prelude::*, px, size,
};
use gpui_component::{Root, TitleBar};

use crate::binder::{Binder, BinderEvent};
use crate::document::Document;
use crate::problems::{OpenProblem, Problems};
use crate::project::{Project, ProjectEvent};

actions!(brink, [Save]);

/// The application root: it owns the model and the features, and hands the
/// shell its panels.
struct Studio {
    project: Entity<Project>,
    workspace: Entity<Workspace>,
    /// Open documents, newest last. A `Document` is itself a dock panel, so
    /// the centre's `TabGroup` provides the tabs — this list exists to find
    /// an already-open file and to know what to save.
    documents: Vec<Entity<Document>>,
    _subscriptions: Vec<Subscription>,
}

impl Studio {
    fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = cx.new(Project::new);
        let workspace = cx.new(|cx| Workspace::new(window, cx));

        let binder = cx.new(|cx| Binder::new(project.clone(), window, cx));
        let problems = cx.new(|cx| Problems::new(project.clone(), cx));

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
            },
        );
        let on_binder = cx.subscribe_in(
            &binder,
            window,
            |this, binder, event: &BinderEvent, window, cx| {
                let BinderEvent::Open { path, offset } = event;
                this.open(path, *offset, window, cx);
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
                this.open(&event.path, Some(event.offset), window, cx);
            },
        );

        project.update(cx, |project, _| project.open(root));

        Self {
            project,
            workspace,
            documents: Vec::new(),
            _subscriptions: vec![on_project, on_binder, on_problem],
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

    /// Open a file, or focus it if it is already open, and optionally reveal
    /// an offset inside it.
    fn open(
        &mut self,
        path: &str,
        offset: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let existing = self
            .documents
            .iter()
            .find(|d| d.read(cx).path().as_ref() == path)
            .cloned();
        let document = match existing {
            Some(document) => document,
            None => {
                let Some(text) = self.project.read(cx).loaded_source(path).map(str::to_owned)
                else {
                    return;
                };
                let project = self.project.clone();
                let key = SharedString::from(path.to_owned());
                let document = cx.new(|cx| Document::new(project, key, text, window, cx));
                self.workspace.update(cx, |workspace, cx| {
                    workspace.set_center(document.clone(), window, cx);
                });
                self.documents.push(document.clone());
                document
            }
        };
        if let Some(offset) = offset {
            document.update(cx, |document, cx| {
                document.reveal(offset, window, cx);
            });
        }
    }

    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        let root = self.project.read(cx).root().to_path_buf();
        for document in &self.documents {
            document.update(cx, |document, cx| {
                if document.is_dirty()
                    && let Err(err) = document.save(&root, cx)
                {
                    eprintln!("failed to save {}: {err:#}", document.path());
                }
            });
        }
    }

    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        let cells = {
            let project = self.project.read(cx);
            let (last, worst) = project.timings();
            vec![
                SharedString::from(project.root().display().to_string()),
                SharedString::from(format!("{} files", project.files().len())),
                SharedString::from(format!("{} problems", project.problem_count())),
                SharedString::from(format!("analyze {last:.1} ms")),
                SharedString::from(format!("worst {worst:.1} ms")),
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
        cx.bind_keys([gpui::KeyBinding::new("cmd-s", Save, None)]);
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
