//! Natural-notation element dispatch (issue #1838) — the slice that makes
//! the prose grammar *mean* something.
//!
//! `docs/decision-log.md`'s 2026-07-31 ruling ("Conventions are annotated
//! handlers: the declarative element surface is subsumed by the annotation
//! surface") collapsed two element mechanisms into one: **a preset element
//! is literally an annotated handler.** A scene heading is a matched line,
//! captures bound to params by name, and *exactly one call* — the same
//! three steps `!radio` takes, minus the sigil. The `lower:` column, the
//! `Conventions` type and the chain-rule engine are dissolved by that same
//! ruling and are deliberately absent here.
//!
//! ```brink
//! @[element(claims = "^INT\\. (?<place>.+)$")]
//! fn interior(place) { return "— inside " + place + " —"; }
//!
//! flow main() {
//!   INT. MARKET SQUARE
//! }
//! ```
//!
//! The heading line no longer reaches `body::lower_one_item`'s loud-`E129`
//! arm: it is claimed, `place` binds to `MARKET SQUARE`, and the line
//! lowers to one call whose value *is* the line.
//!
//! # No invisible expansion
//!
//! Every claimed line is recorded as a [`crate::ElementMatch`] on the
//! `HirFile` — the claimed range, the prose shape it was written as, the
//! handler's name **and the annotation's own source range**, and each
//! capture as a span. The ruling is explicit that a rewritten line must
//! point at real source; that record is how the `LineContext`/IDE query
//! family answers "what happened to this line, and where is the code that
//! did it" without re-running the match.
//!
//! # What claims, and what does not
//!
//! - A pattern is a *claim* only when spelled `claims = "…"`. The
//!   `args = "…"` form declares the `!name`-dispatched handler instead
//!   ([`try_dispatch`], issue #2004) — see that function's own doc for what
//!   it covers and what it still doesn't.
//! - Only a **top-level `fn`** may claim: the rewrite is an expression
//!   call, and a `flow` is not callable as one. A `claims` annotation
//!   anywhere else is `E112` (misplaced), enforced by
//!   [`super::annotation::handle_line`]'s placement rule. "Top-level"
//!   means a direct child of the file, matching exactly what [`collect`]
//!   scans — a `fn` declared inside a `module { … }` block is *also*
//!   misplaced (issue #1847), even though it is otherwise un-nested in
//!   any `flow`/`fn`: `collect` never looks inside a `MODULE_DECL`, so a
//!   claim admitted there would validate and then silently never
//!   dispatch to anything.
//! - Only a **wholly literal** prose line is a candidate — one with no
//!   interpolation, glue, markup, tags, label or embedded divert. A line
//!   carrying dynamic parts has no fixed text for a pattern to match, and
//!   capture spans over it would not point at anything real. This
//!   restriction applies identically to a `CUE`'s name and a
//!   `PARENTHETICAL`'s delivery text (issue #1720 widens [`candidate`] to
//!   these two shapes, alongside `CONTENT_LINE`/`SCENE_HEADING`) — a cue
//!   or parenthetical carrying a trailing tag extension declines the same
//!   way a slug/tag-carrying heading does.
//! - A claiming handler's **own body is not claimable** (the staging rule
//!   §3.5 states for the conventions module: it cannot use the conventions
//!   it defines). Without this, a handler whose body repeats the shape it
//!   claims would rewrite into a call on itself. A `!name`-dispatched
//!   handler carries no such restriction — the sigil already makes every
//!   dispatched line self-announcing, so recursion (a handler's own body
//!   containing a `!name` line naming itself) is not the silent risk
//!   claiming guards against.
//!
//! # Block capture (issue #1839)
//!
//! `@[element(…, block)]` captures the **following run** into the
//! declaration's trailing `content`-typed parameter — the same
//! `BeginFragment`…`EndFragment` → `Value::FragmentRef` machinery an
//! ordinary call already composes through (`brink-codegen-inkb::content::
//! emit_slot_expr`), widened to hold an arbitrary captured statement run
//! rather than one call's own output (`docs/decision-log.md` 2026-08-01
//! "Content-as-value": the internal `hir::Expr::Fragment` / `lir::Expr::
//! Fragment` node this produces). Both [`try_claim`] and [`try_dispatch`]
//! support it identically — see either function's own "Block capture" doc
//! section — via the shared terminator search, [`capture_block`]. The
//! handler **wraps** the captured run (receives it, decides emission) and
//! does not tag it; interior lines are lowered through the ordinary
//! [`super::body::lower_items`] path, so a handler that would claim one of
//! them still claims it, with no special case needed.
//!
//! **Not carried across the cross-file injection join** (issue #1863): an
//! `external`/injected `ClaimHandler` is always `block: false` — see
//! [`ClaimHandler::block`]'s own doc for why.
//!
//! # Deliberately not here
//!
//! `fn conventions()` registration + comptime evaluation is the ruling's
//! other build slice, filed separately (issue #1840). Dispatching to a
//! `flow` target (rather than a top-level `fn`) is also not here: `!name`'s
//! placement is legal on a `flow` too ([`super::annotation::
//! is_consumed_position`]), but [`collect`] only ever scans top-level `fn`
//! declarations into the dispatch table — the same restriction `claims`
//! already has, for the same reason (the rewrite is an expression call).
//! The confinement of claiming to the `brink.toml`-named conventions
//! module needs project identity that single-file lowering does not have
//! — this module only records the raw material for that check ([`collect`]
//! populates `HirFile::claim_handlers` with every declared handler's name
//! and annotation range, independent of `matches`); the check itself runs
//! at the project layer (issue #1844, `brink_db::queries::analysis::
//! conventions_confinement_diagnostics_query`), which is the one seam that
//! has both a file's module identity and the configured pointer.
//!
//! [`collect`] also accepts an optional, already-ordered EXTERNAL handler
//! set (issue #1863, `super::external_conventions`) — claiming handlers
//! declared in some *other* file (the project's conventions module) and
//! injected into this file's dispatch. They are kept in their own list,
//! never merged into the locally declared `handlers` this module
//! collects: `HirFile::claim_handlers` and `E168`'s duplicate-pattern
//! check both mean "declared IN THIS FILE", and folding an injected
//! handler in would silently corrupt both. `!name` dispatch has no
//! external-injection counterpart at all yet — cross-file dispatch-name
//! resolution is `docs/prose-dialect-spec.md` §3.5b's own Deferred item,
//! so [`try_dispatch`] is file-local, matching `claims`'s pre-#1863 scope.

