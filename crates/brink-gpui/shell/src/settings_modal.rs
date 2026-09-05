//! The Settings window — the studio's modal (ruled 2026-08-27: a
//! searchable section rail on the left, ONE section at a time on the
//! right, an App / Project scope switch at the top of the rail).
//!
//! Sections are **registered entries** (id, scope, title, keywords, view),
//! not a hand-laid-out page, so the window cannot drift behind what is
//! actually configurable: a feature that gains a setting registers a
//! section, and the rail lists it. The shell registers the App sections it
//! owns — Appearance (`crate::settings_appearance`) and Keymap
//! (`crate::settings_keymap`); a Project section (writing `brink.toml`) is
//! the feature crate's to register when its edit seam exists.
//!
//! Where a setting is written changes what changing it means: Project
//! settings are versioned and shared with everyone who opens the project;
//! App settings are this machine's and follow the author between
//! projects. The scope switch says which; the SECTION decides the scope
//! shown, so opening a section never shows a rail it is not in.
//!
//! Search reaches across both scopes: an author looking for "theme" should
//! not have to know it is an app setting first. A match from the other
//! scope carries its scope as a label. The rail filters; it does not jump.

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, KeyDownEvent, Render, SharedString, Subscription, Window, div, px,
};
use gpui_component::button::Button;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

/// Which store a section writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `brink.toml`: versioned, shared with everyone who opens the project.
    Project,
    /// This machine's settings, following the author between projects.
    App,
}

impl Scope {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::App => "App",
        }
    }

    #[must_use]
    pub fn hint(self) -> &'static str {
        match self {
            Self::Project => "Written to brink.toml — versioned, shared with the project",
            Self::App => "This machine's settings — yours, across projects",
        }
    }
}

/// What the rail knows about a section — the part search runs over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionMeta {
    pub id: SharedString,
    pub scope: Scope,
    pub title: SharedString,
    /// What search matches besides the title — "todo" reaches Diagnostics,
    /// "theme" reaches Appearance.
    pub keywords: Vec<SharedString>,
}

impl SectionMeta {
    pub fn new(
        id: impl Into<SharedString>,
        scope: Scope,
        title: impl Into<SharedString>,
        keywords: &[&str],
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            title: title.into(),
            keywords: keywords
                .iter()
                .map(|k| SharedString::from((*k).to_owned()))
                .collect(),
        }
    }

    fn matches(&self, needle: &str) -> bool {
        self.title.to_lowercase().contains(needle)
            || self
                .keywords
                .iter()
                .any(|k| k.to_lowercase().contains(needle))
    }
}

/// A registered section: what the rail knows, and the view the pane shows.
#[derive(Clone)]
pub struct Section {
    pub meta: SectionMeta,
    pub view: AnyView,
}

impl Section {
    pub fn new(meta: SectionMeta, view: impl Into<AnyView>) -> Self {
        Self {
            meta,
            view: view.into(),
        }
    }
}

/// The rail after the search: the current scope's sections unsearched;
/// every match across both scopes while searching.
#[must_use]
pub fn rail_entries(sections: &[SectionMeta], scope: Scope, query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    sections
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            if needle.is_empty() {
                s.scope == scope
            } else {
                s.matches(&needle)
            }
        })
        .map(|(ix, _)| ix)
        .collect()
}

pub enum SettingsEvent {
    Close,
}

