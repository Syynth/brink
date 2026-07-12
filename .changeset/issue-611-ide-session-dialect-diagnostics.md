---
"@brink-lang/web": patch
---

Fixed permanent spurious `E051` ("brink extension") diagnostics in the
playground for `brink`-dialect projects (#611, the wasm-side twin of #599).

`IdeSession::analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection`
(`brink-ide`) all built `AnalysisOptions` with `..AnalysisOptions::default()`
for the `dialect` field, ignoring the session's declared T1b compiler
dialect entirely — `EditorSession.set_language_dialect` (#589/#600) only
gated stdlib completion and signature help, never the background analysis
pass that produces diagnostics. A project opened with brink-dialect syntax
(`~ { … }` blocks, `#[…]`/`#{…}` sigil literals, postfix indexing) kept
showing `E051` on every valid construct no matter what dialect was set.

`set_language_dialect` now forwards into `IdeSession`, which threads the
declared dialect through all four analysis entry points and re-analyzes
immediately (like `set_external_check`/`set_semantic_type_check`). No other
wasm-observable behavior changed.