use std::collections::BTreeMap;

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use regex_syntax::hir::Hir;
use rowan::{TextRange, TextSize};

use crate::hir::FileId;
use crate::{
    Content, ContentPart, Diagnostic, DiagnosticCode, ElementCapture, ElementDisposition,
    ElementKind, ElementMatch, Expr, Name, Path, Stmt, StringExpr, StringPart,
};

use super::SyntaxNode;
use super::external_conventions::ExternalConventions;
use super::provenance::native_provenance;
use crate::provenance::NodeClass;

/// One declared natural-notation handler: a top-level `fn` whose
/// `@[element(claims = "…")]` pattern claims prose lines — OR a handler
/// injected from another file's evaluated conventions registry (issue
/// #1863), for which `decl` is `None` (see that field's own doc).
struct ClaimHandler {
    /// The handler's own name, carrying its declaration-site range — in
    /// the injected case, the range is in the DECLARING file, not this
    /// one.
    name: Name,
    /// Parameter names in declaration order — the argument order the
    /// rewritten call uses. Guaranteed by `E160`/`E167` to be exactly the
    /// pattern's named-capture set.
    params: Vec<String>,
    /// The compiled claiming pattern.
    pattern: regex::Regex,
    /// Range of the `@[element(claims = "…")]` line itself.
    annotation: TextRange,
    /// The handler declaration's own range — used to suppress claiming
    /// inside the handler's own body (the staging rule). `None` for an
    /// injected handler (issue #1863): "own body is not claimable" is a
    /// same-file concept, and an injected handler's declaration range is
    /// meaningless — usually plain wrong — against `claimed` ranges in
    /// THIS file's own text; comparing them by coincidence of numeric
    /// offset would be a real bug, not a conservative no-op, so injected
    /// handlers skip the check entirely rather than risk it.
    decl: Option<TextRange>,
    /// The bare `block` clause (issue #1839): `true` when the trailing
    /// parameter is a `content`-typed block-capture receiver, not a
    /// regex-bound capture — see [`try_claim`]'s doc for what that changes
    /// about argument binding. **Always `false` for an injected handler**
    /// (`decl: None`): `super::external_conventions::ExternalClaimHandler`
    /// does not carry the `block` flag across the cross-file injection
    /// join (issue #1863) yet, so a conventions-module handler declared
    /// with `block` can dispatch its non-block capture params when injected
    /// into another file, but never its block capture there — a real,
    /// tracked gap (issue #1839's PR notes it), not a silent one: `collect`
    /// always sets this `false` for `external`, never reads a nonexistent
    /// field.
    block: bool,
}

/// One declared `!name`-dispatched handler: a top-level `fn` whose
/// `@[element(args = "…")]` pattern parses the remainder after a `!name`
/// sigil (issue #2004). Unlike [`ClaimHandler`], there is no `decl`
/// self-suppression field — a dispatched handler's own body is not exempt
/// from matching its own sigil (see the module doc's "What claims, and
/// what does not" section for why), and no `external` counterpart either
/// — `!name` dispatch is file-local (same doc, "Deliberately not here").
struct DispatchHandler {
    /// The handler's own name, carrying its declaration-site range — the
    /// rewritten call's target. May differ from the map key this is stored
    /// under (`Elements::dispatch`), which is the dispatch name (the
    /// `name = "…"` alias, if any, else this same name's text).
    name: Name,
    /// Parameter names in declaration order — the argument order the
    /// rewritten call uses. Not guaranteed to be exactly the pattern's
    /// named-capture set the way `ClaimHandler::params` is (`claims`'s
    /// `E167` has no `args` counterpart: "a `!name` handler … stays
    /// callable by hand with ordinary arguments," `annotation.rs`'s own
    /// `E167` doc) — [`try_dispatch`] declines (rather than mis-dispatches)
    /// a line whose captures don't cover every param, exactly like
    /// [`try_claim`] does for the same missing-argument reason.
    params: Vec<String>,
    /// The compiled `args = "…"` pattern, matched against the dispatched
    /// line's remainder (the text after the `!name ` prefix), not the
    /// whole line.
    pattern: regex::Regex,
    /// Range of the `@[element(args = "…")]` line itself.
    annotation: TextRange,
    /// The bare `block` clause (issue #1839) — see [`ClaimHandler::block`]'s
    /// doc; the same meaning, for a `!name`-dispatched handler instead of a
    /// claiming one.
    block: bool,
}

