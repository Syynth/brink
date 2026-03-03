//! Output buffer with glue handling.

/// A part of accumulated output.
#[derive(Debug, Clone)]
pub(crate) enum OutputPart {
    Text(String),
    Newline,
    Glue,
    /// Marks the start of a captured region (string eval, tag, or function call).
    Checkpoint,
}

/// Accumulates output text with glue resolution.
#[derive(Debug, Clone)]
pub(crate) struct OutputBuffer {
    pub parts: Vec<OutputPart>,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self { parts: Vec::new() }
    }

    pub fn push_text(&mut self, text: &str) {
        if !text.is_empty() {
            self.parts.push(OutputPart::Text(text.to_owned()));
        }
    }

    pub fn push_newline(&mut self) {
        // Suppress leading newlines (no content yet) and duplicate newlines,
        // matching the C# ink runtime's output stream filtering.
        if !self.has_content() || self.ends_in_newline() {
            return;
        }
        self.parts.push(OutputPart::Newline);
    }

    /// Returns true if the buffer contains any text after the last checkpoint
    /// (or from the start if no checkpoint exists).
    fn has_content(&self) -> bool {
        self.parts
            .iter()
            .rev()
            .take_while(|p| !matches!(p, OutputPart::Checkpoint))
            .any(|p| matches!(p, OutputPart::Text(_)))
    }

    /// Returns true if the last part in the buffer is a newline.
    fn ends_in_newline(&self) -> bool {
        matches!(self.parts.last(), Some(OutputPart::Newline))
    }

    pub fn push_glue(&mut self) {
        self.parts.push(OutputPart::Glue);
    }

    /// Push a checkpoint marker. Everything after it will be captured by
    /// [`end_capture`](Self::end_capture).
    pub fn begin_capture(&mut self) {
        self.parts.push(OutputPart::Checkpoint);
    }

    /// Pop everything back to (and including) the most recent checkpoint,
    /// resolve glue on the captured slice, and return the result as a string.
    ///
    /// Returns `None` if there is no checkpoint on the buffer.
    pub fn end_capture(&mut self) -> Option<String> {
        let cp_idx = self
            .parts
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))?;

        let captured: Vec<OutputPart> = self.parts.drain(cp_idx..).collect();
        // Skip the checkpoint itself (first element).
        let captured = &captured[1..];

        Some(resolve_parts(captured))
    }

    /// Remove the most recent checkpoint without capturing its content.
    /// Text after the checkpoint remains in the buffer.
    pub fn discard_capture(&mut self) {
        if let Some(cp_idx) = self
            .parts
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))
        {
            self.parts.remove(cp_idx);
        }
    }

    /// Resolve glue and flush to a string.
    ///
    /// Glue removes the newline immediately before it and any leading
    /// whitespace on the text immediately after it, stitching text together.
    pub fn flush(&mut self) -> String {
        debug_assert!(
            !self
                .parts
                .iter()
                .any(|p| matches!(p, OutputPart::Checkpoint)),
            "flush() called with active checkpoints"
        );
        let parts = core::mem::take(&mut self.parts);
        resolve_parts(&parts)
    }
}

