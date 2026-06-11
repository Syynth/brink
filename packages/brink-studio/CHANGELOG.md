# @brink-lang/studio

## 0.2.1

### Patch Changes

- Updated dependencies [20764ef]
  - @brink-lang/web@0.2.0

## 0.2.0

### Minor Changes

- 6276f29: File-content egress for embedding hosts (#154, closing #137): a debounced,
  batched `onFilesChanged(changes: FileChange[])` mount option fed by every
  mutation path (editor edits, binder structural ops, search replace,
  `file.new`), an `api.getFiles()` / `api.getDirtyFiles()` pull surface,
  `file.save` (Mod-S) / `file.saveAll` commands that flush and deliver
  immediately, and a `dirtyFiles` count on `StudioPublicState` (additive —
  `version` stays 1). Also: a `wasmLocation` mount option forwarded to
  `initWasm` for IIFE-plugin hosts, and a Chromium-88 `adoptedStyleSheets`
  feature-detect shim in the mount bootstrap (NW.js / RPG Maker MZ).
