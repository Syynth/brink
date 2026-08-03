//! Hover projection for the `@[style(...)]` declaration surface — issue
//! #1719's remaining scope.
//!
//! `StyleToken`/`StyleAnnotation` (`brink_ir::hir::types`) are produced by
//! `hir::lower_native::annotation` but, before this module, were read by
//! **nothing** downstream: no analyzer pass, no IDE query, no LSP or web
//! consumer (`docs/prose-dialect-spec.md` §3.5b addenda 3–4 rules the
//! surface; the struct's own doc comment says as much: "nothing in the
//! compiler or runtime reads it yet").
//!
//! **This is a compiler-side query only.** It renders the parsed style
//! entries as hover text through the existing `crate::hover::hover` seam,
//! which `brink-cli`'s `ide hover`, `brink-lsp`'s `textDocument/hover`, and
//! `brink-web`'s editor hover already call — no CSS class, no
//! semantic-token modifier, no CM6/buffer decoration is produced here. That
//! rendering is the editor track (NS-T, #1131/#1350), held by deliberate
//! sequencing (`docs/decision-log.md`, 2026-08-01 "NS-T is held by
//! deliberate sequencing"); the same entry's sibling ruling for the
//! conventions-classification query family (#2006) draws the identical
//! line — "queries emitted from `brink-db`/`brink-ide`... are compiler-side
//! artifacts that happen to have an editor as their consumer" are unheld,
//! only "anything that renders" stays held. This module is that unheld
//! half for `@[style]`.

use brink_ir::{HirFile, StyleAnnotation, StyleToken, SymbolInfo, SymbolKind};
use rowan::TextRange;

/// Hover suffix for a `@[style(...)]` annotation declared on `info`'s
/// knot/stitch, if any — a `**style**` line ready to append to `hover.rs`'s
/// content string. `None` for every other symbol, for an ink file
/// (`style_annotation` is always empty there — the `@[style]` channel is
/// native-only), and for a native knot/stitch with no `@[style(...)]` of
/// its own.
#[must_use]
pub fn style_hover_text(hir: &HirFile, info: &SymbolInfo) -> Option<String> {
    if !hir.native || !matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch) {
        return None;
    }
    let style = find_style_annotation(hir, info.range)?;
    Some(format!(
        "\n\n**style** `@[style({})]`",
        render_entries(style)
    ))
}

/// Find the `style_annotation` of the knot/stitch whose own declaration
/// name range is `range` — the same range-keyed lookup idiom
/// `fn_value_hover` uses for var/const/temp slots. A top-level stitch
/// promoted to knot status (`Knot::symbol_kind`) is a `Knot` struct in
/// `hir.knots`, so knots are searched first; only a genuinely nested
/// stitch is reachable through its owning knot's `stitches`.
fn find_style_annotation(hir: &HirFile, range: TextRange) -> Option<&StyleAnnotation> {
    hir.knots
        .iter()
        .find(|k| k.name.range == range)
        .and_then(|k| k.style_annotation.as_ref())
        .or_else(|| {
            hir.knots
                .iter()
                .flat_map(|k| k.stitches.iter())
                .find(|s| s.name.range == range)
                .and_then(|s| s.style_annotation.as_ref())
        })
}

fn render_entries(style: &StyleAnnotation) -> String {
    style
        .entries
        .iter()
        .map(|e| format!("{} = \"{}\"", e.key, display_token(&e.value)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The value half of one `key = "value"` clause, reproducing exactly the
/// spelling `parse_style_token` (`hir::lower_native::annotation`) accepted
/// — the closed vocabulary's own name for a built-in token, or the raw
/// hex/custom string it fell back to. Never invents a spelling outside what
/// was actually written in source.
fn display_token(token: &StyleToken) -> String {
    match token {
        StyleToken::AlignLeft => "left".to_string(),
        StyleToken::AlignCenter => "center".to_string(),
        StyleToken::AlignRight => "right".to_string(),
        StyleToken::Bold => "bold".to_string(),
        StyleToken::Italic => "italic".to_string(),
        StyleToken::Dim => "dim".to_string(),
        StyleToken::Mono => "mono".to_string(),
        StyleToken::Uppercase => "uppercase".to_string(),
        StyleToken::Conceal => "conceal".to_string(),
        StyleToken::Color(hex) => hex.clone(),
        StyleToken::Custom(name) => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::style_hover_text;
    use crate::navigation::find_def_at_offset;
    use crate::session::IdeSession;
    use rowan::TextSize;

    fn style_at(src: &str, needle: &str) -> Option<String> {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.brink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let hir = session.hir(file_id).expect("hir");
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");
        let info = find_def_at_offset(analysis, file_id, TextSize::from(pos))?;
        style_hover_text(hir, info)
    }

    #[test]
    fn knot_style_annotation_renders_its_entries() {
        let src = "\
@[element(args = \"^(?<chan>[A-Z0-9-]+): (?<text>.+)$\")]
@[style(chan = \"channel\", line = \"radio\")]
fn radio(chan: string, text: string) {
    > [{chan}] {text}
}
flow main() {
    !radio TAC-2: All units report in.
}
";
        let hover = style_at(src, "radio(chan").expect("style hover text");
        assert_eq!(
            hover,
            "\n\n**style** `@[style(chan = \"channel\", line = \"radio\")]`"
        );
    }

    #[test]
    fn built_in_tokens_render_their_closed_vocabulary_spelling() {
        let src = "\
@[element(args = \"^(?<name>.+)$\")]
@[style(name = \"bold\", line = \"right\")]
fn cue(name: string) {
    > {name}
}
";
        let hover = style_at(src, "cue(name").expect("style hover text");
        assert_eq!(
            hover,
            "\n\n**style** `@[style(name = \"bold\", line = \"right\")]`"
        );
    }

    #[test]
    fn custom_token_renders_its_own_hook_name() {
        let src = "\
@[element(args = \"^(?<name>.+)$\")]
@[style(name = \"whisper\")]
fn cue(name: string) {
    > {name}
}
";
        let hover = style_at(src, "cue(name").expect("style hover text");
        assert_eq!(hover, "\n\n**style** `@[style(name = \"whisper\")]`");
    }

    #[test]
    fn a_knot_with_no_style_annotation_has_no_style_hover() {
        let src = "flow main() {\n    Hello.\n}\n";
        assert!(style_at(src, "main()").is_none());
    }

    #[test]
    fn an_ink_file_never_produces_style_hover_text() {
        // `style_annotation` is native-only — always `None` for an ink
        // frontend HIR (`HirFile::native` is `false`), even if a symbol
        // happens to share a name a native fixture would annotate.
        let mut session = IdeSession::new();
        let src = "=== radio ===\nHello.\n-> END\n";
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let hir = session.hir(file_id).expect("hir");
        let pos = u32::try_from(src.find("radio").expect("needle")).expect("offset");
        let info = find_def_at_offset(analysis, file_id, TextSize::from(pos)).expect("def");
        assert!(style_hover_text(hir, info).is_none());
    }
}