/// The dispatcher threaded through body lowering: the file's claiming
/// handlers plus the per-line classification records they produce.
///
/// Built once per file by [`collect`] and passed down by reference, rather
/// than re-derived per line: a per-line whole-tree scan would make body
/// lowering quadratic in file size.
pub(super) struct Elements {
    handlers: Vec<ClaimHandler>,
    /// Handlers injected from another file's evaluated conventions
    /// registry (issue #1863, `super::external_conventions`) — kept
    /// separate from `handlers` for the reason the module doc gives:
    /// `handler_decls`/`E168` both mean "declared in this file", and an
    /// injected handler was declared elsewhere. [`try_claim`] dispatches
    /// over `handlers` first, `external` second — a local declaration
    /// always wins over an injected one of the same name (see
    /// [`collect`]'s dedup).
    external: Vec<ClaimHandler>,
    /// `!name`-dispatched handlers (issue #2004), keyed by dispatch name
    /// (the `name = "…"` alias if declared, else the `fn`'s own name) — a
    /// `BTreeMap`, not a `HashMap`, per this workspace's determinism rule,
    /// though the only operation ever performed over it is a by-key
    /// lookup (`try_dispatch`) or an insert-if-absent during [`collect`]'s
    /// single source-order pass, neither of which is order-sensitive on
    /// its own; a `BTreeMap` costs nothing here and removes any doubt.
    /// Two declarations naming the same dispatch name is an interim
    /// first-declared-wins ([`collect`]'s own doc), the same posture
    /// `try_claim`'s dispatch-order doc already takes for `claims` pending
    /// issue #1840's `fn conventions()` registration order.
    dispatch: BTreeMap<String, DispatchHandler>,
    /// Every claimed line, in the order lowering *reached* it — not
    /// necessarily source order (a `CHOICE_POINT` lowers its source-later
    /// continuation before its source-earlier choice bodies). [`super::lower`]
    /// sorts this by line start before storing it as
    /// `HirFile::element_matches`, whose own doc promises source order; this
    /// field is the pre-sort accumulation, an implementation detail of how
    /// body lowering visits nodes.
    pub(super) matches: Vec<ElementMatch>,
}

impl Elements {
    /// `true` when no handler — local or injected — claims anything in
    /// this file, so callers can skip candidate testing entirely on the
    /// overwhelmingly common path.
    fn is_inert(&self) -> bool {
        self.handlers.is_empty() && self.external.is_empty()
    }

    /// Every claiming handler *declared* in this file, in declaration
    /// order — [`HirFile::claim_handlers`](crate::HirFile::claim_handlers)'s
    /// source (issue #1844's confinement check). Deliberately independent
    /// of `matches`: a handler that claims nothing in its own file is still
    /// a declaration, and still checkable. Deliberately reads `handlers`
    /// only, never `external` — an injected handler (issue #1863) was not
    /// declared in this file, so it must never appear here (it would
    /// otherwise falsely accuse the *injecting* file of a confinement
    /// violation).
    pub(super) fn handler_decls(&self) -> Vec<crate::ClaimHandlerDecl> {
        self.handlers
            .iter()
            .map(|h| crate::ClaimHandlerDecl {
                name: h.name.clone(),
                annotation: h.annotation,
                params: h.params.clone(),
                pattern: h.pattern.as_str().to_string(),
            })
            .collect()
    }
}

/// Collect every claiming handler declared in `root`.
///
/// Silent on everything `container.rs` already validated: each
/// `@[element(…)]` line is parsed and checked against its own declaration
/// exactly once there — that validation pass's own diagnostics are
/// discarded here (into a throwaway `scratch` vec) rather than threaded
/// through, since re-reporting would double every `E159`/`E160`/`E167`.
/// Duplicate-pattern diagnosis ([`diagnose_duplicate_patterns`]) does
/// *not* happen here either: it needs to see which handlers actually
/// fired during body lowering, which hasn't happened yet at collection
/// time — see that function's doc.
///
/// `external` is the issue #1863 injection point: an optional,
/// already-ordered conventions registry built OUTSIDE this file (another
/// file's declared handlers). Any injected handler whose name collides
/// with one this file declares locally is dropped — a local declaration
/// always wins (see [`Elements`]'s doc); in practice this only fires for
/// the conventions module's own file, where the injected registry and
/// this file's own declarations name the very same handlers.
pub(super) fn collect(
    file_id: FileId,
    root: &SyntaxNode,
    external: Option<&ExternalConventions>,
) -> Elements {
    let mut handlers = Vec::new();
    let mut dispatch: BTreeMap<String, DispatchHandler> = BTreeMap::new();
    for node in root.children() {
        // Top-level `fn` only — see the module doc. `handle_line`'s
        // placement rule reports every other position as `E112`, so a
        // claim this loop skips is never silently dropped. `!name`
        // dispatch shares this same top-level-`fn`-only restriction (issue
        // #2004) — a `flow`'s own `args` clause is a legal *declaration*
        // (`annotation::is_consumed_position` allows it) but is not yet a
        // live dispatch target; see the module doc's "Deliberately not
        // here" section.
        if node.kind() != N::FN_DECL {
            continue;
        }
        let Some(decl) = ast::FnDecl::cast(node.clone()) else {
            continue;
        };
        let Some(name) = super::container::name_from(decl.name_token()) else {
            continue;
        };
        let params = super::container::lower_params(decl.param_list());
        let mut scratch: Vec<Diagnostic> = Vec::new();
        let Some(element) =
            super::annotation::element_annotation(file_id, &node, &params, &mut scratch)
        else {
            continue;
        };
        let Ok(pattern) = regex::Regex::new(&element.pattern) else {
            continue;
        };
        let param_names: Vec<String> = params.into_iter().map(|p| p.name.text).collect();
        if element.claims {
            handlers.push(ClaimHandler {
                name,
                params: param_names,
                pattern,
                annotation: element.range,
                decl: Some(node.text_range()),
                block: element.block,
            });
        } else {
            // The dispatch name: the `name = "…"` alias if declared, else
            // the `fn`'s own name — `annotation::parse_element`'s
            // `ELEMENT_NAME` clause. First-declared wins on a collision
            // (`entry`/`or_insert`, never overwriting): an interim rule,
            // not a permanent one — see `Elements::dispatch`'s own doc.
            let dispatch_name = element.alias.clone().unwrap_or_else(|| name.text.clone());
            dispatch.entry(dispatch_name).or_insert(DispatchHandler {
                name,
                params: param_names,
                pattern,
                annotation: element.range,
                block: element.block,
            });
        }
    }
    let mut external_handlers = Vec::new();
    if let Some(external) = external {
        for candidate in external.handlers() {
            if handlers.iter().any(|h| h.name.text == candidate.name.text) {
                continue;
            }
            let Ok(pattern) = regex::Regex::new(&candidate.pattern) else {
                continue;
            };
            external_handlers.push(ClaimHandler {
                name: candidate.name.clone(),
                params: candidate.params.clone(),
                pattern,
                annotation: candidate.annotation,
                decl: None,
                // See `ClaimHandler::block`'s doc: `ExternalClaimHandler`
                // does not carry this flag across the injection join yet
                // (issue #1863's own scope), so an injected handler can
                // never block-capture — always `false`, never read from
                // `candidate` (which has no such field).
                block: false,
            });
        }
    }
    Elements {
        handlers,
        external: external_handlers,
        dispatch,
        matches: Vec::new(),
    }
}

