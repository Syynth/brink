---
"@brink-lang/web": patch
---

Wired `types = strict` (TM-3, #619) reachability through `brink-ide`,
`brink-lsp`, and `brink-web` (#660) — PR #656 landed the strict-mode checks
themselves but left `IdeSession`'s two `AnalysisOptions` literals hardcoded
to `TypePolicy::Gradual` (no setter), so strict mode was reachable only via
the compiler CLI's `--types strict`; the IDE/LSP/web surface could not turn
it on at all.

- `IdeSession` (`brink-ide`) gains `set_type_policy`/`type_policy`,
  mirroring `set_language_dialect`/`language_dialect` exactly. `snapshot()`
  and `analysis_options()` now thread the registered policy through instead
  of a hardcoded `TypePolicy::default()`.
- `brink-lsp` reads `initializationOptions.types` (`"strict"` or
  `"gradual"`; defaults to `"gradual"`), mirroring the existing `.dialect`
  handling, and feeds it to both the foreground session and the background
  `analysis_loop`.
- `EditorSession.set_type_policy(value: "strict" | "gradual")` (`brink-web`)
  mirrors `set_language_dialect` and re-analyzes immediately. `strict`
  requires `set_language_dialect("brink")`, or analysis (and
  `compile_project`) reports a single project-level `E064` config-error
  diagnostic instead of running the normal passes — the caller's
  responsibility, same as the CLI.

**wasm-observable**: `EditorSession.set_type_policy` is a new host-facing
entry point, and `compile_project`/background analysis now surface
`E065`/`E066`/`E067`/error-severity-`E063` diagnostics (or `E064`) for a
project that opts in — behavior no wasm consumer could previously reach at
all. No other wasm-observable behavior changed; the default (`Gradual`,
never calling `set_type_policy`) is byte-identical to before.
