/**
 * Idle-callback scheduling (#722) — a tiny cross-environment wrapper so a
 * heavy synchronous call (a wasm breakage/collision analysis) can be kicked
 * off a tick after the caller has already painted whatever "pending" UI it
 * needs to show, instead of running inline in the same frame as the event
 * that triggered it.
 *
 * `requestIdleCallback` isn't available everywhere (Safari, jsdom under
 * vitest), so this falls back to a macrotask (`setTimeout`) — still enough to
 * yield to the event loop (and a paint) before the heavy call runs, which is
 * the property this module exists to guarantee. It is not a substitute for a
 * worker: a call that itself blocks for seconds still blocks once it starts.
 * That tradeoff is intentional here — see issue #722's scope fence (no
 * web-worker architecture; this is the synchronous-main-thread mitigation).
 */

export type IdleHandle = number;

const FALLBACK_DELAY_MS = 0;
/** Cap on how long we'll let the browser defer an idle callback before we run
 *  it anyway — the analysis result is still user-relevant, it shouldn't wait
 *  for a fully idle main thread indefinitely. */
const IDLE_TIMEOUT_MS = 300;

interface IdleWindow {
  requestIdleCallback?: (cb: () => void, opts?: { timeout?: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
}

function idleWindow(): IdleWindow | undefined {
  return typeof window === "undefined" ? undefined : (window as unknown as IdleWindow);
}

/** Schedule `work` to run off the current call stack — on the next idle
 *  period when available, else on the next macrotask. Returns a handle for
 *  {@link cancelIdleWork}. */
export function scheduleIdleWork(work: () => void): IdleHandle {
  const w = idleWindow();
  if (w?.requestIdleCallback) {
    return w.requestIdleCallback(work, { timeout: IDLE_TIMEOUT_MS });
  }
  return setTimeout(work, FALLBACK_DELAY_MS) as unknown as number;
}

/** Cancel a handle returned by {@link scheduleIdleWork}. Safe to call with a
 *  handle that already fired. */
export function cancelIdleWork(handle: IdleHandle): void {
  const w = idleWindow();
  if (w?.cancelIdleCallback) {
    w.cancelIdleCallback(handle);
    return;
  }
  clearTimeout(handle);
}
