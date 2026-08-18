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
  GUARDED_PREFIXES,
  STUDIO_PACKAGE,
  changesetNamesStudio,
  checkStudioChangeset,
  isChangesetPath,
  isGuardedPath,
  isTestOnlyPath,
  parseNameStatus,
  resolveBaseRef,
} from "./check-studio-changeset.mjs";

describe("isGuardedPath", () => {
  for (const prefix of GUARDED_PREFIXES) {
    it(`matches a file under ${prefix}`, () => {
      assert.equal(isGuardedPath(`${prefix}foo/bar.ts`), true);
    });
  }

  it("does not match the published packages/ink-editor (its own @brink-lang/editor changeset applies)", () => {
    assert.equal(isGuardedPath("packages/ink-editor/src/foo.ts"), false);
  });

  it("does not match the published packages/wasm (@brink-lang/web)", () => {
    assert.equal(isGuardedPath("packages/wasm/src/foo.ts"), false);
  });

  it("does not match a guarded package's root (README, package.json) outside src/", () => {
    assert.equal(isGuardedPath("packages/studio-shell/package.json"), false);
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
      addedChangesets: [],
    });
    assert.equal(result.ok, true);
    assert.deepEqual(result.problems, []);
  });

  it("passes when a guarded package is touched and a @brink-lang/studio changeset is added", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-shell/src/panel.tsx"],
      addedChangesets: [{ path: ".changeset/foo.md", text: '---\n"@brink-lang/studio": patch\n---\n\nx\n' }],
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
      addedChangesets: [],
    });
    assert.equal(result.ok, true, "a test-only diff must not be forced into an empty changeset");
    assert.deepEqual(result.problems, []);
  });

  it("fails when a guarded package is touched and no changeset is added at all — PR #2787's shape", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/brink-studio/src/binder/reveal.tsx", "packages/studio-store/src/slices/conflict.ts"],
      addedChangesets: [],
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
      addedChangesets: [{ path: ".changeset/foo.md", text: '---\n"@brink-lang/web": patch\n---\n\nx\n' }],
    });
    assert.equal(result.ok, false);
    assert.match(result.problems[0], /wasm-observable/);
  });

  it("fails when the added changeset is empty/placeholder (no package named)", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/ink-operations/src/splice.ts"],
      addedChangesets: [{ path: ".changeset/foo.md", text: "---\n---\n" }],
    });
    assert.equal(result.ok, false);
  });

  it("mixed diff: passes as long as ONE added changeset names studio, even alongside unrelated ones", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-ui/src/list.tsx"],
      addedChangesets: [
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
      addedChangesets: [],
    });
    assert.equal(result.ok, false);
    assert.match(result.problems[0], /binder\/reveal\.tsx/);
    assert.doesNotMatch(result.problems[0], /binder-tree\.test\.ts/);
  });

  it("does not require a changeset for a change to the published packages/ink-editor", () => {
    const result = checkStudioChangeset({
      changedFiles: ["packages/ink-editor/src/rename.ts"],
      addedChangesets: [],
    });
    assert.equal(result.ok, true);
  });

  it("does not treat a MODIFIED (not added) changeset as satisfying the guard", () => {
    // gatherDiff only ever includes status "A" changesets in addedChangesets
    // (see its filter), so a diff that merely edits an existing, unrelated
    // changeset must not appear here at all — modeled by simply not passing
    // it, proving the guard still fails on the guarded file.
    const result = checkStudioChangeset({
      changedFiles: ["packages/studio-store/src/store.ts"],
      addedChangesets: [],
    });
    assert.equal(result.ok, false);
  });
});
