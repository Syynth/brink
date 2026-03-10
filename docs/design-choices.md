# Choice Handling: Design & Pipeline

This document describes how ink choices are represented and transformed at every stage of the brink compiler and runtime.

## Ink Language Semantics

### Basic Choices

A choice is marked with `*` (once-only) or `+` (sticky). The player sees the choice text, picks one, and execution continues into the choice body.

```ink
* Hello back!
    Nice to hear from you!
```

Output:
```
1: Hello back!
> 1
Hello back!
Nice to hear from you!
```

By default, the choice text is printed again after selection.

### Three-Part Text Model

Square brackets divide choice text into three regions:

```ink
* start_text [choice_only_text] inner_text
```

| Region | Shown in choice list | Printed after selection |
|--------|---------------------|----------------------|
| **Start** (before `[`) | Yes | Yes |
| **Choice-only** (inside `[]`) | Yes | No |
| **Inner** (after `]`) | No | Yes |

Example:

```ink
* "I am somewhat tired[."]," I repeated.
```

- Player sees: `"I am somewhat tired."`
- After selection: `"I am somewhat tired," I repeated.`

The display text (what the player sees) is `start + choice_only`. The output text (printed after selection) is `start + inner`.

### Once-Only vs Sticky

- `*` — once-only. After being chosen, the choice disappears on subsequent visits.
- `+` — sticky. The choice remains available every time.

Once-only is tracked by visit-counting the choice's target container.

### Fallback (Invisible Default) Choices

A choice with no text at all is a fallback:

```ink
* -> out_of_options
```

Fallback choices are never shown to the player. If all remaining choices in a set are fallbacks, the first one is auto-selected.

### Conditional Choices

Conditions in `{}` control availability. Multiple conditions are ANDed:

```ink
* { not visit_paris } [Go to Paris] -> visit_paris
* { visited_rome } { has_money } [Fly home] -> airport
```

### Multi-Line Choices

When the choice line has only conditions and no text, continuation lines provide the text:

```ink
* { true } { true }
  { true and true }  one
```

Conditions from all lines are ANDed together. The first text encountered becomes start content. This is handled in the parser's continuation loop.

### Weave Structure

Choices group into choice sets. A gather (`-`) is the convergence point after a choice set — all choices without explicit diverts flow into it:

```ink
* Option A
    Text for A.
* Option B
    Text for B.
- Both paths continue here.
```

Gathers can be labeled for divert targeting: `- (label) content`.

---

## Parser (`brink-syntax`)

**File:** `crates/internal/brink-syntax/src/parser/choice.rs`

### The Problem

Ink's choice syntax is deceptively complex. A single choice line can contain a nesting depth indicator (bullet count), an optional label, zero or more conditions, three distinct text regions separated by brackets, an optional divert, and tags. The line can also span multiple physical lines when conditions come first and text follows on a continuation line. The parser needs to decompose all of this into a structured CST node without any semantic interpretation — that's the HIR's job.

The key challenge is that the parser sees choices one line at a time. It doesn't know about choice *sets* or gathers — it just parses individual choice lines. Grouping choices into sets and folding them by nesting depth happens later in HIR lowering (weave folding). The parser's only job is to correctly identify the parts of each individual choice.

### The Approach

The parser produces a flat `CHOICE` CST node with clearly delineated child nodes for each syntactic region. The three-part text model (`start [choice_only] inner`) maps directly to three optional child nodes. The parser doesn't interpret what these regions *mean* for display vs output — it just records what text appears in each position.

```
CHOICE
  CHOICE_BULLETS          (* or +, depth = count of bullets)
  LABEL?                  (label_name)
  CHOICE_CONDITION*       { expr }
  CHOICE_START_CONTENT?   text before [
  CHOICE_BRACKET_CONTENT? [ text ]
  CHOICE_INNER_CONTENT?   text after ]
  DIVERT_NODE?            -> target
  TAGS?                   # tag
  NEWLINE
```

### Details

