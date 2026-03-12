# Sequence Handling: Design & Pipeline

This document describes how ink sequences (alternatives) are represented and transformed at every stage of the brink compiler and runtime.

## Ink Language Semantics

### Basic Sequences

Sequences (also called "alternatives") produce different text depending on how many times they've been evaluated. They're written inside `{`...`}` curly brackets, with elements separated by `|` pipes.

```ink
The radio hissed into life. {"Three!"|"Two!"|"One!"|There was the white noise racket of an explosion.|But it was just static.}
```

On the first evaluation, the first element is shown. On the second, the second element. And so on.

### Sequence Types

There are four fundamental types, controlled by annotation symbols or keywords:

| Type | Symbol | Keyword | Behavior |
|------|--------|---------|----------|
| **Stopping** (default) | `$` | `stopping:` | Show each element in order; repeat the last element forever |
| **Cycle** | `&` | `cycle:` | Show each element in order; loop back to the first |
| **Once-only** | `!` | `once:` | Show each element in order; show nothing after the last |
| **Shuffle** | `~` | `shuffle:` | Show elements in a randomized order |

**Index selection math:**
- **Stopping:** `index = min(visit_count, branch_count - 1)` — clamps to the last element
- **Cycle:** `index = visit_count % branch_count` — wraps around
- **Once-only:** `index = visit_count < branch_count ? visit_count : SKIP_ALL` — past the end, produce nothing
- **Shuffle:** deterministic RNG seeded by path hash + round + story seed (Fisher-Yates)

### Modified Shuffles

Shuffle can be combined with other types:

- **`shuffle`** alone (or `shuffle cycle`) — shuffle, play through, reshuffle, repeat
- **`shuffle once`** — shuffle, play through, then produce nothing
- **`shuffle stopping`** — shuffle all elements except the last, play through, then stick on the last element

### Inline vs Block Forms

**Inline:** embedded in text, `|`-separated

```ink
It was {&Monday|Tuesday|Wednesday} today.
```

**Block (multiline):** each element on its own line with `-` markers

```ink
{ stopping:
    - I entered the casino.
    - I entered the casino again.
    - Once more, I went inside.
}
```

Block sequences can contain complex content — choices, nested sequences, diverts, multiple lines per branch.

### Features

- **Blank elements:** `{!||||Then lights out. -> eek}` — empty branches produce no output
- **Nesting:** `{&{wastes no time and |}swipes|scratches}` — sequences inside sequences
- **Diverts in elements:** `{waited.|gave up. -> leave}` — an element can divert
- **Inside choice text:** `+ "Hello, {&Master|Monsieur}!"` — sequences in choice display text

### Fundamental Invariant: Visit Counting

The core mechanism is simple: each sequence tracks how many times it has been *evaluated*, and that count drives which branch to show. In the reference implementation, this is achieved by wrapping each sequence in its own container with visit counting enabled. The container's visit count *is* the sequence's evaluation count.

This means:
- Two sequences in the same knot advance **independently** — each has its own container, its own visit count
- A sequence inside a loop re-evaluates each iteration — the container is re-entered, incrementing the count
- A sequence inside a function re-evaluates each call — same mechanism

---

## Reference Implementation (C# ink compiler)

Before examining the brink pipeline, it's essential to understand how the reference C# ink compiler handles sequences, because this defines the correct behavior.

### Container-Per-Sequence Architecture

The reference compiler wraps **every sequence** in a dedicated `Container` with:
- `visitsShouldBeCounted = true` — the container tracks its own visit count
- `countingAtStartOnly = true` — the count increments once per entry, not per instruction

### Generated Code Structure

For a stopping sequence `{one|two|three}`:

```
[sequence container, visits=true, count_start_only=true]
    EvalStart
    VisitIndex              ; push this container's visit count (0-based)
    Push(2)                 ; branch_count - 1
    MIN                     ; clamp to max index

    Duplicate               ; for branch 0 check
    Push(0)
    ==
    JumpIfFalse(skip_s0)
    EnterContainer(s0)      ; branch 0 content

    Duplicate               ; for branch 1 check
    Push(1)
    ==
    JumpIfFalse(skip_s1)
    EnterContainer(s1)      ; branch 1 content

    Duplicate               ; for branch 2 check
    Push(2)
    ==
    JumpIfFalse(skip_s2)
    EnterContainer(s2)      ; branch 2 content

    NoOp                    ; exit label
    EvalEnd

[container s0] "one"  → divert to exit
[container s1] "two"  → divert to exit
[container s2] "three" → divert to exit
```

### Key Design Decisions in Reference

1. **Visit count IS the state** — no separate sequence counter variable. The container's visit count, managed by the runtime's standard visit-counting infrastructure, drives branch selection.

2. **Branches are child containers** — each branch is a named sub-container (`s0`, `s1`, ...) in the sequence container's named-only content. They're not inline code — they're separate containers entered via conditional diverts.

3. **Switch-statement pattern** — the computed index is duplicated and compared against each branch number using `Duplicate + Push(N) + Equal + JumpIfFalse`. This is a standard computed-branch pattern.

4. **Once-only adds a phantom branch** — when `SequenceType.Once` is set, an extra empty branch is appended past the real content. After exhaustion, the index points to this empty branch, producing no output.

5. **Shuffle is a runtime operation** — the `SequenceShuffleIndex` control command replaces the `MIN`/`MOD` math. It pops element count and sequence count from the stack and pushes a deterministic-random index.

---

## Parser (`brink-syntax`)

**File:** `crates/internal/brink-syntax/src/parser/inline.rs`

### The Problem

Ink uses `{`...`}` braces for four different things: bare expressions (`{expr}`), conditionals (`{expr: ...}`), sequences with annotation (`{& ...|...}`), and implicit sequences (`{a|b|c}`). The parser must distinguish these without semantic knowledge — it only has syntax to work with.

### The Approach

The parser uses a two-stage dispatch in `inner_logic()`:

1. **Annotation check** — if the first token inside `{` is a sequence annotation symbol (`& ! ~ $`) or keyword (`stopping cycle shuffle once`), parse as `SEQUENCE_WITH_ANNOTATION`.

2. **Lookahead for pipe** — if the pre-computed brace scan found a `|` separator, parse as `IMPLICIT_SEQUENCE` (default stopping behavior).

3. **Otherwise** — parse as expression, then check for `:` to dispatch as conditional or bare interpolation.

### CST Nodes

```
SEQUENCE_WITH_ANNOTATION
  SEQUENCE_SYMBOL_ANNOTATION?   (& ! ~ $ — can combine multiple)
  SEQUENCE_WORD_ANNOTATION?     (stopping: cycle: shuffle: once: — can combine)
  INLINE_BRANCHES_SEQ?          (content | content | ...)
  MULTILINE_BRANCHES_SEQ?       (NEWLINE, then - branch lines)

IMPLICIT_SEQUENCE
  BRANCH_CONTENT*               (separated by PIPE tokens)
```

### Details

- **Symbol annotations** can stack: `{~!a|b}` is `SHUFFLE | ONCE`
- **Word annotations** can stack: `{shuffle once: a|b}` is `SHUFFLE | ONCE`, terminated by `:`
- **Inline branches:** `branch_content()` parses text/interpolations/glue up to `|` or `}`
- **Multiline branches:** each `- content` line is a `MULTILINE_BRANCH_SEQ` node, can contain multiple lines of content (parsed by `multiline_branch_body()`)
- **Implicit sequences** default to `STOPPING` — the HIR assigns the type since the parser has no annotation to read

The parser handles one sequence at a time. Nesting works naturally: a `{` inside a branch starts a new `inner_logic()` dispatch.

---