/// Cap on the number of witness strings [`generate_witnesses_from_hir`]
/// expands a concatenation into, so an alternation-heavy pattern can't blow
/// up the cartesian product it builds.
const MAX_GENERATED_WITNESSES: usize = 16;

/// Try to prove that every string `later_pattern` can match is also matched
/// by `earlier_pattern` — i.e. `later_pattern`'s language is a subset of
/// `earlier_pattern`'s, so under first-match-wins dispatch the later handler
/// can never win a claim the earlier one doesn't already win first.
///
/// Returns `true` if subsumption is proven, `false` otherwise (which does
/// not prove the later pattern is *not* subsumed, only that this heuristic
/// couldn't prove it). Mere overlap — some string both patterns match, but
/// each also matches strings the other doesn't — is not enough: a later
/// handler can still be genuinely useful (see `docs/diagnostics/E170.md`'s
/// "What this does not catch"). Subsumption is proven by generating a set
/// of candidate strings from `later_pattern`'s structure and checking that
/// every one of them is also accepted by `earlier_pattern`.
fn later_pattern_provably_subsumed(
    earlier_pattern: &regex::Regex,
    later_pattern: &regex::Regex,
) -> bool {
    let Ok(later_hir) = regex_syntax::Parser::new().parse(later_pattern.as_str()) else {
        return false;
    };
    let witnesses = generate_witnesses_from_hir(&later_hir);
    !witnesses.is_empty()
        && witnesses
            .iter()
            .all(|witness| earlier_pattern.is_match(witness))
}

/// Generate a set of candidate strings the regex HIR might match.
///
/// This is a best-effort heuristic: an empty result does not mean the regex
/// matches nothing, only that this function couldn't construct an example.
/// Unlike a single-witness generator, this expands **every** branch of an
/// alternation and recurses into capture groups — skipping either would make
/// [`later_pattern_provably_subsumed`] blind to every claim pattern with a
/// named capture (which is all of them, per `E160`/`E167`) or an
/// alternation (a very common way to spell "either of these branches").
/// `Concat` builds its result as the cartesian product of its parts'
/// witnesses, capped at [`MAX_GENERATED_WITNESSES`] so an alternation-heavy
/// pattern can't blow this up.
fn generate_witnesses_from_hir(hir: &Hir) -> Vec<String> {
    use regex_syntax::hir::HirKind;

    match hir.kind() {
        HirKind::Literal(lit) => vec![String::from_utf8_lossy(&lit.0).to_string()],
        HirKind::Capture(cap) => generate_witnesses_from_hir(&cap.sub),
        HirKind::Repetition(r) => generate_witnesses_from_hir(&r.sub),
        HirKind::Class(_) => vec!["a".to_string()],
        HirKind::Empty | HirKind::Look(_) => vec![String::new()],
        HirKind::Alternation(alts) => alts
            .iter()
            .flat_map(generate_witnesses_from_hir)
            .take(MAX_GENERATED_WITNESSES)
            .collect(),
        HirKind::Concat(parts) => {
            let mut acc = vec![String::new()];
            for part in parts {
                let part_witnesses = generate_witnesses_from_hir(part);
                if part_witnesses.is_empty() {
                    // An unhandled construct contributes nothing provable;
                    // stop here rather than silently dropping this part of
                    // the pattern from every witness.
                    return Vec::new();
                }
                let mut next = Vec::with_capacity(acc.len() * part_witnesses.len());
                'outer: for prefix in &acc {
                    for suffix in &part_witnesses {
                        next.push(format!("{prefix}{suffix}"));
                        if next.len() >= MAX_GENERATED_WITNESSES {
                            break 'outer;
                        }
                    }
                }
                acc = next;
            }
            acc
        }
    }
}

