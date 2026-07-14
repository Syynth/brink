---
"@brink-lang/web": patch
---

Closed two M-3 rehydration miss-path gaps disclosed by the renames PR
(#782 / docs/modules-spec.md §5): a saved VAR/CONST/LIST global whose own
name was renamed (`#@was`) — declared-module or bare — now rebinds through
the compiled alias table instead of being dropped as unknown, and a saved
`Value::List`'s active items/origins now deep-rebind on a rename exactly
like `Array`/`Map`/`Record` already did.

- **`SaveState` gains a `global_ids` field** — each saved global's
  compiled `DefinitionId` at save time, keyed by the same name as
  `globals`. Additive and `#[serde(default)]`, so an older save missing
  the field just falls back to the pre-existing unknown-global report — no
  behavior change for saves that don't use `#@was`. This is what lets the
  miss path recover a renamed global's identity: a VAR/CONST/LIST living
  in a **declared** module hashes as `(module, name)`, so the bare name
  string alone can't reconstruct it once the name itself changed.
- **`Value::List` is now deep-rebound** — `load_state`'s recursive
  id-rebind walk previously covered `DivertTarget`/`FnRef`/
  `VariablePointer`/`Closure` and their `Array`/`Map`/`Record` containers,
  but fell through to a no-op for `Value::List` itself; its `items`/
  `origins` `DefinitionId`s are now walked and rebound the same way.
- A global-name miss that resolves via the alias table rebinds silently
  (same discipline as address/global-pointer misses); still unresolved
  (only checked for a program that carries alias-table entries at all)
  reports through `LoadReport::unresolved_renames` alongside the existing
  `unknown_globals` entry.

Compat: `SaveState`'s JSON shape gains one field (`global_ids`) — decoders
that deserialize leniently (ignore unknown/extra fields) are unaffected;
`StoryRunner`/`StorySession`'s `save_state`/`load_state` round-trip it
transparently.
