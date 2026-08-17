// Tests for scripts/check-no-nul-bytes.mjs (#2737). Node's built-in test
// runner, matching check-pnpm-pin.test.mjs / check-grammar-drift.test.mjs:
// this file runs under `pnpm test:scripts`, which CI's `frontend` job
// executes BEFORE `pnpm install`, so it must not depend on anything
// installed.
//
// Two halves:
//   1. Unit tests over the pure functions, driven with a PLANTED NUL byte in
//      a scratch fixture tree — proving the guard actually goes red, not
//      merely that it passes on a healthy tree (the "make it fail first"
//      discipline: a guard never seen red is not a guard).
//   2. An integration test over the REAL repo (`packages/*/src` as it
//      stands today), so `documentKey()`'s fix and any future regression are
//      both pinned by the same guard `pnpm test:scripts` already runs.

import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  REPO_ROOT,
  checkNoNulBytes,
  findFirstNulByte,
  listFilesRecursive,
  listPackages,
} from "./check-no-nul-bytes.mjs";

describe("findFirstNulByte", () => {
  it("reports absent on a clean buffer", () => {
    const result = findFirstNulByte(Buffer.from("hello world", "utf8"));
    assert.deepEqual(result, { hasNul: false, offset: -1 });
  });

  it("finds a NUL byte and its offset", () => {
    const buffer = Buffer.concat([
      Buffer.from("abc", "utf8"),
      Buffer.from([0x00]),
      Buffer.from("def", "utf8"),
    ]);
    const result = findFirstNulByte(buffer);
    assert.deepEqual(result, { hasNul: true, offset: 3 });
  });

  it("finds the FIRST NUL byte when there are several", () => {
    const buffer = Buffer.from([0x61, 0x00, 0x62, 0x00, 0x63]);
    const result = findFirstNulByte(buffer);
    assert.deepEqual(result, { hasNul: true, offset: 1 });
  });

  it("does not false-positive on a printable JSON separator", () => {
    // The #2733/#2733 fix shape: JSON.stringify of a fixed-arity tuple.
    // Nothing in the encoded form is a NUL byte.
    const key = JSON.stringify(["typeId", "docId"]);
    const result = findFirstNulByte(Buffer.from(key, "utf8"));
    assert.equal(result.hasNul, false);
  });
});

