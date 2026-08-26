/**
 * Update check + install (D4, `docs/desktop-shell-spec.md`).
 *
 * Ruled 2026-08-22: **check on launch, prompt to install** — a silent check
 * shortly after startup, a prompt only when something is actually found, and
 * a manual "Check for Updates…" menu item that reports the up-to-date case
 * too. Nothing installs without consent.
 *
 * Ruled alongside it: **save first, then relaunch.** Installing an update
 * means restarting the app, which is the same hazard quitting is — an
 * in-flight canonical write must not be raced. So this reuses
 * `awaitSaveAllBeforeQuit` (quit.ts) rather than standing up a third copy of
 * the dispatch/redispatch/cap discipline that #2370, #2434 and #2444 taught
 * that seam. (Its name still says "Quit"; it has served Close Project since
 * #2444 and now serves update-relaunch — three callers, one teardown
 * contract. Renaming is deliberately left alone rather than churned here.)
 *
 * Every capability this module needs is injected through {@link UpdateApi},
 * for the same reason `QuitSaveApi` exists: the whole decision tree is then
 * exercisable under vitest with no Tauri runtime, no network, and no real
 * update server.
 */

/** A pending update, as returned by the updater plugin's `check()`. */
export interface PendingUpdate {
  version: string;
  /** Download and stage the new bundle. Does NOT restart the app. */
  downloadAndInstall(): Promise<void>;
}

export interface UpdateApi {
  /** Resolve the available update, or null when already current. */
  check(): Promise<PendingUpdate | null>;
  /** Ask the user to install. Resolves false on decline. */
  confirm(version: string): Promise<boolean>;
  /** Surface a message (up-to-date, failures) through the studio. */
  notify(severity: "info" | "error", message: string): void;
  /**
   * Await the canonical save before restarting — `awaitSaveAllBeforeQuit`
   * in production. A no-op when no project is open.
   */
  awaitSave(): Promise<void>;
  /** Restart into the freshly installed bundle. */
  relaunch(): Promise<void>;
}

export interface CheckOptions {
  /**
   * A launch check is silent about the boring outcomes: no update, or a
   * check that failed (offline is the common case and is not the author's
   * problem to see). A manual check reports every outcome — a "Check for
   * Updates…" menu item that can do nothing visible is a broken button.
   */
  silent?: boolean;
}

/**
 * Minimum gap between AUTOMATIC update checks (launch, window focus).
 *
 * Focus fires every time the author alt-tabs back from a browser or a
 * reference doc — dozens of times an hour — and an update server does not
 * need to hear about any of it. Four hours sits far below the rate a
 * release actually ships and far above the rate a person switches windows.
 */
export const AUTO_CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000;

export interface AutoCheckState {
  /** When the last check of any kind ran (epoch ms); 0 = never. */
  lastCheckAt: number;
  /** Whether an update offer is currently on screen awaiting an answer. */
  offerPending: boolean;
  /** Now, injected so this stays pure and testable. */
  now: number;
}

/**
 * Whether an automatic check should actually run.
 *
 * Two reasons to decline, both about not being obnoxious rather than about
 * correctness:
 *  - an offer is already up (re-raising an identical toast under the
 *    author's cursor is churn, and replacing it would settle the live
 *    promise as declined), or
 *  - the last check was too recent.
 *
 * Manual checks never consult this — the author asked.
 */
export function shouldAutoCheck({
  lastCheckAt,
  offerPending,
  now,
  intervalMs = AUTO_CHECK_INTERVAL_MS,
}: AutoCheckState & { intervalMs?: number }): boolean {
  if (offerPending) return false;
  if (lastCheckAt === 0) return true;
  return now - lastCheckAt >= intervalMs;
}

/**
 * Run one update check to completion. Returns what happened, so callers and
 * tests can assert on the outcome rather than on side effects alone.
 */
export async function checkForUpdates(
  api: UpdateApi,
  { silent = false }: CheckOptions = {},
): Promise<"none" | "declined" | "installed" | "failed"> {
  let update: PendingUpdate | null;
  try {
    update = await api.check();
  } catch (e: unknown) {
    // Offline, DNS failure, a malformed manifest — all land here. Silent on
    // launch; explicit when the author asked.
    if (!silent) {
      api.notify("error", `Could not check for updates: ${describe(e)}`);
    }
    return "failed";
  }

  if (update === null) {
    if (!silent) api.notify("info", "Brink Studio is up to date.");
    return "none";
  }

  if (!(await api.confirm(update.version))) return "declined";

  try {
    await update.downloadAndInstall();
  } catch (e: unknown) {
    // Always reported, even on a launch check: the author consented to this
    // one, so its failure is not noise.
    api.notify("error", `Update failed to install: ${describe(e)}`);
    return "failed";
  }

  // The new bundle is staged; restarting is what applies it. Save BEFORE
  // relaunching, never after — after is too late.
  await api.awaitSave();
  await api.relaunch();
  return "installed";
}

function describe(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
