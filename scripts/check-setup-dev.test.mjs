// Tests for scripts/check-setup-dev.mjs (#2648, #2647). Node's built-in test
// runner, matching check-pnpm-pin.test.mjs / check-wasm-pkg.test.mjs /
// guarded-install.test.mjs: this file runs under `pnpm test:scripts`, which
// CI's `frontend` job executes BEFORE `pnpm install`, so it must not depend on
// anything installed.
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
  NETWORK_COMMANDS,
  POINTER_DOCS,
  REPO_ROOT,
  SETUP_DEV_PATH,
  checkDocPointers,
  checkKnobTable,
  checkSetupDev,
  findKnobAssignments,
  findUnboundedFetches,
  findUnrecognizedKnobShapes,
  nextTokenIsVersionFlag,
  parseKnobTable,
  sliceSection,
  toLogicalLines,
} from "./check-setup-dev.mjs";

const realSetupDev = readFileSync(join(REPO_ROOT, SETUP_DEV_PATH), "utf8");

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

  // Honesty check on the header's stated limitation, asserted rather than
  // merely claimed: a TRAILING comment is not stripped, so it is a known
  // false-positive source. If this ever starts passing, the header note is
  // stale and must be updated.
  it("DOES flag a trailing comment mentioning curl — a stated false positive", () => {
    assert.equal(findUnboundedFetches("FOO=1 # fetched with curl elsewhere").ok, false);
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
    const result = checkSetupDev();
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
