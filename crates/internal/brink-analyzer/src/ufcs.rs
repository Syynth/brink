//! B3a — UFCS (uniform function call syntax) resolution: `recv.name(args)`
//! (issue #1482; D1–D5 RULED 2026-07-26, `docs/decision-log.md` "UFCS
//! resolution pass designed: type-directed, in the analyzer, five rulings").
//!
//! ## Why this lives in `brink-analyzer`
//!
//! UFCS resolution is **type-directed name resolution**: field-access-wins
//! is unanswerable without the receiver's type, so the verdict cannot be
//! reached in the frontend or in HIR lowering. The native lowering already
//! produces `Expr::Call(Path, args)` for a dotted callee unchanged (see
//! `brink-ir`'s `hir::lower_native::expr::lower_call`) — this module is the
//! pass that decides what that shape *means*.
//!
//! ## The algorithm, per call site
//!
//! For `recv.name(args)` — an `Expr::Call` whose callee `Path` has more than
//! one segment and whose head names a *value* in scope:
//!
//! 1. Infer the receiver's type (`recv` = every segment but the last).
//! 2. The type declares a field `name` → **field access wins**. The field
//!    must be function-typed; the call is a call *through the field's
//!    value* ([`UfcsVerdict::FieldCall`], rows per #872).
//!    **D1**: a matching but non-callable field is a **hard error**
//!    ([`DiagnosticCode::E140`]) — never a fall-through to a free function,
//!    so a call's meaning never hinges on a field's type.
//! 3. Else resolve `name` as a free function in **ordinary lexical scope
//!    only** (D4 — no method sets, no inherent impls: any in-scope free
//!    function is method-callable) — file `use` + the T1b/NS stdlib
//!    prelude (`len`, `push`, `sort_by`, …) — and record the desugar to
//!    `name(recv, args)` ([`UfcsVerdict::FreeFnDesugar`] for an index
//!    symbol, [`UfcsVerdict::PreludeDesugar`] for a prelude verb, which has
//!    no index symbol to point at).
//! 4. Neither → one diagnostic naming **both** attempts
//!    ([`DiagnosticCode::E141`]).
//!
//! **D3**: an unknown receiver type at the resolution point is an error
//! demanding an annotation ([`DiagnosticCode::E142`]), *not* a deferral —
//! there is deliberately no deferral machinery here. The improvement
//! (smarter inference ordering) is tracked separately and is additive.
//!
//! **D5 — auto-ref** (issue #1462, landed on top of this pass): the desugar
//! is by value *unless* the resolved free function's first parameter is
//! declared `ref`. Then the receiver is passed by reference
//! ([`UfcsVerdict::FreeFnAutoRef`]) and the desugar spells the projection
//! explicitly — `party.members.heal(5)` → `heal(ref party.members, 5)` —
//! riding the T1e ref-argument/projection machinery
//! (`brink_ir::lir::lower::expr::lower_call_args`) for a **durable** root,
//! or (**RULED 2026-07-27**, issue #1531) a frame-local read/call/
//! write-back RMW expansion for a **frame-local** root one field deep
//! (`brink_ir::lir::lower::blocks::try_lower_frame_local_auto_ref_stmt`) —
//! never a parallel path for the durable case. A receiver that cannot be
//! written through is refused with [`DiagnosticCode::E143`] rather than
//! silently desugared by value, which would drop the mutation: see
//! [`UfcsVisitor::auto_ref_fault`] for exactly which receivers those are. A
//! non-`ref` first parameter is unaffected — plain by-value desugar, with
//! no lvalue requirement on the receiver.
//!
//! ## Scope fences
//!
//! - Only the final pre-`(` segment gets this treatment; a bare `a.b` (an
//!   `Expr::FieldAccess`, or a dotted `Expr::Path`) is untouched.
//! - Each call in `a.b().c()` resolves independently — this pass keys
//!   verdicts by call-site range, never by chain.
//! - **The ink dialect is untouched by construction.** ink's own
//!   `FunctionCall` lowering always builds a *single-segment* callee path
//!   (`brink-ir`'s `hir::lower::expr::references`), and its computed-callee
//!   `CallExpr` is a structural `E104`. A multi-segment `Expr::Call` path
//!   can therefore only originate in the native frontend, so no dialect
//!   flag is needed to keep this pass off the ink corpus.
//! - The explicit free-call spelling (`name(recv, args)`) is unaffected.
//!
//! ## The side table (D2)
//!
//! The verdict is recorded in a **side table** keyed by node
//! ([`SideTable`]), not written back into the HIR — HIR stays immutable,
//! matching the analyzer's existing "inference results travel beside the
//! tree" posture (`infer::InferenceResult`).
//!
//! The table is published as the seam (`brink_analyzer::ufcs_resolution`)
//! the two ruled consumers read. **LIR lowering is wired** (issue #1506,
//! `brink-db`'s `ufcs_resolution_query` translates this table into
//! `brink-ir`'s own lowering-facing mirror at the query boundary) — it now
//! emits either a call through the field's value or the desugared free
//! call for real. A resolved site LIR lowering cannot find a verdict for
//! (a caller that never ran this pass) still refuses with
//! [`DiagnosticCode::E144`] rather than lowering against the receiver's own
//! id, which would be a silently wrong program — but that is a defensive
//! fallback now, not the unconditional behavior. IDE hover/go-to-def
//! (issue #1507, `brink-ide`'s `ufcs_hover` module) is wired too — it reads
//! the same memoized table (`brink_db::ProjectDb::ufcs_verdict`) to name the
//! real target rather than the receiver the [`ResolutionMap`] records for
//! the callee path.
//!
//! [`SideTable`] is deliberately generic over its payload: it is
//! `(node → verdict)` plumbing, so a second payload kind can ride the same
//! keying and the same lookup without a parallel structure being invented.
//! Issue #1492 did exactly that — `crate::coalesce`'s [`CoalesceTable`]
//! is a `SideTable<CoalesceChain>` carrying `or`-coalescing's recorded
//! operand/result types to the same LIR-lowering consumer, on this keying,
//! with no second mechanism.
//!
//! [`CoalesceTable`]: crate::CoalesceTable
//! [`CoalesceChain`]: crate::CoalesceChain

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, Path as HirPath, ResolutionMap,
    Stitch, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, InferredSig, Ty, assignable};
use crate::resolve::ImportScope;
use crate::structs::{ShapeInfo, declared_shapes};

// ─── The side table (D2) ─────────────────────────────────────────────

