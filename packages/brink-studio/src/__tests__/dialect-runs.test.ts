/**
 * The emitted-side run rule (#3388, RULED 2026-08-30): `runsOf` folds
 * parsed emitted lines into speaker runs by the dialect's `run_ends_at`
 * — the ONE implementation the Player and an engine share.
 */
import { describe, expect, it } from "vitest";
import {
  AT_CUE_DIALECT,
  DialectParser,
  extendDialect,
  runsOf,
  type DialogueDialect,
} from "@brink-lang/editor";

/** at-cue + a `>` action kind, runs ending at a new cue, an action, or choices. */
function projectDialect(runEndsAt: string[]): DialogueDialect {
  const base = extendDialect(AT_CUE_DIALECT, {
    elements: [
      {
        kind: "action",
        nature: "narrative",
        source: { pattern: "^(?<lead>>)(?<content>.*)$", content_group: "content", hidden: ["lead"], template: ">${content}" },
        emitted: { pattern: "^>\\s*(?<content>.*)$", content_group: "content", reserved_prefix: true },
        malformed: [],
      },
    ],
  });
  return { ...base, chain: (base.chain ?? []).map((r) => ({ ...r, run_ends_at: runEndsAt })) };
}

const EMITTED = [
  "@CUE1: Dialogue line one, cue attached",
  "Dialogue line two, no cue — still CUE1",
  "> An action paragraph",
  "Narrative after the action, nobody speaking",
  "@CUE2: Dialogue three",
  "Dialogue four, still CUE2",
];

function parse(dialect: DialogueDialect, boundaries: number[] = []) {
  const parser = new DialectParser(dialect);
  return EMITTED.map((text, i) => ({
    segments: parser.parseEmitted(text),
    boundary: boundaries.includes(i),
  }));
}

describe("runsOf (#3388)", () => {
  it("attributes cue-less lines to the last cue until an ender or a new cue", () => {
    const d = projectDialect(["character", "action"]);
    const runs = runsOf(parse(d), d);
    expect(runs).toEqual([
      { kind: "character", attrs: { speaker: "CUE1" }, lines: [0, 1] },
      { kind: "action", attrs: {}, lines: [2] },
      { kind: null, attrs: {}, lines: [3] },
      { kind: "character", attrs: { speaker: "CUE2" }, lines: [4, 5] },
    ]);
  });

  it("without an action ender, the action joins the run (the rule is declarative)", () => {
    const d = projectDialect(["character"]);
    const runs = runsOf(parse(d), d);
    expect(runs[0].lines).toEqual([0, 1, 2, 3]);
    expect(runs[1]).toEqual({ kind: "character", attrs: { speaker: "CUE2" }, lines: [4, 5] });
  });

  it("the reserved 'choices' boundary ends a run only when declared", () => {
    const declared = projectDialect(["character", "choices"]);
    const withChoices = runsOf(parse(declared, [1]), declared);
    expect(withChoices[0].lines).toEqual([0]);
    // Outside a run every line stands alone — nobody is speaking, so there
    // is nothing to group; the action paragraph keeps its own kind.
    expect(withChoices.slice(1, 4)).toEqual([
      { kind: null, attrs: {}, lines: [1] },
      { kind: "action", attrs: {}, lines: [2] },
      { kind: null, attrs: {}, lines: [3] },
    ]);

    const undeclared = projectDialect(["character"]);
    const ignored = runsOf(parse(undeclared, [1]), undeclared);
    expect(ignored[0].lines).toEqual([0, 1, 2, 3]);
  });

  it("the shipped preset (empty run_ends_at) only ends a run at the next cue", () => {
    const runs = runsOf(parse(AT_CUE_DIALECT), AT_CUE_DIALECT);
    // No action kind declared: `> …` is plain text, so it joins CUE1's run.
    expect(runs.map((r) => r.lines)).toEqual([[0, 1, 2, 3], [4, 5]]);
  });
});
