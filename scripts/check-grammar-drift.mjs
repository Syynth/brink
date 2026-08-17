// Mechanical guard against `INLINE_WS+`-vs-`skip_ws` grammar-comment drift
// (#2718, #2719). Follow-up from PR #2712's review (delivering #2707), w173
// retro.
//
// The gap. `brink-syntax`'s parser exposes exactly ONE whitespace-consuming
// primitive — `Parser::skip_ws` (`crates/internal/brink-syntax/src/parser/
// mod.rs`) — and it matches ZERO OR MORE. Nothing in this codebase requires
// whitespace. So the pest-notation token `INLINE_WS+` (one-or-more) is, as of
// today, ALWAYS a lie about what the parser does wherever it appears as part
// of a reproduced grammar production — and that lie has now surfaced FOUR
// separate times, each found only by hand-grep during a review:
//
//   #2695 (knot.rs's own `stitch_header` doc comment)
//   -> #2701's review (found the same shape recurring)
//   -> #2707 / PR #2712 (six more in declaration.rs, a seventh in knot.rs)
//   -> PR #2712's OWN review found two more, in docs/brink-studio-spec.md
//      and the studio mock
//   -> #2719 named two of those as still present-tense at the time it was
//      filed
//   -> this file's own audit (#2718) found the #2719 count itself was
//      incomplete: FIVE stale present-tense quotations were still live
//      across docs/brink-studio-spec.md, structural-refusal-shape.test.ts,
//      the studio mock and crates/brink-web/src/editor_refactor.rs (two
//      sites) — not the two #2719 named.
//
// Four rounds of hand enumeration, each incomplete. The fix is mechanical,
// not a fifth sweep.
//
// The rule — what counts as STALE vs. a legitimate HISTORICAL MENTION.
// `INLINE_WS+` is not bannable outright: `knot.rs`'s own comment legitimately
// says "matching the old, wrong `INLINE_WS+` prose" — naming the token while
// explicitly flagging it as superseded. A guard that flagged every occurrence
// of the string would fail on that honest, ALREADY-CORRECT prose (the false-
// positive failure mode #2689's SKIPPED_RE hit on `node --test`'s own
// canonical output — this file exists to avoid the same class of mistake).
//
// The actual distinguishing signal, verified against every occurrence in the
// repo as of #2718 (both the legitimate ones and the five stale ones found
// here): a legitimate historical mention ALWAYS pairs the `INLINE_WS+` quote,
// within the same few lines, with an explicit acknowledgment that the source
// has since been corrected — "the comment now says `INLINE_WS*`", "the old,
// wrong `INLINE_WS+` prose", "used to document/say/spell", "predated #2695",
// "mismatch was fixed separately". A stale quote never carries that
// acknowledgment — it just says "the comment says `INLINE_WS+`" (or
// "documents ... as ... INLINE_WS+") in the present tense, with nothing
// nearby noting the source has moved on.
//
// So: every occurrence of the literal token `INLINE_WS+` anywhere in the
// repo (outside build/dependency output) must have one of the marker phrases
// in `HISTORICAL_MARKER_RE` within `CONTEXT_WINDOW_LINES` lines. No marker
// nearby => flagged as a stale grammar claim.
//
// This is deliberately NOT scoped to brink-syntax's own `///` doc comments —
// #2719's two named sites and three of the five actually found here live in
// docs/brink-studio-spec.md, a TypeScript test, and a TypeScript mock, none
// of which `cargo test -p brink-syntax` would ever see. The drift travels
// wherever the grammar gets quoted, so the scan has to be repo-wide — the
// same lesson CLAUDE.md's "Cloud / fresh-environment sessions" section
// already draws for network-fetch bounds (0-for-3 by hand, then a fourth
// round finding the enumeration of WHICH FILES to scan was itself
// incomplete). This lives in scripts/ (run by `pnpm test:scripts`, CI's
// `frontend` job, non-recursive `scripts/*.test.mjs` glob) rather than as a
// `cargo test -p brink-syntax` unit test for exactly that reason: the sites
// this guard exists to catch are NOT all Rust files.
//
// Exported as pure functions over text so check-grammar-drift.test.mjs can
// drive them with synthetic planted-drift input; the CLI at the bottom
// applies them to the real repo.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");

