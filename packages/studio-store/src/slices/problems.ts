/**
 * Problems panel view state (beta feedback 2026-08-25: "sorted by file,
 * filtered, and toggles for individual severity levels").
 *
 * Why the store and not React state: the controls live in the tool
 * window's CHROME HEADER (`ToolWindowDescriptor.actions`), which the shell
 * renders in a different subtree from the panel body. The two can only
 * share state through a store — the same reason the strip badge reads the
 * store rather than taking a prop.
 *
 * The durable half — which severities are shown, and whether rows are
 * grouped by file — round-trips through localStorage (ruled 2026-08-25:
 * "on by default, and persisted across refreshes, same with the toggles").
 * These are how an author reads their problem list; re-picking them every
 * launch is exactly the kind of small tax that makes a panel annoying.
 *
 * The filter TEXT deliberately does NOT persist. A query restored into a
 * closed filter row is a panel silently hiding rows with no visible cause —
 * the same failure the clear-on-close rule below exists to prevent.
 *
 * Info stays ON by default. It no longer carries the E189 TODO notes: those
 * moved to their own `todo` bucket, off by default (ruled 2026-08-29). An
 * author who wanted TODOs out of the Problems panel had only
 * `[lints] E189 = "allow"` to reach for — which suppresses the code at the
 * COMPILER, and so emptied the TODO panel too, since that panel reads the
 * same diagnostics. Panel visibility is not a compiler concern.
 */

import type { StateCreator } from "zustand";
import type { ProseLint } from "@brink-lang/editor";
import type { Diagnostic } from "@brink/wasm-types";
import type { StudioState } from "../index.js";

/**
 * The `code` prefix that marks a diagnostic as coming from the prose
 * checker rather than the compiler.
 *
 * A prefix on the existing `code` field rather than a new field on
 * `Diagnostic`: `Diagnostic` is a wasm wire type, and prose lints never
 * cross that boundary — inventing a field there would imply the compiler
 * might one day set it. The full code is `prose:Spelling`,
 * `prose:Repetition`, … so the checker's own rule name survives into the
 * panel.
 */
export const PROSE_CODE_PREFIX = "prose:";

/** Whether a diagnostic came from the prose checker. */
export function isProseDiagnostic(diagnostic: Pick<Diagnostic, "code">): boolean {
  return diagnostic.code?.startsWith(PROSE_CODE_PREFIX) === true;
}

/**
 * The editor's prose findings for `file`, as panel diagnostics.
 *
 * A named function rather than an inline `map` at the call site so the
 * mapping is testable: the call site is `mountStudio`, which no unit test
 * constructs, and every field here is a silent failure if wrong — a missing
 * `file` drops the row from its group, a missing prefix puts spelling among
 * the TODO notes.
 *
 * Offsets pass through unconverted, which is correct and not an oversight:
 * the checker's boundary works in UTF-16 code units, which is also what
 * CodeMirror positions are and what `lineColAt` counts.
 */
export function toProseDiagnostics(
  file: string,
  lints: readonly ProseLint[],
): Diagnostic[] {
  return lints.map((lint) => ({
    start: lint.start,
    end: lint.end,
    message: lint.message,
    // Info: a misspelling is not a claim about the program. The panel
    // buckets it by SOURCE, so this never lands among the E189 TODO notes.
    severity: "Info" as const,
    code: `${PROSE_CODE_PREFIX}${lint.kind}`,
    file,
  }));
}

/**
 * The buckets the panel's toggles expose. Info and Hint share one: both are
 * advisory, and the rows already render them identically.
 *
 * `prose` and `todo` are not severities — they are SOURCES, and they are
 * buckets of their own precisely because they must default OFF while every
 * severity defaults on
 * (ruled: "the Problems panel FILTERS THEM OUT BY DEFAULT; the author opts
 * in to seeing them in the list"). Folding spelling into `info` would put
 * fifty proper nouns on top of the E189 TODO notes an author actually
 * reads, which is the outcome that ruling exists to prevent.
 */
export type ProblemSeverityBucket = "error" | "warning" | "info" | "prose" | "todo";

