# The Editor

`@brink-lang/editor` is the ink editor itself — a CodeMirror 6 layer that turns
a text box into an ink IDE: diagnostics, semantic highlighting, completions with
auto-import, hover, go-to-definition, find-references, inline rename with a live
breakage badge, code actions, folding, signature help, inlay hints, and a
screenplay dialect. It's the same editor the [Studio](../studio/index.md) app is
built from — Studio is one consumer, and your tool is on equal footing.

It sits on top of [`@brink-lang/web`](./index.md): the editor owns the *UX*, and
you supply thin callbacks that bridge each feature to a `@brink-lang/web` op. The
compiler and runtime stay in the WASM module.

## Two entry points

- **`brinkStudio(options)`** returns a CodeMirror `Extension` — the editor. You
  add it to an `EditorView` and pass callbacks in `options`.
- **`ProjectSession`** is the reusable session/file/dirty/conflict layer. It owns
  the wasm `EditorSessionHandle`, tracks unsaved buffers, applies cross-file
  edits, and detects external-change conflicts. Studio uses this same class.

## Features light up per callback

`brinkStudio` needs only three things to render a working editor — a `compile`
bridge and the two semantic-token accessors. Every other feature turns on
**only when you provide its callback**: no `getCodeActions`, no code-actions
menu; no `getHover`, no hover. You opt into exactly the surface your tool wants.

```ts
import { EditorView } from "@codemirror/view";
import { brinkStudio } from "@brink-lang/editor";
import { compile } from "@brink-lang/web";

const view = new EditorView({
  parent: document.body,
  extensions: [
    brinkStudio({
      compile,                         // the @brink-lang/web bridge
      getSemanticTokens: () => [],     // wire to a session's semantic_tokens
      getTokenTypeNames: () => [],     // …and its token type names
      // getHover, getCompletions, getCodeActions, prepareRename, … as needed
    }),
  ],
});
```

The full per-callback contract — every option, the `@brink-lang/web` op behind
it, and the host hooks — is in the
[editor consumer guide](https://github.com/Syynth/brink/blob/main/docs/editor-consumer-guide.md).

## The dialogue dialect

Screenplay-style cues (`@Name: line`) are handled by a configurable **dialect**,
not a hardcoded mode. The `dialect` option defaults to the `AT_CUE_DIALECT`
preset; pass your own `DialogueDialect` to change the convention, or `null` to
tear the whole screenplay layer down for a plain editor.

```ts
import { EditorView } from "@codemirror/view";
import { brinkStudio, AT_CUE_DIALECT } from "@brink-lang/editor";
import { compile } from "@brink-lang/web";

const view = new EditorView({
  extensions: [
    brinkStudio({
      compile,
      getSemanticTokens: () => [],
      getTokenTypeNames: () => [],
      dialect: AT_CUE_DIALECT,   // or a custom DialogueDialect, or null
    }),
  ],
});
```

The dialect is **tooling only** — it drives classification, hidden sigils, and
the screenplay transitions in the editor, and never reaches the runtime. Use
`setDialect(view, d)` to reconfigure a mounted editor live.

## Headless and host-styled

Pass `theme: false` for a headless editor — no CodeMirror theme at all. brink
still emits a documented class taxonomy and data attributes (choice/body lines
carry `data-option-path` with their full weave lineage, for example), and your
host stylesheet owns the appearance. This is how a tool matches its own design
system instead of inheriting Studio's skin.

Beyond the editor extension, the package also ships small **boundary helpers**
for building chrome around it — `sortDiagnostics` to order a diagnostics list,
`lineColAt` to turn a byte offset into a line/column — plus standalone versions
of individual features (`foldingExtension`, `hostGutterExtension`,
`renameExtension`, `findPanel`) for hosts that compose extensions directly
instead of going through `brinkStudio`.
