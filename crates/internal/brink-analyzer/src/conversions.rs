//! TM-3 completion: `int(x)`/`float(x)` strict-mode domain compile error
//! (`docs/typed-mode-spec.md` §4, maintainer ruling 2026-07-13, issue #659,
//! ruling 2: "Divert/list/collection inputs: compile error under `types =
//! strict`, runtime fault under gradual").
//!
//! Strict-mode-only, mirroring `structs::check`'s own gating (wired into
//! `strict::check` alongside E065-E071): under `types = gradual` this module
//! is never invoked at all, deferring entirely to the runtime fault the
//! `int`/`float` VM ops already implement
//! (`RuntimeError::InvalidConversionDomain`).
//!
//! `string(x)` accepts every type (ruling 2: "everything, display form") and
//! is never checked here — only `int`/`float` have a restricted domain.
//!
//! Scoped to **statically classifiable** argument literals — a divert-target
//! expression (`-> knot`), a LIST literal, or a `#[...]`/`#{...}`/`Name#{...}`
//! collection/struct literal passed *directly* as the argument. Unlike
//! `structs::check`'s `E071` (mistyped field, issue #670), this pass does
//! *not* extend to variable/call/index-valued arguments — that would need
//! whole-project body inference threaded through arbitrary call-argument
//! positions, a wider surface than this diagnostics-only slice takes on
//! (deeper type-propagation territory, TM-5+). The runtime fault
//! (`InvalidConversionDomain`) is the backstop that still catches those
//! cases at execution time even under `types = strict` — this pass is an
//! additional compile-time convenience for the obvious cases, not the sole
//! enforcement.
//!
//! Shadowing: an unresolved call to `int`/`float` is the builtin (per the
//! stdlib slice-1 ruling, T1b-surface-spec §5); a call that *did* resolve
//! (an author-defined `int`/`float` knot) is an ordinary function call and
//! is never flagged here.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, ResolutionMap};
use rowan::TextRange;

/// The two domain-restricted conversion intrinsics — `string()` accepts
/// every type (ruling 2) and is deliberately excluded.
fn domain_checked_name(name: &str) -> bool {
    matches!(name, "int" | "float")
}

/// Strict-mode-only conversion-domain checks over every `int(x)`/`float(x)`
/// call in the project. Callers only reach this once `strict::config_error`
/// has confirmed `types = strict` + `dialect = brink` (mirrors
/// `structs::check`'s own entry condition).
#[must_use]
pub fn check(files: &[(FileId, &HirFile)], resolutions: &ResolutionMap) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        let mut v = ConversionVisitor {
            file,
            resolution_by_range: &resolution_by_range,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
        // File-level declaration initializers aren't part of `visit::visit`'s
        // block-tree walk (see its module doc) — same pattern
        // `structs::check`/`dialect_gate`/`annotations` use for VAR/CONST.
        for var in &hir.variables {
            check_expr(&var.value, file, &resolution_by_range, &mut out);
        }
        for c in &hir.constants {
            check_expr(&c.value, file, &resolution_by_range, &mut out);
        }
    }
    out
}

struct ConversionVisitor<'a> {
    file: FileId,
    resolution_by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for ConversionVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        check_call(expr, self.file, self.resolution_by_range, self.diagnostics);
    }
}

/// Recurse into `expr` looking for `int`/`float` calls — used only for the
/// file-level VAR/CONST initializers `visit::visit` doesn't cover; every
/// other position is already reached through the `HirVisitor` walk above.
/// Mirrors `structs::check_expr`'s own shape (a small hand recursion, not
/// worth sharing across the two modules for one call site each).
fn check_expr(
    expr: &Expr,
    file: FileId,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<Diagnostic>,
) {
    check_call(expr, file, resolution_by_range, out);
    for child in expr_children(expr) {
        check_expr(child, file, resolution_by_range, out);
    }
}

/// Direct child expressions of `expr` — mirrors `structs::expr_children`
/// (same rationale: needed only because `check_expr` runs outside the
/// `HirVisitor` walk).
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
        // T1e `ref lvalue-path`: only the operand is a child expression.
        Expr::RefArg(ra) => vec![&ra.operand],
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

