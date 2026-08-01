---
"@brink-lang/web": patch
---

Issue #1887: the E119 pure-callback contract gate (`sort_by`/`sorted_by`
and the `map`/`filter`/`fold`/`filter_map` quartet, `docs/stdlib-spec.md`
§4/§4b) now also recognizes a **native bare-name callback** — `map(items,
double)`, the sigil-free spelling ruled 2026-08-01 (#1862) for the
`.brink` surface (`#` is already the tag sigil in native content
position, so `#fn(target)` has no native spelling). Previously the gate
matched a callback argument structurally on the ink/brink `#fn(target)`
literal only, so a native bare-name callback that writes a global,
performs an effectful call, emits content, or touches the tag channel
compiled clean instead of being rejected — this only reaches a project
under brink-dialect analysis over native (`.brink`) source, and only
changes behavior for a callback argument that provably resolves to a
statically-named function definition (an opaque reference — a var,
param, or `bind(…)` result — is unaffected, matching the pre-existing
exceedance-only posture for the ink spelling).
