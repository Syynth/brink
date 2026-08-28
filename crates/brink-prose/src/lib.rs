//! `brink-prose` — spelling and light grammar over a brink document's prose,
//! as its own wasm artifact.
//!
//! # Why this is not part of `brink-web`
//!
//! Measured (2026-08-27, harper-core 2.8): a wasm module containing nothing
//! but `harper-core` is **6.15 MB gzipped**, against `brink-web`'s entire
//! 2.61 MB — compiler, analyzer, IDE queries and runtime included. Linking it
//! in would roughly triple what every consumer downloads, including one that
//! only wants to play a compiled story.
//!
//! It also cannot be optimized away later: `wasm-opt -Os` moved the probe from
//! 9.80 MB to 9.57 MB, a 2% change, because the weight is *data* — the curated
//! FST dictionary plus the Brill POS tagger's weights — not code. So this is a
//! separate artifact loaded on demand, and the editor holds a seam rather than
//! a dependency.
//!
//! # What it knows, and what it deliberately doesn't
//!
//! It receives resolved byte ranges of prose and checks those. It does **not**
//! know about the HIR, `SpanKind::Content`, or interpolation subtraction —
//! that work belongs to the caller, because depending on the compiler crates
//! here would duplicate the 2.6 MB this split exists to keep out.
//!
//! # Offsets
//!
//! In and out, every offset on this boundary is a **UTF-16 code unit** — the
//! unit `CodeMirror` document positions and LSP positions both use, so neither
//! consumer needs a conversion table to call this crate. Harper indexes by
//! `char`; that conversion lives in [`masker`] and happens exactly twice, at
//! the two edges.
//!
//! # The dictionary is the feature
//!
//! Without seeded words, `"Kaelen nodded"` reports a misspelling suggesting
//! *Karen* — measured, and fatal for fiction. `dictionary` on the request is
//! how a project's own proper nouns (knot names, and above all the cue names
//! that say who its characters are) become known words. See #3210.

mod masker;

use harper_core::linting::{LintGroup, Linter};
use harper_core::parsers::{Mask, PlainEnglish};
use harper_core::spell::{FstDictionary, MergedDictionary, MutableDictionary};
use harper_core::{Dialect, Document};
use harper_core::{DictWordMetadata, NounData};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

pub use masker::Utf16Range;
use masker::{CharToUtf16, SpanMasker};

/// One prose range to check, in UTF-16 code units into `text`.
#[derive(Debug, Deserialize)]
pub struct SpanJs {
    pub start: usize,
    pub end: usize,
}

/// What the editor asks for.
#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    /// The whole file. Sent entire, not just the prose, so that returned
    /// spans are file offsets with no mapping layer, and so Harper can see
    /// sentence boundaries across the machinery between two prose runs.
    pub text: String,
    /// The ranges that hold prose. Everything else is never tokenized.
    #[serde(default)]
    pub spans: Vec<SpanJs>,
    /// Project proper nouns — see the module docs. Without these the checker
    /// is unusable on fiction.
    #[serde(default)]
    pub dictionary: Vec<String>,
    /// `american` | `british` | `canadian` | `australian`. Unknown values
    /// fall back to American rather than failing the request: a typo in
    /// `brink.toml` should not silently turn checking off.
    #[serde(default)]
    pub dialect: Option<String>,
}

/// One finding.
#[derive(Debug, Serialize)]
pub struct ProseLintJs {
    /// UTF-16 code-unit offsets into the request's `text` — the unit
    /// `CodeMirror` and LSP both index by, so a consumer needs no conversion.
    pub start: usize,
    pub end: usize,
    /// Harper's rule category — `Spelling`, `Repetition`, `Capitalization`, …
    /// Carried through so the editor can style or filter by kind without this
    /// crate deciding a taxonomy on its behalf.
    pub kind: String,
    pub message: String,
    /// Ready-to-apply fixes. May be empty.
    pub suggestions: Vec<ProseSuggestionJs>,
}

