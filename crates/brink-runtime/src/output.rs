//! Output buffer with glue handling and deferred line resolution.

use brink_format::{
    LineContent, LineEntry, LinePart, PluralCategory, PluralResolver, SelectKey, Value,
};

use crate::program::Program;
use crate::value_ops;

/// A part of accumulated output.
///
/// Output parts are structural references that resolve at read time against
/// the current line tables and plural resolver. This enables locale-hot-swap:
/// the same transcript can be re-rendered in different languages without
/// re-executing the story.
#[derive(Debug, Clone)]
pub enum OutputPart {
    /// Eagerly-resolved text. Not produced by the VM in production —
    /// used in tests and available for external transcript construction.
    Text(String),
    /// Deferred line reference — resolved at read time against the
    /// current line tables and plural resolver.
    LineRef {
        container_idx: u32,
        line_idx: u16,
        slots: Vec<Value>,
        flags: brink_format::LineFlags,
    },
    /// Deferred value — stringified at read time.
    ValueRef(Value),
    Newline,
    /// Word break — renders as a single space between content parts.
    Spring,
    Glue,
    /// Marks the start of a captured region (string eval, tag, or function call).
    Checkpoint,
    /// A tag associated with the current line of output.
    Tag(String),
}

impl OutputPart {
    /// Resolve this output part to its text representation.
    ///
    /// `Text` parts pass through. `LineRef` and `ValueRef` are resolved
    /// using the provided program, line tables, and plural resolver.
    /// Structural parts (`Newline`, `Spring`, `Glue`, `Checkpoint`, `Tag`)
    /// resolve to empty string — they are handled by the resolution pipeline.
    pub fn resolve(
        &self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> String {
        resolve_part(self, program, line_tables, resolver, &[])
    }

    /// Returns true if this part represents non-whitespace text content.
    fn is_content(&self) -> bool {
        match self {
            Self::Text(s) => !s.trim().is_empty(),
            Self::LineRef { flags, .. } => {
                !flags.contains(brink_format::LineFlags::ALL_WS)
                    && !flags.contains(brink_format::LineFlags::EMPTY)
            }
            Self::ValueRef(_) => true,
            _ => false,
        }
    }
}

/// Resolve a single output part to its text representation.
///
/// `Text` parts pass through. `LineRef` and `ValueRef` are resolved
/// using the provided program, line tables, and plural resolver.
fn resolve_part(
    part: &OutputPart,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &[Vec<OutputPart>],
) -> String {
    match part {
        OutputPart::Text(s) => s.clone(),
        OutputPart::LineRef {
            container_idx,
            line_idx,
            slots,
            ..
        } => resolve_line_ref(
            program,
            line_tables,
            *container_idx,
            *line_idx,
            slots,
            resolver,
            fragments,
        ),
        OutputPart::ValueRef(Value::FragmentRef(idx)) => {
            // Resolve the fragment's parts against current line tables.
            let idx = *idx as usize;
            if let Some(parts) = fragments.get(idx) {
                resolve_parts(parts, program, line_tables, resolver, fragments)
            } else {
                String::new()
            }
        }
        OutputPart::ValueRef(val) => value_ops::stringify(val, program),
        OutputPart::Newline
        | OutputPart::Spring
        | OutputPart::Glue
        | OutputPart::Checkpoint
        | OutputPart::Tag(_) => String::new(),
    }
}

/// Resolve a `LineRef` to its text content.
fn resolve_line_ref(
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    container_idx: u32,
    line_idx: u16,
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
    fragments: &[Vec<OutputPart>],
) -> String {
    let scope_idx = program.scope_table_idx(container_idx) as usize;
    let lines = &line_tables[scope_idx];
    let Some(entry) = lines.get(line_idx as usize) else {
        return String::new();
    };

    match &entry.content {
        LineContent::Plain(s) => s.clone(),
        LineContent::Template(parts) => {
            let mut result = String::new();
            for part in parts {
                let owned;
                let fragment: &str = match part {
                    LinePart::Literal(s) => s.as_str(),
                    LinePart::Slot(n) => {
                        owned = slots
                            .get(*n as usize)
                            .map(|v| match v {
                                Value::FragmentRef(idx) => {
                                    let idx = *idx as usize;
                                    fragments.get(idx).map_or_else(String::new, |parts| {
                                        resolve_parts(
                                            parts,
                                            program,
                                            line_tables,
                                            resolver,
                                            fragments,
                                        )
                                    })
                                }
                                other => value_ops::stringify(other, program),
                            })
                            .unwrap_or_default();
                        owned.as_str()
                    }
                    LinePart::Select {
                        slot,
                        variants,
                        default,
                    } => {
                        owned =
                            resolve_select(*slot, variants, default, slots, resolver).to_string();
                        owned.as_str()
                    }
                };
                // Skip empty fragments (null/empty slots) and collapse
                // whitespace at join points when empty slots produce
                // adjacent spaces or leading whitespace.
                if fragment.is_empty() {
                    continue;
                }
                if (result.is_empty() || result.ends_with(' ')) && fragment.starts_with(' ') {
                    result.push_str(fragment.trim_start());
                } else {
                    result.push_str(fragment);
                }
            }
            result
        }
    }
}

/// Resolve a Select part against its slot value.
///
/// Cascade: Exact → Keyword → Cardinal/Ordinal → default.
fn resolve_select<'a>(
    slot: u8,
    variants: &'a [(SelectKey, String)],
    default: &'a str,
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
) -> &'a str {
    let Some(val) = slots.get(slot as usize) else {
        return default;
    };

    #[expect(clippy::cast_possible_truncation)]
    let n: Option<i64> = match val {
        Value::Int(i) => Some(i64::from(*i)),
        Value::Float(f) => Some(*f as i64),
        _ => None,
    };

    // Exact match.
    if let Some(n) = n {
        #[expect(clippy::cast_possible_truncation)]
        let n32 = n as i32;
        for (key, text) in variants {
            if let SelectKey::Exact(e) = key
                && *e == n32
            {
                return text;
            }
        }
    }

    // Keyword match.
    if let Value::String(s) = val {
        for (key, text) in variants {
            if let SelectKey::Keyword(k) = key
                && k == s.as_ref()
            {
                return text;
            }
        }
    }

    // Plural resolution.
    if let (Some(n), Some(r)) = (n, resolver) {
        let cardinal: PluralCategory = r.cardinal(n, None);
        for (key, text) in variants {
            if let SelectKey::Cardinal(cat) = key
                && *cat == cardinal
            {
                return text;
            }
        }
        let ordinal: PluralCategory = r.ordinal(n);
        for (key, text) in variants {
            if let SelectKey::Ordinal(cat) = key
                && *cat == ordinal
            {
                return text;
            }
        }
    }

    default
}

