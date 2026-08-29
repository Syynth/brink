use brink_format::CountingFlags;

use crate::hir;

use super::context::LowerCtx;
use super::expr::lower_expr;
use super::lir;
use super::recognize::build_source_location;

/// Lower HIR Content to LIR Content.
///
/// `source_location` is resolved here — not deferred to codegen (issue
/// #3181) — because `ctx.file_paths` (the `FileId → path` map) lives only
/// on this side of the pipeline; `brink-codegen-inkb` sees a bare
/// `lir::Program` with no such map to resolve one from later. Mirrors
/// `recognize::build_source_location`'s treatment of the exact same
/// `hir::Content::ptr` on the recognized-line path, so both paths answer
/// "where did this line come from" the same way.
pub fn lower_content(content: &hir::Content, ctx: &mut LowerCtx<'_>) -> lir::Content {
    let source_location = build_source_location(content, ctx);
    lir::Content {
        parts: lower_content_parts(&content.parts, ctx),
        tags: content
            .tags
            .iter()
            .map(|t| lower_content_parts(&t.parts, ctx))
            .collect(),
        source_location,
    }
}

/// Lower HIR content parts to LIR content parts (public for use by choice tag lowering).
pub fn lower_content_parts_pub(
    parts: &[hir::ContentPart],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::ContentPart> {
    lower_content_parts(parts, ctx)
}

/// This is the **general/unrecognized-content** fallback path (`EmitContent`
/// — runtime instructions, not a translatable line-table entry). A
/// [`hir::ContentPart::Span`] here gets *flattened*: its `children` splice
/// directly into the surrounding stream, one level up, and its `name`/
/// `attrs` are dropped. That is a deliberate, documented degradation, not a
/// silent data loss — see [`lower_content_part_into`]'s doc.
///
/// The path that actually preserves span structure is
/// `lir::lower::recognize::try_recognize`, which builds a real wire
/// `LinePart::Span` directly from `hir::Content` *before* a line ever
/// reaches this fallback (§4.4). Content only lands here when recognition
/// declines it — e.g. a span nesting something the recognizer doesn't admit
/// (§4.4's still-open "span admission" note) — and even then no text or
/// dynamic content is lost, only the span's presentational boundary is.
fn lower_content_parts(
    parts: &[hir::ContentPart],
    ctx: &mut LowerCtx<'_>,
) -> Vec<lir::ContentPart> {
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        lower_content_part_into(part, ctx, &mut out);
    }
    out
}

/// Lower one part, appending its result(s) to `out`. A plain part appends
/// exactly one; [`hir::ContentPart::Span`] appends its (recursively
/// lowered) children instead of itself — see [`lower_content_parts`]'s doc
/// for why flattening here is safe and deliberate.
fn lower_content_part_into(
    part: &hir::ContentPart,
    ctx: &mut LowerCtx<'_>,
    out: &mut Vec<lir::ContentPart>,
) {
    if let hir::ContentPart::Span(span) = part {
        for child in &span.children {
            lower_content_part_into(child, ctx, out);
        }
        return;
    }
    out.push(lower_content_part(part, ctx));
}

