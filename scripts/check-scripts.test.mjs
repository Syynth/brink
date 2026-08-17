// Tests for scripts/check-scripts.mjs (#2648, #2647, #2666, #2667). Node's
// built-in test runner, matching check-pnpm-pin.test.mjs /
// check-wasm-pkg.test.mjs / guarded-install.test.mjs: this file runs under
// `pnpm test:scripts`, which CI's `frontend` job executes BEFORE `pnpm
// install`, so it must not depend on anything installed.
//
// Two halves, the same shape as check-pnpm-pin.test.mjs:
//
//   1. Unit tests over the pure checkers with SYNTHETIC input — a planted
//      unbounded fetch for every allowlisted command shape, a deleted table
//      row, a drifted default, a stale row, a doc that lost its pointer. These
//      are the proofs that each check goes RED rather than merely passing on a
//      healthy tree.
//   2. Integration tests over the REAL repo files, including two mutations of
//      the real scripts/setup-dev.sh text (unwrap a real fetch; delete a real
//      table row) so the checks are proved non-vacuous against the actual
//      file, not just against fixtures shaped to suit them.

import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  BENCHMARKS_SETUP_PATH,
  CHECK_SCRIPTS_NPM_SCRIPT,
  EXEC_CALL_NAMES,
  JUSTFILE_PATH,
  KNOB_TABLES,
  LOCAL_COMMANDS,
  MIN_WAIVER_REASON,
  NETWORK_COMMANDS,
  PACKAGE_JSON_PATH,
  POINTER_DOCS,
  REFRESH_LOCKFILES_PATH,
  REPO_ROOT,
  SETUP_DEV_PATH,
  checkDocPointers,
  checkKnobTable,
  checkPackageScriptPath,
  checkScripts,
  commandHead,
  discoverNodeOrBashScriptNames,
  discoverPackageScriptSources,
  discoverShellScripts,
  discoverShellSources,
  extractBalancedArgs,
  findFunctionNames,
  findKnobAssignments,
  findUnboundedExecCalls,
  findUnboundedFetches,
  findUnclassifiedCommands,
  findUnregisteredKnobTables,
  findUnrecognizedKnobShapes,
  findWaivers,
  justfileShellView,
  networkBinaries,
  nextTokenIsVersionFlag,
  parseKnobTable,
  sliceSection,
  splitSegmentsQuoteAware,
  stripJsComments,
  toLogicalLines,
} from "./check-scripts.mjs";

const realSetupDev = readFileSync(join(REPO_ROOT, SETUP_DEV_PATH), "utf8");
const realRefreshLockfiles = readFileSync(join(REPO_ROOT, REFRESH_LOCKFILES_PATH), "utf8");
const realJustfile = readFileSync(join(REPO_ROOT, JUSTFILE_PATH), "utf8");
const realBenchmarksSetup = readFileSync(join(REPO_ROOT, BENCHMARKS_SETUP_PATH), "utf8");

/** Strip every `run_with_timeout` wrapper out of real script text. */
function stripBounds(text) {
  return text
    .split("\n")
    .map((line) =>
      /^\s*#/.test(line)
        ? line
        : line.replace(/run_with_timeout\s+"[^"]*"\s*/g, "").replace(/\brun_with_timeout\b/g, ""),
    )
    .join("\n");
}

describe("toLogicalLines", () => {
  it("joins a backslash continuation", () => {
    const lines = toLogicalLines('run_with_timeout 10 \\\n  corepack prepare "pnpm@1.2.3"');
    assert.equal(lines.length, 1);
    assert.equal(lines[0].line, 1);
    assert.match(lines[0].text, /run_with_timeout 10\s+corepack prepare/);
  });

  it("joins across a trailing `||`, which bash continues without a backslash", () => {
    const lines = toLogicalLines("curl https://example.test ||\n  rc=$?");
    assert.equal(lines.length, 1);
    assert.match(lines[0].text, /curl .* rc=\$\?/);
  });

  it("drops whole-line comments and does not let them continue a join", () => {
    const lines = toLogicalLines("# curl https://example.test\nreal_command\n");
    assert.equal(lines.length, 1);
    assert.equal(lines[0].text.trim(), "real_command");
  });

  it("reports the STARTING physical line of a joined command", () => {
    const lines = toLogicalLines("a=1\nb=2\ncurl https://example.test \\\n  | tar zxf -");
    assert.equal(lines.at(-1).line, 3);
  });
});

describe("nextTokenIsVersionFlag", () => {
  for (const rest of [" --version 2>/dev/null)\" = x ]", "--version", "  -V"]) {
    it(`treats ${JSON.stringify(rest)} as a version probe's tail`, () => {
      assert.equal(nextTokenIsVersionFlag(rest), true);
    });
  }

  for (const rest of [" wasm-pack --version 0.14.0 --locked", " -sSf https://example.test", " --all-features check"]) {
    it(`does NOT treat ${JSON.stringify(rest)} as a version probe's tail`, () => {
      assert.equal(nextTokenIsVersionFlag(rest), false);
    });
  }

  // The reviewer-found hole: checking only "is the NEXT token --version" is
  // exemptable-by-arg-order. `cargo install --version 1.2.3 some-crate` is a
  // REAL crates.io fetch — `--version` takes a value here, it does not mean
  // "print version and exit" — even though `--version` is the very next
  // token after `install`. setup-dev.sh:411 runs the same command with the
  // package name and `--version` swapped (`cargo install cargo-deny
  // --version "${CARGO_DENY_VERSION}"`), which was already safe under the
  // old check only by argument order, not by design.
  it("does NOT treat a --version flag as the probe's tail when a bare word follows its value", () => {
    assert.equal(nextTokenIsVersionFlag(' --version "${CARGO_DENY_VERSION}" cargo-deny --locked'), false);
  });

  it("does NOT treat a --version flag as the probe's tail when an unquoted value+word follows", () => {
    assert.equal(nextTokenIsVersionFlag(" --version 1.2.3 some-crate"), false);
  });
});

describe("findUnboundedFetches — planted red, one per allowlisted command shape", () => {
  // Every entry in the allowlist must actually fire on a representative
  // unbounded invocation. A pattern that matches nothing is a silent hole.
  const plants = {
    curl: 'curl --proto \'=https\' -sSfL "https://example.test/x.tar.gz" | tar zxf -',
    wget: "wget https://example.test/x.tar.gz",
    rustup: "rustup show",
    corepack: 'corepack prepare "pnpm@1.2.3" --activate',
    pnpm: "pnpm install --frozen-lockfile",
    npm: "npm ci",
    npx: "npx some-tool",
    yarn: "yarn install",
    "cargo-network": "cargo install cargo-nextest --locked",
    "git-remote": "git clone https://example.test/repo.git",
    "wasm-pack": "wasm-pack build crates/brink-web --target web",
    "cargo-deny-binary": "cargo-deny check advisories",
    "cargo-nextest-binary": "cargo-nextest run --workspace",
    // #2677: added once repo-wide discovery reached benchmarks/setup.sh, where
    // `brew install hyperfine` was unbounded AND unallowlisted — check 3
    // reported it as unclassified before check 1 could see it at all.
    homebrew: "brew install hyperfine",
  };

  for (const command of NETWORK_COMMANDS) {
    it(`flags an unbounded ${command.id}`, () => {
      const plant = plants[command.id];
      assert.ok(plant, `no planted red case for allowlist entry "${command.id}" — add one`);

      const result = findUnboundedFetches(plant);
      assert.equal(result.ok, false);
      assert.equal(result.findings.length >= 1, true);
      assert.equal(
        result.findings.some((finding) => finding.id === command.id),
        true,
        `expected a "${command.id}" finding, got ${JSON.stringify(result.findings.map((f) => f.id))}`,
      );
    });

    it(`accepts the same ${command.id} once wrapped in run_with_timeout`, () => {
      const bounded = `run_with_timeout "\${BRINK_SETUP_X_TIMEOUT}" ${plants[command.id]}`;
      assert.deepEqual(findUnboundedFetches(bounded).problems, []);
    });
  }
});

describe("findUnboundedFetches — the #2642 miss, encoded", () => {
  // The third hand audit missed `pnpm --version`: under corepack's shim it
  // downloads the pinned tarball on a cache miss. No lexical scan can SEE
  // that; it is caught only because `pnpm` carries probeStillFetches. If that
  // flag is ever dropped, this test goes red.
  it("flags an unbounded `pnpm --version` despite the --version exemption", () => {
    const result = findUnboundedFetches('resolved="$(pnpm --version 2>/dev/null || true)"');
    assert.equal(result.ok, false);
    assert.equal(result.findings[0].id, "pnpm");
  });

  it("still exempts a genuinely local `cargo deny --version` probe", () => {
    assert.deepEqual(findUnboundedFetches("[ \"$(cargo deny --version 2>/dev/null)\" = x ]").problems, []);
  });

  // The reviewer-found arg-order hole (proved against setup-dev.sh:411's own
  // command, with --version moved before the package name): the OLD check
  // exempted this as a "local probe" and reported zero problems, even though
  // it is an unbounded crates.io fetch.
  it("does NOT exempt `cargo install --version X pkg` as a local probe", () => {
    const result = findUnboundedFetches('cargo install --version "${CARGO_DENY_VERSION}" cargo-deny --locked');
    assert.equal(result.ok, false);
    assert.equal(
      result.findings.some((finding) => finding.id === "cargo-network"),
      true,
    );
  });

  it("keeps every package manager opted out of the --version exemption", () => {
    for (const id of ["pnpm", "npm", "npx", "yarn"]) {
      const command = NETWORK_COMMANDS.find((entry) => entry.id === id);
      assert.equal(command?.probeStillFetches, true, `${id} must keep probeStillFetches (#2642)`);
    }
  });
});

