//! TM-4c struct-shape bookkeeping (`docs/typed-mode-spec.md` §6).
//!
//! Two pieces of whole-program, read-only data built once in
//! `lower_to_program`, before any container is lowered:
//!
//! - [`ShapeTable`]: every declared `STRUCT`, with a dense `ShapeId` (`u32`)
//!   assigned in `files` order (already topological) then source
//!   declaration order within a file — deterministic, never derived from
//!   `HashMap` iteration (`CLAUDE.md`'s determinism rule). Feeds
//!   `Expr::StructLiteral` construction (needs the shape id + field order to
//!   reorder initializers) and `lir::Program::struct_shapes` (the format's
//!   `StructShapes` table).
//!
//!   **Two same-named shapes can coexist** (issue #2238): once
//!   `brink_environment`'s stdlib mount (#2080) puts, say, a project's own
//!   `struct Cue { … }` alongside `std/conventions/screenplay.brink`'s own
//!   same-named `struct Cue { … }`, M-2d module coexistence
//!   (`brink-analyzer::manifest::is_cross_declared_module_collision`) lets
//!   both stand in the symbol index as genuinely distinct `Struct` symbols
//!   (different declared modules, different `DefinitionId`s) — the same
//!   shape the table already has for knots/externals since #2197. `by_name`
//!   keys a *bucket* of `DefinitionId`s per bare name instead of a single
//!   winner, disambiguated per reference kind:
//!
//!   - A construction literal's own shape name (issue #2246) and a **field
//!     or TM-2/temp type annotation** (issue #2249) are both `RefKind`
//!     references the analyzer already resolves with full module-scope
//!     `Candidacy` semantics (`brink_analyzer::resolve::resolve_struct_ref`/
//!     `resolve_type_ref`, into the same `ResolutionMap`) — `expr::
//!     lower_struct_literal`/`decls::eval_const_struct_literal` (the
//!     literal) and [`build_shape_table`]'s field loop /
//!     [`record_global_annotation`] / `context::LowerCtx::
//!     record_temp_annotation` (the annotation cases) all consume that
//!     recorded resolution directly via a `ResolutionLookup`, never
//!     re-deriving it. Before #2249, a field/annotation name had no HIR
//!     reference registered at all (`symbols::project`'s own prior doc: "a
//!     nominal-only grammar, resolved later by a different mechanism") — a
//!     now-deleted `ShapeTable::resolve` was that "different mechanism",
//!     the one brink-ir-side primitive re-implementing referrer scoping
//!     and std-exclusion on its own, routed through `decls::lookup_global`
//!     even for a sole-candidate bucket (issue #2246 review closed a gap
//!     where a prior "fast path" skipped that exclusion entirely). Once
//!     both real callers moved to the analyzer's own resolution, it had
//!     **no production caller left** and was removed — its std-exclusion
//!     property is now enforced by the analyzer's own
//!     `lookup_by_name`/`ImportScope` machinery instead, a genuinely
//!     different (and, per issue #2233's tracked asymmetry, not-yet-
//!     reconciled) implementation of the same "referrer can't reach an
//!     unimported std sibling" property. [`ShapeTable::get`] remains, test-
//!     only, as a thin by-name lookup for assertions that don't care about
//!     referrer scoping at all.
//!
//!   A bona fide intra-module duplicate (analyzer-diagnosed `E023`, later
//!   declaration dropped from the index) still keeps only its first
//!   declaration's fields here, because both HIR decls resolve to the
//!   *same* `DefinitionId` and the second is skipped once that id already
//!   has an entry — unless every surviving same-name candidate is
//!   std-declared, in which case [`build_shape_table`]'s own
//!   `decls::lookup_global` call comes back `None` and raises the `E181`
//!   backstop instead of silently dropping the declaration (issue #2240;
//!   see that code's own doc, and [`build_struct_shape_data`]'s doc for why
//!   its identical lookup does not duplicate the diagnostic).
//! - [`GlobalShapeMap`]: every global `VAR`/`CONST` whose TM-2 type
//!   annotation names a declared struct, resolved **once, at declaration
//!   time** (using the global's own declaring file as referrer) to that
//!   shape's `DefinitionId` — not a bare name re-resolved at every read
//!   site, which is exactly the ambiguity #2238 closes. This — plus the
//!   equivalent per-temp tracking `LowerCtx::temp_shapes` does as
//!   declarations are lowered — is the *entire* "compile-time known shape"
//!   story `expr::known_shape` uses to decide `RecordGet`/`RecordSet`
//!   (static offset) vs. `RecordGetDyn`/`RecordSetDyn` (by name)
//!   eligibility. Deliberately conservative: this is annotation-driven, not
//!   general type inference (`brink-ir` cannot depend on `brink-analyzer`'s
//!   inference queries — see the TM-4c PR description's scope note) — an
//!   unannotated variable's shape is never statically known here, even
//!   under `types = strict`, and always falls back to the by-name ops,
//!   which are correct under every policy.

use brink_format::{DefinitionId, NameId};

use crate::FileId;
use crate::determinism::{LookupMap, LookupSet};
use crate::hir;
use crate::symbols::{ResolutionMap, SymbolIndex, SymbolKind};
use crate::{Diagnostic, DiagnosticCode};

use super::context::{NameTable, ResolutionLookup};
use super::decls::lookup_global;
use super::lir;

