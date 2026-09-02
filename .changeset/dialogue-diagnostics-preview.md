---
"@brink-lang/studio": patch
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

Dialogue-convention diagnostics + preview (RULED 2026-08-30): a `brink.toml [dialogue]` declaration that fails to resolve (unknown preset, bad element shape, missing artifact) is now an **error row in the Problems panel** keyed to `brink.toml` — the session keeps the resolver's message as state (`getConfiguredDialogueError()`), so the row reflects the current truth rather than a one-shot warning; a malformed `brink.toml` is an error row too. The dialect's own `malformed` near-miss rules (a cue missing its terminator) surface as **warnings on story lines**, re-evaluated on every compile and config apply. A new **Settings → Conventions** section shows the project's resolved dialect and a paste-to-preview pane: how the editor classifies sample lines as source, and the speaker runs the Player would fold the same lines into as emitted text.