## HIR (`brink-ir::hir`)

**Files:** `crates/internal/brink-ir/src/hir/types.rs`, `crates/internal/brink-ir/src/hir/lower.rs`

### The Problem

The CST gives us raw syntax nodes — annotation tokens, branch content, pipe separators. The HIR needs to normalize this into a clean semantic representation: what *kind* of sequence is this, and what are its branches? The two syntactic forms (annotated and implicit) need to produce the same HIR type.

### The Approach

HIR lowering converts both CST forms into a single `Sequence` struct. The sequence type annotation is decoded from CST tokens into a `SequenceType` bitflag. Each branch is lowered as a full `Block` — sequences can contain arbitrary content including choices, conditionals, and nested sequences.

### Types

```rust
bitflags! {
    pub struct SequenceType: u8 {
        const STOPPING = 0x01;  // $ — default
        const CYCLE    = 0x02;  // &
        const ONCE     = 0x04;  // !
        const SHUFFLE  = 0x08;  // ~
    }
}

pub struct Sequence {
    pub ptr: SyntaxNodePtr,     // back-reference to CST
    pub kind: SequenceType,     // bitmask of type flags
    pub branches: Vec<Block>,   // each branch is a full block
}
```

### Where Sequences Appear

- **Block-level:** `Stmt::Sequence(Sequence)` — multiline blocks like `{stopping: - ... - ...}`
- **Inline in content:** `ContentPart::InlineSequence(Sequence)` — `{a|b|c}` embedded in text

Both positions use the same `Sequence` struct.

### Lowering

**`lower_inline_sequence()`** handles `SEQUENCE_WITH_ANNOTATION`:
1. Call `lower_sequence_type()` to decode annotation into `SequenceType`
2. Map each branch through `wrap_content_as_block()` (inline) or full block lowering (multiline)
3. Return `Sequence { kind, branches }`

**`lower_sequence_type()`** decodes annotations:
- Checks symbol annotation tokens: `&` → CYCLE, `!` → ONCE, `~` → SHUFFLE, `$` → STOPPING
- Checks word annotation keywords: same mapping
- Flags are OR'd together (supports combinations like `SHUFFLE | ONCE`)
- **Default when empty:** `SequenceType::STOPPING`

**Implicit sequences** (`IMPLICIT_SEQUENCE`): created directly in the inline content lowering with `kind: SequenceType::STOPPING`.

**`lower_block_sequence()`** handles multiline blocks: same type decoding, but branches come from `MULTILINE_BRANCH_SEQ` nodes with full body lowering.

### What the HIR preserves

- The exact type annotation (as a bitmask)
- Each branch as a complete `Block` (can contain anything)
- A pointer back to the source location

### What the HIR does NOT do

- No container creation — sequences are still tree nodes, not flattened into containers
- No visit-count mechanism — that's a codegen concern
- No special treatment of shuffle vs non-shuffle — all are `Sequence` with different flags

---

## Analyzer (`brink-analyzer`)

The analyzer has **no special handling for sequences**. Sequences are transparent: the analyzer walks into branches for symbol resolution and cross-file analysis, but doesn't need to understand sequence semantics. There are no sequence-specific diagnostics, no special visit-count analysis, no validation of sequence type combinations.

---

## LIR (`brink-ir::lir`)

**Files:** `crates/internal/brink-ir/src/lir/types.rs`, `crates/internal/brink-ir/src/lir/lower/mod.rs`, `crates/internal/brink-ir/src/lir/lower/content.rs`

### The Problem

The HIR has sequences as tree-embedded constructs. The LIR needs to prepare them for codegen into bytecode. This means resolving any symbolic references inside branches, collecting child containers that might be created *within* branches (e.g., a choice set inside a sequence branch), and passing the sequence structure through to codegen.

### The Approach

