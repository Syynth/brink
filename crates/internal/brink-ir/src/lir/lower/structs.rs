//! TM-4c struct-shape bookkeeping (`docs/typed-mode-spec.md` §6).
//!
//! Two pieces of whole-program, read-only data built once in
//! `lower_to_program`, before any container is lowered:
//!
//! - [`ShapeTable`]: every declared `STRUCT`, keyed by name, with a dense
//!   `ShapeId` (`u32`) assigned in `files` order (already topological) then
//!   source declaration order within a file — deterministic, never derived
//!   from `HashMap` iteration (`CLAUDE.md`'s determinism rule). Feeds
//!   `Expr::StructLiteral` construction (needs the shape id + field order to
//!   reorder initializers) and `lir::Program::struct_shapes` (the format's
//!   `StructShapes` table).
//! - [`GlobalShapeMap`]: every global `VAR`/`CONST` whose TM-2 type
//!   annotation names a declared struct, resolved to its `DefinitionId`.
//!   This — plus the equivalent per-temp tracking `LowerCtx::temp_shapes`
//!   does as declarations are lowered — is the *entire* "compile-time known
//!   shape" story `expr::known_shape` uses to decide `RecordGet`/`RecordSet`
//!   (static offset) vs. `RecordGetDyn`/`RecordSetDyn` (by name) eligibility.
//!   Deliberately conservative: this is annotation-driven, not general type
//!   inference (`brink-ir` cannot depend on `brink-analyzer`'s inference
//!   queries — see the TM-4c PR description's scope note) — an unannotated
//!   variable's shape is never statically known here, even under
//!   `types = strict`, and always falls back to the by-name ops, which are
//!   correct under every policy.

use std::collections::{HashMap, HashSet};

use brink_format::{DefinitionId, NameId};

use crate::FileId;
use crate::hir;
use crate::symbols::{SymbolIndex, SymbolKind};

use super::context::NameTable;
use super::decls::lookup_global;
use super::lir;

/// One declared `STRUCT` shape, resolved for lowering.
pub struct ShapeInfo {
    pub id: u32,
    pub name: NameId,
    /// Field `NameId`s in shape declaration order — `RecordNew`'s
    /// construction order and `RecordGet`/`RecordSet`'s offset space.
    pub fields: Vec<NameId>,
    /// Field source-name → `(offset, declared nested-struct-shape-name)`.
    /// The second element is `Some` only when the field's own TM-2
    /// annotation names another declared struct — that's what lets
    /// `expr::known_shape` chase a read chain (`o.inner.v`) across more than
    /// one `.field` hop without any type inference.
    field_index: HashMap<String, (u16, Option<String>)>,
}

impl ShapeInfo {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<(u16, Option<&str>)> {
        self.field_index
            .get(name)
            .map(|(offset, nested)| (*offset, nested.as_deref()))
    }
}

/// Every declared `STRUCT` shape in the project, by name. See the module
/// doc for the id-assignment determinism argument.
#[derive(Default)]
pub struct ShapeTable {
    by_name: HashMap<String, ShapeInfo>,
}

impl ShapeTable {
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ShapeInfo> {
        self.by_name.get(name)
    }

    /// Number of declared shapes — used by [`struct_shape_defs`] to
    /// pre-size the id-ordered output `Vec`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_name.len()
    }
}

