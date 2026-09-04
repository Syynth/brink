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
import type { HirProjection, HirSpan } from "@brink/wasm-types";
import {
  isDebugSessionProvider,
  sessionDegraded,
  type StudioState,
} from "@brink/studio-store";

/** The file's HIR overlay, or a thunk that produces it.
 *
 * A thunk is the road hosts should take (#3490): the projection is only
 * consulted on the choice-point branch, and pulling it eagerly costs a
 * synchronous `getHirSpansDoc` on every call — including the overwhelmingly
 * common "no session at all" case, where the answer is `[]` regardless. */
export type ProjectionSource =
  | HirProjection
  | null
  | undefined
  | (() => HirProjection | null);

/** Resolve a {@link ProjectionSource} at most once. */
function projectionOnce(source: ProjectionSource): () => HirProjection | null {
  if (typeof source !== "function") return () => source ?? null;
  let resolved = false;
  let value: HirProjection | null = null;
  return () => {
    if (!resolved) {
      value = source();
      resolved = true;
    }
    return value;
  };
}

/** The studio's `getExecutionHighlights` host hook, as the play gutter
 * consumes it (#3490).
 *
 * The seam matters as much as the policy behind it. The gutter asks once
 * per render, and `getHirProjection` is a synchronous whole-document
 * `getHirSpansDoc` query — so resolving it at the call site pulls it on
 * EVERY ask, including the overwhelmingly common "no session" one whose
 * answer is `[]` regardless. It rides in as a thunk instead, and only the
 * choice-point branch resolves it.
 *
 * Named and exported (the `location-resolvers.ts` pattern this module
 * already follows) so the WIRING is testable and not just the policy:
 * inlined at the mount site, an eagerly-evaluated argument here is exactly
 * the defect #3490 measured, and nothing could have caught it. */
export function executionHighlightsHook(
  getState: () => StudioState,
  getProjection: (path: string) => HirProjection | null,
): (path: string) => ExecutionHighlight[] {
  return (path) => executionHighlightsFor(getState(), path, () => getProjection(path));
}

/** All execution highlights for `path`, from the live session.
 *
 * `projection` (W11/#3304) is the file's HIR overlay — when the session
 * waits on a choice, presented choices light and their rejected siblings
 * dim with reasons; without it (unopened doc, projection not landed) the
 * choice point falls back to the single position band. Pass a thunk
 * (#3490) so the pull happens only on the branch that uses it. */
export function executionHighlightsFor(
  st: Pick<
    StudioState,
    | "programChecksum"
    | "compiledChecksum"
    | "sessionStatus"
    | "sessionPaused"
    | "debugState"
    | "selectedFrameIdx"
    | "_provider"
    | "sessionLines"
    | "followInEditor"
    | "followPaused"
    | "sessionHoverSource"
    | "sessionPeek"
    | "_resolveSourceBytes"
  >,
  path: string,
  projection?: ProjectionSource,
): ExecutionHighlight[] {
  const out = coreHighlights(st, path, projectionOnce(projection));
  // Bars over tints (ruled 2026-09-03): follow / hover / peek are bar-only
  // attention marks and STACK on a tinted line — a line where play is can
  // also be the one just revealed, hovered, or forecast. No dedupe.
  // Follow (#3437): the last revealed line's source, banded, while the
  // Player plays and follow is on and not paused by an edit. Not while
  // paused — the paused band already says where play is.
  const playing = st.sessionStatus === "running" || st.sessionStatus === "awaiting-choice";
  const resolve = st._resolveSourceBytes;
  if (
    st.followInEditor &&
    !st.followPaused &&
    !st.sessionPaused &&
    playing &&
    resolve &&
    !sessionDegraded(st.programChecksum, st.compiledChecksum)
  ) {
    const last = lastLineWithSource(st.sessionLines);
    if (last?.source && last.source.file === path) {
      const point = resolve(last.source.file, last.source.range_start, last.source.range_end);
      if (point !== null) out.push(band(point, "follow"));
    }
  }
  // Hover (#3437): the transcript row under the pointer.
  const hover = st.sessionHoverSource;
  if (hover !== null && hover.file === path && resolve) {
    const point = resolve(hover.file, hover.range_start, hover.range_end);
    if (point !== null) out.push(band(point, "hover"));
  }
  // Peek (ruled 2026-09-03): what the hovered Continue / choice would hit.
  for (const src of st.sessionPeek ?? []) {
    if (src.file !== path || !resolve) continue;
    const point = resolve(src.file, src.range_start, src.range_end);
    if (point !== null) out.push(band(point, "peek"));
  }
  return out;
}

/** A band over every source line the point spans — a transcript line
 *  built from several source lines (glue, cue + aside + dialogue) lights
 *  as the one line it reads as (feedback 2026-09-02). */
function band(
  point: { line: number; endLine?: number },
  kind: ExecutionHighlight["kind"],
): ExecutionHighlight {
  const line = point.line + 1;
  const endLine = (point.endLine ?? point.line) + 1;
  return endLine > line ? { line, endLine, kind } : { line, kind };
}

/** The newest transcript line that knows where it came from. */
export function lastLineWithSource(
  lines: readonly { source?: { file: string; range_start: number; range_end: number } }[],
): { source: { file: string; range_start: number; range_end: number } } | null {
  for (let i = lines.length - 1; i >= 0; i--) {
    const l = lines[i];
    if (l.source) return { source: l.source };
  }
  return null;
}

