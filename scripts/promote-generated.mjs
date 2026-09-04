// Promote a generated story into the capture tier, tests/tier4-generated/
// (issue #3380, docs/program-generator-spec.md §5).
//
//   pnpm promote:generated -- --name <case-name> --story <story.ink> \
//       --property <property> [--seed <seed>] [--issue #NNNN] \
//       [--expected-mismatch #NNNN] [--source proptest|probe] [--force]
//   pnpm promote:generated -- --name <case-name> --from-log <failing-run.log> …
//   pnpm promote:generated -- --name <case-name> --rebless-csharp
//
// A case is `tests/tier4-generated/<name>/` holding `story.ink`, the golden
// `oracle/*.oracle.json`, and a `case.toml` whose `[provenance]` records
// where the story came from and which oracle blessed the golden. The golden
// comes from `tools/inkjs-oracle` (the sanctioned stand-in for the C#
// reference — `npm ci` there first); `--rebless-csharp` re-runs the C#
// oracle (dotnet, maintainer-local) over an existing case and flips
// `oracle-source` to `csharp`. The script REFUSES when the story does not
// compile under brink, when the oracle cannot produce a golden, or when the
// case exists (without `--force`), and it bumps `GENERATED_CASE_COUNT` in
// `crates/internal/brink-test-harness/tests/tier4_generated.rs` so the
// tier's must-pass target sees the new case by count.
//
// `--from-log` takes a saved failing run of a brink-gen property (the
// differential prints the shrunk story between `--- source ---` and the
// `minimal failing input:` dump) and extracts that story, so a CI log can be
// promoted without retyping it.
//
// Every child process here carries a `timeout` (scripts/check-scripts.mjs
// check 4's discipline for Node scripts): a wedged cargo or node never
// hangs a promotion.

import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = resolve(fileURLToPath(import.meta.url), "..");
export const REPO_ROOT = resolve(here, "..");
export const TIER_DIR = join(REPO_ROOT, "tests", "tier4-generated");
export const COUNT_FILE = join(
  REPO_ROOT,
  "crates",
  "internal",
  "brink-test-harness",
  "tests",
  "tier4_generated.rs",
);
export const INKJS_ORACLE = join(REPO_ROOT, "tools", "inkjs-oracle", "oracle.mjs");
const INKJS_INSTALLED = join(REPO_ROOT, "tools", "inkjs-oracle", "node_modules", "inkjs", "package.json");

/** Bounds for the three child processes (ms). */
export const COMPILE_TIMEOUT_MS = 15 * 60 * 1000; // a cold `cargo run` builds brink-cli
export const ORACLE_TIMEOUT_MS = 5 * 60 * 1000;
export const DOTNET_TIMEOUT_MS = 10 * 60 * 1000;

const CASE_NAME_RE = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/;
const ISSUE_RE = /^#[1-9][0-9]*$/;

export class PromoteError extends Error {}

/** Parse the CLI arguments into an options object; throws on misuse. */
export function parseArgs(argv) {
  const opts = {
    name: null,
    story: null,
    fromLog: null,
    property: null,
    seed: null,
    issue: null,
    expectedMismatch: null,
    source: "proptest",
    force: false,
    reblessCsharp: false,
  };
  const takeValue = (flag, i) => {
    if (i + 1 >= argv.length) throw new PromoteError(`${flag} needs a value`);
    return argv[i + 1];
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case "--name": opts.name = takeValue(a, i++); break;
      case "--story": opts.story = takeValue(a, i++); break;
      case "--from-log": opts.fromLog = takeValue(a, i++); break;
      case "--property": opts.property = takeValue(a, i++); break;
      case "--seed": opts.seed = takeValue(a, i++); break;
      case "--issue": opts.issue = takeValue(a, i++); break;
      case "--expected-mismatch": opts.expectedMismatch = takeValue(a, i++); break;
      case "--source": opts.source = takeValue(a, i++); break;
      case "--force": opts.force = true; break;
      case "--rebless-csharp": opts.reblessCsharp = true; break;
      case "--": break;
      default: throw new PromoteError(`unknown argument ${a}`);
    }
  }
  if (!opts.name) throw new PromoteError("--name is required");
  if (!CASE_NAME_RE.test(opts.name)) {
    throw new PromoteError(`--name must be kebab-case ([a-z0-9-], no leading/trailing '-'): ${opts.name}`);
  }
  if (opts.reblessCsharp) return opts;
  if (!opts.story && !opts.fromLog) throw new PromoteError("one of --story or --from-log is required");
  if (opts.story && opts.fromLog) throw new PromoteError("--story and --from-log are mutually exclusive");
  if (!opts.property) throw new PromoteError("--property is required (the property or probe that produced the story)");
  if (opts.source !== "proptest" && opts.source !== "probe") {
    throw new PromoteError(`--source must be proptest or probe, not ${opts.source}`);
  }
  for (const [flag, v] of [["--issue", opts.issue], ["--expected-mismatch", opts.expectedMismatch]]) {
    if (v !== null && !ISSUE_RE.test(v)) throw new PromoteError(`${flag} must look like #1234, not ${v}`);
  }
  return opts;
}

