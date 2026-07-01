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
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

export default defineConfig(({ command }) => ({
  plugins: [react()],
  resolve: {
    alias: {
      // The lib build externalizes @brink-lang/web (so the brink-web glue
      // is never reached there); the dev server resolves both to source.
      ...(command === "serve"
        ? {
            "brink-web": resolve(wasmPkgPath, "brink_web.js"),
            "@brink-lang/web": resolve(__dirname, "../wasm/src/index.ts"),
          }
        : {}),
      // Internal (private, bundled) packages, resolved to source.
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink-lang/editor": resolve(__dirname, "../ink-editor/src/index.ts"),
      "@brink/studio-shell": resolve(__dirname, "../studio-shell/src/index.ts"),
      "@brink/studio-store": resolve(__dirname, "../studio-store/src/index.ts"),
      "@brink/studio-ui": resolve(__dirname, "../studio-ui/src/index.ts"),
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
