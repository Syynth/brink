// Type rollup for the published @brink-lang/studio package: dist/index.d.ts.
//
// The JS bundle and style.css come from `vite build` (vite.config.ts); this
// config only emits declarations (`dts.only`). The private @brink/* internal
// packages are resolved and inlined; `@brink-lang/web` and react stay
// external imports. Via tsconfig.build.json, @brink-lang/web resolves to its
// package types (packages/wasm/dist/index.d.ts) instead of source, so the
// rollup never duplicates its class declarations — run
// `pnpm --filter @brink-lang/web build` first (the workspace `build` does).
import { defineConfig } from "tsup";

export default defineConfig({
  entry: { index: "src/index.ts" },
  format: ["esm"],
  dts: {
    only: true,
    // Inline the private workspace packages (@brink/studio-shell, studio-ui,
    // studio-store, ink-editor, ink-operations, wasm-types). Note this does
    // NOT match @brink-lang/* — that scope stays external.
    resolve: [/^@brink\//],
  },
  tsconfig: "tsconfig.build.json",
  external: ["@brink-lang/web", "react", "react-dom"],
  outDir: "dist",
  clean: false,
});
