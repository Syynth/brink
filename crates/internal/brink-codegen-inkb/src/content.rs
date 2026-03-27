//! Content emission: LIR `Content` → opcodes + line table entries.

use brink_format::Opcode;
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_recognized_line(&mut self, emission: &lir::ContentEmission) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::EmitLine(idx, 0));
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                // Evaluate slot expressions — each pushes one value onto the stack.
                // Function calls need composition: side-effect output + return value
                // are composed into a single FragmentRef so the line table entry
                // stays clean while the output order matches C#.
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EmitLine(idx, slot_exprs.len() as u8));
            }
        }

        for tag in &emission.tags {
            self.emit(Opcode::BeginTag);
            self.emit_content_parts(tag);
            self.emit(Opcode::EndTag);
        }
    }

    /// Emit a recognized line wrapped in `BeginFragment`/`EndFragment` with
    /// tags **inside** the fragment. Tags captured inside the fragment are
    /// stored on the `Fragment` struct so the runtime can route them to the
    /// consumer (e.g. `BeginChoice` pulls them onto the choice).
    pub(super) fn emit_fragment_recognized_line_with_tags(
        &mut self,
        emission: &lir::ContentEmission,
        extra_tags: &[Vec<lir::ContentPart>],
    ) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::BeginFragment);
                self.emit(Opcode::EmitLine(idx, 0));
                self.emit_tags(&emission.tags);
                self.emit_tags(extra_tags);
                self.emit(Opcode::EndFragment);
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::BeginFragment);
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EmitLine(idx, slot_exprs.len() as u8));
                self.emit_tags(&emission.tags);
                self.emit_tags(extra_tags);
                self.emit(Opcode::EndFragment);
            }
        }
    }

    /// Emit `BeginTag`/content/`EndTag` for each tag.
    pub(super) fn emit_tags(&mut self, tags: &[Vec<lir::ContentPart>]) {
        for tag in tags {
            self.emit(Opcode::BeginTag);
            self.emit_content_parts(tag);
            self.emit(Opcode::EndTag);
        }
    }

    /// Emit a recognized line as an `EvalLine` opcode (pushes result onto value stack).
    /// Used for choice display text promoted to a line table entry.
    pub(super) fn emit_eval_line(&mut self, emission: &lir::ContentEmission) {
        let slot_info = emission.metadata.slot_info.clone();
        let source_location = emission.metadata.source_location.clone();
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(
                    text,
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                self.emit(Opcode::EvalLine(idx, 0));
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                for expr in slot_exprs {
                    self.emit_slot_expr(expr);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EvalLine(idx, slot_exprs.len() as u8));
            }
        }
        // No tags for EvalLine — choice tags are emitted separately after EndChoice.
    }

    pub(super) fn emit_content(&mut self, content: &lir::Content) {
        self.emit_content_parts(&content.parts);

        for tag in &content.tags {
            self.emit(Opcode::BeginTag);
            self.emit_content_parts(tag);
            self.emit(Opcode::EndTag);
        }
    }

    /// Emit content parts for choice display text (no trailing newline).
    pub(super) fn emit_choice_content(&mut self, content: &lir::Content) {
        self.emit_content_parts(&content.parts);
    }

    /// Emit content parts — text, glue, interpolations, inline conditionals/sequences.
    pub(super) fn emit_content_parts(&mut self, parts: &[lir::ContentPart]) {
        for part in parts {
            match part {
                lir::ContentPart::Text(s) => {
                    // Strip boundary whitespace, emit Springs for word breaks.
                    let has_leading_ws = s.starts_with(char::is_whitespace);
                    let has_trailing_ws = s.ends_with(char::is_whitespace);
                    let trimmed = s.trim();

                    if has_leading_ws {
                        self.emit(Opcode::Spring);
                    }
                    if !trimmed.is_empty() {
                        let idx = self.add_line(trimmed);
                        self.emit(Opcode::EmitLine(idx, 0));
                    }
                    if has_trailing_ws && !trimmed.is_empty() {
                        self.emit(Opcode::Spring);
                    }
                    // If the string was entirely whitespace (trimmed is empty),
                    // the leading Spring covers it — no trailing Spring needed.
                }
                lir::ContentPart::Glue => {
                    self.emit(Opcode::Glue);
                }
                lir::ContentPart::Spring => {
                    self.emit(Opcode::Spring);
                }
                lir::ContentPart::Interpolation(expr) => {
                    self.emit_expr(expr, true);
                    self.emit(Opcode::EmitValue);
                }
                lir::ContentPart::InlineConditional(cond) => {
                    self.emit_conditional(cond);
                }
                lir::ContentPart::InlineSequence(seq) => {
                    self.emit_sequence(seq);
                }
                lir::ContentPart::EnterSequence(id) => {
                    self.emit(Opcode::EnterContainer(*id));
                }
            }
        }
    }

    /// Emit a slot expression for a template line.
    ///
    /// For function calls, uses the composition pattern: captures side-effect
    /// output in a fragment, then composes it with the return value into a
    /// single `FragmentRef`.  This ensures the line table entry stays clean
    /// (one slot) while side-effect text appears in the correct position
    /// within the resolved line.
    ///
    /// For non-call expressions, evaluates directly — the result goes on the
    /// value stack with no fragment overhead.
    fn emit_slot_expr(&mut self, expr: &lir::Expr) {
        if expr.is_function_call() {
            // Composition pattern:
            //   BeginFragment (compose)
            //     BeginFragment (side effects)
            //       Call func → side effects captured, return value on stack
            //     EndFragment  → store side effects → FragmentRef on stack
            //                    stack: [return_value, FragmentRef(side_effects)]
            //     EmitValue    → pop FragmentRef → emit side effects into compose
            //     EmitValue    → pop return_value → emit into compose
            //   EndFragment    → store composed → FragmentRef on stack
            self.emit(Opcode::BeginFragment);
            self.emit(Opcode::BeginFragment);
            self.emit_expr(expr, false);
            self.emit(Opcode::EndFragment);
            self.emit(Opcode::EmitValue);
            self.emit(Opcode::EmitValue);
            self.emit(Opcode::EndFragment);
        } else {
            self.emit_expr(expr, false);
        }
    }
}
