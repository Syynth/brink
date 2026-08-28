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
//! Issue #1560 pushed this module's reach past UFCS entirely: the plain
//! (non-call) dotted-field-access fallback (`resolve::lookup_variable` step
//! 11, `resolve.rs:474-503`) records the SAME whole-path `ResolvedRef` range
//! the UFCS-receiver fallback (#1550, above) does, for an ordinary `p.x.y`
//! reference with no call involved at all. [`find_field_access_ref`] /
//! [`field_access_head_range_at_path`] are the non-UFCS counterpart to
//! [`find_ufcs_call`] / [`ufcs_receiver_head_range_at_path`] — same
//! narrowing idea, applied to an `Expr::Path` that is never a UFCS call
//! site's callee. As of #1560 this module's name undersells its scope: it
//! now hosts every "a `ResolvedRef` covers more than its target's own
//! declaration" narrowing `rename`/`find_references` need, UFCS-shaped or
//! not.
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
use brink_ir::{DivertPath, DivertTarget, Expr, FileId, HirFile, Name, Path, Stmt, SymbolKind};
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
    // `visit_with_decl_initializers`, not `visit` (issue #1571): a UFCS call
    // can be written in a `VAR`/`CONST` initializer (`VAR n = p.scaled(2)`),
    // which `symbols::project_manifest` walks and records a reference from,
    // but which the block-tree-only `visit` never reaches.
    visit::visit_with_decl_initializers(hir, &mut finder);
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

/// A plain (non-call) dotted-field-access reference site (`p.x.y`) — the
/// non-UFCS-call mirror of [`UfcsCallSite`], for issue #1560.
struct FieldAccessRefSite {
    /// The FIRST segment's own range (`p`'s own span for `p.x.y`).
    head_range: TextRange,
}

/// Find the plain (non-call) `Expr::Path` in `hir` whose whole range is
/// exactly `path_range` and which has more than one segment — the shape
/// `resolve::lookup_variable`'s dotted-field-access fallback (step 11 in
/// `brink-analyzer/src/resolve.rs`) records a whole-path `ResolvedRef`
/// against. A single-segment path never reaches that fallback (steps 1-2
/// already check the *whole* path for locals/globals first), and an
/// `Expr::Call`'s callee path is [`ufcs_call_at_path_range`]'s shape, not
/// this one — the two are structurally distinct HIR nodes, so there is no
/// overlap between the two narrowing functions' matches.
fn find_field_access_ref(hir: &HirFile, path_range: TextRange) -> Option<FieldAccessRefSite> {
    struct Finder {
        path_range: TextRange,
        found: Option<FieldAccessRefSite>,
    }
    impl HirVisitor for Finder {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, expr: &Expr) {
            if self.found.is_some() {
                return;
            }
            let Expr::Path(path) = expr else {
                return;
            };
            if path.range != self.path_range {
                return;
            }
            let Some((head, rest)) = path.segments.split_first() else {
                return;
            };
            if rest.is_empty() {
                // A bare single-segment path — never the dotted-field-access
                // fallback shape (see this function's own doc).
                return;
            }
            self.found = Some(FieldAccessRefSite {
                head_range: head.range,
            });
        }
    }
    let mut finder = Finder {
        path_range,
        found: None,
    };
    // See `find_ufcs_call` for why this is the initializer-aware walk: a
    // dotted field access is just as legal in `VAR n = p.x.y` as in a knot
    // body, and #1571's whole point is that the block-tree-only `visit`
    // never reached the former.
    visit::visit_with_decl_initializers(hir, &mut finder);
    finder.found
}

