//! The GPUI-native brink studio — `docs/gpui-studio-spec.md`.
//!
//! Tier 3: the features, and the wiring. This file is the one place that
//! knows a Binder is a thing that goes in the left rail and that the three
//! editor views are Code, Single File and the manuscript — the shell does
//! not, and must not.

mod binder;
mod code_view;
mod compiled_output;
mod continuous;
mod document;
mod fixes;
mod icons;
mod inkt_highlight;
mod navigation;
mod output_log;
mod player;
mod problems;
mod program;
mod project;
mod quick_open;
mod rename;
mod search;
mod settings_config;
mod settings_conventions;
mod settings_diagnostics;
mod settings_formatting;
mod settings_general;
mod settings_prose;
mod single_view;
mod todos;

use std::ops::Range;
use std::path::PathBuf;

use brink_gpui_model::query::{QueryKind, QueryResult};
use brink_gpui_shell::editor_view::EditorView;
use brink_gpui_shell::region::RailSlot;
use brink_gpui_shell::settings_modal::{Scope, Section, SectionMeta};
use brink_gpui_shell::tool_window::ToolWindowSpec;
use brink_gpui_shell::workspace::{StatusCell, Workspace};
use gpui::{
    App, AppContext as _, Application, Bounds, Context, Entity, Focusable as _, IntoElement,
    Render, Subscription, Task, Window, WindowBounds, WindowOptions, actions, prelude::*, px, size,
};
use gpui_component::WindowExt as _;
use gpui_component::{Root, TitleBar};

use crate::binder::{Binder, BinderEvent};
use crate::code_view::CodeView;
use crate::code_view::CodeViewEvent;
use crate::compiled_output::CompiledOutputView;
use crate::continuous::ContinuousView;
use crate::output_log::OutputLog;
use crate::player::{Player, PlayerEvent};
use crate::problems::{OpenProblem, Problems};
use crate::program::{ProgramEvent, ProgramExplorer};
use crate::project::{Project, ProjectEvent};
use crate::quick_open::{QuickOpen, QuickOpenEvent};
use crate::search::{SearchEvent, SearchView};
use crate::settings_conventions::ConventionsSection;
use crate::settings_diagnostics::DiagnosticsSection;
use crate::settings_formatting::FormattingSection;
use crate::settings_general::{GeneralSection, OpenConfig};
use crate::settings_prose::ProseSection;
use crate::single_view::SingleFileView;
use crate::todos::{OpenTodo, Todos};

