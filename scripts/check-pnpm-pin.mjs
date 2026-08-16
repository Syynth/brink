// Is the pnpm version pinned to ONE exact version, in ONE place, and is that
// the version actually resolving right now? (#2604)
//
// Background — why an exact pin, not a floating major. This repo used to pin
// pnpm only to a major: `corepack prepare pnpm@10` in scripts/setup-dev.sh and
// `version: 10` passed to `pnpm/action-setup` in five workflow lanes. Which
// 10.x a given machine resolved was therefore ambient, and the OBSERVED
// failure shape for the same trigger (a missing `crates/brink-web/www/pkg`
// `file:` link, #2479) already changed once across that range:
//
//   - #2492 (fixing #2479) recorded a raw `ENOENT … scandir` with exit **0**.
//   - PR #2596's reproduction on pnpm **10.34.5** got a dedicated
//     `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` with exit **1**, in all four
//     permutations tried — the exit-0 half of #2593's report did not
//     reproduce at all on that version.
//
// Same trigger, two different failure shapes, because nothing pinned which
// 10.x a machine got. #2596 fixed the *effect* (scripts/guarded-install.mjs
// never trusts pnpm's exit code); this file is the root-cause side: make the
// version itself non-ambient, and make any drift loud instead of silent.
//
// The pin lives in exactly one place: the root package.json `packageManager`
// field — the corepack-native mechanism, which both the corepack shim and a
// standalone pnpm 10 (whose `manage-package-manager-versions` defaults on)
// honour without any extra wiring. Everything else DERIVES from it:
//
//   - scripts/setup-dev.sh reads the field and `corepack prepare`s that exact
//     version, then verifies what resolved.
//   - `pnpm/action-setup` steps pass NO `version:` input, so the action reads
//     the same `packageManager` field (documented as the supported
//     alternative to the input). Passing both risks the action's
//     "multiple versions specified" path, and re-introduces the two-pins-that-
//     can-disagree problem this issue is about — so the checks below allow a
//     `version:` input ONLY when it exactly equals the pin.
//
// Exported as pure functions over text so scripts/check-pnpm-pin.test.mjs can
// drive them with synthetic inputs (planted drift included); the CLI at the
// bottom applies them to the real files plus the live `pnpm --version`.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");
export const PACKAGE_JSON_PATH = "package.json";
export const SETUP_DEV_PATH = "scripts/setup-dev.sh";
export const WORKFLOWS_DIR = ".github/workflows";

// `.github/workflows/release.yml` is off limits by standing repo rule, so it
// is never read or reported on here.
export const EXCLUDED_WORKFLOWS = new Set(["release.yml"]);

// Exact semver only. A range (`10`, `^10.9.8`, `10.x`, `latest`) is precisely
// the thing this check exists to reject.
const EXACT_SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

const ACTION_SETUP_STEP = /^(\s*)-\s+uses:\s*pnpm\/action-setup@/;
const VERSION_INPUT = /^(\s*)version:\s*(.+?)\s*$/;

/**
 * Read the single source of truth: root package.json's `packageManager`.
 *
 * @param {string} packageJsonText
 * @returns {{ok: true, version: string} | {ok: false, reason: string}}
 */
export function readPin(packageJsonText) {
  let parsed;
  try {
    parsed = JSON.parse(packageJsonText);
  } catch (error) {
    return { ok: false, reason: `root ${PACKAGE_JSON_PATH} is not valid JSON: ${error.message}` };
  }

  const field = parsed.packageManager;
  if (typeof field !== "string" || field.length === 0) {
    return {
      ok: false,
      reason:
        `root ${PACKAGE_JSON_PATH} has no "packageManager" field — pnpm's version is ` +
        `ambient, which is the #2604 root cause. Add "packageManager": "pnpm@<exact>".`,
    };
  }

  // corepack also accepts a `+sha224.<hash>` suffix; tolerate it, pin on the
  // version part.
  const match = /^pnpm@([^+]+)(\+.*)?$/.exec(field);
  if (!match) {
    return {
      ok: false,
      reason: `root ${PACKAGE_JSON_PATH} "packageManager" is "${field}" — expected "pnpm@<exact version>".`,
    };
  }

  const version = match[1];
  if (!EXACT_SEMVER.test(version)) {
    return {
      ok: false,
      reason:
        `root ${PACKAGE_JSON_PATH} pins "pnpm@${version}" — that is a RANGE, not an exact ` +
        `version, so which pnpm a machine resolves is still ambient (#2604).`,
    };
  }

  return { ok: true, version };
}

