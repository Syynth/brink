import { describe, expect, it } from "vitest";
import { bundleUpdateNotice } from "../bundle-update.js";
import type { BundleUpdateOutcome } from "../tauri-provider.js";

describe("bundleUpdateNotice", () => {
  /**
   * The distinction the whole `minShellVersion` gate exists to surface: a
   * refusal is something the author can act on, so it must never be silent
   * and must never read as "up to date" — that would leave them waiting for
   * an update that can never apply.
   */
  it("always reports a refusal, with the shell's own reason, even on a silent check", () => {
    const outcome: BundleUpdateOutcome = {
      kind: "refused",
      reason: "update 0.9.0 needs app version 0.9.0 or newer; this app is 0.7.0.",
    };
    for (const silent of [true, false]) {
      const notice = bundleUpdateNotice(outcome, { silent });
      expect(notice, `silent=${silent}`).not.toBeNull();
      expect(notice?.severity).toBe("error");
      expect(notice?.message).toBe(outcome.reason);
    }
  });

  /**
   * Installing changed something on disk, so it is reported even on an
   * automatic check — and the message has to say activation is deferred, or
   * the author goes looking for a change that is not there yet.
   */
  it("always reports an install and says it applies next launch", () => {
    for (const silent of [true, false]) {
      const notice = bundleUpdateNotice({ kind: "installed", version: "0.7.2" }, { silent });
      expect(notice?.severity).toBe("info");
      expect(notice?.message).toContain("0.7.2");
      expect(notice?.message.toLowerCase()).toContain("next time");
    }
  });

  /** The boring outcomes are what `silent` is for. */
  it("is silent about up-to-date and failure only on an automatic check", () => {
    expect(bundleUpdateNotice({ kind: "upToDate" }, { silent: true })).toBeNull();
    expect(bundleUpdateNotice({ kind: "failed", reason: "offline" }, { silent: true })).toBeNull();

    expect(bundleUpdateNotice({ kind: "upToDate" }, { silent: false })?.severity).toBe("info");
    const failed = bundleUpdateNotice({ kind: "failed", reason: "offline" }, { silent: false });
    expect(failed?.severity).toBe("error");
    expect(failed?.message).toContain("offline");
  });

  /** Defaults to loud: a caller that forgets the option gets the manual
   * behaviour, which is the safe direction for a menu item. */
  it("defaults to reporting everything", () => {
    expect(bundleUpdateNotice({ kind: "upToDate" })).not.toBeNull();
  });

  /** A refusal is the gate working; a failure is the channel broken.
   * Collapsing them teaches the author to ignore both. */
  it("distinguishes a refusal from a failure", () => {
    const refused = bundleUpdateNotice({ kind: "refused", reason: "needs a newer app" });
    const failed = bundleUpdateNotice({ kind: "failed", reason: "needs a newer app" });
    expect(refused?.message).not.toBe(failed?.message);
    expect(failed?.message).toContain("failed");
  });
});
