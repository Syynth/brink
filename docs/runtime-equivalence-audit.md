# Runtime Equivalence Audit: C# ink vs brink

## Scope

Line-by-line comparison of the C# ink runtime's output pipeline against brink's,
identifying behavioral differences and classifying each as:
- **Bug** — brink diverges and should be fixed
- **Acceptable** — intentional architectural difference, oracle harness should tolerate
- **Investigate** — unclear impact, needs testing

---

## 1. Output stream lifecycle

### C# model
```
Continue() {
    ResetOutput()           // clear outputStream
    do {
        ContinueSingleStep()  // one opcode
        // snapshot/lookahead logic for newline safety
    } while (canContinue && !outputStreamEndsInNewline)
    CleanOutputWhitespace()
    return currentText      // join all StringValues, clean whitespace
}
```

### Brink model
```
continue_single() {
    // 1. If buffer has completed line (newline + content after), drain it
    // 2. If buffer has partial content + VM yielded, flush remaining
    // 3. Step VM loop:
    //    - after each step, check has_completed_line → take_first_line
    //    - on Done/Ended, flush_remaining → make_yield_line
}
```

### Key difference: reset vs cursor
C# clears the output stream per `Continue()`. Brink uses an append-only transcript
with a cursor that advances on `take_first_line`/`flush_lines`. The cursor advance
is functionally equivalent to `ResetOutput()` for content-detection purposes.

**Classification: Acceptable.** Brink's cursor model enables locale hot-swap
(re-render without re-execution). The behavioral equivalence was validated by
the Spring content-detection fix — `unread_has_content_or_spring()` scopes
content checks to the current "turn" by scanning from cursor.

---

## 2. Push filtering — `PushToOutputStreamIndividual` vs brink push methods

### 2a. Glue handling

**C#:** When Glue is pushed, `TrimNewlinesFromOutputStream()` walks backward
from the end and removes all trailing newlines + whitespace after the last
non-whitespace content.

**Brink:** Glue is pushed as `OutputPart::Glue`. Resolution is deferred —
`mark_glue_removals()` runs at read time during `has_completed_line`,
`take_first_line`, `flush_lines`, and `resolve_parts`.

**Classification: Acceptable.** Brink resolves glue lazily at read time rather
than eagerly at push time. This is architecturally intentional (deferred
resolution). The end result should be identical. If edge cases arise, they're
bugs in the glue resolution logic, not in the approach.

### 2b. Function output trimming

**C#:** `PushToOutputStreamIndividual` has a complex block (lines 940–1010)
that handles function start/end trimming:
- Tracks `functionStartInOutputStream` — the index where the current function
  call started pushing output
- When new text arrives while inside a function call, and there's a
  `glueTrimIndex` or `functionTrimIndex`:
  - **Newlines are suppressed** (thrown away between function boundary and
    new non-whitespace text)
  - **Non-whitespace text** clears the function trim state and removes glue

**Brink:** Function output trimming is handled by `trim_and_collapse_fragment()`,
which is called from `EndFragment` opcode handling. It walks backward from the
end of the fragment capture and removes trailing `Newline`, `Spring`, and
whitespace-only content.

**Classification: Investigate.** The mechanisms are quite different. C# tracks
trim points as indices into the output stream and makes trim decisions per-push.
Brink captures function output in a separate `fragment_capture` vec and trims
on collapse. Edge cases around functions that contain glue, or nested function
calls, could behave differently.

### 2c. Newline suppression

**C#** (lines 1013–1017):
```csharp
if (text.isNewline) {
    if (outputStreamEndsInNewline || !outputStreamContainsContent)
        includeInOutput = false;
}
```

Two rules:
1. **Duplicate suppression:** if stream already ends in newline → suppress
2. **Leading suppression:** if stream has no content (any StringValue) → suppress

**Brink** (`push_newline`):
```rust
let has_content = if self.capture_depth > 0 {
    self.has_content()          // scope-local scan
} else {
    self.unread_has_content_or_spring()  // transcript from cursor
};
if !has_content || self.ends_in_newline() {
    return;
}
```

