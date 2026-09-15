/**
 * The unified update flow: one update from the author's side, whichever
 * channel carries it (`docs/desktop-ota-spec.md` Stage 4).
 *
 * These assert on WORDING as well as outcome, deliberately. The ruling is
 * about what the author can perceive, so a test that only checked control
 * flow would pass while the mechanism leaked through a message.
 */

import { describe, expect, it, vi } from "vitest";
import { checkForAnyUpdate, type UnifiedUpdateApi, type UpdateNotice } from "../update-flow.js";
import type { BundleUpdateCheck, UpdatePolicy } from "../tauri-provider.js";

function harness(overrides: Partial<UnifiedUpdateApi> = {}) {
  const notices: UpdateNotice[] = [];
  const offers: string[] = [];
  const calls: string[] = [];
  const api: UnifiedUpdateApi = {
    policy: async () => ({ mode: "auto", channel: "stable" }) as UpdatePolicy,
    checkShell: async () => null,
    checkBundle: async () => ({ kind: "upToDate" }) as BundleUpdateCheck,
    applyBundle: async () => {
      calls.push("applyBundle");
      return { kind: "installed", version: "0.0.2" };
    },
    activateBundle: async () => void calls.push("activateBundle"),
    relaunch: async () => void calls.push("relaunch"),
    awaitSave: async () => void calls.push("awaitSave"),
    confirm: async (message) => {
      offers.push(message);
      return true;
    },
    notify: (notice) => void notices.push(notice),
    ...overrides,
  };
  return { api, notices, offers, calls };
}

const shellUpdate = (version: string) => ({
  version,
  downloadAndInstall: async () => {},
});

