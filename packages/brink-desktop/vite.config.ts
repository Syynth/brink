// Dev server + app build for the Tauri desktop shell (docs/desktop-shell-spec.md).
//
// The alias map itself lives in `./alias-map.ts` — the single source of
// truth this config, `vitest.config.ts` and `tsconfig.json`'s `paths` all
// answer to (#2418). Do not re-inline a map here: three unsynchronized
// copies are what silently dropped most of the unit suite once already.
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";
import { desktopAliases, WASM_PKG_DIR } from "./alias-map";

const wasmPkgPath = resolve(__dirname, WASM_PKG_DIR);

export default defineConfig({
  plugins: [react()],
  // Tauri expects a fixed dev port and fails loudly instead of drifting.
  clearScreen: false,
  resolve: {
    alias: desktopAliases(__dirname),
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
