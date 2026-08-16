// Three mechanical checks over the shell scripts in scripts/ (#2648, #2647,
// #2666, #2667).
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
// `cargo update` calls in it. So the SET OF FILES is no longer enumerated
// either — `discoverShellScripts` reads scripts/ (see its own note for what
// it excludes and why) — and check 3 below closes the "is this command even
// known" level that #2666 named.
//
// ─────────────────────────────────────────────────────────────────────────────
// WHAT IS CHECKED WHERE
//
//   Check 1  findUnboundedFetches      every discovered script
//   Check 2  checkKnobTable/DocPointers  setup-dev.sh ONLY (its header table
//                                        and the three docs delegating to it)
//   Check 3  findUnclassifiedCommands  every discovered script
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 1 (`findUnboundedFetches`) DOES
//
// It is a LEXICAL scan of a script's text. Nothing more:
//
//   a. Physical lines are joined into logical lines across a trailing `\`,
//      `|`, `||` or `&&`.
//   b. Whole-line comments (`^\s*#`) are dropped.
//   c. Each logical line is split into segments by `splitSegmentsQuoteAware`
//      on `;`, `|` and `&&` OUTSIDE quotes and outside `$( … )`, with a
//      trailing `#` comment dropped.
//   d. `command -v foo` / `-V` / `-p` is blanked: it is a PATH lookup, not an
//      invocation, so it can never fetch.
//   e. A segment whose first word is a loop/case keyword (`for`, `while`,
//      `until`, `case`) is always skipped. A segment whose first word is an
//      output/read builtin (`echo`, `printf`, `read`) is skipped UNLESS it
//      contains a command substitution — and then only the SUBSTITUTION
//      INTERIORS are scanned, not the prose, because `echo "==> wasm-pack
//      already installed ($(wasm-pack --version))"` runs one command and
//      merely prints the other name. A backslash-escaped backtick
//      (`echo "\`literal\`"`) is displayed text, not a fetch; an UNESCAPED
//      backtick makes the whole segment be scanned, since backtick nesting
//      is not parsed here and over-reporting is the safe direction.
//   f. Each remaining piece is matched against NETWORK_COMMANDS — a small
//      hand-maintained ALLOWLIST of command shapes known to touch the network.
//   g. A matched segment must also contain the literal `run_with_timeout`.
//      If it does not, it is reported.
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 1 CANNOT SEE — read this before trusting it
//
// This check reduces the odds of a fifth miss. It does not eliminate them,
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
//   - HEREDOCS ARE NOT PARSED. A fetch written inside a heredoc body is
//     scanned as if it were a command line (over-report), and — the reason
//     `*.test.sh` is excluded from discovery — the stub binaries the test
//     harnesses plant inside heredocs would each read as an unbounded fetch.
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
// CHECK 2 (`checkKnobTable`, `checkDocPointers`) — #2647
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
//     useless can pass it.
//
// ─────────────────────────────────────────────────────────────────────────────
// CHECK 3 (`findUnclassifiedCommands`) — #2666
//
// Check 1 asks "is this network command bounded". Check 3 asks the question
// one level up: "do we even know this command exists". It extracts the HEAD
// of every command in every discovered script and asserts each is either a
// shell word, a function defined in one of those scripts, a NETWORK_COMMANDS
// binary, or an explicitly-listed LOCAL_COMMANDS binary. Anything else is
// reported with a prompt to classify it, so a brand-new fetching binary
// becomes a decision someone has to make rather than something invisible
// until the next round's review notices it.
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
// Exported as pure functions over text so scripts/check-setup-dev.test.mjs can
// drive them with synthetic inputs (a deleted table row, an unbounded fetch);
// the CLI at the bottom applies them to the real repo files. Node builtins
// only: this runs under `pnpm test:scripts`, which CI's `frontend` job executes
// BEFORE `pnpm install`.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");
export const SETUP_DEV_PATH = "scripts/setup-dev.sh";

/** Where `discoverShellScripts` looks. */
export const SCRIPTS_DIR = "scripts";

/**
 * Every shell script under scripts/, DISCOVERED rather than enumerated
 * (#2667). A hardcoded list of "the scripts we check" would be the same
 * hand-maintained-enumeration failure this file exists to end, just moved one
 * level up — which is precisely how #2667 happened: check 1 shipped in #2656
 * with a single hardcoded `SETUP_DEV_PATH`, and a sibling script with two
 * unbounded `cargo update` calls sat outside the scan from day one.
 *
 * `*.test.sh` is EXCLUDED, deliberately and with a cost: the test harnesses
 * plant stub binaries inside heredocs (`cat > "${dir}/curl" <<'EOF' … EOF`),
 * and this scanner does not parse heredocs — every stub body would read as an
 * unbounded fetch. So a genuine unbounded fetch written in a `*.test.sh` is
 * NOT seen by this check. Those files run under `pnpm test:setup-dev` /
 * `pnpm test:scripts`, where a hang shows up as a hung CI step rather than
 * silently at a developer's first session start, which is why the trade is
 * acceptable here and not in the scripts themselves.
 *
 * @param {string} [repoRoot]
 * @returns {string[]} repo-relative paths, sorted (determinism)
 */
