/**
 * The author's prose dictionary, stored as `[prose] dictionary` in
 * `brink.toml`.
 *
 * Two consumers reach these functions from opposite directions — the
 * editor's "Add to dictionary" tooltip action and the Prose settings
 * panel's list — and they must agree exactly, because the second is where
 * an author goes to check that the first worked. A word added by the
 * tooltip and not visible in the panel is the bug this file exists to make
 * impossible, and it is the bug that shipped when the list lived in a
 * sidecar dotfile with no UI at all.
 *
 * Everything here is a pure `source -> source` string transform: the
 * caller owns reading the file and writing it back, so the same functions
 * serve a React panel and a CodeMirror action without either learning about
 * the other's world.
 */

import { getTomlStringArray, setTomlStringArray } from "./toml-edit.js";

/** The `brink.toml` table and key the list lives under. */
const TABLE = "prose";
const KEY = "dictionary";

/**
 * The words `source` declares, in file order.
 *
 * File order, not sorted: this is the author's own list shown back to them,
 * and re-sorting the view would disagree with their file whenever they have
 * grouped it by hand.
 */
export function dictionaryWords(source: string): string[] {
  return getTomlStringArray(source, TABLE, KEY) ?? [];
}

/**
 * `source` with `word` added, or null when nothing would change.
 *
 * Null rather than an identical string so a caller can skip the write
 * entirely — applying a no-op edit still marks the file dirty and triggers
 * a recompile, which is how "add a word already in the list" turns into a
 * spurious rebuild.
 *
 * Sorted on write so the file does not churn by insertion order: two
 * authors adding the same two words in different orders should produce the
 * same file, not a merge conflict.
 */
export function withDictionaryWord(source: string, word: string): string | null {
  const trimmed = word.trim();
  if (trimmed === "") return null;
  const existing = dictionaryWords(source);
  // Exact-match, deliberately: dictionary matching is literal for now
  // (decision log, 2026-08-28), so `Griswold` and `GRISWOLD` are two
  // entries and folding them together here would silently drop one.
  if (existing.includes(trimmed)) return null;
  return setTomlStringArray(source, TABLE, KEY, sorted([...existing, trimmed]));
}

/** `source` with `word` removed, or null when it was not there. */
export function withoutDictionaryWord(source: string, word: string): string | null {
  const existing = dictionaryWords(source);
  if (!existing.includes(word)) return null;
  return setTomlStringArray(
    source,
    TABLE,
    KEY,
    existing.filter((w) => w !== word),
  );
}

/** Case-insensitive-ish sort, so `Ada` and `bo` interleave as a reader expects. */
function sorted(words: readonly string[]): string[] {
  return [...words].sort((a, b) => a.localeCompare(b));
}
