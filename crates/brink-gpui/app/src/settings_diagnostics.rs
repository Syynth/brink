//! Settings ▸ Diagnostics (Project scope): `[lints]` and `[fix]` — the
//! studio's `LintSettings` (#3148, design round 2026-08-27).
//!
//! Two lists, and **which list a code is in IS whether it is in
//! `brink.toml`** — there is no second state to read. "Configure" moves a
//! code up, writing the key at its CURRENT default so the first click
//! changes nothing about the build; the down arrow moves it back out,
//! removing the key.
//!
//! What is listed is decided by the compiler, not by this file:
//! `brink_ide::diagnostic_registry` is the code list (the same table the
//! web studio reads, so the two sections cannot drift apart), `overridable`
//! gates the lower list (only a minority of codes can be set at all;
//! offering a level picker for the rest would build the silent no-op this
//! surface exists to prevent), and the surfaces filter keeps a `.ink`-only
//! project from being offered settings for `.brink` markup spans.
//!
//! Severity glyphs are the Problems panel's own, showing each code's
//! EFFECTIVE level — so a row reads the same here as the problem it will
//! produce.
//!
//! A Fix column sits beside severity on every row (#3419,
//! `docs/autofix-spec.md` §6.1): `off | ask | auto`, written into `[fix]`
//! through the same write path. `[fix]` and `[lints]` are independent
//! tables keyed by the same code, so a code with a `[fix]` entry is listed
//! even when the `overridable`/surfaces gates would otherwise hide it — it
//! just does not get the `[lints]` Configure affordance, since `[lints]`
//! genuinely cannot act on it.

use std::collections::BTreeSet;

use brink_ide::diagnostic_registry::{DiagnosticInfo, registry};
use brink_ir::Severity;
use brink_project_config::edit::ConfigDocument;
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, Hsla, IntoElement, Render, SharedString, Subscription,
    Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::switch::Switch;
use gpui_component::text::TextView;
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::project::{Project, ProjectEvent};
use crate::settings_config::{
    broken_notice, config_text, edit_config, no_config, parse_error, set_or_remove,
};

/// The levels `[lints]` accepts, in escalating order.
pub const LEVELS: [&str; 4] = ["allow", "hint", "warn", "deny"];

/// The `[fix]` policies, least to most aggressive.
pub const FIX_LEVELS: [&str; 3] = ["off", "ask", "auto"];

/// `[lints] deny-warnings` — the policy key, not a code.
const DENY_WARNINGS: &str = "deny-warnings";

/// The Problems bucket a code's EFFECTIVE level lands in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Error,
    Warning,
    Info,
}

impl Bucket {
    /// The Problems panel's own glyphs.
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Error => "\u{25CF}",
            Self::Warning => "\u{25B2}",
            Self::Info => "\u{2139}",
        }
    }

    pub fn of(severity: Severity) -> Self {
        match severity {
            Severity::Error => Self::Error,
            Severity::Warning => Self::Warning,
            // Hint joins Info: the Problems panel already buckets them
            // together, so splitting them here would hand the UI a
            // distinction it does not draw.
            Severity::Info | Severity::Hint => Self::Info,
        }
    }
}

/// The bucket for a configured level, or the default's when unconfigured.
/// `allow` produces no problem at all, so it gets no glyph rather than a
/// quiet one — an author scanning the list should see nothing where
/// nothing will be reported.
#[must_use]
pub fn bucket_for(level: Option<&str>, default: Severity) -> Option<Bucket> {
    match level {
        None => Some(Bucket::of(default)),
        Some("allow") => None,
        Some("deny") => Some(Bucket::Error),
        Some("warn") => Some(Bucket::Warning),
        Some(_) => Some(Bucket::Info),
    }
}

/// The level "Configure" writes: the code's CURRENT default, so the first
/// click brings it under the project's control without changing what the
/// build does.
#[must_use]
pub fn default_level(default: Severity) -> &'static str {
    match default {
        Severity::Error => "deny",
        Severity::Warning => "warn",
        Severity::Info | Severity::Hint => "hint",
    }
}

