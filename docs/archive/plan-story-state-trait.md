# Plan: StoryState Trait Refactor and brink-test-harness

## Overview

Introduce a `StoryState` trait that bundles program access with mutable state, refactor the VM and Story to use `&mut impl StoryState`, and create a new internal `brink-test-harness` crate for episode-based behavioral testing with state tracking.

## 1. StoryState Trait Definition

Lives in **`brink-runtime`** (not `brink-format`). Reasoning: `StoryState` references `Program`, which is a `brink-runtime` type built by the linker from `StoryData`. The trait's methods operate on runtime-specific concepts that only exist after linking.

The trait absorbs the `R: StoryRng` generic by making RNG operations methods on the trait itself:

```rust
pub trait StoryState {
    // ── Program access ──────────────────────────────────────────
    fn program(&self) -> &Program;

    // ── Globals ─────────────────────────────────────────────────
    fn global(&self, idx: u32) -> &Value;
    fn global_mut(&mut self, idx: u32) -> &mut Value;

    // ── Visit / turn tracking ───────────────────────────────────
    fn visit_count(&self, id: DefinitionId) -> u32;
    fn increment_visit(&mut self, id: DefinitionId);
    fn turn_count(&self, id: DefinitionId) -> Option<u32>;
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32);
    fn turn_index(&self) -> u32;
    fn increment_turn_index(&mut self);

    // ── RNG ─────────────────────────────────────────────────────
    fn rng_seed(&self) -> i32;
    fn set_rng_seed(&mut self, seed: i32);
    fn previous_random(&self) -> i32;
    fn set_previous_random(&mut self, val: i32);
    /// Generate the next random int using the implementation's RNG algorithm.
    fn next_random(&mut self, seed: i32) -> i32;
}
```

## 2. vm.rs Refactor

Signature changes from:

```rust
pub(crate) fn step<R: StoryRng>(
    flow: &mut Flow,
    context: &mut Context<R>,
    stats: &mut Stats,
    program: &Program,
) -> Result<Stepped, RuntimeError>
```

to:

```rust
pub(crate) fn step(
    flow: &mut Flow,
    state: &mut impl StoryState,
    stats: &mut Stats,
) -> Result<Stepped, RuntimeError>
```

### Program access sites (replace `program.` with `state.program().`)

1. `program.container(pos.container_idx)` — resolve current container
2. `resolve_line(program, &pos, idx)` — EmitLine
3. `resolve_line(program, &pos, idx)` — EvalLine
4. `value_ops::stringify(&val, program)` — EmitValue
5. `program.resolve_container(id)` — EnterContainer
6. `program.container(idx)` — EnterContainer counting flags
7. Inside `goto_target` — Goto
8. `program.name(...)` — PushString
9. `program.list_literal(idx)` — PushList
10. `binary(flow, program, ...)` — all arithmetic ops (6 calls)
11. `binary(flow, program, ...)` — all comparison ops (6 calls)
12. `binary(flow, program, ...)` — And/Or
13. `program.resolve_global(id)` — GetGlobal
14. `program.resolve_global(id)` — SetGlobal
15. `program.resolve_global(target_id)` — SetTemp pointer write-through
16. `program.resolve_global(target_id)` — GetTemp pointer dereference
17. `binary(flow, program, ...)` — Pow/Min/Max
18. `program.resolve_container(id)` — Call
19. `program.container(idx)` — Call counting flags
20. `program.resolve_container(id)` — TunnelCall
21. `program.container(idx)` — TunnelCall counting flags
22. `program.resolve_container(id)` — ThreadCall
23. `program.resolve_container(id)` — TunnelCallVariable
24. `program.container(idx)` — TunnelCallVariable counting flags
25. `program.resolve_container(id)` — CallVariable
26. `program.container(idx)` — CallVariable counting flags
27. `program.resolve_target(id)` — TunnelReturn
28. `handle_begin_choice(...)` — BeginChoice
29. `program.container(...)` — CurrentVisitCount
30. `list_ops::list_*(flow, program)` — all list ops taking program
31. `resolve_line(program, ...)` — helper
32. `binary(flow, program, ...)` — helper
33. `goto_target(flow, context, program, id)` — helper
34. `handle_begin_choice(flow, context, stats, program, ...)` — helper
35. `handle_sequence(flow, context, program, kind, count)` — helper
36. `handle_shuffle_sequence(flow, context, program)` — helper
37. `program.container(pos.container_idx).path_hash` — shuffle hash

