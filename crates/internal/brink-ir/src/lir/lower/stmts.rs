use rowan::TextRange;

use crate::hir;
use crate::symbols::SymbolKind;
use crate::{Diagnostic, DiagnosticCode};

use super::content::lower_content;
use super::context::LowerCtx;
use super::expr::{lower_expr, path_to_string};
use super::lir;

/// Lower a single HIR statement to a LIR statement.
///
/// `ChoiceSet`, `LabeledBlock`, `Conditional`, and `Sequence` are handled
/// by the caller (`lower_block_with_children`) since they may produce child
/// containers. This function handles all remaining statement types.
pub(super) fn lower_stmt(stmt: &hir::Stmt, ctx: &mut LowerCtx<'_>) -> Option<lir::Stmt> {
    match stmt {
        hir::Stmt::Divert(divert) => {
            Some(lir::Stmt::Divert(lower_divert_target(&divert.target, ctx)))
        }

        hir::Stmt::TunnelCall(tunnel) => {
            let targets = tunnel
                .targets
                .iter()
                .map(|t| {
                    let d = lower_divert_target(t, ctx);
                    lir::TunnelTarget {
                        target: d.target,
                        args: d.args,
                    }
                })
                .collect();
            Some(lir::Stmt::TunnelCall(lir::TunnelCall { targets }))
        }

        hir::Stmt::ThreadStart(thread) => {
            let d = lower_divert_target(&thread.target, ctx);
            Some(lir::Stmt::ThreadStart(lir::ThreadStart {
                target: d.target,
                args: d.args,
            }))
        }

        hir::Stmt::TempDecl(decl) => {
            let slot = ctx.temp_slot_raw(&decl.name.text)?;
            let name = ctx.names.intern(&decl.name.text);
            let value = decl.value.as_ref().map(|e| lower_expr(e, ctx));
            ctx.visible_temps.insert(decl.name.text.clone());
            ctx.record_temp_annotation(slot, decl.annotation.as_ref());
            Some(lir::Stmt::DeclareTemp { slot, name, value })
        }

        hir::Stmt::Assignment(assign) => {
            let target = lower_assign_target(&assign.target, ctx)?;
            let value = lower_expr(&assign.value, ctx);
            Some(lir::Stmt::Assign {
                target,
                op: assign.op,
                value,
            })
        }

        hir::Stmt::Return(ret) => {
            let value = ret.value.as_ref().map(|e| lower_expr(e, ctx));
            // `->->` (tunnel return) vs `~ return expr` — classified by the
            // explicit `ReturnKind`, never by `ptr` presence.
            let is_tunnel = ret.kind == hir::ReturnKind::TunnelRedirect;
            let args = ret
                .onwards_args
                .iter()
                .map(|a| lir::CallArg::Value(lower_expr(a, ctx)))
                .collect();
            Some(lir::Stmt::Return {
                value,
                is_tunnel,
                args,
            })
        }

        hir::Stmt::ExprStmt(expr) => {
            // Convert x++ / x-- into Assign { target: x, op: Add/Sub, value: 1 }
            if let hir::Expr::Postfix(inner, op) = expr
                && let Some(target) = lower_assign_target(inner, ctx)
            {
                let assign_op = match op {
                    crate::PostfixOp::Increment => crate::AssignOp::Add,
                    crate::PostfixOp::Decrement => crate::AssignOp::Sub,
                };
                return Some(lir::Stmt::Assign {
                    target,
                    op: assign_op,
                    value: lir::Expr::Int(1),
                });
            }
            Some(lir::Stmt::ExprStmt(lower_expr(expr, ctx)))
        }

        // ChoiceSet, LabeledBlock, Conditional, and Sequence are dispatched
        // by lower_block_with_children before reaching lower_stmt — that
        // caller can hand back child containers (`Vec<lir::Container>`),
        // which these four constructs may need and this function's
        // `Option<lir::Stmt>` return can't express. If they reach here, it
        // indicates a dispatch bug.
        // Content is intercepted by lower_block_with_children for glue-aware
        // recognition, but may still reach here from lower_inline_block.
        hir::Stmt::Content(content) => {
            if let Some(emission) = super::recognize::try_recognize(content, ctx) {
                Some(lir::Stmt::EmitLine(emission))
            } else {
                Some(lir::Stmt::EmitContent(lower_content(content, ctx)))
            }
        }

        // Sibling of the `LogicBlock` arm below and reached the exact same
        // way (see #578, `content::lower_inline_block`'s doc comment): an
        // inline conditional/sequence embedded in a `Choice`'s own
        // `start_content`/`bracket_content`/`inner_content` — HIR
        // normalization never lifts *those* into a top-level
        // `Stmt::Conditional`/`Stmt::Sequence`, so it keeps its
        // `ContentPart::InlineConditional`/`InlineSequence` shape and its
        // branch bodies are lowered by `lower_inline_block`, which (unlike
        // `lower_block_with_children`) has no way to hand a child container
        // back to its caller. Unlike `LogicBlock`, these four constructs
        // are not "proper routing" candidates here: a nested `ChoiceSet`
        // fundamentally needs an addressable child container for the
        // runtime to divert into on selection, a `LabeledBlock` needs one
        // to divert into by label, and a nested `Conditional`/`Sequence`
        // needs one per branch to isolate choices — none of which an
        // inline-content position (no choice/gather children possible) can
        // hold. Route this to a real, non-suppressible compile error (E059)
        // instead of `debug_assert!(false, …)`, which panicked in debug
        // builds and silently dropped the construct in release (#585,
        // live-reproduced sibling of #578 — see PR #584's build notes).
        hir::Stmt::ChoiceSet(_)
        | hir::Stmt::LabeledBlock(_)
        | hir::Stmt::Conditional(_)
        | hir::Stmt::Sequence(_) => {
            reject_unsupported_inline_construct(stmt, ctx);
            None
        }

        hir::Stmt::EndOfLine => Some(lir::Stmt::EndOfLine),

        // T1b `~ { … }` blocks (docs/t1b-surface-spec.md §2) are dispatched
        // by `lower_block_with_children` — like ChoiceSet/LabeledBlock/
        // Conditional/Sequence above, they may splice multiple `lir::Stmt`s
        // (one per block statement) at this position, which a function
        // returning a single `Option<lir::Stmt>` can't express. Real
        // lowering lives in `blocks::lower_logic_block`; this arm is a
        // structural dispatch-bug guard, unreachable in practice.
        hir::Stmt::LogicBlock(_) => {
            debug_assert!(
                false,
                "T1b LogicBlock should be dispatched by lower_block_with_children, \
                 not reach lower_stmt"
            );
            None
        }

        // `~ await <cond>` (docs/flow-suspension-spec.md §3): the grammar,
        // HIR, and the effect-free purity gate (E105) land in this FS-2 slice,
        // but the runtime spill/restore that gives `await` its VM semantics is
        // FS-3. Fence lowering with a real, non-suppressible compile error
        // (the E052 pattern — an extension that parses/analyzes before its
        // lowering lands) rather than silently dropping the suspension point.
        hir::Stmt::Await(a) => {
            emit_await_lowering_fence(ctx, a.ptr.text_range());
            None
        }
    }
}