pub struct SettingsModal {
    sections: Vec<Section>,
    active: usize,
    search: Entity<InputState>,
    query: String,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsModal {}

pub const MODAL_WIDTH: f32 = 920.;
pub const MODAL_HEIGHT: f32 = 640.;
const RAIL_WIDTH: f32 = 196.;

impl SettingsModal {
    /// Open on `section` (an id), or the first section when the id is not
    /// registered — a caller naming a section that was removed falls back
    /// rather than showing an empty pane.
    pub fn new(
        sections: Vec<Section>,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let on_search = cx.subscribe(&search, |this: &mut Self, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = state.read(cx).value().to_string();
                cx.notify();
            }
        });
        let active = section
            .and_then(|id| sections.iter().position(|s| s.meta.id.as_ref() == id))
            .unwrap_or(0);
        Self {
            sections,
            active,
            search,
            query: String::new(),
            focus: cx.focus_handle(),
            _subscriptions: vec![on_search],
        }
    }

    /// The search box takes the keyboard when the window opens.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |state, cx| state.focus(window, cx));
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.sections.len() {
            self.active = ix;
            cx.notify();
        }
    }

    /// Switching scope is a navigation: land on that scope's first
    /// section, since leaving the old one showing would make the switch
    /// look broken.
    fn select_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if let Some(first) = self.sections.iter().position(|s| s.meta.scope == scope) {
            self.select(first, cx);
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(SettingsEvent::Close);
            cx.stop_propagation();
        }
    }

    fn render_scopes(&self, shown: Scope, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (accent, on_accent, muted, trough) = (
            theme.primary,
            theme.primary_foreground,
            theme.muted_foreground,
            theme.border.opacity(0.45),
        );
        h_flex()
            .w_full()
            .p(px(2.))
            .gap(px(2.))
            .rounded_md()
            .bg(trough)
            .children([Scope::Project, Scope::App].into_iter().map(|scope| {
                let on = scope == shown;
                div()
                    .id(SharedString::from(format!(
                        "settings-scope-{}",
                        scope.label()
                    )))
                    .flex_1()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_xs()
                    .when(on, |el| {
                        el.bg(accent)
                            .text_color(on_accent)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .when(!on, |el| el.text_color(muted))
                    .child(scope.label())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_scope(scope, cx);
                    }))
            }))
            .into_any_element()
    }

    fn render_rail(&self, shown: Scope, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, pane, border) = (
            theme.foreground,
            theme.muted_foreground,
            theme.popover,
            theme.border,
        );
        let metas: Vec<SectionMeta> = self.sections.iter().map(|s| s.meta.clone()).collect();
        let entries = rail_entries(&metas, shown, &self.query);
        let searching = !self.query.trim().is_empty();
        let rows: Vec<AnyElement> = entries
            .iter()
            .map(|&ix| {
                let section = &self.sections[ix].meta;
                let active = ix == self.active;
                h_flex()
                    .id(("settings-nav", ix))
                    .w_full()
                    .h(px(26.))
                    .px_2()
                    .gap_2()
                    .items_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(if active { fg } else { muted })
                    .when(active, |el| el.bg(pane))
                    .child(div().flex_1().child(section.title.clone()))
                    // A result from the other scope says which one it is.
                    .when(searching && section.scope != shown, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(section.scope.label()),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select(ix, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .w(px(RAIL_WIDTH))
            .flex_shrink_0()
            .h_full()
            .gap_2()
            .px_2()
            .py(px(10.))
            .border_r_1()
            .border_color(border)
            .child(self.render_scopes(shown, cx))
            .child(Input::new(&self.search).small())
            .child(
                v_flex()
                    .id("settings-nav")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(rows)
                    .when(entries.is_empty(), |el| {
                        el.child(
                            div()
                                .px_2()
                                .text_xs()
                                .text_color(muted)
                                .child("No settings match."),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Focusable for SettingsModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (surface, pane, border, fg, muted) = (
            theme.sidebar,
            theme.popover,
            theme.border,
            theme.foreground,
            theme.muted_foreground,
        );
        let viewport = window.viewport_size();
        // Bounded on both axes: big enough for a lint table, never so big
        // it stops reading as a thing over the app.
        let width = px(MODAL_WIDTH).min(viewport.width - px(64.));
        let height = px(MODAL_HEIGHT).min(viewport.height - px(80.));
        let Some(active) = self.sections.get(self.active).cloned() else {
            return div().into_any_element();
        };
        let shown = active.meta.scope;
        let rail = self.render_rail(shown, cx);
        h_flex()
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)))
            .w(width)
            .h(height)
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(surface)
            .shadow_lg()
            .overflow_hidden()
            .child(rail)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(pane)
                    .child(
                        h_flex()
                            .h(px(34.))
                            .flex_shrink_0()
                            .pl_4()
                            .pr_3()
                            .items_center()
                            .gap_2()
                            .border_b_1()
                            .border_color(border)
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(fg)
                                    .child(active.meta.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(active.meta.scope.hint()),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_4()
                            .child(active.view.clone()),
                    ),
            )
            .into_any_element()
    }
}

/// A setting's row: the title and a description on the left, the control
/// on the right — the studio's `SettingsRow`.
pub fn setting_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .py_2()
        .gap_4()
        .items_center()
        .justify_between()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(description.into()),
                ),
        )
        .child(div().flex_shrink_0().child(control))
}

/// `−  14 px  +`: a stepper over a number, `suffix` after it. `on_change`
/// gets the stepped value; clamping is the caller's.
pub fn setting_stepper(
    id: &'static str,
    value: f32,
    suffix: &'static str,
    on_change: impl Fn(f32, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> AnyElement {
    let fg = cx.theme().foreground;
    let dec = on_change.clone();
    h_flex()
        .gap_1()
        .items_center()
        .child(
            Button::new(SharedString::from(format!("{id}-dec")))
                .outline()
                .xsmall()
                .label("\u{2212}")
                .on_click(move |_, window, cx| dec(value - 1., window, cx)),
        )
        .child(
            div()
                .min_w(px(44.))
                .text_center()
                .text_sm()
                .text_color(fg)
                .child(format!("{value:.0}{suffix}")),
        )
        .child(
            Button::new(SharedString::from(format!("{id}-inc")))
                .outline()
                .xsmall()
                .label("+")
                .on_click(move |_, window, cx| on_change(value + 1., window, cx)),
        )
        .into_any_element()
}

/// A subordinate group heading inside a section — the pane header is the
/// only title; sections use these.
pub fn setting_group(title: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.into().to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metas() -> Vec<SectionMeta> {
        vec![
            SectionMeta::new(
                "general",
                Scope::Project,
                "General",
                &["entry", "brink.toml"],
            ),
            SectionMeta::new(
                "diagnostics",
                Scope::Project,
                "Diagnostics",
                &["lint", "todo"],
            ),
            SectionMeta::new("appearance", Scope::App, "Appearance", &["theme", "font"]),
            SectionMeta::new("keymap", Scope::App, "Keymap", &["keys", "shortcut"]),
        ]
    }

    #[test]
    fn the_rail_is_the_scope_unsearched_and_both_scopes_searched() {
        let sections = metas();
        assert_eq!(rail_entries(&sections, Scope::App, ""), vec![2, 3]);
        assert_eq!(rail_entries(&sections, Scope::Project, "  "), vec![0, 1]);
        // "todo" reaches Diagnostics by keyword; "theme" reaches Appearance
        // from the Project scope.
        assert_eq!(rail_entries(&sections, Scope::App, "todo"), vec![1]);
        assert_eq!(rail_entries(&sections, Scope::Project, "THEME"), vec![2]);
        assert_eq!(
            rail_entries(&sections, Scope::App, "nothing"),
            Vec::<usize>::new()
        );
    }
}
