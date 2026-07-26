//! Issue #1507 — wiring `brink-ide` hover/go-to-def to the D2 UFCS
//! resolution side table (issue #1482, LIR-lowering wiring in #1506):
//! `brink-analyzer`'s `ufcs` pass's verdict for a `recv.verb(args)` call
//! site, memoized project-wide by `brink-db`'s `ufcs_resolution_query` and
//! already read by LIR lowering. This reads the *same* memo
//! ([`brink_db::ProjectDb::ufcs_verdict`]) rather than re-running the
//! analyzer's `ufcs` pass a second time — the D2 ruling justified the side
//! table partly on exactly this IDE payoff.
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
//! ## Why a `FreeFnDesugar` target resolves through `db`, not `analysis`
//!
//! A [`brink_ir::lir::UfcsVerdict::FreeFnDesugar`]'s `DefinitionId` is
//! produced by `brink-analyzer`'s `ufcs` pass running over `brink-db`'s own
//! module-qualified project index (`resolutions_index_query`, which
//! `ProjectDb::ufcs_verdict` reads through). Every caller-supplied
//! `brink_analyzer::AnalysisResult` (e.g. `IdeSession`'s own cached
//! analysis, or the LSP's `analysis_loop`) is instead built by
//! `brink_analyzer::analyze_with_options` — an intentionally
//! module-*blind* "whole-project convenience path" (see that function's own
//! doc). For a native `.brink` file, which always carries a real
//! `story::<stem>` module identity (`brink-db`'s `modules.rs`, "path on
//! disk = path in language"), those two computations mint *different*
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
//! - [`brink_ir::lir::UfcsVerdict::PreludeDesugar`] names a VM-native T1b
//!   stdlib/builtin verb with **no** `DefinitionId` at all (the analyzer's
//!   own doc: "there is no index symbol to point at") — go-to-def handles
//!   this arm explicitly rather than unwrapping a `DefinitionId` that
//!   doesn't exist; hover instead reuses the same stdlib/builtin text an
//!   ordinary bare call of that name already shows.

use brink_db::ProjectDb;
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
}

/// Find the UFCS call site whose method segment contains `offset`, if any.
fn ufcs_call_at_offset(hir: &HirFile, offset: TextSize) -> Option<UfcsCallSite> {
    struct Finder {
        offset: TextSize,
        found: Option<UfcsCallSite>,
    }
    impl HirVisitor for Finder {
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
            if !(method.range.contains(self.offset) || method.range.start() == self.offset) {
                return;
            }
            let receiver_text = receiver_segs
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(".");
            self.found = Some(UfcsCallSite {
                path_range: path.range,
                method: method.clone(),
                receiver_text,
            });
        }
    }
    let mut finder = Finder {
        offset,
        found: None,
    };
    visit::visit(hir, &mut finder);
    finder.found
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
            "**field call** `{recv}.{field}(…)`\n\nField access wins over a free function of the \
             same name (D1): dispatches through the receiver's `{field}` field value.",
            recv = call.receiver_text,
            field = call.method.text,
        ),
        UfcsVerdict::FreeFnDesugar { target } => {
            // See the module doc's "Why a `FreeFnDesugar` target resolves
            // through `db`, not `analysis`" section: `target` lives in
            // `db.resolutions_index()`'s identity space, not a caller's
            // separately-computed `AnalysisResult`.
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
    let (_call, verdict) = ufcs_call_and_verdict(db, hir, file_id, offset)?;
    let loc = match verdict {
        // See the module doc's "Why a `FreeFnDesugar` target resolves
        // through `db`, not `analysis`" section.
        UfcsVerdict::FreeFnDesugar { target } => db
            .resolutions_index()
            .index
            .symbols
            .get(&target)
            .map(|info| LocationResult {
                file: info.file,
                range: info.range,
            }),
        // A struct field has no `DefinitionId` of its own, and a prelude
        // verb is VM-native — neither has anywhere for go-to-def to jump.
        // Explicit arms rather than a `_ => None` catch-all, so a third
        // verdict kind added later has to be considered here rather than
        // silently falling into this case.
        UfcsVerdict::FieldCall | UfcsVerdict::PreludeDesugar { .. } => None,
    };
    Some(loc)
}
