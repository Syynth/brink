/**
 * Runtime-value hover policy (W12/#3305, spec §F15 RULED) — the studio
 * half of the merge: the editor identifies the hovered identifier, THIS
 * decides what value (if any) to show. Globals always while a session is
 * live; frame locals only while paused, scoped to the Debugger panel's
 * selected frame (the top frame by default). Suppressed under
 * `sessionDegraded` and outside a live session — suppressed, never
 * stale, same as every source-position feature (spec §5).
 */

import type { DebugValue } from "@brink/wasm-types";
import { sessionDegraded, type StudioState } from "@brink/studio-store";

/** One-line display for a {@link DebugValue} (hover markdown). */
export function debugValueDisplay(v: DebugValue): string {
  switch (v.type) {
    case "int":
    case "float":
      return String(v.value);
    case "bool":
      return v.value ? "true" : "false";
    case "string":
      return JSON.stringify(v.value);
    case "null":
      return "null";
    case "list":
      return `(${v.members.join(", ")})`;
    case "divertTarget":
      return `-> ${v.path ?? "?"}`;
    case "struct":
      return `${v.name ?? "struct"}{${v.fields
        .map((f) => `${f.name}: ${debugValueDisplay(f.value)}`)
        .join(", ")}}`;
    case "handle":
      return `${v.kind}#${v.id}`;
    case "other":
      return v.display;
  }
}

/** The runtime-value markdown note for identifier `name`, or `null`. */
export function runtimeValueNote(
  st: Pick<
    StudioState,
    | "programChecksum"
    | "compiledChecksum"
    | "sessionStatus"
    | "sessionPaused"
    | "selectedFrameIdx"
    | "debugState"
  >,
  name: string,
): string | null {
  if (sessionDegraded(st.programChecksum, st.compiledChecksum)) return null;
  if (
    st.sessionStatus === "none" ||
    st.sessionStatus === "ended" ||
    st.sessionStatus === "error"
  ) {
    return null;
  }
  const debugState = st.debugState;
  if (!debugState) return null;

  // Frame locals first (they shadow globals in scope) — paused only, in
  // the selected frame's scope (RULED: "frame locals while paused in
  // that frame's scope").
  if (st.sessionPaused) {
    const frame = debugState.call_stack[st.selectedFrameIdx ?? 0];
    const local = frame?.locals?.find((l) => l.name === name);
    if (local !== undefined) {
      return `\`${name} = ${debugValueDisplay(local.value)}\` — local, runtime`;
    }
  }

  const global = debugState.globals.find((g) => g.name === name);
  if (global !== undefined) {
    return `\`${name} = ${global.value}\` — global, runtime`;
  }
  return null;
}
