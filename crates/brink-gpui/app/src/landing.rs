//! The landing window, and the doors into a project.
//!
//! Two rulings meet here. "A project is anchored on a FILE" (decision log
//! 2026-08-23): opening a `.ink` makes it the entry, opening a `brink.toml`
//! uses the entry it declares, and a folder is not a door. And the native
//! studio's lifecycle (2026-10-03): with no project open the app shows a
//! landing window — the Tauri app's #3021 screen — and closing the last
//! project window brings it back, so the app never sits there windowless.
//!
//! Everything that decides something is a plain function and tested as
//! one: which door a path is ([`anchor_for`]), what a recent row says
//! ([`recent_display`]), what launching does ([`launch`]), what New Project
//! writes ([`create_project`]) and whether a config governs a story file
//! ([`governing_config`]). The rest is the window and its view.

use std::path::{Path, PathBuf};

use brink_gpui_shell::notify::{Severity, notify};
use brink_gpui_shell::settings::{self, AppSettings};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, ClickEvent, Context, FocusHandle, Focusable,
    Global, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, WindowBounds, WindowOptions, div, img,
    prelude::FluentBuilder as _, px, size,
};
use gpui_component::checkbox::Checkbox;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Root, Sizable as _, TitleBar, h_flex, v_flex};

/// Which door a path opens a project by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// A `brink.toml`: its `[project] entry` names the entry.
    Toml,
    /// A `.ink` story: it IS the entry, whatever a config says.
    Ink,
    /// A `.brink` file. The native door is deferred by the 2026-08-23
    /// ruling, so this opens the surrounding folder the older way.
    Native,
    /// A bare folder: a legacy recent from before the landing window.
    Folder,
}

impl Door {
    /// The badge a recent row carries.
    pub fn badge(self) -> &'static str {
        match self {
            Self::Toml => "TOML",
            Self::Ink => "INK",
            Self::Native => "BRINK",
            Self::Folder => "FOLDER",
        }
    }
}

/// What opening one path means: the folder to load, the explicit entry
/// (the `.ink` door only), and what to remember in the recents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub door: Door,
    pub root: PathBuf,
    /// Root-relative. Beats the config's entry (see `Request::Open`).
    pub entry: Option<String>,
    /// The anchor itself for the two file doors; the folder otherwise.
    pub recent: PathBuf,
}

/// Classify `path` into its door, or say why it cannot be opened. Mirrors
/// the Tauri app's `anchorForPath`, including its one refusal: a `.toml`
/// that is not `brink.toml`, which a file picker cannot filter out.
pub fn anchor_for(path: &Path) -> Result<Anchor, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = || path.parent().map(Path::to_path_buf).unwrap_or_default();
    if path.is_dir() {
        return Ok(Anchor {
            door: Door::Folder,
            root: path.to_path_buf(),
            entry: None,
            recent: path.to_path_buf(),
        });
    }
    if name == "brink.toml" {
        return Ok(Anchor {
            door: Door::Toml,
            root: parent(),
            entry: None,
            recent: path.to_path_buf(),
        });
    }
    if name.ends_with(".toml") {
        return Err(format!(
            "{name} is not a brink.toml — a project config must be named brink.toml."
        ));
    }
    if name.ends_with(".ink") {
        return Ok(Anchor {
            door: Door::Ink,
            root: parent(),
            entry: Some(name),
            recent: path.to_path_buf(),
        });
    }
    if name.ends_with(".brink") {
        let root = parent();
        return Ok(Anchor {
            door: Door::Native,
            recent: root.clone(),
            root,
            entry: None,
        });
    }
    Err(format!(
        "{name} is not a story or a project config — open a .ink file or a brink.toml."
    ))
}

/// What a recent row says: the door's badge, a name, and where it lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentDisplay {
    pub door: Door,
    pub name: String,
    /// The containing folder, `~`-contracted against `home`.
    pub detail: String,
}

