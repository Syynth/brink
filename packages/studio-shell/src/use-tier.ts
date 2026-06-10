/**
 * @brink/studio-shell — responsive tier observer (spec §5.3).
 *
 * Moved from studio-ui with the dock shell (#80): the tier now lives in the
 * shell layout store instead of the old studio-store LayoutSlice. Breakpoints
 * and observer behavior are unchanged.
 */

import { useEffect, type RefObject } from "react";
import { useShell } from "./shell-context.js";
import type { LayoutTier } from "./layout-store.js";

const WIDE = 1000;
const MEDIUM = 720;

function computeTier(width: number): LayoutTier {
  if (width >= WIDE) return "wide";
  if (width >= MEDIUM) return "medium";
  return "narrow";
}

/**
 * Keep the shell layout store's `tier` in sync with the studio root width. A
 * ResizeObserver handles the general case (studio embedded in a sub-region);
 * a `window` resize listener is a fallback for the common case where the
 * studio fills its window / a full-bleed iframe. Both re-measure the live
 * root width and `setTier` no-ops when unchanged, so this only fires on
 * actual tier transitions.
 */
export function useTier(ref: RefObject<HTMLElement | null>): void {
  const { layout } = useShell();
  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const apply = (): void => {
      const next = computeTier(el.clientWidth);
      if (layout.getState().tier !== next) layout.getState().setTier(next);
    };

    apply();
    const ro = new ResizeObserver(apply);
    ro.observe(el);
    window.addEventListener("resize", apply);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", apply);
    };
  }, [ref, layout]);
}
