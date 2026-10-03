//! The menu bar — generated from the command registry (issue #3624).
//!
//! `docs/studio-shell-spec.md` §6 asks for "a grouped menu generated from
//! the command registry — no hand-maintained menu structure", and the
//! decision log (2026-06-10) foresaw "the same registry could feed a native
//! menu bar in a future desktop shell". This is that bar. The registry is
//! still the only list of commands: a [`MenuSpec`] names which registry
//! GROUPS go in which menu, never which commands, so a command added to a
//! group lands in the menus with no edit here — and a group no spec names
//! still gets a menu of its own, so nothing registered is ever unreachable.
//!
//! **One structure, two presentations** (ruled 2026-10-03). On macOS it is
//! the native menu bar (`cx.set_menus`), with the App menu macOS expects.
//! Elsewhere there is no native bar, and the same menus are drawn in the
//! title bar by the kit's `AppMenuBar`; the App menu's items move to where
//! those platforms keep them (Settings and Quit at the foot of the first
//! menu, About under Help), so no title-bar menu reads "brink".
//!
//! What is NOT in the registry is what the platform owns rather than the
//! studio: the text-editing block (Undo … Select All, which act on whatever
//! input has focus and carry the OS's own actions), Hide / Hide Others /
//! Show All, and the Window menu. Those are macOS conventions, and the
//! palette has no business listing them.

use gpui::{App, Menu, MenuItem, OsAction, SharedString, SystemMenuType, actions};

use crate::commands::Command;

/// What macOS shows as the App menu's title (it substitutes the bundle's
/// own name in a packaged app) and what the standard items say.
pub const APP_NAME: &str = "brink";

/// The registry group that holds the App menu's commands — Settings…,
/// registered by the shell. Quit is the App menu's wherever it is
/// registered (see [`Quit`]).
pub const APP_GROUP: &str = "App";

/// The registry group whose commands make up the Help menu.
pub const HELP_GROUP: &str = "Help";

actions!(
    brink,
    [
        /// Quit the application. Defined here so the menu bar can place it
        /// — last in the App menu on the Mac, at the foot of the first menu
        /// elsewhere — whichever registry group the app registers it in.
        /// HANDLED by the app, never here: quitting asks about unsaved
        /// work first, and that is the app's to ask.
        Quit,
        /// Show what this application is.
        About,
        /// macOS: hide the application.
        Hide,
        /// macOS: hide every other application.
        HideOthers,
        /// macOS: show every application again.
        ShowAll,
        /// Minimize the window.
        Minimize,
        /// Zoom the window (macOS's green button).
        Zoom,
    ]
);

/// Which presentation the menus are built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPlatform {
    /// The native menu bar, with an App menu and a Window menu.
    Mac,
    /// The in-window bar: no App or Window menu.
    Other,
}

impl MenuPlatform {
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

/// One part of a menu: a registry group's commands inline, or a group as a
/// submenu named after it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Group(SharedString),
    Submenu(SharedString),
}

impl Part {
    fn group(&self) -> &SharedString {
        match self {
            Self::Group(g) | Self::Submenu(g) => g,
        }
    }
}

/// One top-level menu: a title and the registry groups it holds, in order,
/// separated from each other. The app says which groups go where — the
/// group names are its vocabulary, not the shell's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSpec {
    title: SharedString,
    parts: Vec<Part>,
    text_editing: bool,
}

impl MenuSpec {
    #[must_use]
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            parts: Vec::new(),
            text_editing: false,
        }
    }

    /// A group's commands, inline.
    #[must_use]
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.parts.push(Part::Group(group.into()));
        self
    }

    /// A group's commands as a submenu titled with the group's name.
    #[must_use]
    pub fn submenu(mut self, group: impl Into<SharedString>) -> Self {
        self.parts.push(Part::Submenu(group.into()));
        self
    }

    /// Lead with Undo, Redo, Cut, Copy, Paste and Select All — the Edit
    /// menu's platform block.
    #[must_use]
    pub fn text_editing(mut self) -> Self {
        self.text_editing = true;
        self
    }
}