/// The row for recent `path`. A config is named by its folder, since every
/// config is called `brink.toml`; a story file by its own name.
pub fn recent_display(path: &str, home: Option<&Path>) -> RecentDisplay {
    let path = Path::new(path);
    let door = match path.file_name().map(|n| n.to_string_lossy().into_owned()) {
        Some(n) if n == "brink.toml" => Door::Toml,
        Some(n) if n.ends_with(".ink") => Door::Ink,
        Some(n) if n.ends_with(".brink") => Door::Native,
        _ => Door::Folder,
    };
    let file_name = |p: &Path| {
        p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    let (name, place) = match door {
        Door::Toml => {
            let folder = path.parent().unwrap_or(path);
            (file_name(folder), folder.parent().unwrap_or(folder))
        }
        Door::Folder => (file_name(path), path.parent().unwrap_or(path)),
        Door::Ink | Door::Native => (file_name(path), path.parent().unwrap_or(path)),
    };
    let detail = match home {
        Some(home) if place.starts_with(home) => format!(
            "~{}",
            place.display().to_string()[home.display().to_string().len()..].to_owned()
        ),
        _ => place.display().to_string(),
    };
    RecentDisplay { door, name, detail }
}

/// What launching does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    Open(PathBuf),
    Landing,
}

/// The launch decision. A path on the command line opens it. Otherwise the
/// last project reopens only when the author asked for that AND the last
/// session ended cleanly AND it is still there; anything else lands.
pub fn launch(
    arg: Option<PathBuf>,
    reopen_last: bool,
    previous_was_clean: bool,
    recents: &[String],
) -> Launch {
    if let Some(arg) = arg {
        return Launch::Open(arg);
    }
    if reopen_last
        && previous_was_clean
        && let Some(last) = recents.first()
        && Path::new(last).exists()
    {
        return Launch::Open(PathBuf::from(last));
    }
    Launch::Landing
}

/// The starter story New Project writes: small, but it plays on the first
/// Run. The Tauri app's, verbatim, so the two studios scaffold alike.
pub const NEW_PROJECT_STORY: &str = "Welcome to your new story.\n* [Begin] -> begin\n\n=== begin ===\nThe story starts here.\n-> END\n";

/// The `brink.toml` New Project writes: the two lines whose absence was
/// #3010.
pub fn new_project_config(entry: &str) -> String {
    format!("[project]\nentry = \"{entry}\"\n")
}

/// Create `main.ink` and `brink.toml` in `dir`, answering the config's
/// path. Refuses a folder that already has either — New Project never
/// overwrites a story.
pub fn create_project(dir: &Path) -> Result<PathBuf, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder.", dir.display()));
    }
    let config = dir.join("brink.toml");
    let entry = dir.join("main.ink");
    if config.exists() {
        return Err(format!("{} already has a brink.toml.", dir.display()));
    }
    if entry.exists() {
        return Err(format!("{} already exists.", entry.display()));
    }
    std::fs::write(&entry, NEW_PROJECT_STORY).map_err(|e| format!("{}: {e}", entry.display()))?;
    std::fs::write(&config, new_project_config("main.ink"))
        .map_err(|e| format!("{}: {e}", config.display()))?;
    Ok(config)
}

/// A `brink.toml` that governs a story file opened by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Governing {
    /// Relative to the opened file's folder: `brink.toml`, `../brink.toml`.
    pub config: String,
    /// The config's `[project] entry`, as written.
    pub entry: Option<String>,
    /// Whether the opened file IS that entry.
    pub opened_is_entry: bool,
}

