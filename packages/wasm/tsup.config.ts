// Build for the published @brink-lang/web package.
//
// The wasm-pack output (crates/brink-web/www/pkg, `--target web`) is NOT
// bundled into index.js: the glue's default init locates the binary with
// `new URL("brink_web_bg.wasm", import.meta.url)`, which only works while
// the glue and the .wasm sit next to each other. So the wrapper is compiled
// with `brink-web` rewritten to a relative `./brink_web.js` import, and the
// glue + binary are copied into dist/ alongside it. Bundlers (Vite, Rollup,
// webpack 5) resolve the `new URL` pattern and emit the .wasm as an asset;
// no-bundler consumers get working relative URLs on any static host.
//
// `@brink/wasm-types` (private workspace package, pure type declarations)
// is rolled into dist/index.d.ts so the published types stand alone.
import { defineConfig } from "tsup";

export default defineConfig({
  entry: { index: "src/index.ts" },
  format: ["esm"],
  target: "es2022",
  dts: {
    // Inline the private types package into the rolled-up declarations.
    resolve: ["@brink/wasm-types"],
  },
  sourcemap: false,
  clean: true,
  esbuildPlugins: [
    {
      // Keep the wasm glue out of the bundle, as a relative sibling import.
      name: "brink-web-relative",
      setup(build) {
        build.onResolve({ filter: /^brink-web$/ }, () => ({
          path: "./brink_web.js",
          external: true,
        }));
      },
    },
  ],
});
