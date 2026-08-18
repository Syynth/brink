// Tests for scripts/check-studio-changeset.mjs (#2820). Node's built-in test
// runner, matching check-pnpm-pin.test.mjs / check-wasm-pkg.test.mjs: this
// file runs under `pnpm test:scripts`, which CI's `frontend` job executes
// BEFORE `pnpm install`, so it must not depend on anything installed.
//
// All tests below drive the PURE functions with synthetic file lists and
// changeset text — no git, no filesystem — so the guard's decision logic is
// proven RED (and then green) independent of any CI wiring. `gatherDiff`
// (the git-backed I/O half) is exercised only implicitly by the CLI; it is
// intentionally not unit-tested here since it requires a real git history
// shape (see its doc comment) that a synthetic test cannot honestly fake.

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  CHANGESET_README,
  DEFAULT_BASE_REF,
  GUARDED_BUNDLE_FILES,
  GUARDED_PREFIXES,
  PACKAGE_JSON_BUNDLE_FILES,
  STUDIO_PACKAGE,
  changesetNamesStudio,
  checkStudioChangeset,
  isChangesetPath,
  isGuardedPath,
  isTestOnlyPath,
  packageJsonChangeIsIgnorable,
  parseNameStatus,
  resolveBaseRef,
} from "./check-studio-changeset.mjs";

describe("isGuardedPath", () => {
  for (const prefix of GUARDED_PREFIXES) {
    it(`matches a file under ${prefix}`, () => {
      assert.equal(isGuardedPath(`${prefix}foo/bar.ts`), true);
    });
  }

  for (const file of GUARDED_BUNDLE_FILES) {
    it(`matches the bundle-shaping file ${file} (#2834 item 1)`, () => {
      assert.equal(isGuardedPath(file), true);
    });
  }

  it("does not match the published packages/ink-editor (its own @brink-lang/editor changeset applies)", () => {
    assert.equal(isGuardedPath("packages/ink-editor/src/foo.ts"), false);
  });

  it("does not match the published packages/wasm (@brink-lang/web)", () => {
    assert.equal(isGuardedPath("packages/wasm/src/foo.ts"), false);
  });

  it("does not match a non-allowlisted file in a guarded package's root", () => {
    assert.equal(isGuardedPath("packages/studio-shell/README.md"), false);
  });

  it("does not match package.json for a guarded package NOT in the bundle-files allowlist", () => {
    assert.equal(isGuardedPath("packages/studio-ui/package.json"), false);
  });

  it("does not match a Rust crate", () => {
    assert.equal(isGuardedPath("crates/brink-web/src/lib.rs"), false);
  });
});

describe("isTestOnlyPath", () => {
  it("treats any file under a __tests__/ directory as test-only", () => {
    assert.equal(isTestOnlyPath("packages/brink-studio/src/__tests__/binder-tree.test.ts"), true);
    assert.equal(isTestOnlyPath("packages/brink-studio/src/__tests__/fixtures/helper.ts"), true);
  });

  it("treats a .test.ts(x)/.spec.ts(x) suffix as test-only wherever it lives", () => {
    assert.equal(isTestOnlyPath("packages/studio-ui/src/widgets/button.test.tsx"), true);
    assert.equal(isTestOnlyPath("packages/studio-ui/src/widgets/button.spec.ts"), true);
  });

  it("does not treat an ordinary source file as test-only", () => {
    assert.equal(isTestOnlyPath("packages/brink-studio/src/app.tsx"), false);
  });

  it("does not false-positive on a file merely mentioning 'test' in its name", () => {
    assert.equal(isTestOnlyPath("packages/studio-ui/src/latest-widget.ts"), false);
  });

  it("treats any file under a __mocks__/ directory as test-only — commit f88a3a7b's shape", () => {
    assert.equal(isTestOnlyPath("packages/brink-studio/src/__mocks__/brink-web.ts"), true);
  });

  it("treats any file under a __fixtures__/ directory as test-only", () => {
    assert.equal(isTestOnlyPath("packages/studio-ui/src/__fixtures__/sample-project.ts"), true);
  });
});

