// Tests for scripts/check-wasm-pkg.mjs (#2479). Node's built-in test
// runner: root has no vitest/other framework wired, and this is one small
// script, so `node --test scripts/` (or `pnpm test:scripts`) covers it
// without adding a root-level test dependency.

import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  checkWasmPkg,
  checkWasmPkgLink,
  REQUIRED_FILES,
  LINKED_FILES,
  BUILD_COMMAND,
} from "./check-wasm-pkg.mjs";

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

// A real `pnpm install` links a workspace package's `file:` dependency as a
// symlink chain: `<package>/node_modules/<dep-name>` -> pnpm's virtual
// store (`node_modules/.pnpm/<dep-name>@file+<encoded-path>/node_modules/<dep-name>`)
// -> the real target directory. Verified by direct reproduction (`pnpm
// install --frozen-lockfile` in a worktree, then `readlink` on
// `packages/wasm/node_modules/brink-web`). `buildResolvedLink` reproduces
// that exact two-hop shape so these tests exercise the same symlink-following
// behaviour `checkWasmPkgLink` relies on (`existsSync` follows symlinks by
// default), not just a plain directory standing in for it. `files` defaults
// to LINKED_FILES — a real healthy pnpm link never carries the full
// REQUIRED_FILES set (see LINKED_FILES's comment in check-wasm-pkg.mjs) — a
// caller opts into a different set (e.g. REQUIRED_FILES, or an empty list)
// explicitly.
function buildResolvedLink({ files = LINKED_FILES }) {
  const root = scratchPkgDir();
  const target = join(root, "target");
  mkdirSync(target, { recursive: true });
  for (const file of files) {
    writeFileSync(join(target, file), "stub");
  }

  const store = join(root, "node_modules", ".pnpm", "brink-web@file+stub", "node_modules");
  mkdirSync(store, { recursive: true });
  const storeLink = join(store, "brink-web");
  symlinkSync(target, storeLink, "dir");

  const consumerNodeModules = join(root, "consumer", "node_modules");
  mkdirSync(consumerNodeModules, { recursive: true });
  const linkDir = join(consumerNodeModules, "brink-web");
  symlinkSync(storeLink, linkDir, "dir");

  return linkDir;
}

