// Is a PR that touches a studio-bundled PRIVATE package's src/ carrying a
// changeset naming @brink-lang/studio? (#2820)
//
// Background — this defect has recurred THREE times, always the same shape:
//
//   1. PR #2787 (closed #280) merged with no @brink-lang/studio changeset.
//      A post-merge review caught it, the fast-follow was never committed,
//      and #2791 / PR #2818 had to restore it by hand.
//   2. PR #2798 (#2794) got one only because a reviewer asked.
//   3. PR #2817 (#2797) DECLINED one on the reasoning "no wasm-observable
//      surface" — caught only by luck (a reviewer checking for collisions
//      with a sibling branch) and added by hand in commit 0534e338c.
//
// Why the reasoning keeps failing: the rule people reach for is CLAUDE.md's
// "wasm-observable behavior needs a @brink-lang/web changeset," and they
// correctly conclude a studio-shell change is NOT wasm-observable. But
// that's the wrong package. packages/studio-shell, packages/studio-ui,
// packages/studio-store and packages/ink-operations are all `private: true`
// — yet docs/publishing.md records them as BUNDLED into the published
// @brink-lang/studio, and .changeset/config.json sets
// `privatePackages.version: false`, so @brink-lang/studio is the ONLY
// attribution route a change to one of them has. "Not wasm-observable" is
// true and irrelevant; the message below says so explicitly, because a
// terse "changeset required" invites exactly the wrong-package reasoning
// that has already failed three times.
//
// packages/ink-editor is deliberately NOT in this list: it publishes its own
// @brink-lang/editor package (see .changeset/config.json's fixed group), so
// it is covered by ordinary "does this change the published surface"
// judgment, not this guard.
//
// Design (see #2820's four questions):
//   - Lives under scripts/, run by `pnpm test:scripts` — the natural home
//     for a repo-hygiene guard (check-pnpm-pin.mjs, check-no-nul-bytes.mjs,
//     check-grammar-drift.mjs all live here), not
//     packages/brink-desktop/src-tauri (that guard's YAML-parsing home is
//     somewhat incidental to it, per the issue).
//   - Sees the diff via `git diff --name-status origin/<base>...HEAD`,
//     where <base> is GITHUB_BASE_REF (set automatically by GitHub Actions
//     for pull_request events) or "main" for local/manual use. The CI job
//     wiring this in (.github/workflows/ci.yml, job
//     "studio-changeset-guard") checks out with `fetch-depth: 0`: a
//     shallow checkout's HEAD is grafted (no parent objects at all), so no
//     amount of fetching origin/<base> after the fact can make a merge-base
//     resolve — the checkout step itself has to bring full history.
//   - Carve-out: a changed file whose path has a `__tests__/`, `__mocks__/`,
//     or `__fixtures__/` directory segment, or a `.test.ts(x)`/`.spec.ts(x)`
//     suffix, does not by itself require a changeset (test-support changes
//     are not published behavior). A PR is only forced into a changeset
//     when it touches at least one NON-test-only file under a guarded
//     package. Without this carve-out the guard trains people to add empty
//     changesets to silence it, which the issue names as worse than the
//     current state.
//
// #2834 follow-ups (w186 review of #2827):
//   1. Non-src bundle-shaping files. package.json/tsup/vite configs/
//      alias-map/index.html under packages/brink-studio, plus
//      packages/studio-shell/package.json, also determine what
//      @brink-lang/studio ships but sat outside `**/src`. Extended via a
//      NAMED ALLOWLIST (GUARDED_BUNDLE_FILES) of exact paths, not a
//      blanket `packages/brink-studio/**` — the issue is explicit that the
//      broad glob would start demanding changesets for lockfile-driven
//      version bumps that alter nothing published, and training people to
//      silence a noisy guard with an empty changeset is worse than the gap
//      it closes. For the two package.json files in that allowlist
//      specifically, a further structural carve-out
//      (packageJsonChangeIsIgnorable) parses old vs. new JSON and passes
//      when every top-level key that differs is in that file's entry in
//      PACKAGE_JSON_IGNORABLE_KEYS_BY_PATH — PER FILE, not one blanket set:
//      packages/brink-studio/package.json only carves out "version", since
//      its devDependencies IS the bundle manifest (every bundled `@brink/*`
//      private plus codemirror/zustand lives there); packages/studio-shell/
//      package.json carves out "version" and "devDependencies" too, since
//      it has no devDependencies at all and keeps its real deps in
//      "dependencies". A dependencies/exports/files/scripts change (or any
//      file that fails to parse as JSON, or a newly-added package.json with
//      no "old" side) is NOT ignorable for either file.
//   2. Status-A-only changeset detection. `gatherDiff` used to keep only
//      ADDED (`A`) `.changeset/*.md` files, so a PR that satisfies the
//      rule by EDITING an existing changeset to add the studio key went
//      red despite being correct. Now both `A` and `M` changeset files are
//      collected (as `relevantChangesets`) and checked identically — the
//      decision only cares whether the file's CURRENT text names the
//      package, not which git status produced that text.
//
// Exported as pure functions over string paths + changeset text so
// scripts/check-studio-changeset.test.mjs can drive every branch with
// synthetic diffs; the CLI at the bottom applies them to the real `git diff`
// output.

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");
export const CHANGESET_DIR = ".changeset";
export const CHANGESET_README = ".changeset/README.md";
export const STUDIO_PACKAGE = "@brink-lang/studio";
export const DEFAULT_BASE_REF = "main";

