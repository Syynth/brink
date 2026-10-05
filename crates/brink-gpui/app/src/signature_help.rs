//! Parameter hints while typing a call — the web studio's signature help
//! (`packages/ink-editor/src/signature-help.ts`, `.brink-signature-help`).
//!
//! Triggered the LSP-standard way: typing `(` or `,` asks; while it is open,
//! every edit asks again, which moves the active parameter or, once the
//! cursor has left the call, closes it. Plain typing with no hint showing
//! never queries — the web learned that per-keystroke probing cost a
//! whole-document query on every key.
//!
//! One per host — a Script tab, the Write view — following whichever of its
//! editors was last edited. Drawn above the cursor, in the editor's code
//! face, with the active parameter in the accent.

use gpui::prelude::*;
use gpui::{
    Anchor, AnyElement, App, Context, Entity, Font, FontWeight, StyledText, TextRun,
    UnderlineStyle, WeakEntity, anchored, deferred, div, point, px,
};
use gpui_component::{ActiveTheme as _, input::EditorState, v_flex};

use brink_gpui_model::query::{QueryKind, QueryResult, Signature};

use crate::project::Project;

/// A host's signature hint: what is showing, and for which editor.
#[derive(Default)]
pub(crate) struct SignatureHint {
    showing: Option<(WeakEntity<EditorState>, Signature)>,
    /// Bumped per query, so an answer that a newer query has overtaken is
    /// dropped (typing `((`, or moving on to the next argument).
    seq: u64,
}

impl SignatureHint {
    /// The signature showing, for the tests.
    #[cfg(test)]
    pub(crate) fn showing(&self) -> Option<&Signature> {
        self.showing.as_ref().map(|(_, signature)| signature)
    }

    /// An edit landed in `editor`, which holds `path`. Ask for the call's
    /// signature when a `(` or `,` was just typed, or while one is showing.
    pub(crate) fn edited<T: 'static>(
        this: &mut T,
        hint: impl Fn(&mut T) -> &mut SignatureHint + 'static,
        project: &Entity<Project>,
        path: &str,
        editor: &Entity<EditorState>,
        cx: &mut Context<T>,
    ) {
        let (cursor, before) = {
            let state = editor.read(cx);
            let cursor = state.cursor();
            let before = state.value()[..cursor.min(state.value().len())]
                .chars()
                .next_back();
            (cursor, before)
        };
        let typed_trigger = matches!(before, Some('(' | ','));
        let slot = hint(this);
        if !typed_trigger && slot.showing.is_none() {
            return;
        }
        slot.seq += 1;
        let seq = slot.seq;
        let query = project.read(cx).query(
            QueryKind::SignatureHelp {
                path: path.to_owned(),
                offset: u32::try_from(cursor).unwrap_or(u32::MAX),
            },
            cx,
        );
        let target = editor.downgrade();
        cx.spawn(async move |this, cx| {
            let answer = query.await;
            let _ = this.update(cx, |this, cx| {
                let slot = hint(this);
                if slot.seq != seq {
                    return;
                }
                slot.showing = match answer {
                    Ok(QueryResult::SignatureHelp(Some(signature))) => Some((target, signature)),
                    _ => None,
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Put it away — the editor lost focus, or the author pressed Escape.
    pub(crate) fn dismiss(&mut self) -> bool {
        self.seq += 1;
        self.showing.take().is_some()
    }

    /// The hint, above the cursor of the editor it belongs to; `None` when
    /// nothing shows or the editor is gone.
    pub(crate) fn render(&self, cx: &App) -> Option<AnyElement> {
        let (editor, signature) = self.showing.as_ref()?;
        let editor = editor.upgrade()?;
        let state = editor.read(cx);
        let cursor = state.cursor();
        let at = state.range_to_bounds(&(cursor..cursor))?;
        Some(
            deferred(
                anchored()
                    .position(point(at.left(), at.top() - px(4.)))
                    .anchor(Anchor::BottomLeft)
                    .snap_to_window()
                    .child(card(signature, cx)),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// `.brink-signature-help`: the signature in the code face with the active
/// parameter accented, its documentation muted beneath.
fn card(signature: &Signature, cx: &App) -> AnyElement {
    let tokens = brink_gpui_shell::theme::current(cx).tokens;
    let hsla = brink_gpui_shell::theme::hsla;
    let (fg, muted, accent) = (hsla(tokens.fg), hsla(tokens.fg_muted), hsla(tokens.accent));
    let (panel, border) = (hsla(tokens.panel_bg), hsla(tokens.border));
    let theme = cx.theme();
    let mono = gpui::font(theme.mono_font_family.clone());
    let ui = theme.font_family.clone();

    let (text, runs) = label_runs(signature, &mono, fg, accent);
    v_flex()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(4.))
        .border_1()
        .border_color(border)
        .bg(panel)
        .shadow_md()
        .max_w(px(520.))
        .text_sm()
        .child(StyledText::new(text).with_runs(runs))
        .when_some(signature.documentation.clone(), |el, doc| {
            el.child(
                div()
                    .mt(px(4.))
                    .font_family(ui)
                    .text_xs()
                    .text_color(muted)
                    .child(doc),
            )
        })
        .into_any_element()
}

/// The signature as runs: each parameter found in order within the label,
/// the active one accented, semibold and underlined — the web's walk.
pub(crate) fn label_runs(
    signature: &Signature,
    mono: &Font,
    fg: gpui::Hsla,
    accent: gpui::Hsla,
) -> (String, Vec<TextRun>) {
    let plain = |len: usize| TextRun {
        len,
        font: mono.clone(),
        color: fg,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let label = signature.label.clone();
    let mut runs = Vec::new();
    let mut at = 0usize;
    for (i, param) in signature.parameters.iter().enumerate() {
        let Some(found) = label[at..].find(param.as_str()) else {
            continue;
        };
        let start = at + found;
        if start > at {
            runs.push(plain(start - at));
        }
        if i as u32 == signature.active {
            runs.push(TextRun {
                len: param.len(),
                font: Font {
                    weight: FontWeight::SEMIBOLD,
                    ..mono.clone()
                },
                color: accent,
                background_color: None,
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(accent),
                    wavy: false,
                }),
                strikethrough: None,
            });
        } else {
            runs.push(plain(param.len()));
        }
        at = start + param.len();
    }
    if at < label.len() {
        runs.push(plain(label.len() - at));
    }
    (label, runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_parameter_is_its_own_run() {
        let signature = Signature {
            label: "greet(name, times)".to_owned(),
            documentation: None,
            parameters: vec!["name".to_owned(), "times".to_owned()],
            active: 1,
        };
        let mono = gpui::font("Mono");
        let (text, runs) = label_runs(&signature, &mono, gpui::black(), gpui::white());
        let lens: Vec<usize> = runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![6, 4, 2, 5, 1], "greet( name , times )");
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), text.len());
        assert!(runs[3].underline.is_some(), "times is the active one");
        assert!(runs[1].underline.is_none());
    }
}
