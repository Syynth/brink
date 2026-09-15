/**
 * Reporting for OTA web-bundle updates (`docs/desktop-ota-spec.md` Stage 2).
 *
 * The decision logic lives in Rust (`bundle_update::decide`) because it
 * gates on `minShellVersion`, which is a safety property rather than a
 * presentation one. What is left here is how each outcome is *said* — and
 * the distinctions that matters for are real ones:
 *
 * - **Refused is not up-to-date.** A bundle needing a newer shell is a
 *   thing the author can fix (install the full app update). Reporting it as
 *   "you're up to date" would leave them waiting forever for an update that
 *   can never apply.
 * - **Refused is not failed.** A refusal is the gate working; a failure is
 *   the channel broken. Collapsing them teaches the author to ignore both.
 * - **Installed does not mean running.** Activation is on next launch — the
 *   running webview already holds the old JS and instantiated wasm — so the
 *   message has to say so or the author will look for a change that is not
 *   there yet.
 */

import type { BundleUpdateCheck, BundleUpdateOutcome } from "./tauri-provider.js";

/** One studio notification, matching `StudioApi.notify`'s entry shape. */
export interface BundleUpdateNotice {
  severity: "info" | "error";
  message: string;
}

/**
 * How an outcome should be reported, or `null` when it should be silent.
 *
 * `silent` is for the automatic (launch/focus) check, which must not
 * interrupt to say nothing happened — the same rule `updater.ts` already
 * applies to the full-app channel. A manual check reports every outcome,
 * because a menu item that can do nothing visible is a broken button.
 */
export function bundleUpdateNotice(
  outcome: BundleUpdateOutcome,
  options: { silent?: boolean } = {},
): BundleUpdateNotice | null {
  const silent = options.silent === true;
  switch (outcome.kind) {
    case "installed":
      // Never silent: something changed on disk and will apply next launch.
      return {
        severity: "info",
        message: `Editor update ${outcome.version} installed. It takes effect the next time you open Brink Studio.`,
      };
    case "refused":
      // Never silent either: the author can act on this one.
      return { severity: "error", message: outcome.reason };
    case "failed":
      return silent ? null : { severity: "error", message: `Update check failed: ${outcome.reason}` };
    case "upToDate":
      return silent ? null : { severity: "info", message: "The editor is up to date." };
  }
}

/**
 * How a CHECK should be reported, or `null` when it should be silent.
 *
 * `available` is deliberately absent from the returned shapes: an available
 * update is an offer awaiting an answer, not a notice. Reporting it as one
 * would tell the author something is happening and then not do it — the
 * exact confusion splitting consent out of the install path exists to remove.
 * Callers must handle `available` before reaching here; this returns `null`
 * for it so a caller that forgets says nothing rather than something wrong.
 */
export function bundleCheckNotice(
  check: BundleUpdateCheck,
  options: { silent?: boolean } = {},
): BundleUpdateNotice | null {
  const silent = options.silent === true;
  switch (check.kind) {
    case "available":
      return null;
    case "refused":
      // Never silent: the author can act on this one.
      return { severity: "error", message: check.reason };
    case "failed":
      return silent
        ? null
        : { severity: "error", message: `Update check failed: ${check.reason}` };
    case "upToDate":
      return silent ? null : { severity: "info", message: "The editor is up to date." };
  }
}
