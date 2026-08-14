// Type rollup for the published @brink-lang/studio package: dist/index.d.ts.
//
// The JS bundle and style.css come from `vite build` (vite.config.ts); this
// config only emits declarations (`dts.only`). The private @brink/* internal
// packages are resolved and inlined; `@brink-lang/web` and react stay
// external imports. Via tsconfig.build.json, @brink-lang/web resolves to its
// package types (packages/wasm/dist/index.d.ts) instead of source, so the
// rollup never duplicates its class declarations — run
// `pnpm --filter @brink-lang/web build` first (the workspace `build` does).
//
// `tsconfig` and `external` are guarded from inside this package by
// src/__tests__/alias-map.test.ts (#2464).
import { defineConfig } from "tsup";

export default defineConfig({
  entry: { index: "src/index.ts" },
  format: ["esm"],
  dts: {
    only: true,
    // Inline the workspace packages that studio BUNDLES into its own dist:
    // the private @brink/* packages (studio-shell, studio-ui, studio-store,
    // ink-operations, wasm-types) plus @brink-lang/editor (bundled via the
    // vite alias, so its types must be inlined here too). @brink-lang/web
    // stays external (a real versioned dependency).
    resolve: [/^@brink\//, "@brink-lang/editor"],
  },
  tsconfig: "tsconfig.build.json",
  external: ["@brink-lang/web", "react", "react-dom"],
  outDir: "dist",
  clean: false,
});
