// Repo-wide guard: every crate's integration tests compile as ONE test binary.
//
// Each crate's integration tests live under `tests/integration/`, rooted at
// `tests/integration/main.rs`, which cargo autodiscovers as the test target
// `integration` (docs/decision-log.md, "Integration tests consolidate into one
// test binary per crate"). Two ways to undo that go unnoticed without this check:
//
//   1. A file added under `tests/integration/` that no `mod` declaration
//      reaches silently never compiles. rustc does not warn about a file nobody
//      references, so its tests vanish while every gate stays green.
//   2. A new top-level `tests/<name>.rs` is autodiscovered as its own test
//      binary, which links its own copy of the crate's whole dependency graph.
//      Nothing breaks; the per-file binaries just creep back.
//
// (1) is checked by following `mod` declarations from `main.rs` the way rustc
// resolves them (`#[path = "..."]` included); any `.rs` file under
// `tests/integration/` that the walk does not reach is reported. The walk is a
// line-oriented scan, not a parser: an inline `mod name { ... }` is skipped
// (its contents live in the same file), and `mod` lines inside block comments
// or string literals would be misread. None exist today.
//
// (2) allows only the targets listed in SEPARATE_TARGETS, each kept apart for a
// stated reason: they change or measure process-global state (the working
// directory, process-wide counters), which every other test in a shared binary
// would disturb while they run.
//
// Exported as pure functions over a repo root so check-integration-tests.test.mjs
// can drive them with synthetic crates; the CLI at the bottom checks the real repo.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, normalize, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");

/**
 * Top-level `tests/*.rs` files allowed to stay their own test binary, as
 * repo-relative paths, each with the reason.
 */
export const SEPARATE_TARGETS = new Map([
  [
    "crates/brink-compiler/tests/issue_1504_root_content_identity.rs",
    "calls std::env::set_current_dir, which would move every concurrently running test in a shared binary",
  ],
  [
    "crates/internal/brink-db/tests/issue_2320_root_relative_conventions_pointer.rs",
    "calls std::env::set_current_dir, which would move every concurrently running test in a shared binary",
  ],
  [
    "crates/brink-runtime/tests/field_mutator_cow.rs",
    "resets and reads brink_runtime::bench_counters, process-wide atomics every other VM test in a shared binary bumps",
  ],
  [
    "crates/internal/brink-test-harness/tests/t2_ground_truth_effects.rs",
    "resets and reads brink_runtime::effect_trace, a process-wide recorder every other VM test in a shared binary writes",
  ],
]);

/**
 * Every directory under `root` that holds a `Cargo.toml` and a `tests/`
 * directory, skipping build output and VCS folders. Sorted.
 *
 * @param {string} root absolute directory to search
 * @returns {string[]} absolute crate directories
 */
export function findCratesWithTests(root) {
  const found = [];
  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    if (entries.some((e) => e.isFile() && e.name === "Cargo.toml") && isDir(join(dir, "tests"))) {
      found.push(dir);
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (entry.name === "target" || entry.name === "node_modules" || entry.name.startsWith(".")) continue;
      walk(join(dir, entry.name));
    }
  };
  walk(root);
  return found.sort();
}

