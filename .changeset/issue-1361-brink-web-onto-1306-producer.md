---
"@brink-lang/web": patch
---

#1361: `compile()`/`compile_fragment()` (`crates/brink-web/src/compile.rs`)
now build an in-memory `SourceTree` from the caller-supplied document(s) and
run the #1306 producer — `Project::load(&tree, entry, &overrides)` →
`compile(&env)` — instead of driving a throwaway `Driver` through
`brink_compiler::compile(entry, read_file_closure)`. Two observable
differences:

- `compile_fragment()` now honors a `brink.toml` if one happens to be
  present among the served `sources_json` (previously ignored — the old
  path used `brink_compiler::compile`'s hardcoded default
  `AnalysisOptions` with no config discovery at all). `compile()` cannot
  observe this: its `InMemory` tree only ever contains the single
  `"main.ink"` key, so `Project::load` never discovers a `brink.toml` for
  it. No existing caller passes a `brink.toml` through `compile_fragment()`
  today, so this is a nil delta for current callers of `@brink-lang/web`.
- `compile(source)` previously served `source` verbatim for *every*
  requested path (`|_path| Ok(source.to_owned())`), so an `INCLUDE foo.ink`
  in a single-source playground compile always resolved (against `source`
  itself). The new single-key `InMemory` tree only serves `"main.ink"`, so
  the same `INCLUDE` is now a hard `brink_environment::LoadError` — the
  result is `ok: false` with a populated `error` string and an **empty**
  `warnings` array, not diagnostics. No existing caller feeds `INCLUDE` into
  `compile()` today, so this is also a nil delta for current callers, but
  it is a result-*shape* change (error string vs. diagnostics) for any
  future caller that does.

`EditorSession::compile_project` (brink-ide's `IdeSession::compile`, the
live-editing salsa db shared with brink-lsp) is untouched — out of scope for
this issue.
