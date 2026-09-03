/**
 * The studio's auto-fix verbs (#3420, `docs/autofix-spec.md` §5–§7):
 * the batch commands, the report → write-list mapping, and fix on save.
 *
 * The properties worth pinning are the ones that are invisible when wrong:
 *
 * - **Fix on save is a CEILING, not a tier filter** (§6.2). "Safe only"
 *   must send `ceiling: "ask"`, so the project's own `[fix]` opinion keeps
 *   applying underneath the personal one. A `tiers: ["safe"]` selection
 *   would look identical in the happy case and quietly ignore a code the
 *   project turned `"off"`.
 * - **Off means no call at all.** Not "a call that returns nothing" —
 *   an implicit road that runs the batch on every Ctrl-S and merely
 *   discards the result is a cost the setting exists to avoid.
 * - **A hit round cap is reported.** §5: never silent.
 */
import { describe, it, expect, vi } from "vitest";
import { CommandRegistry } from "@brink/studio-shell";
import {
  DEFAULT_FIX_ON_SAVE,
  fixOnSaveCeiling,
  fixReportToStructuralResult,
  indexFixOffers,
  offersForDiagnostic,
  parseFixOnSave,
  runFixAll,
  runFixOnSave,
  safeSelect,
  summarizeFixReport,
  tierLabel,
  type FixStoreState,
} from "@brink/studio-ui";
import { registerFixCommands } from "../fix-commands";
import type {
  Diagnostic,
  FixOffer,
  FixReport,
  FixSelect,
  StructuralResult,
} from "@brink/wasm-types";

const OFFER: FixOffer = {
  code: "E025",
  path: "main.brink",
  start: 10,
  end: 16,
  batchable: false,
  fix: {
    code: "E025",
    title: "Import `haggle`",
    applicability: "suggested",
    edits: [{ path: "main.brink", start: 0, end: 0, new_text: "use x;\n" }],
  },
};

const DIAGNOSTIC: Diagnostic = {
  start: 10,
  end: 16,
  message: "`haggle` is not in scope",
  severity: "Error",
  code: "E025",
  file: "main.brink",
};

function emptyReport(over: Partial<FixReport> = {}): FixReport {
  return {
    applied: [],
    skipped_overlap: 0,
    remaining: [],
    rounds: 0,
    cap_hit: false,
    files: [],
    ...over,
  };
}

describe("offer lookup", () => {
  it("finds a diagnostic's offers by its own identity", () => {
    const index = indexFixOffers([OFFER]);
    expect(offersForDiagnostic(index, DIAGNOSTIC)).toEqual([OFFER]);
  });

  it("does not match a diagnostic that merely shares a code", () => {
    // Same code, different span — a second E025 elsewhere in the file must
    // not borrow this one's fix.
    const index = indexFixOffers([OFFER]);
    expect(offersForDiagnostic(index, { ...DIAGNOSTIC, start: 99, end: 105 })).toEqual([]);
  });

  it("returns nothing for a diagnostic with no code (a prose finding)", () => {
    const index = indexFixOffers([OFFER]);
    expect(offersForDiagnostic(index, { ...DIAGNOSTIC, code: undefined })).toEqual([]);
  });
});

describe("tier labels", () => {
  it("names each tier in the author's words, not the wire spelling", () => {
    expect(tierLabel("safe")).toBe("Safe");
    expect(tierLabel("suggested")).toBe("Suggested");
    expect(tierLabel("placeholder")).toBe("Needs input");
  });
});

describe("the report → write-list mapping", () => {
  it("is null when nothing was written", () => {
    expect(fixReportToStructuralResult(emptyReport())).toBeNull();
  });

  it("makes the first file the primary and the rest cross-file edits", () => {
    const result = fixReportToStructuralResult(
      emptyReport({
        files: [
          { path: "a.brink", new_source: "A" },
          { path: "b.brink", new_source: "B" },
        ],
      }),
    );
    expect(result).toEqual({
      ok: true,
      path: "a.brink",
      new_source: "A",
      cross_file_edits: [{ path: "b.brink", new_source: "B" }],
      safe: true,
      introduced_diagnostics: [],
    });
  });
});

