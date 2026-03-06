# brink runtime specification

`brink-runtime` executes compiled `.inkb` stories via a stack-based bytecode VM. It depends ONLY on `brink-format`. See [format-spec](format-spec.md) for the types, instruction set, and file formats the runtime consumes.

## Core requirements

- **Bytecode VM:** stack-based execution of compiled stories
- **Multi-instance:** one linked program (immutable, shareable), many story instances with isolated per-instance state
- **Hot-reload:** safe recompilation without invalidating running state
- **Deterministic RNG:** per-instance seed/state for reproducible shuffle sequences

## Two-layer architecture

The runtime maintains two layers:

- **Unlinked layer:** the raw definition tables with symbolic `DefinitionId` references. This is the source of truth, populated from `.inkb`.
- **Linked layer:** the resolved `Program` with fast internal indices. Built by the linker step.

Loading, hot-reload, and patching all flow through the same linker step:

1. **Normal startup:** load `.inkb` → optionally overlay `.inkl` → populate unlinked layer → link → run
2. **Hot-reload (full):** replace entire unlinked layer → re-link → reconcile instances
3. **Hot-reload (patch):** update changed definitions in unlinked layer → re-link → reconcile instances
4. **Locale switch:** load a different `.inkl` → construct new `LinkedLocale` → pair with existing `LinkedBinary` into a new `Program`. No re-linking.

### Linker step

The linker reads all definitions from the unlinked layer and:

1. For each `DefinitionId`, reads the tag and dispatches to the appropriate table
2. Assigns each definition a fast runtime index within its table
3. Builds resolution tables: `DefinitionId → runtime index` (one per tag type)
4. Resolves all `DefinitionId` references in bytecode to runtime indices
5. Indexes external function definitions (assigns runtime indices, builds name lookup tables). Resolution to host bindings or ink fallbacks is a runtime concern, not a link-time concern.
6. Initializes global variables from their default values
7. Splits per-container line tables out of the `.inkb` into the base `LinkedLocale`. Builds name table and other content structures for the `LinkedBinary`.
8. Produces an immutable, shareable `Program`

One codepath processes all definition types uniformly. The tag determines which table, but the resolution mechanism is the same.

## Three-part state model

Runtime state is split into three parts with distinct ownership and lifecycle:

### Flow (isolated execution state)

A Flow is a fully isolated execution context — analogous to a separate conversation or narrative track. Each flow owns:

- **Threads / call stack** — `Vec<Thread>`, each thread containing a `Vec<CallFrame>` (return address, temp slots, container position stack). The current position is `call_stack.top().container_stack.top()`.
- **Value stack** — operand stack for bytecode evaluation
- **Output buffer** — accumulated text with per-line structure and tag association (see [Output buffer](#output-buffer))
- **Pending choices** — choices awaiting player selection, each with a thread fork snapshot
- **Tag state** — current tags, in-tag flag

Positions within a flow use resolved runtime indices — `(u32 container_idx, usize offset)` — for fast execution. Translation to/from symbolic `DefinitionId` happens at reconciliation (`story.reload`) and save/load boundaries, not during execution.

### Context (game state / save state)

A Context holds the narrative and game state that is meaningful to save, load, and synchronize:

- **Globals** — global variable values
- **Visit counts** — per container `DefinitionId`
- **Turn counts** — which turn each container was last visited
- **Turn index** — current turn number
- **RNG seed + state** — for deterministic randomness

Context is the natural serialization boundary — saving a story means serializing its Context (plus Flow state for mid-passage saves). Contexts can be cloned for speculative execution ("what happens if the player picks choice 2?") and diffed to see what changed.

### Program (immutable, shared)

The `Program` is a `(Arc<LinkedBinary>, Arc<LinkedLocale>)` pair. The binary half (containers, bytecode, globals, lists, externals, labels, name table) is linked once and `Arc`-shared across all story instances and locale variants. The locale half (per-container line tables with content and audio refs) is `Arc`-shared across all instances using the same locale. `LinkedBinary` has no line tables — it is purely structural. The `Program` is never mutated after construction. Switching locales constructs a new `Program` with a different locale half — the binary half is reused. `Program` construction is cheap (two `Arc` clones); building the halves is the expensive step.

## Execution model

The execution model is layered: a dumb per-instruction VM at the bottom, with progressively higher-level APIs built on top. Each layer adds intelligence; lower layers know nothing about the layers above.

### Layer 0: VM step (per-instruction)

The VM processes a single bytecode instruction and reports what happened:

```
vm::step(flow: &mut Flow, context: &mut Context, program: &Program) -> Result<Stepped>
```

The VM is maximally dumb — it decodes one opcode, executes it, and returns. It does not loop, does not make decisions about when to stop, and does not know about lines, passages, or the Story.

```
enum Stepped {
    Continue,                                  // opcode executed, nothing special
    ExternalCall,                              // hit external fn — name, args, fallback all on the External frame
    ThreadCompleted,                           // current thread exhausted, switched to next
    Done,                                      // hit Done opcode
    Ended,                                     // hit End opcode
}
```

All runtime errors (type errors, stack underflow, unresolved external calls, etc.) are returned via `Result::Err(RuntimeError)`. Error variants should be detailed enough for the caller to decide recoverability — e.g., a type error includes the types involved, an unresolved external includes the function name. If the VM is in an unrecoverable state, subsequent `step` calls will continue returning the same error; it is the caller's responsibility to detect this and stop.

When the VM yields `ExternalCall`, it has popped the arguments from the value stack and pushed an `External` call frame. The caller must resolve the external call before the next `step` — see [External function handling](#external-function-handling).

### Layer 1: Line-level continuation (`continue_line`)

Public API. Loops the VM step until a complete line is ready or a yield point is reached.

```
story.continue_line() -> Result<LineResult>
```

Termination conditions:
1. **Confirmed newline** — non-glue content after a newline confirms the previous line is complete. The new content stays in the buffer for the next `continue_line()` call. Returns `LineResult::Complete`.
2. **Yield point** — `Done`, `Ended`, or choice set. Returns the appropriate terminal `LineResult` variant.
3. **Pending external** — handler returned `Pending`. Returns `LineResult::PendingExternal`. Buffer is untouched.

Line assembly within one `continue_line()` call:
1. VM steps, pushing text/newlines/glue/tags/SpanStart into the output buffer.
2. Events may fire (VM yields `ExternalCall`, `is_event` returns true) — Story layer accumulates them in a temporary `Vec<Event>`.
3. When a line is complete, flush the output buffer for that single line — produces `(Vec<Span>, Vec<String>)` (resolved spans + tags).
4. Assemble `Line { spans, events, tags }` from the flush result and accumulated events.
5. Return the `Line` in the appropriate `LineResult` variant.

Event accumulation scope is exactly one `continue_line()` call. No cross-line correlation or index tracking needed.

This is equivalent to the reference ink runtime's `Continue()`.

### Layer 2: Passage-level continuation (`continue_maximally`)

Public API. Loops line-level continuation until a yield point. Returns all accumulated lines.

```
story.continue_maximally() -> Result<InkOutcome>
```

Collects `LineResult::Complete` lines, stops at any terminal variant, and assembles `InkOutcome`. See [Public API types](#public-api-types) for the assembly pseudocode.

This is equivalent to the reference ink runtime's `ContinueMaximally()`.

### Layer 3: Story orchestrator

The `Story` manages one or more flows and their contexts, providing the convenient public API:

- **Single-flow** (common case): one flow, one context.
- **External function handling**: `ExternalFnHandler` trait registered at the Story level. `continue_line()` resolves external calls transparently. See [ExternalFnHandler trait](#externalfnhandler-trait).
- **Event capture**: Story layer captures events during `continue_line()` and associates them with the line being built. See [Event capture](#event-capture).
- **Choice selection**: `choose(index)` is a flow-level operation — restores the thread fork, sets execution position, clears pending choices.
- **Global variable access**: `get_global(name) -> ExternalValue`, `set_global(name, ExternalValue)`. Uses `ExternalValue` at the boundary.
- **Pending external query**: `has_pending_external() -> bool` — distinguishes "blocked on async external" from "actual error" when `continue_maximally()` returns `Err(UnresolvedExternalCall)`.
- **External resolution**: `resolve_external(value: ExternalValue)` — provides the result for a pending external call. The next `continue_maximally()` or `continue_line()` call continues from where it froze.

### Flows and instancing

Every flow in the Story is a named **(Flow, Context) pair**. Multi-flow and instanced flows are the same primitive — the difference is usage pattern, not mechanism.

- **Named flows**: the Story manages a collection of named (Flow, Context) pairs. The "default" flow is just the one created at startup. Additional flows can be created with their own entry points and contexts.
- **Instanced flows**: multiple (Flow, Context) pairs can share the same scene template (entry point in the Program). Each instance has a unique identity (e.g., `"shopkeeper:npc_42"`) and fully independent state.

**Variable scoping for instances** is determined at the ink source level via the `FLOW VAR` keyword. `VAR x = 0` (the default) declares a shared global — readable/writable across all instances, backed by a common store. `FLOW VAR x = 0` declares a per-instance variable — each flow instance gets its own copy. The instance flag is a single bit on `GlobalVarDef` in the format. The linker partitions globals into shared and instance ranges. The Context provides a split backing store transparent to the VM — `GetGlobal`/`SetGlobal` don't branch on scoping. Visit counts, turn counts, turn index, and RNG are always per-instance.

**Lifecycle, persistence, and synchronization** are Story-layer or engine/caller-layer concerns — the Flow and VM know nothing about them. The Story (or the engine above it) decides when to spawn or destroy instances, how to serialize their contexts, and whether/how to propagate state between flows. The primitives (named (Flow, Context) pairs, source-level variable scoping) are designed to support a range of policies without prescribing one.

Consumers who need maximum control can bypass the Story and work directly with flows, contexts, and `vm::step`. The Story is a convenience layer that does not sacrifice performance or control.

## Call frames and container positions

The VM distinguishes two kinds of entry into a container:

- **Flow entry** — moving into a child container (stitch, gather, choice branch). Pushes a container position onto the current call frame's position stack. Does NOT create a new call frame. The child shares the parent's temp variable slots.
- **Call entry** — function call or tunnel. Pushes a new call frame with a fresh position stack and fresh temp slots. The callee cannot access the caller's temps.

Each call frame contains:

```
ContainerPosition {
    container_idx: u32,
    offset: usize,
}

CallFrame {
    frame_type: CallFrameType,
    return_address: Option<ContainerPosition>,   // None for Root frames
    temps: Vec<Value>,                           // frame-local temp variable slots
    container_stack: Vec<ContainerPosition>,      // flow positions within this call
}
```

### Call frame types

| Type | Created by | Behavior |
|------|-----------|----------|
| `Root` | Story startup | Main flow entry. Empty → yield Done. |
| `Function` | `Call` opcode | Output captured as return value. Empty → pop frame, push captured text. |
| `Tunnel` | `TunnelCall` opcode | Non-capturing call. Yields for pending choices. |
| `Thread` | `ThreadCall` opcode | Boundary frame — thread won't unwind into inherited frames below. |
| `External` | `CallExternal` opcode | Marker frame for pending external function resolution. Contains no bytecode — the frame exists to track state on the call stack. See [External function handling](#external-function-handling). |

The "current container" is always the top of `call_stack.top().container_stack`. Finishing a container (reaching end of its bytecode) pops from the container stack and resumes the parent. Returning from a function/tunnel pops the entire call frame. Diverts replace the current container position. Threads fork the entire call stack (call frames + their container stacks).

## External function handling

External function resolution uses the call stack itself as the state machine. The `External` call frame type (see [Call frame types](#call-frame-types)) tracks pending external calls with no separate state flags.

### VM behavior

When the VM hits `CallExternal(fn_id, argc)`:

1. Pop `argc` args from the value stack
2. Push an `External` call frame — return address = current position, the external function's `DefinitionId` for fallback lookup, and the popped args stored in the frame's `temps` (frame-local storage)
3. Yield `Stepped::ExternalCall`

Everything about the pending call — name, args, fallback `DefinitionId` — lives on the `External` frame. The `Stepped` variant is a pure signal with no payload. The caller inspects the flow to get what it needs (e.g., `flow.external_name(program)`, `flow.external_args()`). This keeps the yield minimal and ensures all call state survives serialization, debugging, and save/load.

If `step` is called while an `External` frame is on top of the call stack (i.e., the caller forgot to resolve it), the VM returns `Err(RuntimeError::UnresolvedExternalCall)`. The caller can inspect the frame for details.

### Caller resolution

The caller resolves the external call via methods on the Flow:

- **Provide a result**: `flow.resolve_external(value)` — pops the `External` frame and pushes the return value onto the value stack. For fire-and-forget calls (e.g., `~ play_sound(...)`), the caller provides `Value::Null` and the next opcode (`Pop`) discards it.
- **Invoke the ink fallback**: `flow.invoke_fallback(program)` — replaces the `External` frame with a `Function` frame pointing at the ink-defined fallback container. The next `step` call executes the fallback using the existing function call machinery (output capture, return value, frame pop). No special-case VM code needed.

### ExternalFnHandler trait

Per-call dispatch for external function resolution. The trait answers "is this an event or a function?" and resolves function calls.

```
trait ExternalFnHandler {
    fn is_event(&self, name: &str) -> bool;
    fn call(&self, name: &str, args: &[ExternalValue]) -> ExternalResult;
}

enum ExternalResult {
    Resolved(ExternalValue),
    Fallback,
    Pending,
}
```

| Variant | Meaning | Runtime behavior |
|---------|---------|-----------------|
| `Resolved(ExternalValue)` | Function call completed | Push return value via `flow.resolve_external()` |
| `Fallback` | Use the ink-defined fallback body | Invoke fallback via `flow.invoke_fallback()` |
| `Pending` | Async resolution; caller resolves later | Yield `Err(RuntimeError::UnresolvedExternalCall)` to caller |

**`is_event` is always called first.** If it returns `true`, `call` is never invoked for that external. Common implementations use upfront declarative registration (e.g., a `HashSet<String>` of event names and a `HashMap<String, Box<dyn Fn>>` of function bindings), but power users can implement custom per-call logic.

When `continue_line` encounters `Stepped::ExternalCall`:

1. Read name from `flow.external_name(program)` and args from `flow.external_args()`
2. Call `handler.is_event(name)`:
   - **Event**: capture `Event { name, args }` in the Story layer's event accumulator, resolve external with `Value::Null`, continue stepping. The handler's `call()` is never invoked. See [Event capture](#event-capture).
3. If not an event, call `handler.call(name, args)`:
   - **Resolved**: use `flow.resolve_external(result)`, continue stepping
   - **Fallback**: use `flow.invoke_fallback(program)`, continue stepping
   - **Pending**: return `Err(RuntimeError::UnresolvedExternalCall)` to caller

### Event capture

Events are fire-and-forget external call records. They are structurally replay-safe: the handler's `call()` method is never invoked for events.

- **Output buffer**: text-domain only. No event awareness. Handles text, newlines, glue, tags, SpanStart, captures.
- **Event capture**: Story/continuation layer responsibility. Separate from the output buffer.
- **Line assembly**: Story layer attaches accumulated events to the `Line` produced by the current `continue_line()` call. Event accumulation scope is exactly one `continue_line()` invocation.

## Output buffer

The output buffer lives in each Flow and tracks per-line structure as content is emitted. It is purely text-domain — no events.

### OutputPart variants

```
enum OutputPart {
    Text(String),
    Newline,
    Glue,
    Tag(String),
    Checkpoint,
    SpanStart(LineId),
}
```

`SpanStart(LineId)` is pushed by the VM when executing `EmitLine(idx)`. The VM constructs the `LineId` from the current container's `DefinitionId` and the local line index, then pushes `SpanStart(LineId)` followed by the resolved text content. The VM does NOT resolve audio refs — that happens at flush time.

Text pushed without a preceding `SpanStart` (from `EmitValue`, string evaluation, or inline expressions) has no `LineId` and forms identity-less spans.

### Span boundary rules

- A new span starts at every `SpanStart`.
- Text without a preceding `SpanStart` forms an identity-less span.
- Adjacent identity-less text segments are coalesced into a single span.
- Glue fuses lines by concatenating their span vectors. Span boundaries within a fused line are preserved.

### Single-line flush

The output buffer provides a single-line flush operation used by `continue_line()`:

1. Extract parts up to and including the confirmed newline boundary (or end-of-buffer for terminal lines).
2. Resolve glue (same algorithm as today — mark newlines consumed by glue, stitch text across boundaries).
3. Walk the resolved parts, building spans:
   - On `SpanStart(line_id)`: start a new span. Look up `line_id` in the `LinkedLocale` to get `audio_ref`.
   - On `Text(s)`: append to the current span. If no current span exists, start an identity-less span (no `LineId`, no audio ref).
   - On `Text(s)` when current span is identity-less: coalesce (append text to existing span).
   - On `Tag(t)`: collect into the line's tag list.
   - Skip resolved Glue/Newline/Checkpoint markers.
4. Return `(Vec<Span>, Vec<String>)` — resolved spans and tags for the line.

`LineId` is consumed during step 3 (used to look up audio ref) and does not appear in the returned `Span` structs.

### Output capture

The output buffer supports a checkpoint-based capture mechanism used by function calls, string evaluation, and tag collection. Captures nest correctly — inner captures complete before outer ones.

#### API

- `begin_capture()` — pushes a `Checkpoint` marker into the buffer.
- `end_capture() -> Option<String>` — finds the rightmost `Checkpoint`, drains everything after it, resolves glue within the captured region, and returns the result as a flat string. Returns `None` if no checkpoint exists.
- `discard_capture()` — removes the rightmost `Checkpoint` without capturing. Text after the checkpoint remains in the buffer.
- `has_checkpoint() -> bool` — returns whether any capture is active.

#### Usage by the VM

| Context | begin | end | Notes |
|---------|-------|-----|-------|
| **Function call** (`Call` opcode) | At call site, before pushing `Function` frame | At frame pop (implicit return) | Captured text becomes the function's return value (pushed to value stack). Trailing newlines are trimmed — inline callers (`{f()}`) expect clean text. |
| **Explicit return** (`~return`) | Same as above | `discard_capture()` at frame pop | Return value is already on the value stack from `~return`; capture is discarded, text stays in the output buffer. |
| **String evaluation** (`BeginStringEval` / `EndStringEval`) | `BeginStringEval` | `EndStringEval` | Captured text pushed to value stack as `Value::String`. Used for inline expressions like `{"hello"}`. |
| **Tag collection** (`BeginTag` / `EndTag`) | `BeginTag` | `EndTag` | Captured text becomes the tag string. If inside another capture (`has_checkpoint()` is true), the tag is stored in `flow.current_tags` for the enclosing choice/function. Otherwise, it is pushed to the output buffer as `OutputPart::Tag`. |
| **Fallback invocation** | At fallback site, before switching `External` frame to `Function` | Same as function call | Ink fallback for an external function — output capture makes it behave identically to a normal function call. |

#### Nesting

Captures are discovered via rightmost-checkpoint search, so nesting works naturally:

```
begin_capture()           // Outer: function call
  push_text("prefix")
  begin_capture()         // Inner: string eval
    push_text("value")
  end_capture()           // → "value" (pushed to value stack)
  push_text("suffix")
end_capture()             // → "prefixsuffix"
```

`SpanStart` markers inside a captured region are consumed by the capture — they do not leak into the outer buffer. Captured content produces a flat string, not spans. Span identity is meaningless inside a capture.

Checkpoints are transparent to glue resolution — they don't block the search for preceding newlines or affect text joining.

`CaptureUnderflow` is a `RuntimeError` raised when `end_capture()` returns `None` in a context that requires it (e.g., `EndStringEval`, implicit function return).

### Whitespace handling

The runtime applies whitespace normalization matching the reference ink runtime:

- **Newline de-duplication**: consecutive newlines are collapsed. Leading newlines in the output stream are suppressed.
- **Per-line inline whitespace normalization** (`clean_output_whitespace`): applied to each line at flush time.
  - Strips all leading spaces/tabs from each line.
  - Strips all trailing spaces/tabs before `\n` or end of string.
  - Collapses consecutive space/tab runs within a line to a single space.
  - Only affects inline whitespace (space and tab). Newlines are preserved.

## Choice evaluation

### Choice opcodes

The VM processes choices via a `BeginChoiceSet` / `BeginChoice` + `EndChoice` / end-of-set sequence:

1. `BeginChoiceSet` — clears the pending choices list.
2. For each choice: `BeginChoice(flags, target_id)` ... `EndChoice`.
3. After all choices are processed, the Story layer inspects `pending_choices` to decide what to yield.

`ChoiceFlags` is a packed byte on `BeginChoice`:

| Flag | Bit | Meaning |
|------|-----|---------|
| `has_condition` | 0x01 | Condition value is on the stack |
| `has_start_content` | 0x02 | Start content (shown to player + printed on selection) is on the stack |
| `has_choice_only_content` | 0x04 | Choice-only content (shown to player, not printed on selection) is on the stack |
| `once_only` | 0x08 | Skip if target container already visited |
| `is_invisible_default` | 0x10 | Fallback choice — not presented to the player |

### Choice skipping (`skipping_choice`)

When `BeginChoice` determines a choice should be skipped (condition is false, or once-only and already visited), it sets `flow.skipping_choice = true` and pops any text values from the stack to keep it balanced. While `skipping_choice` is true, `Goto` opcodes execute as no-ops — this allows the bytecode between `BeginChoice` and `EndChoice` to be traversed without executing diverts. `EndChoice` always clears `skipping_choice` to `false`.

The skip evaluation order within `BeginChoice`:
1. If `has_condition`: pop and evaluate. If falsy → skip (pop remaining text values, set `skipping_choice`).
2. If `once_only`: check `visit_counts[target_id]`. If > 0 → skip (pop remaining text values, set `skipping_choice`).
3. If not skipped: pop text values, build display text, fork thread, create `PendingChoice`.

### Thread forks

When a choice is created, the VM captures a **thread fork** — a snapshot of the current thread's call stack. This fork is stored on the `PendingChoice` and restored when the player selects that choice.

Choice forks capture **Flow state only** (call stack, temps). The Context (globals, visit counts) is **not** captured or rolled back — modifications to globals between fork creation and choice selection remain visible. This matches the reference ink runtime's behavior.

**Guiding invariant:** the multi-flow model must produce identical results to a single-flow story when only one flow is present. Choice forks are a Flow-local operation and do not interact with Context synchronization.

### Invisible default choice auto-selection

Choices with `is_invisible_default` are fallback choices — they exist to prevent dead ends but should never be presented to the player. After all choices in a set are processed:

- **All choices are invisible defaults**: auto-select the first one and continue execution without yielding. The Story layer calls `select_choice(0)` internally and loops back to stepping.
- **Any visible choice exists**: filter out invisible defaults from the choice set before yielding `Choices` to the caller. Invisible defaults are never presented to the player.

This matches the reference ink runtime's behavior for fallback choices (e.g., `+ [<auto>] -> somewhere`).

## Sequence semantics

Ink sequences (`stopping`, `cycle`, `once`, `shuffle`) are compiled into a `Sequence(kind, count)` opcode. The VM evaluates the opcode and pushes a branch index onto the value stack; subsequent bytecode uses that index to select the appropriate branch.

### Sequence kinds

| Kind | Branch index | Behavior |
|------|-------------|----------|
| `Cycle` | `visit_count % count` | Wraps around to the first branch after exhausting all branches |
| `Stopping` | `visit_count.min(count - 1)` | Stays on the last branch once reached |
| `OnceOnly` | `visit_count < count ? visit_count : count` | Returns `count` (past-the-end) after exhausting all branches, causing all to be skipped |
| `Shuffle` | Fisher-Yates partial shuffle | Random permutation, re-shuffled each full loop |

For non-shuffle sequences, the VM pops a `DivertTarget(DefinitionId)` from the value stack and looks up the container's visit count. Visit counts are 0-based for sequence purposes (`CurrentVisitCount` subtracts 1 from the raw visit count).

### Shuffle algorithm

Shuffle sequences use a partial Fisher-Yates algorithm seeded deterministically. The VM pops `numElements` (branch count) and `seqCount` (total visits) from the value stack, then:

1. Compute `loop_index = seqCount / numElements` (which full permutation cycle we're in).
2. Compute `iteration_index = seqCount % numElements` (position within the current permutation).
3. Seed: `path_hash + loop_index + rng_seed` (wrapping `i32` addition, matching reference).
4. Create a fresh RNG from the seed.
5. Partial Fisher-Yates: maintain an unpicked list `[0..numElements)`, pick `iteration_index + 1` elements using the RNG, return the last picked index.

The same `path_hash + loop_index` combination always produces the same permutation. Re-visiting the sequence with a different `loop_index` produces a different permutation.

### Container path hash (`path_hash`)

Each `LinkedContainer` carries a `path_hash: i32` — the sum of the Unicode code points of the container's ink path string (e.g., `"knot.stitch"` → sum of char values). This provides a container-specific seed component for shuffle sequences, ensuring different containers produce different random orderings even with the same story seed.

## Deterministic RNG

The runtime uses pluggable RNG via the `StoryRng` trait:

```
trait StoryRng {
    fn from_seed(seed: i32) -> Self;
    fn next_int(&mut self) -> i32;    // non-negative
}
```

A fresh RNG instance is created per random operation (shuffle sequence, `LIST_RANDOM`) with a deterministic composite seed derived from story state. This ensures:

- **Reproducibility**: same story seed + same choices → identical random outcomes.
- **Container isolation**: different containers get different seeds via `path_hash`.
- **Progression**: revisiting a shuffle sequence produces a new permutation via `loop_index`.

### Included implementations

| Type | Algorithm | Use case |
|------|-----------|----------|
| `FastRng` | Xorshift32 | Default for production. Fast, decent distribution. |
| `DotNetRng` | Knuth subtractive (port of .NET `System.Random`) | Reference compatibility. Reproduces the exact random sequence of the C# ink runtime for corpus test matching. |

`Story` is generic over `R: StoryRng`, defaulting to `FastRng`. Use `Story::<DotNetRng>::new(...)` for .NET-compatible output.

## Execution statistics

The `Stats` struct provides always-on execution counters accessible via `story.stats()`. Incrementing a `u64` is effectively free compared to opcode dispatch, so stats are unconditionally collected.

```
struct Stats {
    opcodes: u64,                // total bytecode instructions dispatched
    steps: u64,                  // total vm::step calls from the outer loop
    threads_created: u64,        // thread forks (ThreadCall + choice creation)
    threads_completed: u64,      // threads that completed and were popped
    frames_pushed: u64,          // call frames pushed (calls, tunnels, externals)
    frames_popped: u64,          // call frames popped (returns)
    choices_presented: u64,      // choice sets yielded to the player
    choices_selected: u64,       // individual choices selected
    snapshot_cache_hits: u64,    // call stack snapshot reuse (CoW hit)
    snapshot_cache_misses: u64,  // call stack snapshot allocation (CoW miss)
    materializations: u64,       // call stack materializations (CoW flatten)
}
```

Stats are read-only from the public API and do not affect execution. They serve as a diagnostic tool for profiling, benchmarking, and debugging story performance.

## Hot-reload reconciliation

All persistent references in story instances use `DefinitionId`, not runtime indices. When a new program is linked, the reconciliation is a single pass over the old and new definition sets by `DefinitionId`, regardless of type:

1. For each running instance, check every `(DefinitionId, offset)` position across all call frames and their container position stacks:
   - **Container exists, content hash unchanged** → position is valid, do nothing
   - **Container exists, content hash changed** → reset offset to 0 (container entry)
   - **Container gone** → fall back up the container/call stack to deepest valid position, or reset to entry point
3. Detect renames via content hashing (removed container with same content hash as added container = rename)
4. Visit counts keyed by container `DefinitionId` — retain for containers that still exist, orphan the rest
5. Sequence states keyed by `(DefinitionId, sequence_index)` — invalidate if content hash changed
6. Pending choices — invalidate (the choice set may no longer exist)
7. Reconcile variables: diff old and new variable definitions by `DefinitionId` (keep existing, add new with defaults, flag removed/type-changed)
8. Reconcile list definitions: new items are added, removed items are orphaned
9. Return a `ReconcileReport` with warnings for editor integration

## Multi-instance management

The `Story` type covers multi-instance management directly via named flows. Each named flow is a `(Flow, Context)` pair with independent state. The Story handles:

- Spawning/destroying named flow instances (`spawn_flow`, `destroy_flow`)
- Stepping individual flows and collecting results (`step_flow`, `choose_flow`)
- Registering external function handlers (passed to `step_with` / `step_flow_with`)
- Hot-reloading programs and reconciling all flow instances
- Save/load for contexts and flow state

No separate `NarrativeRuntime` host interface is needed — Story is the top-level API for both single-flow and multi-instance use cases.

## Public API types

### ExternalValue

The single public boundary type for all value exchange between host and runtime. Used for: event arguments, external function arguments, external function return values, and host get/set of global variables.

```
enum ExternalValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(String),
    List(ExternalList),
    Null,
}
```

`ExternalList` uses resolved string names for items and origins. Internal `DefinitionId`s never appear in the public API.

```
struct ExternalList {
    items: Vec<String>,       // resolved item names (e.g., "Emotion.happy")
    origins: Vec<String>,     // resolved list definition names (e.g., "Emotion")
}
```

The runtime translates between internal `Value` (which includes `DivertTarget`, `VariablePointer`, `TempPointer`, and `DefinitionId`-based list representations) and `ExternalValue` at the API boundary. `DivertTarget`, `VariablePointer`, and `TempPointer` are internal-only and never cross the boundary.

### Span

A segment of resolved text output with optional audio reference.

```
struct Span {
    text: String,
    audio_ref: Option<AudioRef>,
}
```

`AudioRef` is a `String` (or newtype wrapper) — an opaque audio asset identifier interpreted by the host (filenames, FMOD events, Wwise events, etc.). The runtime does not interpret it.

`LineId` is NOT present in `Span`. It is an internal addressing mechanism used during output buffer flush to resolve text content and audio refs from the `LinkedLocale`. By the time a `Span` reaches the consumer, everything is resolved and `LineId` is discarded.

### Event

A fire-and-forget external call record. Events are replay-safe by construction — the handler is never invoked for events (see [Event capture](#event-capture)).

```
struct Event {
    name: String,
    args: Vec<ExternalValue>,
}
```

### Line

One line of dialogue output. Combines resolved text spans, events that fired during the line's production, and ink tags.

```
struct Line {
    spans: Vec<Span>,
    events: Vec<Event>,
    tags: Vec<String>,
}
```

- **`spans`**: always `Vec<Span>`, even for the common single-span case.
  - Simple case (most common): one span from a single `EmitLine` opcode.
  - Glued case: multiple spans from lines fused by glue, each preserving its own audio ref.
  - Dynamic case: spans with no audio ref, from `EmitValue` or string evaluation.
  - Mixed case: `EmitLine` spans interleaved with dynamic spans.
- **`events`**: events that fired during this line's `continue_line()` invocation. See [Event capture](#event-capture).
- **`tags`**: ink tags associated with this line. Per-line, not per-span.

### Choice

A single choice presented to the player. Uniform structure with `Line`.

```
struct Choice {
    spans: Vec<Span>,
    events: Vec<Event>,
    tags: Vec<String>,
    index: usize,
}
```

Events during choice construction (between `BeginChoice` and `EndChoice`) attach to the choice, not to preceding lines.

### LineResult

Return type of `continue_line()` — the line-level continuation API.

```
enum LineResult {
    Complete(Line),
    Final(Line),
    Choices(Line, Vec<Choice>),
    PendingExternal,
    Ended(Line),
}
```

| Variant | Meaning | Caller action |
|---------|---------|---------------|
| `Complete(Line)` | Confirmed line, more content may follow | Call `continue_line()` again |
| `Final(Line)` | Trailing content before `Done` opcode; story can resume later | Consume line; may call `continue_maximally()` again later |
| `Choices(Line, Vec<Choice>)` | Trailing content + choice set | Consume line, present choices, call `choose()` |
| `PendingExternal` | Blocked on external; no line produced, buffer untouched | Resolve external via `story.resolve_external(value)`, then call `continue_line()` again |
| `Ended(Line)` | Trailing content before `End` opcode; story permanently finished | Consume line; story is done |

Terminal variants always carry a `Line`. The `Line` may have empty spans — this is valid (e.g., tag-only lines, yield points with no preceding text). Validated against the reference C# ink runtime: `Continue()` returns `""` with tags populated for tag-only lines, and returns `""` when choices appear with no preceding content.

### InkOutcome

Return type of `continue_maximally()` — the passage-level continuation API. Assembled by looping `continue_line()`.

```
enum InkOutcome {
    Done {
        lines: Vec<Line>,
    },
    Choices {
        lines: Vec<Line>,
        choices: Vec<Choice>,
    },
    Ended {
        lines: Vec<Line>,
    },
}

impl InkOutcome {
    fn lines(&self) -> &[Line];              // all variants
    fn choices(&self) -> Option<&[Choice]>;  // Choices variant only
    fn is_ended(&self) -> bool;              // Ended variant
}
```

`continue_maximally()` is a mechanical loop:

```
fn continue_maximally() -> Result<InkOutcome>:
    let mut lines = Vec::new()
    loop:
        match continue_line():
            Complete(line)         => lines.push(line)
            Final(line)            => lines.push(line); return Ok(Done { lines })
            Choices(line, choices) => lines.push(line); return Ok(Choices { lines, choices })
            PendingExternal        => return Err(RuntimeError::UnresolvedExternalCall)
            Ended(line)            => lines.push(line); return Ok(Ended { lines })
```

`continue_maximally()` does not filter empty trailing lines. Empty-span lines with tags are valid output (matches reference runtime behavior). The consumer receives them as-is.

## Program composition

### Line entry

Each line entry in the `LinkedLocale`:

```
struct LineEntry {
    content: LineContent,          // Plain(String) or Template(LineTemplate)
    audio_ref: Option<String>,     // audio asset identifier, if any
}
```

`audio_ref` lives alongside content in the same entry, not in a separate audio table. Both explicit audio refs (from `#voice:` tags) and derived audio refs (from tooling) are stored here by the compiler/tooling.

### resolve_line

```
resolve_line(locale: &LinkedLocale, line_id: LineId) -> &LineEntry
```

Always goes through the locale. No fallback logic, no "check locale then check base" branching. The `LinkedLocale` is always complete.

## Locale loading

### Base locale from .inkb

Loading a `.inkb` and linking produces `(LinkedBinary, LinkedLocale)`. The `.inkb`'s per-container line sub-tables are split out of the binary half and become the base `LinkedLocale`. On disk, the `.inkb` is self-contained (line tables are present). In memory, the runtime separates them.

### .inkl overlay loading

```
load_locale(inkb: &[u8], inkl: &[u8], mode: LocaleMode) -> Result<LinkedLocale>

enum LocaleMode {
    Strict,
    Overlay,
}
```

- **`Strict`**: the `.inkl` must provide line tables for every container in the `.inkb`. Missing containers produce a load error. Use for full translations (e.g., en-US to ja-JP).
- **`Overlay`**: the `.inkl` can be partial. Missing containers are filled from the base `.inkb` line tables. Use for dialect patches (e.g., en-US to en-UK where only some lines differ).

Either mode produces a complete `LinkedLocale`. No runtime fallback needed.

The `.inkl` header includes the base `.inkb` checksum. Mismatched checksums (`.inkl` compiled against a different `.inkb` version) produce a load error.

### Locale switching

Switching locales = constructing a new `Program` with a different `Arc<LinkedLocale>`:

```
let french = load_locale(&inkb_bytes, &french_inkl_bytes, LocaleMode::Strict)?;
let program = Program::new(binary.clone(), Arc::new(french));
```

No re-linking. The `LinkedBinary` is reused. Running story instances that reference the old `Program` continue using it; new steps use the new `Program`.

## Ink semantics (runtime perspective)

Key semantics from the reference C# ink implementation relevant to execution:

- **Visit counting:** per-container granularity. Any container (knot, stitch, gather, choice target) can independently track visits and turn indices. `countingAtStartOnly` prevents overcounting on mid-container re-entry.
- **Stitch fall-through:** stitches do NOT fall through to each other at execution time. Only the first stitch is reachable via the implicit divert; all others require explicit `-> stitch_name`.
- **Choices inside conditional blocks:** at runtime, choices inside conditionals participate in the outer choice point via loose end propagation (matching the reference compiler's `Weave.cs` `PassLooseEndsToAncestors`). The HIR keeps conditional blocks opaque; codegen/runtime handles the weave transparency.
