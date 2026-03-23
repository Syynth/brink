# Runtime Restructuring Spec

## Motivation

The runtime was built pre-compiler to support the converter pipeline (ink.json → StoryData → execute). Its internal architecture reflects that origin: bulk text output, eager string resolution, observer instrumentation threaded through the production path, and a monolithic `StoryState` trait that bundles unrelated concerns. The public API (`StepResult`, `can_continue`, `status`) leaked internal VM concepts to consumers.

The recent `Line` enum refactor cleaned up the public API but exposed deeper structural issues: borrow conflicts from ownership layout, fragile tag reconstruction from bulk text, and observer plumbing that infects every call site. This spec describes the target architecture.

## Goals

1. **Clean consumer API** — `continue_single() -> Line` is the primary interface. The `Line` enum (`Text`, `Done`, `Choices`, `End`) tells the consumer exactly what to do next.
2. **Deferred line resolution** — the VM writes structural references into the output buffer, not resolved strings. Resolution happens at read time against the current locale. This enables locale hot-swap without re-execution.
3. **Append-only output buffer** — the buffer is a transcript of what the VM produced. Consumers read via a cursor. Transcripts can be stored and re-rendered against any locale.
4. **Observer extraction** — the test harness instruments execution by wrapping `Context`, not by threading observer parameters through the production path.
5. **Immutable Program** — `Program` is truly immutable after linking. Locale-specific content (line tables) is a separate, swappable object.

## Architecture

### Layers (bottom up)

**`vm::step`** (or method on `Program`)
- Pure opcode dispatch.
- Takes `&self` (Program), `&mut Flow`, `&mut Context`, `&mut Stats`.
- No trait. No resolver. No observer awareness.
- `EmitLine` writes `OutputPart::LineRef` into the output buffer.
- `EmitValue` writes `OutputPart::ValueRef` into the output buffer.
- `EvalLine` (string eval for inline expressions) resolves eagerly — result goes on the value stack, not the output buffer.
- Capture mechanism (`begin_capture` / `end_capture`) resolves eagerly for tags and function return values (internal, not locale-sensitive).

**`Context`**
- Concrete struct, not behind a trait.
- Owns: globals, visit counts, turn counts, turn index, RNG state.
- Methods directly on the struct (the current `StoryState` getters/setters minus program/resolver).
- RNG type parameter lives here: `Context<R: StoryRng>`.
- One `Context` per flow — flows have independent state.

**`ObservedContext`**
- Wrapper around `&mut Context` that delegates all mutations and fires `WriteObserver` callbacks.
- Same decorator pattern as today's `ObservedState`, just on `Context` directly.
- Only constructed by the test harness — never appears in the production path.
- Alternative: `Context` has an `Option<&mut dyn WriteObserver>` field. Zero-cost when `None`. Simpler than a wrapper type but means production `Context` carries the field.

**`FlowInstance`**
- Owns: `Flow`, `StoryStatus`, `Stats`.
- Does NOT own `Context` — Context is a sibling at the `Story` level.
- `step_single_line(&mut self, context: &mut Context, program: &Program, handler, resolver) -> Result<Line, RuntimeError>` — orchestration logic.
- Orchestration: checks output buffer for completed lines, handles invisible default choices, sets status on Done/Ended, resolves external calls.
- No observer awareness — the caller picks observed vs plain context.

**`Story`** (public API)
- Owns: `&Program`, active line tables (`Vec<Vec<LineEntry>>`), resolver, and `(FlowInstance, Context)` pairs (default + named flows).
- Public methods: `continue_single()`, `continue_maximally()`, `choose()`, `set_line_tables()`.
- Creates context and passes it down. The `Line` values returned to the consumer have resolved text — resolution happens inside `Story` using the active line tables and resolver.
- `into_snapshot` / `from_snapshot` may be simplified or removed since `Program` is no longer mutated.

**Test harness**
- Constructs `ObservedContext` wrapping a real `Context`.
- Calls `FlowInstance::step_single_line` directly with the observed context.
- No special `Story` method needed — `continue_maximally_observed` goes away.

### Output buffer

```rust
enum OutputPart {
    LineRef { container_idx: u32, line_idx: u16, slots: Vec<Value> },
    ValueRef(Value),
    Newline,
    Glue,
    Checkpoint,
    Tag(String),
}
```

