//! Passage lines (#3408): the content lines of a knot or stitch, as an
//! author would mark them in the Conventions editor's teach-by-example
//! flow (`docs/decision-log.md` 2026-09-02, "Conventions editor:
//! teach-by-example is the design direction").
//!
//! This is a *source-line* road, not a HIR-content one: the author marks
//! the lines they see in the editor, so the text is the source line with
//! only the weave scaffolding (`*`/`+`/`-` markers, `(label)`, leading
//! `{condition}` groups) and the tags removed. Glue (`<>`), inline logic
//! and choice brackets stay — the cue-glue convention is exactly what the
//! author is about to mark, and the inference reads source shapes.
//!
//! Choice text is included because the ink documentation's own sub-format
//! example puts the cue inside choice text (`*   Lisa: Where did he go?`);
//! the emitted-side classification has to see it.

use brink_ir::hir::HirFile;
use brink_ir::hir::line_context::{LineContext, LineElement};
use serde::Serialize;

/// Where a passage line came from in the weave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PassageOrigin {
    /// A plain content line.
    Line,
    /// The text of a choice (`*`/`+` line), scaffolding removed.
    Choice,
    /// The text on a gather line (`- text`), scaffolding removed.
    Gather,
}

/// One content line of a passage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassageLine {
    /// The line's content with scaffolding and tags removed; never empty.
    pub text: String,
    /// The line's tags, in order, without their `#`; never part of `text`.
    pub tags: Vec<String>,
    /// Zero-based source line.
    pub line: u32,
    pub origin: PassageOrigin,
}

/// The content lines of `path` (`knot` or `knot.stitch`) in `hir`, in
/// source order, or `None` when the path names nothing in this file.
///
/// A knot's passage is its whole extent, stitches included: the author is
/// pointing at a place in the story, not at the slice of it that runs
/// before the first stitch header. Excluded: knot/stitch headers, logic,
/// diverts, declarations, comments, tag-only lines (knot/global tag
/// blocks), blank lines, and anything inside a block comment.
pub fn passage_lines(
    hir: &HirFile,
    source: &str,
    contexts: &[LineContext],
    path: &str,
) -> Option<Vec<PassageLine>> {
    let (knot_name, stitch_name) = match path.split_once('.') {
        Some((k, s)) => (k, Some(s)),
        None => (path, None),
    };
    let knot = hir.knots.iter().find(|k| k.name.text == knot_name)?;
    let (start, end) = if let Some(s) = stitch_name {
        let stitch = knot.stitches.iter().find(|st| st.name.text == s)?;
        let r = stitch.ptr.text_range();
        (usize::from(r.start()), usize::from(r.end()))
    } else {
        let r = knot.ptr.text_range();
        // The knot node may or may not span its stitches depending on the
        // frontend; take the union so a knot is always whole.
        let end = knot
            .stitches
            .iter()
            .map(|st| usize::from(st.ptr.text_range().end()))
            .fold(usize::from(r.end()), usize::max);
        (usize::from(r.start()), end)
    };

    let mut out = Vec::new();
    let mut offset = 0usize;
    for (line_no, raw) in source.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += raw.len();
        if line_start < start || line_start >= end {
            continue;
        }
        let Some(ctx) = contexts.get(line_no) else {
            break;
        };
        if ctx.block_comment {
            continue;
        }
        let origin = match ctx.element {
            LineElement::Narrative => PassageOrigin::Line,
            LineElement::Choice => PassageOrigin::Choice,
            LineElement::Gather => PassageOrigin::Gather,
            _ => continue,
        };
        let line = raw.trim_end_matches(['\n', '\r']);
        let (text, tags) = split_tags(strip_scaffolding(line, origin));
        if text.is_empty() {
            continue;
        }
        out.push(PassageLine {
            text: text.to_owned(),
            tags,
            line: u32::try_from(line_no).unwrap_or(u32::MAX),
            origin,
        });
    }
    Some(out)
}

/// Remove the weave scaffolding an author does not think of as "the line":
/// choice/gather markers, a `(label)`, and leading `{condition}` groups.
fn strip_scaffolding(line: &str, origin: PassageOrigin) -> &str {
    let mut s = line.trim_start();
    let markers: &[char] = match origin {
        PassageOrigin::Choice => &['*', '+'],
        PassageOrigin::Gather => &['-'],
        PassageOrigin::Line => &[],
    };
    if !markers.is_empty() {
        s = s.trim_start_matches(|c: char| markers.contains(&c) || c == ' ' || c == '\t');
        loop {
            let t = s.trim_start();
            if let Some(rest) = t.strip_prefix('(')
                && let Some(close) = rest.find(')')
            {
                s = &rest[close + 1..];
                continue;
            }
            if t.starts_with('{')
                && let Some(len) = balanced_group_len(t)
            {
                s = &t[len..];
                continue;
            }
            s = t;
            break;
        }
    }
    s.trim()
}