describe("checkForAnyUpdate", () => {
  /**
   * The rule, stated as a test. Both channels having something is the case
   * where "one update" is easiest to break — the obvious implementation
   * offers two toasts.
   */
  it("offers exactly one update when both channels have one", async () => {
    const { api, offers } = harness({
      checkShell: async () => shellUpdate("0.9.0"),
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
    });

    await checkForAnyUpdate(api);

    expect(offers).toHaveLength(1);
  });

  /**
   * And the shell wins, for a reason: it may raise minShellVersion, making
   * a bundle installable where it was not, and restarting into it means the
   * next check picks the bundle up anyway.
   */
  it("prefers the shell update when both are available", async () => {
    const { api, calls } = harness({
      checkShell: async () => shellUpdate("0.9.0"),
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
    });

    await checkForAnyUpdate(api);

    expect(calls).toContain("relaunch");
    expect(calls).not.toContain("applyBundle");
  });

  /** A bundle offer must not name a version: the bundle's sequence is
   *  independent of the app's, so naming it leaks the mechanism AND reads
   *  as a downgrade (0.0.2 while the app says 0.8.0). */
  it("never names the bundle version in an offer", async () => {
    const { api, offers } = harness({
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
    });

    await checkForAnyUpdate(api);

    expect(offers).toEqual(["An update is available."]);
    expect(offers[0]).not.toContain("0.0.2");
  });

  /** Nothing the author sees may name the mechanism. */
  it("never says bundle or shell in anything it shows", async () => {
    for (const bundle of [
      { kind: "refused", reason: "update 0.0.3 needs app version 0.9.0 or newer" },
      { kind: "failed", reason: "offline" },
      { kind: "upToDate" },
    ] as BundleUpdateCheck[]) {
      const { api, notices, offers } = harness({ checkBundle: async () => bundle });
      await checkForAnyUpdate(api, { silent: false });
      for (const text of [...notices.map((n) => n.message), ...offers]) {
        expect(text.toLowerCase(), text).not.toMatch(/\bbundle\b|\bshell\b|\bota\b/);
      }
    }
  });

  /** Both channels discarding the document must save first. A reload
   *  destroys the page and every worker with it, exactly as a relaunch
   *  does — cheaper than a restart, not free. */
  it("saves before activating a bundle, not only before relaunching", async () => {
    const { api, calls } = harness({
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
    });

    await checkForAnyUpdate(api);

    expect(calls).toEqual(["applyBundle", "awaitSave", "activateBundle"]);
  });

  /** A pinned install reports the PIN rather than "up to date" — the button
   *  still does something honest, it just does not offer a cliff. */
  it("reports the pin instead of offering, and never checks", async () => {
    const checkBundle = vi.fn(async (): Promise<BundleUpdateCheck> => ({ kind: "upToDate" }));
    const { api, notices, offers } = harness({
      policy: async () => ({ mode: "pinned", version: "0.0.4" }),
      checkBundle,
    });

    const result = await checkForAnyUpdate(api, { silent: false });

    expect(result).toBe("pinned");
    expect(checkBundle).not.toHaveBeenCalled();
    expect(offers).toHaveLength(0);
    expect(notices[0]?.message).toContain("0.0.4");
    expect(notices[0]?.severity).toBe("info");
  });

  /** A refusal survives a silent check: it is the one outcome the author
   *  can act on, and "up to date" would leave them waiting forever. */
  it("always reports a refusal, even on an automatic check", async () => {
    for (const silent of [true, false]) {
      const { api, notices } = harness({
        checkBundle: async () => ({ kind: "refused", reason: "needs a newer app" }),
      });
      const result = await checkForAnyUpdate(api, { silent });
      expect(result, `silent=${silent}`).toBe("refused");
      expect(notices).toHaveLength(1);
      expect(notices[0].severity).toBe("error");
    }
  });

  /** The boring outcomes stay quiet on launch and focus, and speak when
   *  asked — a menu item that can do nothing visible is broken. */
  it("is silent about up-to-date only when asked to be", async () => {
    const quiet = harness();
    expect(await checkForAnyUpdate(quiet.api, { silent: true })).toBe("none");
    expect(quiet.notices).toHaveLength(0);

    const loud = harness();
    expect(await checkForAnyUpdate(loud.api, { silent: false })).toBe("none");
    expect(loud.notices).toHaveLength(1);
  });

  /** Manual means "ask me". An automatic check in that mode is the thing
   *  the author switched off. */
  it("does not run an automatic check under a manual policy", async () => {
    const checkBundle = vi.fn(async (): Promise<BundleUpdateCheck> => ({ kind: "upToDate" }));
    const { api } = harness({
      policy: async () => ({ mode: "manual", channel: "stable" }),
      checkBundle,
    });

    expect(await checkForAnyUpdate(api, { silent: true })).toBe("none");
    expect(checkBundle).not.toHaveBeenCalled();

    await checkForAnyUpdate(api, { silent: false });
    expect(checkBundle).toHaveBeenCalled();
  });

  /** Declining installs nothing and discards nothing. */
  it("installs nothing when the author declines", async () => {
    const { api, calls } = harness({
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
      confirm: async () => false,
    });

    expect(await checkForAnyUpdate(api)).toBe("declined");
    expect(calls).toEqual([]);
  });

  /** Installed-but-not-activated is a real state, not a failure: the next
   *  launch serves it. Saying nothing would imply nothing happened. */
  it("reports an install whose activation failed as installed", async () => {
    const { api, notices } = harness({
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
      activateBundle: async () => {
        throw new Error("window is gone");
      },
    });

    expect(await checkForAnyUpdate(api)).toBe("installed");
    expect(notices[0]?.severity).toBe("info");
    expect(notices[0]?.message).toContain("next time you open");
  });

  /** A settings read that fails must not stop an update. */
  it("falls back to the conservative policy when settings cannot be read", async () => {
    const { api, offers } = harness({
      policy: async () => {
        throw new Error("settings.json is unreadable");
      },
      checkBundle: async () => ({ kind: "available", version: "0.0.2" }),
    });

    expect(await checkForAnyUpdate(api)).toBe("installed");
    expect(offers).toHaveLength(1);
  });
});