- **Append-only log** with a read cursor. The VM only appends. `take_first_line` advances the cursor, doesn't drain.
- **Resolution at read time**: `take_first_line` and `flush_lines` take `&Program`, `&[Vec<LineEntry>]` (active line tables), and `Option<&dyn PluralResolver>` to resolve `LineRef` and `ValueRef` into strings.
- **Capture mechanism** uses separate scratch space for transient captures (tags, function return values). Captures resolve eagerly since they're internal metadata.
- **Transcripts**: the raw log can be read without consuming (`story.transcript()`). Re-render against any locale by resolving the log with different line tables.

### Program and line tables

The linker produces two separate objects:

```rust
fn link(data: &StoryData) -> Result<(Program, Vec<Vec<LineEntry>>), RuntimeError>
```

**`Program`** — truly immutable after linking:
- Containers, bytecode, address map, name table.
- Globals metadata, global map.
- List metadata (literals, item map, definitions).
- External function metadata.
- Scope IDs (structural mapping from scope → line table index).
- Source checksum.

**`Vec<Vec<LineEntry>>`** — the base locale's line content. Swappable:
- One inner `Vec<LineEntry>` per scope.
- Parallel to `Program.scope_ids` (same indexing).

Locale loading becomes a pure function:

```rust
fn load_locale(
    program: &Program,
    locale: &LocaleData,
    base: &[Vec<LineEntry>],
    mode: LocaleMode,
) -> Result<Vec<Vec<LineEntry>>, RuntimeError>
```

No mutation of `Program`. Locale swap on `Story` is `story.set_line_tables(new_tables)`.

### `StoryState` trait — deleted

The trait existed to bundle `&Program` + `&mut Context` into one parameter for the VM. With the VM taking them separately, the trait has no purpose. The observer decorator moves from trait impl (`ObservedState`) to concrete wrapper (`ObservedContext`).

### Whitespace model: Springs

Ink's whitespace handling is complex and was previously spread across push-time filtering, glue resolution, and a `CleanOutputWhitespace` cleanup pass. The restructuring introduces **Springs** — a structural word-break marker — to simplify and formalize whitespace semantics.

#### The problem

The old runtime baked whitespace into string content: `"I have "` + `"5"` + `" apples."`. This required push-time filtering to collapse adjacent whitespace at part boundaries, suppress leading whitespace, and trim trailing whitespace — all of which depended on knowing the resolved text at push time. With deferred resolution (`LineRef`), the text isn't available at push time, breaking these heuristics.

#### The solution

Three structural output markers, no whitespace in content strings:

- **`Newline`** — line break. Already exists.
- **`Glue`** — cancel preceding line break. Already exists.
- **`Spring`** — word break. New.

Content parts (`LineRef`, `ValueRef`) carry no leading or trailing whitespace. The compiler strips boundary whitespace from all line content and emits `Spring` opcodes where word breaks belong.

#### Compiler responsibility

The compiler (HIR lowering / codegen) introduces Springs:

1. Template recognition runs first — packs adjacent text + simple expressions into single `EmitLine` ops with `Template` line entries. Whitespace inside templates is preserved (the template resolver handles empty-slot collapsing).
2. Springs are emitted between separate emissions that can't be packed into a single template (function calls, tunnels, complex conditionals).
3. `EmitNewline` closes the source line.

The converter pipeline makes the same change: strip boundary whitespace from `EmitLine` content, insert `Spring` between consecutive emissions.

#### Push-time rules

- **`push_spring`**: Don't push if the buffer already ends in `Spring`. (Dedup.)
- **`push_newline`**: Don't push if no content yet, or if the buffer already ends in `Newline`. (Existing behavior, unchanged.)
- **`push_line_ref` / `push_value_ref`**: No whitespace filtering needed. Content is clean. Null values are dropped.

#### Resolve-time rule

When resolving a `Spring` part, emit `" "` unless the output string is empty, already ends in `' '`, or already ends in `'\n'`.

This single rule handles:
- Leading Spring (output empty → skip)
- Double Spring after glue removes a Newline (output ends in space → skip)
- Spring before Newline (Spring emits space, then Newline trims trailing whitespace — existing behavior in `resolve_parts`)
- Spring after Newline (output ends in `'\n'` → skip)
- Normal Spring between content (emits one space ✓)

#### Template resolution

Inside a single template, whitespace is part of the literal strings — NOT Springs. The template resolver collapses double spaces from empty slots: when concatenating `Literal + Slot(empty) + Literal`, if the join produces adjacent spaces, collapse to one. This is locale-safe because the translator controls where spaces appear in their template.

#### `CleanOutputWhitespace` — eliminated

With Springs, the output buffer never produces:
- Leading/trailing whitespace (compiler stripped it, Springs handle word breaks)
- Double spaces (Spring dedup at push time, single resolve-time rule)
- Whitespace runs in content (compiler normalizes, template resolver handles empty slots)

