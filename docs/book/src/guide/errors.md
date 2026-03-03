# Error Handling

All runtime operations that can fail return `Result<T, RuntimeError>`.

## `RuntimeError` variants

| Variant | When |
|---------|------|
| `Decode` | Malformed bytecode |
| `UnresolvedDefinition` | Linker can't find a referenced definition |
| `NoRootContainer` | Story has no entry point |
| `StackUnderflow` | Value stack is empty when an operand is expected |
| `CallStackUnderflow` | No call frame to return to |
| `ContainerStackUnderflow` | No container position to pop |
| `InvalidChoiceIndex` | `choose()` called with out-of-range index |
| `NotWaitingForChoice` | `choose()` called when story isn't waiting |
| `StoryEnded` | `step()` called after story has permanently ended |
| `UnresolvedGlobal` | Global variable lookup failed |
| `TypeError` | Type mismatch in an operation |
| `DivisionByZero` | Division or modulo by zero |
| `Unimplemented` | Opcode not yet supported |
| `CaptureUnderflow` | Output capture stack mismatch |

<!-- TODO: guidance on which errors are "bugs in the story" vs "bugs in the host" -->
<!-- TODO: recovery strategies — which errors are recoverable? -->
