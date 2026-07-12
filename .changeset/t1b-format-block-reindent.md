---
"@brink-lang/web": patch
---

Formatting a T1b `~ { … }` multi-line logic block (docs/t1b-surface-spec.md
§2) now reindents its internals instead of passing them through verbatim
(#573): 4-space indent per nesting level inside the block, opening brace on
its statement's line, closing brace on its own line at the parent depth,
one statement per line, comments and blank lines preserved in place
(blank-line runs collapse to one, trailing comments stay attached). This
supersedes T1b-1's verbatim pass-through contract. Everything outside `~ {
… }` blocks is untouched.

This is reachable from the web playground: `brink-web` depends on
`brink-ide`, which calls `brink_fmt::format` in `code_actions.rs` and
`formatting.rs`; `brink-web` exposes those as the `code_actions` /
`code_actions_doc` / `resolve_code_action` wasm-bindgen methods (the
"Format knot" / "Format stitch" code actions). A knot or stitch containing a
`~ { … }` block now formats differently through the playground.
