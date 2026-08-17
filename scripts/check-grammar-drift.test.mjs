// Tests for scripts/check-grammar-drift.mjs (#2718, #2719). Node's built-in
// test runner, matching check-pnpm-pin.test.mjs / check-scripts.test.mjs:
// runs under `pnpm test:scripts`, which CI's `frontend` job executes BEFORE
// `pnpm install`, so it must not depend on anything installed.
//
// Three halves:
//   1. Unit tests over the pure checkers, driven with SYNTHETIC input — both
//      a planted stale claim (must go red, naming the file/line) and
//      realistic CLEAN input that must NOT false-positive. The clean half is
//      not optional: #2689's SKIPPED_RE false-positived on `node --test`'s
//      own canonical `# skipped 0` line, and this guard's whole point is
//      distinguishing an honest historical mention from a stale claim, so a
//      test suite that only tries synthetic BAD input would prove nothing
//      about the distinction actually holding.
//   2. The line-wrap regression this guard's own development hit: a marker
//      phrase split across a wrapped comment line must still be found.
//   3. A FIXTURE-based integration test over the REAL repo files named in
//      #2695/#2701/#2707/#2712/#2718/#2719 — the ones that took four rounds
//      of hand-grepping to find. Walking "the repo as it exists today" with
//      no fixture would pass trivially once those five files were fixed and
//      assert nothing about the guard's logic; this locks BOTH the real
//      files' current state (all clean) AND the discovery walk actually
//      reaching them.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  CONTEXT_WINDOW_LINES,
  HISTORICAL_MARKER_RE,
  REPO_ROOT,
  STALE_TOKEN,
  checkFileForGrammarDrift,
  checkGrammarDrift,
  discoverScanFiles,
  findGrammarDriftOccurrences,
  stripCommentPrefix,
} from "./check-grammar-drift.mjs";

describe("HISTORICAL_MARKER_RE", () => {
  for (const marker of [
    "the comment now says `INLINE_WS*`",
    "matching the old, wrong `INLINE_WS+` prose",
    "matching the old wrong INLINE_WS+ prose",
    "predated #2695",
    "pre-#2695",
    "used to document",
    "used to say",
    "mismatch was fixed separately",
    "fixed separately by #2707",
    "no longer accurate",
  ]) {
    it(`matches "${marker}"`, () => {
      assert.equal(HISTORICAL_MARKER_RE.test(marker), true);
    });
  }

  it("does not match plain present-tense prose with no acknowledgment", () => {
    assert.equal(
      HISTORICAL_MARKER_RE.test("the doc comment there and the code disagree, requiring whitespace"),
      false,
    );
  });
});

describe("stripCommentPrefix", () => {
  it("strips a Rust /// doc-comment prefix", () => {
    assert.equal(stripCommentPrefix("/// stitch_header = { INLINE_WS+ }"), "stitch_header = { INLINE_WS+ }");
  });

  it("strips a Rust // line-comment prefix", () => {
    assert.equal(stripCommentPrefix("// the comment now"), "the comment now");
  });

  it("strips a JSDoc-style ` * ` continuation prefix", () => {
    assert.equal(stripCommentPrefix(" * says `INLINE_WS*`, matching the code"), "says `INLINE_WS*`, matching the code");
  });

  it("leaves prose with no comment marker untouched", () => {
    assert.equal(stripCommentPrefix("stitch_header = { \"=\" ~ INLINE_WS+ }"), "stitch_header = { \"=\" ~ INLINE_WS+ }");
  });
});

