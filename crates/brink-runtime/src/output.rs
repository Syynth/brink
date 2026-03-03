//! Output buffer with glue handling.

/// A part of accumulated output.
#[derive(Debug, Clone)]
pub(crate) enum OutputPart {
    Text(String),
    Newline,
    Glue,
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

    /// Returns true if the buffer contains any non-whitespace text.
    fn has_content(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, OutputPart::Text(_)))
    }

    /// Returns true if the last part in the buffer is a newline.
    fn ends_in_newline(&self) -> bool {
        matches!(self.parts.last(), Some(OutputPart::Newline))
    }

    pub fn push_glue(&mut self) {
        self.parts.push(OutputPart::Glue);
    }

    /// Resolve glue and flush to a string.
    ///
    /// Glue removes the newline immediately before it and any leading
    /// whitespace on the text immediately after it, stitching text together.
    pub fn flush(&mut self) -> String {
        let parts = core::mem::take(&mut self.parts);

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
                        OutputPart::Glue => {}
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
                    after_glue = false;
                    out.push('\n');
                }
                OutputPart::Glue => {
                    after_glue = true;
                }
            }
        }

        out
    }
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
}
