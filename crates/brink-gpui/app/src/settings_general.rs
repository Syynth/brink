//! Settings ▸ General (Project scope): the form over `brink.toml`.
//!
//! The web studio's General section (ruled 2026-08-27) carries the form
//! AND the raw text, because there `brink.toml` has no editor of its own.
//! The native studio differs by the maintainer's call (2026-09-05):
//! **`brink.toml` opens in Code view like any file**, and Settings holds
//! only the form — the `[project] entry` / `conventions` / `dialect` /
//! `types` selects, and the drafts list in the dictionary's shape (ruled
//! 2026-08-29, each glob reporting what it matched). Everything the form
//! does not model, `[lints]` and `[prose]` included, is edited in the file,
//! which the section opens on request.
//!
//! **One text, every view.** `brink.toml` is a file in the project's
//! shared buffer (`Project::config_path`): the form's edits and a Code
//! tab's keystrokes both go through `Project::edit`, and each hears the
//! other through `SourceChanged`. The worker re-applies the config on
//! every change, so a select here moves the Binder's entry mark and the
//! closure like a hand edit would — there is no second model to reconcile.
//!
//! **The form never rewrites the file.** Every structured edit goes through
//! `ConfigDocument` (`brink-project-config`'s `toml_edit` seam), which
//! changes the one key it was asked to and leaves every other byte —
//! comments, key order, quote style — alone. A `brink.toml` that does not
//! parse puts the form out of action, with the reason; fixing the file is
//! the way back.

use brink_gpui_model::worker::DraftGlob;
use brink_gpui_shell::settings_modal::{setting_group, setting_row};
use brink_project_config::edit::{ConfigDocument, EditError};
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString,
    Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::searchable_list::SearchableListItem;
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};

/// The section asks the app to open `brink.toml` (its path) in Code view.
#[derive(Clone, Debug)]
pub struct OpenConfig(pub String);

/// The `[project]` keys the form models. Everything else is the text's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Entry,
    Conventions,
    Dialect,
    Types,
}

impl Key {
    const ALL: [Key; 4] = [Key::Entry, Key::Conventions, Key::Dialect, Key::Types];

    fn name(self) -> &'static str {
        match self {
            Key::Entry => "entry",
            Key::Conventions => "conventions",
            Key::Dialect => "dialect",
            Key::Types => "types",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Key::Entry => "Entry file",
            Key::Conventions => "Conventions",
            Key::Dialect => "Dialect",
            Key::Types => "Types",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Key::Entry => "What the project compiles from.",
            Key::Conventions => "A .brink module of @[convention] handlers.",
            Key::Dialect => "Default: strict-ink.",
            Key::Types => "Default follows the dialect. Strict requires dialect = brink.",
        }
    }
}

/// The value "(not set)" carries: the key is removed from the file.
const UNSET: &str = "";

/// One row of a select: what the file gets, and what the author sees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opt {
    pub value: String,
    pub label: SharedString,
}