export interface ProblemsSlice {
  /** Which severity buckets are shown. */
  problemsSeverities: Readonly<Record<ProblemSeverityBucket, boolean>>;
  /** Case-insensitive filter over message + location; "" = no filter. */
  problemsFilter: string;
  /** Whether the filter row is revealed (the funnel button's state). */
  problemsFilterOpen: boolean;
  /** Group rows into collapsible per-file sections. */
  problemsGrouped: boolean;
  /** Collapsed file sections while grouped, keyed by path. */
  problemsCollapsedFiles: Readonly<Record<string, boolean>>;
  /**
   * Prose-checker findings, keyed by file path.
   *
   * Kept SEPARATE from the compile result's diagnostics rather than merged
   * into `diagnosticsList`, because they have different lifetimes: a
   * compile replaces every compile diagnostic at once, while prose lints
   * arrive per open view on their own debounce. Merging them into one list
   * would mean each producer erasing the other's rows — the same
   * `setDiagnostics`-replaces trap the editor's per-source registry exists
   * to avoid, one layer up.
   */
  proseDiagnostics: Readonly<Record<string, readonly Diagnostic[]>>;
  /** Dialogue-dialect findings per file (#3391): `brink.toml [dialogue]`
   *  validation errors keyed to the config file, and the dialect's own
   *  `malformed` near-miss diagnostics on story lines. Real severities
   *  (error/warning), not a source bucket — a broken convention
   *  declaration is a real problem, and it must never hide silently in
   *  the Player. */
  dialectDiagnostics: Readonly<Record<string, readonly Diagnostic[]>>;

  toggleProblemSeverity(bucket: ProblemSeverityBucket): void;
  setProblemsFilter(query: string): void;
  /** Toggle the filter row. Closing it also clears the query, so a hidden
   *  filter can never silently hide rows. */
  toggleProblemsFilter(): void;
  toggleProblemsGrouped(): void;
  toggleProblemsFileCollapsed(file: string): void;
  /** Replace one file's prose findings. An empty array clears them. */
  setProseDiagnostics(file: string, diagnostics: readonly Diagnostic[]): void;
  /** Replace one file's dialect findings (empty = clear). */
  setDialectDiagnostics(file: string, diagnostics: readonly Diagnostic[]): void;
  /** Apply persisted preferences at boot (mount.tsx). */
  applyProblemsPrefs(prefs: ProblemsPrefs): void;
  /** Injected persistence sink; null until the app binds it. Keeps the
   *  slice free of a direct `window` dependency, like `_notify`. */
  _persistProblemsPrefs: ((prefs: ProblemsPrefs) => void) | null;
  setProblemsPrefsSink(sink: (prefs: ProblemsPrefs) => void): void;
}

/** The persisted subset — the view preferences, never the filter text. */
export interface ProblemsPrefs {
  severities: Record<ProblemSeverityBucket, boolean>;
  grouped: boolean;
}

export const PROBLEMS_STORAGE_KEY = "brink-studio.problems.v1";

const DEFAULT_PREFS: ProblemsPrefs = {
  severities: { error: true, warning: true, info: true, prose: false, todo: false },
  grouped: true,
};

/** Load persisted preferences. Never throws; defaults on anything odd. */
export function loadProblemsPrefs(storage: Pick<Storage, "getItem">): ProblemsPrefs {
  let raw: string | null;
  try {
    raw = storage.getItem(PROBLEMS_STORAGE_KEY);
  } catch {
    return DEFAULT_PREFS;
  }
  if (raw === null || raw === "") return DEFAULT_PREFS;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_PREFS;
  }
  const obj = parsed as { severities?: unknown; grouped?: unknown } | null;
  const sev = (obj?.severities ?? {}) as Record<string, unknown>;
  return {
    // Only an explicit `false` hides a severity: a partial or older record
    // must never silently hide diagnostics.
    severities: {
      error: sev.error !== false,
      warning: sev.warning !== false,
      info: sev.info !== false,
      // The opposite default, and the opposite rule: only an explicit
      // `true` shows prose. A record written before this bucket existed
      // has no `prose` key, and reading that as "shown" would turn the
      // panel's spelling rows on for every existing author at once —
      // exactly what defaulting off is for.
      prose: sev.prose === true,
      // Same inverted rule as `prose`, and the same reason: a record
      // written before this bucket existed has no `todo` key, and reading
      // that as "shown" would put TODO notes back in the Problems panel for
      // every existing author on upgrade.
      todo: sev.todo === true,
    },
    grouped: obj?.grouped !== false,
  };
}

