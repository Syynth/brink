// Detection half of issue #2054: warn (loudly, non-zero exit) when a
// corpus/bucket measurement is about to be taken against a `CARGO_TARGET_DIR`
// that another currently-live git worktree actually collided with.
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
// #2759 — FROM "STRUCTURALLY POSSIBLE" TO A REAL VERDICT
//
// PR #2753's first version of this script stopped at the HAZARD
// PRECONDITION: is at least one other git worktree live, AND does a build
// artifact already exist for at least one tracked package? In the pump's
// normal operating mode that precondition holds almost constantly — many
// worktrees sharing one target dir is the whole point of BRINK-CONFIG.md's
// disk rule — so it made this script a near-constant RED, not a signal an
// agent could act on. Per-package fingerprint dep-info (`.d`) files under a
// shared target dir use paths RELATIVE to the invoking worktree, so they
// alone can never prove which worktree produced a given cached artifact —
// that gap is exactly what a build-time stamp closes.
//
// Every tracked package now carries a `build.rs` (see each crate's own,
// e.g. `crates/internal/brink-respell/build.rs`) that writes its own
// `CARGO_MANIFEST_DIR` — an absolute path, worktree-specific by
// construction, unlike dep-info's relative one — into `OUT_DIR/
// worktree-stamp.txt` whenever cargo actually re-runs the build script for
// this package. No `cargo:rerun-if-changed` is emitted, but that is NOT
// because cargo reruns build scripts on every invocation by default — it
// does not; absent a `rerun-if-*` directive, cargo's documented default is
// to rerun a build script only when a file in the package has changed
// since the run that last executed it. The guarantee this design actually
// relies on, and gets, is narrower: the stamp always names the last
// worktree cargo actually re-ran this script for, and a no-op repeat build
// in that SAME worktree (nothing changed, script does not rerun) simply
// leaves the stamp naming that same worktree — which is still the correct
// answer. `findNewestStamp` below locates the newest such file the same
// way `findNewestArtifact` locates the newest dep-info file — scanning
// `<target>/<profile>/build/` for this package's hash-suffixed
// directories, since the metadata hash (like the dep-info one) carries no
// worktree identity of its own — that identity comes only from the
// stamp's own contents. A single package can have MULTIPLE such
// hash-suffixed directories live at once (e.g. a `cargo test` invocation
// and a `cargo clippy --all-targets` invocation of the same package can
// legitimately produce two build-script units with different metadata
// hashes) — `findAllStamps`/`classifyPackageStamps` below classify every
// one of them rather than letting whichever has the newest mtime stand in
// for the whole package; see `classifyPackageStamps`'s doc comment for the
// aggregation rule.
//
// `classifyStamp` then compares the stamp's manifest dir against `git
// worktree list` to produce one of four verdicts:
//
//   - "self"          — this worktree built it last. No risk from this
//                        package.
//   - "live-sibling"  — a DIFFERENT worktree that `git worktree list`
//                        still reports as live built it last. This is the
//                        real, provable collision signal — the only
//                        verdict that turns the overall result RED.
//   - "dead-worktree" — the stamp names a worktree that no longer appears
//                        in `git worktree list` (removed, pruned, or
//                        merged away). That worktree cannot race this one
//                        for the artifact any further, so this is treated
//                        as safe — the artifact might be stale relative to
//                        what a long-gone worktree last saw, but nothing
//                        is currently contending for it.
//   - "missing" / "unreadable" — no stamp file was found for this package,
//                        or one was found but could not be read/parsed
//                        (empty file, permissions, a stamp format from a
//                        version of this tooling before #2759 shipped).
//                        Both are treated as UNVERIFIED, not RISK: an
//                        artifact built before this change (or by
//                        anything that bypassed the build.rs, e.g. a
//                        hand-rolled `rustc` invocation) will legitimately
//                        have no stamp, and reporting RED for that would
//                        reintroduce the exact always-red failure mode
//                        this issue exists to remove, just in a new shape.
//                        `pnpm check:target-freshness` still logs these so
//                        they are visible, but they never flip the verdict.
//
// A "safe" result here still does not prove artifacts are fresh with
// respect to a worktree that both built them AND has since been removed —
// only that no CURRENTLY LIVE worktree is a proven source of this
// artifact. Re-run this immediately before trusting a number, not once at
// the start of a long session.
//
// Recommended use: run this (or `pnpm check:target-freshness`) immediately
// before trusting any `corpus_report` / `full_corpus_sweep` output. On a
// RISK result, run `cargo clean -p <pkg>` for the reported packages (or
// fall back to a private/worktree-local target dir for just that
// measurement — never a third shared path, per BRINK-CONFIG.md's "Disk
// rule" section) before re-measuring.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
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

