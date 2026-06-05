# Studio

`brink-studio` is the reference web authoring app for ink — a browser IDE built
on the [Web & WASM](../web/index.md) bindings. It's both a usable editor and a
worked example of how to assemble a full client: editor, live preview, project
navigation, and screenplay mode, all driven by `@brink/wasm`.

## What it does

- **Multi-file ink editor** (CodeMirror 6) with diagnostics, semantic
  highlighting, completions, hover, go-to-definition, find-references, rename,
  code actions, folding, signature help, and inlay hints — every
  `EditorSession` query wired to the editor.
- **Live player** — step through the compiled story with choice selection;
  playback state persists to `localStorage` and replays on reload.
- **Screenplay mode** — character lines (`@Name:<>`) and parentheticals
  (`(text)<>`) render with hidden sigils, name coloring, and depth indicators.
- **Project navigation** — a binder tree of knots and stitches (function knots
  marked with a distinct icon) with drag-to-reorder and structural edits, plus
  file tabs (pinned/unpinned) and symbol tabs.
- **Activity-bar sidebar** — a VS Code-style icon column that swaps the left
  dock between views; the binder is the first view, the state view the second.
- **State view** — a read-only runtime debugger that surfaces the running
  story's status, current position, call stack, globals, and pending choices,
  refreshed as the story advances. (Interim raw dump; a structured,
  name-resolved view is planned — see issue #62.)
- **Line-element switching** — convert a line between narrative, choice, sticky
  choice, gather, and divert via keyboard or UI.

## Architecture

The studio is a pnpm workspace of focused packages. The app shell is thin; the
capability lives in libraries, each independently testable.

| Package | Responsibility |
|---------|----------------|
| `@brink/studio` | app shell + entry point (Vite) |
| `@brink/studio-ui` | React components: layout, activity bar, binder, state view, player, tabs, status bar |
| `@brink/studio-store` | Zustand store — `editor` / `compile` / `tabs` / `player` / `binder` / `layout` slices |
| `@brink/ink-editor` | the CodeMirror 6 editor, state management, IDE extensions, screenplay sigils |
| `@brink/ink-operations` | pure line-editing functions (no CM6, React, or wasm) |
| `@brink/wasm` | ergonomic wrappers over the `brink-web` FFI |
| `@brink/wasm-types` | shared TypeScript interfaces (zero runtime) — decouples everything from the FFI |

The dependency flow is one-directional: `@brink/wasm-types` is depended on by
all; `@brink/wasm` wraps the raw `brink-web` module; `@brink/ink-editor` consumes
`@brink/wasm` + `@brink/ink-operations`; `@brink/studio-store` orchestrates
editor, compile, and player state; `@brink/studio` assembles the lot.

## Running it

The WASM package must exist first — `@brink/wasm` resolves `brink-web` through a
`file:` path to `crates/brink-web/www/pkg`:

```sh
# 1. build the wasm package (see Web & WASM)
wasm-pack build crates/brink-web --target web --out-dir www/pkg

# 2. install + run the studio
pnpm install
pnpm dev            # Vite dev server on http://localhost:5180
```

| Command | Purpose |
|---------|---------|
| `pnpm dev` | dev server (port 5180) |
| `pnpm build` | production build |
| `pnpm typecheck` | TypeScript, no emit |
| `pnpm test` | Vitest unit tests (jsdom) |
| `pnpm test:e2e` | Playwright end-to-end suite |

## Tech stack

React 19 · TypeScript 5.7 · Vite 6 · Zustand 5 · CodeMirror 6 ·
`react-resizable-panels` for layout · Vitest + Playwright for tests. The editor
talks to the toolchain entirely through `@brink/wasm`, so it stays a pure
front-end with the compiler and runtime living in the WASM module.
