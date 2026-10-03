//! The GPUI-native brink studio — `docs/gpui-studio-spec.md`.
//!
//! Tier 3: the features, and the wiring. This file is the one place that
//! knows a Binder is a thing that goes in the left rail and that the two
//! modes are the manuscript (Write) and the tabbed editor (Script) — the
//! shell does not, and must not.

mod binder;
mod closing;
mod code_view;
mod compiled_output;
mod continuous;
mod document;
mod files;
mod fixes;
mod graph_layout;
#[cfg(test)]
mod harness;
mod inkt_highlight;
mod knots;
mod landing;
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
mod state_view;
mod story_graph;
mod structural;
mod tab_title;
mod todos;
mod treemap;
mod watch;

use std::ops::Range;
use std::path::PathBuf;

use brink_gpui_model::play::PlayCommand;
use brink_gpui_model::query::{ConvertTarget, QueryKind, QueryResult};
use brink_gpui_shell::editor_view::EditorView;
use brink_gpui_shell::icons;
use brink_gpui_shell::menus::MenuSpec;
use brink_gpui_shell::menus::Quit;
use brink_gpui_shell::region::RailSlot;
use brink_gpui_shell::settings_modal::{Scope, Section, SectionMeta};
use brink_gpui_shell::tool_window::ToolWindowSpec;
use brink_gpui_shell::workspace::{StatusCell, Workspace};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Application, Bounds, Context, Entity, Focusable as _,
    Global, IntoElement, PromptLevel, Render, Subscription, Task, WeakEntity, Window, WindowBounds,
    WindowOptions, actions, prelude::*, px, size,
};
use gpui_component::input::RopeExt as _;
use gpui_component::{Root, TitleBar};

use crate::binder::{Binder, BinderEvent};
use crate::code_view::CodeView;
use crate::code_view::CodeViewEvent;
use crate::compiled_output::{CompiledOutputEvent, CompiledOutputView};
use crate::continuous::ContinuousView;
use crate::output_log::OutputLog;
use crate::player::{Player, PlayerEvent};
use crate::problems::{OpenProblem, Problems, ProblemsMenu};
use crate::program::{ProgramEvent, ProgramExplorer};
use crate::project::{Project, ProjectEvent};
use crate::quick_open::{QuickOpen, QuickOpenEvent};
use crate::search::{SearchEvent, SearchView};
use crate::settings_conventions::ConventionsSection;
use crate::settings_diagnostics::DiagnosticsSection;
use crate::settings_formatting::FormattingSection;
use crate::settings_general::{GeneralSection, OpenConfig};
use crate::settings_prose::ProseSection;
use crate::state_view::StateView;
use crate::todos::{OpenTodo, Todos};
use brink_gpui_shell::commands::CloseWindow;
use brink_gpui_shell::notify::{Severity, notify};

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
        /// Mark or unmark the caret's line as a breakpoint.
        ToggleBreakpoint,
        /// Forget every breakpoint in the project.
        ClearBreakpoints,
        /// Run on to the next breakpoint, choice point, or the end.
        DebugContinue,
        /// Advance one source line.
        DebugStepLine,
        /// Advance one VM instruction — the other granularity, not a
        /// finer setting of the same one (RULED 2026-08-28).
        DebugStepInstruction,
        /// Lift the selection into a new knot, leaving a tunnel call.
        ExtractToKnot,
        /// The same, as a function, leaving a call to it.
        ExtractToFunction,
        /// Turn the caret's line into plain narrative, a choice, a sticky
        /// choice, a gather, or a choice body. The five structural
        /// element types a weave line can be.
        MakeNarrative,
        MakeChoice,
        MakeStickyChoice,
        MakeGather,
        MakeChoiceBody,
        /// The compiled story's `.inkt` dump, as a read-only tab.
        OpenCompiledOutput,
        /// The story graph — knots and diverts as a picture.
        OpenStoryGraph,
        /// Go to a file, knot or stitch by name.
        QuickOpenGoTo,
        /// Choose a story file or a `brink.toml` and open its project in a
        /// new window (the two doors: `landing::anchor_for`).
        OpenProject,
        /// Choose a folder, scaffold `main.ink` + `brink.toml`, open it.
        NewProject,
        /// Give the editor the whole window, and give it back.
        MaximizeEditor,
        /// Take back the last file operation — a create, a rename, a
        /// delete. Not the editor's undo, which is per-document text.
        UndoFileOp,
        /// Leave a tool window and put the keyboard back in the editor.
        /// Bound to `escape` INSIDE a tool window only — every overlay
        /// means something by that key too, and each has its own context.
        FocusEditor,
        /// Close the tab you are in: the focused centre tab, else the
        /// active document. Asks first when the file has unsaved edits.
        CloseTab,
    ]
);

/// Reopen a project from the recents. Data-carrying, so each recent is
/// its own command — a palette entry, and an item in the menu bar's
/// "Open Recent" submenu — `no_json` because the path is the whole payload
/// and nothing outside the app builds one.
#[derive(Clone, PartialEq, Eq, gpui::Action)]
#[action(namespace = brink, no_json)]
struct OpenRecentProject {
    path: String,
}

/// The application root: it owns the model and the features, and hands the
/// shell its panels and views.
struct Studio {
    project: Entity<Project>,
    workspace: Entity<Workspace>,
    /// Script mode's tabbed editor — and with it the open documents.
    /// Opening a file always lands here, whichever mode is showing: in
    /// Write mode the manuscript reveals the file in place.
    code: Entity<CodeView>,
    /// Write mode's manuscript — the whole project as one scroller.
    manuscript: Entity<ContinuousView>,
    search: Entity<SearchView>,
    /// The Player, a centre tab in Code view. Made once; docked on the
    /// first Play, re-docked if its tab was closed.
    player: Entity<Player>,
    /// The Story Graph — a centre tab on the Player's terms, made once.
    graph: Entity<crate::story_graph::StoryGraphView>,
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
    /// The filesystem watch, held for the window's lifetime: dropping the
    /// task stops the pump and the watcher with it.
    _watching: gpui::Task<()>,
    /// Where this window is in closing (see [`closing`]).
    close: CloseState,
    _subscriptions: Vec<Subscription>,
}

