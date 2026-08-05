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
    AssignOp, Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, ResolutionMap, Stitch,
    StructLiteral, SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{
    FieldAssignMismatch, InferenceResult, InferredSig, Ty, is_string_numeric_concat,
};
use crate::resolve::ImportScope;

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

/// Every declared `STRUCT` shape in the project — a referrer-scoped lookup
/// table (issue #2241).
///
/// A bare struct name is **not** a unique key: the stdlib mount (#2080) lets
/// a project's own `struct Cue { … }` coexist with a same-named
/// `struct Cue { … }` a mounted std preset declares (M-2d module
/// coexistence, `manifest::is_cross_declared_module_collision`) — both are
/// genuinely distinct `Struct` symbols with distinct `DefinitionId`s, the
/// same shape `brink_ir::lir::lower::structs::ShapeTable` already handles one
/// layer down (issue #2238). This table used to be a flat
/// `BTreeMap<String, ShapeInfo>` populated by plain last-`insert`-wins —
/// whichever file's declaration was iterated last silently overwrote every
/// earlier same-named one, with no per-caller scope to break the tie.
/// [`ShapeTable::resolve`] is the fix: every caller with an [`ImportScope`]
/// in hand resolves the *right* candidate through the same
/// `Candidacy`-based module scoping [`crate::resolve::lookup_by_name`]
/// already applies to every other symbol kind — instead of a global winner
/// or a second, diverging std-exclusion policy (2026-08-04 peer-root
/// ruling, `docs/decision-log.md`). [`ShapeTable::get_by_def`] is for
/// callers that already hold an exact `DefinitionId` (e.g. a construction
/// literal's shape name, resolved with full module-scope `Candidacy` by
/// `resolve::resolve_struct_ref` and recorded in the project's
/// `ResolutionMap` — see [`check`]).
///
/// Public (issue #858) so tooling outside `brink-analyzer` can resolve a
/// `STRUCT`'s declared fields — e.g. offering field-name completions after
/// `npc.` in a `ref lvalue-path` — without re-deriving the shape table this
/// crate already builds for its own construction-literal checks
/// ([`check`]) and `ref`-projection path-segment validation.
#[must_use]
pub fn declared_shapes(files: &[(FileId, &HirFile)], index: &SymbolIndex) -> ShapeTable {
    // No manifest access at this call site (`structs::check` isn't
    // threaded a `HostManifest` — struct field types aren't in T1d-2's
    // scope), so `Handle<K>` field types resolve `None` here, same as any
    // other name `TypeNames` doesn't recognize — consistent with
    // `annotations::resolve`'s documented "unresolved -> silent" contract.
    let names = annotations::TypeNames::new(index, None);
    let mut by_def = BTreeMap::new();
    for &(file, hir) in files {
        for s in &hir.structs {
            // NOT actually an invariant (review finding on #2240/#2258):
            // `annotations::def_id_for` is exact-file-only, with no
            // fallback arm at all — unlike `lir::lower::structs`'
            // `decls::lookup_global`, which at least rescues a surviving
            // non-std sibling before giving up. So this lookup misses on
            // *every* true intra-module duplicate this file's own
            // declaration lost to (`E023` dropped its symbol-index entry),
            // not only the narrower std-declared-survivor case `E181`
            // reports one layer down. When it misses, this decl silently
            // contributes nothing to `by_def` — a fourth, still-undiagnosed
            // silent-drop site of the exact class `E181` exists to make
            // loud (see that code's own doc and `build_shape_table`'s),
            // just with no diagnostic sink wired here yet.
            let Some(def_id) =
                annotations::def_id_for(index, file, SymbolKind::Struct, &s.name.text)
            else {
                continue;
            };
            if by_def.contains_key(&def_id) {
                continue;
            }
            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let ty = annotations::resolve(&f.ty, &names).unwrap_or(Ty::Unknown);
                    (f.name.text.clone(), ty)
                })
                .collect();
            by_def.insert(def_id, ShapeInfo { fields });
        }
    }
    ShapeTable { by_def }
}

/// [`declared_shapes`]' referrer-scoped lookup table — see that function's
/// doc for the coexistence story this exists to resolve correctly.
#[derive(Default)]
pub struct ShapeTable {
    /// Every shape by its own symbol-index identity — the canonical store,
    /// unambiguous by construction.
    by_def: BTreeMap<DefinitionId, ShapeInfo>,
}

