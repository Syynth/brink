//! TM-4b struct construction-literal semantic checks (docs/typed-mode-spec.md
//! §6).
//!
//! Strict-mode-only (`types = strict`): "missing/extra fields at
//! construction: compile error (strict) / construction fault (gradual)" —
//! under `types = gradual` a project never runs [`check`] at all (mirrors
//! `strict::check`'s own gating), deferring entirely to the runtime fault
//! PR #664 already built (`RecordGetDyn`'s missing-field fault). Wired into
//! `strict::check` alongside E065/E066/E067, behind the same
//! `TypePolicy::Strict` + `dialect = brink` guard `strict::config_error`
//! already enforces.
//!
//! Three checks, each naming the offending field, all strict-only per the
//! spec's own wording ("missing/extra fields at construction: compile error
//! (strict) / construction fault (gradual)"):
//! - **Missing** (`E069`): a declared field with no initializer in the
//!   literal.
//! - **Extra** (`E070`): an initializer for a field the shape doesn't
//!   declare.
//! - **Mistyped** (`E071`): an initializer whose *statically
//!   classifiable* type disagrees with the field's declared type.
//!   Literal-shaped initializers (int/float/bool/string/array/map/nested
//!   struct literals) classify from their own shape alone. A variable-,
//!   call-, or index-valued initializer (issue #670) instead consults the
//!   inference substrate already threaded into `strict::check`: a `Path`
//!   resolving to a param/temp reads its finalized type from that def's own
//!   `BodyTypes::locals`; a `Path` resolving to a global `VAR`/`CONST` reads
//!   its declaration-derived type (`infer::collect_globals`, the same
//!   source `infer::body` itself reads through the firewall); a call reads
//!   the resolved callee's `InferredSig::return_ty`; an index expression
//!   recurses into its base's classified type and takes the
//!   array-element/map-value type. Whenever that resolution lands on
//!   `Unknown` or `Conflicted` (unresolved, unannotated, or genuinely
//!   contradictory), the field stays silently unchecked — same "Unknown
//!   never disagrees" spirit as `annotations::mismatches`.
//!
//! An unresolved shape name (`E068`, already reported by
//! `resolve::resolve_struct_ref`) is not re-reported here — a construction
//! against a shape that doesn't exist has no declared fields to check
//! against.
//!
//! [`check_duplicates`] (`E084`, issue #675) is a fourth, *policy-
//! independent* check: a construction literal supplying the same field
//! name more than once is flagged under both `types = gradual` and
//! `types = strict` — it doesn't need the shape to resolve, so it's wired
//! into `per_file_diagnostics` unconditionally within a file (no
//! `TypePolicy` gate) rather than behind `strict::config_error` the way
//! [`check`] is. Its `dialect`/`is_native` gating is wider than `check`'s
//! own brink-only block, though: B5 (issue #1464, #1103 cascade ruling
//! (A)) made `TypeName { … }` construction reach `StructLiteral` from the
//! native surface (`Point { x: 1 }`) as well as the brink dialect's own
//! `#{…}` spelling, so the caller runs this under `dialect = Brink ||
//! is_native` — same reasoning `map_keys::check_duplicate_keys`'s own doc
//! gives for `E138`.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, ResolutionMap, Stitch, StructLiteral,
    SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, InferredSig, Ty};

/// One declared struct shape: fields in declaration order, name -> declared
/// type (`Ty::Unknown` if the field's own annotation doesn't resolve —
/// e.g. an unrecognized type name, already flagged elsewhere by
/// `annotations::check`'s `E061`).
///
/// Originally `pub(crate)` (issue #831) so `ref_projection`'s strict-mode
/// path-segment check could reuse this exact shape table for `ref
/// lvalue-path` field segments — "reuse existing machinery"
/// (docs/t1e-spec.md §6) rather than building a second one. Promoted to a
/// crate-public API (issue #858) so out-of-crate tooling (e.g. `brink-ide`
/// struct-field ref-path completion, T1e-3's deferred "path continuations
/// after a `.`/`[`" item) can query declared shapes without duplicating this
/// table.
pub struct ShapeInfo {
    fields: Vec<(String, Ty)>,
}

impl ShapeInfo {
    /// The declared type of `name`, or `None` if the shape has no such
    /// field.
    #[must_use]
    pub fn field_ty(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    /// Whether the shape declares a field named `name`.
    #[must_use]
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|(n, _)| n == name)
    }
}

