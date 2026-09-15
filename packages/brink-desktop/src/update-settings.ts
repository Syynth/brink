/**
 * The Updates settings model (`docs/desktop-ota-spec.md` Stage 4, RULED
 * 2026-09-15).
 *
 * **Pinned is a channel, not a flag beside one.** An install pinned to a
 * version and also set to auto-update is a contradiction the author can
 * reach by pressing two controls that each look reasonable, and the shell
 * would then either ignore the pin or ignore the setting. So there are three
 * channels — Stable, Beta, and a pinned version — and "check automatically"
 * is a property only the first two have.
 *
 * That is the shape of the ruling: *"we shouldn't have buttons that if you
 * press them get you bricked, it should just like.. not do what you want.
 * you can't be pinned and on auto-update, that doesn't make sense."* A
 * control that cannot express the broken state is better than one that
 * warns about it.
 *
 * Everything here is pure. The React section is a rendering of it, so the
 * transitions — which are where a policy editor goes wrong — are testable
 * without a DOM, a Tauri runtime or a network.
 */

import type { BundleOffer, UpdateChannel, UpdatePolicy } from "./tauri-provider.js";

/** What the channel control offers. `pinned` is one of them, deliberately. */
export type ChannelChoice = UpdateChannel | "pinned";

export const CHANNEL_LABELS: Record<ChannelChoice, string> = {
  stable: "Stable",
  beta: "Beta",
  pinned: "Pinned to a version",
};

/** Which channel row is selected for a policy. */
export function channelOf(policy: UpdatePolicy): ChannelChoice {
  return policy.mode === "pinned" ? "pinned" : policy.channel;
}

/**
 * Whether this policy checks on its own.
 *
 * A pinned install never checks — there is nothing to check *for*, since the
 * version is named rather than resolved. Reported as `false` rather than as
 * a preserved previous value, because the switch is disabled in that state
 * and a switch showing "on" while nothing happens is the lie this whole
 * model exists to avoid.
 */
export function checksAutomatically(policy: UpdatePolicy): boolean {
  return policy.mode === "auto";
}

/** The version a pinned policy names, or `null` for the other two. */
export function pinnedVersion(policy: UpdatePolicy): string | null {
  return policy.mode === "pinned" ? policy.version : null;
}

/**
 * Move to a channel.
 *
 * `fallbackVersion` is what a move INTO `pinned` pins to — the caller passes
 * what is running, or the newest thing on offer. With neither (a fresh
 * install with no index yet), the move is refused by returning the policy
 * unchanged: pinning to nothing would stop updates with no version to show
 * for it, which is the bricked state in a different costume.
 *
 * `automatic` is what a move OUT of `pinned` restores. It is passed in
 * rather than remembered, because `UpdatePolicy` has nowhere to keep it — a
 * pinned policy carries a version and no channel at all.
 */
export function withChannel(
  policy: UpdatePolicy,
  choice: ChannelChoice,
  { fallbackVersion, automatic }: { fallbackVersion: string | null; automatic: boolean },
): UpdatePolicy {
  if (choice === "pinned") {
    const version = pinnedVersion(policy) ?? fallbackVersion;
    return version === null ? policy : { mode: "pinned", version };
  }
  return automatic ? { mode: "auto", channel: choice } : { mode: "manual", channel: choice };
}

/**
 * Turn automatic checking on or off.
 *
 * A no-op while pinned, rather than a state change the UI then has to
 * explain away: the control is disabled there, and a call that got through
 * anyway (a keyboard path, a future caller) must not produce a policy the
 * model says cannot exist.
 */
export function withAutomatic(policy: UpdatePolicy, automatic: boolean): UpdatePolicy {
  if (policy.mode === "pinned") return policy;
  return automatic
    ? { mode: "auto", channel: policy.channel }
    : { mode: "manual", channel: policy.channel };
}

/** Pin to a specific version. */
export function withPinnedVersion(version: string): UpdatePolicy {
  return { mode: "pinned", version };
}

/**
 * What to pin to when the author picks the pinned channel without naming a
 * version: what is running, else the newest thing this install can actually
 * run.
 *
 * Blocked rows are skipped. Offering one as the default would pin the
 * install to a bundle the shell refuses, which reads to the author as "the
 * app stopped updating and I can't see why".
 */
export function defaultPinTarget(offers: readonly BundleOffer[]): string | null {
  return (
    offers.find((offer) => offer.active)?.version ??
    offers.find((offer) => offer.blocked === null)?.version ??
    null
  );
}

/** How a row reads in the list, beyond its version. */
export interface OfferStatus {
  /** Short state word, or `null` when the row is unremarkable. */
  badge: string | null;
  /** Why it cannot be chosen, or `null`. */
  blocked: string | null;
  selectable: boolean;
}

export function offerStatus(offer: BundleOffer, pinned: string | null): OfferStatus {
  let badge: string | null = null;
  if (offer.active) badge = "Running";
  else if (offer.version === pinned) badge = "Pinned";
  else if (offer.downloaded) badge = "Downloaded";
  return { badge, blocked: offer.blocked, selectable: offer.blocked === null };
}

/**
 * A publication date as a plain local date, or `null` when there isn't one
 * or it does not parse.
 *
 * Deliberately lenient: `pubDate` is display-only and comes from a
 * hand-written publish step, so a malformed one should cost a column and
 * not the whole pane.
 */
export function formatPubDate(pubDate: string | null): string | null {
  if (pubDate === null) return null;
  const at = new Date(pubDate);
  if (Number.isNaN(at.getTime())) return null;
  return at.toLocaleDateString();
}
