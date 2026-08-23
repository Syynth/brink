/**
 * The file-anchored project open model (epic #3021, ruled 2026-08-23 —
 * `docs/decision-log.md` "A project is anchored on a FILE, not a folder").
 *
 * Two doors, both files:
 *
 * 1. **A `.ink` story** — that file IS the entry point (explicit, so a
 *    `brink.toml`'s `[project] entry` never supersedes it; the #2331
 *    precedence applies to host defaults only). The filesystem around it
 *    is shown.
 * 2. **A `brink.toml`** — its `[project] entry` names the entry;
 *    `ProjectSession`'s own discovery applies it (#2331's original case).
 *
 * A third, LEGACY kind survives for old recents entries and `.brink` OS
 * opens: the pre-#3021 folder door (host-fallback entry, config may
 * supersede). Native (`.brink`) is deferred by the same ruling — module
 * identity is root-relative (#1576), so the file door must not extend to
 * it without a separate ruling.
 *
 * Pure decision logic, kept out of `main.tsx` for the same reason
 * `file-open.ts` is: the IO layer stays thin, and "what does opening this
 * path mean?" gets real unit tests
 * (`__tests__/project-open.test.ts`).
 */

import { parentDir } from "./file-open.js";
import type { DiscoveredProjectConfig } from "./tauri-provider.js";

/** What a recents entry (or any anchor path) is, classified lexically —
 *  the path SHAPE carries the door kind, so `recents.json` needs no
 *  migration and no fs round-trip: a `brink.toml` path is the toml door,
 *  a `.ink` path is the story door, anything else is a legacy folder. */
export type RecentKind = "ink" | "toml" | "folder";

export function baseName(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx >= 0 ? path.slice(idx + 1) : path;
}

export function recentKindFor(path: string): RecentKind {
  const base = baseName(path);
  if (base === "brink.toml") return "toml";
  if (base.endsWith(".ink")) return "ink";
  return "folder";
}

/** Everything `openProject` needs to open one anchor path. */
export interface ProjectAnchor {
  kind: RecentKind;
  /** The project root to mount (always a directory). */
  root: string;
  /** Project-relative entry file, when the anchor names one. */
  entryFile: string | null;
  /** Whether that entry is a human's explicit choice (the story door). */
  entryIsExplicit: boolean;
  /** Absolute file to run governing-config discovery from after mount
   *  (the story door's conflict banner), or null. */
  conflictProbe: string | null;
  /** The path recorded in recents — the anchor itself for the two file
   *  doors, the folder for the legacy door. */
  recentPath: string;
}

/** Classify an anchor path into its door, or explain why it cannot open.
 *  A `.toml` that is not `brink.toml` is the one rejected pick the native
 *  file filter cannot exclude (filters match extensions, not basenames). */
export function anchorForPath(path: string): ProjectAnchor | { error: string } {
  const base = baseName(path);
  if (base === "brink.toml") {
    return {
      kind: "toml",
      root: parentDir(path),
      entryFile: null,
      entryIsExplicit: false,
      conflictProbe: null,
      recentPath: path,
    };
  }
  if (base.endsWith(".toml")) {
    return { error: `${base} is not a brink.toml — a project config must be named brink.toml.` };
  }
  if (base.endsWith(".ink")) {
    return {
      kind: "ink",
      root: parentDir(path),
      entryFile: base,
      entryIsExplicit: true,
      conflictProbe: path,
      recentPath: path,
    };
  }
  if (base.endsWith(".brink")) {
    // Deferred native door: open the surrounding folder the pre-#3021 way
    // (host-fallback entry, config may supersede) rather than extending
    // the explicit file door to `.brink` without its own ruling.
    const root = parentDir(path);
    return {
      kind: "folder",
      root,
      entryFile: base,
      entryIsExplicit: false,
      conflictProbe: null,
      recentPath: root,
    };
  }
  // No recognized file extension: a legacy folder recents entry.
  return {
    kind: "folder",
    root: path,
    entryFile: null,
    entryIsExplicit: false,
    conflictProbe: null,
    recentPath: path,
  };
}

/** Landing-screen presentation for one recents entry. */
export interface RecentDisplay {
  kind: RecentKind;
  /** The prominent name: file basename for the file doors, folder name
   *  for the legacy door. */
  name: string;
  /** The dimmed detail: the containing folder, `~`-contracted. For the
   *  toml door the project is the folder, so the detail is that folder. */
  detail: string;
}

export function recentDisplayFor(path: string, home: string | null): RecentDisplay {
  const kind = recentKindFor(path);
  const contract = (p: string): string =>
    home !== null && (p === home || p.startsWith(`${home}/`))
      ? `~${p.slice(home.length)}`
      : p;
  if (kind === "folder") {
    return { kind, name: baseName(path) || path, detail: contract(parentDir(path)) };
  }
  return { kind, name: baseName(path), detail: contract(parentDir(path)) };
}