/**
 * Every private package bundled into the published @brink-lang/studio
 * (docs/publishing.md). A change to any file under `<prefix>src/` requires
 * a @brink-lang/studio changeset unless every such file is test-only.
 */
export const GUARDED_PREFIXES = [
  "packages/brink-studio/src/",
  "packages/studio-ui/src/",
  "packages/studio-shell/src/",
  "packages/ink-operations/src/",
  "packages/studio-store/src/",
];

/**
 * Non-src files that shape what @brink-lang/studio actually ships (#2834
 * item 1): build/bundler entry points and the manifests that declare its
 * runtime deps and export map. Exact paths, deliberately NOT a
 * `packages/brink-studio/**` glob — see this file's header. Two of these
 * (the package.json files) get a further structural carve-out in
 * `packageJsonChangeIsIgnorable`; the rest have no carve-out because any
 * edit to a bundler config or the embed HTML shell is bundle-shaping by
 * definition.
 */
export const GUARDED_BUNDLE_FILES = [
  "packages/brink-studio/package.json",
  "packages/brink-studio/tsup.config.ts",
  "packages/brink-studio/vite.config.ts",
  "packages/brink-studio/vite.config.embed.ts",
  "packages/brink-studio/alias-map.ts",
  "packages/brink-studio/index.html",
  "packages/studio-shell/package.json",
];

/** The subset of GUARDED_BUNDLE_FILES eligible for packageJsonChangeIsIgnorable. */
export const PACKAGE_JSON_BUNDLE_FILES = new Set([
  "packages/brink-studio/package.json",
  "packages/studio-shell/package.json",
]);

/**
 * Generic default ignorable-key set for packageJsonChangeIsIgnorable when
 * called without a path-specific override — exercised directly by this
 * file's generic unit tests. NOT what checkStudioChangeset uses for a real
 * guarded file; see PACKAGE_JSON_IGNORABLE_KEYS_BY_PATH below for why a
 * single blanket set is wrong for that.
 */
export const PACKAGE_JSON_IGNORABLE_KEYS = new Set(["version", "devDependencies"]);