/** The literal drift token this guard exists to police. */
export const STALE_TOKEN = "INLINE_WS+";

/**
 * How many lines on either side of a `STALE_TOKEN` occurrence count as
 * "nearby" for `HISTORICAL_MARKER_RE`. The largest distance measured across
 * every legitimate mention in the repo today is 7 lines
 * (crates/internal/brink-syntax/src/parser/tests/knot/cst.rs, where the
 * marker phrase wraps onto the following comment line's continuation) —
 * verified by sweeping `windowLines` over every real occurrence: the repo is
 * clean at 7 and fails at 6. Kept at 8, one line of margin above the
 * measured need, rather than a much wider blanket: a review on PR #2723
 * found that a stale claim planted 15 lines away from an unrelated
 * `INLINE_WS+` mention stayed invisible to this guard at the old
 * `CONTEXT_WINDOW_LINES = 15`, because the wide window let an unrelated
 * marker phrase satisfy it by proximity alone.
 */
export const CONTEXT_WINDOW_LINES = 8;

/**
 * Phrases that mark an `INLINE_WS+` quotation as an acknowledged HISTORICAL
 * mention rather than a present-tense claim about current source. Broad on
 * purpose — a false NEGATIVE here just means a future drift waits one more
 * review to be caught; a false POSITIVE blocks honest prose outright, which
 * is the failure mode this guard exists to avoid (see file header).
 */
export const HISTORICAL_MARKER_RE =
  /now\s+says|old,?\s*wrong|pre-?(?:dated)?\s*#\d+|used to\b|fixed separately|mismatch was\s+fixed|no longer\b/i;

/**
 * Directories never scanned: dependency and build output. Mirrors
 * check-scripts.mjs's PRUNED_DIRS (same rationale — everything else in the
 * repo IS walked).
 */
export const PRUNED_DIRS = new Set([".git", "node_modules", "target", "dist", "dist-embed", "pkg"]);

/**
 * Extensions scanned — docs (`.md`), Rust source (`.rs`), and TypeScript
 * source (`.ts`/`.tsx`/`.mts`/`.cts`), the three surfaces #2718/#2719
 * actually found drift in.
 *
 * `.mjs`/`.js`/`.mdx` are deliberately NOT in this list, not an oversight:
 * this file's own `STALE_TOKEN = "INLINE_WS+"` literal and
 * `check-grammar-drift.test.mjs`'s deliberately-unmarked planted fixtures
 * would both trip the guard the moment `.mjs` were scanned (verified:
 * scanning this file and its test as `.mjs` sources reports 4 and 7
 * unmarked occurrences respectively). If `.mjs`/`.js`/`.mdx` is ever added
 * here, exempt `scripts/check-grammar-drift*.mjs` by path first. No test
 * covers this extension-selection choice today.
 */
const SCANNED_EXTENSIONS = [".md", ".rs", ".ts", ".tsx", ".mts", ".cts"];

/**
 * Strip a line's leading comment marker (`///`, `//`, `/**`, ` * `) before
 * joining a context window into one string for `HISTORICAL_MARKER_RE` — the
 * marker regex works on prose, and without this a two-word marker phrase
 * split across a line wrap (e.g. "now" at one line's end, "says" at the
 * next's start, each under its own `// ` prefix) would never match. This
 * mirrors the false-negative found while validating this guard: `tests/
 * knot/cst.rs`'s legitimate "the comment now / says `INLINE_WS*`" wrap
 * caused a raw newline-join to miss the marker until the prefix was stripped
 * first.
 *
 * @param {string} line
 * @returns {string}
 */
export function stripCommentPrefix(line) {
  return line.replace(/^\s*(\/\/\/|\/\/|\/\*\*|\*\/|\*)\s?/, "");
}

/**
 * Shape-based nested-checkout detection (a worktree or vendored copy with no
 * `.git` file of its own) — same purpose as check-scripts.mjs's function of
 * the same name, reimplemented here to keep this file dependency-free of
 * check-scripts.mjs's own internals (that file's helper isn't exported).
 *
 * @param {string} dir
 * @returns {boolean}
 */
