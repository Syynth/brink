/**
 * Player run folding (#3389) + the `> ` choice-echo bug (#3390): transcript
 * rows fold into speaker runs with the project dialect; chrome rows never
 * join or parse; no dialect = plain rows.
 */
import { describe, expect, it } from "vitest";
import { AT_CUE_DIALECT, extendDialect } from "@brink-lang/editor";
import { foldPlayerRuns, speakerPaletteIndex } from "@brink/studio-ui";
import type { TranscriptLine } from "@brink/studio-store";

const line = (text: string): TranscriptLine => ({ text, kind: "line", tags: [] });
const marker = (text: string): TranscriptLine => ({ text, kind: "marker", tags: [] });

const DIALECT = (() => {
  const d = extendDialect(AT_CUE_DIALECT, {
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
  return { ...d, chain: (d.chain ?? []).map((r) => ({ ...r, run_ends_at: ["character", "action", "choices"] })) };
})();

describe("foldPlayerRuns (#3389)", () => {
  it("groups a cue and its cue-less lines; action and narrative stand outside", () => {
    const groups = foldPlayerRuns(
      [line("@CUE1: One."), line("Two, still CUE1."), line("> Action."), line("Narrative."), line("@CUE2: Three.")],
      DIALECT,
    );
    expect(groups.map((g) => [g.kind, g.speaker, g.rows.map((r) => r.index)])).toEqual([
      ["character", "CUE1", [0, 1]],
      ["action", null, [2]],
      [null, null, [3]],
      ["character", "CUE2", [4]],
    ]);
    // The cue row exposes its segments so the header renders separately.
    expect(groups[0].rows[0].segments[0]).toMatchObject({ kind: "character", content: "CUE1" });
  });

  it("a choice echo is a marker by KIND and ends the run; '> ' story text is not an echo (#3390)", () => {
    const groups = foldPlayerRuns(
      [line("@CUE1: One."), marker("> Browse his wares"), line("Two — after the choice, unattributed.")],
      DIALECT,
    );
    expect(groups.map((g) => [g.kind, g.rows.map((r) => r.index)])).toEqual([
      ["character", [0]],
      [null, [1]],
      [null, [2]],
    ]);
    expect(groups[1].rows[0].line.kind).toBe("marker");
    expect(groups[1].rows[0].segments).toEqual([]);
    // A story line starting with "> " under THIS dialect is an action, not an echo.
    const [action] = foldPlayerRuns([line("> Another action.")], DIALECT);
    expect(action.kind).toBe("action");
    expect(action.rows[0].line.kind).toBe("line");
  });

  it("a speaker who keeps talking is one group, however the script cued it (per-line cues merge)", () => {
    const lines = [
      line("@Griswold: Pointy end goes in the monster."),
      line("@Griswold: (lowering his voice) Old temple silver."),
      line("@Griswold: Please."),
      line("> He counts the coin."),
      line("@Griswold: Pleasure."),
    ];
    const groups = foldPlayerRuns(lines, DIALECT);
    expect(groups.map((g) => [g.speaker, g.rows.length])).toEqual([
      ["Griswold", 3],
      [null, 1],
      ["Griswold", 1],
    ]);
  });

  it("a choice echo ends the speaker's run even when the dialect's run rule does not say so", () => {
    // AT_CUE_DIALECT has no run_ends_at at all.
    const lines = [
      line("@Griswold: Buying or dying?"),
      marker("> Kneel and pray"),
      line("A cold blessing settles over you."),
      line("You stand again in the nave."),
    ];
    const groups = foldPlayerRuns(lines, AT_CUE_DIALECT);
    expect(groups.map((g) => [g.speaker, g.rows.length])).toEqual([
      ["Griswold", 1],
      [null, 1],
      [null, 2],
    ]);
  });

  it("no dialect: every row is a plain group — nothing is parsed", () => {
    const groups = foldPlayerRuns([line("@CUE1: One."), line("> text")], null);
    expect(groups.map((g) => [g.kind, g.rows[0].segments.length])).toEqual([[null, 0], [null, 0]]);
  });

  it("speaker palette indices are deterministic and bounded", () => {
    expect(speakerPaletteIndex("GRISWOLD", 6)).toBe(speakerPaletteIndex("GRISWOLD", 6));
    expect(speakerPaletteIndex("GRISWOLD", 6)).toBeLessThan(6);
    expect(speakerPaletteIndex("", 6)).toBeLessThan(6);
  });
});
