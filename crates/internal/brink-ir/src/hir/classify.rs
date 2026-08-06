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
//! - **No memoization here.** **Built by #2113**, one layer up:
//!   [`crate::ExplainMatchCache`] memoizes on `(line text, projection)` —
//!   the ruled cost compensation — and additionally caches the *compiled
//!   pattern set* per projection (`compile_entries`/`classify_line_compiled`
//!   in this module), since the w133 review finding on PR #2257 measured
//!   per-entry `Regex::new` compilation, not matching, as the dominant cost
//!   of a *first* classification. This module still never caches anything
//!   itself — `classify_line` alone stays a pure, no-cache function of its
//!   inputs, exactly as before.
//! - **No line-kind detection, still, in `classify_line`/
//!   `classify_line_compiled` themselves.** Whether a line is a scene
//!   heading, a cue, plain content, etc. ([`crate::ElementKind`]) is a
//!   structural fact about the line's own syntax shape, computed once
//!   already by [`crate::hir::lower_native::element::candidate`] at
//!   compile time — a full-CST classification (parenthetical is
//!   chain-gated: it only exists directly after a live cue, so a *single
//!   line of bare text* cannot answer it correctly in isolation). **#2113
//!   decided not to compose this into the bare-text entry points** — see
//!   [`crate::ExplainMatchCache`]'s own module doc for the reasoning.
//!   `classify_node_compiled`/[`nearest_element_candidate`] (issue #2351,
//!   below) are the traced follow-up for a caller that DOES hold a parsed
//!   node: they still don't return an [`crate::ElementKind`] of their own
//!   (that composition stays #2310's job, one layer up, reading the
//!   compiler's own compiled record), but they DO select the same
//!   sub-node `candidate` would, so the winner they report agrees with
//!   the compiler by construction rather than by accident.
//! - **No wasm binding here.** **Built by #2113** in `@brink-lang/web`'s
//!   `EditorSession::explain_match`, which wraps
//!   [`crate::ExplainMatchCache`] — see that module's doc for the JSON
//!   shape. This module itself still exports nothing wasm-specific.

use brink_syntax_native::SyntaxNode;
use regex::Regex;
use rowan::{TextRange, TextSize};

use super::types::{
    ConventionAttachSchema, ConventionMode, ConventionsProjection, ElementDisposition, Name,
};

// ─── Compiled-pattern caching (issue #2113) ──────────────────────────
//
// `classify_line` below compiles every entry's pattern fresh, every call —
// fine for a one-off classification, but the w133 review finding on PR
// #2257 measured this as the dominant cost for the *first* classification
// of each distinct line (the ruled `(line text, projection revision)` memo
// only helps a *repeat* classification of the same line). A caller that
// classifies many distinct lines against the same, unchanging projection —
// [`crate::ExplainMatchCache`], the first such caller — compiles each
// pattern here exactly once per projection, mirroring the precedent the
// finding names: the TS-side `ResolvedDialect` is "the only place regex
// compilation happens; classifying a line never re-compiles."

/// One [`crate::ConventionProjectionEntry`], with its pattern already
/// compiled. Exists only for [`compile_entries`]/[`classify_line_compiled`]
/// — a caller with a single line to classify against a projection it will
/// never reuse should keep calling [`classify_line`], not build one of
/// these for a single use.
#[derive(Debug, Clone)]
pub(crate) struct CompiledEntry {
    name: Name,
    order: i64,
    mode: ConventionMode,
    disposition: ElementDisposition,
    attach: Option<ConventionAttachSchema>,
    pattern: Regex,
}

