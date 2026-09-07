//! Prose checking — spelling and light grammar over the file's PROSE, and
//! nothing else (`docs/prose-checking.md`).
//!
//! `brink-prose` is the checker; this is the part that decides what to
//! hand it. The unit of checking is a **content span minus the machinery
//! nested inside it**: an interpolation lives inside a content span, so
//! taking content spans whole would hand `{gold}` to a spell checker and
//! get `gold` reported as a word the author invented. The subtraction is
//! the whole trick, and it is the same one `packages/ink-editor/src/
//! prose.ts` performs on the web.
//!
//! **Offsets cross two coordinate systems.** brink speaks bytes;
//! `brink-prose` (and CodeMirror, and LSP) speak UTF-16 code units. Both
//! conversions happen here, in one place, against one prefix table — so a
//! lint's range is a byte range by the time it leaves this module and
//! nothing downstream has to know the checker's units.
//!
//! **The dictionary is the project's own names.** Without it every
//! invented character, place and cue reports as a misspelling, which is
//! indistinguishable from the feature being broken (#3210). The knots,
//! stitches and labels the analysis already knows are added to whatever
//! `[prose] dictionary` lists.

use std::collections::BTreeSet;

use brink_ide::session::IdeSession;
use brink_ir::hir::projection::SpanKind;

/// One finding, in BYTE offsets of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProseLint {
    pub start: u32,
    pub end: u32,
    /// Harper's rule category — `Spelling`, `Repetition`, … Carried
    /// through rather than flattened, so the editor can style or filter
    /// by kind without this module inventing a taxonomy.
    pub kind: String,
    pub message: String,
}

/// The prose ranges of `source`, as BYTE ranges: content spans, minus
/// every non-content span nested inside them.
#[must_use]
pub fn prose_ranges(spans: &[brink_ir::hir::projection::ProjectedSpan]) -> Vec<(u32, u32)> {
    let mut content: Vec<(u32, u32)> = Vec::new();
    let mut holes: Vec<(u32, u32)> = Vec::new();
    for span in spans {
        if span.kind.is_container() {
            continue;
        }
        let range = (span.range.start().into(), span.range.end().into());
        if span.kind == SpanKind::Content {
            content.push(range);
        } else {
            holes.push(range);
        }
    }
    subtract(&content, &holes)
}

/// `content` minus `holes` — the gaps left over, in order. One interval
/// walk; the second one written independently is the one that gets the
/// boundary conditions wrong.
fn subtract(content: &[(u32, u32)], holes: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for &(from, to) in content {
        let mut inside: Vec<(u32, u32)> = holes
            .iter()
            .copied()
            .filter(|&(hf, ht)| ht > from && hf < to)
            .collect();
        inside.sort_unstable();
        let mut cursor = from;
        for (hf, ht) in inside {
            if hf > cursor {
                out.push((cursor, hf.min(to)));
            }
            cursor = cursor.max(ht);
            if cursor >= to {
                break;
            }
        }
        if cursor < to {
            out.push((cursor, to));
        }
    }
    out.retain(|(a, b)| b > a);
    out
}

/// Byte offset → UTF-16 code-unit offset, and back.
///
/// One prefix table per check rather than a scan per span: a file is read
/// once and both directions are a binary search.
struct Units {
    /// `(byte, utf16)` at every char boundary, ascending.
    marks: Vec<(u32, u32)>,
}

impl Units {
    fn new(text: &str) -> Self {
        let mut marks = Vec::with_capacity(text.len() / 4 + 2);
        let mut utf16 = 0u32;
        for (byte, ch) in text.char_indices() {
            marks.push((byte as u32, utf16));
            utf16 += ch.len_utf16() as u32;
        }
        marks.push((text.len() as u32, utf16));
        Self { marks }
    }

    fn to_utf16(&self, byte: u32) -> u32 {
        match self.marks.binary_search_by_key(&byte, |(b, _)| *b) {
            Ok(i) => self.marks[i].1,
            // Inside a character: the code unit that character starts at.
            Err(i) => self.marks[i.saturating_sub(1)].1,
        }
    }

