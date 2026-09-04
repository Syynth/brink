#!/usr/bin/env node
// The inkjs reference oracle CLI — tools/ink-oracle's Program.cs on inkjs
// (docs/program-generator-spec.md §6, #3379).
//
//   node oracle.mjs <story.ink> --output-dir <dir> [--strict-warnings]
//   node oracle.mjs --crawl <tests-dir> --output-root <dir> [--strict-warnings]
//
// Unlike the C# tool, `--output-dir` is REQUIRED in single-file mode and
// `--crawl` writes under `--output-root/<case-relative-path>/`: this tool
// never writes next to a checked-in golden, because its whole job is to be
// compared against those goldens (`brink-test-harness`'s inkjs sanction),
// never to replace them. Only the C# oracle blesses a golden.
//
// Exit codes match the C# tool: 0 on success, 1 on a compile/explore failure.

import { existsSync, mkdirSync, readFileSync, readdirSync, unlinkSync, writeFileSync, statSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";

import { CompileError, Explorer, compileStory } from "./explorer.mjs";

function usage() {
  console.error("Usage:");
  console.error("  node oracle.mjs <story.ink> --output-dir <dir> [--strict-warnings]");
  console.error("  node oracle.mjs --crawl <tests-dir> --output-root <dir> [--strict-warnings]");
  return 1;
}

function flagValue(args, name) {
  const at = args.indexOf(name);
  return at !== -1 && at + 1 < args.length ? args[at + 1] : null;
}

/** Explore one story into `outputDir`; returns the episode count. */
export function generateOracle(inkPath, outputDir, { strictWarnings = false } = {}) {
  inkPath = resolve(inkPath);
  const source = readFileSync(inkPath, "utf8");
  const storyDir = dirname(inkPath);

  const story = compileStory(source, storyDir, basename(inkPath));
  const explorer = new Explorer(story, {}, { strictWarnings });
  const episodes = explorer.explore();

  mkdirSync(outputDir, { recursive: true });
  for (const old of readdirSync(outputDir)) {
    if (old.endsWith(".oracle.json")) unlinkSync(join(outputDir, old));
  }
  episodes.forEach((episode, i) => {
    writeFileSync(join(outputDir, `e${i}.oracle.json`), `${JSON.stringify(episode, null, 2)}\n`);
  });
  return { episodes: episodes.length, warnings: explorer.warnings.length };
}

function processSingleFile(args) {
  const inkPath = args[0];
  const outputDir = flagValue(args, "--output-dir");
  if (outputDir === null) return usage();
  if (!existsSync(inkPath)) {
    console.error(`File not found: ${inkPath}`);
    return 1;
  }
  try {
    const { episodes, warnings } = generateOracle(inkPath, outputDir, {
      strictWarnings: args.includes("--strict-warnings"),
    });
    const warn = warnings > 0 ? ` (${warnings} runtime warning(s) recorded, not fatal)` : "";
    console.error(`  OK: ${episodes} episodes -> ${outputDir}${warn}`);
    return 0;
  } catch (e) {
    if (e instanceof CompileError) {
      for (const m of e.messages) console.error(`  Compile error: ${m}`);
      console.error(`  COMPILE FAILED: ${inkPath}`);
    } else {
      console.error(`  EXPLORE FAILED: ${inkPath}: ${e instanceof Error ? e.message : String(e)}`);
    }
    return 1;
  }
}

function* storyFiles(root) {
  const entries = readdirSync(root).sort();
  for (const name of entries) {
    const full = join(root, name);
    if (statSync(full).isDirectory()) yield* storyFiles(full);
    else if (name === "story.ink") yield full;
  }
}

function crawlDirectory(args) {
  const rootDir = flagValue(args, "--crawl");
  const outputRoot = flagValue(args, "--output-root");
  if (rootDir === null || outputRoot === null) return usage();
  if (!existsSync(rootDir)) {
    console.error(`Directory not found: ${rootDir}`);
    return 1;
  }
  const strictWarnings = args.includes("--strict-warnings");
  const files = [...storyFiles(resolve(rootDir))];
  console.error(`Found ${files.length} story.ink files`);
  let succeeded = 0;
  let failed = 0;
  for (const inkPath of files) {
    const rel = relative(resolve(rootDir), dirname(inkPath));
    const outputDir = join(resolve(outputRoot), rel);
    const label = `[${succeeded + failed + 1}/${files.length}] ${rel}`;
    try {
      const { episodes } = generateOracle(inkPath, outputDir, { strictWarnings });
      console.error(`${label}  OK: ${episodes} episodes`);
      succeeded++;
    } catch (e) {
      const msg = e instanceof CompileError ? `COMPILE FAILED: ${e.messages[0] ?? ""}` : `EXPLORE FAILED: ${e instanceof Error ? e.message : String(e)}`;
      console.error(`${label}  ${msg}`);
      failed++;
    }
  }
  console.error(`\nDone: ${succeeded} succeeded, ${failed} failed`);
  return failed > 0 ? 1 : 0;
}

function main(args) {
  if (args.length === 0) return usage();
  if (args[0] === "--crawl") return crawlDirectory(args);
  return processSingleFile(args);
}

if (process.argv[1] && resolve(process.argv[1]) === new URL(import.meta.url).pathname) {
  process.exitCode = main(process.argv.slice(2));
}
