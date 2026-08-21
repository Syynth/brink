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
//! @[convention(claims = "^INT\\. (?<place>.+)$", order = 10)]
//! fn interior(place) { return "— inside " + place + " —"; }
//!
//! flow main() {
//!   INT. MARKET SQUARE
//! }
//! ```
//!
//! Issue #2164 (`docs/decision-log.md` 2026-08-03) split the annotation
//! this module was written against: `claims = "…"` moved from
//! `@[element(…)]` to its own `@[convention(…)]` name, gaining a required
//! `order` property that now drives the precedence this module's own
//! doc describes below — see [`super::annotation`]'s module doc for the
//! full split.
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
//!   interpolation, glue, markup, label or embedded divert. A line carrying
//!   dynamic parts has no fixed text for a pattern to match, and capture
//!   spans over it would not point at anything real. This restriction
//!   applies identically to a `CUE`'s name and a `PARENTHETICAL`'s delivery
//!   text (issue #1720 widens [`candidate`] to these two shapes, alongside
//!   `CONTENT_LINE`/`SCENE_HEADING`).
//!
//!   A trailing `#tag` run is the one exception to "wholly literal", and it
//!   now applies uniformly across every claimed shape but `CONTENT_LINE`
//!   (issue #2077, `docs/decision-log.md` 2026-08-06 "Slug-bearing
//!   headings: strip structure, then match", widened by issue #2350's
//!   2026-08-07 "Cue/parenthetical tag extensions: strip-then-match,
//!   uniformly"): a `SCENE_HEADING`'s optional `[slug]`/trailing `#tag`s,
//!   and a `CUE`/`PARENTHETICAL`'s trailing tag extension (§8d.4, `@VENDOR
//!   #(v.o.)`), are all structure, not payload, so they are stripped
//!   before matching rather than causing a decline — see [`candidate`]'s
//!   own doc and [`try_claim`]'s "Issues #2077/#2350" comment for what
//!   happens to the stripped tags. A `CONTENT_LINE` carrying a tag still
//!   declines outright — neither issue widened that arm.
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
//! # Cross-file claiming reach (issue #2289)
//!
//! `docs/decision-log.md`'s 2026-08-05 ruling corrects a defect that
//! survived §9.1 item 4's confinement rule: *"it's never file local. you
//! configure conventions for a project, that's why they're conventions
//! and not 'local patterns'."* §9.1 item 4 restricted **declaring** a
//! claiming handler to the one module `brink.toml`'s `[project]
//! conventions` names (`E169`, `brink_analyzer::conventions_confinement`)
//! — but until this issue landed, nothing let a line in any *other* file
//! actually match against that module's handlers, which made a
//! correctly-declared conventions module claim nothing outside its own
//! file. [`collect`] now accepts an optional, already-ordered `external`
//! handler set — every `@[convention]` declared in the project's
//! configured conventions module, read off that file's own
//! `HirFile::claim_handlers` — and merges it with this file's local
//! declarations into one precedence-ordered dispatch table (see
//! [`Elements`]'s own doc for how `ClaimHandler::decl` tells the two
//! apart). The project-identity work (which file is the conventions
//! module, and reading its declared handlers) happens one layer up,
//! in `brink_db::queries::analysis::external_claim_handlers_query` — this
//! crate has no project database of its own, matching every other
//! project-identity-gated check here (`conventions_module_diagnostics`'s
//! `is_conventions_module` flag is the precedent this follows).
//!
//! Unlike the earlier #1863 design this replaces (deleted by issue #2165
//! alongside the dissolved `fn conventions()` comptime registration it was
//! built for), an injected handler now carries a **real** `order` and
//! `attach` — issue #2164 made `order` a required property of the
//! `@[convention]` declaration itself, so there is no separate
//! comptime-evaluated identity list to join against; `ClaimHandlerDecl` is
//! already the whole payload. Local and injected handlers are therefore
//! merged and sorted by `order` together, not chained as "local always
//! wins" — true project-wide precedence, not merely a last-resort fallback.
//!
//! `fn conventions()` registration itself stays dissolved machinery (issue
//! #2165, `docs/decision-log.md` 2026-08-03 "`fn conventions()` is
//! DISSOLVED — handler precedence is a property of the `@[element]`
//! annotation") — it never existed as anything but a design placeholder
//! and there is nothing left to build here. Dispatching to a `flow` target
//! (rather than a top-level `fn`) is also not here: `!name`'s placement is
//! legal on a `flow` too ([`super::annotation::is_consumed_position`]), but
//! [`collect`] only ever scans top-level `fn` declarations into the
//! dispatch table — the same restriction `claims` already has, for the
//! same reason (the rewrite is an expression call). `!name` dispatch never
//! had a cross-file counterpart at all — cross-file dispatch-name
//! resolution is `docs/prose-dialect-spec.md` §3.5b's own Deferred item,
//! so [`try_dispatch`] stays file-local; only `claims` (project-configured
//! conventions) reaches across files.
//!
//! What this does **not** do: an injected [`ClaimHandler`]'s `annotation`/
//! `name` ranges are copied straight off the declaring file's
//! `ClaimHandlerDecl` — real source positions, but in *another file's*
//! text. A consumer of `HirFile::element_matches` that assumes every range
//! it carries indexes into *this* file's own source (true for every local
//! match, and true before this issue) will misresolve an injected match's
//! `handler`/`annotation` fields. No such consumer exists yet (checked:
//! `brink-ide`'s only reader tests line equality, not these two fields) —
//! flagged here rather than either fabricating a cross-file-safe schema
//! this issue never asked for, or silently shipping the hazard unremarked.

use std::collections::BTreeMap;

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use regex_syntax::hir::Hir;
use rowan::{TextRange, TextSize};

use crate::hir::FileId;
use crate::{
    Content, ContentPart, Diagnostic, DiagnosticCode, ElementCapture, ElementDisposition,
    ElementKind, ElementMatch, Expr, Name, Path, Stmt, StringExpr, StringPart, Tag,
};

use super::SyntaxNode;
use super::provenance::native_provenance;
use crate::provenance::NodeClass;

