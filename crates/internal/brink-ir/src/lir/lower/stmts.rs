use rowan::TextRange;

use crate::hir;
use crate::symbols::SymbolKind;
use crate::{Diagnostic, DiagnosticCode};

use super::content::lower_content;
use super::context::LowerCtx;
use super::expr::{lower_expr, path_to_string};
use super::lir;

/// Best-known provenance for a HIR statement (issue #3183, `docs/debugger-
/// spec.md`) — the payload's own stamped `.ptr` where it carries one
/// (real, non-`Option`: `TunnelCall`/`ThreadStart`/`TempDecl`/`Assignment`/
/// `Conditional`/`Sequence`/`LogicBlock`/`Await` all always have one). Three
/// payloads carry `Option<Provenance>` for legitimately synthesized nodes
/// with no author-written token (`Content`, `Divert`, `Return` — the last
/// is the retired `Return.ptr`-presence trap this design deliberately does
/// not repeat: presence there now only means "do we have a location",
/// nothing semantic, since `ReturnKind` carries the tunnel-vs-explicit bit)
/// and fall back to `ctx.current_stmt_provenance`. `ChoiceSet`/
/// `LabeledBlock` carry no `.ptr` at all — anchored via the same
/// best-effort range already computed for their `E059` diagnostic, just shy
/// of any frontend-specific token so it stamps `NodeClass::Stmt`, the
/// reserved coarse fallback. `ExprStmt`/`AttachElement` derive a range via
/// [`crate::hir::expr_span`] (own-provenance-first, subtree-union fallback)
/// when the wrapped expression has one, else also fall back to ambient.
/// `EndOfLine`/`EndElementRun` are pure structural markers with no source
/// token of their own — always ambient.
pub(super) fn stmt_provenance(stmt: &hir::Stmt, ctx: &LowerCtx<'_>) -> crate::Provenance {
    match stmt {
        hir::Stmt::Content(c) => c.ptr.unwrap_or(ctx.current_stmt_provenance),
        hir::Stmt::Divert(d) => d.ptr.unwrap_or(ctx.current_stmt_provenance),
        hir::Stmt::TunnelCall(t) => t.ptr,
        hir::Stmt::ThreadStart(t) => t.ptr,
        hir::Stmt::TempDecl(t) => t.ptr,
        hir::Stmt::Assignment(a) => a.ptr,
        hir::Stmt::Return(r) => r.ptr.unwrap_or(ctx.current_stmt_provenance),
        hir::Stmt::ChoiceSet(cs) => {
            ctx.provenance_at(choice_set_anchor_range(cs), crate::NodeClass::Stmt)
        }
        hir::Stmt::LabeledBlock(b) => {
            ctx.provenance_at(labeled_block_anchor_range(b), crate::NodeClass::Stmt)
        }
        hir::Stmt::Conditional(c) => c.ptr,
        hir::Stmt::Sequence(s) => s.ptr,
        hir::Stmt::LogicBlock(lb) => lb.ptr,
        hir::Stmt::Await(a) => a.ptr,
        hir::Stmt::ExprStmt(e) | hir::Stmt::AttachElement(e) => crate::hir::expr_span(e)
            .map_or(ctx.current_stmt_provenance, |r| {
                ctx.provenance_at(r, crate::NodeClass::Expr)
            }),
        hir::Stmt::EndOfLine | hir::Stmt::EndElementRun => ctx.current_stmt_provenance,
    }
}

