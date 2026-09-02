/**
 * Problems — clickable diagnostics list (docs/studio-shell-spec.md §4).
 *
 * Compile-bound: renders `diagnosticsList` from the compile slice (already in
 * canonical order — file, offset, errors first). Rows dispatch `editor.reveal`
 * with a source Location (§6.1), so navigation goes through the shared
 * protocol like every other surface. The strip badge (ProblemsBadge) and the
 * status bar's compile segment surface the same counts.
 */

import { memo, useCallback, useMemo, useState } from "react";
import { EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import { lineColAt } from "@brink-lang/editor";
import { isProseDiagnostic, type ProblemSeverityBucket } from "@brink/studio-store";
import type { Diagnostic, FixOffer } from "@brink/wasm-types";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { TODO_DIAGNOSTIC_CODE } from "./TodosView.js";
import { ChevronIcon, FilterIcon, GroupByFileIcon } from "./icons.js";
import {
  ProblemsContextMenu,
  type ProblemsMenuTarget,
} from "./ProblemsContextMenu.js";
import {
  applyOfferedFix,
  countFixes,
  fixAllSafeLabel,
  fixButtonTitle,
  indexFixOffers,
  offersForDiagnostic,
  pullFixOffers,
  runFixAll,
  safeSelect,
  tierLabel,
  type FixProject,
  type FixStoreState,
} from "./fixActions.js";

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

// ── Filtering / grouping (pure, unit-tested) ────────────────────────

/**
 * Which toggle bucket a diagnostic belongs to. Info and Hint share the
 * advisory bucket — the rows already render them identically, and E189
 * TODO notes (Info) are the common case.
 */
export function severityBucket(diagnostic: Diagnostic): ProblemSeverityBucket {
  // Source before severity. Both of these are Info-severity, and letting
  // them fall through to the `info` bucket would make the "off by default"
  // rule impossible to express, since `info` is on.
  if (isProseDiagnostic(diagnostic)) return "prose";
  // TODO notes belong to the TODOs panel; the Problems panel shows them
  // only when asked (ruled 2026-08-29). Before this the only lever was
  // `[lints] E189 = "allow"`, which suppresses at the COMPILER and so
  // emptied the TODOs panel as well.
  if (diagnostic.code === TODO_DIAGNOSTIC_CODE) return "todo";
  if (diagnostic.severity === "Error") return "error";
  if (diagnostic.severity === "Info" || diagnostic.severity === "Hint") return "info";
  return "warning";
}

/** Per-bucket totals over the UNFILTERED list — the toggles show what you
 *  would get back by re-enabling a bucket, so they never count themselves
 *  out of existence. */
export function countBySeverity(
  rows: readonly ProblemRow[],
): Record<ProblemSeverityBucket, number> {
  const counts: Record<ProblemSeverityBucket, number> = {
    error: 0,
    warning: 0,
    info: 0,
    prose: 0,
    todo: 0,
  };
  for (const row of rows) counts[severityBucket(row.diagnostic)] += 1;
  return counts;
}

/** Case-insensitive match over the message and the display location —
 *  the same shape as the TODOs panel's filter. */
export function matchesProblemFilter(row: ProblemRow, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  return (
    row.diagnostic.message.toLowerCase().includes(q) ||
    row.location.toLowerCase().includes(q)
  );
}

/** Apply the severity toggles and the text filter, preserving order. */
export function filterProblemRows(
  rows: readonly ProblemRow[],
  severities: Readonly<Record<ProblemSeverityBucket, boolean>>,
  query: string,
): ProblemRow[] {
  return rows.filter(
    (row) => severities[severityBucket(row.diagnostic)] && matchesProblemFilter(row, query),
  );
}

export interface ProblemFileGroup {
  file: string;
  rows: ProblemRow[];
  counts: Record<ProblemSeverityBucket, number>;
}

/**
 * Group rows by file, preserving both the canonical row order within a
 * file and first-appearance order of the files themselves (the incoming
 * list is already sorted file/offset with errors first, so this keeps that
 * intent rather than imposing a second sort).
 */
export function groupProblemRows(rows: readonly ProblemRow[]): ProblemFileGroup[] {
  const groups = new Map<string, ProblemFileGroup>();
  for (const row of rows) {
    const file = row.diagnostic.file;
    let group = groups.get(file);
    if (!group) {
      group = {
        file,
        rows: [],
        counts: { error: 0, warning: 0, info: 0, prose: 0, todo: 0 },
      };
      groups.set(file, group);
    }
    group.rows.push(row);
    group.counts[severityBucket(row.diagnostic)] += 1;
  }
  return [...groups.values()];
}

/** "2 errors · 1 warning" — omits empty buckets. */
export function summarizeCounts(counts: Record<ProblemSeverityBucket, number>): string {
  const parts: string[] = [];
  if (counts.error > 0) parts.push(`${counts.error} error${counts.error === 1 ? "" : "s"}`);
  if (counts.warning > 0)
    parts.push(`${counts.warning} warning${counts.warning === 1 ? "" : "s"}`);
  if (counts.info > 0) parts.push(`${counts.info} info`);
  // Named for what it is rather than "prose": an author reading a count
  // wants to know it is spelling, not which subsystem produced it.
  if (counts.prose > 0) parts.push(`${counts.prose} spelling`);
  if (counts.todo > 0) parts.push(`${counts.todo} todo`);
  return parts.join(" · ");
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

// ── Header controls (ToolWindowDescriptor.actions) ──────────────────

/** The glyphs the rows already use, so a toggle reads as "this kind of row". */
const BUCKET_GLYPH: Record<ProblemSeverityBucket, string> = {
  error: "\u25CF",
  warning: "\u25B2",
  info: "\u2139",
  // Not a severity glyph: the prose toggle is a different KIND of control
  // and should not read as a fourth severity tier.
  prose: "\u270E",
  todo: "\u2611",
};
const BUCKET_LABEL: Record<ProblemSeverityBucket, string> = {
  error: "errors",
  warning: "warnings",
  info: "info and hints",
  prose: "spelling and grammar",
  todo: "TODO notes",
};
const BUCKETS: ProblemSeverityBucket[] = ["error", "warning", "info", "prose", "todo"];

/**
 * The compile's diagnostics plus the prose checker's findings, as one list.
 *
 * Two producers with different lifetimes — a compile replaces its whole set
 * at once, prose lints arrive per open view on their own debounce — so they
 * are stored apart and joined here, at the only place that wants them
 * together. The `prose` filter bucket is what keeps them out of the way by
 * default; this function deliberately does no filtering of its own, because
 * the bucket counts in the header must show what turning the bucket ON
 * would restore.
 */
function useAllDiagnostics(): readonly Diagnostic[] {
  const compiled = useStudioStore((s) => s.diagnosticsList);
  const prose = useStudioStore((s) => s.proseDiagnostics);
  const dialect = useStudioStore((s) => s.dialectDiagnostics);
  return useMemo(() => {
    const extra = [...Object.values(prose).flat(), ...Object.values(dialect).flat()];
    return extra.length === 0 ? compiled : [...compiled, ...extra];
  }, [compiled, prose, dialect]);
}

/**
 * Every auto-fix the compilation currently offers, indexed by diagnostic
 * identity (`docs/autofix-spec.md` §7).
 *
 * ONE wasm call per compile, not one per row: `diagnostics` is the compile
 * slice's own array, whose identity changes exactly when a compile lands, so
 * the memo re-pulls then and never on a re-render. `project` is in the deps
 * because opening a different project replaces the session under it.
 */
function useFixOfferIndex(
  diagnostics: readonly Diagnostic[],
): ReadonlyMap<string, FixOffer[]> {
  const project = useStudioStore((s) => s._project);
  return useMemo(
    () => indexFixOffers(pullFixOffers(project as FixProject | null)),
    [project, diagnostics],
  );
}

/**
 * Problems controls, rendered by the shell in the panel's chrome header
 * (`ToolWindowDescriptor.actions`). Subscribes to the studio store
 * directly — same contract as {@link ProblemsBadge}.
 *
 * Counts come from the UNFILTERED list so a muted bucket still shows what
 * turning it back on would restore.
 */
function ProblemsActionsInner() {
  const diagnostics = useAllDiagnostics();
  const storeApi = useStudioStoreApi();
  const project = useStudioStore((s) => s._project);
  // "Fix all safe (N)" — `N` from `countFixes`, the batch's OWN count
  // (identical fixes collapsed, the policy's batching gate applied), so the
  // button never promises more than pressing it will do.
  const safeCount = useMemo(
    () => countFixes(project as FixProject | null, safeSelect()),
    [project, diagnostics],
  );
  const severities = useStudioStore((s) => s.problemsSeverities);
  const filterOpen = useStudioStore((s) => s.problemsFilterOpen);
  const grouped = useStudioStore((s) => s.problemsGrouped);
  const toggleSeverity = useStudioStore((s) => s.toggleProblemSeverity);
  const toggleFilter = useStudioStore((s) => s.toggleProblemsFilter);
  const toggleGrouped = useStudioStore((s) => s.toggleProblemsGrouped);

  const counts = useMemo(
    () => countBySeverity(diagnostics.map((diagnostic) => ({ diagnostic, location: "" }))),
    [diagnostics],
  );

  return (
    <>
      {BUCKETS.map((bucket) => {
        const on = severities[bucket];
        return (
          <button
            key={bucket}
            type="button"
            className={`problems-sev-toggle is-${bucket}` + (on ? " on" : " off")}
            aria-pressed={on}
            title={`${on ? "Hide" : "Show"} ${BUCKET_LABEL[bucket]}`}
            data-bucket={bucket}
            onClick={() => toggleSeverity(bucket)}
          >
            <span className="problems-sev-glyph" aria-hidden="true">
              {BUCKET_GLYPH[bucket]}
            </span>
            {counts[bucket]}
          </button>
        );
      })}
      {safeCount > 0 && (
        <>
          <span className="problems-actions-sep" aria-hidden="true" />
          <button
            type="button"
            className="problems-fix-all"
            title="Apply every safe fix in the project"
            onClick={() => {
              void runFixAll(
                storeApi.getState() as unknown as FixStoreState,
                safeSelect(),
                "Fix all safe",
              );
            }}
          >
            {fixAllSafeLabel(safeCount)}
          </button>
        </>
      )}
      <span className="problems-actions-sep" aria-hidden="true" />
      <button
        type="button"
        className={"brink-binder-tool" + (filterOpen ? " active" : "")}
        aria-pressed={filterOpen}
        title="Filter problems"
        aria-label="Filter problems"
        onClick={toggleFilter}
      >
        <FilterIcon />
      </button>
      <button
        type="button"
        className={"brink-binder-tool" + (grouped ? " active" : "")}
        aria-pressed={grouped}
        title={grouped ? "Show as a flat list" : "Group by file"}
        aria-label="Group by file"
        onClick={toggleGrouped}
      >
        <GroupByFileIcon />
      </button>
    </>
  );
}

export const ProblemsActions = memo(ProblemsActionsInner);

// ── View ────────────────────────────────────────────────────────────

/** One diagnostic row. `showFile` is false inside a file group, where the
 *  heading already names the file and only the line is worth repeating. */
function ProblemRowItem({
  row,
  showFile,
  offers,
  onReveal,
  onFix,
  onContextMenu,
}: {
  row: ProblemRow;
  showFile: boolean;
  /** The auto-fixes offered for THIS diagnostic; empty ⇒ no Fix button. */
  offers: readonly FixOffer[];
  onReveal: (row: ProblemRow) => void;
  onFix: (offer: FixOffer) => void;
  onContextMenu: (row: ProblemRow, x: number, y: number) => void;
}) {
  const d = row.diagnostic;
  const bucket = severityBucket(d);
  const label = bucket === "error" ? "Error" : bucket === "info" ? "Info" : "Warning";
  // Inside a group the file prefix is redundant: "file.ink:12:5" -> "12:5".
  const shown = showFile ? row.location : row.location.slice(d.file.length + 1);
  // The first offer is the button; the row's context menu lists them all.
  const primary = offers[0];
  return (
    // The Fix button is a SIBLING of the row button, not nested in it: a
    // button inside a button is invalid HTML and the inner one's click
    // would still reveal the row.
    <li className="problems-item">
      <button
        type="button"
        className="problems-row"
        onClick={() => onReveal(row)}
        onContextMenu={(e) => {
          e.preventDefault();
          onContextMenu(row, e.clientX, e.clientY);
        }}
        title={`${d.message} — ${row.location}`}
      >
        <span className={`problems-severity is-${bucket}`} aria-label={label}>
          {BUCKET_GLYPH[bucket]}
        </span>
        <span className="problems-message">{d.message}</span>
        <span className="problems-location">{shown}</span>
      </button>
      {primary !== undefined && (
        <button
          type="button"
          className={`problems-fix is-${primary.fix.applicability}`}
          // Tier-labelled (§3): how far this fix goes is the one thing an
          // author needs before pressing it, and it differs per row.
          title={fixButtonTitle(primary)}
          data-tier={primary.fix.applicability}
          onClick={() => onFix(primary)}
        >
          Fix
          <span className="problems-fix-tier">{tierLabel(primary.fix.applicability)}</span>
        </button>
      )}
    </li>
  );
}

function ProblemsViewInner() {
  // Right-click suppression (#3148). One menu for the whole panel rather
  // than one per row: only ever one is open, and a per-row menu would mount
  // hundreds of dismiss listeners on a project with many problems.
  const [menu, setMenu] = useState<ProblemsMenuTarget | null>(null);
  const openMenu = useCallback((row: ProblemRow, x: number, y: number) => {
    setMenu({ x, y, diagnostic: row.diagnostic });
  }, []);

  const diagnostics = useAllDiagnostics();
  const storeApi = useStudioStoreApi();
  const fixOffers = useFixOfferIndex(diagnostics);
  const project = useStudioStore((s) => s._project);
  const severities = useStudioStore((s) => s.problemsSeverities);
  const filter = useStudioStore((s) => s.problemsFilter);
  const filterOpen = useStudioStore((s) => s.problemsFilterOpen);
  const grouped = useStudioStore((s) => s.problemsGrouped);
  const collapsedFiles = useStudioStore((s) => s.problemsCollapsedFiles);
  const setFilter = useStudioStore((s) => s.setProblemsFilter);
  const toggleFileCollapsed = useStudioStore((s) => s.toggleProblemsFileCollapsed);
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

  const visible = useMemo(
    () => filterProblemRows(rows, severities, filter),
    [rows, severities, filter],
  );
  const groups = useMemo(
    () => (grouped ? groupProblemRows(visible) : []),
    [grouped, visible],
  );

  const reveal = (row: ProblemRow): void => {
    commands.dispatch(EDITOR_REVEAL_COMMAND_ID, diagnosticLocation(row.diagnostic));
  };

  const fix = (offer: FixOffer): void => {
    void applyOfferedFix(storeApi.getState() as unknown as FixStoreState, offer);
  };

  // The filter row is part of the panel body, not the header: the chrome
  // header is a single slim line and an input does not belong in it. The
  // funnel button in the header toggles this row (binder precedent).
  const filterRow = filterOpen ? (
    <div className="problems-filter">
      <input
        className="problems-filter-input"
        value={filter}
        placeholder="Filter problems"
        spellCheck={false}
        aria-label="Filter problems"
        autoFocus
        onChange={(event) => setFilter(event.target.value)}
      />
    </div>
  ) : null;

  if (rows.length === 0) {
    return (
      <div className="problems-view">
        {filterRow}
        <p className="problems-empty">No problems</p>
      </div>
    );
  }

  if (visible.length === 0) {
    // Distinct from "No problems": the diagnostics exist, the view is
    // hiding them — say which control to undo, or this reads as a bug.
    return (
      <div className="problems-view">
        {filterRow}
        <p className="problems-empty">
          {rows.length} hidden by the current filter
        </p>
      </div>
    );
  }

  return (
    <div className="problems-view">
      {filterRow}
      {grouped ? (
        <div className="problems-groups">
          {groups.map((group) => {
            const collapsed = collapsedFiles[group.file] === true;
            return (
              <section className="problems-group" key={group.file}>
                <button
                  type="button"
                  className="problems-group-header"
                  aria-expanded={!collapsed}
                  onClick={() => toggleFileCollapsed(group.file)}
                >
                  <span className={"problems-group-chevron" + (collapsed ? " collapsed" : "")}>
                    <ChevronIcon />
                  </span>
                  <span className="problems-group-file">{group.file}</span>
                  <span className="problems-group-counts">{summarizeCounts(group.counts)}</span>
                </button>
                {!collapsed && (
                  <ul className="problems-list">
                    {group.rows.map((row, i) => (
                      <ProblemRowItem
                        key={`${row.location}:${i}`}
                        row={row}
                        showFile={false}
                        offers={offersForDiagnostic(fixOffers, row.diagnostic)}
                        onReveal={reveal}
                        onFix={fix}
                        onContextMenu={openMenu}
                      />
                    ))}
                  </ul>
                )}
              </section>
            );
          })}
        </div>
      ) : (
        <ul className="problems-list">
          {visible.map((row, i) => (
            <ProblemRowItem
              key={`${row.location}:${i}`}
              row={row}
              showFile
              offers={offersForDiagnostic(fixOffers, row.diagnostic)}
              onReveal={reveal}
              onFix={fix}
              onContextMenu={openMenu}
            />
          ))}
        </ul>
      )}
      {menu !== null && (
        <ProblemsContextMenu target={menu} onClose={() => setMenu(null)} />
      )}
    </div>
  );
}

export const ProblemsView = memo(ProblemsViewInner);
