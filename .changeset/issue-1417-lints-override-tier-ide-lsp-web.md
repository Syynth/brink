---
"@brink-lang/web": patch
---

#1417: extends the `[lints]`/`deny-warnings` CLI/API override tier
(#1373's `brink compile` `--deny`/`--warn`/`--allow`/`-D warnings`,
#1394's `BrinkPlugin::with_config`) to `brink ide`, `brink-lsp`, and the
wasm `EditorSession` — the three surfaces #1417 named as still honoring
only a discovered `brink.toml`, so a project that denies a warning saw it
demoted back to a warning in the editor/LSP even though a real
`brink compile` of the same project would fail.

- `brink ide` gains repeatable `--deny`/`--warn`/`--allow <CODE>` and
  `-D warnings`, mirroring `brink compile` exactly (shared resolution via
  the new `brink-cli::lint_overrides` module) — every subcommand that
  loads a project (`def`, `check`, `rename`, `effects-diff`, …) now
  honors them, always winning over the same code in a discovered
  `brink.toml`.
- `brink-lsp` gains `initializationOptions.lints` (an object,
  `{ "<CODE>": "deny" | "warn" | "allow" }`) and
  `initializationOptions.denyWarnings`, applied last in
  `resolve_language_options` — the same `CLI/API > file > default`
  precedence `dialect`/`types` already had.
- **`EditorSession` (wasm-observable)**: two new methods,
  `set_lint_overrides(json)` (replace the explicit per-code override map;
  `"{}"` clears it) and `set_deny_warnings_override(bool)` /
  `clear_deny_warnings_override()`. Always win over an applied
  `brink.toml`'s `[lints]` table, in either call order — the file tier
  reapplies the explicit overrides on every reload rather than clobbering
  them. `compile_project` now reflects the resolved policy exactly as
  `brink compile`/`brink ide`/`brink-lsp` would.

Absent any override (the pre-#1417 default in all three surfaces) is
byte-identical to prior behavior.
