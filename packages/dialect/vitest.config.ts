import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// Pure TS, no DOM: the default node environment. `@brink/wasm-types` is
// type-only, resolved through tsconfig paths; nothing here needs a wasm build.
export default defineConfig({
  test: { include: ["src/**/*.test.ts"] },
  resolve: {
    alias: {
      "@brink/wasm-types": fileURLToPath(new URL("../wasm-types/src/index.ts", import.meta.url)),
    },
  },
});
