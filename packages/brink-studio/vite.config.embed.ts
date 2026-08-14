// Standalone static APP build of brink-studio, for embedding in the mdBook
// playground (or any static host). Unlike vite.config.ts — a *library* build
// that externalizes react/codemirror/zustand and emits a JS module — this
// produces a self-contained app:
//
//   - index.html / main.tsx is the entry (no build.lib)
//   - all deps are bundled (no rollupOptions.external)
//   - base: "./" emits relative asset URLs, so it works under any mount path
//     (an iframe at /playground/, a Pages project subpath, etc.)
//   - the wasm is emitted automatically: Vite resolves the
//     `new URL('brink_web_bg.wasm', import.meta.url)` in the wasm-pack glue and
//     copies + rewrites it — no vite-plugin-wasm needed.
//
//   pnpm --filter @brink-lang/studio build:embed
//
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { studioPackageAliases, studioWasmAliases } from "./alias-map";

export default defineConfig({
  base: "./",
  plugins: [react()],
  resolve: {
    // The same workspace-source map `vite.config.ts` applies on the dev
    // server, with the wasm pair unconditional because this build bundles
    // everything. `@brink/studio-shell` was missing here until #2450 — it
    // still resolved, through the workspace symlink rather than this map,
    // which is the sort of silent near-miss the two guards now catch:
    // src/__tests__/alias-map.test.ts inside this package (#2464) and
    // packages/brink-desktop/src/__tests__/playground-alias-parity.test.ts
    // across the two (#2450).
    alias: {
      ...studioWasmAliases(__dirname),
      ...studioPackageAliases(__dirname),
    },
  },
  build: {
    outDir: "dist-embed",
    emptyOutDir: true,
    target: "es2022",
  },
});
