// Detection half of issue #2054: warn (loudly, non-zero exit) when a
// corpus/bucket measurement is about to be taken against a `CARGO_TARGET_DIR`
// that is shared with another currently-live git worktree.
//
// ─────────────────────────────────────────────────────────────────────────
// WHY THIS EXISTS
//
// The autonomous-pump gives every agent worktree the SAME shared
// `CARGO_TARGET_DIR` (by convention `/tmp/pump-cargo-target-brink` —
// `.claude/skills/autonomous-pump/BRINK-CONFIG.md`), deliberately: a
// per-worktree Rust `target/` is tens of gigabytes and disk is the hard
// constraint here. But cargo's `-C metadata` for a workspace member is
// derived from package name/version/features — NOT the absolute source
// path — so two worktrees of the same package at different paths write the
// SAME fingerprint directory and the SAME output binary path. Whichever
// worktree built most recently silently overwrites the other's artifact.
//
// This was caught directly during PR #2030 (issue #2054): a sweep bucket
// ("thread-start splice") reported stuck at 21 across several runs — the
// exact signal the brief had named as "your merge resolution is wrong". The
// merge was fine; a targeted `cargo clean -p …` produced the true numbers
// (21 → 17, respell OK 261 → 263). A stale artifact produced a confident,
// wrong measurement that nearly caused a correct change to be reverted.
// #2054's first comment records a second symptom shape from #2092/#2143:
// the same collision can serve a DIFFERENT worktree's currently-different
// source for a shared dependency, producing a real (not stale-looking)
// assertion failure that disappears after a rebase — not just a frozen
// number.
//
// ─────────────────────────────────────────────────────────────────────────
// WHAT THIS SCRIPT DOES — AND DOESN'T — PROVE
//
// This is DETECTION, not a fix. The real fix (`CARGO_TARGET_DIR` isolated
// per worktree) is blocked by the disk constraint above — see #2054's own
// "Options" section, which names the shared dir as the disk mitigation AND
// the correctness hazard at the same time. Per-package fingerprint
// dep-info (`.d`) files under a shared target dir use paths RELATIVE to
// the invoking worktree, so this script cannot prove after the fact which
// worktree produced a given cached artifact. What it CAN check cheaply is
// the actual HAZARD CONDITION: is at least one OTHER git worktree
// currently live (`git worktree list`, which reports only real,
// still-registered worktrees — stale leftover directories that were never
// `git worktree add`-ed, or were pruned, do not appear), AND does a build
// artifact already exist for at least one tracked package (nothing built
// yet means nothing can have been served stale)? Whether the configured
// `CARGO_TARGET_DIR` happens to sit inside THIS worktree's own path is
// NOT part of that test — path containment only tells us about this
// worktree, never about whether some OTHER live worktree points its own
// `CARGO_TARGET_DIR` at the very same absolute directory (the
// BRINK-CONFIG.md-mandated shared cache, `<repo>/target`, is exactly this:
// "worktree-local" from the main checkout's own point of view, while every
// other agent worktree also targets it). When a sibling is live and a
// cached artifact exists, ANY measurement taken right now is suspect: some
// other agent may rebuild a shared package out from under this one at any
// moment. That is the collision precondition this script exists to
// surface before a suspect number gets trusted — a synthesis of #2054's
// ask, not a quote from it (#2054's own option (a) is "Targeted clean
// before measuring").
//
// A "safe" result here does not prove the CURRENT artifacts are fresh viz a
// vis a worktree that has since been removed — only that the collision
// precondition does not currently hold. Re-run this immediately before
// trusting a number, not once at the start of a long session.
//
// Recommended use: run this (or `pnpm check:target-freshness`) immediately
// before trusting any `corpus_report` / `full_corpus_sweep` output. On a
// RISK result, run `cargo clean -p <pkg>` for the reported packages (or
// fall back to a private/worktree-local target dir for just that
// measurement — never a third shared path, per BRINK-CONFIG.md's "Disk
// rule" section) before re-measuring.