/// Which surfaces a project actually writes, from the files it has rather
/// than from `dialect`: a `.brink` file is the native surface whatever the
/// dialect says, and the question is "can this project produce that
/// diagnostic". Before the first file list arrives, both — an empty list
/// would read as "no diagnostics exist".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surfaces {
    pub ink: bool,
    pub native: bool,
}

impl Surfaces {
    #[must_use]
    pub fn of_files(files: &[String]) -> Self {
        let ink = files.iter().any(|f| f.ends_with(".ink"));
        let native = files.iter().any(|f| f.ends_with(".brink"));
        if ink || native {
            Self { ink, native }
        } else {
            Self {
                ink: true,
                native: true,
            }
        }
    }

    /// Whether `[lints]` can act on `info` in this project.
    #[must_use]
    pub fn can_configure(self, info: &DiagnosticInfo) -> bool {
        info.overridable && (self.native || !info.native_only)
    }
}

/// The three lists the section draws.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lists {
    /// Named in `[lints]` and known to this compiler.
    pub configured: Vec<DiagnosticInfo>,
    /// Named in `[lints]` or `[fix]` but unknown here — kept, never
    /// dropped: it may belong to a newer compiler.
    pub unknown: Vec<String>,
    /// Not in `[lints]`: configurable here, or carrying a `[fix]` entry.
    pub unconfigured: Vec<DiagnosticInfo>,
}

#[must_use]
pub fn lists(
    registry: &[DiagnosticInfo],
    lint_keys: &[String],
    fix_keys: &[String],
    surfaces: Surfaces,
) -> Lists {
    let codes: Vec<&str> = lint_keys
        .iter()
        .map(String::as_str)
        .filter(|k| *k != DENY_WARNINGS)
        .collect();
    let known = |code: &str| registry.iter().any(|r| r.code.as_str() == code);
    let configured = codes
        .iter()
        .filter_map(|code| registry.iter().find(|r| r.code.as_str() == *code).copied())
        .collect();
    let mut unknown: Vec<String> = codes
        .iter()
        .copied()
        .chain(fix_keys.iter().map(String::as_str))
        .filter(|code| !known(code))
        .map(str::to_owned)
        .collect();
    unknown.sort();
    unknown.dedup();
    let unconfigured = registry
        .iter()
        .filter(|r| !codes.contains(&r.code.as_str()))
        .filter(|r| fix_keys.iter().any(|k| k == r.code.as_str()) || surfaces.can_configure(r))
        .copied()
        .collect();
    Lists {
        configured,
        unknown,
        unconfigured,
    }
}

/// Group rows under their category heading, categories in first-seen
/// order.
#[must_use]
pub fn grouped(rows: &[DiagnosticInfo]) -> Vec<(&'static str, Vec<DiagnosticInfo>)> {
    let mut out: Vec<(&'static str, Vec<DiagnosticInfo>)> = Vec::new();
    for row in rows {
        let key = row.category.unwrap_or("Other");
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(*row),
            None => out.push((key, vec![*row])),
        }
    }
    out
}

#[must_use]
pub fn matches(info: &DiagnosticInfo, needle: &str) -> bool {
    needle.is_empty()
        || info.code.as_str().to_lowercase().contains(needle)
        || info.title.to_lowercase().contains(needle)
        || info
            .category
            .is_some_and(|c| c.to_lowercase().contains(needle))
}

