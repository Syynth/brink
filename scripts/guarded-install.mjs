// A `pnpm install` that verifies its own PRECONDITION and its own EFFECT,
// so the install step cannot report success while having done nothing
// (#2593 — the third variant of the #2479 family).
//
// ## Why this exists rather than a pnpm lifecycle hook
//
// `packages/wasm` (published as `@brink-lang/web`) carries a `file:`
// devDependency named `brink-web` on `crates/brink-web/www/pkg`, which only
// exists after `wasm-pack build crates/brink-web --target web --out-dir
// www/pkg` has run. Install the workspace before that build and pnpm cannot
// resolve the link.
//
// The obvious fix — a root `preinstall` script — DOES NOT WORK, and this was
// re-verified directly on the current pnpm rather than assumed (#2593 asked
// for exactly that check; #2492 found the same on an earlier pnpm):
//
//   1. `crates/brink-web/www/pkg` removed, root `preinstall` + `postinstall`
//      added as probes → `pnpm install` printed
//      `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` and NEITHER probe fired.
//   2. Same tree with the directory restored → both probes fired.
//
// pnpm gates every project lifecycle script on the whole install completing
// without a per-package error, so a `preinstall` hook is dead code in
// precisely the state it would need to fire in. The ordering therefore
// cannot be enforced from inside the install; it has to be enforced by
// something that wraps it. That is this script.
//
// ## What was actually observed for #2593 (pnpm 10.34.5)
//
// #2593 reports the missing-pkg install exiting **0** with no `node_modules`
// written. Reproduced on pnpm 10.34.5 in four permutations — cold store /
// warm store x `node_modules` absent / present — and in all four pnpm
// printed `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` (on stdout, not stderr) and
// exited **1**, never 0. The "total no-op" half of the report reproduced
// exactly: nothing at all is written, so the next command dies with a bare
// `vitest: not found`. The "exit 0" half did not.
//
// That is a reason to stop DEPENDING on pnpm's exit code, not a reason to
// skip the guard. The repo pins pnpm only to a floating major (`corepack
// prepare pnpm@10` in scripts/setup-dev.sh, `pnpm/action-setup version: 10`
// in .github/workflows/ci.yml), so which of the two behaviours a given
// machine gets is whatever 10.x resolved there that day — the exact shape
// that let #2479 and #2531 survive. So the post-check below asserts the
// EFFECT (did an installed tree actually appear?) INDEPENDENTLY of what
// pnpm reported, and fails non-zero on a silent no-op regardless.
//
// ## Contract
//
//   pre   `checkWasmPkg()` — the wasm-pack output exists. If not, exit 1
//         WITHOUT running pnpm, so no half-written tree is produced at all.
//   run   `pnpm install <forwarded args>`, stdio inherited.
//   post  root `node_modules/` exists AND `checkWasmPkgLink()` passes.
//         Either failing is a non-zero exit even if pnpm exited 0.
//
// The logic is EXPORTED and the standalone run sits behind a main-guard,
// matching scripts/check-wasm-pkg.mjs and
// packages/brink-desktop/scripts/ensure-wasm.mjs — every input defaults to
// the real one, so scripts/guarded-install.test.mjs drives the real decision
// against a stubbed pnpm without a toolchain, and `node
// scripts/guarded-install.mjs` standalone still does the whole job.
//
// This script has NO dependencies beyond node builtins and its sibling
// check module — it has to run in a tree with no `node_modules`, which is
// the state it exists to diagnose.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { BUILD_COMMAND, checkWasmPkg, checkWasmPkgLink } from "./check-wasm-pkg.mjs";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "..");

/**
 * Sanitise the args forwarded to `pnpm install`.
 *
 * Two hazards, both found by running the real command rather than reasoning
 * about it:
 *
 *  1. `pnpm install:checked -- --frozen-lockfile` forwards the literal `--`
 *     SEPARATOR to the script (pnpm 10.34.5 — `process.argv` really is
 *     `["--", "--frozen-lockfile"]`). Passing that straight through produces
 *     `pnpm install -- --frozen-lockfile`, and pnpm reads everything after
 *     `--` as PACKAGE NAMES TO ADD: the first run of this script wrote
 *     `"dependencies": { "--frozen-lockfile": "^1.0.0" }` into package.json
 *     and a matching pnpm-lock.yaml entry. So leading `--` separators are
 *     dropped here.
 *  2. Anything left that is not a flag is likewise a package name, i.e.
 *     `pnpm install:checked lodash` would silently ADD a dependency. This
 *     command verifies an install; it must never mutate the manifest. A
 *     positional is rejected rather than forwarded.
 *
 * Returns `{ args }` on success or `{ error }` describing the rejection.
 */
export function sanitizeInstallArgs(rawArgs) {
  const args = rawArgs.filter((arg) => arg !== "--");
  const positional = args.find((arg) => !arg.startsWith("-"));

  if (positional !== undefined) {
    return {
      error:
        `[guarded-install] refusing to forward the bare argument "${positional}" to ` +
        "`pnpm install` — pnpm would read it as a package name and ADD it to " +
        "package.json. This command verifies an install; it never changes the " +
        "manifest. Use `pnpm add` for that.",
    };
  }

  return { args };
}

