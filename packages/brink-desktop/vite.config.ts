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
    // NOT part of the alias map (deliberately): this is a build-flavor
    // swap, not a workspace-path alias, and it must not force a tsconfig
    // `paths` twin (types stay `react-dom/client`). The profiling build
    // makes <Profiler> onRender fire in PRODUCTION bundles, so the perf
    // panel's `react.commit.*` spans exist in the shipped desktop app —
    // the whole point of the prod-perf ruling (2026-08-25). Overhead is
    // the documented modest profiling bookkeeping; the desktop studio is
    // the maintainer's measurement surface, so visibility wins.
    alias: [
      { find: /^react-dom\/client$/, replacement: "react-dom/profiling" },
      ...Object.entries(desktopAliases(__dirname)).map(([find, replacement]) => ({
        find,
        replacement,
      })),
    ],
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
