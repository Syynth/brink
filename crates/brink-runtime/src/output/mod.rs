//! Output buffer with glue handling and deferred line resolution.

use core::mem;

use alloc::collections::BTreeMap;
use alloc::string::String;
#[cfg(test)]
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use brink_format::{
    LineContent, LineEntry, LinePart, PluralCategory, PluralResolver, SelectKey, Value,
};

use crate::program::Program;
use crate::value_ops;

mod completion;
mod consume;
mod fragment;

use completion::LineCompletion;

pub use fragment::{Fragment, FragmentRef, Fragments};

/// A part of accumulated output.
///
/// Output parts are structural references that resolve at read time against
/// the current line tables and plural resolver. This enables locale-hot-swap:
/// the same transcript can be re-rendered in different languages without
/// re-executing the story.
///
/// `PartialEq` (issue #746): structural equality over the part's own fields
/// — used by the `.brkt` transcript round-trip law
/// (`brink-runtime/tests/law_transcript_roundtrip.rs`) to assert decoded
/// parts equal the originals. Every field type already implements it
/// (`Value`'s hand-written impl, `LineFlags`'s derive).
#[derive(Debug, Clone, PartialEq)]
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
    /// One field of an `attach = StructName` convention handler's return
    /// value, merged into the run currently open (issue #2108,
    /// `docs/decision-log.md` 2026-08-03 "The element output model:
    /// attachment is block-level metadata, delivery is per-line"). Embedded
    /// in the SAME append-only stream as `Tag`, rather than mutated on
    /// `Flow` directly, for the identical reason tags are: the output
    /// buffer defers a `Newline`'s commitment until later content proves no
    /// `Glue` reaches back over it (`OutputBuffer::has_completed_line`'s own
    /// doc), so by the time a line is finally drained the VM may already
    /// have stepped past several MORE opcodes (including a later run's own
    /// `ElementAttach`/`ElementAttachEnd`). Reading a live, continuously-
    /// mutated `Flow` field at drain time would misattribute a LATER run's
    /// data to an EARLIER, still-buffered line — embedding the merge as its
    /// own transcript entry, at the exact point it actually happened,
    /// avoids that entirely: [`resolve_lines_annotated`] rebuilds the
    /// correct per-line snapshot by walking the stream in order, the same
    /// way it already does for `Tag`.
    ///
    /// Unlike `Tag` (which resets every line), this ACCUMULATES across
    /// multiple lines until a matching [`Self::ElementAttachEnd`] closes the
    /// run — ruling item 4/5: "the run IS the block" and "every line in it
    /// carries a copy."
    ///
    /// **Not part of the persisted `.brkt` format** (`crate::transcript`'s
    /// `is_persisted`) — like [`Self::Checkpoint`], this is in-memory-only
    /// bookkeeping. Issue #2108 is explicitly scoped to "the in-memory half"
    /// (see its own tracked follow-up on save/resume); a transcript replayed
    /// from a `.brkt` file loses element attachment, matching that scope.
    ElementAttach(String, String),
    /// Closes the run an [`Self::ElementAttach`] opened, clearing the
    /// accumulated data so content after this point is never misattributed
    /// to a run it wasn't part of. See [`Self::ElementAttach`]'s doc for why
    /// this lives in the transcript stream rather than on `Flow`, and for
    /// its non-persisted status.
    ElementAttachEnd,
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
        resolve_part(self, program, line_tables, resolver, &Fragments::default())
    }

    /// Returns true if this part represents non-whitespace text content.
    fn is_content(&self) -> bool {
        match self {
            Self::Text(s) => !s.trim().is_empty(),
            Self::LineRef { flags, .. } => {
                !flags.contains(brink_format::LineFlags::ALL_WS)
                    && !flags.contains(brink_format::LineFlags::EMPTY)
            }
            // B4 (`docs/stdlib-spec.md` §1.6b): a final-`None` value at the
            // display boundary resolves to the empty string (see
            // `value_ops::stringify_display`) — it must not count as
            // content for leading-newline/glue suppression, matching how
            // an eagerly-dropped `Value::Null` never reaches the
            // transcript at all (`push_value_ref`, below). Unlike `Null`,
            // a `None` value IS retained in the transcript (traceability
            // is a §1.6b rider) — only its content-ness for whitespace
            // bookkeeping is suppressed here.
            Self::ValueRef(Value::OptionVal(None)) => false,
            Self::ValueRef(_) => true,
            _ => false,
        }
    }

    /// Issue #3533: does this part render as something other than
    /// whitespace? [`Self::is_content`] mirrors ink's
    /// `outputStreamContainsContent`, where an empty `""` string still
    /// counts (it lets the line's own newline through); this mirrors what
    /// ink's newline lookahead treats as *extending* a line — a blank
    /// `ValueRef` (`""`, `" "`, an empty list) never commits the line
    /// before it, only visible text does.
    fn is_visible(&self) -> bool {
        match self {
            Self::ValueRef(Value::String(s)) => !s.trim().is_empty(),
            Self::ValueRef(Value::List(lv)) => !lv.items.is_empty(),
            _ => self.is_content(),
        }
    }
}

/// Resolve a single output part to its text representation.
///
/// Thin owning wrapper over [`resolve_part_into`]; the production paths
/// ([`resolve_parts`], [`resolve_lines_annotated`]) append straight into
/// the line they are building instead, so a part's text is written once.
fn resolve_part(
    part: &OutputPart,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> String {
    let mut out = String::new();
    resolve_part_into(part, &mut out, program, line_tables, resolver, fragments);
    out
}

/// Append a single output part's text to `out`.
///
/// `Text` parts pass through. `LineRef` and `ValueRef` are resolved
/// using the provided program, line tables, and plural resolver.
/// Structural parts (`Newline`, `Spring`, `Glue`, `Checkpoint`, `Tag`)
/// append nothing — they are handled by the resolution pipeline.
///
/// A plain literal reserves one byte beyond its own length: the common
/// line is a single `Plain` entry, and the caller that hands the line out
/// ([`OutputBuffer::take_first_line`]) terminates it with `'\n'`. Without
/// the spare byte that push reallocates every such line (measured as one
/// `realloc` per delivered line on `TheIntercept`, #3570 follow-up).
fn resolve_part_into(
    part: &OutputPart,
    out: &mut String,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) {
    match part {
        OutputPart::Text(s) => {
            out.reserve(s.len() + 1);
            out.push_str(s);
        }
        OutputPart::LineRef {
            container_idx,
            line_idx,
            slots,
            ..
        } => resolve_line_ref_into(
            out,
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
            if let Some(parts) = fragments.parts(*idx) {
                let s = resolve_parts(parts, program, line_tables, resolver, fragments);
                out.push_str(&s);
            }
        }
        // B4 (`docs/stdlib-spec.md` §1.6b): the display boundary — a
        // final-`None` value renders as nothing, not `"none"`. See
        // `value_ops::stringify_display`'s doc comment for the full ruling.
        OutputPart::ValueRef(val) => out.push_str(&value_ops::stringify_display(val, program)),
        OutputPart::Newline
        | OutputPart::Spring
        | OutputPart::Glue
        | OutputPart::Checkpoint
        | OutputPart::Tag(_)
        | OutputPart::ElementAttach(..)
        | OutputPart::ElementAttachEnd => {}
    }
}

/// Collapse whitespace where a freshly appended segment `out[start..]`
/// meets the text before it: when both sides carry whitespace at the join,
/// the segment's leading run goes. Returns whether the segment holds any
/// non-whitespace — the "this part produced visible content" signal both
/// line walkers use to clear `after_glue`.
///
/// Equivalent to the former `s.trim_start()`-then-`push_str` on an owned
/// per-part `String`, without the per-part allocation.
fn collapse_join(out: &mut String, start: usize) -> bool {
    let segment = &out[start..];
    if segment.is_empty() {
        return false;
    }
    let non_blank = !segment.trim().is_empty();
    if segment.starts_with(char::is_whitespace) && out[..start].ends_with(char::is_whitespace) {
        let lead = segment.len() - segment.trim_start().len();
        out.replace_range(start..start + lead, "");
    }
    non_blank
}

/// Resolve a `LineRef` to its text content.
#[cfg(test)]
fn resolve_line_ref(
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    container_idx: u32,
    line_idx: u16,
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> String {
    let mut out = String::new();
    resolve_line_ref_into(
        &mut out,
        program,
        line_tables,
        container_idx,
        line_idx,
        slots,
        resolver,
        fragments,
    );
    out
}

/// Append a `LineRef`'s text content to `out`.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors `resolve_line_ref`'s parameter list"
)]
fn resolve_line_ref_into(
    out: &mut String,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    container_idx: u32,
    line_idx: u16,
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) {
    let scope_idx = program.scope_table_idx(container_idx) as usize;
    let lines = &line_tables[scope_idx];
    let Some(entry) = lines.get(line_idx as usize) else {
        return;
    };

    match &entry.content {
        LineContent::Plain(s) => {
            // See `resolve_part_into` for the spare byte.
            out.reserve(s.len() + 1);
            out.push_str(s);
        }
        LineContent::Template(parts) => {
            resolve_line_parts_into(out, parts, program, line_tables, slots, resolver, fragments);
        }
    }
}

