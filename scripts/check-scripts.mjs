// Three mechanical checks over this repo's shell sources (#2648, #2647,
// #2666, #2667, #2677, #2678).
//
// ─────────────────────────────────────────────────────────────────────────────
// WHY THIS FILE EXISTS (#2648)
//
// Three times running, a hand-written enumeration of "the unbounded network
// fetches left in scripts/setup-dev.sh" was itself incomplete:
//
//   1. #2591 listed five and missed `rustup show` and `corepack prepare`.
//   2. #2638 was filed for those two and missed a third — `pnpm --version`
//      run through corepack's shim, which downloads the pinned pnpm tarball
//      on a cache miss. Caught only by PR #2642's adversarial review.
//   3. #2642 fixed that one by hand, again with nothing that would catch a
//      fourth instance automatically.
//
// Hand enumeration is 0-for-3 on completeness, so the enumeration moves here.
//
// A FOURTH round happened anyway (#2667), one level up: this file shipped in
// #2656 pointed at a single hardcoded path, and scripts/refresh-excluded-
// lockfiles.sh sat outside the scan the whole time with two bare
// `cargo update` calls in it. So the SET OF FILES stopped being enumerated
// too — `discoverShellScripts` walked scripts/ — and check 3 closed the
// "is this command even known" level that #2666 named.
//
// A FIFTH round is #2677, and it is the same shape a third time: "walk
// scripts/" is an enumeration of the DIRECTORY. Two whole surfaces sat
// outside it, both of them things a developer types:
//
//   - justfile recipe bodies — `just wasm`, `just book-ts-check`, `just
//     book-assets`, `just studio-build` held EIGHT unbounded fetches between
//     them, and `book-assets` is additionally a CI lane (book.yml).
//   - benchmarks/setup.sh — reached from `just cross-language-benchmark`,
//     holding THREE more (`cargo install`, `npm install`, `brew install`).
//
// So the walk now starts at the repo root (see `discoverShellScripts`) and
// the justfile is translated into a shell view (see `justfileShellView`).
// #2678 is the same lesson applied to check 2: it was hardwired to
// setup-dev.sh's `BRINK_SETUP_` prefix, so the `BRINK_REFRESH_` table #2671
// added was checked by nothing. It is now per-`(path, prefix)` over the
// `KNOB_TABLES` registry — with `findUnregisteredKnobTables` DISCOVERING any
// script that assigns a `BRINK_*_TIMEOUT` nobody registered, because a
// registry alone would just be enumeration round six.
//
// ─────────────────────────────────────────────────────────────────────────────
// WHAT IS CHECKED WHERE — read this before saying "every script is checked"
//
//   Check 1  findUnboundedFetches        every discovered source
//   Check 2a checkKnobTable              each script REGISTERED in
//                                        KNOB_TABLES, against ITS OWN prefix
//                                        (+ findUnregisteredKnobTables over
//                                        every discovered source)
//   Check 2b checkDocPointers            setup-dev.sh ONLY — the three
//                                        delegating docs are #2640's, and
//                                        #2640 was about that one script
//   Check 3  findUnclassifiedCommands    every discovered source
//
// "Every discovered source" is defined by `discoverShellSources`, and the
// things it deliberately does NOT discover are listed in their own section
// below. Check 2b is the one check that is still about a single file.
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 1 (`findUnboundedFetches`) DOES
//
// It is a LEXICAL scan of a script's text. Nothing more:
//
//   a. Physical lines are joined into logical lines across a trailing `\`,
//      `|`, `||` or `&&`, and across an unclosed `(` (a multi-line array
//      literal is one statement).
//   b. Whole-line comments (`^\s*#`) are dropped.
//   c. Each logical line is split into segments by `splitSegmentsQuoteAware`
//      on `;`, `|` and `&&` OUTSIDE quotes and outside `$( … )`, with a
//      trailing `#` comment dropped.
//   d. `command -v foo` / `-V` / `-p` is blanked: it is a PATH lookup, not an
//      invocation, so it can never fetch.
//   e. A segment whose first word is a loop/case keyword (`for`, `while`,
//      `until`, `case`) is always skipped. A segment whose first word is an
//      output/read builtin (`echo`, `printf`, `read`) is skipped UNLESS it
//      contains a command substitution or a PROCESS SUBSTITUTION (`<( … )`,
//      `>( … )` — #2677's gap 3) — and then only the SUBSTITUTION
//      INTERIORS are scanned, not the prose, because `echo "==> wasm-pack
//      already installed ($(wasm-pack --version))"` runs one command and
//      merely prints the other name. A backslash-escaped backtick
//      (`echo "\`literal\`"`) is displayed text, not a fetch; an UNESCAPED
//      backtick makes the whole segment be scanned, since backtick nesting
//      is not parsed here and over-reporting is the safe direction.
//   f. Each remaining piece is matched against NETWORK_COMMANDS — a small
//      hand-maintained ALLOWLIST of command shapes known to touch the network.
//   g. A matched segment must also contain the literal `run_with_timeout`,
//      OR carry an `allow-unbounded` waiver comment on the line above it
//      (`WAIVER_PRAGMA`). If neither, it is reported.
//
// ─────────────────────────────────────────────────────────────────────────────
// WHAT IS NOT A SOURCE AT ALL — the scope of the scan, stated (#2677)
//
// `discoverShellSources` = every `*.sh` in the repo outside `PRUNED_DIRS` and
// nested checkouts, plus `.githooks/*`, plus the justfile's recipe bodies.
// Three surfaces #2677 asked about are deliberately NOT in that set. Silence
// about them would be the same failure as an incomplete enumeration:
//
//   - `.github/workflows/*.yml` `run:` BLOCKS. Not scanned, and check 1 as
//     written could not be made to pass over them: "bounded" here means the
//     literal token `run_with_timeout`, which is a bash FUNCTION defined in
//     scripts/lib/run-with-timeout.sh and sourced per-script. A workflow step
//     is a fresh shell that has not sourced it, so greening this check across
//     CI would mean sourcing a repo file into every step in every lane. The
//     hazard also differs in kind: GitHub Actions bounds every job on its own
//     (`timeout-minutes`, defaulting to 360), so a wedged fetch there is a red
//     step with a full log — not the SILENT hang at a developer's first
//     session start that this file exists for. The right guard for a workflow
//     is an explicit `timeout-minutes`, which is a different check over a
//     different file type; #2677's scope note records it.
//     NOTE what this does NOT fix: `every_pnpm_install_lane_builds_wasm_first_
//     in_the_same_job` (packages/brink-desktop/src-tauri) still cannot see
//     into `just book-assets`. Scanning the justfile puts that recipe inside
//     THIS file's checks; it does not extend that Rust ordering test.
//   - `packages/*/scripts/*.mjs`. Not scanned, because they are Node ESM and
//     every check here is a shell-line tokenizer — running it over JavaScript
//     is a category error, not a conservative approximation. They DO invoke
//     fetch-capable commands (`ensure-wasm.mjs` and `ensure-cli-sidecar.mjs`
//     shell out to `wasm-pack`/`cargo` via `execSync`), and the bound for
//     those is `execSync`'s own `timeout` option — a different mechanism
//     needing a different check. Recorded in #2677's scope note.
//   - `*.test.sh`. Excluded from discovery, with a cost that is now MEASURED
//     rather than asserted. Heredoc-awareness alone does not make them
//     scannable: blanking every heredoc body in the two harnesses still
//     leaves 111 false fetch reports, because the harnesses' own assertion
//     PROSE names the tools (`fail "pnpm drift: …"`, `pass "… reached the
//     pnpm section"`) and `pass`/`fail` are not output builtins the way
//     `echo` is. So a genuine unbounded fetch written in a `*.test.sh` is NOT
//     seen. Those files run under `pnpm test:setup-dev` / `pnpm
//     test:scripts`, where a hang is a hung CI step rather than a silent hang
//     at a developer's session start — which is why the trade is acceptable
//     here and not in the scripts themselves.
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 1 CANNOT SEE — read this before trusting it
//
// This check reduces the odds of a sixth miss. It does not eliminate them,
// and a guard's comment must state the check it performs rather than the
// strongest-sounding version of it (#2610, #2613). The known holes:
//
//   - INDIRECT FETCHES ARE INVISIBLE. A command that fetches through some
//     other program's cache-miss path looks exactly like a local command.
//     `pnpm --version` — the miss #2642's review caught — is the canonical
//     example: the text says "print a version", the behaviour is "download a
//     tarball from the npm registry". NOTHING here detects that. `pnpm` is
//     bounded-checked only because it is written into NETWORK_COMMANDS BY
//     HAND, with `probeStillFetches: true` so the `--version` exemption
//     below does not swallow it. A future tool with the same shape is
//     invisible to check 1 until a human adds it to the allowlist — check 3
//     now at least forces someone to LOOK at it and decide.
//   - THE ALLOWLIST IS STILL THE CEILING FOR BOUNDEDNESS. Check 3 makes an
//     unlisted command an explicit decision, but whoever makes that decision
//     can put it in LOCAL_COMMANDS and the boundedness question never gets
//     asked. The judgement is human; only the prompt is mechanical.
//   - THE `--version` EXEMPTION IS A HOLE BY CONSTRUCTION. When the matched
//     command is immediately followed by `--version` or `-V`, it is treated
//     as a local probe, so `cargo deny --version` in `cargo_deny_ok()` does
//     not have to be bounded. That exemption is precisely the shape of the
//     third miss; it is kept because bounding every version probe is noise,
//     and it is made safe only for the package managers explicitly opted out
//     of it via `probeStillFetches`.
//   - A WAIVER IS A HOLE BY CONSTRUCTION. `allow-unbounded` lets any command
//     out of check 1 for the price of a 40-character reason. It is checked
//     both ways — a stale waiver whose command is now bounded is reported —
//     but nothing here can judge whether the reason is TRUE. There is one in
//     the tree (`just studio-dev`, a Vite dev server a bound would kill).
//   - HEREDOCS ARE NOT PARSED. A fetch written inside a heredoc body is
//     scanned as if it were a command line (over-report). This is one of two
//     reasons `*.test.sh` is excluded from discovery — see "what is not a
//     source at all" above for the other, and for the measurement.
//   - THE JUSTFILE IS TRANSLATED, NOT PARSED. `justfileShellView` keeps
//     recipe bodies and comments and blanks everything else; `{{ … }}`
//     interpolation passes through as literal text, so a fetch reached only
//     through a recipe PARAMETER is invisible, the same way one reached
//     through a shell variable is.
//   - QUOTING IS TOKENIZED, NOT PARSED. `splitSegmentsQuoteAware` tracks
//     single/double quotes, backslash escapes and `$( … )` nesting, which is
//     enough for these scripts. Deeply nested double quotes inside a
//     substitution (`"$(cd "$(dirname "$x")" && pwd)"`) defeat the depth
//     counter, and the interior is then taken to end-of-segment — again an
//     over-scan, not an under-scan.
//   - "BOUNDED" IS LEXICAL. The presence of `run_with_timeout` in the same
//     segment is all that is verified. It does NOT verify the bound is
//     applied to the fetching command, that the timeout value is sane, or
//     that the 124 exit code is handled. Those are behavioural properties;
//     scripts/setup-dev.test.sh and scripts/refresh-excluded-lockfiles.test.sh
//     drive the real scripts against stubs for them, and those harnesses —
//     not this file — are what prove the $?-capture control flow is right.
//
// ─────────────────────────────────────────────────────────────────────────────
// CHECK 2 (`checkKnobTable`, `checkDocPointers`) — #2647, #2678
//
// setup-dev.sh's header carries a hand-maintained knob/default/fail-vs-warn
// table. #2640 (landed in PR #2642) made CLAUDE.md, docs/desktop-shell-spec.md
// and docs/releasing.md stop restating the knobs and DELEGATE to that table as
// authoritative. Three documents now point at a comment block, and until this
// file nothing tied the block to the script. `checkKnobTable` asserts the two
// directions (every assigned knob has a row with a matching default; every row
// names a knob the script actually reads) plus that each row's outcome cell
// says FAIL or WARN — the column an agent consults after setup aborts naming
// an env var it has never seen. `checkDocPointers` asserts the three delegating
// pointers still exist, since they are the entire delivery mechanism for #2640.
//
// #2678: that check was hardwired to `BRINK_SETUP_`, so the `BRINK_REFRESH_`
// table PR #2671 added to refresh-excluded-lockfiles.sh was cross-checked by
// nothing — the very drift #2647 was filed about, one script over. It is now
// `(path, prefix)`-parameterised over `KNOB_TABLES`, which registers four
// tables: `BRINK_SETUP_` (setup-dev.sh), `BRINK_REFRESH_`
// (refresh-excluded-lockfiles.sh), `BRINK_JUST_` (justfile) and `BRINK_BENCH_`
// (benchmarks/setup.sh). `checkDocPointers` did NOT generalise with it: only
// setup-dev.sh's table has documents delegating to it, so only it has pointers
// to keep alive.
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 2 CANNOT SEE — read this before trusting it
//
// Same discipline as check 1 above (#2610, #2613): state the check performed,
// not the strongest-sounding version of it.
//
//   - ASSIGNMENT SHAPES ARE ENUMERATED, NOT PARSED. `findKnobAssignments`
//     recognises exactly two spellings — a bare or `export`-ed
//     `NAME="${NAME:-N}"` / `NAME=${NAME:-N}` (quoted or unquoted RHS) — plus
//     `findUnrecognizedKnobShapes` as a backstop that recognises one more,
//     the colon-default idiom `: "${NAME:=N}"`, but only REPORTS it as
//     unparsed rather than validating its default against the table. Any
//     other shape (arithmetic, indirection through a second variable,
//     multi-line `printf -v`, …) is invisible to every cross-check here.
//   - THE TABLE ROW SHAPE IS FIXED. `parseKnobTable` requires
//     `#  NAME  <n>s  <outcome>` — 2+ spaces after the leading `#`, a bare
//     `\d+s` default (no other unit, no range), free-text outcome. A default
//     written any other way does not parse as a row at all, which reads
//     identically to a MISSING row.
//   - THE TABLE'S END IS THE FIRST BLANK COMMENT LINE after rows begin. A
//     table with a genuinely blank `#` line in the middle (for visual
//     grouping) would truncate silently there.
//   - `checkDocPointers` IS A PROSE-SHAPE ASSERTION, not a semantic one (see
//     its own doc comment below): reworded-but-equivalent prose can fail it,
//     and prose that repeats the required phrases while pointing somewhere
//     useless can pass it. It runs for setup-dev.sh ONLY.
//   - THE REGISTRY IS A LIST, AND `findUnregisteredKnobTables` IS ITS ONLY
//     BACKSTOP. That sweep fires on an ASSIGNMENT whose name matches
//     `BRINK_*_TIMEOUT`. A script that documents a knob table in its header
//     and then reads the env var without ever assigning a default — or that
//     names its knobs something other than `BRINK_…_TIMEOUT` — is registered
//     by nobody and caught by nothing.
//
// ─────────────────────────────────────────────────────────────────────────────
// CHECK 3 (`findUnclassifiedCommands`) — #2666
//
// Check 1 asks "is this network command bounded". Check 3 asks the question
// one level up: "do we even know this command exists". It extracts the HEAD
// of every command in every discovered source and asserts each is either a
// shell word, a function defined in one of those sources, a NETWORK_COMMANDS
// binary, or an explicitly-listed LOCAL_COMMANDS binary. Anything else is
// reported with a prompt to classify it, so a brand-new fetching binary
// becomes a decision someone has to make rather than something invisible
// until the next round's review notices it.
//
// It earned its keep again in #2677: the first justfile/benchmarks scan
// reported `brew`, which nobody had classified — and `brew install hyperfine`
// turned out to be a fourth unbounded fetch that check 1 could not have seen,
// because `brew` was not in the allowlist at all.
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 3 CANNOT SEE
//
//   - IT VERIFIES CLASSIFICATION, NOT TRUTH. `LOCAL_COMMANDS` is an
//     assertion by a human that a binary performs no fetch. Nothing here can
//     check that, and `pnpm --version` proves the assertion is easy to get
//     wrong. Check 3 forces the decision; it does not make it.
//   - VARIABLE-DISPATCHED COMMANDS ARE SKIPPED. `"$timeout_bin" -k 10 …` and
//     `"$@"` have no literal head, so they are not classified at all. The
//     binary behind the variable is invisible.
//   - CLASSIFICATION IS PER-BINARY, NOT PER-SUBCOMMAND. Once `cargo` is
//     known, `cargo <anything>` satisfies check 3; whether a PARTICULAR
//     subcommand fetches is check 1's pattern's business, and its subcommand
//     list is hand-maintained.
//   - HEREDOC BODIES AND `bash -c "…"` STRINGS. Commands written inside them
//     are not extracted as heads. `bash` itself is classified; what it is
//     told to run is not.
//   - IT SHARES CHECK 1'S TOKENIZER LIMITS above — heredocs, deep nesting.
//
// Exported as pure functions over text so scripts/check-scripts.test.mjs can
// drive them with synthetic inputs (a deleted table row, an unbounded fetch);
// the CLI at the bottom applies them to the real repo files. Node builtins
// only: this runs under `pnpm test:scripts`, which CI's `frontend` job executes
// BEFORE `pnpm install`.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");
export const SETUP_DEV_PATH = "scripts/setup-dev.sh";
export const REFRESH_LOCKFILES_PATH = "scripts/refresh-excluded-lockfiles.sh";
export const JUSTFILE_PATH = "justfile";
export const BENCHMARKS_SETUP_PATH = "benchmarks/setup.sh";