describe("the report summary", () => {
  it("names a hit cap and what is still fixable — never silent (§5)", () => {
    const summary = summarizeFixReport(
      emptyReport({
        applied: [{ code: "E025", path: "main.brink" }],
        rounds: 5,
        cap_hit: true,
        remaining: [{ code: "E025", path: "main.brink" }],
      }),
    );
    expect(summary.severity).toBe("warning");
    expect(summary.message).toBe(
      "Applied 1 fix — stopped after 5 rounds with 1 still fixable",
    );
  });

  it("reports deferrals, which are why a second run does more", () => {
    const summary = summarizeFixReport(
      emptyReport({
        applied: [
          { code: "E025", path: "a.brink" },
          { code: "E025", path: "b.brink" },
        ],
        skipped_overlap: 2,
      }),
    );
    expect(summary.severity).toBe("info");
    expect(summary.message).toBe("Applied 2 fixes — 2 deferred by overlap");
  });

  it("says so plainly when there was nothing to do", () => {
    expect(summarizeFixReport(emptyReport()).message).toBe("No fixes to apply");
  });
});

describe("runFixAll", () => {
  function state(report: FixReport) {
    const selects: FixSelect[] = [];
    const descriptions: string[] = [];
    const applyMoveResult = vi.fn(
      async (_result: StructuralResult, description: string, _paths: string[]) => {
        descriptions.push(description);
      },
    );
    const notices: { severity: string; message: string }[] = [];
    const s: FixStoreState = {
      _project: {
        getSession: () => ({
          fixAll: (select: FixSelect) => {
            selects.push(select);
            return report;
          },
        }),
      },
      applyMoveResult,
      _notify: (n) => void notices.push({ severity: n.severity, message: n.message }),
    };
    return { s, selects, applyMoveResult, descriptions, notices };
  }

  it("runs the safe selection and applies through the undoable seam", async () => {
    const { s, selects, applyMoveResult, descriptions } = state(
      emptyReport({
        applied: [{ code: "E014", path: "main.brink" }],
        rounds: 1,
        files: [{ path: "main.brink", new_source: "fixed" }],
      }),
    );
    await runFixAll(s, safeSelect(), "Fix all safe");
    expect(selects).toEqual([{ tiers: ["safe"] }]);
    expect(applyMoveResult).toHaveBeenCalledTimes(1);
    expect(descriptions).toEqual(["Fix all safe"]);
  });

  it("reports a refused request instead of claiming success", async () => {
    const { s, applyMoveResult, notices } = state(
      emptyReport({ error: "the fix selection names a file that is not loaded" }),
    );
    await runFixAll(s, safeSelect("gone.brink"), "Fix all safe in gone.brink");
    expect(applyMoveResult).not.toHaveBeenCalled();
    expect(notices).toEqual([
      {
        severity: "error",
        message:
          "Fix all safe in gone.brink failed: the fix selection names a file that is not loaded",
      },
    ]);
  });

  it("says nothing was applied rather than staying silent", async () => {
    const { s, applyMoveResult, notices } = state(emptyReport());
    await runFixAll(s, safeSelect(), "Fix all safe");
    expect(applyMoveResult).not.toHaveBeenCalled();
    expect(notices).toEqual([{ severity: "info", message: "No fixes to apply" }]);
  });
});

describe("the fix commands", () => {
  function deps(activePath: string | null) {
    const selects: FixSelect[] = [];
    const notices: string[] = [];
    return {
      selects,
      notices,
      value: {
        getState: (): FixStoreState => ({
          _project: {
            getSession: () => ({
              fixAll: (select: FixSelect) => {
                selects.push(select);
                return emptyReport();
              },
            }),
          },
          applyMoveResult: async () => {},
          _notify: (n) => void notices.push(n.message),
        }),
        activePath: () => activePath,
        notify: (n: { message: string }) => void notices.push(n.message),
      },
    };
  }

  it("registers both palette entries", () => {
    const commands = new CommandRegistry();
    registerFixCommands(commands, deps("main.brink").value);
    const ids = commands.list().map((c) => c.id);
    expect(ids).toContain("fix.allSafeInProject");
    expect(ids).toContain("fix.allSafeInFile");
  });

  it("runs the whole compilation for the project command", () => {
    const commands = new CommandRegistry();
    const d = deps("main.brink");
    registerFixCommands(commands, d.value);
    commands.dispatch("fix.allSafeInProject");
    expect(d.selects).toEqual([{ tiers: ["safe"] }]);
  });

  it("scopes the file command to the focused file", () => {
    const commands = new CommandRegistry();
    const d = deps("chapter/two.brink");
    registerFixCommands(commands, d.value);
    commands.dispatch("fix.allSafeInFile");
    expect(d.selects).toEqual([{ tiers: ["safe"], path: "chapter/two.brink" }]);
  });

  it("refuses the file command with no editor focused, rather than widening", () => {
    // "in this file" with no file is not "in every file" — the failure mode
    // this guards is a palette command silently rewriting the project.
    const commands = new CommandRegistry();
    const d = deps(null);
    registerFixCommands(commands, d.value);
    commands.dispatch("fix.allSafeInFile");
    expect(d.selects).toEqual([]);
    expect(d.notices).toEqual(["No editor focused — nothing to fix"]);
  });
});

