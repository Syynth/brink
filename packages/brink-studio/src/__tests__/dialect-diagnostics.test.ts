/**
 * Dialogue-dialect diagnostics (#3391): brink.toml config findings as
 * Problems rows (resolver refusals are errors), and the dialect's own
 * `malformed` near-miss rules on narrative lines.
 */
import { describe, expect, it } from "vitest";
import { AT_CUE_DIALECT } from "@brink-lang/editor";
import { createStudioStore } from "@brink/studio-store";
import {
  CONFIG_FILE,
  configDiagnostics,
  malformedCueDiagnostics,
} from "../dialect-diagnostics.js";

describe("configDiagnostics", () => {
  it("maps resolver refusals to errors, other warnings to warnings, and a discovery error to an error", () => {
    const rows = configDiagnostics(
      ["[dialogue]: unknown dialogue preset `fountain`", "unknown key `dialogue.colour` in brink.toml (ignored)"],
      "invalid brink.toml: expected `=`",
    );
    expect(rows.map((r) => [r.severity, r.file])).toEqual([
      ["Error", CONFIG_FILE],
      ["Warning", CONFIG_FILE],
      ["Error", CONFIG_FILE],
    ]);
    expect(configDiagnostics([], null)).toEqual([]);
  });
});

describe("malformedCueDiagnostics", () => {
  it("flags a cue missing its terminator on a narrative line, never a line the dialect matched", () => {
    const src = "=== k ===\n@Alice\n@Bob:<>\nHello.\n";
    const rows = malformedCueDiagnostics("main.ink", src, AT_CUE_DIALECT);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      file: "main.ink",
      severity: "Warning",
      message: expect.stringContaining("missing the ':<>' terminator"),
    });
    expect(src.slice(rows[0].start, rows[0].end)).toBe("@Alice");
  });

  it("a dialect with no malformed rules yields nothing", () => {
    const bare = { ...AT_CUE_DIALECT, elements: (AT_CUE_DIALECT.elements ?? []).map((e) => ({ ...e, malformed: [] })) };
    expect(malformedCueDiagnostics("m.ink", "@Alice\n", bare)).toEqual([]);
  });
});

describe("problems slice: dialectDiagnostics bucket", () => {
  it("replaces per file and clears on an empty set", () => {
    const store = createStudioStore();
    const row = { start: 0, end: 0, message: "x", severity: "Error" as const, file: CONFIG_FILE };
    store.getState().setDialectDiagnostics(CONFIG_FILE, [row]);
    expect(store.getState().dialectDiagnostics[CONFIG_FILE]).toEqual([row]);
    store.getState().setDialectDiagnostics(CONFIG_FILE, []);
    expect(CONFIG_FILE in store.getState().dialectDiagnostics).toBe(false);
  });
});
