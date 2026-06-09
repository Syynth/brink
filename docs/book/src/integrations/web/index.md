# Web & WASM

`brink-web` compiles the toolchain — compiler, runtime, and IDE queries — to
WebAssembly and exposes it to JavaScript. It is the foundation every browser
client builds on: the [Studio](../studio/index.md) authoring app, the embedded
[Playground](./playground.md), and any React/web front-end you write yourself
all sit on this surface.

## Building the package

`brink-web` is built with `wasm-pack`:

```sh
wasm-pack build crates/brink-web --target web --out-dir www/pkg
```

This emits an npm package at `crates/brink-web/www/pkg/` — the glue JS
(`brink_web.js`), TypeScript types (`brink_web.d.ts`), and the `.wasm` binary.
The package must be built **before** the JS workspace installs, because the
ergonomic wrapper `@brink/wasm` depends on it via a `file:` path.

```ts
// raw module
import init, { EditorSession, StoryRunner, compile } from "brink-web";
await init();                       // one-time async load of the wasm

// or the ergonomic wrapper (parses the JSON envelopes for you)
import { EditorSessionHandle, StoryRunnerHandle, compile } from "@brink/wasm";
```

## What it exposes

Three things cross the boundary: a stateless compile function, a runtime runner,
and a stateful editor session.

### `compile(source) → CompileResult`

One-shot, single-file compilation. Returns
`{ ok, story_bytes?, warnings, error? }` — `story_bytes` is the compiled
`.inkb` as a byte array, ready to hand to a `StoryRunner`.

### `StoryRunner` — running a story

| Method | Description |
|--------|-------------|
| `new StoryRunner(bytes: Uint8Array)` | decode + link compiled bytes into a runnable instance |
| `continue_story()` | run to the next choice/end; returns all `Line`s produced |
| `continue_single()` | produce one `Line` (typewriter reveal) |
| `choose(index)` | select a choice by 0-based index |
| `reset()` | return to the start without recompiling |
| `bind_external(name, fn)` | bind an ink `EXTERNAL` to a synchronous JS callback |
| `unbind_external(name)` | remove a previously bound external |
| `set_lenient_unbound(bool)` | unbound externals resolve to `null` instead of using the ink fallback / erroring |
| `get_var(name)` / `set_var(name, value)` | read/write a global ink variable by name |
| `set_seed(n)` | set the RNG seed for reproducible `RANDOM`/shuffle (re-applied across `reset`) |
| `save()` / `save_bytes()` | capture durable game state — JSON string (dev) or MessagePack bytes (release) |
| `load(json)` / `load_bytes(bytes)` | reconcile a save back in; returns a `LoadReport` of anything dropped |

`Line` mirrors the native runtime: `{ type: "text"|"choices"|"done"|"end", text,
tags, choices? }`. This is the same execution model as
[the toolchain runtime](../../toolchain/concepts/execution-model.md), surfaced to JS.

### External functions

When a story calls `EXTERNAL roll(sides)`, the runner asks any binding
registered under that name to resolve it. Arguments arrive as native JS values
(number / boolean / string / null) and the return is read back the same way —
an integer-valued number becomes an ink int, otherwise a float:

```ts
const runner = new StoryRunnerHandle(bytes);
runner.bindExternal("roll", (sides) => 1 + Math.floor(Math.random() * Number(sides)));
runner.bindExternal("play_sound", (id) => { audio.play(String(id)); }); // fire-and-forget
```

An external with no binding falls through to its ink fallback body (erroring if
none exists), unless `setLenientUnbound(true)` is set — then it resolves to
`null`, so content can call host verbs a given build doesn't know without
dead-ending. A binding that throws resolves to `null` (the exception is not
propagated into the VM). Asynchronous bindings (suspend-and-resume) are a
planned addition; today bindings must return synchronously.

### `EditorSession` — IDE queries

A stateful, multi-file project session that powers an editor: diagnostics,
semantic highlighting, navigation, and structural refactors. You feed it source
and it answers queries against cached analysis.

| Group | Methods |
|-------|---------|
| Files | `update_file(path, src)`, `set_active_file(path)`, `remove_file`, `list_files`, … |
| Compile & structure | `compile_project(entry)`, `project_outline`, `document_symbols` |
| Highlighting | `semantic_tokens`, `token_type_names`, `token_modifier_names` |
| Navigation | `goto_definition`, `find_references`, `hover`, `completions`, `signature_help` |
| Refactors | `prepare_rename`, `rename`, `code_actions`, `convert_element` |
| Editing aids | `folding_ranges`, `inlay_hints`, `line_contexts`, `format_document` |
| Structural edits | `reorder_stitch`, `move_stitch`, `promote_stitch`, `demote_knot`, `reorder_knot` |

**View context.** `set_view_context(start, end)` scopes every query to a byte
range — line numbers and offsets become relative to that fragment. This is how a
client edits one knot/stitch in isolation. `clear_view_context()` returns to
full-file mode.

## Conventions

- **JSON envelopes.** Every query returns a JSON *string* (`serde_json`); the JS
  side parses it. The `@brink/wasm` wrapper does this for you and returns typed
  objects (`@brink/wasm-types`).
- **UTF-8 byte offsets.** Positions are UTF-8 byte offsets into the source, not
  UTF-16 char indices or line/column. When a view context is active they are
  relative to the view start; the session translates transparently.
- **Lifecycle.** Construct → feed source / step → dispose. wasm-bindgen objects
  free automatically, or call `.free()` explicitly via the wrapper handles.

## A minimal client

```ts
import init, { EditorSession, StoryRunner } from "brink-web";

await init();

const session = new EditorSession();
session.update_file("main.ink", source);
const result = JSON.parse(session.compile_project("main.ink"));

if (result.ok) {
  const runner = new StoryRunner(new Uint8Array(result.story_bytes));
  let lines = JSON.parse(runner.continue_story());
  // render lines; on a choices line, call runner.choose(i) and continue
}
```

The Studio is the full-featured reference build of exactly this loop — see
[Studio](../studio/index.md).
