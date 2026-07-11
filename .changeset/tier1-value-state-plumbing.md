---
"@brink-lang/web": patch
---

Tier-1 value model, state plumbing (T1a-3, #525): collection values
(`Array`/`Map`) now cross the wasm JSON boundary as structured trees instead of
folding to `null`. `eval_function`/`resume_function_eval` results serialize an
array as `{ "type": "array", "items": [...] }` and a map as
`{ "type": "map", "entries": [{ "key": ..., "value": ... }] }`, preserving
insertion order and each key's scalar type (int/string/bool). A JS binding that
returns a native array or plain object is now read back as an ink `Array`/`Map`
(recursively; object keys become string map keys in JS property order), and an
ink collection passed to a binding marshals to a native JS array/object.
Snapshot-only per value-model spec §8 — the boundary copies trees; the host
never retains a handle into script state. No opcode emits collections yet, so
scalar behavior and the oracle are byte-identical.
