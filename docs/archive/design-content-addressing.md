# Design: Unified Content Addressing

> Status: implemented
>
> Note: `Container.address_id` was removed entirely (replaced by `labeled: bool`)
> rather than made non-optional as originally proposed, because `container.id`
> already serves as the sole address identifier for all containers.
> `resolve_container()` was removed from the runtime; all call sites use
> `resolve_target()` exclusively.

## Problem

The runtime has two addressing mechanisms for navigable positions in a story:

1. **Container IDs** (tag `0x01`) — resolve via `container_map` to `(container_idx, offset 0)`
2. **Label IDs** (tag `0x06`) — resolve via `label_map` to `(container_idx, byte_offset)`

`resolve_target` tries one map, then the other. `goto_target` fires visit tracking for whichever ID the caller happened to pass. The result is that the same logical destination can have different visit tracking behavior depending on which ID was used. This is the source of a class of episode mismatches where the compiler and converter produce different write counts for visit tracking.

### How it manifests

In inklecate's model, a named gather like `- (loop)` isn't its own container — it's a *position* within a parent container. Inklecate assigns it a label. The converter mirrors this: named gathers and choice bodies that are divert targets within their parent get label IDs.

The brink compiler, working from the AST, gives every gather and choice body its own `Container`. These containers have a `label_id` field in the LIR, but codegen ignores it — `StoryData.labels` is hardcoded to `Vec::new()`. The result: the compiler never emits labels, visit tracking for labels never fires, and episode writes diverge.

## Proposal: unified content addresses

Both `DefinitionTag::Container` (`0x01`) and `DefinitionTag::Label` (`0x06`) are replaced by a single tag: **`DefinitionTag::Address`** (`0x01`). All navigable positions — containers, named gathers, labeled choices, intra-container offsets — are identified by address IDs.

### Core principle

A **container** is a structural concept: a chunk of bytecode with its own instruction stream. It answers the question "where is the code?"

An **address** is a navigation concept: a position you can navigate to. It answers the question "where can I go?" Every container has a primary address (offset 0). Named gathers, labeled choices, and other intra-container positions have additional addresses (offset > 0).

All navigation — diverts, gotos, choice targets, tunnel calls, function calls — uses address IDs. All visit tracking uses address IDs. The opcode semantics determine what happens when you arrive (push a call frame, enter a container, etc.); the address determines *where*.

### Naming

| Old | New |
|-----|-----|
| `DefinitionTag::Container` (`0x01`) | `DefinitionTag::Address` (`0x01`) |
| `DefinitionTag::Label` (`0x06`) | Removed |
| `LabelDef` | `AddressDef` |
| `label_map` | `address_map` |
| `label_id` (field) | `address_id` |
| `container_map` | Removed |
| `resolve_container()` | Removed |
| `container_id()` (path helper) | `address_id()` |
| `DivertTarget::Container(id)` | `DivertTarget::Address(id)` |

### What changes

#### `ContainerDef`

The `id` field becomes Address-tagged (`0x01`). This is the container's sole identifier — used for addressing, navigation, and visit tracking.

```
ContainerDef {
    id: DefinitionId,          // Address-tagged (0x01), sole identifier
    bytecode: Vec<u8>,
    content_hash: u64,
    counting_flags: CountingFlags,
    path_hash: i32,
}
```

#### `AddressDef` (formerly `LabelDef`)

Renamed. Every container emits an `AddressDef` for its primary address:

```
AddressDef {
    id: DefinitionId,            // Address-tagged
    container_id: DefinitionId,  // Address-tagged (same value for primary addresses)
    byte_offset: 0,              // primary address = start of container
}
```

Named gathers and intra-container positions also emit `AddressDef`s with `byte_offset > 0`. For these, `container_id` is the parent container's `id`.

#### `ContainerLineTable`

`container_id` becomes Address-tagged (same value as `ContainerDef.id`).

#### `ExternalDef`

`fallback: Option<DefinitionId>` becomes Address-tagged. `invoke_fallback()` resolves via `address_map` instead of `resolve_container()`.

#### `LinkedContainer`

```
LinkedContainer {
    id: DefinitionId,          // Address-tagged (sole identifier)
    bytecode: ...,
    counting_flags: ...,
    path_hash: ...,
}
```

#### Linker (`brink-runtime/src/linker.rs`)

`container_map` is eliminated. `address_map` becomes the sole resolution map. Every container's primary address is registered in `address_map` with offset 0. Intra-container addresses (offset > 0) are registered as today. The `containers: Vec<LinkedContainer>` table is indexed by position; `address_map` entries point into it.

#### Runtime: resolution

`resolve_target(id)` becomes the single resolution path. It checks `address_map` only. `resolve_container(id)` is eliminated.

#### Runtime: opcodes

All opcodes that take navigation targets use Address-tagged IDs:

| Opcode | Current ID | New ID | Notes |
|--------|-----------|--------|-------|
| `Goto(id)` | Container or Label | Address | Single resolution path |
| `GotoIf(id)` | Container or Label | Address | Same |
| `Call(id)` | Container | Address | Function call; resolves → `(container_idx, 0)` |
| `TunnelCall(id)` | Container | Address | Same as Call |
| `ThreadStart(id)` | Container | Address | Same |
| `EnterContainer(id)` | Container | Address | Structural nesting; resolves → `container_idx` |
| `BeginChoice(flags, id)` | Container | Address | Choice target |

