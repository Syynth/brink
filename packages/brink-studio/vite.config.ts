import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

export default defineConfig({
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
  server: {
    port: 5180,
    fs: {
      allow: [wasmPkgPath, ".", ".."],
    },
  },
  optimizeDeps: {
    exclude: ["brink-web"],
  },
  build: {
    lib: {
      entry: resolve(__dirname, "src/index.ts"),
      formats: ["es"],
      fileName: "brink-studio",
    },
    rollupOptions: {
      external: [
        "codemirror",
        /^@codemirror\//,
        /^@lezer\//,
        "react",
        "react-dom",
        "zustand",
      ],
    },
  },
});