// ── Conflict banner model (the story door's governing-config warning) ──

/** Everything the conflict banner renders. Built from the shell command's
 *  discovery result; `null` when no config governs the opened file. */
export interface ConflictModel {
  /** Absolute path of the governing brink.toml. */
  configPath: string;
  /** The config's path relative to the opened file's directory
   *  (`brink.toml`, `../brink.toml`, …) — what the banner text shows. */
  relConfig: string;
  /** The config's `[project] entry`, as written, if set. */
  entry: string | null;
  /** Whether the opened file IS the declared entry — the one-click switch
   *  is only offered then (the ruling's exact condition). */
  openedIsEntry: boolean;
  /** The walk-up trace rows for "How the config was found". */
  trace: TraceRow[];
  /** Discovery/parse warnings to surface alongside. */
  warnings: string[];
}

export interface TraceRow {
  step: number;
  /** Display path (shortened relative to the walk). */
  path: string;
  note: string;
  /** The row that found the config renders emphasized. */
  found: boolean;
}

/** `configPath` relative to the opened file's directory: `../` per walked
 *  directory. The walk starts at the file's own dir, so zero walked dirs
 *  means the config sits beside the file. */
export function relativeConfigPath(walkedCount: number): string {
  return `${"../".repeat(walkedCount)}brink.toml`;
}

export function buildConflictModel(
  openedFile: string,
  discovered: DiscoveredProjectConfig | null,
): ConflictModel | null {
  if (discovered === null) return null;
  const walked = discovered.walked;
  const openedBase = baseName(openedFile);
  const trace: TraceRow[] = [
    { step: 1, path: openedBase, note: "opened", found: false },
    ...walked.map((dir, i) => ({
      step: i + 2,
      path: `${baseName(dir)}/`,
      note: "no brink.toml",
      found: false,
    })),
    {
      step: walked.length + 2,
      path: relativeConfigPath(walked.length),
      note:
        discovered.entry !== null
          ? `governs — entry = ${discovered.entry}`
          : "governs",
      found: true,
    },
  ];
  return {
    configPath: discovered.configPath,
    relConfig: relativeConfigPath(walked.length),
    entry: discovered.entry,
    openedIsEntry: discovered.openedIsEntry,
    trace,
    warnings: discovered.warnings,
  };
}

// ── New Project entry validation (mirror of the shell command's) ──

/** Validate a New Project entry-file name. Returns a human-readable
 *  problem, or null when valid. Mirrors `validate_new_project_entry` in
 *  `src-tauri/src/lib.rs` — the command re-checks authoritatively; this
 *  copy only exists so the dialog can disable Create with a reason
 *  instead of round-tripping IPC per keystroke. */
export function validateEntryName(entry: string): string | null {
  if (entry.includes("/") || entry.includes("\\")) {
    return "must be a bare filename, not a path";
  }
  if (!entry.endsWith(".ink")) return "must end in .ink";
  const stem = entry.slice(0, -".ink".length);
  if (stem.length === 0) return "needs a name before .ink";
  if (stem.startsWith(".")) return "must not be a hidden file";
  return null;
}

// ── Launch decision (#3016: reopen last project) ──

export interface BootContext {
  /** The "Reopen last project on launch" setting. */
  reopenLastProject: boolean;
  /** Whether the previous session exited cleanly (the crash guard). */
  previousExitClean: boolean;
  /** Whether a cold-start OS file-open already opened a project — a
   *  double-clicked file always wins over auto-reopen. */
  osOpenHandled: boolean;
  /** Recents, most-recent-first (anchor paths). */
  recents: string[];
}

export type BootAction =
  | { kind: "none" }
  | { kind: "landing"; note?: string }
  | { kind: "reopen"; path: string };

/**
 * What launch should do (#3016), pure and unit-tested. The crash rule is
 * the middle option the issue weighed: honour the preference only after a
 * clean exit — after an abnormal one, show the landing (with the project
 * one click away at the top of recents) and say why, so the author gets a
 * window to choose instead of being walked back into whatever broke.
 */
export function resolveBootAction(ctx: BootContext): BootAction {
  if (ctx.osOpenHandled) return { kind: "none" };
  if (!ctx.reopenLastProject) return { kind: "landing" };
  const last = ctx.recents[0];
  if (last === undefined) return { kind: "landing" };
  if (!ctx.previousExitClean) {
    return {
      kind: "landing",
      note:
        "Reopen last project was skipped — the previous session didn't exit cleanly. " +
        "Your project is one click away below.",
    };
  }
  return { kind: "reopen", path: last };
}
