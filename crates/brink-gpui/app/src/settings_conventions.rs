//! Settings ▸ Conventions (Project scope): the teach-by-example editor
//! (#3411; RULED 2026-09-02, "Conventions editor: teach-by-example is the
//! design direction").
//!
//! The author points at a passage (the knot/stitch picker — ruled
//! 2026-09-02, "sample lines come from a knot/stitch selector" — or pasted
//! lines), marks what each line is, and the studio shows back what it
//! learned — plain sentences with the lines that support each — plus what
//! it could not settle. Nothing is written until "Use these rules"; the
//! write goes through the `[dialogue]` section road (#3410), which asks
//! before replacing a section it did not write.
//!
//! The inference, the parsers it verifies against and the section writer
//! are `brink_ide::dialect_infer` and `brink_ide::dialogue_section` — the
//! same code, held to the same corpus, the web studio's TypeScript mirrors.
//! Choice text is hidden by default and never taught from while hidden;
//! the section is stacked in working order: the passage, the marks, what
//! was learned, the Player preview (ruled 2026-09-02).

use std::collections::{BTreeMap, BTreeSet};

use brink_gpui_model::query::{PassageLine, PassageSymbol, QueryKind, QueryResult};
use brink_gpui_shell::settings_modal::{setting_group, setting_row};
use brink_ide::dialect_infer::{
    EmittedLine, EmittedParser, Inference, Mark, MarkedLine, Origin, infer_dialect, runs_of,
    to_dialogue_config,
};
use brink_ide::dialogue_section::{
    DialogueSection, DialogueSpec, SectionOwner, find_dialogue_section, render_dialogue_section,
    set_dialogue_section,
};
use brink_ide::passage::PassageOrigin;
use brink_ir::DialogueDialect;
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Focusable as _, FontWeight, Hsla, IntoElement,
    MouseButton, MouseDownEvent, Render, SharedString, Subscription, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};
use crate::settings_config::{config_text, no_config};

const ARTIFACT_FILE: &str = "dialect.json";
const GLUE: &str = "<>";

/// The marks, in the order the buttons show them.
const MARKS: [(Mark, &str); 5] = [
    (Mark::Cue, "Cue"),
    (Mark::Dialogue, "Dialogue"),
    (Mark::Action, "Action"),
    (Mark::Narration, "Narration"),
    (Mark::Parenthetical, "Aside"),
];

/// The lines on the table, and where they came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub label: String,
    pub lines: Vec<PassageLine>,
}

/// The visible line indices: choice text only when asked for. Marks are
/// keyed by the passage index, so toggling the choices never shuffles them.
#[must_use]
pub fn visible_lines(passage: &Passage, include_choices: bool) -> Vec<usize> {
    passage
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| include_choices || l.origin != PassageOrigin::Choice)
        .map(|(i, _)| i)
        .collect()
}

/// The visible lines as the inference takes them, marks attached.
#[must_use]
pub fn marked_lines(
    passage: &Passage,
    visible: &[usize],
    marks: &BTreeMap<usize, Mark>,
) -> Vec<MarkedLine> {
    visible
        .iter()
        .map(|&i| {
            let l = &passage.lines[i];
            MarkedLine {
                text: l.text.clone(),
                tags: l.tags.clone(),
                origin: match l.origin {
                    PassageOrigin::Line => Origin::Line,
                    PassageOrigin::Choice => Origin::Choice,
                    PassageOrigin::Gather => Origin::Gather,
                },
                mark: marks.get(&i).copied(),
            }
        })
        .collect()
}

/// What the Player would see: a line ending in glue (`<>`) joins the next
/// one. Without this a glued cue shows up as its own `<>` row under the
/// speaker header.
#[must_use]
pub fn fold_glue(lines: &[&PassageLine]) -> Vec<String> {
    let mut out = Vec::new();
    let mut carry: Option<String> = None;
    for l in lines {
        let text = match carry.take() {
            Some(c) => format!("{c}{}", l.text),
            None => l.text.clone(),
        };
        if let Some(stripped) = text.strip_suffix(GLUE) {
            carry = Some(stripped.to_owned());
            continue;
        }
        out.push(text);
    }
    if let Some(c) = carry {
        out.push(c);
    }
    out
}

