// @vitest-environment jsdom
/**
 * Settings ▸ Updates, the pane.
 *
 * These cover what the model test cannot reach: that the policy is written
 * BEFORE the install is asked for (the shell re-resolves from disk, so the
 * other order installs the previous pin), that a blocked row cannot be
 * chosen, and that the automatic-checks switch is inert while pinned rather
 * than merely painted off.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { UpdateSettings, type UpdateSettingsApi } from "../UpdateSettings.js";
import type { BundleOffer, UpdatePolicy } from "../tauri-provider.js";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function offer(version: string, over: Partial<BundleOffer> = {}): BundleOffer {
  return {
    version,
    channel: "stable",
    minShellVersion: "0.8.0",
    pubDate: null,
    active: false,
    downloaded: false,
    blocked: null,
    ...over,
  };
}

/** Mount the pane and return the call log its api wrote into. */
async function mount(policy: UpdatePolicy, offers: BundleOffer[]) {
  const calls: string[] = [];
  const written: UpdatePolicy[] = [];
  const api: UpdateSettingsApi = {
    readPolicy: vi.fn(() => Promise.resolve(policy)),
    writePolicy: vi.fn((next: UpdatePolicy) => {
      calls.push(`write:${next.mode === "pinned" ? next.version : next.channel}`);
      written.push(next);
      return Promise.resolve();
    }),
    listVersions: vi.fn(() => Promise.resolve(offers)),
    applyCurrentPolicy: vi.fn(() => {
      calls.push("apply");
      return Promise.resolve();
    }),
    checkNow: vi.fn(() => {
      calls.push("check");
      return Promise.resolve();
    }),
  };

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root!.render(createElement(UpdateSettings, { api }));
  });
  return { calls, written, api };
}

/** The row whose title starts with `version`, as an element. */
function versionRow(version: string): HTMLElement {
  const found = [...container!.querySelectorAll(".settings-row")].find((el) =>
    el.querySelector(".settings-row-title")?.textContent?.startsWith(version),
  );
  expect(found, `no row for ${version}`).toBeDefined();
  return found as HTMLElement;
}

describe("picking a version", () => {
  it("writes the policy before asking the shell to install", async () => {
    // `bundle_update_apply` takes no version and re-resolves from the policy
    // on disk — that is the property that keeps it from installing something
    // the settings do not say. Applying first would install the PREVIOUS
    // pin, which is the one bug this order exists to prevent.
    const { calls } = await mount({ mode: "auto", channel: "stable" }, [
      offer("0.0.3"),
      offer("0.0.2", { active: true }),
    ]);

    const button = versionRow("0.0.3").querySelector("button");
    expect(button).not.toBeNull();
    await act(async () => {
      button!.click();
    });

    expect(calls).toEqual(["write:0.0.3", "apply"]);
  });

  it("cannot pick a version this shell refuses", async () => {
    const { calls } = await mount({ mode: "auto", channel: "stable" }, [
      offer("0.0.9", { blocked: "needs app version 0.9.0 or newer" }),
      offer("0.0.2", { active: true }),
    ]);

    const row = versionRow("0.0.9");
    const button = row.querySelector("button");
    expect(button?.disabled).toBe(true);
    // The refusal is stated in the row, not discovered after a download.
    expect(row.textContent).toContain("needs app version 0.9.0 or newer");
    expect(calls).toEqual([]);
  });

  it("offers no button for the version already running", async () => {
    await mount({ mode: "auto", channel: "stable" }, [offer("0.0.2", { active: true })]);
    const row = versionRow("0.0.2");
    expect(row.querySelector("button")?.disabled).toBe(true);
    expect(row.textContent).toContain("Running");
  });
});

describe("a pinned install", () => {
  it("cannot be switched back to automatic checking from this pane", async () => {
    // Not merely painted off: clicking it must not write a policy at all.
    // The bricked state this model refuses to represent is "pinned AND
    // auto" (RULED 2026-09-15).
    const { calls } = await mount({ mode: "pinned", version: "0.0.2" }, [
      offer("0.0.2", { active: true }),
    ]);

    const toggle = container!.querySelector<HTMLInputElement>("#brink-update-auto");
    expect(toggle).not.toBeNull();
    expect(toggle!.checked).toBe(false);
    await act(async () => {
      toggle!.click();
    });
    expect(calls).toEqual([]);
  });

  it("says why nothing is checking, rather than looking idle", async () => {
    await mount({ mode: "pinned", version: "0.0.2" }, [offer("0.0.2", { active: true })]);
    expect(container!.textContent).toContain("nothing to check for");
  });
});

describe("with nothing published", () => {
  it("cannot select the pinned channel", async () => {
    // Pinning to no version would stop updates with no version to show for
    // it. Disabled rather than hidden, so its absence is explainable.
    await mount({ mode: "auto", channel: "stable" }, []);
    const select = container!.querySelector<HTMLSelectElement>("#brink-update-channel");
    const pinned = [...select!.options].find((o) => o.value === "pinned");
    expect(pinned?.disabled).toBe(true);
  });

  it("says so instead of showing an empty list", async () => {
    await mount({ mode: "auto", channel: "stable" }, []);
    expect(container!.textContent).toContain("Nothing published yet");
  });
});

describe("when the index cannot be fetched", () => {
  it("reports it in the pane with a retry, and leaves the channel usable", async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    const api: UpdateSettingsApi = {
      readPolicy: () => Promise.resolve({ mode: "auto", channel: "stable" }),
      writePolicy: () => Promise.resolve(),
      listVersions: () => Promise.reject(new Error("fetch failed: offline")),
      applyCurrentPolicy: () => Promise.resolve(),
      checkNow: () => Promise.resolve(),
    };
    await act(async () => {
      root!.render(createElement(UpdateSettings, { api }));
    });

    expect(container.textContent).toContain("fetch failed: offline");
    // A failed LIST must not take the channel control with it — switching
    // stable/beta needs no index at all.
    expect(container.querySelector<HTMLSelectElement>("#brink-update-channel")).not.toBeNull();
  });
});
