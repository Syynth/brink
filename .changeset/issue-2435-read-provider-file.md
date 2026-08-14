---
"@brink-lang/editor": minor
---

`ProjectSession` gains `readProviderFile(path)`, a thin pass-through to the
provider's existing `FileProvider.readFile`, bypassing session state. It
lets a caller confirm what a host write actually persisted rather than
assuming a pre-save snapshot still matches (issue #2435) — used by
`@brink-lang/studio`'s `file.save`/`file.saveAll` guard.
