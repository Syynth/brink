/**
 * `[project] unprune-dirs` — which of discovery's always-skipped directories
 * this project wants walked anyway.
 *
 * The value set is CLOSED, and that is the whole reason this is not another
 * free-text list like [[draft-globs]] or the prose dictionary. The walker
 * skips exactly `target/`, `.git/` and `node_modules/`; naming anything else
 * un-prunes nothing, and `brink-project-config` already answers such an
 * entry with "it was never pruned, so this has no effect (check for a
 * typo)". A text field here could therefore only produce one of three right
 * answers or a silent no-op, so the surface offers the three.
 *
 * `PRUNABLE_DIRS` restates a Rust constant, which is a real risk — it is
 * pinned against `brink_source_tree::IGNORED_DIR_NAMES` by
 * `unprune-dirs.test.ts`, which reads that constant out of the Rust source
 * rather than repeating it. If the walker's policy gains a fourth
 * directory, that test fails until this list follows.
 */

import { getTomlStringArray, setTomlStringArray } from "./toml-edit.js";

const TABLE = "project";
const KEY = "unprune-dirs";

/**
 * The directories discovery always skips — the only names this key can
 * meaningfully hold.
 *
 * Must equal `brink_source_tree::IGNORED_DIR_NAMES`, in any order.
 */
export const PRUNABLE_DIRS = ["target", ".git", "node_modules"] as const;

export type PrunableDir = (typeof PRUNABLE_DIRS)[number];

/**
 * The directories `source` asks discovery to walk, in file order.
 *
 * Entries outside {@link PRUNABLE_DIRS} are kept rather than filtered: they
 * are already the subject of a config warning, and quietly dropping one
 * here would let a "fix" that is really a typo look applied.
 */
export function unprunedDirs(source: string): string[] {
  return getTomlStringArray(source, TABLE, KEY) ?? [];
}

/** Whether `dir` is currently un-pruned. */
export function isUnpruned(source: string, dir: string): boolean {
  return unprunedDirs(source).includes(dir);
}

/**
 * `source` with `dir` un-pruned, or null when it already was.
 *
 * Null rather than an equal string so the caller can skip the write: an
 * applied no-op still dirties the file and triggers a recompile.
 */
export function withUnprunedDir(source: string, dir: string): string | null {
  const existing = unprunedDirs(source);
  if (existing.includes(dir)) return null;
  // Written in PRUNABLE_DIRS order rather than click order, so the file does
  // not churn by the sequence the boxes happened to be ticked in.
  const next = [...existing, dir].sort(
    (a, b) => prunableIndex(a) - prunableIndex(b) || a.localeCompare(b),
  );
  return setTomlStringArray(source, TABLE, KEY, next);
}

/**
 * `source` with `dir` pruned again, or null when it already was.
 *
 * Removing the last entry leaves `unprune-dirs = []`, which reads the same
 * as an absent key — the standing policy — and keeps the author's line in
 * the file to toggle again.
 */
export function withoutUnprunedDir(source: string, dir: string): string | null {
  const existing = unprunedDirs(source);
  if (!existing.includes(dir)) return null;
  return setTomlStringArray(
    source,
    TABLE,
    KEY,
    existing.filter((d) => d !== dir),
  );
}

/** Position in the canonical order; unknown names sort last, stably. */
function prunableIndex(dir: string): number {
  const i = (PRUNABLE_DIRS as readonly string[]).indexOf(dir);
  return i === -1 ? PRUNABLE_DIRS.length : i;
}