/// `E168` (issue #1848): flag every claiming handler whose pattern is
/// byte-identical to an earlier-declared one's *and never actually won a
/// claim*.
///
/// `E170` (issue #1859): flag every claiming handler whose pattern can
/// provably overlap with an earlier-declared one's *and never actually won a
/// claim of its own*.
///
/// `elements.handlers` is in declaration order (the order `collect` pushed
/// them, itself `root.children()`'s source order), which is also
/// [`try_claim`]'s dispatch order — see that function's doc for why
/// "earlier-declared" is the same as "wins". An identical pattern
/// *provably* matches the identical set of lines — but "the earlier one
/// always wins" is not quite the same claim as "the later one is dead
/// code". `try_claim` excludes a handler from claiming lines inside its
/// **own** declaration (the staging rule), and that exclusion does not
/// extend to a later, byte-identical twin: a twin is exactly the handler
/// that *can* claim a line living inside the earlier one's own body, since
/// the earlier one is barred from claiming there. So a later twin is
/// genuinely live, not dead code, for precisely those lines.
///
/// The same logic applies to E170: a later handler with an overlapping
/// (but non-identical) pattern is live if it actually won at least one claim
/// (necessarily for a line that the earlier pattern couldn't claim, or where
/// it was barred by the staging rule). Only report when the later handler
/// produced zero actual claims.
///
/// Must therefore run **after** the whole file has been lowered
/// ([`super::lower`] calls this once `walk_top_level` has returned), not
/// from [`collect`] before any body is lowered — `elements.matches` is the
/// only ground truth for "did this handler ever actually win a claim",
/// and it does not exist yet at collection time. A later handler that
/// produced at least one entry there is live and is not diagnosed; one
/// that produced none is provably dead or unreachable.
///
/// Each later handler is reported **at most once**, against the first
/// (source-order) earlier handler it overlaps with — a handler that overlaps
/// two or more earlier ones is not re-reported once per overlap.
///
/// `O(n²)` over a file's claiming handlers, which in practice number in
/// the single digits (one project's worth of prose conventions, not a
/// generated table) — a sorted/hashed pass would trade clarity for
/// headroom this call site doesn't need.
pub(super) fn diagnose_duplicate_patterns(
    file_id: FileId,
    elements: &Elements,
    diags: &mut Vec<Diagnostic>,
) {
    let handlers = &elements.handlers;
    for (later_idx, later) in handlers.iter().enumerate().skip(1) {
        // Ground truth, not assumption: did `later` ever actually win a
        // claim? If so it is live, not dead code.
        let fired = elements
            .matches
            .iter()
            .any(|m| m.handler.range == later.name.range);
        if fired {
            continue;
        }

        // Look for an earlier handler that conflicts with `later`. Iterate
        // forwards (from index 0 up to `later_idx - 1`) to find the first
        // (textually earliest) conflicting handler, and stop there — a
        // `.rev()` here would find the *nearest*, not the first, conflict,
        // and could misclassify a byte-identical twin as a mere overlap if
        // a between them a distinct overlapping handler was declared.
        for earlier in &handlers[..later_idx] {
            let is_byte_identical = earlier.pattern.as_str() == later.pattern.as_str();
            let has_provable_overlap = is_byte_identical
                || later_pattern_provably_subsumed(&earlier.pattern, &later.pattern);

            if !has_provable_overlap {
                continue;
            }

            // Found a conflict. Pick the right diagnostic code.
            let code = if is_byte_identical {
                DiagnosticCode::E168
            } else {
                DiagnosticCode::E170
            };

            diags.push(Diagnostic {
                file: file_id,
                // Emit at `later`'s name token so `@[allow(...)]` can
                // suppress it — `AllowScope::range` covers the annotated
                // declaration but explicitly excludes the annotation line
                // itself, so emitting on the annotation would make
                // `@[allow(code)]` unable to suppress this diagnostic.
                range: later.name.range,
                message: code.title().to_string(),
                code,
            });

            // Stop at the first conflict to avoid re-reporting this handler
            // if it overlaps multiple earlier ones.
            break;
        }
    }
}

/// Try to claim one body item, returning the statements that replace it.
///
/// `None` means "nothing claimed this" and the caller lowers the item the
/// way it always did — the fall-through that keeps every unclaimed line,
/// and every file with no claiming handler, byte-identical.
///
/// # Dispatch order (interim)
///
/// When more than one handler's pattern matches a line, the first one in
/// `elements.handlers` wins — [`Iterator::find`] below, over a `Vec` built
/// by `collect` in top-level declaration order. **This is an interim rule,
/// not a permanent one.** Issue #1848: the 2026-07-31 §9.1 ruling's item
/// (5) says `fn conventions()` will *register* handlers in order once
/// issue #1840 lands, and statement order in that registering function —
/// not a claiming `fn`'s textual position in the file — becomes the
/// authoritative resolution order then. Until #1840 lands, declaration
/// order is the only order there is, so it is what this dispatches on; do
/// not treat it as a rule worth entrenching or optimizing around. Two
/// patterns that can both claim one line get no diagnostic today except
/// the narrow byte-identical case ([`diagnose_duplicate_patterns`], issue
/// #1848) — a genuinely overlapping (non-identical) pair silently
/// prefers the earlier one, exactly the failure mode "pattern power
/// proportional to auditability" (`docs/prose-dialect-spec.md` §3.5b)
/// exists to keep visible, not eliminate outright.
///
/// # Block capture (issue #1839)
///
/// When `handler.block` is set, the trailing declared parameter is not a
/// regex-bound capture at all — `annotation::has_block_content_param`'s own
/// `E166` check already guarantees it is the *last* param, is `content`-
/// typed, and is excluded from `captures`, so only the params *before* it
/// need a named capture here. That trailing param instead binds an
/// `Expr::Fragment` built from the **following run** ([`capture_block`]):
/// `following` is the caller's remaining, not-yet-lowered sibling items,
/// and the terminator search consumes a prefix of it (a blank line, or any
/// non-`CONTENT_LINE` item, ends the run — the ruled terminator). The
/// number of items consumed is returned alongside the statements so the
/// caller (`body::lower_items`) can skip them rather than lowering them
/// a second time.
pub(super) fn try_claim(
    file_id: FileId,
    node: &SyntaxNode,
    elements: &mut Elements,
    following: &[SyntaxNode],
    diags: &mut Vec<Diagnostic>,
) -> Option<(Vec<Stmt>, usize)> {
    if elements.is_inert() {
        return None;
    }
    let (kind, text_node) = candidate(node)?;
    let text = text_node.text().to_string();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Where `trimmed` starts inside `text_node`, so capture offsets land on
    // real source bytes rather than on the untrimmed run's start.
    let lead = u32::try_from(text.len() - text.trim_start().len()).unwrap_or(0);
    let base = text_node.text_range().start() + TextSize::from(lead);

    let claimed = node.text_range();
    // Local handlers are tried before injected ones (issue #1863) — see
    // `Elements`'s doc. Only a local handler's `decl` ever suppresses a
    // claim inside its own body; an injected handler's `decl` is `None`
    // and never suppresses (see `ClaimHandler::decl`'s doc for why
    // comparing a foreign-file range here would be a real bug).
    let handler = elements
        .handlers
        .iter()
        .chain(elements.external.iter())
        .find(|h| {
            !h.decl.is_some_and(|decl| decl.contains_range(claimed)) && h.pattern.is_match(trimmed)
        })?;

    let is_block = handler.block;
    // See this function's own "Block capture" doc: the trailing param is
    // excluded from the regex-bound set when `block` is set.
    let bound_len = handler.params.len().saturating_sub(usize::from(is_block));

    let caps = handler.pattern.captures(trimmed)?;
    let mut captures = Vec::with_capacity(bound_len);
    for param in &handler.params[..bound_len] {
        // `E160`/`E167` already pinned params ≡ named captures at the
        // declaration, so a miss here means the group did not participate
        // in this particular match (an alternation branch). Declining the
        // claim is the honest answer: a call with a missing argument is
        // not "exactly one call", it is a broken one.
        let m = caps.name(param)?;
        captures.push(ElementCapture {
            name: param.clone(),
            text: m.as_str().to_string(),
            range: TextRange::new(
                base + TextSize::from(u32::try_from(m.start()).ok()?),
                base + TextSize::from(u32::try_from(m.end()).ok()?),
            ),
        });
    }

    // Every field taken from `handler` past this point is copied/cloned out
    // — `handler` (and so the borrow of `elements.handlers`/`.external` it
    // holds) is never referenced again, which is what lets the `block`
    // branch below borrow `elements` mutably for the recursive capture.
    let handler_name = handler.name.clone();
    let handler_annotation = handler.annotation;

    let mut call_args: Vec<Expr> = captures
        .iter()
        .map(|c| {
            Expr::String(StringExpr {
                parts: vec![StringPart::Literal(c.text.clone())],
            })
        })
        .collect();

    let mut consumed = 0;
    let mut content_range = None;
    if is_block {
        let (fragment_stmts, n, range) = capture_block(file_id, following, elements, diags);
        consumed = n;
        content_range = range;
        call_args.push(Expr::Fragment(fragment_stmts));
    }

    let call = Expr::Call(
        Path {
            segments: vec![Name {
                text: handler_name.text.clone(),
                // The call is written at the claimed line, not at the
                // handler's declaration — the range a reader clicking the
                // rewritten call should land on.
                range: claimed,
            }],
            range: claimed,
        },
        call_args,
    );

    elements.matches.push(ElementMatch {
        line: claimed,
        kind,
        handler: handler_name,
        annotation: handler_annotation,
        captures,
        disposition: ElementDisposition::Call,
        content: content_range,
    });

    Some((
        vec![
            Stmt::Content(Content {
                ptr: Some(native_provenance(file_id, NodeClass::Content, node)),
                parts: vec![ContentPart::Interpolation(call)],
                tags: Vec::new(),
            }),
            Stmt::EndOfLine,
        ],
        consumed,
    ))
}

