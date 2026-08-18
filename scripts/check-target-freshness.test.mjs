// Tests for scripts/check-target-freshness.mjs (#2054). Node's built-in
// test runner, matching this repo's other scripts/*.test.mjs — no real git
// worktrees or cargo cache are touched; `listWorktrees`/`repoRoot`/
// `targetDir` are all injected against scratch temp directories.

import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  unlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  checkTargetFreshness,
  classifyPackageStamps,
  classifyStamp,
  findAllStamps,
  findNewestArtifact,
  findNewestStamp,
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

// Mirrors what `build.rs` (#2759) actually writes: the plain, untrimmed
// `CARGO_MANIFEST_DIR` text under `<target>/<profile>/build/<pkg>-<hash>/out/`.
// `pkg` is cargo's own package name VERBATIM (hyphens, not underscores) —
// unlike the `deps/*.d` filenames `writeArtifact` mirrors, the `build/`
// directory name is never normalized. Confirmed against a real build.
function writeStamp(targetDir, pkg, hash, manifestDir, { profile = "debug", ageMs, content } = {}) {
  const outDir = join(targetDir, profile, "build", `${pkg}-${hash}`, "out");
  mkdirSync(outDir, { recursive: true });
  const file = join(outDir, "worktree-stamp.txt");
  writeFileSync(file, content !== undefined ? content : manifestDir);
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

  it("skips an entry that vanishes between readdirSync and statSync instead of throwing", () => {
    // Reproduces the concurrent-mutation race this tool is meant to be safe
    // under: a sibling worktree rebuilding, or `cargo clean -p` (this
    // tool's own remediation), removing a dep-info file after it was
    // listed but before it was stat'd. `stat` is injected so the race is
    // deterministic rather than relying on real timing.
    const targetDir = scratchDir("check-target-freshness-");
    const vanished = writeArtifact(targetDir, "brink_respell", "aaaaaaaaaaaaaaaa");
    const survivor = writeArtifact(targetDir, "brink_respell", "bbbbbbbbbbbbbbbb", { ageMs: 1_000 });

    const found = findNewestArtifact(targetDir, "brink-respell", {
      stat: (path) => {
        if (path === vanished) {
          const err = new Error(`ENOENT: no such file or directory, stat '${path}'`);
          err.code = "ENOENT";
          throw err;
        }
        return statSync(path);
      },
    });

    assert.ok(found);
    assert.equal(found.path, survivor);
  });

  it("re-throws a non-ENOENT stat error rather than silently skipping it", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const file = writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");

    assert.throws(
      () =>
        findNewestArtifact(targetDir, "brink-respell", {
          stat: (path) => {
            if (path === file) {
              const err = new Error("EACCES: permission denied");
              err.code = "EACCES";
              throw err;
            }
            return statSync(path);
          },
        }),
      /EACCES/,
    );
  });

  it("does not throw against a real vanished file (integration, no injected stat)", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const vanished = writeArtifact(targetDir, "brink_respell", "aaaaaaaaaaaaaaaa");
    unlinkSync(vanished);
    assert.equal(findNewestArtifact(targetDir, "brink-respell"), null);
  });
});