actions!(
    brink,
    [
        Save,
        /// `search.focus`: show the Search window and put the caret in it.
        SearchFocus,
        /// Jump to the declaration of the symbol under the caret.
        GoToDefinition,
        /// Every use of the symbol under the caret, as Search cards.
        FindReferences,
        /// Rename the symbol under the caret, cross-file and safe-by-default.
        RenameSymbol,
        /// The active file as `brink fmt` would write it.
        FormatDocument,
        /// Every Safe fix in the active file, to a fixpoint.
        FixAllInFile,
        /// Every Safe fix in the compilation, to a fixpoint.
        FixAllInProject,
        /// Run the story from its entry, in the Player.
        Play,
        /// Run the story again from where the last Play began.
        PlayRestart,
        /// The compiled story's `.inkt` dump, as a read-only tab.
        OpenCompiledOutput,
        /// Go to a file, knot or stitch by name.
        QuickOpenGoTo,
        /// Close the studio, saving the window's shape on the way out.
        Quit,
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
    /// The Player, a centre tab in Code view. Made once; docked on the
    /// first Play, re-docked if its tab was closed.
    player: Entity<Player>,
    /// Compiled Output — the `.inkt` dump, a read-only Code-view tab on
    /// the same terms as the Player: made once, docked on first ask.
    compiled: Entity<CompiledOutputView>,
    /// Quick-open while it is up. Made per opening: its items are read
    /// when it opens, so there is nothing to keep alive between times.
    quick_open: Option<(Entity<QuickOpen>, Subscription)>,
    /// An observation of the ACTIVE document's editor, for the status
    /// bar's cursor cell. The caret has no event of its own, but moving it
    /// notifies — so this is an `observe`, replaced whenever the active
    /// document changes and dropped when there is none.
    caret: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl Studio {
    fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = cx.new(Project::new);
        let workspace = cx.new(|cx| Workspace::new(window, cx));

        let binder = cx.new(|cx| Binder::new(project.clone(), window, cx));
        let problems = cx.new(|cx| Problems::new(project.clone(), window, cx));
        let todos = cx.new(|cx| Todos::new(project.clone(), window, cx));
        let search = cx.new(|cx| SearchView::new(project.clone(), window, cx));
        let code = cx.new(|cx| CodeView::new(project.clone(), window, cx));
        let player = cx.new(|cx| Player::new(project.clone(), cx));
        let program = cx.new(|cx| ProgramExplorer::new(project.clone(), cx));
        let compiled = cx.new(|cx| CompiledOutputView::new(project.clone(), window, cx));
        let output = cx.new(|cx| OutputLog::new(project.clone(), cx));
        // The log keeps what the transcript throws away on a Restart.
        output.update(cx, |log, cx| log.watch_player(&player, cx));
        let single = cx.new(|cx| SingleFileView::new(code.clone(), cx));
        let manuscript = cx.new(|cx| ContinuousView::new(project.clone(), window, cx));
        let general = cx.new(|cx| GeneralSection::new(project.clone(), window, cx));
        let formatting = cx.new(|cx| FormattingSection::new(project.clone(), cx));
        let diagnostics = cx.new(|cx| DiagnosticsSection::new(project.clone(), window, cx));
        let prose = cx.new(|cx| ProseSection::new(project.clone(), window, cx));
        let conventions = cx.new(|cx| ConventionsSection::new(project.clone(), window, cx));

        workspace.update(cx, |workspace, cx| {
            // The Project scope: the shell owns the App sections, and this
            // crate owns `brink.toml` — the studio's four, in its order.
            workspace.add_settings_section(Section::new(
                SectionMeta::new(
                    "general",
                    Scope::Project,
                    "General",
                    &[
                        "brink.toml",
                        "entry",
                        "conventions",
                        "dialect",
                        "types",
                        "drafts",
                        "config",
                    ],
                ),
                general.clone(),
            ));
            workspace.add_settings_section(Section::new(
                SectionMeta::new(
                    "formatting",
                    Scope::Project,
                    "Formatting",
                    &[
                        "indent",
                        "spaces",
                        "tabs",
                        "width",
                        "fmt",
                        "format",
                        "whitespace",
                    ],
                ),
                formatting.clone(),
            ));
            workspace.add_settings_section(Section::new(
                SectionMeta::new(
                    "diagnostics",
                    Scope::Project,
                    "Diagnostics",
                    &[
                        "lints", "warnings", "errors", "todo", "suppress", "allow", "deny", "fix",
                    ],
                ),
                diagnostics.clone(),
            ));
            workspace.add_settings_section(Section::new(
                SectionMeta::new(
                    "prose",
                    Scope::Project,
                    "Prose",
                    &[
                        "spelling",
                        "spellcheck",
                        "grammar",
                        "dictionary",
                        "dialect",
                        "british",
                        "american",
                        "typo",
                    ],
                ),
                prose.clone(),
            ));
            workspace.add_settings_section(Section::new(
                SectionMeta::new(
                    "conventions",
                    Scope::Project,
                    "Conventions",
                    &[
                        "dialogue",
                        "dialect",
                        "cue",
                        "speaker",
                        "screenplay",
                        "teach",
                        "rules",
                        "character",
                    ],
                ),
                conventions.clone(),
            ));
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
            // Beside Problems in the lower-left dock: the second tab there.
            workspace.add_tool_window(
                ToolWindowSpec::new("todos", "TODOs", RailSlot::LEFT_LOWER)
                    .icon(icons::TODO)
                    .size(px(160.)),
                todos.clone(),
                window,
                cx,
            );
            // Third tab in the lower-left dock: the studio's Output /
            // compile log (`docs/studio-shell-spec.md` §4) — the timings
            // and the errors that have no file and span to sit on.
            workspace.add_tool_window(
                ToolWindowSpec::new("output", "Output", RailSlot::LEFT_LOWER)
                    .icon(icons::DOC)
                    .size(px(160.)),
                output.clone(),
                window,
                cx,
            );
            // The right dock's first occupant: the compiled program, a tall
            // tree that wants the side rather than the bottom.
            workspace.add_tool_window(
                ToolWindowSpec::new("program", "Program", RailSlot::RIGHT_UPPER)
                    .icon(icons::DOC)
                    .size(px(380.)),
                program.clone(),
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
            // Navigation (INVENTORY §0 item 1). Cmd-click goes through the
            // editor's own provider + `show_document` hook; the keyboard
            // commands resolve the focused editor here, because gpui-base's
            // `GoToDefinition` action only follows a target a Cmd-hover has
            // already resolved.
            workspace.register_command("Go", "Go to Definition", GoToDefinition, Some("f12"), cx);
            workspace.register_command(
                "Go",
                "Find References",
                FindReferences,
                Some("shift-f12"),
                cx,
            );
            workspace.register_command("Refactor", "Rename Symbol", RenameSymbol, Some("f2"), cx);
            workspace.register_command(
                "Refactor",
                "Code Actions",
                gpui_component::input::ToggleCodeActions,
                Some("cmd-."),
                cx,
            );
            workspace.register_command(
                "Refactor",
                "Format Document",
                FormatDocument,
                Some("alt-shift-f"),
                cx,
            );
            workspace.register_command("Fix", "Fix All Safe in File", FixAllInFile, None, cx);
            workspace.register_command("Fix", "Fix All Safe in Project", FixAllInProject, None, cx);
            // The find panel is the TOOLKIT's, not ours: `EditorState::new`
            // sets `searchable`, so every brink editor already carries it —
            // what was missing was a key to open it. Registering the kit's
            // own actions rather than wrapping them keeps one implementation
            // and puts them in the palette like everything else.
            workspace.register_command(
                "Find",
                "Find in File",
                gpui_component::input::Search,
                Some("cmd-f"),
                cx,
            );
            workspace.register_command(
                "Find",
                "Replace in File",
                gpui_component::input::Replace,
                Some("cmd-alt-f"),
                cx,
            );
            workspace.register_command("Play", "Play", Play, Some("cmd-r"), cx);
            workspace.register_command("Play", "Restart", PlayRestart, Some("cmd-shift-r"), cx);
            workspace.register_command("Program", "Compiled Output", OpenCompiledOutput, None, cx);
            workspace.register_command(
                "Go",
                "Go to File\u{2026}",
                QuickOpenGoTo,
                Some("cmd-p"),
                cx,
            );
            // An app with no Quit command is a gap on its own, and it is
            // also the only way the quit hook below is ever reached: a
            // kill signal does not run it.
            workspace.register_command("File", "Quit", Quit, Some("cmd-q"), cx);
            // After every tool window is registered: their `open()`
            // defaults decide the first run, and a saved shape overrides
            // them (`Workspace::apply_layout`).
            let saved = brink_gpui_shell::settings::AppSettings::get(cx).layout;
            workspace.apply_layout(&saved, window, cx);
        });

        // The remembered scrolls, but only if they belong to THIS project:
        // a scroll is per-file, and a path means a different place in a
        // different tree. Restored before the project opens, so the first
        // document to appear already lands where it was left.
        {
            let saved = brink_gpui_shell::settings::AppSettings::get(cx).layout;
            let root = root.display().to_string();
            if saved.scroll_root.as_deref() == Some(root.as_str()) {
                code.update(cx, |code, _| code.set_scroll_state(saved.scroll));
            }
        }

        // Persist the shape on quit. The toolkit fires `LayoutChanged` on
        // every step of a drag and asks subscribers to debounce; a quit
        // hook needs no timer and no debounce, and the shape a person
        // wants back is the one they left, not each frame of getting
        // there. Toggling a tool window or switching view writes too (see
        // `save_layout` in the handlers), so a crash loses at most an
        // unfinished drag.
        cx.on_app_quit({
            let workspace = workspace.clone();
            let code = code.clone();
            let project = project.clone();
            move |_: &mut Studio, cx: &mut Context<Studio>| {
                let root = project.read(cx).root().display().to_string();
                let scroll = code.read(cx).scroll_state(cx);
                Workspace::save_layout(&workspace, Some((root, scroll)), cx);
                async move {}
            }
        })
        .detach();

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
                let BinderEvent::Open { path, offset } = event else {
                    let BinderEvent::Play { path } = event else {
                        return;
                    };
                    this.play_at(Some(path.clone()), window, cx);
                    return;
                };
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
        let on_player = cx.subscribe_in(
            &player,
            window,
            |this, _, event: &PlayerEvent, window, cx| {
                // `Log` is the Output window's business, not navigation's.
                let PlayerEvent::Navigate { path, span } = event else {
                    return;
                };
                this.show(path, span.clone(), window, cx);
            },
        );
        let on_program = cx.subscribe_in(
            &program,
            window,
            |this, _, event: &ProgramEvent, window, cx| match event {
                ProgramEvent::Navigate { path, span } => {
                    this.show(path, span.clone(), window, cx);
                }
                ProgramEvent::OpenCompiledOutput => {
                    this.open_compiled_output(&OpenCompiledOutput, window, cx);
                }
            },
        );
        let on_problem = cx.subscribe_in(
            &problems,
            window,
            |this, _, event: &OpenProblem, window, cx| {
                this.open(&event.path, Some(event.span.clone()), window, cx);
            },
        );
        let on_todo = cx.subscribe_in(&todos, window, |this, _, event: &OpenTodo, window, cx| {
            this.open(&event.path, Some(event.span.clone()), window, cx);
        });
        let on_search = cx.subscribe_in(
            &search,
            window,
            |this, _, event: &SearchEvent, window, cx| {
                let SearchEvent::Reveal { path, span } = event;
                this.open(path, Some(span.clone()), window, cx);
            },
        );
        // A document's navigation raises where to go; the tabs are this
        // view's to open.
        let on_code = cx.subscribe_in(
            &code,
            window,
            |this, _, event: &CodeViewEvent, window, cx| match event {
                CodeViewEvent::Navigate { path, span } => {
                    this.open(path, Some(span.clone()), window, cx);
                }
                CodeViewEvent::ActiveChanged => {
                    this.watch_caret(cx);
                    this.refresh_status(cx);
                }
            },
        );
        // "Open brink.toml" in the General section: the text is a
        // document, so the section hands over to Code view.
        let on_general = cx.subscribe_in(
            &general,
            window,
            |this, _, event: &OpenConfig, window, cx| {
                this.workspace
                    .update(cx, |workspace, cx| workspace.close_settings(window, cx));
                this.open(&event.0, None, window, cx);
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
            player,
            compiled,
            quick_open: None,
            caret: None,
            _subscriptions: vec![
                on_project, on_binder, on_player, on_program, on_problem, on_todo, on_search,
                on_code, on_general,
            ],
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
    /// optionally reveal a span inside it. `brink.toml` included: it is a
    /// document like any other here (unlike the web studio, which routes
    /// it to Settings — the maintainer's call for the native one,
    /// 2026-09-05); its form lives in Settings ▸ General.
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

    /// The editor a navigation command acts on: the manuscript's focused
    /// section in Continuous view, else Code view's active document (which
    /// is also what Single File shows).
    fn focused_site(&self, window: &Window, cx: &gpui::App) -> Option<navigation::EditorSite> {
        let view = self.workspace.read(cx).editor_root().read(cx).view();
        if view == EditorView::Continuous {
            return self.manuscript.read(cx).focused_section(window, cx);
        }
        self.code
            .read(cx)
            .active_document()
            .map(|doc| doc.read(cx).site())
    }

    /// Show `span` of `path` the way the current view shows things: a tab
    /// in Code/Single File, a scroll in the manuscript.
    fn show(
        &mut self,
        path: &str,
        span: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.workspace.read(cx).editor_root().read(cx).view();
        if view == EditorView::Continuous {
            self.manuscript
                .update(cx, |manuscript, cx| manuscript.reveal_span(path, span, cx));
        } else {
            self.open(path, Some(span), window, cx);
        }
    }

    fn go_to_definition(
        &mut self,
        _: &GoToDefinition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        let found = navigation::definition(&site, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(loc) = found.await else {
                let _ = cx.update(|window, cx| {
                    window.push_notification(
                        gpui_component::notification::Notification::info(
                            "No definition for the symbol under the caret.",
                        ),
                        cx,
                    );
                });
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.show(&loc.path, loc.start as usize..loc.end as usize, window, cx);
            });
        })
        .detach();
    }

    fn find_references(&mut self, _: &FindReferences, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        let found = navigation::find_references(&site, cx);
        let search = self.search.clone();
        let workspace = self.workspace.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Some((name, refs)) = found.await else {
                let _ = cx.update(|window, cx| {
                    window.push_notification(
                        gpui_component::notification::Notification::info(
                            "No references for the symbol under the caret.",
                        ),
                        cx,
                    );
                });
                return;
            };
            let _ = cx.update(|window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.open_tool_window("search", window, cx);
                });
                search.update(cx, |search, cx| search.show_references(name, &refs, cx));
            });
        })
        .detach();
    }

    fn rename_symbol(&mut self, _: &RenameSymbol, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        let prepared = navigation::prepare_rename(&site, cx);
        cx.spawn_in(window, async move |_, cx| {
            let prepared = prepared.await;
            let Some((range, current)) = prepared else {
                let _ = cx.update(|window, cx| {
                    window.push_notification(
                        gpui_component::notification::Notification::info(
                            "Nothing renameable under the caret.",
                        ),
                        cx,
                    );
                });
                return;
            };
            let _ = cx.update(|window, cx| {
                rename::prompt(site, range.start, current, window, cx);
            });
        })
        .detach();
    }

    fn fix_all_in_file(&mut self, _: &FixAllInFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        fixes::fix_all(
            &self.project,
            brink_gpui_model::fixes::FixScope::File(site.path.to_string()),
            window,
            cx,
        );
    }

    fn fix_all_in_project(
        &mut self,
        _: &FixAllInProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        fixes::fix_all(
            &self.project,
            brink_gpui_model::fixes::FixScope::Project,
            window,
            cx,
        );
    }

    fn format_document(&mut self, _: &FormatDocument, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        let project = self.project.clone();
        let format = Self::format_files(&project, vec![site.path.to_string()], cx);
        cx.spawn_in(window, async move |_, cx| {
            let formatted = format.await;
            let _ = cx.update(|window, cx| {
                if formatted == 0 {
                    window.push_notification(
                        gpui_component::notification::Notification::info("Already formatted."),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// Format each of `paths` in turn and write the result into the
    /// project; resolves to how many files changed. Formatting is a worker
    /// query, so this is sequential and asynchronous — a save that formats
    /// first waits on it.
    fn format_files(project: &Entity<Project>, paths: Vec<String>, cx: &mut App) -> Task<usize> {
        let queries: Vec<(String, Task<anyhow::Result<QueryResult>>)> = paths
            .into_iter()
            .map(|path| {
                let query = project
                    .read(cx)
                    .query(QueryKind::Format { path: path.clone() }, cx);
                (path, query)
            })
            .collect();
        let project = project.clone();
        cx.spawn(async move |cx| {
            let mut changed = 0;
            for (path, query) in queries {
                if let Ok(QueryResult::Formatted(Some(text))) = query.await {
                    project.update(cx, |project, cx| {
                        if project.edit(&path, text, None, cx) {
                            changed += 1;
                        }
                    });
                }
            }
            changed
        })
    }

    /// Save every dirty file — whichever editor it was changed in. With
    /// "Format on save" on, every dirty file is formatted first, so what is
    /// written is what the editors then show.
    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        let format_first = brink_gpui_shell::settings::AppSettings::get(cx).format_on_save;
        let project = self.project.clone();
        if !format_first {
            write_all(&project, cx);
            return;
        }
        let dirty = project.read(cx).dirty_paths();
        let format = Self::format_files(&project, dirty, cx);
        cx.spawn_in(window, async move |_, cx| {
            let _ = format.await;
            let _ = cx.update(|_, cx| write_all(&project, cx));
        })
        .detach();
    }

    /// Run the story in the Player — from the entry, or from `at`. The
    /// Player is a Code-view tab, so the manuscript gives way to Code; how
    /// the manuscript itself should host a session is parked
    /// (`HANDOFF.md`, "Open, parked").
    fn play_at(&mut self, at: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.workspace.read(cx).editor_root().clone();
        if root.read(cx).view() == EditorView::Continuous {
            root.update(cx, |root, cx| root.set_view(EditorView::Code, cx));
        }
        let player = self.player.clone();
        self.code
            .update(cx, |code, cx| code.show_player(&player, window, cx));
        player.update(cx, |player, cx| player.start(at, cx));
        // Play is an explicit "run it now", and the choices are numbered so
        // they can be picked by key — which needs the Player to have focus.
        // Without this the numbers were dead until you clicked the panel,
        // which is the friction the numbering exists to remove.
        let handle = player.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// Show the `.inkt` dump: dock the tab if it is not docked, then select
    /// it. Like the Player, it is a Code-view tab, so the manuscript gives
    /// way to Code first.
    fn open_compiled_output(
        &mut self,
        _: &OpenCompiledOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_editor_view(EditorView::Code, window, cx);
        });
        let compiled = self.compiled.clone();
        self.code
            .update(cx, |code, cx| code.show_compiled(&compiled, window, cx));
    }

    /// Open quick-open, or close it if it is already up — the palette's
    /// own toggle behaviour, so the key that opened it also dismisses it.
    fn quick_open(&mut self, _: &QuickOpenGoTo, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_open.take().is_some() {
            cx.notify();
            return;
        }
        let project = self.project.clone();
        let picker = cx.new(|cx| QuickOpen::new(project, window, cx));
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this, _, event: &QuickOpenEvent, window, cx| {
                match event {
                    QuickOpenEvent::Open { path, span } => {
                        this.show(path, span.clone().unwrap_or(0..0), window, cx);
                    }
                    QuickOpenEvent::Dismiss => {}
                }
                this.quick_open = None;
                cx.notify();
            },
        );
        picker.update(cx, |picker, cx| picker.focus(window, cx));
        self.quick_open = Some((picker, subscription));
        cx.notify();
    }

    /// Follow the active document's caret. Dropped and remade rather than
    /// kept per document: only one document is active, and an observation
    /// of a closed one would keep it alive.
    fn watch_caret(&mut self, cx: &mut Context<Self>) {
        self.caret = None;
        let Some(document) = self.code.read(cx).active_document().cloned() else {
            return;
        };
        let editor = document.read(cx).editor().clone();
        self.caret = Some(cx.observe(&editor, |this, _, cx| this.refresh_status(cx)));
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        // `on_app_quit` does the saving; this is the door to it.
        cx.quit();
    }

    fn play(&mut self, _: &Play, window: &mut Window, cx: &mut Context<Self>) {
        self.play_at(None, window, cx);
    }

    fn play_restart(&mut self, _: &PlayRestart, window: &mut Window, cx: &mut Context<Self>) {
        let player = self.player.clone();
        if !player.read(cx).is_docked() {
            self.code
                .update(cx, |code, cx| code.show_player(&player, window, cx));
        }
        player.update(cx, |player, cx| player.restart(cx));
        let handle = player.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
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
        // The right-hand group (§7.3): where the caret is, and in what.
        let mut cells = cells;
        if let Some(document) = self.code.read(cx).active_document() {
            let document = document.read(cx);
            let (line, column) = document.cursor_line_column(cx);
            let name = document
                .path()
                .rsplit('/')
                .next()
                .unwrap_or(document.path())
                .to_owned();
            cells.push(StatusCell::new(name).align_end());
            cells.push(StatusCell::new(format!("Ln {line}, Col {column}")).align_end());
        }
        self.workspace
            .update(cx, |workspace, cx| workspace.set_status(cells, cx));
    }
}

fn write_all(project: &Entity<Project>, cx: &mut App) {
    project.update(cx, |project, cx| {
        for (path, err) in project.save_all(cx) {
            eprintln!("failed to save {path}: {err:#}");
        }
    });
}

impl Render for Studio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // gpui-component's `Root` draws the view, tooltips and native menus
        // — and NOT its dialog and notification layers. Those are free
        // functions the application root composes in; without them every
        // `open_dialog` and `push_notification` lands in a list nothing
        // renders (which is how a rename prompt and three toasts went
        // missing on 2026-09-05).
        let notifications = Root::render_notification_layer(window, cx);
        let dialogs = Root::render_dialog_layer(window, cx);
        gpui::div()
            .size_full()
            .relative()
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::search_focus))
            .on_action(cx.listener(Self::go_to_definition))
            .on_action(cx.listener(Self::find_references))
            .on_action(cx.listener(Self::rename_symbol))
            .on_action(cx.listener(Self::format_document))
            .on_action(cx.listener(Self::fix_all_in_file))
            .on_action(cx.listener(Self::fix_all_in_project))
            .on_action(cx.listener(Self::play))
            .on_action(cx.listener(Self::play_restart))
            .on_action(cx.listener(Self::open_compiled_output))
            .on_action(cx.listener(Self::quick_open))
            .on_action(cx.listener(Self::quit))
            .child(self.workspace.clone())
            // After the workspace: later children paint on top, and a
            // dialog under the window it belongs to is no dialog at all.
            .children(self.quick_open.as_ref().map(|(p, _)| p.clone()))
            .children(notifications)
            .children(dialogs)
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
    // The kit's icons (`IconName`) are assets the application has to
    // register; a `Button::icon(IconName::ChevronDown)` with no asset
    // source silently draws nothing.
    Application::with_platform(gpui_platform::current_platform(false))
        .with_assets(gpui_kit_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            // The persisted settings and their theme, before the first paint.
            brink_gpui_shell::settings::init(cx);
            brink_gpui_shell::theme::init(cx);
            let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..TitleBar::window_options()
            };
            let root = root.clone();
            let opened = cx.open_window(options, move |window, cx| {
                // The app font size scales the window's rem.
                let rem = brink_gpui_shell::settings::AppSettings::get(cx).rem_size();
                window.set_rem_size(px(rem));
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