/// Every declared `STRUCT` shape in the project, by name.
///
/// Public (issue #858) so tooling outside `brink-analyzer` can resolve a
/// `STRUCT`'s declared fields — e.g. offering field-name completions after
/// `npc.` in a `ref lvalue-path` — without re-deriving the shape table this
/// crate already builds for its own construction-literal checks
/// ([`check`]) and `ref`-projection path-segment validation.
#[must_use]
pub fn declared_shapes(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
) -> BTreeMap<String, ShapeInfo> {
    // No manifest access at this call site (`structs::check` isn't
    // threaded a `HostManifest` — struct field types aren't in T1d-2's
    // scope), so `handle<K>` field types resolve `None` here, same as any
    // other name `TypeNames` doesn't recognize — consistent with
    // `annotations::resolve`'s documented "unresolved -> silent" contract.
    let names = annotations::TypeNames::new(index, None);
    let mut out = BTreeMap::new();
    for &(_file, hir) in files {
        for s in &hir.structs {
            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let ty = annotations::resolve(&f.ty, &names).unwrap_or(Ty::Unknown);
                    (f.name.text.clone(), ty)
                })
                .collect();
            out.insert(s.name.text.clone(), ShapeInfo { fields });
        }
    }
    out
}

/// Strict-mode construction checks over every struct literal in the
/// project. Callers only reach this once `strict::config_error` has
/// confirmed `types = strict` + `dialect = brink` (mirrors
/// `strict::check`'s own entry condition).
///
/// `inference`/`resolutions` (issue #670): the same whole-project
/// `InferenceResult`/`ResolutionMap` `strict::check` already computes for
/// its own escape/mismatch checks — this is what lets the mistyped-field
/// check (`E071`) classify a variable/call/index-valued initializer instead
/// of only literal-shaped ones (see the module doc).
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    let shapes = declared_shapes(files, index);
    // No manifest access at this call site, same as `declared_shapes` above
    // — a global's own annotation resolving against `handle<K>` isn't in
    // this check's scope any more than a struct field's is.
    let globals = crate::infer::collect_globals(files, index, None);
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        let mut v = ConstructionVisitor {
            file,
            shapes: &shapes,
            index,
            globals: &globals,
            signatures: &inference.signatures,
            bodies: &inference.bodies,
            resolution_by_range: &resolution_by_range,
            current_knot_name: None,
            knot_locals: None,
            stitch_locals: None,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level declaration initializers aren't part of `visit::visit`'s
        // block-tree walk (see its module doc) — same pattern
        // `dialect_gate`/`annotations` use for VAR/CONST. No enclosing def
        // here, so `locals` is `None` — only a reference to a global
        // VAR/CONST is classifiable at file scope, matching `infer::body`'s
        // own firewall (a body never sees another def's locals either).
        let ctx = MistypeCtx {
            index,
            globals: &globals,
            signatures: &inference.signatures,
            resolution_by_range: &resolution_by_range,
            locals: None,
        };
        for var in &hir.variables {
            check_expr(&var.value, file, &shapes, &ctx, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, file, &shapes, &ctx, &mut out);
        }
    }
    out
}

/// `TextRange` has no `Ord` impl, so a `Path`/`Call` reference's range keys
/// this file-local `BTreeMap` as a `(start, end)` `u32` pair — mirrors
/// `infer::mod`'s and `strict`'s own identically-named helper (each module
/// owns its own copy rather than centralizing; the codebase's established
/// convention for this exact utility).
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `strict::resolution_index`, narrowed to one file at a time (a
/// `Path`'s range is only unique within its own file).
fn resolution_index(
    resolutions: &ResolutionMap,
    file: FileId,
) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| (range_key(r.range), r.target))
        .collect()
}

/// Everything [`classify_expr_ty`] needs to resolve a non-literal
/// initializer's type: the project symbol index, declaration-derived
/// global types, every inferable def's finalized signature (for a
/// call-valued initializer's return type), this file's range→`DefinitionId`
/// resolutions, and — when the struct literal sits inside a knot/stitch
/// body — that def's own finalized `BodyTypes::locals` (`None` at file
/// scope, where only globals are in play).
///
/// `pub(crate)` (issue #983) so `conversions::check`'s own non-literal
/// `int()`/`float()` argument classification can reuse this exact
/// inference-substrate plumbing instead of re-deriving it — same "reuse
/// existing machinery" precedent `ref_projection` follows for
/// [`ShapeInfo`].
pub(crate) struct MistypeCtx<'a> {
    pub(crate) index: &'a SymbolIndex,
    pub(crate) globals: &'a BTreeMap<DefinitionId, Ty>,
    pub(crate) signatures: &'a BTreeMap<DefinitionId, InferredSig>,
    pub(crate) resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    pub(crate) locals: Option<&'a BTreeMap<String, Ty>>,
}

