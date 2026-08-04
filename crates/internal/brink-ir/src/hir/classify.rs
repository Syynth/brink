//! The interactive classification walk (issue #2112, NS-T seam 2/6) — the
//! mechanical, per-invocation answer to "what would claim this line, and
//! what else would have?", read straight off the project's
//! [`ConventionsProjection`] (issue #2111) with no compiler pipeline, no
//! salsa, and no handler execution involved.
//!
//! # Ruled semantics (2026-08-02, do not re-derive)
//!
//! Unlike the compiler's own claiming walk
//! ([`crate::hir::lower_native::element::try_claim`], which stops at the
//! first handler whose pattern matches — it is rewriting the line into one
//! call, so it only ever needs the winner), this walk tries **every**
//! entry in `projection.entries` against the line, in the projection's own
//! ascending-`order` sequence. The first entry that matches is the winner;
//! every OTHER entry that also matches is recorded as **shadowed**, not
//! merely "attempted" — a static regex-intersection heuristic cannot say
//! whether two patterns actually collide on a *given* line the way running
//! both regexes can, and continuing past the first hit is what makes
//! #2113's explain-match query exact on a hit as well as a miss (naming
//! what else *would* have matched, not just what was tried).
//!
//! `docs/decision-log.md`'s "conventions module confinement (#2167); the
//! classification walk is Rust-only (#2112)" entry (2026-08-03) settles
//! *where* this runs: **Rust only, exposed to the TS editor through
//! wasm.** A TS-side reimplementation would mean two regex engines that
//! must agree exactly, forever, on every pattern an author writes — this
//! function is the ONE implementation, and a wasm-exposed caller (or the
//! compiler itself, if it ever wants this walk instead of `try_claim`'s
//! stop-at-first-hit shape) gets identical answers from it.
//!
//! # Raw captures, not computed values
//!
//! This walk runs the regex, so a match's [`ClassifiedCapture`]s are real
//! (`name = "VENDOR"`); it never runs the matched handler's body, so a
//! computed value the handler's own code would derive (`voiceover =
//! true`) is never available here and never will be — see
//! `docs/decision-log.md`'s 2026-08-03 "Conventions × the editor" entry,
//! item (5). A consumer of this type must not imply it knows more than
//! this.
//!
//! # What this module deliberately does not do
//!
//! - **No memoization.** The ruled cost compensation — "memoize on `(line
//!   text, projection revision)`" — is a caller concern: a salsa query
//!   wrapping this function (keyed on the line's own text, depending on
//!   [`ConventionsProjection`] the ordinary salsa way so a projection edit
//!   invalidates it) or a small cache the wasm boundary keeps across
//!   keystrokes. Building that cache is left to whichever seam wires this
//!   walk to a live query (#2113's explain-match query is the first
//!   consumer) — inventing a caching layer with no consumer yet risks
//!   locking in the wrong shape, exactly the reasoning
//!   [`ConventionsProjection`]'s own doc gives for deferring its `.inkb`
//!   wire emission.
//! - **No line-kind detection.** Whether a line is a scene heading, a cue,
//!   plain content, etc. ([`crate::ElementKind`]) is a structural fact
//!   about the line's own syntax shape, computed once already by
//!   [`crate::hir::lower_native::element::candidate`] at compile time (and
//!   by the editor's own structural facet interactively) — reusing that
//!   classification is the caller's job, not this walk's; a "matched kind"
//!   column in a composed per-line record is the caller pairing that fact
//!   alongside this function's `LineClassification`, not this module
//!   inventing a second way to derive it.
//! - **No wasm binding.** Follows from the point above: a bare wasm export
//!   of this pure function would have nothing driving it (no memoized
//!   query, no consumer UI) until #2113 exists — see that issue's own
//!   design-backport comment, which reads this exact projection and is
//!   subject to this exact raw-captures boundary.

use regex::Regex;
use rowan::{TextRange, TextSize};

use super::types::{ConventionMode, ConventionsProjection, ElementDisposition, Name};

/// One named capture a matched pattern bound, as a span into the **real
/// source** the caller handed this walk — never a copied string alone
/// (mirrors [`crate::ElementCapture`]'s own no-invisible-expansion
/// guarantee, for this interactive walk rather than the compiler's
/// lowering-time record). Doubles as an editor decoration range, per the
/// classification-metadata requirement (`docs/decision-log.md`,
/// "No invisible expansion").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedCapture {
    /// The capture group's name.
    pub name: String,
    /// The captured text.
    pub text: String,
    /// Where in the source the capture came from.
    pub range: TextRange,
}