function isDir(path) {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

function isFile(path) {
  try {
    return statSync(path).isFile();
  } catch {
    return false;
  }
}

/**
 * Every `.rs` file under `dir`, recursively. Sorted.
 *
 * @param {string} dir absolute directory
 * @returns {string[]} absolute file paths
 */
export function listRustFiles(dir) {
  const found = [];
  const walk = (d) => {
    let entries;
    try {
      entries = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const child = join(d, entry.name);
      if (entry.isDirectory()) walk(child);
      else if (entry.isFile() && entry.name.endsWith(".rs")) found.push(child);
    }
  };
  walk(dir);
  return found.sort();
}

const MOD_DECL = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;/;
const PATH_ATTR = /#\[path\s*=\s*"([^"]+)"\s*\]/;

/**
 * The files a Rust source file declares as out-of-line modules, resolved as
 * rustc resolves them: a `#[path]` is relative to the declaring file's
 * directory; without one, `main.rs`/`lib.rs`/`mod.rs` look beside themselves
 * and any other `name.rs` looks in its own `name/` directory. A declaration
 * whose file does not exist is returned with `exists: false`.
 *
 * @param {string} file absolute path of the declaring file
 * @param {string} text its contents
 * @returns {{ name: string, target: string, exists: boolean }[]}
 */
export function declaredModules(file, text) {
  const dir = dirname(file);
  const base = file.slice(dir.length + 1);
  const ownsDir = base === "main.rs" || base === "lib.rs" || base === "mod.rs";
  const childDir = ownsDir ? dir : join(dir, base.replace(/\.rs$/, ""));
  const lines = text.split("\n");
  const out = [];
  for (let i = 0; i < lines.length; i += 1) {
    const m = MOD_DECL.exec(lines[i]);
    if (!m) continue;
    const name = m[1];
    // The attribute block above the declaration (it can span lines): stop at a
    // blank line or a statement end.
    let path;
    for (let j = i - 1; j >= 0; j -= 1) {
      const s = lines[j].trim();
      if (!s || s.endsWith(";") || s.endsWith("}")) break;
      const p = PATH_ATTR.exec(s);
      if (p) {
        path = p[1];
        break;
      }
    }
    if (path !== undefined) {
      const target = normalize(join(dir, path));
      out.push({ name, target, exists: isFile(target) });
      continue;
    }
    const flat = join(childDir, `${name}.rs`);
    const nested = join(childDir, name, "mod.rs");
    if (isFile(flat)) out.push({ name, target: flat, exists: true });
    else if (isFile(nested)) out.push({ name, target: nested, exists: true });
    else out.push({ name, target: flat, exists: false });
  }
  return out;
}

/**
 * @typedef {{ crate: string, path: string, problem: string }} Offense
 */

/**
 * Check one crate's `tests/` directory.
 *
 * @param {string} crateDir absolute crate directory
 * @param {string} repoRoot absolute repo root (for repo-relative paths in reports)
 * @param {Map<string, string>} separate allowed top-level targets
 * @returns {Offense[]}
 */
export function checkCrate(crateDir, repoRoot, separate = SEPARATE_TARGETS) {
  const offenses = [];
  const crate = relative(repoRoot, crateDir);
  const testsDir = join(crateDir, "tests");
  const rel = (p) => relative(repoRoot, p);

  for (const entry of readdirSync(testsDir, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".rs")) continue;
    const path = rel(join(testsDir, entry.name));
    if (separate.has(path)) continue;
    offenses.push({
      crate,
      path,
      problem:
        "is a top-level tests/*.rs file, so cargo builds it as its own test binary. Move it into " +
        "tests/integration/ and declare it in tests/integration/main.rs (or, if it must stay a " +
        "separate binary, add it to SEPARATE_TARGETS with the reason).",
    });
  }

  const integrationDir = join(testsDir, "integration");
  if (!isDir(integrationDir)) return offenses;
  const root = join(integrationDir, "main.rs");
  if (!isFile(root)) {
    offenses.push({
      crate,
      path: rel(integrationDir),
      problem: "has no main.rs, so nothing under it compiles.",
    });
    return offenses;
  }

  const reached = new Set([root]);
  const queue = [root];
  while (queue.length > 0) {
    const file = queue.shift();
    for (const decl of declaredModules(file, readFileSync(file, "utf8"))) {
      if (!decl.exists) {
        offenses.push({
          crate,
          path: rel(file),
          problem: `declares \`mod ${decl.name};\` but ${rel(decl.target)} does not exist.`,
        });
        continue;
      }
      if (!reached.has(decl.target)) {
        reached.add(decl.target);
        queue.push(decl.target);
      }
    }
  }
  for (const file of listRustFiles(integrationDir)) {
    if (reached.has(file)) continue;
    offenses.push({
      crate,
      path: rel(file),
      problem:
        "is not reached by any `mod` declaration from tests/integration/main.rs, so it never " +
        "compiles and its tests never run. Declare it (`mod <name>;` in main.rs, or in the module " +
        "that owns it).",
    });
  }
  return offenses;
}

/**
 * Check every crate under `crates/`.
 *
 * @param {{ repoRoot?: string, separate?: Map<string, string> }} [options]
 * @returns {{ ok: boolean, offenses: Offense[], crates: number }}
 */
export function checkIntegrationTests({ repoRoot = REPO_ROOT, separate = SEPARATE_TARGETS } = {}) {
  const cratesRoot = join(repoRoot, "crates");
  const crates = existsSync(cratesRoot) ? findCratesWithTests(cratesRoot) : [];
  const offenses = crates.flatMap((c) => checkCrate(c, repoRoot, separate));
  for (const path of separate.keys()) {
    if (!isFile(join(repoRoot, path))) {
      offenses.push({
        crate: dirname(dirname(path)),
        path,
        problem: "is listed in SEPARATE_TARGETS but does not exist; drop the stale entry.",
      });
    }
  }
  return { ok: offenses.length === 0, offenses, crates: crates.length };
}

const invokedDirectly =
  process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkIntegrationTests();
  if (result.ok) {
    console.log(
      `ok - ${result.crates} crates with tests: each crate's integration tests compile as one binary, ` +
        `every file under tests/integration/ is declared, and only the ${SEPARATE_TARGETS.size} ` +
        `sanctioned top-level tests/*.rs targets remain.`,
    );
  } else {
    console.error("integration-test consolidation guard FAILED:");
    for (const o of result.offenses) console.error(`  - ${o.path} ${o.problem}`);
    process.exitCode = 1;
  }
}