- **Bullet parsing:** counts `*`/`+` tokens to determine nesting depth. Depth matters for weave folding but the parser just records the tokens.
- **Label:** `(ident)` parsed if `L_PAREN IDENT R_PAREN` pattern matches
- **Conditions:** zero or more `{ expr }` blocks, each producing a `CHOICE_CONDITION` node
- **Continuation lines:** after conditions, if at NEWLINE and the next line isn't structural (`* + - EOF`), the parser consumes the newline and continues parsing conditions/text on the next line. This handles the ink pattern where conditions are on the `*` line but text follows on the next line.
- **Content elements:** text, `<>` glue, `{expr}` interpolations, `\` escapes

The parser handles a single choice line (plus continuations). The parent (`story.rs:line()`) dispatches `STAR|PLUS` to the choice parser. Consecutive choices become siblings in the CST; the weave folder groups them later.

---

## HIR (`brink-ir::hir`)

**Files:** `crates/internal/brink-ir/src/hir/types.rs`, `crates/internal/brink-ir/src/hir/lower.rs`

### The Problem

The parser gives us a flat sequence of CST nodes — individual choice lines, gather lines, and content lines as siblings. But ink's semantics are hierarchical: choices group into sets, sets converge at gathers, and choices at deeper nesting levels belong *inside* shallower choices. The HIR's job is to reconstruct this hierarchy from the flat stream.

Additionally, the parser preserves syntactic fidelity (multiple `CHOICE_CONDITION` nodes, raw CST text spans) that downstream stages don't want. The HIR needs to normalize: AND multiple conditions together, compute derived properties like `is_fallback`, and attach the choice's body content (the indented lines after the choice) to the choice itself.

### The Approach

HIR lowering does two things: **field normalization** (per-choice) and **weave folding** (grouping into sets).

Field normalization maps each CST choice into a clean `Choice` struct where semantic properties are explicit. Multiple conditions become a single ANDed expression. The three text regions are preserved as-is (the semantic interpretation — "display text = start + choice_only, output text = start + inner" — belongs to codegen, not HIR). Fallback detection is computed from the presence/absence of text.

Weave folding takes the flat `WeaveItem` stream and builds the nesting hierarchy. This is the most complex part of choice handling in the entire pipeline because it reconstructs implicit structure from positional cues (bullet depth, gather depth).

### Types

```rust
struct Choice {
    ptr: AstPtr<ast::Choice>,
    is_sticky: bool,           // + vs *
    is_fallback: bool,         // no text in any region
    label: Option<Name>,
    condition: Option<Expr>,   // multiple conditions ANDed together
    start_content: Option<Content>,
    bracket_content: Option<Content>,
    inner_content: Option<Content>,
    divert: Option<Divert>,
    tags: Vec<Tag>,
    body: Block,               // nested content after selection
}

struct ChoiceSet {
    choices: Vec<Choice>,
    gather: Option<Gather>,           // convergence point
    opening_gather: Option<Gather>,   // for `- * hello` pattern
}