/// Build the project's [`ShapeTable`] from every file's declared `STRUCT`s.
/// `files` is already in topological include order (the same order every
/// other whole-program collector in this module — `decls::collect_globals`
/// et al. — consumes), so iterating it directly and assigning ids
/// sequentially is deterministic without sorting.
///
/// A struct name declared more than once keeps its *first* declaration
/// (deterministic under the same ordering) — duplicate `STRUCT` names are
/// an analyzer concern (out of TM-4c's scope), not something LIR lowering
/// diagnoses; this just needs to not panic or vary run-to-run.
pub fn build_shape_table(files: &[(FileId, &hir::HirFile)], names: &mut NameTable) -> ShapeTable {
    let mut struct_names: HashSet<&str> = HashSet::new();
    for &(_, hir_file) in files {
        for s in &hir_file.structs {
            struct_names.insert(s.name.text.as_str());
        }
    }

    let mut by_name: HashMap<String, ShapeInfo> = HashMap::new();
    let mut next_id: u32 = 0;
    for &(_, hir_file) in files {
        for s in &hir_file.structs {
            if by_name.contains_key(&s.name.text) {
                continue;
            }
            let shape_name = names.intern(&s.name.text);
            let mut fields = Vec::with_capacity(s.fields.len());
            let mut field_index = HashMap::with_capacity(s.fields.len());
            for (i, f) in s.fields.iter().enumerate() {
                let field_name = names.intern(&f.name.text);
                fields.push(field_name);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a struct won't declare anywhere near u16::MAX fields"
                )]
                let offset = i as u16;
                let nested = match &f.ty {
                    hir::TypeExpr::Named { name, .. } if struct_names.contains(name.as_str()) => {
                        Some(name.clone())
                    }
                    _ => None,
                };
                field_index.insert(f.name.text.clone(), (offset, nested));
            }
            by_name.insert(
                s.name.text.clone(),
                ShapeInfo {
                    id: next_id,
                    name: shape_name,
                    fields,
                    field_index,
                },
            );
            next_id += 1;
        }
    }

    ShapeTable { by_name }
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
    for info in shapes.by_name.values() {
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
/// struct, resolved to their `DefinitionId` — see the module doc for why
/// this (plus per-temp tracking) is the whole "compile-time known shape"
/// story.
pub type GlobalShapeMap = HashMap<DefinitionId, String>;

#[must_use]
pub fn build_global_shape_map(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    shapes: &ShapeTable,
) -> GlobalShapeMap {
    let mut out = HashMap::new();
    for &(_, hir_file) in files {
        for var in &hir_file.variables {
            record_global_annotation(
                &var.name.text,
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

fn record_global_annotation(
    name: &str,
    annotation: Option<&hir::TypeExpr>,
    kind: SymbolKind,
    index: &SymbolIndex,
    shapes: &ShapeTable,
    out: &mut GlobalShapeMap,
) {
    let Some(hir::TypeExpr::Named { name: ty_name, .. }) = annotation else {
        return;
    };
    if shapes.get(ty_name).is_none() {
        return;
    }
    if let Some(id) = lookup_global(index, name, kind) {
        out.insert(id, ty_name.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lir::lower::context::NameTable;

    fn hir_for(src: &str) -> hir::HirFile {
        let parsed = brink_syntax::parse(src);
        let (hir, _manifest, _diag) = hir::lower(FileId(0), &parsed.tree());
        hir
    }

    #[test]
    fn shape_table_assigns_dense_ids_in_declaration_order() {
        let hir = hir_for("STRUCT Alpha = #{v: int}\nSTRUCT Beta = #{v: int, w: int}\nHello.\n");
        let mut names = NameTable::new();
        let shapes = build_shape_table(&[(FileId(0), &hir)], &mut names);
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
        let mut names = NameTable::new();
        let shapes = build_shape_table(&[(FileId(0), &hir)], &mut names);
        let outer = shapes.get("Outer").expect("Outer should be in the table");
        let (_, nested) = outer.field("inner").expect("Outer declares `inner`");
        assert_eq!(
            nested,
            Some("Inner"),
            "a struct-typed field records its nested shape name"
        );
        let (_, plain_nested) = outer.field("n").expect("Outer declares `n`");
        assert_eq!(
            plain_nested, None,
            "a non-struct-typed field has no nested shape"
        );
    }

    #[test]
    fn duplicate_struct_names_keep_the_first_declaration() {
        let hir = hir_for("STRUCT Dup = #{a: int}\nSTRUCT Dup = #{b: int, c: int}\nHello.\n");
        let mut names = NameTable::new();
        let shapes = build_shape_table(&[(FileId(0), &hir)], &mut names);
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
}