describe("findGrammarDriftOccurrences / checkFileForGrammarDrift — synthetic input", () => {
  it("flags a planted stale claim with no marker anywhere nearby (MAKE IT FAIL)", () => {
    const text = [
      "/// Some other doc comment above.",
      "///",
      "/// `parser/knot.rs` documents `stitch_header` as",
      "///",
      "/// ```text",
      "/// stitch_header = { \"=\" ~ INLINE_WS+ ~ identifier }",
      "/// ```",
      "///",
      "/// with required whitespace. The code disagrees.",
    ].join("\n");

    const occurrences = findGrammarDriftOccurrences(text);
    assert.equal(occurrences.length, 1);
    assert.equal(occurrences[0].line, 6);
    assert.equal(occurrences[0].ok, false);

    const result = checkFileForGrammarDrift("fixtures/planted-stale.rs", text);
    assert.equal(result.ok, false);
    assert.equal(result.problems.length, 1);
    assert.match(result.problems[0], /fixtures\/planted-stale\.rs:6/);
  });

  it("does NOT flag the same quote once a historical marker is added nearby (REVERT TO GREEN)", () => {
    const text = [
      "/// Some other doc comment above.",
      "///",
      "/// `parser/knot.rs` used to document `stitch_header` as",
      "///",
      "/// ```text",
      "/// stitch_header = { \"=\" ~ INLINE_WS+ ~ identifier }",
      "/// ```",
      "///",
      "/// with required whitespace (the comment now says `INLINE_WS*`,",
      "/// mismatch fixed separately by #2695). The code always matched zero-or-more.",
    ].join("\n");

    const occurrences = findGrammarDriftOccurrences(text);
    assert.equal(occurrences.length, 1);
    assert.equal(occurrences[0].ok, true);

    const result = checkFileForGrammarDrift("fixtures/fixed.rs", text);
    assert.equal(result.ok, true);
    assert.deepEqual(result.problems, []);
  });

  it("does not false-positive on realistic clean prose with no INLINE_WS+ at all", () => {
    // Mirrors #2689's SKIPPED_RE lesson: a checker must be tried against
    // realistic GOOD input, not only synthetic bad input, or a check that
    // cries wolf on honest prose ships unnoticed.
    const text = [
      "/// stitch_header = { \"=\" ~ !(\"=\" | \">\") ~ INLINE_WS* ~ identifier",
      "///                   ~ INLINE_WS* ~ knot_params? ~ INLINE_WS* ~ type_annotation? }",
      "///",
      "/// Whitespace after `=` is optional (`INLINE_WS*`, not `+`): the body",
      "/// calls `p.skip_ws()`, which accepts zero or more.",
    ].join("\n");

    const result = checkFileForGrammarDrift("fixtures/clean.rs", text);
    assert.equal(result.ok, true);
    assert.deepEqual(result.problems, []);
  });

  it("finds a marker even when the phrase wraps across a comment-prefixed line boundary", () => {
    // The exact regression this guard's own development hit: real text in
    // crates/internal/brink-syntax/src/parser/tests/knot/cst.rs wraps "the
    // comment now" / "says `INLINE_WS*`" across two `//`-prefixed lines. A
    // naive newline-join without stripping comment prefixes first would see
    // "now  // says" (the second line's own `// ` marker sitting between the
    // two words) and miss the match.
    const text = [
      "// still claimed `(\"function\" ~ INLINE_WS+)?` — required whitespace —",
      "// while the body accepts zero-or-more (the comment now",
      "// says `INLINE_WS*`, matching the code) but lexically unreachable.",
    ].join("\n");

    const occurrences = findGrammarDriftOccurrences(text);
    assert.equal(occurrences.length, 1);
    assert.equal(occurrences[0].ok, true, "marker phrase split across the line wrap must still be found");
  });

  it("reports one problem per occurrence, not one per file", () => {
    const text = [
      "// first stale quote: INLINE_WS+",
      "// second stale quote, also INLINE_WS+, still no marker",
    ].join("\n");

    const result = checkFileForGrammarDrift("fixtures/two-stale.rs", text);
    assert.equal(result.ok, false);
    assert.equal(result.problems.length, 2);
  });

  it("honors CONTEXT_WINDOW_LINES as an exported constant consistent with the default window", () => {
    // A marker exactly CONTEXT_WINDOW_LINES + 1 away must NOT count as nearby.
    const filler = Array.from({ length: CONTEXT_WINDOW_LINES }, (_, i) => `// filler line ${i}`);
    const text = ["// stitch_header = { INLINE_WS+ }", ...filler, "// now says INLINE_WS* elsewhere"].join("\n");

    const occurrences = findGrammarDriftOccurrences(text);
    assert.equal(occurrences.length, 1);
    assert.equal(occurrences[0].ok, false, "a marker outside the window must not suppress the flag");
  });
});

describe("discoverScanFiles", () => {
  it("finds the real repo files this guard's history centers on", () => {
    const files = new Set(discoverScanFiles(REPO_ROOT));
    for (const expected of [
      "docs/brink-studio-spec.md",
      "packages/brink-studio/src/__tests__/structural-refusal-shape.test.ts",
      "packages/brink-studio/src/__mocks__/brink-web.ts",
      "crates/brink-web/src/editor_refactor.rs",
      "crates/internal/brink-syntax/src/parser/knot.rs",
      "crates/internal/brink-syntax/src/parser/tests/declaration/cst.rs",
      "crates/internal/brink-syntax/src/parser/tests/knot/cst.rs",
    ]) {
      assert.equal(files.has(expected), true, `expected discoverScanFiles to include ${expected}`);
    }
  });

  it("prunes node_modules and target", () => {
    const files = discoverScanFiles(REPO_ROOT);
    assert.equal(
      files.some((f) => f.split("/").includes("node_modules") || f.split("/").includes("target")),
      false,
    );
  });
});

describe("checkGrammarDrift — fixture: the real repo, as it exists today", () => {
  // This is the fixture-based test the task asked for explicitly: not just
  // "walk the repo and assert ok" (which would pass vacuously on an empty
  // scan) but pin the exact files known to have carried this drift, and
  // confirm each is readable, scanned, and clean — while independently
  // re-deriving "clean" from the raw file text via checkFileForGrammarDrift,
  // not by trusting checkGrammarDrift's own aggregate.
  const namedFiles = [
    "docs/brink-studio-spec.md",
    "packages/brink-studio/src/__tests__/structural-refusal-shape.test.ts",
    "packages/brink-studio/src/__mocks__/brink-web.ts",
    "crates/brink-web/src/editor_refactor.rs",
    "crates/internal/brink-syntax/src/parser/knot.rs",
    "crates/internal/brink-syntax/src/parser/tests/declaration/cst.rs",
    "crates/internal/brink-syntax/src/parser/tests/knot/cst.rs",
  ];

  for (const path of namedFiles) {
    it(`${path} carries no unmarked \`${STALE_TOKEN}\` quote`, () => {
      const text = readFileSync(join(REPO_ROOT, path), "utf8");
      const result = checkFileForGrammarDrift(path, text);
      assert.deepEqual(result.problems, []);
      assert.equal(result.ok, true);
    });
  }

  it("the full repo-wide scan is green", () => {
    const result = checkGrammarDrift({ repoRoot: REPO_ROOT });
    if (!result.ok) {
      assert.fail(`grammar-comment drift found:\n${result.problems.map((p) => `  - ${p}`).join("\n")}`);
    }
    assert.equal(result.ok, true);
    assert.ok(result.filesScanned > 100, "sanity: the walk should reach far more than a handful of files");
  });
});
