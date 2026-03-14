//! Content emission: LIR `Content` → opcodes + line table entries.

use brink_format::Opcode;
use brink_ir::lir;

use crate::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_recognized_line(&mut self, emission: &lir::ContentEmission) {
        match &emission.line {
            lir::RecognizedLine::Plain(text) => {
                let idx = self.add_line_with_hash(text, emission.metadata.source_hash);
                self.emit(Opcode::EmitLine(idx, 0));
            }
        }

        for tag in &emission.tags {
            self.emit(Opcode::BeginTag);
            self.emit_content_parts(tag);
            self.emit(Opcode::EndTag);
        }
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
                    let idx = self.add_line(s);
                    self.emit(Opcode::EmitLine(idx, 0));
                }
                lir::ContentPart::Glue => {
                    self.emit(Opcode::Glue);
                }
                lir::ContentPart::Interpolation(expr) => {
                    self.emit_expr(expr);
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
