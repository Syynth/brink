# Bytecode VM

The runtime is a stack-based bytecode VM.

## Design properties

- Stack-based: operands pushed/popped from a value stack
- Jump offsets are container-relative
- Cross-definition references use `DefinitionId`, resolved to compact indices at link time
- Short-circuit `and`/`or` compiled to conditional jumps, not handled by the VM

## Value types

```rust,ignore
enum Value {
    Int(i32),
    Float(f32),
    Bool(bool),
    String(Rc<str>),               // Refcounted for cheap cloning
    List(Rc<ListValue>),           // Refcounted
    DivertTarget(DefinitionId),    // Target address for diverts
    VariablePointer(DefinitionId), // Reference to a global variable
    TempPointer { slot, frame_depth }, // Reference to a local variable
    Null,
}
```

`String` and `List` are `Rc`-wrapped so cloning is O(1), matching C# reference semantics and making call-frame forking cheap.

## Opcode reference

The VM executes 70+ opcodes. Each opcode is encoded as a single discriminant byte followed by zero or more operand bytes.

### Stack and literals

| Opcode | Operands | Description |
|--------|----------|-------------|
| `PushInt` | `i32` | Push an integer constant |
| `PushFloat` | `f32` | Push a float constant |
| `PushBool` | `u8` | Push a boolean (0 = false, 1 = true) |
| `PushString` | `u16` | Push a string by line table index |
| `PushList` | `u16` | Push a list literal by index |
| `PushDivertTarget` | `DefinitionId` | Push a divert target address |
| `PushNull` | -- | Push null |
| `Pop` | -- | Discard the top value |
| `Duplicate` | -- | Duplicate the top value |

### Arithmetic

| Opcode | Description |
|--------|-------------|
| `Add` | Pop two values, push their sum (also concatenates strings) |
| `Subtract` | Pop two values, push their difference |
| `Multiply` | Pop two values, push their product |
| `Divide` | Pop two values, push their quotient |
| `Modulo` | Pop two values, push the remainder |
| `Negate` | Pop one value, push its negation |

### Comparison

| Opcode | Description |
|--------|-------------|
| `Equal` | Pop two values, push whether they are equal |
| `NotEqual` | Pop two values, push whether they differ |
| `Greater` | Pop two values, push whether left > right |
| `GreaterOrEqual` | Pop two values, push whether left >= right |
| `Less` | Pop two values, push whether left < right |
| `LessOrEqual` | Pop two values, push whether left <= right |

### Logic

| Opcode | Description |
|--------|-------------|
| `Not` | Pop one value, push its logical negation |
| `And` | Pop two values, push logical AND |
| `Or` | Pop two values, push logical OR |

### Variables

| Opcode | Operands | Description |
|--------|----------|-------------|
| `GetGlobal` | `DefinitionId` | Push the value of a global variable |
| `SetGlobal` | `DefinitionId` | Pop a value and assign it to a global variable |
| `DeclareTemp` | `u16` (slot) | Declare a temp variable in the current frame |
| `GetTemp` | `u16` (slot) | Push the value of a temp (auto-dereferences pointers) |
| `SetTemp` | `u16` (slot) | Pop a value and assign it to a temp slot |
| `GetTempRaw` | `u16` (slot) | Push a temp's raw value without auto-dereference |
| `PushVarPointer` | `DefinitionId` | Push a pointer to a global variable |
| `PushTempPointer` | `u16` (slot) | Push a pointer to a temp variable |

### Control flow

| Opcode | Operands | Description |
|--------|----------|-------------|
| `Jump` | `i32` (offset) | Unconditional relative jump within the current container |
| `JumpIfFalse` | `i32` (offset) | Pop a value; jump if falsy |
| `Goto` | `DefinitionId` | Absolute jump to a named address |
| `GotoIf` | `DefinitionId` | Pop a value; goto the address if truthy |
| `GotoVariable` | -- | Pop a `DivertTarget` from the stack and goto it |

### Container flow

| Opcode | Operands | Description |
|--------|----------|-------------|
| `EnterContainer` | `DefinitionId` | Push a container onto the container stack (updates visit counts) |
| `ExitContainer` | -- | Pop the current container from the container stack |

### Functions and tunnels

| Opcode | Operands | Description |
|--------|----------|-------------|
| `Call` | `DefinitionId` | Call a function -- pushes a new call frame with fresh temp storage |
| `Return` | -- | Return from a function call |
| `TunnelCall` | `DefinitionId` | Tunnel into a knot -- pushes a return address, shares the output stream |
| `TunnelReturn` | -- | Return from a tunnel |
| `TunnelCallVariable` | -- | Pop a `DivertTarget` and tunnel to it |
| `CallVariable` | -- | Pop a `DivertTarget` and call it as a function |

### Threads

| Opcode | Operands | Description |
|--------|----------|-------------|
| `ThreadCall` | `DefinitionId` | Fork execution to explore a choice branch |
| `ThreadStart` | -- | Mark the beginning of a forked thread's code |
| `ThreadDone` | -- | Mark the end of a forked thread |