/// Resolve glue in a slice of output parts and return the flattened string.
fn resolve_parts(parts: &[OutputPart]) -> String {
    // First pass: mark newlines that should be removed by glue.
    let mut remove = vec![false; parts.len()];
    for (i, part) in parts.iter().enumerate() {
        if matches!(part, OutputPart::Glue) {
            // Remove the nearest preceding newline.
            for j in (0..i).rev() {
                if remove[j] {
                    continue;
                }
                match &parts[j] {
                    OutputPart::Newline => {
                        remove[j] = true;
                        break;
                    }
                    OutputPart::Glue | OutputPart::Checkpoint => {}
                    OutputPart::Text(_) => break,
                }
            }
            // Mark glue itself for removal.
            remove[i] = true;
        }
    }

    let mut out = String::new();
    let mut after_glue = false;

    for (i, part) in parts.iter().enumerate() {
        if remove[i] {
            if matches!(part, OutputPart::Glue) {
                after_glue = true;
            }
            continue;
        }
        match part {
            OutputPart::Text(s) => {
                if after_glue {
                    out.push_str(s.trim_start());
                    after_glue = false;
                } else {
                    out.push_str(s);
                }
            }
            OutputPart::Newline => {
                if !after_glue {
                    out.push('\n');
                }
                // When after_glue, skip the newline (glue eats following newlines too).
            }
            OutputPart::Glue | OutputPart::Checkpoint => {
                after_glue = true;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_text() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        assert_eq!(buf.flush(), "hello");
    }

    #[test]
    fn text_with_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_text("world");
        assert_eq!(buf.flush(), "hello\nworld");
    }

    #[test]
    fn glue_removes_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text("world");
        assert_eq!(buf.flush(), "helloworld");
    }

    #[test]
    fn glue_trims_leading_whitespace() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text("  world");
        assert_eq!(buf.flush(), "helloworld");
    }

    #[test]
    fn double_flush_is_empty() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        let _ = buf.flush();
        assert_eq!(buf.flush(), "");
    }

    #[test]
    fn leading_newline_suppressed() {
        let mut buf = OutputBuffer::new();
        buf.push_newline();
        buf.push_text("hello");
        assert_eq!(buf.flush(), "hello");
    }

    #[test]
    fn duplicate_newline_suppressed() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_newline();
        buf.push_text("world");
        assert_eq!(buf.flush(), "hello\nworld");
    }

    #[test]
    fn leading_newline_after_flush_suppressed() {
        let mut buf = OutputBuffer::new();
        buf.push_text("first");
        let _ = buf.flush();
        // After flush, buffer is empty again — leading newline should be suppressed.
        buf.push_newline();
        buf.push_text("second");
        assert_eq!(buf.flush(), "second");
    }

    #[test]
    fn begin_end_capture_basic() {
        let mut buf = OutputBuffer::new();
        buf.push_text("before");
        buf.begin_capture();
        buf.push_text("captured");
        let result = buf.end_capture();
        assert_eq!(result, Some("captured".to_owned()));
        assert_eq!(buf.flush(), "before");
    }

    #[test]
    fn nested_captures() {
        let mut buf = OutputBuffer::new();
        buf.push_text("outer");
        buf.begin_capture();
        buf.push_text("middle");
        buf.begin_capture();
        buf.push_text("inner");
        let inner = buf.end_capture();
        assert_eq!(inner, Some("inner".to_owned()));
        let middle = buf.end_capture();
        assert_eq!(middle, Some("middle".to_owned()));
        assert_eq!(buf.flush(), "outer");
    }

    #[test]
    fn capture_with_glue() {
        let mut buf = OutputBuffer::new();
        buf.begin_capture();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text(" world");
        let result = buf.end_capture();
        assert_eq!(result, Some("helloworld".to_owned()));
    }

    #[test]
    fn end_capture_no_checkpoint_returns_none() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        assert_eq!(buf.end_capture(), None);
    }

    #[test]
    fn has_content_respects_checkpoint() {
        let mut buf = OutputBuffer::new();
        buf.push_text("before");
        buf.begin_capture();
        // No content after the checkpoint.
        assert!(!buf.has_content());
        buf.push_text("after");
        assert!(buf.has_content());
    }

    #[test]
    fn discard_capture_leaves_text() {
        let mut buf = OutputBuffer::new();
        buf.push_text("before");
        buf.begin_capture();
        buf.push_text("during");
        buf.discard_capture();
        // Text from the captured region stays in the buffer.
        assert_eq!(buf.flush(), "beforeduring");
    }

    #[test]
    fn discard_nested_capture() {
        let mut buf = OutputBuffer::new();
        buf.begin_capture();
        buf.push_text("outer");
        buf.begin_capture();
        buf.push_text("inner");
        // Discard inner capture; then end outer capture gets only "outer".
        buf.discard_capture();
        let result = buf.end_capture();
        assert_eq!(result, Some("outerinner".to_owned()));
    }

    /// Glue should eat the following newline, not just the preceding one.
    /// Pattern: `<>-<>` where glue appears on both sides of the dash.
    #[test]
    fn glue_eats_following_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("fifty");
        buf.push_newline();
        buf.push_glue();
        buf.push_text("-");
        buf.push_glue();
        buf.push_newline();
        buf.push_text("eight");
        assert_eq!(buf.flush(), "fifty-eight");
    }
}