### Context access sites (replace with `state.*` method calls)

**Visit counts (read):**
1. `context.visit_counts.get(&id)` — VisitCount opcode
2. `context.visit_counts.get(&id)` — CurrentVisitCount opcode
3. `context.visit_counts.get(&target_id)` — handle_begin_choice once-only check
4. `context.visit_counts.get(&id)` — handle_sequence

**Visit counts (write):**
5. `*context.visit_counts.entry(id).or_insert(0) += 1` — EnterContainer
6. Same — Call
7. Same — TunnelCall
8. Same — TunnelCallVariable
9. Same — CallVariable
10. Same — goto_target

**Turn counts (read):**
11. `context.turn_counts.get(&id)` — TurnsSince

**Turn counts (write):**
12. `context.turn_counts.insert(id, context.turn_index)` — EnterContainer
13. Same — Call/TunnelCall/TunnelCallVariable/CallVariable
14. Same — goto_target

**Turn index (read):**
15. `context.turn_index` — TurnsSince delta
16. `context.turn_index` — TurnIndex opcode

**Globals (read):**
17. `context.globals[idx as usize].clone()` — GetGlobal
18. `context.globals[idx as usize]` — SetGlobal origin preservation check
19. `context.globals[global_idx as usize].clone()` — GetTemp pointer deref

**Globals (write):**
20. `context.globals[idx as usize] = val` — SetGlobal
21. `context.globals[global_idx as usize] = val` — SetTemp pointer write-through

**RNG:**
22. `context.rng_seed`, `context.previous_random`, setting `context.previous_random` — Random opcode
23. `context.rng_seed = seed; context.previous_random = 0` — SeedRandom
24. `context.rng_seed` — shuffle sequence seed

### Helper function signature changes

```rust
// Before → After
goto_target<R: StoryRng>(flow, context, program, id)
    → goto_target(flow, state: &mut impl StoryState, id)

handle_begin_choice<R: StoryRng>(flow, context, stats, program, flags, target_id)
    → handle_begin_choice(flow, state: &mut impl StoryState, stats, flags, target_id)

handle_sequence<R: StoryRng>(flow, context, program, kind, count)
    → handle_sequence(flow, state: &mut impl StoryState, kind, count)

handle_shuffle_sequence<R: StoryRng>(flow, context, program)
    → handle_shuffle_sequence(flow, state: &mut impl StoryState)

binary(flow, program, op)
    → binary(flow, state: &impl StoryState, op)  // only needs & for program access

resolve_line(program, pos, idx)
    → unchanged, pass state.program() at call site
```

### list_ops.rs changes

`list_random<R: StoryRng>(flow, context)` → `list_random(flow, state: &mut impl StoryState)` — only list op that touches context (for RNG). All other list ops take `(flow, program)` — pass `state.program()` at call site.

### value_ops.rs changes

`stringify(v, program)` and `binary_op(op, left, right, program)` take `&Program`. No signature change — call sites pass `state.program()`.

## 3. story.rs Refactor

### Context drops the R generic

```rust
pub(crate) struct Context {
    pub globals: Vec<Value>,
    pub visit_counts: HashMap<DefinitionId, u32>,
    pub turn_counts: HashMap<DefinitionId, u32>,
    pub turn_index: u32,
    pub rng_seed: i32,
    pub previous_random: i32,
}
```

### FlowInstance drops the R generic

```rust
pub(crate) struct FlowInstance {
    pub(crate) flow: Flow,
    pub(crate) context: Context,
    pub(crate) status: StoryStatus,
    pub(crate) stats: Stats,
}
```

### Story stores program reference, keeps R generic only on Story