/// The block-capture terminator search (issue #1839, ruled 2026-07-31):
/// consume a prefix of `following` — the caller's remaining, not-yet-
/// lowered sibling items — stopping at **a blank line, or any element-level
/// line**. "Element-level" is read structurally against this dialect's own
/// candidate set: a *plain* `CONTENT_LINE` — [`is_plain_content_line`] —
/// continues the run (and is lowered through the ordinary
/// [`super::body::lower_items`] path, so a handler that claims *it* still
/// claims it — "interior lines keep their own handlers" falls out of
/// reusing the normal dispatch loop rather than needing a special case);
/// anything else ends the run: a new `SCENE_HEADING`/`SCENE_STITCH`,
/// `CUE`/`COMPACT_CUE`/`PARENTHETICAL`, another `BANG_DISPATCH`, a
/// standalone choice point or divert, a nested declaration, running out of
/// items — **or a `CONTENT_LINE` that is not plain**, i.e. one carrying a
/// `LABEL` (it would otherwise absorb the rest of the captured run into a
/// `Stmt::LabeledBlock` — reviewer finding, #1839's PR review) or an
/// element-level construct fused onto the same line (`DIVERT_STMT`/
/// `TUNNEL_CALL`/`CHOICE_POINT` — a trailing `-> target`/`->->`/`{?}` on a
/// prose line is still "an element-level line" in the ruled sense, even
/// though the parser fuses it onto the preceding `CONTENT_LINE` rather than
/// giving it its own sibling node; same reviewer finding). Absorbing either
/// shape into the fragment either mis-lowers as `Stmt::LabeledBlock`/
/// `Stmt::ChoiceSet` (rejected at LIR by `reject_unsupported_inline_construct`,
/// E059, with a message that talks about inline-content position rather than
/// block-capture) or — for a bare divert/tunnel, which *does* lower cleanly
/// at LIR — silently corrupts the runtime's fragment-depth tracking, since
/// the divert transfers control before the fragment's own `EndFragment` can
/// run. Both are avoided by simply never absorbing such a line into the
/// capture: it becomes the terminator instead, and lowers normally as the
/// next ordinary body item. Returns the lowered statements, how many items
/// were consumed (so the caller can skip re-lowering them), and the
/// captured span's own source range (`None` when nothing was captured — an
/// immediate terminator, e.g. a claimed header line with a blank line right
/// after it).
fn capture_block(
    file_id: FileId,
    following: &[SyntaxNode],
    elements: &mut Elements,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<Stmt>, usize, Option<TextRange>) {
    let mut end = 0;
    while end < following.len() {
        let item = &following[end];
        if !is_plain_content_line(item) || blank_line_precedes(item) {
            break;
        }
        end += 1;
    }
    let captured = &following[..end];
    let range = match (captured.first(), captured.last()) {
        (Some(first), Some(last)) => Some(first.text_range().cover(last.text_range())),
        _ => None,
    };
    let stmts = super::body::lower_items(file_id, captured, 0, elements, diags);
    (stmts, end, range)
}