/// One declared natural-notation handler: a top-level `fn` whose
/// `@[convention(claims = "…", order = N)]` pattern claims prose lines —
/// OR a handler injected from the project's configured conventions
/// module (issue #2289), for which `decl` is `None` (see that field's own
/// doc).
struct ClaimHandler {
    /// The handler's own name, carrying its declaration-site range — in
    /// the injected case, the range is in the DECLARING file (the
    /// conventions module), not this one.
    name: Name,
    /// Parameter names in declaration order — the argument order the
    /// rewritten call uses. Guaranteed by `E160`/`E167` to be exactly the
    /// pattern's named-capture set.
    params: Vec<String>,
    /// The compiled claiming pattern.
    pattern: regex::Regex,
    /// Range of the `@[convention(claims = "…", order = N)]` line itself
    /// — in the injected case, in the declaring file, same caveat as
    /// `name`.
    annotation: TextRange,
    /// The handler declaration's own range — used to suppress claiming
    /// inside the handler's own body (the staging rule). `None` for an
    /// injected handler (issue #2289): "own body is not claimable" is a
    /// same-file concept, and an injected handler's declaration range is
    /// meaningless — usually plain wrong — against `claimed` ranges in
    /// THIS file's own text; comparing them by coincidence of numeric
    /// offset would be a real bug, not a conservative no-op, so injected
    /// handlers skip the check entirely rather than risk it. `Some` (a
    /// local declaration) is also how [`Elements::handler_decls`] and the
    /// duplicate-order/duplicate-pattern diagnoses tell "declared in this
    /// file" apart from "injected from elsewhere" — see their own docs.
    decl: Option<TextRange>,
    /// The bare `block` clause (issue #1839): `true` when the trailing
    /// parameter is a `content`-typed block-capture receiver, not a
    /// regex-bound capture — see [`try_claim`]'s doc for what that changes
    /// about argument binding.
    block: bool,
    /// The claiming precedence (issue #2164, `docs/decision-log.md`
    /// 2026-08-03 "`order` is REQUIRED on `@[convention]`…") — a bare
    /// integer, required on every `@[convention]` declaration, so every
    /// `ClaimHandler` carries a real one (no "no order" case) — local
    /// *and* injected alike, since `order` lives on the declaration
    /// itself, not on some separate comptime-evaluated identity list
    /// (issue #2289: unlike the deleted #1863 injection seam this
    /// replaces, an injected handler's `order`/`attach` are real values
    /// read straight off `ClaimHandlerDecl`, not placeholders). [`collect`]
    /// sorts the merged local+injected set by this field ascending before
    /// [`try_claim`]/[`diagnose_duplicate_patterns`] ever see it — lower
    /// values are tried first, project-wide.
    order: i64,
    /// The `attach = StructName` clause (issue #2178), if declared — see
    /// `crate::ConventionAnnotation::attach`'s own doc. Real for an
    /// injected handler too (issue #2289) — see this struct's own `order`
    /// doc for why.
    attach: Option<String>,
}

/// One declared `!name`-dispatched handler: a top-level `fn` whose
/// `@[element(args = "…")]` pattern parses the remainder after a `!name`
/// sigil (issue #2004). Unlike [`ClaimHandler`], there is no `decl`
/// self-suppression field — a dispatched handler's own body is not exempt
/// from matching its own sigil (see the module doc's "What claims, and
/// what does not" section for why) — `!name` dispatch is file-local (same
/// doc, "Deliberately not here").
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
    /// Every claiming handler this file's prose is matched against, in
    /// ascending `order` — both this file's own local declarations
    /// (`decl: Some(...)`) and, since issue #2289, every handler injected
    /// from the project's configured conventions module (`decl: None`),
    /// merged and sorted together so precedence is total project-wide,
    /// not "local always wins". [`try_claim`] walks this single ordered
    /// list; [`Elements::handler_decls`] and the duplicate-order/
    /// duplicate-pattern diagnoses filter to the local subset only (see
    /// their own docs) — those checks are about what *this file*
    /// declares, not what it can claim with.
    handlers: Vec<ClaimHandler>,
    /// `!name`-dispatched handlers (issue #2004), keyed by dispatch name
    /// (the `name = "…"` alias if declared, else the `fn`'s own name) — a
    /// `BTreeMap`, not a `HashMap`, per this workspace's determinism rule,
    /// though the only operation ever performed over it is a by-key
    /// lookup (`try_dispatch`) or an insert-if-absent during [`collect`]'s
    /// single source-order pass, neither of which is order-sensitive on
    /// its own; a `BTreeMap` costs nothing here and removes any doubt.
    /// Two declarations naming the same dispatch name is an interim
    /// first-declared-wins ([`collect`]'s own doc) — unlike `claims`
    /// precedence, which issue #2164 (`docs/decision-log.md` 2026-08-03)
    /// made total, explicit, and authored via `order` (see [`try_claim`]'s
    /// own doc), `!name` dispatch has no `order` of its own and stays on
    /// the interim first-declared-wins rule for now.
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
        self.handlers.is_empty()
    }

    /// This file's own locally declared handlers only — `handlers`
    /// filtered to `decl.is_some()` (see [`ClaimHandler::decl`]'s own
    /// doc), preserving the merged vec's overall `order`. Shared by
    /// [`handler_decls`](Self::handler_decls) and every diagnostic that
    /// means "declared IN THIS FILE": an injected handler (issue #2289)
    /// was declared elsewhere, so it must never appear in any of them —
    /// counting it would falsely accuse the *injecting* file of a
    /// confinement violation, or diagnose a duplicate against a
    /// declaration that isn't really here.
    fn local_handlers(&self) -> impl Iterator<Item = &ClaimHandler> {
        self.handlers.iter().filter(|h| h.decl.is_some())
    }

    /// Every claiming handler *declared* in this file, in ascending
    /// `@[convention]` `order` (issue #2164, `docs/decision-log.md`
    /// 2026-08-03 — `handlers` is already sorted by `order` before
    /// [`collect`] returns, so this reflects precedence, not textual
    /// declaration position) —
    /// [`HirFile::claim_handlers`](crate::HirFile::claim_handlers)'s
    /// source (issue #1844's confinement check). Deliberately independent
    /// of `matches`: a handler that claims nothing in its own file is still
    /// a declaration, and still checkable. Reads
    /// [`local_handlers`](Self::local_handlers) only — never an injected
    /// handler (issue #2289).
    pub(super) fn handler_decls(&self) -> Vec<crate::ClaimHandlerDecl> {
        self.local_handlers()
            .map(|h| crate::ClaimHandlerDecl {
                name: h.name.clone(),
                annotation: h.annotation,
                params: h.params.clone(),
                pattern: h.pattern.as_str().to_string(),
                block: h.block,
                order: h.order,
                attach: h.attach.clone(),
            })
            .collect()
    }
}