/**
 * The shrunk story a brink-gen property printed on failure: everything
 * between the `--- source ---` marker and the ` at <file>:<line>.` /
 * `minimal failing input:` trailer. Returns the LAST such block (proptest
 * prints the final, most-shrunk story last).
 */
export function extractSourceFromLog(log) {
  const marker = "--- source ---\n";
  const start = log.lastIndexOf(marker);
  if (start === -1) throw new PromoteError("no `--- source ---` block in the log");
  const rest = log.slice(start + marker.length);
  const endCandidates = [rest.indexOf("\nminimal failing input:"), rest.search(/\n at [^\n]*:\d+\.\n/)]
    .filter((i) => i !== -1);
  const end = endCandidates.length ? Math.min(...endCandidates) : rest.length;
  const story = rest.slice(0, end).replace(/\n+$/, "");
  if (!story.trim()) throw new PromoteError("the `--- source ---` block is empty");
  return `${story}\n`;
}

const tomlString = (s) => JSON.stringify(s);

/** The `case.toml` text for a promotion. */
export function renderCaseToml({ source, property, seed, oracleSource, issue, expectedMismatch }) {
  const lines = [
    "# Capture-tier case (issue #3380, docs/program-generator-spec.md §5).",
    "# Written by scripts/promote-generated.mjs; the golden under oracle/ came",
    `# from the ${oracleSource === "csharp" ? "C# oracle (tools/ink-oracle)" : "inkjs oracle (tools/inkjs-oracle)"}.`,
    "",
    "[provenance]",
    `source = ${tomlString(source)}`,
    `property = ${tomlString(property)}`,
  ];
  if (seed) lines.push(`seed = ${tomlString(seed)}`);
  lines.push(`oracle-source = ${tomlString(oracleSource)}`);
  if (issue) lines.push(`issue = ${tomlString(issue)}`);
  if (expectedMismatch) {
    lines.push("", "[source]", `expected_mismatch = ${tomlString(expectedMismatch)}`);
  }
  return `${lines.join("\n")}\n`;
}

