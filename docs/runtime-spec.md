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
4. **Locale switch:** swap per-container line tables + audio table from a different `.inkl` → re-link

### Linker step

The linker reads all definitions from the unlinked layer and:

1. For each `DefinitionId`, reads the tag and dispatches to the appropriate table
2. Assigns each definition a fast runtime index within its table
3. Builds resolution tables: `DefinitionId → runtime index` (one per tag type)
4. Resolves all `DefinitionId` references in bytecode to runtime indices
5. Indexes external function definitions (assigns runtime indices, builds name lookup tables). Resolution to host bindings or ink fallbacks is a runtime concern, not a link-time concern.
6. Initializes global variables from their default values
7. Builds per-container line tables, name table, and other content structures
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

All persistent positions within a flow use `(DefinitionId, offset)` for recompilation stability.

### Context (game state / save state)

A Context holds the narrative and game state that is meaningful to save, load, and synchronize:

- **Globals** — global variable values
- **Visit counts** — per container `DefinitionId`
- **Turn counts** — which turn each container was last visited
- **Turn index** — current turn number
- **RNG seed + state** — for deterministic randomness

Context is the natural serialization boundary — saving a story means serializing its Context (plus Flow state for mid-passage saves). Contexts can be cloned for speculative execution ("what happens if the player picks choice 2?") and diffed to see what changed.

### Program (immutable, shared)

The linked program — containers, bytecode, line tables, definitions, name table. Loaded once, shared across all story instances and flows. Never mutated after linking.

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

### Layer 1: Line-level continuation

Loops the VM step until a complete line of dialogue is ready:

- Detects newline boundaries in the output buffer (a newline followed by non-glue content confirms the line)
- Handles glue lookahead (a newline is tentative until the next step confirms it wasn't consumed by glue)
- Resolves external function calls via registered handlers
- Returns one line of text with its associated tags

This is equivalent to the reference ink runtime's `Continue()`.

### Layer 2: Passage-level continuation

Loops line-level continuation until a yield point (choices available, done, or ended). Returns all accumulated lines. This is equivalent to the reference ink runtime's `ContinueMaximally()` and the current behavior of `step()`.

### Layer 3: Story orchestrator

The `Story` manages one or more flows and their contexts, providing the convenient public API:

- **Single-flow** (common case): one flow, one context. API behaves like `ContinueMaximally` — step and get text + choices.
- **External function binding**: register handlers at the Story level. Line/passage-level continuation resolves external calls transparently.
- **Choice selection**: `choose(index)` is a flow-level operation — restores the thread fork, sets execution position, clears pending choices.

### Flows and instancing

Every flow in the Story is a named **(Flow, Context) pair**. Multi-flow and instanced flows are the same primitive — the difference is usage pattern, not mechanism.

- **Named flows**: the Story manages a collection of named (Flow, Context) pairs. The "default" flow is just the one created at startup. Additional flows can be created with their own entry points and contexts.
- **Instanced flows**: multiple (Flow, Context) pairs can share the same scene template (entry point in the Program). Each instance has a unique identity (e.g., `"shopkeeper:npc_42"`) and fully independent state.

**Variable scoping for instances** uses explicit registration. When setting up an instance template, the game developer declares which globals are **shared** (readable/writable across all instances, backed by a common store). Everything else in the Context is **per-instance by default** — visit counts, turn counts, turn index, RNG, and all unregistered globals get their own copy per instance. The VM sees a flat key-value store; the backing store handles the shared/instance split transparently.

**Lifecycle, persistence, and synchronization** are Story-layer or engine/caller-layer concerns — the Flow and VM know nothing about them. The Story (or the engine above it) decides when to spawn or destroy instances, how to serialize their contexts, and whether/how to propagate state between flows. The primitives (named (Flow, Context) pairs, explicit shared-global registration) are designed to support a range of policies without prescribing one.

Consumers who need maximum control can bypass the Story and work directly with flows, contexts, and `vm::step`. The Story is a convenience layer that does not sacrifice performance or control.

## Call frames and container positions

The VM distinguishes two kinds of entry into a container:

- **Flow entry** — moving into a child container (stitch, gather, choice branch). Pushes a container position onto the current call frame's position stack. Does NOT create a new call frame. The child shares the parent's temp variable slots.
- **Call entry** — function call or tunnel. Pushes a new call frame with a fresh position stack and fresh temp slots. The callee cannot access the caller's temps.

Each call frame contains:

```
CallFrame {
    frame_type: CallFrameType,
    return_address: (DefinitionId, offset),
    temps: Vec<Value>,                        // frame-local temp variable slots
    container_stack: Vec<(DefinitionId, offset)>,  // flow positions within this call
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

### Story-level integration

Higher-level APIs (line/passage continuation) resolve external calls transparently via an `ExternalFnHandler` trait passed to the orchestration layer. The handler receives the function name and arguments, and returns a resolution:

- `Resolved(Value)` — call completed, push return value
- `Fallback` — use the ink-defined fallback body
- `Pending` — async resolution; caller resolves later via `flow.resolve_external()`

When `continue_line` encounters `Stepped::ExternalCall`:

1. Read name from `flow.external_name(program)` and args from `flow.external_args()`
2. Call the handler trait method
3. **Resolved**: use `flow.resolve_external(result)`, continue stepping
4. **Fallback**: use `flow.invoke_fallback(program)`, continue stepping
5. **Pending**: yield to caller for async resolution
6. **Handler error**: propagate to caller with descriptive context (handler name, args, underlying cause)

The trait approach lets different consumers use different resolution strategies (closure map, ECS lookup, async bridge) without coupling the runtime to any specific pattern.

## Output buffer

The output buffer lives in each Flow and tracks per-line structure as content is emitted:

- The VM calls `push_text`, `push_newline`, `push_glue`, `begin_tag`/`end_tag` — it does not know about lines.
- The buffer internally groups text and tags into line segments, separated by newline boundaries.
- Glue resolution collapses line boundaries (a glue followed by a newline merges the adjacent lines).
- Callers query the buffer for structured output: completed lines with their associated tags, whether a partial line is in progress, etc.

This design solves per-line tag association (e.g., the i18n test case where tags must attach to the line they follow, not the entire passage) without adding complexity to the VM.

## Choice forks

When a choice is created (`BeginChoice`), the VM captures a **thread fork** — a snapshot of the current thread's call stack. This fork is stored on the `PendingChoice` and restored when the player selects that choice.

Choice forks capture **Flow state only** (call stack, temps). The Context (globals, visit counts) is **not** captured or rolled back — modifications to globals between fork creation and choice selection remain visible. This matches the reference ink runtime's behavior.

**Guiding invariant:** the multi-flow model must produce identical results to a single-flow story when only one flow is present. Choice forks are a Flow-local operation and do not interact with Context synchronization.

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

A `NarrativeRuntime` (or equivalent) host interface manages:

- Loading/unloading programs (via the linker step)
- Spawning/destroying story instances (each with its own flows and contexts)
- Stepping instances and collecting results
- Registering external function handlers
- Hot-reloading programs and reconciling instances
- Save/load for contexts and flow state

## Voice acting

Every text emission has a stable `LineId = (DefinitionId, u16)` — its container identity plus local line index. This is the same identity used for localization, so voice acting and text localization share a single addressing scheme.

Audio asset references live in the `.inkl` audio table, keyed by `LineId`. Authors can associate explicit recording IDs with lines via tags (`#voice:blacksmith_greeting_01`).

The runtime's text output includes `LineId` so the host can look up audio:

```
struct TextOutput {
    text: String,
    line_id: LineId,
    audio_ref: Option<AudioRef>,
}
```

The host handles playback — brink provides the mapping.

## Locale overlay loading

At runtime, loading a `.inkl` overlay replaces per-container line content and adds the audio table. The bytecode is unchanged — `EmitLine(2)` still references local line index 2, but the content behind that index is now in the target locale. Since lines are scoped to containers, only containers present in the `.inkl` have their lines replaced; others retain the base locale content.

The `.inkl` header includes the base `.inkb` checksum. The runtime validates this on load — a mismatched `.inkl` (compiled against a different `.inkb` version) is rejected.

## Ink semantics (runtime perspective)

Key semantics from the reference C# ink implementation relevant to execution:

- **Visit counting:** per-container granularity. Any container (knot, stitch, gather, choice target) can independently track visits and turn indices. `countingAtStartOnly` prevents overcounting on mid-container re-entry.
- **Stitch fall-through:** stitches do NOT fall through to each other at execution time. Only the first stitch is reachable via the implicit divert; all others require explicit `-> stitch_name`.
- **Choices inside conditional blocks:** at runtime, choices inside conditionals participate in the outer choice point via loose end propagation (matching the reference compiler's `Weave.cs` `PassLooseEndsToAncestors`). The HIR keeps conditional blocks opaque; codegen/runtime handles the weave transparency.
