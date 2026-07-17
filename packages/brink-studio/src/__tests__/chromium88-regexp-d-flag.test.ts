/**
 * Chromium 88 RegExp `d`-flag guard (#1013).
 *
 * `RegExp`'s `d` (`hasIndices`) flag needs V8 9.0 / Chromium 90+ — NW.js
 * hosts commonly ship older engines (e.g. RPG Maker MZ's bundled NW.js is
 * Chromium 88, and no newer official runtime exists for it). The flags are
 * a runtime string, so no bundler target can lower it away: constructing a
 * RegExp with that flag THROWS at construction on an old engine, before a
 * single line is ever classified.
 *
 * `dialect.ts` is the one place in the editor/studio bundles that ever
 * needed the flag, and it now feature-detects support once at module scope
 * and falls back to a capture-group walk otherwise (proven equivalent in
 * `dialect-fallback.test.ts`). This test scans every source file across the
 * workspace's packages for an UNCONDITIONAL `d`-flag `RegExp(...)`
 * construction — a bare quoted flags literal containing the flag — and
 * fails on any hit, so the guarded fallback in `dialect.ts` stays the only
 * path that ever names it, and no other call site regresses to a
 * hardcoded, unconditional one.
 *
 * The module-scope feature-detection PROBE in `dialect.ts` itself
 * legitimately constructs one such call inside a `try` — that one line is
 * explicitly exempted via the `scan-allow` marker comment on it; everything
 * else must have zero matches.
 */

import { describe, expect, it } from "vitest";

// Every TS/TSX source file in the workspace's packages, as raw text.
// dist/ and node_modules/ live outside src/, so the glob never sees build
// output or dependencies; __tests__/__mocks__ excluded so this file (and
// its own doc comments describing the pattern) never scans itself.
const SOURCES = import.meta.glob(
  ["../../../*/src/**/*.{ts,tsx}", "!**/__tests__/**", "!**/__mocks__/**"],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

const SCAN_ALLOW_MARKER = "scan-allow: chromium88 d-flag feature probe";

/** Blank out block/line comments to spaces (preserving length and newlines,
 *  so every character offset in the result still lines up with the
 *  original source) — explanatory prose mentioning the flag must not trip
 *  the call scan, but reported offender lines and the allow-marker lookup
 *  both need real source positions. */
function blankComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/\/\/[^\n]*/g, (m) => " ".repeat(m.length));
}

/**
 * Find every `RegExp(...)` call whose LAST top-level argument is a bare
 * quoted flags literal containing `d` (e.g. `"d"`, `'gd'`, `` `dg` ``) —
 * the exact shape that throws on Chromium < 90. Walks parens with a depth
 * counter that treats string/template literals as opaque, so a pattern
 * argument containing literal `(`/`)` (e.g. `"(?<x>[^)]*)"`) never confuses
 * the call boundary. A computed/conditional flags argument (e.g.
 * `supportsDFlag ? "d" : ""`) is NOT a bare literal and does not match —
 * that's exactly the shape the feature-detect fallback uses, and exactly
 * why it's safe. Operates on already-comment-blanked source; returned
 * indices are positions of the `RegExp(` token in that (length-preserving)
 * source, so they're valid offsets into the original source too.
 */
function findDFlagCalls(source: string): number[] {
  const hits: number[] = [];
  const callRe = /RegExp\(/g;
  let m: RegExpExecArray | null;
  while ((m = callRe.exec(source)) !== null) {
    const start = m.index + m[0].length;
    let depth = 1;
    let i = start;
    let inString: '"' | "'" | "`" | null = null;
    let lastArgStart = start;
    const args: string[] = [];
    while (i < source.length && depth > 0) {
      const c = source[i];
      if (inString) {
        if (c === "\\") {
          i += 2;
          continue;
        }
        if (c === inString) inString = null;
        i++;
        continue;
      }
      if (c === '"' || c === "'" || c === "`") {
        inString = c;
        i++;
        continue;
      }
      if (c === "(") depth++;
      else if (c === ")") {
        depth--;
        if (depth === 0) {
          args.push(source.slice(lastArgStart, i));
          break;
        }
      } else if (c === "," && depth === 1) {
        args.push(source.slice(lastArgStart, i));
        lastArgStart = i + 1;
      }
      i++;
    }
    const lastArg = (args[args.length - 1] ?? "").trim();
    if (/^["'`][a-z]*d[a-z]*["'`]$/i.test(lastArg)) {
      hits.push(m.index);
    }
  }
  return hits;
}

function lineAt(source: string, index: number): string {
  const before = source.slice(0, index);
  const lineStart = before.lastIndexOf("\n") + 1;
  const lineEnd = source.indexOf("\n", index);
  return source.slice(lineStart, lineEnd === -1 ? source.length : lineEnd);
}

/** Whether the allow marker appears in the original (unblanked) source
 *  within a few lines above the given offset — the marker lives in a real
 *  comment, so this always checks the RAW source, never the blanked one. */
function hasAllowMarkerAbove(rawSource: string, index: number): boolean {
  const linesAbove = rawSource.slice(0, index).split("\n").slice(-4).join("\n");
  return linesAbove.includes(SCAN_ALLOW_MARKER);
}

describe("no unconditional RegExp `d`-flag construction anywhere in workspace bundles (#1013)", () => {
  it("scans a plausible file set (sanity check on the glob)", () => {
    const paths = Object.keys(SOURCES);
    expect(paths.length).toBeGreaterThan(50);
    expect(paths.some((p) => p.endsWith("ink-editor/src/dialect.ts"))).toBe(true);
  });

  it("dialect.ts's own feature-detect probe is present, unique, and carries the allow marker", () => {
    const entry = Object.entries(SOURCES).find(([p]) => p.endsWith("ink-editor/src/dialect.ts"));
    expect(entry).toBeTruthy();
    const [, source] = entry!;
    const hits = findDFlagCalls(blankComments(source));
    expect(hits.length).toBe(1);
    expect(hasAllowMarkerAbove(source, hits[0])).toBe(true);
  });

  it("finds zero unmarked unconditional d-flag RegExp(...) constructions across packages/*/src", () => {
    const offenders: string[] = [];
    for (const [path, rawSource] of Object.entries(SOURCES)) {
      for (const idx of findDFlagCalls(blankComments(rawSource))) {
        if (hasAllowMarkerAbove(rawSource, idx)) continue;
        offenders.push(`${path}:${lineAt(rawSource, idx).trim()}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});