Two rules (matching C#):
1. **Duplicate suppression:** `ends_in_newline()` — checks last part of transcript
2. **Leading suppression:** `unread_has_content_or_spring()` — checks unread
   transcript for content or Spring

**Classification: Mostly equivalent after the Spring fix.** One remaining
subtlety: C#'s `outputStreamContainsContent` counts ANY `StringValue`, including
newlines themselves. Brink's `unread_has_content_or_spring` checks for
`is_content()` (non-whitespace Text/LineRef/ValueRef) or Spring. A buffer
containing only `[Newline]` would have `outputStreamContainsContent = true` in
C# but `unread_has_content_or_spring = false` in brink. However, this shouldn't
matter in practice because `ends_in_newline()` would also be true, triggering
the duplicate suppression path either way.

### 2d. String splitting — `TrySplittingHeadTailWhitespace`

**C#:** Before pushing a StringValue, `PushToOutputStream` calls
`TrySplittingHeadTailWhitespace`, which splits strings like
`"  \n  text  \n  "` into separate StringValues for leading newlines,
inner text, and trailing newlines. This ensures newlines are individual
items in the output stream for proper filtering.

**Brink:** Not applicable. Brink doesn't push raw strings to the output buffer
in production. The VM emits `EmitNewline` and `EmitLine` as separate opcodes,
so newlines and text are already structurally separated. The
`TrySplittingHeadTailWhitespace` splitting exists in C# because inklecate's
bytecode can contain strings with embedded newlines (e.g., `"hello\n"`), which
brink's compiler has already separated.

**Classification: Acceptable.** Different bytecode format makes this unnecessary
in brink. If brink's compiler ever emits a `LineRef` containing embedded
newlines, it would be a compiler bug — the compiler should emit separate
`EmitNewline` + `EmitLine` opcodes.

---

## 3. Yield/break logic — when does a `Continue()` return?

### C# model

`Continue()` breaks when:
1. `outputStreamEndsInNewline` — scans backward from end of stream, skipping
   whitespace-only StringValues and stopping at ControlCommands. Returns true if
   the last non-whitespace-only StringValue is a newline.
2. `!canContinue` — no more content to execute.

Additionally, C# has a **lookahead/snapshot mechanism**:
- When `outputStreamEndsInNewline` is true but `canContinue` is also true, C#
  takes a state snapshot and continues stepping.
- If the next content is non-whitespace text or a tag → the newline was real,
  restore snapshot and return (`ExtendedBeyondNewline`)
- If glue removes the newline → discard snapshot, keep going (`NewlineRemoved`)
- This handles: `"Hello\n<>world"` — the newline is tentatively committed, then
  removed when glue appears.

### Brink model

`continue_single` breaks when:
1. `has_completed_line()` — a surviving Newline (after glue resolution) has
   non-whitespace content after it. This is conceptually similar to C#'s
   `outputStreamEndsInNewline` + lookahead, but implemented as a single scan
   rather than snapshot/restore.
2. VM yields `Done`/`Ended` — flush remaining buffer.

### Key difference: newline commitment strategy

**C#:** Tentatively breaks on trailing newline, then uses snapshot/restore to
handle the case where glue removes it.

**Brink:** Doesn't break until content appears AFTER the newline, proving it
survived glue. No snapshot needed.

**Classification: Acceptable (with one caveat).** Brink's approach is cleaner
and avoids the snapshot/restore complexity. It produces equivalent results in
most cases because a newline only "matters" when there's content after it.

**Caveat — trailing newline at yield points:** When the VM yields (Done/Choices/
Ended), brink calls `flush_remaining` which resolves everything in the buffer.
A trailing newline at a yield point IS committed (no future glue can reach it).
C#'s `Continue()` loop also breaks on `!canContinue`, so trailing newlines at
yield points are committed in both. This should be equivalent.

**Caveat — `outputStreamEndsInNewline` vs `has_completed_line`:**
C#'s check is "does the stream END in a newline (ignoring trailing whitespace)?"
Brink's check is "is there a newline with non-whitespace content AFTER it?"

These are NOT the same for a buffer like `[Text("hello"), Newline]`:
- C#: `outputStreamEndsInNewline` = true → break, return "hello\n"
- Brink: `has_completed_line` = false (no content after Newline) → keep stepping

In brink, this buffer would only yield when:
- More content is pushed after the Newline → `has_completed_line` becomes true
- The VM yields (Done/Ended) → `flush_remaining` commits it

This means brink may **batch more content into a single step** than C# when
a newline is the last thing before a yield point. In practice, the yield point
flush handles this — but the step boundary could differ.

This is the "step alignment problem" from the handoff doc. It's acceptable IF
the oracle comparison tolerates it.

---

## 4. Post-processing — `CleanOutputWhitespace` vs brink resolution

### C# `CleanOutputWhitespace`
Applied to the final concatenated string from `currentText`:
1. Remove all inline whitespace (spaces/tabs) from the start of each line
2. Remove all inline whitespace from just before `\n`
3. Collapse consecutive spaces/tabs into a single space

### Brink equivalent
Brink handles this at multiple points:
- `resolve_parts` / `resolve_lines`: trims trailing whitespace before newlines
  (`out.trim_end_matches([' ', '\t'])` before pushing `\n`)
- `resolve_parts`: collapses adjacent whitespace at part boundaries
- Spring resolution: emits `" "` only when output is non-empty, doesn't end in
  space, and doesn't end in newline

### Classification: Investigate.
The whitespace collapsing and trimming rules are spread across multiple
functions in brink vs a single post-pass in C#. Edge cases around:
- Leading whitespace on the first line of a turn
- Whitespace between Spring-resolved spaces and text content
- Multiple consecutive Springs

These could produce subtly different whitespace. The `caf173c3` commit
("only trim horizontal whitespace before newlines in resolve_parts") suggests
this area has already been a source of bugs.

---

## 5. `outputStreamEndsInNewline` details

**C#** (lines 1082–1101): Walks backward through the output stream. Skips
non-text objects. For text objects: if it's a newline → true; if it's
non-whitespace → false (break). Whitespace-only text is skipped over.
Also stops at ControlCommands (e.g., `BeginString`).

**Brink `ends_in_newline`**: Checks if the LAST part in the transcript is
a Newline. Does NOT skip trailing whitespace or Spring parts.

**Classification: Investigate.** If the buffer is `[Text("hello"), Newline,
Spring]`, C# would return true (Spring equivalent is skipped, Newline is found).
Brink's `ends_in_newline` returns false (last part is Spring, not Newline).
This could cause duplicate newlines in brink where C# suppresses them.
However, `has_completed_line` might mask this since it uses the full glue
resolution scan.

---

## 6. Oracle harness configuration parity

Settings that affect C# runtime behavior and must match brink:

| Setting | C# default | Oracle harness | Brink behavior | Status |
|---------|-----------|---------------|----------------|--------|
| `allowExternalFunctionFallbacks` | false | **now true** | always uses fallbacks | Fixed |
| Error handling mode | throws | caught in explorer | returns RuntimeError | OK |
| `onError` callback | null | not set | N/A | OK |

Other C# Story properties that could affect output:
- `onChoosePathString` — not used
- `onMakeChoice` — not used
- `onEvaluateFunction` / `onCompleteEvaluateFunction` — not used

**Classification: OK after fallback fix.** No other harness config gaps known.

---

## Summary: action items

### Fix (bugs)
None identified beyond what's already been fixed.

### Investigate
1. **Function output trimming** (§2b) — Different mechanism (per-push trim index
   vs fragment capture/collapse). Test with cases that have functions containing
   glue or nested function calls.
2. **Whitespace post-processing** (§4) — Leading whitespace on first line,
   consecutive Springs, Spring+text boundary collapsing. Compare `CleanOutputWhitespace`
   output against brink's `resolve_parts` for edge cases.
3. **`ends_in_newline` precision** (§5) — Brink checks last part only; C# skips
   trailing whitespace. Could cause duplicate newlines in edge cases.

### Acceptable (oracle should tolerate)
1. **Cursor vs ResetOutput** (§1) — Architectural. Already handled by
   `unread_has_content_or_spring`.
2. **Deferred glue resolution** (§2a) — Architectural. Equivalent results.
3. **No string splitting needed** (§2d) — Compiler separates newlines/text.
4. **Newline commitment strategy** (§3) — `has_completed_line` vs
   snapshot/restore. May produce different step boundaries when newline is
   last-before-yield. Oracle comparison should tolerate step-boundary differences
   where the concatenated text and terminal outcome match.

### Harness improvements
The oracle comparison currently diffs step-by-step. For the "acceptable" step
boundary differences, the harness should support a **relaxed comparison mode**
that:
1. Concatenates all step texts within a turn (between choices/done/end)
2. Compares the concatenated text + terminal outcome
3. Reports step-count differences as warnings, not failures

This would separate "content correctness" (are we producing the right text?)
from "step alignment" (are we breaking at the same points?), letting us focus
on real bugs.
