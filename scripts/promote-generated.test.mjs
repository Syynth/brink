// Tests for scripts/promote-generated.mjs (#3380). Node's built-in test
// runner under `pnpm test:scripts`, which CI's `frontend` job executes
// BEFORE `pnpm install` and without tools/inkjs-oracle's `npm ci` — so the
// end-to-end promotion below runs with INJECTED runners (fake brink compile,
// fake oracle) over a scratch tier directory. The real runners are exercised
// by promoting the first cases (tests/tier4-generated) and by
// `cargo test -p brink-test-harness --test tier4_generated`, which is what
// proves the written goldens are real.

import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  PromoteError,
  bumpCount,
  extractSourceFromLog,
  parseArgs,
  promote,
  reblessCaseToml,
  renderCaseToml,
} from "./promote-generated.mjs";

describe("parseArgs", () => {
  it("accepts the promotion shape", () => {
    const o = parseArgs(["--name", "glue-space", "--story", "s.ink", "--property", "inkjs_differential", "--issue", "#3507"]);
    assert.equal(o.name, "glue-space");
    assert.equal(o.story, "s.ink");
    assert.equal(o.property, "inkjs_differential");
    assert.equal(o.issue, "#3507");
    assert.equal(o.source, "proptest");
  });

  it("rejects a bad case name, a bad issue, an unknown source, and both inputs", () => {
    assert.throws(() => parseArgs(["--name", "Bad_Name", "--story", "s", "--property", "p"]), PromoteError);
    assert.throws(() => parseArgs(["--name", "ok", "--story", "s", "--property", "p", "--issue", "3507"]), PromoteError);
    assert.throws(() => parseArgs(["--name", "ok", "--story", "s", "--property", "p", "--source", "manual"]), PromoteError);
    assert.throws(() => parseArgs(["--name", "ok", "--story", "s", "--from-log", "l", "--property", "p"]), PromoteError);
    assert.throws(() => parseArgs(["--name", "ok", "--story", "s"]), PromoteError);
  });

  it("needs only a name for --rebless-csharp", () => {
    const o = parseArgs(["--name", "ok", "--rebless-csharp"]);
    assert.equal(o.reblessCsharp, true);
  });
});

describe("extractSourceFromLog", () => {
  it("takes the last shrunk story between the markers", () => {
    const log = [
      "Test failed: brink diverges",
      "--- source ---",
      "-> k",
      "=== k ===",
      "first",
      "-> END",
      "",
      " at crates/internal/brink-gen/tests/inkjs_differential.rs:158.",
      "minimal failing input: Story {",
      "...",
      "--- source ---",
      "-> k",
      "=== k ===",
      "last",
      "-> END",
      "",
      "minimal failing input: Story {",
    ].join("\n");
    assert.equal(extractSourceFromLog(log), "-> k\n=== k ===\nlast\n-> END\n");
  });

  it("refuses a log without a source block", () => {
    assert.throws(() => extractSourceFromLog("nothing here"), PromoteError);
  });
});

describe("renderCaseToml / reblessCaseToml / bumpCount", () => {
  it("renders provenance and the optional expected-mismatch table", () => {
    const text = renderCaseToml({
      source: "proptest",
      property: "inkjs_differential",
      seed: "cc 1234",
      oracleSource: "inkjs",
      issue: "#3508",
      expectedMismatch: "#3508",
    });
    assert.match(text, /^\[provenance\]$/m);
    assert.match(text, /^source = "proptest"$/m);
    assert.match(text, /^seed = "cc 1234"$/m);
    assert.match(text, /^oracle-source = "inkjs"$/m);
    assert.match(text, /^\[source\]\nexpected_mismatch = "#3508"$/m);
    const plain = renderCaseToml({ source: "probe", property: "p", oracleSource: "inkjs" });
    assert.doesNotMatch(plain, /expected_mismatch|seed =|issue =/);
  });

  it("flips oracle-source to csharp exactly once", () => {
    const text = renderCaseToml({ source: "probe", property: "p", oracleSource: "inkjs" });
    const flipped = reblessCaseToml(text);
    assert.match(flipped, /^oracle-source = "csharp"$/m);
    assert.throws(() => reblessCaseToml(flipped), PromoteError);
  });

  it("bumps the constant", () => {
    const { text, count } = bumpCount("x\nconst GENERATED_CASE_COUNT: usize = 4;\ny\n", 1);
    assert.equal(count, 5);
    assert.match(text, /^const GENERATED_CASE_COUNT: usize = 5;$/m);
    assert.throws(() => bumpCount("no constant", 1), PromoteError);
  });
});

