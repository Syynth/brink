// Copy the wasm-pack output (glue JS + .wasm binary + their declarations)
// into dist/ next to the compiled wrapper, which imports "./brink_web.js".
// Runs AFTER tsup — tsup's DTS writer races with copies made in onSuccess
// and can clobber freshly copied .d.ts files.
//
// `files` below is mirrored by scripts/check-wasm-pkg.mjs's REQUIRED_FILES
// (repo root) — that script's preflight check reads this same set, and
// scripts/check-wasm-pkg.test.mjs asserts the two lists stay in sync.
import { copyFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const wasmPkg = resolve(here, "../../../crates/brink-web/www/pkg");
const dist = resolve(here, "../dist");

const files = [
  "brink_web.js",
  "brink_web.d.ts",
  "brink_web_bg.wasm",
  "brink_web_bg.wasm.d.ts",
];

await mkdir(dist, { recursive: true });
await Promise.all(files.map((f) => copyFile(resolve(wasmPkg, f), resolve(dist, f))));