/// Accumulates output text with glue resolution.
///
/// The buffer is split into two storage areas:
/// - **transcript**: append-only log of all output parts. Never drained.
///   A read cursor advances on `take_first_line`/`flush_lines`.
/// - **capture**: transient scratch space for string eval, tag collection,
///   and function return value capture. Drained by `end_capture`.
#[derive(Debug, Clone)]
pub(crate) struct OutputBuffer {
    /// Append-only output log. Parts are never removed.
    pub(crate) transcript: Vec<OutputPart>,
    /// Read cursor into transcript. Advances on take/flush.
    cursor: usize,
    /// Transient capture scratch space.
    capture: Vec<OutputPart>,
    /// Nesting depth of active captures. When > 0, pushes route to `capture`.
    capture_depth: usize,
    /// Finalized fragments — structural output parts for locale re-rendering.
    fragments: Vec<Vec<OutputPart>>,
    /// Current fragment being captured.
    fragment_capture: Vec<OutputPart>,
    /// Fragment capture nesting depth. When > 0, pushes route to `fragment_capture`.
    fragment_depth: usize,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            transcript: Vec::new(),
            cursor: 0,
            capture: Vec::new(),
            capture_depth: 0,
            fragments: Vec::new(),
            fragment_capture: Vec::new(),
            fragment_depth: 0,
        }
    }

    /// Returns the active push target.
    /// Priority: capture (eagerly resolves) > fragment (structural) > transcript.
    fn target(&mut self) -> &mut Vec<OutputPart> {
        if self.capture_depth > 0 {
            &mut self.capture
        } else if self.fragment_depth > 0 {
            &mut self.fragment_capture
        } else {
            &mut self.transcript
        }
    }

    /// No longer called by the VM — candidate for removal.
    #[cfg(test)]
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
            self.target().push(OutputPart::Text(text.to_owned()));
        }
    }

    pub fn push_newline(&mut self) {
        // Suppress leading newlines (no content yet) and duplicate newlines,
        // matching the C# ink runtime's output stream filtering.
        //
        // Inside a capture, use scope-local has_content().  Outside, check
        // the unread transcript for content **or Spring** — Spring is brink's
        // equivalent of the C# `"^ "` (space) that inklecate emits in choice
        // targets.  In C#, that space is a StringValue which makes
        // `outputStreamContainsContent` true, allowing the subsequent newline
        // through.  Without counting Spring, post-choice newlines are lost.
        let has_content = if self.capture_depth > 0 {
            self.has_content()
        } else {
            self.unread_has_content_or_spring()
        };
        if !has_content || self.ends_in_newline() {
            return;
        }
        self.target().push(OutputPart::Newline);
    }

    /// Returns true if the active target contains any text content.
    /// When inside a capture, scans the capture vec (stopping at checkpoint).
    /// When outside, scans the transcript from cursor position.
    fn has_content(&self) -> bool {
        if self.capture_depth > 0 {
            self.capture
                .iter()
                .rev()
                .take_while(|p| !matches!(p, OutputPart::Checkpoint))
                .any(OutputPart::is_content)
        } else {
            self.transcript[self.cursor..]
                .iter()
                .rev()
                .any(OutputPart::is_content)
        }
    }

    /// Returns true if the unread transcript contains content or a Spring.
    ///
    /// This mirrors the C# runtime's `outputStreamContainsContent` check,
    /// which returns true for ANY `StringValue` in the output stream.  In C#,
    /// the choice target's `"^ "` (a space) is a `StringValue` — its brink
    /// equivalent is `Spring`.  After `ResetOutput()` clears the stream at the
    /// start of each `Continue()`, the choice target's space is the first thing
    /// pushed, making `outputStreamContainsContent` true.  In brink, the
    /// cursor advance at yield points has the same effect as `ResetOutput()`,
    /// so checking unread parts mirrors the per-`Continue()` scope.
    fn unread_has_content_or_spring(&self) -> bool {
        self.transcript[self.cursor..]
            .iter()
            .any(|p| p.is_content() || matches!(p, OutputPart::Spring))
    }

    /// Returns true if the last part in the active target is a newline.
    fn ends_in_newline(&self) -> bool {
        let target = if self.capture_depth > 0 {
            &self.capture
        } else {
            &self.transcript
        };
        matches!(target.last(), Some(OutputPart::Newline))
    }

    /// Returns true if the last part is text ending with whitespace.
    /// Only checks the immediately preceding part — intervening Glue or
    /// Newline parts mean the glue system handles the join instead.
    #[cfg(test)]
    fn ends_in_whitespace(&self) -> bool {
        let target = if self.capture_depth > 0 {
            &self.capture
        } else {
            &self.transcript
        };
        match target.last() {
            Some(OutputPart::Text(s)) => s.ends_with(char::is_whitespace),
            Some(OutputPart::LineRef { flags, .. }) => {
                flags.contains(brink_format::LineFlags::ENDS_WITH_WS)
            }
            _ => false,
        }
    }

    pub fn push_glue(&mut self) {
        self.target().push(OutputPart::Glue);
    }

    /// Push a word break. Deduplicated: no consecutive Springs.
    pub fn push_spring(&mut self) {
        let target = self.target();
        if !matches!(target.last(), Some(OutputPart::Spring)) {
            target.push(OutputPart::Spring);
        }
    }

    /// Push a deferred line reference. Resolved at read time.
    /// Applies the same filtering as `push_text` using precomputed flags.
    pub fn push_line_ref(
        &mut self,
        container_idx: u32,
        line_idx: u16,
        slots: Vec<Value>,
        flags: brink_format::LineFlags,
    ) {
        // Suppress whitespace-only/empty content when there's no content yet.
        if !self.has_content()
            && (flags.contains(brink_format::LineFlags::ALL_WS)
                || flags.contains(brink_format::LineFlags::EMPTY))
        {
            return;
        }
        self.target().push(OutputPart::LineRef {
            container_idx,
            line_idx,
            slots,
            flags,
        });
    }

    /// Push a deferred value. Stringified at read time.
    /// Null values are dropped (they stringify to empty string).
    pub fn push_value_ref(&mut self, value: Value) {
        if matches!(value, Value::Null) {
            return;
        }
        // Suppress whitespace-only string values when there's no content yet.
        if !self.has_content()
            && let Value::String(ref s) = value
            && s.trim().is_empty()
        {
            return;
        }
        self.target().push(OutputPart::ValueRef(value));
    }

    /// Push a tag associated with the current output line.
    pub fn push_tag(&mut self, tag: String) {
        self.target().push(OutputPart::Tag(tag));
    }

    /// Returns true if a capture is currently active.
    pub fn has_checkpoint(&self) -> bool {
        self.capture_depth > 0
    }

    /// Begin a capture. Pushes a checkpoint to the capture scratch space.
    /// While a capture is active, all pushes route to the capture vec.
    pub fn begin_capture(&mut self) {
        self.capture_depth += 1;
        self.capture.push(OutputPart::Checkpoint);
    }

    /// End the most recent capture: drain from the last checkpoint in the
    /// capture vec, resolve glue, and return the result as a string.
    ///
    /// Returns `None` if there is no checkpoint.
    pub fn end_capture(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> Option<String> {
        let cp_idx = self
            .capture
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))?;

        let captured: Vec<OutputPart> = self.capture.drain(cp_idx..).collect();
        // Skip the checkpoint itself (first element).
        let captured = &captured[1..];

        self.capture_depth = self.capture_depth.saturating_sub(1);

        Some(resolve_parts(
            captured,
            program,
            line_tables,
            resolver,
            &self.fragments,
        ))
    }

    // ── Fragment capture ───────────────────────────────────────────────

    /// Begin capturing output into a new fragment.
    pub fn begin_fragment(&mut self) {
        self.fragment_depth += 1;
        self.fragment_capture.push(OutputPart::Checkpoint);
    }

    /// End the current fragment capture: drain from the last checkpoint,
    /// store the parts in the fragment store, return the fragment index.
    #[expect(clippy::cast_possible_truncation)]
    pub fn end_fragment(&mut self) -> Option<u32> {
        let cp_idx = self
            .fragment_capture
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))?;

        let captured: Vec<OutputPart> = self.fragment_capture.drain(cp_idx..).collect();
        // Skip the checkpoint itself (first element).
        let parts: Vec<OutputPart> = captured.into_iter().skip(1).collect();
        let idx = self.fragments.len() as u32;
        self.fragments.push(parts);

        self.fragment_depth = self.fragment_depth.saturating_sub(1);

        Some(idx)
    }

    /// Read access to all finalized fragments.
    pub fn fragments(&self) -> &[Vec<OutputPart>] {
        &self.fragments
    }

    /// Read access to a finalized fragment's parts.
    pub fn fragment(&self, idx: u32) -> Option<&[OutputPart]> {
        self.fragments.get(idx as usize).map(Vec::as_slice)
    }

    /// Resolve a fragment's parts against the current line tables.
    pub fn resolve_fragment(
        &self,
        idx: u32,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> String {
        match self.fragment(idx) {
            Some(parts) => resolve_parts(parts, program, line_tables, resolver, &self.fragments),
            None => String::new(),
        }
    }

    /// Trim trailing whitespace from the most recent function's fragment output,
    /// remove its checkpoint, and collapse parts into the outer context.
    ///
    /// Implements the C# runtime's `TrimWhitespaceFromFunctionEnd`: walk
    /// backward from the end of the fragment capture and remove trailing
    /// `Newline`, `Spring`, and whitespace-only content. Then remove the
    /// checkpoint. If outermost, flush trimmed parts to transcript.
    pub fn trim_and_collapse_fragment(&mut self) {
        let Some(cp_idx) = self
            .fragment_capture
            .iter()
            .rposition(|p| matches!(p, OutputPart::Checkpoint))
        else {
            return;
        };

        // Trim trailing whitespace/newline parts from the function's region.
        while self.fragment_capture.len() > cp_idx + 1 {
            match self.fragment_capture.last() {
                Some(OutputPart::Newline | OutputPart::Spring) => {
                    self.fragment_capture.pop();
                }
                Some(OutputPart::Text(s)) if s.trim().is_empty() => {
                    self.fragment_capture.pop();
                }
                Some(OutputPart::LineRef { flags, .. })
                    if flags.contains(brink_format::LineFlags::ALL_WS) =>
                {
                    self.fragment_capture.pop();
                }
                _ => break,
            }
        }

        // Remove the checkpoint marker.
        self.fragment_capture.remove(cp_idx);
        self.fragment_depth = self.fragment_depth.saturating_sub(1);

        // If outermost, flush to transcript.
        if self.fragment_depth == 0 && !self.fragment_capture.is_empty() {
            self.transcript.append(&mut self.fragment_capture);
        }
    }

    /// Returns true if the buffer contains at least one complete line
    /// (a Newline whose effect survived glue resolution, confirmed by
    /// subsequent non-whitespace content).
    ///
    /// A Newline is "committed" when non-whitespace text appears after it
    /// in the buffer — at that point, no future Glue can reach past the
    /// text to eat the Newline.
    pub(crate) fn has_completed_line(&self) -> bool {
        if self.has_checkpoint() {
            return false;
        }
        let unread = &self.transcript[self.cursor..];
        if unread.is_empty() {
            return false;
        }

        // Quick check: any newline at all?
        if !unread.iter().any(|p| matches!(p, OutputPart::Newline)) {
            return false;
        }

        // Run glue marking pass to determine which newlines survive.
        let mut remove = vec![false; unread.len()];
        mark_glue_removals(unread, &mut remove);

        // Walk and find a committed newline: a surviving Newline (not removed,
        // not in after_glue state) followed by non-whitespace-only text.
        let mut after_glue = false;
        let mut found_newline = false;

        for (i, part) in unread.iter().enumerate() {
            if remove[i] {
                if matches!(part, OutputPart::Glue) {
                    after_glue = true;
                }
                continue;
            }
            if part.is_content() {
                if found_newline {
                    return true;
                }
                after_glue = false;
            } else {
                match part {
                    OutputPart::Newline if !after_glue => {
                        found_newline = true;
                    }
                    OutputPart::Glue => {
                        after_glue = true;
                    }
                    _ => {}
                }
            }
        }

        false
    }

    /// Drain the first complete line from the buffer, resolving glue
    /// on the drained segment. Returns `(text, tags)`. The remainder
    /// stays in the buffer for future calls.
    ///
    /// The returned text includes a trailing `\n` to indicate a complete
    /// line. This matches the convention that `continue_maximally` joins
    /// all single-line results with empty string to produce the same
    /// output as the original `flush_lines` + `finalize_lines`.
    ///
    /// Returns `None` if there is no completed line.
    pub(crate) fn take_first_line(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> Option<(String, Vec<String>)> {
        if self.has_checkpoint() {
            return None;
        }
        let unread = &self.transcript[self.cursor..];
        if unread.is_empty() {
            return None;
        }

        let mut remove = vec![false; unread.len()];
        mark_glue_removals(unread, &mut remove);

        // Find the split point: the first surviving Newline (not removed,
        // not in after_glue state) that has non-whitespace text after it.
        let mut after_glue = false;
        let mut candidate_newline: Option<usize> = None;

        for (i, part) in unread.iter().enumerate() {
            if remove[i] {
                if matches!(part, OutputPart::Glue) {
                    after_glue = true;
                }
                continue;
            }
            if part.is_content() {
                if candidate_newline.is_some() {
                    break;
                }
                after_glue = false;
            } else {
                match part {
                    OutputPart::Newline if !after_glue => {
                        candidate_newline = Some(i);
                    }
                    OutputPart::Glue => {
                        after_glue = true;
                    }
                    _ => {}
                }
            }
        }

        let split_at = candidate_newline?;

        // Resolve the slice through the newline (inclusive). No drain.
        let slice = &self.transcript[self.cursor..=self.cursor + split_at];
        let mut lines = resolve_lines(slice, program, line_tables, resolver, &self.fragments);
        if lines.is_empty() {
            return None;
        }

        // Advance cursor past the consumed newline.
        self.cursor += split_at + 1;

        let (mut text, tags) = lines.swap_remove(0);
        text.push('\n');
        Some((text, tags))
    }

    /// Resolve glue and flush to a string (ignoring tags).
    ///
    /// Glue removes the newline immediately before it and any leading
    /// whitespace on the text immediately after it, stitching text together.
    /// Resolve glue and flush to a string. Test-only — only works with
    /// `Text`/`Newline`/`Glue` parts (no `LineRef`/`ValueRef`).
    #[cfg(test)]
    pub fn flush(&mut self) -> String {
        debug_assert!(
            !self.has_checkpoint(),
            "flush() called with active checkpoints"
        );
        let unread = &self.transcript[self.cursor..];
        let program = test_dummy_program();
        let result = resolve_parts(unread, &program, &[], None, &self.fragments);
        self.cursor = self.transcript.len();
        result
    }

    /// Resolve glue and flush to structured per-line output.
    ///
    /// Each returned element is `(line_text, line_tags)`. Tags are associated
    /// with the line they appear on in the output stream.
    pub fn flush_lines(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> Vec<(String, Vec<String>)> {
        debug_assert!(
            !self.has_checkpoint(),
            "flush_lines() called with active checkpoints"
        );
        let unread = &self.transcript[self.cursor..];
        let result = resolve_lines(unread, program, line_tables, resolver, &self.fragments);
        self.cursor = self.transcript.len();
        result
    }

    /// Returns true if there are unread parts in the transcript.
    pub(crate) fn has_unread(&self) -> bool {
        self.cursor < self.transcript.len()
    }

    /// Returns the full append-only transcript.
    pub fn transcript(&self) -> &[OutputPart] {
        &self.transcript
    }

    /// Reset the read cursor to the beginning for re-rendering.
    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
    }

    /// Returns the number of parts in the transcript.
    pub fn transcript_len(&self) -> usize {
        self.transcript.len()
    }
}

/// First pass of glue resolution: mark newlines and glue parts for removal.
///
/// For each `Glue` part, find the nearest preceding `Newline` (skipping
/// whitespace-only text, tags, checkpoints, and already-removed parts)
/// and mark both the newline and the glue for removal.
fn mark_glue_removals(parts: &[OutputPart], remove: &mut [bool]) {
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
                    OutputPart::Glue
                    | OutputPart::Checkpoint
                    | OutputPart::Tag(_)
                    | OutputPart::Spring => {}
                    OutputPart::Text(s) if s.trim().is_empty() => {}
                    // Content (Text, LineRef, ValueRef) blocks glue scan.
                    OutputPart::Text(_) | OutputPart::LineRef { .. } | OutputPart::ValueRef(_) => {
                        break;
                    }
                }
            }
            remove[i] = true;
        }
    }
}

