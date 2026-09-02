---
"@brink-lang/studio": patch
---

The `[dialogue]` section of `brink.toml` as one owned block (#3410):
`renderDialogueSection` (table or file form, stamped with a marker that
hashes the body), `findDialogueSection` (with `owner`: editor / hand /
edited — the UI asks before replacing anything not its own), and
`setDialogueSection` (replace, append, or remove the section; every byte
outside it preserved). Key-level edits cannot write `[[dialogue.elements]]`.
