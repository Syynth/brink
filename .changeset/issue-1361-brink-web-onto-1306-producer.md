---
"@brink-lang/web": patch
---

#1361: `compile()`/`compile_fragment()` (`crates/brink-web/src/compile.rs`)
now build an in-memory `SourceTree` from the caller-supplied document(s) and
run the #1306 producer — `Project::load(&tree, entry, &overrides)` →
`compile(&env)` — instead of driving a throwaway `Driver` through
`brink_compiler::compile(entry, read_file_closure)`. Observable difference:
`compile()`/`compile_fragment()` now also honor a `brink.toml` if one
happens to be present among the served sources (previously ignored — the
old path used `brink_compiler::compile`'s hardcoded default
`AnalysisOptions` with no config discovery at all). No existing caller
passes a `brink.toml` through either entrypoint today, so this is a nil
delta for current callers of `@brink-lang/web`.

`EditorSession::compile_project` (brink-ide's `IdeSession::compile`, the
live-editing salsa db shared with brink-lsp) is untouched — out of scope for
this issue.
