/**
 * The studio's auto-fix surfaces (`docs/autofix-spec.md` §7), as one module.
 *
 * Four surfaces call the same two verbs — apply ONE fix, or run the batch —
 * so both live here rather than once per component: the Problems row button
 * and its context menu, the editor context menu, and the command palette.
 *
 * **Nothing here recomputes a fix.** `brink-ide` owns the model; this module
 * pulls `getFixOffers` / `countFixes` / `fixAll` off the wasm session and
 * routes their results into `applyMoveResult` — the same undoable seam a
 * rename and a binder move use, so a fix gets the same Undo, the same view
 * refresh, and the same recompile.
 *
 * **A row finds its fixes by the diagnostic's identity**, not by a query per
 * row: one `getFixOffers` call per compile is indexed by
 * `(path, start, end, code)` and every row looks itself up. A Problems panel
 * with hundreds of rows would otherwise make hundreds of wasm calls per
 * render.
 */

import type {
  Applicability,
  Diagnostic,
  Fix,
  FixEdit,
  FixOffer,
  FixReport,
  FixSelect,
  StructuralResult,
} from "@brink/wasm-types";

/**
 * The slice of the wasm session this module needs.
 *
 * Every method is optional: the store's `_project` is a `ProjectSession` in
 * the app but a hand-built stub in tests and in embedders pinned to an older
 * `@brink-lang/web`, and a missing method must read as "no fixes here"
 * rather than throw inside a render.
 */
export interface FixSession {
  getFixOffers?: (select: FixSelect) => FixOffer[];
  countFixes?: (select: FixSelect) => number;
  fixAll?: (select: FixSelect) => FixReport;
  applyFixInFile?: (path: string, fix: Fix) => StructuralResult;
}

/** The slice of `_project` this module needs. */
export interface FixProject {
  getSession: () => FixSession;
}

/** The slice of the studio store this module needs. */
export interface FixStoreState {
  _project: FixProject | null;
  /** `edits` (#3496): a precise, already-known edit list threads through to
   *  the document layer so it can sync mounted views with a minimal change
   *  instead of reloading the whole file (preserving scroll position,
   *  selection, and undo granularity) — optional so a caller with no such
   *  list (or a test double) still type-checks against the seam. */
  applyMoveResult: (
    result: StructuralResult,
    description: string,
    affectedPaths: string[],
    edits?: readonly FixEdit[],
  ) => Promise<void>;
  _notify?: ((n: {
    severity: "info" | "warning" | "error";
    source: string;
    message: string;
  }) => void) | null;
}

// ── Offer lookup ────────────────────────────────────────────────────

/**
 * The identity a Problems row and a `FixOffer` agree on.
 *
 * `JSON.stringify` of a fixed-arity array rather than a delimiter-joined
 * string: a path can contain anything, and a composite key needs an
 * injective encoding (house rule — a literal NUL separator makes the file
 * read as binary to `grep`, and any printable separator can collide).
 */
export function fixOfferKey(
  path: string,
  start: number,
  end: number,
  code: string,
): string {
  return JSON.stringify([path, start, end, code]);
}

/** A diagnostic's key, or `null` when it carries no code (prose findings). */
export function diagnosticFixKey(diagnostic: Diagnostic): string | null {
  if (diagnostic.code === undefined || diagnostic.code === "") return null;
  return fixOfferKey(diagnostic.file, diagnostic.start, diagnostic.end, diagnostic.code);
}

/** Index a compile's offers so each row is an O(1) lookup. */
export function indexFixOffers(offers: readonly FixOffer[]): Map<string, FixOffer[]> {
  const index = new Map<string, FixOffer[]>();
  for (const offer of offers) {
    const key = fixOfferKey(offer.path, offer.start, offer.end, offer.code);
    const list = index.get(key);
    if (list) list.push(offer);
    else index.set(key, [offer]);
  }
  return index;
}

/** The offers for one diagnostic, in the order the compiler offered them. */
export function offersForDiagnostic(
  index: ReadonlyMap<string, FixOffer[]>,
  diagnostic: Diagnostic,
): FixOffer[] {
  const key = diagnosticFixKey(diagnostic);
  if (key === null) return [];
  return index.get(key) ?? [];
}

/**
 * Pull the whole compilation's offers. Returns `[]` — never throws — when
 * the session predates the query or the call fails, so a Problems panel
 * renders its rows either way.
 */
export function pullFixOffers(
  project: FixProject | null,
  select: FixSelect = {},
): FixOffer[] {
  if (project === null) return [];
  try {
    const session = project.getSession();
    return session.getFixOffers?.(select) ?? [];
  } catch {
    return [];
  }
}