describe("findUnboundedFetches — documented non-findings", () => {
  it("does not flag a command named inside an echo", () => {
    assert.deepEqual(findUnboundedFetches('echo "    pnpm install:checked -- --frozen-lockfile"').problems, []);
  });

  it("does not flag a tool NAME listed in a for-loop word list", () => {
    assert.deepEqual(
      findUnboundedFetches("for tool in rustc cargo wasm-pack cargo-nextest pnpm node; do").problems,
      [],
    );
  });

  it("does not flag `corepack enable`, which only writes local shims", () => {
    assert.deepEqual(findUnboundedFetches("corepack enable").problems, []);
  });

  it("does not flag `command -v rustup`", () => {
    assert.deepEqual(findUnboundedFetches("if ! command -v rustup >/dev/null 2>&1; then").problems, []);
  });

  it("does not flag a whole-line comment mentioning curl", () => {
    assert.deepEqual(findUnboundedFetches("# ⚠ never fetch this with curl").problems, []);
  });

  // Was "DOES flag a trailing comment — a stated false positive" until
  // #2667 gave check 1 the quote-aware tokenizer, which strips trailing
  // comments outside quotes. The header's "cannot see" list was updated in
  // the same change; this test now pins the NEW behaviour so the note and
  // the code cannot drift apart again in either direction.
  it("does not flag a trailing comment mentioning curl", () => {
    assert.deepEqual(findUnboundedFetches("FOO=1 # fetched with curl elsewhere").problems, []);
  });

  // The other half of that tokenizer change: a tool NAME printed in prose is
  // not an invocation. `echo "==> wasm-pack already installed ($(wasm-pack
  // --version))"` is live in setup-dev.sh, and the pre-#2667 scan reported
  // the PROSE occurrence as an unbounded fetch while the real (exempt) one
  // sat inside the substitution.
  it("does not flag a tool named in echo prose alongside a real substitution", () => {
    assert.deepEqual(
      findUnboundedFetches('echo "==> wasm-pack already installed ($(wasm-pack --version))"').problems,
      [],
    );
  });

  // `command -v X` is a PATH lookup, never an invocation — so it cannot
  // fetch, whatever X is.
  it("does not flag `command -v` on a network binary", () => {
    assert.deepEqual(findUnboundedFetches("if command -v wasm-pack >/dev/null 2>&1; then").problems, []);
  });
});

describe("findUnboundedFetches — a fetch hidden inside echo/printf's command substitution", () => {
  // The pre-fix behaviour skipped the WHOLE segment for any echo/printf/read
  // head, so `echo "$(curl ...)"` was invisible even though the outer
  // command only prints and the inner `$(...)` runs a real fetch. This shape
  // is live in the real script (`echo "==> … ($(rustup --version …))"`).
  it("flags an unbounded curl hidden inside `echo \"$(...)\"`", () => {
    const result = findUnboundedFetches('echo "$(curl -sSf https://example.test)"');
    assert.equal(result.ok, false);
    assert.equal(
      result.findings.some((finding) => finding.id === "curl"),
      true,
    );
  });

  it("flags an unbounded curl hidden inside `printf \"$(...)\"`", () => {
    const result = findUnboundedFetches('printf "%s" "$(curl -sSf https://example.test)"');
    assert.equal(result.ok, false);
    assert.equal(
      result.findings.some((finding) => finding.id === "curl"),
      true,
    );
  });

  it("still does not flag a plain echo with no command substitution", () => {
    assert.deepEqual(findUnboundedFetches('echo "install curl for local dev"').problems, []);
  });

  it("still does not flag a for-loop word list (structural heads stay unconditional)", () => {
    assert.deepEqual(
      findUnboundedFetches("for tool in rustc cargo wasm-pack cargo-nextest pnpm node; do").problems,
      [],
    );
  });

  // The real setup-dev.sh shape that made the naive "any backtick" version of
  // this fix a false positive: escaped backticks around a command NAME shown
  // to the user are quoting punctuation, not a command substitution.
  it("does not flag an echo whose backticks are backslash-escaped literal quoting", () => {
    assert.deepEqual(
      findUnboundedFetches('echo "check for a standalone pnpm (\\`which -a pnpm\\`) on PATH"').problems,
      [],
    );
  });
});

describe("parseKnobTable / findKnobAssignments", () => {
  const table = [
    "#   Knob                              Default  On timeout",
    "#   ------------------------------------------------------",
    "#   BRINK_SETUP_A_TIMEOUT                120s   FAIL (exit 1) — nothing",
    "#                                                else works.",
    "#   BRINK_SETUP_B_TIMEOUT                 60s   WARN, continue.",
    "#",
    "# Also see BRINK_SETUP_FULL.",
    "",
    'BRINK_SETUP_A_TIMEOUT="${BRINK_SETUP_A_TIMEOUT:-120}"',
    'BRINK_SETUP_B_TIMEOUT="${BRINK_SETUP_B_TIMEOUT:-60}"',
    'run_with_timeout "${BRINK_SETUP_A_TIMEOUT}" rustup show',
    'run_with_timeout "${BRINK_SETUP_B_TIMEOUT}" curl https://example.test',
  ].join("\n");

  it("parses rows, folding continuation lines into the outcome cell", () => {
    const rows = parseKnobTable(table);
    assert.deepEqual(
      rows.map((row) => [row.name, row.default]),
      [
        ["BRINK_SETUP_A_TIMEOUT", 120],
        ["BRINK_SETUP_B_TIMEOUT", 60],
      ],
    );
    assert.match(rows[0].outcome, /else works/);
  });

  it("stops at the blank comment line that ends the table", () => {
    assert.equal(parseKnobTable(table).length, 2);
  });

  it("finds the self-referential assignments", () => {
    assert.deepEqual(
      findKnobAssignments(table).map((assignment) => [assignment.name, assignment.default, assignment.selfReferential]),
      [
        ["BRINK_SETUP_A_TIMEOUT", 120, true],
        ["BRINK_SETUP_B_TIMEOUT", 60, true],
      ],
    );
  });

  it("accepts the healthy fixture", () => {
    assert.deepEqual(checkKnobTable(table).problems, []);
  });

  it("goes red when a row is DELETED (the issue's named proof)", () => {
    const withoutRow = table.replace("#   BRINK_SETUP_B_TIMEOUT                 60s   WARN, continue.\n", "");
    const result = checkKnobTable(withoutRow);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /BRINK_SETUP_B_TIMEOUT.*no row for it/s);
  });

  it("goes red when a default drifts in one place only", () => {
    const drifted = table.replace('BRINK_SETUP_B_TIMEOUT:-60}"', 'BRINK_SETUP_B_TIMEOUT:-90}"');
    const result = checkKnobTable(drifted);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /says 60s, but the assignment at line \d+ defaults to 90s/);
  });

  it("goes red on a STALE row for a knob the script no longer assigns", () => {
    const stale = table.replace('BRINK_SETUP_B_TIMEOUT="${BRINK_SETUP_B_TIMEOUT:-60}"\n', "");
    const result = checkKnobTable(stale);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /which the script never assigns/);
  });

  it("goes red on a documented-but-inert knob (assigned, tabled, never read)", () => {
    const inert = table.replace('run_with_timeout "${BRINK_SETUP_B_TIMEOUT}" curl https://example.test', "");
    const result = checkKnobTable(inert);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /never reads \$\{BRINK_SETUP_B_TIMEOUT\}/);
  });

  it("goes red when a row's fail-vs-warn cell says neither FAIL nor WARN", () => {
    const vague = table.replace("WARN, continue.", "it depends.");
    const result = checkKnobTable(vague);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /does not say FAIL or WARN/);
  });

  it("goes red when the assignment reads a DIFFERENT variable than it sets", () => {
    const crossed = table.replace(
      'BRINK_SETUP_B_TIMEOUT="${BRINK_SETUP_B_TIMEOUT:-60}"',
      'BRINK_SETUP_B_TIMEOUT="${BRINK_SETUP_OTHER:-60}"',
    );
    const result = checkKnobTable(crossed);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /assigns BRINK_SETUP_B_TIMEOUT from a DIFFERENT variable/);
  });

  it("goes red when the header table is missing entirely", () => {
    const result = checkKnobTable('BRINK_SETUP_A_TIMEOUT="${BRINK_SETUP_A_TIMEOUT:-120}"');
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /no parseable table row was found/);
  });
});

