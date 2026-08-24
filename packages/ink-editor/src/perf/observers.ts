/**
 * Browser-level observers (measure-first ruling, 2026-08-24): the metrics
 * that need no cooperation from instrumented code — long tasks, long
 * animation frames, input-event latency, dropped frames. Attached only by
 * dev-mode hosts (studio/desktop); nothing here runs in production builds.
 */

import { isPerfEnabled, perfRecord } from "./probe.js";

/** Only frames slower than this are recorded individually; the full frame
 *  distribution lives in the CDP trace, not the ring buffer. */
const LONG_FRAME_MS = 25;

/** Event-timing entries shorter than this are dropped by the observer itself
 *  (the platform minimum is 16). Keystroke lag we care about is well above. */
const EVENT_DURATION_THRESHOLD_MS = 16;

interface EventTimingEntry extends PerformanceEntry {
  processingStart: number;
  processingEnd: number;
}

/**
 * Attach every available observer; returns a detach function. Safe to call
 * in hosts where some entry types are unsupported (each observer is
 * feature-gated independently; WebKit lacks `longtask`/`long-animation-frame`
 * but has `event` timing).
 */
export function attachPerfObservers(): () => void {
  const observers: PerformanceObserver[] = [];
  const supported: readonly string[] = PerformanceObserver.supportedEntryTypes ?? [];

  const observe = (
    type: string,
    handler: (entries: PerformanceEntryList) => void,
    extra?: Record<string, unknown>,
  ): void => {
    if (!supported.includes(type)) return;
    try {
      const obs = new PerformanceObserver((list) => {
        if (!isPerfEnabled()) return;
        handler(list.getEntries());
      });
      obs.observe({ type, buffered: false, ...extra } as PerformanceObserverInit);
      observers.push(obs);
    } catch {
      // An entry type the UA advertises but refuses to observe: skip it.
    }
  };

  observe("longtask", (entries) => {
    for (const e of entries) {
      perfRecord("browser.longtask", e.startTime, e.duration);
    }
  });

  observe("long-animation-frame", (entries) => {
    for (const e of entries) {
      const blocking = (e as PerformanceEntry & { blockingDuration?: number }).blockingDuration;
      perfRecord("browser.longAnimationFrame", e.startTime, e.duration, blocking);
    }
  });

  observe(
    "event",
    (entries) => {
      for (const e of entries) {
        const et = e as EventTimingEntry;
        // `duration` is input-timestamp → next-paint: the user-felt lag. The
        // meta carries the synchronous handler share (processing time).
        perfRecord(`input.${e.name}`, e.startTime, e.duration, et.processingEnd - et.processingStart);
      }
    },
    { durationThreshold: EVENT_DURATION_THRESHOLD_MS },
  );

  // Long-frame sampler: rAF deltas above LONG_FRAME_MS recorded as spans.
  // Every frame would flood the ring for no benefit — the CDP trace holds the
  // complete frame timeline for offline analysis.
  let rafId = 0;
  let last = performance.now();
  let running = true;
  const tick = (now: number): void => {
    if (!running) return;
    const delta = now - last;
    last = now;
    if (isPerfEnabled() && delta > LONG_FRAME_MS) {
      perfRecord("frame.long", now - delta, delta);
    }
    rafId = requestAnimationFrame(tick);
  };
  rafId = requestAnimationFrame(tick);

  return () => {
    running = false;
    cancelAnimationFrame(rafId);
    for (const obs of observers) obs.disconnect();
  };
}