/** `countFixes`, with the same never-throws contract as {@link pullFixOffers}. */
export function countFixes(project: FixProject | null, select: FixSelect): number {
  if (project === null) return 0;
  try {
    return project.getSession().countFixes?.(select) ?? 0;
  } catch {
    return 0;
  }
}

// ── Labels ──────────────────────────────────────────────────────────

/**
 * The tier as an author-facing word (`docs/autofix-spec.md` §3).
 *
 * Deliberately not the wire spelling: "placeholder" describes the model, not
 * what pressing the button does to your manuscript.
 */
export function tierLabel(applicability: Applicability): string {
  switch (applicability) {
    case "safe":
      return "Safe";
    case "suggested":
      return "Suggested";
    default:
      return "Needs input";
  }
}

/** The Problems row button's tooltip: what the fix does and how far it goes. */
export function fixButtonTitle(offer: FixOffer): string {
  return `${offer.fix.title} (${tierLabel(offer.fix.applicability).toLowerCase()})`;
}

/**
 * The "Fix all safe (N)" header label. `N` is `countFixes` — the batch's own
 * count, so the button never promises more than the batch will take.
 */
export function fixAllSafeLabel(count: number): string {
  return `Fix all safe (${count})`;
}

/**
 * The two batch commands' ids.
 *
 * Declared here rather than beside `registerFixCommands` because the editor
 * context menu (studio-ui) dispatches "…in this file" and the registration
 * lives in `brink-studio`, which depends on this package — a shared constant
 * is the only way both sides can name the same command without a cycle.
 */
export const FIX_ALL_SAFE_PROJECT_COMMAND_ID = "fix.allSafeInProject";
export const FIX_ALL_SAFE_FILE_COMMAND_ID = "fix.allSafeInFile";

/** The selection the "Fix all safe" surfaces run (spec §7). */
export function safeSelect(path?: string): FixSelect {
  return path === undefined ? { tiers: ["safe"] } : { tiers: ["safe"], path };
}

// ── Applying ────────────────────────────────────────────────────────

/**
 * Apply one offered fix through the studio's undoable apply seam.
 *
 * The fix's own file — `offer.path`, the DIAGNOSTIC's file — is the primary,
 * not whatever the editor happens to be showing: a Problems row can name a
 * file that is not open at all.
 */
export async function applyOfferedFix(
  state: FixStoreState,
  offer: FixOffer,
): Promise<void> {
  const project = state._project;
  if (project === null) return;
  let result: StructuralResult | undefined;
  try {
    result = project.getSession().applyFixInFile?.(offer.path, offer.fix);
  } catch (e) {
    notify(state, "error", `${offer.fix.title} failed: ${message(e)}`);
    return;
  }
  if (result === undefined) return;
  if (!result.ok) {
    notify(state, "error", `${offer.fix.title} failed: ${result.error ?? "refused"}`);
    return;
  }
  // #3496: the fix's own edits are already known precisely (UTF-16
  // file-absolute ranges) — hand them to the apply seam so it can sync
  // mounted views with a minimal change instead of a whole-document
  // replace, keeping the editor's scroll position and selection put.
  await state.applyMoveResult(result, offer.fix.title, [offer.path], offer.fix.edits);
}

/**
 * Turn a `FixReport`'s write list into the `StructuralResult` the apply seam
 * takes: the first file is the primary, the rest ride along as cross-file
 * edits. `null` when the batch wrote nothing.
 *
 * `fixAll` deliberately leaves the wasm session unchanged (it rolls its
 * intermediate rounds back), so this is a genuine "sources to write" — the
 * seam's undo snapshot still captures the pre-fix text.
 */
export function fixReportToStructuralResult(report: FixReport): StructuralResult | null {
  const [primary, ...rest] = report.files;
  if (primary === undefined) return null;
  return {
    ok: true,
    path: primary.path,
    new_source: primary.new_source,
    cross_file_edits: rest.map((f) => ({ path: f.path, new_source: f.new_source })),
    safe: true,
    introduced_diagnostics: [],
  };
}

/**
 * "N fixes applied" — plus what the batch could not finish.
 *
 * The cap is never silent (§5): a report that ran out of rounds says so, and
 * names how many diagnostics still admit a fix. Deferred-overlap counts are
 * reported too, because they are the reason a second run does more.
 */
export function summarizeFixReport(report: FixReport): {
  severity: "info" | "warning";
  message: string;
} {
  const applied = report.applied.length;
  const head =
    applied === 0 ? "No fixes to apply" : `Applied ${applied} fix${applied === 1 ? "" : "es"}`;
  const notes: string[] = [];
  if (report.skipped_overlap > 0) {
    notes.push(`${report.skipped_overlap} deferred by overlap`);
  }
  if (report.cap_hit) {
    notes.push(
      `stopped after ${report.rounds} rounds with ${report.remaining.length} still fixable`,
    );
  }
  return {
    severity: report.cap_hit ? "warning" : "info",
    message: notes.length === 0 ? head : `${head} — ${notes.join("; ")}`,
  };
}

