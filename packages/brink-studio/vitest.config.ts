import { defineConfig } from "vitest/config";
import { studioPackageAliases, studioTestWasmAliases } from "./alias-map";

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
    // Guarded from inside this package by src/__tests__/alias-map.test.ts
    // (#2464), and against packages/brink-desktop/alias-map.ts by
    // packages/brink-desktop/src/__tests__/playground-alias-parity.test.ts
    // (#2450) — an alias added, removed, or repointed without a matching
    // update on the other side turns both suites red. brink-web is the one
    // named exception: `studioTestWasmAliases` repoints it at this package's
    // jsdom mock, unlike the desktop package's own vitest suite, which
    // resolves the real wasm-bindgen glue on purpose.
    alias: {
      ...studioTestWasmAliases(__dirname),
      ...studioPackageAliases(__dirname),
    },
  },
});
