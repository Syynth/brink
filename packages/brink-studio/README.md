# @brink-lang/studio

The embeddable [brink](https://github.com/Syynth/brink) studio — a full ink
IDE (CodeMirror editor with diagnostics/completions/rename, compile
pipeline, story player, binder, story graph, problems/output panels) that
mounts into a DOM element with one call.

## Install

```sh
npm install @brink-lang/studio react react-dom
```

`react` / `react-dom` (^19) are peer dependencies. `@brink-lang/web` (the
wasm compiler/runtime) is installed automatically.

## Usage

```ts
import { mountStudio } from "@brink-lang/studio";
import "@brink-lang/studio/style.css";

const handle = await mountStudio(document.getElementById("studio")!, {
  files: { "main.ink": "Hello, world!\n-> END\n" },
  entryFile: "main.ink",
});

// handle.api is the StudioApi facade; handle.unmount() tears down.
```

With **Vite**, exclude the wasm package from dependency pre-bundling:

```ts
// vite.config.ts
export default defineConfig({
  optimizeDeps: { exclude: ["@brink-lang/web"] },
});
```

## Extending

Hosts can register their own tool windows, commands, and status-bar items,
and talk to the studio through the `StudioApi` facade — see the
[embedder API documentation](https://github.com/Syynth/brink/blob/main/docs/embedder-api.md)
and the shipped `createExampleExtension` worked example.

## Related

- [`@brink-lang/web`](https://www.npmjs.com/package/@brink-lang/web) — the
  compiler, IDE session, and story runtime (wasm) this package is built on;
  re-exported here as `initWasm` / `compile` / `EditorSessionHandle` /
  `StoryRunnerHandle` for hosts that drive stories directly.

## License

MIT
