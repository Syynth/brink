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
                // Evaluate slot expressions first — they push values onto the stack.
                for expr in slot_exprs {
                    self.emit_expr(expr, true);
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

    /// Emit a recognized line wrapped in `BeginFragment`/`EndFragment`.
    /// Slot expressions are evaluated BEFORE the fragment starts so function
    /// calls produce values normally. Only the `EmitLine` is inside the fragment.
    pub(super) fn emit_fragment_recognized_line(&mut self, emission: &lir::ContentEmission) {
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
                self.emit(Opcode::EndFragment);
            }
            lir::RecognizedLine::Template {
                parts: template_parts,
                slot_exprs,
            } => {
                // Evaluate slot expressions BEFORE the fragment.
                // display=true ensures function calls get fragment-wrapped.
                for expr in slot_exprs {
                    self.emit_expr(expr, true);
                }
                let idx = self.add_template_line(
                    template_parts.clone(),
                    emission.metadata.source_hash,
                    slot_info,
                    source_location,
                );
                // Only the EmitLine is inside the fragment.
                self.emit(Opcode::BeginFragment);
                #[expect(clippy::cast_possible_truncation)]
                self.emit(Opcode::EmitLine(idx, slot_exprs.len() as u8));
                self.emit(Opcode::EndFragment);
            }
        }

        // Tags after the fragment (not inside it).
        for tag in &emission.tags {
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
                    self.emit_expr(expr, true);
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
}