/// The config that governs `story`, found by the compiler's own bounded
/// walk-up, if any. The 2026-08-23 ruling: the explicit open wins, and the
/// app says so rather than letting either side lose silently.
pub fn governing_config(story: &Path) -> Option<Governing> {
    let (found, _) = brink_project_config::discover_from_entry_with_warnings(story);
    let config_path = found?;
    let entry = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|text| brink_project_config::parse_str(&text).ok())
        .and_then(|(config, _)| config.entry);
    let opened_is_entry = match (&entry, config_path.parent()) {
        (Some(entry), Some(dir)) => same_file(&dir.join(entry), story),
        _ => false,
    };
    let story_dir = story.parent().unwrap_or(story);
    let mut config = PathBuf::new();
    let mut walk = story_dir;
    while Some(walk) != config_path.parent() {
        config.push("..");
        match walk.parent() {
            Some(up) => walk = up,
            None => break,
        }
    }
    config.push("brink.toml");
    Some(Governing {
        config: config.display().to_string(),
        entry,
        opened_is_entry,
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!(
        (a.canonicalize(), b.canonicalize()),
        (Ok(a), Ok(b)) if a == b
    )
}

/// What the author is told when a config governs the story they opened.
pub fn governing_message(opened: &str, governing: &Governing) -> String {
    if governing.opened_is_entry {
        format!(
            "{opened} is the entry of {config}. You opened the story file, so the project is \
             anchored on it; open {config} to anchor on the config instead.",
            config = governing.config
        )
    } else {
        let named = governing
            .entry
            .as_deref()
            .map_or_else(|| "no entry".to_owned(), |e| format!("{e} as its entry"));
        format!(
            "{config} governs {opened} and names {named}. You opened {opened}, so it is the \
             entry instead.",
            config = governing.config
        )
    }
}

// ─── The app side ────────────────────────────────────────────────────────

/// The landing window, while one is open.
#[derive(Default)]
struct LandingWindow(Option<AnyWindowHandle>);

impl Global for LandingWindow {}

/// The app is going away: closing windows now must not bring the landing
/// back.
#[derive(Default)]
struct ShuttingDown(bool);

impl Global for ShuttingDown {}

/// Wire the lifecycle: the last project window closing opens the landing,
/// and a quit marks the session as ended cleanly. Once, at startup.
pub fn install(cx: &mut App) {
    cx.set_global(LandingWindow::default());
    cx.set_global(ShuttingDown::default());
    cx.on_window_closed(|cx, closed| {
        let landing = cx.global::<LandingWindow>().0;
        if landing.is_some_and(|w| w.window_id() == closed) {
            // The landing itself: the author put it away. The Dock icon
            // brings it back (`main`'s `on_reopen`).
            cx.set_global(LandingWindow(None));
            return;
        }
        if cx.global::<ShuttingDown>().0 || !cx.windows().is_empty() {
            return;
        }
        open_landing_window(None, cx);
    })
    .detach();
    cx.on_app_quit(|cx| {
        begin_shutdown(cx);
        settings::end_session(cx);
        async {}
    })
    .detach();
}

/// From here on, a closing window is the app quitting, not the author
/// closing a project.
pub fn begin_shutdown(cx: &mut App) {
    cx.set_global(ShuttingDown(true));
}

/// Show the landing window — the one already open, if there is one —
/// with `error` under its doors.
pub fn open_landing_window(error: Option<String>, cx: &mut App) {
    if let Some(existing) = cx.global::<LandingWindow>().0 {
        let shown = existing.update(cx, |_, window, cx| {
            window.activate_window();
            if let Some(landing) = window
                .root::<Root>()
                .flatten()
                .and_then(|root| root.read(cx).view().clone().downcast::<Landing>().ok())
            {
                landing.update(cx, |landing, cx| {
                    landing.error.clone_from(&error);
                    cx.notify();
                });
            }
        });
        if shown.is_ok() {
            return;
        }
    }
    let bounds = Bounds::centered(None, size(px(880.), px(690.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..TitleBar::window_options()
    };
    let opened = cx.open_window(options, |window, cx| {
        let rem = AppSettings::get(cx).rem_size();
        window.set_rem_size(px(rem));
        let view = cx.new(|cx| Landing {
            error,
            focus: cx.focus_handle(),
        });
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Ok(handle) = opened {
        cx.set_global(LandingWindow(Some(handle.into())));
    }
}

/// Open the project `path` anchors, closing the landing if it is up.
/// Answers the new window, or why nothing opened.
pub fn open_anchor(path: &Path, cx: &mut App) -> Result<AnyWindowHandle, String> {
    let anchor = anchor_for(path)?;
    let window = crate::open_project_window(anchor.root.clone(), anchor.entry.clone(), cx)
        .ok_or_else(|| format!("Could not open a window for {}.", path.display()))?;
    settings::remember_project(&anchor.recent, cx);
    if anchor.door == Door::Ink
        && let Some(governing) = governing_config(path)
        && let Some(opened) = anchor.entry.as_deref()
    {
        let message = governing_message(opened, &governing);
        let _ = window.update(cx, |_, window, cx| {
            notify(Severity::Warning, "project", message, window, cx);
        });
    }
    close_landing(cx);
    Ok(window)
}

/// Open recent `path`; one that is no longer there is dropped from the
/// list rather than opened onto nothing.
pub fn open_recent(path: &str, cx: &mut App) -> Result<AnyWindowHandle, String> {
    if !Path::new(path).exists() {
        let gone = path.to_owned();
        settings::update(cx, |s| s.recents.retain(|p| p != &gone));
        return Err(format!("{path} is no longer there."));
    }
    open_anchor(Path::new(path), cx)
}

/// Put the landing away once a project is up. Deferred, because the
/// caller may be the landing's own click handler, mid-update.
fn close_landing(cx: &mut App) {
    if let Some(landing) = cx.global::<LandingWindow>().0 {
        cx.defer(move |cx| {
            let _ = landing.update(cx, |_, window, _| window.remove_window());
        });
    }
}

/// Ask for a story file or config, and open it. A failure lands on the
/// landing (opening it if need be), where it can be read.
pub fn choose_and_open(cx: &mut App) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = paths.await else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        cx.update(|cx| {
            if let Err(error) = open_anchor(&path, cx) {
                open_landing_window(Some(error), cx);
            }
        });
    })
    .detach();
}

/// Ask for a folder, scaffold `main.ink` + `brink.toml` there, and open it.
pub fn new_project(cx: &mut App) {
    let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Create Project Here".into()),
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = paths.await else {
            return;
        };
        let Some(dir) = paths.into_iter().next() else {
            return;
        };
        cx.update(|cx| {
            let opened = create_project(&dir).and_then(|config| open_anchor(&config, cx));
            if let Err(error) = opened {
                open_landing_window(Some(error), cx);
            }
        });
    })
    .detach();
}

