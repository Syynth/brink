/**
 * Unit tests for `workspace-roots.ts` (issue #2515, hole 2). The integration
 * pin against the REAL `pnpm-workspace.yaml` lives in
 * `save-path-enrolment.test.ts` (where `discoverCallSiteFiles` actually
 * consumes this module) — this file exercises the parser/deriver in
 * isolation, including shapes the real workspace file does not currently
 * use, without touching any file outside `os.tmpdir()`.
 */

import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { deriveScanRoots, parseWorkspacePackageGlobs } from "./workspace-roots.js";

describe("parseWorkspacePackageGlobs (#2515)", () => {
  it("parses a single simple glob", () => {
    expect(parseWorkspacePackageGlobs('packages:\n  - "packages/*"\n')).toEqual(["packages/*"]);
  });

  it("parses multiple globs and stops at the next top-level key", () => {
    const yaml = 'packages:\n  - "packages/*"\n  - "apps/*"\nonlyBuiltDependencies:\n  - foo\n';
    expect(parseWorkspacePackageGlobs(yaml)).toEqual(["packages/*", "apps/*"]);
  });

  it("skips blank lines and comments inside the packages: block", () => {
    const yaml = 'packages:\n  # workspace roots\n  - "packages/*"\n\n  - "apps/*"\n';
    expect(parseWorkspacePackageGlobs(yaml)).toEqual(["packages/*", "apps/*"]);
  });

  it("accepts single-quoted and unquoted entries", () => {
    expect(parseWorkspacePackageGlobs("packages:\n  - 'packages/*'\n")).toEqual(["packages/*"]);
    expect(parseWorkspacePackageGlobs("packages:\n  - packages/*\n")).toEqual(["packages/*"]);
  });

  it("throws on a negated entry rather than silently dropping it", () => {
    // A negation is the exact shape a workspace author would reach for to
    // EXCLUDE a directory the naive glob-expansion below would otherwise
    // include — silently ignoring it would make deriveScanRoots over-scan,
    // not under-scan, but it is still a shape this parser cannot express,
    // so it must fail loudly rather than guess.
    expect(() =>
      parseWorkspacePackageGlobs('packages:\n  - "packages/*"\n  - "!packages/excluded"\n'),
    ).toThrow(/unsupported "packages:" entry/);
  });

  it("throws on an exact-path entry with no /* glob", () => {
    expect(() => parseWorkspacePackageGlobs('packages:\n  - "packages/fixed-name"\n')).toThrow(
      /unsupported "packages:" entry/,
    );
  });

  it("throws when there is no packages: key at all", () => {
    expect(() => parseWorkspacePackageGlobs("onlyBuiltDependencies:\n  - foo\n")).toThrow(
      /no top-level "packages:" key/,
    );
  });

  it("throws when packages: is present but empty", () => {
    expect(() => parseWorkspacePackageGlobs("packages:\nonlyBuiltDependencies:\n  - foo\n")).toThrow(
      /parsed to zero entries/,
    );
  });
});

describe("deriveScanRoots (#2515)", () => {
  const tmpDirs: string[] = [];

  afterEach(() => {
    for (const dir of tmpDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });

  function makeRepo(): string {
    const dir = mkdtempSync(join(tmpdir(), "brink-workspace-roots-"));
    tmpDirs.push(dir);
    return dir;
  }

  it("resolves a single glob to its package directories, sorted", () => {
    const repo = makeRepo();
    mkdirSync(join(repo, "packages", "b", "src"), { recursive: true });
    mkdirSync(join(repo, "packages", "a", "src"), { recursive: true });

    const roots = deriveScanRoots(["packages/*"], repo);
    expect(roots.map((root) => relative(repo, root))).toEqual([
      join("packages", "a"),
      join("packages", "b"),
    ]);
  });

  it("a glob whose directory does not exist contributes no roots (not a throw)", () => {
    const repo = makeRepo();
    expect(deriveScanRoots(["apps/*"], repo)).toEqual([]);
  });

  it("a non-directory entry under the glob's root is skipped", () => {
    const repo = makeRepo();
    mkdirSync(join(repo, "packages"), { recursive: true });
    // A stray file directly under packages/ (e.g. a README) — readdirSync
    // returns it, but it is not a package directory to scan into.
    mkdirSync(join(repo, "packages", "real-pkg"), { recursive: true });
    writeFileSync(join(repo, "packages", "README.md"), "not a package\n");

    const roots = deriveScanRoots(["packages/*"], repo);
    expect(roots.map((root) => relative(repo, root))).toEqual([join("packages", "real-pkg")]);
  });

  it("a SECOND workspace glob widens the scan roots — the actual #2515 mechanism", () => {
    // This is the scenario hole 2 describes: a workspace layout grows a
    // root outside packages/*. Proves the derivation actually WIDENS when
    // the workspace does, not merely that it is pinned to today's shape.
    const repo = makeRepo();
    mkdirSync(join(repo, "packages", "a", "src"), { recursive: true });
    mkdirSync(join(repo, "apps", "b", "src"), { recursive: true });

    const before = deriveScanRoots(parseWorkspacePackageGlobs('packages:\n  - "packages/*"\n'), repo);
    expect(before.map((root) => relative(repo, root))).toEqual([join("packages", "a")]);

    const after = deriveScanRoots(
      parseWorkspacePackageGlobs('packages:\n  - "packages/*"\n  - "apps/*"\n'),
      repo,
    );
    expect(after.map((root) => relative(repo, root)).sort()).toEqual([
      join("apps", "b"),
      join("packages", "a"),
    ]);
  });
});