/**
 * Run the batch for `select` and apply what it produced.
 *
 * Returns the report so a caller can assert on it; the notification is
 * raised here so every entry point reports identically.
 */
export async function runFixAll(
  state: FixStoreState,
  select: FixSelect,
  description: string,
): Promise<FixReport | null> {
  const project = state._project;
  if (project === null) return null;
  let report: FixReport | undefined;
  try {
    report = project.getSession().fixAll?.(select);
  } catch (e) {
    notify(state, "error", `${description} failed: ${message(e)}`);
    return null;
  }
  if (report === undefined) return null;
  if (report.error !== undefined) {
    notify(state, "error", `${description} failed: ${report.error}`);
    return report;
  }
  const result = fixReportToStructuralResult(report);
  if (result !== null && result.path) {
    await state.applyMoveResult(result, description, [result.path]);
  }
  const summary = summarizeFixReport(report);
  // A batch that wrote something already raised the seam's own toast (with
  // Undo); adding a second one for the same action would double-report it.
  // The summary still goes out when nothing was written, or when there is
  // something the seam's message cannot say — a hit cap, or deferrals.
  if (result === null || summary.severity === "warning" || report.skipped_overlap > 0) {
    notify(state, summary.severity, summary.message);
  }
  return report;
}

// ── Fix on save (docs/autofix-spec.md §6.2) ─────────────────────────

/**
 * The app-scope "how far may the editor go on save" setting.
 *
 * Three values, the ceiling §6.2 names: off, safe only, everything the
 * project allows. **Default off** — the M4 ceiling is still TENTATIVE, and
 * an editor that silently rewrites a manuscript on every save is not a
 * default anyone opted into.
 */
export type FixOnSaveMode = "off" | "safe" | "project";

/** The default. Off, deliberately (§6.2). */
export const DEFAULT_FIX_ON_SAVE: FixOnSaveMode = "off";

/** Parse a persisted value. Anything unrecognized lands on the default. */
export function parseFixOnSave(value: unknown): FixOnSaveMode {
  return value === "safe" || value === "project" ? value : DEFAULT_FIX_ON_SAVE;
}

/**
 * The `[fix]` ceiling a mode resolves to, or `null` for "do not run".
 *
 * "Safe only" is `"ask"`, not a tier filter: at that ceiling a Safe fix
 * keeps its `auto` tier default and a Suggested fix — promoted by the
 * project or not — resolves to `ask` and is not batched. That is exactly
 * §6.2's "the app can only be more conservative than the project", resolved
 * by `effective_fix_policy` rather than re-derived here.
 */
export function fixOnSaveCeiling(mode: FixOnSaveMode): "ask" | "auto" | null {
  switch (mode) {
    case "safe":
      return "ask";
    case "project":
      return "auto";
    default:
      return null;
  }
}

/** What {@link runFixOnSave} needs of the app. */
export interface FixOnSaveDeps {
  project: FixProject | null;
  /** The studio's shared write seam — refuses a read-only (mounted) path. */
  applyEdit: (path: string, source: string) => boolean;
  /** Make the editor views of a rewritten file reload their text. */
  invalidate: (path: string) => void;
}

/**
 * Run the on-save batch for one file and write what it produced. Returns the
 * paths actually rewritten (empty when the setting is off, when nothing was
 * admitted, or when a write was refused).
 *
 * Deliberately NOT routed through `applyMoveResult`: that seam pushes an
 * undo entry and raises a toast, and an implicit action that fires on every
 * Ctrl-S would turn both into noise. The author's own undo history is the
 * editor's, and the fix lands in the same buffer they are about to save.
 *
 * Synchronous, and it runs BEFORE the save's write (§7, "On save: run on the
 * save road before the write") — so the bytes that reach disk are the fixed
 * ones, not last save's.
 */
export function runFixOnSave(
  deps: FixOnSaveDeps,
  path: string,
  mode: FixOnSaveMode,
): string[] {
  const ceiling = fixOnSaveCeiling(mode);
  if (ceiling === null || deps.project === null) return [];
  let report: FixReport | undefined;
  try {
    report = deps.project.getSession().fixAll?.({ path, ceiling });
  } catch {
    // A batch that cannot run must never fail the save it was riding on.
    return [];
  }
  if (report === undefined || report.error !== undefined) return [];
  const written: string[] = [];
  for (const file of report.files) {
    if (!deps.applyEdit(file.path, file.new_source)) continue;
    deps.invalidate(file.path);
    written.push(file.path);
  }
  return written;
}

function notify(
  state: FixStoreState,
  severity: "info" | "warning" | "error",
  message_: string,
): void {
  state._notify?.({ severity, source: "fix", message: message_ });
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
