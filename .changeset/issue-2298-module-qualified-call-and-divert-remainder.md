---
"@brink-lang/web": patch
---

Fix #2298: extends #2287/#2296's module-qualified-import fix beyond
diverts. A bare tunnel-function call (`haggle()`, ink allows a knot as a
function via tunnels) after only a module-qualified import
(`use story::market::barter;`, no symbol-level import of `haggle`) now
correctly stays unresolved instead of being wrongly accepted — the same
over-permissive bug #2287 reported for diverts, reproduced at a call
site. `lookup_divert`'s remaining `Stitch`/`Label`/`Variable`+`Constant`
steps share the same exclusion now too (latent for native today, since a
`flow` always classifies `Knot`), and the `Constant` omission on that
`Variable` step (issue #2083's thread) is fixed alongside it — a
`CONST target = -> knot` can now be diverted to via `-> target`. The
resulting `E024`/`E025` diagnostic for a module-imported-but-bare
reference now names the qualified-import-only candidate it skipped
("import it from `module`"), mirroring the framing `modules::check`'s own
E025 already gives an unimported reference.