/// One declared `STRUCT` shape, resolved for lowering.
///
/// `Clone` (issue #839 / FG-4e): [`PreludeDecls`] holds a whole
/// [`ShapeTable`] and is cloned out of `brink-db`'s `lir_prelude_decls_query`
/// Arc once per link execution — see that struct's doc.
#[derive(Clone)]
pub struct ShapeInfo {
    pub id: u32,
    /// This shape's own `Struct` symbol identity (issue #2238) — what
    /// [`ShapeTable::get`] (test-only) or [`ShapeTable::get_by_def`]
    /// disambiguates by when more than one declared shape shares a bare
    /// name, and what every *eager* resolution ([`GlobalShapeMap`],
    /// `LowerCtx::temp_shapes`, a field's own nested annotation) stores
    /// instead of a re-resolvable name.
    pub definition_id: DefinitionId,
    pub name: NameId,
    /// Field `NameId`s in shape declaration order — `RecordNew`'s
    /// construction order and `RecordGet`/`RecordSet`'s offset space.
    pub fields: Vec<NameId>,
    /// Field source-name → `(offset, nested)`, where `nested` is the
    /// declared nested-struct-shape's own `DefinitionId`. `Some` only when
    /// the field's own TM-2 annotation is a `RefKind::Type` reference
    /// (issue #2249) the analyzer resolved against this struct's own
    /// declaring file as referrer — that's what lets `expr::known_shape`
    /// chase a read chain (`o.inner.v`) across more than one `.field` hop
    /// without any type inference or re-resolution ambiguity.
    ///
    /// Behavior delta from pre-#2238 (unremarked in that PR's body): this
    /// used to be a flat `struct_names.contains(name)` membership test over
    /// every file's declared structs, std included — now it is the
    /// analyzer's referrer-scoped, `ImportScope`-aware resolution (issue
    /// #2249 moved this off `decls::lookup_global`'s own, differently-scoped
    /// std-exclusion fallback — see the module doc). A field typed as a
    /// struct that *only* std declares, with no import, now records
    /// `nested: None` where it once recorded `Some(name)`, losing the
    /// static-offset chase (`RecordGet`/`RecordSet`'s `static_offset`
    /// `Some` → `None`) under `types = strict` for that field — by-name ops
    /// remain correct, so this is a lowering-strategy change, not a
    /// correctness one, but it is bytecode-visible.
    field_index: LookupMap<String, (u16, Option<DefinitionId>)>,
}

impl ShapeInfo {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<(u16, Option<DefinitionId>)> {
        self.field_index.get(name).copied()
    }
}

/// Every declared `STRUCT` shape in the project. See the module doc for
/// the id-assignment determinism argument and the referrer-scoped
/// resolution story (issue #2238).
#[derive(Default, Clone)]
pub struct ShapeTable {
    /// Every shape by its own symbol-index identity — the canonical store.
    /// The **only** store as of issue #2249: real code always has a
    /// `DefinitionId` in hand (a `RefKind::Struct`/`RefKind::Type`
    /// reference the analyzer already resolved, issues #2246/#2249) and
    /// looks a shape up via [`get_by_def`](ShapeTable::get_by_def) — a
    /// prior bare-name-keyed `by_name` bucket (backing a now-deleted
    /// `ShapeTable::resolve`, the referrer-scoped/std-excluding lookup a
    /// field/TM-2/temp annotation used before this issue) is gone; nothing
    /// left ever needs to look a shape up by its bare name alone. See the
    /// module doc.
    by_def: LookupMap<DefinitionId, ShapeInfo>,
}

impl ShapeTable {
    /// Resolve a shape already pinned to a specific `DefinitionId` — a
    /// nested field's own annotation, a global's TM-2 type, or a temp's
    /// declared type, every one of which was resolved once, correctly, at
    /// the point it was declared (see the module doc). No referrer
    /// ambiguity possible here — the identity is exact.
    #[must_use]
    pub fn get_by_def(&self, id: DefinitionId) -> Option<&ShapeInfo> {
        self.by_def.get(&id)
    }

    /// Number of declared shapes — used by [`struct_shape_defs`] to
    /// pre-size the id-ordered output `Vec`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_def.len()
    }
}