describe("checkWasmPkgLink", () => {
  it("returns true when the resolved link (through pnpm's two-hop symlink chain) has every LINKED_FILES entry", () => {
    const linkDir = buildResolvedLink({ files: LINKED_FILES });

    const logs = [];
    const errors = [];
    const ok = checkWasmPkgLink({
      linkDir,
      log: (msg) => logs.push(msg),
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, true);
    assert.equal(errors.length, 0);
    assert.ok(logs.some((l) => l.includes("resolves to a complete wasm-pack output")));
  });

  it("returns true even without brink_web_bg.wasm.d.ts, matching a real healthy pnpm link", () => {
    // The exact shape a genuinely correct install produces (confirmed by
    // direct reproduction of `pnpm install --frozen-lockfile` against a
    // real `wasm-pack build` output): wasm-pack's own generated
    // package.json `"files"` field never lists `brink_web_bg.wasm.d.ts`,
    // so pnpm never links it, even when nothing is broken. Asserting
    // REQUIRED_FILES's full 4-file list here would make this check fail on
    // every correctly-ordered, correctly-built CI run — this test is what
    // would go red if `checkWasmPkgLink` regressed to using REQUIRED_FILES
    // instead of LINKED_FILES as its default.
    assert.ok(
      !LINKED_FILES.includes("brink_web_bg.wasm.d.ts"),
      "LINKED_FILES should exclude brink_web_bg.wasm.d.ts",
    );
    // Proves the filter actually removed something, not merely that the
    // removed name happens to be absent: `LINKED_FILES` is
    // `REQUIRED_FILES.filter((file) => file !== "brink_web_bg.wasm.d.ts")` —
    // a string-literal match. If `REQUIRED_FILES` is ever renamed (it is
    // already coupled elsewhere in this file to copy-wasm.mjs's `files`
    // list), the filter predicate stops matching anything, `LINKED_FILES`
    // silently becomes identical to `REQUIRED_FILES`, and `checkWasmPkgLink`
    // starts demanding a file pnpm never links — false-failing every
    // correctly-built CI run. The assertion above alone would stay green in
    // that broken state (the renamed entry is still "not in LINKED_FILES"
    // for the trivial reason nothing is), so this length check is what
    // actually catches it.
    assert.equal(
      LINKED_FILES.length,
      REQUIRED_FILES.length - 1,
      "LINKED_FILES should be REQUIRED_FILES with exactly one entry filtered out",
    );
    const linkDir = buildResolvedLink({ files: LINKED_FILES });

    const errors = [];
    const ok = checkWasmPkgLink({
      linkDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, true);
    assert.equal(errors.length, 0);
  });

  it("returns false and names the missing files when the resolved link's target directory is empty", () => {
    // This is the exact shape #2514 names: the wasm-pack output (cause) can
    // be present while the pnpm-resolved link (effect) is empty — a
    // silently-failed `file:` resolution, not a missing build.
    const linkDir = buildResolvedLink({ files: [] });

    const errors = [];
    const ok = checkWasmPkgLink({
      linkDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, false);
    assert.equal(errors.length, 1);
    for (const file of LINKED_FILES) {
      assert.ok(
        errors[0].includes(file),
        `expected the error message to name missing file ${file}`,
      );
    }
    assert.ok(
      !errors[0].includes("brink_web_bg.wasm.d.ts"),
      "brink_web_bg.wasm.d.ts is not in LINKED_FILES, so it should never be reported missing",
    );
  });

  it("returns false when the link path does not exist at all (pnpm never created it)", () => {
    const linkDir = join(scratchPkgDir(), "does-not-exist", "brink-web");

    const errors = [];
    const ok = checkWasmPkgLink({
      linkDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, false);
    for (const file of LINKED_FILES) {
      assert.ok(errors[0].includes(file));
    }
  });

  it("returns false when the symlink is dangling (target directory was removed after linking)", () => {
    const root = scratchPkgDir();
    const removedTarget = join(root, "removed-target");
    // Deliberately never created — the symlink below points at nothing.
    const linkDir = join(root, "brink-web");
    symlinkSync(removedTarget, linkDir, "dir");

    const errors = [];
    const ok = checkWasmPkgLink({
      linkDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.equal(ok, false);
  });

  it("names `pnpm install:checked` as the remediation, not a wasm-pack build command", () => {
    // checkWasmPkgLink's failure means the wasm-pack output already exists
    // (checkWasmPkg already covers that case with its own remediation) —
    // telling a developer to rebuild wasm here would misdiagnose a link
    // failure as a missing build. The remediation must also point at the
    // GUARDED entry point, not a bare `pnpm install --frozen-lockfile` whose
    // exit code this whole family of fixes (#2593) says not to trust.
    const linkDir = buildResolvedLink({ files: [] });

    const errors = [];
    checkWasmPkgLink({
      linkDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.ok(errors[0].includes("pnpm install:checked"));
    assert.ok(
      !errors[0].includes("pnpm install --frozen-lockfile"),
      "the remediation must not recommend the unguarded command (#2593)",
    );
    assert.ok(
      !errors[0].includes(BUILD_COMMAND),
      "checkWasmPkgLink's failure message should not tell the developer to rebuild wasm — " +
        "the resolved link, not the wasm-pack output, is what's missing",
    );
  });

  it("distinguishes the resolved-link path from the wasm-pack output path in its failure message", () => {
    // The two checks name different locations (packages/wasm/node_modules/
    // brink-web vs crates/brink-web/www/pkg) — a message that quoted the
    // wrong one would send a developer looking in the wrong place.
    const linkDir = buildResolvedLink({ files: [] });

    const errors = [];
    checkWasmPkgLink({
      linkDir,
      log: () => {},
      error: (msg) => errors.push(msg),
    });

    assert.ok(errors[0].includes("packages/wasm/node_modules/brink-web"));
    assert.ok(!errors[0].includes("crates/brink-web/www/pkg is missing"));
  });

  it("is independent of checkWasmPkg: a complete wasm-pack output does not imply a resolved link", () => {
    // Reproduces the exact #2514 scenario end to end: the cause check
    // (wasm-pack output present) passes while the effect check (pnpm's
    // resolved link) fails, proving the two are checking different things
    // rather than one being a subset of the other.
    const pkgDir = scratchPkgDir();
    writeAllRequiredFiles(pkgDir);
    const emptyLinkDir = buildResolvedLink({ files: [] });

    const pkgOk = checkWasmPkg({ pkgDir, log: () => {}, error: () => {} });
    const linkOk = checkWasmPkgLink({ linkDir: emptyLinkDir, log: () => {}, error: () => {} });

    assert.equal(pkgOk, true, "the wasm-pack output (cause) should be complete");
    assert.equal(linkOk, false, "the resolved pnpm link (effect) should still be reported as broken");
  });
});
