---
"@brink-lang/web": patch
---

T1e-3: path-projections tooling tail (docs/t1e-spec.md §8 item 3, issue
#850). Closes the T1e milestone (#828).

- **Fixed a display bug**: a `#fn`/closure-bound `ref` parameter captured
  via a path projection (`#fn(heal, ref npc.hp)`) rendered its display form
  as `fn heal(ref hp = ref npc.hp, amount)` — the projection's own `ref `
  prefix nested inside the outer `ref hp = ` the fn-value display already
  supplies. Now renders `fn heal(ref hp = npc.hp, amount)`, matching the
  spec's `ref npc.inventory[3]` path-display convention. Fixed at both the
  runtime (`string(f)`/interpolation, `brink_runtime::value_ops`) and the
  static IDE hover renderer (`brink-ide`'s `fn_value_hover`) that mirrors
  it, so `@brink-lang/web`'s hover surface picks up the same correction.
- **Completion**: right after typing `ref ` in a call's argument position,
  completion now offers only durable `VAR`s (the only legal `ref
  lvalue-path` root, E080) instead of the full argument-position set
  (which also includes `CONST`/param/temp — none of them legal ref roots).
  Path *continuations* after a `.`/`[` aren't attempted (needs the root's
  resolved shape, out of scope for "where cheap").
- **`brink-fmt`**: `ref lvalue-path` arguments inside a `~ { … }` block now
  format with the canonical zero-space convention around `.`/`[`/`]`
  (`ref npc.hp`, `ref inventory[idx]`), matching the display form's own
  spacing rather than preserving whatever spacing the author typed.
- **bevy-brink pass-through audit**: added end-to-end tests locking in that
  a path-projection ref-argument can never reach an `EXTERNAL`/host binding
  as a raw `Value::Projection` — structurally impossible today (an
  `EXTERNAL` declaration has no `ref`-parameter grammar), and any value
  *derived* from reading a projection-bound parameter inside ink always
  arrives at a binding pre-resolved to a plain snapshot.
- New book chapter, "Path Projections"
  (`docs/book/src/toolchain/dialect/path-projections.md`), with
  compile-checked `ink`/`text` examples following the Function Values
  chapter's precedent.