describe("findKnobAssignments — assignment spellings beyond the bare quoted form", () => {
  it("recognises an `export`-ed assignment", () => {
    const found = findKnobAssignments('export BRINK_SETUP_X_TIMEOUT="${BRINK_SETUP_X_TIMEOUT:-45}"');
    assert.deepEqual(
      found.map((a) => [a.name, a.default, a.selfReferential]),
      [["BRINK_SETUP_X_TIMEOUT", 45, true]],
    );
  });

  it("recognises an unquoted RHS", () => {
    const found = findKnobAssignments("BRINK_SETUP_X_TIMEOUT=${BRINK_SETUP_X_TIMEOUT:-45}");
    assert.deepEqual(
      found.map((a) => [a.name, a.default, a.selfReferential]),
      [["BRINK_SETUP_X_TIMEOUT", 45, true]],
    );
  });

  it("recognises an `export`-ed assignment with an unquoted RHS", () => {
    const found = findKnobAssignments("export BRINK_SETUP_X_TIMEOUT=${BRINK_SETUP_X_TIMEOUT:-45}");
    assert.deepEqual(
      found.map((a) => [a.name, a.default, a.selfReferential]),
      [["BRINK_SETUP_X_TIMEOUT", 45, true]],
    );
  });
});

describe("checkKnobTable — the #2647 silent-drift finding, encoded", () => {
  // Before the fix: an `export`-ed assignment was invisible to
  // findKnobAssignments entirely, so a knob assigned ONLY this way, with NO
  // row in the header table, produced zero problems — checkKnobTable
  // returned [] with no missing-row check, no default comparison and no
  // fail-vs-warn check, none of the three fired. That is the exact silent
  // drift #2647 exists to stop.
  it("catches a missing row for a knob assigned only via `export` (was silently invisible)", () => {
    const table = [
      "#   Knob                              Default  On timeout",
      "#   ------------------------------------------------------",
      "#   BRINK_SETUP_A_TIMEOUT                120s   FAIL (exit 1) — nothing",
      "#                                                else works.",
      "#",
      'export BRINK_SETUP_X_TIMEOUT="${BRINK_SETUP_X_TIMEOUT:-45}"',
      'run_with_timeout "${BRINK_SETUP_X_TIMEOUT}" curl https://example.test',
    ].join("\n");

    const result = checkKnobTable(table);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /BRINK_SETUP_X_TIMEOUT.*no row for it/s);
  });

  it("catches a drifted default in an `export`-ed assignment (was silently invisible)", () => {
    const table = [
      "#   BRINK_SETUP_X_TIMEOUT                 45s   WARN, continue.",
      "#",
      'export BRINK_SETUP_X_TIMEOUT="${BRINK_SETUP_X_TIMEOUT:-90}"',
      'run_with_timeout "${BRINK_SETUP_X_TIMEOUT}" curl https://example.test',
    ].join("\n");

    const result = checkKnobTable(table);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /says 45s, but the assignment at line \d+ defaults to 90s/);
  });
});

describe("findUnrecognizedKnobShapes — the colon-default idiom", () => {
  it("reports `: \"${NAME:=N}\"`, a shape neither check parses", () => {
    const found = findUnrecognizedKnobShapes(': "${BRINK_SETUP_X_TIMEOUT:=30}"');
    assert.deepEqual(
      found.map((f) => f.name),
      ["BRINK_SETUP_X_TIMEOUT"],
    );
  });

  it("does not re-report a line findKnobAssignments already recognised", () => {
    const found = findUnrecognizedKnobShapes('BRINK_SETUP_X_TIMEOUT="${BRINK_SETUP_X_TIMEOUT:-45}"');
    assert.deepEqual(found, []);
  });

  it("does not flag a plain read (no `=` immediately after the identifier)", () => {
    const found = findUnrecognizedKnobShapes('run_with_timeout "${BRINK_SETUP_X_TIMEOUT}" curl https://example.test');
    assert.deepEqual(found, []);
  });

  it("checkKnobTable surfaces it as a problem, not a silent pass", () => {
    const table = [
      "#   BRINK_SETUP_X_TIMEOUT                 30s   WARN, continue.",
      "#",
      ': "${BRINK_SETUP_X_TIMEOUT:=30}"',
    ].join("\n");

    const result = checkKnobTable(table);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /shape this check cannot parse/);
  });
});

describe("sliceSection", () => {
  const markdown = "# Top\n\nintro\n\n## Cloud / fresh-environment sessions\n\nbody\n\n## Key commands\n\nother\n";

  it("slices a section to the next heading of the same level", () => {
    const section = sliceSection(markdown, "Cloud / fresh-environment sessions");
    assert.match(section, /body/);
    assert.doesNotMatch(section, /other/);
  });

  it("returns null when the heading is gone", () => {
    assert.equal(sliceSection(markdown, "Nonexistent"), null);
  });
});

describe("checkDocPointers", () => {
  const pointing = {
    path: "fake.md",
    text: "The knob/default/fail-vs-warn table lives in `scripts/setup-dev.sh`'s own header block; read it there.",
  };

  it("accepts a doc that delegates to the header table", () => {
    assert.deepEqual(checkDocPointers([pointing]).problems, []);
  });

  it("accepts a pointer broken across a hard-wrapped markdown line", () => {
    const wrapped = {
      path: "fake.md",
      text: "every network step in `setup-dev.sh` carries its own knob, and the authoritative\nknob/default/fail-vs-warn table lives in that script's header\nblock.",
    };
    assert.deepEqual(checkDocPointers([wrapped]).problems, []);
  });

  it("goes red when the pointer is replaced by a restated knob list", () => {
    const restated = {
      path: "fake.md",
      text: "`scripts/setup-dev.sh` bounds each fetch; BRINK_SETUP_RUSTUP_TIMEOUT defaults to 120s.",
    };
    const result = checkDocPointers([restated]);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /no longer points at scripts\/setup-dev\.sh's header block/);
  });

  it("goes red when the named section itself disappears", () => {
    const result = checkDocPointers([{ path: "fake.md", section: "Cloud / fresh-environment sessions", text: "# Top\n" }]);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /no longer has a "Cloud \/ fresh-environment sessions" section/);
  });

  it("only looks inside the named section", () => {
    const outside = {
      path: "fake.md",
      section: "Cloud / fresh-environment sessions",
      text:
        "## Cloud / fresh-environment sessions\n\nrun `scripts/setup-dev.sh` first.\n\n" +
        "## Elsewhere\n\nthe fail-vs-warn table lives in `setup-dev.sh`'s header block.\n",
    };
    assert.equal(checkDocPointers([outside]).ok, false);
  });
});

describe("the REAL repo", () => {
  it("passes every check", () => {
    const result = checkScripts();
    assert.deepEqual(result.problems, []);
    assert.equal(result.ok, true);
  });

  it("names the three documents #2640 pointed at the header table", () => {
    assert.deepEqual(
      POINTER_DOCS.map((doc) => doc.path),
      ["CLAUDE.md", "docs/desktop-shell-spec.md", "docs/releasing.md"],
    );
  });

  // Non-vacuity, against the REAL file rather than a fixture: strip every
  // `run_with_timeout` wrapper out of setup-dev.sh and the scan must light up
  // on the fetches that were behind them. A scan that sees nothing here would
  // pass the healthy-tree test above while detecting nothing at all.
  it("detects the real fetches once their bounds are stripped", () => {
    const unwrapped = realSetupDev
      .split("\n")
      .map((line) => (/^\s*#/.test(line) ? line : line.replace(/run_with_timeout\s+"[^"]*"\s*/g, "").replace(/run_with_timeout/g, "")))
      .join("\n");

    const result = findUnboundedFetches(unwrapped);
    const ids = new Set(result.findings.map((finding) => finding.id));

    // Every family the three hand audits enumerated, plus the one they missed.
    for (const id of ["curl", "rustup", "cargo-network", "corepack", "pnpm"]) {
      assert.equal(ids.has(id), true, `stripping bounds should expose a "${id}" fetch; saw ${JSON.stringify([...ids])}`);
    }
    assert.equal(result.findings.length >= 10, true, `expected ≥10 fetch sites, saw ${result.findings.length}`);
  });

  it("goes red when a real header-table row is deleted", () => {
    const rows = parseKnobTable(realSetupDev);
    assert.equal(rows.length > 0, true, "the real script must have a parseable knob table");

    const lines = realSetupDev.split("\n");
    const victim = rows[0];
    // Drop the row line and any continuation lines folded into its cell.
    let end = victim.line; // 1-indexed row line == 0-indexed line after it
    while (end < lines.length && /^#\s{2,}(?!BRINK_SETUP)\S/.test(lines[end])) end += 1;
    const withoutRow = [...lines.slice(0, victim.line - 1), ...lines.slice(end)].join("\n");

    const result = checkKnobTable(withoutRow);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), new RegExp(`${victim.name}.*no row for it`, "s"));
  });

  it("goes red when a real fetch loses its bound", () => {
    const unwrappedToolchain = realSetupDev.replace(
      'run_with_timeout "${BRINK_SETUP_TOOLCHAIN_TIMEOUT}" rustup show',
      "rustup show",
    );
    assert.notEqual(unwrappedToolchain, realSetupDev, "the toolchain fetch must still be wrapped in the real script");

    const result = findUnboundedFetches(unwrappedToolchain);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /rustup/);
  });
});

// =============================================================================
// #2667 — the scan covers EVERY shell script in scripts/, not one hardcoded
// path; and #2666 — every command those scripts invoke is classified.
// =============================================================================

