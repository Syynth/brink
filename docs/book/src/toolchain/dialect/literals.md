# Collections

The dialect adds two literal forms, both **sigils** — a leading `#` marks
them as dialect syntax before the parser has to decide anything else:

- **Array**: `#[expr, expr, …]` — trailing comma allowed, `#[]` for empty.
- **Map**: `#{key: expr, key: expr, …}` — trailing comma allowed, `#{}` for
  empty. Keys are restricted to the ratified key domain — `int`, `string`,
  `bool` — the same domain every other map-keyed operation in the value
  model uses. An out-of-domain key is caught **only at runtime today** — a
  turn-terminating construction fault when the map literal executes (see
  [Indexing & Mutation](./indexing.md)). There is no compile-time warning:
  `lower_map_literal` const-folds in-domain keys but simply falls through to
  the runtime `MapNew` path for any statically-visible non-key type (float,
  null, array, map), and no diagnostic is pushed on that path. If a
  compile-time warning here is spec-mandated, that's a spec/implementation
  divergence — flagged, not asserted as shipped behavior.

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

## A bare `#` in prose swallows the rest of the line

Because `#` opens a tag anywhere in prose, an author writing a literal `#`
into narrative text — a hashtag, a shorthand for "number", a stray comment
marker — silently turns everything from that `#` to the next `#` or the end
of the line into **tag data**. It never reaches the printed text, and
brink emits no diagnostic:

```ink
This costs # 5 dollars, more or less.
-> END
```

```text
This costs
```

`# 5 dollars, more or less.` is consumed whole as one tag's text (readable
via the `Line`/`Choice` `tags` field — see [Runtime API](../reference/runtime-api.md))
— it never prints. This is not a brink bug: it's stock ink behavior, byte-
for-byte identical to the reference C# implementation and inklecate for this
input (verified directly against a local build, issue #858). If prose text
needs a literal `#`, the only way to keep it in the printed line today is to
avoid a bare `#` in content position entirely — e.g. spell out "number" or
move the `#` into a tag deliberately and give it its own line.

## Trailing whitespace on a printed line is stripped

Whitespace at the end of a line of content — including whitespace that comes
*after* interpolated `{…}` content — never reaches the printed text, even
though it's present in the source:

```ink
VAR name = "World"
Hello,    
Hello, {name}    
-> END
```

```text
Hello,
Hello, World
```

Both lines lose their trailing spaces; the second loses them even though
they sit after the interpolation is resolved, not after literal source text.
Again, this matches the reference C# ink compiler exactly (verified against
inklecate, issue #858) — it is not a brink-specific stripping bug to fix.
Leading and *internal* whitespace are unaffected; only the run of whitespace
immediately before the line's terminating newline is dropped.

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