impl ShapeTable {
    /// Number of declared shapes in the project — referrer-free, since a
    /// count needs no disambiguation.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_def.len()
    }

    /// Whether the project declares no `STRUCT` shapes at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_def.is_empty()
    }

    /// Resolve a shape already pinned to an exact `DefinitionId` — no
    /// referrer ambiguity possible, since the identity was already resolved
    /// once, correctly, by whatever recorded it (e.g. a construction
    /// literal's `RefKind::Struct` resolution, `resolve::resolve_struct_ref`).
    #[must_use]
    pub fn get_by_def(&self, id: DefinitionId) -> Option<&ShapeInfo> {
        self.by_def.get(&id)
    }

    /// Scope-aware lookup (issue #2241, corrected per #2245/#2246's
    /// peer-root ruling — `docs/decision-log.md`, 2026-08-04): when more
    /// than one declared `STRUCT` shares `name`, resolve through the same
    /// `Candidacy`-based module scoping every other symbol kind uses —
    /// [`crate::resolve::lookup_by_name`], the exact function
    /// `resolve::resolve_struct_ref` already calls for `SymbolKind::Struct`.
    /// This used to hand-roll its own `find(info.file == referrer)
    /// .or_else(find(!is_std_module))` fallback — a bolt-on std gate the
    /// ruling calls out by name as one of the five symptom gates to unwind,
    /// not a second, diverging implementation of the same policy. Returns
    /// `None` when `name` names no declared `STRUCT` at all, or
    /// [`crate::resolve::lookup_by_name`] itself resolves to none (e.g.
    /// every candidate sharing the name is std-declared and out of
    /// `scope`).
    #[must_use]
    pub fn resolve(
        &self,
        name: &str,
        scope: &ImportScope,
        index: &SymbolIndex,
    ) -> Option<&ShapeInfo> {
        let def_id = crate::resolve::lookup_by_name(index, scope, name, &[SymbolKind::Struct])?;
        self.by_def.get(&def_id)
    }
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
    // — a global's own annotation resolving against `Handle<K>` isn't in
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
        // Issue #2098: `ConstructionVisitor::enter_expr` has no state that
        // needs resetting between the block tree and a file-level
        // declaration's own initializer (`locals` is already `None` at this
        // scope) — so the shared entry point covers both in one drive, and
        // the hand-rolled `check_expr`/`expr_children` mirror of
        // `visit::visit`'s own descent this used to need is gone.
        visit::visit_with_decl_initializers(hir, &mut v);
    }
    out
}

// ─── Issue #1900: plain struct-field assignment target checking ──────

