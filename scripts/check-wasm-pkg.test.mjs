// Tests for scripts/check-wasm-pkg.mjs (#2479). Node's built-in test
// runner: root has no vitest/other framework wired, and this is one small
// script, so `node --test scripts/` (or `pnpm test:scripts`) covers it
// without adding a root-level test dependency.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import { checkWasmPkg, REQUIRED_FILES, BUILD_COMMAND } from "./check-wasm-pkg.mjs";

const temporaries = [];

function scratchPkgDir() {
  const dir = mkdtempSync(join(tmpdir(), "check-wasm-pkg-"));
  temporaries.push(dir);
  return dir;
}

function writeAllRequiredFiles(pkgDir) {
  mkdirSync(pkgDir, { recursive: true });
  for (const file of REQUIRED_FILES) {
    writeFileSync(join(pkgDir, file), "stub");
  }
}

after(() => {
  for (const dir of temporaries) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("checkWasmPkg", () => {
  it("returns true and logs nothing to stderr when every required file is present", () => {
    const pkgDir = scratchPkgDir();
    writeAllRequiredFiles(pkgDir);

    const logs = [];
    const errors = [];
    const ok = checkWasmPkg({
      pkgDir,
      log: (msg) => logs.push(msg),
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, true);
    assert.equal(errors.length, 0);
    assert.ok(logs.some((l) => l.includes("is present")));
  });

  it("returns false and names the missing files when the pkg directory does not exist at all", () => {
    const pkgDir = join(scratchPkgDir(), "does-not-exist");

    const errors = [];
    const ok = checkWasmPkg({
      pkgDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, false);
    assert.equal(errors.length, 1);
    for (const file of REQUIRED_FILES) {
      assert.ok(
        errors[0].includes(file),
        `expected the error message to name missing file ${file}`,
      );
    }
  });

  it("returns false when the directory exists but is missing the wasm binary specifically", () => {
    // The exact partial-failure shape observed in reproduction: wasm-pack's
    // own copy can leave the .d.ts files behind while the large .wasm
    // binary is absent (or, here, a directory present but genuinely
    // incomplete) — REQUIRED_FILES must be checked individually, not just
    // "does the directory exist".
    const pkgDir = scratchPkgDir();
    mkdirSync(pkgDir, { recursive: true });
    writeFileSync(join(pkgDir, "brink_web.js"), "stub");
    writeFileSync(join(pkgDir, "brink_web.d.ts"), "stub");
    // brink_web_bg.wasm and brink_web_bg.wasm.d.ts deliberately omitted.

    const errors = [];
    const ok = checkWasmPkg({
      pkgDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, false);
    assert.ok(errors[0].includes("missing: brink_web_bg.wasm, brink_web_bg.wasm.d.ts)"));
    assert.ok(!errors[0].includes("missing: brink_web.js"));
  });

  it("names the exact remediation command in the failure message", () => {
    const pkgDir = join(scratchPkgDir(), "missing");

    const errors = [];
    checkWasmPkg({
      pkgDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.ok(errors[0].includes(BUILD_COMMAND));
    assert.ok(errors[0].includes("wasm-pack build crates/brink-web --target web --out-dir www/pkg"));
  });
});