/// One run the preview draws: the speaker (if any) and its rows, each row
/// the segments the dialect read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewGroup {
    pub kind: Option<String>,
    pub speaker: Option<String>,
    pub rows: Vec<Vec<brink_ide::dialect_infer::EmittedSegment>>,
}

/// Fold emitted lines into groups under `dialect`; no dialect means every
/// line is its own plain row.
#[must_use]
pub fn preview_groups(lines: &[String], dialect: Option<&DialogueDialect>) -> Vec<PreviewGroup> {
    let plain = |lines: &[String]| -> Vec<PreviewGroup> {
        lines
            .iter()
            .map(|t| PreviewGroup {
                kind: None,
                speaker: None,
                rows: vec![vec![brink_ide::dialect_infer::EmittedSegment {
                    kind: None,
                    text: t.clone(),
                    content: None,
                }]],
            })
            .collect()
    };
    let Some(dialect) = dialect else {
        return plain(lines);
    };
    let Ok(parser) = EmittedParser::compile(dialect) else {
        return plain(lines);
    };
    let emitted: Vec<EmittedLine> = lines
        .iter()
        .map(|t| EmittedLine {
            segments: parser.parse_emitted(t),
            boundary: false,
        })
        .collect();
    runs_of(&emitted, dialect)
        .into_iter()
        .map(|run| PreviewGroup {
            kind: run.kind,
            speaker: run.attrs.get("speaker").cloned(),
            rows: run
                .lines
                .iter()
                .map(|&i| emitted[i].segments.clone())
                .collect(),
        })
        .collect()
}

/// A stable palette slot for a speaker name.
#[must_use]
pub fn speaker_slot(name: &str, size: usize) -> usize {
    let hash = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    usize::try_from(hash).unwrap_or(0) % size.max(1)
}