struct Gather {
    ptr: AstPtr<ast::Gather>,
    label: Option<Name>,
    content: Option<Content>,
    divert: Option<Divert>,
    tags: Vec<Tag>,
    // NOTE: gathers do NOT own a body. Trailing content is sibling stmts.
}
```

### Lowering (AST → HIR)

`lower_choice()` maps each AST field:

1. **Bullets** → `is_sticky` (first bullet determines: `+` = sticky, `*` = once-only)
2. **Label** → `label` + symbol declaration in current scope
3. **Fallback detection:**
   ```rust
   is_fallback = start_content.is_none()
       && bracket_content.is_none()
       && inner_content.is_none();
   ```
4. **Conditions** → multiple conditions reduced with `AND`:
   ```rust
   conditions.reduce(|a, b| Expr::Infix(Box::new(a), InfixOp::And, Box::new(b)))
   ```
5. **Three content regions** → each independently lowered to `Content { parts: Vec<ContentPart> }`
6. **Body** → `lower_choice_body()` iterates CST children (excluding the already-captured divert)

### Weave Folding

The flat stream of `WeaveItem`s (choices, gathers, stmts) is folded into a nested `Block`:

```rust
enum WeaveItem {
    Choice { choice: Choice, depth: usize },
    Gather { gather: Gather, depth: usize },
    Stmt(Stmt),
}
```

**Why this is hard:** ink uses indentation-like nesting via bullet count (`*` = depth 1, `**` = depth 2) but the parser doesn't give us an explicit tree — it gives us a flat sequence with depth annotations. We need to reconstruct which deeper items belong to which shallower choice, and where choice sets begin and end. This is similar to the "off-side rule" problem in languages like Python, except the nesting is defined by bullet count rather than indentation.

**`fold_weave_at_depth(items, base_depth)`:**

1. **Phase 1 — `nest_deeper_items()`:** extract items deeper than `base_depth`, recursively fold them, attach the resulting block to the preceding choice's body. This is the recursive step that turns a flat stream into a tree.

2. **Phase 2 — build choice sets:** iterate the now-single-depth stream:
   - `Stmt` before any choice → parent block
   - `Stmt` after a choice → appended to previous choice's body (matches inklecate's `addContentToPreviousWeavePoint`)
   - `Choice` → accumulate into `choice_acc`
   - `Gather` after choices → flush `choice_acc` into `ChoiceSet` with this gather

This produces `Stmt::ChoiceSet(ChoiceSet { choices, gather, opening_gather })`.

---

## LIR (`brink-ir::lir`)

**Files:** `crates/internal/brink-ir/src/lir/types.rs`, `crates/internal/brink-ir/src/lir/lower/mod.rs`, `crates/internal/brink-ir/src/lir/lower/plan.rs`

### The Problem

The HIR represents choices as nested tree structures with symbolic names. The bytecode VM executes a flat list of containers, each identified by an opaque `DefinitionId`, with explicit `goto` instructions for control flow. The LIR's job is to bridge these two worlds: flatten the tree into containers, resolve all symbolic references into IDs, and make the control flow between choices, their bodies, and gathers explicit.

The core difficulty is that bytecode containers must be defined before they can be referenced. A choice's `BeginChoice` opcode needs to name the target container (the choice body) by ID. A choice body needs to `goto` the gather container. But in the HIR, these are just nested blocks — there's no concept of "container IDs" yet. The LIR needs to allocate all IDs upfront so that forward references work.

### The Approach

LIR lowering uses a **two-phase approach**: plan first, lower second.

The **planning phase** walks the HIR and pre-allocates a `DefinitionId` for every container that will exist in the output — every choice target, every gather, every knot/stitch. This creates a complete ID map before any lowering happens, so that any container can reference any other container by ID regardless of definition order.

The **lowering phase** then walks the HIR again, this time producing actual LIR containers. Each HIR `ChoiceSet` becomes: one `Stmt::ChoiceSet` in the parent container (the evaluation block), plus one child container per choice body, plus one child container for the gather. The lowering phase also handles the semantic transformation: the three-part text model from the HIR is split into "display content" (used in choice evaluation) and "output content" (emitted in the choice body after selection).

### Types

```rust
struct Choice {
    is_sticky: bool,
    is_fallback: bool,
    condition: Option<Expr>,
    start_content: Option<Content>,       // preserved from HIR
    choice_only_content: Option<Content>, // renamed from bracket_content
    inner_content: Option<Content>,
    target: DefinitionId,                 // choice body container
    tags: Vec<String>,
    has_inline_divert: bool,
}

