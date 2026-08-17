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
//
// The logic is EXPORTED and the standalone run sits behind a main-guard at
// the bottom (#2468), matching `ensure-cli-sidecar.mjs` (#2452): the two
// scripts are the `dev` preflight pair (`dev` runs this one immediately
// before that one), and #2452 named only the sibling. Until #2468 this
// module ran its whole job — including a real `wasm-pack build`, and a
// `process.exit(0)` in the already-fresh case — as a side effect of being
// imported, so nothing could call it and nothing could test it.
//
// ⚠ The guard is an invariant of the PAIR, not of one script: see
// docs/desktop-shell-spec.md "The `dev` preflight pair", and
// src/__tests__/ensure-wasm.test.ts's `describe("the main-guard")`.
//
// It is an invariant of the DIRECTORY too (#2478):
// src/__tests__/scripts-main-guard.test.ts scans every
// packages/brink-desktop/scripts/*.mjs rather than naming files, so a third
// preflight script is checked the moment it lands. Removing the guard line
// below fails that scan as well as this script's own tests.

import { execSync } from "node:child_process";
import { readdirSync, statSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "../../..");

/**
 * Default bound for every command this module shells out to (#2697). Before
 * this, `wasm-pack build` ran on no clock at all on the `pnpm --filter
 * @brink/desktop dev` preflight path — the same wedged-proxy hang class
 * scripts/check-scripts.mjs bounds for shell scripts via `run_with_timeout`,
 * one language over: `wasm-pack build` fetches binaryen/wasm-opt from GitHub
 * releases on a cache miss, and a stalled fetch there hung the whole preflight
 * forever with no diagnostic. Baked into `defaultRunCommand`'s own `execSync`
 * call (before `...options`) rather than into each call site, so it is the
 * one real bound instead of something every future caller has to remember to
 * pass — a caller can still override it by spreading its own `timeout` in
 * afterward.
 *
 * Overridable via BRINK_ENSURE_WASM_TIMEOUT_MS (#2702 review), matching
 * scripts/setup-dev.sh's BRINK_SETUP_*_TIMEOUT convention one language over
 * (milliseconds here, not seconds, matching `execSync`'s own `timeout`
 * option's unit):
 *
 *   Knob                           Default              On timeout
 *   ---------------------------------------------------------------------
 *   BRINK_ENSURE_WASM_TIMEOUT_MS   20 * 60 * 1000 (20m)  FAIL — `ensureWasm`
 *                                                         rethrows a
 *                                                         diagnostic naming
 *                                                         the bound and this
 *                                                         env var.
 *
 * Sized against the COLD case, not the warm one it used to be defended
 * against alone: `ensureWasm`'s `built === 0` branch ("no wasm pkg found —
 * building") is exactly the fresh-clone path, where a release-mode
 * `wasm-pack build` of the whole compiler graph plus `wasm-opt` is the slow
 * case, not an incremental rebuild on a warm toolchain cache. 20 minutes is
 * an estimate — no measured cold-build number was available at review time
 * (#2702) — sized to at least match `ensure-cli-sidecar.mjs`'s real
 * `cargo build --release` of a comparable dependency graph; raise
 * BRINK_ENSURE_WASM_TIMEOUT_MS if it proves short on a real fresh clone.
 */
export const DEFAULT_EXEC_TIMEOUT_MS = Number(process.env.BRINK_ENSURE_WASM_TIMEOUT_MS) || 20 * 60 * 1000;

/**
 * Run a command and capture its stdout. The single seam through which this
 * module talks to `wasm-pack`, so a caller can drive the freshness logic
 * without a toolchain.
 *
 * On a timeout (the `execSync` `timeout` option firing), rethrows with a
 * house-style diagnostic naming the bound and the env var to raise (#2702
 * review): a bare `Error: Command failed: wasm-pack build …` left a
 * developer with no indication this was a bound firing rather than a real
 * build failure.
 *
 * The discriminator is `code === "ETIMEDOUT"`, NOT `killed`. Probed on this
 * Node rather than assumed: `execSync("sleep 5", { timeout: 30 })` throws a
 * spawnSync system error with `code: "ETIMEDOUT"`, `errno: -110`,
 * `signal: "SIGTERM"`, message `spawnSync /bin/sh ETIMEDOUT` — and
 * `killed` UNDEFINED, because `killed` lives on spawnSync's RESULT object,
 * not on the error it throws. A `killed`-only predicate never fires and
 * leaves this whole branch dead; `killed` is kept as a second arm for the
 * spawn paths that do set it.
 */
export function defaultRunCommand(command, options = {}) {
  try {
    return execSync(command, { encoding: "utf8", timeout: DEFAULT_EXEC_TIMEOUT_MS, ...options });
  } catch (error) {
    if (error && (error.code === "ETIMEDOUT" || error.killed)) {
      const effectiveTimeout = options.timeout ?? DEFAULT_EXEC_TIMEOUT_MS;
      throw new Error(
        `[ensure-wasm] ✗ \`${command}\` TIMED OUT after ${effectiveTimeout}ms — likely a stalled ` +
          `proxy, or a slow cold build (a release-mode wasm build of the whole compiler graph plus wasm-opt, ` +
          `on the "no wasm pkg found" fresh-clone path, is the case this bound has to cover). Retry when ` +
          `network is stable, or raise BRINK_ENSURE_WASM_TIMEOUT_MS.`,
        { cause: error },
      );
    }
    throw error;
  }
}

/** Newest mtime of any .rs / Cargo.toml under `dir`, skipping build output. */
export function newestSource(dir) {
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

/**
 * Rebuild the wasm pkg the dev server serves if any Rust source under
 * `cratesDir` is newer than the built artifact. Returns true when it
 * rebuilt, false when the pkg was already fresh.
 *
 * Every input defaults to the real one this script has always used, so the
 * standalone invocation below stays a bare `ensureWasm()`.
 */
export function ensureWasm({
  repoRoot = defaultRepoRoot,
  cratesDir = join(repoRoot, "crates"),
  pkgWasm = join(cratesDir, "brink-web/www/pkg/brink_web_bg.wasm"),
  runCommand = defaultRunCommand,
  log = console.log,
} = {}) {
  const built = existsSync(pkgWasm) ? statSync(pkgWasm).mtimeMs : 0;
  const sources = newestSource(cratesDir);

  if (built >= sources) {
    log("[ensure-wasm] pkg is fresh");
    return false;
  }

  log(
    built === 0
      ? "[ensure-wasm] no wasm pkg found — building"
      : "[ensure-wasm] crates/ sources are newer than the built pkg — rebuilding",
  );
  runCommand("wasm-pack build crates/brink-web --target web --out-dir www/pkg", {
    cwd: repoRoot,
    stdio: "inherit",
  });
  log("[ensure-wasm] rebuilt");
  return true;
}

// Main-guard: `node scripts/ensure-wasm.mjs` (what the `dev` package script
// runs, immediately before `ensure-cli-sidecar.mjs`) still does the whole
// job, while `import`ing this module does nothing but hand over the
// functions. The already-fresh case used to `process.exit(0)` here; falling
// off the end of the guard exits 0 the same way, and a rebuild failure
// still throws out of `execSync` and fails `dev`.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  ensureWasm();
}
