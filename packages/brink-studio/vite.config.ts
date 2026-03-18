import { defineConfig } from "vite";
import { resolve } from "path";

const wasmPkgPath = resolve(__dirname, "../../crates/brink-web/www/pkg");

export default defineConfig({
  resolve: {
    alias: {
      "brink-web": resolve(wasmPkgPath, "brink_web.js"),
    },
  },
  server: {
    port: 5180,
    fs: {
      allow: [wasmPkgPath, "."],
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
      ],
    },
  },
});
