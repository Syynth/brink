# Error Handling

All runtime operations that can fail return `Result<T, RuntimeError>`.

## RuntimeError variants

### Host errors

These indicate a bug in your code — the host called the API incorrectly.

| Variant | When |
|---------|------|
| `InvalidChoiceIndex` | `choose()` called with an index outside the valid range |
| `NotWaitingForChoice` | `choose()` called when story isn't in `WaitingForChoice` status |
| `StoryEnded` | Tried to continue a story that has permanently ended |
| `UnknownFlow` | Referenced a named flow that doesn't exist |
| `FlowAlreadyExists` | Tried to spawn a flow with a name that's already active |

### Function evaluation & host-directed entry

Raised by the engine→ink direction: calling an ink function from host code
(`call_function`, `begin_function_eval`), and jumping the story to a path
(`choose_path_string`). See [Runtime API](./runtime-api.md).

| Variant | When |
|---------|------|
| `FunctionNotFound` | `call_function` named something that isn't a function or knot |
| `ArgCountMismatch` | Wrong number of arguments for the target's declared parameters |
| `FunctionYielded` | A host-called function tried to present choices or end the story |
| `AlreadyEvaluatingFunction` | A function evaluation is already in progress on this flow |
| `NotEvaluatingFunction` | `resume_function_eval` called with no evaluation in progress |
| `AsyncExternalInCall` | A function called from the synchronous `call_function` path hit a deferred external |
| `UnknownPath` | `choose_path_string` given a path matching no knot, stitch, or label |
| `JumpWhileAwaitingExternal` | Tried to jump while the flow is parked on an unresolved external |

A function evaluated from host code must run to a return value. `FunctionYielded`
means the ink you called wanted to become the story — present choices, or hit
`-> END` — which the isolated evaluation path cannot honor.

### Safety limits

The VM caps anything that accumulates, so malformed bytecode or a runaway loop
in the story fails loudly instead of hanging.

| Variant | When |
|---------|------|
| `StepLimitExceeded` | Opcode budget exhausted — likely an infinite loop in the story |
| `LineLimitExceeded` | A single turn produced more lines than `continue_maximally` allows |

### Story errors

These indicate a problem in the ink source or an unsupported feature.

| Variant | When |
|---------|------|
| `TypeError` | Type mismatch in an ink expression (e.g., adding a string to a list) |
| `DivisionByZero` | Division or modulo by zero in an ink expression |
| `UnresolvedExternalCall` | Story calls an external function with no handler provided |
| `RanOutOfContent` | Execution fell off the end of a knot — usually a missing `-> DONE` or `-> END` |
| `Unimplemented` | The story uses an opcode not yet supported by the VM |

### Locale errors

Raised by `apply_locale()` when a `.inkl` overlay doesn't match the program it's
applied to. See [Localization](../localization/overview.md).

| Variant | When |
|---------|------|
| `LocaleChecksumMismatch` | The overlay was compiled against different bytecode — recompile it |
| `LocaleScopeNotInBase` | The overlay carries a scope the base program doesn't have |
| `LocaleScopeMissing` | `LocaleMode::Strict` and the overlay omits a scope the base requires |

### Internal errors

These typically indicate a compiler bug — the bytecode is malformed.

| Variant | When |
|---------|------|
| `Decode` | Corrupt or incompatible `.inkb` file |
| `UnresolvedDefinition` | Linker can't find a referenced definition |
| `NoRootContainer` | Story has no entry point |
| `StackUnderflow` | Value stack empty when an operand was expected |
| `CallStackUnderflow` | No call frame to return to |
| `ContainerStackUnderflow` | No container to pop from the container stack |
| `UnresolvedGlobal` | Global variable lookup failed |
| `CaptureUnderflow` | Output capture stack mismatch |

## Recovery

Host errors are recoverable — fix the calling code and retry. Function-evaluation
and locale errors are recoverable in the same sense: the story state is
untouched, so correct the call (or recompile the overlay) and try again. Story
errors may be recoverable depending on context.

Safety-limit errors abort partway through a turn, leaving the story mid-step;
treat the instance as spent and restart it from a snapshot rather than
continuing. Internal errors generally indicate broken bytecode and are not
recoverable.