describe("promote (injected runners)", () => {
  function scratchTier() {
    const root = mkdtempSync(join(tmpdir(), "promote-test-"));
    const tierDir = join(root, "tier4-generated");
    mkdirSync(tierDir);
    const countFile = join(root, "tier4_generated.rs");
    writeFileSync(countFile, "const GENERATED_CASE_COUNT: usize = 0;\n");
    const storyPath = join(root, "story.ink");
    writeFileSync(storyPath, "{0} <>\nworld\n-> END\n");
    return { root, tierDir, countFile, storyPath };
  }
  const fakeOracle = (storyPath, outDir) => {
    mkdirSync(outDir, { recursive: true });
    writeFileSync(join(outDir, "e0.oracle.json"), '{"steps":[],"outcome":"Ended","choice_path":[],"initial_state":{"variables":{},"turn_index":0}}\n');
  };

  it("writes the case, its golden, and bumps the count", () => {
    const t = scratchTier();
    try {
      const opts = parseArgs(["--name", "glue-space", "--story", t.storyPath, "--property", "inkjs_differential", "--issue", "#3507"]);
      const result = promote(opts, {
        tierDir: t.tierDir,
        countFile: t.countFile,
        runners: { compileWithBrink: () => {}, inkjsOracle: fakeOracle },
      });
      assert.equal(result.episodes, 1);
      assert.equal(result.count, 1);
      const caseDir = join(t.tierDir, "glue-space");
      assert.equal(readFileSync(join(caseDir, "story.ink"), "utf8"), "{0} <>\nworld\n-> END\n");
      assert.ok(existsSync(join(caseDir, "oracle", "e0.oracle.json")));
      assert.match(readFileSync(join(caseDir, "case.toml"), "utf8"), /issue = "#3507"/);
      assert.match(readFileSync(t.countFile, "utf8"), /= 1;/);
      // Promoting again without --force refuses and leaves the count alone.
      assert.throws(() => promote(opts, { tierDir: t.tierDir, countFile: t.countFile, runners: { compileWithBrink: () => {}, inkjsOracle: fakeOracle } }), /case exists/);
      assert.match(readFileSync(t.countFile, "utf8"), /= 1;/);
    } finally {
      rmSync(t.root, { recursive: true, force: true });
    }
  });

  it("refuses a story brink rejects, and one the oracle cannot golden, writing nothing", () => {
    const t = scratchTier();
    try {
      const opts = parseArgs(["--name", "bad", "--story", t.storyPath, "--property", "p"]);
      assert.throws(
        () => promote(opts, { tierDir: t.tierDir, countFile: t.countFile, runners: { compileWithBrink: () => { throw new Error("E999 boom"); }, inkjsOracle: fakeOracle } }),
        /does not compile/,
      );
      assert.throws(
        () => promote(opts, { tierDir: t.tierDir, countFile: t.countFile, runners: { compileWithBrink: () => {}, inkjsOracle: () => { throw new Error("COMPILE FAILED"); } } }),
        /could not produce a golden/,
      );
      assert.throws(
        () => promote(opts, { tierDir: t.tierDir, countFile: t.countFile, runners: { compileWithBrink: () => {}, inkjsOracle: (s, d) => mkdirSync(d, { recursive: true }) } }),
        /no episodes/,
      );
      assert.equal(existsSync(join(t.tierDir, "bad")), false);
      assert.match(readFileSync(t.countFile, "utf8"), /= 0;/);
    } finally {
      rmSync(t.root, { recursive: true, force: true });
    }
  });

  it("re-blesses an existing case with the C# oracle", () => {
    const t = scratchTier();
    try {
      const opts = parseArgs(["--name", "c", "--story", t.storyPath, "--property", "p"]);
      promote(opts, { tierDir: t.tierDir, countFile: t.countFile, runners: { compileWithBrink: () => {}, inkjsOracle: fakeOracle } });
      const re = parseArgs(["--name", "c", "--rebless-csharp"]);
      const result = promote(re, { tierDir: t.tierDir, countFile: t.countFile, runners: { csharpOracle: fakeOracle } });
      assert.equal(result.reblessed, true);
      assert.match(readFileSync(join(t.tierDir, "c", "case.toml"), "utf8"), /oracle-source = "csharp"/);
      assert.throws(() => promote(parseArgs(["--name", "missing", "--rebless-csharp"]), { tierDir: t.tierDir, countFile: t.countFile }), /no such case/);
    } finally {
      rmSync(t.root, { recursive: true, force: true });
    }
  });
});
