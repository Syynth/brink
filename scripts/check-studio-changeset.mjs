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
// Exported as pure functions over string paths + changeset text so
// scripts/check-studio-changeset.test.mjs can drive every branch with
// synthetic diffs; the CLI at the bottom applies them to the real `git diff`
// output.

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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

const TEST_SUFFIX = /\.(test|spec)\.[jt]sx?$/;

/** @param {string} path @returns {boolean} */
export function isGuardedPath(path) {
  return GUARDED_PREFIXES.some((prefix) => path.startsWith(prefix));
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
 *   addedChangesets: {path: string, text: string}[],
 * }} input
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkStudioChangeset({ changedFiles, addedChangesets }) {
  const guardedNonTest = changedFiles.filter((path) => isGuardedPath(path) && !isTestOnlyPath(path));

  if (guardedNonTest.length === 0) {
    return { ok: true, problems: [] };
  }

  const hasStudioChangeset = addedChangesets.some((cs) => changesetNamesStudio(cs.text));
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
        `Run \`pnpm changeset\`, pick ${STUDIO_PACKAGE}, and describe the change. ` +
        `(If every touched file above is test-only, see #2820's carve-out — a path under __tests__/, ` +
        `__mocks__/, __fixtures__/, or ending .test.ts(x)/.spec.ts(x) does not itself trigger this guard.)`,
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
 * Gather the real diff against origin/<baseRef>...HEAD plus the text of
 * every changeset ADDED in that diff. Requires the checkout to hold full
 * history (a shallow single-commit HEAD cannot resolve a merge-base no
 * matter how much of origin/<baseRef> is fetched afterward — see this
 * file's header).
 *
 * @param {{repoRoot?: string, baseRef?: string}} [opts]
 * @returns {{ok: true, changedFiles: string[], addedChangesets: {path: string, text: string}[]}
 *   | {ok: false, reason: string}}
 */
export function gatherDiff({ repoRoot = REPO_ROOT, baseRef = resolveBaseRef() } = {}) {
  const diff = spawnSync("git", ["diff", "--name-status", `origin/${baseRef}...HEAD`], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (diff.status !== 0) {
    return {
      ok: false,
      reason:
        `\`git diff --name-status origin/${baseRef}...HEAD\` failed (exit ${diff.status}): ` +
        `${(diff.stderr || "").trim()}. This needs origin/${baseRef} present locally with full history ` +
        `— in CI, the checkout step must use fetch-depth: 0; locally, run \`git fetch origin ${baseRef}\` first.`,
    };
  }

  const entries = parseNameStatus(diff.stdout);
  const changedFiles = entries.map((e) => e.path);
  const addedChangesetPaths = entries
    .filter((e) => e.status === "A" && isChangesetPath(e.path))
    .map((e) => e.path);

  const addedChangesets = addedChangesetPaths.map((path) => {
    const show = spawnSync("git", ["show", `HEAD:${path}`], { cwd: repoRoot, encoding: "utf8" });
    return { path, text: show.status === 0 ? show.stdout : "" };
  });

  return { ok: true, changedFiles, addedChangesets };
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