describe("isChangesetPath", () => {
  it("accepts a real changeset file", () => {
    assert.equal(isChangesetPath(".changeset/issue-2820-studio-changeset-guard.md"), true);
  });

  it("rejects the standing README", () => {
    assert.equal(isChangesetPath(CHANGESET_README), false);
  });

  it("rejects config.json", () => {
    assert.equal(isChangesetPath(".changeset/config.json"), false);
  });

  it("rejects a non-.changeset path", () => {
    assert.equal(isChangesetPath("docs/some.md"), false);
  });
});

describe("changesetNamesStudio", () => {
  it("matches the real changeset frontmatter shape", () => {
    assert.equal(changesetNamesStudio('---\n"@brink-lang/studio": patch\n---\n\nSummary.\n'), true);
  });

  it("matches when studio is one of several named packages", () => {
    assert.equal(
      changesetNamesStudio('---\n"@brink-lang/web": patch\n"@brink-lang/studio": patch\n---\n\nSummary.\n'),
      true,
    );
  });

  it("does not match a changeset naming only @brink-lang/web", () => {
    assert.equal(changesetNamesStudio('---\n"@brink-lang/web": patch\n---\n\nSummary.\n'), false);
  });

  it("does not match an empty/placeholder changeset", () => {
    assert.equal(changesetNamesStudio("---\n---\n"), false);
  });

  it("does not false-positive on studio mentioned only in prose", () => {
    assert.equal(
      changesetNamesStudio('---\n"@brink-lang/web": patch\n---\n\nAlso touches @brink-lang/studio internals.\n'),
      false,
    );
  });

  it("matches a single-quoted frontmatter key — valid YAML @changesets/cli reads fine", () => {
    assert.equal(changesetNamesStudio("---\n'@brink-lang/studio': patch\n---\n\nSummary.\n"), true);
  });

  it("matches an indented double-quoted frontmatter key — valid YAML @changesets/cli reads fine", () => {
    assert.equal(changesetNamesStudio('---\n  "@brink-lang/studio": patch\n---\n\nSummary.\n'), true);
  });

  it("matches with a CRLF line ending", () => {
    assert.equal(changesetNamesStudio('---\r\n"@brink-lang/studio": patch\r\n---\r\n\r\nSummary.\r\n'), true);
  });

  it("matches a major bump, not just patch", () => {
    assert.equal(changesetNamesStudio('---\n"@brink-lang/studio": major\n---\n\nSummary.\n'), true);
  });
});

describe("parseNameStatus", () => {
  it("parses added/modified/deleted lines", () => {
    const raw = "A\t.changeset/foo.md\nM\tpackages/brink-studio/src/app.tsx\nD\tpackages/wasm/src/old.ts\n";
    assert.deepEqual(parseNameStatus(raw), [
      { status: "A", path: ".changeset/foo.md" },
      { status: "M", path: "packages/brink-studio/src/app.tsx" },
      { status: "D", path: "packages/wasm/src/old.ts" },
    ]);
  });

  it("takes the new path for a rename line", () => {
    const raw = "R100\tpackages/brink-studio/src/old.ts\tpackages/brink-studio/src/new.ts\n";
    assert.deepEqual(parseNameStatus(raw), [
      { status: "R", path: "packages/brink-studio/src/new.ts" },
    ]);
  });

  it("ignores blank lines", () => {
    assert.deepEqual(parseNameStatus("\n\nA\t.changeset/foo.md\n\n"), [{ status: "A", path: ".changeset/foo.md" }]);
  });

  it("handles empty input", () => {
    assert.deepEqual(parseNameStatus(""), []);
  });
});

describe("resolveBaseRef", () => {
  it("falls back to main when GITHUB_BASE_REF is unset", () => {
    const saved = process.env.GITHUB_BASE_REF;
    delete process.env.GITHUB_BASE_REF;
    try {
      assert.equal(resolveBaseRef(), DEFAULT_BASE_REF);
    } finally {
      if (saved !== undefined) process.env.GITHUB_BASE_REF = saved;
    }
  });

  it("uses GITHUB_BASE_REF when GitHub Actions sets it for a pull_request event", () => {
    const saved = process.env.GITHUB_BASE_REF;
    process.env.GITHUB_BASE_REF = "release/1.0";
    try {
      assert.equal(resolveBaseRef(), "release/1.0");
    } finally {
      if (saved === undefined) delete process.env.GITHUB_BASE_REF;
      else process.env.GITHUB_BASE_REF = saved;
    }
  });
});

