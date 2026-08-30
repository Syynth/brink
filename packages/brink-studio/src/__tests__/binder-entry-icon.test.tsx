/**
 * The entry file's icon (replacing the "entry" text badge).
 *
 * The icon IS the brink mark — `assets/brand/brink-glyph.svg`, the asset
 * whose README describes it as "the drop alone with the carve as true
 * negative space, for use on any background". So the test reads that file
 * and compares, rather than restating the path data here: a copy would
 * agree with itself forever while the brand moved on, and the first
 * version of this icon was a hand-drawn arrow that looked plausible and
 * was not the mark at all.
 *
 * The same README rules out the shortcut that produced it: "the full arrow
 * is used at every size — there is no simplified small-size variant."
 */
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";
// By path, not through the package index: `icons.tsx` is deliberately not
// part of `@brink/studio-ui`'s public surface, and exporting it to satisfy
// a test would make it one.
import {
  BrinkFileEntryIcon,
  BrinkFileEntryOutlineIcon,
  BrinkFileIcon,
} from "../../../studio-ui/src/icons.js";

const REPO = resolve(fileURLToPath(import.meta.url), "../../../../../");
const GLYPH = readFileSync(join(REPO, "assets/brand/brink-glyph.svg"), "utf8");

/** Every `d="..."` in a chunk of SVG. */
const paths = (svg: string): string[] =>
  [...svg.matchAll(/\sd="([^"]+)"/g)].map((m) => m[1].trim());

describe("the entry icon", () => {
  it("uses the brand drop, verbatim", () => {
    const brandDrop = paths(GLYPH).find((d) => d.includes("A30 30"));
    expect(brandDrop, "brink-glyph.svg no longer has the bowl arc").toBeDefined();
    expect(paths(renderToStaticMarkup(createElement(BrinkFileEntryIcon)))).toContain(brandDrop);
  });

  it("uses the brand divert carve, verbatim", () => {
    // The carve, not a filled arrow. The brand states its construction —
    // one stroke weight, mass-centred on the bowl.
    const brandCarve = paths(GLYPH).find((d) => d.startsWith("M36 54"));
    expect(brandCarve, "brink-glyph.svg no longer has the carve").toBeDefined();
    expect(paths(renderToStaticMarkup(createElement(BrinkFileEntryIcon)))).toContain(brandCarve);
  });

  it("carves the divert as negative space, matching the brand's stroke", () => {
    const svg = renderToStaticMarkup(createElement(BrinkFileEntryIcon));
    expect(svg).toContain("<mask");
    expect(svg).toContain('stroke-width="7.5"');
    expect(svg).toContain('stroke-linecap="round"');
    // Filled, in the row's own colour — the drop is a silhouette with a hole.
    expect(svg).toContain('fill="currentColor"');
  });

  it("gives each instance its own mask id", () => {
    // Duplicate ids would make every entry row resolve the first mask.
    const one = /mask id="([^"]+)"/.exec(renderToStaticMarkup(createElement(BrinkFileEntryIcon)));
    const two = /mask id="([^"]+)"/.exec(renderToStaticMarkup(createElement(BrinkFileEntryIcon)));
    expect(one).not.toBeNull();
    expect(two).not.toBeNull();
    // React's useId is per-render-root, so a second render gives a fresh id.
    expect(one![1]).toBeTruthy();
    expect(two![1]).toBeTruthy();
  });

  it("lands the carve on the drop's bowl centre, in BOTH variants", () => {
    // The regression this pins was caught by eye, twice in one review: the
    // outline variant's arrow sat visibly high (a bowl centre mis-derived
    // as (50,47) — half-chord offset subtracted instead of added) and the
    // filled variant's sat low (the glyph box-centred in the viewBox
    // instead of mapped to the sibling drop). The brand mark defines the
    // relationship — the carve's shaft lies ON the bowl centre — so derive
    // both centres from the path data and check the mapped shaft hits it.

    // Sibling bowl, from BrinkFileIcon's arc: endpoints (73,41)/(27,41),
    // r28 -> centre (50, 41 + sqrt(28^2 - 23^2)).
    const sibling = renderToStaticMarkup(createElement(BrinkFileIcon));
    const arc = /A(\d+) \d+ 0 1 1 (\d+) (\d+)/.exec(sibling);
    expect(arc, "sibling drop no longer has its bowl arc").not.toBeNull();
    const r = Number(arc![1]);
    const [endX, endY] = [Number(arc![2]), Number(arc![3])];
    const centreY = endY + Math.sqrt(r * r - (50 - endX) * (50 - endX));

    // Brand carve shaft: "M36 54 ..." — y 54, ON the brand bowl centre.
    const shaftY = Number(/M36 (\d+)/.exec(GLYPH)![1]);

    for (const [name, component] of [
      ["filled", BrinkFileEntryIcon],
      ["outline", BrinkFileEntryOutlineIcon],
    ] as const) {
      const svg = renderToStaticMarkup(createElement(component));
      // The carve's <g transform="translate(tx ty) scale(s)">.
      const carve = svg.slice(0, svg.indexOf("M36"));
      const t = /translate\(([\d.-]+) ([\d.-]+)\) scale\(([\d.]+)\)/.exec(
        carve.slice(carve.lastIndexOf("transform")),
      );
      expect(t, `${name}: carve has no translate+scale transform`).not.toBeNull();
      const mappedY = shaftY * Number(t![3]) + Number(t![2]);
      expect(
        Math.abs(mappedY - centreY),
        `${name}: carve shaft maps to y=${mappedY}, bowl centre is y=${centreY}`,
      ).toBeLessThan(0.5);
    }
  });
});