struct ChoiceSet {
    choices: Vec<Choice>,
    gather_target: Option<DefinitionId>,  // convergence container
}
```

The three-part content split is preserved in the LIR choice. Each choice now has a `target: DefinitionId` pointing to its body container — this is the ID that was pre-allocated in the planning phase.

### Container Planning (Pre-Pass)

Before lowering, `plan::plan_containers()` pre-allocates `DefinitionId` for every container:

- Each choice target gets an ID keyed by `(file, scope, index)`
- Each gather gets an ID (even implicit ones — every choice set gets a gather container)
- Scope paths encode nesting: knot `tavern`, choice 0, nested choice 1 → `tavern.c0.c1`

This ensures all IDs are known before lowering starts, enabling forward references.

### Lowering (HIR → LIR)

**`lower_block_with_children()`** returns `(Vec<Stmt>, Vec<Container>)` — statements stay in the parent, choice targets and gathers are extracted as child containers.

For each `ChoiceSet`:

1. Look up pre-allocated gather target from the plan
2. For each choice, call `lower_choice_with_child()`:
   - Lower the three content parts
   - Build the choice body container with:
     - **`ChoiceOutput`** preamble: `start + inner` content (what prints after selection)
     - Recursively lowered body statements
     - Auto-divert to gather target (unless body ends with `DONE`/`END`)
   - Container kind: `ChoiceTarget`
   - Counting flags: `VISITS | COUNT_START_ONLY` for once-only, empty for sticky
3. Emit `Stmt::ChoiceSet` with the lowered choices and gather target
4. Build the gather container (explicit or implicit) with trailing statements as its body

---

## Bytecode Codegen (`brink-codegen-inkb`)

**File:** `crates/internal/brink-codegen-inkb/src/container.rs`

### The Problem

The LIR gives us structured choice data (three content regions, conditions, target IDs). The bytecode VM is a stack machine. Codegen's job is to translate choices into a specific sequence of stack operations that the runtime expects. This is where the "two-text" nature of choices becomes concrete: the same choice needs to produce *two different strings* — display text (what the player sees in the choice list) and output text (what gets printed after selection). These are assembled from different combinations of the three text regions.

The challenge is that the runtime's `BeginChoice` opcode has a rigid pop protocol: it expects specific values on the stack in a specific order. Codegen must emit instructions that set up the stack correctly for every combination of flags (has condition? has start text? has choice-only text? etc.).

### The Approach

Codegen emits a **choice evaluation block** (in the parent container) and a **choice body** (in the target container). The evaluation block pushes display text and condition onto the stack, then emits `BeginChoice` which consumes them. The choice body emits output text when entered after selection.

Display text = `start + choice_only` (combined into one string via `BeginStringEval`/`EndStringEval`).
Output text = `start + inner` (emitted as the `ChoiceOutput` preamble in the target container).

This split is the concrete realization of the three-part text model from the ink language spec.

### Choice Set Emission

```
BeginChoiceSet
  [for each choice: emit_choice()]
EndChoiceSet
Done                    ← yields to runtime, presenting choices
```

### Individual Choice Emission

The stack protocol for a single choice:

```
// 1. Push display text (start + choice_only combined)
BeginStringEval
  [emit content parts]
EndStringEval           ← pushes String onto value stack

// 2. Push condition
[emit condition expr]   ← pushes Bool onto value stack

// 3. Choice point
BeginChoice(flags, target_id)   ← pops condition (if has_condition),
                                   pops display (if has_start || has_choice_only)
EndChoice

