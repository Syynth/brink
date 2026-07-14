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
//!   classifiable* type disagrees with the field's declared type. Scoped to
//!   literal-shaped initializers (int/float/bool/string/array/map/nested
//!   struct literals) — a variable/call/index initializer's type would need
//!   the whole-project body inference this diagnostics-only slice doesn't
//!   thread through arbitrary expression positions (TM-4c/deeper
//!   type-propagation territory); those stay silently unchecked here, same
//!   "Unknown never disagrees" spirit as `annotations::mismatches`.
//!
//! An unresolved shape name (`E068`, already reported by
//! `resolve::resolve_struct_ref`) is not re-reported here — a construction
//! against a shape that doesn't exist has no declared fields to check
//! against.

use std::collections::BTreeMap;

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, StructLiteral, SymbolIndex};

use crate::annotations;
use crate::infer::Ty;

/// One declared struct shape: fields in declaration order, name -> declared
/// type (`Ty::Unknown` if the field's own annotation doesn't resolve —
/// e.g. an unrecognized type name, already flagged elsewhere by
/// `annotations::check`'s `E061`).
struct ShapeInfo {
    fields: Vec<(String, Ty)>,
}

impl ShapeInfo {
    fn field_ty(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|(n, _)| n == name)
    }
}

/// Every declared `STRUCT` shape in the project, by name.
fn declared_shapes(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
) -> BTreeMap<String, ShapeInfo> {
    let list_names = annotations::declared_list_names(index);
    let struct_names = annotations::declared_struct_names(index);
    let mut out = BTreeMap::new();
    for &(_file, hir) in files {
        for s in &hir.structs {
            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let ty = annotations::resolve(&f.ty, &list_names, &struct_names)
                        .unwrap_or(Ty::Unknown);
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
#[must_use]
pub fn check(files: &[(FileId, &HirFile)], index: &SymbolIndex) -> Vec<Diagnostic> {
    let shapes = declared_shapes(files, index);
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut v = ConstructionVisitor {
            file,
            shapes: &shapes,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level declaration initializers aren't part of `visit::visit`'s
        // block-tree walk (see its module doc) — same pattern
        // `dialect_gate`/`annotations` use for VAR/CONST.
        for var in &hir.variables {
            check_expr(&var.value, file, &shapes, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, file, &shapes, &mut out);
        }
    }
    out
}

struct ConstructionVisitor<'a> {
    file: FileId,
    shapes: &'a BTreeMap<String, ShapeInfo>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for ConstructionVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::StructLiteral(sl) = expr {
            check_literal(sl, self.file, self.shapes, self.diagnostics);
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
    out: &mut Vec<Diagnostic>,
) {
    if let Expr::StructLiteral(sl) = expr {
        check_literal(sl, file, shapes, out);
    }
    for child in expr_children(expr) {
        check_expr(child, file, shapes, out);
    }
}

/// Direct child expressions of `expr` — a small mirror of
/// `hir::visit::walk_expr`'s recursion shape, needed only because
/// `check_expr` runs outside that walker (see its own doc).
fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => vec![inner],
        Expr::FieldAccess(fa) => vec![&fa.base],
        Expr::Infix(lhs, _, rhs) => vec![lhs, rhs],
        Expr::Call(_, args) => args.iter().collect(),
        Expr::ArrayLiteral(a) => a.elements.iter().collect(),
        Expr::MapLiteral(m) => m.entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        Expr::Index(idx) => vec![&idx.base, &idx.index],
        Expr::StructLiteral(sl) => sl.fields.iter().map(|(_, v)| v).collect(),
        // T1c `#fn(target, args…)`: only the bound arguments are child
        // expressions — the target is a static `Path` field, same as `Call`.
        Expr::FnLiteral(fl) => fl.args.iter().collect(),
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

    // Mistyped fields — only for statically classifiable initializers (see
    // module doc).
    for (name, value) in &sl.fields {
        let Some(declared_ty) = shape.field_ty(&name.text) else {
            continue; // already flagged as an extra field above
        };
        if declared_ty.is_unresolved() {
            continue; // the field's own annotation didn't resolve (E061)
        }
        let Some(actual_ty) = literal_ty(value) else {
            continue; // not statically classifiable — see module doc
        };
        if &actual_ty != declared_ty && crate::infer::unify(declared_ty, &actual_ty) != *declared_ty
        {
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
/// literals. Anything else (a variable/call/index/…) returns `None`
/// ("not statically classifiable"), which [`check_literal`] treats as
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

    #[test]
    fn clean_construction_produces_no_diagnostics() {
        let (hir, index) = build(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0, y: 2.0}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn missing_field_is_e069_naming_the_field() {
        let (hir, index) = build(
            "STRUCT Point = #{x: float, y: float}\n\
             === main ===\n~ p = Point#{x: 1.0}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E069);
        assert!(diags[0].message.contains('y'), "{:?}", diags[0].message);
    }

    #[test]
    fn extra_field_is_e070_naming_the_field() {
        let (hir, index) = build(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: 1.0, z: 2.0}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E070);
        assert!(diags[0].message.contains('z'), "{:?}", diags[0].message);
    }

    #[test]
    fn mistyped_field_is_e071_naming_the_field() {
        let (hir, index) = build(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: \"hi\"}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
        assert!(diags[0].message.contains('x'), "{:?}", diags[0].message);
    }

    #[test]
    fn int_initializer_for_a_float_field_is_the_legal_coercion() {
        // §4's directional int -> float coercion applies here too.
        let (hir, index) = build(
            "STRUCT Point = #{x: float}\n\
             === main ===\n~ p = Point#{x: 1}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_literal_initializer_is_not_checked_for_mistype() {
        // A variable-valued initializer isn't statically classifiable in
        // this slice — stays silently clean rather than false-flagging.
        let (hir, index) = build(
            "STRUCT Point = #{x: float}\n\
             VAR v = \"hi\"\n=== main ===\n~ p = Point#{x: v}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unresolved_shape_name_is_not_double_reported_here() {
        // No `STRUCT Bogus` declared — `resolve::resolve_struct_ref` already
        // reports E068 elsewhere; this pass has nothing to check against.
        let (hir, index) = build("=== main ===\n~ p = Bogus#{x: 1}\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn nested_struct_literal_field_is_checked_by_shape_name() {
        let (hir, index) = build(
            "STRUCT Inner = #{v: float}\nSTRUCT Outer = #{inner: Inner}\n\
             === main ===\n~ o = Outer#{inner: Inner#{v: 1.0}}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn nested_struct_literal_mistyped_field_still_flags_outer() {
        let (hir, index) = build(
            "STRUCT Wrong = #{v: float}\nSTRUCT Inner = #{v: float}\nSTRUCT Outer = #{inner: Inner}\n\
             === main ===\n~ o = Outer#{inner: Wrong#{v: 1.0}}\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }

    #[test]
    fn struct_literal_inside_var_initializer_is_checked() {
        let (hir, index) = build("STRUCT Point = #{x: float}\nVAR p = Point#{x: \"hi\"}\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E071);
    }
}