#### Runtime: visit tracking

All visit tracking uses address IDs:

| Site | Current behavior | New behavior |
|------|-----------------|--------------|
| `EnterContainer` | `increment_visit(container_id)` | `increment_visit(container.id)` (now Address-tagged) |
| `goto_target` | `increment_visit(passed_id)` | Unchanged (all IDs are Address-tagged) |
| `select_choice` | `increment_visit(target_id)` | Unchanged (target is an address ID) |
| `Call` | `increment_visit(container_id)` | `increment_visit(container.id)` (now Address-tagged) |
| `TunnelCall` | same | same |
| `CurrentVisitCount` | `visit_count(container.id)` | Unchanged (`container.id` is now Address-tagged) |
| `VisitCount` | pops `DivertTarget(id)` | Unchanged (value is an address ID) |
| `TurnsSince` | pops `DivertTarget(id)` | Unchanged |

#### Compiler codegen (`brink-codegen-inkb`)

1. `walk_container` uses the container's `address_id` (Address-tagged, hashed from the same path string used today)
2. Emits `AddressDef { id: address_id, container_id: address_id, byte_offset: 0 }` for each container
3. All opcode emission uses `address_id`
4. Containers with intra-container targets (named gathers, labeled choices) emit additional `AddressDef`s with `byte_offset > 0`

#### Converter codegen (`brink-converter`)

Same changes: `container_id()` path helper switches from `DefinitionTag::Container` to `DefinitionTag::Address`. Every container emits a primary `AddressDef`. The converter's existing intra-container address generation continues as-is but uses the `Address` tag. `index.resolve_target` returns address IDs for all cases.

#### LIR

`Container.label_id` becomes `Container.address_id` and changes from `Option<DefinitionId>` to `DefinitionId` (non-optional — every container has one). The planner assigns address IDs alongside container IDs during the migration, and container IDs are eliminated once complete.

`DivertTarget::Container(id)` becomes `DivertTarget::Address(id)`. All divert targets carry Address-tagged IDs.

### What doesn't change

- **Container table structure** — containers are still a vec of bytecode chunks. The table exists; it's just addressed by address IDs.
- **`enter_container` / `exit_container` semantics** — structural nesting is unchanged. You still push/pop container positions on the container stack.
- **Call frame semantics** — function calls still create frames with container positions.
- **Counting flags** — `VISITS`, `TURNS`, `COUNT_START_ONLY` remain on containers. The flags gate whether visit tracking fires; the address ID determines *what* gets tracked.
- **`path_hash`** — precomputed `i32` field on `ContainerDef`, implements inklecate's shuffle seed algorithm (`chars().map(|c| c as i32).sum()`). Cannot be derived from the address ID (different hash function).
- **`ContainerPosition`** — uses `container_idx: u32` (a positional index into the container vec, not a `DefinitionId`). Unaffected.

## Implementation steps

This is a single coordinated change across the format, runtime, converter, and compiler crates. Steps are ordered by dependency — each step's checkpoint must pass before proceeding.

### Step 1: Format — `DefinitionTag` and `DefinitionId`

**File:** `brink-format/src/id.rs`

- Replace `DefinitionTag::Container = 0x01` with `DefinitionTag::Address = 0x01`
- Remove `DefinitionTag::Label = 0x06`
- Update `Display`, `FromStr`, and serialization impls (`$01_` prefix stays, `$06_` goes)
- Update all match arms referencing either variant

This breaks every downstream crate that imports `Container` or `Label` tags — that's the forcing function.

### Step 2: Format — definition structs

**File:** `brink-format/src/definition.rs`

