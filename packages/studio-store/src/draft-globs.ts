/**
 * The author's draft globs, stored as `[project] drafts` in `brink.toml`.
 *
 * A sibling of `prose-dictionary.ts` and deliberately the same shape — pure
 * `source -> source` string transforms, so the caller owns reading the file
 * and writing it back. The settings panel is one caller; a "mark as draft"
 * action on a file would be another, and the two must agree exactly.
 *
 * What a draft IS lives in Rust, not here. A file is a draft when it matches
 * one of these globs **and** the entry does not reach it (the "reachability
 * wins" ruling, 2026-08-27). This module only edits the glob half — ask
 * `getDraftGlobReport` what any of it actually matched. Recomputing draft
 * status in TS would be a second implementation of the ruling, free to
 * drift from the one the compiler uses.
 */

import { getTomlStringArray, setTomlStringArray } from "./toml-edit.js";

/** The `brink.toml` table and key the list lives under. */
const TABLE = "project";
const KEY = "drafts";

/**
 * The globs `source` declares, in file order.
 *
 * File order, not sorted: this is the author's own list shown back to them,
 * matching how the prose dictionary is read.
 */
export function draftGlobs(source: string): string[] {
  return getTomlStringArray(source, TABLE, KEY) ?? [];
}

/**
 * `source` with `glob` added, or null when nothing would change.
 *
 * Null rather than an identical string so the caller can skip the write —
 * an applied no-op still marks the file dirty and triggers a recompile.
 *
 * Sorted on write, like the dictionary: globs are an unordered set as far
 * as matching is concerned (any one of them matching is enough), so
 * insertion order carries no meaning and preserving it would only churn the
 * file between authors.
 */
export function withDraftGlob(source: string, glob: string): string | null {
  const trimmed = glob.trim();
  if (trimmed === "") return null;
  const existing = draftGlobs(source);
  if (existing.includes(trimmed)) return null;
  return setTomlStringArray(source, TABLE, KEY, sorted([...existing, trimmed]));
}

/** `source` with `glob` removed, or null when it was not there. */
export function withoutDraftGlob(source: string, glob: string): string | null {
  const existing = draftGlobs(source);
  if (!existing.includes(glob)) return null;
  return setTomlStringArray(
    source,
    TABLE,
    KEY,
    existing.filter((g) => g !== glob),
  );
}

/**
 * Why `glob` cannot be stored, or null when it is fine.
 *
 * Deliberately thin — this rejects what `brink.toml` or the glob dialect
 * cannot represent, not what looks unusual. A glob matching nothing today
 * is perfectly valid (the folder may not exist yet), and the settings view
 * reports that from the real match report rather than guessing here.
 */
export function draftGlobProblem(glob: string): string | null {
  const trimmed = glob.trim();
  if (trimmed === "") return "Enter a path or pattern.";
  if (trimmed.includes('"')) return "A pattern cannot contain a quote.";
  if (trimmed.startsWith("/")) return "Use a path relative to the project, like scenes/cut/**.";
  if (trimmed.startsWith("../") || trimmed === "..")
    return "A pattern cannot reach outside the project.";
  return null;
}

/** Case-insensitive-ish sort, matching the dictionary's. */
function sorted(globs: readonly string[]): string[] {
  return [...globs].sort((a, b) => a.localeCompare(b));
}
