//! Issue #1507 — wiring `brink-ide` hover/go-to-def to the D2 UFCS
//! resolution side table (issue #1482, LIR-lowering wiring in #1506):
//! `brink-analyzer`'s `ufcs` pass's verdict for a `recv.verb(args)` call
//! site, memoized project-wide by `brink-db`'s `ufcs_resolution_query` and
//! already read by LIR lowering. This reads the *same* memo
//! ([`brink_db::ProjectDb::ufcs_verdict`]) rather than re-running the
//! analyzer's `ufcs` pass a second time — the D2 ruling justified the side
//! table partly on exactly this IDE payoff.
//!
//! Issue #1539 extended this module's reach beyond hover/go-to-def: the same
//! narrow method-segment matching ([`ufcs_call_at_offset`] /
//! [`ufcs_call_at_path_range`]) and the same `db`-identity-space target
//! resolution ([`ufcs_goto_definition_target`]) are the only correct way to
//! answer "what does `.verb(...)` mean" for `find_references`/`rename` and
//! `brink ide def --at` too — those three surfaces used to key off
//! `ResolutionMap`/`find_def_at_offset` alone, which (like the pre-#1507
//! hover) resolves a UFCS call site to the *receiver*. `rename` additionally
//! needs the reverse direction — given a target `DefinitionId`, every UFCS
//! call site that resolves to it — via `ProjectDb::ufcs_call_sites_for_target`
//! and [`ufcs_method_range_at_path`].
//!
//! ## Why this is a narrow, method-segment-only override
//!
//! For a UFCS-shaped call, `brink-analyzer`'s `resolve::resolve_function`
//! records exactly one resolved reference spanning the *whole* `recv.verb`
//! path, targeting the receiver's own `DefinitionId` (its own UFCS-fallback
//! doc: "the resolved target is the head value itself, and the trailing
//! segment is carried structurally by the HIR `Path`") — the verdict
//! answering "what does `.verb(...)` actually mean" lives only in the D2
//! side table, not in `ResolutionMap`. Left alone, hovering *anywhere*
//! across `recv.verb` (including `verb` itself) falls through
//! `navigation::find_def_at_offset` to the receiver's own hover —
//! informative for `recv`, wrong for `verb`. [`ufcs_call_at_offset`] narrows
//! the override to exactly the method segment's own range, so hovering
//! `recv` is untouched and still shows the receiver.
//!
//! ## Why a `FreeFnDesugar`/`FreeFnAutoRef` target resolves through `db`, not `analysis`
//!
//! A [`brink_ir::lir::UfcsVerdict::FreeFnDesugar`] or
//! [`brink_ir::lir::UfcsVerdict::FreeFnAutoRef`]'s `DefinitionId` is
//! produced by `brink-analyzer`'s `ufcs` pass running over `brink-db`'s own
//! module-qualified project index (`resolutions_index_query`, which
//! `ProjectDb::ufcs_verdict` reads through). [`crate::hover::hover`] accepts
//! an arbitrary caller-supplied `brink_analyzer::AnalysisResult`, which may
//! still be module-*blind* if the caller built it via
//! `brink_analyzer::analyze_with_options` — an intentionally module-blind
//! "whole-project convenience path" (see that function's own doc). For a
//! native `.brink` file, which always carries a real `story::<stem>` module
//! identity (`brink-db`'s `modules.rs`, "path on disk = path in language"),
//! a module-blind `AnalysisResult` and the db's own index mint *different*
//! `DefinitionId`s for the same declaration. So the target is looked up in
//! [`ProjectDb::resolutions_index`] here — the same identity space
//! `ufcs_verdict`'s own `target` came from — never in a caller's
//! `AnalysisResult`, which would silently miss for exactly this reason.
//!
//! ## What each verdict means for hover/go-to-def
//!
//! - [`brink_ir::lir::UfcsVerdict::FieldCall`] — the call dispatches through
//!   a function-typed field on the receiver's type. No `DefinitionId`: a
//!   struct field is not an index symbol (`brink-analyzer`'s `ShapeInfo`
//!   carries no source location per field), so go-to-def has nowhere to
//!   jump; hover still names the field (its text is carried structurally by
//!   the call site itself, per the verdict's own doc).
//! - [`brink_ir::lir::UfcsVerdict::FreeFnDesugar`] carries the resolved free
//!   function's `DefinitionId` directly — go-to-def jumps there like any
//!   other resolved reference.
//! - [`brink_ir::lir::UfcsVerdict::FreeFnAutoRef`] (D5 auto-ref, issue
//!   #1462) is the same free-function-target case as `FreeFnDesugar` for
//!   hover/go-to-def purposes — the only difference is the callee's first
//!   parameter is declared `ref`, so hover notes the by-reference dispatch
//!   and go-to-def jumps to the same `target` the same way.
//! - [`brink_ir::lir::UfcsVerdict::PreludeDesugar`] names a VM-native T1b
//!   stdlib/builtin verb with **no** `DefinitionId` at all (the analyzer's
//!   own doc: "there is no index symbol to point at") — go-to-def handles
//!   this arm explicitly rather than unwrapping a `DefinitionId` that
//!   doesn't exist; hover instead reuses the same stdlib/builtin text an
//!   ordinary bare call of that name already shows.