/**
 * Run `pnpm install` with a precondition and a postcondition around it.
 *
 * Returns the process exit code to use: `0` only when pnpm succeeded AND an
 * installed tree actually materialised. Never throws.
 *
 * Every input defaults to the real one, so the standalone invocation stays a
 * bare `guardedInstall({ args })`.
 *
 * @param {object} [options]
 * @param {string} [options.repoRoot]
 * @param {string[]} [options.args] extra args forwarded to `pnpm install`
 * @param {string} [options.nodeModulesDir]
 * @param {() => boolean} [options.checkPkg] precondition (cause check)
 * @param {() => boolean} [options.checkLink] postcondition (effect check)
 * @param {(args: string[]) => number} [options.runInstall] returns pnpm's exit code
 * @param {(msg: string) => void} [options.log]
 * @param {(msg: string) => void} [options.error]
 */
export function guardedInstall({
  repoRoot = defaultRepoRoot,
  args = [],
  nodeModulesDir = join(repoRoot, "node_modules"),
  checkPkg = () => checkWasmPkg({ repoRoot }),
  checkLink = () => checkWasmPkgLink({ repoRoot }),
  runInstall = (installArgs) => defaultRunInstall(installArgs, repoRoot),
  log = console.log,
  error = console.error,
} = {}) {
  // --- argument sanitising --------------------------------------------------
  const sanitized = sanitizeInstallArgs(args);
  if (sanitized.error) {
    error(sanitized.error);
    return 1;
  }
  const installArgs = sanitized.args;

  // --- precondition ---------------------------------------------------------
  // Deliberately BEFORE pnpm runs. Letting pnpm run first and diagnosing
  // afterwards would leave a partially-written tree behind on the failure
  // path, which is the state #2479 was about.
  if (!checkPkg()) {
    error(
      [
        "",
        "[guarded-install] REFUSING TO INSTALL — the wasm-pack output packages/wasm",
        "links against is not built yet, so `pnpm install` would resolve nothing and",
        "write nothing (#2593). Nothing was installed; the tree is unchanged.",
        "",
        "Build it first, then re-run this command:",
        "",
        `    ${BUILD_COMMAND}`,
        "",
      ].join("\n"),
    );
    return 1;
  }

  // --- the install itself ---------------------------------------------------
  log(
    `[guarded-install] running: pnpm install${installArgs.length ? ` ${installArgs.join(" ")}` : ""}`,
  );
  const status = runInstall(installArgs);

  if (status !== 0) {
    error(
      [
        "",
        `[guarded-install] \`pnpm install\` exited ${status}. Nothing further checked —`,
        "pnpm's own error above is the failure. If it names",
        "ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND, the wasm-pack output went missing",
        "between the check above and the install itself; rebuild it with:",
        "",
        `    ${BUILD_COMMAND}`,
        "",
      ].join("\n"),
    );
    return status;
  }

  // --- postcondition --------------------------------------------------------
  // Reached ONLY when pnpm claimed success. Everything below exists because
  // that claim is not trustworthy across pnpm versions (see the header).
  const problems = [];

  if (!existsSync(nodeModulesDir)) {
    problems.push(
      "`pnpm install` exited 0 but wrote NO node_modules at all — a total no-op " +
        "install (#2593). The next command in your sequence would fail with a bare " +
        '"vitest: not found" (or similar), which reads as a missing binary rather ' +
        "than a skipped install.",
    );
  }

  // The #2479/#2514 shape: a tree was written, but packages/wasm's `file:`
  // devDependency did not resolve into it.
  if (!checkLink()) {
    problems.push(
      "`pnpm install` exited 0 but packages/wasm's `file:` devDependency on the " +
        "wasm-pack output did not resolve (#2479, #2514) — see the check-wasm-pkg " +
        "report above.",
    );
  }

  if (problems.length > 0) {
    error(
      [
        "",
        "[guarded-install] INSTALL REPORTED SUCCESS BUT DID NOT PRODUCE AN INSTALLED TREE.",
        "",
        ...problems.map((problem) => `  - ${problem}`),
        "",
        "Failing non-zero rather than letting this scroll past — a warning here is",
        "exactly how #2479 survived three separate reports. Rebuild wasm and retry:",
        "",
        `    ${BUILD_COMMAND}`,
        "    pnpm install:checked",
        "",
      ].join("\n"),
    );
    return 1;
  }

  log("[guarded-install] install verified: node_modules present and brink-web linked");
  return 0;
}

/**
 * The real `pnpm install` invocation. Split out so the test can replace it
 * without stubbing `child_process` globally.
 *
 * `shell: false` — the args are forwarded verbatim, never re-parsed by a
 * shell. Returns 127 for a pnpm that could not be spawned at all, matching
 * the shell convention for "command not found" (spawnSync reports that as
 * `status: null` plus an `error`, which would otherwise read as success).
 */
function defaultRunInstall(args, repoRoot) {
  const result = spawnSync("pnpm", ["install", ...args], {
    cwd: repoRoot,
    stdio: "inherit",
    shell: false,
  });

  if (result.error) {
    console.error(`[guarded-install] could not run pnpm: ${result.error.message}`);
    return 127;
  }
  // A signal-killed child reports status: null; treat that as a failure too.
  return result.status === null ? 1 : result.status;
}

// Main-guard: `node scripts/guarded-install.mjs [args...]` does the whole
// job, while `import`ing this module does nothing but hand over the
// function — same shape as scripts/check-wasm-pkg.mjs (#2492) and the
// desktop preflight pair (#2452, #2468).
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = guardedInstall({ args: process.argv.slice(2) });
}