/// The landing view: the lockup, the two doors, the recents, the reopen
/// checkbox. Laid out after `docs/design/project-open-flow/Main.dc.html`,
/// in the studio's theme rather than the artboard's fixed colours.
pub struct Landing {
    error: Option<String>,
    focus: FocusHandle,
}

impl Focusable for Landing {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Landing {
    fn door(
        id: &'static str,
        title: &'static str,
        body: &'static str,
        tint: Hsla,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .id(id)
            .flex_1()
            // A flex item's minimum is its content by default: without
            // this the body never wraps and the pair runs off the column.
            .min_w_0()
            .gap(px(7.))
            .px(px(15.))
            .py(px(14.))
            .rounded(px(7.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .cursor_pointer()
            .hover(|s| s.bg(theme.secondary_hover))
            .on_click(on_click)
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(div().size(px(6.)).rounded(px(2.)).bg(tint))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(title),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .line_height(px(17.))
                    .text_color(theme.muted_foreground)
                    .child(body),
            )
    }

    fn recents(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let recents = AppSettings::get(cx).recents;
        let list = v_flex()
            .w_full()
            .rounded(px(7.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .overflow_hidden();
        let list = if recents.is_empty() {
            list.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child("No recent projects yet — anything you open shows up here."),
            )
        } else {
            let count = recents.len();
            list.children(recents.into_iter().enumerate().map(|(ix, path)| {
                let row = recent_display(&path, home.as_deref());
                let tint = match row.door {
                    Door::Toml => theme.blue,
                    Door::Ink => theme.magenta,
                    Door::Native | Door::Folder => theme.muted_foreground,
                };
                let open = path.clone();
                h_flex()
                    .id(("recent", ix))
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(8.))
                    .items_center()
                    // The list draws its own edge; the last row adds none.
                    .when(ix + 1 < count, |row| {
                        row.border_b_1().border_color(theme.border)
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .tooltip({
                        let hint = SharedString::from(path);
                        move |window, cx| Tooltip::new(hint.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        match open_recent(&open, cx) {
                            Ok(_) => window.remove_window(),
                            Err(error) => {
                                this.error = Some(error);
                                cx.notify();
                            }
                        }
                    }))
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(5.))
                            .py(px(2.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(tint.opacity(0.4))
                            .text_size(px(8.5))
                            .text_color(tint)
                            .font_family("Menlo")
                            .child(row.door.badge()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(theme.foreground)
                            .child(row.name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_size(px(10.5))
                            .text_color(theme.muted_foreground)
                            .font_family("Menlo")
                            .child(row.detail),
                    )
            }))
        };
        v_flex()
            .w_full()
            .gap(px(7.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child("RECENT"),
            )
            .child(list)
    }
}

impl Render for Landing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let reopen = AppSettings::get(cx).reopen_last;
        let error = self.error.clone();
        v_flex()
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(TitleBar::new().child(
                div().text_size(px(12.)).text_color(theme.muted_foreground).child("Brink Studio"),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .px(px(64.))
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(560.))
                            .items_center()
                            .gap(px(30.))
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .child(img("brand/brink-mark.svg").size(px(84.)))
                                    .child(
                                        v_flex()
                                            .items_center()
                                            .gap(px(5.))
                                            .child(
                                                div()
                                                    .text_size(px(25.))
                                                    .font_weight(gpui::FontWeight::LIGHT)
                                                    .child("Brink Studio"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .text_color(theme.muted_foreground)
                                                    .child(
                                                        "Open a story file or a project config to begin.",
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap(px(10.))
                                    .child(Self::door(
                                        "new-project",
                                        "New Project\u{2026}",
                                        "Pick a folder. Creates main.ink and brink.toml, ready to play.",
                                        theme.green,
                                        |_, _, cx| new_project(cx),
                                        cx,
                                    ))
                                    .child(Self::door(
                                        "open-project",
                                        "Open\u{2026}",
                                        "Open a .ink file \u{2014} it becomes the entry point \u{2014} or a brink.toml.",
                                        theme.blue,
                                        |_, _, cx| choose_and_open(cx),
                                        cx,
                                    )),
                            )
                            .when_some(error, |el, error| {
                                el.child(
                                    div()
                                        .w_full()
                                        .text_size(px(11.5))
                                        .text_color(theme.red)
                                        .child(error),
                                )
                            })
                            .child(self.recents(cx))
                            .child(
                                h_flex().w_full().child(
                                    Checkbox::new("reopen-last")
                                        .small()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .label("Reopen last project on launch")
                                        .checked(reopen)
                                        .on_click(|on, _, cx| {
                                            let on = *on;
                                            settings::update(cx, |s| s.reopen_last = on);
                                        }),
                                ),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(files: &[(&str, &str)]) -> PathBuf {
        let dir = crate::harness::scratch_dir("landing");
        for (name, text) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("making a scratch folder");
            }
            std::fs::write(path, text).expect("writing a scratch file");
        }
        dir
    }

    #[test]
    fn each_door_anchors_where_the_ruling_says() {
        let dir = scratch(&[
            ("brink.toml", ""),
            ("story.ink", ""),
            ("main.brink", ""),
            ("x.toml", ""),
        ]);

        let toml = anchor_for(&dir.join("brink.toml")).expect("a config opens");
        assert_eq!(
            (toml.door, toml.entry, &toml.root),
            (Door::Toml, None, &dir)
        );

        let ink = anchor_for(&dir.join("story.ink")).expect("a story opens");
        assert_eq!(ink.door, Door::Ink);
        assert_eq!(
            ink.entry.as_deref(),
            Some("story.ink"),
            "the opened file IS the entry"
        );
        assert_eq!(
            ink.recent,
            dir.join("story.ink"),
            "the file is what is remembered"
        );

        let native = anchor_for(&dir.join("main.brink")).expect("a native file opens");
        assert_eq!((native.door, native.entry), (Door::Native, None));

        let folder = anchor_for(&dir).expect("a legacy folder recent still opens");
        assert_eq!(folder.door, Door::Folder);

        let refused = anchor_for(&dir.join("x.toml")).expect_err("only brink.toml is a config");
        assert!(refused.contains("not a brink.toml"), "{refused}");
    }

    #[test]
    fn a_recent_row_names_a_config_by_its_folder_and_a_story_by_itself() {
        let home = Path::new("/home/me");
        let toml = recent_display("/home/me/stories/harbour/brink.toml", Some(home));
        assert_eq!(
            (toml.door, toml.name.as_str(), toml.detail.as_str()),
            (Door::Toml, "harbour", "~/stories")
        );
        let ink = recent_display("/home/me/drafts/nightjar/prologue.ink", Some(home));
        assert_eq!(
            (ink.door, ink.name.as_str(), ink.detail.as_str()),
            (Door::Ink, "prologue.ink", "~/drafts/nightjar")
        );
        let elsewhere = recent_display("/srv/story.ink", Some(home));
        assert_eq!(elsewhere.detail, "/srv");
    }

    #[test]
    fn launch_reopens_only_when_asked_after_a_clean_exit() {
        let dir = scratch(&[("story.ink", "")]);
        let last = dir.join("story.ink").display().to_string();
        let recents = [last.clone()];
        let open = Launch::Open(PathBuf::from(&last));
        assert_eq!(launch(None, true, true, &recents), open);
        assert_eq!(
            launch(None, false, true, &recents),
            Launch::Landing,
            "not asked"
        );
        assert_eq!(
            launch(None, true, false, &recents),
            Launch::Landing,
            "after a crash"
        );
        assert_eq!(
            launch(None, true, true, &[]),
            Launch::Landing,
            "nothing to reopen"
        );
        let gone = ["/nowhere/story.ink".to_owned()];
        assert_eq!(
            launch(None, true, true, &gone),
            Launch::Landing,
            "moved away"
        );
        let arg = PathBuf::from("/a/b.ink");
        assert_eq!(
            launch(Some(arg.clone()), false, false, &[]),
            Launch::Open(arg)
        );
    }

    #[test]
    fn new_project_writes_a_story_that_names_itself_and_never_overwrites() {
        let dir = scratch(&[]);
        let config = create_project(&dir).expect("an empty folder takes a project");
        assert_eq!(config, dir.join("brink.toml"));
        assert_eq!(
            std::fs::read_to_string(&config).expect("written"),
            "[project]\nentry = \"main.ink\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("main.ink")).expect("written"),
            NEW_PROJECT_STORY
        );
        let again = create_project(&dir).expect_err("a project is never overwritten");
        assert!(again.contains("already has a brink.toml"), "{again}");
    }

    #[test]
    fn a_config_above_a_story_governs_it_and_says_so() {
        let dir = scratch(&[
            ("brink.toml", "[project]\nentry = \"chapters/one.ink\"\n"),
            ("chapters/one.ink", "One.\n"),
            ("chapters/two.ink", "Two.\n"),
        ]);
        let one = governing_config(&dir.join("chapters/one.ink")).expect("governed");
        assert_eq!(one.config, "../brink.toml");
        assert!(one.opened_is_entry);
        assert!(governing_message("one.ink", &one).contains("is the entry of ../brink.toml"));

        let two = governing_config(&dir.join("chapters/two.ink")).expect("governed");
        assert!(!two.opened_is_entry);
        let message = governing_message("two.ink", &two);
        assert!(
            message.contains("names chapters/one.ink as its entry"),
            "{message}"
        );

        let loose = scratch(&[("solo.ink", "Hi.\n")]);
        assert_eq!(governing_config(&loose.join("solo.ink")), None);
    }
}

/// The lifecycle, driven on the real windows (see `crate::harness`).
#[cfg(test)]
mod driven {
    use brink_gpui_shell::commands::CloseWindow;
    use brink_gpui_shell::notify::Notifications;

    use crate::harness::{Harness, scratch_dir, scratch_project};

    #[test]
    fn closing_the_last_project_brings_the_landing_back() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(
            "tests/tier1-native/conventions-cross-file",
        ));
        assert_eq!(h.landing(), None, "a project is up, so no landing");
        h.dispatch(window, CloseWindow);
        assert!(!h.is_open(window));
        assert!(h.landing().is_some(), "the app never sits windowless");
    }

    #[test]
    fn opening_a_project_puts_the_landing_away() {
        let mut h = Harness::new();
        h.update(|cx| super::open_landing_window(None, cx));
        assert!(h.landing().is_some());
        let dir = scratch_dir("door");
        std::fs::write(dir.join("story.ink"), "Hello.\n-> END\n").expect("writing a story");
        let window = h.open(&dir.join("story.ink"));
        assert!(h.is_open(window));
        assert_eq!(h.landing(), None);
        let recents = h.read(|cx| brink_gpui_shell::settings::AppSettings::get(cx).recents);
        assert_eq!(
            recents.first().map(String::as_str),
            Some(dir.join("story.ink").to_string_lossy().as_ref()),
            "the story file is remembered, not its folder"
        );
    }

    #[test]
    fn a_story_a_config_governs_opens_on_the_story_and_says_so() {
        let mut h = Harness::new();
        let dir = scratch_dir("governed");
        std::fs::write(dir.join("brink.toml"), "[project]\nentry = \"main.ink\"\n")
            .expect("config");
        std::fs::write(dir.join("main.ink"), "Main.\n-> END\n").expect("main");
        std::fs::write(dir.join("side.ink"), "Side.\n-> END\n").expect("side");
        let window = h.open(&dir.join("side.ink"));
        let studio = h.studio(window).expect("open");
        assert!(
            h.settle_until(std::time::Duration::from_secs(5), |h| {
                h.read(|cx| studio.read(cx).project.read(cx).entry().map(str::to_owned))
                    .as_deref()
                    == Some("side.ink")
            }),
            "the opened story is the entry, not the config's"
        );
        let notices = h.read(|cx| Notifications::get(cx));
        assert!(
            notices
                .iter()
                .any(|n| n.message.contains("brink.toml governs side.ink")),
            "{notices:?}"
        );
    }

    /// Not an assertion so much as the picture: the landing, rendered, with
    /// a recent of each door. Written beside the test output so it can be
    /// looked at.
    #[test]
    fn the_landing_renders() {
        let mut h = Harness::new();
        let dir = scratch_dir("recents");
        let story = dir.join("drafts/prologue.ink");
        let config = dir.join("harbour/brink.toml");
        h.update(|cx| {
            brink_gpui_shell::settings::update(cx, |s| {
                s.recents = vec![config.display().to_string(), story.display().to_string()];
            });
            super::open_landing_window(None, cx);
        });
        let landing = h.landing().expect("the landing is up");
        let path = scratch_dir("shot").join("landing.png");
        h.screenshot(landing, &path);
        let image = image::open(&path).expect("a PNG").to_rgba8();
        let first = image.get_pixel(0, 0);
        assert!(image.pixels().any(|p| p != first), "not a blank frame");
        eprintln!("landing screenshot: {}", path.display());
    }
}
