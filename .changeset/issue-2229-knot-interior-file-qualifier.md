---
"@brink-lang/web": patch
---

`brink-ir`: two files that legitimately declare a same-named knot (M-2d —
`native_module_path` always differs per file, so `insert_symbol` lets them
coexist rather than raising a duplicate-definition diagnostic) no longer
fail to compile when both knots contain an unlabeled choice/gather at the
same structural position (issue #2229).

`stamp_container_ids`'s per-knot loop used to qualify a knot's *interior*
anonymous (unlabeled) containers by the knot's own bare name alone, unlike
root content's `root_content_scope_path`, which prefixes `#file:{path}`
(#1504). Two same-named knots then stamped every unlabeled descendant
container at the same structural position (e.g. `start.0.c-0`) to the
identical `DefinitionId`, tripping the `[E060] internal codegen error:
duplicate DefinitionId` guard the moment the project's whole container
tree was walked — the same collision class #2197/#2213/#2215/#2226 already
fixed at three other call sites, now closed at this fourth one.
`brink-web` transitively depends on `brink-ir`, so this is
wasm-observable for any `.brink`/`.ink` source reaching this shape.
