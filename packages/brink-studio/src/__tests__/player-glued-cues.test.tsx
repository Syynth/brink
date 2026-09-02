/**
 * The Player's dialogue path on what the runtime ACTUALLY emits for a
 * glued cue and parenthetical (`@NAME:<>` / `(aside)<>` / line), verified
 * against `brink play` output 2026-09-02:
 *
 *   @GRISWOLD:(lowering his voice)Old temple silver. Stone does not care for it.
 *   @GRISWOLD:Please.
 *   > He counts the coin.
 *   @GRISWOLD:Pleasure doing business.
 *
 * Under the at-cue preset the Player must fold the first two into one
 * GRISWOLD group (per-line cues, nothing between), render the aside on
 * its own italic line, and give the re-entrance after the action its own
 * header.
 */
import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { AT_CUE_DIALECT, extendDialect } from "@brink-lang/editor";
import { foldPlayerRuns, renderRowBody } from "@brink/studio-ui";
import type { TranscriptLine } from "@brink/studio-store";

const EMITTED = [
  "@GRISWOLD:(lowering his voice)Old temple silver. Stone does not care for it.",
  "@GRISWOLD:Please.",
  "> He counts the coin.",
  "@GRISWOLD:Pleasure doing business.",
];
const line = (text: string): TranscriptLine => ({ text, kind: "line", tags: [] });

const DIALECT = (() => {
  const d = extendDialect(AT_CUE_DIALECT, {
    elements: [
      {
        kind: "action",
        nature: "narrative",
        source: { prefix: "> ", content_role: "content" },
        emitted: { pattern: "^>\\s*(?<content>.*)$", content_group: "content", reserved_prefix: true },
        malformed: [],
      },
    ],
  });
  return { ...d, chain: (d.chain ?? []).map((r) => ({ ...r, run_ends_at: ["action", "choices"] })) };
})();

describe("Player: glued cue + parenthetical as the runtime emits them", () => {
  it("folds per-line cues into one group, the action apart, the re-entrance with its own header", () => {
    const groups = foldPlayerRuns(EMITTED.map(line), DIALECT);
    expect(groups.map((g) => [g.speaker, g.kind, g.rows.length])).toEqual([
      ["GRISWOLD", "character", 2],
      [null, "action", 1],
      ["GRISWOLD", "character", 1],
    ]);
  });

  it("renders the aside on its own line and drops the cue text from the row", () => {
    const groups = foldPlayerRuns(EMITTED.map(line), DIALECT);
    const html = renderToStaticMarkup(createElement("p", null, renderRowBody(groups[0].rows[0])));
    expect(html).toBe(
      '<p><span class="player-run-paren">(lowering his voice)</span>Old temple silver. Stone does not care for it.</p>',
    );
    const second = renderToStaticMarkup(createElement("p", null, renderRowBody(groups[0].rows[1])));
    expect(second).toBe("<p>Please.</p>");
  });
});