// The decision proper. Each of #2820's three real recurrences is
// reproduced as a named case below (the "RED" proof the issue asks for):
// a diff shaped exactly like that PR, with no @brink-lang/studio changeset,
// must fail.
describe("checkStudioChangeset", () => {
  it("passes when no guarded package is touched at all", () => {
    const result = checkStudioChangeset({
      changedFiles: ["crates/brink-web/src/lib.rs", "docs/publishing.md"],
      relevantChangesets: [],
    });
    assert.equal(result.ok, true);
    assert.deepEqual(result.problems, []);
  });

  it("passes when a guarded package is touched and a @brink-lang/studio changeset is added", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-shell/src/panel.tsx"],
      relevantChangesets: [{ path: ".changeset/foo.md", text: '---\n"@brink-lang/studio": patch\n---\n\nx\n' }],
    });
    assert.equal(result.ok, true);
    assert.deepEqual(result.problems, []);
  });

  it("passes when every guarded-package file touched is test-only (no changeset forced)", () => {
    const result = checkStudioChangeset({
      changedFiles: [
        "packages/brink-studio/src/__tests__/binder-tree.test.ts",
        "packages/studio-ui/src/widgets/button.spec.tsx",
      ],
      relevantChangesets: [],
    });
    assert.equal(result.ok, true, "a test-only diff must not be forced into an empty changeset");
    assert.deepEqual(result.problems, []);
  });

  it("passes for a __mocks__ + __tests__ diff with no changeset — commit f88a3a7b's shape", () => {
    const result = checkStudioChangeset({
      changedFiles: [
        "packages/brink-studio/src/__mocks__/brink-web.ts",
        "packages/brink-studio/src/__tests__/fold-kinds.test.ts",
      ],
      relevantChangesets: [],
    });
    assert.equal(result.ok, true, "a __mocks__-only diff must not be forced into an empty changeset");
    assert.deepEqual(result.problems, []);
  });

  it("fails when a guarded package is touched and no changeset is added at all — PR #2787's shape", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/brink-studio/src/binder/reveal.tsx", "packages/studio-store/src/slices/conflict.ts"],
      relevantChangesets: [],
    });
    assert.equal(result.ok, false);
    assert.equal(result.problems.length, 1);
    assert.match(result.problems[0], /binder\/reveal\.tsx/);
    assert.match(result.problems[0], /slices\/conflict\.ts/);
    assert.match(result.problems[0], new RegExp(STUDIO_PACKAGE.replace(/[/]/g, "\\/")));
  });

  it("fails when a changeset is added but names only @brink-lang/web — the wrong-rule reasoning, PR #2817's shape", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-shell/src/toolbar.tsx"],
      relevantChangesets: [{ path: ".changeset/foo.md", text: '---\n"@brink-lang/web": patch\n---\n\nx\n' }],
    });
    assert.equal(result.ok, false);
    assert.match(result.problems[0], /wasm-observable/);
  });

  it("fails when the added changeset is empty/placeholder (no package named)", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/ink-operations/src/splice.ts"],
      relevantChangesets: [{ path: ".changeset/foo.md", text: "---\n---\n" }],
    });
    assert.equal(result.ok, false);
  });

  it("mixed diff: passes as long as ONE added changeset names studio, even alongside unrelated ones", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-ui/src/list.tsx"],
      relevantChangesets: [
        { path: ".changeset/a.md", text: '---\n"@brink-lang/web": patch\n---\n\nx\n' },
        { path: ".changeset/b.md", text: '---\n"@brink-lang/studio": patch\n---\n\ny\n' },
      ],
    });
    assert.equal(result.ok, true);
  });

  it("mixed diff: a guarded non-test file alongside test-only ones still requires a changeset", () => {
    const result = checkStudioChangeset({
      changedFiles: [
        "packages/brink-studio/src/__tests__/binder-tree.test.ts",
        "packages/brink-studio/src/binder/reveal.tsx",
      ],
      relevantChangesets: [],
    });
    assert.equal(result.ok, false);
    assert.match(result.problems[0], /binder\/reveal\.tsx/);
    assert.doesNotMatch(result.problems[0], /binder-tree\.test\.ts/);
  });

  it("does not require a changeset for a change to the published packages/ink-editor", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/ink-editor/src/rename.ts"],
      relevantChangesets: [],
    });
    assert.equal(result.ok, true);
  });

  it("still fails a guarded change when an unrelated MODIFIED changeset carries no studio key", () => {
    // Models gatherDiff surfacing a MODIFIED changeset (#2834 item 2) whose
    // current text doesn't name studio — the guard must still fail, same as
    // if that changeset weren't in the diff at all.
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-store/src/store.ts"],
      relevantChangesets: [{ path: ".changeset/unrelated.md", text: '---\n"@brink-lang/web": patch\n---\n\nx\n' }],
    });
    assert.equal(result.ok, false);
  });

  // #2834 item 2: gatherDiff now surfaces both ADDED and MODIFIED
  // changesets (identically, as `relevantChangesets`), so this proves both
  // shapes satisfy the guard through the same decision path — checkStudioChangeset
  // itself does not distinguish "add" from "edit"; that distinction lives
  // only in which git status gatherDiff collected the file under.
  it("ADD shape: passes when a NEW changeset naming studio is added alongside the guarded change", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-store/src/store.ts"],
      relevantChangesets: [{ path: ".changeset/new-file.md", text: '---\n"@brink-lang/studio": patch\n---\n\nx\n' }],
    });
    assert.equal(result.ok, true);
  });

  it("EDIT shape: passes when an EXISTING changeset is edited to add the studio key", () => {
    // Reproduces the #2834 false positive: a PR satisfies the rule by
    // editing a changeset that already existed on main (so it's an M, not
    // an A) to add the @brink-lang/studio key. gatherDiff collects this as
    // a relevantChangesets entry the same way it does an A — this test
    // proves the decision function accepts it.
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-store/src/store.ts"],
      relevantChangesets: [
        { path: ".changeset/existing.md", text: '---\n"@brink-lang/web": patch\n"@brink-lang/studio": patch\n---\n\nx\n' },
      ],
    });
    assert.equal(result.ok, true);
  });

  // #2834 item 1: bundle-shaping non-src files.
  for (const file of GUARDED_BUNDLE_FILES) {
    it(`fails when only ${file} is touched with no changeset`, () => {
      const result = checkStudioChangeset({
        changedFiles: [file],
        relevantChangesets: [],
      });
      assert.equal(result.ok, false);
      assert.match(result.problems[0], new RegExp(file.replace(/[/.]/g, "\\$&")));
    });
  }

  it("passes when a bundle-shaping config file is touched and a studio changeset is added", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/brink-studio/tsup.config.ts"],
      relevantChangesets: [{ path: ".changeset/foo.md", text: '---\n"@brink-lang/studio": patch\n---\n\nx\n' }],
    });
    assert.equal(result.ok, true);
  });

  // #2834 item 1: the package.json devDependencies/version carve-out.
  it("does not require a changeset for a package.json edit that only touches devDependencies", () => {
    const oldText = JSON.stringify({ name: "@brink-lang/studio", version: "1.0.0", devDependencies: { a: "1.0.0" } });
    const newText = JSON.stringify({ name: "@brink-lang/studio", version: "1.0.0", devDependencies: { a: "1.1.0" } });
    const result = checkStudioChangeset({
      changedFiles: ["packages/brink-studio/package.json"],
      relevantChangesets: [],
      packageJsonDiffs: [{ path: "packages/brink-studio/package.json", oldText, newText }],
    });
    assert.equal(result.ok, true, "a devDependencies-only edit alters nothing published");
  });

  it("does not require a changeset for a package.json edit that only bumps version", () => {
    const oldText = JSON.stringify({ name: "@brink-lang/studio", version: "1.0.0" });
    const newText = JSON.stringify({ name: "@brink-lang/studio", version: "1.0.1" });
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-shell/package.json"],
      relevantChangesets: [],
      packageJsonDiffs: [{ path: "packages/studio-shell/package.json", oldText, newText }],
    });
    assert.equal(result.ok, true, "a version-only bump alters nothing published");
  });

  it("DOES require a changeset for a package.json edit touching dependencies", () => {
    const oldText = JSON.stringify({ name: "@brink-lang/studio", dependencies: { a: "1.0.0" } });
    const newText = JSON.stringify({ name: "@brink-lang/studio", dependencies: { a: "2.0.0" } });
    const result = checkStudioChangeset({
      changedFiles: ["packages/brink-studio/package.json"],
      relevantChangesets: [],
      packageJsonDiffs: [{ path: "packages/brink-studio/package.json", oldText, newText }],
    });
    assert.equal(result.ok, false, "a dependencies edit shapes the published bundle");
  });

  it("requires a changeset for a NEWLY ADDED package.json (no old side to carve out)", () => {
    const newText = JSON.stringify({ name: "@brink-lang/studio", version: "1.0.0" });
    const result = checkStudioChangeset({
      changedFiles: ["packages/brink-studio/package.json"],
      relevantChangesets: [],
      packageJsonDiffs: [{ path: "packages/brink-studio/package.json", oldText: "", newText }],
    });
    assert.equal(result.ok, false);
  });
});

