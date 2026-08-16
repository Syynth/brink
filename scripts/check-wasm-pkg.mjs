// Preflight/postflight check: does crates/brink-web/www/pkg actually hold a
// built wasm-pack output, AND did that output actually resolve into
// packages/wasm/node_modules? (#2479, #2514)
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
// ⚠ `checkWasmPkg` below only checks the CAUSE — did `wasm-pack build`
// actually produce output? It does NOT prove `pnpm install` linked that
// output anywhere `packages/wasm`'s own code can resolve it from (#2514,
// follow-up to #2504's item 2 / PR #2509's adversarial review "Scope gaps"
// item 2). A pnpm workspace resolves `packages/wasm`'s `file:` devDependency
// (`"brink-web": "file:../../crates/brink-web/www/pkg"`,
// packages/wasm/package.json) as a symlink chain rooted at
// `packages/wasm/node_modules/brink-web` — that is the EFFECT
// `checkWasmPkgLink` below asserts, by checking for LINKED_FILES (see its
// own comment: NOT the full REQUIRED_FILES — pnpm's own `file:` packing
// never links `brink_web_bg.wasm.d.ts` even on a healthy install) through
// that path instead of the raw wasm-pack output directory.
// `existsSync` follows symlinks, so a missing/dangling link and a resolved-
// but-empty/incomplete target both read the same way here: "not linked" —
// which is exactly the observable effect `packages/wasm/src/index.ts`'s
// `import ... from "brink-web"` (and `packages/wasm/scripts/copy-wasm.mjs`,
// via its own direct path into crates/brink-web/www/pkg) would hit.
// `checkWasmPkg`'s cause check still earns its place alongside it: it is
// what tells a developer to run `wasm-pack build` in the first place
// (`checkWasmPkgLink` alone, faced with a `file:` target that was never
// built, would report the same "missing/incomplete" shape but point at the
// wrong remediation — reinstalling cannot link something that was never
// built) — so the main-guard below runs both, independently, rather than
// short-circuit on the first failure.
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

// The subset of REQUIRED_FILES that a pnpm `file:` install actually links
// into `packages/wasm/node_modules/brink-web` — NOT all of REQUIRED_FILES.
// PROVISIONAL, empirically derived (no spec governs wasm-pack's own output
// shape): `wasm-pack build --target web` writes a `package.json` into
// `crates/brink-web/www/pkg` (regenerated on every build, gitignored, not
// committed) whose own `"files"` field — confirmed by direct reproduction —
// lists only `brink_web_bg.wasm`, `brink_web.js`, `brink_web.d.ts`. pnpm's
// `file:` resolution honours that allowlist the same way `npm pack` would,
// so `brink_web_bg.wasm.d.ts` is never linked into node_modules even on a
// fully healthy install. Asserting REQUIRED_FILES's full 4-file list against
// the resolved link would therefore fail on every correctly-ordered,
// correctly-built CI run — a false positive, not a real #2479/#2514 symptom.
// This is safe to treat as "not required" for the link check specifically:
// nothing downstream reads that file through the resolved link either —
// `crates/brink-web/www/pkg/brink_web.js` references the `.wasm` binary
// directly (`new URL('brink_web_bg.wasm', import.meta.url)`), and
// `packages/wasm/tsconfig.json` sets `skipLibCheck: true`, so
// `brink_web.d.ts` importing from `./brink_web_bg.wasm` is never actually
// checked against a sibling `.d.ts` that isn't there. `checkWasmPkg`'s cause
// check still verifies all four files exist in the raw wasm-pack output
// (via `packages/wasm/scripts/copy-wasm.mjs`'s direct filesystem copy,
// which does not go through node_modules and is unaffected by wasm-pack's
// package.json).
export const LINKED_FILES = REQUIRED_FILES.filter(
  (file) => file !== "brink_web_bg.wasm.d.ts",
);

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

/**
 * Check whether `packages/wasm`'s `file:` devDependency on the wasm-pack
 * output actually RESOLVED — the effect #2479/#2514 named, as opposed to
 * `checkWasmPkg` above's cause check. pnpm links a workspace package's
 * `file:` dependency as a symlink at `<package>/node_modules/<dep-name>`
 * (here: `packages/wasm/node_modules/brink-web`, verified by direct
 * reproduction — `readlink` shows it point through pnpm's virtual store,
 * `node_modules/.pnpm/brink-web@file+crates+brink-web+www+pkg/node_modules/brink-web`,
 * at the real crate output). `existsSync` follows both symlink hops, so a
 * link that is missing entirely and a link that resolves to an
 * empty/incomplete directory both surface here the same way a consumer
 * would actually hit them: the required file is not there to import.
 *
 * Returns `true` when every required file is reachable through the
 * resolved link, `false` otherwise — never throws, same contract as
 * `checkWasmPkg`. Every input defaults to the real one, so the standalone
 * invocation stays a bare `checkWasmPkgLink()`.
 */
export function checkWasmPkgLink({
  repoRoot = defaultRepoRoot,
  linkDir = join(repoRoot, "packages/wasm/node_modules/brink-web"),
  requiredFiles = LINKED_FILES,
  log = console.log,
  error = console.error,
} = {}) {
  const missing = requiredFiles.filter((file) => !existsSync(join(linkDir, file)));

  if (missing.length === 0) {
    log("[check-wasm-pkg] packages/wasm/node_modules/brink-web resolves to a complete wasm-pack output");
    return true;
  }

  error(
    [
      "[check-wasm-pkg] packages/wasm/node_modules/brink-web is missing or " +
        `incomplete (missing: ${missing.join(", ")}).`,
      "",
      "That is the resolved LOCATION of packages/wasm's `file:` devDependency",
      "on crates/brink-web/www/pkg (see packages/wasm/package.json's",
      "`brink-web` key) — not the wasm-pack output itself. `pnpm install",
      "--frozen-lockfile` can report success even when this link silently",
      "failed to resolve, or resolved to an empty/incomplete directory",
      "(#2479, #2514) — the real symptom shows up later and confusingly,",
      "e.g. as a module-not-found error importing \"brink-web\", or",
      "\"vitest: not found\" in an unrelated `pnpm --filter ... test` step.",
      "",
      "Reinstall so pnpm re-links against the wasm-pack output:",
      "",
      "    pnpm install --frozen-lockfile",
      "",
    ].join("\n"),
  );
  return false;
}

// Main-guard: `node scripts/check-wasm-pkg.mjs` still does the whole job,
// while `import`ing this module does nothing but hand over the functions —
// same shape as the desktop preflight pair (#2452, #2468). Both checks run
// independently (not short-circuited on the first failure, per the header
// comment) so a single invocation always reports the full picture.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const pkgOk = checkWasmPkg();
  const linkOk = checkWasmPkgLink();
  if (!pkgOk || !linkOk) {
    process.exitCode = 1;
  }
}