/// `true` when `node` is a `CONTENT_LINE` that [`capture_block`]'s
/// terminator search may fold into the captured run.
///
/// A `CONTENT_LINE` is not automatically "plain" just because its `SyntaxKind`
/// matches: the native parser fuses a `LABEL`, or a trailing `DIVERT_STMT`/
/// `TUNNEL_CALL`/`CHOICE_POINT`, onto the *same* `CONTENT_LINE` node as the
/// preceding prose rather than giving it its own sibling item (see
/// `brink-syntax-native`'s `labeled_content_line_produces_a_label_node` and
/// `divert_inside_multiline_choice_body_after_prose_is_a_divert_node` tests).
/// Each of those is "an element-level line" in the ruled terminator's sense
/// even though it shares a `CONTENT_LINE` wrapper with ordinary text —
/// folding it into the capture is what let a labeled/divert/choice-bearing
/// line be silently absorbed and mis-lowered (reviewer finding, #1839's PR
/// review; see [`capture_block`]'s own doc for the two concrete failure
/// modes). Checked structurally over `node`'s direct children — the same
/// depth [`super::body::lower_content_run`] scans when it lowers a content
/// line's own body — rather than a deep descendant search, since a divert
/// nested inside e.g. a bracketed span is not "this line ends in a divert"
/// in the same sense.
fn is_plain_content_line(node: &SyntaxNode) -> bool {
    node.kind() == N::CONTENT_LINE
        && !node.children().any(|child| {
            matches!(
                child.kind(),
                N::LABEL | N::DIVERT_STMT | N::TUNNEL_CALL | N::CHOICE_POINT
            )
        })
}

/// `true` when at least one blank source line separates `node` from
/// whatever real body item precedes it in the tree — a lone `NEWLINE`
/// token ends the previous item's own line; a *second* bare `NEWLINE` with
/// nothing but trivia between the two is the blank line itself (native's
/// content-ground layer never wraps a blank line in its own node — see
/// `brink-syntax-native`'s `blank_line_produces_no_content_line` test).
/// Walks backwards over raw sibling tokens the same way
/// `annotation::annotations_before`/`attached_declaration` already do,
/// rather than re-deriving trivia classification here.
fn blank_line_precedes(node: &SyntaxNode) -> bool {
    let mut cursor = node.prev_sibling_or_token();
    let mut newlines = 0u32;
    while let Some(el) = cursor {
        match &el {
            rowan::NodeOrToken::Token(tok) => {
                if tok.kind() == N::NEWLINE {
                    newlines += 1;
                    if newlines >= 2 {
                        return true;
                    }
                } else if !tok.kind().is_trivia() {
                    break;
                }
            }
            // A preceding real node is the previous body item itself —
            // stop, no blank line found.
            rowan::NodeOrToken::Node(_) => break,
        }
        cursor = el.prev_sibling_or_token();
    }
    false
}

/// Try to dispatch one body item via the `!name` sigil (issue #2004),
/// returning the statements that replace it.
///
/// `None` means "not dispatched" — either `node` isn't a `BANG_DISPATCH` at
/// all (the harmless, overwhelmingly common case, mirroring how [`try_claim`]
/// is harmless for any node [`candidate`] doesn't recognize), no handler is
/// declared under its dispatch name, its remainder isn't a wholly-literal
/// candidate ([`candidate`], the same requirement [`try_claim`] enforces on
/// a claimed line), or the handler's `args` pattern doesn't match the
/// (trimmed) remainder. Every one of those is a real `!name` line the
/// author wrote that this compiler cannot yet honor — the caller
/// (`body::lower_one_item`) falls through to its own default arm, which
/// diagnoses an unrecognized `BANG_DISPATCH` node loudly (`E129`, "parses
/// cleanly but has no HIR lowering yet") rather than silently dropping it.
///
/// The ruled spec text asks for more than that: "an unmatched remainder is
/// a targeted diagnostic naming both the line and the handler's pattern"
/// (§3.5b). `E129`'s generic message doesn't name either — a sharper
/// diagnostic would need a new code, which this issue's own hint says to
/// treat as a stop-and-report rather than an allocate-it-yourself (rule
/// 12q, no code is pre-assigned here). `E129` is the honest, already-
/// established "not fully implemented" fallback in the meantime, loud
/// rather than silent, not a substitute for the ruled diagnostic.
///
/// No dispatch-*order* question analogous to [`try_claim`]'s own doc
/// section: `elements.dispatch` is keyed by name, so at most one handler
/// can ever match a given `!name` line's own name. Two *declarations*
/// colliding on the same dispatch name is a distinct question — see
/// `Elements::dispatch`'s own doc for the interim first-declared-wins rule
/// there.
///
/// Block capture (issue #1839) works identically to [`try_claim`]'s own
/// "Block capture" doc section: a `block`-declared handler's trailing param
/// binds an `Expr::Fragment` built from `following` via [`capture_block`]
/// instead of a named capture, and the number of items consumed is
/// returned alongside the statements.
pub(super) fn try_dispatch(
    file_id: FileId,
    node: &SyntaxNode,
    elements: &mut Elements,
    following: &[SyntaxNode],
    diags: &mut Vec<Diagnostic>,
) -> Option<(Vec<Stmt>, usize)> {
    if node.kind() != N::BANG_DISPATCH || elements.dispatch.is_empty() {
        return None;
    }
    let dispatch_name = node
        .children()
        .find(|n| n.kind() == N::DISPATCH_NAME)
        .map(|n| n.text().to_string().trim().to_owned())?;
    let handler = elements.dispatch.get(&dispatch_name)?;

    let remainder = node.children().find(|n| n.kind() == N::CONTENT_LINE)?;
    // The inner classification (always `ElementKind::ContentLine`, since a
    // `BANG_DISPATCH`'s remainder is always a fused `CONTENT_LINE`) is
    // discarded — the record below classifies the *outer* shape as
    // `ElementKind::BangDispatch`, the self-announcing sigil, not the
    // content-line-shaped remainder underneath it.
    let (_, text_node) = candidate(&remainder)?;
    let text = text_node.text().to_string();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Where `trimmed` starts inside `text_node`, so capture offsets land on
    // real source bytes rather than on the untrimmed run's start.
    let lead = u32::try_from(text.len() - text.trim_start().len()).unwrap_or(0);
    let base = text_node.text_range().start() + TextSize::from(lead);

    if !handler.pattern.is_match(trimmed) {
        return None;
    }

    let is_block = handler.block;
    // See `try_claim`'s "Block capture" doc: the trailing param is excluded
    // from the regex-bound set when `block` is set.
    let bound_len = handler.params.len().saturating_sub(usize::from(is_block));

    let caps = handler.pattern.captures(trimmed)?;
    let mut captures = Vec::with_capacity(bound_len);
    for param in &handler.params[..bound_len] {
        // Mirrors `try_claim`'s own "declining is the honest answer": a
        // param with no matching capture (an `args` handler is allowed
        // extra params not covered by any capture — `annotation::E167`'s
        // own doc, "a `!name` handler is exempt") has no source of value
        // for the rewritten call to pass, so this line just doesn't
        // dispatch rather than dispatching with a broken call.
        let m = caps.name(param)?;
        captures.push(ElementCapture {
            name: param.clone(),
            text: m.as_str().to_string(),
            range: TextRange::new(
                base + TextSize::from(u32::try_from(m.start()).ok()?),
                base + TextSize::from(u32::try_from(m.end()).ok()?),
            ),
        });
    }

    // See `try_claim`'s identical note: everything from `handler` needed
    // past this point is copied out, ending the borrow of
    // `elements.dispatch` before the `block` branch borrows `elements`
    // mutably.
    let handler_name = handler.name.clone();
    let handler_annotation = handler.annotation;

    let mut call_args: Vec<Expr> = captures
        .iter()
        .map(|c| {
            Expr::String(StringExpr {
                parts: vec![StringPart::Literal(c.text.clone())],
            })
        })
        .collect();

    let mut consumed = 0;
    let mut content_range = None;
    if is_block {
        let (fragment_stmts, n, range) = capture_block(file_id, following, elements, diags);
        consumed = n;
        content_range = range;
        call_args.push(Expr::Fragment(fragment_stmts));
    }

    let claimed = node.text_range();
    let call = Expr::Call(
        Path {
            segments: vec![Name {
                text: handler_name.text.clone(),
                // The call is written at the dispatched line, not at the
                // handler's declaration — the range a reader clicking the
                // rewritten call should land on (mirrors `try_claim`).
                range: claimed,
            }],
            range: claimed,
        },
        call_args,
    );

    elements.matches.push(ElementMatch {
        line: claimed,
        kind: ElementKind::BangDispatch,
        handler: handler_name,
        annotation: handler_annotation,
        captures,
        disposition: ElementDisposition::Call,
        content: content_range,
    });

    Some((
        vec![
            Stmt::Content(Content {
                ptr: Some(native_provenance(file_id, NodeClass::Content, node)),
                parts: vec![ContentPart::Interpolation(call)],
                tags: Vec::new(),
            }),
            Stmt::EndOfLine,
        ],
        consumed,
    ))
}

