//! Content emission: LIR `Content` → opcodes + line table entries.

use brink_format::Opcode;
use brink_ir::lir;

use super::ContainerEmitter;

impl ContainerEmitter<'_> {
    pub(super) fn emit_content(&mut self, content: &lir::Content) {
        let has_newline = self.emit_content_parts(&content.parts);

        for tag in &content.tags {
            self.emit(Opcode::BeginTag);
            let idx = self.add_line(tag);
            self.emit(Opcode::EmitLine(idx));
            self.emit(Opcode::EndTag);
        }

        if has_newline {
            self.emit(Opcode::EmitNewline);
        }
    }

    /// Emit content parts for choice display text (no trailing newline).
    pub(super) fn emit_choice_content(&mut self, content: &lir::Content) {
        self.emit_content_parts(&content.parts);
    }

    /// Emit content parts. Returns `true` if a newline should follow
    /// (i.e., the content doesn't end with glue).
    fn emit_content_parts(&mut self, parts: &[lir::ContentPart]) -> bool {
        let mut ends_with_glue = false;

        for part in parts {
            ends_with_glue = false;
            match part {
                lir::ContentPart::Text(s) => {
                    let idx = self.add_line(s);
                    self.emit(Opcode::EmitLine(idx));
                }
                lir::ContentPart::Glue => {
                    self.emit(Opcode::Glue);
                    ends_with_glue = true;
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
            }
        }

        !ends_with_glue
    }
}
