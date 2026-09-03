/**
 * The editor context menu's auto-fix entries (`fixActionsAt`,
 * `docs/autofix-spec.md` §7's editor-context-menu surface): the fixes for
 * the diagnostics under the pointer, sharing the code-actions menu's own
 * `getFixes`/`applyFix` seams verbatim.
 *
 * Adversarial review on PR #3454 (finding 2): this surface — one of the
 * five #3420 names — shipped with zero cases in `packages/ink-editor`'s own
 * gate (`pnpm --filter @brink-lang/editor test`); deleting `fixActionsAt`'s
 * body turned nothing red. `packages/brink-studio/src/__tests__/
 * editor-menu-line-actions.test.ts` pins the same behavior through the
 * studio's alias map (its sibling `lineActionsAt` precedent), but per
 * `vitest.config.ts`'s own rationale a change spanning both packages needs
 * both suites — this file is the one for `@brink-lang/editor` itself.
 *
 * Imports `fixActionsAt` directly from its source module, not through
 * `./index.js` / `@brink-lang/editor` (this config's own convention).
 */

import { describe, it, expect, vi } from "vitest";
import type { Fix } from "@brink/wasm-types";
import { fixActionsAt, type PlayFromHereOptions } from "../play-from-here.js";

const FIX: Fix = {
  code: "E025",
  title: "Import `haggle` from `story::market::barter`",
  applicability: "suggested",
  edits: [
    {
      path: "main.brink",
      start: 0,
      end: 0,
      new_text: "use story::market::barter::haggle;\n",
    },
  ],
};

function options(overrides: Partial<PlayFromHereOptions> = {}): PlayFromHereOptions {
  return { onPlayFrom: () => {}, ...overrides };
}

describe("fixActionsAt", () => {
  it("labels each entry with the fix's title, code, and applicability tier", () => {
    const actions = fixActionsAt(4, options({ getFixes: () => [FIX], applyFix: vi.fn() }));

    expect(actions).toHaveLength(1);
    expect(actions[0].label).toBe(FIX.title);
    expect(actions[0].code).toBe("E025");
    expect(actions[0].tier).toBe("suggested");
  });

  it("run() calls the host's applyFix with the exact fix object", () => {
    const applyFix = vi.fn();
    const actions = fixActionsAt(4, options({ getFixes: () => [FIX], applyFix }));

    actions[0].run();

    expect(applyFix).toHaveBeenCalledExactlyOnceWith(FIX);
  });

  it("returns [] when the host wired neither getFixes nor applyFix", () => {
    expect(fixActionsAt(4, options())).toEqual([]);
  });

  it("returns [] when only one of getFixes/applyFix is wired", () => {
    expect(fixActionsAt(4, options({ getFixes: () => [FIX] }))).toEqual([]);
    expect(fixActionsAt(4, options({ applyFix: vi.fn() }))).toEqual([]);
  });

  it("a throwing getFixes yields [] rather than taking the menu down (this catch was unreachable-by-test)", () => {
    const getFixes = (): Fix[] => {
      throw new Error("fix query failed");
    };

    expect(fixActionsAt(4, options({ getFixes, applyFix: vi.fn() }))).toEqual([]);
  });

  it("maps multiple offered fixes to multiple entries, preserving order", () => {
    const second: Fix = { ...FIX, code: "E173", title: "Add the required attribute" };
    const actions = fixActionsAt(
      4,
      options({ getFixes: () => [FIX, second], applyFix: vi.fn() }),
    );

    expect(actions.map((a) => a.code)).toEqual(["E025", "E173"]);
  });
});