struct ConstructionVisitor<'a> {
    file: FileId,
    shapes: &'a BTreeMap<String, ShapeInfo>,
    index: &'a SymbolIndex,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    signatures: &'a BTreeMap<DefinitionId, InferredSig>,
    bodies: &'a BTreeMap<DefinitionId, crate::infer::BodyTypes>,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    /// The currently-open knot's own name — `enter_stitch` needs it to
    /// reconstruct the qualified `knot.stitch` name a stitch is indexed
    /// under (mirrors `strict::check_value_calls`' own lookup).
    current_knot_name: Option<String>,
    /// The enclosing knot's own finalized locals, set for the duration of
    /// its body (and every stitch nested inside it — `visit::visit` walks a
    /// knot's own body before descending into its stitches, so a stitch's
    /// `enter_stitch` overrides this with its *own* locals rather than
    /// inheriting the parent knot's).
    knot_locals: Option<&'a BTreeMap<String, Ty>>,
    /// The currently-open stitch's own finalized locals, if any — takes
    /// priority over `knot_locals` while set.
    stitch_locals: Option<&'a BTreeMap<String, Ty>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> ConstructionVisitor<'a> {
    fn current_locals(&self) -> Option<&'a BTreeMap<String, Ty>> {
        self.stitch_locals.or(self.knot_locals)
    }

    fn ctx(&self) -> MistypeCtx<'a> {
        MistypeCtx {
            index: self.index,
            globals: self.globals,
            signatures: self.signatures,
            resolution_by_range: self.resolution_by_range,
            locals: self.current_locals(),
        }
    }

    /// The `DefinitionId` a knot/stitch's own name resolves to, mirroring
    /// `strict::check_escapes`'/`check_value_calls`' own lookup — a top-level
    /// stitch promoted to knot status is indexed under `SymbolKind::Stitch`
    /// (#626), hence the `knot.ptr`-derived `kind`.
    fn knot_def_id(&self, knot: &Knot) -> Option<DefinitionId> {
        let kind = knot.symbol_kind();
        annotations::def_id_for(self.index, self.file, kind, &knot.name.text)
    }
}

impl HirVisitor for ConstructionVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.current_knot_name = Some(knot.name.text.clone());
        self.knot_locals = self
            .knot_def_id(knot)
            .and_then(|id| self.bodies.get(&id))
            .map(|b| &b.locals);
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.current_knot_name = None;
        self.knot_locals = None;
    }

    fn enter_stitch(&mut self, stitch: &Stitch) {
        // Stitches are indexed by qualified `knot.stitch` name (mirrors
        // `strict::check_escapes`'/`check_value_calls`' own lookup).
        // `visit::visit` only ever calls `enter_stitch` nested inside an
        // `enter_knot`/`exit_knot` pair, so `current_knot_name` is always
        // set here.
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
        if let Expr::StructLiteral(sl) = expr {
            let ctx = self.ctx();
            check_literal(sl, self.file, self.shapes, &ctx, self.diagnostics);
        }
    }
}

/// Duplicate-field diagnostic (`E084`, issue #675) — unlike [`check`]'s
/// missing/extra/mistyped diagnostics above, this doesn't need the shape to
/// resolve (a repeated field name is detectable from the literal's own
/// field list alone) and runs under *both* `types` policies: a duplicate
/// field is a structural authoring mistake, not a type-checking concern.
/// Callers wire this in under `dialect = Brink || is_native` (wider than
/// every other TM-4c construction-literal check, matching `E138`'s own
/// wiring — see the module doc) rather than gating it behind
/// `strict::config_error` the way [`check`] is.
#[must_use]
pub fn check_duplicates(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut v = DuplicateFieldVisitor {
            file,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // Same file-level VAR/CONST-initializer gap `check`'s own doc
        // explains — `visit::visit`'s block-tree walk doesn't cover them.
        for var in &hir.variables {
            check_duplicates_expr(&var.value, file, &mut out);
        }
        for c in &hir.constants {
            check_duplicates_expr(&c.value, file, &mut out);
        }
    }
    out
}

struct DuplicateFieldVisitor<'a> {
    file: FileId,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for DuplicateFieldVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::StructLiteral(sl) = expr {
            check_literal_duplicates(sl, self.file, self.diagnostics);
        }
    }
}

/// [`check_expr`]'s twin for the duplicate-field pass — same file-level
/// VAR/CONST-initializer recursion, independent of the shape table.
fn check_duplicates_expr(expr: &Expr, file: FileId, out: &mut Vec<Diagnostic>) {
    if let Expr::StructLiteral(sl) = expr {
        check_literal_duplicates(sl, file, out);
    }
    for child in expr_children(expr) {
        check_duplicates_expr(child, file, out);
    }
}

