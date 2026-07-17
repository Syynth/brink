---
"@brink-lang/web": patch
---

Analyzer: new `E106` warning for statically-visible non-key-domain
map-literal keys (docs/t1b-surface-spec.md §3, issue #598).

`#{key: expr, …}` map-literal keys are ratified to the int/string/bool
domain at runtime (`RuntimeError::InvalidMapKeyType`). §3 already claimed
"the analyzer warns on statically-visible non-key types", but nothing
implemented it — `MapLiteral` lowering did zero key-domain checking, so a
float, array (`#[...]`), nested map (`#{...}`), struct (`Name#{...}`),
function-value (`#fn(...)`), or ink `LIST` literal used directly as a key
compiled silently and only failed at runtime.

`brink-analyzer::map_keys::check` now flags every such entry with `E106`
(warning severity), wired into `per_file_diagnostics` unconditionally under
`dialect = brink` (map literals don't exist under `strict-ink` at all —
already rejected whole by the dialect gate's `E051`). Policy-independent
like the construction-literal duplicate-field check (`E084`): fires
identically under both `types = gradual` and `types = strict`, no shape
resolution needed. A dynamic key (a variable, call, index, or any other
non-literal expression) is not statically visible and is never flagged —
the runtime fault remains the sole backstop for those.

Observable through `@brink-lang/web`: any brink-dialect project compiled
through the wasm runtime with a non-key-domain literal map key now surfaces
this new diagnostic in the returned diagnostics array.
