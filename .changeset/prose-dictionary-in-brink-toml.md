---
"@brink-lang/studio": patch
"@brink-lang/editor": patch
"@brink-lang/web": patch
---

The author's prose dictionary now lives in `brink.toml`, under `[prose]
dictionary`, and is visible and editable in Project → Prose.

It previously went to a `.brink-dictionary` sidecar with no UI anywhere, so
"Add to dictionary" wrote a file nothing displayed — the word stayed
underlined until the next compile and there was no way to see the list or
undo an entry. The settings panel now shows the words, adds and removes
them, and the editor action writes to the same place.

Matching is literal: `Griswold` and `GRISWOLD` are two separate entries.

Package-level notes:

- `@brink-lang/web` gains `EditorSession.getConfiguredProseDictionary()`,
  reading `[prose] dictionary` from the applied config. Like the other
  `configured*` readers it is wholesale-replaced on every apply, so a word
  removed from the file stops being a known word.
- `@brink-lang/editor` gains a `onAddToDictionary` document-session option
  and no longer owns dictionary storage: the list is the embedder's
  `brink.toml`, so the editor package no longer writes it. The
  `PROSE_DICTIONARY_FILE` export is removed. An embedder that does not pass
  `onAddToDictionary` no longer sees the "Add to dictionary" action at all,
  rather than seeing one that silently does nothing.
