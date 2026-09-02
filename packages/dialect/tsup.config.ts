// Build for the published @brink-lang/dialect package: pure TypeScript, no
// runtime dependencies. The private @brink/wasm-types mirrors (the Rust
// wire shapes) are inlined into the rolled-up declarations, exactly as
// @brink-lang/web does — an engine importing this package gets the
// `DialogueDialect` types without depending on anything private.
import { defineConfig } from "tsup";

export default defineConfig({
  entry: { index: "src/index.ts" },
  format: ["esm"],
  target: "es2022",
  dts: { resolve: ["@brink/wasm-types"] },
  sourcemap: false,
  clean: true,
});
