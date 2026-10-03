import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_SRC_LANG,
  defaultXliffName,
  exportXliff,
  type ExportXliffApi,
} from "../export-xliff.js";
import type { ExportApi } from "../export.js";

/** A studio stub modelling the ASYNC compile landing (worker road, W4) the
 * same way `export.test.ts` does: `dispatch("compile.run")` swaps the
 * diagnostics object identity a microtask later — how `landCompileResult`
 * replaces it in the real store — which is the signal the shared
 * `compiledStoryBytes` awaits before reading the bytes. */
function stubStudio(
  storyBytes: Uint8Array | null,
  diagnostics = { errors: 0, warnings: 0 },
): { studio: ExportApi; notify: ReturnType<typeof vi.fn>; dispatch: ReturnType<typeof vi.fn> } {
  const notify = vi.fn();
  let current = { ...diagnostics };
  const dispatch = vi.fn(() => {
    queueMicrotask(() => {
      current = { ...current };
    });
    return true;
  });
  const studio: ExportApi = {
    dispatch,
    getStoryBytes: () => storyBytes,
    select: (sel) => sel({ diagnostics: current, projectDialect: null } as never),
    notify,
  };
  return { studio, notify, dispatch };
}

const XML = '<?xml version="1.0"?><xliff/>';

function stubApi(
  overrides: Partial<ExportXliffApi> = {},
  storyBytes: Uint8Array | null = new Uint8Array([1, 2, 3]),
  diagnostics = { errors: 0, warnings: 0 },
): ExportXliffApi & { notify: ReturnType<typeof vi.fn>; dispatch: ReturnType<typeof vi.fn> } {
  const { studio, notify, dispatch } = stubStudio(storyBytes, diagnostics);
  return {
    studio,
    toXliff: vi.fn(() => XML),
    saveBytes: vi.fn(async () => "/chosen/out.xlf"),
    notify,
    dispatch,
    ...overrides,
  };
}

describe("defaultXliffName", () => {
  it("strips a .brink extension from the basename", () => {
    expect(defaultXliffName("scenes/intro.brink")).toBe("intro.xlf");
  });

  it("strips a .ink extension from the basename", () => {
    expect(defaultXliffName("main.ink")).toBe("main.xlf");
  });

  it("keeps the basename unchanged when the extension isn't .ink/.brink", () => {
    expect(defaultXliffName("notes.txt")).toBe("notes.txt.xlf");
  });
});

describe("exportXliff", () => {
  it("warns and does nothing when no project is open", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const api = stubApi();
    await exportXliff(null, api);
    expect(warn).toHaveBeenCalled();
    expect(api.dispatch).not.toHaveBeenCalled();
    expect(api.toXliff).not.toHaveBeenCalled();
    expect(api.saveBytes).not.toHaveBeenCalled();
    warn.mockRestore();
  });

  /** The point of the whole flow: what leaves the app is the COMPILED
   * artifact's line tables, produced by the same wasm that compiled it —
   * not a subprocess re-reading source off disk. */
  it("compiles via compile.run and renders those exact bytes as XLIFF", async () => {
    const bytes = new Uint8Array([9, 8, 7]);
    const api = stubApi({}, bytes);
    await exportXliff({ entryFile: "scenes/intro.brink" }, api);
    expect(api.dispatch).toHaveBeenCalledWith("compile.run");
    expect(api.toXliff).toHaveBeenCalledWith(bytes, DEFAULT_SRC_LANG);
  });

  it("writes the rendered XML as UTF-8 under the entry file's name", async () => {
    const api = stubApi();
    await exportXliff({ entryFile: "scenes/intro.brink" }, api);
    const saveBytes = api.saveBytes as ReturnType<typeof vi.fn>;
    expect(saveBytes.mock.calls[0][0]).toBe("intro.xlf");
    expect(new TextDecoder().decode(saveBytes.mock.calls[0][1] as Uint8Array)).toBe(XML);
  });

  it("notifies info with the chosen path", async () => {
    const api = stubApi();
    await exportXliff({ entryFile: "story.brink" }, api);
    expect(api.notify).toHaveBeenCalledWith({
      severity: "info",
      source: "export-xliff",
      message: "Exported XLIFF to /chosen/out.xlf",
    });
  });

  it("cancels cleanly when the save dialog is dismissed", async () => {
    const api = stubApi({ saveBytes: vi.fn(async () => null) });
    await exportXliff({ entryFile: "story.brink" }, api);
    expect(api.notify).not.toHaveBeenCalled();
  });

  /** A compile that produced no bytes must not reach the wasm binding at
   * all — and must say why, under this flow's own source tag rather than
   * Export Story's. */
  it("reports a failed compile and never renders", async () => {
    const api = stubApi({}, null, { errors: 3, warnings: 0 });
    await exportXliff({ entryFile: "story.brink" }, api);
    expect(api.toXliff).not.toHaveBeenCalled();
    expect(api.saveBytes).not.toHaveBeenCalled();
    expect(api.notify).toHaveBeenCalledWith({
      severity: "error",
      source: "export-xliff",
      message: "Export XLIFF failed: 3 compile error(s) — fix them and try again.",
    });
  });

  /** The wasm binding throws a `JsError` for an unreadable artifact; that
   * must surface as a notification, not an unhandled rejection. */
  it("notifies error when the binding throws", async () => {
    const api = stubApi({
      toXliff: vi.fn(() => {
        throw new Error("decode error: unsupported container version 9");
      }),
    });
    await exportXliff({ entryFile: "story.brink" }, api);
    expect(api.notify).toHaveBeenCalledWith({
      severity: "error",
      source: "export-xliff",
      message: "Export XLIFF failed: decode error: unsupported container version 9",
    });
  });

  it("notifies error when the save round trip rejects", async () => {
    const api = stubApi({
      saveBytes: vi.fn().mockRejectedValue(new Error("permission denied")),
    });
    await exportXliff({ entryFile: "story.brink" }, api);
    expect(api.notify).toHaveBeenCalledWith({
      severity: "error",
      source: "export-xliff",
      message: "Export XLIFF failed: permission denied",
    });
  });
});