LIR lowering keeps sequences as inline constructs — they do NOT get their own containers at the LIR level. Each branch's statements are lowered and collected. If a branch contains choice sets (which *do* create containers), those child containers are extracted and bubbled up to the parent container's children list.

### Types

```rust
// Block-level sequence
pub struct Sequence {
    pub kind: SequenceType,          // same bitmask from HIR
    pub branches: Vec<Vec<Stmt>>,    // each branch is a flat stmt vector
}

// Inline sequence (inside content)
pub enum ContentPart {
    Text(String),
    Glue,
    Interpolation(Expr),
    InlineConditional(Conditional),
    InlineSequence(Sequence),        // same Sequence struct
}
```

### Lowering

**Block-level** (`mod.rs`, line 371):

```rust
hir::Stmt::Sequence(seq) => {
    let branches = seq.branches.iter().map(|branch| {
        let (body, branch_children) = lower_block_with_children(branch, ctx);
        children.extend(branch_children);  // bubble up choice containers
        body
    }).collect();
    stmts.push(lir::Stmt::Sequence(lir::Sequence {
        kind: seq.kind,
        branches,
    }));
}
```

**Inline** (`content.rs`, line 44):

```rust
hir::ContentPart::InlineSequence(seq) => {
    let branches = seq.branches.iter()
        .map(|b| lower_inline_block(b, ctx))
        .collect();
    lir::ContentPart::InlineSequence(lir::Sequence {
        kind: seq.kind,
        branches,
    })
}
```

### Container Planning

The plan phase (`plan.rs`, line 236) walks into sequence branches to find choice sets that need container IDs, but **does not allocate containers for the sequences themselves**:

```rust
hir::Stmt::Sequence(seq) => {
    for branch in &seq.branches {
        // Recurse into branches looking for choice sets
        for s in &branch.stmts {
            plan_stmt_choices(s, ...);
        }
    }
}
```

### What the LIR does NOT do

- **No container creation for sequences** — the sequence remains an inline statement/content-part, NOT a child container
- **No visit-count setup** — no mechanism to track how many times the sequence has been evaluated
- **No branch selection logic** — deferred entirely to codegen

### Critical Gap

The reference C# compiler creates a **dedicated container** for each sequence with visit counting. The LIR does not. This means the codegen has no container to use for visit counting, and multiple sequences in the same parent container would share a visit count (if any were provided). This is a fundamental architectural mismatch with how sequences should work.

---

## Bytecode Codegen (`brink-codegen-inkb`)

**File:** `crates/internal/brink-codegen-inkb/src/container.rs`

### The Problem

The codegen needs to emit bytecode that selects and executes the correct branch based on the sequence's evaluation count. The runtime has `Opcode::Sequence(kind, count)` and `Opcode::SequenceBranch(offset)` instructions for this purpose.

### Current Implementation

```rust
pub(super) fn emit_sequence(&mut self, seq: &lir::Sequence) {
    let kind = sequence_kind(seq.kind);
    let count = seq.branches.len() as u8;

    // 1. Emit Sequence opcode (determines branch index)
    self.emit(Opcode::Sequence(kind, count));

    // 2. Emit SequenceBranch jump placeholders
    let mut branch_placeholders = Vec::new();
    for _ in 0..count {
        let site = self.emit_jump_placeholder(Opcode::SequenceBranch(0));
        branch_placeholders.push(site);
    }

    // 3. Emit branch bodies inline, with Jump-to-end between them
    let mut end_jumps = Vec::new();
    for (i, branch) in seq.branches.iter().enumerate() {
        self.patch_jump(branch_placeholders[i]);
        self.emit_body(branch);
        if i < seq.branches.len() - 1 {
            let end_site = self.emit_jump_placeholder(Opcode::Jump(0));
            end_jumps.push(end_site);
        }
    }

    // 4. Patch all end jumps
    for site in end_jumps {
        self.patch_jump(site);
    }
}
```

### Type Mapping