/// Identity of one HIR node for side-table purposes: the file it lives in
/// plus its source range.
///
/// `TextRange` has no `Ord` impl (ranges have no single natural total
/// order), so the range travels as a `(start, end)` `u32` pair — the same
/// `range_key` convention `infer`, `strict`, and `structs` each already use
/// for their own range-keyed maps. A range is only unique *within* a file,
/// hence the [`FileId`] half: side-table entries must never be merged
/// across files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeKey {
    /// The file the node was lowered from.
    pub file: FileId,
    /// The node's source range, as `(start, end)`.
    pub range: (u32, u32),
}

impl NodeKey {
    /// The key for a node at `range` in `file`.
    #[must_use]
    pub fn new(file: FileId, range: TextRange) -> Self {
        Self {
            file,
            range: (range.start().into(), range.end().into()),
        }
    }
}

/// A `(node → payload)` side channel: analysis verdicts recorded *beside*
/// the HIR rather than written into it (D2 — the HIR stays immutable).
///
/// Generic over the payload so a second kind of verdict can ride the same
/// plumbing instead of a parallel structure being invented for it. Backed by
/// a `BTreeMap` so iteration order is deterministic (house rule — never
/// iterate a `HashMap` where order affects output); [`Self::iter`] is what a
/// consumer that wants *every* verdict (e.g. an IDE building an overlay)
/// walks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideTable<V> {
    entries: BTreeMap<NodeKey, V>,
}

impl<V> Default for SideTable<V> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<V> SideTable<V> {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `value` for the node at `key`, returning any previous entry.
    pub fn insert(&mut self, key: NodeKey, value: V) -> Option<V> {
        self.entries.insert(key, value)
    }

    /// The payload recorded for the node at `key`, if any.
    #[must_use]
    pub fn get(&self, key: NodeKey) -> Option<&V> {
        self.entries.get(&key)
    }

    /// The payload recorded for the node at `range` in `file`, if any — the
    /// convenience spelling for a consumer holding an HIR node rather than a
    /// pre-built [`NodeKey`].
    #[must_use]
    pub fn at(&self, file: FileId, range: TextRange) -> Option<&V> {
        self.get(NodeKey::new(file, range))
    }

    /// Every recorded entry, in deterministic `(file, range)` order.
    pub fn iter(&self) -> impl Iterator<Item = (NodeKey, &V)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }

    /// How many nodes carry a payload.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no node carries a payload.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What one `recv.name(args)` call site resolved to (D2's "node → resolved
/// target"). Consumed by LIR lowering — which of the two code shapes to
/// emit — and by IDE hover/go-to-def, which needs the *real* target rather
/// than the receiver the [`ResolutionMap`] records for the callee path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UfcsVerdict {
    /// Field access won (step 2): the receiver's type declares a
    /// function-typed field with the called name, so the call is a call
    /// *through that field's value*.
    FieldCall {
        /// The receiver's inferred type.
        receiver: Ty,
        /// The field name — the call's final pre-`(` path segment.
        field: String,
        /// The field's declared type. Always a [`Ty::Fn`] — a
        /// non-callable match is `E140`, never a verdict.
        field_ty: Ty,
    },
    /// **D5 auto-ref** (issue #1462): a free function won (step 3) *and* its
    /// first parameter is declared `ref`, so the call desugars to
    /// `name(ref recv, args)` — the receiver spelled as an explicit T1e
    /// ref-argument/projection, so the callee's writes land in the
    /// receiver's own cell instead of in a copy.
    ///
    /// Only ever recorded for a receiver that can actually be written
    /// through ([`UfcsVisitor::auto_ref_fault`]); anything else is `E143`.
    FreeFnAutoRef {
        /// The receiver's inferred type.
        receiver: Ty,
        /// The free function's name, as written.
        name: String,
        /// The definition the desugared call targets.
        target: DefinitionId,
        /// Issue #1881: statically-checkable argument-type mismatches
        /// between the desugared call `name(recv, args)` and `target`'s
        /// already-known declared param types, computed unconditionally
        /// alongside this verdict — see [`UfcsArgMismatch`]'s own doc.
        /// Reported only by strict mode ([`check_strict`], `E063`).
        arg_mismatches: Vec<UfcsArgMismatch>,
    },
    /// A free function won (step 3): the call desugars to
    /// `name(recv, args)`, by value.
    FreeFnDesugar {
        /// The receiver's inferred type.
        receiver: Ty,
        /// The free function's name, as written.
        name: String,
        /// The definition the desugared call targets.
        target: DefinitionId,
        /// Issue #1881: identical posture to [`Self::FreeFnAutoRef`]'s own
        /// `arg_mismatches` field — the by-value desugar shape.
        arg_mismatches: Vec<UfcsArgMismatch>,
    },
    /// A T1b/NS stdlib prelude name won (step 3, D4's "file `use` + prelude"
    /// candidate set): the call desugars to `name(recv, args)` exactly like
    /// [`Self::FreeFnDesugar`], but the target is a VM-native intrinsic
    /// (`resolve::is_t1b_stdlib_name`/`resolve::is_builtin_function`), not an
    /// index symbol — there is no [`DefinitionId`] to record. `xs.len()`,
    /// `inventory.push(sword)`, `a.sort_by(c)` all land here.
    PreludeDesugar {
        /// The receiver's inferred type.
        receiver: Ty,
        /// The prelude function's name, as written.
        name: String,
        /// Issue #1919: statically-checkable argument-domain mismatches
        /// between the desugared call `name(recv, args)` and the verb's
        /// own container-projected domain (the receiver's element/key/
        /// value type — a prelude verb has no [`DefinitionId`] and so no
        /// declared param list to compare against), computed
        /// unconditionally alongside this verdict — see
        /// [`UfcsVisitor::check_ufcs_prelude_arg_types`]'s own doc.
        /// Reported only by strict mode ([`check_strict`], `E063`), the
        /// same code [`Self::FreeFnDesugar`]'s own `arg_mismatches` uses.
        arg_mismatches: Vec<UfcsArgMismatch>,
    },
}

/// One statically-checkable argument-type mismatch at a UFCS-desugared free
/// function call (`recv.name(args)` → `name(recv, args)`) — issue #1881,
/// the UFCS sibling of `infer::DirectCallArgMismatch` (#1864/PR #1875,
/// direct calls) and `infer::TypedAssignMismatch` (#1877/PR #1899,
/// declaration initializers and assignments). Reported by [`check_strict`]
/// as `E063`, the same code the other two siblings use — no new code minted
/// for this position (docs/t1c-spec.md §8's "existing TM-3 machinery"
/// posture, extended here rather than a parallel checker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UfcsArgMismatch {
    /// The mismatched argument's 0-based position in the **desugared** call
    /// `name(recv, args)` — `0` names the receiver itself (the desugar's
    /// first positional slot); `i` for `i >= 1` names the `(i - 1)`-th
    /// *written* argument. Matches the "receiver counts as the first
    /// argument" convention this call site's own arity-mismatch diagnostic
    /// already uses (see [`UfcsVisitor::try_free_fn_desugar`]).
    pub index: usize,
    /// The desugared target's declared parameter type at `index`.
    pub expected: Ty,
    /// The receiver's (`index == 0`) or written argument's statically
    /// classified type.
    pub found: Ty,
}