function coreHighlights(
  st: Pick<
    StudioState,
    | "programChecksum"
    | "compiledChecksum"
    | "sessionStatus"
    | "sessionPaused"
    | "debugState"
    | "selectedFrameIdx"
    | "_provider"
  >,
  path: string,
  projection: () => HirProjection | null,
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
  const out: ExecutionHighlight[] = [];

  // Choice-point visualization (W11/#3304, F14 RULED): presented choices
  // ARE the live frontier — each lights; authored siblings not added dim
  // with the by-elimination reason. Joins run on `def_id` (#3234).
  // The projection is pulled ONLY here (#3490): every other road out of
  // this function answers without it, and the pull is a synchronous
  // whole-document wasm query.
  const choiceProjection = st.sessionStatus === "awaiting-choice" ? projection() : null;
  const choiceBands =
    choiceProjection !== null
      ? choicePointHighlights(st.debugState, choiceProjection)
      : [];

  if (line !== null && line.file === path) {
    const positionLine = line.line + 1;
    // With the presented set lit, the single position band is redundant
    // noise UNLESS paused (the paused band is the stop marker, F7) —
    // and a choice band never doubles a line the paused band holds.
    if (st.sessionPaused || choiceBands.length === 0) {
      out.push({
        line: positionLine,
        kind: st.sessionPaused ? "paused" : "live",
        rangeStart: line.range_start,
        rangeLen: line.range_len,
      });
    }
    for (const band of choiceBands) {
      if (!(st.sessionPaused && band.line === positionLine)) out.push(band);
    }
  } else {
    out.push(...choiceBands);
  }
  // A selected non-top stack frame (W8/#3301) coexists with the paused
  // band — the plural seam's second consumer: accent band + hollow arrow
  // at the frame's resume position.
  const frameIdx = st.selectedFrameIdx;
  if (st.sessionPaused && frameIdx !== null) {
    const framePos = st.debugState?.call_stack?.[frameIdx]?.position;
    if (framePos) {
      const frameLine = provider.resolveDebugLine(
        framePos.container_idx,
        framePos.offset,
      );
      if (
        frameLine !== null &&
        frameLine.file === path &&
        // The top position already carries the paused band — don't
        // double-mark the same line.
        frameLine.line + 1 !== out[0]?.line
      ) {
        out.push({
          line: frameLine.line + 1,
          kind: "frame",
          rangeStart: frameLine.range_start,
          rangeLen: frameLine.range_len,
        });
      }
    }
  }
  return out;
}

/** The presented/rejected bands for the CURRENT choice point (W11/#3304).
 *
 * Presented = `pending_choices` (their `def_id` joins the projection's
 * choice spans). Rejected = choice spans sharing a lit span's choice
 * point — same parent container and weave depth — that were not
 * presented; reason by elimination: a once-only whose anonymous body has
 * a `visit_ids` count ≥ 1 is "once-only · used", anything else is the
 * failing condition (a catch-all — the editor enriches it with the
 * line's own `{…}` text). */
function choicePointHighlights(
  debugState: StudioState["debugState"],
  projection: HirProjection,
): ExecutionHighlight[] {
  if (!debugState) return [];
  const presented = new Set(
    debugState.pending_choices
      .map((c) => c.def_id)
      .filter((id): id is string => id !== undefined && id !== ""),
  );
  if (presented.size === 0) return [];
  const visitById = new Map(
    (debugState.visit_ids ?? []).map((v) => [v.def_id, v.count]),
  );

  const spans = projection.spans;
  const containers = spans.filter((sp) => sp.container);
  // The innermost container strictly enclosing a span — the choice
  // point's grouping key, paired with weave depth (an inline choice set
  // inherits the surrounding weave's depth; the pair keeps nested sets
  // apart). O(choices × containers), fine at file scale.
  const parentOf = (sp: HirSpan): HirSpan | null => {
    let best: HirSpan | null = null;
    for (const c of containers) {
      if (c === sp) continue;
      const encloses =
        (c.start_line < sp.start_line ||
          (c.start_line === sp.start_line && c.start_char <= sp.start_char)) &&
        (c.end_line > sp.end_line ||
          (c.end_line === sp.end_line && c.end_char >= sp.end_char));
      if (!encloses) continue;
      if (best === null || c.depth > best.depth) best = c;
    }
    return best;
  };
  const groupKey = (sp: HirSpan): string => {
    const parent = parentOf(sp);
    return `${parent?.handle ?? "root"}|${sp.weave_depth ?? "?"}`;
  };

  const choiceSpans = spans.filter((sp) => sp.kind === "choice" && sp.def_id !== undefined);
  const litKeys = new Set<string>();
  const out: ExecutionHighlight[] = [];
  for (const sp of choiceSpans) {
    if (sp.def_id !== undefined && presented.has(sp.def_id)) {
      litKeys.add(groupKey(sp));
      out.push({ line: sp.start_line + 1, kind: "live" });
    }
  }
  if (out.length === 0) return [];
  for (const sp of choiceSpans) {
    if (sp.def_id === undefined || presented.has(sp.def_id)) continue;
    if (!litKeys.has(groupKey(sp))) continue;
    const used = sp.sticky === false && (visitById.get(sp.def_id) ?? 0) >= 1;
    out.push({
      line: sp.start_line + 1,
      kind: "rejected",
      note: used ? "once-only · used" : "condition false",
    });
  }
  return out;
}