    fn to_byte(&self, utf16: u32) -> u32 {
        match self.marks.binary_search_by_key(&utf16, |(_, u)| *u) {
            Ok(i) => self.marks[i].0,
            Err(i) => self.marks[i.saturating_sub(1)].0,
        }
    }
}

/// The words a project's own manuscript is allowed to contain: every knot,
/// stitch and label the analysis knows, plus `[prose] dictionary`.
#[must_use]
pub fn project_dictionary(session: &IdeSession, extra: &[String]) -> Vec<String> {
    let mut words: BTreeSet<String> = extra.iter().cloned().collect();
    if let Some(analysis) = session.analysis() {
        for info in analysis.index.symbols.values() {
            // A qualified name (`knot.stitch`) is not a word; its parts
            // are, and a checker only ever sees the parts.
            for part in info.name.split(['.', '_']) {
                if part.len() > 1 && part.chars().all(char::is_alphanumeric) {
                    words.insert(part.to_owned());
                }
            }
        }
    }
    words.into_iter().collect()
}

/// Check one file. `None` when the file is not in the session.
#[must_use]
pub fn check(
    session: &IdeSession,
    path: &str,
    dictionary: &[String],
    dialect: Option<&str>,
) -> Option<Vec<ProseLint>> {
    let id = session.file_id(path)?;
    let source = session.source(id)?.to_owned();
    let projection = session.projection(id)?;
    let ranges = prose_ranges(&projection.spans);
    if ranges.is_empty() {
        return Some(Vec::new());
    }
    let units = Units::new(&source);
    let request = brink_prose::CheckRequest {
        text: source.clone(),
        spans: ranges
            .iter()
            .map(|&(a, b)| brink_prose::SpanJs {
                start: units.to_utf16(a) as usize,
                end: units.to_utf16(b) as usize,
            })
            .collect(),
        dictionary: dictionary.to_vec(),
        dialect: dialect.map(str::to_owned),
    };
    let response = brink_prose::check(&request);
    Some(
        response
            .lints
            .into_iter()
            .map(|lint| ProseLint {
                start: units.to_byte(lint.start as u32),
                end: units.to_byte(lint.end as u32),
                kind: lint.kind,
                message: lint.message,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hole_splits_the_content_around_it() {
        // `You have {gold} coins.` — the interpolation is inside the
        // content span, and handing it to a spell checker is how `gold`
        // becomes a word the author invented.
        let content = [(0, 22)];
        let holes = [(9, 15)];
        assert_eq!(subtract(&content, &holes), vec![(0, 9), (15, 22)]);
    }

    #[test]
    fn holes_at_the_edges_shorten_rather_than_split() {
        assert_eq!(subtract(&[(0, 10)], &[(0, 4)]), vec![(4, 10)]);
        assert_eq!(subtract(&[(0, 10)], &[(6, 10)]), vec![(0, 6)]);
        assert_eq!(subtract(&[(0, 10)], &[(0, 10)]), Vec::new());
        // A hole outside the span leaves it whole.
        assert_eq!(subtract(&[(4, 10)], &[(0, 4), (10, 12)]), vec![(4, 10)]);
    }

    #[test]
    fn overlapping_holes_are_walked_once() {
        assert_eq!(
            subtract(&[(0, 20)], &[(4, 9), (6, 12), (15, 18)]),
            vec![(0, 4), (12, 15), (18, 20)]
        );
    }

    #[test]
    fn offsets_cross_to_utf16_and_back() {
        // `é` is two bytes and one code unit; `🌊` is four bytes and TWO.
        let text = "aé🌊b";
        let units = Units::new(text);
        assert_eq!(units.to_utf16(0), 0);
        assert_eq!(units.to_utf16(1), 1, "after `a`");
        assert_eq!(units.to_utf16(3), 2, "after `é`");
        assert_eq!(units.to_utf16(7), 4, "after the surrogate pair");
        for byte in [0u32, 1, 3, 7, 8] {
            assert_eq!(
                units.to_byte(units.to_utf16(byte)),
                byte,
                "round trip at {byte}"
            );
        }
    }

    #[test]
    fn a_byte_inside_a_character_lands_on_its_start() {
        let units = Units::new("aé");
        // Byte 2 is the second byte of `é`; there is no code unit there.
        assert_eq!(units.to_utf16(2), 1);
    }
}
