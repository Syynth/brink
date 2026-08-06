//! The explain-match query (issue #2113, NS-T seam 3/6) — the tooling read
//! that discharges the maintainer's "no invisible expansion" requirement:
//! for any line, is it matched, **by what** (handler fn + source location,
//! hoverable), **what bound** (captures as spans), and on a **miss** the
//! patterns attempted (registration order), or on a **hit** what else
//! matched but was **shadowed**.
//!
//! # This is a read, not a second walk
//!
//! Every field this module produces is already sitting in #2112's
//! [`crate::classify_line`] output or in #2111's
//! [`ConventionsProjection::entries`] itself — composing them into the
//! caller-facing shape below is arithmetic-free, walk-free composition.
//! Concretely: `attempted` on a miss is *exactly* `projection.entries`,
//! because the walk already ruled (2026-08-02, `classify.rs`'s own doc)
//! that it tries **every** entry whether or not one wins — there is no
//! separate "did we try this" bit to recompute, the fact that nothing
//! matched at all IS the miss case, and every entry the projection carries
//! was attempted.
//!
//! # Raw captures, not computed values
//!
//! Same boundary [`crate::classify_line`] documents: a capture is a real
//! regex binding (`name = "VENDOR"`), never a value the matched handler's
//! own body would go on to compute. This module does not relax that
//! anywhere.
//!
//! # `ElementKind` composition — deliberately deferred
//!
//! #2112's own review comment (PR #2257, "wave w133 true-up") named this as
//! this issue's to resolve: whether to compose [`crate::ElementKind`] (the
//! "matched kind" column — scene heading / cue / parenthetical / content
//! line) alongside this module's output, since [`ConventionProjectionEntry`]
//! carries no `kind` field.
//!
//! **This module does not compose it, and that is a decision, not an
//! oversight.** [`crate::hir::lower_native::element::candidate`] — the one
//! function that computes [`crate::ElementKind`] — reads a **parsed CST
//! node**, not a bare string, and one variant is chain-gated on more than
//! the line itself: a [`crate::ElementKind::Parenthetical`] only parses as
//! `PARENTHETICAL` directly after a *live cue* (`at_parenthetical` in
//! `brink-syntax-native`). This module's own entry points take `text: &str`
//! with no surrounding-line context — the same shape [`crate::classify_line`]
//! itself takes, deliberately, so it can classify a line an author is
//! actively typing before it has ever been parsed into a real file's CST at
//! all (`classify.rs`'s own "a hypothetically-broken entry is safer than
//! panicking on a line an author is actively typing"). A standalone line of
//! text cannot answer "was the line before this one a live cue" — so this
//! module cannot correctly derive `ElementKind` for its own inputs, only
//! guess at it.
//!
//! The w133 comment named two ways out: **(a)** compose `ElementKind` from
//! the compile-time detector directly, independent of the projection, or
//! **(b)** get a schema extension upstream first. Option (a) needs a real
//! parsed CST node as this module's input (not bare text) to answer the
//! chain-gated case correctly — a bigger seam than a pure per-line function
//! wants to own, and a different one than #2112's own "no compiler
//! pipeline, no salsa" framing for this walk. Option (b) has no consumer
//! yet either. Composing `ElementKind` is left as a follow-up (a caller that
//! *does* hold a parsed CST node — a real, already-open editor
//! document — can call `candidate` itself and pair its result alongside
//! this module's [`LineExplanation`] the same way `brink_ir::HirFile::element_matches`
//! already does for an actually-compiled file; nothing here forecloses
//! that), rather than shipped half-correct.
//!
//! # Memoization (issue #2113's own reassigned remainder)
//!
//! [`ExplainMatchCache`] is the memoized query the ruled cost compensation
//! calls for: cached per `(line text, projection)`, with the projection's
//! compiled pattern set (`brink_ir::hir::classify::compile_entries`) cached
//! alongside it so a **repeat** classification of the same line skips both
//! the walk and the compile, while even a **first** classification of a new
//! line still skips the compile (the w133 perf finding's own target). See
//! its own doc for the caching contract.