// 4. Tags (after EndChoice)
BeginTag / EmitLine / EndTag
```

**Stack order matters:** display is pushed first, condition second. The runtime pops condition from the top first, then display.

**Single-pop protocol:** `BeginChoice` pops at most ONE string for display text. The `has_start_content` and `has_choice_only_content` flags are metadata only — they tell the runtime which parts contributed to the combined string, but don't cause separate pops.

### ChoiceOutput (in target container)

At the start of each choice target container's bytecode:

```
[emit start + inner content parts]   ← output text printed after selection
[optional inline divert]
EmitNewline
[rest of body...]
```

### ChoiceFlags

```rust
struct ChoiceFlags {          // packed as single byte
    has_condition: bool,          // 0x01
    has_start_content: bool,      // 0x02
    has_choice_only_content: bool, // 0x04
    once_only: bool,              // 0x08
    is_invisible_default: bool,   // 0x10
}
```

---

## Runtime (`brink-runtime`)

**Files:** `crates/brink-runtime/src/vm.rs`, `crates/brink-runtime/src/story.rs`, `crates/brink-runtime/src/output.rs`

### The Problem

The runtime's job is to present the player with a list of choices and then, after the player picks one, continue the story as if execution had always been heading toward that choice. This is harder than it sounds.

In a simple interpreter, the VM walks through code linearly. When it hits a choice set, it needs to do three things that conflict with linear execution:

1. **Build display text for each choice** — this requires executing content-producing instructions (string interpolation, inline logic, etc.) for each choice's display text. But these instructions have side effects on the output buffer. The runtime can't just emit them normally or they'd appear in the story output.

2. **Evaluate conditions for each choice** — some choices are conditional (`{has_key}`), and the condition might involve function calls or complex expressions. The runtime needs to evaluate these without committing to any choice.

3. **Remember where it was** — when a choice is finally selected, the runtime needs to resume execution inside that choice's body container. But by the time the player picks a choice, the VM has already walked past all the other choices evaluating their text and conditions. The execution context (call stack, container nesting) has been modified. The runtime needs to "rewind" to the state it was in when it first encountered that choice.

### The Solution: Speculative Evaluation + Thread Forking

The runtime treats choice evaluation as a speculative pass. It walks through all choices, evaluating display text and conditions, but captures text into a side buffer (not the main output) and snapshots the execution context at each choice point. When the player selects a choice, the runtime restores the snapshot from that choice and jumps to its body container.

This gives rise to three mechanisms:

**String capture** solves problem 1. `BeginStringEval` pushes a checkpoint onto the output buffer. All subsequent `EmitLine` calls write into a capture region instead of the main output. `EndStringEval` drains the capture region, assembles it into a single string, and pushes it onto the value stack. The main output is untouched. This is a general mechanism — it's also used for function calls that return strings — but choice display text is its primary use case.

**The value stack** solves problem 2. Conditions are evaluated and pushed onto the value stack, same as any other expression. When `BeginChoice` fires, it pops the condition (if present) and the display string (if present) off the stack. If the condition is false or the choice is once-only and already visited, the choice is skipped — the `PendingChoice` is simply never created.

**Thread forking** solves problem 3. When `BeginChoice` decides a choice is available, it snapshots the current thread's call stack (copy-on-write for efficiency). This snapshot — the "thread fork" — is stored on the `PendingChoice`. When the player later selects that choice, `select_choice()` replaces the current thread with the fork and sets the instruction pointer to the choice's body container. Execution resumes as if the VM had gone directly from the choice point into that choice's body.

### Why Thread Forking Is Necessary

The fork captures the call stack, not globals or visit counts. This is deliberate: globals and visit counts are part of the story's persistent state — if a choice's condition modified a global (rare but legal), that modification should persist regardless of which choice is selected. The call stack, on the other hand, represents the "where am I in the code" state — tunnel return addresses, container nesting — and that needs to be rewound.

Without forking, the runtime would need some other way to reconstruct the call stack at choice-selection time. Forking is the simplest correct approach: take a snapshot when you know the state is right, restore it later.

### The Skip Protocol

Between `BeginChoice` and `EndChoice`, a skipped choice (false condition or already visited) sets `flow.skipping_choice = true`. While this flag is set, `Goto` opcodes become no-ops. This prevents the VM from following diverts inside the choice evaluation block for a choice that won't be presented. `EndChoice` always clears the flag, so the next choice evaluates normally.

### Choice Presentation and Selection

After the choice set is fully evaluated, a `Done` opcode yields control to the story layer. At this point `flow.pending_choices` contains one `PendingChoice` per available choice. The story layer has two paths:

- **All invisible defaults** — every pending choice has the `is_invisible_default` flag. The story auto-selects the first one and continues without yielding to the caller. The player never sees these.
- **Visible choices exist** — invisible defaults are filtered out of the presentation list, and the remaining choices are returned to the caller as `StepResult::Choices`. The caller picks an index, calls `story.choose(index)`, and the runtime restores the fork + jumps to the target.

### Data Structures

```rust
struct PendingChoice {
    display_text: String,       // assembled from string capture
    target_id: DefinitionId,    // choice body container
    target_idx: u32,            // resolved container index
    target_offset: usize,       // byte offset within container
    flags: ChoiceFlags,         // metadata (once-only, fallback, etc.)
    original_index: usize,      // position in choice set
    tags: Vec<String>,          // # tags attached to choice
    thread_fork: Thread,        // call stack snapshot from evaluation time
}