/**
 * Per-file ignorable-key sets for the real guarded package.json bundle
 * files (#2834 review finding). "devDependencies is tooling-only, never
 * shipped" is FALSE for packages/brink-studio/package.json specifically:
 * that manifest puts EVERY bundled input — all four bundled privates
 * (@brink/{studio-shell,studio-ui,studio-store,ink-operations}), plus
 * @brink-lang/editor and every CodeMirror/zustand package it inlines via
 * vite/tsup — in devDependencies, with only @brink-lang/web in
 * dependencies. A blanket devDependencies carve-out there would silently
 * exempt a `zustand ^5 -> ^6` bump, a newly ADDED `@brink/*` workspace
 * package, or an outright DELETED one from ever needing a changeset.
 * packages/studio-shell/package.json has no devDependencies at all (its
 * real deps live in `dependencies`, which is never carved out), so
 * devDependencies is safely ignorable there. "version" alone is safe for
 * both — it's normally produced by the changesets release tooling itself,
 * not hand-authored alongside a feature PR.
 */
export const PACKAGE_JSON_IGNORABLE_KEYS_BY_PATH = new Map([
  ["packages/brink-studio/package.json", new Set(["version"])],
  ["packages/studio-shell/package.json", new Set(["version", "devDependencies"])],
]);

const TEST_SUFFIX = /\.(test|spec)\.[jt]sx?$/;

/** @param {string} path @returns {boolean} */
export function isGuardedPath(path) {
  if (GUARDED_PREFIXES.some((prefix) => path.startsWith(prefix))) return true;
  return GUARDED_BUNDLE_FILES.includes(path);
}

/**
 * Does a package.json edit touch only ignorable keys (version,
 * devDependencies)? Parses both sides as JSON and compares every top-level
 * key present on either side; any key outside `ignorableKeys` whose value
 * differs (structurally, via node:util's isDeepStrictEqual — so key
 * reordering inside e.g. dependencies doesn't false-positive) makes the
 * change NOT ignorable. Unparsable JSON on either side, or a non-object
 * top level, is conservatively NOT ignorable (a newly-ADDED package.json
 * has no "old" side to parse, so it never qualifies — a wholly new
 * manifest is bundle-shaping by construction).
 *
 * @param {string} oldText @param {string} newText
 * @param {Set<string>} [ignorableKeys]
 * @returns {boolean}
 */
export function packageJsonChangeIsIgnorable(oldText, newText, ignorableKeys = PACKAGE_JSON_IGNORABLE_KEYS) {
  let oldJson;
  let newJson;
  try {
    oldJson = JSON.parse(oldText);
    newJson = JSON.parse(newText);
  } catch {
    return false;
  }
  if (typeof oldJson !== "object" || oldJson === null || typeof newJson !== "object" || newJson === null) {
    return false;
  }
  const keys = new Set([...Object.keys(oldJson), ...Object.keys(newJson)]);
  for (const key of keys) {
    if (ignorableKeys.has(key)) continue;
    if (!isDeepStrictEqual(oldJson[key], newJson[key])) return false;
  }
  return true;
}

/**
 * Test-only carve-out: a `__tests__/`, `__mocks__/`, or `__fixtures__/`
 * directory segment anywhere in the path, or a `.test.ts(x)`/`.spec.ts(x)`
 * filename suffix. `__mocks__` is not hypothetical: commit f88a3a7b (the
 * grammar-drift guard PR) touched only
 * `packages/brink-studio/src/__mocks__/brink-web.ts` plus a `__tests__`
 * file, with no changeset — under this guard without the carve-out that
 * would go red and push toward an empty changeset, the exact outcome #2820
 * names as worse than the status quo. Other in-tree guards already
 * special-case `__mocks__` (a glob ignore pattern for any `__mocks__`
 * directory, in fold-kinds.test.ts and chromium88-color-mix.test.ts);
 * `__fixtures__` is added for symmetry.
 *
 * @param {string} path @returns {boolean}
 */
export function isTestOnlyPath(path) {
  const segments = path.split("/");
  if (segments.includes("__tests__")) return true;
  if (segments.includes("__mocks__")) return true;
  if (segments.includes("__fixtures__")) return true;
  const filename = segments[segments.length - 1] ?? "";
  return TEST_SUFFIX.test(filename);
}

