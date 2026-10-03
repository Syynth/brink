/**
 * Settings › Updates — the desktop's own section in the studio's rail
 * (`docs/desktop-ota-spec.md` Stage 4).
 *
 * A host section, passed through `mountStudio`'s `settingsSections` option
 * rather than added to `@brink/studio-ui`: an update channel and a bundle
 * store mean nothing in the browser, where there is no installer and
 * nothing to pin. It is drawn with the studio's own row primitives so it
 * reads as part of the page and not as a panel wedged into it.
 *
 * App scope throughout. Which version of the editor *this machine* runs is
 * never a property of the project — two authors on the same story can sit on
 * different bundles, and that is the point of pinning.
 *
 * The transitions live in `update-settings.ts`, pure and unit-tested; this
 * file is the rendering plus the effects. Every capability is injected, so
 * the section can be exercised without a Tauri runtime.
 */

import { useCallback, useEffect, useState } from "react";
import { SettingsGroup, SettingsRow, SettingsToggle } from "@brink-lang/studio";
import type { BundleOffer, UpdatePolicy } from "./tauri-provider.js";
import {
  CHANNEL_LABELS,
  channelOf,
  checksAutomatically,
  defaultPinTarget,
  formatPubDate,
  offerStatus,
  pinnedVersion,
  withAutomatic,
  withChannel,
  withPinnedVersion,
  type ChannelChoice,
} from "./update-settings.js";

export interface UpdateSettingsApi {
  readPolicy(): Promise<UpdatePolicy>;
  /** Persist the policy. Read-modify-write on the whole settings file. */
  writePolicy(policy: UpdatePolicy): Promise<void>;
  /** Every published bundle. Fetches the index; downloads nothing. */
  listVersions(): Promise<BundleOffer[]>;
  /**
   * Install and activate whatever the CURRENT policy resolves to, then
   * reload.
   *
   * Takes no version argument, deliberately: the shell re-resolves from the
   * policy on disk, so the caller writes the policy first and this cannot
   * install something the settings do not say. The same property
   * `bundle_update_apply` has, for the same reason.
   */
  applyCurrentPolicy(): Promise<void>;
  /** Run the ordinary unified check, exactly as the menu item does. */
  checkNow(): Promise<void>;
}

const CHANNEL_CHOICES: ChannelChoice[] = ["stable", "beta", "pinned"];

export function UpdateSettings({ api }: { api: UpdateSettingsApi }) {
  const [policy, setPolicy] = useState<UpdatePolicy | null>(null);
  const [offers, setOffers] = useState<BundleOffer[] | null>(null);
  const [offersError, setOffersError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    void api.readPolicy().then((next) => {
      if (live) setPolicy(next);
    });
    return () => {
      live = false;
    };
  }, [api]);

  const loadOffers = useCallback(() => {
    let live = true;
    setOffersError(null);
    api
      .listVersions()
      .then((next) => {
        if (live) setOffers(next);
      })
      .catch((e: unknown) => {
        // Reported in the pane rather than as a toast: the author opened
        // this, so there is somewhere to put the message and nobody is
        // interrupted by it.
        if (live) setOffersError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      live = false;
    };
  }, [api]);

  useEffect(loadOffers, [loadOffers]);

  const commit = useCallback(
    (next: UpdatePolicy) => {
      // Optimistic, then persisted: a settings control that waits for a disk
      // write to paint feels broken. A failed write is surfaced by the next
      // read disagreeing, which is the honest recovery.
      setPolicy(next);
      void api.writePolicy(next).catch((e: unknown) => {
        console.error("[brink-desktop] write update policy failed", e);
      });
    },
    [api],
  );

  if (policy === null) return <div className="settings-rows">Loading…</div>;

  const channel = channelOf(policy);
  const pinned = pinnedVersion(policy);
  const automatic = checksAutomatically(policy);

  const switchTo = (version: string) => {
    // Policy FIRST, then apply: the shell re-resolves from what is on disk,
    // so writing after would install the previous pin.
    const next = withPinnedVersion(version);
    setPolicy(next);
    setBusy(true);
    void api
      .writePolicy(next)
      .then(() => api.applyCurrentPolicy())
      .catch((e: unknown) => {
        console.error("[brink-desktop] switch to version failed", e);
      })
      .finally(() => setBusy(false));
  };

  return (
    <>
      <SettingsGroup title="Channel">
        <SettingsRow
          title="Updates"
          description={
            channel === "pinned"
              ? "Stays on the version you picked. Nothing updates until you choose another channel."
              : "Beta carries changes before they reach Stable."
          }
          htmlFor="brink-update-channel"
        >
          <select
            id="brink-update-channel"
            value={channel}
            onChange={(event) =>
              commit(
                withChannel(policy, event.target.value as ChannelChoice, {
                  fallbackVersion: defaultPinTarget(offers ?? []),
                  automatic,
                }),
              )
            }
          >
            {CHANNEL_CHOICES.map((choice) => (
              <option
                key={choice}
                value={choice}
                // Pinning needs something to pin to. Disabled rather than
                // hidden, so the option is visible and its absence
                // explainable, and never selectable into a dead state.
                disabled={choice === "pinned" && defaultPinTarget(offers ?? []) === null}
              >
                {CHANNEL_LABELS[choice]}
              </option>
            ))}
          </select>
        </SettingsRow>
        <SettingsRow
          title="Check automatically"
          description={
            channel === "pinned"
              ? "A pinned version has nothing to check for — pick Stable or Beta to receive updates."
              : "Checks on launch. You are always asked before anything installs."
          }
          htmlFor="brink-update-auto"
          indent
        >
          <SettingsToggle
            id="brink-update-auto"
            checked={automatic}
            label="Check for updates automatically"
            onChange={(next) => {
              if (channel === "pinned") return;
              commit(withAutomatic(policy, next));
            }}
          />
        </SettingsRow>
        <SettingsRow title="Check now" description="Looks for an update in either channel.">
          <button type="button" disabled={busy} onClick={() => void api.checkNow()}>
            Check for updates
          </button>
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="Versions">
        {offersError !== null && (
          <SettingsRow title="Could not list versions" description={offersError}>
            <button type="button" onClick={loadOffers}>
              Try again
            </button>
          </SettingsRow>
        )}
        {offersError === null && offers === null && (
          <SettingsRow title="Loading…" description="Fetching the published list.">
            <span />
          </SettingsRow>
        )}
        {offers !== null && offers.length === 0 && (
          <SettingsRow
            title="Nothing published yet"
            description="You are running the version built into the app."
          >
            <span />
          </SettingsRow>
        )}
        {(offers ?? []).map((offer) => {
          const status = offerStatus(offer, pinned);
          const date = formatPubDate(offer.pubDate);
          const parts = [CHANNEL_LABELS[offer.channel], date].filter((p) => p !== null);
          return (
            <SettingsRow
              key={offer.version}
              title={
                <>
                  {offer.version}
                  {status.badge !== null && (
                    <span className="settings-row-badge"> · {status.badge}</span>
                  )}
                </>
              }
              description={status.blocked ?? parts.join(" · ")}
            >
              <button
                type="button"
                // A blocked row cannot be chosen, and says why in its own
                // description rather than failing after a download.
                disabled={busy || !status.selectable || offer.active}
                onClick={() => switchTo(offer.version)}
              >
                {offer.active ? "Running" : "Use this version"}
              </button>
            </SettingsRow>
          );
        })}
      </SettingsGroup>
    </>
  );
}
