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

1. **Delete `StoryState` trait** — make VM take `&Program` + `&mut Context` directly. Mechanical churn in `vm.rs`.
2. **Pull `Context` out of `FlowInstance`** — `Story` owns `(FlowInstance, Context)` pairs. `step_single_line` takes `&mut Context` as parameter.
3. **Remove observer from production path** — `continue_maximally_observed` goes away. Test harness uses `ObservedContext` + direct `FlowInstance` calls.
4. **Split `Program` and line tables** — linker returns `(Program, Vec<Vec<LineEntry>>)`. `apply_locale` becomes a pure function. `Program` is truly immutable.
5. **Defer line/value resolution** — output buffer stores `LineRef`/`ValueRef`. Resolution at read time.
6. **Append-only buffer with cursor** — `take_first_line` advances cursor, doesn't drain. Transcript API.
7. **Acceptance test** — locale swap + transcript re-render.
