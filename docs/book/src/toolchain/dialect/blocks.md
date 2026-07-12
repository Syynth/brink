# Logic Blocks

Plain ink's logic layer is line-oriented: one `~` line is one statement, and
control flow across lines happens by weaving through choices and diverts. The
brink dialect adds a second shape: a **multi-line logic block**, opened by a
`~` line whose expression is `{`.

```ink
VAR items = 0
VAR total = 0

~ {
    items = #[10, 20, 30]
    total = items[0] + items[1] + items[2]
}

Total is {total}.
-> END
```

Inside the braces, statements are newline-terminated and don't repeat the `~`
sigil — you write `total = total + item`, not `~ total = total + item`, on
every line.

## What a block can do

- **Assignment**, including indexed lvalues (`grid[y][x] = v` —
  see [Indexing & Mutation](./indexing.md)).
- **`temp` declarations**, block-scoped (see below).
- **`if` / `else if` / `else`**, braced:

  ```ink
  ~ {
      temp score = 72
      if score >= 90 {
          label = "A"
      } else if score >= 80 {
          label = "B"
      } else {
          label = "F"
      }
  }
  ```

- **`while cond { … }`** and **`for name in expr { … }`**. `for x in arr`
  iterates array values; `for k in map` iterates map keys, in deterministic
  insertion order (never hash order — the value model guarantees this).
  There's no index/pair destructuring in this slice of the dialect: if you
  need the index too, keep a counter `temp`.
- **`break` / `continue`**, only inside an enclosing `while`/`for` — using
  either outside a loop is a compile error (`E057`).
- **`return`** / **`return expr`** — the only flow-control construct a block
  is allowed to contain.
- **Expression statements** — a function or external call used for its side
  effect (including the stdlib mutators, [Standard Library](./stdlib.md)).

`while`/`for` bodies run under the same VM step budget as every other
bytecode path, so a runaway loop fails loudly (a step-limit fault) instead of
hanging the story.

## The pure-logic fence

This is the load-bearing rule of the whole feature: **a block computes; it
never weaves.** Text output of any kind, choices, gathers, diverts (`->`),
tunnels, and threads are all rejected inside `~ { … }` — not with a parse
error (the grammar accepts the shape), but with a targeted compile error at
lowering time. `return` is the *only* flow construct a block may contain.

Put differently: no weave concept is allowed to appear in an expression or
statement position. This mirrors a hygiene rule that already exists deeper in
the compiler, between the "logic" and "narrative" halves of the low-level IR
— blocks just extend that same seam up to the surface language.

Why draw the line here, and not let a block `-> knot` or present a choice?
Two reasons:

- **It keeps the seam legible.** The moment logic can jump the story around,
  "what does this block do" stops being a local question — you have to trace
  where control goes. Ink's existing weave (choices, gathers, diverts) is
  already the right tool for that; a block staying pure logic means you never
  have two competing ways to express the same flow-control idea.
- **Loosening it later is safe; tightening it wouldn't be.** Shipping a
  narrow, purely-computational block now and adding weave capability to it
  in a later round is an additive change with no existing programs to break.
  Shipping the wide version first and discovering it needs to be narrowed
  would be a breaking change to every story that used the wide surface.

## `temp` scoping and shadowing

A `temp` declared inside a block is **block-scoped**: it's visible for the
rest of that block (and any nested `if`/`while`/`for` body within it), and it
goes out of scope at the closing `}`. It may shadow an already-visible outer
`temp` — either a classic non-block `~ temp` or a `temp` from an enclosing
block scope — but doing so emits a warning, `E054`
(`block-scoped temp shadows an already-visible temp`), not an error. Classic,
non-block `~ temp` semantics outside blocks are unchanged by any of this.

```ink
~ {
    temp x = 1
    if true {
        temp x = 2   // warns: E054, shadows the outer `x`
        x = x + 1
    }
}
```

Shadowing a loop variable follows the same rule — a `for`/`while` body that
redeclares the loop's own name, or an outer visible name, warns rather than
fails.