describe("fix on save (§6.2)", () => {
  it("defaults to off", () => {
    expect(DEFAULT_FIX_ON_SAVE).toBe("off");
    expect(parseFixOnSave(undefined)).toBe("off");
    expect(parseFixOnSave("everything")).toBe("off");
    expect(parseFixOnSave("safe")).toBe("safe");
    expect(parseFixOnSave("project")).toBe("project");
  });

  it("maps each mode to a CEILING, so the project's policy keeps applying", () => {
    expect(fixOnSaveCeiling("off")).toBeNull();
    // Not `tiers: ["safe"]`: a tier filter would ignore a code the project
    // turned "off", which the ceiling road honours.
    expect(fixOnSaveCeiling("safe")).toBe("ask");
    expect(fixOnSaveCeiling("project")).toBe("auto");
  });

  function saveDeps(report: FixReport) {
    const selects: FixSelect[] = [];
    const writes: [string, string][] = [];
    const invalidated: string[] = [];
    return {
      selects,
      writes,
      invalidated,
      value: {
        project: {
          getSession: () => ({
            fixAll: (select: FixSelect) => {
              selects.push(select);
              return report;
            },
          }),
        },
        applyEdit: (p: string, source: string) => {
          writes.push([p, source]);
          return true;
        },
        invalidate: (p: string) => void invalidated.push(p),
      },
    };
  }

  it("makes NO call at all when off", () => {
    const d = saveDeps(emptyReport({ files: [{ path: "main.brink", new_source: "x" }] }));
    expect(runFixOnSave(d.value, "main.brink", "off")).toEqual([]);
    expect(d.selects).toEqual([]);
    expect(d.writes).toEqual([]);
  });

  it("scopes the run to the saved file, at the mode's ceiling", () => {
    const d = saveDeps(emptyReport());
    runFixOnSave(d.value, "main.brink", "safe");
    expect(d.selects).toEqual([{ path: "main.brink", ceiling: "ask" }]);
    runFixOnSave(d.value, "main.brink", "project");
    expect(d.selects[1]).toEqual({ path: "main.brink", ceiling: "auto" });
  });

  it("writes what the batch produced and refreshes those views", () => {
    const d = saveDeps(
      emptyReport({
        applied: [{ code: "E014", path: "main.brink" }],
        rounds: 1,
        files: [
          { path: "main.brink", new_source: "fixed main" },
          { path: "other.brink", new_source: "fixed other" },
        ],
      }),
    );
    expect(runFixOnSave(d.value, "main.brink", "safe")).toEqual([
      "main.brink",
      "other.brink",
    ]);
    expect(d.writes).toEqual([
      ["main.brink", "fixed main"],
      ["other.brink", "fixed other"],
    ]);
    expect(d.invalidated).toEqual(["main.brink", "other.brink"]);
  });

  it("reports nothing written when a refused request comes back", () => {
    const d = saveDeps(
      emptyReport({
        error: "the fix selection names a file that is not loaded",
        files: [{ path: "main.brink", new_source: "never" }],
      }),
    );
    expect(runFixOnSave(d.value, "main.brink", "safe")).toEqual([]);
    expect(d.writes).toEqual([]);
  });

  it("never takes the save down when the batch throws", () => {
    const value = {
      project: {
        getSession: () => ({
          fixAll: () => {
            throw new Error("wasm exploded");
          },
        }),
      },
      applyEdit: () => true,
      invalidate: () => {},
    };
    expect(runFixOnSave(value, "main.brink", "safe")).toEqual([]);
  });

  it("skips a path the write seam refused (a mounted, read-only file)", () => {
    const invalidated: string[] = [];
    const value = {
      project: {
        getSession: () => ({
          fixAll: () =>
            emptyReport({ files: [{ path: "std/core.brink", new_source: "no" }] }),
        }),
      },
      applyEdit: () => false,
      invalidate: (p: string) => void invalidated.push(p),
    };
    expect(runFixOnSave(value, "main.brink", "project")).toEqual([]);
    expect(invalidated).toEqual([]);
  });
});
