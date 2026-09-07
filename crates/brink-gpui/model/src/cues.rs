//! Dialect-classified lines — the studio's cue / parenthetical / dialogue
//! styling, as plain data the UI can paint without holding a session.
//!
//! `brink-ir`'s [`LineContext`](brink_ir::hir::line_context::LineContext)
//! carries a `dialect` facet on every line a registered `[dialogue]`
//! dialect classified. The web studio turns that into a `brink-<kind>`
//! class per line and lets `editor.css` say what a cue looks like; this
//! crate ships the same fact across the worker boundary as absolute byte
//! spans, because a gpui highlighter styles ranges rather than lines.
//!
//! **Nothing is produced for a project with no dialect.** The classified
//! lines are the only ones that carry a `dialect` facet at all, so a plain
//! ink project ships an empty map and pays nothing.

use brink_ide::session::IdeSession;
use brink_ir::FileId;
use brink_ir::hir::line_context::LineContext;

/// One dialect-classified line: its extent, the kind that classified it,
/// and the sigil spans the web studio hides outright (`@`, `:<>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueLine {
    /// Absolute byte offset of the line's first character.
    pub start: u32,
    /// Absolute byte offset of the line's end, newline excluded.
    pub end: u32,
    /// The dialect kind — `character`, `parenthetical`, `dialogue`, or
    /// whatever a project's own dialect declares.
    pub kind: String,
    /// Absolute byte spans of the kind's hidden geometry (the sigils).
    /// Empty for a chain-only kind such as `dialogue`.
    pub hidden: Vec<(u32, u32)>,
}

/// Every dialect-classified line in `source`, from contexts already
/// computed for the file. Split out from [`cue_lines`] so the mapping can
/// be tested without a session.
#[must_use]
pub fn from_contexts(source: &str, contexts: &[LineContext]) -> Vec<CueLine> {
    let mut out = Vec::new();
    let mut at = 0u32;
    for (i, line) in source.split_inclusive('\n').enumerate() {
        let start = at;
        at += u32::try_from(line.len()).unwrap_or(0);
        let end = start + u32::try_from(line.trim_end_matches(['\n', '\r']).len()).unwrap_or(0);
        let Some(context) = contexts.get(i) else {
            continue;
        };
        let Some(dialect) = context.dialect.as_ref() else {
            continue;
        };
        out.push(CueLine {
            start,
            end,
            kind: dialect.kind.clone(),
            hidden: dialect
                .hidden_spans
                .iter()
                .map(|(s, e)| (start + *s, start + *e))
                .filter(|(s, e)| s < e && *e <= end)
                .collect(),
        });
    }
    out
}

/// The file's dialect-classified lines, or an empty vec when no dialect is
/// registered or the file has none.
#[must_use]
pub fn cue_lines(session: &IdeSession, id: FileId) -> Vec<CueLine> {
    if session.dialect().is_none() {
        return Vec::new();
    }
    let (Some(source), Some(contexts)) = (session.db().source(id), session.line_contexts(id))
    else {
        return Vec::new();
    };
    from_contexts(source, &contexts)
}

#[cfg(test)]
mod tests {
    use brink_ir::hir::line_context::{DialectLineInfo, LineContext};

    fn cue(kind: &str, hidden: Vec<(u32, u32)>) -> LineContext {
        LineContext {
            dialect: Some(DialectLineInfo {
                kind: kind.to_owned(),
                attrs: Vec::new(),
                hidden_spans: hidden,
                content_span: None,
                nature: brink_ir::ElementNature::Narrative,
            }),
            ..LineContext::default()
        }
    }

    #[test]
    fn a_classified_line_becomes_an_absolute_span_and_its_sigils_move_with_it() {
        //          0123456789
        let source = "hello\n@Alice:<>\nhi\n";
        let contexts = vec![
            LineContext::default(),
            // `@` at 0..1 and `:<>` at 6..9, line-relative.
            cue("character", vec![(0, 1), (6, 9)]),
            LineContext::default(),
        ];
        let cues = super::from_contexts(source, &contexts);
        assert_eq!(cues.len(), 1, "{cues:?}");
        assert_eq!(cues[0].kind, "character");
        // Line two starts at byte 6 and runs to 15 (the `\n` excluded).
        assert_eq!((cues[0].start, cues[0].end), (6, 15));
        assert_eq!(&source[6..15], "@Alice:<>");
        assert_eq!(cues[0].hidden, vec![(6, 7), (12, 15)]);
        assert_eq!(&source[12..15], ":<>");
    }

    #[test]
    fn an_unclassified_file_and_a_short_context_list_yield_nothing() {
        let source = "plain\nlines\n";
        assert!(
            super::from_contexts(source, &[LineContext::default(), LineContext::default()])
                .is_empty()
        );
        // Fewer contexts than lines is not a panic — the tail is skipped.
        assert!(super::from_contexts(source, &[]).is_empty());
    }

    #[test]
    fn a_sigil_span_running_past_its_line_is_dropped_rather_than_painted() {
        // Defensive: geometry is line-relative and trusted, but a span that
        // would reach into the next line must never style it.
        let source = "@A:<>\nnext\n";
        let cues = super::from_contexts(source, &[cue("character", vec![(0, 1), (3, 40)])]);
        assert_eq!(cues[0].hidden, vec![(0, 1)]);
    }
}