```rust
pub struct Story<'p, R: StoryRng = FastRng> {
    program: &'p Program,
    default: FlowInstance,
    instances: HashMap<String, FlowInstance>,
    _rng: PhantomData<R>,
}
```

### New public API

```rust
impl<'p, R: StoryRng> Story<'p, R> {
    pub fn new(program: &'p Program) -> Self;
    pub fn step(&mut self) -> Result<StepResult, RuntimeError>;
    pub fn step_with(&mut self, handler: &dyn ExternalFnHandler) -> Result<StepResult, RuntimeError>;
    pub fn choose(&mut self, index: usize) -> Result<(), RuntimeError>;
    pub fn spawn_flow(&mut self, name: &str, entry_point: DefinitionId) -> Result<(), RuntimeError>;
    pub fn step_flow(&mut self, name: &str) -> Result<StepResult, RuntimeError>;
    pub fn step_flow_with(&mut self, name: &str, handler: &dyn ExternalFnHandler) -> Result<StepResult, RuntimeError>;
    // etc.
}
```

### FlowInstance::step_with — destructuring for borrow checker

```rust
fn step_with_state(
    &mut self,
    program: &Program,
    handler: &dyn ExternalFnHandler,
) -> Result<StepResult, RuntimeError> {
    let Self { flow, context, status, stats } = self;
    let mut state = RuntimeState::new(program, context);
    loop {
        let stepped = vm::step(flow, &mut state, stats)?;
        match stepped {
            // ... use flow, status, stats directly (not through self)
        }
    }
}
```

Destructuring creates separate borrows for each field, avoiding the conflict between `&mut self.context` (borrowed by state) and `self.flow`/`self.stats`.

## 4. RuntimeState Implementation

```rust
pub(crate) struct RuntimeState<'a, R: StoryRng = FastRng> {
    program: &'a Program,
    context: &'a mut Context,
    _rng: PhantomData<R>,
}

impl<R: StoryRng> StoryState for RuntimeState<'_, R> {
    #[inline]
    fn program(&self) -> &Program { self.program }

    #[inline]
    fn global(&self, idx: u32) -> &Value { &self.context.globals[idx as usize] }

    #[inline]
    fn global_mut(&mut self, idx: u32) -> &mut Value { &mut self.context.globals[idx as usize] }

    #[inline]
    fn visit_count(&self, id: DefinitionId) -> u32 {
        self.context.visit_counts.get(&id).copied().unwrap_or(0)
    }

    #[inline]
    fn increment_visit(&mut self, id: DefinitionId) {
        *self.context.visit_counts.entry(id).or_insert(0) += 1;
    }

    #[inline]
    fn turn_count(&self, id: DefinitionId) -> Option<u32> {
        self.context.turn_counts.get(&id).copied()
    }

    #[inline]
    fn set_turn_count(&mut self, id: DefinitionId, turn: u32) {
        self.context.turn_counts.insert(id, turn);
    }

    #[inline]
    fn turn_index(&self) -> u32 { self.context.turn_index }

    #[inline]
    fn increment_turn_index(&mut self) { self.context.turn_index += 1; }

    #[inline]
    fn rng_seed(&self) -> i32 { self.context.rng_seed }

    #[inline]
    fn set_rng_seed(&mut self, seed: i32) { self.context.rng_seed = seed; }

    #[inline]
    fn previous_random(&self) -> i32 { self.context.previous_random }

    #[inline]
    fn set_previous_random(&mut self, val: i32) { self.context.previous_random = val; }

    #[inline]
    fn next_random(&mut self, seed: i32) -> i32 {
        let mut rng = R::from_seed(seed);
        rng.next_int()
    }
}
```

All methods are trivial inline delegations — zero overhead vs current direct field access.

## 5. Clone Derives Needed

**Already have Clone:** Stats, StoryStatus, CallFrame, CallStack, Thread, PendingChoice, OutputPart, OutputBuffer.

**Need Clone added:**

