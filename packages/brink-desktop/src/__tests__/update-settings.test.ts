/**
 * The update-policy transitions.
 *
 * A policy editor's bugs are all transitions: the state you can reach that
 * the model says cannot exist, and the setting silently dropped on the way
 * to another one. So these assert the moves, not the rendering.
 */
import { describe, expect, it } from "vitest";
import type { BundleOffer } from "../tauri-provider.js";
import {
  channelOf,
  checksAutomatically,
  defaultPinTarget,
  formatPubDate,
  offerStatus,
  pinnedVersion,
  withAutomatic,
  withChannel,
} from "../update-settings.js";

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

describe("a pinned install cannot also be on auto-update", () => {
  it("reports a pinned policy as not checking automatically", () => {
    // RULED: "you can't be pinned and on auto-update, that doesn't make
    // sense." Reporting the previous value here would paint a switch as ON
    // over a policy that never checks.
    expect(checksAutomatically({ mode: "pinned", version: "0.0.2" })).toBe(false);
    expect(channelOf({ mode: "pinned", version: "0.0.2" })).toBe("pinned");
  });

  it("ignores a request to turn automatic checking on while pinned", () => {
    // The control is disabled there. A call that got through anyway — a
    // keyboard path, a future caller — must not produce a policy the model
    // says cannot exist.
    const pinned = { mode: "pinned", version: "0.0.2" } as const;
    expect(withAutomatic(pinned, true)).toEqual(pinned);
    expect(withAutomatic(pinned, false)).toEqual(pinned);
  });

  it("moving to the pinned channel with nothing to pin to changes nothing", () => {
    // Pinning to no version would stop updates with no version to show for
    // it — the bricked state in a different costume. The channel option is
    // disabled in the UI; this is the model refusing it as well.
    const auto = { mode: "auto", channel: "stable" } as const;
    expect(withChannel(auto, "pinned", { fallbackVersion: null, automatic: true })).toEqual(auto);
  });
});

describe("channel moves", () => {
  it("keeps automatic checking across a stable/beta move", () => {
    // Changing channel is not a statement about how often to check, and
    // silently turning checks off would be a setting dropped on the way to
    // another one.
    expect(
      withChannel({ mode: "auto", channel: "stable" }, "beta", {
        fallbackVersion: null,
        automatic: true,
      }),
    ).toEqual({ mode: "auto", channel: "beta" });
    expect(
      withChannel({ mode: "manual", channel: "beta" }, "stable", {
        fallbackVersion: null,
        automatic: false,
      }),
    ).toEqual({ mode: "manual", channel: "stable" });
  });

  it("pins to what the caller offers, and keeps an existing pin over it", () => {
    expect(
      withChannel({ mode: "auto", channel: "beta" }, "pinned", {
        fallbackVersion: "0.0.3",
        automatic: true,
      }),
    ).toEqual({ mode: "pinned", version: "0.0.3" });
    // Re-selecting the channel you are already on must not silently
    // re-pin you to something else.
    expect(
      withChannel({ mode: "pinned", version: "0.0.1" }, "pinned", {
        fallbackVersion: "0.0.3",
        automatic: true,
      }),
    ).toEqual({ mode: "pinned", version: "0.0.1" });
  });

  it("restores the caller's automatic preference on the way out of pinned", () => {
    // `UpdatePolicy` has nowhere to keep it — a pinned policy carries a
    // version and no channel at all — so it is passed in rather than
    // remembered.
    const pinned = { mode: "pinned", version: "0.0.2" } as const;
    expect(withChannel(pinned, "stable", { fallbackVersion: null, automatic: true })).toEqual({
      mode: "auto",
      channel: "stable",
    });
    expect(withChannel(pinned, "beta", { fallbackVersion: null, automatic: false })).toEqual({
      mode: "manual",
      channel: "beta",
    });
    expect(pinnedVersion(pinned)).toBe("0.0.2");
  });
});

describe("what the picker defaults to", () => {
  it("prefers the running version", () => {
    const offers = [offer("0.0.3"), offer("0.0.2", { active: true }), offer("0.0.1")];
    expect(defaultPinTarget(offers)).toBe("0.0.2");
  });

  it("never defaults to a version this shell cannot run", () => {
    // Pinning to a blocked bundle reads to the author as "the app stopped
    // updating and I can't see why".
    const offers = [
      offer("0.0.3", { blocked: "needs app version 0.9.0 or newer" }),
      offer("0.0.2"),
    ];
    expect(defaultPinTarget(offers)).toBe("0.0.2");
  });

  it("has no target when every row is blocked", () => {
    const offers = [offer("0.0.3", { blocked: "needs a newer app" })];
    expect(defaultPinTarget(offers)).toBeNull();
  });
});

describe("how a row reads", () => {
  it("marks running, pinned and downloaded in that order of precedence", () => {
    expect(offerStatus(offer("0.0.2", { active: true, downloaded: true }), "0.0.2").badge).toBe(
      "Running",
    );
    expect(offerStatus(offer("0.0.3", { downloaded: true }), "0.0.3").badge).toBe("Pinned");
    expect(offerStatus(offer("0.0.1", { downloaded: true }), null).badge).toBe("Downloaded");
    expect(offerStatus(offer("0.0.1"), null).badge).toBeNull();
  });

  it("carries the shell's own refusal rather than a recomputed one", () => {
    const status = offerStatus(offer("0.0.9", { blocked: "needs app version 0.9.0" }), null);
    expect(status.selectable).toBe(false);
    expect(status.blocked).toBe("needs app version 0.9.0");
  });

  it("costs a column and not the pane when pubDate is malformed", () => {
    expect(formatPubDate(null)).toBeNull();
    expect(formatPubDate("not a date")).toBeNull();
    expect(formatPubDate("2026-09-15T00:00:00Z")).not.toBeNull();
  });
});
