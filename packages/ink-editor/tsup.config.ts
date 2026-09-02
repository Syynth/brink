// Build for the published @brink-lang/editor package.
//
// The editor is TypeScript source. tsup emits an ESM bundle (dist/index.js)
// plus rolled-up declarations (dist/index.d.ts). The private @brink/* internal
// packages (@brink/wasm-types, @brink/ink-operations) are pure type/helper
// source with no publishable build of their own, so their types are inlined
// into the rollup via `dts.resolve`. `@brink-lang/web`, react, and the
// CodeMirror/Lezer packages stay external runtime imports (declared as
// dependencies / peerDependencies) so consumers dedupe them.
import { defineConfig } from "tsup";

export default defineConfig({
  entry: { index: "src/index.ts" },
  format: ["esm"],
  target: "es2022",
  dts: {
    // Inline the private workspace packages into the published declarations.
    resolve: [/^@brink\//],
  },
  sourcemap: false,
  clean: true,
  external: [
    "@brink-lang/dialect",
    "@brink-lang/web",
    "react",
    /^@codemirror\//,
    /^@lezer\//,
    "codemirror",
  ],
});