| Type | Why it works |
|------|-------------|
| `Flow` | All fields: `Vec<Thread>`, `Vec<Value>`, `OutputBuffer`, `Vec<PendingChoice>`, `Vec<String>`, `bool`, `bool` — all Clone |
| `Context` | All fields: `Vec<Value>`, `HashMap<DefinitionId, u32>`, `u32`, `i32` — all Clone |
| `FlowInstance` | All fields: Flow, Context, StoryStatus, Stats — all Clone after above |

`Story` can also derive Clone: `&'p Program` (Copy), `FlowInstance` (Clone), `HashMap<String, FlowInstance>` (Clone).

## 6. brink-test-harness Crate

### Location and structure

```
crates/internal/brink-test-harness/
├── Cargo.toml
└── src/
    ├── lib.rs          -- public exports
    ├── tracked.rs      -- TrackedState, StateWrite
    ├── episode.rs      -- Episode, StepRecord types
    ├── explorer.rs     -- StoryExplorer, branch exploration
    └── diff.rs         -- Episode comparison, structural diffs
```

Auto-included by workspace glob `"crates/internal/*"`.

### TrackedState (`tracked.rs`)

```rust
pub struct TrackedState<'a, R: StoryRng = FastRng> {
    inner: RuntimeState<'a, R>,
    writes: Vec<StateWrite>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StateWrite {
    SetGlobal { idx: u32, old: Value, new: Value },
    IncrementVisit { id: DefinitionId, new_count: u32 },
    SetTurnCount { id: DefinitionId, turn: u32 },
    IncrementTurnIndex { new_value: u32 },
    SetRngSeed { old: i32, new: i32 },
    SetPreviousRandom { old: i32, new: i32 },
}
```

Implements `StoryState` by delegating reads to `inner`, intercepting writes:

```rust
impl<R: StoryRng> StoryState for TrackedState<'_, R> {
    fn set_global(&mut self, idx: u32, value: Value) {
        let old = self.inner.global(idx).clone();
        self.inner.set_global(idx, value.clone());
        self.writes.push(StateWrite::SetGlobal { idx, old, new: value });
    }
    // delegate everything else, intercept all mutations
}
```

**Visibility requirement:** `StoryState` trait and `RuntimeState` must be public in `brink-runtime`. `Context` needs a public constructor: `Context::new(program: &Program) -> Self`.