/** Kept for the message that names where setup-dev.sh is expected to live. */
export const SCRIPTS_DIR = "scripts";

/**
 * Directories never walked by `discoverShellScripts`: dependency and build
 * output, whose shell scripts are not ours to bound. Everything else in the
 * repo IS walked — see that function's note on why the walk is no longer
 * rooted at scripts/.
 */
export const PRUNED_DIRS = new Set([
  ".git",
  "node_modules",
  "target",
  "dist",
  "dist-embed",
  "pkg",
]);

/**
 * A directory whose every entry is a shell script even though none of them
 * carries a `.sh` extension. git runs whatever executable it finds here, so
 * extension-based discovery would miss the lot.
 */
export const EXTENSIONLESS_SHELL_DIRS = [".githooks"];

/**
 * Every shell script in the repo, DISCOVERED rather than enumerated (#2667,
 * widened in #2677). A hardcoded list of "the scripts we check" would be the
 * same hand-maintained-enumeration failure this file exists to end, just moved
 * one level up — which is precisely how #2667 happened: check 1 shipped in
 * #2656 with a single hardcoded `SETUP_DEV_PATH`, and a sibling script with
 * two unbounded `cargo update` calls sat outside the scan from day one.
 *
 * #2667's fix discovered `scripts/**\/*.sh` — still an enumeration, of the
 * DIRECTORY. #2677 found the next ring: `just cross-language-benchmark` runs
 * `bash benchmarks/setup.sh`, which held three unbounded fetches (`cargo
 * install`, `npm install`, `brew install`) and was outside the scan for the
 * identical reason refresh-excluded-lockfiles.sh had been. So the walk now
 * starts at the repo root and prunes only `PRUNED_DIRS`.
 *
 * `*.test.sh` is EXCLUDED, deliberately and with a MEASURED cost — see the
 * header's "cannot see" section, which carries the number.
 *
 * @param {string} [repoRoot]
 * @returns {string[]} repo-relative paths, sorted (determinism)
 */
