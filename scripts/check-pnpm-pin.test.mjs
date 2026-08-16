// Tests for scripts/check-pnpm-pin.mjs (#2604). Node's built-in test runner,
// matching check-wasm-pkg.test.mjs / guarded-install.test.mjs: this file runs
// under `pnpm test:scripts`, which CI's `frontend` job executes BEFORE
// `pnpm install`, so it must not depend on anything installed.
//
// Two halves:
//   1. Unit tests over the pure checkers, driven with SYNTHETIC drift — the
//      planted-mismatch proofs (a floating `version: 10`, a `pnpm@10` range,
//      a second hardcoded pin in setup-dev.sh) that show each check goes red
//      rather than merely passing on a healthy tree.
//   2. Integration tests over the REAL repo files, so the pin, setup-dev.sh,
//      the workflow lanes and the pnpm actually on PATH cannot drift apart
//      silently. This is the "assert the resolved version in one place" the
//      issue asks for.

import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  EXCLUDED_WORKFLOWS,
  PACKAGE_JSON_PATH,
  REPO_ROOT,
  SETUP_DEV_PATH,
  checkActionSetupFollowsCheckout,
  checkPnpmPin,
  checkResolvedVersion,
  checkSetupDevDerivesPin,
  checkWorkflowPins,
  findActionSetupVersionInputs,
  readPin,
  readWorkflows,
  resolvePnpmVersion,
  splitJobs,
} from "./check-pnpm-pin.mjs";

describe("readPin", () => {
  it("accepts an exact pin", () => {
    const pin = readPin(JSON.stringify({ packageManager: "pnpm@10.34.5" }));
    assert.deepEqual(pin, { ok: true, version: "10.34.5" });
  });

  it("accepts corepack's +hash suffix, pinning on the version part", () => {
    const pin = readPin(JSON.stringify({ packageManager: "pnpm@10.34.5+sha512.abc" }));
    assert.deepEqual(pin, { ok: true, version: "10.34.5" });
  });

  it("rejects a missing field — the #2604 starting state", () => {
    const pin = readPin(JSON.stringify({ scripts: {} }));
    assert.equal(pin.ok, false);
    assert.match(pin.reason, /no "packageManager" field/);
  });

  // The planted mismatch for the pin itself: every range shape the old
  // floating-major setup allowed must come back red.
  for (const range of ["10", "10.x", "10.34.x", "^10.34.5", "~10.34.5", "*", "latest"]) {
    it(`rejects the range "${range}" — a range leaves the version ambient`, () => {
      const pin = readPin(JSON.stringify({ packageManager: `pnpm@${range}` }));
      assert.equal(pin.ok, false, `expected pnpm@${range} to be rejected`);
    });
  }

  it("rejects a non-pnpm package manager", () => {
    const pin = readPin(JSON.stringify({ packageManager: "yarn@4.0.0" }));
    assert.equal(pin.ok, false);
  });
});

describe("findActionSetupVersionInputs", () => {
  it("finds the version input inside a pnpm/action-setup step", () => {
    const workflow = [
      "jobs:",
      "  frontend:",
      "    steps:",
      "      - uses: actions/checkout@abc # v7.0.0",
      "      - uses: pnpm/action-setup@0ebf47 # v6.0.9",
      "        with:",
      "          version: 10",
      "      - uses: actions/setup-node@def # v6.4.0",
      "        with:",
      "          node-version: 22",
    ].join("\n");

    assert.deepEqual(findActionSetupVersionInputs(workflow), [{ line: 7, version: "10" }]);
  });

  it("does not attribute a later step's version input to action-setup", () => {
    const workflow = [
      "    steps:",
      "      - uses: pnpm/action-setup@0ebf47 # v6.0.9",
      "      - uses: jetli/wasm-pack-action@abc # v0.4.0",
      "        with:",
      "          version: v0.14.0",
    ].join("\n");

    assert.deepEqual(findActionSetupVersionInputs(workflow), []);
  });

  it("strips quoting and trailing comments", () => {
    const workflow = [
      "      - uses: pnpm/action-setup@0ebf47 # v6.0.9",
      "        with:",
      '          version: "10.34.5" # pinned',
    ].join("\n");

    assert.deepEqual(findActionSetupVersionInputs(workflow), [{ line: 3, version: "10.34.5" }]);
  });
});

