/**
 * Update offer as a toast, not a modal (beta feedback 2026-08-25: make the
 * update flow "a little more native feeling").
 *
 * The decision tree in `updater.ts` is deliberately UNCHANGED — `confirm`
 * still returns a promise of a boolean. What changed is who answers it: a
 * sticky notification carrying two actions, settled by whichever command
 * the author's click dispatches. These tests pin that contract at the seam
 * `checkForUpdates` actually sees, so the tree's own tests keep covering
 * the tree and these cover the surface.
 */

import { describe, it, expect, vi } from "vitest";
import {
  AUTO_CHECK_INTERVAL_MS,
  checkForUpdates,
  shouldAutoCheck,
  type PendingUpdate,
  type UpdateApi,
} from "../updater.js";
import {
  UPDATE_INSTALL_COMMAND,
  UPDATE_LATER_COMMAND,
} from "../update-commands.js";

/** The half of main.tsx's wiring under test, restated: an offer parks a
 *  resolver, and command dispatch settles it exactly once. */
function offerHarness() {
  let pending: ((accepted: boolean) => void) | null = null;
  const raised: Array<{ id?: string; timeoutMs?: number; actions?: string[]; message: string }> =
    [];

  const settle = (accepted: boolean): void => {
    const resolve = pending;
    pending = null;
    resolve?.(accepted);
  };

  const confirm = (version: string): Promise<boolean> => {
    // A second offer replaces the first; the older promise must not hang.
    settle(false);
    return new Promise<boolean>((resolve) => {
      pending = resolve;
      raised.push({
        id: "update",
        timeoutMs: 0,
        actions: [UPDATE_INSTALL_COMMAND, UPDATE_LATER_COMMAND],
        message: `Brink Studio ${version} is available.`,
      });
    });
  };

  return {
    confirm,
    raised,
    hasPendingOffer: () => pending !== null,
    click: (action: "install" | "later") => settle(action === "install"),
  };
}

function api(over: Partial<UpdateApi> = {}): { api: UpdateApi; notes: Array<[string, string]> } {
  const notes: Array<[string, string]> = [];
  return {
    api: {
      check: vi.fn(async () => null),
      confirm: vi.fn(async () => true),
      notify: (severity, message) => void notes.push([severity, message]),
      awaitSave: vi.fn(async () => {}),
      relaunch: vi.fn(async () => {}),
      ...over,
    },
    notes,
  };
}

function pendingUpdate(version = "0.3.5", onInstall?: () => void): PendingUpdate {
  return { version, downloadAndInstall: vi.fn(async () => void onInstall?.()) };
}