export function discoverShellScripts(repoRoot = REPO_ROOT) {
  const found = [];

  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const child = join(dir, entry.name);
      const relative = prefix === "" ? entry.name : `${prefix}/${entry.name}`;

      if (entry.isDirectory()) {
        if (PRUNED_DIRS.has(entry.name)) continue;
        // A directory holding its own `.git` is a NESTED CHECKOUT — a git
        // worktree (`.git` file) or a submodule. Its scripts belong to that
        // checkout, and on this repo `.claude/worktrees/<id>/` holds a full
        // second copy of the tree, which an unpruned walk would scan as if it
        // were this one.
        if (existsSync(join(child, ".git"))) continue;
        walk(child, relative);
        continue;
      }

      const extensionless = EXTENSIONLESS_SHELL_DIRS.includes(prefix);
      if (!extensionless && !entry.name.endsWith(".sh")) continue;
      if (entry.name.endsWith(".test.sh")) continue;
      found.push(relative);
    }
  };

  walk(repoRoot, "");
  return found.sort();
}

/**
 * A justfile RECIPE HEADER at column 0 — `name:`, `name dep:`, `name *ARGS:`,
 * `@name:`. The `(?!=)` is what keeps a justfile VARIABLE assignment
 * (`fuzz_duration := "300"`) from reading as a recipe whose body is the rest
 * of the file.
 */
const JUST_RECIPE_HEADER = /^@?[\w.-]+(?:\s+[^:]*?)?\s*:(?!=)/;

/**
 * A line-preserving SHELL VIEW of a justfile (#2677).
 *
 * Recipe bodies are shell — either one bash script (when the body opens with
 * a `#!` shebang) or one `sh -c` per line — so the same tokenizer that scans
 * `scripts/**\/*.sh` can scan them, PROVIDED the justfile's own non-shell
 * lines (recipe headers, `name := value` assignments, `set` directives) are
 * removed first. They are replaced by EMPTY LINES rather than deleted so that
 * every reported line number is the justfile's real line number.
 *
 * Comment lines are KEPT, at both indent levels. They are dropped again by
 * `toLogicalLines`, so keeping them changes no command scan — but the knob
 * table (check 2) and the `allow-unbounded` waivers (check 1) are both written
 * as comments, and blanking them here would make this view disagree with the
 * file a human reads.
 *
 * What this view does NOT model: `{{ … }}` interpolation is passed through as
 * literal text (a recipe parameter substituted into a command is invisible,
 * the same way a shell variable is), and just's per-line `sh -c` semantics for
 * non-shebang recipes are ignored — every body line is scanned as if it shared
 * one shell. Both directions are lexical over-approximations, which is the
 * safe direction for check 1.
 *
 * @param {string} text
 * @returns {string}
 */
export function justfileShellView(text) {
  const out = [];
  let inRecipe = false;

  for (const line of text.split("\n")) {
    if (/^\s*#/.test(line)) {
      out.push(line);
      continue;
    }
    if (line.trim() === "") {
      // A blank line does not end a recipe in just — the body resumes if the
      // next line is still indented.
      out.push("");
      continue;
    }
    if (/^\s/.test(line)) {
      out.push(inRecipe ? line : "");
      continue;
    }
    inRecipe = JUST_RECIPE_HEADER.test(line);
    out.push("");
  }

  return out.join("\n");
}

/**
 * Everything checks 1 and 3 scan, as `{path, text}` where `text` is SHELL —
 * literally so for `scripts/**\/*.sh`, and via `justfileShellView` for the
 * justfile (#2677).
 *
 * The justfile is here because its recipes are developer-facing entry points
 * that fetch — `just wasm`, `just book-assets`, `just book-ts-check` — with
 * exactly the wedged-proxy hang this file exists for, and because CLAUDE.md
 * already names one of them (`just book-assets`, run by `.github/workflows/
 * book.yml`) as a lane no YAML parser can see into.
 *
 * WHAT IS DELIBERATELY NOT A SOURCE HERE — see the header's "cannot see".
 *
 * @param {string} [repoRoot]
 * @returns {{path: string, text: string}[]}
 */
export function discoverShellSources(repoRoot = REPO_ROOT) {
  const sources = discoverShellScripts(repoRoot).map((path) => ({
    path,
    text: readFileSync(join(repoRoot, path), "utf8"),
  }));

  const justfile = join(repoRoot, JUSTFILE_PATH);
  if (existsSync(justfile)) {
    sources.push({ path: JUSTFILE_PATH, text: justfileShellView(readFileSync(justfile, "utf8")) });
  }

  return sources;
}

/** The documents #2640 pointed at setup-dev.sh's header table. */
export const POINTER_DOCS = [
  { path: "CLAUDE.md", section: "Cloud / fresh-environment sessions" },
  { path: "docs/desktop-shell-spec.md" },
  { path: "docs/releasing.md" },
];

/**
 * The hand-maintained allowlist of network-touching command shapes. This is
 * the ceiling of check 1 — see "WHAT CHECK 1 CANNOT SEE" above.
 *
 * `probeStillFetches: true` opts a command OUT of the `--version` exemption,
 * because for that command a version probe is itself a fetch. Only the
 * corepack-shimmed package managers qualify, and that is the #2642 miss
 * encoded as data rather than as a fourth hand audit.
 */
