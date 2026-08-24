/**
 * Theme-token consistency guard (originally issue #276's Chromium 88
 * compat file).
 *
 * The color-mix() ban that used to live here was RETIRED with the
 * Chromium 88 / RMMZ floor (ruled 2026-08-23, docs/decision-log.md
 * "Chromium 88 floor dropped") — modern CSS is fine, and #3050's TODO
 * band is the first color-mix() user. What remains is floor-independent:
 * the precomputed `--bs-*-rgb` triplets and opaque two-color mixes in the
 * theme files must stay in sync with their base tokens — this recomputes
 * them from the palette and fails on drift.
 */

import { describe, expect, it } from "vitest";

// Every style source in the workspace, as raw text: CSS files plus the TS
// sources that embed CM6 themes. dist/ and node_modules/ live outside src/,
// so the glob never sees build output or dependencies.
const STYLE_SOURCES = import.meta.glob(
  [
    "../../../*/src/**/*.{css,ts,tsx}",
    "!**/__tests__/**",
    "!**/__mocks__/**",
  ],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

const THEME_FILES: Record<string, string> = Object.fromEntries(
  Object.entries(STYLE_SOURCES)
    .filter(([path]) => path.includes("studio-shell/src/styles/themes/"))
    .map(([path, css]) => [path.replace(/^.*\/([\w-]+)\.css$/, "$1"), css]),
);

/** Strip block and line comments so explanatory mentions don't trip parsing. */
function stripComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
}

describe("style-source glob sanity", () => {
  it("scans a plausible file set", () => {
    const paths = Object.keys(STYLE_SOURCES);
    expect(paths.length).toBeGreaterThan(50);
    expect(paths.some((p) => p.endsWith("studio-ui/src/styles/editor.css"))).toBe(true);
    expect(Object.keys(THEME_FILES).sort()).toEqual(["latte", "mocha"]);
  });
});

// ── Theme-token consistency ──────────────────────────────────────────

type VarMap = Map<string, string>;

function parseCustomProperties(css: string): VarMap {
  const map: VarMap = new Map();
  for (const m of stripComments(css).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    map.set(m[1], m[2].trim());
  }
  return map;
}

/** Follow var(--x) indirections (e.g. --bs-accent: var(--ctp-blue)). */
function resolveVar(map: VarMap, name: string): string {
  let value = map.get(name);
  for (let i = 0; i < 8 && value; i++) {
    const m = /^var\((--[\w-]+)\)$/.exec(value);
    if (!m) break;
    value = map.get(m[1]);
  }
  if (!value) throw new Error(`unresolved custom property ${name}`);
  return value;
}

function hexToRgb(hex: string): [number, number, number] {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) throw new Error(`expected 6-digit hex, got ${hex}`);
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

/** srgb interpolation — what color-mix(in srgb, a W%, b) computed. */
function mixSrgb(a: [number, number, number], w: number, b: [number, number, number]) {
  return a.map((c, i) => c * w + b[i] * (1 - w)) as [number, number, number];
}

// token holding the precomputed mix → [source token, weight, other token]
const OPAQUE_MIXES: Array<[string, string, number, string]> = [
  ["--bs-graph-knot-border", "--bs-symbol-knot", 0.55, "--bs-border"],
  ["--bs-graph-stitch-border", "--bs-symbol-stitch", 0.45, "--bs-border"],
  ["--bs-graph-end-border", "--bs-error", 0.55, "--bs-border"],
  ["--bs-graph-expanded-bg", "--bs-panel-bg", 0.55, "--bs-editor-bg"],
  ["--bs-graph-current-bg", "--bs-accent", 0.18, "--bs-panel-bg"],
  ["--bs-conflict-banner-bg", "--bs-warning", 0.12, "--bs-panel-bg"],
];

const RGB_TRIPLETS = [
  "--bs-accent",
  "--bs-surface-bg",
  "--bs-panel-bg",
  "--bs-fg",
  "--bs-fg-muted",
  "--bs-border",
  "--bs-error",
  "--bs-warning",
  "--bs-success",
];

describe.each(["mocha", "latte"])("theme %s derived colors stay in sync", (theme) => {
  const map = parseCustomProperties(THEME_FILES[theme]);

  it.each(RGB_TRIPLETS)("%s-rgb matches the base token", (token) => {
    const base = hexToRgb(resolveVar(map, token));
    const triplet = resolveVar(map, `${token}-rgb`)
      .split(/\s+/)
      .map(Number);
    expect(triplet).toEqual(base);
  });

  it.each(OPAQUE_MIXES)(
    "%s equals the srgb mix of %s at %d over %s",
    (derived, source, weight, other) => {
      const actual = hexToRgb(resolveVar(map, derived));
      const expected = mixSrgb(
        hexToRgb(resolveVar(map, source)),
        weight,
        hexToRgb(resolveVar(map, other)),
      );
      // ±1 per channel: the stored value is rounded to a hex byte.
      for (let i = 0; i < 3; i++) {
        expect(Math.abs(actual[i] - expected[i])).toBeLessThanOrEqual(1);
      }
    },
  );

  it("defines the translucent active-line token over the surface triplet", () => {
    expect(map.get("--bs-active-line-bg")).toBe(
      "rgb(var(--bs-surface-bg-rgb) / 60%)",
    );
  });
});