use std::collections::BTreeMap;

use rowan::{TextRange, TextSize};

use super::classify::{
    ClassifiedCapture, ClassifiedMatch, CompiledEntry, classify_line_compiled, compile_entries,
};
use super::types::ConventionProjectionEntry;
use crate::ConventionsProjection;

/// One line's full explain-match record — the caller-facing composition
/// this module exists to build. See this module's own doc for why every
/// field here is a read of #2111/#2112 data, never a second walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineExplanation {
    /// A pattern matched this line.
    Matched {
        /// The winning handler — fn name + declaration-site range
        /// (hoverable), and its captures as spans into the classified
        /// line.
        winner: ClassifiedMatch,
        /// Every other handler whose pattern also matched, ascending by
        /// `order` — shadowed by `winner`, ruled 2026-08-02. Empty when
        /// nothing else matched.
        shadowed: Vec<ClassifiedMatch>,
    },
    /// Nothing matched this line.
    Unmatched {
        /// Every entry the walk tried, in registration (ascending-`order`)
        /// sequence — empty only when the line was itself whitespace-only
        /// (the walk never even starts one, per [`crate::classify_line`]'s
        /// own trim contract; reporting the full entry list in that case
        /// would claim an attempt that never happened).
        attempted: Vec<ConventionProjectionEntry>,
    },
}

impl LineExplanation {
    /// `true` for [`Self::Matched`].
    #[must_use]
    pub fn is_matched(&self) -> bool {
        matches!(self, Self::Matched { .. })
    }

    /// `Some((winner, shadowed))` for [`Self::Matched`], `None` for
    /// [`Self::Unmatched`].
    #[must_use]
    pub fn into_matched(self) -> Option<(ClassifiedMatch, Vec<ClassifiedMatch>)> {
        match self {
            Self::Matched { winner, shadowed } => Some((winner, shadowed)),
            Self::Unmatched { .. } => None,
        }
    }

    /// `Some(attempted)` for [`Self::Unmatched`], `None` for
    /// [`Self::Matched`].
    #[must_use]
    pub fn into_attempted(self) -> Option<Vec<ConventionProjectionEntry>> {
        match self {
            Self::Unmatched { attempted } => Some(attempted),
            Self::Matched { .. } => None,
        }
    }
}

/// Explain what [`crate::classify_line`] would do for `text`, composed into
/// [`LineExplanation`]'s caller-facing shape. Pure and uncached — see
/// [`ExplainMatchCache`] for the memoized, compiled-pattern-cached entry
/// point a caller classifying many lines against one projection should use
/// instead.
#[must_use]
pub fn explain_match(
    projection: &ConventionsProjection,
    base: TextSize,
    text: &str,
) -> LineExplanation {
    let compiled = compile_entries(projection);
    explain_match_compiled(&compiled, &projection.entries, base, text)
}

/// [`explain_match`], but against an already-[`compile_entries`]-compiled
/// pattern set — [`ExplainMatchCache`]'s own inner call.
fn explain_match_compiled(
    compiled: &[CompiledEntry],
    entries: &[ConventionProjectionEntry],
    base: TextSize,
    text: &str,
) -> LineExplanation {
    if text.trim().is_empty() {
        return LineExplanation::Unmatched {
            attempted: Vec::new(),
        };
    }
    let classification = classify_line_compiled(compiled, base, text);
    match classification.matched {
        Some(winner) => LineExplanation::Matched {
            winner,
            shadowed: classification.shadowed,
        },
        None => LineExplanation::Unmatched {
            attempted: entries.to_vec(),
        },
    }
}