fn lower_content_part(part: &hir::ContentPart, ctx: &mut LowerCtx<'_>) -> lir::ContentPart {
    match part {
        hir::ContentPart::Text(t) => lir::ContentPart::Text(t.clone()),
        hir::ContentPart::Glue => lir::ContentPart::Glue,
        hir::ContentPart::Spring => lir::ContentPart::Spring,
        hir::ContentPart::Interpolation(expr) => {
            lir::ContentPart::Interpolation(lower_expr(expr, ctx))
        }
        hir::ContentPart::InlineConditional(cond) => {
            let branches = cond
                .branches
                .iter()
                .map(|b| {
                    // B1b (issue #1475): `{if EXPR as n: …}` written inline
                    // on a content line. The scope bracket spans condition
                    // + body only, so the binding dies at the arm boundary
                    // exactly as in the statement form — and because the
                    // bind rides the condition expression itself
                    // (`lir::ExprKind::OptionBind`), nothing has to be hoisted
                    // out of the content line to make room for it.
                    ctx.push_block_scope();
                    let condition = match (b.condition.as_ref(), b.binding.as_ref()) {
                        (Some(e), Some(binding)) => {
                            Some(super::blocks::lower_bound_condition(e, binding, ctx))
                        }
                        (Some(e), None) => Some(lower_expr(e, ctx)),
                        (None, _) => None,
                    };
                    let body = lower_inline_block(&b.body, ctx);
                    ctx.pop_block_scope();
                    lir::CondBranch { condition, body }
                })
                .collect();
            lir::ContentPart::InlineConditional(lir::Conditional {
                kind: lir::CondKind::InitialCondition,
                branches,
            })
        }
        hir::ContentPart::InlineSequence(seq) => lower_inline_sequence(seq, ctx),
        // `lower_content_part_into` intercepts every `Span` before it
        // reaches here (flattening it into `out` directly, since a span
        // lowers to zero-or-many parts, not exactly one) — this function's
        // only caller. Mirrors `try_recognize_template`'s own
        // already-validated `unreachable!` a few files over.
        hir::ContentPart::Span(_) => unreachable!("Span is intercepted by lower_content_part_into"),
    }
}

/// Lower an inline sequence into a wrapper container and return `EnterSequence`.
fn lower_inline_sequence(seq: &hir::Sequence, ctx: &mut LowerCtx<'_>) -> lir::ContentPart {
    // Count existing pending children to derive a unique sequence index.
    let seq_idx = ctx
        .pending_children
        .iter()
        .filter(|c| c.kind == lir::ContainerKind::Sequence)
        .count();
    // #3275 (stage 3a): a weave-line construct arrives pre-stamped (or
    // clone-derived) — read that id so the compiled container matches what
    // the pristine HIR carries. Content-embedded shapes the stamp walk
    // deliberately leaves alone (choice text, tags, span children, lambda
    // fragments) still arrive `None` and keep the derived-path mint.
    let wrapper_id = seq
        .container_id
        .unwrap_or_else(|| ctx.alloc_sequence_id(seq_idx));

    let branches = seq
        .branches
        .iter()
        .map(|b| lower_inline_block(&b.body, ctx))
        .collect();

    // The wrapper container and its one Sequence statement are both
    // synthesized *from* `seq` — stamp both with its own real provenance
    // (issue #3183), not the ambient left over from lowering the last
    // branch above.
    let provenance = ctx.enter_stmt(seq.ptr);
    let display_name = format!("s-{seq_idx}");
    let wrapper = lir::Container {
        id: wrapper_id,
        provenance,
        name: Some(display_name),
        kind: lir::ContainerKind::Sequence,
        params: Vec::new(),
        body: vec![lir::Stmt::new(
            lir::StmtKind::Sequence(lir::Sequence {
                kind: seq.kind,
                branches,
            }),
            provenance,
        )],
        children: Vec::new(),
        counting_flags: CountingFlags::VISITS | CountingFlags::COUNT_START_ONLY,
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
        local: false,
    };
    ctx.pending_children.push(wrapper);

    lir::ContentPart::EnterSequence(wrapper_id)
}