struct ChoiceFlags {              // packed as single byte
    has_condition: bool,          // 0x01 — BeginChoice should pop a condition
    has_start_content: bool,      // 0x02 — display string includes start text
    has_choice_only_content: bool,// 0x04 — display string includes bracket text
    once_only: bool,              // 0x08 — skip if target already visited
    is_invisible_default: bool,   // 0x10 — never shown to player
}
```

The flags serve double duty: they tell the VM what to pop from the stack (`has_condition`, `has_start_content || has_choice_only_content`), and they encode behavioral semantics (`once_only`, `is_invisible_default`). Note that `has_start_content` and `has_choice_only_content` don't cause separate pops — the compiler combines start + choice_only into a single string. The flags are metadata for the story layer and debugging.

### Opcode-by-Opcode Execution

This is the precise sequence the VM walks through when evaluating a choice set:

**`BeginChoiceSet`:** clears `flow.pending_choices`, starting a fresh evaluation.

**`BeginStringEval`:** calls `flow.output.begin_capture()` — pushes a checkpoint marker into the output buffer. All subsequent `EmitLine` calls write into the capture region instead of the main output.

**`EndStringEval`:** calls `flow.output.end_capture()` — drains everything after the checkpoint, resolves glue, pushes the resulting string onto `flow.value_stack` as `Value::String`. The main output is unaffected.

**`BeginChoice(flags, target_id)`** — the core handler:

1. **Condition check:** if `has_condition`, pop value stack. If falsy → pop any display text too (stack balance), set `skipping_choice = true`, return early.
2. **Once-only check:** if `once_only`, check `visit_counts[target_id]`. If > 0 → same skip protocol as condition failure.
3. **Display text:** if `has_start_content || has_choice_only_content`, pop one string from stack. Otherwise use empty string.
4. **Thread fork:** snapshot current thread's call stack (copy-on-write via `snapshot()`).
5. **Create `PendingChoice`** with display text, target, flags, tags, and thread fork.
6. Push onto `flow.pending_choices`.

**`EndChoice`:** always sets `flow.skipping_choice = false`, regardless of whether the choice was skipped.

**`EndChoiceSet`:** no-op (bookkeeping only).

**`Done`:** yields execution. The story layer inspects `pending_choices` and either auto-selects (all invisible defaults) or presents to the caller.

### Choice Selection (`select_choice`)

When the caller calls `story.choose(index)`:

1. Validate index is within range of `pending_choices`
2. `swap_remove` the `PendingChoice` at that index
3. **Increment visit count** for `target_id` — this is what makes once-only choices disappear on subsequent visits
4. **Set turn count** for `target_id` — enables `TURNS_SINCE()` queries
5. **Restore thread fork** — replace the current thread entirely with the snapshot from evaluation time
6. **Clear container stack, set position** to `(target_idx, target_offset)` — the choice body container
7. Clear all pending choices, set status to `Active`

**Critical invariant:** globals and visit counts are NOT rolled back on selection. Only the call stack (flow state) is restored from the fork. If choice evaluation modified globals (rare but legal), those modifications persist regardless of which choice the player picks.

---

## Converter Reference (`brink-converter`)

**File:** `crates/internal/brink-converter/src/codegen.rs`

The converter is much simpler because inklecate's `.ink.json` has already done the heavy lifting. Each choice is a pre-assembled container structure with:

- String evaluation containers for display text
- Conditions already evaluated in the right order
- `ChoicePoint` with flags directly from inklecate

```rust
fn emit_choice_point(&mut self, cp: &ChoicePoint) -> Result<(), ConvertError> {
    let id = self.resolve_divert_target(&cp.target)?;
    let flags = ChoiceFlags {
        has_condition: cp.flags.contains(ChoicePointFlags::HAS_CONDITION),
        has_start_content: cp.flags.contains(ChoicePointFlags::HAS_START_CONTENT),
        has_choice_only_content: cp.flags.contains(ChoicePointFlags::HAS_CHOICE_ONLY_CONTENT),
        once_only: cp.flags.contains(ChoicePointFlags::ONCE_ONLY),
        is_invisible_default: cp.flags.contains(ChoicePointFlags::IS_INVISIBLE_DEFAULT),
    };
    self.emit(&Opcode::BeginChoice(flags, id));
    self.emit(&Opcode::EndChoice);
    Ok(())
}
```

The converter emits `BeginChoice`/`EndChoice` with no stack setup — the surrounding container structure (from inklecate's JSON) already handles string eval and condition pushing via `enter_container` calls.

---

## End-to-End Example

For the ink source:

```ink
* "I am tired[."]," I repeated.
* [Stay silent]
    You say nothing.
