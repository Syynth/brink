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

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  EXCLUDED_WORKFLOWS,
  PACKAGE_JSON_PATH,
  REPO_ROOT,
  SETUP_DEV_PATH,
  checkPnpmPin,
  checkResolvedVersion,
  checkSetupDevDerivesPin,
  checkWorkflowPins,
  findActionSetupVersionInputs,
  readPin,
  readWorkflows,
  resolvePnpmVersion,
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

  it("the pnpm actually on PATH is the pinned version", (t) => {
    const resolved = resolvePnpmVersion();
    if (resolved === null) {
      t.skip("pnpm is not on PATH in this environment");
      return;
    }
    assert.equal(pin.ok, true, "pin must resolve before the running version can be compared");
    const result = checkResolvedVersion(resolved, pin.version);
    assert.equal(result.ok, true, result.problems.join("\n"));
  });

  it("checkPnpmPin() — the CLI's own entry point — is green end to end", () => {
    const result = checkPnpmPin({ checkResolved: resolvePnpmVersion() !== null });
    assert.equal(result.ok, true, result.problems.join("\n"));
  });
});
