/**
 * Problems + Output tool windows (shell issue 2.3 / #84, spec §4, §6.1).
 *
 * Covers: the canonical diagnostic ordering, diagnostics-list storage in the
 * compile slice, Problems row helpers (line:col resolution + the
 * `editor.reveal` Location payload), the Output slice (append / cap / clear),
 * story-error wiring into the Output log, and the component-based strip
 * badge plumbing in the tool-window descriptor.
 */

import { describe, expect, it, vi } from "vitest";
import {
  createStudioStore,
  LocalSessionProvider,
  sortDiagnostics,
  OUTPUT_LOG_LIMIT,
} from "@brink/studio-store";
import { ToolWindowRegistry, type ToolWindowDescriptor } from "@brink/studio-shell";
import {
  buildProblemRows,
  diagnosticLocation,
  offsetToLineCol,
} from "@brink/studio-ui";
import type { Diagnostic } from "@brink/wasm-types";

function diag(
  file: string,
  start: number,
  severity: "Error" | "Warning",
  message = "msg",
  end = start + 1,
): Diagnostic {
  return { file, start, end, severity, message };
}

// ── sortDiagnostics ─────────────────────────────────────────────────

describe("sortDiagnostics", () => {
  it("orders by file path, then offset, errors before warnings at ties", () => {
    const input = [
      diag("b.ink", 5, "Warning"),
      diag("a.ink", 10, "Warning", "w"),
      diag("a.ink", 10, "Error", "e"),
      diag("a.ink", 2, "Warning"),
      diag("b.ink", 0, "Error"),
    ];
    const sorted = sortDiagnostics(input);
    expect(sorted.map((d) => `${d.file}:${d.start}:${d.severity}`)).toEqual([
      "a.ink:2:Warning",
      "a.ink:10:Error",
      "a.ink:10:Warning",
      "b.ink:0:Error",
      "b.ink:5:Warning",
    ]);
  });

  it("is deterministic for full ties (end, then message) and non-mutating", () => {
    const input = [
      diag("a.ink", 1, "Error", "zeta", 9),
      diag("a.ink", 1, "Error", "alpha", 9),
      diag("a.ink", 1, "Error", "alpha", 3),
    ];
    const snapshot = [...input];
    const sorted = sortDiagnostics(input);
    expect(sorted.map((d) => `${d.end}:${d.message}`)).toEqual(["3:alpha", "9:alpha", "9:zeta"]);
    expect(input).toEqual(snapshot); // input untouched
  });
});

// ── Compile slice: diagnostics list storage ─────────────────────────

describe("CompileSlice diagnosticsList", () => {
  it("stores the structured list in canonical order alongside the counts", () => {
    const store = createStudioStore();
    const list = [diag("b.ink", 0, "Warning"), diag("a.ink", 4, "Error")];

    store.getState().setCompileResult([], { errors: 1, warnings: 1 }, list, null);

    const s = store.getState();
    expect(s.diagnostics).toEqual({ errors: 1, warnings: 1 });
    expect(s.diagnosticsList.map((d) => d.file)).toEqual(["a.ink", "b.ink"]);
  });

  it("clears the list on a clean compile", () => {
    const store = createStudioStore();
    store.getState().setCompileResult([], { errors: 1, warnings: 0 }, [diag("a.ink", 0, "Error")], null);
    store.getState().setCompileResult([], { errors: 0, warnings: 0 }, [], null);
    expect(store.getState().diagnosticsList).toEqual([]);
  });
});

// ── Problems row helpers ────────────────────────────────────────────

describe("offsetToLineCol", () => {
  it("computes 1-based line and column", () => {
    const text = "first\nsecond\nthird";
    expect(offsetToLineCol(text, 0)).toEqual({ line: 1, col: 1 });
    expect(offsetToLineCol(text, 4)).toEqual({ line: 1, col: 5 });
    expect(offsetToLineCol(text, 6)).toEqual({ line: 2, col: 1 });
    expect(offsetToLineCol(text, 15)).toEqual({ line: 3, col: 3 });
  });

  it("clamps offsets outside the text", () => {
    expect(offsetToLineCol("ab\ncd", -5)).toEqual({ line: 1, col: 1 });
    expect(offsetToLineCol("ab\ncd", 999)).toEqual({ line: 2, col: 3 });
  });
});