```rust
fn sequence_kind(kind: SequenceType) -> SequenceKind {
    if kind.contains(SequenceType::SHUFFLE) { SequenceKind::Shuffle }
    else if kind.contains(SequenceType::CYCLE) { SequenceKind::Cycle }
    else if kind.contains(SequenceType::ONCE) { SequenceKind::OnceOnly }
    else { SequenceKind::Stopping }  // default
}
```

Note: this collapses combinations. `SHUFFLE | STOPPING` becomes just `Shuffle`. `SHUFFLE | ONCE` becomes just `Shuffle`. The modifiers are lost.

### Generated Bytecode Layout

```
Sequence(Stopping, 3)       ; compute branch index → push to stack
SequenceBranch(→branch_0)   ; jump to branch 0 body
SequenceBranch(→branch_1)   ; jump to branch 1 body
SequenceBranch(→branch_2)   ; jump to branch 2 body
[branch 0 body]
Jump(→end)
[branch 1 body]
Jump(→end)
[branch 2 body]
[end]
```

### KNOWN BUGS

This implementation has multiple fundamental issues:

**Bug 1: Stack underflow.** The `Sequence` opcode's runtime handler (`handle_sequence`) pops a `DivertTarget` from the value stack to look up the visit count of the sequence's container. But the codegen **never pushes a DivertTarget** before emitting `Sequence`. Result: every non-shuffle sequence hits "value stack underflow" at runtime.

**Bug 2: SequenceBranch doesn't select.** In the runtime, `Opcode::SequenceBranch(rel)` is handled identically to `Opcode::Jump(rel)` — it's an unconditional jump. It does not check the index that `Sequence` pushed to the stack. The first `SequenceBranch` always fires, always jumping to branch 0. The index is never consumed. Even if Bug 1 were fixed, every sequence would always show the first branch.

**Bug 3: No per-sequence visit counting.** Because LIR doesn't create a container for each sequence, there's no container whose visit count could track the sequence's evaluation count. Even if we pushed the parent container's divert target, multiple sequences in the same container would share a visit count and advance in lockstep (wrong behavior).

**Bug 4: Modified shuffle types are lost.** `sequence_kind()` maps `SHUFFLE | STOPPING` and `SHUFFLE | ONCE` both to `SequenceKind::Shuffle`. The runtime's shuffle handler doesn't know whether to stop, cycle, or exhaust — it only has the base `Shuffle` kind.

---

## Runtime (`brink-runtime`)

**File:** `crates/brink-runtime/src/vm.rs`

### Opcode Dispatch

```rust
// SequenceBranch is treated as an unconditional jump (Bug 2 above)
Opcode::Jump(rel) | Opcode::SequenceBranch(rel) => {
    apply_jump(flow, rel)?;
}

// Sequence computes and pushes a branch index
Opcode::Sequence(kind, count) => {
    handle_sequence(flow, state, kind, count)?;
}
```

### Non-Shuffle Handler

```rust
fn handle_sequence(flow, state, kind, count) -> Result<()> {
    if kind == Shuffle { return handle_shuffle_sequence(flow, state); }

    // Pop a DivertTarget to look up visit count
    let val = flow.pop_value()?;   // ← Bug 1: nothing pushed this
    let visit_count = if let Value::DivertTarget(id) = val {
        state.visit_count(id)
    } else {
        0
    };

    let idx = match kind {
        Cycle    => visit_count % count,
        Stopping => visit_count.min(count - 1),
        OnceOnly => if visit_count < count { visit_count } else { count },
        Shuffle  => unreachable!(),
    };

    flow.value_stack.push(Value::Int(idx));   // ← Bug 2: nothing reads this
    Ok(())
}
```

### Shuffle Handler

The shuffle handler has a different protocol: it pops `numElements` (Int) and `seqCount` (Int) from the stack. It uses a deterministic Fisher-Yates shuffle seeded by `path_hash + loopIndex + story_seed`. This matches the reference C# implementation's `NextSequenceShuffleIndex`.

