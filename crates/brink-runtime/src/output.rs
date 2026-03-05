//! Output buffer with glue handling.

/// A part of accumulated output.
#[derive(Debug, Clone)]
pub(crate) enum OutputPart {
    Text(String),
    Newline,
    Glue,
    /// Marks the start of a captured region (string eval, tag, or function call).
    Checkpoint,
    /// A tag associated with the current line of output.
    Tag(String),
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
        if text.is_empty() {
            return;
        }
        // Suppress whitespace-only text when there's no content yet,
        // matching the C# ink runtime's output stream filtering.
        // This handles leading spaces after choice selection (`"^ "`).
        if !self.has_content() && text.trim().is_empty() {
            return;
        }
        // Collapse adjacent whitespace at text boundaries: if the
        // previous text part ends with whitespace and this text starts
        // with whitespace, trim the leading whitespace from this text.
        let text = if text.starts_with(char::is_whitespace) && self.ends_in_whitespace() {
            text.trim_start()
        } else {
            text
        };
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

    /// Returns true if the last part is text ending with whitespace.
    /// Only checks the immediately preceding part — intervening Glue or
    /// Newline parts mean the glue system handles the join instead.
    fn ends_in_whitespace(&self) -> bool {
        matches!(
            self.parts.last(),
            Some(OutputPart::Text(s)) if s.ends_with(char::is_whitespace)
        )
    }

    pub fn push_glue(&mut self) {
        self.parts.push(OutputPart::Glue);
    }

    /// Push a tag associated with the current output line.
    pub fn push_tag(&mut self, tag: String) {
        self.parts.push(OutputPart::Tag(tag));
    }

    /// Returns true if the buffer contains any checkpoint markers.
    pub fn has_checkpoint(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, OutputPart::Checkpoint))
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

    /// Resolve glue and flush to a string (ignoring tags).
    ///
    /// Glue removes the newline immediately before it and any leading
    /// whitespace on the text immediately after it, stitching text together.
    #[cfg(test)]
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

    /// Resolve glue and flush to structured per-line output.
    ///
    /// Each returned element is `(line_text, line_tags)`. Tags are associated
    /// with the line they appear on in the output stream.
    pub fn flush_lines(&mut self) -> Vec<(String, Vec<String>)> {
        debug_assert!(
            !self
                .parts
                .iter()
                .any(|p| matches!(p, OutputPart::Checkpoint)),
            "flush_lines() called with active checkpoints"
        );
        let parts = core::mem::take(&mut self.parts);
        resolve_lines(&parts)
    }
}

/// Resolve glue in a slice of output parts and return the flattened string.
fn resolve_parts(parts: &[OutputPart]) -> String {
    // First pass: mark newlines that should be removed by glue.
    let mut remove = vec![false; parts.len()];
    for (i, part) in parts.iter().enumerate() {
        if matches!(part, OutputPart::Glue) {
            // Remove the nearest preceding newline, skipping whitespace-only text.
            for j in (0..i).rev() {
                if remove[j] {
                    continue;
                }
                match &parts[j] {
                    OutputPart::Newline => {
                        remove[j] = true;
                        break;
                    }
                    OutputPart::Glue | OutputPart::Checkpoint | OutputPart::Tag(_) => {}
                    OutputPart::Text(s) if s.trim().is_empty() => {}
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
                out.push_str(s);
                // Only clear after_glue for non-whitespace text; whitespace-only
                // text should not prevent glue from eating a following newline.
                if !s.trim().is_empty() {
                    after_glue = false;
                }
            }
            OutputPart::Newline => {
                if !after_glue {
                    // Trim trailing whitespace before the newline, matching
                    // the C# ink runtime's output cleanup.
                    let trimmed_len = out.trim_end().len();
                    out.truncate(trimmed_len);
                    out.push('\n');
                }
                // When after_glue, skip the newline (glue eats following newlines too).
            }
            OutputPart::Glue | OutputPart::Checkpoint | OutputPart::Tag(_) => {
                after_glue = true;
            }
        }
    }

    out
}

/// Resolve glue and split into per-line output with associated tags.
///
/// Each returned element is `(line_text, line_tags)`. Tags that appear
/// in the stream associate with the current line (the line being built
/// when the tag is encountered).
fn resolve_lines(parts: &[OutputPart]) -> Vec<(String, Vec<String>)> {
    if parts.is_empty() {
        return Vec::new();
    }

    // First pass: mark newlines/glue for removal (same logic as resolve_parts).
    let mut remove = vec![false; parts.len()];
    for (i, part) in parts.iter().enumerate() {
        if matches!(part, OutputPart::Glue) {
            for j in (0..i).rev() {
                if remove[j] {
                    continue;
                }
                match &parts[j] {
                    OutputPart::Newline => {
                        remove[j] = true;
                        break;
                    }
                    OutputPart::Glue | OutputPart::Checkpoint | OutputPart::Tag(_) => {}
                    OutputPart::Text(s) if s.trim().is_empty() => {}
                    OutputPart::Text(_) => break,
                }
            }
            remove[i] = true;
        }
    }

    let mut lines: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_text = String::new();
    let mut current_tags: Vec<String> = Vec::new();
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
                current_text.push_str(s);
                if !s.trim().is_empty() {
                    after_glue = false;
                }
            }
            OutputPart::Newline => {
                if !after_glue {
                    // Trim trailing whitespace before the newline.
                    let trimmed = current_text.trim_end().to_string();
                    lines.push((trimmed, std::mem::take(&mut current_tags)));
                    current_text = String::new();
                }
            }
            OutputPart::Tag(tag) => {
                current_tags.push(tag.clone());
            }
            OutputPart::Glue | OutputPart::Checkpoint => {
                after_glue = true;
            }
        }
    }

    // Always push the final line — even if empty — so that a trailing
    // Newline part produces a trailing `\n` when the lines are joined.
    let trimmed = current_text.trim_end().to_string();
    lines.push((trimmed, current_tags));

    lines
}

