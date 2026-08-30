/**
 * Per-project breakpoint persistence (W4/#3297 — ruled 2026-08-29,
 * "session-only debug state; breakpoints persist per project").
 *
 * Anchors persist as bare `(file, line, enabled)` — bindings are derived
 * state and never stored. Keyed by the host's `sessionScope` (the same
 * per-project scope the editor-tab snapshot uses), so two projects never
 * see each other's breakpoints. Lenient loader, like every settings
 * loader here: corrupt payloads yield the empty set, malformed entries are
 * dropped individually rather than poisoning the rest.
 */

export interface PersistedBreakpoint {
  file: string;
  /** 0-based, matching the store's anchors. */
  line: number;
  enabled: boolean;
}

const KEY_PREFIX = "brink-studio.breakpoints.";
const KEY_SUFFIX = ".v1";

export function breakpointsStorageKey(scope: string): string {
  return `${KEY_PREFIX}${scope}${KEY_SUFFIX}`;
}

/** Load a project's persisted breakpoints. Never throws. */
export function loadBreakpoints(
  storage: Pick<Storage, "getItem">,
  scope: string,
): PersistedBreakpoint[] {
  let raw: string | null;
  try {
    raw = storage.getItem(breakpointsStorageKey(scope));
  } catch {
    return [];
  }
  if (raw === null || raw === "") return [];
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const out: PersistedBreakpoint[] = [];
  for (const entry of parsed) {
    const e = entry as { file?: unknown; line?: unknown; enabled?: unknown } | null;
    if (
      e !== null &&
      typeof e.file === "string" &&
      e.file !== "" &&
      typeof e.line === "number" &&
      Number.isInteger(e.line) &&
      e.line >= 0
    ) {
      out.push({ file: e.file, line: e.line, enabled: e.enabled !== false });
    }
  }
  return out;
}

/** Persist a project's breakpoints. Storage failures degrade to in-session. */
export function saveBreakpoints(
  storage: Pick<Storage, "setItem">,
  scope: string,
  list: readonly PersistedBreakpoint[],
): void {
  try {
    storage.setItem(breakpointsStorageKey(scope), JSON.stringify(list));
  } catch {
    // Quota/denied storage — breakpoints still work for this session.
  }
}