/// The "Current conventions" line.
#[must_use]
pub fn describe_current(dialect: Option<&DialogueDialect>) -> String {
    match dialect {
        None => "None \u{2014} lines print as plain text.".to_owned(),
        Some(d) => format!(
            "{} \u{2014} {}",
            if d.name.is_empty() {
                "project"
            } else {
                &d.name
            },
            d.elements
                .iter()
                .map(|e| e.kind.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub struct ConventionsSection {
    project: Entity<Project>,
    symbols: Vec<PassageSymbol>,
    picker: Entity<InputState>,
    query: String,
    /// Clicking into the field lists every knot and stitch before you
    /// type (ruled 2026-09-02); typing narrows.
    list_open: bool,
    paste: Entity<EditorState>,
    pasting: bool,
    passage: Option<Passage>,
    marks: BTreeMap<usize, Mark>,
    include_choices: bool,
    replace_ask: Option<DialogueSection>,
    status: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ConventionsSection {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Pull lines from a knot or stitch\u{2026}")
        });
        let paste = cx.new(|cx| EditorState::new(window, cx).line_number(false));
        let on_picker = cx.subscribe_in(
            &picker,
            window,
            |this: &mut Self, state, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.query = state.read(cx).value().to_string();
                    cx.notify();
                }
                InputEvent::Focus => {
                    this.list_open = true;
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.list_open = false;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    let focused = state.read(cx).focus_handle(cx).is_focused(window);
                    if let Some(first) = this.matches(focused).first().cloned() {
                        this.pick(&first, window, cx);
                    }
                }
            },
        );
        let on_project = cx.subscribe(&project, |this, _, event: &ProjectEvent, cx| {
            match event {
                ProjectEvent::Opened { .. } => {
                    this.symbols.clear();
                    this.passage = None;
                    this.marks.clear();
                    this.status = None;
                }
                ProjectEvent::Analyzed => this.refresh_symbols(cx),
                _ => {}
            }
            cx.notify();
        });
        let mut this = Self {
            project,
            symbols: Vec::new(),
            picker,
            query: String::new(),
            list_open: false,
            paste,
            pasting: false,
            passage: None,
            marks: BTreeMap::new(),
            include_choices: false,
            replace_ask: None,
            status: None,
            _subscriptions: vec![on_picker, on_project],
        };
        this.refresh_symbols(cx);
        this
    }

    fn refresh_symbols(&mut self, cx: &mut Context<Self>) {
        if !self.project.read(cx).has_analyzed() {
            return;
        }
        let query = self.project.read(cx).query(QueryKind::PassageIndex, cx);
        cx.spawn(async move |this, cx| {
            let answer = query.await;
            _ = this.update(cx, |this, cx| {
                if let Ok(QueryResult::PassageIndex(found)) = answer {
                    this.symbols = found;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The picker's rows: everything while the field has focus and nothing
    /// is typed, the matches once something is. `focused` is read off the
    /// field at render time — the kit's focus events are not something a
    /// subscriber can rely on for the first click.
    fn matches(&self, focused: bool) -> Vec<PassageSymbol> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            if self.list_open || focused {
                self.symbols.clone()
            } else {
                Vec::new()
            }
        } else {
            self.symbols
                .iter()
                .filter(|s| s.path.to_lowercase().contains(&q))
                .cloned()
                .collect()
        }
    }

    fn pick(&mut self, hit: &PassageSymbol, window: &mut Window, cx: &mut Context<Self>) {
        let path = hit.path.clone();
        self.picker.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        let query = self
            .project
            .read(cx)
            .query(QueryKind::Passage { path: path.clone() }, cx);
        self.query.clear();
        self.list_open = false;
        self.pasting = false;
        self.marks.clear();
        self.status = None;
        self.replace_ask = None;
        cx.spawn(async move |this, cx| {
            let answer = query.await;
            _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(QueryResult::Passage(Some(lines))) if !lines.is_empty() => {
                        this.passage = Some(Passage { label: path, lines });
                    }
                    _ => {
                        this.passage = None;
                        this.status = Some(format!("{path} has no lines to mark."));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn use_pasted(&mut self, cx: &mut Context<Self>) {
        let text = self.paste.read(cx).value().to_string();
        let lines: Vec<PassageLine> = text
            .lines()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .enumerate()
            .map(|(i, t)| PassageLine {
                text: t.to_owned(),
                tags: Vec::new(),
                line: u32::try_from(i).unwrap_or(u32::MAX),
                origin: PassageOrigin::Line,
                file: String::new(),
            })
            .collect();
        self.marks.clear();
        self.status = None;
        self.replace_ask = None;
        self.passage = (!lines.is_empty()).then(|| Passage {
            label: "pasted lines".to_owned(),
            lines,
        });
        self.pasting = false;
        cx.notify();
    }

    fn toggle(&mut self, index: usize, mark: Mark, cx: &mut Context<Self>) {
        if self.marks.get(&index) == Some(&mark) {
            self.marks.remove(&index);
        } else {
            self.marks.insert(index, mark);
        }
        self.replace_ask = None;
        cx.notify();
    }

    /// The inference over the visible marked lines, or `None` until
    /// something is marked. Only marks on VISIBLE lines teach.
    fn inference(&self) -> Option<Inference> {
        let passage = self.passage.as_ref()?;
        let visible = visible_lines(passage, self.include_choices);
        let marked = marked_lines(passage, &visible, &self.marks);
        marked
            .iter()
            .any(|l| l.mark.is_some())
            .then(|| infer_dialect(&marked))
    }

    fn write(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(inference) = self.inference() else {
            return;
        };
        let Some(dialect) = inference.dialect.filter(|_| inference.decisions.is_empty()) else {
            return;
        };
        let Some((path, current)) = config_text(&self.project, cx) else {
            return;
        };
        let spec = match to_dialogue_config(&dialect) {
            Some(table) => DialogueSpec::Table(table),
            None => DialogueSpec::File(ARTIFACT_FILE.to_owned()),
        };
        if let Some(existing) = find_dialogue_section(&current)
            && existing.owner != SectionOwner::Editor
            && !force
        {
            self.replace_ask = Some(existing);
            cx.notify();
            return;
        }
        let next = set_dialogue_section(&current, Some(&render_dialogue_section(&spec)));
        let artifact = match &spec {
            DialogueSpec::File(file) => {
                let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
                let artifact_path = if dir.is_empty() {
                    file.clone()
                } else {
                    format!("{dir}/{file}")
                };
                let json = serde_json::to_string_pretty(&dialect)
                    .map(|j| format!("{j}\n"))
                    .unwrap_or_default();
                Some((artifact_path, json))
            }
            DialogueSpec::Table(_) => None,
        };
        self.project.update(cx, |project, cx| {
            // The artifact first, so the config's re-application finds it.
            if let Some((artifact_path, json)) = &artifact {
                project.edit(artifact_path, json.clone(), None, cx);
            }
            project.edit(&path, next, None, cx);
        });
        self.replace_ask = None;
        self.status = Some(match &spec {
            DialogueSpec::Table(table) => format!(
                "Written to {path} as the {} recipe with your rules.",
                table.preset.as_deref().unwrap_or("project")
            ),
            DialogueSpec::File(file) => {
                format!("Written: {path} now points at {file}, which holds your rules in full.")
            }
        });
        cx.notify();
    }

    fn render_picker(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.picker.read(cx).focus_handle(cx).is_focused(window);
        let theme = cx.theme();
        let (fg, muted, border, surface, accent) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.sidebar,
            theme.primary,
        );
        let matches = self.matches(focused);
        let list = (!matches.is_empty() && !self.pasting).then(|| {
            v_flex()
                .id("conv-picker-list")
                .w_full()
                .max_h(px(220.))
                .overflow_y_scroll()
                .border_1()
                .border_color(border)
                .rounded_sm()
                .bg(surface)
                .children(matches.into_iter().take(200).map(|hit| {
                    let pick = hit.clone();
                    h_flex()
                        .id(SharedString::from(format!("conv-hit-{}", hit.path)))
                        .w_full()
                        .px_2()
                        .py_1()
                        .gap_2()
                        .items_center()
                        .cursor_pointer()
                        .hover(|el| el.bg(border.opacity(0.4)))
                        .child(div().text_xs().text_color(accent).child(if hit.is_stitch {
                            "stitch"
                        } else {
                            "knot"
                        }))
                        .child(div().text_sm().text_color(fg).child(hit.path.clone()))
                        .child(div().text_xs().text_color(muted).child(hit.file.clone()))
                        // Mouse down, so the pick lands before the input's
                        // blur closes the list.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                this.pick(&pick, window, cx);
                            }),
                        )
                }))
        });
        let field: AnyElement = if self.pasting {
            v_flex()
                .w_full()
                .gap_1()
                .child(
                    div()
                        .w_full()
                        .h(px(120.))
                        .border_1()
                        .border_color(border)
                        .rounded_sm()
                        .child(Editor::new(&self.paste).h_full().bordered(false)),
                )
                .child(
                    Button::new("conv-use-pasted")
                        .outline()
                        .small()
                        .label("Use these lines")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.use_pasted(cx);
                        })),
                )
                .into_any_element()
        } else {
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(div().flex_1().child(Input::new(&self.picker).small()))
                .child(
                    Button::new("conv-paste")
                        .ghost()
                        .small()
                        .label("Paste lines instead")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.pasting = true;
                            this.list_open = false;
                            cx.notify();
                        })),
                )
                .into_any_element()
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(field)
            .children(list)
            .into_any_element()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one row per line, five marks on each"
    )]
    fn render_lines(
        &self,
        passage: &Passage,
        flagged: &BTreeSet<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, warning, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.warning,
            theme.mono_font_family.clone(),
        );
        let visible = visible_lines(passage, self.include_choices);
        let hidden = passage.lines.len() - visible.len();
        let has_choices = passage
            .lines
            .iter()
            .any(|l| l.origin == PassageOrigin::Choice);
        let rows: Vec<AnyElement> = visible
            .iter()
            .map(|&i| {
                let l = &passage.lines[i];
                let is_flagged = flagged.contains(&i);
                let marks = MARKS.iter().map(|(mark, label)| {
                    let on = self.marks.get(&i) == Some(mark);
                    let mark = *mark;
                    let button = Button::new(SharedString::from(format!("conv-mark-{i}-{label}")))
                        .xsmall()
                        .label(*label)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.toggle(i, mark, cx);
                        }));
                    if on { button.primary() } else { button.ghost() }
                });
                v_flex()
                    .w_full()
                    .py_1()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(if is_flagged {
                        warning
                    } else {
                        border.opacity(0.5)
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .when(l.origin == PassageOrigin::Choice, |el| {
                                el.child(
                                    div()
                                        .px_1()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(border)
                                        .text_xs()
                                        .text_color(muted)
                                        .child("choice"),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .font_family(mono.clone())
                                    .text_sm()
                                    .text_color(fg)
                                    .child(l.text.clone()),
                            ),
                    )
                    .child(h_flex().gap_0p5().children(marks))
                    .into_any_element()
            })
            .collect();
        let project = cx.entity().downgrade();
        v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(div().flex_1().text_xs().text_color(muted).child(format!(
                        "{} \u{b7} {} {}{}",
                        passage.label,
                        visible.len(),
                        if visible.len() == 1 { "line" } else { "lines" },
                        if hidden > 0 && !self.include_choices {
                            format!(
                                " \u{b7} {hidden} {} hidden",
                                if hidden == 1 { "choice" } else { "choices" }
                            )
                        } else {
                            String::new()
                        }
                    )))
                    .when(has_choices, |el| {
                        el.child(
                            Checkbox::new("conv-include-choices")
                                .label("Include choice text")
                                .checked(self.include_choices)
                                .on_click(move |on, _, cx| {
                                    let on = *on;
                                    _ = project.update(cx, |this, cx| {
                                        this.include_choices = on;
                                        cx.notify();
                                    });
                                }),
                        )
                    }),
            )
            .children(rows)
            .child(div().text_xs().text_color(muted).child(
                "Mark at least one of each kind you use. Lines you leave unmarked are checked against the rules, not taught from.",
            ))
            .into_any_element()
    }

    fn render_learned(inference: &Inference, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, success, warning, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.success,
            theme.warning,
            theme.mono_font_family.clone(),
        );
        let mut rows: Vec<AnyElement> = Vec::new();
        if inference.learned.is_empty() && inference.decisions.is_empty() {
            rows.push(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Nothing yet \u{2014} mark a cue to start.")
                    .into_any_element(),
            );
        }
        for l in &inference.learned {
            rows.push(
                h_flex()
                    .w_full()
                    .py_0p5()
                    .gap_2()
                    .items_start()
                    .child(div().text_sm().text_color(success).child("\u{2713}"))
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(fg)
                            .child(l.sentence.clone()),
                    )
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} of {}", l.support.len(), l.total)),
                    )
                    .into_any_element(),
            );
        }
        for d in &inference.decisions {
            let lines = if d.lines.is_empty() {
                String::new()
            } else {
                format!(
                    " (line{} {})",
                    if d.lines.len() == 1 { "" } else { "s" },
                    d.lines
                        .iter()
                        .map(|i| (i + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            rows.push(
                h_flex()
                    .w_full()
                    .py_0p5()
                    .gap_2()
                    .items_start()
                    .child(div().text_sm().text_color(warning).child("\u{2715}"))
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(fg)
                            .child(format!("{}{lines}", d.message)),
                    )
                    .child(
                        div()
                            .font_family(mono.clone())
                            .text_xs()
                            .text_color(warning)
                            .child("needs a decision"),
                    )
                    .into_any_element(),
            );
        }
        v_flex().w_full().children(rows).into_any_element()
    }

    fn render_preview(groups: &[PreviewGroup], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, surface) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.sidebar,
        );
        let palette: [Hsla; 6] = [
            theme.primary,
            theme.info,
            theme.success,
            theme.warning,
            theme.accent,
            theme.danger,
        ];
        let rows: Vec<AnyElement> = groups
            .iter()
            .map(|group| {
                let cue_kind = group.speaker.is_some();
                let lines = group.rows.iter().enumerate().map(|(ri, segments)| {
                    // A character row drops its cue segment (the header
                    // carries it); a parenthetical is its own italic line.
                    let shown: Vec<&brink_ide::dialect_infer::EmittedSegment> = segments
                        .iter()
                        .enumerate()
                        .filter(|(si, s)| {
                            !(cue_kind && ri == 0 && *si == 0 && s.kind == group.kind)
                        })
                        .map(|(_, s)| s)
                        .collect();
                    h_flex()
                        .w_full()
                        .flex_wrap()
                        .children(shown.into_iter().map(|s| {
                            let italic = s.kind.as_deref() == Some("parenthetical");
                            let colour = if s.kind.as_deref() == Some("action") {
                                muted
                            } else {
                                fg
                            };
                            div()
                                .text_sm()
                                .text_color(colour)
                                .when(italic, gpui::Styled::italic)
                                .child(s.text.clone())
                        }))
                });
                match &group.speaker {
                    Some(speaker) => v_flex()
                        .w_full()
                        .pl_4()
                        .pb_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette[speaker_slot(speaker, palette.len())])
                                .child(speaker.clone()),
                        )
                        .children(lines)
                        .into_any_element(),
                    None => v_flex().w_full().pb_1().children(lines).into_any_element(),
                }
            })
            .collect();
        v_flex()
            .w_full()
            .p_3()
            .gap_0p5()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(surface)
            .children(rows)
            .into_any_element()
    }
}

