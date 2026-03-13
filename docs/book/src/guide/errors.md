# Error Handling

All runtime operations that can fail return `Result<T, RuntimeError>`.

## RuntimeError variants

### Host errors

These indicate a bug in your code -- the host called the API incorrectly.

| Variant | When |
|---------|------|
| `InvalidChoiceIndex` | `choose()` called with an index outside the valid range |
| `NotWaitingForChoice` | `choose()` called when story isn't in `WaitingForChoice` status |
| `StoryEnded` | Tried to continue a story that has permanently ended |
| `UnknownFlow` | Referenced a named flow that doesn't exist |
| `FlowAlreadyExists` | Tried to spawn a flow with a name that's already active |
| `StepLimitExceeded` | Safety limit hit -- possible infinite loop in the story |

### Story errors

These indicate a problem in the ink source or an unsupported feature.

| Variant | When |
|---------|------|
| `TypeError` | Type mismatch in an ink expression (e.g., adding a string to a list) |
| `DivisionByZero` | Division or modulo by zero in an ink expression |
| `UnresolvedExternalCall` | Story calls an external function with no handler provided |
| `Unimplemented` | The story uses an opcode not yet supported by the VM |

### Internal errors

These typically indicate a compiler bug -- the bytecode is malformed.

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

Host errors are recoverable -- fix the calling code and retry. Story errors may be recoverable depending on context. Internal errors generally indicate broken bytecode and are not recoverable.