/// Collect every claiming handler declared in `root`.
///
/// Silent on everything `container.rs` already validated: each
/// `@[element(…)]`/`@[convention(…)]` line is parsed and checked against
/// its own declaration exactly once there — that validation pass's own
/// diagnostics are discarded here (into a throwaway `scratch` vec) rather
/// than threaded through, since re-reporting would double every
/// `E159`/`E160`/`E167`/`E178`. Duplicate-pattern diagnosis
/// ([`diagnose_duplicate_patterns`]) does *not* happen here either: it
/// needs to see which handlers actually fired during body lowering, which
/// hasn't happened yet at collection time — see that function's doc.
/// Duplicate-*order* diagnosis ([`diagnose_duplicate_order`]) DOES happen
/// here, since it needs no lowering ground truth — `order` is a static
/// property of the declaration alone.
///
/// `handlers` is sorted by `@[convention]`'s `order` (ascending) before
/// this returns — issue #2164's ruling: "the claiming walk takes its
/// precedence from that property instead of from declaration order." A
/// stable sort, so two declarations that (incorrectly) share an `order`
/// keep their declaration-order relative position — moot in practice,
/// since a shared `order` is `E179`, an `Error`-severity diagnostic that
/// fails the compile; the tie-break only matters for what a caller sees
/// while diagnostics are still being collected.
///
/// `external` is the issue #2289 injection point: every `@[convention]`
/// handler declared in the project's configured conventions module,
/// already resolved by the caller (`brink_db::queries::analysis::
/// external_claim_handlers_query`) — `None`/empty for the conventions
/// module's own file (it already has these declarations locally; the
/// caller skips injecting a file into itself) and for every project with
/// no conventions module configured. Each `external` entry is compiled
/// and merged into `handlers` alongside the local declarations, then the
/// **combined** set is sorted by `order` — real project-wide precedence,
/// not "local always wins" (contrast the deleted #1863 design, which had
/// no real `order` to sort an injected handler by). An external entry
/// whose name collides with a handler this file declares *locally* is
/// dropped: a local declaration always wins over an injected one of the
/// same name — in practice this only fires when a caller mistakenly
/// injects a file into its own lowering, which should never happen, but
/// costs nothing to guard against.
pub(super) fn collect(
    file_id: FileId,
    root: &SyntaxNode,
    external: Option<&[crate::ClaimHandlerDecl]>,
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
        // The declaration's own resolved return-type annotation (issue
        // #2178) — read the same way `container::lower_top_level_container`
        // does, since `convention_annotation`'s `attach = StructName`
        // clause (`E180`) validates against it by name.
        let return_type = decl
            .return_type()
            .as_ref()
            .and_then(super::types::lower_type_annotation);
        // `@[element]` (dispatch) and `@[convention]` (claiming) are two
        // independent annotation reads since issue #2164's split — both
        // are attempted on every top-level `fn`; a declaration carrying
        // neither contributes nothing to either table.
        let mut scratch: Vec<Diagnostic> = Vec::new();
        let element = super::annotation::element_annotation(file_id, &node, &params, &mut scratch);
        let convention = super::annotation::convention_annotation(
            file_id,
            &node,
            &params,
            return_type.as_ref(),
            &mut scratch,
        );
        let param_names: Vec<String> = params.into_iter().map(|p| p.name.text).collect();

        if let Some(convention) = convention
            && let Ok(pattern) = regex::Regex::new(&convention.pattern)
        {
            handlers.push(ClaimHandler {
                name: name.clone(),
                params: param_names.clone(),
                pattern,
                annotation: convention.range,
                decl: Some(node.text_range()),
                block: convention.block,
                order: convention.order,
                attach: convention.attach.map(|a| a.text),
            });
        }
        if let Some(element) = element
            && let Ok(pattern) = regex::Regex::new(&element.pattern)
        {
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
    // Issue #2289: merge in every handler injected from the project's
    // conventions module, dropping any whose name collides with a LOCAL
    // declaration (this function's own doc explains why a collision
    // should never happen in practice). Each is compiled fresh — a
    // pattern that failed to compile in its own declaring file would
    // already be diagnosed there and never reach `ClaimHandlerDecl`, so a
    // compile failure here is unreachable in practice, but declining
    // (rather than panicking) costs nothing.
    if let Some(external) = external {
        for candidate in external {
            if handlers.iter().any(|h| h.name.text == candidate.name.text) {
                continue;
            }
            let Ok(pattern) = regex::Regex::new(&candidate.pattern) else {
                continue;
            };
            handlers.push(ClaimHandler {
                name: candidate.name.clone(),
                params: candidate.params.clone(),
                pattern,
                annotation: candidate.annotation,
                decl: None,
                block: candidate.block,
                order: candidate.order,
                attach: candidate.attach.clone(),
            });
        }
    }
    // Issue #2164: precedence now comes from `order`, not declaration
    // position — see this function's own doc. `E179` (duplicate `order`)
    // is diagnosed separately by [`diagnose_duplicate_order`], called by
    // `super::lower` right after this returns — a real diagnostic sink,
    // unlike this function's own `scratch`-discarding per-declaration
    // reads. A stable sort over the COMBINED local+injected set — issue
    // #2289 makes precedence total project-wide, not "local always wins".
    handlers.sort_by_key(|h| h.order);
    Elements {
        handlers,
        dispatch,
        matches: Vec::new(),
    }
}

/// `E179` (issue #2164, `docs/decision-log.md` 2026-08-03 "`order` is
/// REQUIRED on `@[convention]`…"): flag every group of two or more of this
/// file's locally declared claiming handlers that share the same `order`.
///
/// A static check, unlike [`diagnose_duplicate_patterns`] — it needs no
/// lowering ground truth (`elements.matches`), only the declared `order`
/// values themselves, so `super::lower` calls this right after
/// [`collect`] returns, before `walk_top_level` even runs.
///
/// Reported against **every** conflicting declaration in a shared-`order`
/// group (each one's own `annotation` range), the duplicate-definition
/// posture the ruling calls for — not a single "first one wins" report
/// the way `E048` (duplicate directive on one target) is, since here the
/// declarations are different `fn`s and none is more "the real one" than
/// another. Each diagnostic's message names every *other* declaration in
/// the group and the shared `order` value, the same "name both/all
/// conflicting declarations" posture `E169`'s sibling
/// (`brink_analyzer::conventions_confinement`) already takes. Scoped to
/// `elements.handlers` (this file's own declarations, matching
/// `handler_decls`'s own "declared IN THIS FILE" posture).
///
/// Grouped by `order` rather than walked as all pairs, so three or more
/// handlers sharing one `order` value produce exactly one diagnostic per
/// participating declaration (naming every *other* handler in the group),
/// not one per pair — an all-pairs walk over a group of size `k` would
/// emit `k * (k - 1)` diagnostics, each declaration repeated `k - 1` times.
///
/// `O(n²)` worst case over a file's claiming handlers (the grouping pass
/// itself, plus building each message's "other declarations" list) — see
/// [`diagnose_duplicate_patterns`]'s own doc for why that headroom is
/// never a real concern at this scale.
///
/// Reads [`Elements::local_handlers`], never an injected one (issue
/// #2289) — an order collision between this file's own declaration and a
/// handler injected from elsewhere is not two declarations in *one*
/// module sharing a value, so it is not this check's concern.
pub(super) fn diagnose_duplicate_order(
    file_id: FileId,
    elements: &Elements,
    diags: &mut Vec<Diagnostic>,
) {
    let handlers: Vec<&ClaimHandler> = elements.local_handlers().collect();

    // Group handler indices by their shared `order` value. `BTreeMap`, not
    // `HashMap`, per this workspace's determinism rule — iteration order
    // below must not depend on hash-bucket placement.
    let mut by_order: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, handler) in handlers.iter().enumerate() {
        by_order.entry(handler.order).or_default().push(i);
    }

    for (order, indices) in &by_order {
        if indices.len() < 2 {
            continue;
        }
        // One diagnostic per participating declaration, naming every
        // *other* declaration in this `order` group and the shared value
        // — the duplicate-definition posture (report at each conflicting
        // declaration, name the others), not a single "first wins" report.
        for &i in indices {
            let others = indices
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| format!("`{}`", handlers[j].name.text))
                .collect::<Vec<_>>()
                .join(", ");
            diags.push(Diagnostic {
                file: file_id,
                range: handlers[i].annotation,
                message: format!(
                    "`{name}` shares `order = {order}` with {others} — every \
                     `@[convention]` in a module must have a distinct `order`",
                    name = handlers[i].name.text,
                ),
                code: DiagnosticCode::E179,
            });
        }
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
/// `elements.handlers` is sorted by `@[convention]`'s `order` (issue
/// #2164 — ascending, [`collect`]'s own doc), which is also
/// [`try_claim`]'s dispatch order — see that function's doc for why
/// "earlier" (lower `order`) is the same as "wins". An identical pattern
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
///
/// Reads [`Elements::local_handlers`], never an injected one (issue
/// #2289) — this file's own declarations are what "byte-identical to an
/// earlier-declared one" and "dead code" mean here; an injected handler
/// was not declared in this file and is never a candidate for either
/// role, matching [`Elements::handler_decls`]'s own "declared IN THIS
/// FILE" posture.
pub(super) fn diagnose_duplicate_patterns(
    file_id: FileId,
    elements: &Elements,
    diags: &mut Vec<Diagnostic>,
) {
    let handlers: Vec<&ClaimHandler> = elements.local_handlers().collect();
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
/// # Dispatch order
///
/// When more than one handler's pattern matches a line, the first one in
/// `elements.handlers` wins — [`Iterator::find`] below, over a `Vec`
/// [`collect`] sorts by `@[convention]`'s required `order` property
/// (ascending) before returning it. Issue #2164 (`docs/decision-log.md`
/// 2026-08-03) makes this the RULED mechanism, replacing the interim
/// declaration-order rule issue #1848 first documented here: precedence
/// is now total, explicit, and authored on each declaration, never
/// inferred from textual position. Two patterns that can both claim one
/// line still get no diagnostic except the narrow byte-identical case
/// ([`diagnose_duplicate_patterns`], issue #1848) — a genuinely
/// overlapping (non-identical) pair silently prefers the lower-`order`
/// one, exactly the failure mode "pattern power proportional to
/// auditability" (`docs/prose-dialect-spec.md` §3.5b) exists to keep
/// visible, not eliminate outright.
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
#[expect(
    clippy::too_many_lines,
    reason = "one already-large claim-dispatch body; issue #2079's compact-cue \
              desugar adds one more per-flavor branch (block/attach/plain) to \
              an existing three-way split, not a second responsibility — \
              splitting it would scatter one claim's bookkeeping across \
              several functions threading the same half-dozen locals"
)]
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
    // `decl` suppresses a claim inside the handler's own body (the staging
    // rule — see `ClaimHandler::decl`'s doc); `None` (an injected handler,
    // issue #2289) never suppresses anything, since a range in another
    // file can never contain a range in this one. Iterates the merged
    // local+injected set in `order` — first match wins, project-wide.
    let handler = elements.handlers.iter().find(|h| {
        !h.decl.is_some_and(|d| d.contains_range(claimed)) && h.pattern.is_match(trimmed)
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

    // Issues #2077/#2350: recover the pieces `candidate` stripped from
    // `node`'s literal text before this point — see `element_extras`'s own
    // doc for what each one is and why. `None`/empty for `CONTENT_LINE`,
    // the one candidate kind neither issue widened (candidate would already
    // have returned `None` for a tag-carrying one).
    //
    // Deliberately called here — below both the handler lookup (`?` above)
    // and the per-param capture loop (`?` inside it), not up where
    // `candidate` first returns — because `element_extras` is not
    // side-effect-free: it runs `body::lower_tag` on every trailing tag,
    // which can itself push a diagnostic (`E172`, for an `@`-leading tag).
    // Calling it any earlier would run that speculatively for a claim this
    // function might still abandon (no handler matches, or an alternation
    // branch left a param's capture group unpopulated), reporting a
    // diagnostic against a line that never actually lowered through this
    // handler at all.
    let (slug, extra_tags) = element_extras(node, file_id, diags);

    // Every field taken from `handler` past this point is copied/cloned out
    // — `handler` (and so the borrow of `elements.handlers` it holds) is
    // never referenced again, which is what lets the `block` branch below
    // borrow `elements` mutably for the recursive capture.
    let handler_name = handler.name.clone();
    let handler_annotation = handler.annotation;
    // Issue #2108/#2178: whether this handler declares `attach = StructName`
    // — copied out now, alongside `handler_name`/`handler_annotation`, for
    // the same reason those two are (this function's own note above): the
    // borrow of `elements.handlers`/`.external` `handler` holds must end
    // before the `is_block`/attach branches below borrow `elements` mutably.
    let is_attach = handler.attach.is_some();
    // Attach-mode never emits a `Stmt::Content` at all — ruling item 6,
    // "AN EVENT EXISTS IFF A LINE EXISTS" (`docs/decision-log.md`
    // 2026-08-03) — so there is no line left for a tag-bearing claim's
    // `extra_tags` to ride on if this claim went through
    // (`build_attach_stmts` only ever emits `Stmt::AttachElement`/
    // `Stmt::EndElementRun`, neither of which carries tags). Declining the
    // claim here — rather than silently discarding the tags, the gap
    // review caught for headings (#2344) and #2350 confirms applies
    // identically to a `CUE`/`PARENTHETICAL` — falls through to the same
    // loud `E129` `body::lower_one_item`'s default arm already reports for
    // any other unclaimed element (house rule 9: never a silent drop).
    // Scoped to attach handlers with a nonempty `extra_tags` only:
    // block-mode and ordinary Call-mode handlers both still deliver a
    // tag-bearing claim's tags through the existing `Content.tags` channel
    // below, regardless of element kind.
    if is_attach && !extra_tags.is_empty() {
        return None;
    }
    // Issue #2289: `decl.is_none()` is exactly "this handler was injected
    // from the project's configured conventions module, not declared in
    // this file" (see `ClaimHandler::decl`'s own doc) — copied out now for
    // the same borrow-lifetime reason as `is_attach` above.
    let is_injected = handler.decl.is_none();

    let mut call_args: Vec<Expr> = captures
        .iter()
        .map(|c| {
            Expr::String(StringExpr {
                parts: vec![StringPart::Literal(c.text.clone())],
            })
        })
        .collect();

    // Issue #2079, RULED 2026-08-06 "Compact cue desugars to cue + content
    // line": a `COMPACT_CUE`'s fused dialogue (its second child, an
    // ordinary `CONTENT_LINE`) is not a sibling of `node` — it lives
    // *inside* it — but the ruling treats it as the first line of whatever
    // run follows the (virtual) cue line, in every handler flavor. `None`
    // for every other candidate kind, so this changes nothing for `CUE`/
    // `SCENE_HEADING`/`PARENTHETICAL`/plain `CONTENT_LINE` claims.
    let compact_dialogue = (node.kind() == N::COMPACT_CUE)
        .then(|| node.children().find(|c| c.kind() == N::CONTENT_LINE))
        .flatten();
    // Review finding on #2079's PR: a compact cue's fused dialogue is not
    // shown to the pattern, but it still needs to be checked before it is
    // folded into the claim's captured run/fragment — `content::content_line`
    // parses the SAME shapes here that `is_plain_content_line`'s own doc
    // names for a sibling body item: a leading `(ident)` right after the
    // `@NAME:` prefix is a `LABEL` (`content.rs`'s own comment: the check
    // "fires right after a compact cue's `@NAME:` prefix"), and a trailing
    // `-> target`/`->->`/`{?}` folds into a `DIVERT_STMT`/`TUNNEL_CALL`/
    // `CHOICE_POINT` child of the same `CONTENT_LINE`. Both are "element-
    // level" in exactly the sense `capture_block`'s terminator search
    // already treats as run-ending for a sibling — but `dialogue` isn't
    // offered to that search at all (`capture_block_with_compact_dialogue`
    // takes it unconditionally as the run's first line), so without this
    // check a fused divert would transfer control before the fragment/run's
    // own closing statement (`EndFragment`/`EndElementRun`) ever executes,
    // and a fused label would silently vanish into `Stmt::LabeledBlock`.
    // Declining the whole claim here — before anything is recorded in
    // `elements.matches` or emitted to `diags` — mirrors `try_dispatch`'s
    // own precedent (this file, `candidate(&remainder)?` a few hundred
    // lines below): a fused remainder that isn't itself claimable makes the
    // WHOLE line decline, falling through to the loud "parses but has no
    // HIR lowering yet" `E129` default rather than silently corrupting the
    // run.
    if compact_dialogue
        .as_ref()
        .is_some_and(|dialogue| !is_plain_content_line(dialogue))
    {
        return None;
    }
    // Shared by the `is_block`/`is_attach` arms below: dispatches to
    // whichever of `capture_block`/`capture_block_with_compact_dialogue`
    // applies, so neither arm repeats the `match`.
    let capture = |following: &[SyntaxNode],
                   elements: &mut Elements,
                   diags: &mut Vec<Diagnostic>| match &compact_dialogue {
        Some(dialogue) => {
            capture_block_with_compact_dialogue(file_id, dialogue, following, elements, diags)
        }
        None => capture_block(file_id, following, elements, diags),
    };

    let mut consumed = 0;
    let mut content_range = None;
    // Only populated in the `is_attach` branch below — the captured
    // following run's own (already-lowered) statements, spliced back into
    // the returned stream after this handler's own `AttachElement`/
    // `EndElementRun` pair. Kept outside the `if`/`else if` so `capture_block`
    // (which mutably borrows `elements` to record any interior claims) runs
    // **exactly once** per claimed line — calling it twice would re-lower
    // the captured run and double every diagnostic/`ElementMatch` it
    // produces.
    let mut attach_run_stmts: Vec<Stmt> = Vec::new();
    // `is_block` (wrap mode, #1839) is checked first, so this `if`/`else
    // if` is the one place the ordering decision would still matter — but
    // issue #2264 (`annotation::parse_convention`, `E186`) now rejects a
    // declaration combining `block` with `attach = StructName` outright, at
    // the declaration, before it can ever reach here: `is_block` and
    // `is_attach` are never both true for any registered handler.
    // `block`+`attach` together had no ruled semantics of its own — E186's
    // own doc records that as a deliberate non-decision, not an oversight.
    if is_block {
        let (fragment_stmts, n, range) = capture(following, elements, diags);
        consumed = n;
        content_range = range;
        call_args.push(Expr::Fragment(fragment_stmts));
    } else if is_attach {
        // Issue #2108, `docs/decision-log.md` 2026-08-03 "The element
        // output model: attachment is block-level metadata, delivery is
        // per-line". The "following run" an attach handler's captures
        // attach to is the exact same terminator search `block`-mode uses
        // (item 4: "the run IS the block") — reused here, but the captured
        // statements are spliced back into the returned stream as ordinary
        // sibling statements (bracketed by `EndElementRun`) rather than
        // embedded as this call's own argument.
        let (run_stmts, n, range) = capture(following, elements, diags);
        consumed = n;
        content_range = range;
        attach_run_stmts = run_stmts;
    }
    // Neither `block` nor `attach`: the handler's own claimed line still
    // lowers to a plain call (below), but a compact cue's fused dialogue
    // must not vanish — it lowers right after the call, as though it were a
    // bare sibling `CONTENT_LINE` nobody claimed (no `following` absorbed:
    // a plain handler never has been).
    let plain_dialogue_stmts = (!is_block && !is_attach && compact_dialogue.is_some()).then(|| {
        let (stmts, _n, range) = capture(&[], elements, diags);
        content_range = range;
        stmts
    });

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
            crosses_module_wall: false,
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
        injected: is_injected,
        slug,
    });

    if is_attach {
        return Some((
            build_attach_stmts(call, attach_run_stmts, following),
            consumed,
        ));
    }

    let mut stmts = vec![
        Stmt::Content(Content {
            ptr: Some(native_provenance(file_id, NodeClass::Content, node)),
            parts: vec![ContentPart::Interpolation(call)],
            // Empty for `CONTENT_LINE` (a tag-carrying one never reaches
            // here — see this function's own "Issues #2077/#2350" comment
            // above); a claimed heading/cue/parenthetical's own trailing
            // tags ride this same field, the existing ordinary per-line
            // tag channel.
            //
            // This is a deliberate INTERIM carrier, not a ruled semantic:
            // `docs/prose-dialect-spec.md` §8b.4 rules a *header* line's
            // trailing `#tag`s **container-level per-flow tags** — the
            // authoring surface issue #474 (the per-flow tag API) is
            // iceboxed waiting for — and the 2026-08-06/2026-08-07 rulings
            // this function implements say only that tags are stripped
            // before matching, nothing about which channel delivers them.
            // Routing them here, onto the claimed line's own output
            // `Stmt::Content`, is the closest existing mechanism and
            // better than the silent drop these rulings would otherwise
            // have reintroduced, but it is ordinary per-line runtime tag
            // delivery, not §8b.4's per-flow tags (which apply to headings
            // only anyway — a cue/parenthetical tag extension, §8d.4, was
            // never claimed to be a per-flow tag in the first place).
            // Re-route (or remove, if #474 supersedes this entirely) once
            // #474 lands.
            tags: extra_tags,
        }),
        Stmt::EndOfLine,
    ];
    // Compact cue (#2079), plain (non-attach, non-block) handler flavor:
    // the fused dialogue's own statements go right after the call's line —
    // see `plain_dialogue_stmts`'s own comment above for why this can
    // never be `Some` unless `compact_dialogue` is, and `None` here is the
    // overwhelming common case (every non-compact-cue claim, and every
    // compact cue whose handler is `block`/`attach`) so this is a no-op
    // extend for them.
    if let Some(dialogue_stmts) = plain_dialogue_stmts {
        stmts.extend(dialogue_stmts);
    }
    Some((stmts, consumed))
}

