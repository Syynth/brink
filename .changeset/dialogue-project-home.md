---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Project-declared dialogue dialect (RULED 2026-08-30): `brink.toml` gains a `[dialogue]` table — `preset = "at-cue"` plus `[[dialogue.elements]]` overlays in the spec's affix sugar (`kind`, `prefix`/`suffix`/`glued`/`content-role`, or `pattern`/`template`), or `file = "path.json"` for a full artifact — resolved in the wasm session (`EditorSessionHandle.getConfiguredDialogueDialect()`) and pushed to every editor view by `DocumentSessions` (live on `brink.toml` edits via `ProjectSession`'s new `onProjectConfigApplied` hook and `DocumentSessions.refreshDialectFromProject`). **No dialect by default**: an absent `dialect` option now means plain lines with the screenplay layer's structural decorations kept (`setDialect(view, undefined)`); the `at-cue` preset is opt-in — the demo project opts in through its own `brink.toml`. An explicit `dialect: null` still tears the layer down for headless embedding. Also fixes a latent affix-sugar bug (a suffix-less prefix compiled to the invalid regex `[^]*`).
