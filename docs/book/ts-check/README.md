# Book TypeScript check

Type-checks the book's TypeScript examples, the way `mdbook test` compiles its
Rust ones. mdbook only knows how to run Rust doctests, so this small harness
fills the gap for the `@brink-lang/web` and `@brink-lang/editor` chapters.

Run it with `just book-ts-check` (from the repo root), or here:

```sh
npm install
npm run check      # extract.mjs, then tsc
```

## How it works

`extract.mjs` walks `../src/**/*.md`, pulls out every fenced `ts` / `typescript`
block, and writes each to `generated/`. `tsc` (via `tsconfig.json`) then
type-checks them against the **published** `@brink-lang/*` packages listed in
`package.json` — not the workspace source. The book documents released
packages, so checking against the released `.d.ts` is the right contract, and it
avoids a wasm-pack build (the workspace `@brink-lang/web` types transitively need
the wasm-pack output; the published package bundles them).

## Authoring rules

Mirrors the Rust convention (` ```rust,ignore `):

- ` ```ts ` / ` ```typescript ` — type-checked.
- ` ```ts,no-check ` — skipped, and **logged**. Use only for a deliberate,
  explained exception — e.g. a snippet using the raw `brink-web` wasm-pack module
  directly, which is not a published npm package and can't resolve here.

**Hidden setup.** mdbook hides `#`-prefixed lines only in Rust blocks, so a TS
block can't hide its imports or `declare`s inline. Put them in an HTML comment
immediately before the fence — mdbook doesn't render HTML comments, and
`extract.mjs` prepends the comment body to the snippet:

    <!-- ts-hidden
    import { StoryRunnerHandle } from "@brink-lang/web";
    declare const bytes: Uint8Array;
    -->
    ```ts
    const runner = new StoryRunnerHandle(bytes);
    ```

## Bumping versions

When the book is updated for a new `@brink-lang/*` release, bump the pinned
versions in `package.json` to match. The check then validates the examples
against the API the book now claims to document.
