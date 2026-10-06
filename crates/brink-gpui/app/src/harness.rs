//! The headless harness: the real `Studio`, driven with no window on
//! anyone's screen (decision log 2026-10-03, "The native studio is
//! verified headlessly").
//!
//! Driving the running app from outside does not work on macOS: gpui stops
//! drawing a window the moment anything covers it, so a background
//! screenshot shows the last frame it painted, and only a full screen
//! takeover sees the truth. This runs the same `Studio` on gpui's test
//! platform instead — deterministic scheduling, real text shaping
//! (cosmic-text: see [`Harness::new`] for why not CoreText), and the Metal
//! headless renderer, so [`Harness::screenshot`] is an actual render.
//!
//! Three things differ from the shipping app, each on purpose:
//!
//! - **Settings** live in a fresh temp directory per harness
//!   (`settings::init_at`), so a test can never touch the author's own.
//! - **Prompts** go through a prompt builder installed here rather than
//!   the native alert: each one is recorded, drawn in the window (so a
//!   screenshot shows it), and answered by [`Harness::answer`].
//! - **Quitting** is a no-op on the test platform. Whether a quit went
//!   ahead is read from each window's close state instead.
//!
//! The worker and the file watcher are real threads, which the test
//! dispatcher cannot see; [`Harness::settle_until`] polls for them rather
//! than trusting one `run_until_parked`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gpui::{
    Action, AnyWindowHandle, App, AppContext as _, BorrowAppContext as _, Context, Entity,
    EventEmitter, FocusHandle, Focusable, Global, HeadlessAppContext, InteractiveElement as _,
    IntoElement, Keystroke, ParentElement as _, PromptButton, PromptHandle, PromptLevel,
    PromptResponse, Render, RenderablePromptHandle, Styled as _, Window, div, px, rgb,
};

use crate::{OpenStudios, Studio, landing};

/// A studio under test. Windows are opened on it with [`Harness::open`].
pub struct Harness {
    /// Always `Some` until `drop`, which decides whether the app is really
    /// dropped (running gpui's leak check) or forgotten.
    cx: Option<HeadlessAppContext>,
    check_leaks: bool,
}