/// Clean inline whitespace in the output text, matching the reference ink
/// runtime's `CleanOutputWhitespace`:
///  - Removes all leading inline whitespace (spaces/tabs) from each line
///  - Removes all trailing inline whitespace before `\n` or end of string
///  - Collapses consecutive space/tab runs within a line to a single space
pub(crate) fn clean_output_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws_start: Option<usize> = None;
    let mut start_of_line: usize = 0;

    for (i, c) in s.char_indices() {
        let is_inline_ws = c == ' ' || c == '\t';

        if is_inline_ws && ws_start.is_none() {
            ws_start = Some(i);
        }

        if !is_inline_ws {
            // Emit a single space for a whitespace run, but only if:
            //  - It's not at the start of the string (ws_start > 0)
            //  - It's not at the start of the current line
            //  - The next character is not a newline (trailing ws)
            if c != '\n'
                && let Some(ws) = ws_start
                && ws > 0
                && ws != start_of_line
            {
                out.push(' ');
            }
            ws_start = None;
        }

        if c == '\n' {
            start_of_line = i + 1;
        }

        if !is_inline_ws {
            out.push(c);
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
    fn glue_preserves_leading_whitespace_in_text() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text("  world");
        assert_eq!(buf.flush(), "hello  world");
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

    /// Leading whitespace-only text at the start of output (no prior content)
    /// should be suppressed, just like leading newlines are suppressed.
    /// This happens after choice selection: choice bodies start with `"^ "`.
    #[test]
    fn leading_whitespace_only_text_suppressed() {
        let mut buf = OutputBuffer::new();
        buf.push_text(" ");
        buf.push_text("hello");
        assert_eq!(buf.flush(), "hello");
    }

    /// Leading whitespace-only text after a flush should also be suppressed.
    /// Adjacent whitespace at text boundaries should collapse.
    /// E.g., start content "Hello " + inner content " right back" → "Hello right back".
    #[test]
    fn adjacent_whitespace_collapsed() {
        let mut buf = OutputBuffer::new();
        buf.push_text("Hello ");
        buf.push_text(" right back");
        assert_eq!(buf.flush(), "Hello right back");
    }

    #[test]
    fn leading_whitespace_after_flush_suppressed() {
        let mut buf = OutputBuffer::new();
        buf.push_text("first");
        let _ = buf.flush();
        buf.push_text("  ");
        buf.push_text("second");
        assert_eq!(buf.flush(), "second");
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
        assert_eq!(result, Some("hello world".to_owned()));
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

    /// Trailing whitespace before a newline should be trimmed.
    /// Pattern: `A {f():B}⏎X` where `f()` returns false — the space after
    /// "A" becomes trailing whitespace when the inline expression produces
    /// no output.
    #[test]
    fn trailing_whitespace_before_newline_trimmed() {
        let mut buf = OutputBuffer::new();
        buf.push_text("A ");
        buf.push_newline();
        buf.push_text("X");
        assert_eq!(buf.flush(), "A\nX");
    }

    /// Glue should NOT trim leading whitespace from text content.
    /// Pattern: `Some <>⏎content<> with glue.`
    /// The space in " with glue." is content, not indentation.
    #[test]
    fn glue_preserves_text_whitespace() {
        let mut buf = OutputBuffer::new();
        buf.push_text("Some ");
        buf.push_glue();
        buf.push_newline();
        buf.push_text("content");
        buf.push_glue();
        buf.push_text(" with glue.");
        assert_eq!(buf.flush(), "Some content with glue.");
    }

    /// Glue should skip past whitespace-only text to find the preceding newline.
    /// Pattern: `a\n" "<>b` — the `" "` is whitespace-only and should not block
    /// the glue from removing the newline.
    #[test]
    fn glue_skips_whitespace_only_text_to_find_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("a");
        buf.push_newline();
        buf.push_text(" ");
        buf.push_glue();
        buf.push_text("b");
        assert_eq!(buf.flush(), "a b");
    }

    // ── clean_output_whitespace tests ────────────────────────────────

    #[test]
    fn clean_strips_leading_whitespace() {
        assert_eq!(clean_output_whitespace(" hello"), "hello");
        assert_eq!(clean_output_whitespace("  hello"), "hello");
        assert_eq!(clean_output_whitespace("\thello"), "hello");
    }

    #[test]
    fn clean_strips_trailing_whitespace() {
        assert_eq!(clean_output_whitespace("hello "), "hello");
        assert_eq!(clean_output_whitespace("hello  "), "hello");
    }

    #[test]
    fn clean_strips_per_line() {
        assert_eq!(clean_output_whitespace(" hello \n world "), "hello\nworld");
    }

    #[test]
    fn clean_collapses_internal_whitespace() {
        assert_eq!(clean_output_whitespace("a  b"), "a b");
        assert_eq!(clean_output_whitespace("a   b  c"), "a b c");
    }

    #[test]
    fn clean_preserves_newlines() {
        assert_eq!(clean_output_whitespace("a\nb\n"), "a\nb\n");
    }

    #[test]
    fn clean_empty_string() {
        assert_eq!(clean_output_whitespace(""), "");
    }

    // ── flush_lines tests ────────────────────────────────────────────

    /// Tags should associate with the line they appear on.
    #[test]
    fn flush_lines_associates_tags_with_lines() {
        let mut buf = OutputBuffer::new();
        buf.push_text("line one");
        buf.push_newline();
        buf.push_text("line two");
        buf.push_tag("my_tag".to_string());
        buf.push_newline();
        buf.push_text("line three");
        let lines = buf.flush_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].0, "line one");
        assert!(lines[0].1.is_empty());
        assert_eq!(lines[1].0, "line two");
        assert_eq!(lines[1].1, vec!["my_tag"]);
        assert_eq!(lines[2].0, "line three");
        assert!(lines[2].1.is_empty());
    }

    /// Tags on the last line (no trailing newline) should still be captured.
    #[test]
    fn flush_lines_tag_on_last_line() {
        let mut buf = OutputBuffer::new();
        buf.push_text("only line");
        buf.push_tag("t".to_string());
        let lines = buf.flush_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, "only line");
        assert_eq!(lines[0].1, vec!["t"]);
    }

    /// `flush_lines` should resolve glue the same as `flush`.
    #[test]
    fn flush_lines_resolves_glue() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text(" world");
        let lines = buf.flush_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, "hello world");
    }

    /// Flushing an empty buffer should return no lines.
    /// A spurious `[("", [])]` from an empty buffer causes leading `\n`
    /// when `step_with` calls `flush_lines` multiple times (e.g., before
    /// auto-selecting invisible default choices).
    #[test]
    fn flush_lines_empty_buffer_returns_no_lines() {
        let mut buf = OutputBuffer::new();
        let lines = buf.flush_lines();
        assert!(
            lines.is_empty(),
            "empty buffer should produce no lines, got: {lines:?}"
        );
    }
}