impl Opt {
    fn new(value: impl Into<String>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

impl SearchableListItem for Opt {
    type Value = String;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &String {
        &self.value
    }
}

/// The form's read of the text: each modelled key as written, or why the
/// text could not be read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub entry: Option<String>,
    pub conventions: Option<String>,
    pub dialect: Option<String>,
    pub types: Option<String>,
}

impl Form {
    fn get(&self, key: Key) -> Option<&str> {
        match key {
            Key::Entry => self.entry.as_deref(),
            Key::Conventions => self.conventions.as_deref(),
            Key::Dialect => self.dialect.as_deref(),
            Key::Types => self.types.as_deref(),
        }
    }
}

/// Read the four modelled keys out of `text`, as written — no validation:
/// a value the compiler would reject still shows, marked, rather than
/// being silently re-read as unset.
pub fn read_form(text: &str) -> Result<Form, String> {
    let doc = ConfigDocument::parse(text).map_err(|e| e.to_string())?;
    Ok(Form {
        entry: doc.string("project", "entry"),
        conventions: doc.string("project", "conventions"),
        dialect: doc.string("project", "dialect"),
        types: doc.string("project", "types"),
    })
}

/// Set (or, for `None`, remove) `[project] key`, changing nothing else.
pub fn with_key(text: &str, key: Key, value: Option<&str>) -> Result<String, EditError> {
    let mut doc = ConfigDocument::parse(text)?;
    match value {
        Some(value) => doc.set_string("project", key.name(), value)?,
        None => {
            doc.remove_key("project", key.name())?;
        }
    }
    Ok(doc.to_toml_string())
}

/// Add or remove one `[project] drafts` glob. `None` when nothing changed.
pub fn with_draft(text: &str, glob: &str, add: bool) -> Result<Option<String>, EditError> {
    let mut doc = ConfigDocument::parse(text)?;
    let changed = if add {
        doc.add_to_string_array("project", "drafts", glob)?
    } else {
        doc.remove_from_string_array("project", "drafts", glob)?
    };
    Ok(changed.then(|| doc.to_toml_string()))
}

/// The options for a file-backed key: "(not set)", then the candidate
/// files, then — kept, and flagged — a configured value naming a file the
/// project does not have. A typo'd entry is #3010 exactly, which is why the
/// form offers files instead of free text; keeping the bad value visible is
/// what stops it being silently rewritten.
pub fn file_options(files: &[String], current: Option<&str>) -> Vec<Opt> {
    let mut opts = vec![Opt::new(UNSET, "(not set)")];
    opts.extend(files.iter().map(|f| Opt::new(f.clone(), f.clone())));
    if let Some(current) = current
        && !current.is_empty()
        && !files.iter().any(|f| f == current)
    {
        opts.push(Opt::new(current, format!("{current} (missing)")));
    }
    opts
}

/// The options for a closed-set key, with an out-of-set current value kept
/// and flagged the same way.
pub fn fixed_options(values: &[&str], current: Option<&str>) -> Vec<Opt> {
    let mut opts = vec![Opt::new(UNSET, "(not set)")];
    opts.extend(values.iter().map(|v| Opt::new(*v, *v)));
    if let Some(current) = current
        && !current.is_empty()
        && !values.contains(&current)
    {
        opts.push(Opt::new(current, format!("{current} (unknown)")));
    }
    opts
}

/// What a draft glob's row says beside the pattern (ruled 2026-08-29): the
/// three states, and the fourth before anything is known.
pub fn glob_summary(glob: &DraftGlob, known: bool) -> String {
    if !known {
        return "not checked yet".to_owned();
    }
    match glob.drafts.len() {
        0 if glob.in_story.is_empty() => "matches nothing".to_owned(),
        0 => "no drafts".to_owned(),
        1 => "1 draft".to_owned(),
        n => format!("{n} drafts"),
    }
}

/// "Also matches a file the story reaches, so it is not a draft: x" — the
/// sentence the studio prints, or nothing.
pub fn in_story_note(glob: &DraftGlob) -> Option<String> {
    match glob.in_story.as_slice() {
        [] => None,
        [one] => Some(format!(
            "Also matches a file the story reaches, so it is not a draft: {one}"
        )),
        many => Some(format!(
            "Also matches {} files the story reaches, so they are not drafts: {}",
            many.len(),
            many.join(", ")
        )),
    }
}

pub struct GeneralSection {
    project: Entity<Project>,
    selects: [Entity<SelectState<Vec<Opt>>>; 4],
    draft_input: Entity<InputState>,
    form: Result<Form, String>,
    /// True while the selects are being set from the text, whose Confirm
    /// events are echoes, not edits.
    syncing: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<OpenConfig> for GeneralSection {}

impl GeneralSection {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let selects = Key::ALL.map(|_| cx.new(|cx| SelectState::new(Vec::new(), None, window, cx)));
        let draft_input = cx.new(|cx| InputState::new(window, cx).placeholder("notes/**"));

