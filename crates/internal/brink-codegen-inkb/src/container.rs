//! Per-container bytecode emission.

use brink_format::{ChoiceFlags, Opcode, SequenceKind};
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_body(&mut self, stmts: &[lir::Stmt]) {
        let mut skip_next_newline = false;
        for (i, stmt) in stmts.iter().enumerate() {
            if skip_next_newline && matches!(stmt, lir::Stmt::EndOfLine) {
                skip_next_newline = false;
                continue;
            }
            skip_next_newline = false;

            // Suppress trailing newline on content when followed by a divert
            // (inline divert — the goto should come before any newline).
            // The LIR sequence is: EmitContent, Divert, EndOfLine.
            // We emit content without newline, emit the divert, then skip EndOfLine.
            if let lir::Stmt::EmitContent(content) = stmt
                && stmts
                    .get(i + 1)
                    .is_some_and(|s| matches!(s, lir::Stmt::Divert(_)))
            {
                self.emit_content_inline(content);
                skip_next_newline = true;
            } else {
                self.emit_stmt(stmt);
            }
        }
    }

    fn emit_stmt(&mut self, stmt: &lir::Stmt) {
        match stmt {
            lir::Stmt::EmitContent(content) => self.emit_content(content),
            lir::Stmt::ChoiceOutput {
                content,
                inline_divert,
            } => {
                self.emit_choice_content(content);
                if let Some(divert) = inline_divert {
                    self.emit_divert(divert);
                }
                self.emit(Opcode::EmitNewline);
            }

            lir::Stmt::Divert(divert) => self.emit_divert(divert),

            lir::Stmt::TunnelCall(tunnel) => {
                for target in &tunnel.targets {
                    for arg in &target.args {
                        self.emit_call_arg(arg);
                    }
                    match &target.target {
                        lir::DivertTarget::Container(id) => {
                            self.emit(Opcode::TunnelCall(*id));
                        }
                        lir::DivertTarget::Variable(id) => {
                            self.emit(Opcode::GetGlobal(*id));
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
                    lir::DivertTarget::Container(id) => {
                        self.emit(Opcode::ThreadCall(*id));
                    }
                    lir::DivertTarget::Variable(id) => {
                        self.emit(Opcode::GetGlobal(*id));
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

            lir::Stmt::Return { value, is_tunnel } => {
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
            lir::DivertTarget::Container(id) => {
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
                self.emit(Opcode::GetGlobal(*id));
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
        self.emit(Opcode::BeginChoiceSet);

        for choice in &cs.choices {
            self.emit_choice(choice);
        }

        self.emit(Opcode::EndChoiceSet);
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
            let idx = self.add_line(tag);
            self.emit(Opcode::EmitLine(idx));
            self.emit(Opcode::EndTag);
        }
    }

    pub(super) fn emit_conditional(&mut self, cond: &lir::Conditional) {
        // Collect jump-to-end patch sites for each branch.
        let mut end_jumps: Vec<usize> = Vec::new();

        for (i, branch) in cond.branches.iter().enumerate() {
            let is_last = i == cond.branches.len() - 1;

            if let Some(ref condition) = branch.condition {
                self.emit_expr(condition);
                // Placeholder JumpIfFalse — will be patched to skip this branch body.
                let patch_site = self.emit_jump_placeholder(Opcode::JumpIfFalse(0));

                self.emit_body(&branch.body);

                if !is_last {
                    // Jump to end of entire conditional
                    let end_site = self.emit_jump_placeholder(Opcode::Jump(0));
                    end_jumps.push(end_site);
                }

                // Patch the JumpIfFalse to land here (after body + optional Jump)
                self.patch_jump(patch_site);
            } else {
                // Else branch — no condition, just emit body
                self.emit_body(&branch.body);
            }
        }

        // Patch all end-of-branch jumps to land here
        for site in end_jumps {
            self.patch_jump(site);
        }
    }

    pub(super) fn emit_sequence(&mut self, seq: &lir::Sequence) {
        let kind = sequence_kind(seq.kind);
        #[expect(clippy::cast_possible_truncation)]
        let count = seq.branches.len() as u8;

        self.emit(Opcode::Sequence(kind, count));

        // Emit SequenceBranch placeholders — each will be patched with offset
        // to the end of that branch's body.
        let mut branch_placeholders: Vec<usize> = Vec::new();
        for _ in 0..count {
            let site = self.emit_jump_placeholder(Opcode::SequenceBranch(0));
            branch_placeholders.push(site);
        }

        // Emit branch bodies
        let mut end_jumps: Vec<usize> = Vec::new();
        for (i, branch) in seq.branches.iter().enumerate() {
            // Patch SequenceBranch to point to the start of this branch body
            self.patch_jump(branch_placeholders[i]);

            self.emit_body(branch);

            // Jump to end (skip remaining branches)
            if i < seq.branches.len() - 1 {
                let end_site = self.emit_jump_placeholder(Opcode::Jump(0));
                end_jumps.push(end_site);
            }
        }

        // Patch all end jumps
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

fn sequence_kind(kind: brink_ir::SequenceType) -> SequenceKind {
    if kind.contains(brink_ir::SequenceType::SHUFFLE) {
        SequenceKind::Shuffle
    } else if kind.contains(brink_ir::SequenceType::CYCLE) {
        SequenceKind::Cycle
    } else if kind.contains(brink_ir::SequenceType::ONCE) {
        SequenceKind::OnceOnly
    } else {
        // STOPPING or default
        SequenceKind::Stopping
    }
}