/**
 * Does scripts/setup-dev.sh derive pnpm's version from the pin rather than
 * carrying a second, independently-drifting one of its own?
 *
 * @param {string} setupDevText
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkSetupDevDerivesPin(setupDevText) {
  const problems = [];

  if (!setupDevText.includes("packageManager")) {
    problems.push(
      `${SETUP_DEV_PATH} never reads the "packageManager" field — it must derive pnpm's ` +
        `version from root ${PACKAGE_JSON_PATH}, not carry its own (#2604).`,
    );
  }

  // A `corepack prepare pnpm@<literal>` is a second pin by definition: the
  // version must come from a shell variable holding the derived value.
  for (const [index, line] of setupDevText.split("\n").entries()) {
    const literal = /corepack\s+prepare\s+"?pnpm@(?!\$)([^"\s]+)/.exec(line);
    if (literal) {
      problems.push(
        `${SETUP_DEV_PATH}:${index + 1} hardcodes "pnpm@${literal[1]}" — a second pin that can ` +
          `drift from root ${PACKAGE_JSON_PATH} (#2604).`,
      );
    }
  }

  return { ok: problems.length === 0, problems };
}

/**
 * Every `version:` input given to a `pnpm/action-setup` step in one workflow.
 *
 * The workflows are read as text rather than parsed as YAML on purpose: this
 * runs from `pnpm test:scripts`, which CI executes BEFORE `pnpm install`, so
 * it cannot depend on a YAML library.
 *
 * @param {string} workflowText
 * @returns {{line: number, version: string}[]}
 */