        let mut subscriptions = Vec::new();
        for (ix, key) in Key::ALL.iter().enumerate() {
            let key = *key;
            subscriptions.push(cx.subscribe_in(
                &selects[ix],
                window,
                move |this, _, event: &SelectEvent<Vec<Opt>>, window, cx| {
                    if this.syncing {
                        return;
                    }
                    let SelectEvent::Confirm(value) = event;
                    let value = value.as_deref().filter(|v| !v.is_empty());
                    this.apply_key(key, value, window, cx);
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &draft_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.add_draft(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &project,
            window,
            |this, _, event: &ProjectEvent, window, cx| match event {
                ProjectEvent::Opened { .. } => this.sync_form(window, cx),
                // The drafts report moves with the analysis.
                ProjectEvent::Analyzed => cx.notify(),
                // The file moved under us — a Code tab, or our own write
                // coming back — and the form reads the file.
                ProjectEvent::SourceChanged { path, .. }
                    if this.project.read(cx).is_config(path) =>
                {
                    this.sync_form(window, cx);
                }
                _ => {}
            },
        ));

        let mut this = Self {
            project,
            selects,
            draft_input,
            form: Ok(Form::default()),
            syncing: false,
            _subscriptions: subscriptions,
        };
        this.sync_form(window, cx);
        this
    }

    fn config_text(&self, cx: &Context<Self>) -> Option<(String, String)> {
        let project = self.project.read(cx);
        let path = project.config_path()?.to_owned();
        let text = project.loaded_source(&path)?.to_owned();
        Some((path, text))
    }

    /// Re-read the form from the text and set every select to match.
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (form, files) = {
            let project = self.project.read(cx);
            let text = self.config_text(cx).map(|(_, t)| t).unwrap_or_default();
            (read_form(&text), project.files().to_vec())
        };
        let brink_files: Vec<String> = files
            .iter()
            .filter(|f| f.ends_with(".brink"))
            .cloned()
            .collect();
        self.syncing = true;
        for (ix, key) in Key::ALL.iter().enumerate() {
            let current = form.as_ref().ok().and_then(|f| f.get(*key));
            let options = match key {
                Key::Entry => file_options(&files, current),
                Key::Conventions => file_options(&brink_files, current),
                Key::Dialect => fixed_options(&["strict-ink", "brink"], current),
                Key::Types => fixed_options(&["gradual", "strict"], current),
            };
            let selected = current.unwrap_or(UNSET).to_owned();
            self.selects[ix].update(cx, |select, cx| {
                select.set_items(options, window, cx);
                select.set_selected_value(&selected, window, cx);
            });
        }
        self.syncing = false;
        self.form = form;
        cx.notify();
    }

    /// Write `text` as the config through the shared buffer. Every editor
    /// over the file, and this form, follow through `SourceChanged`.
    fn write(&mut self, text: String, cx: &mut Context<Self>) {
        self.project.update(cx, |project, cx| {
            if let Some(path) = project.config_path().map(str::to_owned) {
                project.edit(&path, text, None, cx);
            }
        });
    }

    fn apply_key(
        &mut self,
        key: Key,
        value: Option<&str>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, text)) = self.config_text(cx) else {
            return;
        };
        match with_key(&text, key, value) {
            Ok(next) if next != text => self.write(next, cx),
            Ok(_) => {}
            Err(err) => eprintln!("brink.toml: {err}"),
        }
    }

