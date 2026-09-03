/**
 * `systemFonts()` (#3439): the shell's font list reaches the studio as
 * family names; a failed or malformed reply is an empty list, never a
 * throw — the studio falls back to its curated list.
 */
import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn<(cmd: string) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (cmd: string) => invoke(cmd) }));

import { systemFonts } from "../system-fonts";

describe("systemFonts", () => {
  it("returns the shell's families", async () => {
    invoke.mockResolvedValueOnce(["Baskerville", "Menlo"]);
    expect(await systemFonts()).toEqual(["Baskerville", "Menlo"]);
    expect(invoke).toHaveBeenCalledWith("list_system_fonts");
  });

  it("drops non-strings and survives a failed command", async () => {
    invoke.mockResolvedValueOnce(["Menlo", 3, null]);
    expect(await systemFonts()).toEqual(["Menlo"]);
    invoke.mockRejectedValueOnce(new Error("no shell"));
    expect(await systemFonts()).toEqual([]);
  });
});
