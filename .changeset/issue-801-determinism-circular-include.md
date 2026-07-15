---
"@brink-lang/web": patch
---

Circular-`INCLUDE` error messages are now deterministic.

`IncludeGraph::find_cycle` (`crates/internal/brink-db/src/include_graph.rs`)
previously picked its DFS start node from a `HashMap`'s key iteration order,
so which rotation of a multi-file `INCLUDE` cycle got reported in
`DiscoverError::CircularInclude` depended on that map's per-process
`RandomState` seed. `brink-web`'s wasm-exported `compile` / `compile_fragment`
/ `compile_project` (`crates/brink-web/src/compile.rs`, `session.rs`) reach
this path through `brink_compiler::compile` -> `brink_driver::discover` ->
`ProjectDb::find_cycle`, and surface the message verbatim into the JSON
`error` field. A multi-file project with a circular `INCLUDE` chain compiled
through `@brink-lang/web` now gets a stable, reproducible cycle-rotation
string across runs instead of one that could vary process to process.
