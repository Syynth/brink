// @vitest-environment node
/**
 * The rails column's width is a two-part contract that spans two packages,
 * and the halves are only correct together:
 *
 * - `@brink-lang/editor` sizes the in-flow spacer inside every rails gutter
 *   element (`RailMarker.toDOM`) to `RAIL_LANE_WIDTH_PX`.
 * - `@brink/studio-ui`'s `editor.css` reserves the column at that same width
 *   with `min-width`, because the gutter holds no markers at all until the
 *   HIR projection arrives a few hundred ms after a file opens.
 *
 * If the reservation is SMALLER than the spacer, the column grows when the
 * projection lands; `detachedGutters` pays gutter width back as the
 * content's padding-left, which is the text's offset, so the prose slides
 * sideways under the cursor on every file open (#3131). If it is LARGER, the
 * extra is permanently blank column.
 *
 * Scope, stated plainly: this pins the two numbers to each other. It does
 * NOT catch someone making the spacer depth-dependent again (the `5n + 2`
 * this replaced) — that invariant is held by the comment in
 * `RailMarker.toDOM` and by the fact that `RAIL_LANE_WIDTH_PX` is a
 * constant, not by this test.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { RAIL_LANE_WIDTH_PX } from "@brink-lang/editor";

// Same resolution as alias-map.test.ts, and the same reason for the `node`
// environment above it: under jsdom `import.meta.url` is not a file: URL, so
// `fileURLToPath` throws.
const packageRoot = resolve(fileURLToPath(new URL(".", import.meta.url)), "../..");
const CSS_PATH = resolve(packageRoot, "../studio-ui/src/styles/editor.css");

describe("rails gutter reservation", () => {
  it("reserves exactly the width the rail marker's spacer occupies", () => {
    const css = readFileSync(CSS_PATH, "utf8");
    const rule = /\.brink-hir-rail-gutter\s*\{[^}]*?min-width:\s*(\d+(?:\.\d+)?)px/s.exec(css);
    expect(rule, "editor.css should reserve .brink-hir-rail-gutter with a px min-width").not.toBe(
      null,
    );
    expect(Number(rule?.[1])).toBe(RAIL_LANE_WIDTH_PX);
  });
});