impl Studio {
    /// `entry` is an explicit entry from the story-file door; `None` lets
    /// the project's config name it.
    fn new(
        root: PathBuf,
        entry: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
        let graph = cx.new(|cx| crate::story_graph::StoryGraphView::new(project.clone(), cx));
        let output = cx.new(|cx| OutputLog::new(project.clone(), cx));
        let state = cx.new(|cx| StateView::new(project.clone(), player.clone(), cx));
        // The log keeps what the transcript throws away on a Restart.
        output.update(cx, |log, cx| log.watch_player(&player, cx));
        // And the Program Explorer says when the running story is on an
        // older program than the one it is showing.
        program.update(cx, |explorer, cx| explorer.watch_player(&player, cx));
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
                    .icon(icons::BrinkIcon::TreeFolder)
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
                    .icon(icons::BrinkIcon::Find)
                    .size(px(320.)),
                search.clone(),
                window,
                cx,
            );
            workspace.add_tool_window(
                // Lower-left: with no bottom rail, this is what addresses
                // the bottom dock (`docs/gpui-studio-spec.md` §4.1).
                ToolWindowSpec::new("problems", "Problems", RailSlot::LEFT_LOWER)
                    .icon(icons::BrinkIcon::WarningMark)
                    .size(px(160.))
                    .open(),
                problems.clone(),
                window,
                cx,
            );
            // Beside Problems in the lower-left dock: the second tab there.
            workspace.add_tool_window(
                ToolWindowSpec::new("todos", "TODOs", RailSlot::LEFT_LOWER)
                    .icon(icons::BrinkIcon::Todo)
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
                    .icon(icons::BrinkIcon::Doc)
                    .size(px(160.)),
                output.clone(),
                window,
                cx,
            );
            // The right dock's first occupant: the compiled program, a tall
            // tree that wants the side rather than the bottom.
            workspace.add_tool_window(
                ToolWindowSpec::new("program", "Program", RailSlot::RIGHT_UPPER)
                    .icon(icons::BrinkIcon::Doc)
                    .size(px(380.)),
                program.clone(),
                window,
                cx,
            );
            // Under the Program Explorer in the right dock: the debugger
            // pane, which reads the same session the Player runs.
            workspace.add_tool_window(
                ToolWindowSpec::new("state", "State", RailSlot::RIGHT_UPPER)
                    .icon(icons::BrinkIcon::Knot)
                    .size(px(380.)),
                state.clone(),
                window,
                cx,
            );
            // The two modes (decision log 2026-10-03). Registered before
            // the project opens so the manuscript is subscribed when the
            // files land.
            let code_focus = code.read(cx).focus_handle(cx);
            let manuscript_focus = manuscript.read(cx).focus_handle(cx);
            workspace.set_view_occupant(EditorView::Script, code.clone().into(), code_focus, cx);
            workspace.set_view_occupant(
                EditorView::Write,
                manuscript.clone().into(),
                manuscript_focus,
                cx,
            );
            // The app's own commands go through the same registry as the
            // shell's, so the palette and the menu list them.
            workspace.register_command("File", "Save", Save, Some("cmd-s"), cx);
            workspace.register_command("File", "Close Tab", CloseTab, Some("cmd-w"), cx);
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
            // Structural line conversion. Under "Line" rather than
            // "Refactor": these change what a line IS, and an author
            // reaches for them while writing, not while tidying.
            workspace.register_command("Line", "Make Narrative", MakeNarrative, None, cx);
            workspace.register_command("Line", "Make Choice", MakeChoice, Some("alt-1"), cx);
            workspace.register_command(
                "Line",
                "Make Sticky Choice",
                MakeStickyChoice,
                Some("alt-2"),
                cx,
            );
            workspace.register_command("Line", "Make Gather", MakeGather, Some("alt-3"), cx);
            workspace.register_command("Line", "Make Choice Body", MakeChoiceBody, None, cx);
            workspace.register_command(
                "Refactor",
                "Extract to Knot\u{2026}",
                ExtractToKnot,
                None,
                cx,
            );
            workspace.register_command(
                "Refactor",
                "Extract to Function\u{2026}",
                ExtractToFunction,
                None,
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
            // Write mode's title bar: the story by its folder's name, and
            // a Play button for the same action `cmd-r` runs.
            // (The title is set once the project has opened and has a root.)
            workspace.set_play_action(Box::new(Play), cx);
            workspace.register_command(
                "Debug",
                "Toggle Breakpoint",
                ToggleBreakpoint,
                Some("f9"),
                cx,
            );
            workspace.register_command(
                "Debug",
                "Clear All Breakpoints",
                ClearBreakpoints,
                None,
                cx,
            );
            workspace.register_command("Debug", "Continue", DebugContinue, Some("f5"), cx);
            workspace.register_command("Debug", "Step", DebugStepLine, Some("f10"), cx);
            workspace.register_command(
                "Debug",
                "Step Instruction",
                DebugStepInstruction,
                Some("f11"),
                cx,
            );
            workspace.register_command("Program", "Compiled Output", OpenCompiledOutput, None, cx);
            workspace.register_command("Program", "Story Graph", OpenStoryGraph, None, cx);
            workspace.register_command(
                "Go",
                "Go to File\u{2026}",
                QuickOpenGoTo,
                Some("cmd-p"),
                cx,
            );
            workspace.register_command(
                "View",
                "Maximize Editor",
                MaximizeEditor,
                Some("cmd-shift-e"),
                cx,
            );
            workspace.register_command("File", "New Project\u{2026}", NewProject, None, cx);
            workspace.register_command(
                "File",
                "Open Project\u{2026}",
                OpenProject,
                Some("cmd-shift-o"),
                cx,
            );
            // One entry per remembered project, listed newest first. The
            // project THIS window opened is not among them: it is
            // remembered after this runs, so a window never offers to
            // reopen itself.
            //
            // Their own group, so the menu bar can make them a submenu; the
            // palette reads them as "Open Recent: harbour (stories)" just as
            // it did when the prefix was in the title.
            for path in brink_gpui_shell::settings::AppSettings::get(cx).recents {
                let title = recent_label(&path);
                let action = OpenRecentProject { path };
                workspace.register_command("Open Recent", title, action, None, cx);
            }
            workspace.register_command_in(
                "Go",
                "Back to the Editor",
                FocusEditor,
                Some("escape"),
                Some(brink_gpui_shell::tool_window::TOOL_WINDOW_CONTEXT),
                cx,
            );
            workspace.register_command("File", "Undo File Operation", UndoFileOp, None, cx);
            workspace.register_command(
                "File",
                "Close Project",
                CloseWindow,
                Some("cmd-shift-w"),
                cx,
            );
            workspace.register_command("File", "Quit", Quit, Some("cmd-q"), cx);
            // The menu bar (`brink_gpui_shell::menus`): which of the groups
            // above go in which menu. Groups, never commands — a command
            // registered into a group is in the bar with no edit here, and
            // a group left out still gets a menu of its own. The shell adds
            // the App, Window and Help menus around these.
            workspace.set_menu_layout(
                vec![
                    MenuSpec::new("File").group("File").submenu("Open Recent"),
                    // Line conversion is writing, not tidying (see its
                    // registration), so it stays with the text.
                    MenuSpec::new("Edit")
                        .text_editing()
                        .group("Find")
                        .group("Search")
                        .submenu("Line"),
                    MenuSpec::new("View").group("View").submenu("Theme"),
                    MenuSpec::new("Go").group("Go"),
                    MenuSpec::new("Refactor").group("Refactor").group("Fix"),
                    MenuSpec::new("Story")
                        .group("Play")
                        .group("Debug")
                        .group("Program"),
                ],
                cx,
            );
            // After every tool window is registered: their `open()`
            // defaults decide the first run, and a saved shape overrides
            // them (`Workspace::apply_layout`).
            let saved = brink_gpui_shell::settings::AppSettings::get(cx).layout;
            workspace.apply_layout(&saved, window, cx);
        });

        // The remembered scrolls and open tabs, but only if they belong
        // to THIS project:
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
        //
        // The hook reads the studio it is handed rather than capturing its
        // parts: a detached quit hook lives as long as the app, so a captured
        // `Entity` would keep a closed window's project, editors and worker
        // thread alive until quit (found by the headless harness's leak
        // check).
        cx.on_app_quit(|this: &mut Studio, cx: &mut Context<Studio>| {
            let documents = document_state(&this.project, &this.code, cx);
            Workspace::save_layout(&this.workspace, Some(documents), cx);
            async move {}
        })
        .detach();

        let on_project = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| match event {
                // The open timing, the load warnings and an open failure
                // all land in the Output log, which subscribes to the same
                // events — this handler is only the window's own reaction.
                ProjectEvent::Opened { .. } => {
                    this.open_initial(window, cx);
                    this.refresh_status(cx);
                    let title = this.project_name(cx);
                    this.workspace
                        .update(cx, |workspace, cx| workspace.set_story_title(title, cx));
                }
                ProjectEvent::Analyzed => this.refresh_status(cx),
                // The file set moving changes the status bar's file count.
                ProjectEvent::FilesChanged => this.refresh_status(cx),
                // A change nobody in the studio made: said out loud, and
                // kept in the Output log, which subscribes to the same
                // event. A conflict is a warning because it is the one
                // case where the author has work the disk disagrees with.
                ProjectEvent::DiskChanged(reports) => {
                    for report in reports {
                        let (severity, text) = match report {
                            crate::project::DiskReport::Reloaded(path) => (
                                Severity::Info,
                                format!("{path} changed on disk \u{2014} reloaded."),
                            ),
                            crate::project::DiskReport::Conflicted(path) => (
                                Severity::Warning,
                                format!(
                                    "{path} changed on disk while you had unsaved edits. \
                                     Your text was kept; saving will overwrite the disk."
                                ),
                            ),
                            crate::project::DiskReport::Vanished { path, dirty } => (
                                if *dirty {
                                    Severity::Warning
                                } else {
                                    Severity::Info
                                },
                                if *dirty {
                                    format!(
                                        "{path} was deleted on disk, and you had unsaved edits."
                                    )
                                } else {
                                    format!("{path} was deleted on disk.")
                                },
                            ),
                            crate::project::DiskReport::Appeared(path) => {
                                (Severity::Info, format!("{path} appeared on disk."))
                            }
                        };
                        notify(severity, "project", text, window, cx);
                    }
                }
                ProjectEvent::OpenFailed(_)
                | ProjectEvent::SourceChanged { .. }
                | ProjectEvent::BreakpointsChanged
                | ProjectEvent::ProseChanged
                | ProjectEvent::Saved
                | ProjectEvent::SaveFailed { .. } => {}
            },
        );
        let on_binder = cx.subscribe_in(
            &binder,
            window,
            |this, binder, event: &BinderEvent, window, cx| {
                let BinderEvent::Open { path, offset } = event else {
                    match event {
                        BinderEvent::Play { path } => {
                            this.play_at(Some(path.clone()), window, cx);
                        }
                        // The file operations live in the studio, not the
                        // panel: they open dialogs and they change the
                        // project, and the Binder's business is the rows.
                        BinderEvent::NewFile { folder } => {
                            files::new_file(this.project.clone(), folder.clone(), window, cx);
                        }
                        BinderEvent::RenameFile { path } => {
                            files::rename_file(this.project.clone(), path.clone(), window, cx);
                        }
                        BinderEvent::DeleteFile { paths } => {
                            // Before the dialog: their editors would write
                            // the files straight back on the next `cmd-s`.
                            this.code.update(cx, |code, cx| {
                                for path in paths {
                                    code.close_document(path, window, cx);
                                }
                            });
                            files::delete_files(this.project.clone(), paths.clone(), window, cx);
                        }
                        BinderEvent::NewKnot { path } => {
                            let reveal = this.reveal_fn(cx);
                            knots::new_knot(this.project.clone(), path.clone(), reveal, window, cx);
                        }
                        BinderEvent::NewStitch { path, full_end } => {
                            let reveal = this.reveal_fn(cx);
                            knots::new_stitch(
                                this.project.clone(),
                                path.clone(),
                                *full_end,
                                reveal,
                                window,
                                cx,
                            );
                        }
                        BinderEvent::Promote { path, knot, stitch } => {
                            structural::promote(
                                this.project.clone(),
                                path.clone(),
                                knot.clone(),
                                stitch.clone(),
                                window,
                                cx,
                            );
                        }
                        BinderEvent::Demote { path, knot } => {
                            structural::demote(
                                this.project.clone(),
                                path.clone(),
                                knot.clone(),
                                window,
                                cx,
                            );
                        }
                        BinderEvent::Open { .. } => {}
                    }
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
                match event {
                    PlayerEvent::Navigate { path, span } => {
                        this.show(path, span.clone(), window, cx);
                    }
                    // Following never opens or selects a tab: the Player
                    // is a centre tab beside the documents, so doing
                    // either would hide the Player behind the source it
                    // is following. In the manuscript it scrolls, which
                    // is where following shows best; in Code view it
                    // moves the caret of an already-open document, which
                    // lands in view when the group is split.
                    PlayerEvent::Follow { path, span } => {
                        this.follow(path, span.clone(), window, cx);
                    }
                    // A debug stop: put the author's eye on the line the
                    // story is halted on. Follow rather than navigate —
                    // the keyboard stays where it was, so F10 keeps
                    // stepping instead of typing into the source.
                    PlayerEvent::Stopped { path, line } => {
                        if let Some(span) = this.line_span(path, *line, cx) {
                            this.follow(path, span, window, cx);
                        }
                    }
                    // `Log` is the Output window's business.
                    PlayerEvent::Log { .. } => {}
                }
            },
        );
        // The status bar carries the story state, and the Player changes it
        // without an event of its own — so observe the entity.
        let on_player_state = cx.observe(&player, |this: &mut Self, _, cx| {
            this.refresh_status(cx);
        });
        let on_compiled = cx.subscribe_in(
            &compiled,
            window,
            |this, _, event: &CompiledOutputEvent, window, cx| match event {
                CompiledOutputEvent::Navigate { path, span } => {
                    this.show(path, span.clone(), window, cx);
                }
                CompiledOutputEvent::NoSource => {
                    notify(
                        Severity::Info,
                        "studio",
                        "That row carries no source location.",
                        window,
                        cx,
                    );
                }
            },
        );
        // A node click opens its declaration, on the same road every
        // other panel's navigation takes.
        let on_graph = cx.subscribe_in(
            &graph,
            window,
            |this, _, event: &crate::story_graph::StoryGraphEvent, window, cx| {
                let crate::story_graph::StoryGraphEvent::Navigate { path, span } = event;
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
        let on_problem_menu = cx.subscribe_in(
            &problems,
            window,
            |this, _, event: &ProblemsMenu, window, cx| match event {
                ProblemsMenu::Suppress { path, line, code } => {
                    this.suppress(path, *line, code, window, cx);
                }
                ProblemsMenu::Configure => {
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.open_settings(Some("diagnostics"), window, cx);
                    });
                }
            },
        );
        // An Output row that names a file opens it — a failed save, a
        // config warning. A row that names no place is not clickable.
        let on_log_row = cx.subscribe_in(
            &output,
            window,
            |this, _, event: &crate::output_log::OpenLogRow, window, cx| {
                this.open(&event.path, None, window, cx);
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
                    // The tabs moved: opened, closed, or a different one
                    // showing. Written now rather than only on quit, so a
                    // kill loses nothing — `settings::update` compares
                    // before writing, so this is cheap to say often.
                    this.save_documents(cx);
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

        // Watching starts with the project: a change made outside the
        // studio between opening a file and saving it was invisible, and
        // the next save simply overwrote it.
        let watching = watch::start(project.clone(), root.clone(), cx);
        project.update(cx, |project, _| project.open(root, entry));

        // Keys have somewhere to land from the first frame.
        let workspace_focus = workspace.read(cx).focus_handle(cx);
        window.focus(&workspace_focus, cx);

        // The platform's close (the red button, a Windows caption) asks
        // here first; `false` keeps the window while the prompt is up.
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            this.update(cx, |studio, cx| studio.should_close(window, cx))
                .unwrap_or(true)
        });
        // Quit asks every project window, so it needs to find them all.
        let me = (window.window_handle(), cx.weak_entity());
        cx.default_global::<OpenStudios>().0.push(me);

        Self {
            project,
            workspace,
            code,
            manuscript,
            search,
            player,
            compiled,
            graph,
            quick_open: None,
            caret: None,
            _watching: watching,
            close: CloseState::default(),
            _subscriptions: vec![
                on_project,
                on_binder,
                on_player,
                on_player_state,
                on_program,
                on_compiled,
                on_graph,
                on_problem,
                on_problem_menu,
                on_todo,
                on_log_row,
                on_search,
                on_code,
                on_general,
            ],
        }
    }

    /// Open the project's entry, or its first file when it names none.
    fn open_initial(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The tabs that were open last time, if they were this project's
        // and they still exist. A file that has since been deleted or
        // renamed is SKIPPED, not opened empty: the studio would be
        // showing a document for something that is not there.
        let restored = self.restore_tabs(window, cx);
        if restored {
            return;
        }
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

    /// Write the open tabs and their scrolls into the settings.
    fn save_documents(&self, cx: &mut App) {
        let documents = document_state(&self.project, &self.code, cx);
        Workspace::save_layout(&self.workspace, Some(documents), cx);
    }

    /// Reopen the remembered tabs. Returns whether any opened — `false`
    /// falls back to the entry, which is also what a first run gets.
    fn restore_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let saved = brink_gpui_shell::settings::AppSettings::get(cx).layout;
        let root = self.project.read(cx).root().display().to_string();
        if saved.scroll_root.as_deref() != Some(root.as_str()) {
            return false;
        }
        let known: Vec<String> = {
            let project = self.project.read(cx);
            saved
                .open_files
                .iter()
                .filter(|path| project.loaded_source(path).is_some())
                .cloned()
                .collect()
        };
        if known.is_empty() {
            return false;
        }
        for path in &known {
            self.open(path, None, window, cx);
        }
        // Last, so it ends up showing: opening a tab selects it.
        if let Some(active) = saved.active_file.filter(|a| known.contains(a)) {
            self.open(&active, None, window, cx);
        }
        true
    }

    /// Open a file in Code view, or select it if it is already open, and
    /// optionally reveal a span inside it. `brink.toml` included: it is a
    /// document like any other here (unlike the web studio, which routes
    /// it to Settings — the maintainer's call for the native one,
    /// 2026-09-05); its form lives in Settings ▸ General.
    /// `Studio::open`, packaged for a module that writes text and then
    /// wants the author looking at it — `knots`, today. The studio owns
    /// how a document is opened; the writer owns what was written.
    fn reveal_fn(&self, cx: &mut Context<Self>) -> knots::Reveal {
        let studio = cx.entity();
        std::rc::Rc::new(
            move |path: &str, at: usize, window: &mut Window, cx: &mut App| {
                let path = path.to_owned();
                studio.update(cx, |this, cx| {
                    this.open(&path, Some(at..at), window, cx);
                });
            },
        )
    }

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
    /// section in Write mode, else Script mode's active document.
    fn focused_site(&self, window: &Window, cx: &gpui::App) -> Option<navigation::EditorSite> {
        let view = self.workspace.read(cx).editor_root().read(cx).view();
        if view == EditorView::Write {
            return self.manuscript.read(cx).focused_section(window, cx);
        }
        self.code
            .read(cx)
            .active_document()
            .map(|doc| doc.read(cx).site())
    }

    /// Show `span` of `path` the way the current mode shows things: a tab
    /// in Script, a scroll in the manuscript.
    fn show(
        &mut self,
        path: &str,
        span: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.workspace.read(cx).editor_root().read(cx).view();
        if view == EditorView::Write {
            self.manuscript
                .update(cx, |manuscript, cx| manuscript.reveal_span(path, span, cx));
        } else {
            self.open(path, Some(span), window, cx);
        }
    }

    /// Follow-in-editor's reveal — see `CodeView::reveal_if_open` for why
    /// it is deliberately quieter than `show`.
    fn follow(
        &mut self,
        path: &str,
        span: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.workspace.read(cx).editor_root().read(cx).view();
        if view == EditorView::Write {
            self.manuscript
                .update(cx, |manuscript, cx| manuscript.reveal_span(path, span, cx));
        } else {
            self.code.update(cx, |code, cx| {
                code.reveal_if_open(path, span, window, cx);
            });
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
                    notify(
                        Severity::Info,
                        "studio",
                        "No definition for the symbol under the caret.",
                        window,
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
                    notify(
                        Severity::Info,
                        "studio",
                        "No references for the symbol under the caret.",
                        window,
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
                    notify(
                        Severity::Info,
                        "studio",
                        "Nothing renameable under the caret.",
                        window,
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

    fn extract_to_knot(&mut self, _: &ExtractToKnot, window: &mut Window, cx: &mut Context<Self>) {
        self.extract(false, window, cx);
    }

    fn extract_to_function(
        &mut self,
        _: &ExtractToFunction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.extract(true, window, cx);
    }

    /// Lift the focused editor's SELECTION into a knot or a function. The
    /// op snaps to whole lines itself, so a partial selection is fine; an
    /// empty one is not, and says so rather than extracting nothing.
    fn extract(&mut self, function: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        let span: std::ops::Range<usize> = site.editor.read(cx).selected_range();
        if span.is_empty() {
            notify(
                Severity::Info,
                "refactor",
                "Select the lines to extract first.",
                window,
                cx,
            );
            return;
        }
        structural::extract(
            self.project.clone(),
            site.path.to_string(),
            span,
            function,
            window,
            cx,
        );
    }

    fn make_narrative(&mut self, _: &MakeNarrative, window: &mut Window, cx: &mut Context<Self>) {
        self.convert_line(ConvertTarget::Narrative, window, cx);
    }

    fn make_choice(&mut self, _: &MakeChoice, window: &mut Window, cx: &mut Context<Self>) {
        self.convert_line(ConvertTarget::Choice { sticky: false }, window, cx);
    }

    fn make_sticky_choice(
        &mut self,
        _: &MakeStickyChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.convert_line(ConvertTarget::Choice { sticky: true }, window, cx);
    }

    fn make_gather(&mut self, _: &MakeGather, window: &mut Window, cx: &mut Context<Self>) {
        self.convert_line(ConvertTarget::Gather, window, cx);
    }

    fn make_choice_body(
        &mut self,
        _: &MakeChoiceBody,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.convert_line(ConvertTarget::ChoiceBody, window, cx);
    }

    /// Turn the focused editor's caret line into `target`. The sigil
    /// arithmetic is the worker's (`brink-ide`'s `convert_element`), which
    /// reads the line's real structural context; this only asks, applies
    /// and reports.
    fn convert_line(&mut self, target: ConvertTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.focused_site(window, cx) else {
            return;
        };
        let path = site.path.to_string();
        let offset = {
            let state = site.editor.read(cx);
            let position = state.cursor_position();
            u32::try_from(state.text().position_to_offset(&position)).unwrap_or(0)
        };
        let query = self.project.read(cx).query(
            QueryKind::ConvertLine {
                path: path.clone(),
                offset,
                target,
            },
            cx,
        );
        let project = self.project.clone();
        cx.spawn_in(window, async move |_, cx| {
            let edit = match query.await {
                Ok(QueryResult::LineEdit(Some(edit))) => edit,
                _ => {
                    let _ = cx.update(|window, cx| {
                        notify(
                            Severity::Info,
                            "studio",
                            "This line cannot become that.",
                            window,
                            cx,
                        );
                    });
                    return;
                }
            };
            let _ = cx.update(|_window, cx| {
                project.update(cx, |project, cx| {
                    let Some(source) = project.loaded_source(&path).map(str::to_owned) else {
                        return;
                    };
                    let from = (edit.from as usize).min(source.len());
                    let to = (edit.to as usize).clamp(from, source.len());
                    let next = format!("{}{}{}", &source[..from], edit.insert, &source[to..]);
                    project.edit(&path, next, None, cx);
                });
            });
        })
        .detach();
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
                    notify(Severity::Info, "studio", "Already formatted.", window, cx);
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
        // A failure is already reported: `save_all` emits `SaveFailed`,
        // which the Output log shows. Only closing needs the list back.
        self.save_dirty(window, cx).detach();
    }

    /// What `Save` does, answering which writes failed — the close and
    /// quit prompts' "Save" must not close over a file it could not write.
    fn save_dirty(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Vec<(String, std::io::Error)>> {
        self.save_dirty_in(None, window, cx)
    }

    /// [`Self::save_dirty`], for every dirty file or for `only` that one —
    /// closing a tab saves its file and nothing else.
    fn save_dirty_in(
        &mut self,
        only: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Vec<(String, std::io::Error)>> {
        let settings = brink_gpui_shell::settings::AppSettings::get(cx);
        let project = self.project.clone();
        // Both are per-FILE and scoped to what is dirty: saving must not
        // rewrite a file the author has not touched.
        let dirty = match only {
            Some(path) if project.read(cx).is_dirty(path) => vec![path.to_owned()],
            Some(_) => Vec::new(),
            None => project.read(cx).dirty_paths(),
        };
        let only = only.map(str::to_owned);
        let write = move |project: &Entity<Project>, cx: &mut App| match &only {
            Some(path) => project.update(cx, |project, cx| project.save(path, cx)),
            None => write_all(project, cx),
        };
        if !settings.format_on_save && !settings.fix_on_save {
            return Task::ready(write(&project, cx));
        }
        // Fixes first, then the formatter — so what is laid out is what
        // the fixes wrote, rather than a fix landing on formatted text and
        // leaving it unformatted again.
        let fixes: Vec<gpui::Task<usize>> = if settings.fix_on_save {
            dirty
                .iter()
                .map(|path| {
                    fixes::fix_all_quietly(
                        &project,
                        brink_gpui_model::fixes::FixScope::File(path.clone()),
                        cx,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let format_on_save = settings.format_on_save;
        cx.spawn_in(window, async move |_, cx| {
            for fix in fixes {
                fix.await;
            }
            if format_on_save {
                let format = cx.update(|_, cx| Self::format_files(&project, dirty, cx));
                if let Ok(format) = format {
                    format.await;
                }
            }
            cx.update(|_, cx| write(&project, cx)).unwrap_or_default()
        })
    }

    /// Run the story in the Player — from the entry, or from `at`. The
    /// Player is a Code-view tab, so the manuscript gives way to Code; how
    /// the manuscript itself should host a session is parked
    /// (`HANDOFF.md`, "Open, parked").
    fn play_at(&mut self, at: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.require_editor_view(EditorView::Script, cx);
        });
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
            workspace.require_editor_view(EditorView::Script, cx);
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

    /// Write a suppression directive: on the diagnostic's line, or for the
    /// whole file. Through `Project::edit` like every other change, so the
    /// tab, the manuscript and any Search card over that file all follow
    /// it, and the worker re-analyzes — which is what makes the row leave.
    fn suppress(
        &mut self,
        path: &str,
        line: Option<u32>,
        code: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.project.read(cx).loaded_source(path).map(str::to_owned) else {
            return;
        };
        let next = match line {
            Some(line) => {
                crate::problems::suppress_line_edit(&source, line, code).map(|(at, text)| {
                    let mut out = source.clone();
                    out.insert_str(at, &text);
                    out
                })
            }
            None => crate::problems::suppress_file_source(&source, code),
        };
        let Some(next) = next else {
            // Already covered, or a line that is no longer there: doing
            // nothing is the honest answer, not a duplicate directive.
            return;
        };
        self.project
            .update(cx, |project, cx| project.edit(path, next, None, cx));
        // Show what was written: a silenced diagnostic that leaves without
        // a visible cause reads as the panel losing track.
        self.open(path, None, window, cx);
    }

    /// Ask for a story file or a config, then open its project in a new
    /// window.
    ///
    /// A new window rather than this one: every panel here is built around
    /// one root — the documents, the Binder's tree, the worker's session —
    /// so swapping the root under them would mean tearing all of it down
    /// and building it again, which is what opening a window does anyway.
    fn open_project(&mut self, _: &OpenProject, _: &mut Window, cx: &mut Context<Self>) {
        landing::choose_and_open(cx);
    }

    fn new_project(&mut self, _: &NewProject, _: &mut Window, cx: &mut Context<Self>) {
        landing::new_project(cx);
    }

    fn open_recent(
        &mut self,
        action: &OpenRecentProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A recent outlives the file it names: `open_recent` drops it from
        // the list, and the reason is said here rather than nowhere.
        if let Err(error) = landing::open_recent(&action.path, cx) {
            notify(Severity::Error, "studio", error, window, cx);
        }
    }

    fn maximize_editor(&mut self, _: &MaximizeEditor, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |workspace, cx| workspace.toggle_maximize(window, cx));
    }

    /// Show the story graph — a centre tab, so the manuscript gives way
    /// to Code first, exactly as the Player and the dump do.
    fn open_story_graph(
        &mut self,
        _: &OpenStoryGraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.require_editor_view(EditorView::Script, cx);
        });
        let graph = self.graph.clone();
        self.code
            .update(cx, |code, cx| code.show_graph(&graph, window, cx));
    }

    /// Put the keyboard back where the writing happens. The active
    /// document if there is one, and the editor region itself if there is
    /// not — a tool window that swallowed `escape` and gave focus to
    /// nothing would be worse than not binding it.
    fn focus_editor(&mut self, _: &FocusEditor, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(document) = self.code.read(cx).active_document().cloned() {
            let handle = document.read(cx).editor().read(cx).focus_handle(cx);
            window.focus(&handle, cx);
            return;
        }
        let handle = self
            .workspace
            .read(cx)
            .editor_root()
            .read(cx)
            .focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// Take back the last create, rename or delete. Deliberately without
    /// a key: `cmd-z` is the editor's, and a chord that sometimes undid a
    /// word and sometimes brought a deleted file back would be worse than
    /// a palette entry that says what it does.
    fn undo_file_op(&mut self, _: &UndoFileOp, window: &mut Window, cx: &mut Context<Self>) {
        // The file an undo may reopen or close is the studio's business:
        // a recreated file should not be reopened behind the author's
        // back, and a file being un-created must have its tab closed
        // first or the next `cmd-s` writes it straight back.
        if let Some(crate::project::FileOp::Created { path }) =
            self.project.read(cx).undoable_file_op().cloned()
        {
            self.code
                .update(cx, |code, cx| code.close_document(&path, window, cx));
        }
        let undone = self
            .project
            .update(cx, |project, cx| project.undo_file_op(cx));
        match undone {
            Ok(done) => notify(
                Severity::Success,
                "files",
                format!("Undid {done}."),
                window,
                cx,
            ),
            Err(err) => notify(Severity::Warning, "files", format!("{err}"), window, cx),
        }
    }

    /// `cmd-w`. The tab holding the keyboard first — the Player, Compiled
    /// Output and the Story Graph are tabs too — then the active document.
    /// The manuscript has no tabs,
    /// and closing a file it cannot show would be closing something out of
    /// sight, so there it does nothing.
    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.read(cx).editor_view(cx) == EditorView::Write {
            return;
        }
        let singletons = [
            (
                self.player.entity_id(),
                self.player.read(cx).is_docked(),
                self.player.read(cx).focus_handle(cx),
            ),
            (
                self.compiled.entity_id(),
                self.compiled.read(cx).is_docked(),
                self.compiled.read(cx).focus_handle(cx),
            ),
            (
                self.graph.entity_id(),
                self.graph.read(cx).is_docked(),
                self.graph.read(cx).focus_handle(cx),
            ),
        ];
        let code = self.code.read(cx);
        let target = code
            .focused_document(window, cx)
            .map(gpui::Entity::entity_id)
            .or_else(|| {
                singletons
                    .iter()
                    .find(|(_, docked, focus)| *docked && focus.contains_focused(window, cx))
                    .map(|(id, _, _)| *id)
            })
            .or_else(|| code.active_document().map(gpui::Entity::entity_id));
        if let Some(id) = target {
            self.close_tab_id(id, window, cx);
        }
    }

    fn close_tab_by_id(
        &mut self,
        action: &tab_title::CloseTabById,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_tab_id(action.id, window, cx);
    }

    /// The one way a centre tab closes, whichever affordance asked.
    fn close_tab_id(&mut self, id: gpui::EntityId, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.code.clone();
        if id == self.player.entity_id() {
            let player = self.player.clone();
            code.update(cx, |code, cx| code.close_panel(player, window, cx));
        } else if id == self.compiled.entity_id() {
            let compiled = self.compiled.clone();
            code.update(cx, |code, cx| code.close_panel(compiled, window, cx));
        } else if id == self.graph.entity_id() {
            let graph = self.graph.clone();
            code.update(cx, |code, cx| code.close_panel(graph, window, cx));
        } else if let Some(path) = self.code.read(cx).document_path(id, cx) {
            self.close_document_asking(path, window, cx);
        }
    }

    /// Close a document's tab, asking first when its file has unsaved
    /// edits: the window-close prompt's wording and answers (`closing`),
    /// for the one file. The edits would survive a silent close — the
    /// buffer is the project's, not the tab's — but as unsaved edits in a
    /// file with no tab, which the author has no reason to go looking for.
    ///
    /// Save is `cmd-s`'s save scoped to the file (fix/format on save
    /// included), and closes only once the write has landed. Don't Save
    /// puts the buffer back to the disk's text before closing, since the
    /// manuscript shows the same buffer and would go on showing the edits.
    fn close_document_asking(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let close = |studio: &mut Self, path: &str, window: &mut Window, cx: &mut Context<Self>| {
            studio
                .code
                .update(cx, |code, cx| code.close_document(path, window, cx));
        };
        if !self.project.read(cx).is_dirty(&path) {
            close(self, &path, window, cx);
            return;
        }
        if self.close.asking {
            // gpui cannot hold two prompts on one window.
            return;
        }
        let name = self.project_name(cx);
        let (message, detail) = closing::prompt_text(&name, std::slice::from_ref(&path));
        self.close.asking = true;
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&detail),
            &closing::answers(),
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let choice = answer
                .await
                .map_or(closing::Choice::Cancel, closing::choice);
            let closing = match choice {
                closing::Choice::Cancel => false,
                closing::Choice::Discard => this
                    .update(cx, |studio, cx| {
                        studio
                            .project
                            .update(cx, |project, cx| project.revert(&path, cx));
                    })
                    .is_ok(),
                closing::Choice::Save => {
                    let saving = this.update_in(cx, |studio, window, cx| {
                        studio.save_dirty_in(Some(&path), window, cx)
                    });
                    match saving {
                        Ok(saving) => saving.await.is_empty(),
                        Err(_) => false,
                    }
                }
            };
            let _ = this.update_in(cx, |studio, window, cx| {
                studio.close.asking = false;
                if closing {
                    close(studio, &path, window, cx);
                } else if choice == closing::Choice::Save {
                    studio.report_unsaved(window, cx);
                }
            });
        })
        .detach();
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        // `on_app_quit` saves the layout; the unsaved FILES are asked
        // about first, window by window, because that hook cannot cancel.
        quit_asking(cx);
    }

    fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        if self.should_close(window, cx) {
            self.close.confirmed = true;
            window.remove_window();
        }
    }

    /// May this window close now? Yes when nothing is dirty or the author
    /// has already answered; otherwise the prompt goes up and the answer
    /// closes the window itself, so this says no.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close.confirmed {
            return true;
        }
        if self.close.asking {
            // gpui cannot hold two prompts on one window; the one up
            // already answers this.
            return false;
        }
        let Some(answer) = self.ask_about_unsaved(window, cx) else {
            return true;
        };
        cx.spawn_in(window, async move |this, cx| {
            let choice = answer
                .await
                .map_or(closing::Choice::Cancel, closing::choice);
            let close = match choice {
                closing::Choice::Cancel => false,
                closing::Choice::Discard => true,
                closing::Choice::Save => {
                    let saving =
                        this.update_in(cx, |studio, window, cx| studio.save_dirty(window, cx));
                    match saving {
                        Ok(saving) => saving.await.is_empty(),
                        Err(_) => false,
                    }
                }
            };
            let _ = this.update_in(cx, |studio, window, cx| {
                studio.close.asking = false;
                if close {
                    studio.close.confirmed = true;
                    window.remove_window();
                } else if choice == closing::Choice::Save {
                    studio.report_unsaved(window, cx);
                }
            });
        })
        .detach();
        false
    }

    /// What the unsaved-work prompts and Write mode's title bar call this
    /// project: its folder's name.
    fn project_name(&self, cx: &App) -> String {
        self.project.read(cx).root().file_name().map_or_else(
            || "this project".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// Put the unsaved-work prompt up if anything is dirty, answering the
    /// button index; `None` when there is nothing to ask about.
    fn ask_about_unsaved(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl std::future::Future<Output = Option<usize>> + use<>> {
        let dirty = self.project.read(cx).dirty_paths();
        if dirty.is_empty() {
            return None;
        }
        let name = self.project_name(cx);
        let (message, detail) = closing::prompt_text(&name, &dirty);
        self.close.asking = true;
        window.activate_window();
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&detail),
            &closing::answers(),
            cx,
        );
        Some(async move { answer.await.ok() })
    }

    /// "Save" was answered and a write failed: the window stays, and says
    /// why. The Output log already has the row per file.
    fn report_unsaved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let still = self.project.read(cx).dirty_paths();
        notify(
            Severity::Error,
            "files",
            format!(
                "Could not save {} — nothing was closed. See the Output log.",
                still.join(", ")
            ),
            window,
            cx,
        );
    }

    fn play(&mut self, _: &Play, window: &mut Window, cx: &mut Context<Self>) {
        self.play_at(None, window, cx);
    }

    /// Mark the caret's line, or unmark it. The line is the EDITOR's,
    /// because a breakpoint is set where you are looking; with no
    /// document open there is no line to mark and the command says so
    /// rather than marking line 1 of something.
    fn toggle_breakpoint(
        &mut self,
        _: &ToggleBreakpoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((path, line)) = self.code.read(cx).caret_line(cx) else {
            notify(
                Severity::Info,
                "debug",
                "Open a file and put the caret on a line to mark it.",
                window,
                cx,
            );
            return;
        };
        let on = self
            .project
            .update(cx, |project, cx| project.toggle_breakpoint(&path, line, cx));
        let what = if on { "Breakpoint at" } else { "Cleared" };
        notify(
            Severity::Info,
            "debug",
            format!("{what} {path}:{line}."),
            window,
            cx,
        );
    }

    fn clear_breakpoints(
        &mut self,
        _: &ClearBreakpoints,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project
            .update(cx, |project, cx| project.clear_breakpoints(cx));
    }

    fn debug_continue(&mut self, _: &DebugContinue, window: &mut Window, cx: &mut Context<Self>) {
        self.debug(PlayCommand::Continue, window, cx);
    }

    fn debug_step_line(&mut self, _: &DebugStepLine, window: &mut Window, cx: &mut Context<Self>) {
        self.debug(PlayCommand::StepLine, window, cx);
    }

    fn debug_step_instruction(
        &mut self,
        _: &DebugStepInstruction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.debug(PlayCommand::StepInstruction, window, cx);
    }

    /// Send a debug verb to the running session, showing the Player first
    /// — the transcript is where its output lands.
    fn debug(&mut self, command: PlayCommand, window: &mut Window, cx: &mut Context<Self>) {
        let player = self.player.clone();
        if !player.read(cx).is_docked() {
            self.code
                .update(cx, |code, cx| code.show_player(&player, window, cx));
        }
        player.update(cx, |player, cx| player.debug(command, cx));
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

    /// The byte span of a file's 1-based line, from the mirror's text.
    /// `None` for a file the project does not hold, or a line past its
    /// end — which an edit since the stop can produce.
    fn line_span(&self, path: &str, line: u32, cx: &App) -> Option<Range<usize>> {
        let project = self.project.read(cx);
        let source = project.loaded_source(path)?;
        let mut at = 0usize;
        for (i, text) in source.split_inclusive('\n').enumerate() {
            if u32::try_from(i + 1).ok()? == line {
                let end = at + text.trim_end_matches(['\n', '\r']).len();
                return Some(at..end);
            }
            at += text.len();
        }
        None
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
        // The story state (§7.3's left group) — said once here rather than
        // read off the Player's own header, which is not on screen unless
        // its tab is.
        let mut cells = cells;
        let state = self.player.read(cx).state();
        if state != crate::player::SessionState::Idle {
            cells.push(StatusCell::new(state.label()));
        }
        // The right-hand group (§7.3): where the caret is, and in what.
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

/// What the layout remembers about the documents: which project they
/// belong to, where each is scrolled, which are open and which is
/// showing. Assembled here because the app owns the documents; the shell
/// only stores it.
fn document_state(
    project: &Entity<Project>,
    code: &Entity<CodeView>,
    cx: &App,
) -> brink_gpui_shell::settings::Documents {
    let code = code.read(cx);
    brink_gpui_shell::settings::Documents {
        root: project.read(cx).root().display().to_string(),
        scroll: code.scroll_state(cx),
        open: code.open_paths(cx),
        active: code.active_path(cx),
    }
}

fn write_all(project: &Entity<Project>, cx: &mut App) -> Vec<(String, std::io::Error)> {
    // `save_all` emits `ProjectEvent::SaveFailed` per failure, which the
    // Output log turns into an error row — so a failed write is visible in
    // the window rather than only on a stderr nobody is reading.
    project.update(cx, |project, cx| project.save_all(cx))
}

/// Where one project window is in closing.
#[derive(Default)]
struct CloseState {
    /// The author has answered (or there was nothing to ask): the next
    /// close goes straight through.
    confirmed: bool,
    /// The unsaved-work prompt is up on this window.
    asking: bool,
}

/// Every project window, so Quit can ask each one. Dead entries (a closed
/// window) are skipped when read rather than tracked on close.
#[derive(Default)]
struct OpenStudios(Vec<(AnyWindowHandle, WeakEntity<Studio>)>);

impl Global for OpenStudios {}

/// A quit is being asked about; a second `cmd-q` meanwhile is the same quit.
#[derive(Default)]
struct Quitting(bool);

impl Global for Quitting {}

/// Quit, asking first in each project window with unsaved files — one
/// prompt at a time, in that window, brought to the front. Cancel in any
/// window, or a Save that cannot write, stops the whole quit.
fn quit_asking(cx: &mut App) {
    if std::mem::replace(&mut cx.default_global::<Quitting>().0, true) {
        return;
    }
    let studios: Vec<(AnyWindowHandle, WeakEntity<Studio>)> = cx
        .default_global::<OpenStudios>()
        .0
        .iter()
        .filter(|(_, studio)| studio.upgrade().is_some())
        .cloned()
        .collect();
    cx.spawn(async move |cx| {
        let mut go = true;
        for (handle, studio) in &studios {
            let asked = cx.update_window(*handle, |_, window, cx| {
                studio.update(cx, |studio, cx| {
                    if studio.close.asking {
                        // A close prompt is already up here; answer that.
                        window.activate_window();
                        Err(())
                    } else {
                        Ok(studio.ask_about_unsaved(window, cx))
                    }
                })
            });
            let answer = match asked {
                Ok(Ok(Ok(Some(answer)))) => answer,
                // Nothing dirty, or the window went away meanwhile.
                Ok(Ok(Ok(None))) | Err(_) | Ok(Err(_)) => continue,
                Ok(Ok(Err(()))) => {
                    go = false;
                    break;
                }
            };
            let choice = answer
                .await
                .map_or(closing::Choice::Cancel, closing::choice);
            let _ = studio.update(cx, |studio, _| studio.close.asking = false);
            match choice {
                closing::Choice::Discard => {}
                closing::Choice::Cancel => {
                    go = false;
                    break;
                }
                closing::Choice::Save => {
                    let saving = cx.update_window(*handle, |_, window, cx| {
                        studio.update(cx, |studio, cx| studio.save_dirty(window, cx))
                    });
                    let saved = match saving {
                        Ok(Ok(saving)) => saving.await.is_empty(),
                        _ => false,
                    };
                    if !saved {
                        let _ = cx.update_window(*handle, |_, window, cx| {
                            studio.update(cx, |studio, cx| studio.report_unsaved(window, cx))
                        });
                        go = false;
                        break;
                    }
                }
            }
        }
        cx.update(|cx| {
            cx.default_global::<Quitting>().0 = false;
            if go {
                for (_, studio) in &studios {
                    let _ = studio.update(cx, |studio, _| studio.close.confirmed = true);
                }
                landing::begin_shutdown(cx);
                cx.quit();
            }
        });
    })
    .detach();
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
            .on_action(cx.listener(Self::extract_to_knot))
            .on_action(cx.listener(Self::extract_to_function))
            .on_action(cx.listener(Self::make_narrative))
            .on_action(cx.listener(Self::make_choice))
            .on_action(cx.listener(Self::make_sticky_choice))
            .on_action(cx.listener(Self::make_gather))
            .on_action(cx.listener(Self::make_choice_body))
            .on_action(cx.listener(Self::play))
            .on_action(cx.listener(Self::toggle_breakpoint))
            .on_action(cx.listener(Self::clear_breakpoints))
            .on_action(cx.listener(Self::debug_continue))
            .on_action(cx.listener(Self::debug_step_line))
            .on_action(cx.listener(Self::debug_step_instruction))
            .on_action(cx.listener(Self::play_restart))
            .on_action(cx.listener(Self::open_compiled_output))
            .on_action(cx.listener(Self::open_story_graph))
            .on_action(cx.listener(Self::quick_open))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::maximize_editor))
            .on_action(cx.listener(Self::open_recent))
            .on_action(cx.listener(Self::undo_file_op))
            .on_action(cx.listener(Self::focus_editor))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::close_tab_by_id))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::close_window))
            .child(self.workspace.clone())
            // After the workspace: later children paint on top, and a
            // dialog under the window it belongs to is no dialog at all.
            .children(self.quick_open.as_ref().map(|(p, _)| p.clone()))
            .children(notifications)
            .children(dialogs)
    }
}