/// Append a sequence of `LinePart`s to `out`.
///
/// A span is presentational (§4.3) and the runtime's current public API
/// (`Line::Text.text`) is flat text with no structured span surface yet
/// (`docs/prose-dialect-spec.md` §7/§9.1: the `Step`/`Part` redesign that
/// would carry `Part::Span` structure through to a consumer is still ⏳) —
/// so a span resolves here to its children's concatenated text, tag name
/// and attrs stripped, recursing through this same function. That is
/// additive groundwork for the future structured surface, not a
/// replacement of it: §4.4 explicitly wants "structural parts over
/// byte-range offsets" once that surface lands.
///
/// Whitespace at part joins collapses exactly as it did when every part
/// was its own `String`: an empty part is skipped, and a part starting
/// with a space loses its leading whitespace when the template's text so
/// far is empty or already ends in a space. "The template's text so far"
/// is `out[base..start]` — the text this call appended, not whatever the
/// caller had in `out` before it — so a nested span behaves like the fresh
/// `String` it used to be.
fn resolve_line_parts_into(
    out: &mut String,
    parts: &[LinePart],
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    slots: &[Value],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) {
    let base = out.len();
    for part in parts {
        let start = out.len();
        match part {
            LinePart::Literal(s) => out.push_str(s),
            LinePart::Slot(n) => match slots.get(*n as usize) {
                Some(Value::FragmentRef(idx)) => {
                    if let Some(parts) = fragments.parts(*idx) {
                        let s = resolve_parts(parts, program, line_tables, resolver, fragments);
                        out.push_str(&s);
                    }
                }
                // B4 (`docs/stdlib-spec.md` §1.6b) — same display-boundary
                // forgiveness as the `ValueRef` arm of `resolve_part_into`;
                // the join collapse below already treats an empty slot
                // fragment correctly.
                Some(other) => out.push_str(&value_ops::stringify_display(other, program)),
                None => {}
            },
            LinePart::Select {
                slot,
                variants,
                default,
            } => out.push_str(resolve_select(*slot, variants, default, slots, resolver)),
            LinePart::Span { children, .. } => {
                resolve_line_parts_into(
                    out,
                    children,
                    program,
                    line_tables,
                    slots,
                    resolver,
                    fragments,
                );
            }
        }
        // Skip empty fragments (null/empty slots) and collapse
        // whitespace at join points when empty slots produce
        // adjacent spaces or leading whitespace.
        if out.len() == start {
            continue;
        }
        let result_empty_or_space = start == base || out[..start].ends_with(' ');
        if result_empty_or_space && out[start..].starts_with(' ') {
            let lead = out[start..].len() - out[start..].trim_start().len();
            out.replace_range(start..start + lead, "");
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

/// Where a function's output began: the active target's length at call
/// time, plus which target it was. The two depths let a later check tell
/// "the same target, further along" from "a different target" (a string
/// capture or fragment that began inside the function), where the length
/// alone would be meaningless (issue #3519).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputMark {
    pub(crate) len: usize,
    pub(crate) capture_depth: usize,
    pub(crate) fragment_depth: usize,
}

/// `OutputBuffer` reaches Bevy as part of `bevy-brink`'s `BrinkFlow`
/// component, and Bevy requires components to be `Send + Sync`. Nothing in
/// this module names that requirement, and violating it fails nowhere near
/// here: an interior-mutability field added for a scratch buffer (`RefCell`
/// and `Cell` are both `!Sync`) surfaced as dozens of
/// `QueryData`/`IterQueryData` bound errors inside `bevy-brink`, on a CI leg
/// this crate's own gates never run. Assert it here, where the field would
/// be added.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<OutputBuffer>();
    assert_send_sync::<OutputPart>();
};

/// Accumulates output text with glue resolution.
///
/// The buffer is split into two storage areas:
/// - **transcript**: append-only log of all output parts. Never drained.
///   A read cursor advances on `take_first_line`/`flush_lines`.
/// - **capture**: transient scratch space for string eval, tag collection,
///   and function return value capture. Drained by `end_capture`.
#[derive(Debug, Clone)]
pub(crate) struct OutputBuffer {
    /// Reusable scan buffer for [`Self::take_first_line`]'s glue pass.
    ///
    /// Not state: it carries nothing between calls and every call refills it
    /// from scratch. It exists purely so the scan stops allocating: it was
    /// introduced when `has_completed_line` still ran this scan once per VM
    /// step and built a fresh `vec![false; unread.len()]` each time —
    /// measured at 467,587 `calloc` calls against 466,851 steps on
    /// `crucible-8` (#3565). `has_completed_line` no longer scans at all
    /// (`completion.rs`); `take_first_line` still does, once per delivered
    /// line.
    ///
    /// A plain field with a `&mut self` receiver, deliberately: `RefCell`
    /// and `Cell` are both `!Sync`, and one here makes `OutputBuffer` —
    /// and transitively `bevy-brink`'s `BrinkFlow` component — non-`Sync`,
    /// which Bevy requires. That failure surfaces far from its cause, as
    /// dozens of `QueryData`/`IterQueryData` bound errors in `bevy-brink`.
    line_scan: Vec<bool>,
    /// Incremental state behind [`Self::has_completed_line`]: the answer the
    /// glue-and-walk scan would give over `transcript[cursor..]`, kept
    /// current by [`Self::push_part`] and rebuilt by
    /// [`Self::rescan_completion`] after a cursor move or a removal. See
    /// `completion.rs` for why this is exact.
    completion: LineCompletion,
    /// Append-only output log. Parts are never removed.
    pub(crate) transcript: Vec<OutputPart>,
    /// Read cursor into transcript. Advances on take/flush.
    pub(crate) cursor: usize,
    /// Transient capture scratch space.
    capture: Vec<OutputPart>,
    /// Nesting depth of active captures. When > 0, pushes route to `capture`.
    capture_depth: usize,
    /// Finalized fragments — structural output parts for locale re-rendering.
    fragments: Fragments,
    /// Current fragment being captured.
    fragment_capture: Vec<OutputPart>,
    /// Fragment capture nesting depth. When > 0, pushes route to `fragment_capture`.
    fragment_depth: usize,
    /// Tags accumulated during each nested fragment capture level.
    fragment_pending_tags: Vec<Vec<String>>,
    /// Element-attachment state (issue #2108) carried forward across
    /// separate [`Self::take_first_line`] calls — the streaming, one-line-
    /// at-a-time API resolves only the slice through each line's own
    /// completing `Newline`, so a run spanning MULTIPLE lines (ruling item
    /// 5: "every line in it carries a copy") would otherwise lose the data
    /// after its first line, once the cursor has advanced past the
    /// `ElementAttach` parts that live before it. Seeded into
    /// `resolve_lines_annotated` at the start of each call and updated from
    /// its trailing state afterward — see `take_first_line`'s own doc.
    /// [`Self::flush_lines`] needs no equivalent: it resolves the entire
    /// remaining tail in one call, so the accumulation stays correct
    /// without carrying anything between separate calls.
    pending_element: BTreeMap<String, String>,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            line_scan: Vec::new(),
            completion: LineCompletion::default(),
            transcript: Vec::new(),
            cursor: 0,
            capture: Vec::new(),
            capture_depth: 0,
            fragments: Fragments::default(),
            fragment_capture: Vec::new(),
            fragment_depth: 0,
            fragment_pending_tags: Vec::new(),
            pending_element: BTreeMap::new(),
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

    /// Append `part` to the active target. The one place a part enters the
    /// transcript, so the line-completion state can follow it there.
    fn push_part(&mut self, part: OutputPart) {
        if self.capture_depth == 0 && self.fragment_depth == 0 {
            self.completion.feed(&part);
            self.transcript.push(part);
        } else {
            self.target().push(part);
        }
    }

    /// Where a function's output starts: the active target's length and
    /// which target it was (by capture/fragment depth), recorded at call
    /// time on the function's frame — see [`OutputMark`].
    pub(crate) fn mark(&self) -> OutputMark {
        OutputMark {
            len: self.target_len(),
            capture_depth: self.capture_depth,
            fragment_depth: self.fragment_depth,
        }
    }

    /// Push a newline emitted while `function` (the innermost function
    /// frame's [`OutputMark`], if the top frame is a function) is active.
    ///
    /// Matches the C# runtime's `functionStartInOutputStream` rule
    /// (`PushToOutputStreamIndividual`): while a function has produced no
    /// non-whitespace output since it was entered, a newline is dropped
    /// outright — so a function whose body begins with a conditional block
    /// (whose branch starts with a newline) does not break the line it was
    /// called from, even when that line already holds content from an
    /// earlier call (issue #3519). Once the function has printed, or when a
    /// string capture / fragment began inside it (C#'s `BeginString`
    /// exception — the mark no longer names the active target), the
    /// ordinary [`Self::push_newline`] rules apply.
    pub(crate) fn push_newline_in_function(&mut self, function: Option<OutputMark>) {
        if let Some(mark) = function
            && mark.capture_depth == self.capture_depth
            && mark.fragment_depth == self.fragment_depth
            && self
                .target_ref()
                .get(mark.len..)
                .is_some_and(|since_call| !since_call.iter().any(OutputPart::is_content))
        {
            return;
        }
        self.push_newline();
    }

    /// The active push target, read-only — same priority as [`Self::target`].
    fn target_ref(&self) -> &Vec<OutputPart> {
        if self.capture_depth > 0 {
            &self.capture
        } else if self.fragment_depth > 0 {
            &self.fragment_capture
        } else {
            &self.transcript
        }
    }

    /// Length of the active push target. Used to record function output
    /// start points for trailing whitespace trim on return.
    pub(crate) fn target_len(&self) -> usize {
        if self.capture_depth > 0 {
            self.capture.len()
        } else if self.fragment_depth > 0 {
            self.fragment_capture.len()
        } else {
            self.transcript.len()
        }
    }

    /// Trim trailing whitespace from the active output target, walking
    /// backward to `start`. Matches the C# runtime's
    /// `TrimWhitespaceFromFunctionEnd`: on function return, remove
    /// trailing `Newline`, `Spring`, and whitespace-only text so that
    /// function output doesn't inject unwanted line breaks.
    ///
    /// `Glue` is transparent to the walk, as it is to the C# loop (which
    /// `continue`s past every non-text object): the glue stays, and the
    /// whitespace beneath it goes — so `{x} <>` at the end of a function
    /// leaves `x` glued to whatever follows, not `x ` (issue #3522).
    ///
    /// **The walk stops at the read cursor.** A function whose body spans a
    /// yield point — it printed a line, the consumer took it, and only then
    /// did the function return — has a `start` recorded before parts that
    /// have since been delivered, and trimming those is both meaningless and
    /// destructive. C# has no such case to handle because its output stream
    /// really is emptied at each yield (`ResetOutput`), leaving nothing
    /// behind the equivalent point; brink keeps the whole transcript with a
    /// cursor over it, so the cursor is where C#'s reset happened and is the
    /// floor the walk owes (issue #3539).
    ///
    /// Without the floor the transcript can end up shorter than the cursor,
    /// and every reader of `transcript[cursor..]` panics on the next step —
    /// which is how this surfaced. Silently worse: a locale hot-swap
    /// re-renders from `reset_cursor`, so parts trimmed from behind the
    /// cursor would vanish from a re-render of output the consumer had
    /// already been shown.
    pub(crate) fn trim_function_end(&mut self, start: usize) {
        // The cursor indexes the transcript alone, so it is a floor only
        // when the transcript is the active target — inside a string
        // capture or a fragment, `start` names a position in *that* buffer
        // and the cursor says nothing about it.
        let floor = if self.capture_depth == 0 && self.fragment_depth == 0 {
            start.max(self.cursor)
        } else {
            start
        };
        let on_transcript = self.capture_depth == 0 && self.fragment_depth == 0;
        let target = self.target();
        let mut removed = false;
        let mut i = target.len();
        while i > floor {
            i -= 1;
            let trimmable = match &target[i] {
                // Glue is transparent to the trim (issue #3522), neither
                // removed nor a stopping point.
                OutputPart::Glue => continue,
                OutputPart::Newline | OutputPart::Spring => true,
                OutputPart::Text(s) => s.trim().is_empty(),
                OutputPart::LineRef { flags, .. } => {
                    flags.contains(brink_format::LineFlags::ALL_WS)
                }
                // Issue #3536: a value that renders as whitespace — an
                // empty list, `""`, a `none` — is trimmed exactly like
                // whitespace text. ink stringifies values into the output
                // stream as they are pushed, so by the time its
                // `TrimWhitespaceFromFunctionEnd` runs an empty
                // interpolation is an inline-whitespace `StringValue`
                // there; brink resolves values later (the transcript holds
                // an unresolved `ValueRef`), so the same judgement is made
                // here from the value itself. A value that renders visibly
                // still stops the trim.
                part @ OutputPart::ValueRef(_) => !part.is_visible(),
                _ => false,
            };
            if !trimmable {
                break;
            }
            target.remove(i);
            removed = true;
        }
        if removed && on_transcript {
            self.rescan_completion();
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
            self.push_part(OutputPart::Text(text.to_owned()));
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
        let has_content = if self.capture_depth > 0 || self.fragment_depth > 0 {
            self.has_content()
        } else {
            self.unread_has_content_or_spring()
        };
        if !has_content || self.ends_in_newline() {
            return;
        }
        self.push_part(OutputPart::Newline);
    }

    /// Returns true if the active target contains any text content.
    /// When inside a capture, scans the capture vec (stopping at checkpoint).
    /// When inside a fragment (and no capture is active — same priority
    /// `target()` uses), scans `fragment_capture` the identical way, stopping
    /// at *its own* checkpoint (issue #1839: a fragment capturing more than
    /// one recognized line needs this to see the lines it has already
    /// captured at THIS nesting level, not the outer transcript, which a
    /// multi-statement block capture is the first producer to ever exercise
    /// — every earlier fragment use captured at most one call's worth of
    /// output). When neither is active, scans the transcript from cursor
    /// position.
    fn has_content(&self) -> bool {
        if self.capture_depth > 0 {
            self.capture
                .iter()
                .rev()
                .take_while(|p| !matches!(p, OutputPart::Checkpoint))
                .any(OutputPart::is_content)
        } else if self.fragment_depth > 0 {
            self.fragment_capture
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
    /// Same three-way priority as [`Self::has_content`] (issue #1839).
    fn ends_in_newline(&self) -> bool {
        let target = if self.capture_depth > 0 {
            &self.capture
        } else if self.fragment_depth > 0 {
            &self.fragment_capture
        } else {
            &self.transcript
        };
        matches!(target.last(), Some(OutputPart::Newline))
    }

    /// Returns true if the last part is text ending with whitespace.
    /// Only checks the immediately preceding part — intervening Glue or
    /// Newline parts mean the glue system handles the join instead.
    ///
    /// `LineRef` is not inspected: `LineFlags` no longer carries an
    /// edge-whitespace bit (`STARTS_WITH_WS`/`ENDS_WITH_WS` were removed —
    /// they had no production consumer, and the C# reference runtime never
    /// does sub-token leading/trailing whitespace detection either, so there
    /// was no conformance gap to preserve). A resolved `LineRef` is treated
    /// as not ending in whitespace, same as before this helper had any
    /// `LineRef` case.
    #[cfg(test)]
    fn ends_in_whitespace(&self) -> bool {
        let target = if self.capture_depth > 0 {
            &self.capture
        } else if self.fragment_depth > 0 {
            &self.fragment_capture
        } else {
            &self.transcript
        };
        matches!(target.last(), Some(OutputPart::Text(s)) if s.ends_with(char::is_whitespace))
    }

    pub fn push_glue(&mut self) {
        self.push_part(OutputPart::Glue);
    }

    /// Push a word break. Deduplicated: no consecutive Springs.
    pub fn push_spring(&mut self) {
        if !matches!(self.target_ref().last(), Some(OutputPart::Spring)) {
            self.push_part(OutputPart::Spring);
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
        self.push_part(OutputPart::LineRef {
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
        self.push_part(OutputPart::ValueRef(value));
    }

    /// Push a tag associated with the current output line.
    pub fn push_tag(&mut self, tag: String) {
        self.push_part(OutputPart::Tag(tag));
    }

    /// Merge one field of an `attach = StructName` handler's return value
    /// into the currently open run (issue #2108, `Opcode::AttachElement`'s
    /// handler). See [`OutputPart::ElementAttach`]'s doc for why this is a
    /// transcript entry rather than a `Flow`-level mutation.
    pub(crate) fn push_element_attach(&mut self, key: String, value: String) {
        self.push_part(OutputPart::ElementAttach(key, value));
    }

    /// Close the run the most recent [`Self::push_element_attach`] calls
    /// opened (`Opcode::EndElementRun`'s handler). See
    /// [`OutputPart::ElementAttachEnd`]'s doc.
    pub(crate) fn push_element_attach_end(&mut self) {
        self.push_part(OutputPart::ElementAttachEnd);
    }

    /// Returns true if a capture is currently active.
    /// Whether a string-eval/tag/function-return capture is active — pushes
    /// currently route to transient scratch, not visible output (NS-A2:
    /// the `effect-trace` emit recorder's visibility guard; unused in
    /// ordinary builds, hence the allow).
    #[cfg_attr(not(feature = "effect-trace"), expect(dead_code))]
    pub fn in_capture(&self) -> bool {
        self.capture_depth > 0
    }

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
                    | OutputPart::Spring
                    | OutputPart::ElementAttach(..)
                    | OutputPart::ElementAttachEnd
                    // B4 (`docs/stdlib-spec.md` §1.6b): a final-`None`
                    // value renders empty at the display boundary — same
                    // pass-through treatment as whitespace-only text below,
                    // consistent with `OutputPart::is_content`.
                    | OutputPart::ValueRef(Value::OptionVal(None)) => {}
                    OutputPart::Text(s) if s.trim().is_empty() => {}
                    // A whitespace-only or empty line-table line is
                    // whitespace-only text by another name (issue #3507:
                    // a lifted arm that rendered to `" "` before glue) —
                    // it is not content and does not block the scan,
                    // exactly as `is_content` already classifies it.
                    OutputPart::LineRef { flags, .. }
                        if flags.contains(brink_format::LineFlags::ALL_WS)
                            || flags.contains(brink_format::LineFlags::EMPTY) => {}
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
///
/// Mirrors [`resolve_lines_annotated`]'s per-line suppression (issue #2091,
/// extended to this path by issue #2147 — the string-capture path #2091's
/// PR #2140 did not touch): if a line within the captured text resolves
/// fully empty and at least one of its parts interpolated a `content`-typed
/// value (`Value::FragmentRef`) that itself rendered empty, the line is
/// dropped entirely — not left behind as a blank line — same as the
/// streaming/batch `resolve_lines` path.
///
/// `resolve_lines_annotated` does **not** call this function directly — the
/// two hold independent copies of the same suppression logic, applied at
/// different granularities. `resolve_parts`'s real callers are:
///
/// - [`OutputBuffer::end_capture`] — `Opcode::EndStringEval`'s resolution
///   path (e.g. an unrecognized choice display, or any
///   `~ temp x = "..."` string-eval capture);
/// - [`OutputBuffer::resolve_fragment`] (`output/fragment.rs`) — the
///   resolver `ChoiceDisplay::Fragment` reads through (`story/mod.rs`,
///   `story/flow_instance.rs`), so a captured choice's display text is
///   affected too (`brink-cli`'s `tui/app.rs` reads it from there);
/// - [`resolve_part`]'s `ValueRef(Value::FragmentRef)` arm and
///   [`resolve_line_parts`]'s `LinePart::Slot` `FragmentRef` arm — both
///   recurse into `resolve_parts` to resolve a fragment's own *interior*,
///   and both are themselves reachable from `resolve_lines_annotated`'s
///   top-level resolution whenever a rendered line references a fragment.
///   So this suppression also reaches inside any nested, multi-line
///   fragment rendered on the streaming/batch path — a blank line
///   contributed purely by an inner, rendered-empty fragment now vanishes
///   from the *interior* of an outer fragment's captured text too, not
///   only at a transcript line's own top level.
///
/// No `current_tags`-style tag exception is needed here (unlike
/// `resolve_lines_annotated`): a `Tag` already sets `after_glue`, which
/// unconditionally skips the newline right after it (pre-existing behavior,
/// untouched by this fix) — so a tag-then-newline sequence never reaches
/// this suppression check in the first place, and tags carry no characters
/// into a captured string's text regardless.
fn resolve_parts(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> String {
    // First pass: mark newlines that should be removed by glue.
    let mut remove = vec![false; parts.len()];
    mark_glue_removals(parts, &mut remove);

    let mut out = String::new();
    let mut after_glue = false;
    // issue #2147: track the start of the current (in-progress) line within
    // `out`, and whether it saw a `content`/Fragment interpolation, so a
    // line that resolves fully empty purely from a rendered-empty fragment
    // can be dropped rather than left as a stray blank line.
    let mut line_start = 0usize;
    let mut saw_fragment_ref = false;
    // Issue #3507: where the current line began in `out`, counting a
    // glue-removed `Newline` too (unlike `line_start`, which only moves on
    // a kept one). ink's glue trims the trailing newline AND every
    // whitespace-only string after it (`TrimNewlinesFromOutputStream`), so
    // `a` / `{false:x} <>` / `b` prints `ab`: the spring's space after the
    // empty construct dies with the newline. When content DID land on the
    // line (`{0} <>`), the newline is not trailing and the space survives
    // (`0 world`).
    let mut since_newline = 0usize;

    for (i, part) in parts.iter().enumerate() {
        if remove[i] {
            match part {
                OutputPart::Glue => {
                    after_glue = true;
                    if out[since_newline..].trim().is_empty() {
                        out.truncate(since_newline);
                    }
                }
                OutputPart::Newline => since_newline = out.len(),
                _ => {}
            }
            continue;
        }
        match part {
            OutputPart::Text(_) | OutputPart::LineRef { .. } | OutputPart::ValueRef(_) => {
                if part_involves_fragment_ref(part) {
                    saw_fragment_ref = true;
                }
                let start = out.len();
                resolve_part_into(part, &mut out, program, line_tables, resolver, fragments);
                // Collapse adjacent whitespace at part boundaries.
                if collapse_join(&mut out, start) {
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
                    if saw_fragment_ref && out[line_start..].trim().is_empty() {
                        // Suppress: drop the whole (whitespace-only) line
                        // and its trailing newline, not just its text.
                        out.truncate(line_start);
                    } else {
                        out.push('\n');
                        line_start = out.len();
                    }
                    saw_fragment_ref = false;
                }
                since_newline = out.len();
            }
            OutputPart::Glue
            | OutputPart::Checkpoint
            | OutputPart::Tag(_)
            | OutputPart::ElementAttach(..)
            | OutputPart::ElementAttachEnd => {
                after_glue = true;
            }
        }
    }

    // issue #2147 (trailing-entry parity with `resolve_lines_annotated`'s
    // own `EXCEPTION (issue #2091)` handling of its final, unterminated
    // entry): a captured string need not end on a `Newline` part. If the
    // text since the last committed line resolves empty and interpolated a
    // Fragment, drop it AND the newline that introduced it — mirroring how
    // `resolve_lines` drops that trailing entry from its `Vec` whole (no
    // join separator left behind for it either). Without this, parts like
    // `[Text("a"), Newline, ValueRef(FragmentRef(<empty>))]` resolved to
    // `"a\n"` here while `resolve_lines` (joining its per-line `Vec`, which
    // dropped the suppressed trailing entry) produced just `"a"`.
    if saw_fragment_ref && line_start > 0 && out[line_start..].trim().is_empty() {
        out.truncate(line_start - 1);
    }

    out
}

/// Returns true if `part` interpolates a `content`-typed value
/// (`Value::FragmentRef`) — either directly (`ValueRef`) or through a
/// template `Slot` (`LineRef`).
///
/// Two distinct mechanisms produce a `FragmentRef` in this position, and
/// this check does not — and structurally cannot — tell them apart: issue
/// #1839's `block`-capture receiver, AND the ordinary display-position
/// call-composition pattern `brink-codegen-inkb::content::emit_slot_expr`
/// emits for *every* template slot whose expr is a function call
/// (`lir::Expr::is_function_call()`, both dialects) — e.g. a line whose
/// only content is `{ f() }`. Both are suppressed identically by the
/// caller.
///
/// Purely structural: it does not need to look inside the referenced
/// fragment to decide suppression. If a line's fully-resolved text comes
/// out empty *and* one of its parts involved a fragment reference, that is
/// sufficient evidence the fragment itself **rendered** empty. It does
/// *not* follow that the fragment "captured nothing" — a fragment that
/// captured a line which itself renders empty (e.g. an interpolated empty
/// variable), or a call-composition fragment whose function simply
/// returned `""`, both reach this same state. "Rendered empty" is the
/// weaker, sufficient invariant suppression actually relies on.
fn part_involves_fragment_ref(part: &OutputPart) -> bool {
    match part {
        OutputPart::LineRef { slots, .. } => {
            slots.iter().any(|v| matches!(v, Value::FragmentRef(_)))
        }
        OutputPart::ValueRef(Value::FragmentRef(_)) => true,
        _ => false,
    }
}

/// Resolve glue and split into per-line output with associated tags and
/// element-attachment data.
///
/// A resolved line: text, tags, element-attachment data (issue #2108,
/// [`OutputPart::ElementAttach`]'s own doc), and the line's source
/// location (W7/#3300 transcript provenance — the FIRST `LineRef` part's
/// line-table `source_location`; `None` when the line has no `LineRef`,
/// e.g. pure interpolation, or its entry carries no location).
pub(crate) type ResolvedLine = (
    String,
    Vec<String>,
    BTreeMap<String, String>,
    Option<brink_format::SourceLocation>,
);

/// [`ResolvedLine`] plus the issue #2091 suppression flag —
/// [`resolve_lines_annotated`]'s own unfiltered form.
pub(crate) type AnnotatedResolvedLine = (
    String,
    Vec<String>,
    bool,
    BTreeMap<String, String>,
    Option<brink_format::SourceLocation>,
);

/// Each returned element is `(line_text, line_tags, line_element_data)`.
/// Tags reset every line; element-attachment data (issue #2108) persists
/// across lines until an `ElementAttachEnd` closes the run — see
/// [`OutputPart::ElementAttach`]'s own doc. Lines that
/// [`resolve_lines_annotated`] marks suppressed (issue #2091 — an empty
/// `content`/Fragment capture) are dropped entirely; nothing else changes.
pub(crate) fn resolve_lines(
    parts: &[OutputPart],
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> Vec<ResolvedLine> {
    resolve_lines_annotated(
        parts,
        BTreeMap::new(),
        program,
        line_tables,
        resolver,
        fragments,
    )
    .into_iter()
    .filter_map(|(text, tags, suppressed, element, source)| {
        (!suppressed).then_some((text, tags, element, source))
    })
    .collect()
}

/// Like [`resolve_lines`], but reports — per resolved line, as the trailing
/// `bool` — whether it should be **suppressed** from reader-visible output:
/// its fully-resolved text came out empty, it carries no tags, and at least
/// one of its parts interpolated a `content`-typed value that itself
/// rendered empty (issue #2091). Two distinct call sites produce that
/// `Value::FragmentRef`, and this check treats them identically:
///
/// - issue #1839's `block`-capture receiver — e.g. a capture that
///   terminated immediately because the next line was itself
///   element-level (`hir::lower_native::element::capture_block`); and
/// - the ordinary **display-position call-composition** pattern
///   `brink-codegen-inkb::content::emit_slot_expr` emits
///   (`BeginFragment`…`EndFragment`) for *every* template slot whose expr
///   is a function call (`lir::Expr::is_function_call()`, both dialects) —
///   e.g. a line whose only content is `{ f() }`, where `f` emits no
///   side-effect text and returns an empty value.
///
/// See [`part_involves_fragment_ref`]'s own doc for why "the fragment
/// rendered empty" is the invariant relied on here, not "the fragment
/// captured nothing" — the two mechanisms above are exactly why the
/// stronger claim does not hold.
///
/// This is a **read-time rendering decision only**: the line-table entry a
/// suppressed line's `LineRef` points at is never touched, omitted, or
/// renumbered — it stays present-but-empty, exactly as compiled, so
/// locale hot-swap (which re-renders the *same* transcript against a
/// swapped-in line vector, matched by index) keeps working unchanged. Only
/// the rendered *output line* disappears; the underlying compiled data does
/// not move.
///
/// A line that resolves empty for any OTHER reason — a literal blank line,
/// or a self-closing inline markup span (`<pause/>`) with no children — is
/// **not** suppressed: that is pre-existing, deliberate output (see the
/// `inline-markup-point-marker` fixture, issue #1716), unrelated to this
/// issue's scope of `content`/Fragment-driven emptiness.
///
/// [`OutputBuffer::take_first_line`] needs this unfiltered, index-aligned
/// form — its single-newline slice always resolves to exactly two entries
/// (the found line, then an always-empty trailing filler) — so it can tell
/// "this line should be skipped, keep scanning for the next one" apart from
/// "there is no completed line at all" without losing that index alignment
/// (naively dropping the suppressed entry from the `Vec` would shift the
/// filler into its place and return the very blank line being suppressed).
///
/// `seed_element` (issue #2108) is the element-attachment state already
/// accumulated BEFORE `parts` starts — `take_first_line` passes its own
/// carried-forward [`OutputBuffer::pending_element`] here (a multi-line
/// attach run spans more than one `take_first_line` call, each resolving
/// only its own line's slice); every other caller passes an empty map,
/// since they resolve from a cold start. The trailing filler entry's own
/// element field is always the state at the END of `parts` — callers that
/// need to carry it forward (again, only `take_first_line`) read it from
/// there.
/// Fold one more `LineRef`'s source into the line's: the first sets it,
/// a later one in the same file widens it to cover both, one from another
/// file is ignored.
fn widen_source(
    current: &mut Option<brink_format::SourceLocation>,
    entry: Option<&brink_format::SourceLocation>,
) {
    match (current, entry) {
        (current @ None, Some(src)) => *current = Some(src.clone()),
        (Some(cur), Some(src)) if cur.file == src.file => {
            cur.range_start = cur.range_start.min(src.range_start);
            cur.range_end = cur.range_end.max(src.range_end);
        }
        _ => {}
    }
}

/// Resolve `parts` into annotated lines, computing the glue marks itself.
///
/// The general entry point (`resolve_lines`, the transcript replayers).
/// The streaming consumers in `consume.rs` already hold the marks for the
/// slice they resolve and go through [`resolve_lines_annotated_marked`] /
/// [`resolve_first_line_annotated`] instead of recomputing them.
pub(crate) fn resolve_lines_annotated(
    parts: &[OutputPart],
    seed_element: BTreeMap<String, String>,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> Vec<AnnotatedResolvedLine> {
    if parts.is_empty() {
        return Vec::new();
    }
    let mut remove = vec![false; parts.len()];
    mark_glue_removals(parts, &mut remove);
    resolve_lines_annotated_marked(
        parts,
        &remove,
        seed_element,
        program,
        line_tables,
        resolver,
        fragments,
    )
}

/// [`resolve_lines_annotated`] over precomputed glue marks (`remove[i]` is
/// whether `parts[i]` is a glue-removed part, as [`mark_glue_removals`]
/// fills them in for exactly this slice).
///
/// The result always carries one final entry for the text after the last
/// `Newline` (possibly empty) — its element field is the attachment state
/// the caller carries forward.
pub(crate) fn resolve_lines_annotated_marked(
    parts: &[OutputPart],
    remove: &[bool],
    seed_element: BTreeMap<String, String>,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> Vec<AnnotatedResolvedLine> {
    if parts.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<AnnotatedResolvedLine> = Vec::new();
    let trailing = drive_lines(
        parts,
        remove,
        seed_element,
        program,
        line_tables,
        resolver,
        fragments,
        |line| lines.push(line),
    );
    lines.push(trailing);
    lines
}

/// The streaming shape of [`resolve_lines_annotated_marked`]: resolve a
/// slice that [`OutputBuffer::take_first_line`] has cut to end exactly on
/// the first completed line's `Newline`, returning that line and the
/// element-attachment state to carry into the next call — without
/// materialising a `Vec` for what is, by construction, one line plus an
/// empty trailing entry.
///
/// Faithful to the batch path's contract even off that construction: the
/// returned line is the first one the walk produces (the trailing entry if
/// it produces none — a slice that is all glue-removed or after-glue), and
/// the carried state is the element field of whatever entry follows it.
pub(crate) fn resolve_first_line_annotated(
    parts: &[OutputPart],
    remove: &[bool],
    seed_element: BTreeMap<String, String>,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
) -> (AnnotatedResolvedLine, BTreeMap<String, String>) {
    let mut first: Option<AnnotatedResolvedLine> = None;
    let mut next_element: Option<BTreeMap<String, String>> = None;
    let trailing = drive_lines(
        parts,
        remove,
        seed_element,
        program,
        line_tables,
        resolver,
        fragments,
        |line| {
            if first.is_none() {
                first = Some(line);
            } else if next_element.is_none() {
                next_element = Some(line.3);
            }
        },
    );
    match first {
        Some(line) => (line, next_element.unwrap_or(trailing.3)),
        None => (trailing, BTreeMap::new()),
    }
}

/// Trim leading and trailing whitespace without reallocating: the tail is
/// truncated and the head shifted down in place. The buffer keeps its
/// capacity, which is what lets `take_first_line`'s terminating `'\n'`
/// land without a `realloc` on the common line.
fn trim_in_place(s: &mut String) {
    let end = s.trim_end().len();
    s.truncate(end);
    let lead = s.len() - s.trim_start().len();
    if lead > 0 {
        s.replace_range(..lead, "");
    }
}

/// One linear pass over `parts`, emitting each completed line through
/// `emit` and returning the trailing (unterminated) entry. Shared by the
/// batch and streaming resolvers above so the two cannot drift.
#[expect(
    clippy::too_many_arguments,
    reason = "the resolver context plus the sink"
)]
fn drive_lines(
    parts: &[OutputPart],
    remove: &[bool],
    seed_element: BTreeMap<String, String>,
    program: &Program,
    line_tables: &[Vec<LineEntry>],
    resolver: Option<&dyn PluralResolver>,
    fragments: &Fragments,
    mut emit: impl FnMut(AnnotatedResolvedLine),
) -> AnnotatedResolvedLine {
    debug_assert_eq!(remove.len(), parts.len(), "one glue mark per part");
    let mut current_text = String::new();
    let mut current_tags: Vec<String> = Vec::new();
    // Issue #2108: unlike `current_tags` (reset every line), this
    // ACCUMULATES across lines — cleared only by `ElementAttachEnd` — so
    // every line materialized while a run is open gets a copy (ruling item
    // 5). Cloned, never moved, into each pushed line entry below. Seeded
    // from the caller's already-accumulated state (see this function's own
    // `seed_element` doc) rather than always starting empty.
    let mut current_element: BTreeMap<String, String> = seed_element;
    // The line's provenance (W7/#3300): the span of its `LineRef`s'
    // line-table `source_location`s — from the first ref's start to the
    // furthest end among refs in that same file (a glue-joined line, or a
    // prose-dialect cue + aside + dialogue, spans several source lines and
    // the host highlights them all; feedback 2026-09-02). A ref from
    // another file never widens it. Reset per line.
    let mut current_source: Option<brink_format::SourceLocation> = None;
    let mut saw_fragment_ref = false;
    let mut after_glue = false;
    // Issue #3507 — see `resolve_parts`'s `since_newline`: the
    // point in `current_text` where the current source line began, counting
    // a glue-removed newline, so glue can drop whitespace-only text that
    // followed that newline the way ink's `TrimNewlinesFromOutputStream`
    // does.
    let mut since_newline = 0usize;

    for (i, part) in parts.iter().enumerate() {
        if remove[i] {
            match part {
                OutputPart::Glue => {
                    after_glue = true;
                    if current_text[since_newline..].trim().is_empty() {
                        current_text.truncate(since_newline);
                    }
                }
                OutputPart::Newline => since_newline = current_text.len(),
                _ => {}
            }
            continue;
        }
        match part {
            OutputPart::Text(_) | OutputPart::LineRef { .. } | OutputPart::ValueRef(_) => {
                if let OutputPart::LineRef {
                    container_idx,
                    line_idx,
                    ..
                } = part
                {
                    // Same table selection as `resolve_line_ref`: a
                    // `LineRef`'s `container_idx` keys the SCOPE table via
                    // `scope_table_idx`, never `line_tables` directly —
                    // indexing raw silently reads another scope's line
                    // (found live: every provenance chip pointed at the
                    // wrong place while the TEXT — resolved through the
                    // correct road — looked fine).
                    let scope_idx = program.scope_table_idx(*container_idx) as usize;
                    let entry_source = line_tables
                        .get(scope_idx)
                        .and_then(|t| t.get(*line_idx as usize))
                        .and_then(|entry| entry.source_location.as_ref());
                    widen_source(&mut current_source, entry_source);
                }
                if part_involves_fragment_ref(part) {
                    saw_fragment_ref = true;
                }
                let start = current_text.len();
                resolve_part_into(
                    part,
                    &mut current_text,
                    program,
                    line_tables,
                    resolver,
                    fragments,
                );
                // Collapse adjacent whitespace at part boundaries.
                if collapse_join(&mut current_text, start) {
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
                    trim_in_place(&mut current_text);
                    let suppressed =
                        current_text.is_empty() && current_tags.is_empty() && saw_fragment_ref;
                    emit((
                        mem::take(&mut current_text),
                        mem::take(&mut current_tags),
                        suppressed,
                        current_element.clone(),
                        current_source.take(),
                    ));
                    saw_fragment_ref = false;
                }
                since_newline = current_text.len();
            }
            OutputPart::Tag(tag) => {
                current_tags.push(tag.clone());
            }
            OutputPart::ElementAttach(key, value) => {
                current_element.insert(key.clone(), value.clone());
            }
            OutputPart::ElementAttachEnd => {
                current_element.clear();
            }
            OutputPart::Glue | OutputPart::Checkpoint => {
                after_glue = true;
            }
        }
    }

    // Push the final line — even if empty — so that a trailing Newline
    // part produces a trailing `\n` when the lines are joined by
    // `resolve_lines`'s callers (e.g. `flush_remaining`'s `\n`-join over
    // consecutive entries).
    //
    // EXCEPTION (issue #2091): this final entry is itself eligible for
    // suppression like any other — if the transcript's unread tail ends
    // with an unterminated fragment-bearing segment that resolves empty
    // (no following `Newline`), `suppressed` is `true` here too, and
    // `resolve_lines` drops this entry from its `Vec` entirely rather than
    // keeping it as a `("", [])` placeholder. When that happens, the
    // trailing-`\n`-via-empty-final-entry guarantee this comment describes
    // does NOT hold for the preceding real line — there is no longer a
    // placeholder entry left for a caller's join loop to add a separator
    // before. This is accepted, not additionally special-cased: it only
    // arises when the story's last visible output is itself an empty
    // `content`/Fragment interpolation, which is precisely the case this
    // issue suppresses.
    trim_in_place(&mut current_text);
    let suppressed = current_text.is_empty() && current_tags.is_empty() && saw_fragment_ref;
    (
        current_text,
        current_tags,
        suppressed,
        current_element,
        current_source,
    )
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
        address_by_path: HashMap::new(),
        container_paths: HashMap::new(),
        root_idx: 0,
        list_literals: vec![],
        literal_pool: vec![],
        list_item_map: HashMap::new(),
        list_defs: vec![],
        list_def_map: HashMap::new(),
        external_fns: HashMap::new(),
        local_scope_defaults: Vec::new(),
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        alias_table: Vec::new(),
        debug_info: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The streaming resolver `take_first_line` uses must agree with the
    /// batch resolver it replaced, entry for entry: same first line, and the
    /// carried element state is the element of the entry that follows it.
    /// Over the shapes the streaming path meets — plain lines, glue across
    /// a newline, tags, and an element run that ends right after the line.
    #[test]
    fn first_line_resolver_matches_batch_resolver() {
        let program = test_dummy_program();
        let seed = |k: &str, v: &str| {
            let mut m = BTreeMap::new();
            m.insert(k.to_string(), v.to_string());
            m
        };
        let cases: Vec<(Vec<OutputPart>, BTreeMap<String, String>)> = vec![
            (
                vec![
                    OutputPart::Text("hello ".to_string()),
                    OutputPart::Text(" world".to_string()),
                    OutputPart::Newline,
                    OutputPart::Text("next".to_string()),
                ],
                BTreeMap::new(),
            ),
            (
                vec![
                    OutputPart::Text("a".to_string()),
                    OutputPart::Newline,
                    OutputPart::Glue,
                    OutputPart::Text("b".to_string()),
                    OutputPart::Newline,
                ],
                BTreeMap::new(),
            ),
            (
                vec![
                    OutputPart::Tag("t".to_string()),
                    OutputPart::Text("  tagged  ".to_string()),
                    OutputPart::Newline,
                ],
                BTreeMap::new(),
            ),
            (
                vec![
                    OutputPart::ElementAttach("k".to_string(), "v".to_string()),
                    OutputPart::Text("in run".to_string()),
                    OutputPart::Newline,
                    OutputPart::ElementAttachEnd,
                ],
                seed("outer", "x"),
            ),
            (
                vec![
                    OutputPart::Text("carried".to_string()),
                    OutputPart::Newline,
                    OutputPart::ElementAttach("k2".to_string(), "v2".to_string()),
                ],
                seed("outer", "x"),
            ),
        ];
        for (parts, seed_element) in cases {
            let mut remove = vec![false; parts.len()];
            mark_glue_removals(&parts, &mut remove);
            // The slice `take_first_line` would cut: through the first
            // newline the glue marks leave standing.
            let split_at = parts
                .iter()
                .enumerate()
                .position(|(i, p)| matches!(p, OutputPart::Newline) && !remove[i])
                .expect("every case carries a kept newline");
            let slice = &parts[..=split_at];
            let marks = &remove[..=split_at];
            let batch = resolve_lines_annotated_marked(
                slice,
                marks,
                seed_element.clone(),
                &program,
                &[],
                None,
                &Fragments::default(),
            );
            let (line, next_element) = resolve_first_line_annotated(
                slice,
                marks,
                seed_element,
                &program,
                &[],
                None,
                &Fragments::default(),
            );
            assert_eq!(
                batch.len(),
                2,
                "one line plus the trailing entry: {parts:?}"
            );
            assert_eq!(line, batch[0], "first line differs: {parts:?}");
            assert_eq!(
                next_element, batch[1].3,
                "carried element differs: {parts:?}"
            );
        }
    }

    /// Test helpers — `OutputBuffer` methods that need resolution context.
    /// Tests only use Text/Newline/Glue, so we pass an empty program.
    impl OutputBuffer {
        fn test_flush_lines(&mut self) -> Vec<(String, Vec<String>)> {
            let p = test_dummy_program();
            // Element-attachment data (issue #2108) is dropped here — none
            // of these pre-existing tests exercise attach conventions, and
            // widening every existing `(text, tags)` assertion in this
            // module for a field they never populate would just be noise.
            // `crates/brink-runtime/tests/element.rs` exercises the real
            // per-line element data end to end instead.
            self.flush_lines(&p, &[], None)
                .into_iter()
                .map(|(text, tags, _element, _source)| (text, tags))
                .collect()
        }

        fn test_take_first_line(&mut self) -> Option<(String, Vec<String>)> {
            let p = test_dummy_program();
            self.take_first_line(&p, &[], None)
                .map(|(text, tags, _element, _source)| (text, tags))
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
    /// the glue from removing the newline — and (issue #3507) it goes WITH
    /// the newline: ink's `TrimNewlinesFromOutputStream` removes the trailing
    /// newline and every whitespace-only string after it, so `a` /
    /// `{false:x} <>` / `b` prints `ab` (inkjs 2.4.0 via
    /// `tools/inkjs-oracle`). This test used to pin `a b`, which was the
    /// divergence.
    #[test]
    fn glue_skips_whitespace_only_text_to_find_newline() {
        let mut buf = OutputBuffer::new();
        buf.push_text("a");
        buf.push_newline();
        buf.push_text(" ");
        buf.push_glue();
        buf.push_text("b");
        assert_eq!(buf.flush(), "ab");
    }

    /// Issue #3507: a `Spring` between a glue-removed newline and the glue
    /// is whitespace after that newline and dies with it (`ab`); with
    /// content on the line the newline is not trailing, so the spring's
    /// space survives (`0 world`).
    #[test]
    fn spring_before_glue_survives_only_after_line_content() {
        let mut buf = OutputBuffer::new();
        buf.push_text("a");
        buf.push_newline();
        buf.push_spring();
        buf.push_glue();
        buf.push_text("b");
        assert_eq!(buf.flush(), "ab");

        let mut buf = OutputBuffer::new();
        buf.push_text("a");
        buf.push_newline();
        buf.push_text("0");
        buf.push_spring();
        buf.push_glue();
        buf.push_text("world");
        assert_eq!(buf.flush(), "a\n0 world");
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
                param_count: 0,
                params: Vec::new(),
                scope_table_idx: 0,
                scope_id: id,
            }],
            address_map: HashMap::new(),
            scope_ids: vec![id],
            source_checksum: 0,
            globals: vec![],
            global_map: HashMap::new(),
            name_table: vec![],
            address_by_path: HashMap::new(),
            container_paths: HashMap::new(),
            root_idx: 0,
            list_literals: vec![],
            literal_pool: vec![],
            list_item_map: HashMap::new(),
            list_defs: vec![],
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
            local_scope_defaults: Vec::new(),
            struct_shapes: Vec::new(),
            private_defs: Vec::new(),
            alias_table: Vec::new(),
            debug_info: None,
        };

        let line_tables = vec![vec![LineEntry {
            content: LineContent::Template(parts),
            source_hash: 0,
            flags: LineFlags::empty(),
            audio_ref: None,
            slot_info: vec![],
            source_location: None,
        }]];

        resolve_line_ref(
            &program,
            &line_tables,
            0,
            0,
            slots,
            None,
            &Fragments::default(),
        )
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

    // ── Inline markup spans (#1716, docs/prose-dialect-spec.md §4) ─────
    //
    // No structured `Part::Span` consumer surface exists yet (§7/§9.1 ⏳)
    // — a span resolves to its children's concatenated text, tag name/
    // attrs stripped, recursing through the same `resolve_line_parts` a
    // plain Template does.

    #[test]
    fn span_resolves_to_its_children_text_tag_stripped() {
        let result = resolve_template(
            vec![
                LinePart::Literal("Hello ".into()),
                LinePart::Span {
                    name: "wave".into(),
                    attrs: vec![],
                    children: vec![LinePart::Literal("world".into())],
                },
            ],
            &[],
        );
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn a_self_closing_span_with_no_children_resolves_to_nothing() {
        let result = resolve_template(
            vec![
                LinePart::Literal("Bell tolls. ".into()),
                LinePart::Span {
                    name: "pause".into(),
                    attrs: vec![],
                    children: vec![],
                },
                LinePart::Literal(" Door slams.".into()),
            ],
            &[],
        );
        assert_eq!(result, "Bell tolls. Door slams.");
    }

    #[test]
    fn a_span_containing_a_slot_resolves_the_slot() {
        let result = resolve_template(
            vec![LinePart::Span {
                name: "b".into(),
                attrs: vec![],
                children: vec![LinePart::Literal("hello ".into()), LinePart::Slot(0)],
            }],
            &[Value::String("Fogg".into())],
        );
        assert_eq!(result, "hello Fogg");
    }

    #[test]
    fn nested_spans_resolve_recursively() {
        let result = resolve_template(
            vec![LinePart::Span {
                name: "b".into(),
                attrs: vec![],
                children: vec![LinePart::Span {
                    name: "i".into(),
                    attrs: vec![],
                    children: vec![LinePart::Literal("hi".into())],
                }],
            }],
            &[],
        );
        assert_eq!(result, "hi");
    }

    // ── B4 display-boundary forgiveness (`docs/stdlib-spec.md` §1.6b) ──

    /// A final-`None` template slot renders as nothing — the surrounding
    /// whitespace collapses exactly like the pre-existing `Null`/empty-
    /// string slot cases above.
    #[test]
    fn template_none_option_slot_renders_as_nothing() {
        let result = resolve_template(
            vec![
                LinePart::Literal("Hello ".into()),
                LinePart::Slot(0),
                LinePart::Literal(" world".into()),
            ],
            &[Value::none()],
        );
        assert_eq!(result, "Hello world");
    }

    /// `Some(v)` at the same slot position is unaffected by the boundary —
    /// still `some(<v>)`, the F28 total rendering `stringify` gives it.
    #[test]
    fn template_some_option_slot_renders_totally() {
        let result = resolve_template(
            vec![LinePart::Literal("val: ".into()), LinePart::Slot(0)],
            &[Value::some(Value::Int(3))],
        );
        assert_eq!(result, "val: some(3)");
    }

    /// A bare `OutputPart::ValueRef` (the `EmitValue`/unrecognized-content
    /// path, not a template slot) gets the same forgiveness — and the
    /// surrounding whitespace collapses across it exactly like it already
    /// does across an eagerly-dropped `Value::Null` or an empty string
    /// (`adjacent_whitespace_collapsed`, above): "before " + (nothing) +
    /// " after" reads as one collapsed space, not two.
    #[test]
    fn value_ref_none_option_renders_as_nothing() {
        let mut buf = OutputBuffer::new();
        buf.push_text("before ");
        buf.push_value_ref(Value::none());
        buf.push_text(" after");
        assert_eq!(buf.flush(), "before after");
    }

    /// Traceability rider (§1.6b): the append-only transcript is never
    /// eagerly resolved (`docs/runtime-restructuring-spec.md`'s
    /// deferred-resolution model) — a forgiven `None`-render still shows up
    /// as `Value::OptionVal(None)` in `transcript()`, distinct from a slot
    /// that carried no value at all. Resolving to text loses the
    /// information; the structural transcript never does.
    #[test]
    fn none_render_is_traceable_in_the_raw_transcript() {
        let mut buf = OutputBuffer::new();
        buf.push_value_ref(Value::none());
        assert!(
            buf.transcript()
                .iter()
                .any(|p| matches!(p, OutputPart::ValueRef(Value::OptionVal(None)))),
            "the raw None value must survive in the transcript: {:?}",
            buf.transcript()
        );
        // Resolving it, separately, gives the forgiven empty text.
        assert_eq!(buf.flush(), "");
    }

    /// A leading `None`-rendering value must not count as content for
    /// leading-newline suppression — otherwise a story that opens with a
    /// forgiven interpolation would get a spurious blank line before its
    /// real content.
    #[test]
    fn leading_none_option_value_does_not_block_newline_suppression() {
        let mut buf = OutputBuffer::new();
        buf.push_value_ref(Value::none());
        buf.push_newline();
        buf.push_text("hello");
        assert_eq!(buf.flush(), "hello");
    }

    /// A `None`-rendering value between glue and its target newline must
    /// not block the glue scan — it passes through like whitespace-only
    /// text, matching `mark_glue_removals`'s existing arms.
    #[test]
    fn none_option_value_does_not_block_glue_scan() {
        let mut buf = OutputBuffer::new();
        buf.push_text("hello");
        buf.push_newline();
        buf.push_value_ref(Value::none());
        buf.push_glue();
        buf.push_text("world");
        assert_eq!(buf.flush(), "helloworld");
    }

    // ── #2091: suppress a blank line from an empty content/Fragment capture ──
    //
    // A `block`-capturing handler (issue #1839) whose captured run is empty —
    // e.g. a cue immediately followed by a parenthetical, so
    // `hir::lower_native::element::capture_block` finds zero interior lines
    // — still binds its `content`-typed parameter to a real (empty)
    // `Value::FragmentRef`. Interpolating that alone on a template line
    // (`{body}` in a prose-ground handler body) used to render its own
    // visible blank line. These tests exercise the fix directly against the
    // output-resolution layer, independent of the full compiler pipeline
    // (see `tests/tier1-native/conventions-screenplay-preset/` for the e2e
    // golden fixture this same fix corrects).

    /// Build a minimal one-container `Program` plus a matching line table
    /// from a caller-supplied list of `LineEntry`s (indices become
    /// `line_idx`), for `resolve_lines`/`take_first_line` tests that need
    /// more than `resolve_template`'s single entry.
    fn program_with_line_table(entries: Vec<LineEntry>) -> (Program, Vec<Vec<LineEntry>>) {
        use crate::program::LinkedContainer;
        use brink_format::{CountingFlags, DefinitionId, DefinitionTag};
        use std::collections::HashMap;

        let id = DefinitionId::new(DefinitionTag::Address, 0);
        let program = Program {
            containers: vec![LinkedContainer {
                id,
                bytecode: vec![],
                counting_flags: CountingFlags::empty(),
                path_hash: 0,
                param_count: 0,
                params: Vec::new(),
                scope_table_idx: 0,
                scope_id: id,
            }],
            address_map: HashMap::new(),
            scope_ids: vec![id],
            source_checksum: 0,
            globals: vec![],
            global_map: HashMap::new(),
            name_table: vec![],
            address_by_path: HashMap::new(),
            container_paths: HashMap::new(),
            root_idx: 0,
            list_literals: vec![],
            literal_pool: vec![],
            list_item_map: HashMap::new(),
            list_defs: vec![],
            list_def_map: HashMap::new(),
            external_fns: HashMap::new(),
            local_scope_defaults: Vec::new(),
            struct_shapes: Vec::new(),
            private_defs: Vec::new(),
            alias_table: Vec::new(),
            debug_info: None,
        };
        (program, vec![entries])
    }

    fn plain_entry(s: &str) -> LineEntry {
        LineEntry {
            content: LineContent::Plain(s.to_string()),
            source_hash: 0,
            flags: brink_format::LineFlags::from_plain(s),
            audio_ref: None,
            slot_info: vec![],
            source_location: None,
        }
    }

    fn one_slot_template_entry() -> LineEntry {
        LineEntry {
            content: LineContent::Template(vec![LinePart::Slot(0)]),
            source_hash: 0,
            // A Slot always defeats the compile-time conservative flags —
            // see `LineFlags::from_template`'s own doc/tests.
            flags: brink_format::LineFlags::empty(),
            audio_ref: None,
            slot_info: vec![],
            source_location: None,
        }
    }

    fn line_ref(line_idx: u16, slots: Vec<Value>, flags: brink_format::LineFlags) -> OutputPart {
        OutputPart::LineRef {
            container_idx: 0,
            line_idx,
            slots,
            flags,
        }
    }

    #[test]
    fn resolve_lines_suppresses_a_blank_line_from_an_empty_content_capture() {
        // line 0: "VENDOR", line 1: `{body}` (the block-capture receiver),
        // line 2: "(hushed)" — matches the shape of the real regression
        // (`tests/tier1-native/conventions-screenplay-preset/story.brink`).
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("VENDOR"),
            one_slot_template_entry(),
            plain_entry("(hushed)"),
        ]);
        // The captured block was empty: a real, present `Fragment` with no
        // parts — not an omitted line-table entry (issue #2091's own "what
        // happens to the line-table entry" question: present-but-empty).
        let fragments = Fragments::from(vec![Fragment {
            parts: vec![],
            tags: vec![],
        }]);

        let parts = vec![
            line_ref(0, vec![], brink_format::LineFlags::from_plain("VENDOR")),
            OutputPart::Newline,
            line_ref(
                1,
                vec![Value::FragmentRef(0)],
                brink_format::LineFlags::empty(),
            ),
            OutputPart::Newline,
            line_ref(2, vec![], brink_format::LineFlags::from_plain("(hushed)")),
        ];

        // Element-attachment data (issue #2108) is dropped here — these
        // pre-existing fixtures don't exercise attach conventions.
        let lines: Vec<(String, Vec<String>)> =
            resolve_lines(&parts, &program, &line_tables, None, &fragments)
                .into_iter()
                .map(|(text, tags, _element, _source)| (text, tags))
                .collect();
        assert_eq!(
            lines,
            vec![
                ("VENDOR".to_string(), Vec::<String>::new()),
                ("(hushed)".to_string(), Vec::<String>::new()),
            ],
            "an empty content/Fragment capture must not render its own blank \
             line between real content: {lines:?}"
        );
    }

    /// Reviewer finding (PR #2140, issue #2091): the scope is NOT limited to
    /// issue #1839's `block`-capture receiver. `part_involves_fragment_ref`
    /// keys on `Value::FragmentRef` alone, and `brink-codegen-inkb::content::
    /// emit_slot_expr`'s `BeginFragment`…`EndFragment` composition pattern
    /// wraps *every* template slot whose expr is a function call
    /// (`lir::Expr::is_function_call()`), in ordinary display position, in
    /// both dialects — not just a `block` receiver. This pins that broader,
    /// actual scope directly: a line whose only content is a call like
    /// `{ f() }`, where `f` emits no side-effect text and returns an empty
    /// value, is suppressed by the exact same mechanism as the block-capture
    /// case above, with no `block`-capture machinery involved at all.
    #[test]
    fn resolve_lines_suppresses_a_blank_line_from_an_empty_display_position_call_composition() {
        // line 0: "Before.", line 1: `{f()}` (ordinary call composition —
        // NOT a `block`-capture receiver), line 2: "After."
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("Before."),
            one_slot_template_entry(),
            plain_entry("After."),
        ]);
        // Models `emit_slot_expr`'s composition pattern for `{ f() }`
        // where `f` produced no side-effect output and its return value
        // stringified to empty — a real, present `Fragment` with no parts,
        // exactly as a `block` capture's empty fragment looks structurally.
        let fragments = Fragments::from(vec![Fragment {
            parts: vec![],
            tags: vec![],
        }]);

        let parts = vec![
            line_ref(0, vec![], brink_format::LineFlags::from_plain("Before.")),
            OutputPart::Newline,
            line_ref(
                1,
                vec![Value::FragmentRef(0)],
                brink_format::LineFlags::empty(),
            ),
            OutputPart::Newline,
            line_ref(2, vec![], brink_format::LineFlags::from_plain("After.")),
        ];

        // Element-attachment data (issue #2108) is dropped here — these
        // pre-existing fixtures don't exercise attach conventions.
        let lines: Vec<(String, Vec<String>)> =
            resolve_lines(&parts, &program, &line_tables, None, &fragments)
                .into_iter()
                .map(|(text, tags, _element, _source)| (text, tags))
                .collect();
        assert_eq!(
            lines,
            vec![
                ("Before.".to_string(), Vec::<String>::new()),
                ("After.".to_string(), Vec::<String>::new()),
            ],
            "an empty display-position call-composition FragmentRef must be \
             suppressed identically to a block capture — this is the \
             broader scope the discriminator actually covers, not just \
             #1839's block-capture receiver: {lines:?}"
        );
    }

    /// Scope boundary: this fix is specifically about `content`/Fragment
    /// captures, not "any interpolation that happens to render empty". A
    /// `Slot` bound to a plain, non-`FragmentRef` value that resolves empty
    /// keeps its pre-existing blank beat — unchanged, matching the
    /// deliberately-preserved `inline-markup-point-marker` fixture (a
    /// self-closing markup span with no children, issue #1716).
    #[test]
    fn resolve_lines_does_not_suppress_a_blank_line_from_a_non_fragment_empty_slot() {
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("VENDOR"),
            one_slot_template_entry(),
            plain_entry("(hushed)"),
        ]);
        let fragments = Fragments::default();

        let parts = vec![
            line_ref(0, vec![], brink_format::LineFlags::from_plain("VENDOR")),
            OutputPart::Newline,
            line_ref(1, vec![Value::Null], brink_format::LineFlags::empty()),
            OutputPart::Newline,
            line_ref(2, vec![], brink_format::LineFlags::from_plain("(hushed)")),
        ];

        // Element-attachment data (issue #2108) is dropped here — these
        // pre-existing fixtures don't exercise attach conventions.
        let lines: Vec<(String, Vec<String>)> =
            resolve_lines(&parts, &program, &line_tables, None, &fragments)
                .into_iter()
                .map(|(text, tags, _element, _source)| (text, tags))
                .collect();
        assert_eq!(
            lines,
            vec![
                ("VENDOR".to_string(), Vec::<String>::new()),
                (String::new(), Vec::<String>::new()),
                ("(hushed)".to_string(), Vec::<String>::new()),
            ],
            "a non-Fragment empty slot must keep rendering its blank line: {lines:?}"
        );
    }

    /// Streaming-API regression (the actual bug shape): `take_first_line`
    /// must skip the suppressed blank line silently — never handing it back
    /// as its own `Line::Text` — while still returning "VENDOR" and
    /// "(hushed)" as two separate completed lines, in order, with the
    /// cursor correctly advanced (no stall on the suppressed segment).
    #[test]
    fn take_first_line_skips_a_suppressed_line_and_returns_the_next_real_line() {
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("VENDOR"),
            one_slot_template_entry(),
            plain_entry("(hushed)"),
        ]);

        let mut buf = OutputBuffer::new();
        buf.push_line_ref(0, 0, vec![], brink_format::LineFlags::from_plain("VENDOR"));
        buf.push_newline();
        buf.begin_fragment();
        let frag_idx = buf.end_fragment().expect("checkpoint was just pushed");
        buf.push_line_ref(
            0,
            1,
            vec![Value::FragmentRef(frag_idx)],
            brink_format::LineFlags::empty(),
        );
        buf.push_newline();
        buf.push_line_ref(
            0,
            2,
            vec![],
            brink_format::LineFlags::from_plain("(hushed)"),
        );
        buf.push_newline();

        let mut got = Vec::new();
        // Bounded loop (VM-test hygiene): at most 3 real lines are possible
        // here, so 5 iterations is generous headroom against a stall.
        for _ in 0..5 {
            match buf.take_first_line(&program, &line_tables, None) {
                Some((text, _, _, _)) => got.push(text),
                None => break,
            }
        }

        assert_eq!(
            got,
            vec!["VENDOR\n".to_string(), "(hushed)\n".to_string()],
            "the empty content capture must not surface as its own \
             (blank) streamed line: {got:?}"
        );
    }

    /// Issue #2147 (gap 1 of #2091's follow-through review): `end_capture`
    /// -> `resolve_parts` is the string-capture path — the `EndStringEval`
    /// path an unrecognized choice display or `~ temp x = "..."` string-eval
    /// rides — and PR #2140 only fixed the line-oriented
    /// `resolve_lines`/`take_first_line` path. Same VENDOR / `{body}` /
    /// (hushed) shape as `resolve_lines_suppresses_a_blank_line_from_an_
    /// empty_content_capture`, but captured as a single string via
    /// `begin_capture`/`end_capture` instead of resolved line-by-line.
    #[test]
    fn end_capture_suppresses_a_blank_line_from_an_empty_content_capture() {
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("VENDOR"),
            one_slot_template_entry(),
            plain_entry("(hushed)"),
        ]);

        let mut buf = OutputBuffer::new();
        // A real, present (empty) Fragment — same shape #1839's block
        // capture and #2140's display-position call composition produce.
        buf.begin_fragment();
        let frag_idx = buf.end_fragment().expect("checkpoint was just pushed");

        buf.begin_capture();
        buf.push_line_ref(0, 0, vec![], brink_format::LineFlags::from_plain("VENDOR"));
        buf.push_newline();
        buf.push_line_ref(
            0,
            1,
            vec![Value::FragmentRef(frag_idx)],
            brink_format::LineFlags::empty(),
        );
        buf.push_newline();
        buf.push_line_ref(
            0,
            2,
            vec![],
            brink_format::LineFlags::from_plain("(hushed)"),
        );

        let text = buf
            .end_capture(&program, &line_tables, None)
            .expect("checkpoint was just pushed");
        assert_eq!(
            text, "VENDOR\n(hushed)",
            "an empty content/Fragment capture inside a captured string \
             must not leave a stray blank line — must match resolve_lines' \
             suppression: {text:?}"
        );
    }

    /// Scope boundary, mirrored from
    /// `resolve_lines_does_not_suppress_a_blank_line_from_a_non_fragment_
    /// empty_slot`: a `Slot` bound to a plain, non-`FragmentRef` value that
    /// resolves empty keeps its pre-existing blank line inside a captured
    /// string too — this fix is about `content`/Fragment captures
    /// specifically, not "any interpolation that happens to render empty".
    #[test]
    fn end_capture_does_not_suppress_a_blank_line_from_a_non_fragment_empty_slot() {
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("VENDOR"),
            one_slot_template_entry(),
            plain_entry("(hushed)"),
        ]);

        let mut buf = OutputBuffer::new();
        buf.begin_capture();
        buf.push_line_ref(0, 0, vec![], brink_format::LineFlags::from_plain("VENDOR"));
        buf.push_newline();
        buf.push_line_ref(0, 1, vec![Value::Null], brink_format::LineFlags::empty());
        buf.push_newline();
        buf.push_line_ref(
            0,
            2,
            vec![],
            brink_format::LineFlags::from_plain("(hushed)"),
        );

        let text = buf
            .end_capture(&program, &line_tables, None)
            .expect("checkpoint was just pushed");
        assert_eq!(
            text, "VENDOR\n\n(hushed)",
            "a non-Fragment empty slot must keep its blank line inside a \
             captured string: {text:?}"
        );
    }

    /// Review finding on issue #2147's PR: `resolve_lines_annotated`
    /// deliberately suppresses its own final, unterminated entry (the
    /// `EXCEPTION (issue #2091)` block above it) — dropping the trailing
    /// newline along with it — while `resolve_parts`'s suppression only
    /// fired on an `OutputPart::Newline`. A captured string whose *last*
    /// segment (no trailing `Newline` part) is empty and Fragment-derived
    /// must drop that trailing newline too, matching `resolve_lines`.
    #[test]
    fn end_capture_drops_trailing_newline_before_an_unterminated_empty_fragment() {
        let (program, line_tables) =
            program_with_line_table(vec![plain_entry("a"), one_slot_template_entry()]);

        let mut buf = OutputBuffer::new();
        // A real, present (empty) Fragment — same shape as the other
        // tests in this module.
        buf.begin_fragment();
        let frag_idx = buf.end_fragment().expect("checkpoint was just pushed");

        buf.begin_capture();
        buf.push_line_ref(0, 0, vec![], brink_format::LineFlags::from_plain("a"));
        buf.push_newline();
        // No trailing newline after this — the capture ends mid-line, same
        // as an unread transcript tail ending on an empty Fragment
        // interpolation.
        buf.push_line_ref(
            0,
            1,
            vec![Value::FragmentRef(frag_idx)],
            brink_format::LineFlags::empty(),
        );

        let text = buf
            .end_capture(&program, &line_tables, None)
            .expect("checkpoint was just pushed");
        assert_eq!(
            text, "a",
            "an unterminated trailing empty Fragment interpolation must \
             drop its introducing newline too, matching resolve_lines' \
             final-entry suppression: {text:?}"
        );
    }

    /// Review finding on issue #2147's PR: `resolve_parts`'s new
    /// suppression is reached not only from `end_capture`'s string-capture
    /// path but also from [`OutputBuffer::resolve_fragment`] — including
    /// when resolving a *nested* fragment's own interior, when that inner
    /// fragment's captured region spans more than one line and one of
    /// those interior lines is contributed purely by a further-nested,
    /// rendered-empty fragment. Pin that this interior suppression fires
    /// identically to the top-level `resolve_lines`/`end_capture` case —
    /// this is the "nested/multi-line fragment interior" effect the
    /// doc comment on `resolve_parts` discloses.
    #[test]
    fn resolve_fragment_suppresses_a_blank_line_from_a_nested_empty_fragment_interior() {
        let (program, line_tables) = program_with_line_table(vec![
            plain_entry("VENDOR"),
            one_slot_template_entry(),
            plain_entry("(hushed)"),
        ]);

        let mut buf = OutputBuffer::new();

        // The inner, empty Fragment (e.g. a block-capture receiver that
        // captured nothing).
        buf.begin_fragment();
        let inner_idx = buf.end_fragment().expect("checkpoint was just pushed");

        // The outer Fragment: three lines, with the middle one contributed
        // purely by the (empty) inner Fragment — i.e. a multi-line
        // fragment whose own interior has a suppressible blank line.
        buf.begin_fragment();
        buf.push_line_ref(0, 0, vec![], brink_format::LineFlags::from_plain("VENDOR"));
        buf.push_newline();
        buf.push_line_ref(
            0,
            1,
            vec![Value::FragmentRef(inner_idx)],
            brink_format::LineFlags::empty(),
        );
        buf.push_newline();
        buf.push_line_ref(
            0,
            2,
            vec![],
            brink_format::LineFlags::from_plain("(hushed)"),
        );
        let outer_idx = buf.end_fragment().expect("checkpoint was just pushed");

        let text = buf.resolve_fragment(outer_idx, &program, &line_tables, None);
        assert_eq!(
            text, "VENDOR\n(hushed)",
            "a multi-line fragment's own interior must suppress a blank \
             line from a nested, rendered-empty fragment the same way the \
             top-level resolve_lines/end_capture paths do: {text:?}"
        );
    }

    /// Review finding on issue #2108's PR: unlike [`OutputBuffer::
    /// take_first_line`], [`OutputBuffer::flush_lines`] seeded
    /// `pending_element` from `self.pending_element` but never wrote the
    /// end-of-slice state back — so an `ElementAttachEnd` consumed by a
    /// `flush_lines` call was lost and the attach data stayed live forever
    /// on whatever the buffer resolved next. Drain an attach run's first
    /// line through `take_first_line` (the call that seeds
    /// `pending_element` in the first place) and its remainder — including
    /// the closing `ElementAttachEnd` — through `flush_lines` in one shot,
    /// then prove a line pushed afterward does NOT inherit the closed
    /// run's data.
    #[test]
    fn flush_lines_writes_back_pending_element_past_the_closed_run() {
        let p = test_dummy_program();
        let mut buf = OutputBuffer::new();

        buf.push_element_attach("speaker".to_string(), "VENDOR".to_string());
        buf.push_text("Line one.");
        buf.push_newline();
        buf.push_text("Line two.");
        buf.push_newline();
        buf.push_element_attach_end();

        let (first_text, _, first_element, _) = buf
            .take_first_line(&p, &[], None)
            .expect("first line of the attach run");
        assert_eq!(first_text, "Line one.\n");
        assert_eq!(
            first_element.get("speaker").map(String::as_str),
            Some("VENDOR")
        );

        let rest = buf.flush_lines(&p, &[], None);
        let line_two = rest
            .iter()
            .find(|(text, ..)| text == "Line two.")
            .expect("Line two. present in the flush");
        assert_eq!(
            line_two.2.get("speaker").map(String::as_str),
            Some("VENDOR"),
            "the last line of the run itself must still carry the attach data: {rest:?}"
        );

        // Pushed after the run closed — must not inherit "speaker": "VENDOR".
        buf.push_text("Unattached.");
        buf.push_newline();
        let (after_text, _, after_element, _) = buf
            .take_first_line(&p, &[], None)
            .expect("line after the closed run");
        assert_eq!(after_text, "Unattached.\n");
        assert!(
            after_element.is_empty(),
            "flush_lines must write pending_element back to empty once it \
             consumes the run-closing ElementAttachEnd: {after_element:?}"
        );
    }

    /// Review finding on issue #2108's PR: [`OutputBuffer::reset_cursor`]
    /// rewound `self.cursor` but left `pending_element` populated. At index
    /// 0 no attach run has accumulated yet, so a locale hot-swap re-render
    /// (the public use of `reset_cursor`) leaked the previous drain pass's
    /// element data onto the re-drained leading line.
    ///
    /// Transcript: `[narration, NL, ElementAttach(speaker=VENDOR), dialogue,
    /// NL]` — the exact probe from the finding. The narration line reports
    /// `{}` on the first pass (the attach hasn't happened yet); after
    /// draining the whole buffer once and calling `reset_cursor`, the
    /// re-drained narration line must report `{}` again too, not the
    /// dialogue run's `speaker` leaking backward from the previous pass.
    #[test]
    fn reset_cursor_clears_pending_element() {
        let p = test_dummy_program();
        let mut buf = OutputBuffer::new();

        buf.push_text("Intro.");
        buf.push_newline();
        buf.push_element_attach("speaker".to_string(), "VENDOR".to_string());
        buf.push_text("Dialogue.");
        buf.push_newline();

        let (first_text, _, first_element, _) =
            buf.take_first_line(&p, &[], None).expect("narration line");
        assert_eq!(first_text, "Intro.\n");
        assert!(first_element.is_empty(), "{first_element:?}");

        let (second_text, _, second_element, _) =
            buf.take_first_line(&p, &[], None).expect("dialogue line");
        assert_eq!(second_text, "Dialogue.\n");
        assert_eq!(
            second_element.get("speaker").map(String::as_str),
            Some("VENDOR")
        );

        buf.reset_cursor();
        let (text_after_reset, _, element_after_reset, _) = buf
            .take_first_line(&p, &[], None)
            .expect("re-drained narration line after reset_cursor");
        assert_eq!(text_after_reset, "Intro.\n");
        assert!(
            element_after_reset.is_empty(),
            "reset_cursor must clear pending_element — no attach run has \
             accumulated yet at index 0, so the re-drained leading line \
             must not inherit the previous pass's speaker: \
             {element_after_reset:?}"
        );
    }

    /// Issue #3556: `trim_function_end` must not walk behind the read
    /// cursor.
    ///
    /// A function whose body spans a yield point — it printed a line, the
    /// consumer took it, and only then did the function return — has a
    /// `start` recorded before parts that have since been delivered. C# has
    /// no such case because its output stream really is emptied at each
    /// yield (`ResetOutput`); brink keeps the whole transcript with a cursor
    /// over it, so the cursor is where that reset happened.
    ///
    /// Without the floor the transcript ends up shorter than the cursor and
    /// the next reader of `transcript[cursor..]` panics — which is how this
    /// surfaced, out of `brink-gen`'s `both_roads_agree`.
    #[test]
    fn trim_function_end_stops_at_the_read_cursor() {
        let mut buf = OutputBuffer::new();
        // The function's output, all of it after `start = 0`.
        let start = buf.target_len();
        buf.push_text("1");
        buf.push_newline();
        // An empty list renders as whitespace, so #3536 makes it trimmable
        // — and it is content, so it commits the newline behind it.
        buf.push_value_ref(Value::List(alloc::sync::Arc::new(
            brink_format::ListValue {
                items: Vec::new(),
                origins: Vec::new(),
            },
        )));
        buf.push_newline();

        // The consumer takes the completed line; the cursor advances past
        // the two parts that produced it.
        assert_eq!(
            buf.test_take_first_line().map(|(t, _)| t),
            Some("1\n".to_owned())
        );
        let cursor = buf.cursor;
        assert_eq!(cursor, 2, "the delivered line is the first two parts");

        buf.trim_function_end(start);

        assert!(
            buf.transcript.len() >= cursor,
            "the trim walked behind the cursor: transcript is {} parts, \
             cursor is at {cursor}",
            buf.transcript.len()
        );
        // What it *should* have trimmed: everything the consumer has not
        // seen, since all of it renders as whitespace.
        assert_eq!(buf.transcript.len(), cursor, "the unread tail is trimmed");
        // And the invariant every reader depends on now holds.
        assert!(buf.test_take_first_line().is_none());
    }
}