/// Lower a block in inline content context (no choice/gather children possible).
///
/// `hir::Stmt::LogicBlock` (a T1b `~ { … }` block, docs/t1b-surface-spec.md
/// §2) is intercepted here exactly like `lower_block_with_children` does at
/// the top level — it splices possibly-many `lir::Stmt`s via
/// `blocks::lower_logic_block`, which `stmts::lower_stmt`'s `Option<Stmt>`
/// return can't express. This branch is reachable in practice: an inline
/// multiline conditional/sequence that is *not* the first inline construct
/// on its content line (or one embedded in choice display/bracket/inner
/// text, which HIR normalization never touches — see `hir::normalize`)
/// keeps its `InlineConditional`/`InlineSequence` shape all the way to LIR
/// lowering instead of being lifted to a top-level `Stmt::Conditional`/
/// `Stmt::Sequence`, and its branches can legally contain a `LogicBlock`.
/// Before this fix that reached `stmts::lower_stmt`'s `debug_assert!`-guarded
/// "should be dispatched by `lower_block_with_children`" arm — a no-op in
/// release (silently dropping the block's statements) and a panic in debug
/// (see #578 review).
///
/// `hir::Stmt::Assignment` with an `Index` target (`~ a[i] = v`, issue
/// #2174 review finding) gets the same parallel guarded dispatch
/// `mod.rs`'s classic-line statement match added: `stmts::lower_stmt`'s
/// `Assignment` arm only resolves a bare `Path` target via
/// `lower_assign_target`, so an `Index` target silently dropped the whole
/// statement (`?` short-circuits to `None`) with zero diagnostics whenever
/// this inline path — not just `mod.rs`'s top-level classic-line dispatch —
/// was the one to reach it. `try_lower_indexed_assignment` is tried before
/// `try_lower_mutator_stmt` and the `stmts::lower_stmt` fallback (though,
/// per the paragraph below, after `try_lower_field_assignment`) so it can
/// also raise the non-suppressible `E074` for a struct-field-projected
/// root, mirroring `reject_field_projection_index_root`.
///
/// Issue #2222 — the remaining two-thirds of `mod.rs`'s three-helper
/// classic-line dispatch (`lower_block_with_children`, mirrored at
/// mod.rs:1359-1394) that PR #2211/#2174 didn't bring here: a `Path`
/// **field** assignment target (`try_lower_field_assignment`, tried
/// *before* the `Index` arm, matching `mod.rs`'s ordering) and a
/// **mutator** call statement (`try_lower_mutator_stmt`, over
/// `hir::Stmt::ExprStmt`). Before this fix, `~ p.hp = 99` inside a choice's
/// inline block (e.g. a bracket/inner-content conditional branch) never
/// reached `try_lower_field_assignment` — it fell through to
/// `stmts::lower_stmt`'s `lower_assign_target`, which resolves `p.hp` to
/// the bare root `p` and **overwrites the whole record** with the RHS
/// value instead of writing the field, faulting at runtime with
/// `NotARecord` the next time `p` was read as a struct. A mutator call
/// (`~ push(a, 9)`) in the same position was rejected outright with E056
/// ("collection mutator used in expression position") instead of lowering,
/// inconsistent with both the `~ { … }` block form and the top-level
/// classic line, which both accept it. `mod.rs` also dispatches a fourth
/// helper, `try_lower_frame_local_auto_ref_stmt` (issue #1531,
/// `g.hp.heal(5)`-shaped auto-ref calls) — that arm is NOT mirrored here;
/// it's a distinct, not-yet-confirmed parity gap, tracked separately (see
/// PR body / issue comment) rather than folded into this fix.
fn lower_inline_block(block: &hir::Block, ctx: &mut LowerCtx<'_>) -> Vec<lir::Stmt> {
    let mut stmts = Vec::new();
    for stmt in &block.stmts {
        if let hir::Stmt::LogicBlock(lb) = stmt {
            stmts.extend(super::blocks::lower_logic_block(lb, ctx));
        } else if let hir::Stmt::Assignment(a) = stmt
            && super::blocks::try_lower_field_assignment(a, ctx, &mut stmts)
        {
        } else if let hir::Stmt::Assignment(a) = stmt
            && super::blocks::try_lower_indexed_assignment(a, ctx, &mut stmts)
        {
        } else if let hir::Stmt::ExprStmt(e) = stmt
            && super::blocks::try_lower_postfix_stmt(e, ctx, &mut stmts)
        {
            // Issue #2903 — mirrors the same arm `mod.rs`'s top-level
            // classic-line dispatch added: an Index-operand postfix
            // (`a[0]++`) routes through `lower_indexed_assignment`, which
            // can splice several `lir::Stmt`s — the `stmts::lower_stmt`
            // fallback below returns only a single `Option<Stmt>` and would
            // truncate that RMW sequence to its harmless-but-non-mutating
            // first step, same shape of gap `try_lower_indexed_assignment`
            // above prevents for `~ a[i] = v` in this same inline context.
        } else if let hir::Stmt::ExprStmt(e) = stmt
            && super::blocks::try_lower_mutator_stmt(e, ctx, &mut stmts)
        {
        } else if let Some(s) = super::stmts::lower_stmt(stmt, ctx) {
            stmts.push(s);
        }
    }
    stmts
}