/// One quick-fix.
///
/// The `kind` is carried rather than flattened into "here is some text",
/// because the three are applied differently and collapsing them produces a
/// WRONG edit rather than a missing one: an `insert_after` rendered as a
/// replacement would delete the word it was meant to follow.
#[derive(Debug, Serialize)]
pub struct ProseSuggestionJs {
    /// `replace` — swap the lint's span for `text`.
    /// `insert_after` — insert `text` at the span's end, keeping the span.
    /// `remove` — delete the lint's span; `text` is empty.
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct CheckResponseJs {
    pub lints: Vec<ProseLintJs>,
}

fn dialect_from(name: Option<&str>) -> Dialect {
    match name.map(str::to_ascii_lowercase).as_deref() {
        Some("british") => Dialect::British,
        Some("canadian") => Dialect::Canadian,
        Some("australian") => Dialect::Australian,
        // Includes `None` and anything unrecognized. A bad `brink.toml` value
        // should degrade to the default dialect, not to no checking at all.
        _ => Dialect::American,
    }
}

/// Run the checker. Pure: same request, same response.
///
/// Kept separate from the `wasm_bindgen` entry point so it is testable on the
/// host — the offset behaviour is this crate's whole correctness claim, and
/// pinning it through a wasm harness would be slower and prove less.
pub fn check(request: &CheckRequest) -> CheckResponseJs {
    let ranges: Vec<Utf16Range> = request
        .spans
        .iter()
        .map(|s| Utf16Range {
            start: s.start,
            end: s.end,
        })
        .collect();

    // Nothing marked as prose means nothing to check. Returning early also
    // skips building the dictionary, which is the expensive part.
    let masker = SpanMasker::new(&request.text, &ranges);
    if masker.allowed_chars() == 0 {
        return CheckResponseJs { lints: Vec::new() };
    }

    // The SAME dictionary for both, and that is not a tidiness point: the
    // Document does its own word lookup while tokenizing, so handing it the
    // curated dictionary while merging the project's names only into the
    // linter leaves every invented name still reported — the exact failure
    // this feature exists to prevent, and one that looks like it works
    // because the merged dictionary does reach the *suggestions*.
    let dictionary = build_dictionary(&request.dictionary);
    let parser = Mask::new(masker, PlainEnglish);
    let document = Document::new(&request.text, &parser, &dictionary);

    let mut linter = LintGroup::new_curated(dictionary, dialect_from(request.dialect.as_deref()));
    let lints = linter.lint(&document);

    let to_utf16 = CharToUtf16::new(&request.text);
    let lints = lints
        .into_iter()
        .map(|lint| ProseLintJs {
            start: to_utf16.offset(lint.span.start),
            end: to_utf16.offset(lint.span.end),
            kind: format!("{:?}", lint.lint_kind),
            message: lint.message,
            suggestions: lint.suggestions.iter().map(suggestion_js).collect(),
        })
        .collect();

    CheckResponseJs { lints }
}

/// The curated dictionary plus the project's own words.
///
/// Merged rather than replaced: the project's words are additions, and a
/// story that happens to name a character "The" must not knock the real word
/// out of the dictionary.
fn build_dictionary(words: &[String]) -> Arc<MergedDictionary> {
    let mut merged = MergedDictionary::new();
    merged.add_dictionary(FstDictionary::curated());

    if !words.is_empty() {
        let mut project = MutableDictionary::new();
        for word in words.iter().filter(|w| !w.is_empty()) {
            // Tagged as a proper noun, not as a bare word. The names arriving
            // here ARE proper nouns — knots, structs, and above all the cue
            // names that say who a story's characters are — and Harper's
            // capitalization rules read this metadata. An untagged entry
            // silences the misspelling but leaves `kaelen` at the start of a
            // sentence looking as correct as `Kaelen`.
            project.append_word_str(word, proper_noun_metadata());
        }
        merged.add_dictionary(Arc::new(project));
    }

    Arc::new(merged)
}

/// Metadata for a seeded project name.
fn proper_noun_metadata() -> DictWordMetadata {
    DictWordMetadata {
        noun: Some(NounData {
            is_proper: Some(true),
            ..NounData::default()
        }),
        ..DictWordMetadata::default()
    }
}

/// Map one of Harper's suggestions onto the wire shape.
fn suggestion_js(suggestion: &harper_core::linting::Suggestion) -> ProseSuggestionJs {
    use harper_core::linting::Suggestion as S;
    match suggestion {
        S::ReplaceWith(chars) => ProseSuggestionJs {
            kind: "replace",
            text: chars.iter().collect(),
        },
        S::InsertAfter(chars) => ProseSuggestionJs {
            kind: "insert_after",
            text: chars.iter().collect(),
        },
        S::Remove => ProseSuggestionJs {
            kind: "remove",
            text: String::new(),
        },
    }
}

/// wasm entry point. JSON in, JSON out — the same shape every other brink
/// wasm boundary uses (`editor_dto.rs`), so this module needs no shared
/// memory with `brink-web` and the two can be separate instances.
///
/// A malformed request returns an empty result rather than throwing: this
/// runs on a debounce behind the keystroke path, and an exception there would
/// surface to the author as a broken editor rather than as absent squiggles.
#[wasm_bindgen]
#[must_use]
pub fn check_prose(request_json: &str) -> String {
    let Ok(request) = serde_json::from_str::<CheckRequest>(request_json) else {
        return r#"{"lints":[]}"#.to_owned();
    };
    let response = check(&request);
    serde_json::to_string(&response).unwrap_or_else(|_| r#"{"lints":[]}"#.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{CheckRequest, SpanJs, check};

    /// UTF-16 length, for a fixture that is entirely prose.
    fn utf16_len(text: &str) -> usize {
        text.chars().map(char::len_utf16).sum()
    }

    /// UTF-16 offset of a byte position, so fixtures can be written with
    /// `str::find` rather than hand-counted units.
    fn utf16_of(text: &str, byte: usize) -> usize {
        text[..byte].chars().map(char::len_utf16).sum()
    }

    /// The text a lint covers.
    ///
    /// Every assertion goes through this rather than slicing `text` directly:
    /// the offsets are UTF-16 and Rust slices by byte, so `&text[a..b]` is
    /// right only for ASCII — which would make exactly the multi-byte case
    /// this crate must get right the one the tests could not see.
    fn covered(text: &str, start: usize, end: usize) -> String {
        text.chars()
            .scan(0usize, |at, ch| {
                let here = *at;
                *at += ch.len_utf16();
                Some((here, ch))
            })
            .filter(|(at, _)| *at >= start && *at < end)
            .map(|(_, ch)| ch)
            .collect()
    }

    /// A request whose whole text is prose — keeps the tests about behaviour,
    /// not arithmetic.
    fn whole(text: &str) -> CheckRequest {
        CheckRequest {
            text: text.to_owned(),
            spans: vec![SpanJs {
                start: 0,
                end: utf16_len(text),
            }],
            dictionary: Vec::new(),
            dialect: None,
        }
    }

    #[test]
    fn finds_a_misspelling_and_offers_the_right_word() {
        let out = check(&whole("The squre is empty."));
        let lint = out.lints.first().expect("a misspelling is reported");
        assert_eq!(
            covered("The squre is empty.", lint.start, lint.end),
            "squre"
        );
        assert!(
            lint.suggestions
                .iter()
                .any(|s| s.kind == "replace" && s.text == "square"),
            "expected `square` among {:?}",
            lint.suggestions
        );
    }

    #[test]
    fn machinery_outside_the_spans_is_never_checked() {
        // The divert and the tag hold no prose. Were they tokenized, `knot`
        // and `act1` would both report as misspellings — which is exactly the
        // failure this crate's masker exists to prevent.
        let text = "-> barter::hagle\nThe squre is empty.\n#act1";
        let prose_start = utf16_of(text, text.find("The").expect("fixture contains 'The'"));
        let prose_end = prose_start + utf16_len("The squre is empty.");
        let out = check(&CheckRequest {
            text: text.to_owned(),
            spans: vec![SpanJs {
                start: prose_start,
                end: prose_end,
            }],
            dictionary: Vec::new(),
            dialect: None,
        });

        assert_eq!(out.lints.len(), 1, "got {:?}", out.lints);
        let lint = &out.lints[0];
        assert_eq!(covered(text, lint.start, lint.end), "squre");
        // `hagle` in the divert is a misspelling of a real word and is NOT
        // reported — proving the mask, not just the absence of lints.
        assert!(
            !out.lints
                .iter()
                .any(|l| covered(text, l.start, l.end).contains("hagle"))
        );
    }

    #[test]
    fn spans_are_utf16_offsets_even_after_astral_text() {
        // The offset bug this design exists to avoid. Harper indexes by char;
        // the boundary is UTF-16. An emoji is one char and TWO units, so a
        // char-indexed span would point one unit short per emoji ahead of it —
        // and an accented char (one char, one unit, two BYTES) would be off in
        // the other direction under a byte-indexed one. Both are in this
        // fixture, so a span that survives it is right in the only sense that
        // matters to `CodeMirror`.
        let text = "The café 🎭 — warm, bright 🎭 — was squre.";
        let out = check(&whole(text));
        let lint = out
            .lints
            .iter()
            .find(|l| covered(text, l.start, l.end) == "squre")
            .expect("the misspelling after astral text is found at the right offset");

        // Pin the arithmetic too: `covered` and the checker could in principle
        // share a mistake, so state the expected offset independently.
        let expected = utf16_of(text, text.find("squre").expect("fixture has squre"));
        assert_eq!(lint.start, expected);
    }

    #[test]
    fn seeded_dictionary_silences_an_invented_name() {
        // The measured make-or-break case (#3210): unseeded, Harper reports
        // `Kaelen` and suggests "Karen".
        let text = "Kaelen nodded at the warden.";
        let unseeded = check(&whole(text));
        assert!(
            unseeded
                .lints
                .iter()
                .any(|l| covered(text, l.start, l.end) == "Kaelen"),
            "fixture assumption: unseeded Harper flags the invented name"
        );

        let seeded = check(&CheckRequest {
            text: text.to_owned(),
            spans: vec![SpanJs {
                start: 0,
                end: utf16_len(text),
            }],
            dictionary: vec!["Kaelen".to_owned()],
            dialect: None,
        });
        assert!(
            !seeded
                .lints
                .iter()
                .any(|l| covered(text, l.start, l.end) == "Kaelen"),
            "seeding the project's own names must silence it; got {:?}",
            seeded.lints
        );
    }

    #[test]
    fn dialect_decides_whether_british_spelling_is_an_error() {
        let text = "The colour of the harbour at night.";
        let american = check(&whole(text));
        assert!(
            american
                .lints
                .iter()
                .any(|l| covered(text, l.start, l.end) == "colour"),
            "American dialect flags `colour`; got {:?}",
            american.lints
        );

        let british = check(&CheckRequest {
            text: text.to_owned(),
            spans: vec![SpanJs {
                start: 0,
                end: utf16_len(text),
            }],
            dictionary: Vec::new(),
            dialect: Some("british".to_owned()),
        });
        assert!(
            !british
                .lints
                .iter()
                .any(|l| covered(text, l.start, l.end) == "colour"),
            "British dialect accepts `colour`; got {:?}",
            british.lints
        );
    }

    #[test]
    fn no_spans_means_no_work_and_no_lints() {
        let out = check(&CheckRequest {
            text: "The squre is empty.".to_owned(),
            spans: Vec::new(),
            dictionary: Vec::new(),
            dialect: None,
        });
        assert!(out.lints.is_empty());
    }

    #[test]
    fn a_malformed_request_yields_no_lints_rather_than_throwing() {
        assert_eq!(super::check_prose("not json"), r#"{"lints":[]}"#);
    }
}
