import { describe, expect, it, vi } from "vitest";
import {
  defaultXliffName,
  exportXliff,
  type ExportXliffApi,
} from "../export-xliff.js";

function stubApi(overrides: Partial<ExportXliffApi> = {}): ExportXliffApi {
  return {
    runCli: vi.fn().mockResolvedValue(0),
    save: vi.fn().mockResolvedValue("/chosen/out.xlf"),
    notify: vi.fn(),
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

  it("falls back to story.xlf for an unrecognized shape", () => {
    expect(defaultXliffName("")).toBe("story.xlf");
  });
});

describe("exportXliff", () => {
  it("warns and does nothing when no project is open", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const api = stubApi();
    await exportXliff(null, api);
    expect(warn).toHaveBeenCalled();
    expect(api.save).not.toHaveBeenCalled();
    expect(api.runCli).not.toHaveBeenCalled();
    warn.mockRestore();
  });

  it("cancels cleanly when the save dialog is dismissed", async () => {
    const api = stubApi({ save: vi.fn().mockResolvedValue(null) });
    await exportXliff({ root: "/proj", entryFile: "story.brink" }, api);
    expect(api.runCli).not.toHaveBeenCalled();
    expect(api.notify).not.toHaveBeenCalled();
  });

  it("runs export-xliff with the project root/entry and the chosen output path", async () => {
    const api = stubApi();
    await exportXliff({ root: "/proj", entryFile: "scenes/intro.brink" }, api);
    expect(api.save).toHaveBeenCalledWith({
      defaultPath: "intro.xlf",
      filters: [{ name: "XLIFF", extensions: ["xlf"] }],
    });
    expect(api.runCli).toHaveBeenCalledWith({
      root: "/proj",
      rel: "scenes/intro.brink",
      subcommand: "export-xliff",
      rest: ["--output", "/chosen/out.xlf"],
    });
  });

  it("notifies info on a zero exit code", async () => {
    const api = stubApi();
    await exportXliff({ root: "/proj", entryFile: "story.brink" }, api);
    expect(api.notify).toHaveBeenCalledWith({
      severity: "info",
      source: "cli",
      message: "Exported XLIFF to /chosen/out.xlf",
    });
  });

  it("notifies error on a non-zero exit code", async () => {
    const api = stubApi({ runCli: vi.fn().mockResolvedValue(2) });
    await exportXliff({ root: "/proj", entryFile: "story.brink" }, api);
    expect(api.notify).toHaveBeenCalledWith({
      severity: "error",
      source: "cli",
      message: "export-xliff exited with code 2",
    });
  });

  it("notifies error when runCli rejects", async () => {
    const api = stubApi({ runCli: vi.fn().mockRejectedValue(new Error("sidecar spawn failed")) });
    await exportXliff({ root: "/proj", entryFile: "story.brink" }, api);
    expect(api.notify).toHaveBeenCalledWith({
      severity: "error",
      source: "cli",
      message: "export-xliff failed: sidecar spawn failed",
    });
  });
});