/// Length of the `{…}` group `s` starts with, braces balanced, or `None`
/// when it never closes.
fn balanced_group_len(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split a line into its text and its `# tag` list: the first `#` at brace
/// depth zero (and not escaped) starts the tags, which run to the end of
/// the line separated by further `#`s.
fn split_tags(line: &str) -> (&str, Vec<String>) {
    let mut depth = 0usize;
    let mut prev = '\0';
    for (i, c) in line.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '#' if depth == 0 && prev != '\\' => {
                let tags = line[i + 1..]
                    .split('#')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
                    .collect();
                return (line[..i].trim_end(), tags);
            }
            _ => {}
        }
        prev = c;
    }
    (line, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::{PassageLine, PassageOrigin, passage_lines};

    const SRC: &str = "\
# author: Someone
=== chapel ===
# location: crypt
@MARA: <>
We don't have until morning. # surly
~ temp x = 1
* Not even close.
* (again) {x > 0} Lisa: Where did he go?
    > She sets the lantern down.
- Then we go now. # beat # cue.ogg
= argument
/* a block
   comment */
@JUNO: <>
I always wait for you.
-> END
= after_dark
The lantern gutters.
=== elsewhere ===
Nothing here.
";

    type Row = (String, Vec<String>, u32, PassageOrigin);

    fn lines_for(src: &str, path: &str) -> Option<Vec<Row>> {
        let parsed = brink_syntax::parse(src);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parsed.tree());
        let projection = crate::hir_projection::project_hir_structural(&hir, src);
        let contexts =
            brink_ir::hir::line_context::line_contexts(src, &parsed.syntax(), &projection);
        passage_lines(&hir, src, &contexts, path).map(|v| {
            v.into_iter()
                .map(
                    |PassageLine {
                         text,
                         tags,
                         line,
                         origin,
                     }| (text, tags, line, origin),
                )
                .collect()
        })
    }

    #[test]
    fn a_stitch_is_its_own_lines_only() {
        let got = lines_for(SRC, "chapel.argument").expect("stitch resolves");
        assert_eq!(
            got,
            vec![
                ("@JUNO: <>".to_owned(), vec![], 13, PassageOrigin::Line),
                (
                    "I always wait for you.".to_owned(),
                    vec![],
                    14,
                    PassageOrigin::Line
                ),
            ],
            "block comment, divert and the header are not content"
        );
    }

    #[test]
    fn a_knot_is_whole_stitches_included_headers_tags_logic_excluded() {
        let got = lines_for(SRC, "chapel").expect("knot resolves");
        let texts: Vec<&str> = got.iter().map(|(t, ..)| t.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "@MARA: <>",
                "We don't have until morning.",
                "Not even close.",
                "Lisa: Where did he go?",
                "> She sets the lantern down.",
                "Then we go now.",
                "@JUNO: <>",
                "I always wait for you.",
                "The lantern gutters.",
            ]
        );
        assert!(
            !texts.iter().any(|t| t.contains("Nothing here")),
            "the next knot's lines must not leak in"
        );
    }

    #[test]
    fn tags_are_split_off_never_part_of_text() {
        let got = lines_for(SRC, "chapel").expect("knot resolves");
        let surly = got
            .iter()
            .find(|(t, ..)| t.starts_with("We don't"))
            .unwrap();
        assert_eq!(surly.1, vec!["surly".to_owned()]);
        let beat = got.iter().find(|(t, ..)| t == "Then we go now.").unwrap();
        assert_eq!(beat.1, vec!["beat".to_owned(), "cue.ogg".to_owned()]);
        assert_eq!(beat.3, PassageOrigin::Gather);
    }

    #[test]
    fn choice_scaffolding_is_stripped_but_text_kept() {
        let got = lines_for(SRC, "chapel").expect("knot resolves");
        let lisa = got.iter().find(|(t, ..)| t.starts_with("Lisa")).unwrap();
        assert_eq!(
            lisa.0, "Lisa: Where did he go?",
            "label and condition removed"
        );
        assert_eq!(lisa.3, PassageOrigin::Choice);
        assert_eq!(lisa.2, 7, "zero-based source line");
    }

    #[test]
    fn unknown_paths_resolve_to_none() {
        assert!(lines_for(SRC, "nave").is_none());
        assert!(lines_for(SRC, "chapel.nave").is_none());
    }

    #[test]
    fn a_bare_gather_or_choice_marker_yields_no_line() {
        let src = "=== k ===\n* [Go]\n    - \n    Went.\n";
        let got = lines_for(src, "k").expect("knot resolves");
        let texts: Vec<&str> = got.iter().map(|(t, ..)| t.as_str()).collect();
        assert_eq!(texts, vec!["[Go]", "Went."]);
    }
}