/**
 * Find the single newest per-worktree build stamp for `pkg`'s build-script
 * output under `<targetDir>/<profile>/build/` (#2759) — a thin wrapper over
 * `findAllStamps` (below) that reduces its list to the entry with the
 * greatest `mtimeMs`. Cargo lays a build script's `OUT_DIR` out as
 * `<target>/<profile>/build/<pkg>-<hash>/out/`, using the same
 * collision-prone `-C metadata` hash as `findNewestArtifact` — so, as
 * there, the hash is intentionally NOT matched exactly, only the package
 * name prefix. Unlike the normalized (`-` → `_`) crate-object names
 * `findNewestArtifact` matches under `deps/`, this `build/<pkg>-<hash>`
 * directory name is cargo's own package name verbatim, hyphens included —
 * confirmed against a real build during this change; do not "fix" this to
 * match the underscored form. `data` is the stamp file's raw text (this crate's
 * `CARGO_MANIFEST_DIR` at the moment it was built, written verbatim by
 * `build.rs` with no serialization format to get wrong), trimmed, or `null`
 * when a stamp directory/file exists but its content could not be read —
 * distinct from returning `null` outright, which means no stamp exists at
 * all for this package. Both cases are handled explicitly by
 * `classifyStamp`; neither one throws.
 *
 * NOTE: a package can have more than one such unit alive at once (see
 * `findAllStamps`), and this single "newest" one is NOT necessarily the
 * unit backing whatever artifact `findNewestArtifact` finds — the two are
 * found under different hash namespaces and cannot be correlated by hash.
 * Use this only for display of "the most recent stamp activity for this
 * package"; use `classifyPackageStamps` (which considers every unit) to
 * decide whether a package is actually safe.
 */
export function findNewestStamp(
  targetDir,
  pkg,
  opts = {},
) {
  const stamps = findAllStamps(targetDir, pkg, opts);
  if (stamps.length === 0) return null;
  return stamps.reduce((a, b) => (b.mtimeMs > a.mtimeMs ? b : a));
}

/**
 * Find EVERY per-worktree build stamp for `pkg`'s build-script output under
 * `<targetDir>/<profile>/build/` (#2759 finding: a package can legitimately
 * have more than one such hash-suffixed unit alive at once — e.g. a
 * `cargo test` invocation and a `cargo clippy --all-targets` invocation of
 * the same package produce build-script units under different `-C
 * metadata` hashes, which can belong to different worktrees). Returns one
 * entry per matching directory — `{ path, mtimeMs, data }`, `data` being
 * the trimmed stamp content or `null` when present-but-unreadable (see
 * `findNewestStamp`'s doc comment for the directory-naming details this
 * shares with it). `findNewestStamp` is now a thin wrapper that reduces
 * this list to the single newest entry.
 */