### CurrentVisitCount

The runtime has `Opcode::CurrentVisitCount` which pushes the 0-based visit count of the current container. This is used by the **converter's** sequence output and works correctly. The compiler could use this if sequences were wrapped in their own containers.

---

## Converter Reference (`brink-converter`)

**File:** `crates/internal/brink-converter/src/codegen.rs`

The converter works from inklecate's `.ink.json` output, which has already compiled sequences into the container-per-sequence architecture. The converter just faithfully translates this structure.

### What inklecate provides (and the converter preserves)

For a cycle sequence `{cycle: - I held my breath. - I waited. - I paused.}`:

```
(container seq_wrapper                    ; dedicated container
    (flags visits start_only)             ; visit counting enabled
    (code
        current_visit_count               ; push 0-based visit count
        push_int 3                        ; branch count
        modulo                            ; cycle: wrap around

        duplicate                         ; check branch 0
        push_int 0
        equal
        jump_if_false +9
        enter_container branch_0

        duplicate                         ; check branch 1
        push_int 1
        equal
        jump_if_false +9
        enter_container branch_1

        duplicate                         ; check branch 2
        push_int 2
        equal
        jump_if_false +9
        enter_container branch_2

        nop                               ; fallthrough (no match)
    )
)

(container branch_0
    (lines 0 "I held my breath.")
    (code
        pop                               ; discard the duplicated index
        emit_newline
        emit_line 0
        emit_newline
        goto seq_end_label
    )
)
; ... branch_1, branch_2 similar
```

### Key Observations

1. **Container-per-sequence** — each sequence gets a container with `visits start_only` flags
2. **`current_visit_count`** — reads the container's own visit count (0-based)
3. **Math for index** — `modulo` for cycle, `min` for stopping, etc.
4. **Switch pattern** — `duplicate / push_int N / equal / jump_if_false / enter_container` repeated for each branch
5. **Branch containers** — each branch is a child container that pops the duplicated index, emits content, and gotos to the end
6. **Shuffle uses `Sequence(Shuffle, 0)`** — the only case where the `Sequence` opcode is emitted, specifically for the shuffle RNG

### The converter's `Sequence` emission

```rust
ControlCommand::Sequence => self.emit(&Opcode::Sequence(SequenceKind::Shuffle, 0)),
```

This is emitted ONLY for the `seq` control command in inklecate's output, which corresponds to the shuffle RNG operation. Non-shuffle sequences don't use the `Sequence` opcode at all — they use the math + branch pattern above.

---

## The Architectural Gap

Comparing the reference implementation and converter output against the brink compiler reveals a fundamental design mismatch:

### What the reference does
- Each sequence → dedicated container with visit counting
- `current_visit_count` (or `VisitIndex`) reads the container's visit count
- Math computes the branch index from visit count
- Switch-statement pattern selects the branch
- Each branch is a child container

### What the brink compiler does
- Each sequence → inline statements in the parent container
- `Sequence(kind, count)` opcode expects a DivertTarget on the stack (never provided)
- `SequenceBranch(offset)` opcodes that behave as unconditional jumps
- No per-sequence container, no visit counting
- Branches are inline code, not containers

### The gap
The compiler treats sequences as a flat, inline control-flow construct (like a switch statement). The reference treats them as **containers with implicit state** (visit count). The state management is offloaded to the runtime's existing visit-counting infrastructure rather than requiring custom opcodes.

The brink runtime has the `Sequence` and `SequenceBranch` opcodes, but they are incomplete:
- `Sequence` computes an index correctly (given a visit count) but can't obtain the visit count because nothing pushes the right value
- `SequenceBranch` doesn't consume the index at all — it's just `Jump`

### Path forward

The correct fix likely requires changes at the LIR and codegen levels:

