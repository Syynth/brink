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
//! **D5 is out of scope** (issue #1462 builds on top of this pass): the
//! desugar recorded here is **by value only**. A free function whose first
//! parameter is declared `ref` is refused with
//! [`DiagnosticCode::E143`] — pointing at #1462 — rather than desugared by
//! value, which would silently drop the mutation.
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
//! fallback now, not the unconditional behavior. IDE hover/go-to-def, to
//! name the real target rather than the receiver the [`ResolutionMap`]
//! records for the callee path, is still unwired.
//!
//! [`SideTable`] is deliberately generic over its payload: it is
//! `(node → verdict)` plumbing, so a second payload kind can ride the same
//! keying and the same lookup without a parallel structure being invented
//! (issue #1492 is expected to do exactly that).

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, Path as HirPath, ResolutionMap,
    Stitch, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, Ty};
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
    /// A free function won (step 3): the call desugars to
    /// `name(recv, args)`, by value.
    FreeFnDesugar {
        /// The receiver's inferred type.
        receiver: Ty,
        /// The free function's name, as written.
        name: String,
        /// The definition the desugared call targets.
        target: DefinitionId,
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
    },
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
/// and `brink-test-harness`'s hand-assembled native pipeline
/// (`corpus::compile_and_explore_from_brink_native`, which has no salsa
/// layer to memoize the table in).
#[must_use]
pub fn to_lir_lookup(table: &UfcsTable) -> brink_ir::lir::UfcsLookup {
    let entries = table
        .iter()
        .map(|(key, verdict)| {
            let range = TextRange::new(key.range.0.into(), key.range.1.into());
            let mirrored = match verdict {
                UfcsVerdict::FieldCall { .. } => brink_ir::lir::UfcsVerdict::FieldCall,
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
            resolution_by_range: &resolution_by_range,
            current_knot_name: None,
            knot_locals: None,
            stitch_locals: None,
            table: &mut table,
            diagnostics: &mut diagnostics,
        };
        visit::visit(hir, &mut v);
    }

    (table, diagnostics)
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
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    current_knot_name: Option<String>,
    knot_locals: Option<&'a BTreeMap<String, Ty>>,
    stitch_locals: Option<&'a BTreeMap<String, Ty>>,
    table: &'a mut UfcsTable,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for UfcsVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.current_knot_name = Some(knot.name.text.clone());
        self.knot_locals =
            annotations::def_id_for(self.index, self.file, knot.symbol_kind(), &knot.name.text)
                .and_then(|id| self.bodies.get(&id))
                .map(|b| &b.locals);
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.current_knot_name = None;
        self.knot_locals = None;
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        self.stitch_locals = self.current_knot_name.as_ref().and_then(|knot_name| {
            let qualified = format!("{knot_name}.{}", stitch.name.text);
            annotations::def_id_for(self.index, self.file, SymbolKind::Stitch, &qualified)
                .and_then(|id| self.bodies.get(&id))
                .map(|b| &b.locals)
        });
    }

    fn exit_stitch(&mut self, _stitch: &Stitch) {
        self.stitch_locals = None;
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::Call(path, args) = expr {
            self.resolve_call(path, args.len());
        }
    }
}

impl UfcsVisitor<'_> {
    fn current_locals(&self) -> Option<&BTreeMap<String, Ty>> {
        self.stitch_locals.or(self.knot_locals)
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

        // Step 2 — field access wins outright (D1).
        if self.try_field_call(path, method, &receiver_ty, &receiver_text) {
            return;
        }

        // Step 3 — a free function in ordinary lexical scope (D4).
        if self.try_free_fn_desugar(path, method, receiver_ty.clone(), &receiver_text, arg_count) {
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
                recv_ty = receiver_ty.display(),
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
        receiver_ty: &Ty,
        receiver_text: &str,
    ) -> bool {
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
    /// today). Recorded as the by-value desugar, or refused with `E143` when
    /// an index-symbol target's first parameter is `ref` (auto-ref is issue
    /// #1462, built on top of this pass) — the prelude verbs have no
    /// user-declared `ref` params to refuse.
    fn try_free_fn_desugar(
        &mut self,
        path: &HirPath,
        method: &brink_ir::Name,
        receiver_ty: Ty,
        receiver_text: &str,
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
                let verdict = UfcsVerdict::PreludeDesugar {
                    receiver: receiver_ty,
                    name: method.text.clone(),
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
        if first_param_is_ref {
            let message = format!(
                "`{name}`'s first parameter is `ref`, and method-call syntax onto a `ref` \
                 parameter (auto-ref) is not supported yet — see issue #1462. Spell the call \
                 explicitly as `{name}(ref {receiver_text}{comma})` for now",
                name = method.text,
                comma = if arg_count == 0 { "" } else { ", …" },
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
            );
            self.push(path.range, DiagnosticCode::E031, &message);
        }
        let verdict = UfcsVerdict::FreeFnDesugar {
            receiver: receiver_ty,
            name: method.text.clone(),
            target,
        };
        self.table
            .insert(NodeKey::new(self.file, path.range), verdict);
        true
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