export function findAllStamps(
  targetDir,
  pkg,
  { profiles = ["debug", "release"], stat = statSync, readFile = readFileSync } = {},
) {
  // NOT normalized to underscores — see `findNewestStamp`'s doc comment.
  const pattern = new RegExp(`^${pkg}-[0-9a-f]+$`);

  const stamps = [];
  for (const profile of profiles) {
    const buildDir = join(targetDir, profile, "build");
    if (!existsSync(buildDir)) continue;
    for (const entry of readdirSync(buildDir)) {
      if (!pattern.test(entry)) continue;
      const stampPath = join(buildDir, entry, "out", "worktree-stamp.txt");
      let mtimeMs;
      try {
        mtimeMs = stat(stampPath).mtimeMs;
      } catch (err) {
        // Same race `findNewestArtifact` guards against, plus the ordinary
        // case of a build directory that exists for this package but never
        // ran (or predates) the #2759 build.rs — either way, no stamp here.
        if (err && err.code === "ENOENT") continue;
        throw err;
      }

      let data = null;
      try {
        const raw = readFile(stampPath, "utf8").trim();
        if (raw.length > 0) data = raw;
      } catch {
        data = null; // exists but unreadable (permissions, race, truncated write)
      }

      stamps.push({ path: stampPath, mtimeMs, data });
    }
  }
  return stamps;
}

/**
 * Turn a `findNewestStamp` result into one of four verdicts by comparing
 * the stamp's manifest-dir path against the worktrees `git worktree list`
 * currently reports as live. `worktrees` entries must already have
 * absolute, `resolve()`d `path`s.
 *
 * - `{ kind: "missing" }` — no stamp file exists for this package at all
 *   (predates the #2759 build.rs, or was produced by something that
 *   bypassed it).
 * - `{ kind: "unreadable" }` — a stamp file exists but was empty or could
 *   not be read.
 * - `{ kind: "self" }` — the stamp names this worktree.
 * - `{ kind: "dead-worktree", manifestDir }` — the stamp names a directory
 *   that isn't inside ANY currently-live worktree (the worktree that built
 *   it has since been removed or pruned).
 * - `{ kind: "live-sibling", manifestDir, worktree }` — the stamp names a
 *   directory inside a currently-live worktree that is NOT this one. This
 *   is the only verdict that should ever flip the overall result to RISK.
 *
 * "missing" and "unreadable" are kept distinct (rather than collapsed into
 * one "unknown") purely for diagnostic wording — both are UNVERIFIED, and
 * neither one is ever treated as a collision.
 */
export function classifyStamp(stamp, { repoRootAbs, worktrees }) {
  if (!stamp) return { kind: "missing" };
  if (stamp.data === null) return { kind: "unreadable" };

  const manifestDirAbs = resolve(stamp.data);

  // Longest-prefix match: if worktree paths were ever nested (not expected
  // in practice, but cheap to get right), the most specific one wins.
  let owner = null;
  for (const w of worktrees) {
    const wAbs = w.path; // already resolve()d by the caller
    const contains = manifestDirAbs === wAbs || manifestDirAbs.startsWith(wAbs + sep);
    if (!contains) continue;
    if (!owner || wAbs.length > owner.path.length) owner = w;
  }

  if (!owner) return { kind: "dead-worktree", manifestDir: manifestDirAbs };
  if (owner.path === repoRootAbs) return { kind: "self", manifestDir: manifestDirAbs };
  return { kind: "live-sibling", manifestDir: manifestDirAbs, worktree: owner };
}

/**
 * Classify EVERY build-script stamp unit `findAllStamps` finds for `pkg`,
 * then reduce them to one package-level verdict under an EXPLICIT
 * aggregation rule — not mtime order.
 *
 * Why this exists (finding against the original `findNewestStamp`-only
 * design): a package can have more than one build-script unit alive at
 * once under different `-C metadata` hashes — e.g. worktree A runs
 * `cargo test -p pkg` (stamps unit 1, owned by A) and, later, sibling
 * worktree B runs `cargo clippy -p pkg --all-targets` (stamps unit 2,
 * owned by B, now the newer one by mtime). Collapsing to "whichever stamp
 * is newest" before classifying made A's own check report a false-positive
 * RISK for `pkg`, even though the unit A's own measurement actually
 * depends on is still A's.
 *
 * Aggregation rule (explicit):
 *   1. If ANY unit is classified "self", the package is safe — this
 *      worktree has direct proof, from cargo's own build-script run, that
 *      it built this package itself. A different, merely-newer unit
 *      belonging to someone else (a different feature/profile/target
 *      invocation) does not make that proof less true.
 *   2. Else, if ANY unit is classified "live-sibling", the package is a
 *      confirmed collision — this is the real, provable hazard signal.
 *   3. Else, if ANY unit is "dead-worktree", report that (safe — nothing
 *      currently live is contending for this package).
 *   4. Else "unreadable" if every unit was found but unreadable.
 *   5. Else "missing" (no units at all).
 *
 * Returns `{ kind, worktree?, manifestDir?, units }` — `units` is every
 * classified stamp (`{ path, mtimeMs, data, verdict }`), always present,
 * so callers can surface per-unit ownership rather than only the
 * aggregate.
 */
