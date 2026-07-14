//! Per-container bytecode emission.

use brink_format::{ChoiceFlags, Opcode, SequenceKind};
use brink_ir::lir;

use crate::{CodegenError, ContainerEmitter, LoopCtx};

impl ContainerEmitter<'_> {
    pub(super) fn emit_body(&mut self, stmts: &[lir::Stmt]) {
        for stmt in stmts {
            self.emit_stmt(stmt);
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one match arm per LIR Stmt variant; splitting would obscure the dispatch"
    )]
    fn emit_stmt(&mut self, stmt: &lir::Stmt) {
        match stmt {
            lir::Stmt::EmitContent(content) => self.emit_content(content),
            lir::Stmt::EmitLine(emission) => self.emit_recognized_line(emission),
            lir::Stmt::EvalLine(emission) => self.emit_eval_line(emission),
            lir::Stmt::ChoiceOutput {
                content, emission, ..
            } => {
                if let Some(em) = emission {
                    // Recognized output — emit as a line table entry.
                    self.emit_recognized_line(em);
                } else {
                    // Fallback: emit content parts + tags inline.
                    self.emit_content(content);
                }
            }

            lir::Stmt::Divert(divert) => self.emit_divert(divert),

            lir::Stmt::TunnelCall(tunnel) => {
                for target in &tunnel.targets {
                    for arg in &target.args {
                        self.emit_call_arg(arg);
                    }
                    match &target.target {
                        lir::DivertTarget::Address(id) => {
                            self.emit(Opcode::TunnelCall(*id));
                        }
                        lir::DivertTarget::Variable(id) => {
                            self.emit(Opcode::GetGlobal(*id));
                            self.emit(Opcode::TunnelCallVariable);
                        }
                        lir::DivertTarget::VariableTemp(slot, _) => {
                            self.emit(Opcode::GetTemp(*slot));
                            self.emit(Opcode::TunnelCallVariable);
                        }
                        lir::DivertTarget::Done => self.emit(Opcode::Done),
                        lir::DivertTarget::End => self.emit(Opcode::End),
                    }
                }
            }

            lir::Stmt::ThreadStart(thread) => {
                for arg in &thread.args {
                    self.emit_call_arg(arg);
                }
                match &thread.target {
                    lir::DivertTarget::Address(id) => {
                        self.emit(Opcode::ThreadCall(*id));
                    }
                    lir::DivertTarget::Variable(id) => {
                        self.emit(Opcode::GetGlobal(*id));
                        self.emit(Opcode::GotoVariable);
                    }
                    lir::DivertTarget::VariableTemp(slot, _) => {
                        self.emit(Opcode::GetTemp(*slot));
                        self.emit(Opcode::GotoVariable);
                    }
                    lir::DivertTarget::Done => self.emit(Opcode::Done),
                    lir::DivertTarget::End => self.emit(Opcode::End),
                }
            }

            lir::Stmt::DeclareTemp { slot, value, .. } => {
                if let Some(expr) = value {
                    self.emit_expr(expr, false);
                } else {
                    self.emit(Opcode::PushNull);
                }
                self.emit(Opcode::DeclareTemp(*slot));
            }

            lir::Stmt::Assign { target, op, value } => {
                self.emit_assign(target, *op, value);
            }

            lir::Stmt::Return {
                value,
                is_tunnel,
                args,
            } => {
                for arg in args {
                    self.emit_call_arg(arg);
                }
                if let Some(e) = value {
                    self.emit_expr(e, false);
                } else {
                    self.emit(Opcode::PushNull);
                }
                if *is_tunnel {
                    self.emit(Opcode::TunnelReturn);
                } else {
                    self.emit(Opcode::Return);
                }
            }

            lir::Stmt::ChoiceSet(cs) => self.emit_choice_set(cs),

            lir::Stmt::Conditional(cond) => self.emit_conditional(cond),

            lir::Stmt::Sequence(seq) => self.emit_sequence(seq),

            lir::Stmt::EnterContainer(id) => {
                self.emit(Opcode::EnterContainer(*id));
            }

            lir::Stmt::ExprStmt(expr) => {
                self.emit_expr(expr, false);
                self.emit(Opcode::Pop);
            }

            lir::Stmt::EndOfLine => {
                self.emit(Opcode::EmitNewline);
            }

            lir::Stmt::LogicWhile(w) => self.emit_logic_while(w),

            lir::Stmt::LogicBreak => {
                // Patched to land just after the whole loop once it's fully
                // emitted (`emit_logic_while`). LIR lowering (E057,
                // `brink-ir::lir::lower::blocks`) rejects `break` outside
                // any loop and never emits this statement in that case — it
                // is a non-suppressible LIR-lowering-time compile error, not
                // a suppressible analysis diagnostic, so a well-formed
                // `Program` never contains an unguarded `LogicBreak`. Trust
                // that invariant the same way every other statement in this
                // file trusts LIR is well-formed (no other arm here
                // defensively re-checks its input) — see #577 review, which
                // replaced a silent `Nop` degradation with a real, upstream
                // error path (E057).
                //
                // That upstream guarantee is enforced by a *different*
                // compiler stage, though, and codegen has no way to verify
                // it structurally beyond this checkpoint — "safe today only
                // by construction" (#586 review). If a future or
                // refactored LIR producer (or, as here, a hand-assembled
                // `Program` in a test) ever hands codegen a `LogicBreak`
                // with an empty `loop_stack` anyway, there is no patch
                // target for the jump this statement would otherwise emit:
                // silently falling through to `Opcode::Jump(0)` would
                // corrupt the bytecode with a jump to the start of the
                // container, indistinguishable from a valid jump. Fail
                // loudly instead — and skip emitting the dangling jump
                // placeholder entirely, so no unpatched opcode ever lands
                // in the output.
                if self.loop_stack.is_empty() {
                    self.errors.push(CodegenError::new(
                        "codegen: `break` (LogicBreak) reached codegen outside any loop \
                         context — LIR lowering (E057) should have rejected this before it \
                         reached codegen; refusing to emit an unpatched jump (#586)",
                    ));
                } else {
                    let site = self.emit_jump_placeholder(Opcode::Jump(0));
                    if let Some(ctx) = self.loop_stack.last_mut() {
                        ctx.break_patches.push(site);
                    }
                }
            }

            lir::Stmt::LogicContinue => {
                // See `LogicBreak` above — identical reasoning, `continue`'s
                // own jump target.
                if self.loop_stack.is_empty() {
                    self.errors.push(CodegenError::new(
                        "codegen: `continue` (LogicContinue) reached codegen outside any loop \
                         context — LIR lowering (E057) should have rejected this before it \
                         reached codegen; refusing to emit an unpatched jump (#586)",
                    ));
                } else {
                    let site = self.emit_jump_placeholder(Opcode::Jump(0));
                    if let Some(ctx) = self.loop_stack.last_mut() {
                        ctx.continue_patches.push(site);
                    }
                }
            }
        }
    }

    /// Compile a `while`/desugared-`for` loop to a flat backward-jump loop
    /// in the same container's bytecode — no child container, since block
    /// bodies never contain choices/gathers that would need one.
    ///
    /// ```text
    /// loop_start: <condition>
    ///             JumpIfFalse loop_end   ; jf_exit
    ///             <body>                 ; break -> loop_end, continue -> post_start
    /// post_start: <post>                 ; empty for a plain `while`
    ///             Jump loop_start
    /// loop_end:
    /// ```
    #[expect(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    fn emit_logic_while(&mut self, w: &lir::LogicWhile) {
        let loop_start = self.bytecode.len();
        self.emit_expr(&w.condition, false);
        let jf_exit = self.emit_jump_placeholder(Opcode::JumpIfFalse(0));

        self.loop_stack.push(LoopCtx {
            break_patches: Vec::new(),
            continue_patches: Vec::new(),
        });
        self.emit_body(&w.body);
        let LoopCtx {
            break_patches,
            continue_patches,
        } = self.loop_stack.pop().unwrap_or(LoopCtx {
            break_patches: Vec::new(),
            continue_patches: Vec::new(),
        });
        // `continue` lands here, right before `post` — for a plain `while`
        // (`post` empty) that's exactly the backward jump below, i.e.
        // "re-check the condition"; for a desugared `for`, that's the index
        // increment, so `continue` still advances the loop instead of
        // spinning forever.
        for site in continue_patches {
            self.patch_jump(site);
        }
        self.emit_body(&w.post);

        // Backward jump to re-check the condition.
        let relative = loop_start as i32 - (self.bytecode.len() as i32 + 5);
        self.emit(Opcode::Jump(relative));

        // `loop_end`: both the false-condition exit and every `break` land here.
        self.patch_jump(jf_exit);
        for site in break_patches {
            self.patch_jump(site);
        }
    }

    fn emit_divert(&mut self, divert: &lir::Divert) {
        match &divert.target {
            lir::DivertTarget::Address(id) => {
                if divert.args.is_empty() {
                    self.emit(Opcode::Goto(*id));
                } else {
                    for arg in &divert.args {
                        self.emit_call_arg(arg);
                    }
                    self.emit(Opcode::Goto(*id));
                }
            }
            lir::DivertTarget::Variable(id) => {
                for arg in &divert.args {
                    self.emit_call_arg(arg);
                }
                self.emit(Opcode::GetGlobal(*id));
                self.emit(Opcode::GotoVariable);
            }
            lir::DivertTarget::VariableTemp(slot, _) => {
                for arg in &divert.args {
                    self.emit_call_arg(arg);
                }
                self.emit(Opcode::GetTemp(*slot));
                self.emit(Opcode::GotoVariable);
            }
            lir::DivertTarget::Done => self.emit(Opcode::Done),
            lir::DivertTarget::End => self.emit(Opcode::End),
        }
    }

    fn emit_assign(
        &mut self,
        target: &lir::AssignTarget,
        op: brink_ir::AssignOp,
        value: &lir::Expr,
    ) {
        match op {
            brink_ir::AssignOp::Set => {
                self.emit_expr(value, false);
            }
            brink_ir::AssignOp::Add => {
                match target {
                    lir::AssignTarget::Global(id) => self.emit(Opcode::GetGlobal(*id)),
                    lir::AssignTarget::Temp(slot, _) => self.emit(Opcode::GetTemp(*slot)),
                }
                self.emit_expr(value, false);
                self.emit(Opcode::Add);
            }
            brink_ir::AssignOp::Sub => {
                match target {
                    lir::AssignTarget::Global(id) => self.emit(Opcode::GetGlobal(*id)),
                    lir::AssignTarget::Temp(slot, _) => self.emit(Opcode::GetTemp(*slot)),
                }
                self.emit_expr(value, false);
                self.emit(Opcode::Subtract);
            }
        }

        match target {
            lir::AssignTarget::Global(id) => self.emit(Opcode::SetGlobal(*id)),
            lir::AssignTarget::Temp(slot, _) => self.emit(Opcode::SetTemp(*slot)),
        }
    }

    fn emit_choice_set(&mut self, cs: &lir::ChoiceSet) {
        for choice in &cs.choices {
            self.emit_choice(choice);
        }

        // Yield to present pending choices. Without this, execution falls
        // through to whatever follows the choice set in the same container
        // (e.g., a gather's `goto end`), terminating the story before the
        // VM can present choices.
        //
        // Uses `Yield` (not `Done`) so `did_safe_exit` is NOT set — if
        // no choices are pending, the story ran out of content.
        //
        // Inside a conditional branch, the yield is deferred to the outer
        // gather/container — emitting it here would block flow to the gather.
        if !self.in_conditional_branch {
            self.emit(Opcode::Yield);
        }
    }

    fn emit_choice(&mut self, choice: &lir::Choice) {
        let has_start = choice.start_content.is_some();
        let has_choice_only = choice.choice_only_content.is_some();

        let display = combine_choice_content(
            choice.start_content.as_ref(),
            choice.choice_only_content.as_ref(),
        );

        let flags = ChoiceFlags {
            has_condition: choice.condition.is_some(),
            has_start_content: has_start,
            has_choice_only_content: has_choice_only,
            once_only: !choice.is_sticky,
            is_invisible_default: choice.is_fallback,
        };

        // All evaluation BEFORE BeginChoice.
        // Push order: display first, condition second. The runtime pops
        // condition first (from top), then display.

        // 1. Display text (combined start + choice_only) — pushed first.
        //    Tags must be emitted INSIDE the display eval so the runtime
        //    routes them to the choice (via fragment tags or current_tags),
        //    not to the output line.
        if let Some(ref emission) = choice.display_emission {
            // Recognized display — fragment with tags inside.
            self.emit_fragment_recognized_line_with_tags(emission, &choice.tags);
        } else if let Some(ref display) = display {
            // Unrecognized display — string eval with tags inside the capture.
            self.emit(Opcode::BeginStringEval);
            self.emit_choice_content(display);
            self.emit_tags(&display.tags);
            self.emit_tags(&choice.tags);
            self.emit(Opcode::EndStringEval);
        } else if !choice.tags.is_empty() {
            // No display content but tags exist — wrap in string eval so
            // the capture context routes them to current_tags.
            self.emit(Opcode::BeginStringEval);
            self.emit_tags(&choice.tags);
            self.emit(Opcode::EndStringEval);
        }

        // 2. Condition — pushed second (on top for runtime to pop first)
        if let Some(ref cond) = choice.condition {
            self.emit_expr(cond, false);
        }

        // 3. BeginChoice pops condition + display from stack
        self.emit(Opcode::BeginChoice(flags, choice.target));
        self.emit(Opcode::EndChoice);
    }

    pub(super) fn emit_conditional(&mut self, cond: &lir::Conditional) {
        let is_switch = matches!(&cond.kind, lir::CondKind::Switch(_));

        // For switch: push the switch expression once; each branch will
        // Duplicate + Equal against it.
        if let lir::CondKind::Switch(ref expr) = cond.kind {
            self.emit_expr(expr, false);
        }

        // Collect jump-to-end patch sites for each branch.
        let mut end_jumps: Vec<usize> = Vec::new();

        for (i, branch) in cond.branches.iter().enumerate() {
            let is_last = i == cond.branches.len() - 1;

            if let Some(ref condition) = branch.condition {
                if is_switch {
                    // Switch: duplicate switch value, push case value, compare.
                    self.emit(Opcode::Duplicate);
                    self.emit_expr(condition, false);
                    self.emit(Opcode::Equal);
                } else {
                    self.emit_expr(condition, false);
                }
                // Placeholder JumpIfFalse — will be patched to skip this branch body.
                let patch_site = self.emit_jump_placeholder(Opcode::JumpIfFalse(0));

                if is_switch {
                    // Pop the switch value inside the taken branch (it was
                    // duplicated, so one copy remains on the stack).
                    self.emit(Opcode::Pop);
                }

                let prev = self.in_conditional_branch;
                self.in_conditional_branch = true;
                self.emit_body(&branch.body);
                self.in_conditional_branch = prev;

                if !is_last || is_switch {
                    // Jump to end of entire conditional.
                    // For switch: the last conditional branch must also jump
                    // past the cleanup Pop emitted for "no branch taken".
                    let end_site = self.emit_jump_placeholder(Opcode::Jump(0));
                    end_jumps.push(end_site);
                }

                // Patch the JumpIfFalse to land here (after body + optional Jump)
                self.patch_jump(patch_site);
            } else {
                // Else branch — no condition, just emit body.
                if is_switch {
                    // Pop the switch value before the else body.
                    self.emit(Opcode::Pop);
                }
                let prev = self.in_conditional_branch;
                self.in_conditional_branch = true;
                self.emit_body(&branch.body);
                self.in_conditional_branch = prev;
            }
        }

        // If no branch was taken (and there's no else), pop the switch value.
        if is_switch && !cond.branches.iter().any(|b| b.condition.is_none()) {
            self.emit(Opcode::Pop);
        }

        // Patch all end-of-branch jumps to land here
        for site in end_jumps {
            self.patch_jump(site);
        }
    }

    #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    pub(super) fn emit_sequence(&mut self, seq: &lir::Sequence) {
        let count = seq.branches.len();
        let is_shuffle = seq.kind.contains(brink_ir::SequenceType::SHUFFLE);
        let mut exhaustion_skip: Option<usize> = None;

        if is_shuffle {
            let is_once = seq.kind.contains(brink_ir::SequenceType::ONCE);
            let is_stopping = seq.kind.contains(brink_ir::SequenceType::STOPPING);

            if is_once {
                // shuffle once: clamp visit count to N, skip all branches when exhausted
                self.emit(Opcode::CurrentVisitCount);
                self.emit(Opcode::PushInt(count as i32));
                self.emit(Opcode::Min);
                self.emit(Opcode::Duplicate);
                self.emit(Opcode::PushInt(count as i32));
                self.emit(Opcode::Equal);
                self.emit(Opcode::Not);
                let site = self.emit_jump_placeholder(Opcode::JumpIfFalse(0));
                // Not exhausted: do shuffle
                self.emit(Opcode::PushInt(count as i32));
                self.emit(Opcode::Sequence(SequenceKind::Shuffle, 0));
                exhaustion_skip = Some(site);
            } else if is_stopping {
                // shuffle stopping: clamp to N-1, skip shuffle when exhausted (pin to last)
                // When exhausted: clamped value (N-1) stays on stack → matches last branch
                // When not exhausted: shuffle among first N-1 branches only
                self.emit(Opcode::CurrentVisitCount);
                self.emit(Opcode::PushInt(count as i32 - 1));
                self.emit(Opcode::Min);
                self.emit(Opcode::Duplicate);
                self.emit(Opcode::PushInt(count as i32 - 1));
                self.emit(Opcode::Equal);
                self.emit(Opcode::Not);
                let site = self.emit_jump_placeholder(Opcode::JumpIfFalse(0));
                // Not exhausted: shuffle among first N-1 branches using clamped value as seq_count
                self.emit(Opcode::PushInt(count as i32 - 1));
                self.emit(Opcode::Sequence(SequenceKind::Shuffle, 0));
                // Patch exhaustion jump to land here (right before branch switch)
                self.patch_jump(site);
            } else {
                // Plain shuffle or cycle shuffle
                self.emit(Opcode::CurrentVisitCount);
                self.emit(Opcode::PushInt(count as i32));
                self.emit(Opcode::Sequence(SequenceKind::Shuffle, 0));
            }
        } else {
            // Non-shuffle: use CurrentVisitCount + math to compute branch index.
            self.emit(Opcode::CurrentVisitCount);

            if seq.kind.contains(brink_ir::SequenceType::CYCLE) {
                // cycle: index = visit_count % count
                self.emit(Opcode::PushInt(count as i32));
                self.emit(Opcode::Modulo);
            } else if seq.kind.contains(brink_ir::SequenceType::ONCE) {
                // once: index = min(visit_count, count) — when index == count, no branch taken
                self.emit(Opcode::PushInt(count as i32));
                self.emit(Opcode::Min);
            } else {
                // stopping (default): index = min(visit_count, count - 1)
                self.emit(Opcode::PushInt(count as i32 - 1));
                self.emit(Opcode::Min);
            }
        }

        // Switch pattern: for each branch, Duplicate/PushInt(i)/Equal/JumpIfFalse
        let mut end_jumps: Vec<usize> = Vec::new();
        let mut skip_sites: Vec<usize> = Vec::new();

        for (i, branch) in seq.branches.iter().enumerate() {
            // Patch previous skip to land here
            if let Some(site) = skip_sites.pop() {
                self.patch_jump(site);
            }

            self.emit(Opcode::Duplicate);
            self.emit(Opcode::PushInt(i as i32));
            self.emit(Opcode::Equal);
            let skip_site = self.emit_jump_placeholder(Opcode::JumpIfFalse(0));

            // Pop the duplicated index value
            self.emit(Opcode::Pop);

            self.emit_body(branch);

            // Jump to the Nop at end (skip remaining branches)
            let end_site = self.emit_jump_placeholder(Opcode::Jump(0));
            end_jumps.push(end_site);

            skip_sites.push(skip_site);
        }

        // Patch last skip — no match (once-only exhausted, or shuffle overflow)
        if let Some(site) = skip_sites.pop() {
            self.patch_jump(site);
        }
        // Patch shuffle-once exhaustion skip to land here (at Pop, skipping all branches)
        if let Some(site) = exhaustion_skip {
            self.patch_jump(site);
        }
        // Pop unmatched index
        self.emit(Opcode::Pop);

        // Landing target for all taken branches
        self.emit(Opcode::Nop);
        for site in end_jumps {
            self.patch_jump(site);
        }
    }
}

/// Reconstruct combined content from two optional parts (e.g. start + bracket).
fn combine_choice_content(
    a: Option<&lir::Content>,
    b: Option<&lir::Content>,
) -> Option<lir::Content> {
    match (a, b) {
        (None, None) => None,
        (Some(content), None) | (None, Some(content)) => Some(content.clone()),
        (Some(a_content), Some(b_content)) => {
            let mut parts = a_content.parts.clone();
            parts.extend(b_content.parts.clone());
            let mut tags = a_content.tags.clone();
            tags.extend(b_content.tags.clone());
            Some(lir::Content { parts, tags })
        }
    }
}
