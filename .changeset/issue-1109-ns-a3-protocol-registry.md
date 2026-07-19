---
"@brink-lang/web": patch
---

NS-A3 (#1109): the protocol registry machinery — the CLOSED
`display`/`compare`/`iterate` set with per-protocol effect contracts.
Observable through `@brink-lang/web`, brink dialect only:

- **New hard diagnostic E113** (F6, ruled 2026-07-19): the registry method
  names `display`/`compare`/`next` are reserved — an author declaration of
  any callable or value-bindable kind (knot/stitch/function, param, temp,
  VAR, CONST, EXTERNAL, for-loop variable) is a compile error, not an
  E035-style warning. (E114/E115 — impl contract/shape validation — also
  ship, but impl registration has no source spelling until the
  code-dialect sitting, so they are unreachable from `.ink` input.)
- **Struct display gains its structural default** (F1: one display path):
  interpolating or `string()`-ing a whole struct now renders
  `Name { field: value, … }` in declared field order (previously a
  provisional positional `{1, 2}`), recursing through nested
  structs/collections/Options; `string()` stays total (a stale shape from
  a foreign save falls back to the positional form).

Vanilla-ink stories are unaffected; the oracle corpus is byte-identical.
