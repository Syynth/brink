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
//!   winner; [`ShapeTable::resolve`] is the referrer-scoped lookup a **field
//!   or TM-2/temp type annotation** uses to pick the right one — the
//!   candidate declared in the referring file itself, else whichever
//!   `decls::lookup_global` returns (its own std-exclusion rule, which
//!   `resolve` itself now always goes through — issue #2246 review closed a
//!   gap where a sole-candidate bucket skipped that exclusion entirely). A
//!   construction literal's own shape name is a **different** case (issue
//!   #2246): it is a `RefKind::Struct` reference the analyzer already
//!   resolved with full module-scope `Candidacy` semantics
//!   (`brink_analyzer::resolve::resolve_struct_ref`), so `expr::
//!   lower_struct_literal`/`decls::eval_const_struct_literal` consume that
//!   recorded resolution directly and never call `ShapeTable::resolve` at
//!   all — a field/annotation name, by contrast, is never registered as a
//!   ref at all (`symbols::project`'s own doc: "a nominal-only grammar,
//!   resolved later by a different mechanism"), so `ShapeTable::resolve`
//!   remains the only resolution those cases ever get.
//!   A bona fide intra-module duplicate (analyzer-diagnosed `E023`, later
//!   declaration dropped from the index) still keeps only its first
//!   declaration's fields here, because both HIR decls resolve to the
//!   *same* `DefinitionId` and the second is skipped once that id already
//!   has an entry.
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
use crate::symbols::{SymbolIndex, SymbolKind};

use super::context::NameTable;
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
    /// [`ShapeTable::resolve`] disambiguates by when more than one declared
    /// shape shares a bare name, and what every *eager* resolution
    /// ([`GlobalShapeMap`], `LowerCtx::temp_shapes`, a field's own nested
    /// annotation) stores instead of a re-resolvable name.
    pub definition_id: DefinitionId,
    pub name: NameId,
    /// Field `NameId`s in shape declaration order — `RecordNew`'s
    /// construction order and `RecordGet`/`RecordSet`'s offset space.
    pub fields: Vec<NameId>,
    /// Field source-name → `(offset, nested)`, where `nested` is the
    /// declared nested-struct-shape's own `DefinitionId`. `Some` only when
    /// the field's own TM-2 annotation names a struct `decls::lookup_global`
    /// resolves against this struct's own declaring file as referrer —
    /// that's what lets `expr::known_shape` chase a read chain
    /// (`o.inner.v`) across more than one `.field` hop without any type
    /// inference or re-resolution ambiguity.
    ///
    /// Behavior delta from pre-#2238 (unremarked in that PR's body): this
    /// used to be a flat `struct_names.contains(name)` membership test over
    /// every file's declared structs, std included — now it is
    /// `lookup_global`'s referrer-scoped, std-excluding-fallback
    /// resolution. A field typed as a struct that *only* std declares (the
    /// referrer's own file has no same-named struct of its own) now records
    /// `nested: None` where it previously recorded `Some(name)`, losing the
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
    by_def: LookupMap<DefinitionId, ShapeInfo>,
    /// Bare name → every `DefinitionId` sharing it, in declaration order.
    /// More than one entry only when std and the project (or two declared
    /// modules) coexist on the same name (#2238) — [`ShapeTable::resolve`]
    /// picks the right one.
    by_name: LookupMap<String, Vec<DefinitionId>>,
}

impl ShapeTable {
    /// Referrer-free lookup for callers with no file context — **not**
    /// unambiguous: when more than one declared `STRUCT` shares `name` this
    /// returns whichever was declared *first* in `files` order, silently
    /// discarding any later coexisting sibling. Every real lowering call
    /// site has a referrer file and uses [`resolve`](Self::resolve)
    /// instead; this is test-only.
    #[cfg(test)]
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ShapeInfo> {
        self.by_name
            .get(name)?
            .first()
            .and_then(|id| self.by_def.get(id))
    }

