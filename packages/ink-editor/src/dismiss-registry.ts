/**
 * Global "dismiss all transient surfaces on Escape" safety net (#279).
 *
 * Every transient surface in this package (the code-actions menu, the
 * argument-widget popover/modal chrome) already dismisses on Escape and
 * outside-pointerdown through its OWN per-instance listeners. This registry
 * is a second, independent layer, deliberately NOT wired through those same
 * listeners: a surface registers its close callback here (in a separate,
 * minimal effect/lifecycle hook) while it is open, and one capture-phase
 * `document` Escape listener — installed once, the first time anything
 * registers, not tied to any single surface's listener logic — walks the
 * registry and closes everything registered.
 *
 * The point is resilience against exactly the failure #279 named: a surface
 * whose own dismiss listeners get lost (a re-render/error orphaning them)
 * while the surface stays mounted. Because registration lives in code
 * separate from the per-instance listener setup, a bug in the latter does
 * not take the former down with it.
 */

export type DismissHandler = () => void;

const registry = new Set<DismissHandler>();
let installedListener: ((event: KeyboardEvent) => void) | null = null;

/**
 * Register a transient surface's close callback while it is open. Call the
 * returned function to unregister (on close/unmount) — mirrors the
 * `addEventListener`-style cleanup-function convention used throughout this
 * package so it drops straight into a `useEffect`/lifecycle `return`.
 *
 * Installs the global Escape listener on first use, so a consumer never has
 * to remember to wire that up separately.
 */
export function registerDismissible(onClose: DismissHandler): () => void {
  installGlobalDismissNet();
  registry.add(onClose);
  return () => {
    registry.delete(onClose);
  };
}

/** Close every currently-registered transient surface. */
export function dismissAllTransientSurfaces(): void {
  if (registry.size === 0) return;
  // Snapshot first: an onClose may synchronously unregister itself (its own
  // unmount/cleanup) or others, which would otherwise mutate `registry` out
  // from under this iteration.
  for (const onClose of [...registry]) {
    onClose();
  }
}

/**
 * Install the global Escape safety net. Idempotent — safe to call from every
 * mount point (and from `registerDismissible` itself); only the first call
 * attaches the listener.
 */
export function installGlobalDismissNet(): void {
  if (installedListener !== null) return;
  installedListener = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    dismissAllTransientSurfaces();
  };
  document.addEventListener("keydown", installedListener, true);
}

/**
 * Test-only: reset registry + net-installed state between tests. Removes
 * the actual `document` listener (not just an internal flag) — otherwise a
 * second `installGlobalDismissNet()` call after a reset would attach a
 * SECOND listener alongside the first still-live one, double-firing every
 * registered `onClose` on the next Escape.
 */
export function resetDismissRegistryForTests(): void {
  registry.clear();
  if (installedListener !== null) {
    document.removeEventListener("keydown", installedListener, true);
    installedListener = null;
  }
}