describe("checkWorkflowPins", () => {
  const floating = [
    "      - uses: pnpm/action-setup@0ebf47 # v6.0.9",
    "        with:",
    "          version: 10",
  ].join("\n");

  const derived = "      - uses: pnpm/action-setup@0ebf47 # v6.0.9";

  it("is green when no lane passes a version input at all", () => {
    const result = checkWorkflowPins([{ name: "ci.yml", text: derived }], "10.34.5");
    assert.deepEqual(result, { ok: true, problems: [] });
  });

  // Planted mismatch: this is the exact state of main before this change.
  it("goes red on a floating `version: 10`", () => {
    const result = checkWorkflowPins([{ name: "ci.yml", text: floating }], "10.34.5");
    assert.equal(result.ok, false);
    assert.equal(result.problems.length, 1);
    assert.match(result.problems[0], /ci\.yml:3 passes "version: 10"/);
  });

  it("tolerates a version input that exactly equals the pin", () => {
    const exact = floating.replace("version: 10", "version: 10.34.5");
    assert.equal(checkWorkflowPins([{ name: "ci.yml", text: exact }], "10.34.5").ok, true);
  });

  it("never reports on release.yml — off limits by standing repo rule", () => {
    assert.ok(EXCLUDED_WORKFLOWS.has("release.yml"));
    assert.equal(checkWorkflowPins([{ name: "release.yml", text: floating }], "10.34.5").ok, true);
  });
});

describe("checkActionSetupFollowsCheckout", () => {
  const job = (steps) => ["jobs:", "  frontend:", "    steps:", ...steps].join("\n");
  const checkout = "      - uses: actions/checkout@abc # v7.0.0";
  const setup = "      - uses: pnpm/action-setup@0ebf47 # v6.0.9";

  it("is green when checkout precedes action-setup in the same job", () => {
    assert.deepEqual(checkActionSetupFollowsCheckout([{ name: "ci.yml", text: job([checkout, setup]) }]), {
      ok: true,
      problems: [],
    });
  });

  // Planted mismatch: without the `version:` input the action has nothing to
  // read until the repo is on disk.
  it("goes red when action-setup runs before checkout", () => {
    const result = checkActionSetupFollowsCheckout([{ name: "ci.yml", text: job([setup, checkout]) }]);
    assert.equal(result.ok, false);
    assert.match(result.problems[0], /job "frontend".*without a preceding actions\/checkout/s);
  });

  it("goes red when the job has no checkout at all", () => {
    assert.equal(checkActionSetupFollowsCheckout([{ name: "ci.yml", text: job([setup]) }]).ok, false);
  });

  it("does not credit a checkout from a DIFFERENT job", () => {
    const text = [
      "jobs:",
      "  build:",
      "    steps:",
      checkout,
      "  frontend:",
      "    steps:",
      setup,
    ].join("\n");
    assert.equal(checkActionSetupFollowsCheckout([{ name: "ci.yml", text }]).ok, false);
  });
});

describe("splitJobs", () => {
  it("skips comment lines shaped like job headers", () => {
    const text = ["jobs:", "  # a note ending in a colon:", "  frontend:", "    steps:"].join("\n");
    assert.deepEqual(
      splitJobs(text).map((j) => j.id),
      ["frontend"],
    );
  });
});

describe("checkSetupDevDerivesPin", () => {
  it("is green when the script reads packageManager and prepares a variable", () => {
    const script = [
      'PNPM_VERSION="$(node -p "require(\'./package.json\').packageManager")"',
      'corepack prepare "pnpm@${PNPM_VERSION}" --activate',
    ].join("\n");
    assert.deepEqual(checkSetupDevDerivesPin(script), { ok: true, problems: [] });
  });

  // Planted mismatch: main's `PNPM_MAJOR="10"` + `corepack prepare pnpm@10`.
  it("goes red on a hardcoded second pin", () => {
    const script = ['PNPM_MAJOR="10"', 'corepack prepare "pnpm@10" --activate'].join("\n");
    const result = checkSetupDevDerivesPin(script);
    assert.equal(result.ok, false);
    assert.ok(result.problems.some((p) => /never reads the "packageManager" field/.test(p)));
    assert.ok(result.problems.some((p) => /hardcodes "pnpm@10"/.test(p)));
  });
});

describe("checkResolvedVersion", () => {
  it("is green when the resolved version is the pinned version", () => {
    assert.deepEqual(checkResolvedVersion("10.34.5", "10.34.5"), { ok: true, problems: [] });
  });

  // Planted mismatch: the drift this issue exists to make visible.
  it("goes red — with a remediation — when they differ", () => {
    const result = checkResolvedVersion("10.20.0", "10.34.5");
    assert.equal(result.ok, false);
    assert.match(result.problems[0], /reports 10\.20\.0.*pins pnpm@10\.34\.5/s);
    assert.match(result.problems[0], /corepack prepare pnpm@10\.34\.5 --activate/);
  });
});