/// The global halves of the standard items: the ones that need no window.
/// Call once at startup. The window-scoped ones (About, Minimize, Zoom)
/// are the workspace's.
pub fn init(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    // The platform's own chords, so the native menu shows them and they
    // work. Not registry commands, so not rebindable: they are the OS's.
    if MenuPlatform::current() == MenuPlatform::Mac {
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-h", Hide, None),
            gpui::KeyBinding::new("alt-cmd-h", HideOthers, None),
            gpui::KeyBinding::new("cmd-m", Minimize, None),
        ]);
    }
}

/// The whole menu bar, from the registry. Pure: the platform is a
/// parameter so both presentations are testable on either.
#[must_use]
pub fn build(commands: &[Command], layout: &[MenuSpec], platform: MenuPlatform) -> Vec<Menu> {
    // Quit is placed by platform, not by group, so no group lists it.
    let is_quit = |c: &Command| c.action.partial_eq(&Quit);
    let in_group = |group: &str| -> Vec<&Command> {
        commands
            .iter()
            .filter(|c| c.group.as_ref() == group && !is_quit(c))
            .collect()
    };
    let item = |c: &Command| MenuItem::Action {
        name: c.title.clone(),
        action: c.action.boxed_clone(),
        os_action: None,
        checked: false,
        disabled: false,
    };
    // Built per use: `MenuItem` is not `Clone`.
    let app_items = || -> Vec<MenuItem> { in_group(APP_GROUP).into_iter().map(item).collect() };
    let quit = commands.iter().find(|c| is_quit(c));

    let mut menus = Vec::new();
    if platform == MenuPlatform::Mac {
        let mut items = vec![MenuItem::action(format!("About {APP_NAME}"), About)];
        push_section(&mut items, app_items());
        push_section(
            &mut items,
            vec![MenuItem::os_submenu("Services", SystemMenuType::Services)],
        );
        push_section(
            &mut items,
            vec![
                MenuItem::action(format!("Hide {APP_NAME}"), Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
            ],
        );
        if quit.is_some() {
            push_section(
                &mut items,
                vec![MenuItem::action(format!("Quit {APP_NAME}"), Quit)],
            );
        }
        menus.push(Menu::new(APP_NAME).items(items));
    }

    let mut placed: Vec<&str> = vec![APP_GROUP, HELP_GROUP];
    for (ix, spec) in layout.iter().enumerate() {
        let mut items = Vec::new();
        if spec.text_editing {
            items.extend(text_editing_items());
        }
        for part in &spec.parts {
            placed.push(part.group());
            let commands = in_group(part.group());
            if commands.is_empty() {
                continue;
            }
            let section: Vec<MenuItem> = commands.into_iter().map(item).collect();
            match part {
                Part::Group(_) => push_section(&mut items, section),
                Part::Submenu(group) => push_section(
                    &mut items,
                    vec![MenuItem::submenu(Menu::new(group.clone()).items(section))],
                ),
            }
        }
        // Off the Mac, the App menu's items close the first menu — File,
        // where Windows and Linux applications keep Settings and Quit.
        if platform == MenuPlatform::Other && ix == 0 {
            push_section(&mut items, app_items());
            if let Some(quit) = quit {
                push_section(&mut items, vec![item(quit)]);
            }
        }
        if !items.is_empty() {
            menus.push(Menu::new(spec.title.clone()).items(items));
        }
    }

    // A group nobody placed still gets a menu: generated means complete.
    let mut leftover: Vec<&SharedString> = Vec::new();
    for c in commands {
        if !is_quit(c) && !placed.contains(&c.group.as_ref()) && !leftover.contains(&&c.group) {
            leftover.push(&c.group);
        }
    }
    for group in leftover {
        let items = in_group(group).into_iter().map(item).collect::<Vec<_>>();
        menus.push(Menu::new(group.clone()).items(items));
    }

    if platform == MenuPlatform::Mac {
        menus.push(Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]));
    }

    let mut help: Vec<MenuItem> = in_group(HELP_GROUP).into_iter().map(item).collect();
    if platform == MenuPlatform::Other {
        push_section(
            &mut help,
            vec![MenuItem::action(format!("About {APP_NAME}"), About)],
        );
    }
    if !help.is_empty() {
        menus.push(Menu::new(HELP_GROUP).items(help));
    }
    menus
}

