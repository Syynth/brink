//! Divert lowering phase.
//!
//! Handles all divert variants: simple diverts, tunnel calls, tunnel
//! returns (onwards), thread starts, and the various path forms
//! (named path, `DONE`, `END`).

use brink_syntax::ast::{self, AstNode, AstPtr, SyntaxNodePtr};

use crate::{
    DiagnosticCode, Divert, DivertPath, DivertTarget, Expr, RefKind, Return, Stmt, ThreadStart,
    TunnelCall,
};

use super::context::{LowerScope, LowerSink, Lowered};
use super::expr::LowerExpr;
use super::helpers::{lower_path, path_full_name};

// ─── Trait definition ───────────────────────────────────────────────

/// Extension trait for lowering divert-related AST nodes.
pub trait LowerDivert {
    fn lower_divert(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Stmt>;
}

// ─── DivertNode ─────────────────────────────────────────────────────

impl LowerDivert for ast::DivertNode {
    fn lower_divert(&self, scope: &LowerScope, sink: &mut impl LowerSink) -> Lowered<Stmt> {
        let range = self.syntax().text_range();

        // Thread start: `<- target`
        if let Some(thread) = self.thread_start() {
            if let Some(ts) = lower_thread_target(&thread, scope, sink) {
                return Ok(Stmt::ThreadStart(ts));
            }
            return Err(sink.diagnose(range, DiagnosticCode::E013));
        }

        // Tunnel call: `-> target ->`
        if let Some(tunnel) = self.tunnel_call() {
            let targets: Vec<DivertTarget> = tunnel
                .targets()
                .filter_map(|t| lower_divert_target_with_args(&t, scope, sink))
                .collect();
            if !targets.is_empty() {
                return Ok(Stmt::TunnelCall(TunnelCall {
                    ptr: AstPtr::new(self),
                    targets,
                }));
            }
            return Err(sink.diagnose(range, DiagnosticCode::E012));
        }

        // Tunnel onwards: `->->` (with optional target/chained tunnel)
        if let Some(tunnel_onwards) = self.tunnel_onwards() {
            let onwards_targets: Vec<DivertTarget> = tunnel_onwards
                .targets()
                .filter_map(|t| lower_divert_target_with_args(&t, scope, sink))
                .collect();

            if let Some(tc) = tunnel_onwards.tunnel_call() {
                // `->-> A -> B` — chained tunnel call through onwards target
                let mut targets = onwards_targets;
                targets.extend(
                    tc.targets()
                        .filter_map(|t| lower_divert_target_with_args(&t, scope, sink)),
                );
                if !targets.is_empty() {
                    return Ok(Stmt::TunnelCall(TunnelCall {
                        ptr: AstPtr::new(self),
                        targets,
                    }));
                }
            } else if let Some(target) = onwards_targets.into_iter().next() {
                // `->-> B` — tunnel return with divert override.
                match &target.path {
                    DivertPath::Path(path) => {
                        return Ok(Stmt::Return(Return {
                            ptr: None,
                            value: Some(Expr::DivertTarget(path.clone())),
                            onwards_args: target.args,
                        }));
                    }
                    DivertPath::Done => {
                        return Ok(Stmt::Divert(Divert {
                            ptr: Some(SyntaxNodePtr::from_node(self.syntax())),
                            target: DivertTarget {
                                path: DivertPath::Done,
                                args: Vec::new(),
                            },
                        }));
                    }
                    DivertPath::End => {
                        return Ok(Stmt::Divert(Divert {
                            ptr: Some(SyntaxNodePtr::from_node(self.syntax())),
                            target: DivertTarget {
                                path: DivertPath::End,
                                args: Vec::new(),
                            },
                        }));
                    }
                }
            }

            // Bare `->->` with no targets — tunnel return
            return Ok(Stmt::Return(Return {
                ptr: None,
                value: None,
                onwards_args: Vec::new(),
            }));
        }

        // Simple divert: `-> target` or multi-target `-> A -> B`
        if let Some(simple) = self.simple_divert() {
            let targets: Vec<DivertTarget> = simple
                .targets()
                .filter_map(|t| lower_divert_target_with_args(&t, scope, sink))
                .collect();
            return match targets.len() {
                0 => Err(sink.diagnose(range, DiagnosticCode::E012)),
                1 => Ok(Stmt::Divert(Divert {
                    ptr: Some(SyntaxNodePtr::from_node(self.syntax())),
                    #[expect(clippy::unwrap_used, reason = "length checked to be 1")]
                    target: targets.into_iter().next().unwrap(),
                })),
                _ => Ok(Stmt::TunnelCall(TunnelCall {
                    ptr: AstPtr::new(self),
                    targets,
                })),
            };
        }

        Err(sink.diagnose(range, DiagnosticCode::E012))
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

fn lower_thread_target(
    thread: &ast::ThreadStart,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Option<ThreadStart> {
    let ast_path = thread.target()?;
    let path = lower_path(&ast_path);
    let full = path_full_name(&path);
    sink.add_unresolved(
        &full,
        ast_path.syntax().text_range(),
        RefKind::Divert,
        &scope.to_scope(),
        None,
    );

    let args: Vec<Expr> = thread
        .arg_list()
        .map(|al| {
            al.args()
                .filter_map(|a| a.lower_expr(scope, sink).ok())
                .collect()
        })
        .unwrap_or_default();

    Some(ThreadStart {
        ptr: AstPtr::new(thread),
        target: DivertTarget {
            path: DivertPath::Path(path),
            args,
        },
    })
}

pub fn lower_divert_target_with_args(
    t: &ast::DivertTargetWithArgs,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Option<DivertTarget> {
    let path = lower_divert_path(t, scope, sink)?;
    let args: Vec<Expr> = t
        .arg_list()
        .map(|al| {
            al.args()
                .filter_map(|a| a.lower_expr(scope, sink).ok())
                .collect()
        })
        .unwrap_or_default();
    Some(DivertTarget { path, args })
}

fn lower_divert_path(
    t: &ast::DivertTargetWithArgs,
    scope: &LowerScope,
    sink: &mut impl LowerSink,
) -> Option<DivertPath> {
    if t.done_kw().is_some() {
        return Some(DivertPath::Done);
    }
    if t.end_kw().is_some() {
        return Some(DivertPath::End);
    }
    let ast_path = t.path()?;
    let path = lower_path(&ast_path);
    let full = path_full_name(&path);
    sink.add_unresolved(
        &full,
        ast_path.syntax().text_range(),
        RefKind::Divert,
        &scope.to_scope(),
        None,
    );
    Some(DivertPath::Path(path))
}
