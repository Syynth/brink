---
"@brink-lang/web": patch
---

T1d-2 (#767): manifest handle-kind vocabulary + the `handle<K>` typed-mode
annotation form (`docs/t1d-spec.md` §3). A registered `HostManifest` can now
declare a handle kind — `{ "name": "AudioInstance", "base": "handle" }` — and
the brink-dialect typed annotation grammar gains `handle<K>` (`docs/typed-mode-spec.md`
§3's first amendment), resolving to a new `Ty::Handle(K)` lattice point:
pointwise kind match, cross-kind = `Ty::Conflicted` (the #627 lattice). Under
`types = strict`, a mismatched/unregistered handle kind reuses the existing
`E065`/`E066`/`E061` machinery — no new diagnostic codes. `Ty::Fn` composes
with handle-typed params/returns for free (the existing pointwise row
unification needed no special-casing).

Observable through `@brink-lang/web`:

- `HostManifest`'s `BaseType` (`packages/wasm-types`, re-exported by
  `@brink-lang/web`) gains a `"handle"` variant — a host can register
  `{ "base": "handle" }` semantic types.
- `setHostManifest`'s diagnostics now recognize `handle<K>` annotations: a
  `handle<K>` naming an undeclared/unregistered kind reports `E061` (same
  code, extended message); a declared kind resolves cleanly.

Scope: this slice wires the manifest vocabulary, the grammar/lattice, and
the annotation-firewall/diagnostic-content seams (`per_file_diagnostics`,
`strict::check`'s escape-exemption path). It does not thread the manifest
through the salsa fine-grained-incremental type-inference substrate
(`brink-db`'s FG-2 `solve_scc_query`/`call_edges_query` pipeline, or the
non-salsa `infer_project`/`signature()` seams) — so a genuine cross-kind
handle mismatch detected purely from body-usage inference (as opposed to an
explicit annotation) isn't caught yet. Flagged as a follow-up, not silently
dropped.