/// Shift every [`ClassifiedCapture::range`] inside `explanation` by `delta`
/// — never [`ClassifiedMatch::handler`]'s own range, which is a location in
/// the *conventions module's* source, unrelated to whatever line this
/// explanation classified. [`ExplainMatchCache`] uses this to rebase a
/// cached, text-relative result onto the real caller-supplied `base`,
/// exactly the way [`crate::classify_line`] itself composes `base` plus a
/// local offset for a fresh call — see this function's own caller for why
/// caching at `base = 0` and rebasing here is what keeps the cache valid
/// across every position the same line text occurs at.
fn rebase(explanation: LineExplanation, delta: TextSize) -> LineExplanation {
    fn rebase_capture(capture: ClassifiedCapture, delta: TextSize) -> ClassifiedCapture {
        ClassifiedCapture {
            range: TextRange::new(capture.range.start() + delta, capture.range.end() + delta),
            ..capture
        }
    }
    fn rebase_match(m: ClassifiedMatch, delta: TextSize) -> ClassifiedMatch {
        ClassifiedMatch {
            captures: m
                .captures
                .into_iter()
                .map(|c| rebase_capture(c, delta))
                .collect(),
            ..m
        }
    }
    match explanation {
        LineExplanation::Matched { winner, shadowed } => LineExplanation::Matched {
            winner: rebase_match(winner, delta),
            shadowed: shadowed
                .into_iter()
                .map(|m| rebase_match(m, delta))
                .collect(),
        },
        // `attempted` entries carry no captures into the classified line at
        // all — nothing here needs shifting.
        unmatched @ LineExplanation::Unmatched { .. } => unmatched,
    }
}

/// A cache pairing a compiled pattern set with memoized per-line results,
/// both invalidated together whenever the underlying [`ConventionsProjection`]
/// changes — the memoization issue #2113 owns as #2112's reassigned
/// remainder (ruled: memoize on `(line text, projection revision)`), plus
/// the w133 perf finding's own ask (cache the *compiled* pattern set, not
/// just the classification result).
///
/// There is no synthetic "revision" counter: [`ConventionsProjection`]
/// already derives `Eq`, so equality against the last-seen projection *is*
/// the revision check — the same cutoff signal salsa's own
/// `conventions_projection_query` already gives a caller for free. A caller
/// that already sits on a salsa graph may prefer wrapping [`explain_match`]
/// in a tracked query directly instead of this cache; both are the "ordinary
/// salsa way" / "small cache at the boundary" options #2112's own doc left
/// open. `@brink-lang/web`'s `EditorSession` is not itself salsa-tracked at
/// the wasm boundary, so it holds one of these.
///
/// # Caching at a text-relative base
///
/// Results are computed and cached at `base = 0` and rebased to the
/// caller's real `base` on every lookup (`rebase`) — otherwise two
/// occurrences of byte-identical line text at different file offsets would
/// collide on the same cache entry and one would get the other's capture
/// ranges. This mirrors [`crate::classify_line`]'s own documented
/// text-relative-at-`base`-zero convention for a caller with no real source
/// position yet.
/// A cached line's classification, stripped of the `attempted` payload a
/// miss would otherwise carry. Every miss's `attempted` is *exactly*
/// `self.projection.entries` — see `explain_match_compiled`'s own doc — so
/// storing that clone in every one of potentially many distinct-miss cache
/// entries would be O(distinct lines × projection size) for a value that is
/// byte-identical across every non-blank miss under one projection. `Miss`
/// carries nothing; the real `attempted` list is materialized from
/// `self.projection.entries` at lookup time instead (`ExplainMatchCache::explain`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum CachedLine {
    Matched {
        winner: ClassifiedMatch,
        shadowed: Vec<ClassifiedMatch>,
    },
    Miss,
}

fn to_cached(explanation: LineExplanation) -> CachedLine {
    match explanation {
        LineExplanation::Matched { winner, shadowed } => CachedLine::Matched { winner, shadowed },
        // The blank-line empty case never reaches here (see `explain`'s own
        // early return), so this is always a real, attempted-and-failed miss.
        LineExplanation::Unmatched { .. } => CachedLine::Miss,
    }
}