1. **LIR:** Each sequence (both block-level and inline) should produce a **child container** with `VISITS | COUNT_START_ONLY` counting flags, similar to how choice sets produce child containers.

2. **Codegen:** Inside the sequence container, emit the reference pattern:
   - `current_visit_count` to read the container's visit count
   - Math for index computation (min/modulo/shuffle)
   - `duplicate / push_int N / equal / jump_if_false / enter_container` for branch selection
   - Each branch as a child container with `pop` + content + `goto end`

3. **Runtime:** The existing `CurrentVisitCount`, math, and `EnterContainer` opcodes already work (the converter proves this). The `Sequence` opcode may still be useful for shuffle, but `SequenceBranch` should either be removed or repurposed.

This would align the compiler's sequence handling with both the reference C# implementation and the converter's proven output.

---

## End-to-End Example

For the ink source:

```ink
-> test
=== test
{ cycle:
    - I held my breath.
    - I waited impatiently.
    - I paused.
}
+ [Try again] -> test
```

### Parser Output (CST)

```
DIVERT_NODE -> test

KNOT: test
  MULTILINE_BLOCK
    SEQUENCE_WITH_ANNOTATION
      SEQUENCE_WORD_ANNOTATION: cycle:
      MULTILINE_BRANCHES_SEQ
        MULTILINE_BRANCH_SEQ: - I held my breath.
        MULTILINE_BRANCH_SEQ: - I waited impatiently.
        MULTILINE_BRANCH_SEQ: - I paused.

  CHOICE
    CHOICE_BULLETS: +
    CHOICE_BRACKET_CONTENT: [Try again]
    DIVERT_NODE: -> test
```

### HIR

```
Divert(test)

Knot(test) {
  Sequence {
    kind: CYCLE,
    branches: [
      Block([Content("I held my breath."), EndOfLine]),
      Block([Content("I waited impatiently."), EndOfLine]),
      Block([Content("I paused."), EndOfLine]),
    ],
  }
  ChoiceSet {
    choices: [
      Choice {
        is_sticky: true,
        bracket_content: Content("Try again"),
        divert: -> test,
      },
    ],
  }
}
```

### Current Compiler Output (BROKEN)

```
# test container
sequence cycle 3            ← Bug: nothing on stack
sequence_branch →branch_0   ← Bug: unconditional jump
sequence_branch →branch_1
sequence_branch →branch_2
emit_line 0  "I held my breath."
emit_newline
jump →end
emit_line 1  "I waited impatiently."
emit_newline
jump →end
emit_line 2  "I paused."
emit_newline
[choice set follows]
```

Result: "value stack underflow" — the `sequence` opcode tries to pop a DivertTarget that was never pushed.

### Correct Output (from converter)

```
# test container
goto seq_wrapper
[choice evaluation container follows]

# seq_wrapper container (flags: visits, start_only)
current_visit_count          ← 0-based count of entries
push_int 3
modulo                       ← cycle: wrap around
duplicate
push_int 0
equal
jump_if_false +9
enter_container branch_0     ← enter child for branch 0
duplicate
push_int 1
equal
jump_if_false +9
enter_container branch_1
duplicate
push_int 2
equal
jump_if_false +9
enter_container branch_2
nop

# branch_0 container
pop                          ← discard duplicated index
emit_newline
emit_line 0  "I held my breath."
emit_newline
goto seq_end_label

# branch_1 container
pop
emit_newline
emit_line 1  "I waited impatiently."
emit_newline
goto seq_end_label

# branch_2 container
pop
emit_newline
emit_line 2  "I paused."
emit_newline
goto seq_end_label
```

Each evaluation of `test` increments `seq_wrapper`'s visit count. On first visit: count=0, 0%3=0, branch_0 fires ("I held my breath."). On second: count=1, 1%3=1, branch_1 fires. On third: count=2, 2%3=2, branch_2. On fourth: count=3, 3%3=0, back to branch_0. The cycle continues.