/// Emit the `await` lowering fence (`E052`, docs/flow-suspension-spec.md §3):
/// the FS-2 compiler slice parses and purity-checks `await`, but its runtime
/// semantics (spill/restore of the `FlowFrame`) are FS-3, so lowering is fenced.
/// Non-suppressible Error severity — the same shape as the other LIR fences —
/// so a program using `await` refuses to lower to bytecode rather than
/// silently dropping the suspension point.
pub(super) fn emit_await_lowering_fence(ctx: &mut LowerCtx<'_>, range: TextRange) {
    ctx.diagnostics.push(Diagnostic {
        file: ctx.file,
        range,
        message: format!(
            "{}: `await` (FlowFrame suspension) parses and purity-checks, but its runtime \
             spill/restore semantics are not implemented yet (docs/flow-suspension-spec.md \
             §3, FS-3)",
            DiagnosticCode::E052.title(),
        ),
        code: DiagnosticCode::E052,
    });
}

/// Dispatch a `ChoiceSet`/`LabeledBlock`/`Conditional`/`Sequence` reaching
/// `lower_stmt` to a real, non-suppressible `E059` compile error, naming
/// the construct and its best-effort source anchor. Split out of
/// `lower_stmt`'s match arm purely to keep that function's line count
/// under the `too_many_lines` clippy budget — see its match arm for the
/// full rationale (#585).
fn reject_unsupported_inline_construct(stmt: &hir::Stmt, ctx: &mut LowerCtx<'_>) {
    let (range, construct) = match stmt {
        hir::Stmt::ChoiceSet(cs) => (choice_set_anchor_range(cs), "a nested choice"),
        hir::Stmt::LabeledBlock(inner) => (
            labeled_block_anchor_range(inner),
            "a nested labeled gather block (`- (label)`)",
        ),
        hir::Stmt::Conditional(cond) => (cond.ptr.text_range(), "a nested multi-line conditional"),
        hir::Stmt::Sequence(seq) => (
            seq.ptr.text_range(),
            "a nested sequence (stopping/cycle/once/shuffle)",
        ),
        _ => unreachable!(
            "reject_unsupported_inline_construct is only called for ChoiceSet/LabeledBlock/\
             Conditional/Sequence"
        ),
    };
    emit_unsupported_nested_construct(ctx, range, construct);
}

