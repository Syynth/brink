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
        self.parts.push(OutputPart::Newline);
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
}
