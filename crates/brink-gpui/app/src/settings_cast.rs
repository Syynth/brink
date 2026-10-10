//! Settings ▸ Cast (Project scope): the colour each speaker's lines are
//! drawn in, over `brink.toml`'s `[cast]` (decision log 2026-10-10).
//!
//! One row per declared speaker, as written in the file: a colour picker,
//! the name, the hex the file holds, and a remove button. Below them, a row
//! adds a speaker. A speaker the cast does not list keeps the Player's
//! automatic colour, so the list starts empty and holds only the speakers
//! the author cares about.
//!
//! The same text model as Settings ▸ General: `brink.toml` is one buffer in
//! the project, every edit goes through `ConfigDocument` (which changes the
//! one entry and keeps comments and key order), and the section re-reads
//! the text whenever it moves, whoever moved it.

use brink_gpui_shell::settings_modal::setting_group;
use brink_project_config::edit::{ConfigDocument, EditError};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, Hsla, IntoElement, Render, SharedString,
    Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Colorize as _, Sizable as _, h_flex, v_flex};

use crate::player::Player;
use crate::project::{Project, ProjectEvent};

/// A picked colour as `[cast]` stores it: lowercase `#rrggbb`.
#[must_use]
pub fn hex_of(colour: Hsla) -> String {
    // `to_hex` adds an alpha byte below full opacity; the cast has none.
    let opaque = Hsla { a: 1., ..colour };
    opaque.to_hex().to_ascii_lowercase()
}

/// The text with `name`'s colour set to `colour` (`None` when the file
/// already says exactly that).
pub fn with_cast_color(text: &str, name: &str, colour: &str) -> Result<Option<String>, EditError> {
    let mut doc = ConfigDocument::parse(text)?;
    doc.set_cast_color(name, colour)?;
    let next = doc.to_toml_string();
    Ok((next != text).then_some(next))
}

/// The text without `name` in the cast (`None` when it was not there).
pub fn without_cast_member(text: &str, name: &str) -> Result<Option<String>, EditError> {
    let mut doc = ConfigDocument::parse(text)?;
    Ok(doc.remove_cast_member(name)?.then(|| doc.to_toml_string()))
}

/// One declared speaker's row.
struct Row {
    name: String,
    /// The `color` as written, shown beside the picker; `None` when unset.
    written: Option<String>,
    picker: Entity<ColorPickerState>,
    _changes: Subscription,
}

pub struct CastSection {
    project: Entity<Project>,
    rows: Vec<Row>,
    new_name: Entity<InputState>,
    new_colour: Entity<ColorPickerState>,
    /// Why the text could not be read, which puts the section out of action.
    broken: Option<String>,
    /// Why the last add was refused, shown under the add row.
    refused: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl CastSection {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_name = cx.new(|cx| InputState::new(window, cx).placeholder("Speaker name"));
        let new_colour = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(Player::automatic_colour("", cx))
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &new_name,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::PressEnter { .. } => this.add(window, cx),
                        // The picker previews the colour the name gets today.
                        InputEvent::Change => {
                            let name = this.new_name.read(cx).value().to_string();
                            let automatic = Player::automatic_colour(name.trim(), cx);
                            this.new_colour.update(cx, |picker, cx| {
                                picker.set_value(automatic, window, cx);
                            });
                            this.refused = None;
                            cx.notify();
                        }
                        _ => {}
                    }
                },
            ),
            cx.subscribe_in(
                &project,
                window,
                |this, _, event: &ProjectEvent, window, cx| match event {
                    ProjectEvent::Opened { .. } | ProjectEvent::FilesChanged => {
                        this.sync(window, cx);
                    }
                    ProjectEvent::SourceChanged { path, .. }
                        if this.project.read(cx).is_config(path) =>
                    {
                        this.sync(window, cx);
                    }
                    _ => {}
                },
            ),
        ];
        let mut this = Self {
            project,
            rows: Vec::new(),
            new_name,
            new_colour,
            broken: None,
            refused: None,
            _subscriptions: subscriptions,
        };
        this.sync(window, cx);
        this
    }

    fn config_text(&self, cx: &App) -> Option<String> {
        let project = self.project.read(cx);
        let path = project.config_path()?;
        project.loaded_source(path).map(str::to_owned)
    }

    /// Re-read the cast from the text: rows follow the file, keeping each
    /// existing row's picker so an open picker is not torn down under the
    /// pointer.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.config_text(cx).unwrap_or_default();
        let members = match ConfigDocument::parse(&text) {
            Ok(doc) => {
                self.broken = None;
                doc.cast_members()
            }
            Err(e) => {
                self.broken = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let mut old = std::mem::take(&mut self.rows);
        for (name, written) in members {
            let colour = written
                .as_deref()
                .and_then(brink_project_config::normalize_color)
                .and_then(|hex| gpui::Rgba::try_from(hex.as_str()).ok())
                .map_or_else(|| Player::automatic_colour(&name, cx), Hsla::from);
            let row = match old.iter().position(|r| r.name == name) {
                Some(ix) => {
                    let mut row = old.swap_remove(ix);
                    row.picker.update(cx, |picker, cx| {
                        if picker.value() != Some(colour) {
                            picker.set_value(colour, window, cx);
                        }
                    });
                    row.written = written;
                    row
                }
                None => self.new_row(name, written, colour, window, cx),
            };
            self.rows.push(row);
        }
        cx.notify();
    }

    fn new_row(
        &mut self,
        name: String,
        written: Option<String>,
        colour: Hsla,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Row {
        let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(colour));
        let for_name = name.clone();
        let changes = cx.subscribe(&picker, move |this, _, event: &ColorPickerEvent, cx| {
            let ColorPickerEvent::Change(Some(colour)) = event else {
                return;
            };
            this.edit(cx, |text| {
                with_cast_color(text, &for_name, &hex_of(*colour))
            });
        });
        Row {
            name,
            written,
            picker,
            _changes: changes,
        }
    }

    /// Apply `change` to the config text through the shared buffer; every
    /// view of the file, this section included, follows.
    fn edit(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&str) -> Result<Option<String>, EditError>,
    ) {
        let Some(text) = self.config_text(cx) else {
            return;
        };
        match change(&text) {
            Ok(Some(next)) => self.project.update(cx, |project, cx| {
                if let Some(path) = project.config_path().map(str::to_owned) {
                    project.edit(&path, next, None, cx);
                }
            }),
            Ok(None) => {}
            Err(err) => eprintln!("brink.toml: {err}"),
        }
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.new_name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.refused = Some("Type the speaker's name as the script cues it.".to_owned());
            cx.notify();
            return;
        }
        if self
            .rows
            .iter()
            .any(|r| r.name.to_lowercase() == name.to_lowercase())
        {
            self.refused = Some(format!(
                "{name} is already in the cast — names match ignoring case."
            ));
            cx.notify();
            return;
        }
        let colour = self
            .new_colour
            .read(cx)
            .value()
            .unwrap_or_else(|| Player::automatic_colour(&name, cx));
        self.edit(cx, |text| with_cast_color(text, &name, &hex_of(colour)));
        self.refused = None;
        self.new_name
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn remove(&mut self, name: &str, cx: &mut Context<Self>) {
        self.edit(cx, |text| without_cast_member(text, name));
    }
}