describe("findNewestStamp", () => {
  it("returns null when nothing has stamped this package", () => {
    const targetDir = scratchDir("check-target-freshness-");
    assert.equal(findNewestStamp(targetDir, "brink-respell"), null);
  });

  it("finds a stamp regardless of its -C metadata hash suffix", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const file = writeStamp(targetDir, "brink-respell", "0e4243ba15ba10cd", "/repo/crates/internal/brink-respell");
    const found = findNewestStamp(targetDir, "brink-respell");
    assert.ok(found);
    assert.equal(found.path, file);
    assert.equal(found.data, "/repo/crates/internal/brink-respell");
  });

  it("picks the most recently modified stamp when several hashes exist", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeStamp(targetDir, "brink-respell", "aaaaaaaaaaaaaaaa", "/worktree-a/crates/internal/brink-respell", {
      ageMs: 60_000,
    });
    writeStamp(targetDir, "brink-respell", "bbbbbbbbbbbbbbbb", "/worktree-b/crates/internal/brink-respell", {
      ageMs: 1_000,
    });
    const found = findNewestStamp(targetDir, "brink-respell");
    assert.equal(found.data, "/worktree-b/crates/internal/brink-respell");
  });

  it("does not match an unrelated package with a similar name", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeStamp(targetDir, "brink-respell-extra", "0e4243ba15ba10cd", "/repo/crates/internal/brink-respell-extra");
    assert.equal(findNewestStamp(targetDir, "brink-respell"), null);
  });

  it("trims trailing whitespace/newline from the stamp content", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeStamp(targetDir, "brink-respell", "0e4243ba15ba10cd", null, {
      content: "/repo/crates/internal/brink-respell\n",
    });
    const found = findNewestStamp(targetDir, "brink-respell");
    assert.equal(found.data, "/repo/crates/internal/brink-respell");
  });

  it("reports data: null for an empty stamp file (exists but unreadable-as-content)", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeStamp(targetDir, "brink-respell", "0e4243ba15ba10cd", null, { content: "" });
    const found = findNewestStamp(targetDir, "brink-respell");
    assert.ok(found, "the stamp file itself was found");
    assert.equal(found.data, null);
  });

  it("reports data: null when the stamp file cannot be read (injected error)", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const file = writeStamp(targetDir, "brink-respell", "0e4243ba15ba10cd", "/repo/crates/internal/brink-respell");
    const found = findNewestStamp(targetDir, "brink-respell", {
      readFile: (path) => {
        if (path === file) throw new Error("EACCES: permission denied");
        return readFileSync(path, "utf8");
      },
    });
    assert.ok(found);
    assert.equal(found.data, null);
  });

  it("skips a stamp path that vanishes between readdirSync and statSync instead of throwing", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeStamp(targetDir, "brink-respell", "aaaaaaaaaaaaaaaa", "/worktree-a/crates/internal/brink-respell");
    const survivor = writeStamp(
      targetDir,
      "brink-respell",
      "bbbbbbbbbbbbbbbb",
      "/worktree-b/crates/internal/brink-respell",
      { ageMs: 1_000 },
    );

    const found = findNewestStamp(targetDir, "brink-respell", {
      stat: (path) => {
        if (path.includes("aaaaaaaaaaaaaaaa")) {
          const err = new Error("ENOENT: no such file or directory");
          err.code = "ENOENT";
          throw err;
        }
        return statSync(path);
      },
    });

    assert.equal(found.path, survivor);
  });
});