/// Emit a real, non-suppressible `E059` compile error for a choice/gather
/// construct that reached inline-content lowering (`lower_inline_block`)
/// instead of the top-level `lower_block_with_children` dispatch that can
/// create the child container it needs. See the `lower_stmt` match arms
/// above for the full rationale (#585).
fn emit_unsupported_nested_construct(ctx: &mut LowerCtx<'_>, range: TextRange, construct: &str) {
    ctx.diagnostics.push(Diagnostic {
        file: ctx.file,
        range,
        message: format!(
            "{}: {construct} is not supported embedded in inline content (e.g. a choice's own \
             display/bracket/inner text) — it needs a child container, which an inline-content \
             position cannot hold",
            DiagnosticCode::E059.title(),
        ),
        code: DiagnosticCode::E059,
    });
}

/// Best-effort diagnostic anchor for a `ChoiceSet` reaching
/// `lower_inline_block` — `ChoiceSet` itself carries no `AstPtr`/range (its
/// only provenance is per-choice), so anchor on the first choice, which
/// always exists (a choice set is folded from at least one choice).
fn choice_set_anchor_range(cs: &hir::ChoiceSet) -> TextRange {
    cs.choices.first().map_or_else(
        || TextRange::new(0.into(), 0.into()),
        |c| c.ptr.text_range(),
    )
}

/// Best-effort diagnostic anchor for a `LabeledBlock` reaching
/// `lower_inline_block` — the wrapped `Block` carries no `AstPtr` of its
/// own; use the label's range if labeled, otherwise fall back to a
/// zero-width range. This path is a defense-in-depth backstop, not a
/// routine user-facing diagnostic — no plausible ink source reaches an
/// *unlabeled* nested gather block this way.
fn labeled_block_anchor_range(block: &hir::Block) -> TextRange {
    block
        .label
        .as_ref()
        .map_or_else(|| TextRange::new(0.into(), 0.into()), |label| label.range)
}

fn lower_divert_target(target: &hir::DivertTarget, ctx: &mut LowerCtx<'_>) -> lir::Divert {
    let lir_target = match &target.path {
        hir::DivertPath::Done => lir::DivertTarget::Done,
        hir::DivertPath::End => lir::DivertTarget::End,
        hir::DivertPath::Path(path) => {
            // Check temp slot first — divert parameters (`-> x`) are temps,
            // not in the analyzer's global symbol table.
            let name = path_to_string(path);
            if let Some(slot) = ctx.temp_slot(&name) {
                let name_id = ctx.names.intern(&name);
                lir::DivertTarget::VariableTemp(slot, name_id)
            } else if let Some(info) = ctx.resolve_path(path.range) {
                match info.kind {
                    SymbolKind::Variable | SymbolKind::Constant => {
                        lir::DivertTarget::Variable(info.id)
                    }
                    _ => lir::DivertTarget::Address(info.id),
                }
            } else {
                lir::DivertTarget::Done
            }
        }
    };

    // Look up target's param info to handle ref params correctly.
    // For ref params, we emit pointer-pushing opcodes instead of values.
    let target_params = match &target.path {
        hir::DivertPath::Path(path) => ctx
            .resolve_path(path.range)
            .map(|info| info.params.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    let args = super::expr::lower_call_args(&target.args, &target_params, ctx);

    lir::Divert {
        target: lir_target,
        args,
    }
}

pub(super) fn lower_assign_target(
    expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
) -> Option<lir::AssignTarget> {
    match expr {
        hir::Expr::Path(path) => {
            let name = path_to_string(path);
            if let Some(slot) = ctx.temp_slot(&name) {
                // B1b (issue #1475): an `as` binding is immutable. Every
                // write path funnels through here — plain/compound
                // assignment, an indexed or field assignment's root cell,
                // and the in-place mutators (`pop`, `clear`, …) — so this
                // one refusal covers them all rather than each site
                // re-deriving the rule.
                if ctx.as_binding_slots.contains(&slot) {
                    ctx.diagnostics.push(crate::Diagnostic {
                        file: ctx.file,
                        range: path.range,
                        message: format!(
                            "{}: `{name}` is an `as` binding — it is immutable and cannot be \
                             assigned to or mutated in place",
                            crate::DiagnosticCode::E148.title(),
                        ),
                        code: crate::DiagnosticCode::E148,
                    });
                    return None;
                }
                let name_id = ctx.names.intern(&name);
                return Some(lir::AssignTarget::Temp(slot, name_id));
            }
            if let Some(info) = ctx.resolve_path(path.range) {
                let id = if info.kind == SymbolKind::List {
                    super::decls::list_def_to_global_var(info.id)
                } else {
                    info.id
                };
                return Some(lir::AssignTarget::Global(id));
            }
            None
        }
        _ => None,
    }
}