pub struct DiagnosticsSection {
    project: Entity<Project>,
    registry: Vec<DiagnosticInfo>,
    filter: Entity<InputState>,
    query: String,
    /// Codes whose explanation is unfolded.
    open: BTreeSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl DiagnosticsSection {
    pub fn new(project: Entity<Project>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Search codes"));
        let on_filter = cx.subscribe(&filter, |this: &mut Self, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = state.read(cx).value().to_string();
                cx.notify();
            }
        });
        let on_project = cx.subscribe(&project, |_, _, event: &ProjectEvent, cx| {
            if matches!(
                event,
                ProjectEvent::Opened { .. }
                    | ProjectEvent::Analyzed
                    | ProjectEvent::SourceChanged { .. }
            ) {
                cx.notify();
            }
        });
        Self {
            project,
            registry: registry(),
            filter,
            query: String::new(),
            open: BTreeSet::new(),
            _subscriptions: vec![on_filter, on_project],
        }
    }

    fn set_level(&self, code: &str, level: Option<&str>, cx: &mut Context<Self>) {
        edit_config(&self.project, cx, |doc| {
            set_or_remove(doc, "lints", code, level)
        });
    }

    fn set_fix(&self, code: &str, level: Option<&str>, cx: &mut Context<Self>) {
        edit_config(&self.project, cx, |doc| {
            set_or_remove(doc, "fix", code, level)
        });
    }

    /// `− [allow] [hint] [warn] [deny]` — one button lit.
    fn picker(
        &self,
        id: &str,
        options: &'static [&'static str],
        value: Option<&str>,
        on_pick: impl Fn(&mut Self, &'static str, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_0p5()
            .children(options.iter().map(|option| {
                let on = value == Some(*option);
                let pick = on_pick.clone();
                let button = Button::new(SharedString::from(format!("{id}-{option}")))
                    .xsmall()
                    .label(*option)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        pick(this, option, cx);
                    }));
                if on { button.primary() } else { button.ghost() }
            }))
            .into_any_element()
    }

    #[expect(clippy::too_many_lines, reason = "one row, every affordance on it")]
    fn render_row(
        &self,
        info: DiagnosticInfo,
        level: Option<&str>,
        fix: Option<&str>,
        can_configure: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, danger, warning, info_colour, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.danger,
            theme.warning,
            theme.info,
            theme.mono_font_family.clone(),
        );
        let code = info.code.as_str().to_owned();
        let configured = level.is_some();
        let bucket = bucket_for(level, info.default_severity);
        let glyph_colour = match bucket {
            Some(Bucket::Error) => danger,
            Some(Bucket::Warning) => warning,
            Some(Bucket::Info) => info_colour,
            None => Hsla::transparent_black(),
        };
        let open = self.open.contains(&code);
        let default_text = match bucket_for(None, info.default_severity) {
            Some(Bucket::Error) => "error",
            Some(Bucket::Warning) => "warning",
            _ => "info",
        };
        let code_for_level = code.clone();
        let code_for_fix = code.clone();
        let code_for_move = code.clone();
        let code_for_open = code.clone();
        let mut main = h_flex()
            .w_full()
            .h(px(28.))
            .gap_2()
            .items_center()
            .child(if info.explanation.is_some() {
                Button::new(SharedString::from(format!("explain-{code}")))
                    .ghost()
                    .xsmall()
                    .label(if open { "\u{25BE}" } else { "\u{25B8}" })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !this.open.remove(&code_for_open) {
                            this.open.insert(code_for_open.clone());
                        }
                        cx.notify();
                    }))
                    .into_any_element()
            } else {
                div().w(px(22.)).into_any_element()
            })
            .child(
                div()
                    .w(px(14.))
                    .text_xs()
                    .text_color(glyph_colour)
                    .child(bucket.map_or("", Bucket::glyph)),
            )
            .child(
                div()
                    .font_family(mono)
                    .text_xs()
                    .text_color(fg)
                    .child(code.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_sm()
                    .text_color(fg)
                    .child(info.title),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(div().text_xs().text_color(muted).child("Fix"))
                    .child(self.picker(
                        &format!("fix-{code}"),
                        &FIX_LEVELS,
                        fix,
                        move |this, option, cx| {
                            // Clicking the lit button clears it — a `[fix]`
                            // value is not permanent once set.
                            let current = config_text(&this.project, cx)
                                .and_then(|(_, t)| ConfigDocument::parse(&t).ok())
                                .and_then(|d| d.string("fix", &code_for_fix));
                            let next = (current.as_deref() != Some(option)).then_some(option);
                            this.set_fix(&code_for_fix, next, cx);
                        },
                        cx,
                    )),
            );
        if configured {
            main = main
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("default {default_text}")),
                )
                .child(self.picker(
                    &format!("level-{code}"),
                    &LEVELS,
                    level,
                    move |this, option, cx| {
                        this.set_level(&code_for_level, Some(option), cx);
                    },
                    cx,
                ))
                .child(
                    Button::new(SharedString::from(format!("unconfigure-{code}")))
                        .ghost()
                        .xsmall()
                        .label("\u{2193}")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.set_level(&code_for_move, None, cx);
                        })),
                );
        } else {
            main = main
                .child(div().text_xs().text_color(muted).child(default_text))
                .when(can_configure, |el| {
                    let default = default_level(info.default_severity);
                    el.child(
                        Button::new(SharedString::from(format!("configure-{code}")))
                            .outline()
                            .xsmall()
                            .label("\u{2191} Configure")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.set_level(&code_for_move, Some(default), cx);
                            })),
                    )
                });
        }
        v_flex()
            .w_full()
            .border_b_1()
            .border_color(border.opacity(0.5))
            .child(main)
            .when(open, |el| {
                el.children(info.explanation.map(|text| {
                    div()
                        .pl(px(30.))
                        .pb_2()
                        .text_sm()
                        .text_color(fg)
                        .child(TextView::markdown(
                            SharedString::from(format!("explanation-{code}")),
                            text,
                        ))
                }))
            })
            .into_any_element()
    }

    fn render_unknown(&self, code: &str, fix: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, border, warning, mono) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.warning,
            theme.mono_font_family.clone(),
        );
        let code_owned = code.to_owned();
        h_flex()
            .w_full()
            .h(px(28.))
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(border.opacity(0.5))
            .child(div().w(px(22.)))
            .child(
                div()
                    .w(px(14.))
                    .text_xs()
                    .text_color(warning)
                    .child(Bucket::Warning.glyph()),
            )
            .child(
                div()
                    .font_family(mono)
                    .text_xs()
                    .text_color(fg)
                    .child(code.to_owned()),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(muted)
                    .child("Kept — it may belong to a newer compiler."),
            )
            .children(fix.map(|fix| {
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Fix {fix}"))
            }))
            .child(
                Button::new(SharedString::from(format!("remove-{code}")))
                    .ghost()
                    .xsmall()
                    .label("\u{2193}")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        // One write carrying both removals.
                        let code = code_owned.clone();
                        edit_config(&this.project, cx, |doc| {
                            let a = doc.remove_key("lints", &code)?;
                            let b = doc.remove_key("fix", &code)?;
                            Ok(a || b)
                        });
                    })),
            )
            .into_any_element()
    }

    fn render_group(
        &self,
        title: &'static str,
        rows: Vec<DiagnosticInfo>,
        doc: &ConfigDocument,
        surfaces: Surfaces,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let lints_configured = |code: &str| doc.string("lints", code);
        v_flex()
            .w_full()
            .child(
                div()
                    .pt_2()
                    .pb_0p5()
                    .text_xs()
                    .text_color(muted)
                    .child(title),
            )
            .children(rows.into_iter().map(|info| {
                let code = info.code.as_str();
                let level = lints_configured(code);
                let fix = doc.string("fix", code);
                self.render_row(
                    info,
                    level.as_deref().filter(|l| LEVELS.contains(l)),
                    fix.as_deref().filter(|f| FIX_LEVELS.contains(f)),
                    surfaces.can_configure(&info),
                    cx,
                )
            }))
            .into_any_element()
    }
}