describe("classifyStamp", () => {
  const repoRootAbs = "/home/user/brink/.claude/worktrees/this-one";
  const siblingAbs = "/home/user/brink/.claude/worktrees/other-one";
  const worktrees = [
    { path: repoRootAbs, locked: false },
    { path: siblingAbs, locked: true },
  ];

  it("is 'missing' when no stamp was found", () => {
    assert.deepEqual(classifyStamp(null, { repoRootAbs, worktrees }), { kind: "missing" });
  });

  it("is 'unreadable' when a stamp was found but its content is null", () => {
    const stamp = { path: "/x", mtimeMs: 0, data: null };
    assert.deepEqual(classifyStamp(stamp, { repoRootAbs, worktrees }), { kind: "unreadable" });
  });

  it("is 'self' when the stamp names this worktree", () => {
    const stamp = { path: "/x", mtimeMs: 0, data: `${repoRootAbs}/crates/internal/brink-respell` };
    const verdict = classifyStamp(stamp, { repoRootAbs, worktrees });
    assert.equal(verdict.kind, "self");
  });

  it("is 'live-sibling' when the stamp names a different, currently-live worktree", () => {
    const stamp = { path: "/x", mtimeMs: 0, data: `${siblingAbs}/crates/internal/brink-respell` };
    const verdict = classifyStamp(stamp, { repoRootAbs, worktrees });
    assert.equal(verdict.kind, "live-sibling");
    assert.equal(verdict.worktree.path, siblingAbs);
  });

  it("is 'dead-worktree' when the stamp names a worktree git no longer lists", () => {
    const stamp = { path: "/x", mtimeMs: 0, data: "/home/user/brink/.claude/worktrees/removed-long-ago/crates/internal/brink-respell" };
    const verdict = classifyStamp(stamp, { repoRootAbs, worktrees });
    assert.equal(verdict.kind, "dead-worktree");
  });

  it("never classifies a bare path-prefix collision as 'self' or 'live-sibling'", () => {
    // repoRootAbs + "-decoy" starts with repoRootAbs as a raw string but is
    // NOT nested under it as a path — this guards against a naive
    // `startsWith(repoRootAbs)` (no separator) false positive.
    const stamp = { path: "/x", mtimeMs: 0, data: `${repoRootAbs}-decoy/crates/internal/brink-respell` };
    const verdict = classifyStamp(stamp, { repoRootAbs, worktrees });
    assert.equal(verdict.kind, "dead-worktree");
  });

  describe("with a worktree nested inside another (production layout)", () => {
    // This repo's real worktree layout nests agent worktrees INSIDE the
    // main checkout (`/home/user/brink/.claude/worktrees/wf_*`), which is
    // exactly why `classifyStamp`'s longest-prefix loop exists: a
    // shortest-prefix (or first-match) resolution would classify every
    // self-built package as `live-sibling` (the main checkout "contains"
    // every nested worktree path too) and make this check always-red again
    // — the exact regression this file guards against. The two flat
    // siblings above never exercise that nesting; this block does.
    const mainAbs = "/home/user/brink";
    const aAbs = "/home/user/brink/.claude/worktrees/a";
    const bAbs = "/home/user/brink/.claude/worktrees/b";
    const nestedWorktrees = [
      { path: mainAbs, locked: false },
      { path: aAbs, locked: false },
      { path: bAbs, locked: false },
    ];

    it("is 'self' for a stamp under the nested worktree that IS repoRootAbs, not the enclosing main checkout", () => {
      const stamp = { path: "/x", mtimeMs: 0, data: `${bAbs}/crates/internal/brink-respell` };
      const verdict = classifyStamp(stamp, { repoRootAbs: bAbs, worktrees: nestedWorktrees });
      assert.equal(verdict.kind, "self");
    });

    it("is 'live-sibling' naming the nested worktree, not the enclosing main checkout, when repoRootAbs is a different nested worktree", () => {
      const stamp = { path: "/x", mtimeMs: 0, data: `${aAbs}/crates/internal/brink-respell` };
      const verdict = classifyStamp(stamp, { repoRootAbs: bAbs, worktrees: nestedWorktrees });
      assert.equal(verdict.kind, "live-sibling");
      assert.equal(verdict.worktree.path, aAbs);
    });
  });
});

describe("findAllStamps", () => {
  it("returns every matching hash-suffixed unit, not just the newest", () => {
    const targetDir = scratchDir("check-target-freshness-");
    writeStamp(targetDir, "brink-test-harness", "aaaaaaaaaaaaaaaa", "/worktree-a/crates/internal/brink-test-harness", {
      ageMs: 60_000,
    });
    writeStamp(targetDir, "brink-test-harness", "bbbbbbbbbbbbbbbb", "/worktree-b/crates/internal/brink-test-harness", {
      ageMs: 1_000,
    });
    const all = findAllStamps(targetDir, "brink-test-harness");
    assert.equal(all.length, 2);
    assert.deepEqual(
      all.map((s) => s.data).sort(),
      ["/worktree-a/crates/internal/brink-test-harness", "/worktree-b/crates/internal/brink-test-harness"],
    );
  });

  it("returns an empty array (not null) when nothing has stamped this package", () => {
    const targetDir = scratchDir("check-target-freshness-");
    assert.deepEqual(findAllStamps(targetDir, "brink-test-harness"), []);
  });
});