/// Every UFCS call site's verdict for one project.
pub type UfcsTable = SideTable<UfcsVerdict>;

/// Translate a [`UfcsTable`] into `brink-ir`'s own lowering-facing mirror
/// (`brink_ir::lir::UfcsLookup`/`UfcsVerdict`) — issue #1506's one
/// conversion point, so the `UfcsVerdict` → `brink_ir::lir::UfcsVerdict`
/// mapping lives in exactly one place rather than once per caller.
/// `brink-ir` sits below this crate in the crate graph (this crate depends
/// on `brink-ir`, never the reverse), so it cannot provide this itself —
/// see `brink_ir::lir::UfcsVerdict`'s own doc. Every LIR-lowering caller
/// shares this: `brink-db`'s `ufcs_resolution_query` (the production path)
/// and [`assemble_analyzer_tables`](crate::assemble_analyzer_tables) — the
/// salsa-free path used by `brink-test-harness`
/// (`corpus::compile_and_explore_from_brink_native`) and any other caller
/// with no salsa layer of its own to memoize the table in.
#[must_use]
pub fn to_lir_lookup(table: &UfcsTable) -> brink_ir::lir::UfcsLookup {
    let entries = table
        .iter()
        .map(|(key, verdict)| {
            let range = TextRange::new(key.range.0.into(), key.range.1.into());
            let mirrored = match verdict {
                UfcsVerdict::FieldCall { .. } => brink_ir::lir::UfcsVerdict::FieldCall,
                UfcsVerdict::FreeFnAutoRef { target, .. } => {
                    brink_ir::lir::UfcsVerdict::FreeFnAutoRef { target: *target }
                }
                UfcsVerdict::FreeFnDesugar { target, .. } => {
                    brink_ir::lir::UfcsVerdict::FreeFnDesugar { target: *target }
                }
                UfcsVerdict::PreludeDesugar { name, .. } => {
                    brink_ir::lir::UfcsVerdict::PreludeDesugar { name: name.clone() }
                }
            };
            (key.file, range, mirrored)
        })
        .collect();
    brink_ir::lir::UfcsLookup::from_entries(entries)
}

// ─── The pass ────────────────────────────────────────────────────────

/// Resolve every UFCS-shaped call in the project, returning the verdict
/// side table plus the diagnostics the four outcomes above produce.
///
/// `inference` supplies the receiver types (the pass is type-directed by
/// construction); `resolutions` identifies which dotted callee paths are
/// UFCS-shaped at all — a path already resolving to a knot/stitch/external
/// is an ordinary qualified call and is left completely alone.
///
/// Callers gate this on [`project_has_ufcs_call`] so a project without a
/// single dotted-callee call never pays for whole-project inference on this
/// pass's account.
#[must_use]
pub fn resolve(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    inference: &InferenceResult,
) -> (UfcsTable, Vec<Diagnostic>) {
    let shapes = declared_shapes(files, index);
    let globals = crate::infer::collect_globals(files, index, None);
    let mut table = UfcsTable::new();
    let mut diagnostics = Vec::new();

    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
        let mut v = UfcsVisitor {
            file,
            index,
            scope: &scope,
            shapes: &shapes,
            globals: &globals,
            bodies: &inference.bodies,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            current_knot_name: None,
            knot_body: None,
            stitch_body: None,
            table: &mut table,
            diagnostics: &mut diagnostics,
        };
        visit::visit(hir, &mut v);
    }

    (table, diagnostics)
}

/// The **strict-mode-only** diagnostics that fall out of the verdict table:
///
/// - Issue #1540 (second symptom): a typed check keyed on an intrinsic's
///   receiver must see the UFCS spelling of that intrinsic too.
///
///   `infer::body::infer_call` deliberately branches away for a
///   multi-segment callee *before* `infer_intrinsic` runs (a UFCS receiver
///   is not the thing being called, so classifying it as a
///   call-through-a-value would be a false `E066` on every legal method
///   call — see that function's own note). Issue #1909 later gave that
///   branch a result type for the *free-function* desugar
///   (`infer::body::InferPass::infer_ufcs_free_fn_result`), but
///   deliberately not for a prelude verb, precisely because routing one
///   through `infer_intrinsic` would record the `array_remove_calls` fact
///   below a second time and double-report this very `E149`. The
///   consequence stands: `arr.remove(0)` records none of the facts
///   `remove(arr, 0)` records, so
///   every intrinsic-receiver diagnostic silently stopped at the free-call
///   spelling. This pass is where the UFCS spelling gets them back: the
///   verdict table already carries the receiver's resolved `Ty` next to the
///   verb's name, which is exactly the `(receiver type, verb)` pair those
///   checks key on — no second inference, and no `TypePolicy` threaded into
///   [`resolve`] (which stays policy-independent, as LIR lowering and the
///   IDE need it to be).
///
/// - Issue #1881: a `FreeFnDesugar`/`FreeFnAutoRef` verdict's own
///   `arg_mismatches` (computed unconditionally alongside the verdict by
///   [`UfcsVisitor::try_free_fn_desugar`]) — the UFCS sibling of
///   `strict::check_direct_call_args` (#1864/PR #1875) and
///   `strict::check_typed_assign_mismatches` (#1877/PR #1899): a UFCS
///   receiver resolves to a *value*, so `InferenceResult::signatures` has
///   no entry for it the way a direct call's callee does, which is exactly
///   why this class of mismatch couldn't be checked by extending either of
///   those two passes — the resolved free-function *target*'s signature is
///   only ever available here, where this pass has already resolved it.
///
/// - Issue #1919: a `PreludeDesugar` verdict's own `arg_mismatches`
///   ([`UfcsVisitor::check_ufcs_prelude_arg_types`]) — the prelude sibling
///   of the `FreeFnDesugar` bullet above, for the collection verbs whose
///   domain is a plain container projection (`xs.push(v)`, `m.get(k)`, and
///   the rest of that family). `remove`'s array leg stays the hand-written
///   `E149` check just below rather than folding into this fact: the two
///   are disjoint diagnostic families keyed on the same `(receiver, name)`
///   pair, not a double-report risk.
///
/// Strict-mode-only **by convention, not by construction**, exactly like
/// `coalesce::resolve`'s `E066` half: production reaches this only from
/// `strict::check`, after `strict::config_error` has confirmed
/// `types = strict` + `dialect = brink`. A caller that surfaces these
/// without that gate would emit strict-only codes under `types = gradual`.
///
/// Gated on [`project_has_ufcs_call`] internally so a project with no
/// dotted-callee call anywhere pays nothing — the same laziness
/// `whole_project_diagnostics` applies to [`resolve`]'s own diagnostics.
#[must_use]
pub fn check_strict(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    inference: &InferenceResult,
) -> Vec<Diagnostic> {
    if !files.iter().any(|&(_, hir)| project_has_ufcs_call(hir)) {
        return Vec::new();
    }
    // The unconditional `E140`–`E144` half is discarded here: it is already
    // reported by `whole_project_diagnostics`' own call to `resolve`, and
    // double-reporting it under strict would be a regression.
    let (table, _unconditional) = resolve(files, index, resolutions, inference);
    table
        .iter()
        .flat_map(|(key, verdict)| strict_verdict_diagnostics(key, verdict))
        .collect()
}

