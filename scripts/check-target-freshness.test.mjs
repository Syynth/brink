// Tests for scripts/check-target-freshness.mjs (#2054). Node's built-in
// test runner, matching this repo's other scripts/*.test.mjs — no real git
// worktrees or cargo cache are touched; `listWorktrees`/`repoRoot`/
// `targetDir` are all injected against scratch temp directories.

import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  checkTargetFreshness,
  findNewestArtifact,
  listLiveWorktrees,
  DEFAULT_PACKAGES,
} from "./check-target-freshness.mjs";

const temporaries = [];

function scratchDir(prefix) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  temporaries.push(dir);
  return dir;
}

after(() => {
  for (const dir of temporaries) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function writeArtifact(targetDir, pkgUnderscored, hash, { profile = "debug", ageMs } = {}) {
  const depsDir = join(targetDir, profile, "deps");
  mkdirSync(depsDir, { recursive: true });
  const file = join(depsDir, `${pkgUnderscored}-${hash}.d`);
  writeFileSync(file, `${file}: stub\n`);
  if (ageMs !== undefined) {
    const t = new Date(Date.now() - ageMs);
    utimesSync(file, t, t);
  }
  return file;
}

describe("findNewestArtifact", () => {
  it("returns null when nothing has been built for this package", () => {
    const targetDir = scratchDir("check-target-freshness-");
    assert.equal(findNewestArtifact(targetDir, "brink-respell"), null);
  });

  it("finds a dep-info file regardless of its -C metadata hash suffix", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const file = writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");
    const found = findNewestArtifact(targetDir, "brink-respell");
    assert.ok(found);
    assert.equal(found.path, file);
  });

  it("picks the most recently modified match when several hashes exist", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeArtifact(targetDir, "brink_respell", "aaaaaaaaaaaaaaaa", { ageMs: 60_000 });
    const newer = writeArtifact(targetDir, "brink_respell", "bbbbbbbbbbbbbbbb", { ageMs: 1_000 });
    const found = findNewestArtifact(targetDir, "brink-respell");
    assert.equal(found.path, newer);
  });

  it("does not match an unrelated package with a similar name", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeArtifact(targetDir, "brink_respell_extra", "0e4243ba15ba10cd");
    assert.equal(findNewestArtifact(targetDir, "brink-respell"), null);
  });
});

describe("listLiveWorktrees", () => {
  it("parses `git worktree list --porcelain` output, including a locked entry", () => {
    const porcelain = [
      "worktree /home/user/brink",
      "HEAD 5091b6372035eb72fdf336cf7d51f75fa7e99458",
      "branch refs/heads/main",
      "",
      "worktree /home/user/brink/.claude/worktrees/wf_a",
      "HEAD 472c5185c98ae9c3c117d73acca966e2f1bcd2bd",
      "branch refs/heads/worktree-wf_a",
      "locked claude agent wf_a (pid 510 start 540)",
      "",
    ].join("\n");

    const worktrees = listLiveWorktrees({
      repoRoot: "/home/user/brink",
      exec: () => porcelain,
    });

    assert.deepEqual(worktrees, [
      { path: "/home/user/brink", locked: false },
      { path: "/home/user/brink/.claude/worktrees/wf_a", locked: true },
    ]);
  });
});

describe("checkTargetFreshness", () => {
  it("is safe when the target dir does not exist yet", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "..", "never-built-target");

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      listWorktrees: () => {
        throw new Error("must not be called when the target dir is absent");
      },
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.shared, false);
  });

  it("is safe when the target dir lives inside this worktree, regardless of siblings", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    mkdirSync(targetDir, { recursive: true });

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      listWorktrees: () => [{ path: "/some/other/worktree", locked: false }],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.shared, false);
  });

  it("is safe when the target dir is shared but no other worktree is currently live", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = scratchDir("check-target-freshness-shared-target-");

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      listWorktrees: () => [{ path: repoRoot, locked: false }],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.shared, true);
    assert.deepEqual(result.siblingWorktrees, []);
  });

  it("reports RISK — not safe — when the target dir is shared with another live worktree", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = scratchDir("check-target-freshness-shared-target-");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");

    const warnings = [];
    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: true },
      ],
      log: () => {},
      warn: (msg) => warnings.push(msg),
    });

    assert.equal(result.safe, false);
    assert.equal(result.shared, true);
    assert.deepEqual(
      result.siblingWorktrees.map((w) => w.path),
      [siblingPath],
    );
    assert.equal(result.evidence.length, 1);
    assert.ok(result.evidence[0].artifact, "should have found the stubbed artifact");
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /RISK/);
    assert.match(warnings[0], /cargo clean -p brink-respell/);
    assert.match(warnings[0], new RegExp(siblingPath.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
  });

  it("defaults to DEFAULT_PACKAGES when no override is given", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = scratchDir("check-target-freshness-shared-target-");
    const siblingPath = scratchDir("check-target-freshness-sibling-");

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.deepEqual(
      result.evidence.map((e) => e.package),
      DEFAULT_PACKAGES,
    );
  });
});
