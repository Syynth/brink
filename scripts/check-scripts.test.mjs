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

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  LOCAL_COMMANDS,
  NETWORK_COMMANDS,
  POINTER_DOCS,
  REPO_ROOT,
  SETUP_DEV_PATH,
  checkDocPointers,
  checkKnobTable,
  checkScripts,
  commandHead,
  discoverShellScripts,
  findFunctionNames,
  findKnobAssignments,
  findUnboundedFetches,
  findUnclassifiedCommands,
  findUnrecognizedKnobShapes,
  networkBinaries,
  nextTokenIsVersionFlag,
  parseKnobTable,
  sliceSection,
  splitSegmentsQuoteAware,
  toLogicalLines,
} from "./check-scripts.mjs";

const realSetupDev = readFileSync(join(REPO_ROOT, SETUP_DEV_PATH), "utf8");
const REFRESH_LOCKFILES_PATH = "scripts/refresh-excluded-lockfiles.sh";
const realRefreshLockfiles = readFileSync(join(REPO_ROOT, REFRESH_LOCKFILES_PATH), "utf8");

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
    assert.match(result.problems.join("\n"), /no parseable knob table/);
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

  it("returns only .sh files", () => {
    for (const path of scripts) assert.match(path, /\.sh$/);
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