impl Render for ConventionsSection {
    #[expect(
        clippy::too_many_lines,
        reason = "the section is stacked in working order"
    )]
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, warning, danger, border, mono) = (
            theme.muted_foreground,
            theme.warning,
            theme.danger,
            theme.border,
            theme.mono_font_family.clone(),
        );
        let Some(_) = config_text(&self.project, cx) else {
            return no_config("conventions", cx);
        };
        let (current, error) = {
            let (d, e) = self.project.read(cx).dialogue();
            (d.cloned(), e.map(str::to_owned))
        };
        let inference = self.inference();
        let flagged: BTreeSet<usize> = self
            .passage
            .as_ref()
            .zip(inference.as_ref())
            .map(|(passage, inf)| {
                let visible = visible_lines(passage, self.include_choices);
                inf.decisions
                    .iter()
                    .flat_map(|d| d.lines.iter().filter_map(|&j| visible.get(j).copied()))
                    .collect()
            })
            .unwrap_or_default();
        let preview = self.passage.as_ref().map(|passage| {
            let visible = visible_lines(passage, self.include_choices);
            let lines: Vec<&PassageLine> = visible.iter().map(|&i| &passage.lines[i]).collect();
            let folded = fold_glue(&lines);
            let dialect = inference
                .as_ref()
                .and_then(|i| i.dialect.as_ref())
                .or(current.as_ref());
            preview_groups(&folded, dialect)
        });
        let can_confirm = inference
            .as_ref()
            .is_some_and(|i| i.dialect.is_some() && i.decisions.is_empty());
        let confirm_hint = match &inference {
            None => Some("Mark some lines first"),
            Some(_) if !can_confirm => Some("Settle the decisions above first"),
            Some(_) => None,
        };

        v_flex()
            .w_full()
            .gap_1()
            .child(setting_group("Teach the studio your script", cx))
            .child(div().pb_1().text_xs().text_color(muted).child(
                "Point at a passage the way you actually write it, then mark what each line is. The studio works out the rules and shows them back to you before anything is saved.",
            ))
            .child(setting_row(
                "Current conventions",
                describe_current(current.as_ref()),
                div()
                    .font_family(mono.clone())
                    .text_xs()
                    .text_color(muted)
                    .child(current.as_ref().map_or("none".to_owned(), |d| {
                        if d.name.is_empty() { "project".to_owned() } else { d.name.clone() }
                    })),
                cx,
            ))
            .children(error.map(|e| {
                div()
                    .text_xs()
                    .text_color(danger)
                    .child(format!("[dialogue] did not resolve: {e}"))
            }))
            .child(setting_group("Your lines", cx))
            .child(self.render_picker(window, cx))
            .children(
                self.passage
                    .as_ref()
                    .map(|passage| self.render_lines(passage, &flagged, cx)),
            )
            .children(
                self.status
                    .as_ref()
                    .map(|s| div().pt_1().text_xs().text_color(muted).child(s.clone())),
            )
            .children(inference.as_ref().map(|inf| {
                v_flex()
                    .w_full()
                    .child(setting_group("What the studio learned", cx))
                    .child(Self::render_learned(inf, cx))
            }))
            .children(preview.as_ref().map(|groups| {
                v_flex()
                    .w_full()
                    .child(setting_group("How it reads in the Player", cx))
                    .child(Self::render_preview(groups, cx))
                    .child(div().pt_1().text_xs().text_color(muted).child("Updates as you mark lines."))
            }))
            .children(self.replace_ask.as_ref().map(|ask| {
                v_flex()
                    .w_full()
                    .mt_2()
                    .p_3()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(warning)
                    .child(div().text_sm().text_color(warning).child(
                        if ask.owner == SectionOwner::Hand {
                            "brink.toml already has a [dialogue] section written by hand. Replace it with these rules?"
                        } else {
                            "The [dialogue] section in brink.toml was edited since the studio last wrote it. Replace it with these rules?"
                        },
                    ))
                    .child(
                        div()
                            .p_2()
                            .rounded_sm()
                            .border_1()
                            .border_color(border)
                            .font_family(mono.clone())
                            .text_xs()
                            .text_color(muted)
                            .child(ask.text.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("conv-keep")
                                    .outline()
                                    .small()
                                    .label("Keep it")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.replace_ask = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("conv-replace")
                                    .primary()
                                    .small()
                                    .label("Replace it")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.write(true, cx);
                                    })),
                            ),
                    )
            }))
            .child(
                h_flex()
                    .w_full()
                    .pt_3()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().text_xs().text_color(muted).child(
                        "Nothing is written until you confirm.",
                    ))
                    .child(
                        Button::new("conv-start-over")
                            .outline()
                            .small()
                            .label("Start over")
                            .disabled(self.marks.is_empty())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.marks.clear();
                                this.replace_ask = None;
                                this.status = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("conv-use-rules")
                            .primary()
                            .small()
                            .label("Use these rules")
                            .disabled(!can_confirm)
                            .when_some(confirm_hint, |b, hint| b.tooltip(hint))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.write(false, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, origin: PassageOrigin) -> PassageLine {
        PassageLine {
            text: text.to_owned(),
            tags: Vec::new(),
            line: 0,
            origin,
            file: "a.ink".to_owned(),
        }
    }

    #[test]
    fn choices_hide_by_default_and_marks_stay_keyed_to_the_passage() {
        let passage = Passage {
            label: "start".to_owned(),
            lines: vec![
                line("@MARA: <>", PassageOrigin::Line),
                line("Lisa: Where?", PassageOrigin::Choice),
                line("Not even close.", PassageOrigin::Line),
            ],
        };
        assert_eq!(visible_lines(&passage, false), [0, 2]);
        assert_eq!(visible_lines(&passage, true), [0, 1, 2]);
        let mut marks = BTreeMap::new();
        marks.insert(2usize, Mark::Dialogue);
        let marked = marked_lines(&passage, &visible_lines(&passage, false), &marks);
        assert_eq!(marked.len(), 2);
        assert_eq!(
            marked[1].mark,
            Some(Mark::Dialogue),
            "keyed by passage index, not row"
        );
        assert_eq!(marked[0].origin, Origin::Line);
    }

    #[test]
    fn glue_folds_into_the_next_line_for_the_preview() {
        let a = line("@MARA: <>", PassageOrigin::Line);
        let b = line("We go now.", PassageOrigin::Line);
        let c = line("Alone <>", PassageOrigin::Line);
        assert_eq!(fold_glue(&[&a, &b]), ["@MARA: We go now."]);
        assert_eq!(fold_glue(&[&b, &c]), ["We go now.", "Alone "]);
    }

    #[test]
    fn the_preview_groups_a_run_under_its_speaker() {
        let dialect = brink_ir::dialect::at_cue_preset();
        let groups = preview_groups(
            &[
                "@Mara: We go now.".to_owned(),
                "Not even close.".to_owned(),
                "The lantern gutters.".to_owned(),
            ],
            Some(&dialect),
        );
        // The preset has no run ender, so the narration joins the run.
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].speaker.as_deref(), Some("Mara"));
        assert_eq!(groups[0].rows.len(), 3);
        assert_eq!(groups[0].rows[0][0].kind.as_deref(), Some("character"));
        let plain = preview_groups(&["Hello.".to_owned()], None);
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].speaker, None);
        assert_eq!(
            describe_current(None),
            "None \u{2014} lines print as plain text."
        );
        assert!(describe_current(Some(&dialect)).starts_with("at-cue \u{2014} character"));
        assert!(speaker_slot("Mara", 6) < 6);
    }
}