// Planted-drift proof for the "pnpm absent" vs "pnpm ran and failed" conflation
// this review finding raised: a stub `pnpm` on PATH that exits non-zero must
// be reported as `failed` (carrying the exit code and stderr), never folded
// into the same `missing` bucket a genuinely absent binary gets — collapsing
// them silently discarded the real cause (e.g. a corepack fetch failure) and
// let checkPnpmPin()/the test above SKIP GREEN instead of failing loud.
describe("resolvePnpmVersion", () => {
  const originalPath = process.env.PATH;
  const stubDirs = [];

  after(() => {
    process.env.PATH = originalPath;
    for (const dir of stubDirs) {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  function withStubOnPath(script, fn) {
    const dir = mkdtempSync(join(tmpdir(), "check-pnpm-pin-stub-"));
    stubDirs.push(dir);
    if (script !== null) {
      const stubPath = join(dir, "pnpm");
      writeFileSync(stubPath, script);
      chmodSync(stubPath, 0o755);
    }
    const previous = process.env.PATH;
    // Restrict PATH to the stub dir only, so a real pnpm elsewhere on this
    // machine's PATH cannot mask the case under test.
    process.env.PATH = dir;
    try {
      return fn();
    } finally {
      process.env.PATH = previous;
    }
  }

  it("reports failed — with the real exit code and stderr — for a pnpm that exists but exits non-zero", () => {
    const result = withStubOnPath(
      '#!/bin/sh\necho "corepack: cannot fetch pnpm@10.34.5 (offline)" >&2\nexit 1\n',
      () => resolvePnpmVersion(),
    );
    assert.equal(result.status, "failed");
    assert.equal(result.code, 1);
    assert.match(result.stderr, /cannot fetch pnpm@10\.34\.5 \(offline\)/);
  });

  it("reports missing only when no pnpm binary exists on PATH at all", () => {
    const result = withStubOnPath(null, () => resolvePnpmVersion());
    assert.deepEqual(result, { status: "missing" });
  });

  it("reports ok with the version for a pnpm that runs cleanly", () => {
    const result = withStubOnPath('#!/bin/sh\necho "10.34.5"\nexit 0\n', () => resolvePnpmVersion());
    assert.deepEqual(result, { status: "ok", version: "10.34.5" });
  });
});

// Planted-drift proof that checkPnpmPin() surfaces a `failed` resolution as a
// real problem quoting the captured stderr, rather than the softer "not on
// PATH" message reserved for a genuinely missing pnpm.
describe("checkPnpmPin() problem text for a failed (not missing) resolution", () => {
  it("quotes the stub's stderr and exit code instead of claiming pnpm is absent", () => {
    const originalPath = process.env.PATH;
    const dir = mkdtempSync(join(tmpdir(), "check-pnpm-pin-stub-"));
    const stubPath = join(dir, "pnpm");
    writeFileSync(stubPath, '#!/bin/sh\necho "corepack: cannot fetch pnpm@10.34.5 (offline)" >&2\nexit 1\n');
    chmodSync(stubPath, 0o755);
    process.env.PATH = dir;
    try {
      const result = checkPnpmPin({ checkResolved: true });
      assert.equal(result.ok, false);
      const problem = result.problems.find((p) => /ran but failed/.test(p));
      assert.ok(problem, `expected a "ran but failed" problem, got:\n${result.problems.join("\n")}`);
      assert.match(problem, /exit 1/);
      assert.match(problem, /cannot fetch pnpm@10\.34\.5 \(offline\)/);
      assert.ok(
        !result.problems.some((p) => /is not on PATH/.test(p)),
        "must not also report the missing-pnpm message when pnpm ran and failed",
      );
    } finally {
      process.env.PATH = originalPath;
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("the real repository", () => {
  const pin = readPin(readFileSync(join(REPO_ROOT, PACKAGE_JSON_PATH), "utf8"));

  it(`root ${PACKAGE_JSON_PATH} pins pnpm to an exact version`, () => {
    assert.equal(pin.ok, true, pin.ok ? "" : pin.reason);
  });

  it(`${SETUP_DEV_PATH} derives that version instead of carrying its own`, () => {
    const result = checkSetupDevDerivesPin(readFileSync(join(REPO_ROOT, SETUP_DEV_PATH), "utf8"));
    assert.equal(result.ok, true, result.problems.join("\n"));
  });

  it("no workflow lane passes a pnpm/action-setup version that disagrees with the pin", () => {
    assert.equal(pin.ok, true, "pin must resolve before workflows can be checked");
    const result = checkWorkflowPins(readWorkflows(), pin.version);
    assert.equal(result.ok, true, result.problems.join("\n"));
  });

  it("every pnpm/action-setup lane checks out the repo first", () => {
    const result = checkActionSetupFollowsCheckout(readWorkflows());
    assert.equal(result.ok, true, result.problems.join("\n"));
  });

  it("the pnpm actually on PATH is the pinned version", (t) => {
    const resolved = resolvePnpmVersion();
    if (resolved.status === "missing") {
      t.skip("pnpm is not on PATH in this environment");
      return;
    }
    assert.equal(
      resolved.status,
      "ok",
      resolved.status === "failed" ? `pnpm on PATH exited ${resolved.code}: ${resolved.stderr}` : "",
    );
    assert.equal(pin.ok, true, "pin must resolve before the running version can be compared");
    const result = checkResolvedVersion(resolved.version, pin.version);
    assert.equal(result.ok, true, result.problems.join("\n"));
  });

  it("checkPnpmPin() — the CLI's own entry point — is green end to end", () => {
    const result = checkPnpmPin({ checkResolved: resolvePnpmVersion().status !== "missing" });
    assert.equal(result.ok, true, result.problems.join("\n"));
  });
});