describe("classifyPackageStamps", () => {
  // Reproduces the exact false-positive scenario a reported finding named
  // against the original `findNewestStamp`-only design: worktree A runs
  // `cargo test -p brink-test-harness` (stamps unit 1, owned by A);
  // sibling worktree B later runs `cargo clippy -p brink-test-harness
  // --all-targets` under a DIFFERENT `-C metadata` hash (stamps unit 2,
  // owned by B, now the newer unit by mtime). A's own freshness check must
  // still report this package safe, because A's own unit proves A built it
  // itself — a different, merely-newer unit belonging to a sibling must not
  // outrank that.
  it("is 'self' when this worktree owns ANY unit, even if a live sibling's unit is newer", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const repoRootAbs = "/home/user/brink/.claude/worktrees/a";
    const siblingAbs = "/home/user/brink/.claude/worktrees/b";
    const worktrees = [
      { path: repoRootAbs, locked: false },
      { path: siblingAbs, locked: false },
    ];

    // A's own unit — older.
    writeStamp(
      targetDir,
      "brink-test-harness",
      "aaaaaaaaaaaaaaaa",
      join(repoRootAbs, "crates", "internal", "brink-test-harness"),
      { ageMs: 60_000 },
    );
    // Sibling B's unit — newer, DIFFERENT hash (simulating a different
    // cargo invocation, e.g. `--all-targets`).
    writeStamp(
      targetDir,
      "brink-test-harness",
      "bbbbbbbbbbbbbbbb",
      join(siblingAbs, "crates", "internal", "brink-test-harness"),
      { ageMs: 1_000 },
    );

    const verdict = classifyPackageStamps(targetDir, "brink-test-harness", { repoRootAbs, worktrees });
    assert.equal(verdict.kind, "self");
    assert.equal(verdict.units.length, 2);
    // Both units are still surfaced, each with its own verdict, so a
    // caller can render per-unit ownership rather than only the aggregate.
    const kinds = verdict.units.map((u) => u.verdict.kind).sort();
    assert.deepEqual(kinds, ["live-sibling", "self"]);
  });

  it("is 'live-sibling' when NO unit is self but at least one live sibling unit exists", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const repoRootAbs = "/home/user/brink/.claude/worktrees/a";
    const siblingAbs = "/home/user/brink/.claude/worktrees/b";
    const worktrees = [
      { path: repoRootAbs, locked: false },
      { path: siblingAbs, locked: false },
    ];

    writeStamp(
      targetDir,
      "brink-test-harness",
      "bbbbbbbbbbbbbbbb",
      join(siblingAbs, "crates", "internal", "brink-test-harness"),
    );

    const verdict = classifyPackageStamps(targetDir, "brink-test-harness", { repoRootAbs, worktrees });
    assert.equal(verdict.kind, "live-sibling");
    assert.equal(verdict.worktree.path, siblingAbs);
  });

  it("is 'missing' when no stamp units exist at all", () => {
    const targetDir = scratchDir("check-target-freshness-");
    const verdict = classifyPackageStamps(targetDir, "brink-test-harness", {
      repoRootAbs: "/home/user/brink",
      worktrees: [{ path: "/home/user/brink", locked: false }],
    });
    assert.equal(verdict.kind, "missing");
    assert.deepEqual(verdict.units, []);
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

  it("is safe when the target dir lives inside this worktree AND nothing has been built yet, even with a live sibling", () => {
    // Locality alone must never be the verdict (the false-negative this
    // guards): a worktree-local path does not rule out another live
    // worktree pointing its own CARGO_TARGET_DIR at the same absolute
    // directory (e.g. the BRINK-CONFIG.md shared cache at the main
    // checkout's own `<repo>/target`). This case is genuinely safe only
    // because no artifact exists yet, not because of the path shape.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    mkdirSync(targetDir, { recursive: true });

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: "/some/other/worktree", locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.shared, true);
  });

  it("is safe (unverified) — NOT risk — when a build artifact exists, a sibling is live, but no stamp names anyone (#2759)", () => {
    // This is the exact precondition (worktree-local target dir + live
    // sibling + existing artifact) that used to be the WHOLE verdict and
    // made the old heuristic a near-constant RED. Per #2759 that
    // precondition alone must no longer flip the result: with no stamp at
    // all for this package (predates the build.rs, or bypassed it), the
    // verdict is "missing" — unverified, not unsafe. Reintroducing red
    // here in a new shape is exactly the failure mode #2759 exists to
    // remove.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.shared, true);
    assert.equal(result.evidence[0].verdict.kind, "missing");
  });

  it("reports RISK when a package's stamp names a currently-live sibling, even with a worktree-local target dir", () => {
    // The real replacement for the test above: the same worktree-local +
    // live-sibling + existing-artifact shape, but now the package's own
    // build stamp names the live sibling as its last builder — a proven
    // collision, not a structural possibility.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");
    writeStamp(
      targetDir,
      "brink-respell",
      "0e4243ba15ba10cd",
      join(siblingPath, "crates", "internal", "brink-respell"),
    );

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, false);
    assert.equal(result.shared, true);
    assert.equal(result.evidence[0].verdict.kind, "live-sibling");
  });

  it("is safe when a package's stamp names this worktree itself", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");
    writeStamp(
      targetDir,
      "brink-respell",
      "0e4243ba15ba10cd",
      join(repoRoot, "crates", "internal", "brink-respell"),
    );

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.evidence[0].verdict.kind, "self");
  });

  it("is safe when a package's stamp names a worktree that git no longer lists as live (dead-worktree)", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    const removedWorktreePath = scratchDir("check-target-freshness-removed-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");
    // A worktree that built this once but is no longer registered with
    // git — `removedWorktreePath` is deliberately absent from
    // `listWorktrees`'s return value below.
    writeStamp(
      targetDir,
      "brink-respell",
      "0e4243ba15ba10cd",
      join(removedWorktreePath, "crates", "internal", "brink-respell"),
    );

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.evidence[0].verdict.kind, "dead-worktree");
  });

  it("flags RISK only for the package whose own stamp names a live sibling, not for a self-built or unverified sibling package", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");
    writeArtifact(targetDir, "brink_ir", "1a2b3c4d5e6f7089");
    writeArtifact(targetDir, "brink_runtime", "aabbccddeeff0011");
    // brink-respell: this worktree built it — safe.
    writeStamp(
      targetDir,
      "brink-respell",
      "0e4243ba15ba10cd",
      join(repoRoot, "crates", "internal", "brink-respell"),
    );
    // brink-ir: the live sibling built it — the one real collision.
    writeStamp(targetDir, "brink-ir", "1a2b3c4d5e6f7089", join(siblingPath, "crates", "internal", "brink-ir"));
    // brink-runtime: no stamp at all — unverified, not a risk.

    const warnings = [];
    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell", "brink-ir", "brink-runtime"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: (msg) => warnings.push(msg),
    });

    assert.equal(result.safe, false);
    const byPkg = Object.fromEntries(result.evidence.map((e) => [e.package, e.verdict.kind]));
    assert.deepEqual(byPkg, {
      "brink-respell": "self",
      "brink-ir": "live-sibling",
      "brink-runtime": "missing",
    });
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /- brink-ir: last built by/);
    assert.doesNotMatch(warnings[0], /- brink-respell: last built by/);
    assert.doesNotMatch(warnings[0], /- brink-runtime: last built by/);
  });

  it("is safe when the target dir is outside this worktree but no other worktree is currently live", () => {
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
    assert.equal(result.shared, false);
    assert.deepEqual(result.siblingWorktrees, []);
  });

  it("is safe when the target dir is shared with a live sibling but nothing has been built yet for any tracked package", () => {
    // The false-positive this guards: an existing-but-empty target dir
    // must not take the RISK path just because a sibling worktree is
    // live — its own evidence (every package null) says nothing has been
    // built, so there is nothing that could have been served stale.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = scratchDir("check-target-freshness-shared-target-");
    const siblingPath = scratchDir("check-target-freshness-sibling-");

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell", "brink-ir"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.shared, true);
    assert.deepEqual(
      result.evidence.map((e) => e.artifact),
      [null, null],
    );
  });

  it("is safe (unverified) — not risk — when an artifact exists and a sibling is live but that package has no stamp", () => {
    // Same shape as PR #2753's original heuristic (artifact + live sibling),
    // now with no stamp at all: #2759 says this must stay safe, since the
    // artifact may simply predate the build.rs.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = scratchDir("check-target-freshness-shared-target-");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: true },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.evidence[0].verdict.kind, "missing");
  });

  it("reports RISK — not safe — when a package's own stamp names a currently-live sibling worktree", () => {
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = scratchDir("check-target-freshness-shared-target-");
    const siblingPath = scratchDir("check-target-freshness-sibling-");
    writeArtifact(targetDir, "brink_respell", "0e4243ba15ba10cd");
    writeStamp(
      targetDir,
      "brink-respell",
      "0e4243ba15ba10cd",
      join(siblingPath, "crates", "internal", "brink-respell"),
    );

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
    assert.equal(result.evidence[0].verdict.kind, "live-sibling");
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

  it("is safe (not RISK) when this worktree's own build stamp is older than a sibling's for the same package (#2759 finding)", () => {
    // Full-pipeline reproduction of the classifyPackageStamps false-positive
    // scenario: this worktree built `brink-test-harness` first (unit 1),
    // then a live sibling ran a differently-invoked build of the same
    // package under a different hash (unit 2, newer mtime). The overall
    // result must stay safe for this package because THIS worktree's own
    // unit proves it built it itself.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");

    writeArtifact(targetDir, "brink_test_harness", "aaaaaaaaaaaaaaaa");
    writeStamp(
      targetDir,
      "brink-test-harness",
      "aaaaaaaaaaaaaaaa",
      join(repoRoot, "crates", "internal", "brink-test-harness"),
      { ageMs: 60_000 },
    );
    writeStamp(
      targetDir,
      "brink-test-harness",
      "bbbbbbbbbbbbbbbb",
      join(siblingPath, "crates", "internal", "brink-test-harness"),
      { ageMs: 1_000 },
    );

    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-test-harness"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: () => {},
    });

    assert.equal(result.safe, true);
    assert.equal(result.evidence[0].verdict.kind, "self");
  });

  it("does not report RISK for a package whose stamp names a live sibling but which never produced a cached artifact", () => {
    // Reported finding: `collisions` used to test only
    // `verdict.kind === "live-sibling"`, while the adjacent `unverified`
    // filter (and `formatEvidenceLine`'s early return) required an
    // artifact too — so a package whose build script ran (stamp present,
    // naming a live sibling) but whose compile never produced a `deps/*.d`
    // artifact flipped the WHOLE run RED while its own evidence line read
    // "no build artifact found yet", contradicting the "Confirmed
    // collisions" block above it. `collisions` must require `e.artifact`
    // just like `unverified` does.
    //
    // A second, unrelated package (with a real artifact) is included so
    // the run does not take the earlier "no artifact for ANY package"
    // early-return path — that path already reported safe before this fix,
    // so on its own it would not exercise the `collisions` filter at all.
    const repoRoot = scratchDir("check-target-freshness-repo-");
    const targetDir = join(repoRoot, "target");
    const siblingPath = scratchDir("check-target-freshness-sibling-");

    // brink-respell: a normal, unrelated, genuinely-safe package with a
    // real artifact, so the run does not short-circuit on "nothing built".
    writeArtifact(targetDir, "brink_respell", "aaaaaaaaaaaaaaaa");

    // brink-test-harness: build script ran and stamped a LIVE sibling as
    // owner, but the compile itself never produced a deps/*.d file.
    writeStamp(
      targetDir,
      "brink-test-harness",
      "cccccccccccccccc",
      join(siblingPath, "crates", "internal", "brink-test-harness"),
    );

    const warnings = [];
    const result = checkTargetFreshness({
      repoRoot,
      targetDir,
      packages: ["brink-respell", "brink-test-harness"],
      listWorktrees: () => [
        { path: repoRoot, locked: false },
        { path: siblingPath, locked: false },
      ],
      log: () => {},
      warn: (msg) => warnings.push(msg),
    });

    assert.equal(result.safe, true);
    assert.equal(warnings.length, 0);
    const harnessEvidence = result.evidence.find((e) => e.package === "brink-test-harness");
    assert.equal(harnessEvidence.artifact, null);
    assert.equal(harnessEvidence.verdict.kind, "live-sibling");
  });
});
