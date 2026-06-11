# @brink-lang/web

The [brink](https://github.com/Syynth/brink) ink-language compiler, IDE
session, and story runtime, compiled to WebAssembly with ergonomic
TypeScript wrappers.

- `compile(source)` — compile `.ink` source to a runnable story binary
- `StoryRunnerHandle` — drive a compiled story (continue, choices,
  variables, save/load, external functions, engine→ink calls)
- `EditorSessionHandle` — incremental multi-file IDE session (diagnostics,
  completions, hover, rename, semantic tokens, outline, story graph, …)

All boundary types (`CompileResult`, `Line`, `Choice`, `HostManifest`, …)
ship with the package — no extra `@types` install.

## Install

```sh
npm install @brink-lang/web
```

## Usage

Initialize the wasm module once, then use anything:

```ts
import { initWasm, compile, StoryRunnerHandle } from "@brink-lang/web";

await initWasm();

const result = compile(`Hello, world!\n-> END\n`);
if (!result.ok) throw new Error(result.error);

const story = new StoryRunnerHandle(new Uint8Array(result.story_bytes!));
for (;;) {
  const line = story.continueSingle();
  if (line.type === "text") { console.log(line.text); continue; }
  if (line.type === "choices") { story.choose(0); continue; }
  break; // done | end
}
```

### Bundlers (Vite, Rollup, webpack 5)

The package locates its `.wasm` binary with
`new URL("brink_web_bg.wasm", import.meta.url)`, which modern bundlers
resolve and emit as an asset automatically — no plugin needed.

With **Vite**, exclude the package from dependency pre-bundling so the
relative wasm URL survives the dev server's optimizer:

```ts
// vite.config.ts
export default defineConfig({
  optimizeDeps: { exclude: ["@brink-lang/web"] },
});
```

(Production `vite build` works either way; the exclude is for `vite dev`.)

### No bundler / custom hosting

Pass the binary's location (URL, path string, or precompiled
`WebAssembly.Module`) to `initWasm`:

```ts
await initWasm(new URL("/assets/brink_web_bg.wasm", location.origin));
```

## Related

- [`@brink-lang/studio`](https://www.npmjs.com/package/@brink-lang/studio) —
  the embeddable brink studio IDE (`mountStudio`), built on this package.

## License

MIT
