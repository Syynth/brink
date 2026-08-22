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