/// One verdict's strict-mode diagnostics, if it has any.
///
/// `E149` (issue #1540) — `remove` went map-only in issue #1484 with no
/// compatibility shim, so an array receiver means the site wants
/// `remove_at`. The free-call spelling of this exact check lives in
/// `strict::check_array_remove_calls`, reading the fact
/// `infer::body`'s `remove` arm records; the two spellings must agree, so
/// the receiver test here (`Ty::Array`) is deliberately the same one.
///
/// `E063` (issue #1881, widened to `PreludeDesugar` by issue #1919) —
/// every recorded [`UfcsArgMismatch`] on a `FreeFnDesugar`/`FreeFnAutoRef`/
/// `PreludeDesugar` verdict, reported the same way
/// `strict::check_direct_call_args` reports `DirectCallArgMismatch`.
///
/// Every future collection-typed check that keys on `(receiver type, verb)`
/// belongs in this match rather than in a parallel walk — that is the point
/// of routing through the verdict table at all.
fn strict_verdict_diagnostics(key: NodeKey, verdict: &UfcsVerdict) -> Vec<Diagnostic> {
    match verdict {
        UfcsVerdict::PreludeDesugar {
            receiver,
            name,
            arg_mismatches,
        } => {
            let mut out: Vec<Diagnostic> = arg_mismatches
                .iter()
                .map(|mismatch| ufcs_arg_mismatch_diagnostic(key, name, mismatch))
                .collect();
            if let ("remove", Ty::Array(_)) = (name.as_str(), receiver) {
                out.push(Diagnostic {
                    file: key.file,
                    range: TextRange::new(key.range.0.into(), key.range.1.into()),
                    message: DiagnosticCode::E149.title().to_owned(),
                    code: DiagnosticCode::E149,
                });
            }
            out
        }
        UfcsVerdict::FreeFnDesugar {
            name,
            arg_mismatches,
            ..
        }
        | UfcsVerdict::FreeFnAutoRef {
            name,
            arg_mismatches,
            ..
        } => arg_mismatches
            .iter()
            .map(|mismatch| ufcs_arg_mismatch_diagnostic(key, name, mismatch))
            .collect(),
        UfcsVerdict::FieldCall { .. } => Vec::new(),
    }
}

/// One [`UfcsArgMismatch`] as a diagnostic — the message wording matches
/// `strict::check_direct_call_args`'s own `E063` phrasing exactly (the
/// direct-call sibling this parallels), just against the *desugared* call's
/// own argument numbering (`index` `0` is the receiver).
fn ufcs_arg_mismatch_diagnostic(
    key: NodeKey,
    name: &str,
    mismatch: &UfcsArgMismatch,
) -> Diagnostic {
    Diagnostic {
        file: key.file,
        range: TextRange::new(key.range.0.into(), key.range.1.into()),
        message: format!(
            "argument {} of call to `{name}` has type `{}` but its known type expects `{}`",
            mismatch.index + 1,
            mismatch.found.display(),
            mismatch.expected.display(),
        ),
        code: DiagnosticCode::E063,
    }
}

/// Cheap structural scan: does any call in `hir` have a multi-segment
/// callee path? The laziness gate for [`resolve`]'s caller — a project
/// (every ink project, by construction; see the module doc) with no
/// dotted-callee call never triggers whole-project inference on this pass's
/// account, mirroring `whole_project_diagnostics`' own `needs_effects`
/// gate.
#[must_use]
pub fn project_has_ufcs_call(hir: &HirFile) -> bool {
    struct Scan {
        found: bool,
    }
    impl HirVisitor for Scan {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, expr: &Expr) {
            if let Expr::Call(path, _) = expr
                && path.segments.len() > 1
            {
                self.found = true;
            }
        }
    }
    let mut scan = Scan { found: false };
    visit::visit(hir, &mut scan);
    scan.found
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `structs::resolution_index` (a `Path`'s range is only unique
/// within its own file).
fn resolution_index(
    resolutions: &ResolutionMap,
    file: FileId,
) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| ((r.range.start().into(), r.range.end().into()), r.target))
        .collect()
}

/// Walks one file's knot/stitch bodies, tracking the enclosing def's
/// finalized locals so a receiver's head segment can be typed. Structurally
/// a twin of `structs::ConstructionVisitor` — same `enter_knot`/
/// `enter_stitch` locals bookkeeping, for the same reason (`BodyTypes` is
/// keyed by def, `locals` by name).
struct UfcsVisitor<'a> {
    file: FileId,
    index: &'a SymbolIndex,
    scope: &'a ImportScope,
    shapes: &'a BTreeMap<String, ShapeInfo>,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    bodies: &'a BTreeMap<DefinitionId, crate::infer::BodyTypes>,
    /// Every inferable def's finalized signature (issue #1881) — the UFCS
    /// desugar's *target* (a knot/stitch, never the receiver) has its
    /// declared param types here, the same firewall-facing projection a
    /// direct call's `known_sigs` lookup reads. See
    /// [`UfcsVisitor::try_free_fn_desugar`]'s own argument-type check.
    signatures: &'a BTreeMap<DefinitionId, InferredSig>,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    current_knot_name: Option<String>,
    /// The enclosing knot's own `BodyTypes` (issue #1881 widened this from
    /// `locals` alone to the whole `BodyTypes`, so [`Self::current_body`]
    /// can also read back the enclosing def's own recorded
    /// `ufcs_call_args` — see [`Self::current_locals`] for the `locals`
    /// projection every earlier call site still wants).
    knot_body: Option<&'a crate::infer::BodyTypes>,
    stitch_body: Option<&'a crate::infer::BodyTypes>,
    table: &'a mut UfcsTable,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for UfcsVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.current_knot_name = Some(knot.name.text.clone());
        self.knot_body =
            annotations::def_id_for(self.index, self.file, knot.symbol_kind(), &knot.name.text)
                .and_then(|id| self.bodies.get(&id));
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.current_knot_name = None;
        self.knot_body = None;
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        self.stitch_body = self.current_knot_name.as_ref().and_then(|knot_name| {
            let qualified = format!("{knot_name}.{}", stitch.name.text);
            annotations::def_id_for(self.index, self.file, SymbolKind::Stitch, &qualified)
                .and_then(|id| self.bodies.get(&id))
        });
    }

    fn exit_stitch(&mut self, _stitch: &Stitch) {
        self.stitch_body = None;
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::Call(path, args) = expr {
            self.resolve_call(path, args.len());
        }
    }
}