import { existsSync, readdirSync, statSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
const defaultRepoRoot = resolve(here, "..");

// Packages most directly implicated by #2054's own evidence (the
// full_corpus_sweep / corpus_report dependency chain: brink-respell's
// sweep, and the compiler pipeline corpus_report drives). Callers can
// override with `--package <name>` (repeatable) for a narrower or wider
// check.
export const DEFAULT_PACKAGES = [
  "brink-respell",
  "brink-ir",
  "brink-syntax-native",
  "brink-runtime",
  "brink-test-harness",
];

/**
 * Parse `git worktree list --porcelain` into `{ path, locked }` entries.
 * Only worktrees git itself still considers live are ever listed — a stale
 * `.claude/worktrees/wf_*` directory that was never registered (or was
 * pruned) does not appear, so this does not over-warn on leftover dirs.
 */
export function listLiveWorktrees({
  repoRoot = defaultRepoRoot,
  exec = execFileSync,
} = {}) {
  const output = exec("git", ["worktree", "list", "--porcelain"], {
    cwd: repoRoot,
    encoding: "utf8",
  });

  const worktrees = [];
  let current = null;
  for (const line of output.split("\n")) {
    if (line.startsWith("worktree ")) {
      if (current) worktrees.push(current);
      current = { path: line.slice("worktree ".length).trim(), locked: false };
    } else if (line.startsWith("locked") && current) {
      current.locked = true;
    }
  }
  if (current) worktrees.push(current);
  return worktrees;
}

/**
 * Find the most-recently-modified cargo dep-info file for `pkg`'s lib
 * target under `<targetDir>/<profile>/deps/` (cargo/rustc normalize the
 * crate name's `-` to `_` in that filename; the hash suffix is the
 * collision-prone `-C metadata` value #2054 is about, so it is
 * intentionally NOT matched exactly — any hash for this package name
 * counts as evidence). Returns `null` if nothing has been built yet.
 */
export function findNewestArtifact(
  targetDir,
  pkg,
  { profiles = ["debug", "release"], stat = statSync } = {},
) {
  const normalized = pkg.replace(/-/g, "_");
  const pattern = new RegExp(`^${normalized}-[0-9a-f]+\\.d$`);

  let newest = null;
  for (const profile of profiles) {
    const depsDir = join(targetDir, profile, "deps");
    if (!existsSync(depsDir)) continue;
    for (const entry of readdirSync(depsDir)) {
      if (!pattern.test(entry)) continue;
      const full = join(depsDir, entry);
      let mtimeMs;
      try {
        mtimeMs = stat(full).mtimeMs;
      } catch (err) {
        // The entry can vanish between readdirSync and statSync — a sibling
        // worktree rebuilding, or a `cargo clean -p` (this tool's own
        // remediation) racing us. That is exactly the kind of concurrent
        // mutation this tool exists to be safe under, so skip the vanished
        // entry rather than throwing.
        if (err && err.code === "ENOENT") continue;
        throw err;
      }
      if (!newest || mtimeMs > newest.mtimeMs) {
        newest = { path: full, mtimeMs };
      }
    }
  }
  return newest;
}

function formatEvidenceLine({ package: pkg, artifact }) {
  if (!artifact) return `    - ${pkg}: no build artifact found yet`;
  const age = new Date(artifact.mtimeMs).toISOString();
  return `    - ${pkg}: last (re)built ${age} — ${artifact.path}`;
}

/**
 * Core check. Every input defaults to the real one so a bare
 * `checkTargetFreshness()` does the whole job, and tests can inject a
 * scratch repoRoot/targetDir/listWorktrees without touching git or the
 * real cache.
 *
 * Returns `{ safe, shared, siblingWorktrees, evidence }` and never throws
 * on the expected "risk detected" path — only on a genuine inability to
 * run `git worktree list` (e.g. not a git repo at all).
 */
export function checkTargetFreshness({
  repoRoot = defaultRepoRoot,
  targetDir = process.env.CARGO_TARGET_DIR ?? join(repoRoot, "target"),
  packages = DEFAULT_PACKAGES,
  listWorktrees = () => listLiveWorktrees({ repoRoot }),
  log = console.log,
  warn = console.warn,
} = {}) {
  const targetDirAbs = resolve(targetDir);
  const repoRootAbs = resolve(repoRoot);

  if (!existsSync(targetDirAbs)) {
    log(`[check-target-freshness] ${targetDirAbs} does not exist yet — nothing built, no staleness risk.`);
    return { safe: true, shared: false, siblingWorktrees: [], evidence: [] };
  }

  // Path containment tells us whether OUR target dir sits inside OUR
  // worktree — it does NOT tell us whether some OTHER live worktree points
  // its own CARGO_TARGET_DIR at this same absolute path (e.g. the
  // BRINK-CONFIG.md-mandated shared cache at the main checkout's own
  // `<repo>/target`, which every other agent worktree also targets, is
  // "worktree-local" only from the main checkout's point of view). So
  // locality is informational only, folded into the messages below — it
  // must never gate the verdict by itself; only actual sibling-worktree
  // presence, and actual cached-artifact evidence, does.
  const isWorktreeLocal =
    targetDirAbs === repoRootAbs || targetDirAbs.startsWith(repoRootAbs + sep);
  const localityNote = isWorktreeLocal
    ? " (this path is worktree-local, but that does not rule out another live worktree pointing at the same absolute directory)"
    : "";

  const siblingWorktrees = listWorktrees().filter((w) => resolve(w.path) !== repoRootAbs);

  if (siblingWorktrees.length === 0) {
    log(
      `[check-target-freshness] ${targetDirAbs} has no other live worktree right now${localityNote} — ` +
        "safe for the moment. Re-run this immediately before trusting a measurement if a " +
        "sibling worktree could have appeared since.",
    );
    return { safe: true, shared: false, siblingWorktrees: [], evidence: [] };
  }

  const evidence = packages.map((pkg) => ({
    package: pkg,
    artifact: findNewestArtifact(targetDirAbs, pkg),
  }));

  if (evidence.every((e) => e.artifact === null)) {
    log(
      `[check-target-freshness] ${targetDirAbs}: no build artifact found yet for any tracked ` +
        `package${localityNote} — nothing built, no staleness risk, even though ` +
        `${siblingWorktrees.length} other worktree(s) are live.`,
    );
    return { safe: true, shared: true, siblingWorktrees, evidence };
  }

  warn(
    [
      "[check-target-freshness] RISK: CARGO_TARGET_DIR may be shared with another live worktree.",
      "",
      `    target dir:  ${targetDirAbs}${localityNote}`,
      `    this worktree: ${repoRootAbs}`,
      "    other live worktree(s):",
      ...siblingWorktrees.map((w) => `      - ${w.path}${w.locked ? " (locked)" : ""}`),
      "",
      "Cargo's `-C metadata` for a workspace member is derived from package",
      "name/version/features, NOT the absolute source path — so this worktree",
      "and any of the ones above can write the SAME fingerprint and output",
      "path for the same package. A number measured right now (corpus_report,",
      "full_corpus_sweep, or any other sweep) may be reading a binary another",
      "worktree just rebuilt, or may itself be silently overwritten by one of",
      "them mid-measurement (issue #2054).",
      "",
      "Current cached-artifact evidence (informational — does not prove which",
      "worktree produced these; only that a build exists to be stale):",
      ...evidence.map(formatEvidenceLine),
      "",
      "Before trusting a measured number: `cargo clean -p " +
        `${packages.join(" -p ")}` +
        "` (or the specific package(s) you changed), then re-measure. Never point",
      "at a third ad-hoc target dir — see .claude/skills/autonomous-pump/BRINK-CONFIG.md's",
      '"Disk rule" section ("never a third path").',
      "",
    ].join("\n"),
  );
  return { safe: false, shared: true, siblingWorktrees, evidence };
}

// Main-guard: `node scripts/check-target-freshness.mjs` (or
// `pnpm check:target-freshness`) does the whole job and exits non-zero on
// a detected risk; importing this module does nothing but hand over the
// functions, matching this repo's other check-*.mjs scripts.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  const packages = [];
  let targetDirArg;
  let repoRootArg;
  for (let i = 0; i < args.length; i += 1) {
    if (args[i] === "--package" && args[i + 1]) {
      packages.push(args[i + 1]);
      i += 1;
    } else if (args[i] === "--target-dir" && args[i + 1]) {
      targetDirArg = args[i + 1];
      i += 1;
    } else if (args[i] === "--repo-root" && args[i + 1]) {
      repoRootArg = args[i + 1];
      i += 1;
    }
  }

  const result = checkTargetFreshness({
    ...(repoRootArg ? { repoRoot: resolve(repoRootArg) } : {}),
    ...(targetDirArg ? { targetDir: targetDirArg } : {}),
    ...(packages.length > 0 ? { packages } : {}),
  });

  if (!result.safe) {
    process.exitCode = 1;
  }
}