/** Flip `oracle-source` in an existing case.toml to `csharp`. */
export function reblessCaseToml(text) {
  if (!/^oracle-source = "inkjs"$/m.test(text)) {
    throw new PromoteError("case.toml has no `oracle-source = \"inkjs\"` line to flip");
  }
  return text
    .replace(/^oracle-source = "inkjs"$/m, 'oracle-source = "csharp"')
    .replace(/^# from the inkjs oracle \(tools\/inkjs-oracle\)\.$/m, "# from the C# oracle (tools/ink-oracle), re-blessed.");
}

/** Bump `GENERATED_CASE_COUNT` by `delta`; returns the new text and count. */
export function bumpCount(text, delta) {
  const re = /^const GENERATED_CASE_COUNT: usize = (\d+);$/m;
  const m = text.match(re);
  if (!m) throw new PromoteError(`GENERATED_CASE_COUNT not found in ${COUNT_FILE}`);
  const next = Number(m[1]) + delta;
  return { text: text.replace(re, `const GENERATED_CASE_COUNT: usize = ${next};`), count: next };
}

/** Real process runners; tests inject fakes. */
export const defaultRunners = {
  compileWithBrink(storyPath, outPath) {
    execFileSync(
      "cargo",
      ["run", "-q", "-p", "brink-cli", "--", "compile", storyPath, "-o", outPath],
      { cwd: REPO_ROOT, stdio: ["ignore", "pipe", "pipe"], timeout: COMPILE_TIMEOUT_MS },
    );
  },
  inkjsOracle(storyPath, outDir) {
    if (!existsSync(INKJS_INSTALLED)) {
      throw new PromoteError("tools/inkjs-oracle is not installed — run `npm ci` in tools/inkjs-oracle first");
    }
    execFileSync("node", [INKJS_ORACLE, storyPath, "--output-dir", outDir], {
      cwd: REPO_ROOT,
      stdio: ["ignore", "pipe", "pipe"],
      timeout: ORACLE_TIMEOUT_MS,
    });
  },
  csharpOracle(storyPath, outDir) {
    execFileSync(
      "dotnet",
      ["run", "--project", join(REPO_ROOT, "tools", "ink-oracle"), "--", storyPath, "--output-dir", outDir, "--warn-and-continue"],
      { cwd: REPO_ROOT, stdio: ["ignore", "pipe", "pipe"], timeout: DOTNET_TIMEOUT_MS },
    );
  },
};

function describeFailure(e) {
  const stderr = e && e.stderr ? String(e.stderr).trim() : "";
  return stderr ? `${e.message}\n${stderr}` : String(e && e.message ? e.message : e);
}

/**
 * Promote one story. `deps` lets tests substitute the process runners and
 * the tier/count paths.
 */
export function promote(opts, deps = {}) {
  const runners = { ...defaultRunners, ...(deps.runners ?? {}) };
  const tierDir = deps.tierDir ?? TIER_DIR;
  const countFile = deps.countFile ?? COUNT_FILE;
  const caseDir = join(tierDir, opts.name);

  if (opts.reblessCsharp) {
    if (!existsSync(join(caseDir, "case.toml"))) throw new PromoteError(`no such case: ${caseDir}`);
    const oracleDir = join(caseDir, "oracle");
    try {
      runners.csharpOracle(join(caseDir, "story.ink"), oracleDir);
    } catch (e) {
      throw new PromoteError(`the C# oracle failed (is dotnet installed?):\n${describeFailure(e)}`);
    }
    const tomlPath = join(caseDir, "case.toml");
    writeFileSync(tomlPath, reblessCaseToml(readFileSync(tomlPath, "utf8")));
    return { caseDir, reblessed: true, episodes: countEpisodes(oracleDir) };
  }

  const story = opts.story ? readFileSync(opts.story, "utf8") : extractSourceFromLog(readFileSync(opts.fromLog, "utf8"));
  if (!story.trim()) throw new PromoteError("the story is empty");
  const existed = existsSync(caseDir);
  if (existed && !opts.force) throw new PromoteError(`case exists: ${caseDir} (pass --force to overwrite)`);

  // 1. brink must compile it — a story brink rejects has nothing to compare.
  const scratch = mkdtempSync(join(tmpdir(), "brink-promote-"));
  try {
    const storyPath = join(scratch, "story.ink");
    writeFileSync(storyPath, story);
    try {
      runners.compileWithBrink(storyPath, join(scratch, "story.inkb"));
    } catch (e) {
      throw new PromoteError(`brink does not compile the story — not promoted:\n${describeFailure(e)}`);
    }
    // 2. the reference must produce a golden.
    const oracleScratch = join(scratch, "oracle");
    try {
      runners.inkjsOracle(storyPath, oracleScratch);
    } catch (e) {
      throw new PromoteError(`the inkjs oracle could not produce a golden — not promoted:\n${describeFailure(e)}`);
    }
    const episodes = countEpisodes(oracleScratch);
    if (episodes === 0) throw new PromoteError("the inkjs oracle produced no episodes — not promoted");

    // 3. write the case.
    if (existed) rmSync(caseDir, { recursive: true, force: true });
    mkdirSync(join(caseDir, "oracle"), { recursive: true });
    writeFileSync(join(caseDir, "story.ink"), story);
    for (const f of readdirSync(oracleScratch)) {
      if (f.endsWith(".oracle.json")) writeFileSync(join(caseDir, "oracle", f), readFileSync(join(oracleScratch, f)));
    }
    writeFileSync(
      join(caseDir, "case.toml"),
      renderCaseToml({
        source: opts.source,
        property: opts.property,
        seed: opts.seed,
        oracleSource: "inkjs",
        issue: opts.issue,
        expectedMismatch: opts.expectedMismatch,
      }),
    );
    // 4. the must-pass target counts cases; a new one bumps the constant.
    let count = null;
    if (!existed) {
      const bumped = bumpCount(readFileSync(countFile, "utf8"), 1);
      writeFileSync(countFile, bumped.text);
      count = bumped.count;
    }
    return { caseDir, reblessed: false, episodes, count };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function countEpisodes(dir) {
  if (!existsSync(dir)) return 0;
  return readdirSync(dir).filter((f) => f.endsWith(".oracle.json")).length;
}

function main(argv) {
  let opts;
  try {
    opts = parseArgs(argv);
  } catch (e) {
    console.error(`promote-generated: ${e.message}`);
    console.error("usage: pnpm promote:generated -- --name <case> (--story <story.ink> | --from-log <log>) --property <name> [--seed <s>] [--issue #N] [--expected-mismatch #N] [--source proptest|probe] [--force]");
    console.error("       pnpm promote:generated -- --name <case> --rebless-csharp");
    return 2;
  }
  try {
    const result = promote(opts);
    if (result.reblessed) {
      console.error(`promote-generated: re-blessed ${result.caseDir} with the C# oracle (${result.episodes} episodes); oracle-source is now csharp`);
    } else {
      console.error(`promote-generated: wrote ${result.caseDir} (${result.episodes} episodes, golden by inkjs)`);
      if (result.count !== null) console.error(`promote-generated: GENERATED_CASE_COUNT is now ${result.count} (${COUNT_FILE})`);
      console.error("promote-generated: run `cargo test -p brink-test-harness --test tier4_generated` to confirm");
    }
    return 0;
  } catch (e) {
    if (e instanceof PromoteError) {
      console.error(`promote-generated: ${e.message}`);
      return 1;
    }
    throw e;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv.slice(2));
}
