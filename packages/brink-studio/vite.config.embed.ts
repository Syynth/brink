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
//   pnpm --filter @brink/studio build:embed
//
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

export default defineConfig({
  base: "./",
  plugins: [react()],
  resolve: {
    alias: {
      "brink-web": resolve(wasmPkgPath, "brink_web.js"),
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink/wasm": resolve(__dirname, "../wasm/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink/ink-editor": resolve(__dirname, "../ink-editor/src/index.ts"),
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