fn from_cached(cached: CachedLine, entries: &[ConventionProjectionEntry]) -> LineExplanation {
    match cached {
        CachedLine::Matched { winner, shadowed } => LineExplanation::Matched { winner, shadowed },
        CachedLine::Miss => LineExplanation::Unmatched {
            attempted: entries.to_vec(),
        },
    }
}

/// Hard cap on distinct cached line texts (CLAUDE.md: "any loop that
/// accumulates data must have a limit"). `lines` is keyed on keystroke-driven
/// text over the lifetime of one wasm session, with no natural revision
/// boundary to clear it on short of a projection change — an author who
/// edits a long document line by line for a whole session could otherwise
/// grow it without bound. Not a real LRU: once the cap is hit the whole map
/// is cleared and rebuilt from scratch, trading a burst of re-classification
/// for zero extra bookkeeping — a session churns through far more repeat
/// classifications of a small working set of lines than it does distinct
/// line texts, so hitting the cap at all is the uncommon case.
const MAX_CACHED_LINES: usize = 4096;

#[derive(Debug, Default)]
pub struct ExplainMatchCache {
    projection: ConventionsProjection,
    compiled: Vec<CompiledEntry>,
    lines: BTreeMap<String, CachedLine>,
}

impl ExplainMatchCache {
    /// A fresh, empty cache — equivalent to [`Self::default`], spelled out
    /// for callers that prefer a constructor over a trait bound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Explain `text` at `base` against `projection`, memoized. Recompiles
    /// the pattern set and clears every cached line only when `projection`
    /// differs (by `Eq`) from the last call's — an unchanged projection
    /// reuses both the compiled patterns and, for a repeat `text`, the
    /// classification itself.
    #[must_use]
    pub fn explain(
        &mut self,
        projection: &ConventionsProjection,
        base: TextSize,
        text: &str,
    ) -> LineExplanation {
        if &self.projection != projection {
            self.projection = projection.clone();
            self.compiled = compile_entries(projection);
            self.lines.clear();
        }
        // A blank line never even starts the walk (`explain_match_compiled`'s
        // own trim contract) — distinguishable before the cache is consulted,
        // so it is never cached at all.
        if text.trim().is_empty() {
            return LineExplanation::Unmatched {
                attempted: Vec::new(),
            };
        }
        if let Some(cached) = self.lines.get(text) {
            return rebase(from_cached(cached.clone(), &self.projection.entries), base);
        }
        if self.lines.len() >= MAX_CACHED_LINES {
            self.lines.clear();
        }
        let explanation = explain_match_compiled(
            &self.compiled,
            &self.projection.entries,
            TextSize::from(0),
            text,
        );
        self.lines
            .insert(text.to_owned(), to_cached(explanation.clone()));
        rebase(explanation, base)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{ClaimHandlerDecl, ConventionAttachField, Name};

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
    fn a_hit_reports_the_winner_and_every_shadowed_entry_in_order() {
        let p = projection(&[
            decl("cue", 10, "^(?<name>[A-Z]+)$"),
            decl("any_line", 20, "^.*$"),
            decl("also_any_line", 30, "^.*$"),
        ]);
        let explanation = explain_match(&p, TextSize::from(0), "VENDOR");
        let (winner, shadowed) = explanation.into_matched().expect("expected a match");
        assert_eq!(winner.handler.text, "cue");
        let shadowed_names: Vec<&str> = shadowed.iter().map(|m| m.handler.text.as_str()).collect();
        assert_eq!(shadowed_names, vec!["any_line", "also_any_line"]);
    }

    /// The core case this module exists for: on a miss, `attempted` names
    /// every entry the walk tried, in registration order — not just the
    /// winner-shaped subset a naive "what matched" view would give.
    #[test]
    fn a_miss_reports_every_entry_attempted_in_registration_order() {
        let p = projection(&[
            decl("interior", 10, "^INT\\. (?<place>.+)$"),
            decl("exterior", 20, "^EXT\\. (?<place>.+)$"),
            decl("cue", 30, "^(?<name>[A-Z]+)$"),
        ]);
        let explanation = explain_match(&p, TextSize::from(0), "plain content, matches nothing");
        let attempted = explanation.into_attempted().expect("expected a miss");
        let names: Vec<&str> = attempted.iter().map(|e| e.name.text.as_str()).collect();
        assert_eq!(
            names,
            vec!["interior", "exterior", "cue"],
            "registration order is resolution order — report attempted patterns in it"
        );
    }

    /// A whitespace-only line never even starts the walk
    /// ([`crate::classify_line`]'s own trim contract) — `attempted` must
    /// stay empty rather than falsely claim every entry was tried.
    #[test]
    fn a_blank_line_reports_no_attempted_patterns_at_all() {
        let p = projection(&[decl("any_line", 10, "^.*$")]);
        let explanation = explain_match(&p, TextSize::from(0), "   \t  ");
        let attempted = explanation.into_attempted().expect("expected a miss");
        assert!(attempted.is_empty());
    }

    /// An empty projection is a miss with nothing attempted — there is
    /// nothing to try.
    #[test]
    fn an_empty_projection_attempts_nothing() {
        let p = projection(&[]);
        let explanation = explain_match(&p, TextSize::from(0), "anything at all");
        let attempted = explanation.into_attempted().expect("expected a miss");
        assert!(attempted.is_empty());
    }

    /// A declined-entirely entry (a named group that never participated,
    /// e.g. the losing branch of an alternation — `classify_line`'s own
    /// rule) still shows up in `attempted` on a miss: it WAS tried, even
    /// though the walk declined to report it as a partial match.
    #[test]
    fn a_declined_entirely_entry_still_appears_as_attempted_on_a_miss() {
        let p = projection(&[decl(
            "interior_or_exterior",
            10,
            "^(?:INT\\. (?<place>.+)|EXT\\. (?<outside>.+))$",
        )]);
        let explanation = explain_match(&p, TextSize::from(0), "no match here");
        let attempted = explanation.into_attempted().expect("expected a miss");
        assert_eq!(attempted.len(), 1);
        assert_eq!(attempted[0].name.text, "interior_or_exterior");
    }

    /// The captures on a hit carry real spans into the classified line —
    /// this module composes, it does not re-derive.
    #[test]
    fn captures_on_a_hit_are_real_spans_at_the_given_base() {
        let p = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let explanation = explain_match(&p, TextSize::from(100), "INT. MARKET SQUARE");
        let (winner, _shadowed) = explanation.into_matched().expect("expected a match");
        assert_eq!(winner.captures.len(), 1);
        assert_eq!(winner.captures[0].text, "MARKET SQUARE");
        assert_eq!(
            winner.captures[0].range,
            TextRange::new(105.into(), 118.into())
        );
    }

    // ─── ExplainMatchCache ────────────────────────────────────────

    #[test]
    fn the_cache_gives_the_same_answer_as_the_uncached_call() {
        let p = projection(&[
            decl("interior", 10, "^INT\\. (?<place>.+)$"),
            decl("any_line", 20, "^.*$"),
        ]);
        let direct = explain_match(&p, TextSize::from(50), "INT. MARKET SQUARE");
        let mut cache = ExplainMatchCache::new();
        let cached = cache.explain(&p, TextSize::from(50), "INT. MARKET SQUARE");
        assert_eq!(direct, cached);
    }

    /// The bug this cache design specifically has to avoid: caching the
    /// full (already-based) result keyed only on text would make a SECOND
    /// occurrence of the identical line, at a different offset, silently
    /// reuse the FIRST occurrence's capture ranges. Rebasing at lookup time
    /// (`rebase`) is what this test proves actually happens.
    #[test]
    fn identical_line_text_at_two_different_bases_gets_correctly_rebased_captures() {
        let p = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let mut cache = ExplainMatchCache::new();

        let first = cache.explain(&p, TextSize::from(0), "INT. MARKET SQUARE");
        let second = cache.explain(&p, TextSize::from(1000), "INT. MARKET SQUARE");

        let (w1, _) = first.into_matched().expect("expected a match");
        let (w2, _) = second.into_matched().expect("expected a match");
        assert_eq!(w1.captures[0].range, TextRange::new(5.into(), 18.into()));
        assert_eq!(
            w2.captures[0].range,
            TextRange::new(1005.into(), 1018.into()),
            "the second occurrence must be rebased onto its own base, not \
             reuse the first occurrence's cached range"
        );
    }

    /// A projection change must invalidate every cached line, not merely
    /// the compiled pattern set — a stale cache entry from the OLD
    /// projection must never survive.
    #[test]
    fn a_changed_projection_invalidates_every_cached_result() {
        let before = projection(&[decl("any_line", 10, "^.*$")]);
        let after = projection(&[decl("cue", 10, "^(?<name>[A-Z]+)$")]);

        let mut cache = ExplainMatchCache::new();
        let first = cache.explain(&before, TextSize::from(0), "VENDOR");
        let (winner, _) = first.into_matched().expect("expected a match");
        assert_eq!(winner.handler.text, "any_line");

        let second = cache.explain(&after, TextSize::from(0), "VENDOR");
        let (winner, _) = second.into_matched().expect("expected a match");
        assert_eq!(
            winner.handler.text, "cue",
            "the stale `any_line` entry from the old projection must not \
             still win after the projection changed"
        );
    }

    /// Guard against unbounded growth (CLAUDE.md's own rule): a session that
    /// classifies more distinct line texts than `MAX_CACHED_LINES` under one
    /// unchanged projection must never grow `lines` past the cap.
    #[test]
    fn the_cache_never_grows_the_line_map_past_its_cap() {
        let p = projection(&[decl("any_line", 10, "^.*$")]);
        let mut cache = ExplainMatchCache::new();
        for i in 0..(MAX_CACHED_LINES + 10) {
            let _ = cache.explain(&p, TextSize::from(0), &format!("line number {i}"));
        }
        assert!(
            cache.lines.len() <= MAX_CACHED_LINES,
            "cache grew to {} entries, past its {MAX_CACHED_LINES} cap",
            cache.lines.len()
        );
    }

    /// A blank line is never inserted into the cache at all — it is decided
    /// before the cache is even consulted, so it must not consume a cap slot.
    #[test]
    fn a_blank_line_never_occupies_a_cache_slot() {
        let p = projection(&[decl("any_line", 10, "^.*$")]);
        let mut cache = ExplainMatchCache::new();
        let _ = cache.explain(&p, TextSize::from(0), "   \t  ");
        assert!(cache.lines.is_empty());
    }

    /// A miss must also be memoized correctly — `attempted` reflects the
    /// CURRENT projection's entries, not a stale snapshot.
    #[test]
    fn a_changed_projection_updates_attempted_patterns_on_a_repeat_miss() {
        let before = projection(&[decl("interior", 10, "^INT\\. (?<place>.+)$")]);
        let after = projection(&[
            decl("interior", 10, "^INT\\. (?<place>.+)$"),
            decl("exterior", 20, "^EXT\\. (?<place>.+)$"),
        ]);

        let mut cache = ExplainMatchCache::new();
        let first = cache.explain(&before, TextSize::from(0), "plain content");
        let attempted = first.into_attempted().expect("expected a miss");
        assert_eq!(attempted.len(), 1);

        let second = cache.explain(&after, TextSize::from(0), "plain content");
        let attempted = second.into_attempted().expect("expected a miss");
        assert_eq!(
            attempted.len(),
            2,
            "the new entry must appear after the projection changed"
        );
    }
}
