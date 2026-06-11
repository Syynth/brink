/**
 * Search engine tests (issue #94, spec §4 "Search").
 *
 * Pure-function coverage: pattern building (case / whole-word / regex,
 * invalid-regex errors), multi-file search with grouping, line/offset
 * bookkeeping in UTF-16 space, the result cap (unbounded-growth guard),
 * zero-length-match termination, replacement text (capture groups), span
 * edits, and the result-row display segments.
 */

import { describe, expect, it } from "vitest";
import {
  DEFAULT_SEARCH_OPTIONS,
  SEARCH_CONTEXT_BEFORE,
  SEARCH_RESULT_CAP,
  applyReplacements,
  buildSearchPattern,
  escapeRegExp,
  matchLineSegments,
  replacementTextFor,
  searchSources,
  type SearchQueryOptions,
} from "@brink/studio-store";

function options(over: Partial<SearchQueryOptions> = {}): SearchQueryOptions {
  return { ...DEFAULT_SEARCH_OPTIONS, ...over };
}

function pattern(query: string, over: Partial<SearchQueryOptions> = {}): RegExp {
  const built = buildSearchPattern(query, options(over));
  if (!built.ok) throw new Error(built.error);
  return built.pattern;
}

// ── buildSearchPattern ───────────────────────────────────────────────

describe("buildSearchPattern", () => {
  it("escapes literal queries (regex metacharacters match literally)", () => {
    expect(escapeRegExp("a.b*c")).toBe("a\\.b\\*c");
    const p = pattern("-> intro");
    expect(p.test("-> intro")).toBe(true);
    expect(p.test("-X intro")).toBe(false);
  });

  it("is case-insensitive by default, case-sensitive on request", () => {
    expect(pattern("the").test("The")).toBe(true);
    expect(pattern("the", { caseSensitive: true }).test("The")).toBe(false);
  });

  it("whole word wraps with word boundaries (literal and regex)", () => {
    expect(pattern("the", { wholeWord: true }).test("that")).toBe(false);
    expect(pattern("the", { wholeWord: true }).test("the light")).toBe(true);
    // Regex alternation is group-wrapped, so \b applies to the whole query.
    const p = pattern("the|light", { wholeWord: true, regex: true });
    expect(p.test("lighthouse")).toBe(false);
    expect(p.test("a light on")).toBe(true);
  });

  it("validates regex queries with an inline error", () => {
    const built = buildSearchPattern("(", options({ regex: true }));
    expect(built.ok).toBe(false);
    if (!built.ok) expect(built.error).toContain("Invalid regex");
  });

  it("multiline anchors match per line", () => {
    const p = pattern("^=== \\w+", { regex: true });
    expect(p.test("text\n=== intro ===")).toBe(true);
  });
});

// ── searchSources ────────────────────────────────────────────────────

describe("searchSources", () => {
  const files = [
    { path: "a.ink", source: "The lights dim.\nA figure steps into the light.\n" },
    { path: "b.ink", source: "No matches here.\n" },
    { path: "c.ink", source: "the end\n" },
  ];

  it("groups matches by file, skipping files without matches", () => {
    const result = searchSources(files, pattern("the"));
    expect(result.files.map((f) => f.path)).toEqual(["a.ink", "c.ink"]);
    expect(result.files[0].matches).toHaveLength(2);
    expect(result.files[1].matches).toHaveLength(1);
    expect(result.totalMatches).toBe(3);
    expect(result.capped).toBe(false);
  });

  it("reports 1-based lines and line-relative spans", () => {
    const result = searchSources(files, pattern("figure"));
    const match = result.files[0].matches[0];
    expect(match.line).toBe(2);
    expect(match.lineText).toBe("A figure steps into the light.");
    expect(match.lineStart).toBe(2);
    expect(match.lineEnd).toBe(8);
    expect(match.text).toBe("figure");
    // Absolute span is the editor.reveal source span.
    expect(files[0].source.slice(match.start, match.end)).toBe("figure");
  });

  it("offsets are UTF-16 code units (astral chars count as two)", () => {
    const source = "🙂 the\n";
    const result = searchSources([{ path: "e.ink", source }], pattern("the"));
    const match = result.files[0].matches[0];
    expect(match.start).toBe(3); // emoji = 2 units + space
    expect(source.slice(match.start, match.end)).toBe("the");
  });

  it("caps total matches across files and reports it", () => {
    const result = searchSources(files, pattern("e"), 4);
    expect(result.totalMatches).toBe(4);
    expect(result.capped).toBe(true);
    // Capped mid-file: later files are not scanned.
    const counted = result.files.reduce((n, f) => n + f.matches.length, 0);
    expect(counted).toBe(4);
  });

  it("defaults the cap to SEARCH_RESULT_CAP", () => {
    const big = { path: "big.ink", source: "x".repeat(SEARCH_RESULT_CAP + 50) };
    const result = searchSources([big], pattern("x"));
    expect(result.totalMatches).toBe(SEARCH_RESULT_CAP);
    expect(result.capped).toBe(true);
  });

  it("terminates on zero-length regex matches", () => {
    const result = searchSources(
      [{ path: "z.ink", source: "abc" }],
      pattern("x*", { regex: true }),
      10,
    );
    expect(result.capped).toBe(false);
    expect(result.totalMatches).toBe(4); // empty match at each position
  });
});

// ── Replacement ──────────────────────────────────────────────────────

describe("replacementTextFor / applyReplacements", () => {
  it("literal replace uses the replacement verbatim", () => {
    const result = searchSources(
      [{ path: "a.ink", source: "the $1 thing" }],
      pattern("the"),
    );
    const match = result.files[0].matches[0];
    expect(replacementTextFor(match, pattern("the"), "a $1", false)).toBe("a $1");
  });

  it("regex replace expands capture groups", () => {
    const p = pattern("(\\w+)-(\\w+)", { regex: true });
    const result = searchSources([{ path: "a.ink", source: "ab-cd" }], p);
    const match = result.files[0].matches[0];
    expect(replacementTextFor(match, p, "$2-$1", true)).toBe("cd-ab");
  });

  it("applies span edits independent of order", () => {
    const source = "one two three";
    const edits = [
      { start: 0, end: 3, text: "1" },
      { start: 8, end: 13, text: "3" },
      { start: 4, end: 7, text: "2" },
    ];
    expect(applyReplacements(source, edits)).toBe("1 2 3");
  });
});

// ── Display segments ─────────────────────────────────────────────────

describe("matchLineSegments", () => {
  it("splits the line around the match, trimming leading whitespace", () => {
    const result = searchSources(
      [{ path: "a.ink", source: "    The lights dim." }],
      pattern("lights"),
    );
    const segments = matchLineSegments(result.files[0].matches[0]);
    expect(segments).toEqual({ before: "The ", matchText: "lights", after: " dim." });
  });

  it("elides long leading context to keep the match visible", () => {
    const prefix = "x".repeat(SEARCH_CONTEXT_BEFORE + 20);
    const result = searchSources(
      [{ path: "a.ink", source: `${prefix}match` }],
      pattern("match"),
    );
    const segments = matchLineSegments(result.files[0].matches[0]);
    expect(segments.before).toBe(`…${"x".repeat(SEARCH_CONTEXT_BEFORE)}`);
    expect(segments.matchText).toBe("match");
  });
});
