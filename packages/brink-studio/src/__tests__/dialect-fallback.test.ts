/**
 * `d`-flag (`hasIndices`) feature-detect fallback (#1013).
 *
 * `RegExp`'s `d` flag needs V8 9.0 / Chromium 90+ — NW.js-hosted embedders
 * on older Chromium (e.g. RPG Maker MZ's bundled Chromium 88) throw
 * `SyntaxError: Invalid flags supplied to RegExp constructor 'd'` at
 * CONSTRUCTION time, black-screening the embedder at boot. `dialect.ts` now
 * detects `d`-flag support ONCE at module scope and falls back to a
 * capture-group walk (`walkGroupSpans`) reconstructing the same per-group
 * `[start, end)` spans `indices` would otherwise give for free.
 *
 * CI's own engine supports the `d` flag, so `ResolvedDialect.compile`'s
 * normal auto-detected path never exercises the fallback. This suite forces
 * it unconditionally via `ResolvedDialect.compileWithDFlagSupport(dialect,
 * false)` (a test-only entry point `dialect.ts` exposes for exactly this)
 * and asserts every `classify()` result — kind, attrs, hiddenSpans,
 * contentSpan — is byte-identical to the `d`-flag path on the same inputs.
 */

import { describe, expect, it } from "vitest";
import {
  AT_CUE_DIALECT,
  extendDialect,
  ResolvedDialect,
  type DialogueDialect,
  type DialectMatch,
} from "@brink-lang/editor";
import fixture from "../../../../tests/dialect_fixtures/at_cue.json";

interface FixtureCase {
  id: string;
  description: string;
  line: string;
  chain_after?: string;
  expect: { kind: string; attrs?: Record<string, string> } | null;
}

const fixtureCases = fixture.cases as FixtureCase[];

/** Classify `line` against BOTH compile paths and assert identical output. */
function expectEqualAcrossPaths(dialect: DialogueDialect, line: string, leadingWs = 0): DialectMatch | null {
  const dFlag = ResolvedDialect.compileWithDFlagSupport(dialect, true);
  const fallback = ResolvedDialect.compileWithDFlagSupport(dialect, false);

  const a = dFlag.classify(line, leadingWs);
  const b = fallback.classify(line, leadingWs);
  expect(b).toEqual(a);
  return a;
}

