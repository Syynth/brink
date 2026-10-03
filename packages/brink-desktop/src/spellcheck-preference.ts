/**
 * "Use the system spell checker" — the desktop's one spelling preference.
 *
 * ## Why this lives in `localStorage` and not `settings.json`
 *
 * Not convenience: `settings.json` is owned by the Rust side, so a new field
 * there ships only in a signed release. This preference is bundle-side
 * behaviour, so putting it in `AppSettings` would mean an OTA'd bundle
 * writing a key the installed 0.8.0 shell does not know — and that shell
 * deserialises into its own struct and writes it back, silently dropping the
 * key on every save. The author would toggle it off and find it on again.
 *
 * `localStorage` has none of that coupling. The webview origin is stable at
 * `brink://localhost` from 0.8.0 on (the origin change is an INSTALL-time
 * event, not a bundle-publish one), so the value survives bundle updates and
 * app updates alike. It is per-machine, which is exactly the scope of a
 * question about *this* machine's dictionary.
 *
 * Every accessor is wrapped: storage throws in a private window and can come
 * back empty after a data clear. The default is the answer in either case.
 */

/** The key, namespaced like the studio's own. */
export const SYSTEM_SPELLCHECK_KEY = "brink.desktop.spellcheck.useSystem";

/**
 * On by default.
 *
 * The platform checker knows the author's own words — every name they have
 * taught the OS in any app — where a bundled dictionary starts from zero on
 * every machine. Defaulting off would hide the better answer behind a
 * setting nobody knows to look for.
 */
export const SYSTEM_SPELLCHECK_DEFAULT = true;

/**
 * Read the preference. Anything unreadable or unrecognised reads as the
 * default, which keeps a corrupted value from silently disabling spelling.
 */
export function readUseSystemSpellcheck(storage: Storage | undefined): boolean {
  if (storage === undefined) return SYSTEM_SPELLCHECK_DEFAULT;
  try {
    const raw = storage.getItem(SYSTEM_SPELLCHECK_KEY);
    if (raw === "true") return true;
    if (raw === "false") return false;
    return SYSTEM_SPELLCHECK_DEFAULT;
  } catch {
    return SYSTEM_SPELLCHECK_DEFAULT;
  }
}

/** Persist the preference. A failed write is not worth failing the toggle
 *  over — the session still honours it, it just does not survive a restart. */
export function writeUseSystemSpellcheck(storage: Storage | undefined, next: boolean): void {
  if (storage === undefined) return;
  try {
    storage.setItem(SYSTEM_SPELLCHECK_KEY, next ? "true" : "false");
  } catch {
    // Private window, or site data blocked. Nothing to recover.
  }
}
