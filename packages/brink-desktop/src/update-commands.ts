/**
 * The updater toast's host command ids.
 *
 * A module of their own, with NO side effects, so a test can import them
 * plainly. `main.tsx` runs the shell's bootstrap at module scope
 * (`listen(...)`, DOM wiring), so importing it means a dynamic
 * `await import("../main.js")` after `vi.mock` setup — which
 * `autosave-reopen.test.ts` does do for `AUTOSAVE_MS`. It is possible, just
 * awkward enough that the updater's test restated the ids as literals
 * instead. This removes the excuse rather than the obstacle.
 *
 * That is not hypothetical. They shipped in desktop 0.4.0 as `update.install`
 * / `update.later` / `update.check` — unnamespaced. Host commands MUST be
 * `host.<vendor>.<name>` (studio-shell spec §8.1, enforced by
 * `CommandRegistry.registerHost`), so `mountStudio` threw and opening ANY
 * project failed, landing the user back on the landing screen with the
 * validator's message. The tests were green throughout, because they asserted
 * against the same wrong literals they were meant to be checking.
 *
 * `update-commands.test.ts` now runs the real registry over these.
 */

/** Install the staged update and relaunch. */
export const UPDATE_INSTALL_COMMAND = "host.brink.update.install";

/** Dismiss the offer for now. */
export const UPDATE_LATER_COMMAND = "host.brink.update.later";

/** Check for updates (the menu item and the toast's Try Again share it). */
export const UPDATE_CHECK_COMMAND = "host.brink.update.check";

/** Every id this host contributes — the guard test's input. */
export const UPDATE_COMMAND_IDS = [
  UPDATE_INSTALL_COMMAND,
  UPDATE_LATER_COMMAND,
  UPDATE_CHECK_COMMAND,
] as const;
