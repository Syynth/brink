# Collections

The dialect adds two literal forms, both **sigils** — a leading `#` marks
them as dialect syntax before the parser has to decide anything else:

- **Array**: `#[expr, expr, …]` — trailing comma allowed, `#[]` for empty.
- **Map**: `#{key: expr, key: expr, …}` — trailing comma allowed, `#{}` for
  empty. Keys are restricted to the ratified key domain — `int`, `string`,
  `bool` — the same domain every other map-keyed operation in the value
  model uses. The analyzer warns when a key's type is statically visible and
  outside that domain; an actual out-of-domain key is also a runtime fault
  (see [Indexing & Mutation](./indexing.md)).

```ink
~ {
    temp m = #{"z": 1, "a": 2, "m": 3}
    for k in m {
        keys = keys + k
    }
}
```

Nesting is unrestricted — `#[#{a: 1}, #{a: 2}]` is a two-element array of
one-entry maps, and `grid = #[#[1, 2], #[3]]` is exactly how you'd build a
ragged 2-D grid.

## Why sigils, and why only here

Every existing scripting-language convention for array/map literals —
`[1, 2, 3]`, `{a: 1}` — collides with something ink already uses those
characters for (choice brackets, weave structure). The `#` sigil sidesteps
that: `#` cannot begin an ordinary ink expression anywhere the grammar
already permits one, so `#[…]`/`#{…}` are unambiguous wherever they're legal
syntax:

- `~` lines, block statements, call arguments, condition expressions — all
  **expression position**, and all fully supported.

## Legal in expression position only

Collection literals are **not legal in prose position** — you can't write
`Loot: #[10, 20].` as narrative text. This isn't an arbitrary restriction;
it's forced by ink's own grammar. `#` already means something in prose: it
opens a **tag** (`Some text # a_tag`), and tags legally contain `{}`
interpolation. `#{…}` mid-prose is genuinely ambiguous with tag syntax —
there is no clean way to tell "a map literal" from "a tag that happens to
start with a brace" without either breaking existing tags or making the
grammar context-sensitive in a way that would leak into the formatter and
IDE.

Expression position has no such clash — `#` can never begin an ordinary ink
expression there, so the sigil is collision-free. This is the honest scope
of "collision-proof": true in expression position, not true in prose.

## The pattern: build in a `temp`, interpolate the temp

Since a literal can't appear directly in prose, the idiom is to build the
collection (or the piece of it you want to show) in a `~ { … }` block first,
then interpolate the resulting `temp`/`VAR` with ink's ordinary `{…}`
interpolation — which was never restricted, because it isn't the collection
literal syntax, just a normal expression reference:

```ink
VAR arr = 0

~ {
    arr = #[]
    push(arr, 1)
    push(arr, 2)
    push(arr, 3)
}

Arr is {arr}.
-> END
```

```text
Arr is [1, 2, 3].
```

Read that as: compute first, narrate second. It's the same discipline plain
ink already asks for when a `~` line's result needs to reach prose — this
just extends it to values that happen to be collections.
