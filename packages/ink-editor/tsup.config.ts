// Build for the published @brink-lang/editor package.
//
// @brink-lang/web, CodeMirror (@codemirror/*, codemirror, @lezer/*), and react
// are EXTERNALIZED — they're dependencies / peer deps the consumer provides
// (CM6 especially must stay a singleton; bundling a second @codemirror/state
// breaks editor state). The private workspace packages are FOLDED IN instead:
// @brink/ink-operations (runtime) is bundled into dist, and @brink/wasm-types
// (pure type declarations) is inlined into the rolled-up .d.ts. So the published
// package depends only on @brink-lang/web plus the CM6/react peers — no @brink/*
// internals leak out. tsup externalizes deps + peerDeps by default and bundles
// devDeps, so the dependency placement in package.json drives this automatically.
import { defineConfig } from "tsup";

export default defineConfig({
  entry: { index: "src/index.ts" },
  format: ["esm"],
  target: "es2022",
  dts: {
    resolve: ["@brink/wasm-types", "@brink/ink-operations"],
  },
  sourcemap: false,
  clean: true,
});