/// Compile every entry in `projection`, once, in the projection's own
/// ascending-`order` sequence — the shared building block behind
/// [`classify_line_compiled`]. A pattern that fails to compile is skipped
/// rather than treated as an error, for the exact reason
/// [`classify_line`]'s own doc gives: every entry
/// [`ConventionsProjection::from_decls`] can produce was already compiled
/// successfully once, upstream, to exist as a
/// [`crate::ClaimHandlerDecl`] at all.
#[must_use]
pub(crate) fn compile_entries(projection: &ConventionsProjection) -> Vec<CompiledEntry> {
    projection
        .entries
        .iter()
        .filter_map(|entry| {
            Some(CompiledEntry {
                name: entry.name.clone(),
                order: entry.order,
                mode: entry.mode,
                disposition: entry.disposition,
                attach: entry.attach.clone(),
                pattern: Regex::new(&entry.pattern).ok()?,
            })
        })
        .collect()
}

/// Trim `text` and compute the base-adjusted start offset, exactly
/// [`classify_line`]'s own "Trim contract" — factored out so
/// [`classify_line_compiled`] shares it instead of re-deriving it. `None`
/// means nothing should be tried at all (a whitespace-only line, or an
/// offset that cannot fit a `u32`).
fn trim_for_classification(base: TextSize, text: &str) -> Option<(TextSize, &str)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lead = u32::try_from(text.len() - text.trim_start().len()).ok()?;
    Some((base + TextSize::from(lead), trimmed))
}