/// Flag every field-name occurrence in `sl` beyond its first — one
/// diagnostic per repeated initializer, naming the field and pointing at
/// the *repeated* occurrence (not the first, so authors see exactly which
/// initializer is the redundant one).
fn check_literal_duplicates(sl: &StructLiteral, file: FileId, out: &mut Vec<Diagnostic>) {
    let mut seen: crate::determinism::LookupSet<&str> = crate::determinism::LookupSet::new();
    for (name, _value) in &sl.fields {
        if !seen.insert(name.text.as_str()) {
            out.push(Diagnostic {
                file,
                range: name.range,
                message: format!(
                    "{}: field `{}` is initialized more than once",
                    DiagnosticCode::E084.title(),
                    name.text
                ),
                code: DiagnosticCode::E084,
            });
        }
    }
}

/// Recurse into `expr` looking for struct literals — used only for the
/// file-level VAR/CONST initializers `visit::visit` doesn't cover; every
/// other position is already reached through the `HirVisitor` walk above.
fn check_expr(
    expr: &Expr,
    file: FileId,
    shapes: &BTreeMap<String, ShapeInfo>,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    if let Expr::StructLiteral(sl) = expr {
        check_literal(sl, file, shapes, ctx, out);
    }
    for child in expr_children(expr) {
        check_expr(child, file, shapes, ctx, out);
    }
}

/// Direct child expressions of `expr` — a small mirror of
/// `hir::visit::walk_expr`'s recursion shape, needed only because
/// `check_expr` runs outside that walker (see its own doc).
fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => vec![inner],
        Expr::FieldAccess(fa) => vec![&fa.base],
        Expr::Infix(ie) => vec![&ie.lhs, &ie.rhs],
        Expr::Call(_, args) => args.iter().collect(),
        Expr::ArrayLiteral(a) => a.elements.iter().collect(),
        Expr::MapLiteral(m) => m.entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::Index(idx) => vec![&idx.base, &idx.index],
        Expr::StructLiteral(sl) => sl.fields.iter().map(|(_, v)| v).collect(),
        // T1c `#fn(target, args…)`: only the bound arguments are child
        // expressions — the target is a static `Path` field, same as `Call`.
        Expr::FnLiteral(fl) => fl.args.iter().collect(),
        // T1e `ref lvalue-path`: only the operand is a child expression.
        Expr::RefArg(ra) => vec![&ra.operand],
        // A lambda's whole body (issue #1685, #1764). A construction
        // literal's shape agreement doesn't depend on where the literal
        // sits, so this walk must reach a braced body's *statements* too,
        // not just its trailing value expression — see
        // `LambdaBody::all_exprs`. (Locals declared inside the body are not
        // in `MistypeCtx::locals`, which is `None` at file scope anyway, so
        // an initializer naming one classifies `Unknown` and stays silent:
        // "Unknown never disagrees", the same posture as every other
        // unclassifiable initializer here.)
        Expr::Lambda(l) => l.body.all_exprs(),
        Expr::Range(r) => vec![&r.start, &r.end],
        Expr::String(s) => s
            .parts
            .iter()
            .filter_map(|p| match p {
                brink_ir::StringPart::Interpolation(e) => Some(e.as_ref()),
                brink_ir::StringPart::Literal(_) => None,
            })
            .collect(),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => Vec::new(),
    }
}

/// Check one struct literal against its declared shape (if resolvable — an
/// unresolved shape name has nothing to check against, and is already
/// diagnosed separately by `resolve::resolve_struct_ref`'s `E068`).
fn check_literal(
    sl: &StructLiteral,
    file: FileId,
    shapes: &BTreeMap<String, ShapeInfo>,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(shape) = shapes.get(&sl.shape.text) else {
        return;
    };

    // Extra fields (strict-only, since `check` only ever runs under strict
    // per its own doc — the module's `structs::check` is only reached from
    // `strict::check`).
    for (name, _value) in &sl.fields {
        if !shape.has_field(&name.text) {
            out.push(Diagnostic {
                file,
                range: name.range,
                message: format!(
                    "{}: `{}` has no field `{}`",
                    DiagnosticCode::E070.title(),
                    sl.shape.text,
                    name.text
                ),
                code: DiagnosticCode::E070,
            });
        }
    }

    // Missing fields (strict-only, since `check` only ever runs under
    // strict per its own doc).
    for (field_name, _ty) in &shape.fields {
        if !sl.fields.iter().any(|(n, _)| &n.text == field_name) {
            out.push(Diagnostic {
                file,
                range: sl.ptr.text_range(),
                message: format!(
                    "{}: `{}` is missing field `{field_name}`",
                    DiagnosticCode::E069.title(),
                    sl.shape.text
                ),
                code: DiagnosticCode::E069,
            });
        }
    }

    // Mistyped fields — only for classifiable initializers (see module doc:
    // literal-shaped classify from their own shape; variable/call/index-
    // valued ones consult `ctx`'s inference substrate).
    for (name, value) in &sl.fields {
        let Some(declared_ty) = shape.field_ty(&name.text) else {
            continue; // already flagged as an extra field above
        };
        if declared_ty.is_unresolved() {
            continue; // the field's own annotation didn't resolve (E061)
        }
        let Some(actual_ty) = classify_expr_ty(value, ctx) else {
            continue; // not classifiable — see module doc
        };
        // Row-insensitive (issue #1680): a `fn`-typed field's declared type
        // carries the top effect row and the initializer's carries its real
        // creation target, so rows must not decide this comparison — see
        // `infer::assignable`.
        if !crate::infer::assignable(declared_ty, &actual_ty) {
            out.push(Diagnostic {
                file,
                range: name.range,
                message: format!(
                    "{}: field `{}` declared `{}` but initialized with `{}`",
                    DiagnosticCode::E071.title(),
                    name.text,
                    declared_ty.display(),
                    actual_ty.display()
                ),
                code: DiagnosticCode::E071,
            });
        }
    }
}

