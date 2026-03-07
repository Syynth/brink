//! Per-container bytecode emission.

use brink_format::{ChoiceFlags, Opcode, SequenceKind};
use brink_ir::lir;

use super::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_body(&mut self, stmts: &[lir::Stmt]) {
        for stmt in stmts {
            self.emit_stmt(stmt);
        }
    }

    fn emit_stmt(&mut self, stmt: &lir::Stmt) {
        match stmt {
            lir::Stmt::EmitContent(content) => self.emit_content(content),

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

            lir::Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.emit_expr(e);
                } else {
                    self.emit(Opcode::PushNull);
                }
                self.emit(Opcode::Return);
            }

            lir::Stmt::ChoiceSet(cs) => self.emit_choice_set(cs),

            lir::Stmt::Conditional(cond) => self.emit_conditional(cond),

            lir::Stmt::Sequence(seq) => self.emit_sequence(seq),

            lir::Stmt::ExprStmt(expr) => {
                self.emit_expr(expr);
                self.emit(Opcode::Pop);
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
                    lir::AssignTarget::Temp(slot) => self.emit(Opcode::GetTemp(*slot)),
                }
                self.emit_expr(value);
                self.emit(Opcode::Add);
            }
            brink_ir::AssignOp::Sub => {
                match target {
                    lir::AssignTarget::Global(id) => self.emit(Opcode::GetGlobal(*id)),
                    lir::AssignTarget::Temp(slot) => self.emit(Opcode::GetTemp(*slot)),
                }
                self.emit_expr(value);
                self.emit(Opcode::Subtract);
            }
        }

        match target {
            lir::AssignTarget::Global(id) => self.emit(Opcode::SetGlobal(*id)),
            lir::AssignTarget::Temp(slot) => self.emit(Opcode::SetTemp(*slot)),
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
        let flags = ChoiceFlags {
            has_condition: choice.condition.is_some(),
            has_start_content: choice.display.is_some(),
            has_choice_only_content: choice.display.is_some() && choice.output.is_some(),
            once_only: !choice.is_sticky,
            is_invisible_default: choice.is_fallback,
        };

        self.emit(Opcode::BeginChoice(flags, choice.target));

        // Condition
        if let Some(ref cond) = choice.condition {
            self.emit_expr(cond);
        }

        // Display content (start content + choice-only content)
        if let Some(ref display) = choice.display {
            self.emit(Opcode::BeginStringEval);
            self.emit_choice_content(display);
            self.emit(Opcode::EndStringEval);
        }

        // Output content
        if let Some(ref output) = choice.output {
            let mut line_text = String::new();
            for part in &output.parts {
                if let lir::ContentPart::Text(s) = part {
                    line_text.push_str(s);
                }
            }
            let idx = self.add_line(&line_text);
            self.emit(Opcode::ChoiceOutput(idx));
        }

        self.emit(Opcode::EndChoice);

        // Tags
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
