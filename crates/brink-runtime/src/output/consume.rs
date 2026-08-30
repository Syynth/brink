//! Transcript consumption: draining completed lines and flushing to text.
//!
//! These methods read from `OutputBuffer::transcript` starting at the read
//! cursor, resolve glue, and either drain a single completed line
//! (`take_first_line`, for the streaming `Line`-at-a-time API) or the whole
//! unread tail at once (`flush_lines`). Reading never rewinds the cursor
//! except via `reset_cursor` (used for locale-hot-swap re-rendering).

use alloc::collections::BTreeMap;
#[cfg(test)]
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use brink_format::{LineEntry, PluralResolver};

use super::{OutputBuffer, OutputPart, ResolvedLine, mark_glue_removals, resolve_lines_annotated};
use crate::program::Program;

impl OutputBuffer {
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
        // not in after_glue state) followed by non-whitespace-only content.
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
    /// on the drained segment. Returns `(text, tags, element_data)` — the
    /// third element is issue #2108's per-line element-attachment snapshot
    /// (see [`super::OutputPart::ElementAttach`]'s doc for why this is
    /// correct despite the buffer's deferred-commit glue handling: the
    /// resolved slice below stops at THIS line's own completing `Newline`,
    /// so a later run's `ElementAttach`/`ElementAttachEnd` — which live
    /// strictly after that point in the transcript — can never contaminate
    /// it, even though the VM may already have executed them by the time
    /// this method runs). The remainder stays in the buffer for future
    /// calls.
    ///
    /// The returned text includes a trailing `\n` to indicate a complete
    /// line. This matches the convention that `continue_maximally` joins
    /// all single-line results with empty string to produce the same
    /// output as the original `flush_lines` + `finalize_lines`.
    ///
    /// A completed segment that [`super::resolve_lines_annotated`] marks
    /// suppressed (issue #2091 — an empty `content`/Fragment capture) is
    /// never handed back as a `Line::Text` of its own: the cursor still
    /// advances past it, but the loop keeps scanning for the next real
    /// completed line instead of yielding a blank one.
    ///
    /// Returns `None` if there is no completed (non-suppressed) line.
    pub(crate) fn take_first_line(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> Option<ResolvedLine> {
        if self.has_checkpoint() {
            return None;
        }

        loop {
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
            // Seeded with `pending_element` (issue #2108) — the element-
            // attachment state already accumulated before this slice, so a
            // multi-line attach run's second-and-later lines still see it
            // even though the `ElementAttach` part(s) that produced it live
            // before `self.cursor` now (consumed by an earlier call).
            let slice = &self.transcript[self.cursor..=self.cursor + split_at];
            let mut lines = resolve_lines_annotated(
                slice,
                self.pending_element.clone(),
                program,
                line_tables,
                resolver,
                &self.fragments,
            );
            if lines.is_empty() {
                return None;
            }

            // Advance cursor past the consumed newline — unconditionally: a
            // suppressed line still consumed real transcript space and must
            // not be re-scanned on the next loop iteration.
            self.cursor += split_at + 1;

            let (mut text, tags, suppressed, element, source) = lines.swap_remove(0);
            // The trailing filler entry — now at index 0 after `swap_remove`
            // — carries the element-attachment state as of the END of this
            // slice (past this line's own `Newline`, so it reflects any
            // `ElementAttachEnd` that immediately followed it too). Carry
            // that forward for whatever line the next call resolves,
            // whether or not this one is suppressed.
            self.pending_element = lines
                .first()
                .map_or_else(BTreeMap::new, |(_, _, _, e, _)| e.clone());
            if suppressed {
                continue;
            }
            text.push('\n');
            return Some((text, tags, element, source));
        }
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
        let program = super::test_dummy_program();
        let result = super::resolve_parts(unread, &program, &[], None, &self.fragments);
        self.cursor = self.transcript.len();
        result
    }

    /// Resolve glue and flush to structured per-line output.
    ///
    /// Each returned element is `(line_text, line_tags, element_data)`. Tags
    /// are associated with the line they appear on in the output stream;
    /// `element_data` (issue #2108) accumulates across lines the same way
    /// `take_first_line` does. Seeded with `pending_element` for the same
    /// reason `take_first_line` seeds it: a run's `ElementAttach` part(s)
    /// may already be behind the cursor if an earlier `take_first_line` call
    /// drained the run's first line(s) before this flush drains the rest.
    pub fn flush_lines(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        resolver: Option<&dyn PluralResolver>,
    ) -> Vec<ResolvedLine> {
        debug_assert!(
            !self.has_checkpoint(),
            "flush_lines() called with active checkpoints"
        );
        let unread = &self.transcript[self.cursor..];
        let annotated = resolve_lines_annotated(
            unread,
            self.pending_element.clone(),
            program,
            line_tables,
            resolver,
            &self.fragments,
        );
        // Carry the end-of-slice element-attachment state forward, mirroring
        // `take_first_line` — otherwise an `ElementAttachEnd` consumed by this
        // flush is lost and the attach data stays live on every later line
        // (issue #2108 review finding).
        self.pending_element = annotated
            .last()
            .map_or_else(BTreeMap::new, |(_, _, _, e, _)| e.clone());
        let result: Vec<_> = annotated
            .into_iter()
            .filter_map(|(text, tags, suppressed, element, source)| {
                (!suppressed).then_some((text, tags, element, source))
            })
            .collect();
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
        // At index 0 no attach run has accumulated yet — without this, a
        // locale hot-swap re-render (issue #2108 review finding) would carry
        // the previous pass's element data onto the newly re-drained leading
        // lines.
        self.pending_element = BTreeMap::new();
    }

    /// Returns the number of parts in the transcript.
    pub fn transcript_len(&self) -> usize {
        self.transcript.len()
    }
}