export function findActionSetupVersionInputs(workflowText) {
  const lines = workflowText.split("\n");
  const found = [];

  for (let i = 0; i < lines.length; i += 1) {
    const step = ACTION_SETUP_STEP.exec(lines[i]);
    if (!step) continue;

    // Keys of this step are indented past the "- " marker; the next line at
    // or below the marker's own indent starts a sibling step or a new block.
    const bodyIndent = step[1].length + "- ".length;

    for (let j = i + 1; j < lines.length; j += 1) {
      const line = lines[j];
      if (line.trim().length === 0) continue;
      if (line.search(/\S/) < bodyIndent) break;

      const input = VERSION_INPUT.exec(line);
      if (input) {
        // Strip a trailing `# comment` and any quoting.
        const version = input[2]
          .replace(/\s+#.*$/, "")
          .trim()
          .replace(/^["']|["']$/g, "");
        found.push({ line: j + 1, version });
      }
    }
  }

  return found;
}

/**
 * @param {{name: string, text: string}[]} workflows
 * @param {string} pinnedVersion
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkWorkflowPins(workflows, pinnedVersion) {
  const problems = [];

  for (const workflow of workflows) {
    if (EXCLUDED_WORKFLOWS.has(workflow.name)) continue;

    for (const input of findActionSetupVersionInputs(workflow.text)) {
      if (input.version === pinnedVersion) continue;
      problems.push(
        `${WORKFLOWS_DIR}/${workflow.name}:${input.line} passes "version: ${input.version}" to ` +
          `pnpm/action-setup, which disagrees with the "pnpm@${pinnedVersion}" pin in root ` +
          `${PACKAGE_JSON_PATH}. Drop the input so the action reads "packageManager" (#2604).`,
      );
    }
  }

  return { ok: problems.length === 0, problems };
}

/**
 * Split a workflow's `jobs:` block into `(id, body)` pairs. Same line-based
 * approach (and the same comment-line caveat) as `jobs()` in
 * packages/brink-desktop/src-tauri/src/lib.rs, for the same reason: no YAML
 * parser is available to a check that runs before `pnpm install`.
 *
 * @param {string} workflowText
 * @returns {{id: string, lines: {text: string, line: number}[]}[]}
 */
export function splitJobs(workflowText) {
  const jobs = [];
  let inJobs = false;

  for (const [index, text] of workflowText.split("\n").entries()) {
    if (text === "jobs:") {
      inJobs = true;
      continue;
    }
    if (!inJobs) continue;
    if (text.trimStart().startsWith("#")) continue;

    const isHeader = text.startsWith("  ") && !text.startsWith("   ") && text.trimEnd().endsWith(":");
    if (isHeader) {
      jobs.push({ id: text.trim().replace(/:$/, ""), lines: [] });
      continue;
    }
    if (jobs.length > 0) jobs[jobs.length - 1].lines.push({ text, line: index + 1 });
  }

  return jobs;
}

/**
 * Dropping the `version:` input means each lane's pnpm version now comes from
 * the CHECKED-OUT root package.json — so `actions/checkout` must precede
 * `pnpm/action-setup` in the same job, or the action has no `packageManager`
 * field to read. That precondition was implicit before this change and is
 * load-bearing after it, so it gets a guard rather than a comment (#2604).
 *
 * @param {{name: string, text: string}[]} workflows
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkActionSetupFollowsCheckout(workflows) {
  const problems = [];

  for (const workflow of workflows) {
    if (EXCLUDED_WORKFLOWS.has(workflow.name)) continue;

    for (const job of splitJobs(workflow.text)) {
      const checkoutAt = job.lines.findIndex((l) => l.text.includes("actions/checkout@"));
      const setupAt = job.lines.findIndex((l) => l.text.includes("pnpm/action-setup@"));
      if (setupAt === -1) continue;

      if (checkoutAt === -1 || checkoutAt > setupAt) {
        problems.push(
          `${WORKFLOWS_DIR}/${workflow.name}:${job.lines[setupAt].line} (job "${job.id}") runs ` +
            `pnpm/action-setup without a preceding actions/checkout in the same job. The action ` +
            `reads root ${PACKAGE_JSON_PATH}'s "packageManager" for its version (#2604), so it ` +
            `needs the repo on disk first.`,
        );
      }
    }
  }

  return { ok: problems.length === 0, problems };
}

/**
 * The drift assertion proper: is the pin the version actually running?
 *
 * @param {string} resolvedVersion
 * @param {string} pinnedVersion
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkResolvedVersion(resolvedVersion, pinnedVersion) {
  if (resolvedVersion === pinnedVersion) return { ok: true, problems: [] };

  return {
    ok: false,
    problems: [
      `pnpm on PATH reports ${resolvedVersion}, but root ${PACKAGE_JSON_PATH} pins ` +
        `pnpm@${pinnedVersion}. Run \`corepack prepare pnpm@${pinnedVersion} --activate\` ` +
        `(or \`bash ${SETUP_DEV_PATH}\`) — this is exactly the silent drift #2604 exists to make loud.`,
    ],
  };
}

/** @returns {{name: string, text: string}[]} */
export function readWorkflows(repoRoot = REPO_ROOT) {
  const dir = join(repoRoot, WORKFLOWS_DIR);
  return readdirSync(dir)
    .filter((name) => name.endsWith(".yml") || name.endsWith(".yaml"))
    .sort()
    .map((name) => ({ name, text: readFileSync(join(dir, name), "utf8") }));
}

/** @returns {string | null} the version `pnpm --version` reports, or null if pnpm is absent. */
export function resolvePnpmVersion(cwd = REPO_ROOT) {
  const result = spawnSync("pnpm", ["--version"], { cwd, encoding: "utf8" });
  if (result.error || result.status !== 0) return null;
  return result.stdout.trim();
}

/**
 * Run every check against the real repo.
 *
 * @returns {{ok: boolean, problems: string[], version: string | null}}
 */
export function checkPnpmPin({ repoRoot = REPO_ROOT, checkResolved = true } = {}) {
  const pin = readPin(readFileSync(join(repoRoot, PACKAGE_JSON_PATH), "utf8"));
  if (!pin.ok) return { ok: false, problems: [pin.reason], version: null };

  const workflows = readWorkflows(repoRoot);
  const problems = [
    ...checkSetupDevDerivesPin(readFileSync(join(repoRoot, SETUP_DEV_PATH), "utf8")).problems,
    ...checkWorkflowPins(workflows, pin.version).problems,
    ...checkActionSetupFollowsCheckout(workflows).problems,
  ];

  if (checkResolved) {
    const resolved = resolvePnpmVersion(repoRoot);
    if (resolved === null) {
      problems.push("pnpm is not on PATH — cannot verify the resolved version against the pin.");
    } else {
      problems.push(...checkResolvedVersion(resolved, pin.version).problems);
    }
  }

  return { ok: problems.length === 0, problems, version: pin.version };
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkPnpmPin();
  if (result.ok) {
    console.log(`ok - pnpm pinned to ${result.version} (root package.json "packageManager"), and that is what resolved.`);
  } else {
    console.error("pnpm version pin check FAILED (#2604):");
    for (const problem of result.problems) console.error(`  - ${problem}`);
    process.exitCode = 1;
  }
}