/// Classify a struct-field initializer's type when it's statically obvious
/// from its own shape — literals, and (recursively) array/map/struct
/// literals. Anything else (a variable/call/index/…) returns `None` here;
/// [`classify_expr_ty`] is the entry point [`check_literal`] actually calls,
/// falling back to the inference-substrate classification for those forms
/// (issue #670) before finally treating an unclassifiable expression as
/// silently clean — the same "Unknown never disagrees" posture
/// `annotations::mismatches` takes.
fn literal_ty(expr: &Expr) -> Option<Ty> {
    match expr {
        Expr::Int(_) => Some(Ty::Int),
        Expr::Float(_) => Some(Ty::Float),
        Expr::Bool(_) => Some(Ty::Bool),
        Expr::String(s) => match s.parts.as_slice() {
            [] | [brink_ir::StringPart::Literal(_)] => Some(Ty::String),
            _ => None, // interpolated — not purely a literal
        },
        Expr::ArrayLiteral(a) => {
            let elems: Vec<Ty> = a.elements.iter().map(literal_ty).collect::<Option<_>>()?;
            Some(Ty::Array(Box::new(crate::infer::unify_all(elems))))
        }
        Expr::MapLiteral(m) => {
            let mut keys = Vec::with_capacity(m.entries.len());
            let mut vals = Vec::with_capacity(m.entries.len());
            for (k, v) in &m.entries {
                keys.push(literal_ty(k)?);
                vals.push(literal_ty(v)?);
            }
            Some(Ty::Map(
                Box::new(crate::infer::unify_all(keys)),
                Box::new(crate::infer::unify_all(vals)),
            ))
        }
        Expr::StructLiteral(sl) => Some(Ty::Struct(sl.shape.text.clone())),
        _ => None,
    }
}