/// Build the project's [`ShapeTable`] from every file's declared `STRUCT`s.
/// `files` is already in topological include order (the same order every
/// other whole-program collector in this module — `decls::collect_globals`
/// et al. — consumes), so iterating it directly and assigning ids
/// sequentially is deterministic without sorting.
///
/// Each declared `STRUCT` resolves its own `DefinitionId` via
/// `decls::lookup_global` using **its own declaring file** as referrer —
/// always an exact match against itself, regardless of any other file
/// sharing its bare name (issue #2238). A true intra-module duplicate
/// (analyzer-diagnosed `E023`, the later HIR decl already dropped from the
/// index) resolves to the *same* `DefinitionId` as its earlier sibling, so
/// the second occurrence is skipped here too — first declaration wins,
/// deterministic under the same ordering, same as before #2238. Two
/// declared-module coexisting shapes (project vs. mounted std, say) get
/// *different* `DefinitionId`s and both keep their own table entry.
///
/// `diagnostics` is the same accumulator `build_prelude_decls` threads
/// through `decls::collect_globals`/`eval_const_expr` — pushed to, never
/// read, if the lookup below ever comes back `None` (issue #2240's `E181`
/// backstop; see that code's own doc for the exact, narrow condition that
/// triggers it).
///
/// `resolutions` (issue #2249): each field's own nested-struct chase no
/// longer re-derives its answer through `decls::lookup_global` — a field's
/// TM-2 type is now a `RefKind::Type` reference the analyzer already
/// resolved (`symbols::project`'s walk + `resolve::resolve_type_ref`), so
/// this consumes that recorded resolution directly. See the module doc's
/// "field or TM-2/temp type annotation" paragraph.
pub fn build_shape_table(
    files: &[(FileId, &hir::HirFile)],
    names: &mut NameTable,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    diagnostics: &mut Vec<Diagnostic>,
) -> ShapeTable {
    let mut by_def: LookupMap<DefinitionId, ShapeInfo> = LookupMap::new();
    let mut next_id: u32 = 0;
    for &(file_id, hir_file) in files {
        for s in &hir_file.structs {
            // Usually resolves via `lookup_global`'s exact-file arm, since
            // `s` is declared in `file_id` itself. It can also come back
            // `None`: if the analyzer already dropped `s` as a true
            // intra-module duplicate (E023, same declared module as an
            // earlier file), no symbol carries `(file_id, s.name)` any
            // more, the exact-file arm misses, and the non-std fallback
            // arm has to rescue the id instead (which is exactly what the
            // `by_def.contains_key` dedup below depends on to recognize a
            // "true intra-module duplicate"). `None` here means even that
            // fallback found nothing — every surviving same-name candidate
            // is std-declared. Issue #2240: this used to silently drop `s`
            // from both the shape table and `NameTable` seeding, shifting
            // subsequent `NameId`s and emitted bytecode with no diagnostic —
            // `E181` is the non-suppressible backstop that makes the drop
            // loud instead (see that code's own doc for the full argument,
            // including why `build_struct_shape_data`'s identical lookup
            // deliberately does *not* duplicate this diagnostic).
            let Some(definition_id) =
                lookup_global(index, file_id, &s.name.text, SymbolKind::Struct)
            else {
                diagnostics.push(Diagnostic {
                    file: file_id,
                    range: s.name.range,
                    message: DiagnosticCode::E181.title().to_string(),
                    code: DiagnosticCode::E181,
                });
                continue;
            };
            if by_def.contains_key(&definition_id) {
                // Same identity already resolved — either the exact same
                // decl reached twice (unreachable given `files`' identity)
                // or a true intra-module duplicate. First occurrence wins.
                continue;
            }
            let shape_name = names.intern(&s.name.text);
            let mut fields = Vec::with_capacity(s.fields.len());
            let mut field_index = LookupMap::with_capacity(s.fields.len());
            for (i, f) in s.fields.iter().enumerate() {
                let field_name = names.intern(&f.name.text);
                fields.push(field_name);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a struct won't declare anywhere near u16::MAX fields"
                )]
                let offset = i as u16;
                // Issue #2249: `f.ty`'s own span is a `RefKind::Type`
                // reference (`symbols::project::Projector::walk_type_annotation`)
                // — a `Generic`/`Fn` field type never registered one (there
                // is nothing to look up at `f.ty.range()` for those), so
                // `resolve` correctly falls through to `None` for them too,
                // matching the prior `match` arm's `_ => None`.
                let nested = resolutions.resolve(file_id, f.ty.range());
                field_index.insert(f.name.text.clone(), (offset, nested));
            }
            let id = next_id;
            next_id += 1;
            by_def.insert(
                definition_id,
                ShapeInfo {
                    id,
                    definition_id,
                    name: shape_name,
                    fields,
                    field_index,
                },
            );
        }
    }

    ShapeTable { by_def }
}

/// Every declared shape as an [`lir::StructShapeDef`], ordered by
/// `ShapeId` (`defs[i].id == i`) — ready for `lir::Program::struct_shapes`.
/// Iterates `shapes`' internal `HashMap` but only to *place* each
/// already-fully-determined entry at its own fixed, non-overlapping index —
/// the resulting `Vec`'s content is independent of iteration order (see the
/// module doc's determinism note).
#[must_use]
pub fn struct_shape_defs(shapes: &ShapeTable) -> Vec<lir::StructShapeDef> {
    let mut defs: Vec<Option<lir::StructShapeDef>> = vec![None; shapes.len()];
    for info in shapes.by_def.values() {
        if let Some(slot) = defs.get_mut(info.id as usize) {
            *slot = Some(lir::StructShapeDef {
                id: info.id,
                name: info.name,
                fields: info.fields.clone(),
            });
        }
    }
    defs.into_iter().flatten().collect()
}

/// Global `VAR`/`CONST` declarations whose TM-2 annotation names a declared
/// struct, resolved to the *shape's own* `DefinitionId` — see the module
/// doc for why this (plus per-temp tracking) is the whole "compile-time
/// known shape" story, and why the value is a `DefinitionId` rather than a
/// re-resolvable name (issue #2238).
pub type GlobalShapeMap = LookupMap<DefinitionId, DefinitionId>;

// ─── FG-4d: cutoff-friendly struct-shape projection ──────────────────
//
// The whole-program struct-shape data ([`ShapeTable`] + [`GlobalShapeMap`])
// is derived from every file's declared `STRUCT`s, so a per-container LIR
// chunk memo that needs it would depend on all files' HIR — defeating
// cross-file cutoff. [`StructShapeData`] is the same data as a
// range-free, `NameId`-free, `Eq`-able value: it can back a whole-project
// salsa query that *backdates* when no struct declaration changed, so a
// per-chunk memo reading it survives an unrelated edit (the FG-4d
// non-re-execution property). The `NameId`s in the reconstructed
// [`ShapeTable`] are chunk-throwaway — per-chunk lowering re-interns every
// field/shape name into its own local table (`expr.rs`'s
// `ctx.names.intern`), reading only shape *ids*, field *offsets*, and the
// nested-shape *names* from the table — so a throwaway numbering is
// byte-identical after [`super::chunk::assemble_scopes`] relocation.

/// One declared struct shape, `NameId`-free. Fields are in declaration
/// (offset) order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructShapeEntry {
    /// Declared struct name.
    pub name: String,
    /// This shape's own `Struct` symbol identity (issue #2238) — see
    /// [`ShapeInfo::definition_id`].
    pub definition_id: DefinitionId,
    /// Fields in declaration order — index `i` is offset `i`.
    pub fields: Vec<StructFieldEntry>,
}