/** Persist preferences. Storage failures degrade to in-session. */
export function saveProblemsPrefs(
  storage: Pick<Storage, "setItem">,
  prefs: ProblemsPrefs,
): void {
  try {
    storage.setItem(PROBLEMS_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Quota/denied — the choice still applies for this session.
  }
}

export const createProblemsSlice: StateCreator<StudioState, [], [], ProblemsSlice> = (
  set,
  get,
) => ({
  problemsSeverities: { error: true, warning: true, info: true, prose: false, todo: false },
  problemsFilter: "",
  problemsFilterOpen: false,
  // Grouped by default (ruled): a flat list of every diagnostic in a
  // project reads as noise; per-file sections are how you actually scan it.
  problemsGrouped: true,
  problemsCollapsedFiles: {},
  proseDiagnostics: {},
  dialectDiagnostics: {},

  _persistProblemsPrefs: null,

  setProblemsPrefsSink(sink) {
    set({ _persistProblemsPrefs: sink });
  },

  applyProblemsPrefs(prefs) {
    set({ problemsSeverities: prefs.severities, problemsGrouped: prefs.grouped });
  },

  setDialectDiagnostics(file, diagnostics) {
    const current = get().dialectDiagnostics;
    if (diagnostics.length === 0) {
      if (!(file in current)) return;
      const { [file]: _dropped, ...rest } = current;
      set({ dialectDiagnostics: rest });
      return;
    }
    set({ dialectDiagnostics: { ...current, [file]: diagnostics } });
  },

  setProseDiagnostics(file, diagnostics) {
    const current = get().proseDiagnostics;
    const existing = current[file];
    // A view republishes on every debounce, usually with nothing new. An
    // unconditional `set` here would re-render the panel on every keystroke
    // pause in a document with no prose findings at all.
    if (existing === undefined && diagnostics.length === 0) return;
    if (existing !== undefined && sameDiagnostics(existing, diagnostics)) return;
    if (diagnostics.length === 0) {
      const { [file]: _dropped, ...rest } = current;
      set({ proseDiagnostics: rest });
      return;
    }
    set({ proseDiagnostics: { ...current, [file]: diagnostics } });
  },

  toggleProblemSeverity(bucket) {
    const current = get().problemsSeverities;
    const severities = { ...current, [bucket]: !current[bucket] };
    set({ problemsSeverities: severities });
    get()._persistProblemsPrefs?.({ severities, grouped: get().problemsGrouped });
  },

  setProblemsFilter(query) {
    set({ problemsFilter: query });
  },

  toggleProblemsFilter() {
    const open = !get().problemsFilterOpen;
    // Clearing on close is the load-bearing half: a filter you can't see is
    // a filter you can't explain, and "where did my errors go" is the bug
    // that pattern always produces.
    set({ problemsFilterOpen: open, problemsFilter: open ? get().problemsFilter : "" });
  },

  toggleProblemsGrouped() {
    const grouped = !get().problemsGrouped;
    set({ problemsGrouped: grouped });
    get()._persistProblemsPrefs?.({ severities: get().problemsSeverities, grouped });
  },

  toggleProblemsFileCollapsed(file) {
    const current = get().problemsCollapsedFiles;
    set({ problemsCollapsedFiles: { ...current, [file]: !current[file] } });
  },
});

/** Shallow equality over the fields the panel renders. */
function sameDiagnostics(a: readonly Diagnostic[], b: readonly Diagnostic[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((d, i) => {
    const other = b[i];
    return (
      other !== undefined &&
      d.start === other.start &&
      d.end === other.end &&
      d.message === other.message &&
      d.code === other.code
    );
  });
}