/// A recent's label: the folder's own name, with its parent for context —
/// a list of `story`, `story`, `story` names nothing.
fn recent_label(path: &str) -> String {
    let path = std::path::Path::new(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    match path.parent().and_then(std::path::Path::file_name) {
        Some(parent) => format!("{name} ({})", parent.to_string_lossy()),
        None => name,
    }
}

/// Open a studio window on `root`, answering its handle.
///
/// The one place a project window is made: `landing::open_anchor` is the
/// one caller, so the rem size, the title bar options and the recents
/// bookkeeping cannot drift apart between the first window and the rest.
/// The recent is remembered there, after this, because `Studio::new`
/// registers one command per recent and a window must not offer to reopen
/// itself.
fn open_project_window(
    root: PathBuf,
    entry: Option<String>,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    let root = root.canonicalize().unwrap_or(root);
    let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..TitleBar::window_options()
    };
    let opened = cx.open_window(options, move |window, cx| {
        // The app font size scales the window's rem.
        let rem = brink_gpui_shell::settings::AppSettings::get(cx).rem_size();
        window.set_rem_size(px(rem));
        let view = cx.new(|cx| Studio::new(root, entry, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    match opened {
        Ok(window) => Some(window.into()),
        Err(err) => {
            eprintln!("failed to open window: {err:#}");
            None
        }
    }
}

fn main() {
    // A path on the command line is opened by its door (`landing::anchor_for`):
    // a `.ink`, a `brink.toml`, or a folder.
    let arg = std::env::args().nth(1).map(PathBuf::from);

    // gpui-pre publishes the core without a platform backend; the macOS/
    // Windows/Linux implementations live in `gpui-pre-platform`.
    // The kit's icons (`IconName`) are assets the application has to
    // register; a `Button::icon(IconName::ChevronDown)` with no asset
    // source silently draws nothing.
    let app = Application::with_platform(gpui_platform::current_platform(false))
        .with_assets(brink_gpui_shell::icons::Assets);
    // The Dock icon with nothing open brings the landing back.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            landing::open_landing_window(None, cx);
        }
    });
    app.run(move |cx| {
        gpui_component::init(cx);
        // Hide and the platform's own chords — before any window, since
        // the menu bar is the application's.
        brink_gpui_shell::menus::init(cx);
        // The persisted settings and their theme, before the first paint.
        brink_gpui_shell::settings::init(cx);
        brink_gpui_shell::theme::init(cx);
        landing::install(cx);
        let previous_was_clean = brink_gpui_shell::settings::begin_session(cx);
        let settings = brink_gpui_shell::settings::AppSettings::get(cx);
        match landing::launch(
            arg,
            settings.reopen_last,
            previous_was_clean,
            &settings.recents,
        ) {
            landing::Launch::Open(path) => {
                if let Err(error) = landing::open_anchor(&path, cx) {
                    landing::open_landing_window(Some(error), cx);
                }
            }
            landing::Launch::Landing => landing::open_landing_window(None, cx),
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::recent_label;

    #[test]
    fn a_recent_is_labelled_by_its_folder_and_its_parent() {
        assert_eq!(
            recent_label("/home/me/stories/harbour"),
            "harbour (stories)"
        );
        // Two projects both called `story` are told apart by the parent —
        // which is why the parent is there at all.
        assert_eq!(recent_label("/a/one/story"), "story (one)");
        assert_eq!(recent_label("/a/two/story"), "story (two)");
        assert_eq!(
            recent_label("/"),
            "/",
            "no name and no parent: say the path"
        );
    }
}

/// The two modes, driven on the real `Studio` (see `crate::harness`).
#[cfg(test)]
mod modes_driven {
    use brink_gpui_shell::commands::ToggleToolWindow;
    use brink_gpui_shell::editor_view::{EditorView, ModeScript, ModeWrite};
    use brink_gpui_shell::settings;
    use gpui::AnyWindowHandle;

    use crate::harness::{Harness, scratch_dir, scratch_project};

    const FIXTURE: &str = "tests/tier1-native/conventions-cross-file";

    fn mode(h: &mut Harness, window: AnyWindowHandle) -> EditorView {
        let studio = h.studio(window).expect("open");
        h.read(|cx| studio.read(cx).workspace.read(cx).editor_view(cx))
    }

    #[test]
    fn the_mode_actions_switch_between_write_and_script() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(FIXTURE));
        h.dispatch(window, ModeWrite);
        assert_eq!(mode(&mut h, window), EditorView::Write);
        h.dispatch(window, ModeScript);
        assert_eq!(mode(&mut h, window), EditorView::Script);
    }

    #[test]
    fn a_layout_saved_in_a_removed_or_renamed_view_reopens_in_its_mode() {
        for (saved, expected) in [
            ("single", EditorView::Script),
            ("code", EditorView::Script),
            ("continuous", EditorView::Write),
        ] {
            let mut h = Harness::new();
            h.update(|cx| {
                settings::update(cx, |s| s.layout.editor_view = Some(saved.to_owned()));
            });
            let window = h.open(&scratch_project(FIXTURE));
            assert_eq!(mode(&mut h, window), expected, "saved as {saved:?}");
        }
    }

    /// Write mode draws no docks, but must not CLOSE them: Script comes
    /// back exactly as it was, and the saved layout never sees Write.
    #[test]
    fn write_mode_hides_the_docks_without_closing_them() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(FIXTURE));
        let studio = h.studio(window).expect("open");
        let docks = |h: &mut Harness| {
            h.read(|cx| {
                let workspace = studio.read(cx).workspace.read(cx);
                workspace.layout(cx).docks
            })
        };
        let before = docks(&mut h);
        assert!(
            before.values().any(|d| d.open),
            "the fixture opens with a dock, or this proves nothing"
        );
        h.dispatch(window, ModeWrite);
        assert_eq!(docks(&mut h), before, "entering Write closed a dock");
        // Maximize is meaningless with no docks drawn; it must not
        // quietly close Script's.
        h.dispatch(window, super::MaximizeEditor);
        assert_eq!(docks(&mut h), before, "maximize in Write closed a dock");
        h.dispatch(window, ModeScript);
        assert_eq!(docks(&mut h), before);
    }

    /// Tool windows live in Script's docks, so asking for one from Write
    /// shows it there — without changing the mode the author chose.
    #[test]
    fn a_tool_window_asked_for_in_write_opens_in_script() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(FIXTURE));
        let studio = h.studio(window).expect("open");
        h.dispatch(window, ModeWrite);
        h.dispatch(
            window,
            ToggleToolWindow {
                id: "search".into(),
            },
        );
        assert_eq!(mode(&mut h, window), EditorView::Script);
        let (search_open, chosen) = h.read(|cx| {
            let workspace = studio.read(cx).workspace.read(cx);
            let layout = workspace.layout(cx);
            (layout.docks["left"].open, layout.editor_view)
        });
        assert!(
            search_open,
            "the toggle closed the dock instead of showing it"
        );
        assert_eq!(
            chosen.as_deref(),
            Some("write"),
            "the studio's switch is not the author's choice"
        );
    }

    /// The picture: the title bar's two-mode switch, for checking by eye.
    #[test]
    fn the_title_bar_shows_the_two_mode_switch() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(FIXTURE));
        h.dispatch(window, ModeWrite);
        let shot = scratch_dir("shot").join("modes.png");
        h.screenshot(window, &shot);
        eprintln!("modes screenshot: {}", shot.display());
    }
}