/// The FIRST segment's own range (`p`'s own span for `p.x.y`) of the plain
/// (non-UFCS-call) dotted-field-access reference at `path_range` in `hir`,
/// if any — issue #1560, the non-UFCS-call mirror of
/// [`ufcs_receiver_head_range_at_path`].
///
/// `resolve::lookup_variable`'s dotted-field-access fallback (step 11,
/// `resolve.rs:474-503`) records the SAME whole-path `ResolvedRef` shape the
/// UFCS-callee fallback does — for a plain reference like `p.x.y` (not a
/// call), the resolved target is the head variable/constant/local itself,
/// and the trailing segments are field names carried structurally by the
/// HIR `Path`, not by resolution. A caller that finds a `ResolvedRef` whose
/// range equals a plain field-access reference site's whole-path range must
/// narrow to this span instead of using the `ResolvedRef`'s range directly,
/// or a rename of the head variable collapses `p.x.y` into `newname`,
/// silently dropping `.x.y`.
///
/// `target_kind` must be the `SymbolKind` of the `ResolvedRef`'s own target
/// — narrowing only ever applies when it is `Variable`, `Constant`,
/// `Param`, or `Temp`, the only kinds the analyzer's fallback can resolve a
/// *multi*-segment path to (mirroring `resolve::resolve_function`'s own
/// UFCS-callee fallback, which applies the identical restriction to its
/// receiver lookup). A legitimate whole-path reference to some other kind
/// (e.g. a qualified `knot.stitch` visit-count reference, step 8) has the
/// same whole-path `ResolvedRef` shape but is not field access on a value —
/// checking `target_kind` here, rather than trusting every caller to gate
/// correctly, keeps that case from being wrongly narrowed too.
#[must_use]
pub fn field_access_head_range_at_path(
    hir: &HirFile,
    path_range: TextRange,
    target_kind: SymbolKind,
) -> Option<TextRange> {
    if !matches!(
        target_kind,
        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Param | SymbolKind::Temp
    ) {
        return None;
    }
    find_field_access_ref(hir, path_range).map(|site| site.head_range)
}

/// Find the multi-segment path in `hir` whose whole range is exactly
/// `path_range` and return its LAST segment's own range.
///
/// Unlike [`find_field_access_ref`] (which only has to consider
/// `Expr::Path`, the single shape `lookup_variable`'s step-11 fallback can
/// fire on), a *qualified* reference to a stitch / list item / label is
/// written in every path-bearing HIR position there is, and
/// `symbols::project::Projector` records a whole-path `UnresolvedRef` from
/// each of them:
///
/// - `Stmt::Divert` / `Stmt::TunnelCall` / `Stmt::ThreadStart` — the
///   `-> hub.market`, `-> hub.market ->`, `<- hub.market` forms
///   (`Projector::walk_divert_target`);
/// - `Expr::DivertTarget` — a divert target used as a *value*
///   (`~ t = -> hub.market`);
/// - `Expr::ListLiteral` — each `(Colors.Red, Colors.Green)` member path;
/// - `Expr::Path` — a plain value-position reference (`~ y = hub.market`,
///   `{Colors.Red}`).
///
/// A call's callee path (`Expr::Call`), a function literal's target
/// (`Expr::FnLiteral`, both `RefKind::Function`) and a struct literal's
/// shape (`Expr::StructLiteral`, `RefKind::Struct`) are deliberately
/// absent: `resolve`'s `resolve_function` has no stitch / list-item /
/// label lookup at all, and `resolve_struct_ref` resolves only against
/// `SymbolKind::Struct`, so neither reference position can ever target one
/// of the kinds [`qualified_tail_range_at_path`] gates on.
fn find_qualified_tail(hir: &HirFile, path_range: TextRange) -> Option<TextRange> {
    struct Finder {
        path_range: TextRange,
        found: Option<TextRange>,
    }
    impl Finder {
        fn consider(&mut self, path: &Path) {
            if self.found.is_some() || path.range != self.path_range {
                return;
            }
            let Some((tail, leading)) = path.segments.split_last() else {
                return;
            };
            if leading.is_empty() {
                // A bare single-segment path — its whole range already *is*
                // the target's own segment, so there is nothing to narrow.
                return;
            }
            self.found = Some(tail.range);
        }
        fn consider_target(&mut self, target: &DivertTarget) {
            if let DivertPath::Path(p) = &target.path {
                self.consider(p);
            }
        }
    }
    impl HirVisitor for Finder {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::Divert(d) => self.consider_target(&d.target),
                Stmt::ThreadStart(t) => self.consider_target(&t.target),
                Stmt::TunnelCall(t) => {
                    for target in &t.targets {
                        self.consider_target(target);
                    }
                }
                _ => {}
            }
        }
        fn enter_expr(&mut self, expr: &Expr) {
            match expr {
                Expr::Path(p) | Expr::DivertTarget(p) => self.consider(p),
                Expr::ListLiteral(items) => {
                    for p in items {
                        self.consider(p);
                    }
                }
                _ => {}
            }
        }
    }
    let mut finder = Finder {
        path_range,
        found: None,
    };
    visit::visit_with_decl_initializers(hir, &mut finder);
    finder.found
}