use brink_db::ProjectDb;
use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::lir::UfcsVerdict;
use brink_ir::{Expr, FileId, HirFile, Name};
use rowan::{TextRange, TextSize};

use crate::hover::HoverInfo;
use crate::navigation::LocationResult;
use crate::{builtin_hover_text, stdlib_hover_text};

/// A UFCS-shaped call site (`recv.verb(args)`) located by its *method*
/// segment — see the module doc for why only that segment triggers the
/// override.
struct UfcsCallSite {
    /// The whole `recv.verb` path range — the key the D2 side table (and
    /// its `brink_ir::lir::UfcsLookup` mirror) is keyed against.
    path_range: TextRange,
    /// The called method's own segment: its text is the field/free-fn/
    /// prelude name, its range is the narrow hover/go-to-def span.
    method: Name,
    /// Every segment before `method`, dot-joined exactly as written (`a.b`
    /// for `a.b.verb()`).
    receiver_text: String,
    /// The FIRST receiver segment's own range (`a`'s own span for
    /// `a.b.verb()`) — issue #1550's narrow-span need: `resolve`'s
    /// UFCS-shaped-callee fallback records the receiver's `ResolvedRef`
    /// against the *whole* `recv.verb` path (mirroring the D2 side table's
    /// own key), so a plain-reference rename of just the receiver must
    /// narrow that whole-path range down to this span itself, or it
    /// silently corrupts the trailing method segment.
    receiver_head_range: TextRange,
}

/// Visit every UFCS-shaped call site (`recv.verb(args)` — at least one
/// receiver segment before the method) in `hir`, stopping at the first one
/// `matches` accepts. Shared by [`ufcs_call_at_offset`] (matches by cursor
/// offset, for hover/go-to-def/`def --at`) and [`ufcs_call_at_path_range`]
/// (matches by the whole path's own range, for consumers that already hold
/// a `(file, range)` key from [`ProjectDb::ufcs_verdict`]/
/// `ProjectDb::ufcs_call_sites_for_target` and need the call site's narrow
/// method-only span back out).
fn find_ufcs_call(
    hir: &HirFile,
    matches: impl Fn(TextRange, &Name) -> bool,
) -> Option<UfcsCallSite> {
    struct Finder<'a> {
        matches: &'a dyn Fn(TextRange, &Name) -> bool,
        found: Option<UfcsCallSite>,
    }
    impl HirVisitor for Finder<'_> {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, expr: &Expr) {
            if self.found.is_some() {
                return;
            }
            let Expr::Call(path, _) = expr else {
                return;
            };
            let Some((method, receiver_segs)) = path.segments.split_last() else {
                return;
            };
            if receiver_segs.is_empty() {
                // A bare `name(args)` — never UFCS (mirrors
                // `brink_analyzer::ufcs::UfcsVisitor::resolve_call`'s own
                // early return for the same shape).
                return;
            }
            if !(self.matches)(path.range, method) {
                return;
            }
            // Non-empty per the `receiver_segs.is_empty()` guard above.
            let Some(receiver_head) = receiver_segs.first() else {
                return;
            };
            let receiver_text = receiver_segs
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".");
            self.found = Some(UfcsCallSite {
                path_range: path.range,
                method: method.clone(),
                receiver_text,
                receiver_head_range: receiver_head.range,
            });
        }
    }
    let mut finder = Finder {
        matches: &matches,
        found: None,
    };
    visit::visit(hir, &mut finder);
    finder.found
}

/// Find the UFCS call site whose method segment contains `offset`, if any.
fn ufcs_call_at_offset(hir: &HirFile, offset: TextSize) -> Option<UfcsCallSite> {
    find_ufcs_call(hir, |_path_range, method| {
        method.range.contains(offset) || method.range.start() == offset
    })
}

/// Find the UFCS call site whose whole `recv.verb` path spans exactly
/// `path_range` — the reverse direction of [`ufcs_call_at_offset`], for
/// find-references/rename (issue #1539): those already know a call site's
/// `(file, path_range)` key (from `ProjectDb::ufcs_call_sites_for_target`)
/// and need its method-only span to reference/rewrite, not a cursor offset.
fn ufcs_call_at_path_range(hir: &HirFile, path_range: TextRange) -> Option<UfcsCallSite> {
    find_ufcs_call(hir, move |range, _method| range == path_range)
}