function isNestedCheckoutByShape(dir) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return false;
  }
  return entries.includes("Cargo.toml") && entries.includes("package.json") && entries.includes(".claude");
}

/**
 * Every file under `repoRoot` worth scanning for `STALE_TOKEN`, discovered
 * rather than enumerated (the same discipline check-scripts.mjs's own header
 * draws the lesson from — a hardcoded list of "the files we check" is the
 * identical hand-enumeration failure this guard exists to end, moved one
 * level up).
 *
 * @param {string} [repoRoot]
 * @returns {string[]} repo-relative paths, sorted (determinism)
 */
export function discoverScanFiles(repoRoot = REPO_ROOT) {
  const found = [];

  const walk = (dir, prefix) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const child = join(dir, entry.name);
      const relative = prefix === "" ? entry.name : `${prefix}/${entry.name}`;

      if (entry.isDirectory()) {
        if (PRUNED_DIRS.has(entry.name)) continue;
        if (existsSync(join(child, ".git"))) continue;
        if (isNestedCheckoutByShape(child)) continue;
        walk(child, relative);
        continue;
      }

      if (SCANNED_EXTENSIONS.some((ext) => entry.name.endsWith(ext))) {
        found.push(relative);
      }
    }
  };

  walk(repoRoot, "");
  return found.sort();
}

/**
 * Find every `STALE_TOKEN` occurrence in `text` lacking a nearby historical
 * marker.
 *
 * @param {string} text
 * @param {{windowLines?: number}} [options]
 * @returns {{line: number, ok: boolean}[]} one entry per occurrence
 */
export function findGrammarDriftOccurrences(text, { windowLines = CONTEXT_WINDOW_LINES } = {}) {
  const lines = text.split("\n");
  const results = [];

  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].includes(STALE_TOKEN)) continue;

    const start = Math.max(0, i - windowLines);
    const end = Math.min(lines.length, i + windowLines + 1);
    const window = lines
      .slice(start, end)
      .map(stripCommentPrefix)
      .join(" ");

    results.push({ line: i + 1, ok: HISTORICAL_MARKER_RE.test(window) });
  }

  return results;
}

/**
 * Check one file's text for stale grammar-comment quotations.
 *
 * @param {string} path repo-relative path, for the problem message only
 * @param {string} text
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkFileForGrammarDrift(path, text) {
  const problems = findGrammarDriftOccurrences(text)
    .filter((occurrence) => !occurrence.ok)
    .map(
      (occurrence) =>
        `${path}:${occurrence.line} quotes \`${STALE_TOKEN}\` (required whitespace) with no nearby ` +
        `acknowledgment that the source has since been corrected to \`INLINE_WS*\` (optional) — ` +
        `either this is a NEW drift (a grammar comment reproduction disagreeing with brink-syntax's ` +
        `actual, current \`Parser::skip_ws\`-based zero-or-more behavior) or a historical mention ` +
        `that needs one of the marker phrases nearby (e.g. "the comment now says \`INLINE_WS*\`", ` +
        `"predated #<issue>", "used to say"). See scripts/check-grammar-drift.mjs's header.`,
    );

  return { ok: problems.length === 0, problems };
}

/**
 * Run the guard over the real repo.
 *
 * @param {{repoRoot?: string}} [options]
 * @returns {{ok: boolean, problems: string[], filesScanned: number}}
 */
export function checkGrammarDrift({ repoRoot = REPO_ROOT } = {}) {
  const files = discoverScanFiles(repoRoot);
  const problems = [];

  for (const path of files) {
    const text = readFileSync(join(repoRoot, path), "utf8");
    problems.push(...checkFileForGrammarDrift(path, text).problems);
  }

  return { ok: problems.length === 0, problems, filesScanned: files.length };
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkGrammarDrift();
  if (result.ok) {
    console.log(
      `ok - no stale \`${STALE_TOKEN}\` grammar-comment quotations found (${result.filesScanned} files scanned).`,
    );
  } else {
    console.error("Grammar-comment drift check FAILED (#2718):");
    for (const problem of result.problems) console.error(`  - ${problem}`);
    process.exitCode = 1;
  }
}
