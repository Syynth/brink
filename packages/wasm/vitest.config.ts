import { defineConfig } from "vitest/config";

// Unit tests here cover only the pure, wasm-free logic (`src/evaluate-dispatch.ts`).
// `src/index.ts` imports the wasm binary, so it is deliberately NOT exercised
// here — its behavior is covered by the Rust-side `wasm_bindgen_test` modules
// in `crates/brink-web`.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