impl Harness {
    pub fn new() -> Self {
        // cosmic-text rather than CoreText: the macOS text system is only
        // reachable through `MacPlatform`, which refuses any thread but the
        // main one, and a test never runs there. The shaping is real, so
        // layout is too; glyphs may differ from the shipping app's by a hair.
        let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("Helvetica"));
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(brink_gpui_shell::icons::Assets),
            gpui_platform::current_headless_renderer,
        );
        let config = scratch_dir("config");
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::hover_card::install(cx);
            brink_gpui_shell::settings::init_at(Some(config), cx);
            brink_gpui_shell::theme::init(cx);
            cx.set_global(Prompts::default());
            cx.set_prompt_builder(record_prompt);
            // The app's window lifecycle (last project closed → landing).
            // Not `main`'s `on_reopen`: the test platform cannot take one.
            landing::install(cx);
        });
        Self {
            cx: Some(cx),
            check_leaks: false,
        }
    }

    /// Run gpui's leak check when this harness is dropped: every entity a
    /// closed window held must be gone, or the test panics naming them.
    ///
    /// Off by default only because the studio has a known leak, one code
    /// editor per closed window (#3628), which would fail every test. Once
    /// that is fixed this should become the default.
    pub fn check_leaks(mut self) -> Self {
        self.check_leaks = true;
        self
    }

    fn app(&mut self) -> &mut HeadlessAppContext {
        self.cx
            .as_mut()
            .expect("the app lives until the harness drops")
    }

    /// Open `path` by its door, the way the app does (a `.ink`, a
    /// `brink.toml`, or a folder), and answer its window.
    pub fn open(&mut self, path: &Path) -> AnyWindowHandle {
        let opened = self.app().update(|cx| landing::open_anchor(path, cx));
        let window = opened.unwrap_or_else(|error| {
            unreachable!("the studio could not open {}: {error}", path.display())
        });
        self.settle();
        window
    }

    /// The landing window, while one is open.
    pub fn landing(&mut self) -> Option<AnyWindowHandle> {
        self.app().update(|cx| {
            let studios: Vec<AnyWindowHandle> = cx
                .try_global::<OpenStudios>()
                .map(|open| open.0.iter().map(|(w, _)| *w).collect())
                .unwrap_or_default();
            cx.windows().into_iter().find(|w| !studios.contains(w))
        })
    }

    /// The studio in `window`, while it is open.
    pub fn studio(&mut self, window: AnyWindowHandle) -> Option<Entity<Studio>> {
        self.app().update(|cx| {
            cx.global::<OpenStudios>()
                .0
                .iter()
                .find(|(w, _)| *w == window)
                .and_then(|(_, studio)| studio.upgrade())
        })
    }

    /// Whether `window` is still open.
    pub fn is_open(&mut self, window: AnyWindowHandle) -> bool {
        self.app().update(|cx| cx.windows().contains(&window))
    }

    /// Read something off the app.
    pub fn read<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        self.app().update(f)
    }

    /// Run `f` inside `window` — for what needs the window itself (focus,
    /// whether something is focused). Settles after.
    pub fn app_window<R>(
        &mut self,
        window: AnyWindowHandle,
        f: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> R {
        let r = self
            .app()
            .update_window(window, |_, window, cx| f(window, cx))
            .expect("a window that is open");
        self.settle();
        r
    }

    /// Change something on the app, then settle.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        let r = self.app().update(f);
        self.settle();
        r
    }

    /// Dispatch `action` in `window` as a keybinding would, then settle.
    pub fn dispatch(&mut self, window: AnyWindowHandle, action: impl Action) {
        self.app()
            .update_window(window, |_, window, cx| {
                window.dispatch_action(Box::new(action), cx);
            })
            .expect("dispatching into a window that is open");
        self.settle();
    }

    /// Press `keystrokes` (gpui's syntax: `"cmd-s"`, space-separated for a
    /// sequence) in `window`, then settle.
    pub fn press(&mut self, window: AnyWindowHandle, keystrokes: &str) {
        for source in keystrokes.split_whitespace() {
            let keystroke = Keystroke::parse(source).expect("a keystroke the test spelled");
            self.app()
                .update_window(window, |_, window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                })
                .expect("typing into a window that is open");
        }
        self.settle();
    }

    /// Move the pointer to `(x, y)` (logical pixels) in `window`, so hover
    /// styles apply, then settle.
    pub fn hover(&mut self, window: AnyWindowHandle, x: f32, y: f32) {
        let event = gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
            position: gpui::point(px(x), px(y)),
            pressed_button: None,
            modifiers: gpui::Modifiers::default(),
        });
        self.app()
            .update_window(window, |_, window, cx| {
                window.dispatch_event(event, cx);
                window.refresh();
            })
            .expect("hovering in a window that is open");
        self.settle();
    }

    /// Type `text` into whatever has focus in `window`, a character at a
    /// time, as a keyboard would.
    pub fn type_text(&mut self, window: AnyWindowHandle, text: &str) {
        for ch in text.chars() {
            let key = if ch == ' ' {
                "space".to_owned()
            } else {
                ch.to_string()
            };
            let keystroke = Keystroke {
                key,
                key_char: Some(ch.to_string()),
                ..Keystroke::default()
            };
            self.app()
                .update_window(window, |_, window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                })
                .expect("typing into a window that is open");
        }
        self.settle();
    }

    /// The prompt that is up, if any: its message, detail and buttons.
    pub fn prompt(&mut self) -> Option<PromptSeen> {
        self.app().update(|cx| {
            cx.global::<Prompts>().0.last().map(|prompt| {
                let p = prompt.read(cx);
                PromptSeen {
                    message: p.message.clone(),
                    detail: p.detail.clone(),
                    buttons: p.buttons.clone(),
                }
            })
        })
    }

    /// Answer the prompt that is up by its button's label, then settle.
    pub fn answer(&mut self, label: &str) {
        self.app().update(|cx| {
            let prompt = cx
                .update_global::<Prompts, _>(|prompts, _| prompts.0.pop())
                .expect("answering a prompt, so one is up");
            let index = prompt.read(cx).buttons.iter().position(|b| b == label);
            assert!(index.is_some(), "no {label:?} button on this prompt");
            let index = index.expect("just asserted above");
            prompt.update(cx, |_, cx| cx.emit(PromptResponse(index)));
        });
        self.settle();
    }

    /// Render `window` and write it to `path` as a PNG.
    pub fn screenshot(&mut self, window: AnyWindowHandle, path: &Path) {
        self.capture(window)
            .save(path)
            .expect("writing the screenshot");
    }

    /// Render `window` and hand back its pixels, for a test that measures
    /// the picture rather than looking at it.
    pub fn capture(&mut self, window: AnyWindowHandle) -> image::RgbaImage {
        self.app()
            .update_window(window, |_, window, _| window.refresh())
            .ok();
        self.settle();
        self.app()
            .capture_screenshot(window)
            .expect("the headless renderer is installed")
    }

    /// Run everything that is ready, and give the real threads (the worker,
    /// the watcher) a moment to answer.
    pub fn settle(&mut self) {
        for _ in 0..3 {
            self.app().run_until_parked();
            std::thread::sleep(Duration::from_millis(15));
        }
        self.app().run_until_parked();
    }

    /// Move the clock on by `by`, then settle. The harness runs on
    /// simulated time: a `timer` — a slide's finish, a debounce — fires
    /// only when the test says that much time has passed.
    pub fn advance(&mut self, by: Duration) {
        self.app().advance_clock(by);
        self.settle();
    }

    /// Settle until `done` holds, or `within` passes. Answers whether it
    /// held — a test asserts on that, so a slow machine fails loudly rather
    /// than reading a half-finished state.
    pub fn settle_until(
        &mut self,
        within: Duration,
        mut done: impl FnMut(&mut Self) -> bool,
    ) -> bool {
        let deadline = Instant::now() + within;
        loop {
            self.settle();
            if done(self) {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
        }
    }
}

