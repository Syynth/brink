// Tests for scripts/guarded-install.mjs (#2593). Node's built-in test
// runner, matching scripts/check-wasm-pkg.test.mjs — the root has no
// vitest/other framework wired, and adding one for two small scripts would
// be a new root devDependency for no gain. Run via `pnpm test:scripts`
// (wired into .github/workflows/ci.yml's `frontend` job).
//
// Two layers, deliberately:
//
//   - UNIT: `guardedInstall()` with every dependency injected, so the
//     decision table (pre-check / pnpm status / post-check) is exercised
//     directly.
//   - END TO END: the REAL script, spawned as a process, against a fixture
//     tree and a PATH-injected `pnpm` stub — the pattern
//     scripts/setup-dev.test.sh established (#2584). This is what proves the
//     main-guard actually wires the exit code, and that the pre-check runs
//     BEFORE pnpm is spawned (asserted by the stub's own marker file, not by
//     reading the source).
//
// A pure string/grep assertion on the source would not catch either bug this
// guards: both are control flow (does pnpm run at all? does a 0 from pnpm
// still fail?), not text that is present or absent.

import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import { guardedInstall, sanitizeInstallArgs } from "./guarded-install.mjs";
import { LINKED_FILES, REQUIRED_FILES } from "./check-wasm-pkg.mjs";

const here = dirname(fileURLToPath(import.meta.url));

const temporaries = [];