/// [`try_claim`]'s attach-mode (issue #2108) rewrite: `call` becomes a
/// `Stmt::AttachElement` rather than ordinary `Stmt::Content`, followed by
/// the captured following run's own already-lowered statements and,
/// usually, a closing `EndElementRun`.
///
/// Ruling item 6: "AN EVENT EXISTS IFF A LINE EXISTS" — an attaching
/// convention emits no `Stmt::Content`/`Stmt::EndOfLine` at all.
///
/// **The one exception** (found by tracing the actual bytecode for `cue`
/// immediately followed by `parenthetical` — #2108's own fixture):
/// [`capture_block`]'s terminator search stops at ANY element-level line,
/// including another `CUE`/`PARENTHETICAL` — so `cue`'s own captured run is
/// *empty* when a `parenthetical` immediately follows it, exactly the
/// shape ruling item 3 ("cue and parenthetical both attach to the SAME
/// run") requires to chain. Closing right there (as an earlier version of
/// this code unconditionally did) would clear `cue`'s data before
/// `parenthetical`'s own `AttachElement` — or the dialogue after it — ever
/// reads it. So: an empty capture caused SPECIFICALLY by an
/// immediately-following `CUE`/`PARENTHETICAL` node leaves the run open
/// (no `EndElementRun` here — the following claim's own rewrite closes it
/// once IT finishes); any other empty-capture reason (a blank line, a
/// heading, a dispatch, end of statements) closes immediately, so an
/// attach with genuinely nothing after it never leaks into unrelated later
/// content.
fn build_attach_stmts(
    call: Expr,
    attach_run_stmts: Vec<Stmt>,
    following: &[SyntaxNode],
) -> Vec<Stmt> {
    let chains_into_next_attach = attach_run_stmts.is_empty()
        && following
            .first()
            .is_some_and(|n| matches!(n.kind(), N::CUE | N::PARENTHETICAL));
    let mut stmts = vec![Stmt::AttachElement(call)];
    stmts.extend(attach_run_stmts);
    if !chains_into_next_attach {
        stmts.push(Stmt::EndElementRun);
    }
    stmts
}