/// Resolve glue in a slice of output parts and return the flattened string.
fn resolve_parts(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &[Vec<OutputPart>],
) -> String {
    // First pass: mark newlines that should be removed by glue.
    let mut remove = vec![false; parts.len()];
    mark_glue_removals(parts, &mut remove);

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
            OutputPart::Text(_) | OutputPart::LineRef { .. } | OutputPart::ValueRef(_) => {
                let s = resolve_part(part, program, line_tables, resolver, fragments);
                // Collapse adjacent whitespace at part boundaries.
                let s = if s.starts_with(char::is_whitespace) && out.ends_with(char::is_whitespace)
                {
                    s.trim_start()
                } else {
                    &s
                };
                out.push_str(s);
                if !s.trim().is_empty() {
                    after_glue = false;
                }
            }
            OutputPart::Spring => {
                // Emit " " unless output is empty, ends in space, or ends in newline.
                if !out.is_empty() && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
            }
            OutputPart::Newline => {
                if !after_glue {
                    let trimmed_len = out.trim_end_matches([' ', '\t']).len();
                    out.truncate(trimmed_len);
                    out.push('\n');
                }
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
pub(crate) fn resolve_lines(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &[Vec<OutputPart>],
) -> Vec<(String, Vec<String>)> {
    if parts.is_empty() {
        return Vec::new();
    }

    // First pass: mark newlines/glue for removal (same logic as resolve_parts).
    let mut remove = vec![false; parts.len()];
    mark_glue_removals(parts, &mut remove);

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
            OutputPart::Text(_) | OutputPart::LineRef { .. } | OutputPart::ValueRef(_) => {
                let s = resolve_part(part, program, line_tables, resolver, fragments);
                // Collapse adjacent whitespace at part boundaries.
                let s = if s.starts_with(char::is_whitespace)
                    && current_text.ends_with(char::is_whitespace)
                {
                    s.trim_start()
                } else {
                    &s
                };
                current_text.push_str(s);
                if !s.trim().is_empty() {
                    after_glue = false;
                }
            }
            OutputPart::Spring => {
                if !current_text.is_empty()
                    && !current_text.ends_with(' ')
                    && !current_text.ends_with('\n')
                {
                    current_text.push(' ');
                }
            }
            OutputPart::Newline => {
                if !after_glue {
                    let trimmed = current_text.trim().to_string();
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
    let trimmed = current_text.trim().to_string();
    lines.push((trimmed, current_tags));

    lines
}

/// Create a minimal `Program` for tests that only use `Text`/`Newline`/`Glue`.
#[cfg(test)]
fn test_dummy_program() -> Program {
    use std::collections::HashMap;
    Program {
        containers: vec![],
        address_map: HashMap::new(),
        scope_ids: vec![],
        source_checksum: 0,
        globals: vec![],
        global_map: HashMap::new(),
        name_table: vec![],
        root_idx: 0,
        list_literals: vec![],
        list_item_map: HashMap::new(),
        list_defs: vec![],
        list_def_map: HashMap::new(),
        external_fns: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helpers — `OutputBuffer` methods that need resolution context.
    /// Tests only use Text/Newline/Glue, so we pass an empty program.
    impl OutputBuffer {
        fn test_flush_lines(&mut self) -> Vec<(String, Vec<String>)> {
            let p = test_dummy_program();
            self.flush_lines(&p, &[], None)
        }

        fn test_take_first_line(&mut self) -> Option<(String, Vec<String>)> {
            let p = test_dummy_program();
            self.take_first_line(&p, &[], None)
        }

        fn test_end_capture(&mut self) -> Option<String> {
            let p = test_dummy_program();
            self.end_capture(&p, &[], None)
        }
    }

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
        let result = buf.test_end_capture();
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
        let inner = buf.test_end_capture();
        assert_eq!(inner, Some("inner".to_owned()));
        let middle = buf.test_end_capture();
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
        let result = buf.test_end_capture();
        assert_eq!(result, Some("hello world".to_owned()));
    }

    #[test]
    fn end_capture_no_checkpoint_returns_none() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        assert_eq!(buf.test_end_capture(), None);
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
    fn trim_and_collapse_trims_trailing_newline() {
        let mut buf = OutputBuffer::new();
        buf.begin_fragment();
        buf.push_text("Hello");
        buf.push_newline();
        buf.trim_and_collapse_fragment();
        assert_eq!(buf.flush(), "Hello");
    }

    #[test]
    fn trim_and_collapse_nested() {
        let mut buf = OutputBuffer::new();
        buf.begin_fragment(); // outer
        buf.push_text("outer");
        buf.begin_fragment(); // inner
        buf.push_text("inner");
        buf.push_newline();
        buf.trim_and_collapse_fragment(); // collapse inner
        let idx = buf.end_fragment();
        assert!(idx.is_some());
        let p = test_dummy_program();
        let resolved = buf.resolve_fragment(idx.unwrap(), &p, &[], None);
        assert_eq!(resolved, "outerinner");
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
        let lines = buf.test_flush_lines();
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
        let lines = buf.test_flush_lines();
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
        let lines = buf.test_flush_lines();
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
        let lines = buf.test_flush_lines();
        assert!(
            lines.is_empty(),
            "empty buffer should produce no lines, got: {lines:?}"
        );
    }

    // ── has_completed_line / take_first_line tests ──────────────────

    #[test]
    fn has_completed_line_empty() {
        let buf = OutputBuffer::new();
        assert!(!buf.has_completed_line());
    }

    #[test]
    fn has_completed_line_text_only() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        assert!(!buf.has_completed_line());
    }

    #[test]
    fn has_completed_line_text_newline_only() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        // No content after the newline → not committed.
        assert!(!buf.has_completed_line());
    }

    #[test]
    fn has_completed_line_text_newline_text() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_text("world");
        assert!(buf.has_completed_line());
    }

    #[test]
    fn has_completed_line_glue_eats_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text("world");
        // Glue eats the newline → no committed newline.
        assert!(!buf.has_completed_line());
    }

    #[test]
    fn has_completed_line_during_capture() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_text("world");
        buf.begin_capture();
        // Active capture → not available for line extraction.
        assert!(!buf.has_completed_line());
    }

    #[test]
    fn take_first_line_basic() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_text("world");

        let result = buf.test_take_first_line();
        assert!(result.is_some());
        let (text, tags) = result.unwrap();
        assert_eq!(text, "hello\n");
        assert!(tags.is_empty());

        // Remainder should produce "world" when flushed.
        assert_eq!(buf.flush(), "world");
    }

    #[test]
    fn take_first_line_with_tags() {
        let mut buf = OutputBuffer::new();
        buf.push_text("tagged line");
        buf.push_tag("my_tag".to_string());
        buf.push_newline();
        buf.push_text("next line");

        let (text, tags) = buf.test_take_first_line().unwrap();
        assert_eq!(text, "tagged line\n");
        assert_eq!(tags, vec!["my_tag"]);

        assert_eq!(buf.flush(), "next line");
    }

    #[test]
    fn take_first_line_multiple_lines() {
        let mut buf = OutputBuffer::new();
        buf.push_text("line one");
        buf.push_newline();
        buf.push_text("line two");
        buf.push_newline();
        buf.push_text("line three");

        let (text1, _) = buf.test_take_first_line().unwrap();
        assert_eq!(text1, "line one\n");

        let (text2, _) = buf.test_take_first_line().unwrap();
        assert_eq!(text2, "line two\n");

        // Only "line three" remains, no newline after it → no completed line.
        assert!(!buf.has_completed_line());
        assert_eq!(buf.flush(), "line three");
    }

    #[test]
    fn take_first_line_matches_flush_lines() {
        // Verify take_first_line produces the same first line as flush_lines.
        let parts = |buf: &mut OutputBuffer| {
            buf.push_text("A ");
            buf.push_tag("t1".to_string());
            buf.push_newline();
            buf.push_text("B");
            buf.push_newline();
            buf.push_text("C");
        };

        let mut buf1 = OutputBuffer::new();
        parts(&mut buf1);
        let all_lines = buf1.test_flush_lines();
        let first_from_flush = &all_lines[0].0;

        let mut buf2 = OutputBuffer::new();
        parts(&mut buf2);
        let (first_from_take, tags) = buf2.test_take_first_line().unwrap();
        // take_first_line appends \n; strip it for comparison.
        let first_trimmed = first_from_take.trim_end_matches('\n');

        assert_eq!(first_trimmed, first_from_flush);
        assert_eq!(tags, all_lines[0].1);
    }

    #[test]
    fn take_first_line_glue_preserves_subsequent() {
        // Glue eats the first newline; second newline survives.
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_glue();
        buf.push_text(" world");
        buf.push_newline();
        buf.push_text("next");

        let (text, _) = buf.test_take_first_line().unwrap();
        assert_eq!(text, "hello world\n");
        assert_eq!(buf.flush(), "next");
    }

    #[test]
    fn take_first_line_none_when_empty() {
        let mut buf = OutputBuffer::new();
        assert!(buf.test_take_first_line().is_none());
    }

    #[test]
    fn take_first_line_none_when_no_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("no newline");
        assert!(buf.test_take_first_line().is_none());
    }

    // ── resolve_line_ref template collapsing tests ────────────────────

    /// Build a minimal `Program` with one container (`scope_table_idx` = 0)
    /// and a line table with a single template entry, then resolve it.
    fn resolve_template(parts: Vec<LinePart>, slots: &[Value]) -> String {
        use crate::program::LinkedContainer;
        use brink_format::{CountingFlags, DefinitionId, DefinitionTag, LineEntry, LineFlags};
        use std::collections::HashMap;

        let id = DefinitionId::new(DefinitionTag::Address, 0);
        let program = Program {
            containers: vec![LinkedContainer {
                id,
                bytecode: vec![],
                counting_flags: CountingFlags::empty(),
                path_hash: 0,
                scope_table_idx: 0,
            }],
            address_map: HashMap::new(),
            scope_ids: vec![id],
            source_checksum: 0,
            globals: vec![],
            global_map: HashMap::new(),
            name_table: vec![],
            root_idx: 0,
            list_literals: vec![],
            list_item_map: HashMap::new(),
            list_defs: vec![],
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
        };

        let line_tables = vec![vec![LineEntry {
            content: LineContent::Template(parts),
            source_hash: 0,
            flags: LineFlags::empty(),
            audio_ref: None,
            slot_info: vec![],
            source_location: None,
        }]];

        resolve_line_ref(&program, &line_tables, 0, 0, slots, None, &[])
    }

    #[test]
    fn template_collapses_double_space_from_empty_slot() {
        let result = resolve_template(
            vec![
                LinePart::Literal("Hello ".into()),
                LinePart::Slot(0),
                LinePart::Literal(" world".into()),
            ],
            &[Value::Null],
        );
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn template_preserves_spaces_with_nonempty_slot() {
        let result = resolve_template(
            vec![
                LinePart::Literal("Hello ".into()),
                LinePart::Slot(0),
                LinePart::Literal(" world".into()),
            ],
            &[Value::String("dear".into())],
        );
        assert_eq!(result, "Hello dear world");
    }

    #[test]
    fn template_multiple_empty_slots_collapse() {
        let result = resolve_template(
            vec![
                LinePart::Literal("a ".into()),
                LinePart::Slot(0),
                LinePart::Literal(" ".into()),
                LinePart::Slot(1),
                LinePart::Literal(" b".into()),
            ],
            &[Value::Null, Value::Null],
        );
        assert_eq!(result, "a b");
    }

    #[test]
    fn template_empty_string_slot_same_as_null() {
        let result = resolve_template(
            vec![
                LinePart::Literal("Hello ".into()),
                LinePart::Slot(0),
                LinePart::Literal(" world".into()),
            ],
            &[Value::String("".into())],
        );
        assert_eq!(result, "Hello world");
    }
}
