---
"@brink-lang/web": patch
---

B1b (#1475): the `as` binding lands on the native `.brink` surface — one
construct in both of the language's condition positions, per the ruling in
`docs/decision-log.md` 2026-07-26 ("The `as` binding: one construct, both
condition positions, `{if}` spelling"):

- **Statements:** `if EXPR as NAME { … }`, `while EXPR as NAME { … }`
  (the `while` form rebinds on every iteration).
- **Templates:** `{if EXPR as NAME: … else: …}`, riding the already-ruled
  `{if}` spelling rather than a second binding grammar.

The binding is immutable, typed `T` from the condition's `Option[T]`, and
scoped strictly to the success arm — an `else`/`else if` arm never sees it.
For v1 the binding must be the **entire** condition; composing it with
`&&`/`||` is an error (let-chains can land later, additively).

One new opcode, `OptionBind(slot)` at `0xFC`: it pops an `Option`, writes
the unwrapped payload into the binding's temp slot on `some`, and pushes
the bool the construct branches on. The web package's disassembly view
(`program_model.rs`) and the `.inkt` text format (read + write) both gain
the `option_bind` mnemonic — this is the web-observable surface of the
change. Vanilla-ink and brink-dialect stories are byte-identical and the
oracle corpus is unaffected: the new opcode, node kind and HIR fields are
reachable only through native `.brink` lowering.

New diagnostics: `E145` (an `as` over a `&&`/`||` composition), `E146` (an
`as` in a choice guard — ruled, but sequenced with the `.inkb` v6 Choice
record, so it is diagnosed as *not yet supported* rather than half-
lowered), `E147` (an `as` over a statically known non-`Option` condition),
`E148` (a write to a binding). The runtime gains a matching
`AsBindingNotOption` fault as `E147`'s gradual-mode residual.

F27's `E116` ("an `Option[T]` has no truthiness") no longer fires on a
condition that carries an `as` binding — the binding is the third explicit
spelling that ruling named, alongside `== none` and `== some(x)`.
