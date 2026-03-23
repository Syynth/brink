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

## Staging

Each step is independently testable against the episode corpus:

1. ~~**Delete `StoryState` trait**~~ — ✅ Done. VM takes `&Program` + `&mut impl ContextAccess`.
2. ~~**Pull `Context` out of `FlowInstance`**~~ — ✅ Done. `Story` owns `(FlowInstance, Context)` pairs.
3. ~~**Remove observer from production path**~~ — ✅ Done. `ObservedContext` wraps `Context`, no observer param in `step_single_line`.
4. ~~**Split `Program` and line tables**~~ — ✅ Done. `link()` returns `(Program, Vec<Vec<LineEntry>>)`. `Program` is truly immutable.
5. **Defer line/value resolution** — ✅ Partial. `LineRef`/`ValueRef` in output buffer, resolution at read time. 847/950 episodes due to whitespace filtering interactions. Requires Spring implementation to complete.
6. **Add `Spring` opcode and output part** — format, compiler, converter, runtime.
   - a. Add `Opcode::Spring` to `brink-format`.
   - b. Add `OutputPart::Spring` to the output buffer with push-time dedup and resolve-time rule.
   - c. Update converter codegen to strip boundary whitespace from `EmitLine` content and emit `Spring` between consecutive emissions.
   - d. Update compiler codegen (`brink-codegen-inkb`) — same changes.
   - e. Remove `CleanOutputWhitespace` and push-time whitespace filtering (`push_text`, `ends_in_whitespace`, adjacent whitespace collapsing).
   - f. Add empty-slot whitespace collapsing to template resolution.
   - g. Verify episode corpus at 950/950.
7. **Append-only buffer with cursor** — `take_first_line` advances cursor, doesn't drain. Transcript API.
8. **Acceptance test** — locale swap + transcript re-render.