/// The receiver half of one UFCS call site, resolved: everything the two
/// resolution steps and D5's auto-ref gate need to know about `recv` in
/// `recv.name(args)`.
struct Receiver<'a> {
    /// The definition the head segment resolved to (a param/temp/`VAR`/
    /// `CONST` — [`UfcsVisitor::value_receiver_def`]).
    def: DefinitionId,
    /// Every segment before the final pre-`(` one, head first.
    segments: &'a [brink_ir::Name],
    /// The receiver as written (`party.members`), for diagnostics.
    text: String,
    /// The receiver's inferred type ([`UfcsVisitor::receiver_ty`]) — never
    /// `Unknown`/`Conflicted`, which is `E142` one step earlier.
    ty: Ty,
}

impl UfcsVisitor<'_> {
    /// The innermost enclosing def's own `BodyTypes` — a stitch's own body
    /// wins over its enclosing knot's, exactly like [`Self::current_locals`]
    /// already preferred.
    fn current_body(&self) -> Option<&crate::infer::BodyTypes> {
        self.stitch_body.or(self.knot_body)
    }

    fn current_locals(&self) -> Option<&BTreeMap<String, Ty>> {
        self.current_body().map(|b| &b.locals)
    }

    /// The single call-site decision. Returns without touching the table or
    /// the diagnostics for any call that is not UFCS-shaped.
    fn resolve_call(&mut self, path: &HirPath, arg_count: usize) {
        let Some((method, receiver_segs)) = path.segments.split_last() else {
            return;
        };
        if receiver_segs.is_empty() {
            // A bare `name(args)` — ordinary direct call, never UFCS.
            return;
        }
        let Some(head_def) = self.value_receiver_def(path) else {
            // The callee path resolves to a real callable (a
            // module-qualified free call, an ink `knot.stitch()` visit) —
            // an ordinary qualified call, not method-call syntax.
            return;
        };

        let receiver_text = receiver_segs
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");

        let Some(receiver_ty) = self.receiver_ty(head_def, receiver_segs) else {
            // D3: no deferral machinery — demand an annotation.
            self.push(
                path.range,
                DiagnosticCode::E142,
                &format!(
                    "cannot resolve `{receiver_text}.{method}(…)`: the type of `{receiver_text}` \
                     is not known here, so it is undecidable whether `{method}` is one of its \
                     fields — annotate the receiver",
                    method = method.text,
                ),
            );
            return;
        };

        let receiver = Receiver {
            def: head_def,
            segments: receiver_segs,
            text: receiver_text,
            ty: receiver_ty,
        };

        // Step 2 — field access wins outright (D1).
        if self.try_field_call(path, method, &receiver) {
            return;
        }

        // Step 3 — a free function in ordinary lexical scope (D4), by value
        // or auto-ref'd (D5).
        if self.try_free_fn_desugar(path, method, &receiver, arg_count) {
            return;
        }

        // Step 4 — neither; one diagnostic naming both attempts.
        self.push(
            path.range,
            DiagnosticCode::E141,
            &format!(
                "cannot resolve `{receiver_text}.{method}(…)`: `{recv_ty}` declares no field \
                 `{method}`, and no function `{method}` is in scope here",
                method = method.text,
                receiver_text = receiver.text,
                recv_ty = receiver.ty.display(),
            ),
        );
    }

    /// Step 2 (D1). Returns `true` when the receiver's type declares a field
    /// of the called name — the call is settled either way, as a
    /// [`UfcsVerdict::FieldCall`] or as the `E140` hard error, and never
    /// falls through to step 3.
    fn try_field_call(
        &mut self,
        path: &HirPath,
        method: &brink_ir::Name,
        receiver: &Receiver<'_>,
    ) -> bool {
        let receiver_ty = &receiver.ty;
        let receiver_text = &receiver.text;
        let Ty::Struct(shape_name) = receiver_ty else {
            return false;
        };
        let Some(field_ty) = self
            .shapes
            .get(shape_name)
            .and_then(|shape| shape.field_ty(&method.text))
        else {
            return false;
        };
        if matches!(field_ty, Ty::Fn(..)) {
            let verdict = UfcsVerdict::FieldCall {
                receiver: receiver_ty.clone(),
                field: method.text.clone(),
                field_ty: field_ty.clone(),
            };
            self.table
                .insert(NodeKey::new(self.file, path.range), verdict);
        } else {
            let message = format!(
                "field `{field}` on `{shape_name}` is not callable (its type is `{found}`) — \
                 field access wins over a free function of the same name, so this is never \
                 re-read as `{field}({receiver_text}, …)`",
                field = method.text,
                found = field_ty.display(),
            );
            self.push(path.range, DiagnosticCode::E140, &message);
        }
        true
    }

    /// Step 3 (D4/D5). Returns `true` when a free function of the called
    /// name is in ordinary lexical scope, or the name is a T1b/NS stdlib
    /// prelude verb (D4's candidate set is "ordinary lexical scope only
    /// (file `use` + prelude)" — `resolve::is_t1b_stdlib_name`/
    /// `resolve::is_builtin_function`, e.g. `len`/`push`/`sort_by`, are not
    /// index symbols and would otherwise fall through to the `E141` "no
    /// function in scope" diagnostic, which is false: `push(xs, v)` compiles
    /// today).
    ///
    /// **D5** picks the desugar's shape from the target's *first declared
    /// parameter*: `ref` → [`UfcsVerdict::FreeFnAutoRef`] (the receiver is
    /// passed by reference, provided it can be written through — otherwise
    /// `E143`, see [`Self::auto_ref_fault`]); anything else → the plain
    /// by-value [`UfcsVerdict::FreeFnDesugar`], with no lvalue requirement on
    /// the receiver at all. The prelude verbs have no user-declared params to
    /// read, so they are always the by-value shape here — the collection
    /// mutators' own lvalue discipline is LIR lowering's ruled RMW expansion
    /// (`brink_ir::lir::lower::blocks::try_lower_mutator_stmt`), unchanged.
    fn try_free_fn_desugar(
        &mut self,
        path: &HirPath,
        method: &brink_ir::Name,
        receiver: &Receiver<'_>,
        arg_count: usize,
    ) -> bool {
        let Some(target) = crate::resolve::lookup_by_name(
            self.index,
            self.scope,
            &method.text,
            &[SymbolKind::Knot, SymbolKind::External],
        ) else {
            // No index symbol of this name — the T1b/NS stdlib prelude is
            // the other half of D4's candidate set. It has no `DefinitionId`
            // (VM-native, resolved at LIR lowering) and so no arity to check
            // here.
            if crate::resolve::is_t1b_stdlib_name(&method.text)
                || crate::resolve::is_builtin_function(&method.text)
            {
                // Issue #1919: the prelude sibling of the `arg_mismatches`
                // computed below for the free-fn desugar — see
                // `check_ufcs_prelude_arg_types`'s own doc for why this is
                // safe to compute here (D1's field-access-wins check has
                // already run by the time this verdict is reached) and why
                // it cannot double-report `E149` (a disjoint diagnostic
                // family from the domain mismatches this checks).
                let arg_mismatches =
                    self.check_ufcs_prelude_arg_types(path.range, &receiver.ty, &method.text);
                let verdict = UfcsVerdict::PreludeDesugar {
                    receiver: receiver.ty.clone(),
                    name: method.text.clone(),
                    arg_mismatches,
                };
                self.table
                    .insert(NodeKey::new(self.file, path.range), verdict);
                return true;
            }
            return false;
        };
        let first_param_is_ref = self
            .index
            .symbols
            .get(&target)
            .and_then(|info| info.params.first())
            .is_some_and(|p| p.is_ref);
        if first_param_is_ref && let Some(cause) = self.auto_ref_fault(receiver) {
            let message = format!(
                "cannot mutate `{receiver_text}` through `{name}`: `{name}`'s first parameter is \
                 `ref`, so `{receiver_text}.{name}(…)` auto-refs its receiver (D5) — but {cause}. \
                 Bind the receiver to a durable cell, or call a by-value function on it",
                name = method.text,
                receiver_text = receiver.text,
            );
            self.push(path.range, DiagnosticCode::E143, &message);
            return true;
        }
        // Every other resolved call gets an arity check (`resolve::
        // check_arity`) before it is declared resolved; this desugar owes
        // the same — the receiver counts as the first argument.
        let expected = self
            .index
            .symbols
            .get(&target)
            .map(|info| info.params.len());
        let actual = arg_count + 1;
        if let Some(expected) = expected
            && expected != actual
        {
            let message = format!(
                "`{name}` expects {expected} argument(s), got {actual} \
                 (`{receiver_text}.{name}(…)` desugars to `{name}({receiver_text}, …)`, counting \
                 the receiver as the first argument)",
                name = method.text,
                receiver_text = receiver.text,
            );
            self.push(path.range, DiagnosticCode::E031, &message);
        }
        // Issue #1881: the argument-type half of this call site's check —
        // computed here (unconditionally, like everything else in this
        // resolution pass) and carried on the verdict for `check_strict` to
        // report as `E063`.
        let arg_mismatches = self.check_ufcs_arg_types(path.range, target, receiver);
        let verdict = if first_param_is_ref {
            UfcsVerdict::FreeFnAutoRef {
                receiver: receiver.ty.clone(),
                name: method.text.clone(),
                target,
                arg_mismatches,
            }
        } else {
            UfcsVerdict::FreeFnDesugar {
                receiver: receiver.ty.clone(),
                name: method.text.clone(),
                target,
                arg_mismatches,
            }
        };
        self.table
            .insert(NodeKey::new(self.file, path.range), verdict);
        true
    }

    /// Issue #1881: the desugared call's argument-type check — `target`'s
    /// already-known declared param types (`self.signatures`, this pass's
    /// own `InferenceResult::signatures` projection) against the receiver
    /// (param `0`) and every *written* argument (param `1..`, read back
    /// from `infer::body`'s own recorded [`super::UfcsCallArgs`] fact for
    /// this exact call-site `range` — this pass has no expression-type
    /// inference of its own, see that struct's own doc for why the split
    /// lives here).
    ///
    /// Mirrors `infer::body::InferPass::infer_call`'s own direct-call check
    /// (`assignable`, skipping whenever either side is `Unknown`/
    /// `Conflicted`) with one simplification: unlike a direct call's
    /// argument, nothing in `infer::body`'s own walk ever `observe`s a
    /// UFCS receiver or written argument against `target`'s declared param
    /// type — the multi-segment branch in `infer_call` runs no `observe`
    /// call at all (issue #1909's `infer_ufcs_free_fn_result` gave that
    /// branch a *result type*, deliberately read-only in the receiver and
    /// silent on the arguments, exactly so this stays true). So there
    /// is no `arg_is_observed_local`-style double-report risk against
    /// `E066` to guard against here, unlike `DirectCallArgMismatch`'s own
    /// exclusion.
    ///
    /// **The D5 auto-ref interaction** (both call sites — `first_param_is_ref`
    /// or not — share this one check): a `ref` first param's entry in
    /// `self.signatures` is *not* a special "reference" `Ty` — `InferredSig`
    /// carries no such variant. `body::infer_def_body` derives every
    /// param's row (`ref` included) from `pass.locals`, the type the body
    /// itself observed that parameter holding — i.e. the **referent's**
    /// own type, exactly what `receiver.ty` (a value type, never wrapped)
    /// already is. So comparing `receiver.ty` against `sig.params[0]` with
    /// the same plain `assignable` this whole function otherwise uses is
    /// correct for `FreeFnAutoRef` too, not just `FreeFnDesugar` — no
    /// auto-ref-specific unwrapping needed, and none of the false positives
    /// #1895 shipped by assuming a receiver's call-site type must literally
    /// equal a param's declared spelling.
    fn check_ufcs_arg_types(
        &self,
        range: TextRange,
        target: DefinitionId,
        receiver: &Receiver<'_>,
    ) -> Vec<UfcsArgMismatch> {
        let Some(sig) = self.signatures.get(&target) else {
            return Vec::new();
        };
        let empty: Vec<Ty> = Vec::new();
        let written: &[Ty] = self
            .current_body()
            .and_then(|b| b.ufcs_call_args.iter().find(|f| f.range == range))
            .map_or(empty.as_slice(), |f| f.args.as_slice());

        let mut mismatches = Vec::new();
        // Index 0: the receiver itself — `name(recv, args)`'s first
        // positional slot, same "receiver counts as the first argument"
        // convention this call site's own arity-mismatch diagnostic uses.
        if let Some(param_ty) = sig.params.first()
            && !param_ty.is_unresolved()
            && !assignable(param_ty, &receiver.ty)
        {
            mismatches.push(UfcsArgMismatch {
                index: 0,
                expected: param_ty.clone(),
                found: receiver.ty.clone(),
            });
        }
        for (i, arg_ty) in written.iter().enumerate() {
            if arg_ty.is_unresolved() {
                continue;
            }
            let Some(param_ty) = sig.params.get(i + 1) else {
                continue;
            };
            if !param_ty.is_unresolved() && !assignable(param_ty, arg_ty) {
                mismatches.push(UfcsArgMismatch {
                    index: i + 1,
                    expected: param_ty.clone(),
                    found: arg_ty.clone(),
                });
            }
        }
        mismatches
    }

    /// Issue #1919: `PreludeDesugar`'s own argument-domain check — the T1b/
    /// NS-A1 stdlib-verb sibling of [`Self::check_ufcs_arg_types`] (issue
    /// #1881, `FreeFnDesugar`/`FreeFnAutoRef`). A prelude verb has no
    /// [`DefinitionId`] and no declared parameter list
    /// ([`UfcsVerdict::PreludeDesugar`]'s own doc) — its "signature" instead
    /// lives as `infer::body::InferPass::infer_intrinsic`'s own per-verb
    /// domain rules, keyed off the receiver's *inferred container type*
    /// (an array's element type, a map's key/value types).
    ///
    /// This mirrors that domain knowledge declaratively, for the verbs
    /// whose domain is a plain container projection, rather than calling
    /// `infer_intrinsic` itself: that method needs a live `Expr` for its
    /// receiver-write arms (`push`/`insert`/… call
    /// `record_write(args.first())`, resolving an `Expr::Path` back through
    /// the `ResolutionMap` — keyed at the *whole call's* own range, so a
    /// synthetic receiver-only `Expr` would resolve to nothing there) and
    /// mutates the body-inference walk's own `self.locals`/
    /// `self.array_remove_calls` state, neither of which this post-hoc,
    /// already-fully-resolved verdict pass has access to (see
    /// `check_strict`'s own module doc for why the `E149` half stays a
    /// hand-written twin rather than an `infer_intrinsic` call, for the
    /// same reason).
    ///
    /// Unlike [`infer::body::InferPass::infer_ufcs_free_fn_result`]
    /// (issue #1909), which has to decline a `Ty::Struct` receiver because
    /// D1's field-access-wins rule has not run yet at body-inference time,
    /// this check runs no such risk: [`Self::resolve_call`] always tries
    /// [`Self::try_field_call`] (D1) before [`Self::try_free_fn_desugar`]
    /// (D3/D4), so a `PreludeDesugar` verdict is only ever constructed once
    /// field access has already lost.
    ///
    /// `remove`'s *array* leg is deliberately excluded from the domain
    /// table below: that shape is `E149` (issue #1540), already reported
    /// unconditionally for this exact verdict by
    /// [`strict_verdict_diagnostics`] — a disjoint diagnostic family from
    /// the domain mismatches this method reports, so there is no risk of
    /// double-reporting between the two.
    fn check_ufcs_prelude_arg_types(
        &self,
        range: TextRange,
        receiver: &Ty,
        name: &str,
    ) -> Vec<UfcsArgMismatch> {
        let expected: Vec<Ty> = match (name, receiver) {
            ("push" | "heap_push" | "index_of", Ty::Array(elem)) => vec![(**elem).clone()],
            ("contains", Ty::Array(elem)) => vec![(**elem).clone()],
            ("contains", Ty::Map(k, _)) => vec![(**k).clone()],
            ("get" | "remove", Ty::Map(k, _)) => vec![(**k).clone()],
            ("contains_value", Ty::Map(_, v)) => vec![(**v).clone()],
            ("insert", Ty::Map(k, v)) => vec![(**k).clone(), (**v).clone()],
            _ => return Vec::new(),
        };
        let empty: Vec<Ty> = Vec::new();
        let written: &[Ty] = self
            .current_body()
            .and_then(|b| b.ufcs_call_args.iter().find(|f| f.range == range))
            .map_or(empty.as_slice(), |f| f.args.as_slice());

        let mut mismatches = Vec::new();
        for (i, expected_ty) in expected.iter().enumerate() {
            let Some(arg_ty) = written.get(i) else {
                continue;
            };
            if arg_ty.is_unresolved() {
                continue;
            }
            if !assignable(expected_ty, arg_ty) {
                mismatches.push(UfcsArgMismatch {
                    index: i + 1,
                    expected: expected_ty.clone(),
                    found: arg_ty.clone(),
                });
            }
        }
        mismatches
    }

    /// **D5's receiver gate.** `Some(cause)` when auto-ref cannot write
    /// through this receiver, phrased as the tail of the `E143` message;
    /// `None` when it can.
    ///
    /// The desugar rides the T1e ref-argument machinery verbatim
    /// (`brink_ir::lir::lower::expr::lower_call_args`), so it inherits that
    /// machinery's own rules rather than inventing a second set:
    ///
    /// - A **bare** receiver (`gold.bump(1)`) binds like any unmarked
    ///   ref-argument: a frame slot (param/temp) or a global `VAR` both work
    ///   — `lower_ref_path_call_arg`'s `RefTemp`/`RefGlobal` pair.
    /// - A **projection off a durable cell** (`party.leader.heal(5)` where
    ///   `party` is a `VAR`) becomes a real `lir::CallArg::RefProjection`,
    ///   whose root must be durable (`docs/t1e-spec.md` §2, the `E080` rule
    ///   `ref_projection::check_durable_root` enforces for the explicitly
    ///   spelled form) — that requirement is unchanged by this gate.
    /// - A **projection off a frame-local** (`g.hp.heal(5)` where `g` is a
    ///   `let`/param) is legal too, **RULED 2026-07-27** (issue #1531,
    ///   `docs/decision-log.md`): a frame-local cell is a valid projection
    ///   root, and the mutation needs no effect row because it is
    ///   unobservable outside the frame. `RefProjection`'s own root stays
    ///   durable-only (`docs/format-v4-rfc.md` §1), so LIR lowering does not
    ///   reuse that machinery for this case — it splices a read/call/
    ///   write-back RMW sequence instead (`brink_ir::lir::lower::blocks::
    ///   try_lower_frame_local_auto_ref_stmt`), the same discipline plain
    ///   assignment (`g.hp = 5`) already uses. That lowering only has a
    ///   statement-shaped expansion, so it covers a **single field level**
    ///   only — the same boundary `try_lower_field_assignment` draws
    ///   (`E074` for a deeper chain); this gate mirrors that boundary by
    ///   only clearing a two-segment receiver (root + one field).
    /// - A `CONST` is never writable at any depth.
    ///
    /// The ruled rvalue receivers (`[1,2].push(3)`, `a.sorted().push(x)` —
    /// "mutating a temporary loses the mutation") reach this gate as soon as
    /// they are spellable: today's native grammar admits only a dotted path
    /// as a call's callee (`brink-syntax-native`'s `parser::expr::
    /// path_or_call`), so a literal or a call cannot yet sit in receiver
    /// position at all.
    fn auto_ref_fault(&self, receiver: &Receiver<'_>) -> Option<String> {
        let head = receiver.segments.first().map_or("", |s| s.text.as_str());
        // A frame-local projection root is legal (issue #1531) only one
        // field level deep — `head.field`, i.e. exactly two receiver
        // segments. Anything deeper has no lowering (LIR's RMW expansion is
        // single-level, matching `try_lower_field_assignment`'s own `E074`
        // boundary), so it still faults here.
        let frame_local = || {
            (receiver.segments.len() > 2).then(|| {
                format!(
                    "`{head}` is a temp/param — a frame-local projection can only reach one \
                     field level (`{head}.field`); this receiver goes deeper than that"
                )
            })
        };
        match self.index.symbols.get(&receiver.def) {
            Some(info) => match info.kind {
                SymbolKind::Variable => None,
                SymbolKind::Constant => Some(format!("`{head}` is a CONST, not a mutable cell")),
                SymbolKind::Param | SymbolKind::Temp => frame_local(),
                // `value_receiver_def` admits no other kind as a receiver.
                _ => Some(format!(
                    "`{head}` is not a value that can be written through"
                )),
            },
            // Absent from `brink-db`'s narrowed index projection: a local
            // temp/param, exactly as `value_receiver_def`/`head_ty` already
            // treat it (and `ref_projection::check_durable_root`'s own
            // `LocalVar` fallback).
            None => frame_local(),
        }
    }

    /// The resolved definition of `path`'s head when `path` is a
    /// *method-call-shaped* callee: the resolver recorded the head value (a
    /// param/temp/VAR/CONST) as the callee's target rather than a callable
    /// definition. `None` for an ordinary qualified call (a module-qualified
    /// free call, an ink `knot.stitch()` visit).
    ///
    /// This is the mirror of `resolve::resolve_function`'s own UFCS-shaped
    /// fallback — the two must agree, or a call would either be diagnosed
    /// twice or not at all. `resolve_function`'s lookup is project-wide
    /// (`resolve::lookup_by_name`), not file-scoped, so this returns the
    /// same project-wide [`DefinitionId`] rather than re-deriving one from
    /// the head's name alone — [`Self::head_ty`] types it from exactly that
    /// id, the same way `structs::resolved_symbol_ty` types any other
    /// resolved reference.
    fn value_receiver_def(&self, path: &HirPath) -> Option<DefinitionId> {
        // `path.range` here is the callee `Path`'s whole span — this lookup
        // is one of the four consumers keyed on the call-path
        // `ResolvedRef::range` contract (issue #1561); see that field's doc.
        let key = (path.range.start().into(), path.range.end().into());
        let &target = self.resolution_by_range.get(&key)?;
        match self.index.symbols.get(&target) {
            Some(info)
                if matches!(
                    info.kind,
                    SymbolKind::Param
                        | SymbolKind::Temp
                        | SymbolKind::Variable
                        | SymbolKind::Constant
                ) =>
            {
                Some(target)
            }
            // brink-db's narrowed index projection can strip locals; the
            // definition tag still identifies them (mirrors
            // `infer::body::infer_call`'s own `is_value_callee`).
            None if target.tag() == brink_format::DefinitionTag::LocalVar => Some(target),
            Some(_) | None => None,
        }
    }

    /// The receiver's type: the head segment's own type (typed from
    /// `head_def`, the definition `resolve::resolve_function` actually
    /// bound the head to), then each further segment walked through the
    /// declared shape table. `None` whenever any step lands on an unknown or
    /// conflicted type — the D3 case.
    fn receiver_ty(&self, head_def: DefinitionId, segments: &[brink_ir::Name]) -> Option<Ty> {
        let (head, rest) = segments.split_first()?;
        let mut ty = self.head_ty(head_def, head)?;
        for seg in rest {
            let Ty::Struct(shape_name) = &ty else {
                return None;
            };
            let field = self.shapes.get(shape_name)?.field_ty(&seg.text)?.clone();
            ty = field;
        }
        (!ty.is_unknown() && ty != Ty::Conflicted).then_some(ty)
    }

    /// The head segment's type, read from `def` — the *resolved* definition,
    /// exactly as `structs::resolved_symbol_ty` reads any other resolved
    /// reference: a param/temp reads the enclosing def's finalized local *by
    /// name* (`def`'s own name — locals are keyed by name, not id); a global
    /// `VAR`/`CONST` reads `infer::collect_globals`'s declaration-derived
    /// type *by id*, project-wide, never file-scoped. Dispatching on `def`'s
    /// own kind (rather than trying `current_locals()` by `head.text` first,
    /// unconditionally) also means a body-local shadowing a same-named
    /// global after the call site can never be mistaken for the global the
    /// resolver actually bound.
    fn head_ty(&self, def: DefinitionId, head: &brink_ir::Name) -> Option<Ty> {
        match self.index.symbols.get(&def) {
            Some(info) => match info.kind {
                SymbolKind::Param | SymbolKind::Temp => {
                    self.current_locals()?.get(&info.name).cloned()
                }
                SymbolKind::Variable | SymbolKind::Constant => self.globals.get(&def).cloned(),
                _ => None,
            },
            // brink-db's narrowed index projection can strip locals (see
            // `value_receiver_def`'s own fallback); the enclosing body's
            // finalized locals are keyed by name and unaffected by that
            // projection, so fall back to `head.text`.
            None => self.current_locals()?.get(&head.text).cloned(),
        }
    }

    fn push(&mut self, range: TextRange, code: DiagnosticCode, detail: &str) {
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message: format!("{}: {detail}", code.title()),
            code,
        });
    }
}