export const NETWORK_COMMANDS = [
  { id: "curl", pattern: /(?:^|[\s"'`(])curl\s/, why: "a direct HTTPS fetch" },
  { id: "wget", pattern: /(?:^|[\s"'`(])wget\s/, why: "a direct HTTPS fetch" },
  {
    id: "rustup",
    pattern: /\brustup\s+(?:show|update|install|toolchain|target|component|self)\b/,
    why: "rustup resolves/downloads channels, components and targets",
  },
  {
    id: "corepack",
    pattern: /\bcorepack\s+(?:prepare|install|use|up)\b/,
    why: "corepack downloads the pinned package-manager tarball from the npm registry",
  },
  {
    id: "pnpm",
    pattern: /(?:^|[\s"'`(])pnpm\s/,
    probeStillFetches: true,
    why: "under corepack's shim ANY pnpm invocation — `--version` included — fetches the pinned tarball on a cache miss (#2642)",
  },
  {
    id: "npm",
    pattern: /(?:^|[\s"'`(])npm\s/,
    probeStillFetches: true,
    why: "npm-registry access",
  },
  {
    id: "npx",
    pattern: /(?:^|[\s"'`(])npx\s/,
    probeStillFetches: true,
    why: "npx resolves and downloads packages on demand",
  },
  {
    id: "yarn",
    pattern: /(?:^|[\s"'`(])yarn\s/,
    probeStillFetches: true,
    why: "under corepack's shim, same tarball fetch as pnpm",
  },
  {
    id: "cargo-nextest-binary",
    binary: "cargo-nextest",
    pattern: /(?:^|[\s"'`(])cargo-nextest\s/,
    why: "running nextest drives cargo's dependency resolution, so a cold cache is a crates.io fetch (setup-dev.sh only ever LOOKS it up with `command -v`, which is not an invocation)",
  },
  {
    id: "cargo-deny-binary",
    binary: "cargo-deny",
    pattern: /(?:^|[\s"'`(])cargo-deny\s/,
    why: "the standalone cargo-deny binary clones the RUSTSEC advisory DB (the `cargo deny` spelling is covered by cargo-network below; this is the one cargo does not dispatch)",
  },
  {
    id: "wasm-pack",
    pattern: /(?:^|[\s"'`(])wasm-pack\s/,
    why: "`wasm-pack build` downloads binaryen/wasm-opt from GitHub releases on a cache miss (setup-dev.sh installs wasm-opt up front precisely to pre-empt that fetch)",
  },
  {
    id: "cargo-network",
    binary: "cargo",
    pattern: /\bcargo\s+(?:install|deny|fetch|update|publish|add)\b/,
    why: "a crates.io index/dependency fetch (cargo deny additionally clones the RUSTSEC advisory DB)",
  },
  {
    id: "homebrew",
    binary: "brew",
    pattern: /\bbrew\s+(?:install|upgrade|update|tap|fetch|reinstall|bundle)\b/,
    why: "Homebrew fetches formulae and bottles over the network (found in benchmarks/setup.sh once the scan reached it, #2677)",
  },
  {
    id: "git-remote",
    binary: "git",
    pattern: /\bgit\s+(?:clone|fetch|pull|push|ls-remote|submodule)\b/,
    why: "a remote git operation",
  },
];

/** The binary each allowlist entry governs (`id` unless it says otherwise). */
export function networkBinaries() {
  return new Set(NETWORK_COMMANDS.map((command) => command.binary ?? command.id));
}

/**
 * Loop/case keywords: a segment headed by one of these never itself invokes
 * a network command (`for tool in curl pnpm; do` names tools without running
 * them), so it is always skipped, unconditionally.
 */
const STRUCTURAL_HEADS = new Set(["for", "while", "until", "case"]);

/**
 * Output/read builtins. A segment headed by one of these is skipped UNLESS
 * it contains a command substitution (`$(` or an UNESCAPED backtick) — that
 * substitution runs a real subshell command, so `echo "$(curl ...)"` still
 * fetches even though the outer command only prints. Skipping the whole
 * segment unconditionally (the pre-fix behaviour) made that fetch invisible;
 * `setup-dev.sh` contains this exact shape today, e.g.
 * `echo "==> … ($(rustup --version | head -n1))"`.
 *
 * A backtick preceded by `\` is excluded: setup-dev.sh also contains
 * `echo "… (\`which -a pnpm\`) …"`, where the backticks are escaped inside a
 * double-quoted string specifically so they print as literal punctuation
 * around a command NAME shown to the user, not so the shell runs it.
 */
const OUTPUT_HEADS = new Set(["echo", "printf", "read"]);

/**
 * Shapes that make an output/read builtin's segment run a real command:
 * `$( … )`, an UNESCAPED backtick, and — added for #2677's gap 3 — PROCESS
 * SUBSTITUTION, `<( … )` / `>( … )`. `read -r v < <(curl …)` runs curl in a
 * subshell while its segment head is the `read` builtin, so without the last
 * two alternatives the whole segment was skipped and the fetch was invisible.
 *
 * `commandSubstitutions` still only extracts `$( … )` interiors, so a segment
 * that matches ONLY on a process substitution falls back to scanning the whole
 * segment — an over-scan, which is check 1's safe direction.
 */
const COMMAND_SUBSTITUTION = /\$\(|(?<!\\)`|(?<!\\)[<>]\(/;

/** Leading tokens that wrap a command without being one. */
const WRAPPER_HEADS = new Set(["if", "elif", "then", "else", "do", "!", "{", "(", "[", "[[", "&&", "||"]);

/**
 * `(` and `)` counts for a line, taken OUTSIDE quotes and outside a trailing
 * `#` comment. Returned UNCLAMPED so the caller can clamp the running depth
 * once: clamping per line would swallow the `)` that closes a paren opened on
 * an EARLIER line, which is the whole point of the multi-line join.
 *
 * The clamp still belongs somewhere — a `)` that closes nothing is a `case`
 * label (`*)`), not a negative depth to carry forward — so `toLogicalLines`
 * applies `Math.max(0, …)` to the accumulator.
 *
 * @param {string} line
 * @returns {{opens: number, closes: number}}
 */
function parenDelta(line) {
  let opens = 0;
  let closes = 0;
  let quote = null;

  for (let i = 0; i < line.length; i += 1) {
    const char = line[i];
    if (char === "\\") {
      i += 1;
      continue;
    }
    if (quote) {
      if (char === quote) quote = null;
      continue;
    }
    if (char === "'" || char === '"') {
      quote = char;
      continue;
    }
    if (char === "#" && (i === 0 || /\s/.test(line[i - 1]))) break;
    if (char === "(") opens += 1;
    else if (char === ")") closes += 1;
  }

  return { opens, closes };
}

/**
 * Join physical lines into logical ones across a trailing `\`, `|`, `||` or
 * `&&`, and across an UNCLOSED `(` (#2677).
 *
 * The paren rule exists because a multi-line array literal —
 *
 *     targets=(
 *         "brink-syntax:parse_no_panic"
 *         …
 *     )
 *
 * — is one bash statement spread over many physical lines, and without it each
 * element line became its own logical line whose head was the element string.
 * The justfile's `fuzz` recipe reported six phantom commands
 * (`brink-format:read_no_panic`, …) that shape. Joined, the logical line opens
 * with `targets=(`, which `commandHead` already recognises as an assignment
 * whose value continues, so it invokes nothing.
 *
 * A comment line never continues, and never continues a join — including an
 * open paren, which is terminated rather than absorbed (over-reporting again,
 * not under-reporting).
 *
 * @param {string} text
 * @returns {{line: number, text: string}[]} 1-indexed start line per logical line
 */
export function toLogicalLines(text) {
  const physical = text.split("\n");
  const logical = [];

  let buffer = null;
  let openParens = 0;

  for (let i = 0; i < physical.length; i += 1) {
    const raw = physical[i];
    const isComment = /^\s*#/.test(raw);

    if (!buffer && raw.trim().length === 0) continue;

    if (isComment) {
      // Terminate any open join rather than absorbing a comment into it.
      if (buffer) {
        logical.push(buffer);
        buffer = null;
        openParens = 0;
      }
      continue;
    }

    const trimmedEnd = raw.replace(/\s+$/, "");
    const piece = trimmedEnd.replace(/\\$/, "");

    if (buffer) {
      buffer.text += ` ${piece.trim()}`;
    } else {
      buffer = { line: i + 1, text: piece };
      openParens = 0;
    }

    const delta = parenDelta(piece);
    openParens = Math.max(0, openParens + delta.opens - delta.closes);
    const continues = /\\$/.test(trimmedEnd) || /(?:\|\||&&|\||&)$/.test(trimmedEnd) || openParens > 0;

    if (!continues) {
      logical.push(buffer);
      buffer = null;
      openParens = 0;
    }
  }

  if (buffer) logical.push(buffer);
  return logical;
}

/** @param {string} segment */
function firstWord(segment) {
  let rest = segment.trim();

  // Peel wrapper tokens so `if run_with_timeout …` reports `run_with_timeout`,
  // while `for tool in … pnpm …` still reports `for`.
  for (;;) {
    const match = /^(\S+)\s+/.exec(rest);
    if (!match || !WRAPPER_HEADS.has(match[1])) break;
    rest = rest.slice(match[0].length);
  }

  return (/^\S+/.exec(rest) ?? [""])[0];
}

/**
 * Does the text immediately following a matched command start with a version
 * flag AND is that flag the entire rest of the command? See the "--version
 * exemption is a hole" note in the header.
 *
 * Checking only "is the next token --version/-V" is not enough: for some
 * subcommands `--version` takes a VALUE rather than meaning "print version
 * and exit" — `cargo install --version 1.2.3 some-crate` is a real crates.io
 * fetch even though its very next token is `--version`, because the token
 * after THAT is a package name, not a redirect/pipe/end. (`cargo install
 * some-crate --version 1.2.3`, the arg order this repo actually uses at
 * setup-dev.sh:411, was already safe under the old check only because the
 * token immediately following `install` there is the package name, not
 * `--version` — the old check was one arg-order away from a false exemption,
 * not protected by design.) So the exemption now requires the flag to be the
 * rest of the command: nothing after it but optional whitespace and then
 * end-of-string or a shell operator. Quote characters are deliberately NOT
 * accepted as "the end" here — a `"` right after `--version` is just as
 * likely to be the OPENING quote of a value (`--version "${VERSION}"`) as a
 * closing one, and this is a lexical scan with no way to tell those apart.
 *
 * @param {string} rest text after the matched command in its segment
 */
export function nextTokenIsVersionFlag(rest) {
  const match = /^\s*(?:--version|-V)\b(.*)$/.exec(rest);
  if (!match) return false;
  const after = match[1].replace(/^\s+/, "");
  return after === "" || /^(?:\)|2?>|\|)/.test(after);
}

/**
 * An explicit, reason-carrying opt-out from check 1, written as a comment
 * immediately above the command it waives:
 *
 *     # check-scripts: allow-unbounded <reason, at least MIN_WAIVER_REASON chars>
 *
 * It exists because widening the scan to the justfile (#2677) turned up a
 * command for which a bound is not merely unnecessary but WRONG: `just
 * studio-dev` runs a Vite dev server that is supposed to run until Ctrl-C, and
 * `run_with_timeout` would kill it at the bound. Before this, the only ways to
 * green such a line were to leave the surface unscanned or to delete the
 * allowlist entry — both of which hide every OTHER instance of that command
 * too.
 *
 * It is a hole by construction, and named as one in the header: anyone can
 * write a waiver. What it buys is that the decision is written down next to
 * the command, is checked for a non-trivial reason, and is checked in the
 * OTHER direction too — a waiver whose command is now bounded (or gone) is
 * reported as stale, so waivers cannot silently accumulate.
 */
export const WAIVER_PRAGMA = /^\s*#\s*check-scripts:\s*allow-unbounded\b\s*(.*)$/;
export const MIN_WAIVER_REASON = 40;

/**
 * Every `allow-unbounded` waiver in a script, resolved to the line it waives:
 * the next line that is neither blank nor a comment.
 *
 * @param {string} text
 * @returns {{pragmaLine: number, target: number, reason: string}[]}
 */
export function findWaivers(text) {
  const lines = text.split("\n");
  const waivers = [];

  for (const [index, line] of lines.entries()) {
    const match = WAIVER_PRAGMA.exec(line);
    if (!match) continue;

    let target = 0;
    for (let j = index + 1; j < lines.length; j += 1) {
      if (lines[j].trim() === "" || /^\s*#/.test(lines[j])) continue;
      target = j + 1;
      break;
    }

    waivers.push({ pragmaLine: index + 1, target, reason: match[1].trim() });
  }

  return waivers;
}

/**
 * Check 1: every allowlisted network command is lexically wrapped in
 * `run_with_timeout`, or carries an `allow-unbounded` waiver.
 *
 * @param {string} setupDevText
 * @param {string} [path] label used in problem messages
 * @returns {{ok: boolean, problems: string[], findings: {line: number, id: string, segment: string, why: string}[], waivers: {pragmaLine: number, target: number, reason: string}[]}}
 */
export function findUnboundedFetches(setupDevText, path = SETUP_DEV_PATH) {
  const findings = [];

  for (const rawSegment of toLogicalLines(setupDevText).flatMap((logical) =>
    splitSegmentsQuoteAware(logical.text).map((text) => ({ line: logical.line, text })),
  )) {
    {
      const logical = rawSegment;
      // `command -v foo` is a PATH LOOKUP, not an invocation — it never runs
      // foo and so never fetches. Neutralised before matching so that
      // `command -v wasm-pack` does not read as a wasm-pack run.
      const segment = rawSegment.text.replace(/\bcommand\s+-[vVp]\s+\S+/g, " ");
      if (segment.trim().length === 0) continue;
      const head = firstWord(segment);
      if (STRUCTURAL_HEADS.has(head)) continue;

      // What text of this segment is a COMMAND. For an output builtin it is
      // only the command-substitution interiors: `echo "==> wasm-pack already
      // installed ($(wasm-pack --version))"` runs one command (the
      // substitution) and PRINTS a tool name in prose, and matching the prose
      // reports a fetch that does not exist. If the segment has no `$(` at
      // all it invokes nothing and is skipped; if it uses an UNESCAPED
      // backtick instead, the whole segment is scanned, because this file
      // does not parse backtick nesting and over-reporting is the safe
      // direction.
      let scanned = [segment];
      if (OUTPUT_HEADS.has(head)) {
        if (!COMMAND_SUBSTITUTION.test(segment)) continue;
        const interiors = commandSubstitutions(segment);
        scanned = interiors.length > 0 ? interiors : [segment];
      }

      for (const text of scanned) {
        for (const command of NETWORK_COMMANDS) {
          const match = command.pattern.exec(text);
          if (!match) continue;

          const rest = text.slice(match.index + match[0].length);
          if (!command.probeStillFetches && nextTokenIsVersionFlag(rest)) continue;
          if (segment.includes("run_with_timeout")) continue;

          findings.push({ line: logical.line, id: command.id, segment: segment.trim(), why: command.why });
        }
      }
    }
  }

  const waivers = findWaivers(setupDevText);
  const waivedLines = new Map(waivers.map((waiver) => [waiver.target, waiver]));
  const used = new Set();

  const reported = findings.filter((finding) => {
    const waiver = waivedLines.get(finding.line);
    if (!waiver) return true;
    used.add(waiver.pragmaLine);
    return false;
  });

  const problems = reported.map(
    (finding) =>
      `${path}:${finding.line} runs a network command (${finding.id} — ${finding.why}) that is NOT wrapped in ` +
      `run_with_timeout, so a wedged proxy hangs setup with no diagnostic: ${finding.segment}`,
  );

  for (const waiver of waivers) {
    if (waiver.reason.length < MIN_WAIVER_REASON) {
      problems.push(
        `${path}:${waiver.pragmaLine} carries an \`allow-unbounded\` waiver whose reason is ${waiver.reason.length} ` +
          `characters ("${waiver.reason}"). A waiver is the one place a fetch may go unbounded, so it must say WHY ` +
          `in at least ${MIN_WAIVER_REASON} characters — "runs forever by design" is a reason, "ok" is not (#2677).`,
      );
      continue;
    }

    if (!used.has(waiver.pragmaLine)) {
      problems.push(
        `${path}:${waiver.pragmaLine} carries an \`allow-unbounded\` waiver, but the command below it is NOT ` +
          `reported as an unbounded fetch — it is bounded now, or it moved, or it is gone. Delete the stale ` +
          `waiver: a waiver nobody needs is a standing permission slip for the next command to land under it ` +
          `(#2677).`,
      );
    }
  }

  return { ok: problems.length === 0, problems, findings: reported, waivers };
}

/**
 * Shell keywords and builtins. These are the language, not binaries — there
 * is nothing to classify and nothing that could fetch.
 */
const SHELL_WORDS = new Set([
  "if", "elif", "else", "then", "fi", "for", "while", "until", "do", "done", "case", "esac", "in",
  "function", "select", "time", "coproc", "{", "}", "(", ")", "[[", "]]", "!",
  ".", "source", "alias", "bg", "bind", "break", "builtin", "cd", "command", "continue", "declare",
  "dirs", "echo", "enable", "eval", "exec", "exit", "export", "false", "fg", "getopts", "hash",
  "help", "history", "jobs", "kill", "let", "local", "logout", "mapfile", "popd", "printf",
  "pushd", "pwd", "read", "readarray", "readonly", "return", "set", "shift", "shopt", "test",
  "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias", "unset", "wait", "[",
]);

/**
 * Known-LOCAL external binaries: things these scripts invoke that do not
 * touch the network. This list exists to be the OTHER half of the inventory
 * check — a command lands here or in NETWORK_COMMANDS, and anything in
 * neither is a decision someone has to make (#2666).
 *
 * Adding a name here is an assertion that the binary performs no fetch. That
 * assertion is human judgement, not something this file can verify: `pnpm
 * --version` looks exactly this local and downloads a tarball (#2642). Only
 * add a name after checking what it does on a cache miss.
 */
export const LOCAL_COMMANDS = new Set([
  "awk", "basename", "cat", "chmod", "cp", "cut", "date", "dirname", "du", "env", "expr", "find",
  "grep", "head", "install", "ln", "ls", "mkdir", "mktemp", "mv", "node", "rm", "rmdir", "sed",
  "sh", "sleep", "sort", "tail", "tar", "tee", "touch", "tr", "uname", "uniq", "wc", "which",
  "xargs",
  // The bound itself. GNU coreutils `timeout` (macOS Homebrew: `gtimeout`) —
  // spawns a child and starts a clock; no network of its own.
  "timeout", "gtimeout",
  // binaryen's optimizer. setup-dev.sh installs it from a tarball (that FETCH
  // is bounded, and is a `curl`); running the resulting binary is local.
  "wasm-opt",
  // bash invoking bash. Whatever it runs is scanned as its own text when it
  // lives in a scanned script; a `bash -c "…"` STRING is not (see the header).
  "bash",
  // The book renderer, reached once the justfile became a scanned source
  // (#2677). `mdbook build` renders local markdown and `mdbook test` shells
  // out to the rustdoc already on PATH; neither resolves a dependency graph.
  // The fetch-capable neighbours in the same recipes — wasm-pack, pnpm — are
  // allowlisted separately and bounded.
  "mdbook",
  // The benchmark harness's three subjects, reached once discovery went
  // repo-wide (#2677). Each is an already-installed binary being RUN over
  // local files: `binkplayer` and `inklecate` play/compile an ink story,
  // `hyperfine` times a command. Their INSTALLS — `cargo install binkplayer`,
  // `brew install hyperfine` — are the fetches, and those are matched by
  // NETWORK_COMMANDS above and bounded in benchmarks/setup.sh.
  "binkplayer", "inklecate", "hyperfine",
]);

/**
 * Commands that WRAP another command, whose real head is therefore further
 * along the segment. The number is how many tokens to drop after the wrapper
 * itself before the wrapped command begins.
 *
 * Without this the inventory would see only `run_with_timeout` for every
 * bounded fetch in setup-dev.sh and never learn that `corepack`, `cargo` or
 * `curl` are invoked at all — the check would pass while knowing nothing.
 */
/**
 * Keywords whose NEXT word is a command: `if curl …`, `while read …`,
 * `! cargo …`. Without peeling these the inventory saw `if` (a shell word,
 * skipped) and never learned that setup-dev.sh invokes `curl` at all.
 *
 * `for`, `case`, `select` and `in` are deliberately absent: their next word
 * is a loop VARIABLE or a subject, not a command (`for tool in curl pnpm`
 * names tools without running them).
 */
const COMMAND_INTRODUCERS = new Set(["if", "elif", "while", "until", "then", "else", "do", "!", "time"]);

const WRAPPER_COMMANDS = new Map([
  ["run_with_timeout", 1],
  ["command", 0],
  ["exec", 0],
  ["builtin", 0],
]);

/** `NAME=`, `NAME+=`, `NAME[i]=` — an assignment or an env prefix, not a command. */
const ASSIGNMENT_TOKEN = /^[A-Za-z_][A-Za-z0-9_]*(?:\[[^\]]*\])?\+?=/;

/** A head whose name comes from a variable (`"$timeout_bin"`, `"$@"`). */
const INDIRECT_TOKEN = /^["']?\$/;

/** Shell function definitions — `name() {`. Their names become local commands. */
export function findFunctionNames(text) {
  const names = new Set();
  for (const line of text.split("\n")) {
    if (/^\s*#/.test(line)) continue;
    const match = /^\s*(?:function\s+)?([A-Za-z_][A-Za-z0-9_-]*)\s*\(\)\s*\{?/.exec(line);
    if (match) names.add(match[1]);
  }
  return names;
}

/**
 * Quote-aware segment split, shared by CHECK 1 and CHECK 3.
 *
 * Both checks split a logical line into segments on `;`, `|` and `&&`
 * outside quotes and outside `$( … )`. Splitting on one of those inside a
 * string can only ever produce EXTRA segments and therefore extra reports,
 * and over-reporting an unbounded fetch is the safe direction for check 1's
 * hang guard — so quote-awareness there is a refinement, not a requirement.
 * Check 3 is the opposite — an inventory that emits a report per prose word
 * ("committing", "retry", "see") is one nobody reads, so it needs to know
 * where strings are, which is why this splitter exists in the first place.
 *
 * Handles: single and double quotes, backslash escapes, `$( … )` nesting (a
 * `|` inside a substitution does not split the outer line — the substitution
 * is extracted and walked separately), and trailing `#` comments.
 *
 * Still NOT handled: heredoc bodies (scanned as if they were commands, same
 * as check 1) and `${VAR#pattern}` outside quotes with a space before the
 * `#` — the first would need a real parser, the second does not occur.
 *
 * @param {string} line
 * @returns {string[]}
 */
export function splitSegmentsQuoteAware(line) {
  const segments = [];
  let current = "";
  let quote = null;
  let substitutionDepth = 0;

  for (let i = 0; i < line.length; i += 1) {
    const char = line[i];

    if (quote) {
      current += char;
      if (char === "\\" && quote === '"' && i + 1 < line.length) {
        i += 1;
        current += line[i];
      } else if (char === quote) {
        quote = null;
      }
      continue;
    }

    if (char === "\\" && i + 1 < line.length) {
      current += char + line[i + 1];
      i += 1;
      continue;
    }

    if (char === "'" || char === '"') {
      quote = char;
      current += char;
      continue;
    }

    if (char === "$" && line[i + 1] === "(") {
      substitutionDepth += 1;
      current += "$(";
      i += 1;
      continue;
    }

    if (substitutionDepth > 0) {
      if (char === "(") substitutionDepth += 1;
      else if (char === ")") substitutionDepth -= 1;
      current += char;
      continue;
    }

    if (char === "#" && (current === "" || /\s$/.test(current))) break;

    // `;`, `|` and `&&` separate commands. A LONE `&` does not: it is almost
    // always a redirection (`2>&1`, `&>/dev/null`), and splitting there
    // produced a phantom command named `1`.
    if (char === ";" || char === "|" || (char === "&" && line[i + 1] === "&")) {
      segments.push(current);
      current = "";
      while (i + 1 < line.length && [";", "|", "&"].includes(line[i + 1])) i += 1;
      continue;
    }

    current += char;
  }

  segments.push(current);
  return segments;
}

/**
 * Text inside every `$( … )` in a segment.
 *
 * Quote-aware in both directions: a `$(` inside SINGLE quotes is literal text
 * and is not a substitution, while a `$(` inside DOUBLE quotes is one (that
 * is the common shape — `"$(cd … && pwd)"`). `$(( … ))` is arithmetic and is
 * skipped outright: it evaluates numbers, never commands, and treating it as
 * one reported a phantom command named `+`.
 *
 * Deeply nested double quotes (`"$(cd "$(dirname "$x")" && pwd)"`) defeat the
 * depth counter, in which case the interior is taken to the end of the
 * segment. That over-scans rather than under-scans, which is the safe
 * direction for check 1.
 */
function commandSubstitutions(segment) {
  const found = [];
  const n = segment.length;
  let i = 0;
  let single = false;

  while (i < n - 1) {
    const char = segment[i];

    if (single) {
      if (char === "'") single = false;
      i += 1;
      continue;
    }
    if (char === "\\") {
      i += 2;
      continue;
    }
    if (char === "'") {
      single = true;
      i += 1;
      continue;
    }
    if (char !== "$" || segment[i + 1] !== "(") {
      i += 1;
      continue;
    }

    if (segment[i + 2] === "(") {
      let depth = 2;
      let j = i + 3;
      while (j < n && depth > 0) {
        if (segment[j] === "(") depth += 1;
        else if (segment[j] === ")") depth -= 1;
        j += 1;
      }
      i = j;
      continue;
    }

    let depth = 1;
    let j = i + 2;
    let innerSingle = false;
    let innerDouble = false;
    while (j < n && depth > 0) {
      const inner = segment[j];
      if (inner === "\\") {
        j += 2;
        continue;
      }
      if (innerSingle) {
        if (inner === "'") innerSingle = false;
      } else if (innerDouble) {
        if (inner === '"') innerDouble = false;
      } else if (inner === "'") innerSingle = true;
      else if (inner === '"') innerDouble = true;
      else if (inner === "(") depth += 1;
      else if (inner === ")") depth -= 1;
      j += 1;
    }

    found.push(segment.slice(i + 2, depth === 0 ? j - 1 : n));
    i = j;
  }

  return found;
}

/**
 * The head command of a segment, after peeling wrapper tokens, grouping
 * punctuation, env-prefix assignments and command wrappers. Returns "" when
 * the segment invokes nothing (a bare assignment, a function definition, an
 * `esac`).
 *
 * @param {string} segment
 */
export function commandHead(segment) {
  let rest = segment.trim();

  for (;;) {
    rest = rest.replace(/^[(){}!\s]+/, "");
    const match = /^(\S+)(\s+|$)/.exec(rest);
    if (!match) return "";

    const token = match[1].replace(/^["']|["']$/g, "");

    if (ASSIGNMENT_TOKEN.test(token)) {
      // `FOO=bar cmd` peels; a bare `FOO=bar` (nothing follows) invokes nothing.
      if (match[2] === "") return "";
      // An UNBALANCED `(` means the value continues past this whitespace —
      // an array literal (`dirs=(a b c)`), an arithmetic assignment
      // (`n=$((n + 1))`) or a substitution (`V="$(node -p "…")"`). The next
      // token is part of the VALUE, not a command, so peeling would invent
      // one (`benchmarks/tools/gen-input`, `+`, `-p` were all real reports of
      // this shape). Any command inside a substitution is recovered
      // separately by `commandSubstitutions`.
      const opens = (token.match(/\(/g) ?? []).length;
      const closes = (token.match(/\)/g) ?? []).length;
      if (opens > closes) return "";
      // An UNCLOSED QUOTE means the same thing for the same reason: the value
      // continues past this whitespace. `pkgs="-p brink-runtime -p …"` in the
      // justfile's `book-test` recipe otherwise peeled to a command named
      // `brink-runtime` (#2677). Counted on the RAW token — `token` has had a
      // leading/trailing quote stripped, which would make the balanced
      // `FOO="bar" cmd` look unbalanced and hide `cmd`.
      const quotes = (match[1].match(/"/g) ?? []).length;
      const ticks = (match[1].match(/'/g) ?? []).length;
      if (quotes % 2 === 1 || ticks % 2 === 1) return "";
      rest = rest.slice(match[0].length);
      continue;
    }

    if (COMMAND_INTRODUCERS.has(token)) {
      rest = rest.slice(match[0].length);
      continue;
    }

    if (/^[A-Za-z_][A-Za-z0-9_-]*\(\)$/.test(token)) return ""; // function definition

    // A `case` PATTERN label — `Linux/x86_64)`, `*)`, `-d)`. The `)` closes
    // nothing this token opened, so it is a label, not a command; drop it and
    // keep peeling so `*) exit 0 ;;` still reports `exit`.
    if (token.endsWith(")") && !token.includes("(")) {
      rest = rest.slice(match[0].length);
      continue;
    }

    if (WRAPPER_COMMANDS.has(token)) {
      rest = rest.slice(match[0].length);
      for (let skip = WRAPPER_COMMANDS.get(token); skip > 0; skip -= 1) {
        rest = rest.replace(/^\S+\s*/, "");
      }
      // `command -v foo` / `command -p foo`: the flags belong to the wrapper,
      // the command being named is what we want to classify.
      while (/^-{1,2}[A-Za-z]/.test(rest)) rest = rest.replace(/^\S+\s*/, "");
      continue;
    }

    return token;
  }
}

/**
 * Check 3 (#2666): every command these scripts invoke is CLASSIFIED — either
 * a known network command (whose boundedness check 1 then enforces) or an
 * explicitly known-local one. A command in neither list is reported, so a
 * brand-new fetching binary becomes an explicit decision instead of staying
 * invisible until someone notices it in review.
 *
 * This is the "allowlist is the ceiling" hole named in this file's own header
 * from the day it landed (#2648), raised one level: check 1 asks "is this
 * wrapped", check 3 asks "do we even know this command exists".
 *
 * @param {string} text
 * @param {string} path
 * @param {Set<string>} [knownFunctions] function names defined in ANY scanned
 *   script — `run_with_timeout` is defined in scripts/lib/run-with-timeout.sh
 *   and called from two others.
 * @returns {{ok: boolean, problems: string[], heads: {line: number, head: string}[]}}
 */
export function findUnclassifiedCommands(text, path, knownFunctions = new Set()) {
  const network = networkBinaries();
  const functions = new Set([...knownFunctions, ...findFunctionNames(text)]);
  const heads = [];
  const unclassified = new Map();

  for (const logical of toLogicalLines(text)) {
    for (const segment of splitSegmentsQuoteAware(logical.text)) {
      // A substitution's interior is itself a command line, so it is split
      // again rather than scanned whole.
      const pieces = [segment, ...commandSubstitutions(segment).flatMap(splitSegmentsQuoteAware)];

      for (const piece of pieces) {
        const head = commandHead(piece);
        if (head === "") continue;
        if (INDIRECT_TOKEN.test(head)) continue; // variable-dispatched; see the header
        if (SHELL_WORDS.has(head)) continue;

        heads.push({ line: logical.line, head });

        if (functions.has(head)) continue;
        if (network.has(head)) continue;
        if (LOCAL_COMMANDS.has(head)) continue;
        if (!unclassified.has(head)) unclassified.set(head, logical.line);
      }
    }
  }

  const problems = [...unclassified.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(
      ([head, line]) =>
        `${path}:${line} invokes \`${head}\`, which is in neither NETWORK_COMMANDS nor LOCAL_COMMANDS in ` +
        `scripts/check-scripts.mjs. Classify it (#2666): if it can touch the network — including on a cache ` +
        `miss, the way \`pnpm --version\` does — add it to NETWORK_COMMANDS so its boundedness is checked; if it ` +
        `is genuinely local, add it to LOCAL_COMMANDS. Leaving it unlisted is how a new fetching binary stays ` +
        `invisible.`,
    );

  return { ok: problems.length === 0, problems, heads };
}

/**
 * The scripts that carry a header knob table, each with the env-var PREFIX
 * that table governs (#2678).
 *
 * This registry is the #2678 fix: `checkKnobTable`/`findKnobAssignments` were
 * hardwired to `BRINK_SETUP_`, so when #2671 gave
 * refresh-excluded-lockfiles.sh a `BRINK_REFRESH_*` table it was checked by
 * nothing — the same silent drift #2647 was filed about, one script over.
 *
 * A registry is itself a hand-maintained enumeration, which is the failure
 * this whole file exists to end (#2591 → #2638 → #2642 → #2667). So it is not
 * trusted on its own: `findUnregisteredKnobTables` below DISCOVERS
 * `BRINK_*_TIMEOUT` assignments across every scanned source and reports any
 * script holding one that is not registered here.
 */
export const KNOB_TABLES = [
  { path: SETUP_DEV_PATH, prefix: "BRINK_SETUP_" },
  { path: REFRESH_LOCKFILES_PATH, prefix: "BRINK_REFRESH_" },
  { path: JUSTFILE_PATH, prefix: "BRINK_JUST_" },
  { path: BENCHMARKS_SETUP_PATH, prefix: "BRINK_BENCH_" },
];

/** The shape every knob in every registered table shares. */
export const KNOB_SUFFIX = "_TIMEOUT";

/** Any `BRINK_<something>_TIMEOUT`, whatever the prefix — the discovery net. */
const ANY_KNOB = /\bBRINK_[A-Z0-9_]*_TIMEOUT\b/;

/** Escape a literal prefix for embedding in a RegExp. */
function knobNamePattern(prefix) {
  return `${prefix.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}[A-Z0-9_]*${KNOB_SUFFIX}`;
}

/**
 * Parse a header knob table. A row is `#  <PREFIX>X_TIMEOUT  <n>s  <outcome>`;
 * further-indented comment lines continue the previous row's outcome cell. The
 * table ends at the first blank comment line after the rows begin.
 *
 * @param {string} text
 * @param {string} [prefix] env-var prefix this table governs (#2678)
 * @returns {{name: string, default: number, outcome: string, line: number}[]}
 */
export function parseKnobTable(text, prefix = "BRINK_SETUP_") {
  const rowPattern = new RegExp(`^#\\s{2,}(${knobNamePattern(prefix)})\\s+(\\d+)s\\s+(.*)$`);
  const rows = [];
  let started = false;

  for (const [index, line] of text.split("\n").entries()) {
    if (!/^#/.test(line)) {
      if (started) break;
      continue;
    }

    if (started && /^#\s*$/.test(line)) break;

    const row = rowPattern.exec(line);
    if (row) {
      started = true;
      rows.push({ name: row[1], default: Number(row[2]), outcome: row[3].trim(), line: index + 1 });
      continue;
    }

    if (started && rows.length > 0) {
      const continuation = /^#\s{2,}(\S.*)$/.exec(line);
      if (continuation) rows[rows.length - 1].outcome += ` ${continuation[1].trim()}`;
    }
  }

  return rows;
}

/**
 * Every `[export] <PREFIX>*_TIMEOUT=["]${SAME:-<n>}["]` assignment in the
 * script body — quoted or unquoted RHS, with or without a leading `export`.
 *
 * @param {string} text
 * @param {string} [prefix]
 * @returns {{name: string, default: number, line: number, selfReferential: boolean}[]}
 */
export function findKnobAssignments(text, prefix = "BRINK_SETUP_") {
  const pattern = new RegExp(
    `^\\s*(?:export\\s+)?(${knobNamePattern(prefix)})=("?)\\$\\{([A-Z0-9_]+):-(\\d+)\\}\\2`,
  );
  const found = [];

  for (const [index, line] of text.split("\n").entries()) {
    if (/^\s*#/.test(line)) continue;
    const match = pattern.exec(line);
    if (!match) continue;
    found.push({
      name: match[1],
      default: Number(match[4]),
      line: index + 1,
      selfReferential: match[1] === match[3],
    });
  }

  return found;
}

/**
 * The #2678 fix's own backstop, and the reason the registry above is not the
 * ceiling: any scanned source that assigns SOME `BRINK_*_TIMEOUT` knob while
 * having no `KNOB_TABLES` entry whose prefix covers it.
 *
 * #2678 happened because a knob table landed in a script the checker did not
 * know about. Registering the two known scripts fixes that instance; this
 * function is what makes the THIRD instance loud instead of silent.
 *
 * @param {{path: string, text: string}[]} sources
 * @param {{path: string, prefix: string}[]} [registry]
 * @returns {{path: string, name: string, line: number}[]}
 */
export function findUnregisteredKnobTables(sources, registry = KNOB_TABLES) {
  const found = [];

  for (const source of sources) {
    const prefixes = registry.filter((entry) => entry.path === source.path).map((entry) => entry.prefix);

    for (const [index, line] of source.text.split("\n").entries()) {
      if (/^\s*#/.test(line)) continue;
      const match = new RegExp(`^\\s*(?:export\\s+)?(${ANY_KNOB.source})=`).exec(line);
      if (!match) continue;
      if (prefixes.some((prefix) => match[1].startsWith(prefix))) continue;
      found.push({ path: source.path, name: match[1], line: index + 1 });
    }
  }

  return found;
}

/**
 * Backstop for the assignment shapes `findKnobAssignments` above does not
 * parse — e.g. the colon-default idiom `: "${NAME:=30}"`, which assigns via
 * a parameter-expansion side effect with no literal `NAME=` on the line at
 * all. Rather than growing `findKnobAssignments`'s regex to cover every bash
 * assignment shape (open-ended, and this file is a lexical scanner, not a
 * shell parser — see "WHAT CHECK 1 CANNOT SEE"), this sweeps the non-comment
 * body for any `BRINK_SETUP_*_TIMEOUT` identifier written in assignment
 * position that `findKnobAssignments` did NOT already recognise on that same
 * line, and reports it as an unparsed shape rather than silently skipping it
 * — a knob assigned only this way was previously invisible to every
 * `checkKnobTable` cross-check (no missing-row check, no default-drift
 * check, no fail-vs-warn check), which is exactly the silent drift #2647
 * exists to stop.
 *
 * @param {string} text
 * @param {string} [prefix]
 * @returns {{name: string, line: number}[]}
 */
export function findUnrecognizedKnobShapes(text, prefix = "BRINK_SETUP_") {
  const name = knobNamePattern(prefix);
  const asAssignmentPattern = new RegExp(`(?:^|[\\s:])(${name})=(?!=)`);
  const asColonDefaultPattern = new RegExp(`\\$\\{(${name}):=`);
  const recognizedLines = new Set(findKnobAssignments(text, prefix).map((assignment) => assignment.line));
  const found = [];

  for (const [index, line] of text.split("\n").entries()) {
    if (/^\s*#/.test(line)) continue;
    const lineNumber = index + 1;
    if (recognizedLines.has(lineNumber)) continue;

    // A literal `NAME=` (the common assignment shape), or the colon-default
    // idiom `${NAME:=...}` (assigns via side effect, no literal `NAME=`).
    const match = asAssignmentPattern.exec(line) ?? asColonDefaultPattern.exec(line);
    if (!match) continue;

    found.push({ name: match[1], line: lineNumber });
  }

  return found;
}

/**
 * Check 2a: a script's header knob table agrees with that script, in both
 * directions. Parameterised by `(path, prefix)` since #2678 — see
 * `KNOB_TABLES` for who is registered and why a registry alone is not trusted.
 *
 * @param {string} text
 * @param {string | {path?: string, prefix?: string}} [options] a bare string is
 *   read as the path, keeping the pre-#2678 call shape
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkKnobTable(text, options = {}) {
  const { path = SETUP_DEV_PATH, prefix = "BRINK_SETUP_" } =
    typeof options === "string" ? { path: options } : options;

  const problems = [];
  const rows = parseKnobTable(text, prefix);
  const assignments = findKnobAssignments(text, prefix);

  if (rows.length === 0) {
    problems.push(
      `${path} is registered in KNOB_TABLES as carrying a ${prefix}* knob table, but no parseable table row was ` +
        `found in its header comment block. A row is \`#  ${prefix}X${KNOB_SUFFIX}  <n>s  <FAIL|WARN …>\` ` +
        `(#2647/#2678).`,
    );
    return { ok: false, problems };
  }

  const byName = new Map(rows.map((row) => [row.name, row]));

  for (const assignment of assignments) {
    if (!assignment.selfReferential) {
      problems.push(
        `${path}:${assignment.line} assigns ${assignment.name} from a DIFFERENT variable — the knob a caller ` +
          `sets is then not the knob the script reads.`,
      );
    }

    const row = byName.get(assignment.name);
    if (!row) {
      problems.push(
        `${path}:${assignment.line} assigns ${assignment.name} (default ${assignment.default}s) but the header ` +
          `knob table has no row for it. That table is what a reader consults after a step aborts naming an env ` +
          `var they have never seen (#2647) — add the row, including its fail-vs-warn outcome.`,
      );
      continue;
    }

    if (row.default !== assignment.default) {
      problems.push(
        `${path}: header table row for ${assignment.name} (line ${row.line}) says ${row.default}s, but the ` +
          `assignment at line ${assignment.line} defaults to ${assignment.default}s (#2647).`,
      );
    }
  }

  const assignedNames = new Set(assignments.map((assignment) => assignment.name));
  const body = text
    .split("\n")
    .filter((line) => !/^\s*#/.test(line))
    .join("\n");

  for (const row of rows) {
    if (!assignedNames.has(row.name)) {
      problems.push(
        `${path}: header table row at line ${row.line} names ${row.name}, which the script never assigns — a ` +
          `stale row for a removed knob misleads exactly as badly as a missing one (#2647).`,
      );
      continue;
    }

    if (!body.includes(`\${${row.name}}`)) {
      problems.push(
        `${path}: ${row.name} has a header table row (line ${row.line}) and an assignment, but the script never ` +
          `reads \${${row.name}} — the knob is documented but inert (#2647).`,
      );
    }

    if (!/\bFAIL\b|\bWARN\b/.test(row.outcome)) {
      problems.push(
        `${path}: header table row for ${row.name} (line ${row.line}) does not say FAIL or WARN in its ` +
          `"On timeout" cell. That column is the one an agent consults after setup aborts naming an env var ` +
          `it has never seen (#2647).`,
      );
    }
  }

  for (const shape of findUnrecognizedKnobShapes(text, prefix)) {
    problems.push(
      `${path}:${shape.line} assigns ${shape.name} in a shape this check cannot parse (not a bare, possibly ` +
        `\`export\`-ed \`NAME="\${NAME:-N}"\` assignment, nor a recognised \`\${NAME:=N}\` colon-default) — it is ` +
        `invisible to every check above, which is the silent drift #2647 exists to stop. Rewrite it in the ` +
        `recognised form.`,
    );
  }

  return { ok: problems.length === 0, problems };
}

/** Slice a markdown section out by heading text, to the next heading of the same level. */
export function sliceSection(markdown, headingText) {
  const lines = markdown.split("\n");
  const startIndex = lines.findIndex((line) => /^#{1,6}\s/.test(line) && line.includes(headingText));
  if (startIndex === -1) return null;

  const level = (/^(#{1,6})\s/.exec(lines[startIndex]) ?? ["", "#"])[1].length;
  let endIndex = lines.length;

  for (let i = startIndex + 1; i < lines.length; i += 1) {
    const heading = /^(#{1,6})\s/.exec(lines[i]);
    if (heading && heading[1].length <= level) {
      endIndex = i;
      break;
    }
  }

  return lines.slice(startIndex, endIndex).join("\n");
}

/** How far from a `setup-dev.sh` mention the delegating phrases may sit. */
export const POINTER_WINDOW = 400;

/**
 * Check 2b: the three documents #2640 pointed at the header table still do.
 *
 * A pointer counts when a `setup-dev.sh` mention has both "header block" and
 * "fail-vs-warn" within POINTER_WINDOW characters either side. Runs of
 * whitespace are collapsed first, so a phrase broken across a hard-wrapped
 * markdown line still matches (docs/releasing.md wraps between "header" and
 * "block"). That is a prose-shape assertion, not a semantic one:
 * reworded-but-equivalent prose can fail it, and prose that says the words
 * while pointing somewhere useless can pass it.
 *
 * @param {{path: string, text: string, section?: string}[]} docs
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkDocPointers(docs) {
  const problems = [];

  for (const doc of docs) {
    let text = doc.text;

    if (doc.section) {
      const section = sliceSection(text, doc.section);
      if (section === null) {
        problems.push(
          `${doc.path} no longer has a "${doc.section}" section — that section is where #2640 put the pointer ` +
            `to ${SETUP_DEV_PATH}'s knob table (#2647).`,
        );
        continue;
      }
      text = section;
    }

    text = text.replace(/\s+/g, " ");

    let pointed = false;
    for (let index = text.indexOf("setup-dev.sh"); index !== -1; index = text.indexOf("setup-dev.sh", index + 1)) {
      const window = text.slice(Math.max(0, index - POINTER_WINDOW), index + POINTER_WINDOW);
      if (window.includes("header block") && window.includes("fail-vs-warn")) {
        pointed = true;
        break;
      }
    }

    if (!pointed) {
      problems.push(
        `${doc.path}${doc.section ? ` ("${doc.section}")` : ""} no longer points at ${SETUP_DEV_PATH}'s header ` +
          `block as the authoritative knob/default/fail-vs-warn table. That pointer is the entire delivery ` +
          `mechanism for #2640 — restate it rather than re-listing the knobs here (#2647).`,
      );
    }
  }

  return { ok: problems.length === 0, problems };
}

/**
 * Run every check against the real repo.
 *
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkScripts({ repoRoot = REPO_ROOT } = {}) {
  // Checks 1 and 3 apply to EVERY discovered shell source — every
  // scripts/**/*.sh (#2667) plus the justfile's recipe bodies (#2677).
  // Check 2a applies to each script REGISTERED in KNOB_TABLES with its own
  // prefix (#2678), backstopped by a discovery sweep for knob tables in
  // scripts nobody registered. Check 2b is setup-dev.sh's alone: the three
  // delegating documents are about that one script (#2640).
  const scripts = discoverShellSources(repoRoot);

  // Function names are collected across ALL scanned sources before any is
  // classified, because they cross files: `run_with_timeout` is defined in
  // scripts/lib/run-with-timeout.sh and called from two scripts and the
  // justfile.
  const functions = new Set();
  for (const script of scripts) {
    for (const name of findFunctionNames(script.text)) functions.add(name);
  }

  const byPath = new Map(scripts.map((script) => [script.path, script]));
  const docs = POINTER_DOCS.map((doc) => ({
    ...doc,
    text: readFileSync(join(repoRoot, doc.path), "utf8"),
  }));

  const problems = [];

  if (!byPath.has(SETUP_DEV_PATH)) {
    problems.push(
      `${SETUP_DEV_PATH} was not found under ${SCRIPTS_DIR}/. CLAUDE.md, docs/desktop-shell-spec.md and ` +
        `docs/releasing.md all delegate to its header table (#2640/#2647).`,
    );
  }

  for (const script of scripts) {
    problems.push(...findUnboundedFetches(script.text, script.path).problems);
    problems.push(...findUnclassifiedCommands(script.text, script.path, functions).problems);
  }

  for (const entry of KNOB_TABLES) {
    const script = byPath.get(entry.path);
    if (!script) {
      problems.push(
        `${entry.path} is registered in KNOB_TABLES (prefix ${entry.prefix}) but is not among the scanned ` +
          `sources, so its knob table is checked by nothing — the #2678 gap, re-opened (#2678).`,
      );
      continue;
    }
    problems.push(...checkKnobTable(script.text, entry).problems);
  }

  for (const stray of findUnregisteredKnobTables(scripts)) {
    problems.push(
      `${stray.path}:${stray.line} assigns ${stray.name}, a timeout knob no KNOB_TABLES entry for this file ` +
        `covers, so its header table (if any) is cross-checked by nothing. That is exactly how ` +
        `${REFRESH_LOCKFILES_PATH}'s BRINK_REFRESH_* table went unchecked (#2678) — register the file and its ` +
        `prefix in scripts/check-scripts.mjs.`,
    );
  }

  if (byPath.has(SETUP_DEV_PATH)) {
    problems.push(...checkDocPointers(docs).problems);
  }

  return { ok: problems.length === 0, problems, scripts: scripts.map((script) => script.path) };
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkScripts();
  if (result.ok) {
    console.log(
      `ok - across ${result.scripts.length} shell source(s) (${result.scripts.join(", ")}): every allowlisted ` +
        `network command is wrapped in run_with_timeout or carries an allow-unbounded waiver, and every command ` +
        `invoked is classified as network or local. The ${KNOB_TABLES.length} registered knob tables ` +
        `(${KNOB_TABLES.map((entry) => `${entry.path}:${entry.prefix}*`).join(", ")}) each match their own ` +
        `script, no unregistered script assigns a BRINK_*_TIMEOUT, and ${SETUP_DEV_PATH}'s three delegating ` +
        `docs still point at it. ` +
        `(Read this file's header for what these scans CANNOT see — workflow run: blocks, packages/*/scripts, ` +
        `heredocs and *.test.sh are NOT scanned, and the header says why for each.)`,
    );
  } else {
    console.error(`shell-source checks FAILED (#2648/#2647/#2666/#2667/#2677/#2678):`);
    for (const problem of result.problems) console.error(`  - ${problem}`);
    process.exitCode = 1;
  }
}
