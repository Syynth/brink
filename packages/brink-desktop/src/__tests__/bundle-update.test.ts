import { describe, expect, it } from "vitest";
import { bundleCheckNotice, bundleUpdateNotice } from "../bundle-update.js";
import type { BundleUpdateCheck, BundleUpdateOutcome } from "../tauri-provider.js";

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

describe("bundleCheckNotice", () => {
  /**
   * The load-bearing one. `available` means an offer is owed, and a notice
   * would announce something is happening and then not do it — the exact
   * confusion the Stage 4 consent split exists to remove. Returning null
   * makes a caller that forgets to handle it say NOTHING rather than
   * something wrong.
   */
  it("never turns an available update into a notice, silent or not", () => {
    const check: BundleUpdateCheck = { kind: "available", version: "0.0.2" };
    for (const silent of [true, false]) {
      expect(bundleCheckNotice(check, { silent }), `silent=${silent}`).toBeNull();
    }
  });

  /** A refusal is actionable, so it survives a silent (launch/focus) check —
   *  the same rule bundleUpdateNotice applies, and it must not diverge. */
  it("always reports a refusal, even silently", () => {
    const check: BundleUpdateCheck = { kind: "refused", reason: "needs a newer app" };
    for (const silent of [true, false]) {
      const notice = bundleCheckNotice(check, { silent });
      expect(notice?.severity, `silent=${silent}`).toBe("error");
      expect(notice?.message).toBe(check.reason);
    }
  });

  /** A launch check must not interrupt to say nothing happened; a manual one
   *  must, because a menu item that can do nothing visible is broken. */
  it("is silent about the boring outcomes only when asked to be", () => {
    for (const check of [
      { kind: "upToDate" } as const,
      { kind: "failed", reason: "offline" } as const,
    ]) {
      expect(bundleCheckNotice(check, { silent: true }), check.kind).toBeNull();
      expect(bundleCheckNotice(check, { silent: false }), check.kind).not.toBeNull();
    }
  });

  /** The two enums are separate types for a reason; the shared vocabulary
   *  between them must still agree, or the author sees the same situation
   *  described two ways depending on which call reported it. */
  it("words up-to-date identically to the install path", () => {
    expect(bundleCheckNotice({ kind: "upToDate" })?.message).toBe(
      bundleUpdateNotice({ kind: "upToDate" })?.message,
    );
  });
});