/// [`try_claim`]'s issue #2077/#2350 seam: recover the pieces `candidate`
/// stripped from `node`'s literal text before matching
/// (`docs/decision-log.md` 2026-08-06 "Slug-bearing headings: strip
/// structure, then match", generalized by 2026-08-07 "Cue/parenthetical tag
/// extensions: strip-then-match, uniformly"). `(None, Vec::new())` for
/// every other `node` kind — `CONTENT_LINE` still declines outright on a
/// tag, since #1720/#2350 never widened `candidate`'s `CONTENT_LINE` arm
/// the way they did `SCENE_HEADING`/`CUE`/`PARENTHETICAL`.
///
/// - The slug: the *address capture* role `docs/prose-dialect-spec.md`
///   §8b.5 reserves, returned as a reserved `ElementCapture` for the caller
///   to store ALONGSIDE `ElementMatch::captures`, not merged into it —
///   `captures`' own doc says "bound into the call, in parameter order",
///   and the slug is never bound into the rewritten call. Named `slug`,
///   not the spec's more generic "address": only a scene heading has one
///   today (`SCENE_SLUG`, §3.3), so the concrete CST/spec term is more
///   honest than a generalization nothing else uses yet — a future element
///   gaining its own address capture can rename/generalize then. Wiring
///   the slug into structure/`DefinitionId` (what would make it
///   load-bearing rather than descriptive) is heading→stitch promotion,
///   issue #2078 — deliberately untouched here; a caller reads a real
///   source span and nothing more. `CUE`/`PARENTHETICAL` have no address
///   capture of their own, so this is always `None` for them.
/// - The tags: the EXISTING tag channel (`Content.tags`, via the same
///   `lower_tag` every other tagged line already goes through) — not a
///   second delivery mechanism invented for headings, and not a new one
///   invented for cues/parentheticals either. This is a deliberate INTERIM
///   carrier pending issue #474 (the per-flow tag API), not a ruled
///   semantic — see `try_claim`'s own call site for why. That interim
///   posture is what #2350's ruling explicitly carries over unchanged
///   ("the heading-tags delivery caveat from #2344 … applies identically
///   here"): `docs/prose-dialect-spec.md` §8b.4 names a *heading's*
///   trailing `#tag`s container-level per-flow tags, a concept a cue or
///   parenthetical tag extension (§8d.4) never claimed to be — both still
///   ride the same ordinary per-line `Content.tags` field regardless.
fn element_extras(
    node: &SyntaxNode,
    file_id: FileId,
    diags: &mut Vec<Diagnostic>,
) -> (Option<ElementCapture>, Vec<Tag>) {
    let slug = ast::SceneHeading::cast(node.clone()).and_then(|heading| {
        heading
            .slug()
            .and_then(|s| s.name_token())
            .map(|tok| ElementCapture {
                name: "slug".to_string(),
                text: tok.text().to_string(),
                range: tok.text_range(),
            })
    });
    let tags = raw_element_tags(node)
        .iter()
        .map(|t| super::body::lower_tag(file_id, t, diags))
        .collect();
    (slug, tags)
}

