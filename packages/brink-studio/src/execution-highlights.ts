/**
 * The execution-highlight computation (W6/#3299) — extracted from
 * `mount.tsx` (the `location-resolvers.ts` pattern) so the policy is
 * testable over a real store state: "play is stepping" means the band
 * follows the runtime position continuously; paused turns it warning and
 * adds the gutter arrow; degraded and non-live statuses suppress —
 * suppressed, never stale (`docs/live-inspector-spec.md` §5).
 *
 * Plural by design: W11's choice-point set and W8's selected-frame band
 * join the returned array later with no seam change.
 */

import type { ExecutionHighlight } from "@brink-lang/editor";
import {
  isDebugSessionProvider,
  sessionDegraded,
  type StudioState,
} from "@brink/studio-store";

/** All execution highlights for `path`, from the live session. */
export function executionHighlightsFor(
  st: Pick<
    StudioState,
    | "programChecksum"
    | "compiledChecksum"
    | "sessionStatus"
    | "sessionPaused"
    | "debugState"
    | "_provider"
  >,
  path: string,
): ExecutionHighlight[] {
  if (sessionDegraded(st.programChecksum, st.compiledChecksum)) return [];
  if (
    st.sessionStatus === "none" ||
    st.sessionStatus === "ended" ||
    st.sessionStatus === "error"
  ) {
    return [];
  }
  const pos = st.debugState?.position;
  const provider = st._provider;
  if (!pos || provider === null || !isDebugSessionProvider(provider)) return [];
  const line = provider.resolveDebugLine(pos.container_idx, pos.offset);
  if (line === null || line.file !== path) return [];
  return [
    {
      line: line.line + 1,
      kind: st.sessionPaused ? "paused" : "live",
      rangeStart: line.range_start,
      rangeLen: line.range_len,
    },
  ];
}
