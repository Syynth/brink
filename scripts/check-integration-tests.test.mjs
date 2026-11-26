// Tests for scripts/check-integration-tests.mjs. Node's built-in test runner,
// like the other scripts/*.test.mjs: `pnpm test:scripts` runs this in CI's
// `frontend` job before `pnpm install`, so it depends on nothing installed.
//
// Two halves:
//   1. Unit tests over synthetic crates in a scratch tree, each planting the
//      mistake the guard exists for, so every branch is seen going red.
//   2. A check of the real repo as it stands.

import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, describe, it } from "node:test";
import assert from "node:assert/strict";

import { REPO_ROOT, checkIntegrationTests, declaredModules } from "./check-integration-tests.mjs";

const scratch = [];
after(() => {
  for (const dir of scratch) rmSync(dir, { recursive: true, force: true });
});

/** A scratch repo whose `crates/demo` crate holds `files` (path -> contents). */
function repoWith(files) {
  const root = mkdtempSync(join(tmpdir(), "check-integration-tests-"));
  scratch.push(root);
  const crate = join(root, "crates", "demo");
  mkdirSync(join(crate, "tests"), { recursive: true });
  writeFileSync(join(crate, "Cargo.toml"), '[package]\nname = "demo"\n');
  for (const [path, text] of Object.entries(files)) {
    const full = join(crate, path);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, text);
  }
  return root;
}

const NONE = new Map();

describe("checkIntegrationTests on synthetic crates", () => {
  it("accepts a crate whose every test file main.rs reaches", () => {
    const root = repoWith({
      "tests/integration/main.rs": 'mod alpha;\n#[cfg(feature = "x")]\nmod beta;\n',
      "tests/integration/alpha.rs": '#[path = "alpha/cases.rs"]\nmod cases;\n',
      "tests/integration/alpha/cases.rs": "",
      "tests/integration/beta.rs": "mod nested;\n",
      "tests/integration/beta/nested.rs": "",
    });
    const result = checkIntegrationTests({ repoRoot: root, separate: NONE });
    assert.deepEqual(result.offenses, []);
    assert.equal(result.ok, true);
  });

  it("flags a file under tests/integration/ that no mod declaration reaches", () => {
    const root = repoWith({
      "tests/integration/main.rs": "mod alpha;\n",
      "tests/integration/alpha.rs": "",
      "tests/integration/forgotten.rs": "#[test] fn never_runs() {}\n",
    });
    const result = checkIntegrationTests({ repoRoot: root, separate: NONE });
    assert.equal(result.ok, false);
    assert.deepEqual(
      result.offenses.map((o) => o.path),
      ["crates/demo/tests/integration/forgotten.rs"],
    );
  });

  it("flags a file reached only through an unreached module", () => {
    const root = repoWith({
      "tests/integration/main.rs": "",
      "tests/integration/orphan.rs": "mod child;\n",
      "tests/integration/orphan/child.rs": "",
    });
    const paths = checkIntegrationTests({ repoRoot: root, separate: NONE }).offenses.map((o) => o.path);
    assert.deepEqual(paths.sort(), [
      "crates/demo/tests/integration/orphan.rs",
      "crates/demo/tests/integration/orphan/child.rs",
    ]);
  });

  it("flags a new top-level tests/*.rs, and allows a sanctioned one", () => {
    const root = repoWith({
      "tests/integration/main.rs": "",
      "tests/own_binary.rs": "#[test] fn t() {}\n",
      "tests/needs_chdir.rs": "#[test] fn t() {}\n",
    });
    const separate = new Map([["crates/demo/tests/needs_chdir.rs", "changes the working directory"]]);
    const result = checkIntegrationTests({ repoRoot: root, separate });
    assert.deepEqual(
      result.offenses.map((o) => o.path),
      ["crates/demo/tests/own_binary.rs"],
    );
  });

  it("flags a declaration whose file is missing", () => {
    const root = repoWith({ "tests/integration/main.rs": "mod gone;\n" });
    const result = checkIntegrationTests({ repoRoot: root, separate: NONE });
    assert.equal(result.offenses.length, 1);
    assert.match(result.offenses[0].problem, /does not exist/);
  });

  it("flags tests/integration/ without a main.rs", () => {
    const root = repoWith({ "tests/integration/alpha.rs": "" });
    const result = checkIntegrationTests({ repoRoot: root, separate: NONE });
    assert.deepEqual(
      result.offenses.map((o) => o.path),
      ["crates/demo/tests/integration"],
    );
  });

  it("flags a stale SEPARATE_TARGETS entry", () => {
    const root = repoWith({ "tests/integration/main.rs": "" });
    const separate = new Map([["crates/demo/tests/deleted.rs", "was separate once"]]);
    const result = checkIntegrationTests({ repoRoot: root, separate });
    assert.equal(result.offenses.length, 1);
    assert.match(result.offenses[0].problem, /stale entry/);
  });
});

describe("declaredModules", () => {
  it("finds a #[path] above a multi-line attribute, and skips inline modules", () => {
    const root = repoWith({
      "tests/integration/main.rs":
        '#[path = "../shared/model.rs"]\n#[expect(\n    dead_code,\n    reason = "x"\n)]\nmod model;\nmod inline {\n}\n',
      "tests/shared/model.rs": "",
    });
    const main = join(root, "crates/demo/tests/integration/main.rs");
    const decls = declaredModules(main, "");
    assert.deepEqual(decls, []);
    const text =
      '#[path = "../shared/model.rs"]\n#[expect(\n    dead_code,\n    reason = "x"\n)]\nmod model;\nmod inline {\n}\n';
    const found = declaredModules(main, text);
    assert.equal(found.length, 1);
    assert.equal(found[0].name, "model");
    assert.equal(found[0].exists, true);
    assert.equal(found[0].target, join(root, "crates/demo/tests/shared/model.rs"));
  });
});

describe("the real repo", () => {
  it("has every crate's integration tests in one binary, all declared", () => {
    const result = checkIntegrationTests({ repoRoot: REPO_ROOT });
    assert.deepEqual(result.offenses, []);
    assert.ok(result.crates > 0, "found no crates with tests/ at all");
  });
});