/// Classify a struct-field initializer's type — [`literal_ty`]'s
/// literal-shaped classification first, falling back to the non-literal
/// forms issue #670 adds: a `Path` (variable), a `Call` (function), or an
/// `Index` expression, each resolved through `ctx`'s inference substrate.
/// `None` — "not classifiable" — whenever the resolved type is itself
/// `Unknown`/`Conflicted`, or the expression shape isn't handled at all
/// (e.g. a field access, an infix expression): the same "Unknown never
/// disagrees" posture [`literal_ty`] and `annotations::mismatches` both take.
///
/// `pub(crate)` (issue #983) — see [`MistypeCtx`]'s doc for why.
pub(crate) fn classify_expr_ty(expr: &Expr, ctx: &MistypeCtx<'_>) -> Option<Ty> {
    if let Some(ty) = literal_ty(expr) {
        return Some(ty);
    }
    match expr {
        Expr::Path(p) => resolved_symbol_ty(p.range, ctx),
        Expr::Call(path, _args) => {
            // Only a direct call to a known inferable knot/stitch is
            // classified here — a call through a function *value* is T1c's
            // own domain (`strict::check_value_calls`'s `ValueCallFact`s),
            // not this diagnostic's.
            let def = ctx.resolution_by_range.get(&range_key(path.range))?;
            let sig = ctx.signatures.get(def)?;
            (!sig.return_ty.is_unresolved()).then(|| sig.return_ty.clone())
        }
        Expr::Index(idx) => {
            let base_ty = classify_expr_ty(&idx.base, ctx)?;
            match base_ty {
                Ty::Array(elem) if !elem.is_unresolved() => Some(*elem),
                Ty::Map(_key, val) if !val.is_unresolved() => Some(*val),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Resolve a `Path` expression's own range to a concrete [`Ty`]: a
/// param/temp reads the enclosing def's finalized `BodyTypes::locals`
/// (`ctx.locals`, `None` at file scope — see [`MistypeCtx`]'s doc); a
/// global `VAR`/`CONST` reads `infer::collect_globals`'s declaration-derived
/// type; a `LIST`/list-item name is nominally `list<L>`. Mirrors
/// `infer::body::InferPass::ty_of_def`'s own dispatch exactly (the same
/// firewall a body's own inference already enforces), just read post hoc
/// from the finalized results instead of live during a body solve.
fn resolved_symbol_ty(range: TextRange, ctx: &MistypeCtx<'_>) -> Option<Ty> {
    let def = *ctx.resolution_by_range.get(&range_key(range))?;
    let info = ctx.index.symbols.get(&def)?;
    let ty = match info.kind {
        SymbolKind::Param | SymbolKind::Temp => ctx.locals?.get(&info.name)?.clone(),
        SymbolKind::Variable | SymbolKind::Constant => ctx.globals.get(&def)?.clone(),
        SymbolKind::List => Ty::List(info.name.clone()),
        SymbolKind::ListItem => {
            let (list, _item) = info.name.split_once('.')?;
            Ty::List(list.to_string())
        }
        SymbolKind::Knot
        | SymbolKind::Stitch
        | SymbolKind::External
        | SymbolKind::Struct
        | SymbolKind::Label => {
            return None;
        }
    };
    if ty.is_unresolved() { None } else { Some(ty) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;

    fn build(src: &str) -> (HirFile, SymbolIndex) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        (hir, (*index).clone())
    }

    /// Like [`build`], but also computes real resolutions and a whole-project
    /// [`InferenceResult`] — needed by every test exercising the non-literal
    /// (variable/call/index) classification issue #670 adds, since that path
    /// consults exactly this substrate.
    fn build_with_inference(src: &str) -> (HirFile, SymbolIndex, ResolutionMap, InferenceResult) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        (hir, (*index).clone(), (*resolutions).clone(), inference)
    }

    /// [`check`] driven by [`build_with_inference`]'s output — the harness
    /// every non-literal-classification test below shares.
    fn check_all(src: &str) -> Vec<Diagnostic> {
        let (hir, index, resolutions, inference) = build_with_inference(src);
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    /// [`build_with_inference`]'s native-surface twin. Lambdas exist only on
    /// the native surface, so the #1764 fixtures below must go through
    /// `lower_native` (the same reason `coalesce`'s `build_native` exists).
    fn build_native(src: &str) -> (HirFile, SymbolIndex, ResolutionMap, InferenceResult) {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, manifest, _diag) = brink_ir::hir::lower_native::lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        let inference = crate::infer_project(
            &[(FileId(0), &hir)],
            &index,
            &resolutions,
            None,
            &BTreeMap::new(),
        );
        (hir, (*index).clone(), (*resolutions).clone(), inference)
    }

    /// [`check`] over native source — [`check_all`]'s native twin.
    fn check_all_native(src: &str) -> Vec<Diagnostic> {
        let (hir, index, resolutions, inference) = build_native(src);
        check(&[(FileId(0), &hir)], &index, &inference, &resolutions)
    }

    #[test]
    fn clean_construction_produces_no_diagnostics() {
        let diags = check_all(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0, y: 2.0}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn missing_field_is_e069_naming_the_field() {
        let diags = check_all(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E069);
        assert!(diags[0].message.contains('y'), "{:?}", diags[0].message);
    }

    #[test]
    fn extra_field_is_e070_naming_the_field() {
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: 1.0, z: 2.0}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E070);
        assert!(diags[0].message.contains('z'), "{:?}", diags[0].message);
    }

    #[test]
    fn mistyped_field_is_e071_naming_the_field() {
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: \"hi\"}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
        assert!(diags[0].message.contains('x'), "{:?}", diags[0].message);
    }

    #[test]
    fn int_initializer_for_a_float_field_is_the_legal_coercion() {
        // §4's directional int -> float coercion applies here too.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: 1}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── issue #670: variable/call/index-valued initializers ────────────

    #[test]
    fn global_variable_valued_initializer_fires_when_provably_mistyped() {
        // `v`'s declaration-derived type is a concrete `string` (its own
        // literal initializer) — disagrees with `Point.x`'s declared
        // `float`, so this now fires exactly like a literal `"hi"` would.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             VAR v = \"hi\"\n=== main ===\n~ p = Point#{x: v}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
        assert!(diags[0].message.contains('x'), "{:?}", diags[0].message);
    }

    #[test]
    fn global_variable_valued_initializer_of_the_right_type_is_clean() {
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             VAR v = 1.0\n=== main ===\n~ p = Point#{x: v}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn param_variable_valued_initializer_fires_when_provably_mistyped() {
        // `n`'s only use in the body is compared against a string literal,
        // so its inferred `BodyTypes::locals` type is a concrete `string` —
        // disagrees with `Point.x`'s declared `float`.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main(n) ===\n\
             {n == \"a\":\n  yes\n}\n~ p = Point#{x: n}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn unused_param_variable_valued_initializer_stays_silent_when_unknown() {
        // `n` is never used anywhere else in the body, so it stays `Unknown`
        // — "Unknown never disagrees" holds even for a variable initializer.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main(n) ===\n~ p = Point#{x: n}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn call_valued_initializer_fires_when_provably_mistyped() {
        // `label()`'s only `~ return` is a string literal, so its
        // finalized `InferredSig::return_ty` is a concrete `string`.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === function label() ===\n~ return \"a\"\n\
             === main ===\n~ p = Point#{x: label()}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn call_valued_initializer_of_the_right_type_is_clean() {
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === function label() ===\n~ return 1.0\n\
             === main ===\n~ p = Point#{x: label()}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn index_valued_initializer_fires_when_provably_mistyped() {
        // `xs` is a local `~ temp` bound to a `#[...]` array-of-strings
        // literal, so its finalized locals type is `array<string>` — indexing
        // it yields `string`, disagreeing with `Point.x`'s declared `float`.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n\
             ~ temp xs = #[\"a\", \"b\"]\n~ p = Point#{x: xs[0]}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn index_valued_initializer_of_the_right_type_is_clean() {
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n\
             ~ temp xs = #[1.0, 2.0]\n~ p = Point#{x: xs[0]}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn index_valued_initializer_stays_silent_when_unknown() {
        // `xs` is only ever indexed, never assigned/observed to a concrete
        // type elsewhere — reading through an `Unknown` base never learns an
        // array shape (`infer::body`'s own `Expr::Index` arm), so this stays
        // silent rather than false-flagging.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === main(xs) ===\n~ p = Point#{x: xs[0]}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unresolved_shape_name_is_not_double_reported_here() {
        // No `STRUCT Bogus` declared — `resolve::resolve_struct_ref` already
        // reports E068 elsewhere; this pass has nothing to check against.
        let diags = check_all("=== main ===\n~ p = Bogus#{x: 1}\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn nested_struct_literal_field_is_checked_by_shape_name() {
        let diags = check_all(
            "STRUCT Inner = #{v: float}\nSTRUCT Outer = #{inner: Inner}\n\
             === main ===\n~ o = Outer#{inner: Inner#{v: 1.0}}\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn nested_struct_literal_mistyped_field_still_flags_outer() {
        let diags = check_all(
            "STRUCT Wrong = #{v: float}\nSTRUCT Inner = #{v: float}\nSTRUCT Outer = #{inner: Inner}\n\
             === main ===\n~ o = Outer#{inner: Wrong#{v: 1.0}}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn struct_literal_inside_var_initializer_is_checked() {
        let diags = check_all("STRUCT Point = #{x: float}\nVAR p = Point#{x: \"hi\"}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn variable_valued_initializer_inside_var_initializer_uses_global_scope_only() {
        // A struct literal in a file-level VAR/CONST initializer has no
        // enclosing knot/stitch body — only a reference to *another* global
        // is classifiable there (never a param/temp, which can't exist at
        // file scope). `other`'s declared type disagrees with `Point.x`.
        let diags =
            check_all("STRUCT Point = #{x: float}\nVAR other = \"hi\"\nVAR p = Point#{x: other}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn stitch_local_variable_valued_initializer_fires_when_provably_mistyped() {
        // Every other non-literal-classification test above only ever
        // exercises knot scope (`main`), file scope, or `main(n)`'s own
        // params — never a stitch body. This drives the `enter_stitch`/
        // `stitch_locals` dispatch path specifically: `t`'s finalized
        // `BodyTypes::locals` type (a concrete `string`, from its own
        // literal initializer) disagrees with `Point.x`'s declared `float`.
        let diags = check_all(
            "STRUCT Point = #{x: float}\n\
             === room ===\n= inside\n~ temp t = \"hi\"\n~ p = Point#{x: t}\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
        assert!(diags[0].message.contains('x'), "{:?}", diags[0].message);
    }

    #[test]
    fn mistyped_variable_field_diagnostic_is_order_independent() {
        // Issue #670's own scope names "order-independence property tests
        // per the #627 discipline" as a deliverable — mirrors strict.rs's
        // `escape_diagnostics_are_order_independent` forward/reversed
        // pattern. `v`'s mistyped classification (and the resulting E071)
        // must not depend on which position its field initializer occupies
        // in the literal.
        let forward = "STRUCT Point = #{x: float, y: float}\n\
             VAR v = \"hi\"\n=== main ===\n~ p = Point#{x: v, y: 1.0}\n-> DONE\n";
        let reversed = "STRUCT Point = #{x: float, y: float}\n\
             VAR v = \"hi\"\n=== main ===\n~ p = Point#{y: 1.0, x: v}\n-> DONE\n";

        let diags_f = check_all(forward);
        let diags_r = check_all(reversed);

        assert_eq!(diags_f.len(), 1, "{diags_f:?}");
        assert_eq!(diags_f[0].code, DiagnosticCode::E071);
        assert!(diags_f[0].message.contains('x'), "{:?}", diags_f[0].message);

        assert_eq!(diags_r.len(), 1, "{diags_r:?}");
        assert_eq!(diags_r[0].code, DiagnosticCode::E071);
        assert!(diags_r[0].message.contains('x'), "{:?}", diags_r[0].message);
    }

    // ─── check_duplicates (E084, issue #675) ──────────────────────────

    #[test]
    fn duplicate_field_is_e084_naming_the_field() {
        let (hir, _index) = build(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0, x: 2.0, y: 3.0}\n-> DONE\n",
        );
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E084);
        assert!(diags[0].message.contains('x'), "{:?}", diags[0].message);
    }

    #[test]
    fn duplicate_field_points_at_the_repeated_occurrence_not_the_first() {
        let src =
            "STRUCT Point = #{x: float}\n=== main ===\n~ p = Point#{x: 1.0, x: 2.0}\n-> DONE\n";
        let (hir, _index) = build(src);
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        let second_x = src.rfind("x: 2.0").expect("second x initializer");
        assert_eq!(usize::from(diags[0].range.start()), second_x);
    }

    #[test]
    fn clean_construction_has_no_duplicate_diagnostic() {
        let (hir, _index) = build(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0, y: 2.0}\n-> DONE\n",
        );
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn duplicate_field_flagged_even_under_gradual_and_unresolved_shape() {
        // No `types = strict` context needed here (this build() harness
        // never runs `strict::check`'s gate) — and the shape name doesn't
        // even need to resolve, unlike `check`'s missing/extra/mistyped
        // trio: a repeated field name is a mistake regardless.
        let (hir, _index) = build("=== main ===\n~ p = Bogus#{x: 1, x: 2}\n-> DONE\n");
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E084);
    }

    #[test]
    fn duplicate_field_inside_var_initializer_is_checked() {
        let (hir, _index) = build("STRUCT Point = #{x: float}\nVAR p = Point#{x: 1.0, x: 2.0}\n");
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E084);
    }

    // ─── issue #1764: a lambda's statements in a VAR/CONST initializer ──

    /// The VAR/CONST-initializer recursion is the one walk that isn't
    /// `visit::visit`'s (which already descends a lambda's statements), so it
    /// has to descend them itself. A block-bodied lambda's `let` is a
    /// statement, not the body's value expression.
    #[test]
    fn a_duplicate_field_in_a_lambda_statement_of_a_var_initializer_is_reported() {
        let (hir, _index, _res, _inf) = build_native(
            "struct Point { x: float }\nvar f = ||: int {\n  let p = Point { x: 1.0, x: 2.0 };\n  0\n};\n",
        );
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E084);
    }

    /// The shape-agreement trio reaches the same position — a literal-valued
    /// initializer classifies without any locals, so `MistypeCtx::locals =
    /// None` is no obstacle here.
    #[test]
    fn a_mistyped_field_in_a_lambda_statement_of_a_var_initializer_is_reported() {
        let diags = check_all_native(
            "struct Point { x: float }\nvar f = ||: int {\n  let p = Point { x: \"hi\" };\n  0\n};\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
        assert!(diags[0].message.contains('x'), "{:?}", diags[0].message);
    }

    /// The tail position was already covered — pinned so a later refactor
    /// can't trade one half of the body for the other.
    #[test]
    fn a_mistyped_field_in_a_lambda_tail_of_a_var_initializer_is_still_reported() {
        let diags = check_all_native(
            "struct Point { x: float }\nvar f = ||: Point {\n  let a = 1;\n  Point { x: \"hi\" }\n};\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn three_way_duplicate_flags_every_repeat_after_the_first() {
        let (hir, _index) = build(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: 1.0, x: 2.0, x: 3.0}\n-> DONE\n",
        );
        let diags = check_duplicates(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E084));
    }
}
