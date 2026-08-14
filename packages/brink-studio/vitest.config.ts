import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  test: {
    environment: "jsdom",
    exclude: ["e2e/**", "node_modules/**"],
    // Without this, vitest stubs every .css import to an empty module — even
    // `?raw` imports — which would blind the Chromium-88 style scan in
    // chromium88-color-mix.test.ts (#276).
    css: true,
  },
  resolve: {
    // Guarded against packages/brink-desktop/alias-map.ts by
    // packages/brink-desktop/src/__tests__/playground-alias-parity.test.ts
    // (#2450) — an alias added, removed, or repointed here without a
    // matching update there turns that suite red. brink-web is the one
    // named exception: it stays mocked here, unlike the desktop package's
    // own vitest suite, which resolves the real wasm-bindgen glue on
    // purpose.
    alias: {
      "brink-web": resolve(__dirname, "src/__mocks__/brink-web.ts"),
      "@brink/wasm-types": resolve(__dirname, "../wasm-types/src/index.ts"),
      "@brink-lang/web": resolve(__dirname, "../wasm/src/index.ts"),
      "@brink/ink-operations": resolve(__dirname, "../ink-operations/src/index.ts"),
      "@brink-lang/editor": resolve(__dirname, "../ink-editor/src/index.ts"),
      "@brink/studio-shell": resolve(__dirname, "../studio-shell/src/index.ts"),
      "@brink/studio-store": resolve(__dirname, "../studio-store/src/index.ts"),
      "@brink/studio-ui": resolve(__dirname, "../studio-ui/src/index.ts"),
    },
  },
});
