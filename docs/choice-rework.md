# Choice Design Rework

Items to address in the choice handling pipeline. Each item should be implemented and verified against the episode corpus.

## 1. Remove `divert` field from HIR `Choice`; fold into body

**Current state:** HIR `Choice` has a separate `divert: Option<Divert>` field for inline diverts (e.g. `* hello -> world`). This propagates as `has_inline_divert: bool` on the LIR choice, and as `inline_divert: Option<Divert>` on the LIR `ChoiceOutput` statement. Both codegen backends (bytecode and JSON) special-case this to place the divert before the newline.

**Problem:** The newline-vs-divert ordering is a statement ordering concern. HIR already owns newline emission. The separate field pushes a codegen concern up into the IR and forces both backends to handle it.

**Fix:** During HIR lowering (in `lower_choice` or weave folding), fold the inline divert into `Choice.body` in the correct position — divert before `EndOfLine`. Remove the `divert` field from HIR `Choice`, `has_inline_divert` from LIR `Choice`, and `inline_divert` from LIR `ChoiceOutput`. Both backends just walk the statement list; newline suppression falls out naturally from the divert jumping before the newline is reached.

## 2. Replace `Gather` type with labeled blocks; remove `opening_gather`

### Current state

The HIR has a `Gather` struct with fields for label, content, divert, and tags:

```rust
struct Gather {
    ptr: AstPtr<ast::Gather>,
    label: Option<Name>,
    content: Option<Content>,
    divert: Option<Divert>,
    tags: Vec<Tag>,
}
```

`ChoiceSet` carries two optional gathers:

```rust
struct ChoiceSet {
    choices: Vec<Choice>,
    gather: Option<Gather>,           // convergence point after choices
    opening_gather: Option<Gather>,   // wrapping gather before choices (- * pattern)
}
```

This creates irregularity throughout the pipeline. The `Gather` type bundles three distinct concerns — a label (structural), content/divert/tags (just statements), and its relationship to a choice set (convergence vs opening). Downstream, the LIR has special-case logic for `opening_gather`, the container planner has separate allocation paths for gathers vs opening gathers, and `Gather.divert` has the same problem as `Choice.divert` (item 1).

### The insight

A gather is not a distinct semantic concept. It's a **labeled block** — an optionally-named point in the statement flow that can be addressed (for divert targets and visit counting). The label makes it addressable; the content is just regular statements. These two roles don't need their own type.

### The design

Add an optional label to `Block`:

```rust
struct Block {
    label: Option<Name>,
    stmts: Vec<Stmt>,
}
```

Replace `ChoiceSet` with:

```rust
struct ChoiceSet {
    choices: Vec<Choice>,
    /// The continuation block — where choices converge after selection.
    /// Always present. If the ink has an explicit gather, its label goes
    /// on this block and its content/divert/tags become statements inside it.
    /// If there's no explicit gather, this is unlabeled with trailing stmts.
    continuation: Block,
}
```

Remove `Gather` entirely.

### How each pattern maps

**Basic gather after choices:**

```ink
* Option A
* Option B
- (meeting) Everyone meets here. -> next
```

```
ChoiceSet {
    choices: [A, B],
    continuation: Block {
        label: Some("meeting"),
        stmts: [Content("Everyone meets here."), Divert(next)]
    }
}
```

**No explicit gather (choices at end of knot):**

```
ChoiceSet {
    choices: [A, B],
    continuation: Block { label: None, stmts: [] }
}
```

Unlabeled, so no container allocated by the planner. The LIR creates an implicit gather container as needed.

**Opening gather (`- (label) * choice`):**

```ink
- (label) * Choice A
```

```
Block {
    label: Some("label"),
    stmts: [
        ChoiceSet {
            choices: [A],
            continuation: Block { ... }
        }
    ]
}
```

The labeled block wraps the choice set. No `opening_gather` field needed.

**Gather-choice chains:**

```ink
- (a) * choice 1
- (b) * choice 2
- (c) End.
```

```
Block {
    label: Some("a"),
    stmts: [
        ChoiceSet {
            choices: [1],
            continuation: Block {
                label: Some("b"),
                stmts: [
                    ChoiceSet {
                        choices: [2],
                        continuation: Block {
                            label: Some("c"),
                            stmts: [Content("End."), EndOfLine]
                        }
                    }
                ]
            }
        }
    ]
}
```

**Standalone labeled gather (no choices):**

```ink
- (label) Some text.
```

```
Block {
    label: Some("label"),
    stmts: [Content("Some text."), EndOfLine]
}
```

Just a labeled block. No choice set involved.

### The universal rule

**Labeled block → container.** The LIR container planner walks the HIR tree. Every `Block` with a label gets a pre-allocated container ID. Unlabeled blocks inline into their parent. One rule, no special cases for gathers vs opening gathers vs standalone gathers.

This also unifies with other labeled constructs. Choice labels, gather labels, and knot/stitch names are all the same concept — a named entry point that gets a container, can be a divert target, and participates in visit counting.

### What gets removed

- `Gather` struct from HIR types
- `gather: Option<Gather>` from `ChoiceSet`
- `opening_gather: Option<Gather>` from `ChoiceSet`
- `emit_standalone_gather()` from weave folding
- `lower_gather_choice_chain()` special case in LIR lowering
- Separate gather allocation paths in container planner

### What gets added/changed

- `label: Option<Name>` on `Block`
- `continuation: Block` on `ChoiceSet` (always present)
- Weave folding builds continuations directly instead of attaching gathers
- Container planner: one pass, "label → allocate ID"
- LIR lowering: choice bodies `goto` the continuation's container ID
