//! TM-2 inline type annotation lowering (docs/typed-mode-spec.md §3).
//!
//! Purely structural: AST `TypeExpr`/`TypeAnnotation` -> HIR [`TypeExpr`].
//! Mirrors the T1b pattern — `brink-ir` always lowers the superset grammar
//! faithfully regardless of dialect; whether an annotation is *allowed*
//! (`strict-ink` E051) or names something recognized (E061 unknown name,
//! E062 `fn(...)` reserved-until-T1c) is `brink-analyzer`'s job, not this
//! module's. No diagnostics are emitted here — an annotation that fails to
//! parse into a well-formed `ast::TypeExpr` (parser depth-limit recovery)
//! simply has no HIR representation (`None`), same "absent data is legal"
//! contract as every other optional AST child in this crate.

use brink_syntax::ast::{self, AstNode};

use crate::hir::types::TypeExpr;

/// Lower an `ast::TypeAnnotation`'s inner type expression, if well-formed.
pub(crate) fn lower_type_annotation(annotation: &ast::TypeAnnotation) -> Option<TypeExpr> {
    annotation.type_expr().and_then(|te| lower_type_expr(&te))
}

/// Lower an `ast::TypeExpr` to its HIR shape.
pub(crate) fn lower_type_expr(te: &ast::TypeExpr) -> Option<TypeExpr> {
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
            let ret = f.return_type().and_then(|r| lower_type_expr(&r))?;
            Some(TypeExpr::Fn {
                params,
                ret: Box::new(ret),
                range,
            })
        }
    }
}