/// The walk's shared core: run every already-compiled `compiled` entry
/// against `trimmed` (already trimmed, per [`trim_for_classification`]), in
/// order — see [`classify_line`]'s own doc for the ruled semantics this
/// implements. Both [`classify_line`] and [`classify_line_compiled`]
/// delegate here so the walk itself is written exactly once.
fn classify_trimmed(
    compiled: &[CompiledEntry],
    base: TextSize,
    trimmed: &str,
) -> LineClassification {
    let mut hits = Vec::with_capacity(compiled.len());
    for entry in compiled {
        let Some(caps) = entry.pattern.captures(trimmed) else {
            continue;
        };
        let Some(captures) = bind_captures(&entry.pattern, &caps, base) else {
            // Either a capture's byte offset does not fit a `u32` (cannot
            // be a real span on any line short enough for an editor to
            // hold in memory), or a named group declared by the pattern
            // failed to participate in this particular match — e.g. an
            // alternation branch that did not fire. `try_claim` declines
            // the claim entirely in that second case (`caps.name(param)?`
            // returns from the whole function), and this walk mirrors that
            // exactly: such an entry is neither `matched` nor `shadowed`,
            // not reported with a partial capture list. This is a
            // deliberate divergence from a *static* "these patterns could
            // collide" heuristic — the compiler's refusal is total, so
            // this walk's is too.
            continue;
        };
        hits.push(ClassifiedMatch {
            handler: entry.name.clone(),
            order: entry.order,
            mode: entry.mode,
            disposition: entry.disposition,
            attach: entry.attach.clone(),
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

/// [`classify_line`], but against an already-[`compile_entries`]-compiled
/// pattern set — the entry point [`crate::ExplainMatchCache`] uses so
/// classifying many lines against one projection compiles nothing after the
/// first line. Semantics are identical to [`classify_line`] in every other
/// respect (same trim contract, same declined-entirely rule); only the
/// compile step moves to the caller.
#[must_use]
pub(crate) fn classify_line_compiled(
    compiled: &[CompiledEntry],
    base: TextSize,
    text: &str,
) -> LineClassification {
    let Some((base, trimmed)) = trim_for_classification(base, text) else {
        return LineClassification::default();
    };
    classify_trimmed(compiled, base, trimmed)
}

// ─── Node-aware classification (issue #2351) ─────────────────────────
//
// `classify_line`/`classify_line_compiled` above match a pattern against
// the WHOLE line text handed to them. `crate::hir::lower_native::element`'s
// own `candidate`/`try_claim` — the compiler's real claiming path — never
// does that: it selects a SUB-NODE first (a `CUE`'s `CUE_NAME` alone, a
// `PARENTHETICAL`'s inner `TEXT` alone, a `SCENE_HEADING`'s `SCENE_TITLE`
// stripped of its slug/tags, a `COMPACT_CUE`'s `CUE_NAME` segment alone)
// and matches only that. A caller that hands this module the raw line for
// one of those four shapes structurally cannot agree with the compiler —
// the two matchers are seeing different text. The functions below are for
// a caller that DOES hold a parsed CST node (the editor, via a real
// document's syntax tree) and wants the walk's answer to agree with
// `try_claim`'s, by construction, rather than by keeping two independent
// copies of the same selection rules in sync by hand.

/// Classify the CST node `node` — one of the five shapes
/// [`crate::hir::lower_native::element::candidate`] recognizes (`CUE`,
/// `COMPACT_CUE`, `PARENTHETICAL`, `SCENE_HEADING`, or a wholly-literal
/// `CONTENT_LINE`) — against an already-[`compile_entries`]-compiled
/// pattern set, matching the exact sub-node text `candidate`/`try_claim`
/// extract for that shape (issue #2351) rather than `node`'s own whole
/// text.
///
/// Ranges in the result are relative to `node`'s own start, as if `node`
/// began at offset zero — mirrors [`classify_line`]'s own
/// zero-based-then-rebase convention (see its own doc), so a caller (e.g.
/// [`crate::ExplainMatchCache`]) can shift the result onto `node`'s real
/// position the same way it already does for the raw-text path.
///
/// `None` when `node`'s own kind is not one of the five shapes `candidate`
/// recognizes at all (e.g. a knot/stitch header, a logic line, a
/// `BANG_DISPATCH`) — the caller should fall back to
/// [`classify_line_compiled`] against `node`'s own raw text in that case,
/// exactly the pre-#2351 behavior for anything outside these five shapes.
#[must_use]
pub(crate) fn classify_node_compiled(
    compiled: &[CompiledEntry],
    node: &SyntaxNode,
) -> Option<LineClassification> {
    let node_start = node.text_range().start();
    let (_kind, text_node) = super::lower_native::candidate(node)?;
    // `text_node` is always a descendant of `node` (one of its children, or
    // a child's child for `SCENE_HEADING`), so its start never precedes
    // `node`'s own — this subtraction never underflows.
    let local_base = text_node.text_range().start() - node_start;
    let text = text_node.text().to_string();
    Some(classify_line_compiled(compiled, local_base, &text))
}

/// The innermost node in `node`'s own ancestor chain (`node` itself,
/// first) that is one of the five CST shapes
/// [`crate::hir::lower_native::element::candidate`] recognizes as a claim
/// candidate — issue #2351. Lets a caller that only holds a token/offset
/// derived starting point (e.g. `root.token_at_offset(offset)` then
/// `.parent()`) find the right node to hand to
/// [`classify_node_compiled`]/[`crate::explain_match_node`], without
/// keeping its own copy of `candidate`'s kind list — asking `candidate`
/// itself, on each ancestor in turn, is what keeps this from re-diverging
/// the way #2351 itself happened.
///
/// `None` if no ancestor up to the file root is one of those five shapes at
/// all (e.g. `offset` lands on a knot/stitch header or a logic line) — the
/// caller should fall back to the raw-text walk in that case.
#[must_use]
pub fn nearest_element_candidate(node: &SyntaxNode) -> Option<SyntaxNode> {
    std::iter::successors(Some(node.clone()), SyntaxNode::parent)
        .find(|n| super::lower_native::candidate(n).is_some())
}

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
    /// The `attach = StructName` clause's resolution outcome, read straight
    /// off [`crate::ConventionProjectionEntry::attach`] — `None` for a
    /// handler that only ever emits text. Carried through unchanged, the
    /// same way `mode`/`disposition` are: this walk composes the
    /// projection's own declarative data, it never re-resolves it (see this
    /// module's own doc).
    pub attach: Option<ConventionAttachSchema>,
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
///
/// # Trim contract
///
/// `text` is trimmed before any pattern is tried, exactly the convention
/// [`crate::hir::lower_native::element::try_claim`] and the TS editor's own
/// classifier (`packages/ink-editor/src/dialect.ts`) both use: `base` is
/// advanced by the leading-whitespace byte count so every returned range
/// still lands on real source, and a whitespace-only `text` short-circuits
/// to [`LineClassification::default`] (nothing can match an empty line, and
/// the compiler never tries). Without this, an indented line classifies
/// differently here than in the compiler, and `^.*$` would "match" a blank
/// line the compiler explicitly refuses.
#[must_use]
pub fn classify_line(
    projection: &ConventionsProjection,
    base: TextSize,
    text: &str,
) -> LineClassification {
    let Some((base, trimmed)) = trim_for_classification(base, text) else {
        return LineClassification::default();
    };
    // Compiled fresh, every call — the right choice for a one-off
    // classification. A caller classifying many lines against the same
    // projection should use [`classify_line_compiled`] instead (via
    // [`crate::ExplainMatchCache`]) — see this module's own "Compiled-pattern
    // caching" doc for why.
    let compiled = compile_entries(projection);
    classify_trimmed(&compiled, base, trimmed)
}

/// Bind every named capture group `pattern` declares to its span in this
/// particular match, in the group's declaration order (the `regex` crate's
/// own `capture_names()` contract) — `None` if *any* declared named group
/// failed to participate in this match (e.g. the losing branch of an
/// alternation) or any offset does not fit a `u32`, so the caller declines
/// the whole hit rather than emit a capture list with a group silently
/// missing from it. E160/E167 pin named captures ≡ params exactly, so a
/// non-participating group here is the same condition `try_claim`'s own
/// `caps.name(param)?` declines a claim for.
fn bind_captures(
    pattern: &Regex,
    caps: &regex::Captures<'_>,
    base: TextSize,
) -> Option<Vec<ClassifiedCapture>> {
    pattern
        .capture_names()
        .flatten()
        .map(|name| {
            let m = caps.name(name)?;
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

    /// #2311: a hit's `attach` schema must be the same resolved value the
    /// projection entry carries — this walk composes #2111's own data, it
    /// never drops it on the way through `CompiledEntry`/`ClassifiedMatch`.
    /// Reverting `attach: entry.attach.clone()` in `compile_entries`/
    /// `classify_trimmed` (leaving the field defaulted or unset) fails this
    /// test.
    #[test]
    fn a_hit_carries_the_resolved_attach_schema_through_from_the_projection_entry() {
        let mut decl_with_attach = decl("cue", 10, "^(?<who>[A-Z]+)$");
        decl_with_attach.attach = Some("Cue".to_string());
        let mut structs = BTreeMap::new();
        structs.insert(
            "Cue".to_string(),
            vec![ConventionAttachField {
                name: "who".to_string(),
                ty: crate::SchemaTypeShape::Named("string".to_string()),
            }],
        );
        let p = ConventionsProjection::from_decls(&[decl_with_attach], &structs);
        let result = classify_line(&p, TextSize::from(0), "VENDOR");
        let matched = result.matched.expect("expected a match");
        assert_eq!(
            matched.attach,
            Some(ConventionAttachSchema::Resolved {
                name: "Cue".to_string(),
                fields: vec![ConventionAttachField {
                    name: "who".to_string(),
                    ty: crate::SchemaTypeShape::Named("string".to_string()),
                }],
            })
        );
    }

    /// The unresolved case (a declared `attach = StructName` naming a struct
    /// that does not exist) must also survive the walk, carrying the
    /// declared name rather than being silently dropped or coerced to
    /// `None` (house rule: flag silent data drops).
    #[test]
    fn a_hit_carries_an_unresolved_attach_schema_through_too() {
        let mut decl_with_attach = decl("cue", 10, "^(?<who>[A-Z]+)$");
        decl_with_attach.attach = Some("NoSuchStruct".to_string());
        let p = ConventionsProjection::from_decls(&[decl_with_attach], &no_structs());
        let result = classify_line(&p, TextSize::from(0), "VENDOR");
        let matched = result.matched.expect("expected a match");
        assert_eq!(
            matched.attach,
            Some(ConventionAttachSchema::Unresolved(
                "NoSuchStruct".to_string()
            ))
        );
    }

    #[test]
    fn an_empty_projection_never_matches_anything() {
        let p = projection(&[]);
        let result = classify_line(&p, TextSize::from(0), "anything at all");
        assert!(result.is_empty());
    }

    /// An alternation where only one branch's named group participates
    /// must be declined entirely — neither `matched` nor `shadowed` — the
    /// same total refusal `try_claim`'s `caps.name(param)?` gives, since
    /// E160/E167 pin named captures to params exactly and a group that
    /// didn't fire is a call with a missing argument.
    #[test]
    fn an_entry_with_a_non_participating_named_group_is_declined_entirely() {
        let p = projection(&[decl(
            "interior",
            10,
            "^(?:INT\\. (?<place>.+)|EXT\\. (?<outside>.+))$",
        )]);
        let result = classify_line(&p, TextSize::from(0), "INT. MARKET");
        assert!(
            result.is_empty(),
            "the `outside` group never participated on this branch, so the \
             whole entry must be declined, not reported as a partial match"
        );
    }

    /// The same declined-entirely rule applies when the non-participating
    /// entry would otherwise have been shadowed, not just when it would
    /// have won.
    #[test]
    fn a_non_participating_entry_is_not_recorded_as_shadowed_either() {
        let p = projection(&[
            decl("any_line", 10, "^.*$"),
            decl(
                "interior_or_exterior",
                20,
                "^(?:INT\\. (?<place>.+)|EXT\\. (?<outside>.+))$",
            ),
        ]);
        let result = classify_line(&p, TextSize::from(0), "INT. MARKET");
        let matched = result.matched.expect("expected a match");
        assert_eq!(matched.handler.text, "any_line");
        assert!(
            result.shadowed.is_empty(),
            "interior_or_exterior's `outside` group never participated, so \
             it must not appear as a shadow"
        );
    }

    /// Leading whitespace is trimmed before matching, and `base` is
    /// advanced by the leading-whitespace byte count so capture ranges
    /// still land on real source — mirrors `try_claim`'s own contract.
    #[test]
    fn an_indented_line_is_trimmed_before_matching_and_captures_land_on_real_source() {
        let p = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let result = classify_line(&p, TextSize::from(100), "    INT. MARKET SQUARE");
        let matched = result.matched.expect("expected a match after trimming");
        assert_eq!(matched.handler.text, "interior");
        let capture = &matched.captures[0];
        assert_eq!(capture.text, "MARKET SQUARE");
        // 4 leading spaces + "INT. " (5 bytes) = 9; base 100 + 9 = 109 is
        // where `place` starts, and "MARKET SQUARE" is 13 bytes long.
        assert_eq!(capture.range, TextRange::new(109.into(), 122.into()));
    }

    /// A whitespace-only line never matches anything, even a pattern that
    /// would otherwise match any text at all — the compiler never tries
    /// `try_claim` on one either.
    #[test]
    fn a_whitespace_only_line_never_matches_anything() {
        let p = projection(&[decl("any_line", 10, "^.*$")]);
        let result = classify_line(&p, TextSize::from(0), "   \t  ");
        assert!(result.is_empty());
    }

    // ─── Node-aware classification (issue #2351) ─────────────────────

    /// The four claim-candidate shapes this issue names explicitly: a
    /// block cue (`@VENDOR`), a chain-gated parenthetical (`(hushed)`), a
    /// compact cue with genuinely interpolated fused dialogue
    /// (`@KID: I have {count} coins.`), and a slugged/tag-bearing scene
    /// heading — mirrors `tests/tier1-native/conventions-screenplay-preset/
    /// story.brink`'s own fixture shape.
    fn node_agreement_fixture_src() -> &'static str {
        "\
var count = 3

@[convention(claims = \"^(?<kind>INT|EXT)\\\\. (?<title>.+)$\", order = 10)]
fn heading(kind: string, title: string) {
  return title;
}

@[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", order = 20)]
fn cue(name: string) {
  return name;
}

@[convention(claims = \"^(?<delivery>[a-z][a-z' -]*)$\", order = 30)]
fn parenthetical(delivery: string) {
  return delivery;
}

flow main() {
  INT. MARKET SQUARE - NIGHT [market] #act1
  @VENDOR
  (hushed)
  @KID: I have {count} coins.
  -> END
}
"
    }

    /// Lower `src` through the real native frontend, returning the compiled
    /// `HirFile` (whose `element_matches` is the compiler's own claiming
    /// record, `try_claim`'s output) alongside the root [`SyntaxNode`] the
    /// same parse produced — so a test can find the exact node `try_claim`
    /// saw for each claimed line.
    fn lower_src(src: &str) -> (crate::HirFile, SyntaxNode) {
        use brink_syntax_native::ast::AstNode as _;

        let parse = brink_syntax_native::parse(src);
        let tree = parse.tree();
        let (hir, _manifest, diags) = crate::hir::lower_native::lower(crate::FileId(0), &tree);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        (hir, tree.syntax().clone())
    }

    /// The projection a real file's own declared `@[convention]` handlers
    /// give, exactly as `brink_db::queries::analysis::conventions_projection_query`
    /// builds it from `HirFile::claim_handlers` — none of the fixture's
    /// handlers declares `attach`, so an empty struct map is enough.
    fn projection_from(hir: &crate::HirFile) -> ConventionsProjection {
        ConventionsProjection::from_decls(&hir.claim_handlers, &BTreeMap::new())
    }

    /// Find the node `try_claim` claimed for `elm` (via
    /// [`nearest_element_candidate`], starting from the token at the
    /// claimed line's own start) and assert [`classify_node_compiled`]'s
    /// answer for it agrees with `elm` — the compiler's own compiled
    /// record — on both the winning handler and every capture's text and
    /// span. This is issue #2351's own ask: "assert winner + captures
    /// against the compiled `ElementMatch` for the same fixture" (the
    /// agreement-guard precedent PR #2328's review names).
    fn assert_agrees_with_compiled(
        root: &SyntaxNode,
        compiled: &[CompiledEntry],
        elm: &crate::ElementMatch,
    ) {
        let token = root
            .token_at_offset(elm.line.start())
            .right_biased()
            .expect("a real token must start at the claimed line's own start");
        let start_node = token
            .parent()
            .expect("every token in a well-formed tree has a parent node");
        let not_a_candidate_msg = format!(
            "the compiler claimed this line ({:?}), so some ancestor must be one of \
             candidate's five recognized shapes: {elm:?}",
            elm.kind
        );
        let node = nearest_element_candidate(&start_node).expect(&not_a_candidate_msg);
        assert_eq!(
            node.text_range(),
            elm.line,
            "the located node must be the EXACT node try_claim saw for this claim"
        );
        // `classify_node_compiled`'s own doc: ranges come back relative to
        // `node`'s own start, exactly [`classify_line`]'s zero-based
        // convention — rebase onto the node's real position before
        // comparing against `elm`'s absolute, file-real spans.
        let node_start = node.text_range().start();
        let classification = classify_node_compiled(compiled, &node)
            .expect("candidate() must recognize a node this same call just found via it");
        let missed_msg = format!(
            "the live node-aware walk must agree the compiler DID claim this line (issue \
             #2351's own bug: it used to miss every sigil-bearing line here) — attempted \
             nothing for {elm:?}"
        );
        let winner = classification.matched.expect(&missed_msg);
        assert_eq!(
            winner.handler.text, elm.handler.text,
            "must agree with the compiler on which handler claimed the line"
        );
        assert_eq!(
            winner.captures.len(),
            elm.captures.len(),
            "must bind exactly the same number of captures as the compiler did"
        );
        for (live, from_compiler) in winner.captures.iter().zip(&elm.captures) {
            assert_eq!(live.name, from_compiler.name);
            assert_eq!(
                live.text, from_compiler.text,
                "capture text must match the compiler's own recorded capture exactly"
            );
            let live_range = TextRange::new(
                live.range.start() + node_start,
                live.range.end() + node_start,
            );
            assert_eq!(
                live_range, from_compiler.range,
                "capture spans, rebased onto the node's real position, must land on the \
                 exact same source bytes the compiler recorded"
            );
        }
    }

    /// The core fix: for a cue, a parenthetical, a compact cue with
    /// interpolated dialogue, and a slugged/tag-bearing scene heading, the
    /// node-aware walk's winner and captures agree EXACTLY with what the
    /// compiler's own `try_claim` actually claimed — not just "also
    /// matched something".
    #[test]
    fn classify_node_agrees_with_the_compiler_for_every_claim_candidate_shape() {
        let src = node_agreement_fixture_src();
        let (hir, root) = lower_src(src);
        assert_eq!(
            hir.element_matches.len(),
            4,
            "expected one match each for the heading/cue/parenthetical/compact-cue \
             lines: {:?}",
            hir.element_matches
        );
        let kinds: Vec<_> = hir.element_matches.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                crate::ElementKind::SceneHeading,
                crate::ElementKind::Cue,
                crate::ElementKind::Parenthetical,
                crate::ElementKind::Cue,
            ],
            "the fixture must exercise all four named shapes, in this order — a smaller \
             set here would silently weaken the agreement check below"
        );
        let compiled = compile_entries(&projection_from(&hir));
        for elm in &hir.element_matches {
            assert_agrees_with_compiled(&root, &compiled, elm);
        }
    }

    /// Issue #2351's own bug, pinned so a future change cannot silently
    /// resurrect it: the raw-line-text walk ([`classify_line_compiled`])
    /// genuinely diverges from the compiler for every one of these four
    /// shapes — a cue/parenthetical/compact-cue line is missed entirely
    /// (the pattern never sees the `@`/`(`/`)`), and a slugged/tag-bearing
    /// heading, while it still matches (its own `.+` is greedy enough to
    /// swallow the trailing slug/tag too), captures the WRONG, longer text.
    /// `classify_node_compiled` (proven correct above) is a genuinely
    /// different code path from this one, not a thin wrapper around it.
    #[test]
    fn classify_line_against_the_raw_text_still_diverges_from_the_compiler() {
        let src = node_agreement_fixture_src();
        let (hir, _root) = lower_src(src);
        let compiled = compile_entries(&projection_from(&hir));
        for elm in &hir.element_matches {
            let start = usize::from(elm.line.start());
            let end = usize::from(elm.line.end());
            let raw = classify_line_compiled(&compiled, elm.line.start(), &src[start..end]);
            match elm.kind {
                crate::ElementKind::Cue | crate::ElementKind::Parenthetical => {
                    assert!(
                        raw.matched.is_none(),
                        "the raw whole-line walk must still miss a real {:?} line \
                         entirely — got {:?}",
                        elm.kind,
                        raw.matched
                    );
                }
                crate::ElementKind::SceneHeading => {
                    let raw_winner = raw
                        .matched
                        .as_ref()
                        .expect("the raw walk still matches a heading line at all");
                    // `captures[0]` is `kind` (`"INT"`), unaffected by the
                    // trailing slug/tag — the divergence is in `title`,
                    // whose greedy `.+` swallows the slug/tag on the raw
                    // walk but not on the compiler's own stripped one.
                    let raw_title = &raw_winner
                        .captures
                        .iter()
                        .find(|c| c.name == "title")
                        .expect("heading pattern declares a `title` capture")
                        .text;
                    let compiled_title = &elm
                        .captures
                        .iter()
                        .find(|c| c.name == "title")
                        .expect("heading pattern declares a `title` capture")
                        .text;
                    assert_ne!(
                        raw_title, compiled_title,
                        "the raw walk's `title` capture must diverge from the compiler's \
                         own stripped-slug/tag capture — raw: {raw_title:?}, compiler: \
                         {compiled_title:?}"
                    );
                }
                other => unreachable!("fixture should not produce a {other:?} match"),
            }
        }
    }
}
