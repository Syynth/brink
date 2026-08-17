/**
 * Global "dismiss all transient surfaces on Escape" safety net (#279).
 *
 * Every transient surface in this package (the code-actions menu, the
 * argument-widget popover/modal chrome) already dismisses on Escape and
 * outside-pointerdown through its OWN per-instance listeners. This registry
 * is a second, independent layer, deliberately NOT wired through those same
 * listeners: a surface registers its close callback here (in a separate,
 * minimal effect/lifecycle hook) while it is open, and one `window`,
 * BUBBLE-phase Escape listener — installed once, the first time anything
 * registers, not tied to any single surface's listener logic — walks the
 * registry and closes everything registered.
 *
 * The point is resilience against exactly the failure #279 named: a surface
 * whose own dismiss listeners get lost (a re-render/error orphaning them)
 * while the surface stays mounted. Because registration lives in code
 * separate from the per-instance listener setup, a bug in the latter does
 * not take the former down with it.
 *
 * LISTENER ORDERING (must run LAST, not first): every surface's own dismiss
 * listener is `document`-level, CAPTURE-phase (matching `Overlay`'s
 * contract, mirrored in this package). Capture-phase listeners on the same
 * node fire in registration order — and this net's listener is installed
 * once, on the very first `registerDismissible()` call anywhere in this
 * module's lifetime, so it was almost always registered BEFORE any
 * individual surface's own listener, which only gets (re)attached each time
 * that surface opens. If this net were also `document`-capture, it would
 * then run FIRST on every open after the first: it would call every
 * registered `onClose` — including the surface's own — before that
 * surface's own capture-phase handler ever got a chance to run its
 * `preventDefault()`/focus-return logic, silently defeating it (and, since
 * `dismissAllTransientSurfaces()` closes everything, tearing down every
 * registered surface instead of just the top-most one).
 *
 * Attaching on `window` in the BUBBLE phase fixes this structurally, not by
 * accident of registration order: capture-phase listeners on `document`
 * (every surface's own) always run to completion, in the capture phase,
 * strictly BEFORE any bubble-phase listener on `window` gets a chance to run
 * — regardless of which was attached first. A surface that handles Escape
 * itself calls `preventDefault()`, which this net's `defaultPrevented` guard
 * then honors; a surface that calls `stopPropagation()` keeps the event from
 * reaching `window` at all. The net only fires — and only then closes
 * whatever is left registered — when nothing in the dispatch path already
 * handled the key, which is exactly the orphan case #279 is a safety net
 * for.
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
 *
 * `window`, bubble phase (capture=false) — deliberately NOT `document`
 * capture-phase, which every individual surface uses for its own listener.
 * See the "LISTENER ORDERING" note on the module doc comment above.
 */
export function installGlobalDismissNet(): void {
  if (installedListener !== null) return;
  installedListener = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    dismissAllTransientSurfaces();
  };
  window.addEventListener("keydown", installedListener, false);
}

/**
 * Test-only: reset registry + net-installed state between tests. Removes
 * the actual `window` listener (not just an internal flag) — otherwise a
 * second `installGlobalDismissNet()` call after a reset would attach a
 * SECOND listener alongside the first still-live one, double-firing every
 * registered `onClose` on the next Escape.
 */
export function resetDismissRegistryForTests(): void {
  registry.clear();
  if (installedListener !== null) {
    window.removeEventListener("keydown", installedListener, false);
    installedListener = null;
  }
}
