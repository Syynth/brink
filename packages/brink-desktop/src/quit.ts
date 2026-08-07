/**
 * The await-the-final-save seam for app quit (#2370).
 *
 * Ruled 2026-08-07 (docs/decision-log.md, "Desktop close: no dirty prompt;
 * quit awaits the final save"): no dirty-close confirmation — autosave +
 * save-on-close make it dead UI. The one real safety piece is that quitting
 * the app must not race the in-flight canonical write: the window's
 * `onCloseRequested` handler (main.tsx) awaits this before the window is
 * actually destroyed.
 *
 * `StudioApi.dispatch` (docs/studio-shell-spec.md §8.2) is fire-and-forget —
 * it reports whether a command was found, not a promise of its completion —
 * and `file.saveAll`'s host-save branch is a promise chain private to
 * `registerFileCommands` (file-commands.ts), with no seam exposed on
 * `StudioApi` for a caller outside the command system to await. Rather than
 * growing that shared surface for this one caller, we poll `getDirtyFiles()`
 * to empty: the same observable the autosave ticker already reads (D2).
 *
 * The wait is capped (~3s default) so a hung write can never make the app
 * unquittable — the backup ring (#154) already holds the content, so on
 * timeout it is safe to let quit proceed regardless.
 */

/** The slice of `StudioApi` this seam needs — kept minimal and structural. */
export interface QuitSaveApi {
  dispatch(commandId: string, args?: unknown): boolean;
  getDirtyFiles(): string[];
}

const DEFAULT_TIMEOUT_MS = 3000;
const DEFAULT_POLL_INTERVAL_MS = 50;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Dispatch `file.saveAll` unconditionally and poll `getDirtyFiles()` until
 * it empties or `timeoutMs` elapses. Never rejects: a save failure
 * (surfaced elsewhere via the notification center) or a timeout both just
 * return, and the caller proceeds with quit either way.
 *
 * The dispatch is unconditional — not gated on `getDirtyFiles()` being
 * non-empty — because `getDirtyFiles()` only reflects changes recorded via
 * `ProjectSession.notifyFileChanged`, which runs off the 500ms trailing
 * diagnostics debounce. Per-keystroke edits reach the wasm session
 * immediately but are not staged or marked dirty until that debounce
 * fires, so a quit within the debounce window would see an empty dirty
 * set and skip the save entirely, dropping the edit. `file.saveAll`
 * synchronously flushes pending edits (push + notifyFileChanged -> stage +
 * dirty) before writing, so dispatching it here always sees the true
 * state; a genuinely clean project just performs a no-op rewrite of
 * identical content, which the file provider's self-write suppression
 * absorbs.
 */
export async function awaitSaveAllBeforeQuit(
  api: QuitSaveApi,
  timeoutMs = DEFAULT_TIMEOUT_MS,
  pollIntervalMs = DEFAULT_POLL_INTERVAL_MS,
): Promise<void> {
  api.dispatch("file.saveAll");
  const deadline = Date.now() + timeoutMs;
  while (api.getDirtyFiles().length > 0 && Date.now() < deadline) {
    await sleep(pollIntervalMs);
  }
}