/**
 * Which parsed name-status entries are changeset files eligible as
 * "relevant" (#2834 item 2: ADDED (`A`) or MODIFIED (`M`), so editing an
 * existing changeset to add the studio key satisfies the guard exactly
 * like adding a new one). Extracted as a pure function over
 * parseNameStatus's output so it's directly unit-testable with synthetic
 * A/M/D/R100-shaped rows (#2834 review finding) without needing a real git
 * history.
 *
 * A renamed changeset (raw git status `R100`, which parseNameStatus
 * collapses to `"R"`) is NOT selected here: it matches neither `"A"` nor
 * `"M"`. This fails closed — a PR that renames-and-edits a changeset to
 * add the studio key would not have its (renamed) text read, so the guard
 * would keep demanding a changeset rather than silently trusting one it
 * never actually inspected under its new name.
 *
 * @param {{status: string, path: string}[]} entries
 * @returns {{status: string, path: string}[]}
 */
export function selectRelevantChangesets(entries) {
  return entries.filter((e) => (e.status === "A" || e.status === "M") && isChangesetPath(e.path));
}

/**
 * Which parsed name-status entries are guarded package.json bundle files
 * eligible for the packageJsonChangeIsIgnorable carve-out (#2834 item 1).
 * Extracted as a pure function for the same reason as
 * selectRelevantChangesets — directly unit-testable with synthetic
 * A/M/D/R100 rows.
 *
 * Deleted files (`D`) are excluded — there's no "new" side to diff, and a
 * deletion of a guarded manifest is bundle-shaping by construction. A
 * renamed guarded package.json (raw `R100` -> parseNameStatus's `"R"`) IS
 * selected here (its `path` is already the new path per parseNameStatus),
 * but gatherDiff's `git show <mergeBase>:<newPath>` for the "old" side will
 * fail to resolve under the pre-rename name and fall back to `oldText: ""`
 * — packageJsonChangeIsIgnorable treats an unparsable/empty old side as
 * NOT ignorable, so a renamed guarded manifest fails closed (demands a
 * changeset) rather than silently passing one it never actually compared.
 *
 * @param {{status: string, path: string}[]} entries
 * @returns {{status: string, path: string}[]}
 */
export function selectPackageJsonEntries(entries) {
  return entries.filter((e) => e.status !== "D" && PACKAGE_JSON_BUNDLE_FILES.has(e.path));
}

/**
 * A changeset file proper: under .changeset/, a `.md` file, not the
 * standing README.
 *
 * @param {string} path @returns {boolean}
 */
export function isChangesetPath(path) {
  return path.startsWith(`${CHANGESET_DIR}/`) && path.endsWith(".md") && path !== CHANGESET_README;
}

/**
 * Does a changeset's frontmatter name @brink-lang/studio? Changesets
 * conventionally list packages as double-quoted frontmatter keys
 * (`"@brink-lang/studio": patch`), but @changesets/cli's YAML frontmatter
 * parser also accepts single-quoted keys and indented keys — both are
 * valid YAML it reads fine (e.g. `'@brink-lang/studio': patch` or
 * `  "@brink-lang/studio": patch`), so a stricter regex here would
 * red-flag a PR that already carries a valid changeset. The leading
 * `["']?` is intentionally optional on both sides rather than paired,
 * since YAML itself allows an unquoted plain-scalar key here too.
 *
 * @param {string} changesetText @returns {boolean}
 */
export function changesetNamesStudio(changesetText) {
  const escaped = STUDIO_PACKAGE.replace(/[/]/g, "\\/");
  return new RegExp(`^\\s*["']?${escaped}["']?\\s*:`, "m").test(changesetText);
}

