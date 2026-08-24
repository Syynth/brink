/**
 * TODOs tool window helpers (#3050): E189 collection with container
 * attribution + line resolution, the file → knot/stitch grouping, the
 * filter, and the exit-animation keying (occurrence-disambiguated JSON
 * keys, per the composite-key house rule).
 */

import { describe, expect, it } from "vitest";
import {
  TODO_DIAGNOSTIC_CODE,
  collectTodoItems,
  groupTodoItems,
  keyTodoItems,
  matchesTodoFilter,
  todoKey,
  type TodoItem,
} from "@brink/studio-ui";
import type { Diagnostic, DocumentSymbol, FileOutline } from "@brink/wasm-types";

function todoDiag(file: string, start: number, message: string): Diagnostic {
  return {
    file,
    start,
    end: start + 4,
    message,
    severity: "Info",
    code: TODO_DIAGNOSTIC_CODE,
  };
}

function sym(
  name: string,
  kind: string,
  full_start: number,
  full_end: number,
  children: DocumentSymbol[] = [],
): DocumentSymbol {
  return { name, kind, start: full_start, end: full_start + name.length, full_start, full_end, children };
}

const OUTLINE: FileOutline[] = [
  {
    path: "prologue.ink",
    symbols: [
      sym("start_new_game", "knot", 30, 120, [sym("letter", "stitch", 80, 120)]),
    ],
  },
];

describe("collectTodoItems", () => {
  it("keeps only E189, strips the TODO prefix, resolves line and container", () => {
    const diags: Diagnostic[] = [
      todoDiag("prologue.ink", 0, "TODO: tighten the opening"),
      { file: "prologue.ink", start: 5, end: 6, message: "unused", severity: "Warning", code: "W012" },
      todoDiag("prologue.ink", 40, "TODO: pacing pass"),
      todoDiag("prologue.ink", 90, "TODO: voice is too old"),
    ];
    const src = "TODO: tighten the opening\n" + "x".repeat(200);
    const items = collectTodoItems(diags, OUTLINE, () => src);
    expect(items.map((i) => [i.text, i.container, i.line])).toEqual([
      ["tighten the opening", null, 1],
      ["pacing pass", "start_new_game", 2],
      ["voice is too old", "start_new_game.letter", 2],
    ]);
  });

  it("bare TODO message keeps empty text; missing source drops the line", () => {
    const items = collectTodoItems([todoDiag("a.ink", 0, "TODO")], [], () => null);
    expect(items).toHaveLength(1);
    expect(items[0].text).toBe("");
    expect(items[0].line).toBeNull();
  });
});

describe("groupTodoItems", () => {
  const item = (file: string, start: number, container: string | null, text = "t"): TodoItem => ({
    file,
    start,
    end: start + 1,
    text,
    line: null,
    container,
  });

  it("groups by file then container, preserving document order", () => {
    const groups = groupTodoItems([
      item("a.ink", 0, null),
      item("a.ink", 40, "start"),
      item("a.ink", 60, "start"),
      item("b.ink", 10, "clue"),
    ]);
    expect(groups.map((g) => [g.file, g.count])).toEqual([
      ["a.ink", 3],
      ["b.ink", 1],
    ]);
    expect(groups[0].groups.map((g) => [g.container, g.items.length])).toEqual([
      [null, 1],
      ["start", 2],
    ]);
  });
});

describe("matchesTodoFilter", () => {
  const base: TodoItem = {
    file: "prologue.ink",
    start: 0,
    end: 1,
    text: "Minnie's letter needs a pass",
    line: 1,
    container: "start_new_game",
  };
  it("matches text, file, and container case-insensitively; empty matches all", () => {
    expect(matchesTodoFilter(base, "")).toBe(true);
    expect(matchesTodoFilter(base, "LETTER")).toBe(true);
    expect(matchesTodoFilter(base, "prologue")).toBe(true);
    expect(matchesTodoFilter(base, "new_game")).toBe(true);
    expect(matchesTodoFilter(base, "casefile")).toBe(false);
  });
});

describe("keyTodoItems", () => {
  it("disambiguates identical notes by occurrence", () => {
    const a: TodoItem = { file: "a.ink", start: 0, end: 1, text: "same", line: 1, container: null };
    const b: TodoItem = { ...a, start: 50 };
    const keyed = keyTodoItems([a, b]);
    expect(keyed.size).toBe(2);
    expect(keyed.get(todoKey(a, 0))).toBe(a);
    expect(keyed.get(todoKey(b, 1))).toBe(b);
  });
});