export function classifyPackageStamps(targetDir, pkg, { repoRootAbs, worktrees, ...opts }) {
  const units = findAllStamps(targetDir, pkg, opts).map((stamp) => ({
    ...stamp,
    verdict: classifyStamp(stamp, { repoRootAbs, worktrees }),
  }));

  if (units.length === 0) return { kind: "missing", units };

  const self = units.find((u) => u.verdict.kind === "self");
  if (self) return { kind: "self", manifestDir: self.verdict.manifestDir, units };

  const liveSibling = units.find((u) => u.verdict.kind === "live-sibling");
  if (liveSibling) {
    return {
      kind: "live-sibling",
      manifestDir: liveSibling.verdict.manifestDir,
      worktree: liveSibling.verdict.worktree,
      units,
    };
  }

  const dead = units.find((u) => u.verdict.kind === "dead-worktree");
  if (dead) return { kind: "dead-worktree", manifestDir: dead.verdict.manifestDir, units };

  if (units.every((u) => u.verdict.kind === "unreadable")) return { kind: "unreadable", units };

  return { kind: "missing", units };
}

function formatEvidenceLine({ package: pkg, artifact, verdict }) {
  const units = verdict?.units ?? [];
  const unitLines =
    units.length > 1
      ? units.map((u) => {
          const age = new Date(u.mtimeMs).toISOString();
          switch (u.verdict?.kind) {
            case "self":
              return `        · unit ${age} — ${u.path} (built by this worktree)`;
            case "live-sibling":
              return `        · unit ${age} — ${u.path} (built by LIVE sibling ${u.verdict.worktree.path})`;
            case "dead-worktree":
              return `        · unit ${age} — ${u.path} (built by a worktree that no longer exists — ${u.verdict.manifestDir})`;
            case "unreadable":
              return `        · unit ${age} — ${u.path} (stamp unreadable)`;
            default:
              return `        · unit ${age} — ${u.path}`;
          }
        })
      : [];

  if (!artifact) {
    const base = `    - ${pkg}: no build artifact found yet`;
    return unitLines.length > 0
      ? [`${base}, but ${units.length} build-script stamp unit(s) exist:`, ...unitLines].join("\n")
      : base;
  }

  const age = new Date(artifact.mtimeMs).toISOString();
  const base = `    - ${pkg}: last (re)built ${age} — ${artifact.path}`;
  const summary = (() => {
    switch (verdict?.kind) {
      case "self":
        return `${base} (stamp: built by this worktree)`;
      case "live-sibling":
        return `${base} (stamp: built by LIVE sibling worktree ${verdict.worktree.path})`;
      case "dead-worktree":
        return `${base} (stamp: built by a worktree that no longer exists — ${verdict.manifestDir} — not currently a risk)`;
      case "unreadable":
        return `${base} (stamp exists but could not be read/parsed — unverified, rebuild for a real signal)`;
      case "missing":
      default:
        return `${base} (no worktree stamp found — predates #2759, or was built without it — unverified, rebuild for a real signal)`;
    }
  })();

  return unitLines.length > 0 ? [summary, ...unitLines].join("\n") : summary;
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

  const liveWorktrees = listWorktrees().map((w) => ({ ...w, path: resolve(w.path) }));
  const siblingWorktrees = liveWorktrees.filter((w) => w.path !== repoRootAbs);

  if (siblingWorktrees.length === 0) {
    log(
      `[check-target-freshness] ${targetDirAbs} has no other live worktree right now${localityNote} — ` +
        "safe for the moment. Re-run this immediately before trusting a measurement if a " +
        "sibling worktree could have appeared since.",
    );
    return { safe: true, shared: false, siblingWorktrees: [], evidence: [] };
  }

  const evidence = packages.map((pkg) => {
    const artifact = findNewestArtifact(targetDirAbs, pkg);
    const stamp = findNewestStamp(targetDirAbs, pkg);
    const verdict = classifyPackageStamps(targetDirAbs, pkg, {
      repoRootAbs,
      worktrees: liveWorktrees,
    });
    return { package: pkg, artifact, stamp, verdict };
  });

  if (evidence.every((e) => e.artifact === null)) {
    log(
      `[check-target-freshness] ${targetDirAbs}: no build artifact found yet for any tracked ` +
        `package${localityNote} — nothing built, no staleness risk, even though ` +
        `${siblingWorktrees.length} other worktree(s) are live.`,
    );
    return { safe: true, shared: true, siblingWorktrees, evidence };
  }

  // `e.artifact &&` keeps this consistent with `unverified` below: a
  // package whose build script ran (so its stamp exists and can even name
  // a live sibling) but whose actual compile never produced a cached
  // artifact must not flip the whole run RED while its own evidence line
  // reports "no build artifact found yet" — that contradiction was a
  // reported finding against the original filter.
  const collisions = evidence.filter((e) => e.artifact && e.verdict.kind === "live-sibling");
  const unverified = evidence.filter(
    (e) => e.artifact && (e.verdict.kind === "missing" || e.verdict.kind === "unreadable"),
  );

  if (collisions.length === 0) {
    // No package's stamp names a currently-live sibling as its last
    // builder — the precondition (sibling live + artifact exists) that
    // used to be the whole verdict is, on its own, no longer enough to go
    // red (#2759). Packages this worktree built itself, or that were last
    // built by a worktree that's since gone, are genuinely safe; packages
    // with no readable stamp are unverified, not risky — see this file's
    // header for why treating "unverified" as "unsafe" would just
    // reintroduce the always-red failure mode in a new shape.
    log(
      [
        `[check-target-freshness] ${targetDirAbs}: ${siblingWorktrees.length} other worktree(s) are ` +
          `live${localityNote}, but no tracked package's build stamp names one of them as its last ` +
          "builder — safe for the moment.",
        ...evidence.map(formatEvidenceLine),
        unverified.length > 0
          ? "\nNote: the package(s) above marked unverified have no readable stamp (built before " +
            "#2759, or by something that bypassed its build.rs) — this check cannot vouch for them " +
            "either way. `cargo build -p <pkg>` once to get a real stamp before it matters."
          : "",
      ]
        .filter(Boolean)
        .join("\n"),
    );
    return { safe: true, shared: true, siblingWorktrees, evidence };
  }

  warn(
    [
      "[check-target-freshness] RISK: at least one tracked package was last built by a DIFFERENT,",
      "still-live worktree.",
      "",
      `    target dir:  ${targetDirAbs}${localityNote}`,
      `    this worktree: ${repoRootAbs}`,
      "",
      "Confirmed collisions (this package's own build stamp names a live sibling, not this worktree):",
      ...collisions.map(
        (e) => `    - ${e.package}: last built by ${e.verdict.worktree.path}` +
          `${e.verdict.worktree.locked ? " (locked)" : ""}`,
      ),
      "",
      "Cargo's `-C metadata` for a workspace member is derived from package",
      "name/version/features, NOT the absolute source path — so this worktree",
      "and the sibling(s) above wrote the SAME fingerprint and output path for",
      "the same package, and the sibling wrote it last. A number measured",
      "right now (corpus_report, full_corpus_sweep, or any other sweep) may be",
      "reading a binary that sibling worktree just rebuilt, or may itself be",
      "silently overwritten by it mid-measurement (issue #2054).",
      "",
      "Full evidence, including packages that are NOT part of the collision:",
      ...evidence.map(formatEvidenceLine),
      "",
      "Before trusting a measured number: `cargo clean -p " +
        `${collisions.map((e) => e.package).join(" -p ")}` +
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