impl Drop for Harness {
    /// Close every window before the app goes, as quitting does, so gpui's
    /// leak check (on in test builds) reports only what a closed window
    /// really left behind — not everything a still-open one holds. Without
    /// [`Harness::check_leaks`] the app is then forgotten, not dropped.
    fn drop(&mut self) {
        // Closing everything here is the app quitting, so the lifecycle
        // must not answer the last close with a landing window.
        self.app().update(landing::begin_shutdown);
        let windows = self.app().update(|cx| cx.windows());
        for window in windows {
            let _ = self
                .app()
                .update_window(window, |_, window, _| window.remove_window());
        }
        self.settle();
        let app = self.cx.take();
        if !self.check_leaks {
            // Not dropped, so the leak check never runs (see `check_leaks`).
            std::mem::forget(app);
        }
    }
}

/// What a prompt said, as the author would have read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSeen {
    pub message: String,
    pub detail: Option<String>,
    pub buttons: Vec<String>,
}

/// A fresh directory under the system temp dir, unique to this process
/// and call. Tests write projects and settings here, never in the repo.
pub fn scratch_dir(what: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "brink-gpui-harness-{}-{}-{what}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("making a scratch directory");
    dir
}

/// Copy the fixture at `fixture` (repo-relative, e.g.
/// `tests/tier1-native/conventions-cross-file`) to a scratch directory, so
/// a test may save into it.
pub fn scratch_project(fixture: &str) -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(fixture);
    let to = scratch_dir("project");
    for entry in std::fs::read_dir(&from).expect("the fixture exists") {
        let entry = entry.expect("reading the fixture");
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            std::fs::copy(entry.path(), to.join(entry.file_name())).expect("copying the fixture");
        }
    }
    to
}

