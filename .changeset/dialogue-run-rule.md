---
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

`run_ends_at` — the emitted-side run rule (RULED 2026-08-30): a chain rule now declares which kinds (plus the reserved `"choices"` turn boundary) END the active speaker's run in runtime-emitted text, where the source-side "blank always breaks" has no counterpart. Declared in `brink.toml` as `[dialogue] run-ends-at = [...]` (applied to every chain rule of the resolved dialect, validated like `after`/`becomes`) and applied through the new shared `runsOf(lines, dialect)` helper in `@brink-lang/editor`, so the studio Player and an engine importing the resolved dialect fold emitted lines into runs identically.
