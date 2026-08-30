---
"@brink-lang/studio": patch
---

The last `brink.toml` key without a Settings surface: `unprune-dirs`

An audit of the config schema against the Settings UI found twelve of the
thirteen settable keys had a surface and one did not. `[project]
unprune-dirs` names which of discovery's always-skipped directories
(`target`, `.git`, `node_modules`) a project wants walked anyway — for a
project that genuinely keeps story files in one of them.

It is three checkboxes rather than another free-text list, because the
value set is closed: naming anything outside those three un-prunes
nothing, and the config parser already answers such an entry with "it was
never pruned, so this has no effect". A text field could only produce one
of three right answers or a silent typo.

The three names restate a Rust constant, so a test reads
`brink_source_tree::IGNORED_DIR_NAMES` out of the source and compares,
rather than repeating the names and agreeing with itself forever.
