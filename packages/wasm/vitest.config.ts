import { defineConfig } from "vitest/config";

// Unit tests here cover the pure, wasm-free logic (`src/evaluate-dispatch.ts`)
// plus the `src/index.ts` wrapper layer with `brink-web` replaced by a stub
// (`vi.mock` in `editor-session-handle.test.ts`) — the wrapper's passthrough
// wiring is package-observable API surface (PR #534 review caught a lever
// that was unreachable because a passthrough was missing). Real wasm behavior
// stays covered by the Rust-side `wasm_bindgen_test` modules in
// `crates/brink-web`; nothing here loads the wasm binary.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