after(() => {
  for (const dir of temporaries) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function scratchDir(prefix) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  temporaries.push(dir);
  return dir;
}

// --- unit layer --------------------------------------------------------------

/** Collects stdout/stderr and records whether the install actually ran. */
function harness(overrides = {}) {
  const logs = [];
  const errors = [];
  const installCalls = [];

  const code = guardedInstall({
    checkPkg: () => true,
    checkLink: () => true,
    nodeModulesDir: here, // an existing directory == "node_modules present"
    runInstall: (args) => {
      installCalls.push(args);
      return 0;
    },
    log: (msg) => logs.push(msg),
    error: (msg) => errors.push(msg),
    ...overrides,
  });

  return { code, logs, errors, installCalls, stderr: errors.join("\n") };
}

describe("guardedInstall (decision table)", () => {
  it("returns 0 and reports verification when every condition holds", () => {
    const { code, installCalls, errors } = harness();

    assert.equal(code, 0);
    assert.equal(installCalls.length, 1, "pnpm install should have run exactly once");
    assert.deepEqual(errors, [], "the healthy path must stay silent on stderr");
  });

  it("refuses to run pnpm at all when the wasm-pack output is missing", () => {
    const { code, installCalls, stderr } = harness({ checkPkg: () => false });

    assert.equal(code, 1);
    assert.equal(
      installCalls.length,
      0,
      "the pre-check must short-circuit BEFORE pnpm runs, so no half-written tree is left behind",
    );
    assert.match(stderr, /REFUSING TO INSTALL/);
    assert.match(stderr, /wasm-pack build crates\/brink-web --target web --out-dir www\/pkg/);
  });

  // The #2593 shape itself: pnpm claims success, nothing was written.
  it("fails non-zero when pnpm exits 0 but wrote no node_modules at all", () => {
    const { code, stderr } = harness({
      nodeModulesDir: join(scratchDir("guarded-install-"), "node_modules"),
    });

    assert.equal(code, 1, "a total no-op install must be non-zero, not a warning");
    assert.match(stderr, /INSTALL REPORTED SUCCESS BUT DID NOT PRODUCE AN INSTALLED TREE/);
    assert.match(stderr, /total no-op/);
    // The confusing downstream symptom is named, because that is what the
    // reader actually saw first (#2593).
    assert.match(stderr, /vitest: not found/);
  });

  // The #2479/#2514 shape: a tree exists, but the file: link did not resolve.
  it("fails non-zero when pnpm exits 0 but the brink-web link did not resolve", () => {
    const { code, stderr } = harness({ checkLink: () => false });

    assert.equal(code, 1);
    assert.match(stderr, /did not resolve/);
    assert.match(stderr, /#2479/);
  });

  it("propagates pnpm's own non-zero exit code unchanged", () => {
    const { code, stderr } = harness({ runInstall: () => 254 });

    assert.equal(code, 254, "a real pnpm failure must not be flattened to 1");
    assert.match(stderr, /exited 254/);
  });

  it("does not run the post-check when pnpm already failed", () => {
    let linkChecked = false;
    const { code } = harness({
      runInstall: () => 1,
      checkLink: () => {
        linkChecked = true;
        return true;
      },
    });

    assert.equal(code, 1);
    assert.equal(linkChecked, false, "pnpm's own error is the failure; don't pile on");
  });

  it("forwards extra flags to pnpm install", () => {
    const { installCalls } = harness({ args: ["--frozen-lockfile", "--prefer-offline"] });

    assert.deepEqual(installCalls[0], ["--frozen-lockfile", "--prefer-offline"]);
  });

  // Regression: `pnpm install:checked -- --frozen-lockfile` forwards the
  // literal `--` to the script, and `pnpm install -- --frozen-lockfile`
  // reads everything after `--` as a PACKAGE NAME. The first real run of
  // this script wrote `"dependencies": {"--frozen-lockfile": "^1.0.0"}` into
  // package.json and pnpm-lock.yaml exactly this way.
  it("drops the `--` separator pnpm forwards, so it is never read as a package name", () => {
    const { code, installCalls } = harness({ args: ["--", "--frozen-lockfile"] });

    assert.equal(code, 0);
    assert.deepEqual(
      installCalls[0],
      ["--frozen-lockfile"],
      "a forwarded `--` must not reach pnpm — it turns the next arg into a package to add",
    );
  });

  it("refuses a bare positional argument rather than letting pnpm add it as a package", () => {
    const { code, installCalls, stderr } = harness({ args: ["lodash"] });

    assert.equal(code, 1);
    assert.equal(installCalls.length, 0, "must not reach pnpm at all");
    assert.match(stderr, /refusing to forward the bare argument "lodash"/);
  });
});

describe("sanitizeInstallArgs", () => {
  it("passes flags through unchanged", () => {
    assert.deepEqual(sanitizeInstallArgs(["--frozen-lockfile"]).args, ["--frozen-lockfile"]);
  });

  it("strips every `--` separator, wherever it appears", () => {
    assert.deepEqual(sanitizeInstallArgs(["--", "-r", "--"]).args, ["-r"]);
  });

  it("reports an error for a positional instead of returning args", () => {
    const result = sanitizeInstallArgs(["--frozen-lockfile", "left-pad"]);

    assert.equal(result.args, undefined);
    assert.match(result.error, /left-pad/);
  });

  // A space-separated flag VALUE (`--filter @scope/pkg`, `--reporter default`)
  // hits this same rejection, because it is indistinguishable from a bare
  // package name at this layer. The rejection is correct to keep (this
  // command must never mutate the manifest) but the error text must name the
  // real escape hatch — `--flag=value` — rather than pointing at `pnpm add`,
  // which is not what a caller passing `--filter @scope/pkg` meant to do.
  it("names the `--flag=value` escape hatch for a would-be flag value", () => {
    const result = sanitizeInstallArgs(["--filter", "@brink-lang/studio"]);

    assert.equal(result.args, undefined);
    assert.match(result.error, /@brink-lang\/studio/);
    assert.match(result.error, /--flag=value/);
  });
});

// --- end-to-end layer --------------------------------------------------------

/**
 * Build a minimal fixture repo: the two REAL scripts under `scripts/`, plus
 * whatever tree state the case under test needs. The script derives its
 * repoRoot from its own location, so copying it into the fixture is what
 * points it at the fixture — no test-only env knob in production code.
 */
function makeFixture({ withPkg }) {
  const root = scratchDir("guarded-install-e2e-");

  mkdirSync(join(root, "scripts"), { recursive: true });
  for (const script of ["guarded-install.mjs", "check-wasm-pkg.mjs"]) {
    cpSync(join(here, script), join(root, "scripts", script));
  }

  if (withPkg) {
    const pkgDir = join(root, "crates/brink-web/www/pkg");
    mkdirSync(pkgDir, { recursive: true });
    for (const file of REQUIRED_FILES) {
      writeFileSync(join(pkgDir, file), "stub");
    }
  }

  return root;
}

/**
 * A `pnpm` stub on PATH. Always records that it ran (so "the pre-check
 * short-circuited" is observable), then behaves as configured.
 *
 * `writeTree` reproduces what a healthy pnpm leaves behind: a root
 * node_modules plus packages/wasm's resolved `file:` link. Omitting it is
 * exactly the #2593 no-op.
 */
function makePnpmStub(root, { exitCode, writeTree }) {
  const binDir = join(root, "stub-bin");
  mkdirSync(binDir, { recursive: true });

  const markerPath = join(root, "pnpm-ran.marker");
  const linkDir = join(root, "packages/wasm/node_modules/brink-web");

  const treeCommands = writeTree
    ? [
        `mkdir -p ${JSON.stringify(join(root, "node_modules"))}`,
        `mkdir -p ${JSON.stringify(linkDir)}`,
        ...LINKED_FILES.map(
          (file) => `printf stub > ${JSON.stringify(join(linkDir, file))}`,
        ),
      ]
    : [];

  writeFileSync(
    join(binDir, "pnpm"),
    [
      "#!/usr/bin/env bash",
      `printf '%s\\n' "$*" >> ${JSON.stringify(markerPath)}`,
      ...treeCommands,
      `exit ${exitCode}`,
      "",
    ].join("\n"),
  );
  chmodSync(join(binDir, "pnpm"), 0o755);

  return { binDir, markerPath };
}

function runRealScript(root, binDir, args = []) {
  const result = spawnSync(
    process.execPath,
    [join(root, "scripts/guarded-install.mjs"), ...args],
    {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, PATH: `${binDir}${delimiter}${process.env.PATH ?? ""}` },
    },
  );
  return { status: result.status, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
}

describe("scripts/guarded-install.mjs end to end (real script, stubbed pnpm)", () => {
  // RED case the issue asked for: a tree with crates/brink-web/www/pkg
  // removed must make this fire.
  it("exits non-zero and never spawns pnpm when crates/brink-web/www/pkg is absent", () => {
    const root = makeFixture({ withPkg: false });
    const { binDir, markerPath } = makePnpmStub(root, { exitCode: 0, writeTree: true });

    const { status, output } = runRealScript(root, binDir);

    assert.notEqual(status, 0, "the missing-pkg tree must FAIL, not warn");
    assert.match(output, /REFUSING TO INSTALL/);
    assert.equal(
      existsSync(markerPath),
      false,
      "pnpm must never have been spawned — the guard runs before it",
    );
    // checkWasmPkg's own remediation prints above guardedInstall's REFUSING TO
    // INSTALL message (it runs first, as the precondition check) — it must
    // not tell a fresh-checkout dev to trust the unguarded command this whole
    // family of fixes exists to stop relying on.
    assert.doesNotMatch(
      output,
      /^ {4}pnpm install --frozen-lockfile$/m,
      "the refusal output must not recommend a bare `pnpm install --frozen-lockfile`",
    );
    assert.match(
      output,
      /pnpm install:checked/,
      "the refusal output should point at the guarded entry point instead",
    );
  });

  // GREEN case: the same script must stay quiet on a healthy tree.
  it("exits 0 on a healthy tree where pnpm produces an installed tree", () => {
    const root = makeFixture({ withPkg: true });
    const { binDir, markerPath } = makePnpmStub(root, { exitCode: 0, writeTree: true });

    // Passed exactly as `pnpm install:checked -- --frozen-lockfile` really
    // delivers them: pnpm forwards the `--` separator too.
    const { status, output } = runRealScript(root, binDir, ["--", "--frozen-lockfile"]);

    assert.equal(status, 0, `healthy tree should pass, got:\n${output}`);
    assert.doesNotMatch(output, /REFUSING TO INSTALL/);
    assert.doesNotMatch(output, /DID NOT PRODUCE AN INSTALLED TREE/);
    assert.equal(
      readFileSync(markerPath, "utf8").trim(),
      "install --frozen-lockfile",
      "flags must reach pnpm, and the forwarded `--` must NOT — pnpm reads args after it as packages to add",
    );
  });

  // The exact #2593 report: pnpm exits 0 having written nothing.
  it("exits non-zero when pnpm exits 0 having written nothing", () => {
    const root = makeFixture({ withPkg: true });
    const { binDir } = makePnpmStub(root, { exitCode: 0, writeTree: false });

    const { status, output } = runRealScript(root, binDir);

    assert.notEqual(
      status,
      0,
      "an install that exits 0 having produced nothing must be a hard failure",
    );
    assert.match(output, /INSTALL REPORTED SUCCESS BUT DID NOT PRODUCE AN INSTALLED TREE/);
  });

  it("propagates a real pnpm failure", () => {
    const root = makeFixture({ withPkg: true });
    const { binDir } = makePnpmStub(root, { exitCode: 254, writeTree: false });

    const { status } = runRealScript(root, binDir);

    assert.equal(status, 254);
  });
});

// --- doc reachability --------------------------------------------------------
// House rule (.claude/skills/autonomous-pump/BRINK-CONFIG.md): a doc claim
// about reachability needs a test that exercises the real chain. CLAUDE.md's
// "Cloud / fresh-environment sessions" and scripts/setup-dev.sh's printed
// "Next steps" both tell a fresh worktree to run `pnpm install:checked`. If
// that script name is renamed or dropped, those instructions become a
// command that does not exist — the same class of silent gap #2593 is about.

const repoRoot = join(here, "..");

describe("the documented entry point actually exists", () => {
  it("package.json exposes `install:checked` pointing at this script", () => {
    const manifest = JSON.parse(readFileSync(join(repoRoot, "package.json"), "utf8"));

    assert.equal(
      manifest.scripts["install:checked"],
      "node scripts/guarded-install.mjs",
      "CLAUDE.md and scripts/setup-dev.sh both instruct readers to run `pnpm install:checked`",
    );
  });

  it("scripts/setup-dev.sh's printed Next steps name it", () => {
    const setupDev = readFileSync(join(repoRoot, "scripts/setup-dev.sh"), "utf8");

    assert.match(
      setupDev,
      /pnpm install:checked/,
      "setup-dev.sh is the documented fresh-environment entry point; its Next steps must name the guarded install",
    );
    assert.doesNotMatch(
      setupDev,
      /^echo "    pnpm check:wasm-pkg && pnpm install --frozen-lockfile"$/m,
      "the old unguarded sequence must not come back — it relies on pnpm's exit code (#2593)",
    );
  });

  it("CLAUDE.md's cloud-session section names it", () => {
    const claudeMd = readFileSync(join(repoRoot, "CLAUDE.md"), "utf8");

    assert.match(claudeMd, /pnpm install:checked/);
  });
});
