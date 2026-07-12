---
"@brink-lang/web": patch
---

Wired `completions()`/`completions_doc()`, `signature_help()`/
`signature_help_doc()`, and `folding_ranges()`/`folding_ranges_doc()` up to
the #589 IDE entry points (#600), which had landed in `brink-ide` and
`brink-lsp` but were never called from the wasm bridge:

- Completion now offers the T1b stdlib slice 1 functions (`len`/`keys`/
  `values`/`contains`/`push`/`insert`/`remove`, docs/t1b-surface-spec.md §5)
  as `kind: "stdlib"` items, once the new `set_language_dialect("brink")`
  session method is called (defaults to `"strict-ink"`, matching
  `AnalysisOptions::default()` — stdlib names are never offered until a host
  opts in, mirroring `brink-lsp`'s `initializationOptions.dialect`).
- Signature help now calls `signature_help_with_dialect`, so a call to one of
  those same names shows its signature (mutators render their first
  parameter as `name: lvalue`, e.g. `push(a: lvalue, v)`) once brink dialect
  is set — falling back to `None` under the default, exactly like completion.
- Folding now includes `~ { … }` logic-block folds (and their nested
  `if`/`while`/`for` sub-folds) as `kind: "structural"` ranges, unconditionally
  — no dialect gate, since the construct parses and lowers identically in
  both dialects (only strict-ink flags it as a diagnostic, `E051`).

New host-facing API: `EditorSession.set_language_dialect(value: "brink" |
"strict-ink")`. No other wasm-observable behavior changed.