- Rename `LabelDef` → `AddressDef`
- Field `container_id` stays (it still points to the parent container's address ID)

**File:** `brink-format/src/story.rs`

- Rename `StoryData.labels: Vec<LabelDef>` → `StoryData.addresses: Vec<AddressDef>`

### Step 3: Format — serialization

**Files:**
- `brink-format/src/inkt/read.rs`
- `brink-format/src/inkt/write.rs`
- `brink-format/src/inkt/inkt.pest`
- `brink-format/src/inkb/read.rs`
- `brink-format/src/inkb/write.rs`

Update grammar keywords, read/write functions to use `AddressDef` and `addresses`. Tag `0x01` = Address, `0x06` no longer valid.

**Files:** `brink-format/tests/proptest_inkt.rs`, `brink-format/tests/proptest_inkb.rs`

Update `DefinitionTag::Container`/`Label` references in generators.

**Checkpoint:** `cargo check -p brink-format` compiles. `cargo test -p brink-format` passes.

### Step 4: Runtime — linker

**File:** `brink-runtime/src/linker.rs`

- Build `address_map: HashMap<DefinitionId, (u32, usize)>` instead of separate `container_map` + `label_map`
- Containers register as `address_map.insert(cdef.id, (idx, 0))` — primary address at offset 0
- `AddressDef`s register as `address_map.insert(addr.id, (container_idx, byte_offset))` — look up `container_idx` from `address_map` entry for `addr.container_id`
- Containers loop must run before addresses loop (already the case)

### Step 5: Runtime — Program struct

**File:** `brink-runtime/src/program.rs`

- Remove `container_map: HashMap<DefinitionId, u32>`
- Rename `label_map` → `address_map: HashMap<DefinitionId, (u32, usize)>`
- Remove `resolve_container()`
- `resolve_target()` — single lookup in `address_map`, no fallback chain

### Step 6: Runtime — VM and story

**File:** `brink-runtime/src/vm.rs`

- `EnterContainer` handler: resolve via `resolve_target`, use Address-tagged ID for visit tracking
- `goto_target`: already uses `resolve_target` — verify callers pass Address-tagged IDs
- `CurrentVisitCount`: `visit_count(container.id)` — `id` is now Address-tagged, works as-is
- `handle_shuffle_sequence`: positional (`container(pos.container_idx).path_hash`), no change

**File:** `brink-runtime/src/story.rs`

- `invoke_fallback()`: switch from `resolve_container()` to `resolve_target()`, extract `container_idx` from `(u32, usize)` tuple
- Any remaining `resolve_container` call sites → `resolve_target`

**Checkpoint:** `cargo check -p brink-runtime` compiles. Runtime unit tests pass.

### Step 7: Converter — path helpers

**File:** `brink-converter/src/path.rs`

- Rename `container_id(path)` → `address_id(path)`, change tag to `DefinitionTag::Address`
- Remove `label_id(path)` (or merge — same function, same tag now)

### Step 8: Converter — index

**File:** `brink-converter/src/index.rs`

- `register_container()`: use `path::address_id()` instead of `path::container_id()`
- `resolve_target()`: returns Address-tagged IDs for all cases
- `register_labels()` → `register_addresses()`: same logic, uses `address_id()`

### Step 9: Converter — codegen and lib

**File:** `brink-converter/src/codegen.rs`

- All container ID lookups now return Address-tagged IDs (automatic from step 8)
- `ContainerDef` creation: `id` is Address-tagged (automatic)
- All opcode emission: Address-tagged IDs (automatic)

**File:** `brink-converter/src/lib.rs`

- Rename `build_labels()` → `build_addresses()`
- Every container emits a primary `AddressDef { id: container.id, container_id: container.id, byte_offset: 0 }`
- Existing intra-container address generation continues, using Address tag

**Checkpoint:** `cargo check -p brink-converter` compiles. Regenerate golden episodes. Converter tests pass.

### Step 10: Compiler — LIR types

**File:** `brink-ir/src/lir/types.rs`

- `Container.label_id: Option<DefinitionId>` → `Container.address_id: DefinitionId` (non-optional)
- `DivertTarget::Container(DefinitionId)` → `DivertTarget::Address(DefinitionId)`
- Update all match arms across the LIR crate

### Step 11: Compiler — ID allocation and planning

**File:** `brink-ir/src/lir/lower/context.rs`

- `IdAllocator::alloc_container()` → `alloc_address()`, uses `DefinitionTag::Address`

**File:** `brink-ir/src/lir/lower/plan.rs`

- All container ID allocation switches to `alloc_address()`
- `Container.address_id` populated for every container

**File:** `brink-ir/src/lir/lower/mod.rs`

- `DivertTarget::Container(id)` → `DivertTarget::Address(id)` everywhere
- `container.label_id` → `container.address_id` everywhere

### Step 12: Compiler — bytecode codegen

**File:** `brink-codegen-inkb/src/lib.rs`

- Remove `labels: Vec::new()` — emit `AddressDef`s
- `walk_container`: emit `AddressDef { id: container.address_id, container_id: container.address_id, byte_offset: 0 }` for every container
- All opcode emission uses `container.address_id`

**File:** `brink-codegen-inkb/src/container.rs`

- `EnterContainer`, `Goto`, `Call`, etc. emission uses address IDs

**Checkpoint:** `cargo check --workspace` compiles. `cargo test --workspace` passes. Episode ratchet holds or improves.

### Step 13: Test harness

- Re-run full corpus, verify ratchet
- Tighten `diff.rs` to compare write *content*, not just vector length
- Bump ratchet if episodes improved

### Risk

Golden episodes must be regenerated after the converter changes (step 9). Until both pipelines are updated, cross-pipeline write comparison is unreliable (though it only checks lengths today, so it won't break). The migration should be done as a single coordinated change — all 13 steps in one branch, one commit per logical unit.

## Decided

1. **Container-tagged IDs are eliminated.** Address IDs are the universal identifier for all navigable positions. `DefinitionTag::Container` (`0x01`) becomes `DefinitionTag::Address` (`0x01`). `DefinitionTag::Label` (`0x06`) is removed.

2. **`path_hash` stays as a precomputed field.** It implements inklecate's specific shuffle seed algorithm and cannot be derived from the address ID (different hash function).

3. **`DivertTarget::Container` renamed to `DivertTarget::Address`.** All navigable targets are address IDs.