### Episode types (`episode.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub entry_point: Option<String>,  // None = root, Some = knot name
    pub steps: Vec<StepRecord>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    pub text: String,
    pub tags: Vec<Vec<String>>,
    pub writes: Vec<StateWrite>,
    pub externals: Vec<ExternalCall>,
    pub step_outcome: StepOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepOutcome {
    Continue,
    Choices {
        presented: Vec<ChoiceRecord>,
        selected: usize,
    },
    Done,
    Ended,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceRecord {
    pub text: String,
    pub index: usize,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalCall {
    pub name: String,
    pub args: Vec<Value>,
    pub returned: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ended,
    Done,
    Cycle { step_index: usize },
    DepthLimit,
    Error(String),
}
```

### StoryExplorer (`explorer.rs`)

```rust
pub struct StoryExplorer {
    max_depth: usize,
    max_episodes: usize,
}

impl StoryExplorer {
    pub fn new() -> Self;
    pub fn with_max_depth(self, depth: usize) -> Self;
    pub fn with_max_episodes(self, max: usize) -> Self;

    /// Explore all branches of a story, returning all episodes.
    pub fn explore(&self, program: &Program) -> Vec<Episode>;

    /// Replay a story with a specific choice sequence.
    pub fn replay(&self, program: &Program, choices: &[usize]) -> Episode;
}
```

**Exploration algorithm (DFS with cloning):**

1. Create initial `Story` from `Program`.
2. Step until yield point.
3. If `Done` or `Ended`, record episode and return.
4. If `Choices`, for each choice index `i`:
   a. Clone the current `Story`.
   b. Call `clone.choose(i)`.
   c. Recursively explore the clone.
5. Cycle detection: hash `(turn_index, visit_counts, current_container_idx, current_offset)` after each choice. Track in `HashSet<u64>` per branch. If repeated, terminate with `Outcome::Cycle`.

### Episode diff (`diff.rs`)

```rust
#[derive(Debug)]
pub struct EpisodeDiff {
    pub steps: Vec<StepDiff>,
    pub outcome_match: bool,
}

#[derive(Debug)]
pub enum StepDiff {
    Match,
    TextDiff { expected: String, actual: String },
    TagsDiff { expected: Vec<Vec<String>>, actual: Vec<Vec<String>> },
    WritesDiff { expected: Vec<StateWrite>, actual: Vec<StateWrite> },
    OutcomeDiff { expected: StepOutcome, actual: StepOutcome },
    Extra(StepRecord),
    Missing(StepRecord),
}

pub fn diff_episodes(expected: &Episode, actual: &Episode) -> EpisodeDiff;
```

## 7. Migration Path

### Phase 1: Internal refactor (no public API change)

1. Add `Clone` derives to `Flow`, `Context` (now non-generic).
2. Define `StoryState` trait in `brink-runtime`.
3. Create `RuntimeState` implementing `StoryState`.
4. Refactor `vm::step` and all helpers to take `&mut impl StoryState`.
5. Refactor `list_ops::list_random` to take `&mut impl StoryState`.
6. `FlowInstance::step_with` constructs `RuntimeState` internally.
7. **All existing tests continue to compile and pass** — public API unchanged.

### Phase 2: Simplify public API

1. Add `program: &'p Program` field to `Story`.
2. Change `Story::new(program)` to store the reference.
3. Remove `program` parameter from `step`, `step_with`, `spawn_flow`, `step_flow`, `step_flow_with`, `invoke_fallback`.
4. **Update all tests** — mechanical find-and-replace:
   - `story.step(&program)` → `story.step()`
   - `story.step_with(&program, &handler)` → `story.step_with(&handler)`
   - `story.spawn_flow("name", id, &program)` → `story.spawn_flow("name", id)`
5. Update `brink-runtime` public exports.

### Phase 3: Add brink-test-harness

1. Add crate to workspace.
2. Implement `TrackedState`, `Episode`, `StoryExplorer`, diff types.
3. Write tests for the harness itself.

## 8. Risk Areas

1. **Borrow checker with destructured FlowInstance.** The `step_with` loop accesses `self.flow`, `self.context` (via state), `self.stats`, and `self.status`. The `select_choice` method currently takes `&mut self`. Needs to be inlined or take individual fields. Highest-risk refactor point.

2. **`SetGlobal` origin preservation.** Current code reads old global value and conditionally modifies new value before writing. With trait: read via `state.global(idx)`, clone old origins, then write via `state.global_mut(idx)`. Need to clone old origins before mutable borrow.

3. **`Story` lifetime parameter propagation.** Adding `'p` to `Story` means every function holding a `Story` needs the lifetime. Check if any external crate stores `Story` in a struct.

4. **Clone performance for StoryExplorer.** Cloning `Story` at each choice point clones all `HashMap`s, `Vec`s. `CallStack`'s `Rc<[CallFrame]>` shared prefix helps. For initial implementation this is fine; optimization later.

5. **Monomorphization.** Every function taking `impl StoryState` is monomorphized per concrete type. Not a regression vs current `<R: StoryRng>` — just changes what gets monomorphized.

6. **Visibility for brink-test-harness.** `StoryState` trait and `RuntimeState` must be public in `brink-runtime`. `Context` needs public constructor.

## Critical Files

| File | Role |
|------|------|
| `crates/brink-runtime/src/vm.rs` | Core refactor: ~38 program access sites, ~24 context access sites |
| `crates/brink-runtime/src/story.rs` | FlowInstance, Context, Story struct changes; remove R generic from Context/FlowInstance |
| `crates/brink-runtime/src/list_ops.rs` | `list_random` signature change |
| `crates/brink-runtime/src/lib.rs` | Public exports: expose StoryState, Context, RuntimeState |
| `crates/brink-runtime/src/rng.rs` | Add Clone derives to FastRng, DotNetRng |
| `crates/brink-runtime/src/program.rs` | No changes (already immutable, passed by ref) |