/// Strict-mode-only: every [`crate::infer::FieldAssignMismatch`] fact body
/// inference recorded (`~ p.x = expr`, a dotted assignment target — see that
/// type's own doc for why the field chain is left unresolved until now),
/// walked against [`declared_shapes`]/[`ShapeInfo`] to resolve the specific
/// field's declared type and reported as `E063` — the same code
/// `strict::check_typed_assign_mismatches` reports for a *bare* assignment
/// target (issue #1877); this is that check's dotted sibling, split into
/// its own issue (#1900) because the root's declared type is not the
/// field's, so the bare-name comparison doesn't apply as-is. Callers only
/// reach this once `strict::config_error` has confirmed `types = strict` +
/// `dialect = brink` (mirrors [`check`]'s own entry condition).
///
/// Walks `inference.bodies` directly (keyed by `DefinitionId`, itself
/// `Ord`) rather than re-deriving a per-file `def_ids` list the way
/// `strict::check_typed_assign_mismatches` does — every fact already
/// carries its own diagnostic range, so grouping by file first buys nothing
/// extra here. Correction (issue #1900 review finding): the caller
/// (`strict::check`) does *not* sort this aggregate — `strict::check` and
/// `strict_diagnostics` only concatenate each check's output in a fixed
/// call order, with no sort in either. The only downstream ordering is
/// `brink_db::queries::mod::partition_diagnostics` grouping by `FileId` for
/// the salsa query path; the pure `analyze_with_options` path has no
/// ordering step at all. Iterating `inference.bodies` (`Ord`-keyed by
/// `DefinitionId`) still makes this function's own output deterministic —
/// just not because anything downstream re-sorts it.
#[must_use]
pub fn check_assignments(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<Diagnostic> {
    let shapes = declared_shapes(files, index);
    // Per-file scope, keyed the same way `resolve::resolve` and `ufcs::resolve`
    // build one per file — `check_field_assign_mismatch` doesn't loop `files`
    // itself (it's driven by `inference.bodies`, keyed by `DefinitionId`), so
    // the scope for the fact's own declaring file is looked up here instead.
    let scopes: BTreeMap<FileId, ImportScope> = files
        .iter()
        .map(|&(file, hir)| {
            let scope = ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
            (file, scope)
        })
        .collect();
    let mut out = Vec::new();
    for (def, body) in &inference.bodies {
        let Some(info) = index.symbols.get(def) else {
            continue;
        };
        let Some(scope) = scopes.get(&info.file) else {
            continue;
        };
        for fact in &body.field_assign_mismatches {
            check_field_assign_mismatch(fact, info.file, scope, index, &shapes, &mut out);
        }
    }
    out
}

/// Walk one [`FieldAssignMismatch`]'s field chain from its already-resolved
/// root type down to the specific field being assigned, comparing the
/// result against the RHS's own type. Silently unclassifiable (no
/// diagnostic) whenever the walk hits a non-`Struct` type, an unresolved
/// shape name (`E068` already covers that separately), or a field name the
/// shape doesn't declare (not this check's job — "Unknown never disagrees",
/// the same posture every other shape-agreement check in this module and
/// `ref_projection`'s E098 take) — matching [`check_literal`]'s own
/// "unresolved -> silent" contract for the same reasons.
///
/// BLOCKING review finding (issue #1900): the `+=` string-numeric
/// display-concat carve-out (issue #1911, `body::is_string_numeric_concat`)
/// applies to a dotted target exactly like it applies to a bare one — `~
/// v.s += 5` on a `string`-declared field desugars to the identical runtime
/// `String`/`Int`|`Float` `Add` arm as `~ v.s = v.s + 5` — but body
/// inference can't decide that carve-out itself: it only ever resolves the
/// *root's* type (`Ty::Struct("S")`, never `string`) when it records the
/// fact, well before the field's own declared type is known. So the
/// carve-out has to be re-applied here, once `current` has been walked all
/// the way down to the field's actual declared type.
fn check_field_assign_mismatch(
    fact: &FieldAssignMismatch,
    file: FileId,
    scope: &ImportScope,
    index: &SymbolIndex,
    shapes: &ShapeTable,
    out: &mut Vec<Diagnostic>,
) {
    let mut current = fact.root_ty.clone();
    for segment in &fact.path {
        let Ty::Struct(shape_name) = &current else {
            return;
        };
        let Some(shape) = shapes.resolve(shape_name, scope, index) else {
            return;
        };
        let Some(field_ty) = shape.field_ty(&segment.text) else {
            return;
        };
        current = field_ty.clone();
    }
    if current.is_unresolved() || crate::infer::assignable(&current, &fact.found) {
        return;
    }
    // Issue #1911's carve-out, re-applied here (BLOCKING review finding,
    // issue #1900) now that `current` is the field's own resolved declared
    // type, not the root's — see this function's own doc.
    if fact.op == AssignOp::Add && is_string_numeric_concat(&current, &fact.found) {
        return;
    }
    // `path` is never empty by construction (the recording site only ever
    // records a fact for a multi-segment target — `segments[1..]` is
    // therefore non-empty), but a defensive `None` here (rather than
    // `expect`, denied in production code) just silently skips the
    // diagnostic instead of panicking if that invariant ever changes.
    let Some(last) = fact.path.last() else {
        return;
    };
    let dotted: Vec<&str> = std::iter::once(fact.root.as_str())
        .chain(fact.path.iter().map(|n| n.text.as_str()))
        .collect();
    out.push(Diagnostic {
        file,
        range: last.range,
        message: format!(
            "`{}` has type `{}` but its declared type is `{}`",
            dotted.join("."),
            fact.found.display(),
            current.display()
        ),
        code: DiagnosticCode::E063,
    });
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
    shapes: &'a ShapeTable,
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
        // Issue #2098: `DuplicateFieldVisitor::enter_expr` carries no
        // per-position state at all, so the shared entry point covers the
        // block tree and every file-level declaration's own initializer in
        // one drive — the hand-rolled `check_duplicates_expr`/`expr_children`
        // mirror of `visit::visit`'s own descent this used to need is gone.
        visit::visit_with_decl_initializers(hir, &mut v);
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

/// Check one struct literal against its declared shape (if resolvable — an
/// unresolved shape name has nothing to check against, and is already
/// diagnosed separately by `resolve::resolve_struct_ref`'s `E068`).
///
/// Issue #2241: `sl.shape`'s own name is a `RefKind::Struct` reference the
/// analyzer already resolved with full module-scope `Candidacy`
/// (`resolve::resolve_struct_ref`, walked into the `ResolutionMap` every
/// construction literal's shape name gets — `symbols::project`'s `walk_expr`
/// registers one for every `Expr::StructLiteral`, unconditionally). Consuming
/// that recorded resolution by range (`ctx.resolution_by_range`) rather than
/// re-deriving the shape from `sl.shape.text` by bare name is exactly the
/// "lowering consumes analyzer types" fix PR #2248 already applied on the LIR
/// side for this same reference kind — this is its analyzer-side twin. A
/// missing entry here means `resolve_struct_ref` itself couldn't resolve the
/// name (already reported as `E068`), so there is nothing to check against,
/// same as before.
fn check_literal(
    sl: &StructLiteral,
    file: FileId,
    shapes: &ShapeTable,
    ctx: &MistypeCtx<'_>,
    out: &mut Vec<Diagnostic>,
) {
    let Some(shape) = ctx
        .resolution_by_range
        .get(&range_key(sl.shape.range))
        .and_then(|def_id| shapes.get_by_def(*def_id))
    else {
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
/// type; a `LIST`/list-item name is nominally `List<L>`. Mirrors
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
        // literal, so its finalized locals type is `Array<string>` — indexing
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

    // ─── check_assignments (E063, issue #1900) ─────────────────────────

    /// [`check_assignments`] driven by [`build_with_inference`]'s output —
    /// mirrors [`check_all`] for the plain-assignment sibling check.
    fn check_assignments_all(src: &str) -> Vec<Diagnostic> {
        let (hir, index, _resolutions, inference) = build_with_inference(src);
        check_assignments(&[(FileId(0), &hir)], &index, &inference)
    }

    #[test]
    fn field_assignment_mismatch_on_var_is_e063_naming_the_dotted_target() {
        // The issue's own repro: `p.x`'s declared `float` disagrees with the
        // RHS `string`.
        let diags = check_assignments_all(
            "STRUCT Point = #{x: float, y: float}\n\
             VAR p: Point = Point#{x: 0.0, y: 0.0}\n\
             === main ===\n~ p.x = \"wrong\"\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
        assert!(diags[0].message.contains("p.x"), "{:?}", diags[0].message);
    }

    #[test]
    fn field_assignment_of_the_declared_type_is_clean() {
        let diags = check_assignments_all(
            "STRUCT Point = #{x: float, y: float}\n\
             VAR p: Point = Point#{x: 0.0, y: 0.0}\n\
             === main ===\n~ p.x = 1.0\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn field_assignment_int_initializer_for_a_float_field_is_the_legal_coercion() {
        // §4's directional int -> float coercion applies here too, same as
        // `int_initializer_for_a_float_field_is_the_legal_coercion` above.
        let diags = check_assignments_all(
            "STRUCT Point = #{x: float}\nVAR p: Point = Point#{x: 0.0}\n\
             === main ===\n~ p.x = 1\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn field_assignment_mismatch_on_annotated_temp_is_e063() {
        // The second of the issue's two named root sources: an annotated `~
        // temp`'s own ascription, not a global.
        let diags = check_assignments_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ temp p: Point = Point#{x: 0.0}\n~ p.x = \"wrong\"\n-> DONE\n",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
    }

    #[test]
    fn field_assignment_on_unannotated_temp_stays_silent_when_unknown() {
        // An unannotated `~ temp` has no declared shape to check against —
        // "Unknown never disagrees", same posture every other shape-
        // agreement check in this module takes.
        let diags = check_assignments_all(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ temp p = Point#{x: 0.0}\n~ p.x = \"wrong\"\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn field_assignment_to_a_nonexistent_field_stays_silent() {
        // A field name the shape doesn't declare isn't this check's job —
        // an unresolvable classification, "Unknown never disagrees".
        let diags = check_assignments_all(
            "STRUCT Point = #{x: float}\n\
             VAR p: Point = Point#{x: 0.0}\n\
             === main ===\n~ p.bogus = \"wrong\"\n-> DONE\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn bare_var_assignment_is_not_double_reported_by_check_assignments() {
        // A single-segment target is `check_declared_assign_target`'s job
        // (issue #1877 / E063 via `strict::check_typed_assign_mismatches`),
        // never this dotted-target check's — `check_assignments` must stay
        // silent for it (no double-report across the two checks).
        let diags = check_assignments_all("VAR v: int = 5\n=== main ===\n~ v = \"hi\"\n-> DONE\n");
        assert!(diags.is_empty(), "{diags:?}");
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

    /// Coverage for a lambda's statements in a VAR/CONST initializer comes
    /// from `visit::visit_with_decl_initializers` (which reaches the
    /// initializer at all) composed with `walk_expr`'s `Expr::Lambda` arm
    /// (which already descends a lambda's statements) — there is no
    /// separate hand-rolled recursion for this position (issue #2098). A
    /// block-bodied lambda's `let` is a statement, not the body's value
    /// expression.
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

    // ─── issue #2241: declared_shapes is referrer-scoped, not last-wins ──

    /// Build a project file coexisting with a "std"-shaped file (M-2d
    /// cross-declared-module coexistence, mirroring the real stdlib mount
    /// #2080) — each declares its own `STRUCT Cue`, with the project's own
    /// carrying MORE fields than the coexisting file's same-named one.
    /// Returns everything [`check`] needs to validate the project's own
    /// construction literal.
    ///
    /// No `#@module` directive in either source: the module tag a real
    /// compile derives from `#@module`/the native path is supplied directly
    /// here via `ModuleMap`, exactly as `manifest::tests`'s own
    /// `cross_declared_module_duplicate_coexists_under_brink` does — this
    /// test only needs the tag on the index, not the parsed source's own
    /// (irrelevant) `HirFile::module`.
    fn build_project_with_std_homonym(
        project_src: &str,
        std_src: &str,
    ) -> (
        FileId,
        HirFile,
        FileId,
        HirFile,
        SymbolIndex,
        ResolutionMap,
        InferenceResult,
    ) {
        let project_file = FileId(0);
        let std_file = FileId(1);

        let project_parsed = brink_syntax::parse(project_src);
        let (project_hir, project_manifest, _diag) = lower(project_file, &project_parsed.tree());
        let std_parsed = brink_syntax::parse(std_src);
        let (std_hir, std_manifest, _diag) = lower(std_file, &std_parsed.tree());

        let mut modules = crate::ModuleMap::new();
        modules.insert(
            project_file,
            crate::ResolvedModule {
                name: "story::main".to_string(),
                declared: true,
                was: None,
            },
        );
        modules.insert(
            std_file,
            crate::ResolvedModule {
                name: "std::conventions::screenplay".to_string(),
                declared: true,
                was: None,
            },
        );

        let (index, diag) = crate::symbol_index_with_modules(
            &[(project_file, &project_manifest), (std_file, &std_manifest)],
            &modules,
            crate::Dialect::Brink,
            false,
        );
        assert!(
            diag.is_empty(),
            "cross-declared-module `Cue`s must coexist with no diagnostic: {diag:?}"
        );

        let project_scope =
            crate::ImportScope::new(Some("story::main".to_string()), &project_hir.imports);
        let (project_resolutions, _diag) =
            crate::resolve(project_file, &project_manifest, &index, &project_scope);
        let std_scope = crate::ImportScope::new(
            Some("std::conventions::screenplay".to_string()),
            &std_hir.imports,
        );
        let (std_resolutions, _diag) = crate::resolve(std_file, &std_manifest, &index, &std_scope);

        let mut resolutions: ResolutionMap = (*project_resolutions).clone();
        resolutions.extend((*std_resolutions).iter().cloned());

        let files = [(project_file, &project_hir), (std_file, &std_hir)];
        let inference = crate::infer_project(&files, &index, &resolutions, None, &BTreeMap::new());

        (
            project_file,
            project_hir,
            std_file,
            std_hir,
            (*index).clone(),
            resolutions,
            inference,
        )
    }

    /// The wave's own headline scenario: a project's own `STRUCT Cue`
    /// coexists with a same-named `STRUCT Cue` from a distinct declared
    /// module (mirrors `std/conventions/screenplay.brink`'s real one-field
    /// `Cue`). Before this fix, `declared_shapes` built a flat
    /// `BTreeMap<String, ShapeInfo>` via plain last-`insert`-wins — with
    /// `files` ordered `[project, std]` below, std's `ShapeInfo` is inserted
    /// LAST and silently overwrites the project's own in the table, even
    /// though the construction literal itself lives in the project file and
    /// `resolve::resolve_struct_ref` already resolves it to the PROJECT's own
    /// `Cue` (never std's — the referrer's own module wins that tie-break).
    /// The missing-field check would then validate against std's one-field
    /// shape, which the literal's sole `speaker` initializer already
    /// satisfies — a silent E069 false negative: "accepted when it should
    /// error" (issue #2241's own words).
    ///
    /// Rule 20a: verified this test FAILS on the pre-fix code (reverting
    /// `declared_shapes` to the flat bare-name `BTreeMap::insert` and
    /// `check_literal` to `shapes.get(&sl.shape.text)`) — the assertion
    /// below (`diags.len() == 1`, `E069` naming `voiceover`) fails with
    /// `diags` empty instead, because std's one-field shape (which won the
    /// last-insert race with `files = [project, std]`) sees the literal's
    /// sole `speaker` field as complete.
    #[test]
    fn construction_check_resolves_the_referrers_own_shape_when_std_and_project_share_a_name() {
        let project_src = "STRUCT Cue = #{speaker: string, voiceover: string}\n\
             === main ===\n~ p = Cue#{speaker: \"A\"}\n-> DONE\n";
        let std_src = "STRUCT Cue = #{speaker: string}\nHello.\n";

        let (project_file, project_hir, std_file, std_hir, index, resolutions, inference) =
            build_project_with_std_homonym(project_src, std_src);

        // Deliberately `[project, std]` — std's shape is inserted LAST into
        // the pre-fix flat table, exposing the last-wins bug.
        let files = [(project_file, &project_hir), (std_file, &std_hir)];
        let diags = check(&files, &index, &inference, &resolutions);

        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E069);
        assert!(
            diags[0].message.contains("voiceover"),
            "the missing-field diagnostic must name the PROJECT's own missing field \
             (`voiceover`), proving the check validated the literal against the project's own \
             2-field `Cue` shape rather than the coexisting file's 1-field one: {diags:?}"
        );
    }

    /// F2 review finding (#2253): [`ShapeTable::resolve`] is the path
    /// [`check_assignments`]/[`check_field_assign_mismatch`] (E063) uses —
    /// unlike [`check`]/[`check_literal`] above, which never calls
    /// `resolve` at all (it goes through [`ShapeTable::get_by_def`] with an
    /// identity already resolved by `resolve::resolve_struct_ref`). This
    /// exercises the multi-candidate branch `resolve` exists to handle,
    /// through its own dedicated consumer rather than a proxy.
    ///
    /// Project and std each declare their own `STRUCT Cue`, deliberately
    /// with *different* declared types for the same field name (`x: float`
    /// vs `x: string`) so a wrong resolution doesn't just report the wrong
    /// message — it silently reports NOTHING: assigning the string
    /// `"wrong"` to `p.x` disagrees with the project's own `float`, but
    /// would agree with std's `string`. If `resolve` ever picked std's
    /// `Cue` for a reference inside the project file, this regresses to an
    /// empty `diags` exactly like the pre-fix last-insert-wins bug did for
    /// E069 above.
    ///
    /// Rule 20a: verified this test FAILS (empty `diags` instead of one
    /// `E063`) against `ShapeTable::resolve` reverted to always return the
    /// std candidate (i.e. simulating a resolution that ignores the
    /// referrer's own module) — restored before committing.
    #[test]
    fn check_assignments_resolves_the_referrers_own_shape_when_std_and_project_share_a_name() {
        let project_src = "STRUCT Cue = #{x: float}\n\
             VAR p: Cue = Cue#{x: 0.0}\n\
             === main ===\n~ p.x = \"wrong\"\n-> DONE\n";
        let std_src = "STRUCT Cue = #{x: string}\nHello.\n";

        let (project_file, project_hir, std_file, std_hir, index, _resolutions, inference) =
            build_project_with_std_homonym(project_src, std_src);

        let files = [(project_file, &project_hir), (std_file, &std_hir)];
        let diags = check_assignments(&files, &index, &inference);

        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
        assert!(
            diags[0].message.contains("float"),
            "the mismatch must be reported against the PROJECT's own `float`-declared `x`, not \
             std's `string`-declared one — which would silently accept the identically-typed \
             \"wrong\" RHS and produce zero diagnostics: {diags:?}"
        );
    }
}
