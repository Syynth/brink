//! Native `: type` annotation lowering (NG-A/NG-B/NG-C, issues
//! #1487/#1488/#1489).
//!
//! Purely structural: native AST `TypeAnnotation`/`TypeExpr` → HIR
//! [`TypeExpr`]. The exact counterpart of the ink frontend's
//! `hir::lower::types` (same target type, same "absent data is legal"
//! contract), so a `fn f(g: Guest): float` written in either dialect
//! produces the same HIR shape and every downstream consumer — most
//! importantly `brink-analyzer::strict`'s annotation firewall — sees one
//! vocabulary.
//!
//! No diagnostics are emitted here. Whether a name is *recognized*
//! (`brink-analyzer`'s type resolution) is not this module's business, and
//! an annotation that failed to parse into a well-formed `TypeExpr`
//! (depth-limit recovery) simply has no HIR representation (`None`) — the
//! same contract every other optional AST child in `lower_native` has.

use brink_syntax_native::ast::{self, AstNode as _};

use crate::hir::types::TypeExpr;

/// Lower a `TypeAnnotation`'s inner type expression, if well-formed.
pub(super) fn lower_type_annotation(annotation: &ast::TypeAnnotation) -> Option<TypeExpr> {
    annotation.type_expr().as_ref().and_then(lower_type_expr)
}

/// Lower a native `ast::TypeExpr` to its HIR shape.
pub(super) fn lower_type_expr(te: &ast::TypeExpr) -> Option<TypeExpr> {
    let range = te.syntax().text_range();
    match te.kind()? {
        ast::TypeExprKind::Name(n) => Some(TypeExpr::Named {
            name: n.name()?,
            range,
        }),
        ast::TypeExprKind::Generic(g) => {
            let name = g.name()?;
            let args = g.args().filter_map(|a| lower_type_expr(&a)).collect();
            Some(TypeExpr::Generic { name, args, range })
        }
        ast::TypeExprKind::Fn(f) => {
            let params = f.params().iter().filter_map(lower_type_expr).collect();
            let ret = f.return_type().as_ref().and_then(lower_type_expr)?;
            Some(TypeExpr::Fn {
                params,
                ret: Box::new(ret),
                range,
            })
        }
    }
}
