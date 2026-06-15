# @brink-lang/studio

## 0.3.0

### Minor Changes

- bcd23b7: Live inspector and host-aware authoring.

  - The story session is driven by a `SessionProvider`, so the transcript, State
    View, and Story Graph render against a provider rather than the wasm runner
    directly — the groundwork for inspecting a VM running in a host.
  - Capability-gated session commands, program-identity degraded mode, and
    multi-session support (independent runners + shared-context flows) with a
    session/flow picker.
  - A host-aware argument picker: a value dropdown and inline value labels for
    `EXTERNAL` arguments whose semantic type declares a value source (static, or
    pushed live by a host), plus a `StudioExtensions.argumentProviders` surface
    for embedders to supply those values.

### Patch Changes

- Updated dependencies [bcd23b7]
  - @brink-lang/web@0.3.0

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
