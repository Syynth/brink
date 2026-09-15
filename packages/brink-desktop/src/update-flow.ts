/**
 * One update, whichever channel carries it (`docs/desktop-ota-spec.md`
 * Stage 4, RULED 2026-09-15).
 *
 * The desktop has two update channels — the full-app updater for anything
 * native, and the OTA web-bundle channel for everything else. **From the
 * author's side there is either an update or there isn't.** Which one
 * carries it is our problem, so nothing this module produces names a
 * "bundle" or a "shell".
 *
 * Stages 1-3 shipped them as two visible mechanisms, and it showed: one
 * manual check raised two near-identical "up to date" toasts, and one
 * channel asked consent while the other installed silently. Both are bugs
 * under the rule above, not details.
 *
 * Every capability is injected, for the reason `updater.ts` gives: the whole
 * decision tree is then exercisable under vitest with no Tauri runtime, no
 * network, and no update server. That matters more here than there, because
 * this tree is where the two channels finally meet.
 */

import type { BundleUpdateCheck, BundleUpdateOutcome, UpdatePolicy } from "./tauri-provider.js";

/** A pending full-app update, as the updater plugin reports it. */
export interface PendingShellUpdate {
  version: string;
  /** Download and stage. Does NOT restart. */
  downloadAndInstall(): Promise<void>;
}

/** One notice, matching `StudioApi.notify`'s entry shape. */
export interface UpdateNotice {
  severity: "info" | "error";
  message: string;
}

export interface UnifiedUpdateApi {
  /** This install's policy. `Pinned` means nothing moves, in either channel. */
  policy(): Promise<UpdatePolicy>;
  /** Check the full-app channel; null when current. */
  checkShell(): Promise<PendingShellUpdate | null>;
  /** Check the bundle channel. Downloads nothing. */
  checkBundle(): Promise<BundleUpdateCheck>;
  /** Install the available bundle update. */
  applyBundle(): Promise<BundleUpdateOutcome>;
  /** Point the shell at the installed bundle and reload the webview. */
  activateBundle(): Promise<void>;
  /** Restart the whole process, for a shell update. */
  relaunch(): Promise<void>;
  /** Await the canonical save before anything that discards the document. */
  awaitSave(): Promise<void>;
  /** Ask. Resolves false on decline. */
  confirm(message: string): Promise<boolean>;
  /** Report. */
  notify(notice: UpdateNotice): void;
}

export interface CheckOptions {
  /**
   * An automatic check (launch, focus) is silent about the boring outcomes.
   * A manual one reports everything, because a menu item that can do nothing
   * visible is a broken button.
   */
  silent?: boolean;
}

export type UpdateResult =
  | "none"
  | "pinned"
  | "declined"
  | "installed"
  | "refused"
  | "failed";

/**
 * Run one check to completion.
 *
 * Returns what happened so callers and tests can assert on the outcome
 * rather than on side effects alone.
 */