    fn add_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let glob = self.draft_input.read(cx).value().trim().to_string();
        if glob.is_empty() {
            return;
        }
        let Some((_, text)) = self.config_text(cx) else {
            return;
        };
        match with_draft(&text, &glob, true) {
            Ok(Some(next)) => {
                self.write(next, cx);
                self.draft_input.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                });
            }
            Ok(None) => {}
            Err(err) => eprintln!("brink.toml: {err}"),
        }
    }

    fn remove_draft(&mut self, glob: &str, cx: &mut Context<Self>) {
        let Some((_, text)) = self.config_text(cx) else {
            return;
        };
        match with_draft(&text, glob, false) {
            Ok(Some(next)) => self.write(next, cx),
            Ok(None) => {}
            Err(err) => eprintln!("brink.toml: {err}"),
        }
    }

    fn render_drafts(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, warning, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.warning,
            theme.mono_font_family.clone(),
        );
        let (globs, known) = {
            let (globs, known) = self.project.read(cx).draft_globs();
            (globs.to_vec(), known)
        };
        let rows: Vec<AnyElement> = globs
            .iter()
            .enumerate()
            .map(|(ix, glob)| {
                let empty = known && glob.drafts.is_empty() && glob.in_story.is_empty();
                let pattern = glob.glob.clone();
                v_flex()
                    .w_full()
                    .py_1()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(border.opacity(0.5))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_center()
                            .child(
                                div()
                                    .font_family(mono.clone())
                                    .text_sm()
                                    .text_color(fg)
                                    .child(glob.glob.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(if empty { warning } else { muted })
                                    .child(glob_summary(glob, known)),
                            )
                            .child(
                                Button::new(("draft-remove", ix))
                                    .ghost()
                                    .xsmall()
                                    .label("\u{00d7}")
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.remove_draft(&pattern, cx);
                                    })),
                            ),
                    )
                    .when(!glob.drafts.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(glob.drafts.join(", ")),
                        )
                    })
                    .children(
                        in_story_note(glob)
                            .map(|note| div().text_xs().text_color(warning).child(note)),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .w_full()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Globs naming work in progress. A match is a draft only if nothing includes it; a file the story still reaches is never a draft."),
            )
            .children(rows)
            .child(
                h_flex()
                    .w_full()
                    .pt_1()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(Input::new(&self.draft_input).small()))
                    .child(
                        Button::new("draft-add")
                            .outline()
                            .small()
                            .label("Add")
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.add_draft(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for GeneralSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let Some((path, _)) = self.config_text(cx) else {
            return v_flex()
                .w_full()
                .gap_2()
                .child(setting_group("Project", cx))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("This project has no brink.toml, so there is nothing to configure here. Add one beside the entry file and reopen the project."),
                )
                .into_any_element();
        };
        let broken = self.form.as_ref().err().cloned();
        let rows: Vec<AnyElement> = Key::ALL
            .iter()
            .enumerate()
            .map(|(ix, key)| {
                setting_row(
                    key.title(),
                    key.hint(),
                    Select::new(&self.selects[ix])
                        .small()
                        .w(px(260.))
                        .placeholder("(not set)")
                        .disabled(broken.is_some()),
                    cx,
                )
                .into_any_element()
            })
            .collect();
        v_flex()
            .w_full()
            .gap_1()
            .child(setting_group("Project", cx))
            .child(
                h_flex()
                    .w_full()
                    .pb_1()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .child(SharedString::from(format!(
                                "Written to {path}, keeping your comments. Anything the form doesn't cover — [lints], [prose] — is edited in the file itself."
                            ))),
                    )
                    .child(
                        Button::new("open-config")
                            .outline()
                            .xsmall()
                            .label(SharedString::from(format!("Open {path}")))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if let Some((path, _)) = this.config_text(cx) {
                                    cx.emit(OpenConfig(path));
                                }
                            })),
                    ),
            )
            .children(broken.map(|reason| {
                div()
                    .pb_1()
                    .text_xs()
                    .text_color(danger)
                    .child(SharedString::from(format!(
                        "The form is off until the text parses: {reason}"
                    )))
            }))
            .children(rows)
            .child(setting_group("Drafts", cx))
            .child(self.render_drafts(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# My story\n[project]\n# the entry\nentry = \"main.ink\"\ndialect = \"strict-ink\"\n\n[lints]\nE063 = \"allow\"\n";

    #[test]
    fn the_form_reads_what_is_written_and_only_that() {
        let form = read_form(TEXT).expect("valid");
        assert_eq!(form.entry.as_deref(), Some("main.ink"));
        assert_eq!(form.dialect.as_deref(), Some("strict-ink"));
        assert_eq!(form.conventions, None);
        assert_eq!(form.types, None);
        assert!(
            read_form("[project\n").is_err(),
            "a broken text is a reason, not a blank form"
        );
    }

    #[test]
    fn a_key_edit_changes_one_line_and_keeps_every_comment() {
        let set = with_key(TEXT, Key::Entry, Some("chapter1.ink")).expect("edit");
        assert_eq!(
            set,
            TEXT.replace("entry = \"main.ink\"", "entry = \"chapter1.ink\""),
            "one line moved, the comments and [lints] did not"
        );
        let added = with_key(TEXT, Key::Conventions, Some("screenplay.brink")).expect("edit");
        assert!(added.contains("conventions = \"screenplay.brink\""));
        assert!(added.contains("# the entry"));
        let removed = with_key(TEXT, Key::Dialect, None).expect("edit");
        assert!(!removed.contains("dialect"));
        assert!(removed.contains("entry = \"main.ink\""));
        assert_eq!(
            with_key(TEXT, Key::Types, None).expect("edit"),
            TEXT,
            "removing an absent key changes nothing"
        );
    }

    #[test]
    fn drafts_are_added_once_and_removed_cleanly() {
        let added = with_draft(TEXT, "notes/**", true)
            .expect("edit")
            .expect("changed");
        assert!(added.contains("drafts = [\"notes/**\"]"), "{added}");
        assert_eq!(
            with_draft(&added, "notes/**", true).expect("edit"),
            None,
            "already there"
        );
        let removed = with_draft(&added, "notes/**", false)
            .expect("edit")
            .expect("changed");
        assert!(removed.contains("drafts = []"), "{removed}");
        assert_eq!(with_draft(TEXT, "x", false).expect("edit"), None);
    }

    #[test]
    fn options_keep_a_configured_value_the_project_lacks_and_say_so() {
        let files = vec!["a.ink".to_owned(), "b.brink".to_owned()];
        let opts = file_options(&files, Some("gone.ink"));
        assert_eq!(opts[0].value, UNSET);
        assert_eq!(opts[0].label.as_ref(), "(not set)");
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[3].value, "gone.ink");
        assert_eq!(opts[3].label.as_ref(), "gone.ink (missing)");
        assert_eq!(
            file_options(&files, Some("a.ink")).len(),
            3,
            "a present value is not doubled"
        );
        let fixed = fixed_options(&["strict-ink", "brink"], Some("sideways"));
        assert_eq!(fixed[3].label.as_ref(), "sideways (unknown)");
        assert_eq!(fixed_options(&["gradual", "strict"], None).len(), 3);
    }

    #[test]
    fn a_glob_row_names_its_state() {
        let glob = |drafts: &[&str], in_story: &[&str]| DraftGlob {
            glob: "g".to_owned(),
            drafts: drafts.iter().map(|s| (*s).to_owned()).collect(),
            in_story: in_story.iter().map(|s| (*s).to_owned()).collect(),
        };
        assert_eq!(glob_summary(&glob(&[], &[]), false), "not checked yet");
        assert_eq!(glob_summary(&glob(&[], &[]), true), "matches nothing");
        assert_eq!(glob_summary(&glob(&["a"], &[]), true), "1 draft");
        assert_eq!(glob_summary(&glob(&["a", "b"], &[]), true), "2 drafts");
        assert_eq!(glob_summary(&glob(&[], &["a"]), true), "no drafts");
        assert_eq!(in_story_note(&glob(&[], &[])), None);
        assert_eq!(
            in_story_note(&glob(&[], &["a.ink"])).as_deref(),
            Some("Also matches a file the story reaches, so it is not a draft: a.ink")
        );
        assert_eq!(
            in_story_note(&glob(&[], &["a.ink", "b.ink"])).as_deref(),
            Some("Also matches 2 files the story reaches, so they are not drafts: a.ink, b.ink")
        );
    }
}
