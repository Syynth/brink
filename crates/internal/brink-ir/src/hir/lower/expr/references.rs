//! Reference expression lowering: paths, function calls, divert targets, list literals.

use brink_syntax::ast::{self, AstNode};

use crate::{DiagnosticCode, Expr, Path};

use super::super::context::{LowerScope, LowerSink, Lowered};
use super::super::helpers::{lower_path, make_name};
use super::LowerExpr;

// ─── Path / variable reference ──────────────────────────────────────

impl LowerExpr for ast::Path {
    fn lower_expr(&self, _scope: &LowerScope, _sink: &mut impl LowerSink) -> Lowered<Expr> {
        let p = lower_path(self);
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
        Ok(Expr::Call(path, args))
    }
}

// ─── Computed-callee call attempt (docs/t1c-spec.md §3/§10, issue #869) ──

impl LowerExpr for ast::CallExpr {
    /// `expr(args…)` where `expr` isn't a bare name (`CALL_EXPR` — see the
    /// grammar comment on that kind). Always rejected: Direct-call syntax
    /// is RULED to a bare variable/temp/param callee (t1c-spec §3), and
    /// dispatch through a computed callee via bare-call sugar
    /// ("method-call syntax") is explicitly out of T1c (§10). The author's
    /// fix is the ratified Explicit form, `call(f, args…)`, which already
    /// dispatches through exactly this class of expression correctly (it
    /// reuses the same `CallValue` runtime op). Pre-#869 this construct
    /// wasn't even representable — the parser left the trailing `(args…)`
    /// unconsumed and it resurfaced as prose TEXT on the content line, so
    /// the call silently vanished *and* corrupted output; a diagnostic
    /// here is strictly a fix, never a regression on previously-working
    /// source (nothing could reach this node before the grammar existed).
    fn lower_expr(&self, _scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Expr> {
        let range = self.syntax().text_range();
        Err(sink.diagnose(range, DiagnosticCode::E104))
    }
}

// ─── Divert targets and list literals ───────────────────────────────

impl LowerExpr for ast::DivertTargetExpr {
    fn lower_expr(&self, _scope: &LowerScope, _sink: &mut impl LowerSink) -> Lowered<Expr> {
        // The parser always creates a PATH node (empty on error + E037),
        // so self.target() always returns Some (lane-A audit, #709: E018 is
        // unreachable).
        let Some(ast_path) = self.target() else {
            unreachable!("parser guarantees PATH node in DivertTargetExpr")
        };
        let path = lower_path(&ast_path);
        Ok(Expr::DivertTarget(path))
    }
}

impl LowerExpr for ast::ListExpr {
    fn lower_expr(&self, _scope: &LowerScope, _sink: &mut impl LowerSink) -> Lowered<Expr> {
        let items: Vec<Path> = self.items().map(|p| lower_path(&p)).collect();
        Ok(Expr::ListLiteral(items))
    }
}
