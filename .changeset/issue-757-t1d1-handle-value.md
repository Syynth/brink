---
"@brink-lang/web": patch
---

T1d-1 (#757): `Value::Handle` — the runtime + format spine for opaque
host-resource tokens (`docs/t1d-spec.md` §2/§6), the first emission of the
V4-reserved `VAL_HANDLE` wire tag. No literal syntax and no new opcode —
handles enter the script world only via bindings. Observable through
`@brink-lang/web`:

- **Native binding-argument marshal** (`value_to_js`): a handle passed as
  an argument to a JS-implemented external now crosses as a plain object
  `{ kind, id }` (`kind` the raw manifest `NameId`, `id` a decimal string
  so a full-range `u64` never loses precision as an `f64`) instead of
  silently folding to `null` (the #667 wildcard-arm hazard class).
  Deliberately **not** reconstructed by `js_to_value` — letting any JS
  object shaped `{kind, id}` become a real `Handle` would let a binding
  forge a capability token out of thin air.
- **Speculation / eval-function results** (`value_to_typed_js`): a handle
  crosses the typed-value JSON boundary as `{ type: "handle", kind, id }`
  — `kind` resolved to its manifest name where possible (`"?"` for a stale
  `NameId`), `id` as a decimal string for the same precision reason.
- **Program model / disassembly**: a handle default value (reachable once
  T1d-2 wires manifest-aware bindings into declaration defaults) renders
  as `handle <Kind>#<id>`, not `null`.

Runtime-side (not directly `@brink-lang/web`-observable, but load-bearing
for the above): `Value::Handle { kind: NameId, id: u64 }` with token
equality (`kind == kind && id == id`), no ordering (any `<`/`>`/`<=`/`>=`
is a runtime `TypeError` fault), and never a legal map key. `string(h)`
displays as `handle <Kind>#<id>`. Handles save/load and journal-replay as
ordinary values. The `.inkt` textual format gains a matching `(handle
<kind> <id>)` atom and `:handle` declared-type keyword, both with a real
reader landing in this same PR (the #742 write/read-asymmetry class this
PR does not repeat for its own new atom).