impl Render for CastSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, danger, border, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.danger,
            theme.border,
            theme.mono_font_family.clone(),
        );
        if self.config_text(cx).is_none() {
            return v_flex()
                .w_full()
                .gap_2()
                .child(setting_group("Cast", cx))
                .child(crate::settings_config::no_config(
                    &self.project,
                    "the cast",
                    cx,
                ))
                .into_any_element();
        }
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                let name = row.name.clone();
                h_flex()
                    .w_full()
                    .py_1()
                    .gap_3()
                    .items_center()
                    .border_b_1()
                    .border_color(border.opacity(0.5))
                    .child(ColorPicker::new(&row.picker).small())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(fg)
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_xs()
                            .text_color(muted)
                            .child(SharedString::from(
                                row.written
                                    .clone()
                                    .unwrap_or_else(|| "no colour".to_owned()),
                            )),
                    )
                    .child(
                        Button::new(("cast-remove", ix))
                            .ghost()
                            .xsmall()
                            .label("\u{00d7}")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove(&name, cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let broken = self.broken.is_some();
        v_flex()
            .w_full()
            .gap_1()
            .child(setting_group("Cast", cx))
            .child(div().pb_1().text_xs().text_color(muted).child(
                "The colour each speaker's lines are drawn in, in the Player. Names match the \
                 script's cues ignoring case; a speaker not listed keeps an automatic colour.",
            ))
            .children(self.broken.clone().map(|reason| {
                div()
                    .pb_1()
                    .text_xs()
                    .text_color(danger)
                    .child(SharedString::from(format!(
                        "The cast is off until brink.toml parses: {reason}"
                    )))
            }))
            .when(!broken, |el| {
                el.children(rows).child(
                    h_flex()
                        .w_full()
                        .pt_2()
                        .gap_2()
                        .items_center()
                        .child(ColorPicker::new(&self.new_colour).small())
                        .child(div().flex_1().child(Input::new(&self.new_name).small()))
                        .child(
                            Button::new("cast-add")
                                .outline()
                                .small()
                                .label("Add")
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.add(window, cx);
                                })),
                        ),
                )
            })
            .children(
                self.refused
                    .clone()
                    .map(|why| div().text_xs().text_color(danger).child(why)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# My story\n[project]\nentry = \"main.ink\"\n";

    #[test]
    fn a_colour_is_set_once_and_a_member_removed_cleanly() {
        let set = with_cast_color(TEXT, "Mara", "#d97757")
            .expect("edit")
            .expect("changed");
        assert!(set.starts_with(TEXT), "{set}");
        assert!(set.contains("[cast.Mara]\ncolor = \"#d97757\""), "{set}");
        assert_eq!(
            with_cast_color(&set, "MARA", "#d97757").expect("edit"),
            None,
            "the same colour again writes nothing"
        );
        let removed = without_cast_member(&set, "mara")
            .expect("edit")
            .expect("changed");
        assert_eq!(removed, TEXT);
        assert_eq!(
            without_cast_member(TEXT, "Mara").expect("edit"),
            None,
            "nothing to remove"
        );
    }

    #[test]
    fn a_picked_colour_is_stored_as_opaque_lowercase_hex() {
        let picked = Hsla::from(gpui::Rgba::try_from("#D97757").expect("a colour"));
        assert_eq!(hex_of(picked), "#d97757");
        assert_eq!(hex_of(Hsla { a: 0.5, ..picked }), "#d97757", "no alpha");
        assert_eq!(
            brink_project_config::normalize_color(&hex_of(picked)).as_deref(),
            Some("#d97757"),
            "what the form writes, the parser reads"
        );
    }
}