describe("discoverShellScripts (#2667)", () => {
  const scripts = discoverShellScripts();

  it("finds setup-dev.sh — the file the scan used to be hardcoded to", () => {
    assert.equal(scripts.includes(SETUP_DEV_PATH), true);
  });

  // The regression that IS #2667: this script had two bare `cargo update`
  // calls and sat entirely outside the scan because SETUP_DEV_PATH was a
  // single hardcoded string.
  it("finds refresh-excluded-lockfiles.sh — the script #2667 was invisible in", () => {
    assert.equal(scripts.includes(REFRESH_LOCKFILES_PATH), true);
  });

  it("descends into scripts/lib/", () => {
    assert.equal(scripts.includes("scripts/lib/run-with-timeout.sh"), true);
  });

  it("excludes *.test.sh (their heredoc stub bodies would all read as fetches)", () => {
    assert.deepEqual(scripts.filter((path) => path.endsWith(".test.sh")), []);
    assert.equal(scripts.includes("scripts/setup-dev.test.sh"), false);
  });

  it("returns a sorted list, so problem order is deterministic", () => {
    assert.deepEqual(scripts, [...scripts].sort());
  });

  // Since #2677 the walk starts at the repo root, so `.sh` is no longer the
  // only admissible shape: `.githooks/` holds extensionless git hooks that git
  // executes as shell.
  it("returns only .sh files and .githooks entries", () => {
    for (const path of scripts) {
      assert.equal(
        path.endsWith(".sh") || path.startsWith(".githooks/"),
        true,
        `unexpected discovered path ${path}`,
      );
    }
  });

  it("never returns a *.test.sh — the excluded-by-design harnesses", () => {
    for (const path of scripts) assert.equal(path.endsWith(".test.sh"), false, path);
  });

  // The #2677 regression: benchmarks/setup.sh sat one directory outside the
  // scripts/ walk with three unbounded fetches in it, reachable from
  // `just cross-language-benchmark`.
  it("reaches benchmarks/setup.sh — the script #2677 found outside the scripts/ walk", () => {
    assert.equal(scripts.includes(BENCHMARKS_SETUP_PATH), true, JSON.stringify(scripts));
  });

  // A repo-root walk must not wander into the agent worktrees under
  // .claude/worktrees/, each of which is a full second copy of this tree.
  it("prunes nested checkouts, so no path repeats a whole second tree", () => {
    for (const path of scripts) {
      assert.equal(path.includes("/worktrees/"), false, `walked into a nested checkout: ${path}`);
      assert.equal(path.includes("node_modules"), false, `walked into dependencies: ${path}`);
    }
  });
});

