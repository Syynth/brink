// Dev server + app build for the Tauri desktop shell (docs/desktop-shell-spec.md).
//
// Mirrors packages/brink-studio/vite.config.ts's DEV-MODE resolution exactly:
// the studio and every internal package resolve to workspace SOURCE, and the
// wasm glue resolves to the built pkg — so `tauri dev` runs against the live
// tree without requiring `pnpm build` in five packages first. Keep the alias
// map in sync with the playground's when packages move.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

export default defineConfig({
  plugins: [react()],
  // Tauri expects a fixed dev port and fails loudly instead of drifting.
  clearScreen: false,
  resolve: {
    alias: {
      "brink-web": resolve(wasmPkgPath, "brink_web.js"),
      "@brink-lang/web": resolve(__dirname, "../wasm/src/index.ts"),
      "@brink-lang/studio": resolve(__dirname, "../brink-studio/src/index.ts"),
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink-lang/editor": resolve(__dirname, "../ink-editor/src/index.ts"),
      "@brink/studio-shell": resolve(__dirname, "../studio-shell/src/index.ts"),
      "@brink/studio-store": resolve(__dirname, "../studio-store/src/index.ts"),
      "@brink/studio-ui": resolve(__dirname, "../studio-ui/src/index.ts"),
    },
  },
  server: {
    port: 5183,
    strictPort: true,
    fs: {
      allow: [wasmPkgPath, ".", "..", "../.."],
    },
  },
  optimizeDeps: {
    exclude: ["brink-web"],
  },
  build: {
    outDir: "dist",
    target: "es2022",
  },
});