/// Every prompt raised and not yet answered, oldest first.
#[derive(Default)]
struct Prompts(Vec<Entity<HarnessPrompt>>);

impl Global for Prompts {}

/// The prompt builder the harness installs: record the prompt and draw it
/// in the window, where a screenshot can see it.
fn record_prompt(
    _level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let view = cx.new(|cx| HarnessPrompt {
        message: message.to_owned(),
        detail: detail.map(ToOwned::to_owned),
        buttons: buttons.iter().map(|b| b.label().to_string()).collect(),
        focus: cx.focus_handle(),
    });
    cx.update_global::<Prompts, _>(|prompts, _| prompts.0.push(view.clone()));
    handle.with_view(view, window, cx)
}

struct HarnessPrompt {
    message: String,
    detail: Option<String>,
    buttons: Vec<String>,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for HarnessPrompt {}

impl Focusable for HarnessPrompt {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HarnessPrompt {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(360.))
                    .p(px(16.))
                    .gap(px(8.))
                    .flex()
                    .flex_col()
                    .bg(rgb(0x2b_2d_31))
                    .text_color(rgb(0xe6_e6_e6))
                    .rounded(px(8.))
                    .child(self.message.clone())
                    .children(self.detail.clone())
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .children(self.buttons.iter().map(|b| {
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .bg(rgb(0x44_47_4e))
                                    .child(b.clone())
                            })),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use brink_gpui_shell::commands::CloseWindow;

    use super::{Harness, scratch_project};

    /// The leak canary: a project window opened and closed leaves nothing
    /// behind. Ignored while #3628 (one code editor per closed window) is
    /// open; un-ignore it with the fix so the check guards from then on.
    #[test]
    #[ignore = "#3628: a closed window still leaks one code editor"]
    fn a_closed_window_frees_everything() {
        let mut h = Harness::new().check_leaks();
        let window = h.open(&scratch_project(
            "tests/tier1-native/conventions-cross-file",
        ));
        h.dispatch(window, CloseWindow);
        assert!(!h.is_open(window));
    }

    /// The harness itself: keystrokes reach the real editor the way a
    /// keyboard's do, and a chord runs its command.
    #[test]
    fn typing_reaches_the_editor_and_cmd_s_saves() {
        let mut h = Harness::new();
        let root = scratch_project("tests/tier1-native/conventions-cross-file");
        let window = h.open(&root);
        h.dispatch(window, crate::FocusEditor);
        h.type_text(window, "// typed");
        let studio = h.studio(window).expect("open");
        let active = h.read(|cx| studio.read(cx).code.read(cx).active_path(cx));
        let active = active.expect("the entry is open in a tab");
        let dirty = h.read(|cx| studio.read(cx).project.read(cx).is_dirty(&active));
        assert!(dirty, "typing made {active} dirty");
        h.press(window, "cmd-s");
        assert!(
            h.settle_until(std::time::Duration::from_secs(5), |_| {
                std::fs::read_to_string(root.join(&active)).is_ok_and(|t| t.contains("// typed"))
            }),
            "cmd-s wrote the typed text"
        );
    }

    /// The harness itself: a real render comes back, the size of the
    /// window, and it is not one flat colour.
    #[test]
    fn a_screenshot_is_a_real_render() {
        let mut h = Harness::new();
        let window = h.open(&scratch_project(
            "tests/tier1-native/conventions-cross-file",
        ));
        let path = super::scratch_dir("shot").join("studio.png");
        h.screenshot(window, &path);
        let image = image::open(&path)
            .expect("the screenshot is a PNG")
            .to_rgba8();
        assert!(
            image.width() > 100 && image.height() > 100,
            "{}x{}",
            image.width(),
            image.height()
        );
        let first = image.get_pixel(0, 0);
        assert!(
            image.pixels().any(|p| p != first),
            "a blank frame is no render"
        );
    }
}