// The real-repo assertion above (`prunes nested checkouts`) is VACUOUS on CI
// and on any machine without agent worktrees: discoverShellScripts() over the
// real tree just never contains a nested path to begin with, so the assertion
// has nothing to catch. This is exactly how the #2692 review finding shipped
// — `.claude/worktrees/wf_stale/benchmarks/setup.sh` (a worktree with its
// `.git` file stripped) walked straight through the `.git`-only check with no
// test able to see it. Build a synthetic repo tree instead, so all three
// shapes the walk must tell apart are exercised directly.
describe("discoverShellScripts nested-checkout pruning (#2692 review)", () => {
  it("prunes a nested dir with its own `.git` FILE, keeps a plain nested dir with a real script, and — per the fix above — also prunes a nested tree copy with NO `.git` at all", () => {
    const root = mkdtempSync(join(tmpdir(), "check-scripts-nested-"));
    try {
      // Shape 1: a real git worktree/submodule — a `.git` FILE (not a
      // directory), the same as `.claude/worktrees/<id>/.git`. Must be pruned.
      mkdirSync(join(root, "worktree-with-git", "scripts"), { recursive: true });
      writeFileSync(join(root, "worktree-with-git", ".git"), "gitdir: /elsewhere/.git\n");
      writeFileSync(join(root, "worktree-with-git", "scripts", "inner.sh"), "#!/usr/bin/env bash\n");

      // Shape 2: a tree copy with NO `.git` at all — an extracted archive, a
      // `cp -r` backup, or a worktree stripped of its `.git` file, matching
      // the reproduction in the review finding. Only catchable by shape
      // (Cargo.toml + justfile), which is exactly what the fix above adds.
      mkdirSync(join(root, "copy-no-git", "scripts"), { recursive: true });
      writeFileSync(join(root, "copy-no-git", "Cargo.toml"), "[workspace]\n");
      writeFileSync(join(root, "copy-no-git", "justfile"), "default:\n  echo hi\n");
      writeFileSync(join(root, "copy-no-git", "scripts", "inner2.sh"), "#!/usr/bin/env bash\n");

      // Shape 3: an ordinary nested directory that is NOT a checkout copy —
      // no `.git`, no Cargo.toml/justfile pair — holding a real script that
      // must still be discovered.
      mkdirSync(join(root, "plain-subdir"), { recursive: true });
      writeFileSync(join(root, "plain-subdir", "util.sh"), "#!/usr/bin/env bash\n");

      const found = discoverShellScripts(root);

      assert.deepEqual(
        found.filter((p) => p.startsWith("worktree-with-git/")),
        [],
        "a nested dir with its own .git file must be pruned",
      );
      assert.deepEqual(
        found.filter((p) => p.startsWith("copy-no-git/")),
        [],
        "a nested tree copy with no .git (Cargo.toml + justfile) must also be pruned",
      );
      assert.deepEqual(
        found,
        ["plain-subdir/util.sh"],
        "a plain nested directory's real script must still be discovered",
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});

describe("splitSegmentsQuoteAware (#2667)", () => {
  it("does not split on a `;` inside a string", () => {
    assert.deepEqual(splitSegmentsQuoteAware('echo "a; b"'), ['echo "a; b"']);
  });

  it("does not split on a `|` inside a single-quoted regex", () => {
    assert.deepEqual(splitSegmentsQuoteAware("grep -E '^(a|b)'"), ["grep -E '^(a|b)'"]);
  });

  it("does split on a real pipe", () => {
    assert.deepEqual(
      splitSegmentsQuoteAware("curl x | tar zxf -").map((piece) => piece.trim()),
      ["curl x", "tar zxf -"],
    );
  });

  it("splits on `&&` but NOT on the lone `&` of a redirection", () => {
    assert.deepEqual(
      splitSegmentsQuoteAware("command -v timeout >/dev/null 2>&1 && echo yes").map((piece) => piece.trim()),
      ["command -v timeout >/dev/null 2>&1", "echo yes"],
    );
  });

  it("drops a trailing comment", () => {
    assert.equal(splitSegmentsQuoteAware("FOO=1 # see curl(1)")[0].trim(), "FOO=1");
  });

  it("does not split inside a `$( … )` substitution", () => {
    assert.equal(splitSegmentsQuoteAware("pkgs=$(grep x f | sort -u)").length, 1);
  });
});

describe("commandHead (#2666)", () => {
  const cases = [
    ["curl -sSf https://example.test", "curl"],
    ["if curl -sSf https://example.test", "curl"],
    ["! cargo update", "cargo"],
    ["while read -r line", "read"],
    // `commandHead` runs on an ALREADY-SPLIT segment, so a subshell's `cd`
    // and its `&&` right-hand side arrive separately; the grouping `(` is
    // peeled off the first.
    ["(cd dir", "cd"],
    ["run_with_timeout 60 corepack prepare", "corepack"],
    ['run_with_timeout "${T}" cargo install x', "cargo"],
    ["command -v rustup >/dev/null", "rustup"],
    ["FOO=1 BAR=2 curl x", "curl"],
    ["*) exit 0 ;;", "exit"],
    ["Linux/x86_64) ASSET=a", ""],
    ["run_with_timeout() {", ""],
    ["rc=0", ""],
    ["dirs=(a b c)", ""],
    ["n=$((n + 1))", ""],
    ['V="$(node -p "x")"', ""],
    ["for tool in curl pnpm; do", "for"],
  ];

  for (const [segment, expected] of cases) {
    it(`${JSON.stringify(segment)} → ${JSON.stringify(expected)}`, () => {
      assert.equal(commandHead(segment), expected);
    });
  }

  // The shape #2667 introduced into refresh-excluded-lockfiles.sh, driven
  // through the same split→head pipeline the checks use.
  it("reaches `cargo` inside `( cd dir && run_with_timeout N cargo update )`", () => {
    const heads = splitSegmentsQuoteAware('(cd "$dir" && run_with_timeout "${T}" cargo update -p brink)').map(
      commandHead,
    );
    assert.equal(heads.includes("cargo"), true, `saw ${JSON.stringify(heads)}`);
  });
});

describe("findFunctionNames (#2666)", () => {
  it("finds a plain definition", () => {
    assert.equal(findFunctionNames("run_with_timeout() {\n  :\n}").has("run_with_timeout"), true);
  });

  it("finds a `function`-keyword definition", () => {
    assert.equal(findFunctionNames("function wasm_pack_ok() {").has("wasm_pack_ok"), true);
  });

  it("ignores a definition inside a comment", () => {
    assert.equal(findFunctionNames("# helper() { … }").size, 0);
  });
});

describe("findUnclassifiedCommands (#2666)", () => {
  it("reports a binary in neither list", () => {
    const result = findUnclassifiedCommands("brand_new_fetcher --pull https://example.test", "fake.sh");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /invokes `brand_new_fetcher`/);
    assert.match(result.problems.join("\n"), /Classify it \(#2666\)/);
  });

  it("names the file and line so the report is actionable", () => {
    const result = findUnclassifiedCommands("#!/usr/bin/env bash\nset -e\nmystery_tool run\n", "fake.sh");
    assert.match(result.problems.join("\n"), /^fake\.sh:3 /);
  });

  it("accepts a known-LOCAL binary", () => {
    assert.deepEqual(findUnclassifiedCommands("mkdir -p /tmp/x", "fake.sh").problems, []);
  });

  it("accepts a known-NETWORK binary (boundedness is check 1's job, not this one)", () => {
    assert.deepEqual(findUnclassifiedCommands("curl -sSf https://example.test", "fake.sh").problems, []);
  });

  it("accepts a shell function defined in another scanned script", () => {
    const known = new Set(["run_with_timeout"]);
    assert.deepEqual(findUnclassifiedCommands("run_with_timeout 60 curl x", "fake.sh", known).problems, []);
  });

  it("reports that same function name when NO scanned script defines it", () => {
    assert.equal(findUnclassifiedCommands("some_helper 60", "fake.sh").ok, false);
  });

  // The whole point of peeling wrappers: without it the inventory would see
  // only `run_with_timeout` and never learn `mystery_fetcher` is invoked.
  it("looks THROUGH run_with_timeout at the command it wraps", () => {
    const known = new Set(["run_with_timeout"]);
    const result = findUnclassifiedCommands('run_with_timeout "${T}" mystery_fetcher --go', "fake.sh", known);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /mystery_fetcher/);
  });

  it("does not report a tool NAME listed in a for-loop word list", () => {
    assert.deepEqual(findUnclassifiedCommands("for tool in mystery_tool other; do", "fake.sh").problems, []);
  });

  it("does not report prose printed by echo", () => {
    assert.deepEqual(findUnclassifiedCommands('echo "==> committing; retry later"', "fake.sh").problems, []);
  });

  it("reports each unknown head once, however many times it appears", () => {
    const result = findUnclassifiedCommands("mystery_tool a\nmystery_tool b\nmystery_tool c\n", "fake.sh");
    assert.equal(result.problems.length, 1);
  });

  it("finds a command hidden inside a substitution", () => {
    const result = findUnclassifiedCommands('V="$(mystery_tool --print)"', "fake.sh");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /mystery_tool/);
  });
});

describe("the classification lists themselves (#2666)", () => {
  it("keeps NETWORK and LOCAL disjoint — a command cannot be both", () => {
    const overlap = [...networkBinaries()].filter((binary) => LOCAL_COMMANDS.has(binary));
    assert.deepEqual(overlap, [], "a binary listed as both network and local makes the check meaningless");
  });

  it("gives every allowlist entry a resolvable binary name", () => {
    for (const command of NETWORK_COMMANDS) {
      const binary = command.binary ?? command.id;
      assert.match(binary, /^[a-z][a-z0-9-]*$/, `${command.id} has no usable binary name`);
    }
  });
});

describe("the REAL repo, widened scan (#2667/#2666)", () => {
  it("scans more than one script", () => {
    const result = checkScripts();
    assert.equal(result.scripts.length > 1, true);
    assert.equal(result.scripts.includes(REFRESH_LOCKFILES_PATH), true);
  });

  // NON-VACUITY for #2667 specifically. Strip the bounds out of
  // refresh-excluded-lockfiles.sh and its `cargo update` calls must light up.
  // Before #2667 this file was not scanned at all, so this went green with
  // the hazard present — which is exactly how the bug shipped.
  it("detects refresh-excluded-lockfiles.sh's cargo update once its bounds are stripped", () => {
    const unwrapped = realRefreshLockfiles
      .split("\n")
      .map((line) => (/^\s*#/.test(line) ? line : line.replace(/run_with_timeout\s+"[^"]*"\s*/g, "")))
      .join("\n");
    assert.notEqual(unwrapped, realRefreshLockfiles, "the cargo update calls must still be wrapped");

    const result = findUnboundedFetches(unwrapped, REFRESH_LOCKFILES_PATH);
    assert.equal(result.ok, false);
    assert.equal(
      result.findings.some((finding) => finding.id === "cargo-network"),
      true,
      `expected a cargo finding, got ${JSON.stringify(result.findings.map((f) => f.id))}`,
    );
    assert.match(result.problems.join("\n"), new RegExp(REFRESH_LOCKFILES_PATH.replace(/[/.]/g, "\\$&")));
  });

  it("goes red when an unclassified binary is planted in a real script", () => {
    const planted = `${realRefreshLockfiles}\nbrand_new_fetcher --sync\n`;
    const result = findUnclassifiedCommands(planted, REFRESH_LOCKFILES_PATH, new Set(["run_with_timeout"]));
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /brand_new_fetcher/);
  });

  // The inventory must actually be SEEING commands. A head extractor that
  // silently returned nothing would pass every green assertion above.
  it("extracts the commands setup-dev.sh really invokes", () => {
    const functions = new Set();
    for (const path of discoverShellScripts()) {
      for (const name of findFunctionNames(readFileSync(join(REPO_ROOT, path), "utf8"))) functions.add(name);
    }

    const heads = new Set(findUnclassifiedCommands(realSetupDev, SETUP_DEV_PATH, functions).heads.map((h) => h.head));
    for (const expected of ["curl", "cargo", "rustup", "corepack", "pnpm", "node", "tar"]) {
      assert.equal(heads.has(expected), true, `expected to see \`${expected}\`; saw ${JSON.stringify([...heads])}`);
    }
  });

  it("extracts the commands refresh-excluded-lockfiles.sh really invokes", () => {
    const heads = new Set(
      findUnclassifiedCommands(realRefreshLockfiles, REFRESH_LOCKFILES_PATH, new Set(["run_with_timeout"])).heads.map(
        (head) => head.head,
      ),
    );
    for (const expected of ["cargo", "grep", "sed", "sort"]) {
      assert.equal(heads.has(expected), true, `expected to see \`${expected}\`; saw ${JSON.stringify([...heads])}`);
    }
  });
});

// =============================================================================
// #2677 — the scan reaches past scripts/**/*.sh: justfile recipe bodies and
// every other shell script in the repo. #2678 — the knob-table check is
// per-(script, prefix) instead of hardwired to BRINK_SETUP_.
// =============================================================================

describe("justfileShellView (#2677)", () => {
  const view = justfileShellView(realJustfile);

  it("preserves line numbers exactly, so a report points at the real line", () => {
    assert.equal(view.split("\n").length, realJustfile.split("\n").length);
  });

  it("blanks recipe HEADERS, which are just syntax rather than shell", () => {
    const lines = view.split("\n");
    const headerIndex = realJustfile.split("\n").findIndex((line) => line === "book-assets:");
    assert.ok(headerIndex >= 0, "the book-assets recipe must still exist");
    assert.equal(lines[headerIndex], "");
  });

  it("keeps recipe BODY lines verbatim", () => {
    assert.match(view, /pnpm --filter @brink-lang\/studio build:embed/);
  });

  it("blanks a `name := value` assignment rather than reading it as a recipe header", () => {
    const lines = view.split("\n");
    const index = realJustfile.split("\n").findIndex((line) => line.startsWith("fuzz_duration :="));
    assert.ok(index >= 0, "the fuzz_duration assignment must still exist");
    assert.equal(lines[index], "");
    // If `:=` had read as a recipe header, every following line would have
    // been treated as a body.
    assert.equal(view.includes("fuzz_duration :="), false);
  });

  it("keeps comment lines, which is where the knob table and the waivers live", () => {
    assert.match(view, /BRINK_JUST_WASM_TIMEOUT\s+900s/);
    assert.match(view, /check-scripts: allow-unbounded/);
  });
});

describe("the REAL justfile (#2677)", () => {
  const view = justfileShellView(realJustfile);

  it("is one of the scanned sources", () => {
    assert.equal(
      discoverShellSources().some((source) => source.path === JUSTFILE_PATH),
      true,
    );
  });

  it("has no unbounded fetch as committed", () => {
    assert.deepEqual(findUnboundedFetches(view, JUSTFILE_PATH).problems, []);
  });

  // NON-VACUITY, the #2656 standard applied to the newly-scanned surface:
  // strip every bound out of the REAL justfile and the scan must light up on
  // the fetches that were behind them. Before #2677 this scan did not exist,
  // so all of these shipped bare.
  it("detects the real justfile fetches once their bounds are stripped", () => {
    const unwrapped = stripBounds(view);
    assert.notEqual(unwrapped, view, "the justfile's fetches must still be wrapped");

    const result = findUnboundedFetches(unwrapped, JUSTFILE_PATH);
    const ids = new Set(result.findings.map((finding) => finding.id));

    for (const id of ["wasm-pack", "npm", "pnpm"]) {
      assert.equal(ids.has(id), true, `expected a "${id}" finding; saw ${JSON.stringify([...ids])}`);
    }
    assert.equal(
      result.findings.length >= 7,
      true,
      `expected >=7 fetch sites in the unwrapped justfile, saw ${result.findings.length}`,
    );
  });

  it("goes red when one specific real bound is removed", () => {
    const unwrapped = view.replace(
      'run_with_timeout "${BRINK_JUST_PNPM_INSTALL_TIMEOUT}" pnpm install:checked',
      "pnpm install:checked",
    );
    assert.notEqual(unwrapped, view, "the book-assets install must still be wrapped");

    const result = findUnboundedFetches(unwrapped, JUSTFILE_PATH);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /pnpm install:checked/);
  });

  it("invokes the commands we think it does — the head extractor is not silent", () => {
    const functions = new Set();
    for (const source of discoverShellSources()) {
      for (const name of findFunctionNames(source.text)) functions.add(name);
    }
    const heads = new Set(findUnclassifiedCommands(view, JUSTFILE_PATH, functions).heads.map((h) => h.head));
    // `run_with_timeout` is deliberately absent: `commandHead` PEELS it as a
    // wrapper, so a bounded fetch reports the wrapped binary instead. That is
    // what makes the inventory see `wasm-pack`/`pnpm` at all.
    for (const expected of ["cargo", "wasm-pack", "pnpm", "npm", "mdbook", "git"]) {
      assert.equal(heads.has(expected), true, `expected \`${expected}\`; saw ${JSON.stringify([...heads])}`);
    }
  });

  // The phantom heads the justfile scan produced before the tokenizer fixes.
  it("reports no phantom command from the fuzz recipe's multi-line array literal", () => {
    const problems = findUnclassifiedCommands(view, JUSTFILE_PATH, new Set(["run_with_timeout"])).problems.join("\n");
    assert.equal(problems.includes("brink-format:"), false, problems);
    assert.equal(problems.includes("brink-syntax:"), false, problems);
  });
});

describe("the REAL benchmarks/setup.sh (#2677)", () => {
  it("is discovered — the script that sat one directory outside the scripts/ walk", () => {
    assert.equal(
      discoverShellSources().some((source) => source.path === BENCHMARKS_SETUP_PATH),
      true,
    );
  });

  it("has no unbounded fetch as committed", () => {
    assert.deepEqual(findUnboundedFetches(realBenchmarksSetup, BENCHMARKS_SETUP_PATH).problems, []);
  });

  // NON-VACUITY for the second newly-reached file.
  it("detects its three installs once their bounds are stripped", () => {
    const unwrapped = stripBounds(realBenchmarksSetup);
    assert.notEqual(unwrapped, realBenchmarksSetup);

    const ids = new Set(findUnboundedFetches(unwrapped, BENCHMARKS_SETUP_PATH).findings.map((f) => f.id));
    for (const id of ["cargo-network", "npm", "homebrew"]) {
      assert.equal(ids.has(id), true, `expected a "${id}" finding; saw ${JSON.stringify([...ids])}`);
    }
  });
});

describe("toLogicalLines — the multi-line array literal (#2677)", () => {
  it("joins across an unclosed `(`", () => {
    const lines = toLogicalLines('targets=(\n    "a:b"\n    "c:d"\n)\necho done');
    assert.equal(lines.length, 2);
    assert.equal(lines[0].line, 1);
    assert.match(lines[0].text, /targets=\(\s+"a:b"\s+"c:d"\s+\)/);
    assert.match(lines[1].text, /echo done/);
  });

  it("does not treat a `case` label's lone `)` as a negative depth", () => {
    const lines = toLogicalLines("case $x in\n  *) exit 0 ;;\nesac\necho after");
    assert.equal(lines.length, 4);
    assert.match(lines[3].text, /echo after/);
  });

  it("does not join on a `(` inside quotes", () => {
    const lines = toLogicalLines('echo "a ( b"\necho next');
    assert.equal(lines.length, 2);
  });
});

describe("commandHead — an assignment whose quoted value continues (#2677)", () => {
  it('invokes nothing for `pkgs="-p a -p b"`', () => {
    assert.equal(commandHead('pkgs="-p brink-runtime -p brink-compiler"'), "");
  });

  it("still peels a BALANCED env prefix onto the command it wraps", () => {
    assert.equal(commandHead('FOO="bar" curl https://example.test'), "curl");
  });

  it("still peels an unquoted env prefix", () => {
    assert.equal(commandHead("FOO=bar curl https://example.test"), "curl");
  });
});

describe("process substitution (#2677 gap 3)", () => {
  it("flags a fetch reached through `< <( … )` behind a read builtin", () => {
    const result = findUnboundedFetches('read -r v < <(curl -sSfL "https://example.test/x")');
    assert.equal(result.ok, false);
    assert.equal(
      result.findings.some((finding) => finding.id === "curl"),
      true,
      JSON.stringify(result.findings),
    );
  });

  it("accepts the same shape once bounded", () => {
    assert.deepEqual(
      findUnboundedFetches('read -r v < <(run_with_timeout 10 curl -sSfL "https://example.test/x")').problems,
      [],
    );
  });

  it("still skips an echo that merely NAMES a tool with no substitution at all", () => {
    assert.deepEqual(findUnboundedFetches('echo "install curl first"').problems, []);
  });
});

describe("allow-unbounded waivers (#2677)", () => {
  const reason = "x".repeat(MIN_WAIVER_REASON);

  it("resolves a pragma to the next non-blank, non-comment line", () => {
    const waivers = findWaivers(`# check-scripts: allow-unbounded ${reason}\n\n# noise\npnpm dev`);
    assert.equal(waivers.length, 1);
    assert.equal(waivers[0].target, 4);
    assert.equal(waivers[0].reason, reason);
  });

  it("suppresses the finding on the waived line", () => {
    assert.deepEqual(findUnboundedFetches(`# check-scripts: allow-unbounded ${reason}\npnpm dev`).problems, []);
  });

  it("does NOT suppress the line after the waived one", () => {
    const result = findUnboundedFetches(`# check-scripts: allow-unbounded ${reason}\npnpm dev\npnpm build`);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /pnpm build/);
  });

  it("rejects a waiver whose reason is too short to be a reason", () => {
    const result = findUnboundedFetches("# check-scripts: allow-unbounded ok\npnpm dev");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /at least \d+ characters/);
  });

  it("reports a STALE waiver whose command is bounded now", () => {
    const result = findUnboundedFetches(`# check-scripts: allow-unbounded ${reason}\nrun_with_timeout 10 pnpm dev`);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /stale/);
  });

  it("the real justfile carries exactly one waiver, and it is load-bearing", () => {
    const view = justfileShellView(realJustfile);
    const waivers = findWaivers(view);
    assert.equal(waivers.length, 1, JSON.stringify(waivers));
    assert.equal(waivers[0].reason.length >= MIN_WAIVER_REASON, true);

    // Load-bearing: delete it and the scan must go red on that very line.
    const withoutWaiver = view
      .split("\n")
      .map((line) => (/check-scripts: allow-unbounded/.test(line) ? "" : line))
      .join("\n");
    const result = findUnboundedFetches(withoutWaiver, JUSTFILE_PATH);
    assert.equal(result.ok, false);
    assert.equal(
      result.findings.some((finding) => finding.line === waivers[0].target),
      true,
      JSON.stringify(result.findings),
    );
  });
});