Thread forking clones the current VM state (call stack, variable state) to explore choice branches in isolation. Each choice's thread is evaluated independently to determine its display text and conditions.

### Output

| Opcode | Operands | Description |
|--------|----------|-------------|
| `EmitLine` | `u16` (index) | Emit a line from the container's line table |
| `EmitValue` | -- | Pop a value and emit its string representation |
| `EmitNewline` | -- | Emit a newline character |
| `Glue` | -- | Suppress the previous newline (joins lines) |
| `BeginTag` | -- | Begin capturing tag content |
| `EndTag` | -- | End tag capture and attach to current output |
| `EvalLine` | `u16` (index) | Evaluate an interpolated line template |

### Choices

| Opcode | Operands | Description |
|--------|----------|-------------|
| `BeginChoice` | `flags: u8`, `DefinitionId` | Begin a choice with flags and a target address |
| `EndChoice` | -- | Finalize the current choice |

`BeginChoice` flags (packed into a single byte):
- Bit 0: `has_condition` -- choice has a conditional guard
- Bit 1: `has_start_content` -- choice has text before `[`
- Bit 2: `has_choice_only_content` -- choice has text inside `[]`
- Bit 3: `once_only` -- choice can only be selected once
- Bit 4: `is_invisible_default` -- fallback choice when no others are available

### Sequences

| Opcode | Operands | Description |
|--------|----------|-------------|
| `Sequence` | `kind: u8`, `count: u8` | Begin a sequence (kind: 0=cycle, 1=stopping, 2=once-only, 3=shuffle) |
| `SequenceBranch` | `i32` (offset) | Jump offset for a sequence branch |

### Intrinsics

| Opcode | Description |
|--------|-------------|
| `VisitCount` | Pop a `DivertTarget`, push its visit count |
| `CurrentVisitCount` | Push the visit count of the current container |
| `TurnsSince` | Pop a `DivertTarget`, push turns since last visit (-1 if never) |
| `TurnIndex` | Push the current turn index |
| `ChoiceCount` | Push the number of currently available choices |
| `Random` | Pop max and min, push a random integer in [min, max] |
| `SeedRandom` | Pop a seed value and set the RNG seed |

### Casts and math

| Opcode | Description |
|--------|-------------|
| `CastToInt` | Pop a value, push it as an integer |
| `CastToFloat` | Pop a value, push it as a float |
| `Floor` | Pop a float, push its floor as an integer |
| `Ceiling` | Pop a float, push its ceiling as an integer |
| `Pow` | Pop exponent and base, push base^exponent |
| `Min` | Pop two values, push the smaller |
| `Max` | Pop two values, push the larger |

### External functions

| Opcode | Operands | Description |
|--------|----------|-------------|
| `CallExternal` | `DefinitionId`, `u8` (arg count) | Call an externally-bound function |

### List operations

| Opcode | Description |
|--------|-------------|
| `ListContains` | Pop item and list, push whether the list contains the item |
| `ListNotContains` | Pop item and list, push whether the list does not contain the item |
| `ListIntersect` | Pop two lists, push their intersection |
| `ListAll` | Pop a list, push all possible items from its origin lists |
| `ListInvert` | Pop a list, push the complement (all origin items not in the list) |
| `ListCount` | Pop a list, push its item count |
| `ListMin` | Pop a list, push its minimum item |
| `ListMax` | Pop a list, push its maximum item |
| `ListValue` | Pop a list, push its integer value (ordinal of single item) |
| `ListRange` | Pop max, min, and list; push items within the ordinal range |
| `ListFromInt` | Pop an integer and list origin, push the item with that ordinal |
| `ListRandom` | Pop a list, push a random item from it |

### String evaluation

| Opcode | Description |
|--------|-------------|
| `BeginStringEval` | Begin capturing output as a string value (for string interpolation) |
| `EndStringEval` | End string capture and push the result onto the stack |

### Lifecycle

| Opcode | Description |
|--------|-------------|
| `Done` | Yield -- the story pauses and can be resumed |
| `End` | Permanent end -- the story is finished |
| `Nop` | No operation |

### Debug

| Opcode | Operands | Description |
|--------|----------|-------------|
| `SourceLocation` | `u32` (line), `u32` (col) | Record source location for debugging |

## Execution model

The step function (`continue_maximally`) executes opcodes in a loop until reaching a yield point: `Done`, `End`, or choice presentation. At each yield, accumulated output text is returned to the host via `StepResult`.

**Call stack**: Function and tunnel calls push frames onto the call stack. Each frame has its own local variable storage (temp slots). `Return` and `TunnelReturn` pop frames.

**Container stack**: Each call frame tracks which containers are currently active. `EnterContainer` pushes, `ExitContainer` pops. This drives visit counting and turn tracking.

**Thread forking**: `ThreadCall` forks the current execution state (stacks, globals, output) to explore a choice branch. All threads run within the same step. At yield, threads are merged: each live thread contributes its choices to the final `StepResult`.
