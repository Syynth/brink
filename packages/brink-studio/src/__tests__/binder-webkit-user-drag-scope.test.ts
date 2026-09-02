/**
 * Review finding on PR #3359 (issue #3351): `binder.css` applied
 * `-webkit-user-drag: element` unconditionally to every
 * `.brink-studio .brink-binder-row`, including rows React renders with
 * `draggable={false}` (the three placeholder rows, and any file/folder row
 * whose `ProjectSession` reports it cannot be renamed/deleted — see
 * `Binder.tsx`'s `canRenameFiles` gate).
 *
 * In WebKit, the `draggable` content attribute is a *presentational hint*
 * mapping to `-webkit-user-drag`, and presentational hints lose to author
 * rules in the cascade. An unscoped author rule therefore re-arms rows that
 * were deliberately made non-draggable as drag sources — in WKWebView, a
 * drag could start on a row whose `onDragStart` calls `preventDefault()` (or
 * that has no real drag handler at all), and reach `handleDrop` against a
 * `FileProvider` that declared it cannot move files.
 *
 * The fix scopes the rule to `[draggable="true"]` — React renders
 * `draggable={false}` as the literal DOM attribute `draggable="false"`, so
 * this selector is exact, not a heuristic.
 */

import { describe, expect, it } from "vitest";
import binderCss from "../../../studio-ui/src/styles/binder.css?raw";

/** Strip block comments so explanatory prose doesn't confuse the scan. */
function stripComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** Split a CSS source into {selector, body} rule pairs (no nested at-rules
 *  in this file, so a flat brace split is sufficient — mirrors the parsing
 *  approach in chromium88-color-mix.test.ts). */
function parseRules(css: string): Array<{ selector: string; body: string }> {
  const rules: Array<{ selector: string; body: string }> = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (const m of stripComments(css).matchAll(re)) {
    rules.push({ selector: m[1].trim(), body: m[2] });
  }
  return rules;
}

describe("binder.css: -webkit-user-drag scoping (review finding, PR #3359)", () => {
  const rules = parseRules(binderCss);
  const userDragRules = rules.filter((r) => /-webkit-user-drag\s*:/.test(r.body));

  it("declares -webkit-user-drag at least once (sanity: the fix under test is present)", () => {
    expect(userDragRules.length).toBeGreaterThan(0);
  });

  it("never applies -webkit-user-drag to a selector unscoped by [draggable=\"true\"]", () => {
    for (const rule of userDragRules) {
      expect(rule.selector).toContain('[draggable="true"]');
    }
  });

  it("does not opt in the bare, unscoped .brink-binder-row selector", () => {
    const bareRowRule = rules.find(
      (r) => r.selector === ".brink-studio .brink-binder-row",
    );
    expect(bareRowRule).toBeDefined();
    expect(bareRowRule!.body).not.toMatch(/-webkit-user-drag\s*:/);
  });
});
