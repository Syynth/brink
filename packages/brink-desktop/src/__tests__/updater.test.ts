/**
 * Update check/install decision tree (D4). Every capability is injected, so
 * these run with no Tauri runtime, no network, and no update server — the
 * same testability seam `quit.test.ts` gets from `QuitSaveApi`.
 */

import { describe, it, expect, vi } from "vitest";
import { checkForUpdates, type PendingUpdate, type UpdateApi } from "../updater.js";

function stubApi(overrides: Partial<UpdateApi> = {}) {
  const notes: Array<[string, string]> = [];
  const order: string[] = [];
  const api: UpdateApi = {
    check: vi.fn(async () => null),
    confirm: vi.fn(async () => true),
    notify: (severity, message) => void notes.push([severity, message]),
    awaitSave: vi.fn(async () => void order.push("save")),
    relaunch: vi.fn(async () => void order.push("relaunch")),
    ...overrides,
  };
  return { api, notes, order };
}

function pending(version = "0.2.0", onInstall?: () => void): PendingUpdate {
  return {
    version,
    downloadAndInstall: vi.fn(async () => void onInstall?.()),
  };
}

describe("checkForUpdates", () => {
  it("says nothing on a silent launch check when already current", async () => {
    const { api, notes } = stubApi();
    expect(await checkForUpdates(api, { silent: true })).toBe("none");
    expect(notes).toEqual([]);
  });

  it("reports the up-to-date case on a MANUAL check — a button that can do nothing visible is broken", async () => {
    const { api, notes } = stubApi();
    expect(await checkForUpdates(api)).toBe("none");
    expect(notes).toEqual([["info", "Brink Studio is up to date."]]);
  });

  it("declining installs nothing and never relaunches", async () => {
    const update = pending();
    const { api, order } = stubApi({
      check: async () => update,
      confirm: async () => false,
    });
    expect(await checkForUpdates(api)).toBe("declined");
    expect(update.downloadAndInstall).not.toHaveBeenCalled();
    expect(order).toEqual([]);
  });

  it("SAVES BEFORE RELAUNCHING — the whole point of the ruling", async () => {
    const order: string[] = [];
    const update = pending("0.3.0", () => void order.push("install"));
    const api: UpdateApi = {
      check: async () => update,
      confirm: async () => true,
      notify: () => {},
      awaitSave: async () => void order.push("save"),
      relaunch: async () => void order.push("relaunch"),
    };
    expect(await checkForUpdates(api)).toBe("installed");
    expect(order).toEqual(["install", "save", "relaunch"]);
  });

  it("a failed check is silent on launch but reported when asked for", async () => {
    const boom = { check: async () => { throw new Error("offline"); } };
    const silent = stubApi(boom);
    expect(await checkForUpdates(silent.api, { silent: true })).toBe("failed");
    expect(silent.notes).toEqual([]);

    const manual = stubApi(boom);
    expect(await checkForUpdates(manual.api)).toBe("failed");
    expect(manual.notes[0][0]).toBe("error");
    expect(manual.notes[0][1]).toContain("offline");
  });

  it("a failed INSTALL is always reported, even on a silent launch check, and never relaunches", async () => {
    const update: PendingUpdate = {
      version: "0.4.0",
      downloadAndInstall: async () => { throw new Error("disk full"); },
    };
    const { api, notes, order } = stubApi({ check: async () => update });
    expect(await checkForUpdates(api, { silent: true })).toBe("failed");
    expect(notes[0][0]).toBe("error");
    expect(notes[0][1]).toContain("disk full");
    expect(order).toEqual([]); // no save, no relaunch — nothing was staged
  });
});