export function discoverShellScripts(repoRoot = REPO_ROOT) {
  const root = join(repoRoot, SCRIPTS_DIR);
  const found = [];

  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const child = join(dir, entry.name);
      const relative = `${prefix}/${entry.name}`;
      if (entry.isDirectory()) {
        walk(child, relative);
        continue;
      }
      if (!entry.name.endsWith(".sh")) continue;
      if (entry.name.endsWith(".test.sh")) continue;
      found.push(relative);
    }
  };

  walk(root, SCRIPTS_DIR);
  return found.sort();
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
const COMMAND_SUBSTITUTION = /\$\(|(?<!\\)`/;

/** Leading tokens that wrap a command without being one. */
const WRAPPER_HEADS = new Set(["if", "elif", "then", "else", "do", "!", "{", "(", "[", "[[", "&&", "||"]);

/**
 * Join physical lines into logical ones across a trailing `\`, `|`, `||` or
 * `&&`. A comment line never continues, and never continues a join.
 *
 * @param {string} text
 * @returns {{line: number, text: string}[]} 1-indexed start line per logical line
 */
export function toLogicalLines(text) {
  const physical = text.split("\n");
  const logical = [];

  let buffer = null;

  for (let i = 0; i < physical.length; i += 1) {
    const raw = physical[i];
    const isComment = /^\s*#/.test(raw);

    if (!buffer && raw.trim().length === 0) continue;

    if (isComment) {
      // Terminate any open join rather than absorbing a comment into it.
      if (buffer) {
        logical.push(buffer);
        buffer = null;
      }
      continue;
    }

    const trimmedEnd = raw.replace(/\s+$/, "");
    const continues = /\\$/.test(trimmedEnd) || /(?:\|\||&&|\||&)$/.test(trimmedEnd);
    const piece = trimmedEnd.replace(/\\$/, "");

    if (buffer) {
      buffer.text += ` ${piece.trim()}`;
    } else {
      buffer = { line: i + 1, text: piece };
    }

    if (!continues) {
      logical.push(buffer);
      buffer = null;
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
 * Check 1: every allowlisted network command is lexically wrapped in
 * `run_with_timeout`.
 *
 * @param {string} setupDevText
 * @param {string} [path] label used in problem messages
 * @returns {{ok: boolean, problems: string[], findings: {line: number, id: string, segment: string, why: string}[]}}
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

  const problems = findings.map(
    (finding) =>
      `${path}:${finding.line} runs a network command (${finding.id} — ${finding.why}) that is NOT wrapped in ` +
      `run_with_timeout, so a wedged proxy hangs setup with no diagnostic: ${finding.segment}`,
  );

  return { ok: problems.length === 0, problems, findings };
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
        `scripts/check-setup-dev.mjs. Classify it (#2666): if it can touch the network — including on a cache ` +
        `miss, the way \`pnpm --version\` does — add it to NETWORK_COMMANDS so its boundedness is checked; if it ` +
        `is genuinely local, add it to LOCAL_COMMANDS. Leaving it unlisted is how a new fetching binary stays ` +
        `invisible.`,
    );

  return { ok: problems.length === 0, problems, heads };
}

/**
 * Parse the header knob table. A row is `#  BRINK_SETUP_X_TIMEOUT  <n>s  <outcome>`;
 * further-indented comment lines continue the previous row's outcome cell. The
 * table ends at the first blank comment line after the rows begin.
 *
 * @param {string} setupDevText
 * @returns {{name: string, default: number, outcome: string, line: number}[]}
 */