describe("checkKnobTable, generalized by (script, prefix) — #2678", () => {
  it("registers refresh-excluded-lockfiles.sh's BRINK_REFRESH_ table — the #2678 gap", () => {
    assert.equal(
      KNOB_TABLES.some((entry) => entry.path === REFRESH_LOCKFILES_PATH && entry.prefix === "BRINK_REFRESH_"),
      true,
      JSON.stringify(KNOB_TABLES),
    );
  });

  // NON-VACUITY for #2678: before this, `checkKnobTable(realRefreshLockfiles)`
  // parsed ZERO rows and ZERO assignments (both regexes said BRINK_SETUP_), so
  // it could not have caught a drift in that file at all.
  it("parses the BRINK_REFRESH_ table that used to be invisible", () => {
    const rows = parseKnobTable(realRefreshLockfiles, "BRINK_REFRESH_");
    assert.deepEqual(
      rows.map((row) => row.name),
      ["BRINK_REFRESH_DRY_RUN_TIMEOUT", "BRINK_REFRESH_UPDATE_TIMEOUT"],
    );
    assert.equal(parseKnobTable(realRefreshLockfiles).length, 0, "the old BRINK_SETUP_ default finds nothing here");
    assert.equal(findKnobAssignments(realRefreshLockfiles).length, 0, "the old BRINK_SETUP_ default finds nothing here");
  });

  it("catches a drifted default in EVERY registered table, not just setup-dev.sh's", () => {
    const sources = new Map(discoverShellSources().map((source) => [source.path, source.text]));

    for (const entry of KNOB_TABLES) {
      const text = sources.get(entry.path);
      assert.ok(text, `${entry.path} must be a scanned source`);

      const assignments = findKnobAssignments(text, entry.prefix);
      assert.equal(assignments.length > 0, true, `${entry.path} must assign at least one ${entry.prefix}* knob`);
      assert.deepEqual(checkKnobTable(text, entry).problems, [], `${entry.path} must be green as committed`);

      const victim = assignments[0];
      const drifted = text.replace(`${victim.name}:-${victim.default}}`, `${victim.name}:-${victim.default + 7}}`);
      assert.notEqual(drifted, text, `failed to drift ${victim.name} in ${entry.path}`);

      const result = checkKnobTable(drifted, entry);
      assert.equal(result.ok, false, `drifting ${victim.name} in ${entry.path} must go red`);
      assert.match(result.problems.join("\n"), new RegExp(`${victim.name}.*${victim.default + 7}s`, "s"));
    }
  });

  it("catches a deleted row in EVERY registered table", () => {
    const sources = new Map(discoverShellSources().map((source) => [source.path, source.text]));

    for (const entry of KNOB_TABLES) {
      const text = sources.get(entry.path);
      const rows = parseKnobTable(text, entry.prefix);
      assert.equal(rows.length > 0, true, `${entry.path} must have a parseable table`);

      const lines = text.split("\n");
      const victim = rows[0];
      let end = victim.line;
      while (end < lines.length && new RegExp(`^#\\s{2,}(?!${entry.prefix})\\S`).test(lines[end])) end += 1;
      const withoutRow = [...lines.slice(0, victim.line - 1), ...lines.slice(end)].join("\n");

      const result = checkKnobTable(withoutRow, entry);
      assert.equal(result.ok, false, `deleting ${victim.name}'s row in ${entry.path} must go red`);
      assert.match(result.problems.join("\n"), new RegExp(victim.name));
    }
  });

  it("keeps checkDocPointers scoped to setup-dev.sh — only its table has delegating docs", () => {
    assert.deepEqual(
      POINTER_DOCS.map((doc) => doc.path),
      ["CLAUDE.md", "docs/desktop-shell-spec.md", "docs/releasing.md"],
    );
  });
});

