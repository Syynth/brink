---
"@brink-lang/web": patch
---

#1674: anonymous-container state — report it, and lint the opt-in.

- **`LoadReport` gains `anonymous_states_dropped: u32`.** A saved visit/turn
  count for an anonymous scope (an unlabeled once-only choice or a
  sequence — no author `(label)`) that no longer resolves against the
  current program is counted here, instead of being silently retained
  under an orphaned id. Additive, wire-visible through `@brink-lang/web`'s
  `load()`/`loadBytes()` JSON (`saveState.load`/`Story.loadState`
  equivalents): the field is always present now (`0` on a clean load),
  where it was previously absent from the shape entirely.
- **New diagnostic `E157`** — an unlabeled once-only choice, or a sequence
  with genuine durable state, carries an anonymous, position-derived
  identity that a later content edit can shift. **Off/info by default**
  (`Severity::Info`, immune to `deny-warnings` unless explicitly raised) —
  a project that never touches `[lints] E157` sees no new build-breaking
  behavior, but the wasm editor session's own diagnostics list (which
  already renders every code's `effective_severity`, `EditorSession`'s
  `apply_project_config`/`apply_lint_overrides`) will start surfacing this
  as a new `Info`-tier entry for any unlabeled once-only choice or
  qualifying sequence already in a project's source. Tier-able through
  `[lints] E157 = "warn"/"deny"/"hint"/"allow"` like any other diagnostic.

Oracle-neutral: neither change touches compiled `StoryData`/bytecode, so
the oracle corpus is byte-identical.