    /// Referrer-scoped lookup (issue #2238): when more than one declared
    /// `STRUCT` shares `name` — the project and a mounted std preset both
    /// declaring `Cue`, say — resolve exactly like `decls::lookup_global`
    /// already does for knots/externals: the candidate declared in
    /// `referrer` itself, else whichever non-std candidate
    /// `decls::lookup_global`'s own fallback picks. That fallback excludes
    /// **every** std-declared candidate unconditionally, with no
    /// referrer-is-std carve-out — unlike `resolve.rs`'s
    /// `lookup_by_name_direct`, whose `InScope` tier lets a referrer that is
    /// itself part of `story::std…` resolve a std-mounted sibling. A std
    /// file here that references a same-named shape it does not itself
    /// declare therefore resolves to `None` rather than a std sibling (see
    /// issue #2233 for the analogous `lookup_unique_by_name` asymmetry;
    /// this call path has the same gap and needs its own follow-up).
    ///
    /// Always routes through [`lookup_global`], even when `name`'s bucket
    /// holds exactly one candidate (issue #2246 review): a prior "fast
    /// path" returned that sole candidate unconditionally, bypassing
    /// `lookup_global`'s own std-exclusion entirely — so a struct name only
    /// a mounted `story::std…` module declares (no project-side homonym at
    /// all) resolved silently, the exact "reach into std with no import"
    /// class every other one of the five bare-name lookups this issue
    /// audited was already taught to refuse. `lookup_global` is itself a
    /// single `by_name` bucket scan, so this costs the same for the
    /// overwhelmingly common zero/one-candidate case; it no longer answers
    /// a different question than the multi-candidate branch does.
    #[must_use]
    pub fn resolve(&self, name: &str, referrer: FileId, index: &SymbolIndex) -> Option<&ShapeInfo> {
        // Cheap early bailout: `name` isn't a declared `STRUCT` at all (in
        // this table, by construction — every successfully-resolved
        // declaration is added here), so there's nothing `lookup_global`
        // could find that would map back to a `ShapeInfo` anyway.
        self.by_name.get(name)?;
        let def_id = lookup_global(index, referrer, name, SymbolKind::Struct)?;
        self.by_def.get(&def_id)
    }

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
pub fn build_shape_table(
    files: &[(FileId, &hir::HirFile)],
    names: &mut NameTable,
    index: &SymbolIndex,
) -> ShapeTable {
    let mut by_def: LookupMap<DefinitionId, ShapeInfo> = LookupMap::new();
    let mut by_name: LookupMap<String, Vec<DefinitionId>> = LookupMap::new();
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
            // is std-declared. That silently drops `s` from both the shape
            // table and `NameTable` seeding, shifting subsequent `NameId`s
            // and emitted bytecode with no diagnostic (tracked: issue
            // #2240 — this needs a real diagnostic or an explicit
            // documented-drop contract, not a silent `continue`).
            let Some(definition_id) =
                lookup_global(index, file_id, &s.name.text, SymbolKind::Struct)
            else {
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
                let nested = match &f.ty {
                    hir::TypeExpr::Named { name, .. } => {
                        lookup_global(index, file_id, name, SymbolKind::Struct)
                    }
                    _ => None,
                };
                field_index.insert(f.name.text.clone(), (offset, nested));
            }
            let id = next_id;
            next_id += 1;
            by_name
                .entry(s.name.text.clone())
                .or_default()
                .push(definition_id);
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

    ShapeTable { by_def, by_name }
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
/// offset = declaration index, same `decls::lookup_global`-resolved nested
/// shape reference.
#[must_use]
pub fn build_struct_shape_data(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
) -> StructShapeData {
    let mut seen: LookupSet<DefinitionId> = LookupSet::new();
    let mut shapes = Vec::new();
    for &(file_id, hir_file) in files {
        for s in &hir_file.structs {
            // See `build_shape_table`'s identical lookup for why this can
            // come back `None` (a true intra-module duplicate whose every
            // surviving candidate is std-declared) and why that silent
            // drop is tracked, not intended, behavior (issue #2240).
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
                let nested = match &f.ty {
                    hir::TypeExpr::Named { name, .. } => {
                        lookup_global(index, file_id, name, SymbolKind::Struct)
                    }
                    _ => None,
                };
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
    let global_map = build_global_shape_map(files, index, &shape_table);
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
    let mut by_name: LookupMap<String, Vec<DefinitionId>> = LookupMap::new();
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
        by_name
            .entry(entry.name.clone())
            .or_default()
            .push(entry.definition_id);
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
    ShapeTable { by_def, by_name }
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
fn record_global_annotation(
    name: &str,
    file: FileId,
    annotation: Option<&hir::TypeExpr>,
    kind: SymbolKind,
    index: &SymbolIndex,
    shapes: &ShapeTable,
    out: &mut GlobalShapeMap,
) {
    let Some(hir::TypeExpr::Named { name: ty_name, .. }) = annotation else {
        return;
    };
    let Some(shape) = shapes.resolve(ty_name, file, index) else {
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

    use crate::symbols::{SymbolInfo, Visibility};

    fn hir_for(src: &str) -> hir::HirFile {
        let parsed = brink_syntax::parse(src);
        let (hir, _manifest, _diag) = hir::lower(FileId(0), &parsed.tree());
        hir
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
            let mut hasher = DefaultHasher::new();
            file.0.hash(&mut hasher);
            s.name.text.hash(&mut hasher);
            let def_id = DefinitionId::new(DefinitionTag::StructDef, hasher.finish());
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

    #[test]
    fn shape_table_assigns_dense_ids_in_declaration_order() {
        let hir = hir_for("STRUCT Alpha = #{v: int}\nSTRUCT Beta = #{v: int, w: int}\nHello.\n");
        let index = index_for_structs(&hir, FileId(0));
        let mut names = NameTable::new();
        let shapes = build_shape_table(&[(FileId(0), &hir)], &mut names, &index);
        assert_eq!(shapes.len(), 2);
        let alpha = shapes.get("Alpha").expect("Alpha should be in the table");
        let beta = shapes.get("Beta").expect("Beta should be in the table");
        assert_eq!(alpha.id, 0, "first declared struct gets shape id 0");
        assert_eq!(beta.id, 1, "second declared struct gets shape id 1");
        assert_eq!(beta.fields.len(), 2);
        assert_eq!(beta.field("v").map(|(offset, _)| offset), Some(0));
        assert_eq!(beta.field("w").map(|(offset, _)| offset), Some(1));
        assert!(shapes.get("Bogus").is_none());
    }

    #[test]
    fn shape_table_tracks_nested_struct_typed_fields() {
        let hir =
            hir_for("STRUCT Inner = #{v: int}\nSTRUCT Outer = #{inner: Inner, n: int}\nHello.\n");
        let index = index_for_structs(&hir, FileId(0));
        let mut names = NameTable::new();
        let shapes = build_shape_table(&[(FileId(0), &hir)], &mut names, &index);
        let inner = shapes.get("Inner").expect("Inner should be in the table");
        let outer = shapes.get("Outer").expect("Outer should be in the table");
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

    /// Issue #2246 review: `ShapeTable::resolve`'s old "fast path" answered
    /// a *different*, unscoped question whenever `name`'s bucket held
    /// exactly one candidate — it returned that candidate unconditionally,
    /// never consulting `lookup_global`'s std-exclusion at all. A struct
    /// name that only a mounted `story::std…` module declares, with **no**
    /// project-side homonym anywhere (so the bucket really does hold just
    /// one entry), used to resolve straight through — the referrer silently
    /// reaching into std with no import, the exact bug class #2197/#2238
    /// closed for every other bare-name lookup in this crate.
    ///
    /// Rule 20a: verified this assertion fails (returns `Some`, not `None`)
    /// with the production fix reverted to the old `if ids.len() <= 1 {
    /// return ids.first()... }` fast path.
    #[test]
    fn resolve_excludes_a_sole_std_declared_shape_with_no_project_homonym() {
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
                module: Some("story::std::conventions::screenplay".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("Cue".to_string())
            .or_default()
            .push(def_id);

        let mut by_def: LookupMap<DefinitionId, ShapeInfo> = LookupMap::new();
        by_def.insert(
            def_id,
            ShapeInfo {
                id: 0,
                definition_id: def_id,
                name: NameId(0),
                fields: Vec::new(),
                field_index: LookupMap::new(),
            },
        );
        let mut by_name: LookupMap<String, Vec<DefinitionId>> = LookupMap::new();
        by_name.insert("Cue".to_string(), vec![def_id]);
        let shapes = ShapeTable { by_def, by_name };

        assert!(
            shapes.resolve("Cue", referrer_file, &index).is_none(),
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

        let mut direct_names = NameTable::new();
        let direct = build_shape_table(&files, &mut direct_names, &index);

        let data = build_struct_shape_data(&files, &index);
        let mut throwaway = NameTable::new();
        let rebuilt = rebuild_shape_table(&data, &mut throwaway);

        assert_eq!(direct.len(), rebuilt.len());
        for name in ["Inner", "Outer", "Alpha"] {
            let d = direct.get(name).expect("shape in direct table");
            let r = rebuilt.get(name).expect("shape in rebuilt table");
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
        let mut names = NameTable::new();
        let shapes = build_shape_table(&[(FileId(0), &hir)], &mut names, &index);
        assert_eq!(
            shapes.len(),
            1,
            "the duplicate name occupies one table slot"
        );
        let dup = shapes.get("Dup").expect("Dup should be in the table");
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
    /// their own table entry, and `resolve` must pick the one declared in
    /// the referrer's own file rather than whichever was seen first.
    #[test]
    fn resolve_picks_the_referrers_own_shape_when_names_collide() {
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
        let mut names = NameTable::new();
        let shapes = build_shape_table(&files, &mut names, &index);

        // Both shapes coexist — two distinct table entries for one name.
        assert_eq!(
            shapes.len(),
            2,
            "std's Cue and the project's Cue both keep a shape id"
        );

        let from_std = shapes
            .resolve("Cue", std_file, &index)
            .expect("std's own file resolves its own Cue");
        assert_eq!(
            from_std.fields.len(),
            1,
            "std's Cue keeps its own 1-field shape"
        );

        let from_project = shapes
            .resolve("Cue", project_file, &index)
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
}