export async function checkForAnyUpdate(
  api: UnifiedUpdateApi,
  { silent = false }: CheckOptions = {},
): Promise<UpdateResult> {
  let policy: UpdatePolicy;
  try {
    policy = await api.policy();
  } catch {
    // A settings read that fails should not stop an update; the default
    // policy is the conservative one.
    policy = { mode: "auto", channel: "stable" };
  }

  // A pinned install has nothing to check — not because a flag suppresses
  // it, but because there is no manifest to consult. Reporting the PIN
  // rather than "up to date" is what lets the author act: the button still
  // does something honest, it just does not offer them a cliff.
  if (policy.mode === "pinned") {
    if (!silent) {
      api.notify({
        severity: "info",
        message: `Pinned to version ${policy.version}. Switch to Stable or Beta in Settings to receive updates.`,
      });
    }
    return "pinned";
  }

  // Manual means "ask me"; an automatic check in that mode is the thing the
  // author turned off.
  if (policy.mode === "manual" && silent) return "none";

  const [shell, bundle] = await Promise.all([
    api.checkShell().catch((e: unknown) => describeAsFailure(e)),
    api.checkBundle().catch((e: unknown) => ({
      kind: "failed" as const,
      reason: describe(e),
    })),
  ]);

  // A shell failure and a bundle failure are the same event to the author:
  // the check did not work. Reporting both would name the mechanism.
  if (shell instanceof CheckFailure || bundle.kind === "failed") {
    const reason = shell instanceof CheckFailure ? shell.reason : (bundle as { reason: string }).reason;
    if (!silent) {
      api.notify({ severity: "error", message: `Could not check for updates: ${reason}` });
    }
    return "failed";
  }

  // A refusal is never silent: it is the one outcome the author can act on,
  // and reporting it as "up to date" would leave them waiting forever for
  // an update that can never apply.
  if (bundle.kind === "refused") {
    api.notify({ severity: "error", message: bundle.reason });
    return "refused";
  }

  // BOTH channels can have something. The shell update wins: it is the
  // larger change, it may raise minShellVersion (making the bundle update
  // installable where it was not), and restarting into it means the next
  // check picks the bundle up anyway. Offering both would be the mechanism
  // showing through.
  if (shell !== null) return offerShellUpdate(api, shell);
  if (bundle.kind === "available") return offerBundleUpdate(api);

  if (!silent) api.notify({ severity: "info", message: "Brink Studio is up to date." });
  return "none";
}

/** Offer, install and restart a full-app update. */
async function offerShellUpdate(
  api: UnifiedUpdateApi,
  update: PendingShellUpdate,
): Promise<UpdateResult> {
  if (!(await api.confirm(`Brink Studio ${update.version} is available.`))) return "declined";

  try {
    await update.downloadAndInstall();
  } catch (e: unknown) {
    // Always reported: the author consented to this one, so its failure is
    // not noise.
    api.notify({ severity: "error", message: `Update failed to install: ${describe(e)}` });
    return "failed";
  }

  // Save BEFORE restarting, never after — after is too late.
  await api.awaitSave();
  await api.relaunch();
  return "installed";
}

/**
 * Offer, install and activate a bundle update.
 *
 * Deliberately worded without a version. The bundle's sequence is
 * independent of the app's (0.0.2 while the app is at 0.8.0), so naming it
 * would both leak the mechanism and read as a downgrade.
 */
async function offerBundleUpdate(api: UnifiedUpdateApi): Promise<UpdateResult> {
  if (!(await api.confirm("An update is available."))) return "declined";

  let outcome: BundleUpdateOutcome;
  try {
    outcome = await api.applyBundle();
  } catch (e: unknown) {
    outcome = { kind: "failed", reason: describe(e) };
  }

  switch (outcome.kind) {
    case "installed":
      break;
    case "refused":
      api.notify({ severity: "error", message: outcome.reason });
      return "refused";
    case "failed":
      api.notify({ severity: "error", message: `Update failed to install: ${outcome.reason}` });
      return "failed";
    case "upToDate":
      // Raced: something installed it between the check and the apply.
      api.notify({ severity: "info", message: "Brink Studio is up to date." });
      return "none";
  }

  // Activation discards the document exactly as a relaunch does — a reload
  // destroys the page and every worker with it — so it takes the same save
  // first. It is cheaper than a restart, not free.
  await api.awaitSave();
  try {
    await api.activateBundle();
  } catch (e: unknown) {
    // Installed but not activated is a real state and not a failure: the
    // next launch serves it. Say so rather than implying nothing happened.
    api.notify({
      severity: "info",
      message: `Update installed. It takes effect the next time you open Brink Studio. (${describe(e)})`,
    });
    return "installed";
  }
  return "installed";
}

/** A shell check that threw, carried rather than thrown so both channels
 *  can be awaited together. */
class CheckFailure {
  constructor(readonly reason: string) {}
}

function describeAsFailure(e: unknown): CheckFailure {
  return new CheckFailure(describe(e));
}

function describe(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
