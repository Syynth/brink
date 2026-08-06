# Standard Library

The first stdlib slice ships four pure functions and four mutators, all
lowercase free functions — no method-call syntax (`a.push(v)` isn't
supported; it collides with ink's dotted knot/stitch paths).

| Function | Kind | Signature | Result |
|---|---|---|---|
| `len(x)` | pure | array or map → `Int` | element/entry count |
| `keys(m)` | pure | map → `Array` | keys, insertion order |
| `values(m)` | pure | map → `Array` | values, insertion order |
| `contains(x, v)` | pure | array or map, any `v` → `Bool` | array: element membership; map: key membership |
| `push(a, v)` | mutator | array, any `v` | appends `v` |
| `insert(x, k_or_i, v)` | mutator | array or map, key/index, value | array: insert at index (shifts right); map: insert-or-overwrite |
| `remove(m, k)` | mutator | map, key | remove key (no-op if absent) |
| `remove_at(a, i)` | mutator | array, index | remove at index (shifts left); out of bounds faults |

```ink
VAR arr = 0
VAR m = 0

~ {
    arr = #[10, 20, 30]
    m = #{"a": 1, "b": 2}
}

len(arr) = {len(arr)}, len(m) = {len(m)},
contains(arr, 20) = {contains(arr, 20)}, contains(arr, 99) = {contains(arr, 99)},
contains(m, "a") = {contains(m, "a")}, contains(m, "z") = {contains(m, "z")}
-> END
```

```text
len(arr) = 3, len(m) = 2,
contains(arr, 20) = true, contains(arr, 99) = false,
contains(m, "a") = true, contains(m, "z") = false
```

`push(a, v)` is exactly `insert(a, len(a), v)` — it's a named shorthand for
"append," not a separate operation with its own semantics. On an array, the
one write `insert` is allowed to reach one past the current end is `index ==
len` — that's what makes appending well-defined without silent growth
anywhere else: any *other* out-of-range index still faults, matching the
indexing rule in [Indexing & Mutation](./indexing.md).

## `contains` is total

Unlike indexed reads, `contains` never faults — it always answers `true` or
`false`, including on inputs that would fault elsewhere:

- On an **array**, `contains(arr, v)` is a structural-equality scan; `v` can
  be any value at all, including another collection.
- On a **map**, `contains(m, v)` checks key membership. If `v` is outside
  the ratified key domain (a float, an array, a map, …), the answer is
  simply `false` — not a fault. A value that can *never* be a key isn't a
  member; there's no "the key isn't there" failure mode to escalate to a
  runtime fault the way an indexed read (`m[v]`) legitimately does for a
  present-but-wrong-typed key. This makes `contains` behaviorally uniform
  across both container kinds — you don't need to already know a map's key
  domain just to test membership without risking a crash.

  Under `types = strict`, though, when both the map and the needle's
  out-of-domain type are already statically visible, `contains(m, v)` is
  flagged at compile time by `E152` (`Warning` severity) — the call is
  always `false`, and the diagnostic exists to catch what's usually a typo
  or a stale key type rather than intentional code. `E152` is strict-mode
  only (it stays silent under `types = gradual`, where the runtime's total
  `false` above remains the only behavior), and like other warnings it can
  be re-leveled or suppressed via `[lints]` (e.g. `E152 = "deny"` or
  `E152 = "allow"`), a `//brink-disable` comment, or (native dialect only)
  a declaration-scoped `@[allow(E152)]` annotation.

## Mutators require an lvalue

`push`/`insert`/`remove`/`remove_at` mutate their first argument, so that argument has
to be a **place to write the mutated container back into**: a bare variable
or `temp`, or an (arbitrarily chained) indexed path rooted in one —
`grid[1]` is a valid mutator target, a bare literal or call result is not.
Passing anything else is a compile error (`E055`,
"collection mutator's first argument is not an lvalue"):

```ink,error(E055)
~ push(#[1, 2, 3], 4)
// E055: `push` mutates its first argument — bind it to a variable first
```

The rule exists so the surface never implies reference semantics that the
value model doesn't have. A collection is a value; "mutating" it always
means "compute the new value and write it back somewhere" — the lvalue rule
just makes that "somewhere" explicit and mandatory instead of silently
discarding the result.

Because they mutate, the four mutators lower through the identical
take → `make_mut` → write-back path indexed assignment uses, and a nested
target works the same way an indexed assignment's chain does:

```ink
VAR grid = 0

~ {
    grid = #[#[1, 2], #[3]]
    push(grid[1], 4)
    insert(grid[0], 0, 0)
}

Grid is {grid}.
-> END
```

```text
Grid is [[0, 1, 2], [3, 4]].
```

Mutators also **return nothing** — they're statement-only. Using one in
expression position (`~ x = push(a, v)`) is a compile error, `E056`
(`collection mutator used in expression position`).

## Wrong argument count is a compile error

Calling a mutator with the wrong number of arguments — `push(arr)`,
`insert(m, "k")`, `remove_at(arr, 0, 1)` — is a targeted, error-severity compile
error (`E058`, `collection mutator argument count mismatch`) naming the
expected signature. `push(arr)`'s diagnostic message (as returned by
`ResolvedDiagnostic` — see [Enabling the Dialect](./enabling.md) for how the
CLI vs. library API surface it) reads:

```text
collection mutator argument count mismatch: `push` expects 2 argument(s),
got 1 — expected signature: `push(container, value)`
```

This is stricter than ordinary function-call arity checking (`E031`), which
is only a warning and still compiles — a pure stdlib function
(`len`/`keys`/`values`/`contains`) called with the wrong arity keeps using
`E031`, unchanged. Mutators are held to the harder standard because a
malformed mutator statement has no fallback value to silently produce: a
`push`/`insert`/`remove`/`remove_at` call that doesn't lower to anything is a
read-modify-write that never happened, which is exactly the kind of silent
data-drop this project treats as a bug rather than a warning.

## Author-defined functions shadow the builtins

These eight names live in the brink dialect only — a `strict-ink` project
never sees them as reserved words, so plain ink content that happens to
define a knot or function called `len` keeps working unmodified even after
a project turns the dialect on. If an author defines a function with the
same name as a stdlib builtin, the author's definition wins, with a warning
(`E035`, the same "name shadows a built-in function" diagnostic ordinary
built-ins already use):

```ink
VAR arr = 0

~ {
    arr = #[1, 2, 3]
}

len is {len(arr)}.
-> END

=== function len(x: Array<int>)
~ return 999
```

```text
len is 999.
```