/// Lower a single HIR statement to a LIR statement.
///
/// `ChoiceSet`, `LabeledBlock`, `Conditional`, and `Sequence` are handled
/// by the caller (`lower_block_with_children`) since they may produce child
/// containers. This function handles all remaining statement types.
pub(super) fn lower_stmt(stmt: &hir::Stmt, ctx: &mut LowerCtx<'_>) -> Option<lir::Stmt> {
    let provenance = ctx.enter_stmt(stmt_provenance(stmt, ctx));
    let kind = match stmt {
        hir::Stmt::Divert(divert) => Some(lir::StmtKind::Divert(lower_divert_target(
            &divert.target,
            ctx,
        ))),

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
            Some(lir::StmtKind::TunnelCall(lir::TunnelCall { targets }))
        }

        hir::Stmt::ThreadStart(thread) => {
            let d = lower_divert_target(&thread.target, ctx);
            Some(lir::StmtKind::ThreadStart(lir::ThreadStart {
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
            Some(lir::StmtKind::DeclareTemp { slot, name, value })
        }

        hir::Stmt::Assignment(assign) => {
            let target = lower_assign_target(&assign.target, ctx)?;
            let value = lower_expr(&assign.value, ctx);
            Some(lir::StmtKind::Assign {
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
            Some(lir::StmtKind::Return {
                value,
                is_tunnel,
                args,
            })
        }

        hir::Stmt::ExprStmt(expr) => {
            // x++ / x-- convert to Assign { target: x, op: Add/Sub, value: 1
            // } — including the #2185-sibling field-operand refusal
            // (`~ a.count++` on a struct field is the exact same
            // field-projection misroute as `pop(a.items)`: compiles clean,
            // then faults at runtime with `TypeError("cannot apply Add to
            // Record and Int")`, the same silent-misroute symptom, this
            // time via the postfix `x++`/`x--` desugaring rather than a
            // mutator call). This is the identical conversion the `~ { … }`
            // block surface needs (issue #2894) — delegated to
            // `blocks::try_lower_postfix_stmt` rather than kept as a
            // second, divergence-prone copy here (post-#2900 review: the
            // two copies had already drifted once).
            //
            // Issue #2903: an Index-operand postfix (`a[0]++`, `m["k"]++`)
            // now routes through `lower_indexed_assignment`, which can
            // splice *several* `lir::Stmt`s (the RMW take/mutate/write-back
            // sequence), not just one — this function's `Option<Stmt>`
            // return can't express that, so `.next()` below would silently
            // keep only the harmless first step and drop the actual
            // write-back. Both real classic-line callers of `lower_stmt`
            // (`mod.rs`'s top-level dispatch, `content.rs`'s
            // `lower_inline_block`) now intercept an Index-operand postfix
            // with their own dedicated multi-stmt-splicing arm *before*
            // falling through to this function, mirroring
            // `try_lower_indexed_assignment`'s existing precedent for `~
            // a[i] = v` — so this arm only ever sees a bare-variable or
            // field-projected operand in practice (both always produce 0 or
            // 1 element, `postfix_out.into_iter().next()` is exact for
            // those). The one caller that does NOT pre-intercept is
            // `expr::lower_expr`'s `hir::Expr::Fragment` arm (native-surface
            // block-capture, issue #1839) — an Index-operand postfix
            // reaching *there* still silently drops its write-back today,
            // the same pre-existing shape of gap this function's own doc
            // above already accepts for `try_lower_frame_local_auto_ref_stmt`
            // (issue #2222's "distinct, not-yet-confirmed parity gap");
            // #2903 does not close it, only the two surfaces the issue
            // scoped in.
            let mut postfix_out = Vec::new();
            if super::blocks::try_lower_postfix_stmt(expr, ctx, &mut postfix_out) {
                return postfix_out.into_iter().next();
            }
            Some(lir::StmtKind::ExprStmt(lower_expr(expr, ctx)))
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
                Some(lir::StmtKind::EmitLine(emission))
            } else {
                Some(lir::StmtKind::EmitContent(lower_content(content, ctx)))
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

        hir::Stmt::EndOfLine => Some(lir::StmtKind::EndOfLine),

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

        // Issue #2108: straight expression-to-statement lowering, mirroring
        // `ExprStmt` above — the difference from an ordinary call-for-
        // side-effects is entirely in what codegen emits after evaluating
        // it (`Opcode::AttachElement` vs `Pop`), not in how the call
        // expression itself lowers.
        hir::Stmt::AttachElement(expr) => Some(lir::StmtKind::AttachElement(lower_expr(expr, ctx))),
        hir::Stmt::EndElementRun => Some(lir::StmtKind::EndElementRun),
    };
    kind.map(|k| lir::Stmt::new(k, provenance))
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

/// The shared choke point for resolving an lvalue expression to a LIR
/// [`lir::AssignTarget`] — but it only ever recognizes one shape: a bare
/// (single- or multi-segment) `hir::Expr::Path`. It is the root-resolution
/// step for far more write shapes than its own callers suggest, because
/// every one of them ultimately calls back into this function for its own
/// root:
///
/// - Plain/compound assignment (`hir::Stmt::Assignment`'s classic-line and
///   T1b-block arms) call this directly for `assign.target`.
/// - [`lower_indexed_assignment`](super::blocks::lower_indexed_assignment)
///   (`a[i] = v`/`a[i] op= v`) calls this for the flattened index chain's
///   *root* expression — so an indexed write's root is covered here too,
///   not only a bare `a = v`.
/// - A bare-variable postfix `x++`/`x--`
///   ([`try_lower_postfix_stmt`](super::blocks::try_lower_postfix_stmt))
///   desugars to an `Assign` whose target comes from calling this function.
/// - The `pop`/`heap_pop` mutator intrinsics (`lir::lower::expr`) call this
///   for their single lvalue argument's root.
/// - [`lower_bare_mutator`](super::blocks::lower_bare_mutator) — the bare-
///   variable fast path for the entire `MutatorKind` family (`push`,
///   `insert`, `remove`, `remove_at`, not just `pop`/`heap_pop` above) —
///   calls this for its root.
/// - [`lower_lvalue_container_chain`](super::blocks::lower_lvalue_container_chain)
///   — the indexed-lvalue mutator path (`push(grid[y], v)`) — also calls
///   this for the chain's root before reading any index level.
///
/// It does **not** cover every write shape in the language: a single-level
/// struct-field write/mutator (`p.field = v`, `push(p.field, v)`) resolves
/// its root `SymbolInfo` independently, via
/// [`lower_single_level_field_write`](super::blocks::lower_single_level_field_write)/
/// [`lower_field_mutator`](super::blocks::lower_field_mutator) — those two
/// functions need `head_info` before this function's `Path`-only shape
/// match would give it to them (the caller has already split a two-segment
/// path into head/field), so they never call back into this function at
/// all. Nor does it cover a `ref`-argument call site (`ref x`, `ref p.field`,
/// or the UFCS auto-ref desugar) — passing something by `ref` hands the
/// callee a raw pointer to the storage cell without ever routing through
/// assignment lowering; those live at
/// [`lower_ref_path_call_arg`](super::expr::lower_ref_path_call_arg)/
/// [`lower_ref_projection_arg`](super::expr::lower_ref_projection_arg)/
/// [`try_lower_frame_local_auto_ref_stmt`](super::blocks::try_lower_frame_local_auto_ref_stmt)
/// instead. This four-locations-plus-inline-comments spread (this doc,
/// `context.rs`'s `as_binding_slots` field doc, and the two functions named
/// above) has already drifted out of sync once — see issue #2201's own
/// finding, filed from the #2122/#2191 review history.
///
/// Both immutability checks that apply to a resolved root — `as`-binding
/// immutability ([`reject_as_binding_write`], issue #2122) and `CONST`
/// immutability ([`reject_const_write`], issue #2201) — are threaded
/// through every one of these choke points individually for exactly this
/// reason: no single call site sees every write shape, so the check has to
/// be repeated at each one that resolves a root on its own.
pub(super) fn lower_assign_target(
    expr: &hir::Expr,
    ctx: &mut LowerCtx<'_>,
) -> Option<lir::AssignTarget> {
    match expr {
        hir::Expr::Path(path) => {
            let name = path_to_string(path);
            if let Some(slot) = ctx.temp_slot(&name) {
                // B1b (issue #1475): an `as` binding is immutable — see
                // `reject_as_binding_write`'s doc for the full choke-point
                // story (issue #2122: this is no longer the *only* site
                // that calls it).
                if reject_as_binding_write(slot, &name, path.range, ctx) {
                    return None;
                }
                let name_id = ctx.names.intern(&name);
                return Some(lir::AssignTarget::Temp(slot, name_id));
            }
            if let Some(info) = ctx.resolve_path(path.range) {
                // Issue #2201: `CONST` is immutable — see
                // `reject_const_write`'s doc for the full choke-point story.
                if reject_const_write(info, path.range, ctx) {
                    return None;
                }
                // Issue #3362, the write half of the same hole
                // (`expr::lower_path`'s `SymbolKind::Temp` arm is the read
                // half): a classic temp assigned from a position its
                // declaration has not run at resolves to a `LocalVar`-tagged
                // id that no global table registers, so `AssignTarget::
                // Global` produced an `unresolved global` link fault. The
                // frame's slot already exists (`alloc_temps` walks the whole
                // frame first), so write it.
                if info.kind == SymbolKind::Temp
                    && let Some(slot) = ctx.temp_slot_raw(&name)
                {
                    if reject_as_binding_write(slot, &name, path.range, ctx) {
                        return None;
                    }
                    let name_id = ctx.names.intern(&name);
                    return Some(lir::AssignTarget::Temp(slot, name_id));
                }
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

/// Issue #2122: the `as`-binding-immutability half of `lower_assign_target`'s
/// refusal, factored out so `blocks::lower_single_level_field_write` and
/// `blocks::lower_field_mutator` can call it too.
///
/// Those two functions resolve a `Param`/`Temp` root's slot themselves
/// (`ctx.temp_slot(&head_name)`, the *head* of a two-segment `p.field`
/// path) rather than routing through `lower_assign_target` — the resolution
/// that function performs for a bare path is keyed on the **whole**
/// dotted-path string (`path_to_string`, e.g. `"b.items"`), which is a
/// different (and for these two callers, wrong) lookup than the head-only
/// one they need, so calling `lower_assign_target` itself is not a drop-in
/// substitute here. This helper is the "equivalent shared check" the issue
/// asks for instead: the actual E148-diagnosing logic lives in exactly one
/// place, even though each caller still derives its own slot.
///
/// Returns `true` (diagnosed, caller must stop and lower nothing) when
/// `slot` is an immutable `as`-binding; `false` otherwise.
pub(super) fn reject_as_binding_write(
    slot: u16,
    name: &str,
    range: TextRange,
    ctx: &mut LowerCtx<'_>,
) -> bool {
    if ctx.as_binding_slots.contains(&slot) {
        ctx.diagnostics.push(crate::Diagnostic {
            file: ctx.file,
            range,
            message: format!(
                "{}: `{name}` is an `as` binding — it is immutable and cannot be \
                 assigned to or mutated in place",
                crate::DiagnosticCode::E148.title(),
            ),
            code: crate::DiagnosticCode::E148,
        });
        return true;
    }
    false
}

/// Issue #2201: the `CONST`-immutability check, shared across every choke
/// point that resolves a write root's `SymbolInfo` — the `CONST` analog of
/// [`reject_as_binding_write`] just above (same rationale for why this
/// needs its own call at each of `lower_assign_target`,
/// `blocks::lower_single_level_field_write`, `blocks::lower_field_mutator`,
/// `expr::lower_ref_path_call_arg`, and `expr::lower_ref_projection_arg`
/// individually rather than being centralized in one function: no single
/// one of those call sites sees every write shape — see
/// [`lower_assign_target`]'s own doc for the full enumeration).
///
/// ink semantics reject a `CONST` reassignment at compile time
/// (`ink/compiler/ParsedHierarchy/VariableAssignment.cs`, "Can't re-assign
/// to a constant") — this applies identically on both surfaces (`.ink` and
/// `.brink`), since `SymbolKind::Constant` is resolved the same way for
/// both frontends by the time LIR lowering runs.
///
/// A local (`Param`/`Temp`) that merely shares a `CONST`'s name is
/// unaffected: name resolution has already picked the innermost binding by
/// the time `info` is resolved (issue #2947, locals-first shadowing), so
/// `info.kind` is never `SymbolKind::Constant` for a shadowing local — this
/// function only ever sees the info that name resolution actually settled
/// on.
///
/// Returns `true` (diagnosed, caller must stop and lower nothing) when
/// `info.kind` is `SymbolKind::Constant`; `false` otherwise (a plain `VAR`
/// write stays legal, as does any read of a `CONST` — this is a write-path
/// check only, never called from an expression-position read).
pub(super) fn reject_const_write(
    info: &crate::symbols::SymbolInfo,
    range: TextRange,
    ctx: &mut LowerCtx<'_>,
) -> bool {
    if info.kind == SymbolKind::Constant {
        ctx.diagnostics.push(crate::Diagnostic {
            file: ctx.file,
            range,
            message: format!(
                "{}: `{}` is declared CONST — it cannot be reassigned, mutated, or passed by \
                 `ref`",
                crate::DiagnosticCode::E187.title(),
                info.name,
            ),
            code: crate::DiagnosticCode::E187,
        });
        return true;
    }
    false
}