/// The trailing `#tag` nodes [`element_extras`] recovers for `node`'s own
/// kind — empty for anything but `SceneHeading`/`Cue`/`Parenthetical`
/// (`CONTENT_LINE` never reaches here; see [`element_extras`]'s own doc for
/// why). Factored out of `element_extras` itself (issue #2351's review
/// finding) so it and [`has_trailing_tags`] — a pure predicate a caller
/// outside this module needs — select the exact same set of tag nodes and
/// can never drift apart the way `candidate`'s selection and a hand-copied
/// duplicate would.
fn raw_element_tags(node: &SyntaxNode) -> Vec<ast::Tag> {
    if let Some(heading) = ast::SceneHeading::cast(node.clone()) {
        return heading.tags().collect();
    }
    if let Some(cue) = ast::Cue::cast(node.clone()) {
        return cue.tags().collect();
    }
    if let Some(parenthetical) = ast::Parenthetical::cast(node.clone()) {
        return parenthetical.tags().collect();
    }
    Vec::new()
}

/// Whether `node` carries at least one trailing tag in the position
/// [`element_extras`] would recover tags from — issue #2351's review
/// finding: `hir::classify::classify_node_compiled` needs this exact
/// answer to mirror [`try_claim`]'s own attach-mode decline (`if is_attach
/// && !extra_tags.is_empty() { return None; }`, above) for a caller that
/// never runs `try_claim` itself and must not trigger `element_extras`'s
/// diagnostic side effects (`lower_tag`'s `E172`) just to ask the
/// question. Built on the exact same [`raw_element_tags`] selection
/// `element_extras` itself uses, so the two can never disagree on which
/// nodes have tags at all.
#[must_use]
pub(crate) fn has_trailing_tags(node: &SyntaxNode) -> bool {
    !raw_element_tags(node).is_empty()
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

/// [`capture_block`]'s twin for a compact cue's fused dialogue (issue
/// #2079, RULED 2026-08-06 "Compact cue desugars to cue + content line").
///
/// `dialogue` is unconditionally the captured run's first line — it is
/// **not** run through [`is_plain_content_line`]/[`blank_line_precedes`]
/// the way a real sibling is: those two checks exist to decide whether a
/// *sibling* item is eligible to extend an already-open run, and `dialogue`
/// isn't a sibling being offered for absorption, it is structurally *part
/// of* the compact cue itself. That said, the caller ([`try_claim`]) has
/// already required `dialogue` itself to satisfy [`is_plain_content_line`]
/// before ever reaching here — a fused `LABEL` or trailing divert/choice on
/// `dialogue` declines the WHOLE claim instead (review finding, #2079's PR:
/// unconditionally absorbing either shape here let a divert transfer
/// control before this run's own closing statement ran, corrupting the
/// runtime's fragment/attach-run bookkeeping). So by the time this function
/// runs, `dialogue` is known plain — this function itself does not
/// re-check it, it only extends the run with whatever *plain* siblings
/// follow. `following`'s normal terminator search still applies to
/// whatever comes after `dialogue` — unchanged from before this finding.
///
/// Returns the same three-part shape as [`capture_block`], except the
/// range is never `None`: `dialogue` alone guarantees at least one
/// captured item.
fn capture_block_with_compact_dialogue(
    file_id: FileId,
    dialogue: &SyntaxNode,
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
    let mut captured: Vec<SyntaxNode> = Vec::with_capacity(1 + end);
    captured.push(dialogue.clone());
    captured.extend(following[..end].iter().cloned());
    let range = captured
        .last()
        .map(|last| dialogue.text_range().cover(last.text_range()));
    let stmts = super::body::lower_items(file_id, &captured, 0, elements, diags);
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
            crosses_module_wall: false,
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
        // `!name` dispatch never has a cross-file counterpart (this
        // module's own doc, "Deliberately not here") — every
        // `DispatchHandler` is declared in this same file.
        injected: false,
        // A `BANG_DISPATCH`'s remainder is always a fused `CONTENT_LINE`
        // (this function's own comment above) — never a `SCENE_HEADING` —
        // so there is never a slug to recover here (issue #2077 is a
        // `try_claim`-only concern).
        slug: None,
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
/// `TAG`, `GLUE_NODE`, `ESCAPE`, embedded divert or choice point).
///
/// A `SCENE_HEADING`'s title run qualifies by selecting its `SCENE_TITLE`
/// child, **not** by requiring it be the heading's only child (issue
/// #2077, `docs/decision-log.md` 2026-08-06 "Slug-bearing headings: strip
/// structure, then match"): the heading's optional `[slug]` and trailing
/// `#tag`s are structure the pattern is never shown, so they are simply
/// not selected — rather than causing the whole heading to decline, the
/// way an equivalent shape does for the other three node kinds below.
/// `try_claim` recovers both stripped pieces separately: the slug as a
/// reserved capture (the *address capture* role
/// `docs/prose-dialect-spec.md` §8b.5 reserves), the tags through the
/// same `Content.tags` channel any other tagged line already uses. This
/// makes `SCENE_HEADING` the one arm below whose "wholly literal" bar is
/// about the *title* alone, not the whole node.
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
/// - A `CUE`'s `CUE_NAME` run qualifies the same way `SCENE_TITLE` does —
///   selecting the `CUE_NAME` child without requiring it be the only one.
///   **Issue #2350** (`docs/decision-log.md` 2026-08-07 "Cue/parenthetical
///   tag extensions: strip-then-match, uniformly") extends the #2077
///   heading rule here: a `CUE`'s trailing tag extension (§8d.4, `@VENDOR
///   #(v.o.)`) is structure the pattern is never shown, exactly like a
///   heading's `[slug]`/`#tag`s, so any run of trailing `TAG` children
///   after `CUE_NAME` no longer causes a decline. `try_claim` recovers
///   those tags the same way it recovers a heading's — see
///   [`element_extras`]'s own doc.
/// - A `PARENTHETICAL`'s `TEXT` run (the text strictly between the
///   parens — `(`/`)` are tokens, not part of the child) qualifies the
///   same way, with the identical #2350 widening: trailing `TAG` children
///   after `TEXT` no longer cause a decline.
///
/// **Issue #2079** (RULED 2026-08-06, "Compact cue desugars to cue +
/// content line") adds a third arm: `COMPACT_CUE` (`@NAME: text`)
/// qualifies against its `CUE_NAME` segment alone, exactly like a bare
/// `CUE`'s own arm — but unlike the two arms above, `COMPACT_CUE` is not a
/// lone-child node (it always carries a second child, the fused dialogue
/// `CONTENT_LINE`), so its arm below deliberately does not require
/// `children.next().is_none()`. The fused dialogue itself is never shown
/// to the pattern here — `try_claim` desugars it separately (its own
/// "Compact cue" note) and, unlike the two arms above, does not feed the
/// *same* mechanism unchanged: `try_claim` requires the fused dialogue to
/// independently satisfy `is_plain_content_line` before folding it into
/// the claim's captured run, declining the whole line otherwise (review
/// finding, #2079's PR) — a check the `CUE`/`PARENTHETICAL` arms have no
/// analog of, since neither carries any fused content of its own.
///
/// The `CUE`/`PARENTHETICAL` arms feed the exact same `try_claim`/
/// `try_dispatch` mechanism unchanged; the block-capture terminator
/// (`capture_block`) already treats an upcoming `CUE`/`COMPACT_CUE`/
/// `PARENTHETICAL` as "ends the run" regardless of whether any of them is
/// itself claimable, so nothing there changes.
///
/// `pub(crate)` (issue #2351): this is the ONE place the sub-node selection
/// rules above are written down. `crate::hir::classify`'s node-aware
/// entry points (`classify_node_compiled`, `nearest_element_candidate`)
/// call this exact function — via `super::candidate` in
/// `hir::lower_native::mod`'s crate-visible re-export — rather than
/// re-deriving the selection against the raw line text, which is
/// precisely the divergence #2351 exists to close: a copy would
/// re-diverge from this one the next time a `candidate` arm changes.
pub(crate) fn candidate(node: &SyntaxNode) -> Option<(ElementKind, SyntaxNode)> {
    match node.kind() {
        N::CONTENT_LINE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::TEXT && children.next().is_none())
                .then_some((ElementKind::ContentLine, first))
        }
        // Issue #2077 (`docs/decision-log.md` 2026-08-06, "Slug-bearing
        // headings: strip structure, then match"): unlike the other three
        // arms, a `SCENE_HEADING` does NOT require its `SCENE_TITLE` to be
        // the only child — an optional `SCENE_SLUG` and any trailing `TAG`s
        // are grammar-parsed siblings, not part of the title. Selecting the
        // `SCENE_TITLE` child directly (rather than requiring it be alone)
        // is the whole fix: the pattern sees only the title text exactly as
        // it always did, so no existing preset pattern needs to change.
        // `try_claim` recovers the stripped slug/tags separately — see its
        // own "Issue #2077" comment.
        N::SCENE_HEADING => node
            .children()
            .find(|c| c.kind() == N::SCENE_TITLE)
            .map(|title| (ElementKind::SceneHeading, title)),
        // Issue #2350: unlike the pre-fix "exactly one child" rule, a
        // trailing run of `TAG` children (the cue's tag extension, §8d.4)
        // no longer disqualifies the node — only a non-`TAG` sibling after
        // `CUE_NAME` still declines (e.g. a `COMPACT_CUE`'s fused dialogue,
        // which is a different node kind entirely and never reaches this
        // arm anyway). `Iterator::all` on the now-empty remainder is
        // vacuously `true`, so an untagged cue is unaffected.
        N::CUE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::CUE_NAME && children.all(|c| c.kind() == N::TAG))
                .then_some((ElementKind::Cue, first))
        }
        // A compact cue (`@NAME: dialogue`, §8b.9) — issue #2079, RULED
        // 2026-08-06 "Compact cue desugars to cue + content line": it
        // matches its pattern against the name segment only, **exactly as
        // if it were a block cue's line** — same `ElementKind::Cue`, same
        // `CUE_NAME` sub-node offered to matching. Structurally identical
        // to the `CUE` arm above, except this one deliberately does NOT
        // require `children.next().is_none()`: `COMPACT_CUE` has exactly
        // two children by construction (`CUE_NAME`, then the fused
        // dialogue `CONTENT_LINE` — `brink-syntax-native`'s `cue_line`), so
        // requiring a lone child would make every compact cue unclaimable.
        // Literalness still applies to the name segment (an interpolated
        // or otherwise non-`CUE_NAME`-shaped first child still declines);
        // the fused dialogue keeps full markup/interpolation rights and is
        // never inspected here — `try_claim` desugars it separately into
        // an ordinary content line inside the attached run.
        N::COMPACT_CUE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::CUE_NAME).then_some((ElementKind::Cue, first))
        }
        // Issue #2350: same widening as the `CUE` arm above — a trailing
        // run of `TAG` children on a parenthetical's delivery no longer
        // disqualifies it.
        N::PARENTHETICAL => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::TEXT && children.all(|c| c.kind() == N::TAG))
                .then_some((ElementKind::Parenthetical, first))
        }
        _ => None,
    }
}