describe("checkNoNulBytes — planted-fixture proof (make it fail first)", () => {
  function withFixtureTree(build, run) {
    const tmp = mkdtempSync(join(tmpdir(), "check-no-nul-bytes-fixture-"));
    try {
      build(tmp);
      run(tmp);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  }

  it("is CLEAN (ok: true, no offenses) on a healthy fixture tree", () => {
    withFixtureTree(
      (root) => {
        const srcDir = join(root, "packages", "demo-pkg", "src");
        mkdirSync(srcDir, { recursive: true });
        writeFileSync(join(srcDir, "clean.ts"), 'export const key = `${a}:${b}`;\n', "utf8");
      },
      (root) => {
        const result = checkNoNulBytes({ repoRoot: root });
        assert.equal(result.ok, true);
        assert.deepEqual(result.offenses, []);
        assert.equal(result.filesScanned, 1);
      },
    );
  });

  it("goes RED and names the offending file + offset when a NUL byte is planted", () => {
    withFixtureTree(
      (root) => {
        const srcDir = join(root, "packages", "demo-pkg", "src");
        mkdirSync(srcDir, { recursive: true });
        writeFileSync(join(srcDir, "clean.ts"), "export const ok = 1;\n", "utf8");
        // Plant the exact defect shape: a template-literal NUL separator.
        const planted = Buffer.concat([
          Buffer.from("export function documentKey(a, b) {\n  return `${a}", "utf8"),
          Buffer.from([0x00]),
          Buffer.from("${b}`;\n}\n", "utf8"),
        ]);
        writeFileSync(join(srcDir, "tainted.ts"), planted);
      },
      (root) => {
        const result = checkNoNulBytes({ repoRoot: root });
        assert.equal(result.ok, false);
        assert.equal(result.offenses.length, 1);
        assert.match(result.offenses[0].path, /tainted\.ts$/);
        assert.equal(result.offenses[0].offset, 50);
        assert.equal(result.filesScanned, 2);
      },
    );
  });

  it("scans multiple packages, not just the first found", () => {
    withFixtureTree(
      (root) => {
        const srcA = join(root, "packages", "pkg-a", "src");
        const srcB = join(root, "packages", "pkg-b", "src");
        mkdirSync(srcA, { recursive: true });
        mkdirSync(srcB, { recursive: true });
        writeFileSync(join(srcA, "clean.ts"), "export const a = 1;\n", "utf8");
        writeFileSync(
          join(srcB, "tainted.ts"),
          Buffer.concat([Buffer.from("const k = `x", "utf8"), Buffer.from([0x00])]),
        );
      },
      (root) => {
        const result = checkNoNulBytes({ repoRoot: root });
        assert.equal(result.ok, false);
        assert.equal(result.offenses.length, 1);
        assert.match(result.offenses[0].path, /pkg-b[\\/]src[\\/]tainted\.ts$/);
      },
    );
  });

  it("ignores a package with no src/ directory rather than erroring", () => {
    withFixtureTree(
      (root) => {
        mkdirSync(join(root, "packages", "no-src-pkg"), { recursive: true });
      },
      (root) => {
        const result = checkNoNulBytes({ repoRoot: root });
        assert.equal(result.ok, true);
        assert.equal(result.filesScanned, 0);
      },
    );
  });
});

describe("listPackages / listFilesRecursive", () => {
  it("listPackages skips dotfiles and non-directories", () => {
    const tmp = mkdtempSync(join(tmpdir(), "list-packages-"));
    try {
      mkdirSync(join(tmp, "real-pkg"));
      mkdirSync(join(tmp, ".hidden"));
      writeFileSync(join(tmp, "README.md"), "not a directory\n", "utf8");
      assert.deepEqual(listPackages(tmp), ["real-pkg"]);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });

  it("listPackages returns [] for a missing directory rather than throwing", () => {
    assert.deepEqual(listPackages(join(tmpdir(), "definitely-does-not-exist-xyz")), []);
  });

  it("listFilesRecursive walks nested directories", () => {
    const tmp = mkdtempSync(join(tmpdir(), "list-files-"));
    try {
      mkdirSync(join(tmp, "nested", "deeper"), { recursive: true });
      writeFileSync(join(tmp, "top.ts"), "", "utf8");
      writeFileSync(join(tmp, "nested", "mid.ts"), "", "utf8");
      writeFileSync(join(tmp, "nested", "deeper", "leaf.ts"), "", "utf8");
      const files = listFilesRecursive(tmp).map((f) => f.slice(tmp.length + 1));
      assert.deepEqual(files.sort(), [
        "nested/deeper/leaf.ts",
        "nested/mid.ts",
        "top.ts",
      ]);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  });
});

describe("checkNoNulBytes — real repo (packages/*/src today)", () => {
  it("resolves REPO_ROOT to a directory that actually has a packages/ dir", () => {
    assert.deepEqual(listPackages(join(REPO_ROOT, "packages")).length > 0, true);
  });

  it("no file under packages/*/src contains a literal NUL byte", () => {
    const result = checkNoNulBytes({ repoRoot: REPO_ROOT });
    assert.deepEqual(
      result.offenses.map((o) => `${o.path}@${o.offset}`),
      [],
      `NUL-byte cache-key defect found (#2737 class) in: ${result.offenses
        .map((o) => `${o.path} (offset ${o.offset})`)
        .join(", ")}`,
    );
    assert.equal(result.ok, true);
    // Sanity: this must actually have scanned real files, not silently
    // walked an empty tree because of a path typo.
    assert.equal(result.filesScanned > 0, true);
  });
});
