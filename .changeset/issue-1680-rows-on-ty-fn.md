---
"@brink-lang/web": patch
---

Analyzer: `Ty::Fn` now carries an **effect row**, and the unifier joins it
(issue #1680 steps 2 and 3 — `docs/effects-spec.md` §5 / the new §6.1c).

The row is the structural set of in-project **creation targets** whose fn
values may inhabit a slot — the keys effects-spec §7's `DefinitionId → row`
table is looked up by, not a computed row. It is minted only at syntactic
creation sites (a `#fn(target, …)` literal, and a global cell's `#fn`
initializer through the declaration-derived signature path), carried through
`bind` unchanged, and joined by `unify` as set union with an absorbing
`unknown` top element — so a slot accumulates every fn value assigned into it
"through copies, parameters, returns, and nesting", and a single untraceable
source keeps the slot conservative.

**The diagnostic surface is deliberately unchanged.** Effect rows are inferred
provenance, never part of the written type language, so they must not decide
whether an argument fits a parameter: the new `infer::assignable` erases rows
on both sides and replaces the structural `unify(param, arg) == param` test at
all four assignability checks — both `ValueCallKind::ArgMismatch` sites,
`annotations`' `E063`, and `structs`' `E071`. Without that, two `fn(int): int`
values born at different targets would join to a third row and fire an `E063`
whose own message is self-refuting ("expected `fn(int): int`, found
`fn(int): int`"), promoted to a hard **error** under `types = strict`.

No new diagnostic, none removed, and no change to emitted bytecode — rows live
only in the analyzer's type universe. What this unblocks rather than delivers
is §6 mechanism 3 (the heap): effect inference still cannot read the
type-carried row, because that walk runs with empty globals and empty
signatures by design. Which stratum should read it is the open question §6.1c
now names.
