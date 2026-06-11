---
"@brink-lang/web": minor
"@brink-lang/studio": minor
---

Initial public release.

- `@brink-lang/web` — the brink ink-language compiler, IDE session, and
  story runtime compiled to WebAssembly, with typed TypeScript wrappers
  (`compile`, `EditorSessionHandle`, `StoryRunnerHandle`) and every
  boundary type included.
- `@brink-lang/studio` — the embeddable brink studio IDE: `mountStudio`
  mounts the full editor/compiler/player environment into a DOM element,
  with the `StudioApi` facade and host extension points (tool windows,
  commands, status-bar items, host-capability manifest).