/// One struct field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructFieldEntry {
    /// Field source name.
    pub name: String,
    /// Static offset (declaration index).
    pub offset: u16,
    /// The field's own declared struct-shape's `DefinitionId`, if its TM-2
    /// annotation names another declared struct (the nested read-chain
    /// enabler) — resolved once here, not a re-resolvable name (#2238).
    pub nested: Option<DefinitionId>,
}

/// The whole-program struct-shape data as a cutoff-friendly, `Eq`-able,
/// `NameId`-free value. `shapes[i].id == i` implicitly (ordered by
/// `ShapeId`); `global_shapes` is sorted by `DefinitionId` for determinism.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StructShapeData {
    /// Shapes in `ShapeId` order (index `i` has id `i`).
    pub shapes: Vec<StructShapeEntry>,
    /// Global `VAR`/`CONST` `DefinitionId` → its shape's own `DefinitionId`,
    /// sorted by the global's id.
    pub global_shapes: Vec<(DefinitionId, DefinitionId)>,
}

/// Build the cutoff-friendly [`StructShapeData`] directly from HIR, mirroring
/// [`build_shape_table`]'s id/offset/nested/referrer-resolution rules
/// exactly (verified equivalent in the module tests) — same topological
/// `files` order, same first-declaration-(by identity)-wins dedup, same
/// offset = declaration index, same analyzer-`RefKind::Type`-resolved
/// nested shape reference (issue #2249).
///
/// `resolutions`: the project's whole resolution map (not yet the
/// `ResolutionLookup` index — built once, internally, exactly like
/// `build_prelude_decls`'s own `resolutions: &ResolutionMap` parameter),
/// so `struct_shape_data_query` (a pure, `Eq`-keyed salsa query with no
/// `ResolutionLookup` of its own to reuse) can hand this the same
/// `resolutions_index_query().resolutions` field `build_prelude_decls`
/// reads.
///
/// **Issue #2240 ruling — deliberately no diagnostic sink here.** This
/// function's own `lookup_global` call below can miss for the identical
/// narrow reason [`build_shape_table`]'s does (see [`crate::DiagnosticCode::E181`]'s
/// doc: a true intra-module duplicate whose every surviving candidate is
/// std-declared), and unlike `build_shape_table` — whose caller
/// (`build_prelude_decls`) already threads a `Vec<Diagnostic>` accumulator
/// through every other decl-collection pass — this function has no
/// diagnostic conduit available at all: `brink-db`'s `struct_shape_data_query`
/// is a `#[salsa::tracked(returns(ref))]` pure data query, cutoff-friendly
/// and `Eq`-keyed by design (the whole point of [`StructShapeData`]'s
/// `NameId`-free projection), not a lowering pass that could thread one
/// through without widening the query's return shape and, with it, every
/// downstream consumer's cutoff contract — exactly the kind of "thread a
/// diagnostic sink through a whole-program salsa-memoized collector"
/// architecture change issue #2240 itself named as a real design decision,
/// not a substitution to make unilaterally inside a single-function fix.
///
/// This is safe to leave silent rather than a second unremarked drop,
/// because the two functions are never independently reachable: every real
/// compile (`brink-db`'s `lir_query`) computes both `build_shape_table` (via
/// `lir_prelude_decls_query`) and this function (via
/// `struct_shape_data_query` → `chunk_lowering_ctx_query` →
/// `lir_knot_chunk_query`) in the same salsa revision, over the same
/// `resolutions_index_query` index and the same files' `structs` HIR (a
/// per-file decl-only HIR projection and the raw HIR agree on their
/// `structs` field — see `PreludeDecls`'s own doc). So the exact same drop
/// condition always also reaches `build_shape_table`'s identical lookup in
/// that same compile and raises `E181` there. The only caller that could
/// ever observe this function running *without* `build_shape_table` also
/// running is `Db::knot_chunk`, a `#[doc(hidden)]` test-only probe — never a
/// real product path.
#[must_use]
pub fn build_struct_shape_data(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> StructShapeData {
    let resolutions = ResolutionLookup::build(resolutions);
    let mut seen: LookupSet<DefinitionId> = LookupSet::new();
    let mut shapes = Vec::new();
    for &(file_id, hir_file) in files {
        for s in &hir_file.structs {
            // See this function's own doc above, and `E181`'s doc, for why
            // a `None` here (a true intra-module duplicate whose every
            // surviving candidate is std-declared) is deliberately left
            // undiagnosed at this call site specifically — issue #2240.
            let Some(definition_id) =
                lookup_global(index, file_id, &s.name.text, SymbolKind::Struct)
            else {
                continue;
            };
            if seen.contains(&definition_id) {
                continue;
            }
            seen.insert(definition_id);
            let mut fields = Vec::with_capacity(s.fields.len());
            for (i, f) in s.fields.iter().enumerate() {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a struct won't declare anywhere near u16::MAX fields"
                )]
                let offset = i as u16;
                // Issue #2249: mirrors `build_shape_table`'s identical
                // migration — see that function's doc.
                let nested = resolutions.resolve(file_id, f.ty.range());
                fields.push(StructFieldEntry {
                    name: f.name.text.clone(),
                    offset,
                    nested,
                });
            }
            shapes.push(StructShapeEntry {
                name: s.name.text.clone(),
                definition_id,
                fields,
            });
        }
    }

    // Rebuild a ShapeTable (throwaway names) only to reuse the exact
    // `build_global_shape_map` logic without depending on its internals.
    let mut throwaway = NameTable::new();
    let shape_table = rebuild_shape_table(
        &StructShapeData {
            shapes: shapes.clone(),
            global_shapes: Vec::new(),
        },
        &mut throwaway,
    );
    let global_map = build_global_shape_map(files, index, &resolutions, &shape_table);
    let mut global_shapes: Vec<(DefinitionId, DefinitionId)> = global_map.into_iter().collect();
    global_shapes.sort_by_key(|a| a.0.to_raw());

    StructShapeData {
        shapes,
        global_shapes,
    }
}