/// The LAST segment's own range (`market`'s span for `hub.market`, `Red`'s
/// for `Colors.Red`) of the qualified reference at `path_range` in `hir`, if
/// any — issue #1571, the *tail* mirror of
/// [`field_access_head_range_at_path`].
///
/// The whole-path `ResolvedRef` shape that #1550/#1560 narrowed from the
/// head has a second, equally corrupting reading: when the resolved target
/// is a **stitch** (`resolve::lookup_variable` steps 6–8, `lookup_divert`), a
/// **list item** (step 4) or a **label** (steps 8–10), the segment that
/// actually names it is the path's *last* one, not its first. Rewriting the
/// whole range there collapses `-> hub.market` into `-> newname` and
/// `Colors.Red` into `Crimson`, silently dropping the qualifier — the same
/// silent-corruption class, from the other end.
///
/// `target_kind` must be the `SymbolKind` of the `ResolvedRef`'s own target.
/// The gate is checked here rather than at each call site, and is disjoint
/// by construction from [`field_access_head_range_at_path`]'s
/// `Variable | Constant | Param | Temp`: a reference can be narrowed to the
/// head or to the tail, never to both, because the two kind sets do not
/// intersect.
#[must_use]
pub fn qualified_tail_range_at_path(
    hir: &HirFile,
    path_range: TextRange,
    target_kind: SymbolKind,
) -> Option<TextRange> {
    if !matches!(
        target_kind,
        SymbolKind::Stitch | SymbolKind::ListItem | SymbolKind::Label
    ) {
        return None;
    }
    find_qualified_tail(hir, path_range)
}

/// Narrow a `ResolvedRef`'s range down to the single segment that actually
/// names the resolved symbol, or `None` when the range already spans exactly
/// that segment.
///
/// The one place the three whole-path narrowings are composed, so every
/// consumer of `analysis.resolutions` that rewrites or reports a reference
/// range — `rename`, `find_references`, `prepare_rename` — applies the same
/// set (issue #1571; before it, `prepare_rename` applied none of them and
/// highlighted the whole `p.x.y` / `recv.verb` path on an F2 at its head).
///
/// The three are mutually exclusive: [`ufcs_receiver_head_range_at_path`]
/// matches only an `Expr::Call` callee path, and the other two are gated on
/// disjoint `SymbolKind` sets (see their own docs).
#[must_use]
pub fn narrowed_reference_range(
    hir: &HirFile,
    ref_range: TextRange,
    target_kind: SymbolKind,
) -> Option<TextRange> {
    ufcs_receiver_head_range_at_path(hir, ref_range)
        .or_else(|| field_access_head_range_at_path(hir, ref_range, target_kind))
        .or_else(|| qualified_tail_range_at_path(hir, ref_range, target_kind))
}

/// `true` when `range` is a compiler-*synthesized* reference from
/// natural-notation element dispatch (issue #1838), not real identifier
/// source at all.
///
/// `hir::lower_native::element::try_claim` rewrites a claimed prose line
/// into a call whose `Path`/`Name` range is stamped to the **entire claimed
/// line** — `element.rs`'s own doc: "written at the claimed line … the
/// range a reader clicking the rewritten call should land on" — not to any
/// occurrence of the handler's name in source. [`narrowed_reference_range`]
/// only *narrows* a whole-path range down to a real segment; it has nothing
/// to narrow a claimed line down to, so it correctly returns `None` for one
/// — and a caller that then falls back to the unnarrowed range would
/// rewrite or report the claimed prose line itself as if it were the
/// identifier's own text (the source-corruption bug this function exists to
/// stop).
///
/// Every consumer of `analysis.resolutions` that rewrites or reports a
/// reference range must check this **before** falling back to the
/// unnarrowed range — the same three surfaces [`narrowed_reference_range`]
/// itself serves: `rename`, `prepare_rename`, `find_references`. A
/// synthesized ref still resolves correctly (the call does target the real
/// handler `fn`); it is only unsafe to *rewrite or highlight*, so the
/// caller's answer is to skip it entirely, not merely leave it unnarrowed.
#[must_use]
pub fn is_synthesized_element_ref(hir: &HirFile, range: TextRange) -> bool {
    hir.element_matches.iter().any(|m| m.line == range)
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
        links: Vec::new(),
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
