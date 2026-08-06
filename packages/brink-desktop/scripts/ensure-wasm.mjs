// Wasm freshness preflight for `pnpm dev` (docs/desktop-shell-spec.md, D1).
//
// The dev server serves the wasm glue from crates/brink-web/www/pkg — a
// manually-built artifact. A stale pkg silently runs OLD compiler/editor
// behavior against current UI code, which produced a false bug report
// within hours of the first D1 session (the app faithfully showed
// diagnostics main had fixed the night before — the same
// stale-instrument failure mode as the shared-cargo-target phantoms).
// Ruled 2026-08-06: just rebuild. If any Rust source or manifest under
// crates/ is newer than the built wasm, run wasm-pack before vite starts;
// fail the dev command rather than serve stale.
//
// CARGO_TARGET_DIR is passed through from the environment untouched — the
// repo's shared-target conventions are a session concern, not this
// script's.

import { execSync } from "node:child_process";
import { readdirSync, statSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const repoRoot = resolve(here, "../../..");
const cratesDir = join(repoRoot, "crates");
const pkgWasm = join(cratesDir, "brink-web/www/pkg/brink_web_bg.wasm");

/** Newest mtime of any .rs / Cargo.toml under `dir`, skipping build output. */
function newestSource(dir) {
  let newest = 0;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      // `pkg` is the OUTPUT this check exists to compare against; `target`
      // and `node_modules` are build state; dotdirs are never sources.
      if (
        entry.name === "pkg" ||
        entry.name === "target" ||
        entry.name === "node_modules" ||
        entry.name.startsWith(".")
      ) {
        continue;
      }
      newest = Math.max(newest, newestSource(path));
    } else if (entry.name.endsWith(".rs") || entry.name === "Cargo.toml") {
      newest = Math.max(newest, statSync(path).mtimeMs);
    }
  }
  return newest;
}

const built = existsSync(pkgWasm) ? statSync(pkgWasm).mtimeMs : 0;
const sources = newestSource(cratesDir);

if (built >= sources) {
  console.log("[ensure-wasm] pkg is fresh");
  process.exit(0);
}

console.log(
  built === 0
    ? "[ensure-wasm] no wasm pkg found — building"
    : "[ensure-wasm] crates/ sources are newer than the built pkg — rebuilding",
);
execSync("wasm-pack build crates/brink-web --target web --out-dir www/pkg", {
  cwd: repoRoot,
  stdio: "inherit",
});
console.log("[ensure-wasm] rebuilt");