/// One handler's classification-time match — the winner if this is
/// [`LineClassification::matched`], or one of the runners-up if it is one
/// of [`LineClassification::shadowed`]. The two cases carry identically
/// shaped data: only the walk's own placement of this hit in the
/// ascending-`order` sequence decides which bucket it lands in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedMatch {
    /// The matched handler's own name, carrying its declaration-site range
    /// — the "fn + source location" half of the classification-metadata
    /// requirement, read straight off
    /// [`crate::ConventionProjectionEntry::name`].
    pub handler: Name,
    /// The claiming precedence this entry was tried at
    /// ([`crate::ConventionProjectionEntry::order`]) — `matched.order` is
    /// always the lowest among every hit this walk recorded for the line;
    /// every `shadowed` entry's `order` is strictly greater.
    pub order: i64,
    /// Attach or wrap — see [`ConventionMode`]'s own doc.
    pub mode: ConventionMode,
    /// What a match on this handler would produce — see
    /// [`ElementDisposition`]'s own doc for why this is carried as a real
    /// field.
    pub disposition: ElementDisposition,
    /// This handler's pattern's named captures, bound from the actual
    /// match against the line — real source spans, never computed values
    /// (see this module's own doc).
    pub captures: Vec<ClassifiedCapture>,
}

/// One line's classification result (issue #2112). Every registered
/// pattern was tried, in the projection's own ascending-`order` sequence;
/// the first to match is `matched`, and every OTHER pattern that also
/// matched is `shadowed`, in that same ascending-`order` sequence. Both
/// are `None`/empty when nothing matched at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineClassification {
    /// The winning handler, if any pattern matched at all.
    pub matched: Option<ClassifiedMatch>,
    /// Every other handler whose pattern also matched, ascending by
    /// `order` — always empty when `matched` is `None` (nothing can be
    /// shadowed by a match that never happened).
    pub shadowed: Vec<ClassifiedMatch>,
}

impl LineClassification {
    /// `true` when nothing matched at all — no winner, nothing shadowed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.matched.is_none() && self.shadowed.is_empty()
    }
}

/// Run every entry in `projection` against `text`, in the projection's own
/// ascending-`order` sequence (issue #2112's ruled semantics — see this
/// module's own doc). Never stops at the first hit: every entry is tried,
/// every hit is recorded, and the first hit becomes
/// [`LineClassification::matched`] while the rest become
/// [`LineClassification::shadowed`].
///
/// `base` is the absolute source offset `text`'s own start sits at, so
/// every [`ClassifiedCapture::range`] this returns points at real source —
/// the same `base`-plus-local-offset composition
/// [`crate::hir::lower_native::element::try_claim`] uses for
/// [`crate::ElementCapture::range`]. A caller classifying an in-memory
/// line with no real source position yet (e.g. scratch text) may pass
/// [`TextSize::from`]`(0)` and treat the resulting ranges as
/// text-relative.
///
/// A pattern that fails to compile is skipped rather than treated as a
/// match or propagated as an error: every entry
/// [`ConventionsProjection::from_decls`] can produce was already compiled
/// successfully once, upstream, to exist as a
/// [`crate::ClaimHandlerDecl`] at all (`collect`'s own
/// `regex::Regex::new(...).ok()?` gate) — recompiling here can only fail
/// if that invariant is somehow violated, and silently declining a
/// hypothetically-broken entry is safer than panicking on a line an
/// author is actively typing.
#[must_use]
pub fn classify_line(
    projection: &ConventionsProjection,
    base: TextSize,
    text: &str,
) -> LineClassification {
    let mut hits = Vec::with_capacity(projection.entries.len());
    for entry in &projection.entries {
        let Ok(pattern) = Regex::new(&entry.pattern) else {
            continue;
        };
        let Some(caps) = pattern.captures(text) else {
            continue;
        };
        let Some(captures) = bind_captures(&pattern, &caps, base) else {
            // A capture's byte offset does not fit a `u32` — cannot be a
            // real span on any line short enough for an editor to hold in
            // memory, but declined rather than faked (house rule: never
            // fabricate data) if it were ever somehow reached.
            continue;
        };
        hits.push(ClassifiedMatch {
            handler: entry.name.clone(),
            order: entry.order,
            mode: entry.mode,
            disposition: entry.disposition,
            captures,
        });
    }
    let mut hits = hits.into_iter();
    let matched = hits.next();
    LineClassification {
        matched,
        shadowed: hits.collect(),
    }
}