/**
 * The pure decision, over already-classified inputs — no filesystem or git
 * access, so every branch is directly unit-testable with synthetic data.
 *
 * @param {{
 *   changedFiles: string[],
 *   relevantChangesets: {path: string, text: string}[],
 *   packageJsonDiffs?: {path: string, oldText: string, newText: string}[],
 * }} input
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkStudioChangeset({ changedFiles, relevantChangesets, packageJsonDiffs = [] }) {
  const packageJsonDiffByPath = new Map(packageJsonDiffs.map((d) => [d.path, d]));

  const guardedNonTest = changedFiles.filter((path) => {
    if (!isGuardedPath(path) || isTestOnlyPath(path)) return false;
    if (PACKAGE_JSON_BUNDLE_FILES.has(path)) {
      const diff = packageJsonDiffByPath.get(path);
      const ignorableKeys = PACKAGE_JSON_IGNORABLE_KEYS_BY_PATH.get(path) ?? PACKAGE_JSON_IGNORABLE_KEYS;
      if (diff && packageJsonChangeIsIgnorable(diff.oldText, diff.newText, ignorableKeys)) return false;
    }
    return true;
  });

  if (guardedNonTest.length === 0) {
    return { ok: true, problems: [] };
  }

  const hasStudioChangeset = relevantChangesets.some((cs) => changesetNamesStudio(cs.text));
  if (hasStudioChangeset) {
    return { ok: true, problems: [] };
  }

  const fileList = guardedNonTest.map((f) => `    - ${f}`).join("\n");
  return {
    ok: false,
    problems: [
      `This PR touches a file bundled into the published ${STUDIO_PACKAGE} but adds no changeset ` +
        `naming it:\n${fileList}\n\n` +
        `Four of these five packages (studio-ui, studio-shell, ink-operations, studio-store) are ` +
        `private:true; the fifth, brink-studio, IS ${STUDIO_PACKAGE} itself. docs/publishing.md records ` +
        `the four private ones as BUNDLED into the published ${STUDIO_PACKAGE}, and ` +
        `.changeset/config.json sets privatePackages.version:false — so ${STUDIO_PACKAGE} is the ONLY ` +
        `attribution route a change to any of them has. "This isn't wasm-observable, so no changeset" is ` +
        `the WRONG rule here (that one is about @brink-lang/web) — it has already been reached for, and ` +
        `been wrong, three times (#2787, #2798, #2817). ` +
        `Run \`pnpm changeset\` (a NEW changeset file), or add the ${STUDIO_PACKAGE} key to an EXISTING ` +
        `one you're already touching in this PR — either satisfies this guard. ` +
        `(If every touched file above is test-only, see #2820's carve-out — a path under __tests__/, ` +
        `__mocks__/, __fixtures__/, or ending .test.ts(x)/.spec.ts(x) does not itself trigger this guard. ` +
        `A package.json edit that only touches version (and, for packages/studio-shell/package.json only, ` +
        `devDependencies) is also carved out — #2834.)`,
    ],
  };
}

/**
 * @param {string} nameStatus raw `git diff --name-status` output
 * @returns {{status: string, path: string}[]}
 */
export function parseNameStatus(nameStatus) {
  const entries = [];
  for (const line of nameStatus.split("\n")) {
    if (line.trim().length === 0) continue;
    // Rename/copy lines are "R100\told\tnew" / "C100\told\tnew"; take the
    // new path. Everything else is "A\tpath" / "M\tpath" / "D\tpath".
    const parts = line.split("\t");
    const status = parts[0];
    const path = parts[parts.length - 1];
    entries.push({ status: status[0], path });
  }
  return entries;
}

/**
 * Resolve the base ref to diff against: GITHUB_BASE_REF is set automatically
 * by GitHub Actions for pull_request/pull_request_target events; local/
 * manual runs fall back to "main".
 *
 * @returns {string}
 */
export function resolveBaseRef() {
  return process.env.GITHUB_BASE_REF && process.env.GITHUB_BASE_REF.length > 0
    ? process.env.GITHUB_BASE_REF
    : DEFAULT_BASE_REF;
}