describe("update offer toast", () => {
  it("raises ONE sticky toast with both actions instead of blocking", async () => {
    const offer = offerHarness();
    const { api: a } = api({
      check: async () => pendingUpdate("0.3.5"),
      confirm: offer.confirm,
    });

    const run = checkForUpdates(a);
    // The check has not resolved: the flow is parked on the author, but
    // nothing is blocking — the toast is just sitting there.
    await Promise.resolve();
    expect(offer.raised).toHaveLength(1);
    expect(offer.raised[0]?.message).toContain("0.3.5");
    // Sticky (<= 0), because an offer that times out while you read it is
    // worse than no offer.
    expect(offer.raised[0]?.timeoutMs).toBe(0);
    expect(offer.raised[0]?.actions).toEqual([UPDATE_INSTALL_COMMAND, UPDATE_LATER_COMMAND]);

    offer.click("install");
    expect(await run).toBe("installed");
  });

  it("Later declines without installing, and settles the promise", async () => {
    const offer = offerHarness();
    const install = vi.fn();
    const { api: a } = api({
      check: async () => pendingUpdate("0.3.5", install),
      confirm: offer.confirm,
    });

    const run = checkForUpdates(a);
    await Promise.resolve();
    offer.click("later");

    expect(await run).toBe("declined");
    expect(install).not.toHaveBeenCalled();
    expect(offer.hasPendingOffer()).toBe(false);
  });

  it("installing saves BEFORE relaunching", async () => {
    const offer = offerHarness();
    const order: string[] = [];
    const { api: a } = api({
      check: async () => pendingUpdate("0.3.5", () => order.push("install")),
      confirm: offer.confirm,
      awaitSave: async () => void order.push("save"),
      relaunch: async () => void order.push("relaunch"),
    });

    const run = checkForUpdates(a);
    await Promise.resolve();
    offer.click("install");
    await run;

    expect(order).toEqual(["install", "save", "relaunch"]);
  });

  it("a second check replaces the offer and never leaves the first hanging", async () => {
    const offer = offerHarness();
    const { api: a } = api({
      check: async () => pendingUpdate("0.3.5"),
      confirm: offer.confirm,
    });

    const first = checkForUpdates(a);
    await Promise.resolve();
    const second = checkForUpdates(a);
    await Promise.resolve();

    // The first offer settled as declined the moment the second replaced it.
    expect(await first).toBe("declined");
    expect(offer.raised).toHaveLength(2);

    offer.click("later");
    expect(await second).toBe("declined");
  });

  it("still reports outcomes for a manual check, and stays silent on a launch check", async () => {
    const up = api();
    expect(await checkForUpdates(up.api)).toBe("none");
    expect(up.notes).toEqual([["info", "Brink Studio is up to date."]]);

    const silent = api();
    expect(await checkForUpdates(silent.api, { silent: true })).toBe("none");
    expect(silent.notes).toEqual([]);
  });

  it("an install failure is reported (never silent — the author consented)", async () => {
    const offer = offerHarness();
    const failing: PendingUpdate = {
      version: "0.3.5",
      downloadAndInstall: async () => {
        throw new Error("disk full");
      },
    };
    const { api: a, notes } = api({ check: async () => failing, confirm: offer.confirm });

    const run = checkForUpdates(a, { silent: true });
    await Promise.resolve();
    offer.click("install");

    expect(await run).toBe("failed");
    expect(notes[0]?.[0]).toBe("error");
    expect(notes[0]?.[1]).toContain("disk full");
  });
});

// ── Automatic checks: launch + window focus (ruled 2026-08-25) ───────

describe("shouldAutoCheck", () => {
  const HOUR = 60 * 60 * 1000;

  it("runs the very first time (nothing has ever been checked)", () => {
    expect(shouldAutoCheck({ lastCheckAt: 0, offerPending: false, now: 1_000 })).toBe(true);
  });

  it("declines while an offer is already on screen", () => {
    // Re-raising the same toast under the author's cursor is churn, and
    // replacing it would settle the live promise as declined.
    expect(shouldAutoCheck({ lastCheckAt: 0, offerPending: true, now: 1_000 })).toBe(false);
  });

  it("throttles bursts of focus events", () => {
    const last = 10 * HOUR;
    // Alt-tabbing back a minute later must not hit the update server.
    expect(
      shouldAutoCheck({ lastCheckAt: last, offerPending: false, now: last + 60_000 }),
    ).toBe(false);
    expect(
      shouldAutoCheck({ lastCheckAt: last, offerPending: false, now: last + 3 * HOUR }),
    ).toBe(false);
  });

  it("runs again once the interval has elapsed", () => {
    const last = 10 * HOUR;
    expect(
      shouldAutoCheck({
        lastCheckAt: last,
        offerPending: false,
        now: last + AUTO_CHECK_INTERVAL_MS,
      }),
    ).toBe(true);
  });

  it("takes an explicit interval, so the policy is tunable without editing it", () => {
    expect(
      shouldAutoCheck({ lastCheckAt: 1_000, offerPending: false, now: 2_000, intervalMs: 500 }),
    ).toBe(true);
  });

  it("is far below release cadence and far above window-switch cadence", () => {
    // Guards the constant against a careless edit: minutes would hammer the
    // server, days would make focus checks pointless.
    expect(AUTO_CHECK_INTERVAL_MS).toBeGreaterThanOrEqual(HOUR);
    expect(AUTO_CHECK_INTERVAL_MS).toBeLessThanOrEqual(24 * HOUR);
  });
});