/// Classify a body item as a claim candidate, yielding the node whose text
/// a pattern is matched against.
///
/// A `CONTENT_LINE` qualifies only when it is *wholly* literal — exactly
/// one `TEXT` child and nothing else (no `LABEL`, `INTERPOLATION`, `SPAN`,
/// `TAG`, `GLUE_NODE`, `ESCAPE`, embedded divert or choice point). A
/// `SCENE_HEADING`'s title run qualifies the same way; the heading's
/// optional `[slug]` and trailing tags are structure the pattern is not
/// shown, so a heading carrying either is declined rather than matched
/// against a partial line.
///
/// **Issue #1720** (the built-in screenplay preset) widens this to the
/// two remaining literal-line grammar shapes `docs/prose-dialect-spec.md`
/// §3.5b now names as claim candidates alongside a prose line and scene
/// heading — the spec's clause was amended in the same PR (rule 20d: a
/// ruling lands in the spec, not only in a code comment) — the ruling's own
/// natural-notation examples are cue/heading text, and the wave retro
/// posted on #1720 itself names the gap this closes: without it, real
/// `@NAME` cue and `(parenthetical)` lines are structurally invisible to
/// `claims`/`args` dispatch and always fall to `body::lower_one_item`'s
/// loud `E129`, no matter what a project or preset declares):
///
/// - A `CUE`'s `CUE_NAME` run qualifies the same way — exactly that one
///   child, nothing else. A cue carrying a trailing tag extension (§8d.4,
///   `@VENDOR #(v.o.)`) is declined, mirroring the heading/slug case: the
///   tag is structure the pattern is not shown.
/// - A `PARENTHETICAL`'s `TEXT` run (the text strictly between the
///   parens — `(`/`)` are tokens, not part of the child) qualifies the
///   same way; a parenthetical carrying trailing tags is declined too.
///
/// Both new arms feed the exact same `try_claim`/`try_dispatch`
/// mechanism unchanged — this function is the only seam that needed
/// widening; the block-capture terminator (`capture_block`) already
/// treats an upcoming `CUE`/`PARENTHETICAL` as "ends the run" regardless
/// of whether either is itself claimable, so nothing there changes.
fn candidate(node: &SyntaxNode) -> Option<(ElementKind, SyntaxNode)> {
    match node.kind() {
        N::CONTENT_LINE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::TEXT && children.next().is_none())
                .then_some((ElementKind::ContentLine, first))
        }
        N::SCENE_HEADING => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::SCENE_TITLE && children.next().is_none())
                .then_some((ElementKind::SceneHeading, first))
        }
        N::CUE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::CUE_NAME && children.next().is_none())
                .then_some((ElementKind::Cue, first))
        }
        N::PARENTHETICAL => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::TEXT && children.next().is_none())
                .then_some((ElementKind::Parenthetical, first))
        }
        _ => None,
    }
}
