import { describe, expect, it, vi } from "vitest";
import { defaultExportName, exportStoryToInkb, type ExportApi } from "../export.js";

/** An `ExportApi` stub: `dispatch` triggers nothing on its own — the test
 * sets `storyBytes`/`diagnostics` directly to model what a real
 * `compile.run` dispatch would have already produced synchronously in the
 * store by the time it returns. */
function stubApi(storyBytes: Uint8Array | null, diagnostics = { errors: 0, warnings: 0 }): {
  api: ExportApi;
  notify: ReturnType<typeof vi.fn>;
  dispatch: ReturnType<typeof vi.fn>;
} {
  const notify = vi.fn();
  const dispatch = vi.fn(() => true);
  const api: ExportApi = {
    dispatch,
    getStoryBytes: () => storyBytes,
    select: (sel) => sel({ diagnostics } as never),
    notify,
  };
  return { api, notify, dispatch };
}

describe("defaultExportName", () => {
  it("derives <project-folder>.inkb from the root path", () => {
    expect(defaultExportName("/Users/ben/projects/my-story")).toBe("my-story.inkb");
  });

  it("falls back to the whole root when there is no path separator", () => {
    expect(defaultExportName("my-story")).toBe("my-story.inkb");
  });
});

describe("exportStoryToInkb", () => {
  it("compiles via compile.run, then hands the bytes to the save dialog", async () => {
    const bytes = new Uint8Array([1, 2, 3]);
    const { api, dispatch, notify } = stubApi(bytes);
    const saveDialog = vi.fn(async () => "/tmp/my-story.inkb");

    await exportStoryToInkb(api, "/projects/my-story", saveDialog);

    expect(dispatch).toHaveBeenCalledWith("compile.run");
    expect(saveDialog).toHaveBeenCalledWith("my-story.inkb", bytes);
    expect(notify).not.toHaveBeenCalled();
  });

  it("notifies an error and never opens the dialog when the compile failed", async () => {
    // Regression guard: revert the `bytes === null` early-return in
    // export.ts and this test goes red — saveDialog gets called with an
    // empty/undefined artifact instead of being skipped.
    const { api, notify } = stubApi(null, { errors: 2, warnings: 1 });
    const saveDialog = vi.fn(async () => "/tmp/x.inkb");

    await exportStoryToInkb(api, "/projects/my-story", saveDialog);

    expect(saveDialog).not.toHaveBeenCalled();
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ severity: "error", source: "export" }),
    );
    expect(notify.mock.calls[0]?.[0].message).toContain("2 compile error(s)");
  });

  it("notifies an error when the save dialog itself rejects", async () => {
    const bytes = new Uint8Array([9]);
    const { api, notify } = stubApi(bytes);
    const saveDialog = vi.fn(async () => {
      throw new Error("disk full");
    });

    await exportStoryToInkb(api, "/projects/my-story", saveDialog);

    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ severity: "error", source: "export" }),
    );
    expect(notify.mock.calls[0]?.[0].message).toContain("disk full");
  });

  it("does not throw when the user cancels the dialog (null resolve)", async () => {
    const bytes = new Uint8Array([9]);
    const { api, notify } = stubApi(bytes);
    const saveDialog = vi.fn(async () => null);

    await expect(exportStoryToInkb(api, "/projects/my-story", saveDialog)).resolves.toBeUndefined();
    expect(notify).not.toHaveBeenCalled();
  });
});