describe("checkPackageScriptPath — the #2688 gap 2 self-consistency check", () => {
  it("passes when the named script exists", () => {
    const result = checkPackageScriptPath(
      JSON.stringify({ scripts: { "check:scripts": "node scripts/check-scripts.mjs" } }),
    );
    assert.deepEqual(result.problems, []);
    assert.equal(result.ok, true);
  });

  it("goes red when the named script does not exist on disk — the #2681-style drift", () => {
    const result = checkPackageScriptPath(
      JSON.stringify({ scripts: { "check:scripts": "node scripts/does-not-exist.mjs" } }),
    );
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /does-not-exist\.mjs.*does not exist on disk/s);
  });

  it("goes red when package.json is not valid JSON", () => {
    const result = checkPackageScriptPath("{ not json");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /not valid JSON/);
  });

  it("goes red when the script is missing entirely", () => {
    const result = checkPackageScriptPath(JSON.stringify({ scripts: {} }));
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /has no "check:scripts" script/);
  });

  it("goes red on a shape it does not recognise, rather than silently passing", () => {
    const result = checkPackageScriptPath(
      JSON.stringify({ scripts: { "check:scripts": "node scripts/check-scripts.mjs --strict" } }),
    );
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /not a bare `node <path>` \/ `bash <path>` invocation/);
  });

  it("respects a custom scriptName option", () => {
    const result = checkPackageScriptPath(JSON.stringify({ scripts: { "check:pnpm-pin": "node scripts/check-pnpm-pin.mjs" } }), {
      scriptName: "check:pnpm-pin",
    });
    assert.equal(result.ok, true);
  });
});

describe("checkPackageScriptPath — the REAL root package.json", () => {
  const realPackageJson = readFileSync(join(REPO_ROOT, PACKAGE_JSON_PATH), "utf8");

  it("passes today", () => {
    const result = checkPackageScriptPath(realPackageJson);
    assert.deepEqual(result.problems, []);
    assert.equal(result.ok, true);
  });

  it("names the real script this repo currently uses", () => {
    const parsed = JSON.parse(realPackageJson);
    assert.equal(parsed.scripts[CHECK_SCRIPTS_NPM_SCRIPT], "node scripts/check-scripts.mjs");
  });

  // Non-vacuity, the #2688 house rule ("make it fail before you make it
  // pass"): a real desync — package.json still pointing at the PRE-#2681
  // filename — must be caught, not just a synthetic fixture shaped to suit
  // the check.
  it("goes red on the real #2681 desync shape, reproduced", () => {
    const desynced = realPackageJson.replace(
      '"check:scripts": "node scripts/check-scripts.mjs"',
      '"check:scripts": "node scripts/check-setup-dev.mjs"',
    );
    assert.notEqual(desynced, realPackageJson, "the real package.json must still name check-scripts.mjs");

    const result = checkPackageScriptPath(desynced);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /check-setup-dev\.mjs.*does not exist on disk/s);
  });

  it("discovers every node/bash <path> sibling, not just check:scripts (#2702 review)", () => {
    const names = discoverNodeOrBashScriptNames(realPackageJson);
    for (const expected of [
      "check:pnpm-pin",
      "check:scripts",
      "check:wasm-pkg",
      "install:checked",
      "test:setup-dev",
      "test:refresh-lockfiles",
    ]) {
      assert.equal(names.includes(expected), true, `expected ${expected} in ${JSON.stringify(names)}`);
    }
    // Sorted, for deterministic output — this module's own discipline.
    assert.deepEqual([...names].sort(), names);
  });

  it("skips scripts.json entries in a shape this check cannot resolve (e.g. `vitest run`)", () => {
    const names = discoverNodeOrBashScriptNames(
      JSON.stringify({ scripts: { test: "vitest run", "check:scripts": "node scripts/check-scripts.mjs" } }),
    );
    assert.deepEqual(names, ["check:scripts"]);
  });

  // Non-vacuity for the sibling enumeration gap itself (#2702 review): a
  // renamed `test:refresh-lockfiles` — the exact scenario named in the
  // review, since this PR wires that script into ci.yml by name — must be
  // caught by checkPackageScriptPath once discovered, not silently passed
  // because only "check:scripts" was ever hardcoded.
  it("catches a renamed test:refresh-lockfiles sibling once discovered", () => {
    const desynced = realPackageJson.replace(
      '"test:refresh-lockfiles": "bash scripts/refresh-excluded-lockfiles.test.sh"',
      '"test:refresh-lockfiles": "bash scripts/does-not-exist.test.sh"',
    );
    assert.notEqual(
      desynced,
      realPackageJson,
      "the real package.json must still name refresh-excluded-lockfiles.test.sh",
    );

    const names = discoverNodeOrBashScriptNames(desynced);
    assert.equal(names.includes("test:refresh-lockfiles"), true);

    const result = checkPackageScriptPath(desynced, { scriptName: "test:refresh-lockfiles" });
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /does-not-exist\.test\.sh.*does not exist on disk/s);
  });

  it("is wired into checkScripts() end-to-end", () => {
    // Same non-vacuity proof, through the aggregate entry point rather than
    // the unit function, over a REAL temp checkout so `checkScripts`'s own
    // `readFileSync(join(repoRoot, PACKAGE_JSON_PATH), ...)` reads the
    // planted drift rather than this repo's real, healthy package.json.
    const tmpRoot = mkdtempSync(join(tmpdir(), "check-scripts-pkg-"));
    try {
      mkdirSync(join(tmpRoot, "scripts"), { recursive: true });
      writeFileSync(
        join(tmpRoot, "package.json"),
        JSON.stringify({ scripts: { "check:scripts": "node scripts/does-not-exist.mjs" } }),
      );
      // checkScripts() also requires setup-dev.sh + the three POINTER_DOCS
      // to exist under repoRoot; stub them minimally so the ONLY problem
      // produced is the package.json one this test cares about.
      writeFileSync(join(tmpRoot, "scripts", "setup-dev.sh"), "#!/usr/bin/env bash\n");
      for (const doc of POINTER_DOCS) {
        mkdirSync(join(tmpRoot, doc.path, ".."), { recursive: true });
        writeFileSync(join(tmpRoot, doc.path), "no pointer here");
      }

      const result = checkScripts({ repoRoot: tmpRoot });
      assert.equal(result.ok, false);
      assert.match(
        result.problems.join("\n"),
        /does-not-exist\.mjs.*does not exist on disk/s,
        `expected checkScripts() to surface the planted package.json drift; problems: ${JSON.stringify(result.problems)}`,
      );
    } finally {
      rmSync(tmpRoot, { recursive: true, force: true });
    }
  });

  it("catches a sibling drift through checkScripts() end-to-end, not only check:scripts (#2702 review)", () => {
    // Same shape as the test above, but the PLANTED drift is on a sibling
    // ("test:refresh-lockfiles") while "check:scripts" itself stays healthy
    // — proving checkScripts() enumerates every node/bash <path> script
    // rather than only ever looking at the one hardcoded name.
    const tmpRoot = mkdtempSync(join(tmpdir(), "check-scripts-pkg-sibling-"));
    try {
      mkdirSync(join(tmpRoot, "scripts"), { recursive: true });
      writeFileSync(join(tmpRoot, "scripts", "check-scripts.mjs"), "// stub\n");
      writeFileSync(
        join(tmpRoot, "package.json"),
        JSON.stringify({
          scripts: {
            "check:scripts": "node scripts/check-scripts.mjs",
            "test:refresh-lockfiles": "bash scripts/does-not-exist.test.sh",
          },
        }),
      );
      writeFileSync(join(tmpRoot, "scripts", "setup-dev.sh"), "#!/usr/bin/env bash\n");
      for (const doc of POINTER_DOCS) {
        mkdirSync(join(tmpRoot, doc.path, ".."), { recursive: true });
        writeFileSync(join(tmpRoot, doc.path), "no pointer here");
      }

      const result = checkScripts({ repoRoot: tmpRoot });
      assert.equal(result.ok, false);
      assert.match(
        result.problems.join("\n"),
        /test:refresh-lockfiles.*does-not-exist\.test\.sh.*does not exist on disk/s,
        `expected checkScripts() to surface the planted sibling drift; problems: ${JSON.stringify(result.problems)}`,
      );
    } finally {
      rmSync(tmpRoot, { recursive: true, force: true });
    }
  });
});