`CleanOutputWhitespace` becomes unnecessary. It may be retained temporarily as a safety net during migration.

#### Format changes

- **`brink-format`**: New `Opcode::Spring`. `LineFlags` bitflags on `LineEntry` (already added: `STARTS_WITH_WS`, `ENDS_WITH_WS`, `ALL_WS`, `EMPTY`) may be simplified or removed once all content is guaranteed clean.
- **`brink-format`**: `OutputPart::Spring` added to the output buffer.
- **Compiler** (`brink-codegen-inkb`): Codegen emits `Spring` between non-templateable emissions. Strips boundary whitespace from line content.
- **Converter** (`brink-converter`): Same change — strip boundary whitespace, insert Springs between consecutive emissions.

### Value stringification

`ValueRef(Value)` in the output buffer defers stringification to read time. Ink's reference implementation does not localize value stringification (numbers, list item names). We follow the same approach — `ValueRef` uses non-localized formatting. This is an extension point for future locale-aware number formatting if needed. List item display names raise questions about substring queries operating against source-language forms; this needs careful design if localized in the future.

## Acceptance test

Swap locales and re-render the full transcript in the new locale without re-executing the story:

```rust
// Run story, accumulating transcript
loop {
    match story.continue_single()? {
        Line::Text { text, .. } => print!("{text}"),
        Line::Choices { choices, .. } => { story.choose(0)?; }
        Line::Done { .. } => break,
        Line::End { .. } => break,
    }
}

// Swap locale and re-render entire history
let es_tables = load_locale(&program, &es_data, &base_tables, LocaleMode::Overlay)?;
story.set_line_tables(es_tables);
for part in story.transcript() {
    print!("{}", part.resolve(&program, &story.line_tables(), story.resolver()));
}
```

Same execution, different language. If this works, the restructuring is sound.

**Current status**: Basic transcript re-rendering works for main output (LineRef parts resolve at read time). Transcript serialization (`.brkt`), CLI replay (`brink replay`), and TUI re-render on locale switch are implemented. However, slot values that cross a value boundary (function returns, inline expressions) are eagerly resolved to strings and lose structural data. The fragment model (below) addresses this.

### Fragment model for locale-safe slot values

**Problem**: When text crosses a value boundary — function return, inline expression like `{func()}` — the VM resolves structural `LineRef` parts to a plain string via `end_capture`. The resolved `Value::String` goes into a template slot. On locale switch, the template text re-renders but the slot value doesn't. Example: `Result: {greeting(x)}` where `greeting` returns localized text — the slot bakes in `"Dear hello"` and locale switch can't change it.

**Solution**: A fragment store in the output buffer, referenced by a new `Value::FragmentRef(u32)` variant, populated by new `BeginFragment`/`EndFragment` opcodes.

#### Data model

```rust
struct OutputBuffer {
    transcript: Vec<OutputPart>,       // main output stream
    cursor: usize,
    capture: Vec<OutputPart>,          // transient (string eval, tags)
    capture_depth: usize,
    fragments: Vec<Vec<OutputPart>>,   // structural pieces for slot values
}
```

`Value` gains a variant:
```rust
Value::FragmentRef(u32)  // index into OutputBuffer.fragments
```

#### Opcodes

- `BeginFragment` — start capturing output to a new fragment (not the transcript, not the capture vec)
- `EndFragment` — finalize the fragment, push `Value::FragmentRef(fragment_index)` onto the value stack

#### Codegen rules

The compiler decides at emit time whether an expression needs structural preservation:

- **Fragment** (display context): function calls in output content (`{func()}`), choice display text. Codegen emits `BeginFragment`/`EndFragment` around these.
- **String eval** (computation context): expressions in conditionals, assignments, comparisons. Codegen emits `BeginStringEval`/`EndStringEval` as today.
- **Tags**: continue using `BeginTag`/`EndTag` (tags don't need localization).

The distinction is: will this text be displayed to the user, or consumed by the VM for computation? Display → fragment. Computation → string eval.

#### Resolution

When `resolve_line_ref` encounters a `Value::FragmentRef(idx)` in a template slot:
1. Look up `fragments[idx]` in the output buffer
2. Resolve the fragment's parts against current line tables (via `resolve_parts`)
3. Use the resulting string as the slot value

This is recursive: a fragment can contain `LineRef` parts whose templates have slots with `FragmentRef` values. Resolution depth is bounded by function call nesting depth.

#### Interaction with function capture model

Fragments interact cleanly with the function capture model:
- Inside a `BeginFragment`/`EndFragment`, function output flows to the fragment's capture space as structural `LineRef` parts.
- The function does NOT need `begin_capture` (the C# model) — its output goes to whatever the current target is (fragment or transcript).
- The fragment capture replaces function-level capture for display contexts.

This means the function capture model redesign (removing `begin_capture` from `Call`) and the fragment model are complementary. Fragments provide the "where does the output go?" mechanism; removing function capture provides the "don't resolve at the function boundary" mechanism.

#### Choice display text

Choice display text is currently built via `EndStringEval` → resolved to string → stored in `PendingChoice.display_text`. With fragments:
1. Codegen wraps choice text composition in `BeginFragment`/`EndFragment`
2. `PendingChoice` stores `FragmentRef(u32)` instead of `String`
3. `Choice` (public API) still exposes `text: String` — resolved on demand from the fragment
4. After locale switch, `story.pending_choices()` re-resolves against new line tables
5. On `story.choose(idx)`, the selected choice's fragment parts are pushed to the transcript for replay

#### Serialization

`Value::FragmentRef(u32)` serializes as a tag byte + `u32` index. Fragment contents serialize as part of the `.brkt` transcript format (new section after the main parts). On deserialization, fragment indices are preserved.

#### Staging

9. **TODO: Fragment model** — implementation order:
   - a. Add `Value::FragmentRef(u32)` to `brink-format`'s `Value` enum (encoding, decoding, display).
   - b. Add `BeginFragment`/`EndFragment` opcodes to `brink-format`.
   - c. Add `fragments: Vec<Vec<OutputPart>>` to `OutputBuffer`. VM handles `BeginFragment` (start fragment capture) and `EndFragment` (finalize, push `FragmentRef`).
   - d. Update `resolve_line_ref` / `value_ops::stringify` to resolve `FragmentRef` slots.
   - e. Codegen: emit `BeginFragment`/`EndFragment` around function calls in display context (inline expressions in output content).
   - f. Codegen: emit `BeginFragment`/`EndFragment` for choice display text composition.
   - g. `PendingChoice` stores `FragmentRef` instead of `String`. `Choice` resolves on demand.
   - h. Update `.brkt` transcript format to include fragment store.
   - i. Verify episode corpus + locale re-rendering.

## Staging

Each step is independently testable against the episode corpus:

1. ~~**Delete `StoryState` trait**~~ — ✅ Done. VM takes `&Program` + `&mut impl ContextAccess`.
2. ~~**Pull `Context` out of `FlowInstance`**~~ — ✅ Done. `Story` owns `(FlowInstance, Context)` pairs.
3. ~~**Remove observer from production path**~~ — ✅ Done. `ObservedContext` wraps `Context`, no observer param in `step_single_line`.
4. ~~**Split `Program` and line tables**~~ — ✅ Done. `link()` returns `(Program, Vec<Vec<LineEntry>>)`. `Program` is truly immutable.
5. ~~**Defer line/value resolution**~~ — ✅ Done. `LineRef`/`ValueRef` in output buffer, resolution at read time. `LineFlags` on `LineEntry` for push-time filtering decisions.
6. ~~**Add `Spring` opcode and output part**~~ — ✅ Done.
   - a. ✅ `Opcode::Spring` added to `brink-format` (encoding, decoding, inkt read/write).
   - b. ✅ `OutputPart::Spring` added with push-time dedup and resolve-time rule.
   - c. ✅ Converter codegen strips boundary whitespace and emits `Spring`.
   - d. ✅ Compiler codegen (`brink-codegen-inkb`) — `emit_content_parts` strips boundary whitespace from `Text` parts and emits `Spring`.
   - e. ✅ `CleanOutputWhitespace` removed. Replaced by: parser `skip_ws()` for indentation in multiline bodies, `resolve_lines` `trim()` for output-line boundary whitespace, template empty-slot collapsing for leading empty slots, and `compose_hir_content` boundary-whitespace collapsing for choice text composition.
   - f. ✅ Template empty-slot whitespace collapsing in `resolve_line_ref`. When a slot/select resolves to empty and the join produces adjacent spaces or leading whitespace, collapses it.
   - g. ✅ Episode corpus verified: 847/950 — same as pre-restructuring baseline. The 103 mismatches are from 4 pre-existing cases (function capture model, see investigation notes below), NOT regressions.
7. ~~**Append-only buffer with cursor**~~ — ✅ Done. Transcript is append-only (`Vec<OutputPart>`) with a read cursor. Captures use separate scratch space. `transcript()`, `reset_cursor()`, `resolve_transcript_slice()` exposed on Story.
8. ~~**Transcript serialization + locale re-render**~~ — ✅ Done. Binary `.brkt` format for transcript persistence. CLI: `--save-transcript` on play, `replay` subcommand with optional `--locale`. TUI: history re-renders on locale switch via transcript ranges.
9. ~~**Fragment model for locale-safe slot values**~~ — ✅ Done. `Value::FragmentRef(u32)`, `BeginFragment`/`EndFragment` opcodes (0x68/0x69), fragment store in output buffer. Recognized choice display text wrapped in fragments. Inline function calls in display context wrapped in per-call fragments. Function capture skipped inside fragments (output flows structurally). Choice re-resolution via `Story::pending_choices()`. TUI updates choices on locale switch. Transcript serialization includes fragment store.

## Open investigations

### Function capture model mismatch

**Status**: Identified, not fixed. Reverted an attempt.

**The problem**: Our VM calls `begin_capture()` on every `Call`/`CallVariable` opcode, capturing function output to use as a return value. The C# reference runtime does NOT capture at the function level — function output goes directly into the output stream. The C# runtime only captures during string evaluation (`BeginString`/`EndString`, our `BeginStringEval`/`EndStringEval`).

**Impact**: For statement calls (`~ func()`), leaked content from `discard_capture` (which removes the checkpoint but leaves content) causes spurious whitespace/newlines. This is the root cause of the I096, tower-of-hanoi, and nested-pass-by-reference failures.

**What the C# runtime does instead**:
- Function output goes directly to the output stream (no capture).
- On function pop, `TrimWhitespaceFromFunctionEnd` walks backward and removes trailing whitespace/newlines from the function's output region.
- For inline `{func()}`, `BeginString`/`EndString` handles the capture — it collects text from the output stream, removes it, and pushes the concatenated string as a value.

**What we tried**: Removed `begin_capture` from `Call`/`CallVariable`, added `trim_trailing_whitespace` to `OutputBuffer`, changed `pop_call_frame` to trim instead of capture/discard. Result: 8 run_story test failures (`StackUnderflow`) and 35 additional episode regressions. The failures were from inline `{func()}` calls where our hand-written test JSON uses `ev, f(), out, /ev` without `BeginStringEval`/`EndStringEval`. Real inklecate output may use different bytecode.

**Relationship to fragments**: The fragment model (step 9) provides a cleaner path than the reverted fix. Instead of removing function capture and relying on `BeginStringEval`/`EndStringEval`, fragments introduce `BeginFragment`/`EndFragment` as a display-specific capture mechanism. Inside a fragment, function output flows structurally (no resolution at the function boundary). This addresses both the whitespace issue (function output goes directly to the fragment, no `discard_capture` to leak newlines) and the locale re-rendering issue (structural data preserved in the fragment). The function capture model redesign may still be needed for correctness, but fragments reduce the blast radius by providing a controlled capture context for display-bound output.

**Next steps**:
1. Implement the fragment model (step 9) — this may resolve the function capture issues for display contexts without needing the full C# model redesign.
2. After fragments are working, evaluate whether the remaining function capture issues (statement calls like `~ func()`) still need the `TrimWhitespaceFromFunctionEnd` approach.
3. Update hand-written tests that have incorrect bytecode (missing string eval wrappers) regardless of approach.

**Key C# reference locations**:
- `Story.cs` lines 1301-1440: `BeginString`/`EndString` handling
- `StoryState.cs` lines 1201-1228: `TrimWhitespaceFromFunctionEnd`
- `StoryState.cs` lines 1230-1237: `PopCallstack` calls `TrimWhitespaceFromFunctionEnd`
- `StoryState.cs` lines 920-1024: `PushToOutputStreamIndividual` (function trimming state)

### Pre-existing episode failures (4 cases, 103 episodes)

1. **`tower-of-hanoi`** (100 episodes): Leading `\n` in text output from function body content leaking after `discard_capture`. Root cause: function capture model mismatch (see above).

2. **`I096-nested-pass-by-reference`** (1 episode): Extra `\n` from `squaresquare` function body's `Spring` + `Newline` leaking. Same root cause.

3. **`nested-pass-by-reference`** (tier3, 1 episode): Same issue as I096.

4. **`I075-clean-callstack-reset-on-path-choice`** (1 episode): Tag count mismatch. Pre-existing from the Line enum refactor — likely related to how `build_per_line_tags` reconstructs per-text-line tags from per-Line tags in the test harness.
