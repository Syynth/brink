// Two mechanical checks over scripts/setup-dev.sh (#2648, #2647).
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
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 1 (`findUnboundedFetches`) DOES
//
// It is a LEXICAL scan of setup-dev.sh's text. Nothing more:
//
//   a. Physical lines are joined into logical lines across a trailing `\`,
//      `|`, `||` or `&&`.
//   b. Whole-line comments (`^\s*#`) are dropped.
//   c. Each logical line is split into segments on `;`, `|`, `||`, `&&`.
//   d. A segment whose first word is a loop/case keyword (`for`, `while`,
//      `until`, `case`) is always skipped. A segment whose first word is an
//      output/read builtin (`echo`, `printf`, `read`) is skipped UNLESS it
//      contains a command substitution (`$(` or an unescaped backtick) —
//      that substitution runs a real subshell command, so `echo "$(curl …)"`
//      is still scanned even though the outer command only prints. A
//      backslash-escaped backtick (`echo "\`literal\`"`) does not count —
//      that is quoting punctuation around displayed text, not a fetch.
//   e. Each remaining segment is matched against NETWORK_COMMANDS — a small
//      hand-maintained ALLOWLIST of command shapes known to touch the network.
//   f. A matched segment must also contain the literal `run_with_timeout`.
//      If it does not, it is reported.
//
// ─────────────────────────────────────────────────────────────────────────────
// EXACTLY WHAT CHECK 1 CANNOT SEE — read this before trusting it
//
// This check reduces the odds of a fourth miss. It does not eliminate them,
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
//     below does not swallow it. A future tool with the same shape stays
//     invisible until a human adds it to the allowlist.
//   - THE ALLOWLIST IS THE CEILING. A brand-new fetching binary added to
//     setup-dev.sh is not flagged until someone lists it here.
//   - THE `--version` EXEMPTION IS A HOLE BY CONSTRUCTION. When the matched
//     command is immediately followed by `--version` or `-V`, it is treated
//     as a local probe, so `cargo deny --version` in `cargo_deny_ok()` does
//     not have to be bounded. That exemption is precisely the shape of the
//     third miss; it is kept because bounding every version probe is noise,
//     and it is made safe only for the package managers explicitly opted out
//     of it via `probeStillFetches`.
//   - TRAILING COMMENTS ARE NOT STRIPPED. `foo # see curl(1)` reports a curl.
//     (Whole-line comments ARE stripped, which is the common case here.)
//   - QUOTES AND HEREDOCS ARE NOT PARSED. Segment splitting is quote-naive,
//     so a `;` or `|` inside a string splits a segment; a fetch written
//     inside a heredoc body is scanned as if it were a command line.
//   - "BOUNDED" IS LEXICAL. The presence of `run_with_timeout` in the same
//     segment is all that is verified. It does NOT verify the bound is
//     applied to the fetching command, that the timeout value is sane, or
//     that the 124 exit code is handled. Those are behavioural properties;
//     scripts/setup-dev.test.sh drives the real script against stubs for
//     them, and that harness — not this file — is what proves the $?-capture
//     control flow is right.
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
// Exported as pure functions over text so scripts/check-setup-dev.test.mjs can
// drive them with synthetic inputs (a deleted table row, an unbounded fetch);
// the CLI at the bottom applies them to the real repo files. Node builtins
// only: this runs under `pnpm test:scripts`, which CI's `frontend` job executes
// BEFORE `pnpm install`.

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = resolve(here, "..");
export const SETUP_DEV_PATH = "scripts/setup-dev.sh";

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
    id: "cargo-network",
    pattern: /\bcargo\s+(?:install|deny|fetch|update|publish|add)\b/,
    why: "a crates.io index/dependency fetch (cargo deny additionally clones the RUSTSEC advisory DB)",
  },
  {
    id: "git-remote",
    pattern: /\bgit\s+(?:clone|fetch|pull|push|ls-remote|submodule)\b/,
    why: "a remote git operation",
  },
];

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

const SEGMENT_SPLIT = /\|\||&&|[|;]/;

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

  for (const logical of toLogicalLines(setupDevText)) {
    for (const segment of logical.text.split(SEGMENT_SPLIT)) {
      if (segment.trim().length === 0) continue;
      const head = firstWord(segment);
      if (STRUCTURAL_HEADS.has(head)) continue;
      if (OUTPUT_HEADS.has(head) && !COMMAND_SUBSTITUTION.test(segment)) continue;

      for (const command of NETWORK_COMMANDS) {
        const match = command.pattern.exec(segment);
        if (!match) continue;

        const rest = segment.slice(match.index + match[0].length);
        if (!command.probeStillFetches && nextTokenIsVersionFlag(rest)) continue;
        if (segment.includes("run_with_timeout")) continue;

        findings.push({ line: logical.line, id: command.id, segment: segment.trim(), why: command.why });
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
  const setupDevText = readFileSync(join(repoRoot, SETUP_DEV_PATH), "utf8");
  const docs = POINTER_DOCS.map((doc) => ({
    ...doc,
    text: readFileSync(join(repoRoot, doc.path), "utf8"),
  }));

  const problems = [
    ...findUnboundedFetches(setupDevText).problems,
    ...checkKnobTable(setupDevText).problems,
    ...checkDocPointers(docs).problems,
  ];

  return { ok: problems.length === 0, problems };
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));

if (invokedDirectly) {
  const result = checkSetupDev();
  if (result.ok) {
    console.log(
      `ok - every allowlisted network command in ${SETUP_DEV_PATH} is wrapped in run_with_timeout, the header ` +
        `knob table matches the script, and the three delegating docs still point at it. ` +
        `(Read this file's header for what the scan CANNOT see.)`,
    );
  } else {
    console.error(`${SETUP_DEV_PATH} checks FAILED (#2648/#2647):`);
    for (const problem of result.problems) console.error(`  - ${problem}`);
    process.exitCode = 1;
  }
}