/// Reconstruct a [`ShapeTable`] from [`StructShapeData`], interning every
/// shape/field name into `names`. For a per-chunk lowering memo `names` is a
/// throwaway table (the reconstructed `NameId`s are never read — see the
/// module note); the id/offset/`field_index`/`definition_id` data is what
/// lowering uses and is reproduced exactly.
#[must_use]
pub fn rebuild_shape_table(data: &StructShapeData, names: &mut NameTable) -> ShapeTable {
    let mut by_def: LookupMap<DefinitionId, ShapeInfo> = LookupMap::new();
    for (id, entry) in data.shapes.iter().enumerate() {
        let shape_name = names.intern(&entry.name);
        let mut fields = Vec::with_capacity(entry.fields.len());
        let mut field_index = LookupMap::with_capacity(entry.fields.len());
        for f in &entry.fields {
            fields.push(names.intern(&f.name));
            field_index.insert(f.name.clone(), (f.offset, f.nested));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "shape count won't exceed u32::MAX"
        )]
        let shape_id = id as u32;
        by_def.insert(
            entry.definition_id,
            ShapeInfo {
                id: shape_id,
                definition_id: entry.definition_id,
                name: shape_name,
                fields,
                field_index,
            },
        );
    }
    ShapeTable { by_def }
}

/// Reconstruct the [`GlobalShapeMap`] from [`StructShapeData`]'s sorted
/// `global_shapes` list — the inverse of the flattening in
/// [`build_struct_shape_data`].
#[must_use]
pub fn rebuild_global_shape_map(data: &StructShapeData) -> GlobalShapeMap {
    data.global_shapes.iter().copied().collect()
}

#[must_use]
pub fn build_global_shape_map(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    shapes: &ShapeTable,
) -> GlobalShapeMap {
    let mut out = LookupMap::new();
    for &(file_id, hir_file) in files {
        for var in &hir_file.variables {
            record_global_annotation(
                &var.name.text,
                file_id,
                var.annotation.as_ref(),
                SymbolKind::Variable,
                index,
                resolutions,
                shapes,
                &mut out,
            );
        }
        for cst in &hir_file.constants {
            record_global_annotation(
                &cst.name.text,
                file_id,
                cst.annotation.as_ref(),
                SymbolKind::Constant,
                index,
                resolutions,
                shapes,
                &mut out,
            );
        }
    }
    out
}

