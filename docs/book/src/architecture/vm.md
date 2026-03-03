# Bytecode VM

The runtime is a stack-based bytecode VM.

## Design properties

- Stack-based: operands on value stack
- Jump offsets are container-relative
- Cross-definition references use `DefinitionId` in the file format, resolved to compact indices at link time
- Short-circuit `and`/`or` handled by compiler (conditional jumps), not VM

## Value type

```
Int(i32) | Float(f32) | Bool(bool) | String | List | DivertTarget | Null
```

<!-- TODO: explain each value type and when it appears -->

## Opcode categories

<!-- TODO: expand each category with the actual opcodes:
  - Stack & literals
  - Arithmetic
  - Comparison & logic
  - Global variables
  - Temp variables
  - Control flow (jump, divert)
  - Container flow (enter/exit)
  - Functions & tunnels
  - Threads
  - Output (emit line, emit value, newline, glue, tags)
  - Choices (begin/end choice set, begin/end choice)
  - Sequences (cycle, stopping, once-only, shuffle)
  - Intrinsics (visit count, turns since, choice count, random)
  - External function calls
  - List operations
  - Lifecycle (done, end)
-->

## Execution model

<!-- TODO: explain call frames, container stacks, the step function,
     yield points, and how the VM suspends/resumes -->
