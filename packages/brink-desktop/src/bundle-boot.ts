/**
 * Boot confirmation for the OTA web bundle (`docs/desktop-ota-spec.md`
 * Stage 2).
 *
 * The shell stamps a rollback sentinel into `current.json` before the window
 * loads and this module clears it. The asymmetry is the whole design: a
 * bundle that boots clears its own sentinel, and nothing else can — so a
 * sentinel found at the NEXT launch means that bundle wedged the webview,
 * and the shell deletes it and reverts to the previous bundle or to the
 * embedded floor.
 *
 * Two consequences worth stating plainly, because both are easy to break:
 *
 * - **The confirm must not be conditional on anything that can fail.** Gate
 *   it behind a project being open, a store being ready, or a feature flag,
 *   and a perfectly good bundle gets rolled back on every launch.
 * - **A failed confirm is safe, not silent.** If the IPC call throws, the
 *   sentinel survives and the next launch reverts. That errs toward the
 *   known-good floor, which is the direction this channel should fail in —
 *   but it also means a persistently-throwing confirm looks exactly like a
 *   persistently-broken bundle, so the failure is logged rather than
 *   swallowed.
 */

import type { BundleLaunchInfo } from "./tauri-provider.js";

/** The one capability this module needs, injected for testability. */
export type BundleReadyFn = () => Promise<BundleLaunchInfo>;

/**
 * The message shown when the shell reverted a bundle that would not boot.
 * Pure, and exported for direct testing.
 *
 * It names both versions because the author needs to know two separate
 * things: that the update they installed is gone, and what they are running
 * instead — otherwise they debug a fix that is no longer present.
 */
export function rollbackMessage(info: BundleLaunchInfo): string | null {
  if (info.rolledBackFrom === null) return null;
  const running =
    info.version === null ? "the version built into the app" : `version ${info.version}`;
  return `Update ${info.rolledBackFrom} did not start and was removed. Now running ${running}.`;
}

/**
 * Confirm this launch booted. Resolves to what the shell reports, or `null`
 * when the call failed — see the module note on why that is the safe
 * direction.
 */
export async function confirmBundleBoot(ready: BundleReadyFn): Promise<BundleLaunchInfo | null> {
  try {
    return await ready();
  } catch (e: unknown) {
    console.error(
      "[brink-desktop] bundle boot confirmation failed; this launch will be treated as a " +
        "failed boot and rolled back on next start:",
      e,
    );
    return null;
  }
}
