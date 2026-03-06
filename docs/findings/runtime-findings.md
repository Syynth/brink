# Runtime audit findings

Audit of `runtime-spec.md` against the `brink-runtime` and `brink-format` crates. Performed 2026-03-05.

## 1. Spec/implementation misalignments

Things where both the spec and code address the same concept but disagree.

### 1a. Position representation — symbolic vs resolved

**RESOLVED.** Spec updated to reflect resolved runtime indices. Decision logged: positions use `(u32, usize)` at runtime; translation to/from `DefinitionId` happens at reconciliation and save/load boundaries.

### 1b. CallFrame.return_address type

**RESOLVED.** Spec updated to `Option<ContainerPosition>` with `None` for Root frames.

### 1c. Named flows — shared vs independent globals

**RESOLVED.** Spec updated: variable scoping uses the `FLOW VAR` keyword at the ink source level instead of runtime registration. `VAR` is shared (default), `FLOW VAR` is per-instance. Decision logged.

### 1d. External function resolution API surface

**Spec is correct, implementation needs to catch up.** `resolve_external` and `invoke_fallback` should be public methods on Flow so callers can bypass Story.

### 1e. ExternalResult — no Pending variant

**Spec is correct, implementation needs to catch up.** `Pending` variant not yet implemented.

### 1f. Value type enum

**RESOLVED.** Format spec updated to include `VariablePointer(DefinitionId)` with `PushVarPointer`, `GetTempRaw` opcodes and write-through/auto-deref semantics on `SetTemp`/`GetTemp`. Flagged as **needs review** — semantics are implemented but not validated against the full ref parameter design.

### 1g. Definition tags

**RESOLVED.** Format spec updated to include `0x06 Label`.

## 2. Implementation features not covered by spec

Behaviors and mechanisms in the code with no corresponding spec text.

| Feature | Location | Resolution |
|---------|----------|------------|
| `BeginStringEval` / `EndStringEval` opcodes | `vm.rs` | **Needs review.** Must verify we're accounting for localization and not duplicating functionality with the line template system. |
| Copy-on-write `CallStack` (inherited `Rc<[CallFrame]>` + own `Vec`) | `story.rs` | **No spec needed.** Implementation detail — spec describes fork semantics, CoW is just the optimization. |
| `MAX_OPS_PER_STEP = 1_000_000` safety limit | `story.rs:468` | **Remove.** Vestige of monolithic `step()`. Now that `vm::step` does one opcode and Story loops, callers control the loop. |
| `Stats` struct (always-on counters) | `story.rs` | **Needs spec coverage** and a feature flag to compile it out. |
| `path_hash: i32` on `LinkedContainer` | `program.rs` | **Needs spec coverage** as part of sequence semantics documentation. |
| `skipping_choice` flag on `Flow` | `story.rs` / `vm.rs` | **Needs format spec coverage** in the choice opcode documentation — it's a bytecode contract detail. |
| Invisible default choice auto-selection | `story.rs` | **Needs runtime spec coverage** in the choice section. Matches reference ink behavior. |
| `clean_output_whitespace` | `output.rs` | **Needs runtime spec coverage** in the output buffer section. |
| `DotNetRng` | `rng.rs` | **Needs runtime spec coverage.** Spec should explain that RNG is pluggable via `StoryRng` trait, why, and list included implementations (`FastRng` default, `DotNetRng` for reference compat). |
| Output capture mechanism (`begin_capture`/`end_capture`/`Checkpoint`) | `output.rs` | **Needs thorough spec coverage.** Core VM machinery used by function return values, string eval, and tag capture. Spec mentions function output capture but doesn't explain the mechanism. |
| `smallvec` dependency | `Cargo.toml` | **RESOLVED.** Removed — was unused. |

## 3. Spec sections with no implementation

Spec concepts that have zero or stub-only implementation.

| Spec section | Status | Priority |
|-------------|--------|----------|
| **Hot-reload reconciliation** (§ Hot-reload reconciliation) | Nothing implemented. No reconciliation code exists. Includes `ReconcileReport`. | **High — implement soon.** |
| **Multi-instance management / `NarrativeRuntime`** (§ Multi-instance management) | **RESOLVED.** Spec updated — `Story` with named flows covers this role. No separate type needed. | N/A |
| **Voice acting** (§ Voice acting) | **RESOLVED.** Spec updated — replaced `TextOutput` with structured output types (`Span` with `audio_ref`, `Line`, `LineId`-internal model). See Public API types and Program composition sections. | N/A |
| **Locale overlay loading** (§ Locale overlay loading) | **RESOLVED.** Spec updated — `load_locale` with `Strict`/`Overlay` modes, `Program` composition (`LinkedBinary` + `LinkedLocale`), locale switching. No `.inkl` loading code yet. | **High — implement.** |
| **Save/load serialization** | Context comment says "(deferred)". No serialization code. Mostly involves making Context data cleanly marshallable in/out of a buffer for the host. | **Medium — blocked on Context design** (`FLOW VAR` shared/instance split). |
| **Line template resolution** (§ Output buffer + format-spec templates) | `resolve_line` returns `"[template]"` for all `LineContent::Template` — hard-coded stub. The entire `PluralResolver` / `LinePart::Select` / slot interpolation system is non-functional. Ties into `BeginStringEval`/`EndStringEval` review — need to understand how string eval and line templates interact for localization. | **Needs review and design.** |
| **`flow.resolve_external()` / `flow.invoke_fallback()`** as Flow methods | Resolution is Story-internal only. See §1d. | Spec is correct, implementation needs to catch up. |
| **`ExternalResult::Pending`** async path | Not implemented. See §1e. | Spec is correct, implementation needs to catch up. |

## 4. Stubs and dead code

| Item | Location | Resolution |
|------|----------|------------|
| `ListUnion` / `ListExcept` opcodes | `vm.rs:779` — return `Unimplemented`. Equivalent operations exist via `Add`/`Subtract` on lists in `value_ops`. | **Remove.** Redundant with binary operators. |
| `GlobalSlot.id` and `GlobalSlot.name` | `program.rs` | **RESOLVED.** Added reason to `#[expect]` — needed for save/load and debugging. |
| `PendingChoice.original_index` and `.output_line_idx` | `story.rs` | **RESOLVED.** Added reason to `#[expect]` — needs research, likely needed for structured output / voice acting. |
| `Thread.thread_index` and `Flow.thread_counter` | `story.rs` | **RESOLVED.** Removed — unused and unclear purpose. |
| `OutputBuffer::flush` | `output.rs` — `#[cfg(test)]` only | **No action.** Test helper. |