- The conversation continues.
```

### Parser Output (CST)

```
CHOICE
  CHOICE_BULLETS: *
  CHOICE_START_CONTENT: "I am tired"
  CHOICE_BRACKET_CONTENT: [."]
  CHOICE_INNER_CONTENT: ," I repeated.

CHOICE
  CHOICE_BULLETS: *
  CHOICE_BRACKET_CONTENT: [Stay silent]

CONTENT_LINE: "You say nothing."

GATHER
  GATHER_MARKS: -
  CONTENT_LINE: "The conversation continues."
```

### HIR

```
ChoiceSet {
  choices: [
    Choice {
      is_sticky: false,
      is_fallback: false,
      start_content: Content(["I am tired"]),
      bracket_content: Content([".""]),
      inner_content: Content(["," I repeated."]),
      body: Block([]),
    },
    Choice {
      is_sticky: false,
      is_fallback: false,
      start_content: None,
      bracket_content: Content(["Stay silent"]),
      inner_content: None,
      body: Block([Content("You say nothing."), EndOfLine]),
    },
  ],
  gather: Gather {
    content: Content(["The conversation continues."]),
  },
}
```

### LIR

```
Container(root) {
  body: [ChoiceSet {
    choices: [
      Choice { target: c-0, start: "I am tired", choice_only: ".\"", inner: ",\" I repeated." },
      Choice { target: c-1, start: None, choice_only: "Stay silent", inner: None },
    ],
    gather_target: g-0,
  }]
  children: [
    Container(c-0, ChoiceTarget, VISITS|COUNT_START_ONLY) {
      body: [ChoiceOutput("I am tired,\" I repeated."), EndOfLine, Goto(g-0)]
    },
    Container(c-1, ChoiceTarget, VISITS|COUNT_START_ONLY) {
      body: [ChoiceOutput(""), EndOfLine, EmitContent("You say nothing."), EndOfLine, Goto(g-0)]
    },
    Container(g-0, Gather) {
      body: [EmitContent("The conversation continues."), EndOfLine, Done]
    },
  ]
}
```

### Bytecode

```
# root container
BeginChoiceSet
  # choice 0
  BeginStringEval
    EmitLine 0          # "I am tired.\""  (start + choice_only combined)
  EndStringEval
  BeginChoice(cond=false, start=true, choice_only=true, once=true, fallback=false, target=c-0)
  EndChoice
  # choice 1
  BeginStringEval
    EmitLine 1          # "Stay silent"  (choice_only only)
  EndStringEval
  BeginChoice(cond=false, start=false, choice_only=true, once=true, fallback=false, target=c-1)
  EndChoice
EndChoiceSet
Done

# c-0 container
EmitLine 2              # "I am tired,\" I repeated."  (start + inner)
EmitNewline
Goto g-0

# c-1 container
EmitNewline             # empty ChoiceOutput
EmitLine 3              # "You say nothing."
EmitNewline
Goto g-0

# g-0 container
EmitLine 4              # "The conversation continues."
EmitNewline
Done
```

### Runtime Execution (player picks choice 0)

1. **BeginChoiceSet** → clear pending choices
2. **BeginStringEval** → begin capture
3. **EmitLine 0** → "I am tired.\"" captured
4. **EndStringEval** → push `"I am tired.\""` to value stack
5. **BeginChoice** → no condition, not visited → pop display text, fork thread, create PendingChoice { text: "I am tired.\"", target: c-0 }
6. **EndChoice** → clear skipping
7. (repeat for choice 1)
8. **Done** → yield. Story layer sees 2 visible choices, presents them.
9. Player picks index 0. **`choose(0)`:**
   - Increment visit count for c-0
   - Restore thread fork
   - Set position to c-0 offset 0
10. **EmitLine 2** → "I am tired,\" I repeated." to output
11. **EmitNewline** → confirm line
12. **Goto g-0** → enter gather container
13. **EmitLine 4** → "The conversation continues."
14. **EmitNewline** → confirm line
15. **Done** → yield final text