/**
 * Gather the real diff against the merge-base of origin/<baseRef> and HEAD,
 * plus the text of every changeset ADDED or MODIFIED in that diff (#2834
 * item 2), plus old/new text for any guarded package.json bundle file
 * (#2834 item 1's carve-out). Requires the checkout to hold full history (a
 * shallow single-commit HEAD cannot resolve a merge-base no matter how much
 * of origin/<baseRef> is fetched afterward — see this file's header).
 *
 * The old side of a package.json diff is read from the MERGE-BASE commit,
 * not origin/<baseRef>'s tip (#2834 review finding). Those disagree exactly
 * when base has moved since the branch point — e.g. this repo's merge
 * trains, where a PR can sit with `mergeable_state: "behind"` while main
 * keeps moving. Reading the tip there compares this PR's package.json
 * against a "before" state the PR never actually branched from: a PR
 * touching only devDependencies, while main concurrently changed
 * dependencies in the same manifest, would see dependencies differ against
 * the tip and wrongly demand a changeset the carve-out was supposed to
 * grant. The file LIST above already avoids this — three-dot
 * `origin/<baseRef>...HEAD` diffs against the merge-base by definition —
 * this made the "old text" lookup agree with it.
 *
 * @param {{repoRoot?: string, baseRef?: string}} [opts]
 * @returns {{ok: true, changedFiles: string[], relevantChangesets: {path: string, text: string}[],
 *   packageJsonDiffs: {path: string, oldText: string, newText: string}[]}
 *   | {ok: false, reason: string}}
 */
export function gatherDiff({ repoRoot = REPO_ROOT, baseRef = resolveBaseRef() } = {}) {
  const mergeBaseResult = spawnSync("git", ["merge-base", `origin/${baseRef}`, "HEAD"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (mergeBaseResult.status !== 0) {
    return {
      ok: false,
      reason:
        `\`git merge-base origin/${baseRef} HEAD\` failed (exit ${mergeBaseResult.status}): ` +
        `${(mergeBaseResult.stderr || "").trim()}. This needs origin/${baseRef} present locally with full ` +
        `history — in CI, the checkout step must use fetch-depth: 0; locally, run \`git fetch origin ${baseRef}\` first.`,
    };
  }
  const mergeBase = mergeBaseResult.stdout.trim();

  const diff = spawnSync("git", ["diff", "--name-status", `${mergeBase}..HEAD`], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (diff.status !== 0) {
    return {
      ok: false,
      reason:
        `\`git diff --name-status ${mergeBase}..HEAD\` failed (exit ${diff.status}): ` +
        `${(diff.stderr || "").trim()}. This needs origin/${baseRef} present locally with full history ` +
        `— in CI, the checkout step must use fetch-depth: 0; locally, run \`git fetch origin ${baseRef}\` first.`,
    };
  }

  const entries = parseNameStatus(diff.stdout);
  const changedFiles = entries.map((e) => e.path);

  const relevantChangesets = selectRelevantChangesets(entries).map(({ path }) => {
    const show = spawnSync("git", ["show", `HEAD:${path}`], { cwd: repoRoot, encoding: "utf8" });
    return { path, text: show.status === 0 ? show.stdout : "" };
  });

  const packageJsonDiffs = selectPackageJsonEntries(entries).map((e) => {
    const newShow = spawnSync("git", ["show", `HEAD:${e.path}`], { cwd: repoRoot, encoding: "utf8" });
    const oldShow =
      e.status === "A"
        ? { status: 1, stdout: "" }
        : spawnSync("git", ["show", `${mergeBase}:${e.path}`], { cwd: repoRoot, encoding: "utf8" });
    return {
      path: e.path,
      oldText: oldShow.status === 0 ? oldShow.stdout : "",
      newText: newShow.status === 0 ? newShow.stdout : "",
    };
  });

  return { ok: true, changedFiles, relevantChangesets, packageJsonDiffs };
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const gathered = gatherDiff();
  if (!gathered.ok) {
    console.error(`studio changeset guard could not read the diff: ${gathered.reason}`);
    process.exitCode = 1;
  } else {
    const result = checkStudioChangeset(gathered);
    if (result.ok) {
      console.log(`ok - no @brink-lang/studio changeset required (or one was found).`);
    } else {
      console.error(`studio changeset guard FAILED (#2820):`);
      for (const problem of result.problems) console.error(problem);
      process.exitCode = 1;
    }
  }
}
