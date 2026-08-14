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
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

export default defineConfig({
  base: "./",
  plugins: [react()],
  resolve: {
    // The same workspace-source map `vite.config.ts` applies on the dev
    // server, with the wasm pair unconditional because this build bundles
    // everything. `@brink/studio-shell` was missing here until #2450 — it
    // still resolved, through the workspace symlink rather than this map,
    // which is the sort of silent near-miss
    // `packages/brink-desktop/src/__tests__/playground-alias-parity.test.ts`
    // now catches.
    alias: {
      "brink-web": resolve(wasmPkgPath, "brink_web.js"),
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink-lang/web": resolve(__dirname, "../wasm/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink-lang/editor": resolve(__dirname, "../ink-editor/src/index.ts"),
      "@brink/studio-shell": resolve(__dirname, "../studio-shell/src/index.ts"),
      "@brink/studio-store": resolve(__dirname, "../studio-store/src/index.ts"),
      "@brink/studio-ui": resolve(__dirname, "../studio-ui/src/index.ts"),
    },
  },
  build: {
    outDir: "dist-embed",
    emptyOutDir: true,
    target: "es2022",
  },
});