describe("packageJsonChangeIsIgnorable", () => {
  it("is true when only version differs", () => {
    const oldText = JSON.stringify({ name: "x", version: "1.0.0", dependencies: { a: "1" } });
    const newText = JSON.stringify({ name: "x", version: "1.0.1", dependencies: { a: "1" } });
    assert.equal(packageJsonChangeIsIgnorable(oldText, newText), true);
  });

  it("is true when only devDependencies differs", () => {
    const oldText = JSON.stringify({ name: "x", devDependencies: { a: "1" } });
    const newText = JSON.stringify({ name: "x", devDependencies: { a: "2" } });
    assert.equal(packageJsonChangeIsIgnorable(oldText, newText), true);
  });

  it("is false when dependencies differs", () => {
    const oldText = JSON.stringify({ name: "x", dependencies: { a: "1" } });
    const newText = JSON.stringify({ name: "x", dependencies: { a: "2" } });
    assert.equal(packageJsonChangeIsIgnorable(oldText, newText), false);
  });

  it("is false when exports differs", () => {
    const oldText = JSON.stringify({ name: "x", exports: { ".": "./a.js" } });
    const newText = JSON.stringify({ name: "x", exports: { ".": "./b.js" } });
    assert.equal(packageJsonChangeIsIgnorable(oldText, newText), false);
  });

  it("is true when nothing differs at all", () => {
    const text = JSON.stringify({ name: "x", version: "1.0.0" });
    assert.equal(packageJsonChangeIsIgnorable(text, text), true);
  });

  it("does not false-positive on key reordering inside an ignored key's nested object", () => {
    const oldText = '{"name":"x","devDependencies":{"a":"1","b":"2"}}';
    const newText = '{"name":"x","devDependencies":{"b":"2","a":"1"}}';
    assert.equal(packageJsonChangeIsIgnorable(oldText, newText), true);
  });

  it("is false (conservative) when either side fails to parse as JSON", () => {
    assert.equal(packageJsonChangeIsIgnorable("not json", JSON.stringify({ name: "x" })), false);
    assert.equal(packageJsonChangeIsIgnorable(JSON.stringify({ name: "x" }), "not json"), false);
  });

  it("is false (conservative) for an empty old side — a newly added file", () => {
    assert.equal(packageJsonChangeIsIgnorable("", JSON.stringify({ name: "x", version: "1.0.0" })), false);
  });
});

describe("GUARDED_BUNDLE_FILES / PACKAGE_JSON_BUNDLE_FILES", () => {
  it("PACKAGE_JSON_BUNDLE_FILES is a subset of GUARDED_BUNDLE_FILES", () => {
    for (const path of PACKAGE_JSON_BUNDLE_FILES) {
      assert.ok(GUARDED_BUNDLE_FILES.includes(path), `${path} should also be in GUARDED_BUNDLE_FILES`);
    }
  });

  it("names exactly the files from #2834 item 1", () => {
    assert.deepEqual(
      [...GUARDED_BUNDLE_FILES].sort(),
      [
        "packages/brink-studio/alias-map.ts",
        "packages/brink-studio/index.html",
        "packages/brink-studio/package.json",
        "packages/brink-studio/tsup.config.ts",
        "packages/brink-studio/vite.config.embed.ts",
        "packages/brink-studio/vite.config.ts",
        "packages/studio-shell/package.json",
      ].sort(),
    );
  });
});
