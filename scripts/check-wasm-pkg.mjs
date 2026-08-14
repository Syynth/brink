// Preflight/postflight check: does crates/brink-web/www/pkg actually hold a
// built wasm-pack output? (#2479)
//
// `@brink-lang/web` (packages/wasm) carries a `file:` devDependency on that
// directory. It only exists after
// `wasm-pack build crates/brink-web --target web --out-dir www/pkg` has run,
// which is why every CI lane that runs `pnpm install --frozen-lockfile`
// builds it first (.github/workflows/ci.yml's `frontend` and `e2e` jobs,
// .github/workflows/desktop-smoke.yml, .github/workflows/npm-release.yml) —
// see CLAUDE.md's "Cloud / fresh-environment sessions". That ordering is
// itself CI-self-enforcing (#2504): `every_pnpm_install_lane_builds_wasm_first_in_the_same_job`
// (packages/brink-desktop/src-tauri/src/lib.rs) checks every job in every
// workflow file and fails if any of the four lanes above (or a new one)
// runs `pnpm install --frozen-lockfile` without this build preceding it in
// the same job — this script is the runtime check for the failure mode,
// that test is the CI-time guard against the ordering regressing in the
// first place.
//
// ⚠ `pnpm install --frozen-lockfile` does NOT fail loudly when that
// ordering is skipped. Confirmed by direct reproduction (worktree with the
// pkg directory removed, pnpm store already warm from an earlier install):
// pnpm reports the per-package `file:` link failure —
//   ENOENT: no such file or directory, scandir '.../crates/brink-web/www/pkg'
// — but still **exits 0**, and it skips the root project's own
// `preinstall`/`postinstall` lifecycle scripts entirely (verified directly:
// pnpm gates ALL project lifecycle scripts on the whole install completing
// without a per-package error, so neither hook ever fires in exactly the
// state this check exists to catch). That rules out wiring this check as a
// pnpm lifecycle script — it would be dead code in the one case it needs to
// fire. Run it as an explicit, separate step instead, immediately after
// `pnpm install --frozen-lockfile` (see `pnpm check:wasm-pkg` and
// `scripts/setup-dev.sh`'s "Next steps").
//
// (On a genuinely cold pnpm store — nothing ever installed in this
// environment — the same missing-pkg install DOES exit non-zero; only a
// warm store hits the silent path. Since a warm store, not a cold one, is
// the common case for CI/dev-machine/agent-worktree pnpm caches, this check
// is not a rare-edge-case guard.)
//
// The logic is EXPORTED and the standalone run sits behind a main-guard,
// matching `packages/brink-desktop/scripts/ensure-wasm.mjs` /
// `ensure-cli-sidecar.mjs`'s "the `dev` preflight pair" shape
// (docs/desktop-shell-spec.md) — every input defaults to the real one, so
// `scripts/check-wasm-pkg.test.mjs` drives the real decision without a
// toolchain, and running `node scripts/check-wasm-pkg.mjs` standalone still
// does the whole job.
//
// This is a CHECK, not a build: it never invokes `wasm-pack` itself — it
// only reports what to run. Keep it that way; it has to stay silent and
// fast on the (already-correctly-sequenced) CI happy path.

import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "..");

// Keep in sync with packages/wasm/scripts/copy-wasm.mjs's `files` list —
// that script copies exactly these out of the wasm-pack output, so their
// presence is what actually makes @brink-lang/web buildable.
export const REQUIRED_FILES = [
  "brink_web.js",
  "brink_web.d.ts",
  "brink_web_bg.wasm",
  "brink_web_bg.wasm.d.ts",
];

export const BUILD_COMMAND =
  "wasm-pack build crates/brink-web --target web --out-dir www/pkg";

/**
 * Check whether the wasm-pack output `@brink-lang/web` links against is
 * present and complete. Returns `true` when every required file exists,
 * `false` otherwise — never throws, so a caller can decide what to do with
 * a `false` result (the main-guard below turns it into a failing process
 * exit code).
 *
 * Every input defaults to the real one this script has always used, so the
 * standalone invocation stays a bare `checkWasmPkg()`.
 */
export function checkWasmPkg({
  repoRoot = defaultRepoRoot,
  pkgDir = join(repoRoot, "crates/brink-web/www/pkg"),
  requiredFiles = REQUIRED_FILES,
  log = console.log,
  error = console.error,
} = {}) {
  const missing = requiredFiles.filter((file) => !existsSync(join(pkgDir, file)));

  if (missing.length === 0) {
    log("[check-wasm-pkg] crates/brink-web/www/pkg is present");
    return true;
  }

  error(
    [
      "[check-wasm-pkg] crates/brink-web/www/pkg is missing or incomplete " +
        `(missing: ${missing.join(", ")}).`,
      "",
      "packages/wasm (published as @brink-lang/web) declares a `file:`",
      "devDependency named `brink-web` (see packages/wasm/package.json) on",
      "that directory. `pnpm install --frozen-lockfile` can report success even",
      "when this link silently failed to resolve (#2479) — the real symptom",
      "shows up later and confusingly, e.g. as \"vitest: not found\" in an",
      "unrelated `pnpm --filter ... test` step.",
      "",
      "Build the wasm package, then reinstall:",
      "",
      `    ${BUILD_COMMAND}`,
      "    pnpm install --frozen-lockfile",
      "",
    ].join("\n"),
  );
  return false;
}

// Main-guard: `node scripts/check-wasm-pkg.mjs` still does the whole job,
// while `import`ing this module does nothing but hand over the functions —
// same shape as the desktop preflight pair (#2452, #2468).
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const ok = checkWasmPkg();
  if (!ok) {
    process.exitCode = 1;
  }
}
