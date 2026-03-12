//! Per-container bytecode emission.

use brink_format::{ChoiceFlags, Opcode, SequenceKind};
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_body(&mut self, stmts: &[lir::Stmt]) {
        for stmt in stmts {
            self.emit_stmt(stmt);
        }
    }

    fn emit_stmt(&mut self, stmt: &lir::Stmt) {
        match stmt {
            lir::Stmt::EmitContent(content) => self.emit_content(content),
            lir::Stmt::ChoiceOutput(content) => {
                // Emit content parts + tags (tags appear in output after choosing).
                self.emit_content(content);
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
                    self.emit_expr(expr);
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
                    self.emit_expr(e);
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
                self.emit_expr(expr);
                self.emit(Opcode::Pop);
            }

            lir::Stmt::EndOfLine => {
                self.emit(Opcode::EmitNewline);
            }
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
                self.emit_expr(value);
            }
            brink_ir::AssignOp::Add => {
                match target {
                    lir::AssignTarget::Global(id) => self.emit(Opcode::GetGlobal(*id)),
                    lir::AssignTarget::Temp(slot, _) => self.emit(Opcode::GetTemp(*slot)),
                }
                self.emit_expr(value);
                self.emit(Opcode::Add);
            }
            brink_ir::AssignOp::Sub => {
                match target {
                    lir::AssignTarget::Global(id) => self.emit(Opcode::GetGlobal(*id)),
                    lir::AssignTarget::Temp(slot, _) => self.emit(Opcode::GetTemp(*slot)),
                }
                self.emit_expr(value);
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
        // Inside a conditional branch, the `done` is deferred to the outer
        // gather/container — emitting it here would block flow to the gather.
        if !self.in_conditional_branch {
            self.emit(Opcode::Done);
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

        // 1. Display text (combined start + choice_only) — pushed first
        if let Some(ref display) = display {
            self.emit(Opcode::BeginStringEval);
            self.emit_choice_content(display);
            self.emit(Opcode::EndStringEval);
        }

        // 2. Condition — pushed second (on top for runtime to pop first)
        if let Some(ref cond) = choice.condition {
            self.emit_expr(cond);
        }

        // 3. BeginChoice pops condition + display from stack
        self.emit(Opcode::BeginChoice(flags, choice.target));
        self.emit(Opcode::EndChoice);

        // Tags after EndChoice
        for tag in &choice.tags {
            self.emit(Opcode::BeginTag);
            self.emit_content_parts(tag);
            self.emit(Opcode::EndTag);
        }
    }

    pub(super) fn emit_conditional(&mut self, cond: &lir::Conditional) {
        let is_switch = matches!(&cond.kind, lir::CondKind::Switch(_));

        // For switch: push the switch expression once; each branch will
        // Duplicate + Equal against it.
        if let lir::CondKind::Switch(ref expr) = cond.kind {
            self.emit_expr(expr);
        }

        // Collect jump-to-end patch sites for each branch.
        let mut end_jumps: Vec<usize> = Vec::new();

        for (i, branch) in cond.branches.iter().enumerate() {
            let is_last = i == cond.branches.len() - 1;

            if let Some(ref condition) = branch.condition {
                if is_switch {
                    // Switch: duplicate switch value, push case value, compare.
                    self.emit(Opcode::Duplicate);
                    self.emit_expr(condition);
                    self.emit(Opcode::Equal);
                } else {
                    self.emit_expr(condition);
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

        if is_shuffle {
            // Shuffle: the runtime's handle_shuffle_sequence pops two values:
            //   num_elements (top) and seq_count (below).
            // Push them in order: seq_count first, then num_elements.
            self.emit(Opcode::CurrentVisitCount);
            self.emit(Opcode::PushInt(count as i32));
            self.emit(Opcode::Sequence(SequenceKind::Shuffle, 0));
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