impl Render for DiagnosticsSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let Some((path, text)) = config_text(&self.project, cx) else {
            return no_config("diagnostics settings", cx);
        };
        let doc = match ConfigDocument::parse(&text) {
            Ok(doc) => doc,
            Err(_) => {
                return v_flex()
                    .w_full()
                    .children(parse_error(&text).map(|reason| broken_notice(&reason, cx)))
                    .into_any_element();
            }
        };
        let surfaces = Surfaces::of_files(self.project.read(cx).files());
        let lint_keys = doc.keys("lints");
        let fix_keys = doc.keys("fix");
        let lists = lists(&self.registry, &lint_keys, &fix_keys, surfaces);
        let deny = doc.bool("lints", DENY_WARNINGS) == Some(true);
        let needle = self.query.trim().to_lowercase();
        let shown: Vec<DiagnosticInfo> = lists
            .unconfigured
            .iter()
            .filter(|r| matches(r, &needle))
            .copied()
            .collect();
        let configured_count = lists.configured.len() + lists.unknown.len();
        let project = self.project.clone();

        v_flex()
            .w_full()
            .gap_1()
            .child(div().pb_1().text_xs().text_color(muted).child(format!(
                "Written to [lints] (and [fix], the Fix column) in {path}."
            )))
            .child(brink_gpui_shell::settings_modal::setting_row(
                "Deny warnings",
                "Promote every warning to an error, the way -D warnings does.",
                Switch::new("deny-warnings")
                    .checked(deny)
                    .on_click(move |on, _, cx| {
                        let on = *on;
                        edit_config(&project, cx, |doc| {
                            if on {
                                doc.set_bool("lints", DENY_WARNINGS, true)?;
                                Ok(true)
                            } else {
                                doc.remove_key("lints", DENY_WARNINGS)
                            }
                        });
                    }),
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .pt_2()
                    .items_baseline()
                    .gap_2()
                    .child(brink_gpui_shell::settings_modal::setting_group(
                        "Project lint configuration",
                        cx,
                    ))
                    .child(div().text_xs().text_color(muted).child(format!(
                        "{configured_count} code{}",
                        if configured_count == 1 { "" } else { "s" }
                    ))),
            )
            .when(configured_count == 0, |el| {
                el.child(div().text_xs().text_color(muted).child(
                    "Nothing configured — every diagnostic is running at its built-in default.",
                ))
            })
            .children(
                grouped(&lists.configured)
                    .into_iter()
                    .map(|(title, rows)| self.render_group(title, rows, &doc, surfaces, cx)),
            )
            .when(!lists.unknown.is_empty(), |el| {
                el.child(
                    div()
                        .pt_2()
                        .pb_0p5()
                        .text_xs()
                        .text_color(muted)
                        .child("Unknown to this compiler"),
                )
                .children(lists.unknown.iter().map(|code| {
                    let fix = doc.string("fix", code);
                    self.render_unknown(code, fix.as_deref(), cx)
                }))
            })
            .child(
                h_flex()
                    .w_full()
                    .pt_3()
                    .items_center()
                    .gap_3()
                    .child(brink_gpui_shell::settings_modal::setting_group(
                        "Not configured",
                        cx,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Running at their built-in defaults."),
                    )
                    .child(div().flex_1())
                    .child(div().w(px(180.)).child(Input::new(&self.filter).small()))
                    .child(div().text_xs().text_color(muted).child(format!(
                        "{}/{}",
                        shown.len(),
                        lists.unconfigured.len()
                    ))),
            )
            .children(
                grouped(&shown)
                    .into_iter()
                    .map(|(title, rows)| self.render_group(title, rows, &doc, surfaces, cx)),
            )
            .when(shown.is_empty(), |el| {
                el.child(div().pt_1().text_xs().text_color(muted).child(
                    if lists.unconfigured.is_empty() {
                        "Every configurable diagnostic is already in the list above.".to_owned()
                    } else {
                        format!("No code matches \u{201c}{}\u{201d}.", self.query.trim())
                    },
                ))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(code: brink_ir::hir::DiagnosticCode) -> DiagnosticInfo {
        registry()
            .into_iter()
            .find(|r| r.code == code)
            .expect("a real code")
    }

    #[test]
    fn buckets_follow_the_effective_level() {
        assert_eq!(bucket_for(None, Severity::Warning), Some(Bucket::Warning));
        assert_eq!(bucket_for(None, Severity::Hint), Some(Bucket::Info));
        assert_eq!(bucket_for(Some("allow"), Severity::Warning), None);
        assert_eq!(
            bucket_for(Some("deny"), Severity::Warning),
            Some(Bucket::Error)
        );
        assert_eq!(
            bucket_for(Some("hint"), Severity::Warning),
            Some(Bucket::Info)
        );
        assert_eq!(default_level(Severity::Warning), "warn");
        assert_eq!(default_level(Severity::Info), "hint");
        assert_eq!(default_level(Severity::Error), "deny");
    }

    #[test]
    fn surfaces_come_from_the_files_and_default_to_both() {
        let ink = Surfaces::of_files(&["a.ink".to_owned()]);
        assert!(ink.ink && !ink.native);
        let none = Surfaces::of_files(&[]);
        assert!(none.ink && none.native, "an empty project hides nothing");
        let e063 = info(brink_ir::hir::DiagnosticCode::E063);
        assert!(e063.overridable);
        assert!(ink.can_configure(&e063));
        let native_only: Vec<DiagnosticInfo> = registry()
            .into_iter()
            .filter(|r| r.overridable && r.native_only)
            .collect();
        for r in &native_only {
            assert!(!ink.can_configure(r), "{} is native-only", r.code.as_str());
            assert!(none.can_configure(r));
        }
    }

    #[test]
    fn the_lists_partition_by_the_file_and_keep_the_unknown() {
        let reg = registry();
        let all = Surfaces {
            ink: true,
            native: true,
        };
        let lint_keys = vec![
            "E063".to_owned(),
            DENY_WARNINGS.to_owned(),
            "E999".to_owned(),
        ];
        let fix_keys = vec!["E014".to_owned(), "E998".to_owned(), "E001".to_owned()];
        let l = lists(&reg, &lint_keys, &fix_keys, all);
        assert_eq!(
            l.configured
                .iter()
                .map(|r| r.code.as_str())
                .collect::<Vec<_>>(),
            ["E063"]
        );
        assert_eq!(
            l.unknown,
            ["E998", "E999"],
            "sorted, deduped, policy key excluded"
        );
        assert!(l.unconfigured.iter().all(|r| r.code.as_str() != "E063"));
        assert!(
            l.unconfigured.iter().any(|r| r.code.as_str() == "E001"),
            "a [fix]-only code is listed even though [lints] cannot act on it"
        );
        assert!(
            !all.can_configure(&info(brink_ir::hir::DiagnosticCode::E001)),
            "…and it must not get the Configure affordance"
        );
        assert!(l.unconfigured.iter().any(|r| r.code.as_str() == "E014"));
        assert!(
            l.unconfigured
                .iter()
                .all(|r| r.overridable || fix_keys.contains(&r.code.as_str().to_owned())),
            "nothing else that [lints] cannot set is offered"
        );
    }

    #[test]
    fn grouping_keeps_first_seen_order_and_matching_reads_three_fields() {
        let reg = registry();
        let overridable: Vec<DiagnosticInfo> =
            reg.iter().filter(|r| r.overridable).copied().collect();
        let groups = grouped(&overridable);
        assert!(groups.len() >= 4);
        assert_eq!(groups[0].0, "Logic", "E014 is the first overridable code");
        let e063 = info(brink_ir::hir::DiagnosticCode::E063);
        assert!(matches(&e063, "e063"));
        assert!(matches(&e063, "types"));
        assert!(matches(&e063, ""));
        assert!(!matches(&e063, "zzzz"));
    }
}
