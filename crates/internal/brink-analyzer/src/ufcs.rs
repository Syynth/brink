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
//!    function is method-callable) and record the desugar to
//!    `name(recv, args)` ([`UfcsVerdict::FreeFnDesugar`]).
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
//! tree" posture (`infer::InferenceResult`). LIR lowering reads the table to
//! emit either a field-value call or the desugared free call; IDE
//! hover/go-to-def reads it to name the real target.
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
}

/// Every UFCS call site's verdict for one project.
pub type UfcsTable = SideTable<UfcsVerdict>;

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
        if !self.is_value_receiver(path) {
            // The callee path resolves to a real callable (a
            // module-qualified free call, an ink `knot.stitch()` visit) —
            // an ordinary qualified call, not method-call syntax.
            return;
        }

        let receiver_text = receiver_segs
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");

        let Some(receiver_ty) = self.receiver_ty(receiver_segs) else {
            // D3: no deferral machinery — demand an annotation.
            self.push(
                path.range,
                DiagnosticCode::E142,
                format!(
                    "cannot resolve `{receiver_text}.{method}(…)`: the type of `{receiver_text}` \
                     is not known here, so it is undecidable whether `{method}` is one of its \
                     fields — annotate the receiver",
                    method = method.text,
                ),
            );
            return;
        };

        // Step 2 — field access wins outright (D1).
        if let Ty::Struct(shape_name) = &receiver_ty
            && let Some(shape) = self.shapes.get(shape_name)
            && let Some(field_ty) = shape.field_ty(&method.text)
        {
            if matches!(field_ty, Ty::Fn(..)) {
                self.table.insert(
                    NodeKey::new(self.file, path.range),
                    UfcsVerdict::FieldCall {
                        receiver: receiver_ty.clone(),
                        field: method.text.clone(),
                        field_ty: field_ty.clone(),
                    },
                );
            } else {
                self.push(
                    path.range,
                    DiagnosticCode::E140,
                    format!(
                        "field `{field}` on `{shape_name}` is not callable (its type is \
                         `{found}`) — field access wins over a free function of the same name, \
                         so this is never re-read as `{field}({receiver_text}, …)`",
                        field = method.text,
                        found = display_ty(field_ty),
                    ),
                );
            }
            return;
        }

        // Step 3 — a free function in ordinary lexical scope (D4).
        if let Some(target) = crate::resolve::lookup_by_name(
            self.index,
            self.scope,
            &method.text,
            &[SymbolKind::Knot, SymbolKind::External],
        ) {
            // D5 fence: by-value desugar only until #1462 lands auto-ref.
            if self
                .index
                .symbols
                .get(&target)
                .and_then(|info| info.params.first())
                .is_some_and(|p| p.is_ref)
            {
                self.push(
                    path.range,
                    DiagnosticCode::E143,
                    format!(
                        "`{name}`'s first parameter is `ref`, and method-call syntax onto a \
                         `ref` parameter (auto-ref) is not supported yet — see issue #1462. \
                         Spell the call explicitly as `{name}(ref {receiver_text}{comma})` for \
                         now",
                        name = method.text,
                        comma = if arg_count == 0 { "" } else { ", …" },
                    ),
                );
                return;
            }
            self.table.insert(
                NodeKey::new(self.file, path.range),
                UfcsVerdict::FreeFnDesugar {
                    receiver: receiver_ty,
                    name: method.text.clone(),
                    target,
                },
            );
            return;
        }

        // Step 4 — neither; one diagnostic naming both attempts.
        self.push(
            path.range,
            DiagnosticCode::E141,
            format!(
                "cannot resolve `{receiver_text}.{method}(…)`: `{recv_ty}` declares no field \
                 `{method}`, and no function `{method}` is in scope here",
                method = method.text,
                recv_ty = display_ty(&receiver_ty),
            ),
        );
    }

    /// Whether `path` is a *method-call-shaped* callee: the resolver
    /// recorded the head value (a param/temp/VAR/CONST) as the callee's
    /// target rather than a callable definition.
    ///
    /// This is the mirror of `resolve::resolve_function`'s own UFCS-shaped
    /// fallback — the two must agree, or a call would either be diagnosed
    /// twice or not at all.
    fn is_value_receiver(&self, path: &HirPath) -> bool {
        let key = (path.range.start().into(), path.range.end().into());
        let Some(&target) = self.resolution_by_range.get(&key) else {
            return false;
        };
        match self.index.symbols.get(&target) {
            Some(info) => matches!(
                info.kind,
                SymbolKind::Param | SymbolKind::Temp | SymbolKind::Variable | SymbolKind::Constant
            ),
            // brink-db's narrowed index projection can strip locals; the
            // definition tag still identifies them (mirrors
            // `infer::body::infer_call`'s own `is_value_callee`).
            None => target.tag() == brink_format::DefinitionTag::LocalVar,
        }
    }

    /// The receiver's type: the head segment's own type, then each further
    /// segment walked through the declared shape table. `None` whenever any
    /// step lands on an unknown or conflicted type — the D3 case.
    fn receiver_ty(&self, segments: &[brink_ir::Name]) -> Option<Ty> {
        let (head, rest) = segments.split_first()?;
        let mut ty = self.head_ty(head)?;
        for seg in rest {
            let Ty::Struct(shape_name) = &ty else {
                return None;
            };
            let field = self.shapes.get(shape_name)?.field_ty(&seg.text)?.clone();
            ty = field;
        }
        (!ty.is_unknown() && ty != Ty::Conflicted).then_some(ty)
    }

    /// The head segment's type: an enclosing def's finalized local
    /// (param/temp) by name, else a declaration-derived global — the same
    /// two sources `structs::classify_expr_ty` reads, and the same firewall
    /// (`infer::body` never sees another def's locals either).
    fn head_ty(&self, head: &brink_ir::Name) -> Option<Ty> {
        if let Some(locals) = self.current_locals()
            && let Some(ty) = locals.get(&head.text)
        {
            return Some(ty.clone());
        }
        let id = annotations::def_id_for(self.index, self.file, SymbolKind::Variable, &head.text)
            .or_else(|| {
            annotations::def_id_for(self.index, self.file, SymbolKind::Constant, &head.text)
        })?;
        self.globals.get(&id).cloned()
    }

    fn push(&mut self, range: TextRange, code: DiagnosticCode, detail: String) {
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message: format!("{}: {detail}", code.title()),
            code,
        });
    }
}

/// A receiver/field type as it reads in a diagnostic. Nominal types show
/// their declared name; everything else shows the checker's own spelling.
fn display_ty(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".into(),
        Ty::Float => "float".into(),
        Ty::Bool => "bool".into(),
        Ty::String => "string".into(),
        Ty::Struct(name) | Ty::List(name) | Ty::Handle(name) => name.clone(),
        other => format!("{other:?}"),
    }
}
