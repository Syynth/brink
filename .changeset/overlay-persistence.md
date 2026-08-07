---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

Overlay persistence for embedding hosts (the celeris file model, 2026-08-07
decision): `FileChangeHub`/`ProjectSession`/`mountStudio` gain an
`egressPersists: false` contract under which `onFilesChanged` delivery feeds
a backup ring rather than counting as persistence — dirty then means
"diverges from the last canonical save" and only the save commands clear it
(an undo back to the saved text drops to clean). New `OverlayPersistence`
coordinator in `@brink-lang/editor`: routes egress batches to a
host-provided `BackupSink` (ring bounds are sink-owned), owns canonical
`save`/`saveAll` (write + re-baseline, rejected writes stay dirty for
retry), and an autosave scheduler where an autosave tick IS `saveAll` —
one save path, one artifact class. The default (`egressPersists` absent)
is byte-identical to the previous write-through behavior.