describe("stripJsComments (#2697)", () => {
  it("blanks a line comment but keeps the newline", () => {
    const out = stripJsComments('const x = 1; // timeout: 5000\nconst y = 2;');
    assert.equal(out.includes("timeout"), false);
    assert.equal(out.split("\n").length, 2);
  });

  it("blanks a block comment, preserving embedded newlines for line counting", () => {
    const out = stripJsComments("const x = 1;\n/* timeout:\n   5000 */\nconst y = 2;");
    assert.equal(out.includes("timeout"), false);
    assert.equal(out.split("\n").length, 4);
  });

  it("leaves // and /* inside string/template literals alone", () => {
    const out = stripJsComments('const url = "https://example.com"; const s = `a/*b`;');
    assert.match(out, /https:\/\/example\.com/);
    assert.match(out, /a\/\*b/);
  });
});

describe("extractBalancedArgs (#2697)", () => {
  it("extracts a simple call's args", () => {
    const text = 'execSync("cmd", { timeout: 5000 })';
    const call = extractBalancedArgs(text, text.indexOf("("));
    assert.equal(call.args, '"cmd", { timeout: 5000 }');
  });

  it("does not unbalance on a paren inside a string argument", () => {
    const text = 'execSync("echo (hi)", { timeout: 5000 })';
    const call = extractBalancedArgs(text, text.indexOf("("));
    assert.equal(call.args, '"echo (hi)", { timeout: 5000 }');
  });

  it("returns null for an unclosed call", () => {
    const text = 'execSync("cmd", { timeout: 5000 }';
    assert.equal(extractBalancedArgs(text, text.indexOf("(")), null);
  });
});

describe("findUnboundedExecCalls — planted red and green, one per shape (#2697)", () => {
  it("reports a bare execSync call with no options at all", () => {
    const result = findUnboundedExecCalls('execSync("cargo build --release");', "fixture.mjs");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /execSync.*no `timeout` option/);
  });

  it("reports execSync with an options object that omits timeout", () => {
    const result = findUnboundedExecCalls(
      'execSync("cargo build --release", { cwd: repoRoot, stdio: "inherit" });',
      "fixture.mjs",
    );
    assert.equal(result.ok, false);
  });

  it("reports spawnSync the same way execSync is reported", () => {
    const result = findUnboundedExecCalls('spawnSync("wasm-pack", ["build"]);', "fixture.mjs");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /spawnSync/);
  });

  it("passes when the call's own args carry a literal timeout key", () => {
    const result = findUnboundedExecCalls(
      'execSync("cargo build --release", { cwd: repoRoot, timeout: 1200000, stdio: "inherit" });',
      "fixture.mjs",
    );
    assert.deepEqual(result.problems, []);
    assert.equal(result.ok, true);
  });

  it("passes when timeout is set before a spread — the real defaultRunCommand shape", () => {
    const result = findUnboundedExecCalls(
      'function defaultRunCommand(command, options = {}) {\n' +
        "  return execSync(command, { encoding: 'utf8', timeout: DEFAULT_EXEC_TIMEOUT_MS, ...options });\n" +
        "}",
      "fixture.mjs",
    );
    assert.deepEqual(result.problems, []);
  });

  // The #2689 house rule: prove the heuristic does not cry wolf on
  // legitimate output — here, a comment that merely MENTIONS "timeout:"
  // must not satisfy the check. Comments are stripped before scanning
  // specifically so this cannot happen.
  it("does NOT treat a comment mentioning timeout as bounding the call", () => {
    const result = findUnboundedExecCalls(
      'execSync("cargo build --release", {\n' +
        "  // no timeout: set intentionally, revisit later\n" +
        '  stdio: "inherit",\n' +
        "});",
      "fixture.mjs",
    );
    assert.equal(result.ok, false);
  });

  it("reports the real line number, comments and all", () => {
    const text = '// header comment\n\nfunction f() {\n  execSync("cmd");\n}\n';
    const result = findUnboundedExecCalls(text, "fixture.mjs");
    assert.equal(result.findings[0].line, 4);
  });

  it("is non-vacuous: a healthy fixture with two bounded calls reports nothing", () => {
    const result = findUnboundedExecCalls(
      'execSync("a", { timeout: 1000 });\nspawnSync("b", { timeout: 2000 });',
      "fixture.mjs",
    );
    assert.deepEqual(result.problems, []);
  });
});

describe("EXEC_CALL_NAMES / discoverPackageScriptSources (#2697)", () => {
  it("names all six node:child_process spawn APIs (#2702 review)", () => {
    assert.deepEqual(
      [...EXEC_CALL_NAMES].sort(),
      ["exec", "execFile", "execFileSync", "execSync", "spawn", "spawnSync"],
    );
  });

  it("catches an unbounded execFileSync call (previously invisible — #2702 review)", () => {
    const result = findUnboundedExecCalls('execFileSync("cargo", ["build"]);', "fixture.mjs");
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /execFileSync/);
  });

  it("catches unbounded async exec/spawn calls (previously invisible — #2702 review)", () => {
    const result = findUnboundedExecCalls('exec("a");\nspawn("b");', "fixture.mjs");
    assert.equal(result.ok, false);
    assert.equal(result.findings.length, 2);
  });

  it("discovers the real packages/*/scripts/*.mjs sources, non-recursively", () => {
    const sources = discoverPackageScriptSources();
    const paths = sources.map((s) => s.path);
    assert.equal(paths.includes("packages/brink-desktop/scripts/ensure-wasm.mjs"), true, paths.join(", "));
    assert.equal(paths.includes("packages/brink-desktop/scripts/ensure-cli-sidecar.mjs"), true, paths.join(", "));
    // Sorted, for deterministic output.
    assert.deepEqual([...paths].sort(), paths);
  });

  it("is silent (empty) for a repoRoot with no packages/ directory", () => {
    const tmpRoot = mkdtempSync(join(tmpdir(), "check-scripts-nopkgs-"));
    try {
      assert.deepEqual(discoverPackageScriptSources(tmpRoot), []);
    } finally {
      rmSync(tmpRoot, { recursive: true, force: true });
    }
  });
});

describe("findUnboundedExecCalls — the REAL packages/*/scripts/*.mjs (#2697)", () => {
  const realSources = discoverPackageScriptSources();

  it("passes on the real tree today", () => {
    for (const source of realSources) {
      const result = findUnboundedExecCalls(source.text, source.path);
      assert.deepEqual(result.problems, [], `${source.path}: ${JSON.stringify(result.problems)}`);
    }
  });

  it("is non-vacuous: stripping the real timeout out of ensure-wasm.mjs goes red", () => {
    const source = realSources.find((s) => s.path.endsWith("ensure-wasm.mjs"));
    assert.notEqual(source, undefined, "expected to discover ensure-wasm.mjs");

    const stripped = source.text.replace(/timeout:\s*DEFAULT_EXEC_TIMEOUT_MS,\s*/, "");
    assert.notEqual(stripped, source.text, "the real file must still carry the literal timeout default");

    const result = findUnboundedExecCalls(stripped, source.path);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /execSync/);
  });

  it("is non-vacuous: stripping the real timeout out of ensure-cli-sidecar.mjs goes red", () => {
    const source = realSources.find((s) => s.path.endsWith("ensure-cli-sidecar.mjs"));
    assert.notEqual(source, undefined, "expected to discover ensure-cli-sidecar.mjs");

    const stripped = source.text.replace(/timeout:\s*DEFAULT_EXEC_TIMEOUT_MS,\s*/, "");
    assert.notEqual(stripped, source.text, "the real file must still carry the literal timeout default");

    const result = findUnboundedExecCalls(stripped, source.path);
    assert.equal(result.ok, false);
    assert.match(result.problems.join("\n"), /execSync/);
  });

  it("is wired into checkScripts() end-to-end", () => {
    const result = checkScripts();
    assert.equal(result.ok, true);
    assert.equal(result.packageScripts.includes("packages/brink-desktop/scripts/ensure-wasm.mjs"), true);
  });
});

describe("findUnregisteredKnobTables — the backstop under the registry (#2678)", () => {
  it("is silent on the real repo", () => {
    assert.deepEqual(findUnregisteredKnobTables(discoverShellSources()), []);
  });

  it("reports a knob table in a script nobody registered", () => {
    const stray = findUnregisteredKnobTables([
      { path: "scripts/brand-new.sh", text: 'BRINK_NEWTHING_FETCH_TIMEOUT="${BRINK_NEWTHING_FETCH_TIMEOUT:-30}"' },
    ]);
    assert.equal(stray.length, 1);
    assert.equal(stray[0].name, "BRINK_NEWTHING_FETCH_TIMEOUT");
  });

  it("does not report a knob a registered prefix already covers", () => {
    assert.deepEqual(
      findUnregisteredKnobTables([
        { path: SETUP_DEV_PATH, text: 'BRINK_SETUP_X_TIMEOUT="${BRINK_SETUP_X_TIMEOUT:-30}"' },
      ]),
      [],
    );
  });

  it("goes red end-to-end: an unregistered knob planted in a real script", () => {
    const planted = `${realBenchmarksSetup}\nBRINK_OTHER_X_TIMEOUT="\${BRINK_OTHER_X_TIMEOUT:-5}"\n`;
    const stray = findUnregisteredKnobTables([{ path: BENCHMARKS_SETUP_PATH, text: planted }]);
    assert.equal(stray.length, 1);
    assert.equal(stray[0].name, "BRINK_OTHER_X_TIMEOUT");
  });
});
