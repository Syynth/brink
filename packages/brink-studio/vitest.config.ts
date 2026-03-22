import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  test: {
    environment: "jsdom",
    exclude: ["e2e/**", "node_modules/**"],
  },
  resolve: {
    alias: {
      "brink-web": resolve(__dirname, "src/__mocks__/brink-web.ts"),
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink/wasm": resolve(__dirname, "../wasm/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink/ink-editor": resolve(__dirname, "../ink-editor/src/index.ts"),
      "@brink/studio-store": resolve(__dirname, "../studio-store/src/index.ts"),
      "@brink/studio-ui": resolve(__dirname, "../studio-ui/src/index.ts"),
    },
  },
});