/// Bind every named capture group `pattern` declares to its span in this
/// particular match, in the group's declaration order (the `regex` crate's
/// own `capture_names()` contract) — `None` if any offset does not fit a
/// `u32`, so the caller can decline the whole hit rather than emit a
/// partially-bogus capture list.
fn bind_captures(
    pattern: &Regex,
    caps: &regex::Captures<'_>,
    base: TextSize,
) -> Option<Vec<ClassifiedCapture>> {
    pattern
        .capture_names()
        .flatten()
        .filter_map(|name| caps.name(name).map(|m| (name, m)))
        .map(|(name, m)| {
            let start = u32::try_from(m.start()).ok()?;
            let end = u32::try_from(m.end()).ok()?;
            Some(ClassifiedCapture {
                name: name.to_string(),
                text: m.as_str().to_string(),
                range: TextRange::new(base + TextSize::from(start), base + TextSize::from(end)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{ClaimHandlerDecl, ConventionAttachField};

    fn name(text: &str) -> Name {
        Name {
            text: text.to_string(),
            range: TextRange::default(),
        }
    }

    fn decl(name_text: &str, order: i64, pattern: &str) -> ClaimHandlerDecl {
        ClaimHandlerDecl {
            name: name(name_text),
            annotation: TextRange::default(),
            params: Vec::new(),
            pattern: pattern.to_string(),
            block: false,
            order,
            attach: None,
        }
    }

    fn no_structs() -> BTreeMap<String, Vec<ConventionAttachField>> {
        BTreeMap::new()
    }

    fn projection(decls: &[ClaimHandlerDecl]) -> ConventionsProjection {
        ConventionsProjection::from_decls(decls, &no_structs())
    }

    #[test]
    fn no_entry_matching_yields_empty_classification() {
        let p = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let result = classify_line(&p, TextSize::from(0), "EXT. THE DOCK");
        assert!(result.is_empty());
        assert_eq!(result.matched, None);
        assert!(result.shadowed.is_empty());
    }

    #[test]
    fn a_single_matching_entry_becomes_the_winner_with_no_shadows() {
        let p = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let result = classify_line(&p, TextSize::from(0), "INT. MARKET SQUARE");
        let matched = result.matched.expect("expected a match");
        assert_eq!(matched.handler.text, "interior");
        assert_eq!(matched.order, 10);
        assert!(result.shadowed.is_empty());
    }

    /// The ruled semantics (2026-08-02): the walk keeps going and records
    /// every other matching entry as shadowed, in ascending `order` — not
    /// merely the first hit. This is the core behavior #2112 exists for.
    #[test]
    fn every_other_matching_entry_is_recorded_as_shadowed_in_order() {
        let p = projection(&[
            decl("any_line", 10, "^.*$"),
            decl("also_any_line", 20, "^.*$"),
            decl("still_any_line", 30, "^.*$"),
        ]);
        let result = classify_line(&p, TextSize::from(0), "INT. MARKET SQUARE");
        let matched = result.matched.expect("expected a match");
        assert_eq!(matched.handler.text, "any_line", "lowest order wins");
        let shadowed_names: Vec<&str> = result
            .shadowed
            .iter()
            .map(|m| m.handler.text.as_str())
            .collect();
        assert_eq!(
            shadowed_names,
            vec!["also_any_line", "still_any_line"],
            "every other match is recorded, in ascending order — not just \
             one, and not dropped"
        );
    }

    /// A non-matching entry sandwiched between two matching ones must not
    /// appear as a phantom shadow — only entries whose pattern actually
    /// matched this specific line count.
    #[test]
    fn a_non_matching_entry_between_two_hits_is_not_shadowed() {
        let p = projection(&[
            decl("interior", 10, "^INT\\. (?<place>.+)$"),
            decl("exterior_only", 20, "^EXT\\. .+$"),
            decl("any_line", 30, "^.*$"),
        ]);
        let result = classify_line(&p, TextSize::from(0), "INT. MARKET SQUARE");
        let matched = result.matched.expect("expected a match");
        assert_eq!(matched.handler.text, "interior");
        let shadowed_names: Vec<&str> = result
            .shadowed
            .iter()
            .map(|m| m.handler.text.as_str())
            .collect();
        assert_eq!(
            shadowed_names,
            vec!["any_line"],
            "exterior_only never matched this line and must not appear"
        );
    }

    /// Captures are raw regex bindings, real spans into the given text —
    /// never a value a handler would have computed (see this module's own
    /// doc, "Raw captures, not computed values").
    #[test]
    fn captures_are_bound_as_real_spans_at_the_given_base() {
        let p = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let result = classify_line(&p, TextSize::from(100), "INT. MARKET SQUARE");
        let matched = result.matched.expect("expected a match");
        assert_eq!(matched.captures.len(), 1);
        let capture = &matched.captures[0];
        assert_eq!(capture.name, "place");
        assert_eq!(capture.text, "MARKET SQUARE");
        // "INT. " is 5 bytes; base 100 + 5 = 105 is where `place` starts,
        // and "MARKET SQUARE" is 13 bytes long.
        assert_eq!(capture.range, TextRange::new(105.into(), 118.into()));
    }

    #[test]
    fn mode_and_disposition_are_carried_through_from_the_projection_entry() {
        let mut decl_wrap = decl("cue", 10, "^@(?<who>.+)$");
        decl_wrap.block = true;
        let p = projection(&[decl_wrap]);
        let result = classify_line(&p, TextSize::from(0), "@VENDOR");
        let matched = result.matched.expect("expected a match");
        assert_eq!(matched.mode, ConventionMode::Wrap);
        assert_eq!(matched.disposition, ElementDisposition::Call);
    }

    #[test]
    fn an_empty_projection_never_matches_anything() {
        let p = projection(&[]);
        let result = classify_line(&p, TextSize::from(0), "anything at all");
        assert!(result.is_empty());
    }
}