/// If `expr` is an unresolved (builtin, not author-shadowed) call to
/// `int`/`float` whose single argument is a statically out-of-domain
/// literal, push `E078`. Anything else — resolved calls (shadowed), other
/// names, wrong arity (flagged separately as `E031`), or an argument whose
/// type isn't statically obvious — is silently clean.
fn check_call(
    expr: &Expr,
    file: FileId,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<Diagnostic>,
) {
    let Expr::Call(path, args) = expr else {
        return;
    };
    let [seg] = path.segments.as_slice() else {
        return;
    };
    if !domain_checked_name(&seg.text) {
        return;
    }
    if resolution_by_range.contains_key(&range_key(path.range)) {
        return; // resolved to an author-defined symbol — shadows the builtin
    }
    let [arg] = args.as_slice() else {
        return; // wrong arity — E031's job, not this pass's
    };
    let Some(kind) = out_of_domain_kind(arg) else {
        return;
    };
    out.push(Diagnostic {
        file,
        range: path.range,
        message: format!(
            "{}: `{}(x)` cannot accept a {kind} value under `types = strict` — permitted \
             domain: int, float, string, bool (docs/typed-mode-spec.md §4)",
            DiagnosticCode::E078.title(),
            seg.text,
        ),
        code: DiagnosticCode::E078,
    });
}

/// Classify an argument expression as statically out-of-domain for
/// `int`/`float`, or `None` if it's in-domain or not statically
/// classifiable (a variable/call/index — deferred to the runtime fault).
fn out_of_domain_kind(expr: &Expr) -> Option<&'static str> {
    match expr {
        Expr::DivertTarget(_) => Some("divert"),
        Expr::ListLiteral(_) => Some("list"),
        Expr::ArrayLiteral(_) => Some("array"),
        Expr::MapLiteral(_) => Some("map"),
        Expr::StructLiteral(_) => Some("struct"),
        _ => None,
    }
}

fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// — mirrors `strict::resolution_index` (same rationale: a `Path`'s range is
/// only unique within its own file).
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

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;
    use brink_ir::{FileId, SymbolIndex};

    fn build(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        (hir, (*index).clone(), (*resolutions).clone())
    }

    #[test]
    fn int_of_a_divert_target_literal_is_e078() {
        let (hir, _index, res) =
            build("=== knot ===\nHello.\n-> DONE\n=== main ===\n~ x = int(-> knot)\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("divert"));
    }

    #[test]
    fn float_of_an_array_literal_is_e078() {
        let (hir, _index, res) = build("=== main ===\n~ x = float(#[1, 2])\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E078);
        assert!(diags[0].message.contains("array"));
    }

    #[test]
    fn int_of_a_map_literal_is_e078() {
        let (hir, _index, res) = build("=== main ===\n~ x = int(#{\"a\": 1})\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("map"));
    }

    #[test]
    fn int_of_a_struct_literal_is_e078() {
        let (hir, _index, res) =
            build("STRUCT Point = #{x: float}\n=== main ===\n~ y = int(Point#{x: 1.0})\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("struct"));
    }

    #[test]
    fn int_of_a_list_literal_is_e078() {
        let (hir, _index, res) =
            build("LIST Colors = red, blue\n=== main ===\n~ x = int((red))\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("list"));
    }

    #[test]
    fn int_of_a_numeric_literal_is_clean() {
        let (hir, _index, res) = build("=== main ===\n~ x = int(2.9)\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_of_a_string_literal_is_clean() {
        let (hir, _index, res) = build("=== main ===\n~ x = int(\"42\")\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_of_a_variable_is_not_statically_checked() {
        // Not statically classifiable — deferred to the runtime fault.
        let (hir, _index, res) = build("VAR gold = 5\n=== main ===\n~ x = int(gold)\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn string_of_a_divert_target_is_never_checked() {
        // Ruling 2: `string()` accepts every type.
        let (hir, _index, res) =
            build("=== knot ===\nHello.\n-> DONE\n=== main ===\n~ x = string(-> knot)\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn author_defined_int_shadowing_the_builtin_is_never_checked() {
        let (hir, _index, res) = build(
            "=== function int(x) ===\n~ return 0\n=== main ===\n~ y = int(-> main)\n-> DONE\n",
        );
        let diags = check(&[(FileId(0), &hir)], &res);
        assert!(
            diags.is_empty(),
            "a resolved call to the author's own `int` must never be flagged: {diags:?}"
        );
    }

    #[test]
    fn wrong_arity_int_call_is_not_flagged_here() {
        // E031's job, not this pass's — this pass only checks the arg when
        // arity is exactly 1.
        let (hir, _index, res) = build("=== main ===\n~ x = int(1, 2)\n-> DONE\n");
        let diags = check(&[(FileId(0), &hir)], &res);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