/// The UFCS call site and its recorded verdict at `offset`, read from the
/// memoized [`ProjectDb::ufcs_verdict`] rather than re-running the
/// analyzer's `ufcs` pass. `None` when `offset` isn't on a UFCS call's
/// method segment, or the pass recorded no verdict there.
fn ufcs_call_and_verdict(
    db: &ProjectDb,
    hir: &HirFile,
    file_id: FileId,
    offset: TextSize,
) -> Option<(UfcsCallSite, UfcsVerdict)> {
    let call = ufcs_call_at_offset(hir, offset)?;
    let verdict = db.ufcs_verdict(file_id, call.path_range)?.clone();
    Some((call, verdict))
}

/// The method-segment range of the UFCS call site at `path_range` in `hir`,
/// if any — find-references/rename's narrow-span counterpart to
/// [`ufcs_call_at_path_range`], for consumers that only need the span, not
/// the receiver text.
#[must_use]
pub fn ufcs_method_range_at_path(hir: &HirFile, path_range: TextRange) -> Option<TextRange> {
    ufcs_call_at_path_range(hir, path_range).map(|call| call.method.range)
}

/// The FIRST receiver segment's own range (`a`'s own span for
/// `a.b.verb()`) of the UFCS call site at `path_range` in `hir`, if any.
///
/// Issue #1550 (mirror of #1539, from the receiver side): `resolve`'s
/// UFCS-shaped-callee fallback records the receiver's `ResolvedRef` against
/// the *whole* `recv.verb` path (`path_range` here — the same key
/// [`ufcs_method_range_at_path`] takes), targeting the receiver's own
/// `DefinitionId`. A plain-reference rename of just the receiver that
/// blindly rewrites that whole-path range therefore corrupts the trailing
/// method segment (`g.greet(3)` collapsing to `newname(3)` instead of
/// `newname.greet(3)`). Callers that find a `ResolvedRef` whose range
/// equals a UFCS call site's `path_range` should narrow to this span
/// instead of using the `ResolvedRef`'s range directly.
#[must_use]
pub fn ufcs_receiver_head_range_at_path(hir: &HirFile, path_range: TextRange) -> Option<TextRange> {
    ufcs_call_at_path_range(hir, path_range).map(|call| call.receiver_head_range)
}

/// The method-segment range of the UFCS call site at `offset` in `hir`, if
/// any — `prepare_rename`'s narrow-span need (issue #1539): the cursor's
/// own reference range under the cursor, not the resolved target's
/// declaration range (mirrors how a plain reference's own range, rather
/// than its target's, is what `prepare_rename` returns elsewhere).
#[must_use]
pub fn ufcs_method_range_at_offset(hir: &HirFile, offset: TextSize) -> Option<TextRange> {
    ufcs_call_at_offset(hir, offset).map(|call| call.method.range)
}