/// Resolve `name`'s TM-2 struct annotation exactly once, using `file` — the
/// global's own declaring file — as referrer (issue #2238): every later
/// read of this entry (`LowerCtx::global_shape`) gets the already-correct
/// shape identity, with no re-resolution (and no referrer-file tracking)
/// needed at any call site.
///
/// Issue #2249: `annotation`'s own span is a `RefKind::Type` reference the
/// analyzer already resolved — consumed directly via `resolutions` instead
/// of re-deriving it through `ShapeTable::resolve`'s own narrower
/// primitive. Mirrors `context::LowerCtx::record_temp_annotation`'s
/// identical migration (its `temp` twin).
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors decls::lookup_global's own doc precedent for this shape; adding \
              `resolutions` (issue #2249) pushed this one over the 7-arg default"
)]
fn record_global_annotation(
    name: &str,
    file: FileId,
    annotation: Option<&hir::TypeExpr>,
    kind: SymbolKind,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    shapes: &ShapeTable,
    out: &mut GlobalShapeMap,
) {
    let Some(ann) = annotation else {
        return;
    };
    let Some(shape) = resolutions
        .resolve(file, ann.range())
        .and_then(|id| shapes.get_by_def(id))
    else {
        return;
    };
    if let Some(id) = lookup_global(index, file, name, kind) {
        out.insert(id, shape.definition_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::lower::context::NameTable;

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use brink_format::DefinitionTag;

    use crate::symbols::{ResolvedRef, SymbolInfo, Visibility};

    fn hir_for(src: &str) -> hir::HirFile {
        let parsed = brink_syntax::parse(src);
        let (hir, _manifest, _diag) = hir::lower(FileId(0), &parsed.tree());
        hir
    }

    /// Deterministic `(file, name)` → `DefinitionId` hash, matching
    /// `index_for_structs`' own per-struct id assignment exactly — the
    /// **only** way a test recovers a specific shape from a [`ShapeTable`]
    /// (issue #2249 deleted `ShapeTable::get`'s bare-name lookup; every
    /// caller now goes through [`ShapeTable::get_by_def`], the same
    /// `DefinitionId`-only surface production code uses, so a test needs
    /// this to reproduce the id `index_for_structs` assigned).
    fn struct_def_id(file: FileId, name: &str) -> DefinitionId {
        let mut hasher = DefaultHasher::new();
        file.0.hash(&mut hasher);
        name.hash(&mut hasher);
        DefinitionId::new(DefinitionTag::StructDef, hasher.finish())
    }

    /// Build a minimal [`SymbolIndex`] with exactly `hir`'s `STRUCT`
    /// declarations indexed under `file` — enough for `build_shape_table`/
    /// `build_struct_shape_data`'s own `decls::lookup_global` calls to
    /// resolve, without pulling in `brink-analyzer` (which depends on this
    /// crate, not the reverse). Mirrors
    /// `brink-analyzer::manifest::insert_symbol`'s own-file dedup: a second
    /// decl of an already-indexed (file, name) pair is a real intra-file
    /// duplicate (`E023`) and is never inserted — only the first reaches
    /// the index, exactly as in a real compile.
    fn index_for_structs(hir: &hir::HirFile, file: FileId) -> SymbolIndex {
        let mut index = SymbolIndex::default();
        for s in &hir.structs {
            let already_indexed = index.by_name.get(&s.name.text).is_some_and(|ids| {
                ids.iter().any(|id| {
                    index
                        .symbols
                        .get(id)
                        .is_some_and(|info| info.kind == SymbolKind::Struct && info.file == file)
                })
            });
            if already_indexed {
                continue;
            }
            let def_id = struct_def_id(file, &s.name.text);
            index.symbols.insert(
                def_id,
                SymbolInfo {
                    kind: SymbolKind::Struct,
                    file,
                    range: s.name.range,
                    id: def_id,
                    name: s.name.text.clone(),
                    params: Vec::new(),
                    detail: None,
                    scope: None,
                    param_detail: None,
                    module: None,
                    visibility: Visibility::Public,
                },
            );
            index
                .by_name
                .entry(s.name.text.clone())
                .or_default()
                .push(def_id);
        }
        index
    }

    /// Test-only stand-in for `brink_analyzer::resolve::resolve_type_ref`
    /// (issue #2249): resolves every struct field's / global's `Named` TM-2
    /// type annotation against `index`'s declared `Struct` symbols, without
    /// pulling in `brink-analyzer` (crate layering forbids the reverse edge
    /// — see `index_for_structs`'s own doc). No referrer-scoping/std-exclusion
    /// nuance — every fixture in this module either declares its structs in
    /// one file or (`lookup_global_picks_the_referrers_own_shape_when_names_collide`)
    /// never exercises a nested-field/global-annotation lookup at all, so a
    /// bare by-name match is faithful to what the analyzer would resolve
    /// for the cases this module actually covers; the analyzer's own
    /// referrer/std-exclusion behavior has its own dedicated test coverage
    /// (`brink-analyzer::resolve`'s tests).
    fn resolutions_for(files: &[(FileId, &hir::HirFile)], index: &SymbolIndex) -> ResolutionMap {
        fn record(
            file: FileId,
            ty: &hir::TypeExpr,
            index: &SymbolIndex,
            refs: &mut Vec<ResolvedRef>,
        ) {
            let hir::TypeExpr::Named { name, range } = ty else {
                return;
            };
            let Some(id) = index.by_name.get(name).and_then(|ids| {
                ids.iter()
                    .find(|id| {
                        index
                            .symbols
                            .get(id)
                            .is_some_and(|info| info.kind == SymbolKind::Struct)
                    })
                    .copied()
            }) else {
                return;
            };
            refs.push(ResolvedRef {
                file,
                range: *range,
                target: id,
            });
        }
        let mut refs = Vec::new();
        for &(file_id, hir_file) in files {
            for s in &hir_file.structs {
                for f in &s.fields {
                    record(file_id, &f.ty, index, &mut refs);
                }
            }
            for v in &hir_file.variables {
                if let Some(ann) = &v.annotation {
                    record(file_id, ann, index, &mut refs);
                }
            }
            for c in &hir_file.constants {
                if let Some(ann) = &c.annotation {
                    record(file_id, ann, index, &mut refs);
                }
            }
        }
        refs
    }

    #[test]
    fn shape_table_assigns_dense_ids_in_declaration_order() {
        let hir = hir_for("STRUCT Alpha = #{v: int}\nSTRUCT Beta = #{v: int, w: int}\nHello.\n");
        let index = index_for_structs(&hir, FileId(0));
        let files = [(FileId(0), &hir)];
        let resolutions = ResolutionLookup::build(&resolutions_for(&files, &index));
        let mut names = NameTable::new();
        let shapes = build_shape_table(&files, &mut names, &index, &resolutions, &mut Vec::new());
        assert_eq!(shapes.len(), 2);
        let alpha = shapes
            .get_by_def(struct_def_id(FileId(0), "Alpha"))
            .expect("Alpha should be in the table");
        let beta = shapes
            .get_by_def(struct_def_id(FileId(0), "Beta"))
            .expect("Beta should be in the table");
        assert_eq!(alpha.id, 0, "first declared struct gets shape id 0");
        assert_eq!(beta.id, 1, "second declared struct gets shape id 1");
        assert_eq!(beta.fields.len(), 2);
        assert_eq!(beta.field("v").map(|(offset, _)| offset), Some(0));
        assert_eq!(beta.field("w").map(|(offset, _)| offset), Some(1));
        assert!(
            shapes
                .get_by_def(struct_def_id(FileId(0), "Bogus"))
                .is_none()
        );
    }

    #[test]
    fn shape_table_tracks_nested_struct_typed_fields() {
        let hir =
            hir_for("STRUCT Inner = #{v: int}\nSTRUCT Outer = #{inner: Inner, n: int}\nHello.\n");
        let index = index_for_structs(&hir, FileId(0));
        let files = [(FileId(0), &hir)];
        let resolutions = ResolutionLookup::build(&resolutions_for(&files, &index));
        let mut names = NameTable::new();
        let shapes = build_shape_table(&files, &mut names, &index, &resolutions, &mut Vec::new());
        let inner = shapes
            .get_by_def(struct_def_id(FileId(0), "Inner"))
            .expect("Inner should be in the table");
        let outer = shapes
            .get_by_def(struct_def_id(FileId(0), "Outer"))
            .expect("Outer should be in the table");
        let (_, nested) = outer.field("inner").expect("Outer declares `inner`");
        assert_eq!(
            nested,
            Some(inner.definition_id),
            "a struct-typed field records its nested shape's own identity"
        );
        let (_, plain_nested) = outer.field("n").expect("Outer declares `n`");
        assert_eq!(
            plain_nested, None,
            "a non-struct-typed field has no nested shape"
        );
    }

    /// Issue #2246 review: `lookup_global`'s std-exclusion fallback must
    /// refuse a struct name only a mounted `std…` module declares, with
    /// **no** project-side homonym anywhere (so the candidate bucket holds
    /// just one entry) — the referrer must not silently reach into std
    /// with no import, the exact bug class #2197/#2238 closed for every
    /// other bare-name lookup in this crate. (Formerly exercised through
    /// `ShapeTable::resolve`'s own now-deleted wrapper — issue #2249 — this
    /// test now calls `lookup_global` directly, the primitive that
    /// actually implements the property.)
    #[test]
    fn lookup_global_excludes_a_sole_std_declared_struct_with_no_project_homonym() {
        let mut index = SymbolIndex::default();
        let std_file = FileId(1);
        let referrer_file = FileId(0);
        let def_id = DefinitionId::new(DefinitionTag::StructDef, 1);
        index.symbols.insert(
            def_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: std_file,
                range: rowan::TextRange::default(),
                id: def_id,
                name: "Cue".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::conventions::screenplay".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("Cue".to_string())
            .or_default()
            .push(def_id);

        assert!(
            lookup_global(&index, referrer_file, "Cue", SymbolKind::Struct).is_none(),
            "a struct name only a mounted std module declares must not resolve for a \
             referrer that never declares (or imports) it itself — even when it is the \
             sole candidate in the bucket"
        );
    }

    /// FG-4d byte-identity contract: the cutoff-friendly [`StructShapeData`]
    /// projection reconstructs a [`ShapeTable`] with the exact same ids,
    /// offsets, nested-shape identities, and field counts as the direct
    /// [`build_shape_table`] the monolithic path uses. Only the `NameId`s
    /// differ (throwaway vs. shared), which per-chunk lowering never reads.
    #[test]
    fn struct_shape_data_roundtrips_to_shape_table() {
        let hir = hir_for(
            "STRUCT Inner = #{v: int}\n\
             STRUCT Outer = #{inner: Inner, n: int, tail: Inner}\n\
             STRUCT Alpha = #{a: int, b: int}\nHello.\n",
        );
        let files = [(FileId(0), &hir)];
        let index = index_for_structs(&hir, FileId(0));
        let resolutions_map = resolutions_for(&files, &index);
        let resolutions = ResolutionLookup::build(&resolutions_map);

        let mut direct_names = NameTable::new();
        let direct = build_shape_table(
            &files,
            &mut direct_names,
            &index,
            &resolutions,
            &mut Vec::new(),
        );

        let data = build_struct_shape_data(&files, &index, &resolutions_map);
        let mut throwaway = NameTable::new();
        let rebuilt = rebuild_shape_table(&data, &mut throwaway);

        assert_eq!(direct.len(), rebuilt.len());
        for name in ["Inner", "Outer", "Alpha"] {
            let def_id = struct_def_id(FileId(0), name);
            let d = direct.get_by_def(def_id).expect("shape in direct table");
            let r = rebuilt.get_by_def(def_id).expect("shape in rebuilt table");
            assert_eq!(d.id, r.id, "{name} shape id");
            assert_eq!(d.fields.len(), r.fields.len(), "{name} field count");
            // Every field: same offset + same nested shape identity.
            for field in ["inner", "n", "tail", "a", "b", "v"] {
                let d_field = d.field(field);
                let r_field = r.field(field);
                assert_eq!(d_field, r_field, "{name}.{field} offset/nested");
            }
        }
    }

    #[test]
    fn duplicate_struct_names_keep_the_first_declaration() {
        let hir = hir_for("STRUCT Dup = #{a: int}\nSTRUCT Dup = #{b: int, c: int}\nHello.\n");
        let index = index_for_structs(&hir, FileId(0));
        let files = [(FileId(0), &hir)];
        let resolutions = ResolutionLookup::build(&resolutions_for(&files, &index));
        let mut names = NameTable::new();
        let shapes = build_shape_table(&files, &mut names, &index, &resolutions, &mut Vec::new());
        assert_eq!(
            shapes.len(),
            1,
            "the duplicate name occupies one table slot"
        );
        let dup = shapes
            .get_by_def(struct_def_id(FileId(0), "Dup"))
            .expect("Dup should be in the table");
        assert_eq!(
            dup.fields.len(),
            1,
            "the first declaration's single field `a` wins"
        );
        assert!(dup.field("a").is_some());
        assert!(dup.field("b").is_none());
    }

    /// Issue #2238's own regression: two *coexisting* declared-module
    /// shapes sharing a bare name (project vs. mounted std, modeled here as
    /// two distinct files each declaring its own `Cue`) must each keep
    /// their own table entry, and `lookup_global` (the primitive
    /// `ShapeTable::resolve` used to wrap, before its issue #2249 removal)
    /// must pick the one declared in the referrer's own file rather than
    /// whichever was seen first.
    #[test]
    fn lookup_global_picks_the_referrers_own_shape_when_names_collide() {
        let std_file = FileId(0);
        let std_hir = hir_for("STRUCT Cue = #{speaker: string}\nHello.\n");
        let project_file = FileId(1);
        let project_hir = hir_for("STRUCT Cue = #{speaker: string, voiceover: bool}\nHello.\n");

        // Build one index spanning both files, as the real merged project
        // index would (M-2d: different declared-module files coexist).
        let mut index = index_for_structs(&std_hir, std_file);
        for (id, info) in index_for_structs(&project_hir, project_file).symbols {
            index.by_name.entry(info.name.clone()).or_default().push(id);
            index.symbols.insert(id, info);
        }

        let files = [(std_file, &std_hir), (project_file, &project_hir)];
        let resolutions = ResolutionLookup::build(&resolutions_for(&files, &index));
        let mut names = NameTable::new();
        let shapes = build_shape_table(&files, &mut names, &index, &resolutions, &mut Vec::new());

        // Both shapes coexist — two distinct table entries for one name.
        assert_eq!(
            shapes.len(),
            2,
            "std's Cue and the project's Cue both keep a shape id"
        );

        let from_std = lookup_global(&index, std_file, "Cue", SymbolKind::Struct)
            .and_then(|id| shapes.get_by_def(id))
            .expect("std's own file resolves its own Cue");
        assert_eq!(
            from_std.fields.len(),
            1,
            "std's Cue keeps its own 1-field shape"
        );

        let from_project = lookup_global(&index, project_file, "Cue", SymbolKind::Struct)
            .and_then(|id| shapes.get_by_def(id))
            .expect("the project's own file resolves its own Cue");
        assert_eq!(
            from_project.fields.len(),
            2,
            "the project's Cue keeps its own 2-field shape, not std's"
        );
        assert_ne!(
            from_std.id, from_project.id,
            "the two coexisting shapes have distinct shape ids"
        );
    }

    /// Issue #2240's own regression: when a declared `STRUCT`'s own
    /// `(file, name)` symbol entry is missing from the index — the same
    /// exact-file-arm miss a true intra-module duplicate produces — and
    /// every surviving same-name candidate in the bucket is std-declared,
    /// `lookup_global`'s non-std fallback also misses. Before the `E181`
    /// backstop this silently dropped the struct from the shape table with
    /// no diagnostic at all; now it raises a real, non-suppressible compile
    /// diagnostic instead.
    ///
    /// Rule 20a: verified this assertion fails (`diagnostics` stays empty)
    /// with the `E181` push removed from `build_shape_table`'s `else`
    /// branch (leaving a bare `continue`, matching the pre-fix code) —
    /// restored before committing.
    #[test]
    fn build_shape_table_reports_e181_when_every_surviving_candidate_is_std_declared() {
        let ghost_file = FileId(7);
        let ghost_hir = hir_for("STRUCT Ghost = #{v: int}\nHello.\n");

        // The only symbol the index knows about for "Ghost" is declared in
        // a *different* file, in a mounted std module — modeling the
        // aftermath of an analyzer-dropped intra-module duplicate whose
        // surviving sibling happens to be std-declared. `ghost_file` itself
        // carries no symbol for "Ghost" at all, so the exact-file arm
        // misses, and the non-std fallback arm excludes the only candidate
        // there is — exactly the condition `E181`'s doc describes.
        let mut index = SymbolIndex::default();
        let std_file = FileId(9);
        let std_def_id = DefinitionId::new(DefinitionTag::StructDef, 1);
        index.symbols.insert(
            std_def_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: std_file,
                range: rowan::TextRange::default(),
                id: std_def_id,
                name: "Ghost".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::x".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("Ghost".to_string())
            .or_default()
            .push(std_def_id);

        let files = [(ghost_file, &ghost_hir)];
        let resolutions = ResolutionLookup::build(&resolutions_for(&files, &index));
        let mut names = NameTable::new();
        let mut diagnostics = Vec::new();
        let shapes = build_shape_table(&files, &mut names, &index, &resolutions, &mut diagnostics);

        assert_eq!(
            shapes.len(),
            0,
            "the struct still can't resolve its own identity, so it still \
             occupies no table slot — E181 makes the drop loud, not stops \
             it from happening"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "the unresolvable lookup must raise exactly one diagnostic"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::E181);
        assert_eq!(diagnostics[0].file, ghost_file);
        assert_eq!(
            diagnostics[0].range, ghost_hir.structs[0].name.range,
            "reported at the struct's own name span"
        );
    }

    /// Companion to the test above, backing the `E181` doc's "always
    /// co-computed with `build_shape_table` in the same compile" ruling for
    /// why [`build_struct_shape_data`] deliberately raises no diagnostic of
    /// its own: the identical unresolvable-lookup condition silently empties
    /// its output too, exactly mirroring `build_shape_table`'s drop.
    #[test]
    fn build_struct_shape_data_silently_mirrors_the_same_unresolvable_drop() {
        let ghost_file = FileId(7);
        let ghost_hir = hir_for("STRUCT Ghost = #{v: int}\nHello.\n");

        let mut index = SymbolIndex::default();
        let std_file = FileId(9);
        let std_def_id = DefinitionId::new(DefinitionTag::StructDef, 1);
        index.symbols.insert(
            std_def_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: std_file,
                range: rowan::TextRange::default(),
                id: std_def_id,
                name: "Ghost".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::x".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("Ghost".to_string())
            .or_default()
            .push(std_def_id);

        let files = [(ghost_file, &ghost_hir)];
        let resolutions = resolutions_for(&files, &index);
        let data = build_struct_shape_data(&files, &index, &resolutions);

        assert!(
            data.shapes.is_empty(),
            "the mirrored, diagnostic-sink-free path drops the same struct \
             — see E181's doc for why that's a documented ruling and not a \
             second unremarked silent drop"
        );
    }
}