describe("d-flag vs capture-group-walk fallback produce identical ranges (#1013)", () => {
  it("both compile paths accept the at-cue preset without throwing", () => {
    expect(() => ResolvedDialect.compileWithDFlagSupport(AT_CUE_DIALECT, true)).not.toThrow();
    expect(() => ResolvedDialect.compileWithDFlagSupport(AT_CUE_DIALECT, false)).not.toThrow();
  });

  it("loads a non-empty fixture corpus", () => {
    expect(fixtureCases.length).toBeGreaterThan(0);
  });

  // The full conformance corpus, both paths, every non-chain positive case —
  // proves equivalence across every line shape the at-cue preset itself is
  // pinned against (simple affix groups AND the nested parenthetical group).
  for (const c of fixtureCases) {
    if (c.chain_after !== undefined) continue; // chain rules don't touch regex spans
    it(`${c.id}: ${c.description}`, () => {
      const match = expectEqualAcrossPaths(AT_CUE_DIALECT, c.line);
      if (c.expect === null) {
        expect(match).toBeNull();
      } else {
        expect(match?.kind).toBe(c.expect.kind);
      }
    });
  }

  it("character cue: hiddenSpans + contentSpan match exactly (simple sibling groups: lead/speaker/tail)", () => {
    const match = expectEqualAcrossPaths(AT_CUE_DIALECT, "@Alice:<>");
    expect(match).toEqual({
      kind: "character",
      attrs: [["speaker", "Alice"]],
      hiddenSpans: [
        [0, 1],
        [6, 9],
      ],
      contentSpan: [1, 6],
    });
  });

  it("parenthetical: hiddenSpans + contentSpan match exactly (NESTED groups: content wraps content_inner)", () => {
    const match = expectEqualAcrossPaths(AT_CUE_DIALECT, "(warmly)<>");
    expect(match).toEqual({
      kind: "parenthetical",
      attrs: [["content", "(warmly)"]],
      hiddenSpans: [[8, 10]],
      contentSpan: [0, 8],
    });
  });

  it("empty captured groups (a bare cue with no speaker) still agree", () => {
    expectEqualAcrossPaths(AT_CUE_DIALECT, "@:<>");
  });

  it("leadingWs offsets propagate identically on both paths", () => {
    const match = expectEqualAcrossPaths(AT_CUE_DIALECT, "@Bob:<>", 4);
    expect(match).toEqual({
      kind: "character",
      attrs: [["speaker", "Bob"]],
      hiddenSpans: [
        [4, 5],
        [8, 11],
      ],
      contentSpan: [5, 8],
    });
  });

  it("a custom dialect with differently-sized affixes (<<name>>) agrees on both paths", () => {
    const custom = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "channel",
          nature: "narrative",
          source: {
            pattern: "^(?<lead><<)(?<speaker>[^>]*)(?<tail>>>)$",
            content_group: "speaker",
            hidden: ["lead", "tail"],
            template: "<<${speaker}>>",
          },
        },
      ],
    });
    const match = expectEqualAcrossPaths(custom, "<<radio>>");
    expect(match).toEqual({
      kind: "channel",
      attrs: [["speaker", "radio"]],
      hiddenSpans: [
        [0, 2],
        [7, 9],
      ],
      contentSpan: [2, 7],
    });
  });

  it("a custom dialect with a wider glue on parenthetical (nested groups, non-default sizes) agrees", () => {
    const custom = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "parenthetical",
          nature: "narrative",
          source: {
            pattern: "^(?<content>\\((?<content_inner>[^)]*)\\))(?<tail><<>>)$",
            content_group: "content",
            template_group: "content_inner",
            hidden: ["tail"],
            template: "(${content_inner})<<>>",
          },
        },
      ],
    });
    const match = expectEqualAcrossPaths(custom, "(aside)<<>>");
    expect(match).toEqual({
      kind: "parenthetical",
      attrs: [["content", "(aside)"]],
      hiddenSpans: [[7, 11]],
      contentSpan: [0, 7],
    });
  });

  it("repeated identical substrings at the same nesting level resolve in source order on both paths", () => {
    // `lead` and `tail` both capture "|" — the fallback's group walk must
    // advance its search cursor past each sibling in turn (not repeatedly
    // find the FIRST "|"), same as `d`-flag indices would.
    const custom = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "piped",
          nature: "narrative",
          source: {
            pattern: "^(?<lead>\\|)(?<speaker>[^|]*)(?<tail>\\|)$",
            content_group: "speaker",
            hidden: ["lead", "tail"],
            template: "|${speaker}|",
          },
        },
      ],
    });
    expectEqualAcrossPaths(custom, "|Eve|");
  });

  it("nested consumed groups with multiple siblings at the same nesting level", () => {
    // A pattern where a parent group `wrapper` contains two sibling nested
    // groups `inner1` and `inner2` separated by literal text. Both nested
    // groups participate in the match and must be found correctly within
    // their parent's span on the fallback path. The challenge: each nested
    // group's search must start from where the previous one ended, just like
    // at the top level.
    const custom = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "bracketed",
          nature: "narrative",
          source: {
            pattern: "^(?<wrapper>\\[(?<inner1>[^,]*)(?:,)(?<inner2>[^\\]]*)\\])$",
            content_group: "wrapper",
            hidden: [],
            template: "[${inner1},${inner2}]",
          },
        },
      ],
    });
    const match = expectEqualAcrossPaths(custom, "[first,second]");
    expect(match).toEqual({
      kind: "bracketed",
      attrs: [
        ["inner1", "first"],
        ["inner2", "second"],
        ["wrapper", "[first,second]"],
      ],
      hiddenSpans: [],
      contentSpan: [0, 14],
    });
  });
});