/// Hover content for the UFCS call site at `offset`, if any (issue #1507 —
/// the D2 side table's IDE hover payoff). `None` when `offset` isn't on a
/// UFCS call's method segment, or the pass never resolved a verdict there.
///
/// `project_files` mirrors `hover::hover`'s own parameter — it's only
/// consulted for the "Defined in" note on a [`UfcsVerdict::FreeFnDesugar`].
#[must_use]
pub fn ufcs_hover(
    db: &ProjectDb,
    hir: &HirFile,
    file_id: FileId,
    offset: TextSize,
    project_files: &[(FileId, String, String)],
) -> Option<HoverInfo> {
    let (call, verdict) = ufcs_call_and_verdict(db, hir, file_id, offset)?;
    let content = match verdict {
        UfcsVerdict::FieldCall => format!(
            "**field call** `{recv}.{field}(…)`\n\nDispatches through the receiver's `{field}` \
             field value (a function-typed field wins over a free function of the same name, \
             per D1).",
            recv = call.receiver_text,
            field = call.method.text,
        ),
        UfcsVerdict::FreeFnDesugar { target } => {
            // See the module doc's "Why a `FreeFnDesugar`/`FreeFnAutoRef`
            // target resolves through `db`, not `analysis`" section:
            // `target` lives in `db.resolutions_index()`'s identity space,
            // not a caller's separately-computed `AnalysisResult`.
            let info = db.resolutions_index().index.symbols.get(&target).cloned();
            let name = info
                .as_ref()
                .map_or(call.method.text.as_str(), |i| i.name.as_str());
            let file_note = info
                .as_ref()
                .and_then(|i| project_files.iter().find(|(fid, _, _)| *fid == i.file))
                .map_or(String::new(), |(_, p, _)| format!("\n\n*Defined in `{p}`*"));
            format!(
                "**free function** `{name}({recv}, …)`\n\nDesugared from `{recv}.{name}(…)` — \
                 resolves to the free function `{name}` in ordinary lexical scope (D4).{file_note}",
                recv = call.receiver_text,
            )
        }
        UfcsVerdict::FreeFnAutoRef { target } => {
            // Same target identity space as `FreeFnDesugar` above (see the
            // module doc) — only the description differs, to surface the D5
            // by-reference dispatch.
            let info = db.resolutions_index().index.symbols.get(&target).cloned();
            let name = info
                .as_ref()
                .map_or(call.method.text.as_str(), |i| i.name.as_str());
            let file_note = info
                .as_ref()
                .and_then(|i| project_files.iter().find(|(fid, _, _)| *fid == i.file))
                .map_or(String::new(), |(_, p, _)| format!("\n\n*Defined in `{p}`*"));
            format!(
                "**free function (by ref)** `{name}(ref {recv}, …)`\n\nDesugared from \
                 `{recv}.{name}(…)` — resolves to the free function `{name}` in ordinary lexical \
                 scope (D4), whose first parameter is declared `ref`, so `{recv}` is passed by \
                 reference (D5 auto-ref).{file_note}",
                recv = call.receiver_text,
            )
        }
        UfcsVerdict::PreludeDesugar { name } => {
            let base = stdlib_hover_text(&name)
                .or_else(|| builtin_hover_text(&name))
                .unwrap_or_else(|| format!("**brink stdlib** `{name}`"));
            format!(
                "{base}\n\n*(desugared from `{recv}.{name}(…)` — UFCS method-call syntax)*",
                recv = call.receiver_text,
            )
        }
    };
    Some(HoverInfo {
        content,
        range: Some(call.method.range),
    })
}

/// The `DefinitionId` a UFCS call site's go-to-def should jump to.
///
/// Doubly-`Option`al on purpose — see [`ufcs_goto_definition`]'s doc for the
/// outer/inner contract, which this shares. Factored out (issue #1539) so
/// every consumer that needs the raw target id rather than a resolved
/// [`LocationResult`] — `find_references`, `rename`, `brink ide def --at` —
/// reuses this exact method-segment matching instead of re-deriving it.
#[must_use]
pub fn ufcs_goto_definition_target(
    db: &ProjectDb,
    hir: &HirFile,
    file_id: FileId,
    offset: TextSize,
) -> Option<Option<DefinitionId>> {
    let (_call, verdict) = ufcs_call_and_verdict(db, hir, file_id, offset)?;
    let target = match verdict {
        UfcsVerdict::FreeFnDesugar { target } | UfcsVerdict::FreeFnAutoRef { target } => {
            Some(target)
        }
        // A struct field has no `DefinitionId` of its own, and a prelude
        // verb is VM-native — neither has anywhere for go-to-def to jump.
        // Explicit arms rather than a `_ => None` catch-all, so a third
        // verdict kind added later has to be considered here rather than
        // silently falling into this case.
        UfcsVerdict::FieldCall | UfcsVerdict::PreludeDesugar { .. } => None,
    };
    Some(target)
}

/// The definition location a UFCS call site's go-to-def should jump to.
///
/// Doubly-`Option`al on purpose, so the caller can tell "not applicable"
/// from "applicable, but nowhere to jump": the **outer** `None` means
/// `offset` isn't on a UFCS call's method segment at all (or the pass
/// recorded no verdict there) — the caller's generic
/// `navigation::find_def_at_offset` fallback is appropriate exactly then,
/// and only then. The **inner** `None` means `offset` *is* on such a call,
/// but the verdict has no `DefinitionId` to jump to (a field call — a
/// struct field isn't its own index symbol — or a prelude intrinsic, which
/// is VM-native) — the caller must stop here rather than falling through,
/// or it would recreate the exact bug this module exists to fix (jumping to
/// the receiver's own declaration instead).
#[must_use]
pub fn ufcs_goto_definition(
    db: &ProjectDb,
    hir: &HirFile,
    file_id: FileId,
    offset: TextSize,
) -> Option<Option<LocationResult>> {
    let target = ufcs_goto_definition_target(db, hir, file_id, offset)?;
    // See the module doc's "Why a `FreeFnDesugar`/`FreeFnAutoRef` target
    // resolves through `db`, not `analysis`" section.
    let loc = target.and_then(|target| {
        db.resolutions_index()
            .index
            .symbols
            .get(&target)
            .map(|info| LocationResult {
                file: info.file,
                range: info.range,
            })
    });
    Some(loc)
}
