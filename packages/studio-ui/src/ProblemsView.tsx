/**
 * Problems — clickable diagnostics list (docs/studio-shell-spec.md §4).
 *
 * Compile-bound: renders `diagnosticsList` from the compile slice (already in
 * canonical order — file, offset, errors first). Rows dispatch `editor.reveal`
 * with a source Location (§6.1), so navigation goes through the shared
 * protocol like every other surface. The strip badge (ProblemsBadge) and the
 * status bar's compile segment surface the same counts.
 */

import { memo, useMemo } from "react";
import { EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import { lineColAt } from "@brink-lang/editor";
import type { Diagnostic } from "@brink/wasm-types";
import { useStudioStore } from "./StoreContext.js";

// ── Pure helpers (unit-tested) ──────────────────────────────────────

/**
 * 1-based line:col for an offset into `text` (clamped to the text). The
 * canonical implementation is the published boundary helper `lineColAt`
 * (@brink-lang/editor, #369); the old name is kept for existing consumers.
 */
export const offsetToLineCol = lineColAt;

export interface ProblemRow {
  diagnostic: Diagnostic;
  /** "file.ink:12:5" when source text is available; "file.ink@offset" fallback. */
  location: string;
}

/**
 * Decorate diagnostics with display locations. `getSource` is consulted once
 * per file (null = source unavailable → offset fallback). Order is preserved.
 */
export function buildProblemRows(
  diagnostics: readonly Diagnostic[],
  getSource: (file: string) => string | null,
): ProblemRow[] {
  const sources = new Map<string, string | null>();
  return diagnostics.map((diagnostic) => {
    let text = sources.get(diagnostic.file);
    if (text === undefined) {
      text = getSource(diagnostic.file);
      sources.set(diagnostic.file, text);
    }
    let location: string;
    if (text !== null) {
      const { line, col } = offsetToLineCol(text, diagnostic.start);
      location = `${diagnostic.file}:${line}:${col}`;
    } else {
      location = `${diagnostic.file}@${diagnostic.start}`;
    }
    return { diagnostic, location };
  });
}

/** The `editor.reveal` argument for a diagnostic (spec §6.1, source space). */
export function diagnosticLocation(diagnostic: Diagnostic) {
  return {
    kind: "source" as const,
    file: diagnostic.file,
    span: { start: diagnostic.start, end: diagnostic.end },
  };
}

// ── Strip badge ─────────────────────────────────────────────────────

/**
 * Problems strip badge (spec §5.1): error count bubble, hidden when clean.
 * Registered as the descriptor's `badge` component — it subscribes to the
 * studio store itself, so the count stays live without the shell knowing
 * about the store.
 */
export function ProblemsBadge() {
  const errors = useStudioStore((s) => s.diagnostics.errors);
  if (errors === 0) return null;
  return <span className="shell-strip-badge">{errors > 99 ? "99+" : errors}</span>;
}

// ── View ────────────────────────────────────────────────────────────

function ProblemsViewInner() {
  const diagnostics = useStudioStore((s) => s.diagnosticsList);
  const project = useStudioStore((s) => s._project);
  const { commands } = useShell();

  // line:col is resolved against the wasm session's current file sources —
  // resolution deferred to render time (latest text), memoized per list.
  const rows = useMemo(
    () =>
      buildProblemRows(diagnostics, (file) => {
        if (!project) return null;
        try {
          return project.getSession().getFileSource(file);
        } catch {
          return null;
        }
      }),
    [diagnostics, project],
  );

  if (rows.length === 0) {
    return (
      <div className="problems-view">
        <p className="problems-empty">No problems</p>
      </div>
    );
  }

  return (
    <div className="problems-view">
      <ul className="problems-list">
        {rows.map((row, i) => {
          const d = row.diagnostic;
          const isError = d.severity === "Error";
          return (
            <li key={`${row.location}:${i}`}>
              <button
                type="button"
                className="problems-row"
                onClick={() => commands.dispatch(EDITOR_REVEAL_COMMAND_ID, diagnosticLocation(d))}
                title={`${d.message} — ${row.location}`}
              >
                <span
                  className={"problems-severity " + (isError ? "is-error" : "is-warning")}
                  aria-label={isError ? "Error" : "Warning"}
                >
                  {isError ? "●" : "▲"}
                </span>
                <span className="problems-message">{d.message}</span>
                <span className="problems-location">{row.location}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export const ProblemsView = memo(ProblemsViewInner);
