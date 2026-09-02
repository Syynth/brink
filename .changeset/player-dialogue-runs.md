---
"@brink-lang/studio": patch
---

The Player renders dialogue RUNS from the project's dialect (RULED 2026-08-30): delivered lines are classified with the resolved `brink.toml [dialogue]` artifact — the same one the editor uses — and folded into runs by the shared `runsOf` rule: the cue header once (speaker coloured by a deterministic palette index — the hardcoded demo cast table is gone), its spoken lines beneath, parentheticals styled inline, action/narrative outside. No dialect ⇒ plain lines, as Inky. The `@NAME:` regex is gone with it. Also fixes the choice-echo bug: an echo is styled because the row's `kind` is `marker`, never because its text starts with `> ` — a story line beginning with `> ` (an action convention) is story text.
