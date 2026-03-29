//! Reference expression lowering: paths, function calls, divert targets, list literals.

use brink_syntax::ast::{self, AstNode};

use crate::{DiagnosticCode, Expr, Path, RefKind};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::{lower_path, make_name, path_full_name};
use super::LowerExpr;

// ─── Path / variable reference ──────────────────────────────────────

impl LowerExpr for ast::Path {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let p = lower_path(self);
        let full = path_full_name(&p);
        sink.add_unresolved(
            &full,
            self.syntax().text_range(),
            RefKind::Variable,
            &scope.to_scope(),
            None,
        );
        Ok(Expr::Path(p))
    }
}

// ─── Function calls ─────────────────────────────────────────────────

impl LowerExpr for ast::FunctionCall {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let ident = self
            .identifier()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let name_text = ident
            .name()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E017))?;
        let ident_range = ident.syntax().text_range();
        let path = Path {
            segments: vec![make_name(name_text.clone(), ident_range)],
            range: ident_range,
        };
        let args: Vec<Expr> = self
            .arg_list()
            .map(|al| {
                al.args()
                    .filter_map(|a| a.lower_expr(scope, sink).ok())
                    .collect()
            })
            .unwrap_or_default();
        sink.add_unresolved(
            &name_text,
            ident_range,
            RefKind::Function,
            &scope.to_scope(),
            Some(args.len()),
        );
        Ok(Expr::Call(path, args))
    }
}

// ─── Divert targets and list literals ───────────────────────────────

impl LowerExpr for ast::DivertTargetExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        let ast_path = self
            .target()
            .ok_or_else(|| sink.diagnose(range, DiagnosticCode::E018))?;
        let path = lower_path(&ast_path);
        let full = path_full_name(&path);
        sink.add_unresolved(
            &full,
            ast_path.syntax().text_range(),
            RefKind::Divert,
            &scope.to_scope(),
            None,
        );
        Ok(Expr::DivertTarget(path))
    }
}

impl LowerExpr for ast::ListExpr {
    fn lower_expr(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let items: Vec<Path> = self.items().map(|p| lower_path(&p)).collect();
        for item in &items {
            let full = path_full_name(item);
            sink.add_unresolved(&full, item.range, RefKind::List, &scope.to_scope(), None);
        }
        Ok(Expr::ListLiteral(items))
    }
}