describe("buildProblemRows", () => {
  it("renders file:line:col when source is available, reading each file once", () => {
    const getSource = vi.fn((file: string) => (file === "a.ink" ? "one\ntwo\nthree" : null));
    const rows = buildProblemRows(
      [diag("a.ink", 4, "Error"), diag("a.ink", 8, "Warning"), diag("missing.ink", 7, "Error")],
      getSource,
    );

    expect(rows.map((r) => r.location)).toEqual([
      "a.ink:2:1",
      "a.ink:3:1",
      "missing.ink@7", // offset fallback without source text
    ]);
    expect(getSource).toHaveBeenCalledTimes(2); // once per file
  });
});

describe("diagnosticLocation", () => {
  it("produces the source-space editor.reveal payload (spec §6.1)", () => {
    expect(diagnosticLocation(diag("toppled-temple.ink", 12, "Error", "bad divert", 21))).toEqual({
      kind: "source",
      file: "toppled-temple.ink",
      span: { start: 12, end: 21 },
    });
  });
});

// ── Output slice ────────────────────────────────────────────────────

describe("OutputSlice", () => {
  it("appends entries with timestamp, source, and message", () => {
    const store = createStudioStore();
    const before = Date.now();
    store.getState().appendOutput("compile", "Compile succeeded");
    store.getState().appendOutput("story", "Runtime error: boom");
    const after = Date.now();

    const entries = store.getState().outputEntries;
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({ source: "compile", message: "Compile succeeded" });
    expect(entries[1]).toMatchObject({ source: "story", message: "Runtime error: boom" });
    expect(entries[0]!.timestamp).toBeGreaterThanOrEqual(before);
    expect(entries[0]!.timestamp).toBeLessThanOrEqual(after);
  });

  it("caps the log at OUTPUT_LOG_LIMIT, dropping the oldest entries", () => {
    const store = createStudioStore();
    for (let i = 0; i < OUTPUT_LOG_LIMIT + 10; i++) {
      store.getState().appendOutput("compile", `entry ${i}`);
    }
    const entries = store.getState().outputEntries;
    expect(entries).toHaveLength(OUTPUT_LOG_LIMIT);
    expect(entries[0]!.message).toBe("entry 10"); // oldest 10 dropped
    expect(entries.at(-1)!.message).toBe(`entry ${OUTPUT_LOG_LIMIT + 9}`);
  });

  it("clearOutput empties the log", () => {
    const store = createStudioStore();
    store.getState().appendOutput("compile", "x");
    store.getState().clearOutput();
    expect(store.getState().outputEntries).toEqual([]);
  });
});

describe("story errors feed the Output log", () => {
  it("revealNext logs a runtime error entry on a throwing session", () => {
    const store = createStudioStore();
    const session = {
      // Throw from the verb `reveal()` actually calls (#3011).
      continueSingle: vi.fn(() => {
        throw new Error("divert target not found");
      }),
      continueToPause: vi.fn(() => {
        throw new Error("divert target not found");
      }),
      free: vi.fn(),
      onJournalDirty: () => () => {},
    };
    store.getState()._bindProvider(
      new LocalSessionProvider({ session: session as never, status: "running" }),
    );

    store.getState().revealNext();

    expect(store.getState().sessionStatus).toBe("error");
    const entries = store.getState().outputEntries;
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      source: "story",
      message: "Runtime error: divert target not found",
    });
  });

  it("chooseOption logs a choose error entry", () => {
    const store = createStudioStore();
    const session = {
      choose: vi.fn(() => {
        throw new Error("no such choice");
      }),
      free: vi.fn(),
      onJournalDirty: () => () => {},
    };
    store.getState()._bindProvider(
      new LocalSessionProvider({
        session: session as never,
        status: "awaiting-choice",
        choices: [{ index: 0, text: "Go", tags: [] }],
      }),
    );

    store.getState().chooseOption(0);

    expect(store.getState().outputEntries).toMatchObject([
      { source: "story", message: "Choose error: no such choice" },
    ]);
  });
});

// ── Strip badge plumbing (descriptor `badge` component) ────────────

describe("tool-window badge descriptor", () => {
  it("registers and returns the badge component untouched", () => {
    const Badge = () => null;
    const descriptor: ToolWindowDescriptor = {
      id: "problems",
      title: "Problems",
      icon: null,
      defaultPlacement: { dock: "bottom", section: "start" },
      defaultOpen: false,
      badge: Badge,
      component: () => null,
    };
    const registry = new ToolWindowRegistry();
    registry.register(descriptor);
    expect(registry.get("problems")?.badge).toBe(Badge);
  });

  it("badge stays optional", () => {
    const registry = new ToolWindowRegistry();
    registry.register({
      id: "output",
      title: "Output",
      icon: null,
      defaultPlacement: { dock: "bottom", section: "end" },
      defaultOpen: false,
      component: () => null,
    });
    expect(registry.get("output")?.badge).toBeUndefined();
  });
});
