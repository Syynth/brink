// Dev server + published LIBRARY build of @brink-lang/studio.
//
// `vite dev` serves the standalone playground app (index.html / main.tsx,
// itself a mountStudio embedder). `vite build` produces the npm artifact:
//
//   - dist/index.js     ESM bundle of the public surface (src/index.ts:
//                       mountStudio, StudioApi, extension types, example
//                       extension)
//   - dist/index.d.ts   rolled-up declarations (internal @brink/* package
//                       types inlined; @brink-lang/web stays an import) —
//                       emitted by tsup/rollup-plugin-dts (tsup.config.ts),
//                       run by the `build` script after `vite build`
//   - dist/style.css    every stylesheet the studio side-effect imports
//                       (shell + ui aggregators, @xyflow/react), in one
//                       file; consumers `import "@brink-lang/studio/style.css"`
//
// Externals are exactly react / react-dom (peer deps) and @brink-lang/web
// (a regular versioned dependency). The internal workspace packages
// (@brink/studio-shell, studio-ui, studio-store, ink-editor,
// ink-operations) and the third-party UI deps (codemirror, zustand,
// @xyflow/react, …) are BUNDLED — the internal packages stay private and
// unpublished, and nothing in the bundle leaks across the React boundary.
//
// The self-contained playground APP build lives in vite.config.embed.ts.
//
// The alias map itself lives in ./alias-map.ts — the single source of truth
// this config, vite.config.embed.ts, vitest.config.ts and both tsconfigs'
// `paths` all answer to (#2464). Do not re-inline a map here.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";
import { studioPackageAliases, studioWasmAliases, WASM_PKG_DIR } from "./alias-map";

const wasmPkgPath = resolve(__dirname, WASM_PKG_DIR);

export default defineConfig(({ command }) => ({
  plugins: [react()],
  resolve: {
    // Guarded from inside this package by src/__tests__/alias-map.test.ts
    // (#2464), and against packages/brink-desktop/alias-map.ts by
    // packages/brink-desktop/src/__tests__/playground-alias-parity.test.ts
    // (#2450) — an alias added, removed, or repointed without a matching
    // update on the other side turns both suites red.
    alias: {
      // The lib build externalizes @brink-lang/web (so the brink-web glue
      // is never reached there); the dev server resolves both to source.
      ...(command === "serve" ? studioWasmAliases(__dirname) : {}),
      // Internal (private, bundled) packages, resolved to source.
      ...studioPackageAliases(__dirname),
    },
  },
  server: {
    port: 5180,
    fs: {
      // `../..` is the monorepo root — lets the dev server serve workspace deps
      // from the pnpm store (e.g. the self-hosted @fontsource woff2, #155).
      allow: [wasmPkgPath, ".", "..", "../.."],
    },
  },
  optimizeDeps: {
    exclude: ["brink-web"],
  },
  build: {
    lib: {
      entry: resolve(__dirname, "src/index.ts"),
      formats: ["es"],
      fileName: "index",
      cssFileName: "style",
    },
    rollupOptions: {
      external: ["react", "react-dom", /^react\//, /^react-dom\//, "@brink-lang/web"],
    },
  },
}));
