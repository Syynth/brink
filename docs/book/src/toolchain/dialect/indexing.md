# Indexing & Mutation

## Reading and writing an element

Postfix indexing works in expression position on either collection kind, and
chains:

```ink
a[0]
m["k"]
grid[y][x]
```

Indexed assignment is a statement (a `~` line, or a block statement) and
chains the same way:

```ink
a[0] = v
m["k"] = v
grid[y][x] = v
```

```ink
VAR grid = 0
VAR result = 0

~ {
    grid = #[#[1, 2], #[3, 4]]
    grid[0][1] = 99
    grid[1][0] = grid[1][0] + 100
    result = grid[0][1] + grid[1][0]
}

Result is {result}.
-> END
```

```text
Result is 202.
```

## How a write reaches the collection

An indexed write never mutates "in place" from the language's point of view
— brink collections are values, not references, and the compiler doesn't
special-case indexing to pretend otherwise. `a[0] = v` (and, one level
deeper, `grid[y][x] = v`) lowers to a **read → modify → write-back** sequence
on the root variable: read the container out, apply the change, write the
whole container back to `a`. A chained path like `grid[y][x] = v` lowers to
*nested* read-modify-write, one level at a time — never to an interior
reference into the array. This is exactly the discipline the value model
specifies for the language generally, so the fact that a write is efficient
in practice (the runtime shares the underlying storage until something
actually forks it, and mutates in place once the last owner is doing the
writing) is never something a brink program needs to reason about; the
observable behavior is always "as if" a fresh copy.

There's a limit to how deep this goes in this round of the dialect: a chain
like `grid[y][x]` is nested indexing, lowered to nested read-modify-write —
there's no way yet to take a standalone reference into the middle of a
collection and hand it around (that's a later, separate piece of the
language).

## Faults

Plain ink is famous for tolerating a lot — a missing content path doesn't
crash the story. Indexing breaks from that: out-of-bounds and missing-key
access are **turn-terminating runtime faults**, not values that quietly
become `null` or an empty result. Every one of these ends the current turn,
the same way dividing by zero already does:

| Situation | What happens |
|---|---|
| `a[i]` / `a[i] = v` with `i` outside `[0, len(a))` | Fault — array index out of bounds |
| `m[k]` / `m[k] = v` with `k` not already a key in `m` | Fault — map has no such key |
| Indexing into a value that isn't an array or a map | Fault — not indexable |
| An array index expression that isn't an `Int` | Fault — invalid array index |
| A map key expression outside the key domain (not `int`/`string`/`bool`) | Fault — invalid map key type |

Two points worth being explicit about:

- **A write never grows the array.** `a[i] = v` requires `i` to already be a
  valid, in-bounds index — writing one past the end doesn't append; it
  faults. If you want to grow a collection, use the stdlib mutators
  ([Standard Library](./stdlib.md)) — `push`/`insert` are the only
  operations that add elements, and they say so in their name.
- **An indexed map write never inserts.** `m["new_key"] = v` on a key that
  isn't already present faults, the same as reading it would. Indexing
  assumes the shape is already there; `insert()` is the operation that adds
  a key.

These are total operations with a well-defined failure outcome, not
undefined behavior — a fault is deterministic, gets recorded in the
transcript/journal like anything else the runtime does, and a replay
reproduces it identically. What a host does in response (abort the turn,
show a debug message, roll back to a snapshot) is a host policy question,
not something the ink script has any way to catch — v1 scripts are
infallible from the inside; there's no in-language `try`/`catch` for this.