export function parseKnobTable(setupDevText) {
  const rows = [];
  let started = false;

  for (const [index, line] of setupDevText.split("\n").entries()) {
    if (!/^#/.test(line)) {
      if (started) break;
      continue;
    }

    if (started && /^#\s*$/.test(line)) break;

    const row = /^#\s{2,}(BRINK_SETUP_[A-Z0-9_]*_TIMEOUT)\s+(\d+)s\s+(.*)$/.exec(line);
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
 * Every `[export] BRINK_SETUP_*_TIMEOUT=["]${SAME:-<n>}["]` assignment in the
 * script body — quoted or unquoted RHS, with or without a leading `export`.
 *
 * @param {string} setupDevText
 * @returns {{name: string, default: number, line: number, selfReferential: boolean}[]}
 */
export function findKnobAssignments(setupDevText) {
  const found = [];

  for (const [index, line] of setupDevText.split("\n").entries()) {
    if (/^\s*#/.test(line)) continue;
    const match =
      /^\s*(?:export\s+)?(BRINK_SETUP_[A-Z0-9_]*_TIMEOUT)=("?)\$\{([A-Z0-9_]+):-(\d+)\}\2/.exec(line);
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
 * @param {string} setupDevText
 * @returns {{name: string, line: number}[]}
 */
export function findUnrecognizedKnobShapes(setupDevText) {
  const recognizedLines = new Set(findKnobAssignments(setupDevText).map((assignment) => assignment.line));
  const found = [];

  for (const [index, line] of setupDevText.split("\n").entries()) {
    if (/^\s*#/.test(line)) continue;
    const lineNumber = index + 1;
    if (recognizedLines.has(lineNumber)) continue;

    // A literal `NAME=` (the common assignment shape), or the colon-default
    // idiom `${NAME:=...}` (assigns via side effect, no literal `NAME=`).
    const asAssignment = /(?:^|[\s:])(BRINK_SETUP_[A-Z0-9_]*_TIMEOUT)=(?!=)/.exec(line);
    const asColonDefault = /\$\{(BRINK_SETUP_[A-Z0-9_]*_TIMEOUT):=/.exec(line);
    const match = asAssignment ?? asColonDefault;
    if (!match) continue;

    found.push({ name: match[1], line: lineNumber });
  }

  return found;
}

/**
 * Check 2a: the header knob table agrees with the script, in both directions.
 *
 * @param {string} setupDevText
 * @param {string} [path]
 * @returns {{ok: boolean, problems: string[]}}
 */
export function checkKnobTable(setupDevText, path = SETUP_DEV_PATH) {
  const problems = [];
  const rows = parseKnobTable(setupDevText);
  const assignments = findKnobAssignments(setupDevText);

  if (rows.length === 0) {
    problems.push(
      `${path} has no parseable knob table in its header block, but CLAUDE.md, docs/desktop-shell-spec.md ` +
        `and docs/releasing.md all delegate to it as authoritative (#2640/#2647).`,
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
          `knob table has no row for it. Three documents delegate to that table as authoritative (#2647) — add ` +
          `the row, including its fail-vs-warn outcome.`,
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
  const body = setupDevText
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

  for (const shape of findUnrecognizedKnobShapes(setupDevText)) {
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
export function checkSetupDev({ repoRoot = REPO_ROOT } = {}) {
  // Checks 1 and 3 apply to EVERY shell script under scripts/ (#2667). Checks
  // 2a/2b are setup-dev.sh's alone: the knob table and the three delegating
  // documents are about that one script.
  const scripts = discoverShellScripts(repoRoot).map((path) => ({
    path,
    text: readFileSync(join(repoRoot, path), "utf8"),
  }));

  // Function names are collected across ALL scanned scripts before any is
  // classified, because they cross files: `run_with_timeout` is defined in
  // scripts/lib/run-with-timeout.sh and called from two other scripts.
  const functions = new Set();
  for (const script of scripts) {
    for (const name of findFunctionNames(script.text)) functions.add(name);
  }

  const setupDev = scripts.find((script) => script.path === SETUP_DEV_PATH);
  const docs = POINTER_DOCS.map((doc) => ({
    ...doc,
    text: readFileSync(join(repoRoot, doc.path), "utf8"),
  }));

  const problems = [];

  if (!setupDev) {
    problems.push(
      `${SETUP_DEV_PATH} was not found under ${SCRIPTS_DIR}/. CLAUDE.md, docs/desktop-shell-spec.md and ` +
        `docs/releasing.md all delegate to its header table (#2640/#2647).`,
    );
  }

  for (const script of scripts) {
    problems.push(...findUnboundedFetches(script.text, script.path).problems);
    problems.push(...findUnclassifiedCommands(script.text, script.path, functions).problems);
  }

  if (setupDev) {
    problems.push(...checkKnobTable(setupDev.text).problems);
    problems.push(...checkDocPointers(docs).problems);
  }

  return { ok: problems.length === 0, problems, scripts: scripts.map((script) => script.path) };
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkSetupDev();
  if (result.ok) {
    console.log(
      `ok - across ${result.scripts.length} shell script(s) in ${SCRIPTS_DIR}/ (${result.scripts.join(", ")}): ` +
        `every allowlisted network command is wrapped in run_with_timeout, and every command invoked is ` +
        `classified as network or local. ${SETUP_DEV_PATH}'s header knob table matches the script, and the ` +
        `three delegating docs still point at it. ` +
        `(Read this file's header for what these scans CANNOT see — heredocs and *.test.sh above all.)`,
    );
  } else {
    console.error(`${SCRIPTS_DIR}/ shell-script checks FAILED (#2648/#2647/#2666/#2667):`);
    for (const problem of result.problems) console.error(`  - ${problem}`);
    process.exitCode = 1;
  }
}