/// Append `section` after a separator, unless either side is empty.
fn push_section(items: &mut Vec<MenuItem>, section: Vec<MenuItem>) {
    if section.is_empty() {
        return;
    }
    if !items.is_empty() {
        items.push(MenuItem::separator());
    }
    items.extend(section);
}

/// Edit's platform block. The kit's input actions, which every text field
/// and editor in the studio already handles; `OsAction` lets macOS route
/// them to native text views (a file dialog's name field) too.
fn text_editing_items() -> Vec<MenuItem> {
    use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
    vec![
        MenuItem::os_action("Undo", Undo, OsAction::Undo),
        MenuItem::os_action("Redo", Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", Cut, OsAction::Cut),
        MenuItem::os_action("Copy", Copy, OsAction::Copy),
        MenuItem::os_action("Paste", Paste, OsAction::Paste),
        MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
    ]
}

/// The bar as the in-window `AppMenuBar` reads it, and installed natively.
/// `Menu` is not `Clone`, so it is built twice; it is small.
pub fn install(commands: &[Command], layout: &[MenuSpec], cx: &mut App) {
    let platform = MenuPlatform::current();
    cx.set_menus(build(commands, layout, platform));
    let owned = build(commands, layout, platform)
        .into_iter()
        .map(Menu::owned)
        .collect();
    gpui_component::GlobalState::global_mut(cx).set_app_menus(owned);
}

/// Open a dialog that a menu item may have asked for.
///
/// Off the Mac, the kit's in-window `AppMenuBar` hands focus back to
/// whatever held it before the menu opened — and does so AFTER the chosen
/// item has run. A dialog opened synchronously by that item takes focus and
/// then loses it straight back to the editor, so Escape and Enter reach the
/// editor instead of the dialog. Opened one effect cycle late, the dialog
/// comes after the hand-back and keeps its focus. Harmless from a key or
/// the native menu, which move no focus. (A dialog whose builder focuses
/// an input every render, like the rename prompt, survives either way.)
pub fn open_dialog<F>(window: &mut gpui::Window, cx: &mut App, build: F)
where
    F: Fn(
            gpui_component::dialog::Dialog,
            &mut gpui::Window,
            &mut App,
        ) -> gpui_component::dialog::Dialog
        + 'static,
{
    use gpui_component::WindowExt as _;
    window.defer(cx, move |window, cx| window.open_dialog(cx, build));
}

/// Whether an action is one of the standard items above — for a test that
/// the generated bar holds every registered command and nothing twice.
#[cfg(test)]
fn is_standard(action: &dyn gpui::Action) -> bool {
    action.partial_eq(&About)
        || action.partial_eq(&Hide)
        || action.partial_eq(&HideOthers)
        || action.partial_eq(&ShowAll)
        || action.partial_eq(&Minimize)
        || action.partial_eq(&Zoom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandRegistry, OpenSettings, TogglePalette};
    use crate::editor_view::{ViewCode, ViewContinuous, ViewSingle};
    use gpui::Action;

    actions!(test, [Save, Play, Step, Stray]);

    fn registry() -> CommandRegistry {
        let mut r = CommandRegistry::default();
        r.register("View", "Code", ViewCode, None);
        r.register("View", "Single File", ViewSingle, None);
        r.register("View", "Command Palette", TogglePalette, None);
        r.register(APP_GROUP, "Settings\u{2026}", OpenSettings, None);
        // The app registers Quit under File; the bar moves it.
        r.register("File", "Quit", Quit, None);
        r.register("File", "Save", Save, None);
        r.register("Play", "Play", Play, None);
        r.register("Debug", "Step", Step, None);
        r.register("Unplaced", "Stray", Stray, None);
        r.register("Theme", "Mocha", ViewContinuous, None);
        r
    }

    fn layout() -> Vec<MenuSpec> {
        vec![
            MenuSpec::new("File").group("File"),
            MenuSpec::new("Edit").text_editing(),
            MenuSpec::new("View").group("View").submenu("Theme"),
            MenuSpec::new("Story").group("Play").group("Debug"),
        ]
    }

    fn titles(menus: &[Menu]) -> Vec<&str> {
        menus.iter().map(|m| m.name.as_ref()).collect()
    }

    /// Every action a menu tree dispatches, submenus included, in order.
    fn actions_of(items: &[MenuItem], out: &mut Vec<Box<dyn Action>>) {
        for item in items {
            match item {
                MenuItem::Action { action, .. } => out.push(action.boxed_clone()),
                MenuItem::Submenu(menu) => actions_of(&menu.items, out),
                MenuItem::Separator | MenuItem::SystemMenu(_) => {}
            }
        }
    }

    fn names(items: &[MenuItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Action { name, .. } => name.to_string(),
                MenuItem::Submenu(menu) => format!("{} \u{25B8}", menu.name),
                MenuItem::Separator => "-".to_owned(),
                MenuItem::SystemMenu(menu) => format!("({})", menu.name),
            })
            .collect()
    }

    #[test]
    fn the_mac_bar_has_the_app_and_window_menus_around_the_layout() {
        let r = registry();
        let menus = build(r.commands(), &layout(), MenuPlatform::Mac);
        assert_eq!(
            titles(&menus),
            [
                "brink", "File", "Edit", "View", "Story", "Unplaced", "Window"
            ],
            "a group nobody placed gets its own menu, before Window"
        );
        assert_eq!(
            names(&menus[0].items),
            [
                "About brink",
                "-",
                "Settings\u{2026}",
                "-",
                "(Services)",
                "-",
                "Hide brink",
                "Hide Others",
                "Show All",
                "-",
                "Quit brink",
            ]
        );
        assert_eq!(names(&menus[1].items), ["Save"], "Quit is the App menu's");
        assert_eq!(
            names(&menus[3].items),
            [
                "Code",
                "Single File",
                "Command Palette",
                "-",
                "Theme \u{25B8}"
            ]
        );
        assert_eq!(names(&menus[4].items), ["Play", "-", "Step"]);
    }

    #[test]
    fn off_the_mac_the_app_menu_dissolves_into_file_and_help() {
        let r = registry();
        let menus = build(r.commands(), &layout(), MenuPlatform::Other);
        assert_eq!(
            titles(&menus),
            ["File", "Edit", "View", "Story", "Unplaced", "Help"],
            "no menu is titled with the app's name, and no Window menu"
        );
        assert_eq!(
            names(&menus[0].items),
            ["Save", "-", "Settings\u{2026}", "-", "Quit"]
        );
        assert_eq!(names(&menus[5].items), ["About brink"]);
    }

    #[test]
    fn edit_leads_with_the_platform_block_and_an_empty_menu_still_shows_it() {
        let r = registry();
        let menus = build(r.commands(), &layout(), MenuPlatform::Mac);
        assert_eq!(
            names(&menus[2].items),
            ["Undo", "Redo", "-", "Cut", "Copy", "Paste", "Select All"]
        );
    }

    #[test]
    fn every_registered_command_is_in_the_bar_exactly_once() {
        let r = registry();
        for platform in [MenuPlatform::Mac, MenuPlatform::Other] {
            let menus = build(r.commands(), &layout(), platform);
            let mut found = Vec::new();
            for menu in &menus {
                actions_of(&menu.items, &mut found);
            }
            for command in r.commands() {
                let n = found
                    .iter()
                    .filter(|a| a.partial_eq(command.action.as_ref()))
                    .count();
                assert_eq!(n, 1, "{} on {platform:?}", command.full_title());
            }
            // Everything else is a standard item or the text block.
            let registered = |a: &dyn Action| r.commands().iter().any(|c| c.action.partial_eq(a));
            let text = text_editing_items();
            let mut text_actions = Vec::new();
            actions_of(&text, &mut text_actions);
            for action in &found {
                assert!(
                    registered(action.as_ref())
                        || is_standard(action.as_ref())
                        || text_actions.iter().any(|t| t.partial_eq(action.as_ref())),
                    "{} came from nowhere",
                    action.name()
                );
            }
        }
    }

    #[test]
    fn a_group_with_no_commands_leaves_no_trace() {
        let mut r = CommandRegistry::default();
        r.register("File", "Save", Save, None);
        let layout = vec![
            MenuSpec::new("File").group("File").submenu("Open Recent"),
            MenuSpec::new("Story").group("Play"),
        ];
        let menus = build(r.commands(), &layout, MenuPlatform::Mac);
        assert_eq!(titles(&menus), ["brink", "File", "Window"]);
        assert_eq!(names(&menus[1].items), ["Save"], "no empty submenu");
    }
}
