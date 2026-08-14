// Tests for scripts/check-wasm-pkg.mjs (#2479). Node's built-in test
// runner: root has no vitest/other framework wired, and this is one small
// script, so `node --test scripts/` (or `pnpm test:scripts`) covers it
// without adding a root-level test dependency.

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import { checkWasmPkg, REQUIRED_FILES, BUILD_COMMAND } from "./check-wasm-pkg.mjs";

const here = dirname(fileURLToPath(import.meta.url));

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

  it("names the actual `brink-web` devDependency, not the published package name, in the failure message", () => {
    const pkgDir = join(scratchPkgDir(), "missing");

    const errors = [];
    checkWasmPkg({
      pkgDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    // Assert the exact phrasing, not a bare `includes("brink-web")` — that
    // substring also occurs inside BUILD_COMMAND's `crates/brink-web` path
    // and would pass whether or not the devDependency is actually named.
    assert.ok(
      errors[0].includes("devDependency named `brink-web`"),
      "expected the failure message to name the real devDependency key (`brink-web`, packages/wasm/package.json), not just the published package name",
    );
  });

  it("stays in sync with packages/wasm/scripts/copy-wasm.mjs's `files` list", () => {
    // REQUIRED_FILES carries a "keep in sync" comment pointing at
    // copy-wasm.mjs; nothing else enforces it. copy-wasm.mjs can't be
    // imported for its `files` array (it performs the copy at top-level
    // await on import), so read its source and pull the list out directly.
    const copyWasmPath = resolve(here, "../packages/wasm/scripts/copy-wasm.mjs");
    const copyWasmSource = readFileSync(copyWasmPath, "utf8");

    const match = copyWasmSource.match(/const files = \[([\s\S]*?)\];/);
    assert.ok(match, "expected to find copy-wasm.mjs's `const files = [...]` list");

    const copyWasmFiles = [...match[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    assert.ok(copyWasmFiles.length > 0, "expected to parse at least one file out of copy-wasm.mjs");

    // Symmetric check: every REQUIRED_FILES entry must be something
    // copy-wasm.mjs actually copies, AND copy-wasm.mjs must not copy
    // anything REQUIRED_FILES doesn't also check for — either direction of
    // drift would leave one side silently stale.
    for (const file of REQUIRED_FILES) {
      assert.ok(
        copyWasmFiles.includes(file),
        `REQUIRED_FILES has "${file}" but copy-wasm.mjs's files list does not`,
      );
    }
    for (const file of copyWasmFiles) {
      assert.ok(
        REQUIRED_FILES.includes(file),
        `copy-wasm.mjs's files list has "${file}" but REQUIRED_FILES does not — check-wasm-pkg would miss it`,
      );
    }
  });
});
