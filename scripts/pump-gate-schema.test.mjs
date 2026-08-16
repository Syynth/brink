// Sanity check for #2645: `.claude/skills/autonomous-pump/pump.js`'s BUILD
// schema replaces free-text `gateOutput` with a `gateResults` array whose
// `minItems` is a compile-time constant derived from the item's own gate
// string (split on `&&`). This file does NOT execute pump.js as a whole — it
// is a Workflow-tool script that expects an injected agent-harness (`agent`,
// `pipeline`, `phase`, `log`) and is never `require`/`import`-able on its
// own. Instead it extracts the self-contained
// GATE_SCHEMA_HELPERS_START..._END block (gateFor through
// formatGateEvidence) by marker and evaluates THAT, so this test exercises
// the actual shipped source, not a hand-copied duplicate of it.
//
// This proves two things a review can't tell from reading the file:
//   1. The extracted block itself parses and runs (catches a marker/brace
//      slip that `node --check` on the whole file would not, since
//      `node --check` only proves the FULL file is syntactically valid).
//   2. A hand-written valid BUILD object actually validates against the
//      generated schema, and a short `gateResults` array is actually
//      rejected — i.e. minItems does what #2645 says it does.
//
// It runs under `pnpm test:scripts` (`node --test scripts/*.test.mjs`), the
// same command CLAUDE.md's autonomous-pump gate requires.
//
// ── WHICH HALF THIS FILE CAN CHECK, AND WHICH IT CANNOT (#2665) ──────────────
// The enforcement story has TWO halves and only ONE of them is reachable from
// in-tree tests. Saying so explicitly is the whole point of #2665, because
// three rounds of work (#2612 -> #2645 -> #2657) rested on the second half
// being ASSUMED.
//
//   IN-TREE (this file checks it, `pnpm test:scripts` goes red if it breaks):
//     that pump.js GENERATES a schema carrying real, enforceable constraints —
//     `gateResults.minItems` equal to this item's own gate command count,
//     `minLength` on the strings, `required` on the rows — that the build
//     agent's call site actually passes that schema, and that a hand-rolled
//     checker over the same vocabulary rejects a short array.
//
//   NOT IN-TREE (no test here can reach it): that the AGENT HARNESS actually
//     compiles that schema and REJECTS a tool call violating it. That is the
//     harness's behaviour, not this repo's code — there is no harness to
//     import, and the schema alone is inert without a validator that honours
//     it. It was verified ONCE by direct probe; the evidence lives in
//     `.claude/skills/autonomous-pump/SKILL.md` (see "Harness enforcement of
//     the build schema"), and the last test in this file asserts that record
//     has not been deleted — which is a check on the RECORD, not on the
//     harness.
//
// So: if someone deletes `minItems` from pump.js, this file catches it. If a
// future harness release stops honouring `minItems`, NOTHING here catches it —
// the only signal would be builds shipping short `gateResults` arrays again,
// which `formatGateEvidence`'s INCOMPLETE banner surfaces to the reviewer.
//
// ── LATER ROUNDS ON THE SAME AREA ───────────────────────────────────────────
// #2672 — the schema pins a FLOOR and deliberately NO ceiling, and the banner
//   is direction-aware so an over-long array reads OVER-COMPLETE rather than
//   "INCOMPLETE". The ruling and its reasoning live on `gateResultsSchema` in
//   pump.js; the tests at the bottom of this file are what stop it eroding.
// #2664 — the MERGE/fix phase carries the same `gateResults` array, since it
//   re-runs the same gate on the commit that actually lands on main.
//
// ⚠ NEITHER ROUND CHANGES THE BOUNDARY ABOVE, and neither strengthens what any
// of this buys. A FABRICATED row ("36 passed" for a command never run) still
// validates — at BUILD and now at MERGE alike. Only a MISSING command is
// mechanically impossible. And no test in this repo can detect a future
// harness release that stops honouring `minItems` or `minLength` at either
// phase; that half was probed once (#2665) and written down, never automated.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PUMP_JS = join(__dirname, "..", ".claude", "skills", "autonomous-pump", "pump.js");
const SKILL_MD = join(__dirname, "..", ".claude", "skills", "autonomous-pump", "SKILL.md");

function extractBlock(source, startMarker, endMarker) {
  const startMarkerPos = source.indexOf(startMarker);
  const endMarkerPos = source.indexOf(endMarker);
  assert.ok(startMarkerPos !== -1, `missing ${startMarker} in pump.js`);
  assert.ok(endMarkerPos !== -1, `missing ${endMarker} in pump.js`);
  assert.ok(endMarkerPos > startMarkerPos, `${endMarker} appears before ${startMarker}`);
  // Both markers live on their own `// MARKER — comment...` lines. Take the
  // code strictly BETWEEN those two comment lines, not the marker lines
  // themselves — including "GATE_SCHEMA_HELPERS_START — kept..." verbatim
  // would drop its leading `//` and produce invalid JS.
  const start = source.indexOf("\n", startMarkerPos) + 1;
  const end = source.lastIndexOf("\n", endMarkerPos);
  return source.slice(start, end);
}

function extractConst(source, name) {
  // Matches both `const NAME = "...";` and the two-line
  // `const NAME =\n  "...";` style pump.js uses for long strings.
  const re = new RegExp(`const ${name} =\\s*\\n?\\s*"((?:[^"\\\\]|\\\\.)*)"`);
  const m = source.match(re);
  assert.ok(m, `could not extract const ${name} from pump.js`);
  return m[1];
}

const source = readFileSync(PUMP_JS, "utf8");
const cache = extractConst(source, "CACHE");
const gate = extractConst(source, "GATE");
const helpersSrc = extractBlock(source, "GATE_SCHEMA_HELPERS_START", "GATE_SCHEMA_HELPERS_END");

// Evaluate the extracted block with CACHE/GATE injected as the real pump.js
// values, exposing exactly the functions it defines — nothing hand-copied.
const helpers = new Function(
  "CACHE",
  "GATE",
  `${helpersSrc}\nreturn { gateFor, gateCmds, buildSchemaFor, mergeSchemaFor, formatGateEvidence, auditGateEvidence, GATE_ROWS_CAP, GATE_CMD_CAP };`,
)(cache, gate);

// Minimal hand-rolled validator covering only the JSON-Schema vocabulary
// buildSchemaFor actually uses (type/additionalProperties/required/
// properties/items/minLength/minItems) — no external dependency (ajv is not
// installed in this repo, and this runs before `pnpm install` in some CI
// lanes per check-pnpm-pin.test.mjs's own header note).
function validate(schema, value, path = "$") {
  const errors = [];
  if (schema.type === "object") {
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      errors.push(`${path}: expected object`);
      return errors;
    }
    for (const key of schema.required ?? []) {
      if (!(key in value)) errors.push(`${path}: missing required property "${key}"`);
    }
    if (schema.additionalProperties === false) {
      for (const key of Object.keys(value)) {
        if (!(key in (schema.properties ?? {}))) errors.push(`${path}: unexpected property "${key}"`);
      }
    }
    for (const [key, subSchema] of Object.entries(schema.properties ?? {})) {
      if (key in value) errors.push(...validate(subSchema, value[key], `${path}.${key}`));
    }
  } else if (schema.type === "array") {
    if (!Array.isArray(value)) {
      errors.push(`${path}: expected array`);
      return errors;
    }
    if (typeof schema.minItems === "number" && value.length < schema.minItems) {
      errors.push(`${path}: has ${value.length} items, needs at least ${schema.minItems}`);
    }
    // Honoured here so the "no maxItems" ruling (#2672) is testable rather
    // than vacuously true: if someone adds `maxItems`, the over-long
    // gateResults case below starts producing an error and goes red.
    if (typeof schema.maxItems === "number" && value.length > schema.maxItems) {
      errors.push(`${path}: has ${value.length} items, allows at most ${schema.maxItems}`);
    }
    if (schema.items) {
      value.forEach((item, i) => errors.push(...validate(schema.items, item, `${path}[${i}]`)));
    }
  } else if (schema.type === "string") {
    if (typeof value !== "string") {
      errors.push(`${path}: expected string`);
    } else if (typeof schema.minLength === "number" && value.length < schema.minLength) {
      errors.push(`${path}: length ${value.length} below minLength ${schema.minLength}`);
    }
  } else if (schema.type === "boolean") {
    if (typeof value !== "boolean") errors.push(`${path}: expected boolean`);
  } else if (schema.type === "number") {
    if (typeof value !== "number") errors.push(`${path}: expected number`);
  }
  return errors;
}

describe("pump.js gate-evidence schema (#2645)", () => {
  it("gateFor/gateCmds/buildSchemaFor/formatGateEvidence extract and evaluate cleanly", () => {
    assert.equal(typeof helpers.gateFor, "function");
    assert.equal(typeof helpers.gateCmds, "function");
    assert.equal(typeof helpers.buildSchemaFor, "function");
    assert.equal(typeof helpers.formatGateEvidence, "function");
  });

  it("gateCmds splits the real repo default GATE into its actual && legs", () => {
    const cmds = helpers.gateCmds({});
    // CACHE contributes one leg, then each `&&`-joined command in GATE.
    const expectedLegCount = 1 + gate.split("&&").length;
    assert.equal(cmds.length, expectedLegCount);
    assert.ok(cmds[0].startsWith("export CARGO_TARGET_DIR"), "first leg should be the CACHE prefix");
  });

  it("buildSchemaFor's gateResults.minItems equals THIS item's own gate command count", () => {
    const twoCmdItem = { gate: "pnpm run a && pnpm run b" };
    const schema = helpers.buildSchemaFor(twoCmdItem);
    // CACHE (1) + the two `gate:` commands = 3.
    assert.equal(schema.properties.gateResults.minItems, helpers.gateCmds(twoCmdItem).length);
    assert.equal(schema.properties.gateResults.minItems, 3);
  });

  it("a hand-written COMPLETE build object validates against the generated schema", () => {
    const item = { n: 9999, gate: "pnpm run a && pnpm run b" };
    const schema = helpers.buildSchemaFor(item);
    const validObject = {
      ok: true,
      issue: 9999,
      pr: "https://github.com/Syynth/brink/pull/1",
      gateGreen: true,
      gateOutput: "PRE-FLIGHT: df -h / showed plenty of space; du of the cache was small.",
      gateResults: [
        { command: "export CARGO_TARGET_DIR=... (CACHE)", result: "exit 0, no output" },
        { command: "pnpm run a", result: "12 passed, 0 failed" },
        { command: "pnpm run b", result: "exit 0, clean" },
      ],
      reachability: "wired into the CLI's `foo` subcommand, exercised by `brink foo`",
      summary: "did the thing",
    };
    const errors = validate(schema, validObject);
    assert.deepEqual(errors, [], `expected no validation errors, got: ${errors.join("; ")}`);
  });

  it("a SHORT gateResults array (fewer rows than gate commands) is rejected — the whole point of #2645", () => {
    const item = { n: 9999, gate: "pnpm run a && pnpm run b" };
    const schema = helpers.buildSchemaFor(item);
    const truncatedObject = {
      ok: true,
      issue: 9999,
      pr: "https://github.com/Syynth/brink/pull/1",
      gateGreen: true,
      gateOutput: "All commands ran; a result line for each — I promise.",
      // Only 2 of 3 required rows (CACHE leg silently dropped), the exact
      // shape #2645 measured in w167/w168's real gateOutput strings.
      gateResults: [
        { command: "pnpm run a", result: "12 passed, 0 failed" },
        { command: "pnpm run b", result: "exit 0, clean" },
      ],
      reachability: "wired into the CLI's `foo` subcommand",
      summary: "did the thing",
    };
    const errors = validate(schema, truncatedObject);
    assert.ok(
      errors.some((e) => e.includes("gateResults") && e.includes("needs at least")),
      `expected a gateResults minItems error, got: ${JSON.stringify(errors)}`,
    );
  });

  it("a missing gateResults field is rejected by `required`", () => {
    const item = { n: 9999, rustOnly: true };
    const schema = helpers.buildSchemaFor(item);
    const noArrayObject = {
      ok: true,
      issue: 9999,
      pr: "https://github.com/Syynth/brink/pull/1",
      gateGreen: true,
      gateOutput: "Ran the gate, everything passed, all green across the board here.",
      reachability: "wired into the CLI",
      summary: "did the thing",
    };
    const errors = validate(schema, noArrayObject);
    assert.ok(
      errors.some((e) => e.includes('missing required property "gateResults"')),
      `expected a missing-gateResults error, got: ${JSON.stringify(errors)}`,
    );
  });

  it("the build-agent call site actually wires buildSchemaFor(b) into the tool-call options", () => {
    // The helpers block alone proves buildSchemaFor works in isolation; this
    // guards the OTHER half — that the real prompt-assembly code in pump.js
    // actually passes it to the agent() call, not just defines it unused.
    assert.ok(
      source.includes("schema: buildSchemaFor(b)"),
      "pump.js's build agent() call must pass `schema: buildSchemaFor(b)`",
    );
  });

  it("the review prompt renders formatGateEvidence with this item's own expected command count", () => {
    // Guards the fix for the reviewer finding that formatGateEvidence(build)
    // (no `expected` arg) let a short gateResults array self-report as
    // complete (e.g. "[1/1]" for a 1-row array against a 5-command gate).
    // The call site must pass this item's own gate commands so a shortfall
    // renders a banner instead of a falsely-complete-looking fraction.
    // ⚠ #2686 widened `expected` from a COUNT to the command LIST (a number is
    // still accepted): the list is what lets `auditGateEvidence` check that
    // each command is actually covered, not merely counted. `gateCmds(b)`
    // carries its own length, so the shortfall banner is unaffected.
    assert.ok(
      source.includes("<gate-output>") && source.includes("${formatGateEvidence(build, gateCmds(b))}"),
      "the <gate-output> block in the review prompt must call formatGateEvidence(build, gateCmds(b))",
    );
  });

  it("the old single-block gateOutput truncation is gone from the prompt path", () => {
    // The pre-#2645 bug: a single `.slice(0, 2000)` over free-text gateOutput
    // silently dropped the 4th of 4 rows for PR #2642. Reverting the
    // call-site fix (while leaving buildSchemaFor/formatGateEvidence intact)
    // would still pass every other test in this file — this assertion is the
    // one that catches that specific silent revert.
    assert.ok(
      !source.includes('build.gateOutput ?? "(none returned)").slice('),
      "pump.js must not reintroduce the old single-block `.slice(0, 2000)` truncation on the prompt path",
    );
  });

  it("formatGateEvidence shows every command row even when the combined text would exceed the old 2000-char cap", () => {
    // Reproduces the #2645 root cause: PR #2642's build reported a COMPLETE
    // 4-row gateResults/gateOutput totalling 3928 chars, but the old
    // reviewer-prompt interpolation did `gateOutput.slice(0, 2000)`, which
    // cut the block off inside the 3rd of 4 rows and dropped the 4th
    // entirely (verified against wf_44e6f21c-603's journal for #2645).
    const longResult = "x".repeat(900); // longer than one GATE_ROW_CAP-worth
    const build = {
      gateGreen: true,
      gateOutput: "y".repeat(3000), // longer than the free-text notes cap
      gateResults: [
        { command: "step one", result: longResult },
        { command: "step two", result: longResult },
        { command: "step three", result: longResult },
        { command: "step four — the one that used to vanish", result: "PASS 4/4" },
      ],
    };
    const shown = helpers.formatGateEvidence(build);
    assert.ok(shown.includes("[4/4] step four"), "the 4th row's command must be visible");
    assert.ok(shown.includes("PASS 4/4"), "the 4th row's result must be visible");
  });

  it("formatGateEvidence prepends an INCOMPLETE banner when gateResults is shorter than the gate's own command count", () => {
    // Reviewer finding: a 1-row gateResults against a 5-command gate used to
    // render as "[1/1] cargo nextest run --workspace" — reading to the
    // reviewer as a COMPLETE one-command gate, strictly LESS detectable than
    // the free-text "[1/4]...[3/4]" convention it replaced. Passing the
    // item's own expected count must surface the shortfall explicitly.
    const build = {
      gateGreen: true,
      gateOutput: "z".repeat(100),
      gateResults: [{ command: "cargo nextest run --workspace", result: "36 passed" }],
    };
    const shown = helpers.formatGateEvidence(build, 5);
    assert.ok(
      shown.includes("1 gateResults rows returned for a 5-command gate") && shown.includes("INCOMPLETE"),
      `expected an INCOMPLETE banner naming 1 of 5, got: ${shown.slice(0, 200)}`,
    );
    assert.ok(shown.includes("[1/5] cargo nextest run --workspace"), "the single row must still render, denominated against the true 5");

    // The happy path (row count matches expected) must NOT show the banner.
    const completeBuild = {
      gateGreen: true,
      gateOutput: "z".repeat(100),
      gateResults: [{ command: "a", result: "ok" }],
    };
    const completeShown = helpers.formatGateEvidence(completeBuild, 1);
    assert.ok(!completeShown.includes("INCOMPLETE"), "a complete gateResults array must not show the INCOMPLETE banner");
  });
});

// ── #2665: the constraints must stay ENFORCEABLE, not decorative ─────────────
// #2645/#2657 pinned `minItems` to the gate's command count and #2612 added
// `minLength`. Both are load-bearing ONLY if (a) they are actually emitted and
// (b) they are spelled with keywords a JSON-Schema validator honours. The
// tests below cover exactly that — the schema pump.js hands the harness. They
// do NOT and cannot test the harness's own validator; see this file's header.
describe("pump.js build schema keeps enforceable constraints (#2665)", () => {
  // Every gate shape BRINK-CONFIG.md actually uses. `{}` is the default
  // (Rust) gate; the TS override is BRINK-CONFIG.md:10's real "TS entries"
  // gate string verbatim (7 pnpm/wasm-pack legs -> minItems 8 with CACHE).
  // The single-command override is a two-leg gate once CACHE is prepended —
  // no shape here yields minItems 1.
  const GATE_SHAPES = [
    { label: "default Rust gate", item: {} },
    {
      label: "TS override",
      item: {
        gate: "wasm-pack build crates/brink-web --target web --out-dir www/pkg && wasm-pack test --node crates/brink-web && pnpm install:checked -- --frozen-lockfile && pnpm --filter @brink-lang/editor typecheck && pnpm --filter @brink-lang/studio typecheck && pnpm --filter @brink-lang/studio test && pnpm --filter @brink-lang/editor build",
      },
    },
    { label: "single-command override (two legs once CACHE is prepended)", item: { gate: "pnpm run test:scripts" } },
  ];

  for (const { label, item } of GATE_SHAPES) {
    it(`emits a positive integer gateResults.minItems matching gateCmds for the ${label}`, () => {
      const cmds = helpers.gateCmds(item);
      const minItems = helpers.buildSchemaFor(item).properties.gateResults.minItems;
      assert.equal(typeof minItems, "number", "minItems must be a number, not a string or undefined");
      assert.ok(Number.isInteger(minItems) && minItems >= 1, `minItems must be a positive integer, got ${minItems}`);
      assert.equal(minItems, cmds.length, "minItems must equal this item's own gate command count");
    });
  }

  it("keeps a numeric minLength on every gate-evidence string, so an empty one cannot satisfy the schema", () => {
    // #2612's lesson: `required` alone let an EMPTY string through, which
    // reproduced the exact hole it was added to close. These three are the
    // strings a build could otherwise return blank.
    const props = helpers.buildSchemaFor({ gate: "pnpm run a && pnpm run b" }).properties;
    const row = props.gateResults.items.properties;
    for (const [name, sub] of [["gateOutput", props.gateOutput], ["gateResults.items.command", row.command], ["gateResults.items.result", row.result]]) {
      assert.equal(typeof sub.minLength, "number", `${name} must carry a numeric minLength`);
      assert.ok(sub.minLength > 0, `${name}'s minLength must be > 0, got ${sub.minLength}`);
    }
    assert.deepEqual(props.gateResults.items.required, ["command", "result"], "a row missing either half must be rejected by `required`");
  });

  it("uses only JSON-Schema keywords a standard validator enforces — a typo'd keyword would be silently ignored", () => {
    // This is the failure this test exists to prevent: `minItem` (singular),
    // `minlength` (lowercase) or a hand-invented `minRows` all PARSE fine and
    // are simply IGNORED by a JSON-Schema validator, so the schema would look
    // strict while enforcing nothing — exactly the "documented is not
    // verified" trap #2665 was opened about. The check is lexical: walk the
    // generated schema and assert every key is one this vocabulary knows.
    // `description` is on the list because it is a legal ANNOTATION; it is
    // listed here as allowed-but-inert, and is never the sole constraint on a
    // field (the assertions above are what make that true).
    const KNOWN = new Set([
      "type", "properties", "required", "additionalProperties", "items",
      "minItems", "maxItems", "minLength", "enum", "description",
    ]);
    const unknown = [];
    const walk = (node, path) => {
      if (!node || typeof node !== "object" || Array.isArray(node)) return;
      for (const [key, value] of Object.entries(node)) {
        if (!KNOWN.has(key)) unknown.push(`${path}.${key}`);
        if (key === "properties") {
          for (const [propName, sub] of Object.entries(value ?? {})) walk(sub, `${path}.properties.${propName}`);
        } else if (key === "items") {
          walk(value, `${path}.items`);
        }
      }
    };
    walk(helpers.buildSchemaFor({ gate: "pnpm run a && pnpm run b" }), "$");
    assert.deepEqual(unknown, [], `unknown/ignored schema keywords would enforce nothing: ${unknown.join(", ")}`);
  });

  it("SKILL.md still records the one-off harness probe — the half no test here can re-run", () => {
    // ⚠ THIS ASSERTS THE RECORD EXISTS, NOT THAT THE HARNESS ENFORCES
    // ANYTHING. The harness's validator is out of this repo's reach; the probe
    // that observed it was run once (#2665) and written down so the next
    // iteration on this hole is not a fourth guess. Deleting the record
    // silently is what this catches.
    const skill = readFileSync(SKILL_MD, "utf8");
    assert.ok(
      skill.includes("Harness enforcement of the build schema"),
      "SKILL.md must keep the 'Harness enforcement of the build schema' probe record (#2665)",
    );
    assert.ok(
      skill.includes("#2665"),
      "the probe record in SKILL.md must cite #2665 so the evidence is traceable",
    );
  });
});

// ── #2672: the OVER-length direction — no maxItems, direction-aware banner ───
// `minItems` pins the FLOOR. The ceiling is deliberately left open, and the
// reviewer-facing banner has to say the right thing in BOTH directions. See
// pump.js's `gateResultsSchema` comment for the ruling and its reasoning;
// these tests are what keep that ruling from silently eroding.
describe("gateResults over-length is allowed and correctly labelled (#2672)", () => {
  const item = { n: 9999, gate: "pnpm run a && pnpm run b" }; // 3 legs with CACHE

  it("emits NO maxItems — an over-long gateResults must not be REJECTED at the tool-call layer", () => {
    // RULED (#2672): `maxItems: cmds.length` was the obvious fix and is the
    // WRONG one. `gateCmds` is a crude `&&` split, not a shell parser, and it
    // UNDER-counts any gate hiding a step behind `;` or a subshell. `minItems`
    // fails SAFE under that under-count (the floor only ever comes out too
    // low); `maxItems` would fail UNSAFE — it would REJECT an honest agent
    // that ran and reported MORE steps than the split could see, forcing it to
    // DELETE evidence to satisfy the schema. Extra rows can also be legitimate
    // (a preflight step, a re-run after a fix). Over-evidence is not the hole
    // #2612 -> #2657 was closing; under-evidence was.
    const gateResults = helpers.buildSchemaFor(item).properties.gateResults;
    assert.equal(gateResults.maxItems, undefined, "gateResults must NOT carry maxItems — see the #2672 ruling in pump.js");
  });

  it("the BUILD prompt states a FLOOR ('AT LEAST'), never an exact count, and forbids deleting rows", () => {
    // The schema's no-maxItems ruling is only real if the prompt agents
    // actually read agrees with it. The prompt used to say "MUST have
    // exactly N entries", which directly contradicts "no ceiling" — an
    // agent following that literally would trim to N and delete real
    // evidence to comply, making the OVER-COMPLETE banner near-unreachable.
    assert.ok(
      /gateResults.*MUST have AT LEAST \$\{gateCmds\(b\)\.length\}/s.test(source),
      "the BUILD prompt must state gateResults needs AT LEAST N entries, not exactly N",
    );
    assert.ok(!/MUST have exactly \$\{gateCmds\(b\)\.length\}/.test(source), "the old exact-count BUILD prompt wording must be gone");
    assert.ok(/NEVER delete a row to hit a count/.test(source), "the BUILD prompt must forbid trimming rows to satisfy a count");
    assert.ok(/extra rows are accepted and are NOT a violation/.test(source), "the BUILD prompt must say extra rows (preflight, re-run legs) are accepted");
  });

  it("an OVER-long gateResults array still validates against the generated schema", () => {
    const schema = helpers.buildSchemaFor(item);
    const overLong = {
      ok: true,
      issue: 9999,
      pr: "https://github.com/Syynth/brink/pull/1",
      gateGreen: true,
      gateOutput: "PRE-FLIGHT: df -h / and du of the shared cargo cache both recorded.",
      // 4 rows for a 3-leg gate: the extra one is a real preflight step.
      gateResults: [
        { command: "df -h / (preflight)", result: "17G avail, well over the 15GiB floor" },
        { command: "export CARGO_TARGET_DIR=... (CACHE)", result: "exit 0, no output" },
        { command: "pnpm run a", result: "12 passed, 0 failed" },
        { command: "pnpm run b", result: "exit 0, clean" },
      ],
      reachability: "wired into the CLI's `foo` subcommand",
      summary: "did the thing",
    };
    const errors = validate(schema, overLong);
    assert.deepEqual(errors, [], `an over-long gateResults must be accepted, got: ${errors.join("; ")}`);
  });

  it("formatGateEvidence calls an over-long array OVER-COMPLETE, never INCOMPLETE", () => {
    // The #2672 bug verbatim: the banner fired on `results.length !== expected`
    // in BOTH directions and always said "evidence is INCOMPLETE; gateGreen is
    // unsupported" — telling the reviewer the opposite of what happened when
    // the array was padded rather than short.
    const build = {
      gateGreen: true,
      gateOutput: "z".repeat(100),
      gateResults: [
        { command: "df -h /", result: "preflight, 17G avail" },
        { command: "cargo test", result: "36 passed" },
        { command: "pnpm run test:scripts", result: "230 passed" },
      ],
    };
    const shown = helpers.formatGateEvidence(build, 2);
    assert.ok(!shown.includes("INCOMPLETE"), `an over-long array must NOT be called INCOMPLETE, got: ${shown.slice(0, 300)}`);
    assert.ok(shown.includes("OVER-COMPLETE"), `expected an OVER-COMPLETE banner, got: ${shown.slice(0, 300)}`);
    assert.ok(shown.includes("3 gateResults rows returned for a 2-command gate"), "the banner must name both counts");
    // Every row still renders — the extra row is the information, not noise.
    assert.ok(shown.includes("cargo test") && shown.includes("df -h /") && shown.includes("pnpm run test:scripts"));
  });

  it("the OVER-COMPLETE banner instructs the reviewer to CHECK coverage, not stand down", () => {
    // Follow-up finding on #2672: with no maxItems, an over-long gateResults
    // can satisfy minItems while OMITTING one of the gate's real commands —
    // extra rows mask a missing one. The old closing line ("do not read this
    // banner as missing evidence") told the reviewer the opposite of what it
    // should: it must tell them to verify coverage, not reassure them away
    // from checking it.
    const build = {
      gateGreen: true,
      gateOutput: "z".repeat(10),
      gateResults: [
        { command: "a", result: "ok" },
        { command: "b", result: "ok" },
        { command: "c", result: "ok" },
      ],
    };
    const shown = helpers.formatGateEvidence(build, 2);
    assert.ok(shown.includes("OVER-COMPLETE"));
    assert.ok(
      !shown.includes("do not read this banner as missing evidence"),
      "the old stand-down reassurance must be gone from the over-length banner",
    );
    assert.ok(
      shown.includes("does NOT imply coverage") && shown.includes("CHECK that each"),
      `expected a coverage-check instruction in the over-length banner, got: ${shown.slice(0, 500)}`,
    );
  });

  it("the UNDER-length direction still reads INCOMPLETE and unsupported", () => {
    const shown = helpers.formatGateEvidence(
      { gateGreen: true, gateOutput: "z".repeat(100), gateResults: [{ command: "cargo test", result: "36 passed" }] },
      5,
    );
    assert.ok(shown.includes("INCOMPLETE") && shown.includes("gateGreen is unsupported"), "a short array must still read INCOMPLETE");
    assert.ok(!shown.includes("OVER-COMPLETE"), "a short array must not be called OVER-COMPLETE");
  });

  it("bounds the rendered row count and each command string, and SAYS SO when it drops rows (#2664 secondary)", () => {
    // #2664's secondary note: `result` was capped per row but the `command`
    // string and the NUMBER of rows were unbounded, so a pathological return
    // could balloon the review prompt. Bounded — but the omission must be
    // VISIBLE, because the whole point of per-row capping (#2645) was that
    // truncation may shorten evidence and must never make a command silently
    // disappear.
    assert.ok(Number.isInteger(helpers.GATE_ROWS_CAP) && helpers.GATE_ROWS_CAP >= 12, "GATE_ROWS_CAP must be a generous positive integer");
    assert.ok(Number.isInteger(helpers.GATE_CMD_CAP) && helpers.GATE_CMD_CAP >= 80, "GATE_CMD_CAP must be a positive integer with room for a real command line");
    const rows = Array.from({ length: helpers.GATE_ROWS_CAP + 7 }, (_, i) => ({
      command: `cmd ${i} ` + "q".repeat(5000),
      result: `result ${i}`,
    }));
    const shown = helpers.formatGateEvidence({ gateGreen: true, gateOutput: "n".repeat(50), gateResults: rows }, rows.length);
    assert.ok(shown.length < 60000, `rendered evidence must be bounded, got ${shown.length} chars`);
    assert.ok(!shown.includes("q".repeat(500)), "a pathological command string must be capped");
    assert.ok(shown.includes("7 further gateResults rows not shown"), `the dropped rows must be announced, got tail: ${shown.slice(-400)}`);
  });
});

// ── #2664: the MERGE/fix phase gets the same structured evidence as BUILD ────
// The merge-train and fix-loop agents re-run the SAME `gateFor(b)` gate, on the
// commit that actually lands on main, and used to report it as one free-text
// `detail` string — the exact unevidenced-claim shape #2612 -> #2645 -> #2657
// spent three rounds removing from BUILD, one phase later and higher stakes.
describe("MERGE/fix schema carries per-command gate evidence (#2664)", () => {
  const item = { n: 9999, gate: "pnpm run a && pnpm run b" }; // 3 legs with CACHE

  it("mergeSchemaFor pins gateResults.minItems to THIS item's own gate command count", () => {
    const schema = helpers.mergeSchemaFor(item);
    assert.equal(schema.properties.gateResults.minItems, helpers.gateCmds(item).length);
    assert.equal(schema.properties.gateResults.minItems, 3);
    assert.ok(schema.required.includes("gateResults"), "gateResults must be REQUIRED on the merge schema, not optional");
    assert.equal(schema.properties.gateResults.maxItems, undefined, "the #2672 no-ceiling ruling applies to the merge schema too");
  });

  it("keeps landedState's three-value enum and detail — #2422's fix must survive", () => {
    const schema = helpers.mergeSchemaFor(item);
    assert.deepEqual(schema.properties.landedState.enum, ["landed", "armed", "parked"]);
    assert.ok(schema.required.includes("detail") && schema.required.includes("landedState") && schema.required.includes("pr"));
  });

  it("a hand-written COMPLETE merge object validates", () => {
    const errors = validate(helpers.mergeSchemaFor(item), {
      pr: "https://github.com/Syynth/brink/pull/1",
      landedState: "armed",
      detail: "clean merge with main, no conflicts; auto-merge armed, required checks still running",
      gateResults: [
        { command: "export CARGO_TARGET_DIR=... (CACHE)", result: "exit 0, no output" },
        { command: "pnpm run a", result: "12 passed, 0 failed" },
        { command: "pnpm run b", result: "exit 0, clean" },
      ],
    });
    assert.deepEqual(errors, [], `expected no validation errors, got: ${errors.join("; ")}`);
  });

  it("a PARKED merge must still account for every gate command — 'not run' is a valid, honest result", () => {
    // A merge that aborts before gating has no gate output. It must still
    // submit a row per command SAYING so, rather than being silent: that is
    // what keeps "a missing command is mechanically impossible" true for this
    // phase, and it is the same rule the build schema already states.
    const errors = validate(helpers.mergeSchemaFor(item), {
      pr: "https://github.com/Syynth/brink/pull/1",
      landedState: "parked",
      detail: "merge conflict in the LIR lowering was untangleable; git merge --abort, PR left open",
      gateResults: [
        { command: "export CARGO_TARGET_DIR=... (CACHE)", result: "not run — merge aborted before the gate" },
        { command: "pnpm run a", result: "not run — merge aborted before the gate" },
        { command: "pnpm run b", result: "not run — merge aborted before the gate" },
      ],
    });
    assert.deepEqual(errors, [], `an honest parked report must validate, got: ${errors.join("; ")}`);
  });

  it("a SHORT merge gateResults array is rejected, exactly as at BUILD", () => {
    const errors = validate(helpers.mergeSchemaFor(item), {
      pr: "https://github.com/Syynth/brink/pull/1",
      landedState: "landed",
      detail: "re-gated after merging main; all green; merged as sha abc1234",
      gateResults: [{ command: "pnpm run a", result: "12 passed, 0 failed" }],
    });
    assert.ok(
      errors.some((e) => e.includes("gateResults") && e.includes("needs at least")),
      `expected a gateResults minItems error, got: ${JSON.stringify(errors)}`,
    );
  });

  it("the merge AND fix call sites both pass mergeSchemaFor(b) — no static shared MERGE survives", () => {
    // Guards the other half: the per-item schema is inert unless BOTH train
    // jobs actually hand it to the harness. The old code passed one static
    // `MERGE` const to both, which cannot carry a per-item minItems.
    const wirings = source.match(/schema: mergeSchemaFor\(b\)/g) ?? [];
    assert.equal(wirings.length, 2, "both the merge and the fix agent() calls must pass `schema: mergeSchemaFor(b)`");
    assert.ok(!/schema: MERGE\b/.test(source), "the static `schema: MERGE` wiring must be gone");
  });

  it("the merge and fix prompts both instruct one gateResults row per gate command", () => {
    // A schema the agent is never TOLD about produces retry churn instead of
    // evidence; the build prompt states the count explicitly and these must too.
    // Split on the CALL SITE marker, not on the bare identifier — the latter
    // also matches prose in the comments and would drift with any edit there.
    // ⚠ Match on the PROSE, not on "`gateResults`" — inside pump.js these
    // prompts are template literals, so their backticks are written escaped
    // (\`gateResults\`) and a naive backtick match silently matches a nearby
    // COMMENT instead, passing vacuously. This phrase is unique to the two
    // train prompts (the build prompt says "MUST have AT LEAST N entries").
    const ROW_RULE = "MUST have one entry per command";
    assert.equal((source.match(new RegExp(ROW_RULE, "g")) ?? []).length, 2, "exactly the merge and fix prompts should carry the per-command row rule");
    const chunks = source.split("schema: mergeSchemaFor(b)");
    assert.equal(chunks.length, 3, "expected exactly two `schema: mergeSchemaFor(b)` call sites to split on");
    for (const [i, chunk] of [chunks[0], chunks[1]].entries()) {
      const which = i === 0 ? "merge" : "fix";
      assert.ok(chunk.includes(ROW_RULE), `the ${which} prompt must state the per-command gateResults rule`);
      assert.ok(chunk.includes("${gateCmds(b).length}"), `the ${which} prompt must interpolate THIS item's own gate command count`);
      // The park-before-gating case is the one an agent will actually hit; if
      // the prompt doesn't cover it, the schema just blocks honest parks.
      assert.ok(chunk.includes("not run"), `the ${which} prompt must tell a parked agent to report "not run" rows rather than omit them`);
    }
  });

  it("the wave's returned landings render the merge gate evidence, so it is not write-only", () => {
    // Reachability: a structured field nobody renders is dead weight. The
    // wave's return payload (landed / awaitingChecks / parked) is what the
    // orchestrator and the human read at wave close.
    assert.ok(
      source.includes("formatGateEvidence(r.land,"),
      "the wave's landings must be rendered through formatGateEvidence(r.land, ...)",
    );
    // All three landing buckets must carry it — `landed` and `awaitingChecks`
    // unconditionally, `parked` only when a merge/fix agent actually ran.
    const gateEvidenceWirings = source.match(/gateEvidence: landEvidence\(r\)/g) ?? [];
    assert.equal(gateEvidenceWirings.length, 3, "landed, awaitingChecks and parked must each surface the merge gate evidence");
  });

  it("the retro prompt's table legend explains the 'merge gate rows k/N' column", () => {
    // trackerFacts (L652) adds a `merge gate rows k/N` ratio per item, but the
    // retro prompt used to document every OTHER column ("issue -> PR -> merge
    // state -> what the PR claims to close") and never mentioned this one — so
    // the retro had no instruction to treat k < N as an under-evidenced
    // landing claim. A structured field nobody's told to read is dead weight
    // by this PR's own standard.
    assert.ok(
      source.includes("merge gate rows k/N"),
      "the retro prompt must name the 'merge gate rows k/N' column in its legend",
    );
    assert.ok(
      /merge gate rows k\/N.{0,400}k < N.{0,200}under-evidenced/s.test(source),
      "the legend must instruct the retro to treat k < N as an under-evidenced landing claim",
    );
    assert.ok(
      /merge gate rows k\/N.{0,600}"-".{0,100}no merge\/fix agent ran/s.test(source),
      "the legend must explain that '-' means no merge/fix agent ran for that item",
    );
  });
});

// ── #2686 Gap 1: a MECHANICAL READER over the merge-phase gate evidence ──────
// #2664 gave the MERGE/fix phase the same structured `gateResults` array the
// BUILD phase carries. But the BUILD rows are interpolated into the ADVERSARIAL
// REVIEWER's prompt — read by an agent whose entire job is to disbelieve them —
// while the MERGE rows went into the wave's returned payload and a bare `k/N`
// ratio in the retro's table. Required, structured, schema-enforced, and
// obliged to be read by nobody: #2612's original hole one layer over.
//
// `auditGateEvidence` is the reader. It is DETERMINISTIC, not an agent — it
// runs on every rendered evidence block (build and merge alike) and produces
// named concerns that ride into the reviewer prompt, the retro prompt and the
// wave's return payload.
//
// ⚠ WHAT IT CAN AND CANNOT CATCH — the honest boundary, restated in code so it
// cannot erode:
//   CAN:    a row count below the gate's command count (the signal that
//           survives if a future harness stops enforcing `minItems`);
//           two rows reporting the SAME command (padding that satisfies
//           `minItems` while a real command goes unreported);
//           a gate command no row plausibly corresponds to;
//           a row whose own `result` says the command did NOT run, in a report
//           that simultaneously claims the work landed / the gate was green.
//   CANNOT: fabrication. A row reading "36 passed" for a command never run
//           passes every check here, exactly as it passes the schema. Only a
//           MISSING command is mechanically impossible; a false one is not.
//           The command-coverage check is also a FUZZY string match — it can
//           flag a paraphrased row that is really fine, and it is a prompt for
//           a human/agent to look, never a verdict.
describe("mechanical audit over gate evidence (#2686)", () => {
  const CMDS = ["export CARGO_TARGET_DIR=/tmp/x", "pnpm run a", "pnpm run b"];

  it("exports auditGateEvidence and returns no concerns for verbatim, complete, all-ran evidence", () => {
    assert.equal(typeof helpers.auditGateEvidence, "function");
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "landed",
        detail: "merged clean",
        gateResults: [
          { command: "export CARGO_TARGET_DIR=/tmp/x", result: "exit 0, no output" },
          { command: "pnpm run a", result: "12 passed, 0 failed" },
          { command: "pnpm run b", result: "exit 0, clean" },
        ],
      },
      CMDS,
    );
    assert.deepEqual(concerns, [], `clean evidence must raise no concerns, got: ${concerns.join(" | ")}`);
  });

  it("flags a gate command that no row plausibly covers", () => {
    // 3 rows for a 3-command gate — `minItems` is satisfied — but `pnpm run b`
    // was never reported; a preflight row took its slot. This is the exact
    // "extra rows can hide a missing one" case the OVER-COMPLETE banner could
    // only ASK a reader to check by hand.
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "landed",
        gateResults: [
          { command: "df -h / (preflight)", result: "21G avail" },
          { command: "export CARGO_TARGET_DIR=/tmp/x", result: "exit 0" },
          { command: "pnpm run a", result: "12 passed, 0 failed" },
        ],
      },
      CMDS,
    );
    assert.ok(
      concerns.some((c) => c.includes("UNCOVERED COMMAND") && c.includes("pnpm run b")),
      `expected an UNCOVERED COMMAND concern naming "pnpm run b", got: ${concerns.join(" | ")}`,
    );
  });

  it("flags two rows reporting the SAME command — padding that satisfies minItems", () => {
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "landed",
        gateResults: [
          { command: "export CARGO_TARGET_DIR=/tmp/x", result: "exit 0" },
          { command: "pnpm run a", result: "12 passed" },
          { command: "pnpm  run   a", result: "12 passed" },
        ],
      },
      CMDS,
    );
    assert.ok(
      concerns.some((c) => c.includes("DUPLICATE COMMAND")),
      `expected a DUPLICATE COMMAND concern, got: ${concerns.join(" | ")}`,
    );
  });

  it("flags a row count below the gate's command count — the signal that survives a harness that stops enforcing minItems", () => {
    const concerns = helpers.auditGateEvidence(
      { landedState: "landed", gateResults: [{ command: "pnpm run a", result: "12 passed" }] },
      CMDS,
    );
    assert.ok(
      concerns.some((c) => c.includes("UNDER-EVIDENCED") && c.includes("1") && c.includes("3")),
      `expected an UNDER-EVIDENCED concern naming 1 of 3, got: ${concerns.join(" | ")}`,
    );
  });

  it("flags a self-declared 'not run' row in a report that claims the work LANDED", () => {
    // The contradiction worth catching: the commit is on main, and the merge
    // agent's own evidence says one of the gate's commands never executed.
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "landed",
        gateResults: [
          { command: "export CARGO_TARGET_DIR=/tmp/x", result: "exit 0" },
          { command: "pnpm run a", result: "12 passed, 0 failed" },
          { command: "pnpm run b", result: "not run — I ran out of turns" },
        ],
      },
      CMDS,
    );
    assert.ok(
      concerns.some((c) => c.includes("SELF-DECLARED UNRUN") && c.includes("landed")),
      `expected an escalated SELF-DECLARED UNRUN concern, got: ${concerns.join(" | ")}`,
    );
  });

  it("does NOT flag 'not run' rows on an honest PARK — that is the documented, valid answer", () => {
    // A merge that aborts before gating MUST return a row per command saying
    // "not run — merge aborted before the gate" (#2664). Flagging that every
    // time would make the audit noise on the normal path, and an audit nobody
    // trusts is the same as no audit.
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "parked",
        gateResults: CMDS.map((command) => ({ command, result: "not run — merge aborted before the gate" })),
      },
      CMDS,
    );
    assert.deepEqual(concerns, [], `an honest park must raise no concerns, got: ${concerns.join(" | ")}`);
  });

  it("flags a self-declared unrun row in a BUILD report that claims gateGreen", () => {
    const concerns = helpers.auditGateEvidence(
      {
        gateGreen: true,
        gateResults: [
          { command: "export CARGO_TARGET_DIR=/tmp/x", result: "exit 0" },
          { command: "pnpm run a", result: "12 passed, 0 failed" },
          { command: "pnpm run b", result: "output was lost when the shell died" },
        ],
      },
      CMDS,
    );
    assert.ok(
      concerns.some((c) => c.includes("SELF-DECLARED UNRUN") && c.includes("gateGreen")),
      `expected an escalated SELF-DECLARED UNRUN concern for a green build, got: ${concerns.join(" | ")}`,
    );
  });

  it("does NOT flag an unrun row on a build that honestly reports gateGreen:false", () => {
    const concerns = helpers.auditGateEvidence(
      {
        gateGreen: false,
        gateResults: CMDS.map((command) => ({ command, result: "not run — stopped on the ENOSPC preflight" })),
      },
      CMDS,
    );
    assert.deepEqual(concerns, [], `an honest red build must raise no concerns, got: ${concerns.join(" | ")}`);
  });

  it("does NOT flag `node --test`'s own canonical summary line as a self-declared skip", () => {
    // Found post-merge: SKIPPED_RE's lookbehind alone let "# skipped 0" through
    // because it is preceded by "# ", not a digit/comma. This is the repo's
    // OWN gate leg (`"test:scripts": "node --test scripts/*.test.mjs"`), so it
    // fired on the normal path for every pump-config item, in the reviewer's
    // <gate-output> block AND in gateEvidenceConcerns -> the retro, whose
    // prompt then instructs the agent to comment on the issue and recommend
    // DISARMING an armed PR. This is this PR's own verbatim [3/3] row text.
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "landed",
        gateResults: [
          { command: CMDS[0], result: "exit 0" },
          { command: CMDS[1], result: "exit 0, clean" },
          {
            command: CMDS[2],
            result: "# tests 271\n# pass 271\n# fail 0\n# cancelled 0\n# skipped 0\n# todo 0",
          },
        ],
      },
      CMDS,
    );
    assert.deepEqual(concerns, [], `an ordinary node --test summary must raise no concerns, got: ${concerns.join(" | ")}`);
  });

  it("does NOT flag a result that explicitly NEGATES a timeout — same false-positive class as SKIPPED_RE", () => {
    // "\btimed out\b" alone cannot see the "none " immediately before it, so
    // an honest, fully-green result stating that nothing timed out used to
    // read as SELF-DECLARED UNRUN.
    const concerns = helpers.auditGateEvidence(
      {
        landedState: "landed",
        gateResults: [
          { command: CMDS[0], result: "exit 0" },
          { command: CMDS[1], result: "exit 0, 271/271 pass. All green; none timed out." },
          { command: CMDS[2], result: "exit 0, clean" },
        ],
      },
      CMDS,
    );
    assert.deepEqual(concerns, [], `a stated non-timeout must raise no concerns, got: ${concerns.join(" | ")}`);
  });

  it("formatGateEvidence accepts the command LIST and renders the audit above the rows", () => {
    const shown = helpers.formatGateEvidence(
      {
        landedState: "landed",
        detail: "merged as abc1234 after reading all four checks green",
        gateResults: [
          { command: "df -h /", result: "21G avail" },
          { command: "export CARGO_TARGET_DIR=/tmp/x", result: "exit 0" },
          { command: "pnpm run a", result: "12 passed" },
        ],
      },
      CMDS,
    );
    assert.ok(shown.includes("MECHANICAL EVIDENCE AUDIT"), `expected an audit block, got: ${shown.slice(0, 300)}`);
    assert.ok(shown.includes("UNCOVERED COMMAND"), "the audit block must carry the concern");
    // Passing the LIST must still denominate the rows against its length.
    assert.ok(shown.includes("[1/3]"), "row denominators must come from the command list's length");
  });

  it("formatGateEvidence still accepts a plain NUMBER as `expected` — the coverage check just goes unrun", () => {
    // Back-compatibility is load-bearing: this function is called from four
    // places and a signature change that silently broke one would blind a
    // phase. With a number there are no command strings to match against, so
    // the audit reports only the count-based checks.
    const shown = helpers.formatGateEvidence(
      { gateGreen: true, gateOutput: "z".repeat(50), gateResults: [{ command: "cargo test", result: "36 passed" }] },
      5,
    );
    assert.ok(shown.includes("INCOMPLETE"), "the direction-aware banner must still fire on a number");
    assert.ok(shown.includes("[1/5]"), "a numeric expected must still denominate the rows");
  });

  it("formatGateEvidence's clean-audit wording depends on whether coverage was actually checked", () => {
    // The clean line used to be UNCONDITIONAL — "every gate command has a
    // corresponding row" — even though the coverage half of the audit
    // (UNCOVERED/DUPLICATE COMMAND) only runs when `expected` is the command
    // LIST. With the number form (kept working on purpose, see above), that
    // line asserted a check that never ran. Both wordings are pinned here.
    const cleanWithList = helpers.formatGateEvidence(
      { landedState: "landed", gateResults: CMDS.map((command) => ({ command, result: "exit 0, clean" })) },
      CMDS,
    );
    assert.ok(
      cleanWithList.includes("No mechanical concerns: every gate command has a corresponding row"),
      `expected the coverage-checked clean wording, got: ${cleanWithList.slice(0, 400)}`,
    );

    const cleanWithNumber = helpers.formatGateEvidence(
      { landedState: "landed", gateResults: CMDS.map((command) => ({ command, result: "exit 0, clean" })) },
      CMDS.length,
    );
    assert.ok(
      /count-based checks only.*coverage was not checked/is.test(cleanWithNumber),
      `expected a clean wording that admits coverage was NOT checked for the number form, got: ${cleanWithNumber.slice(0, 400)}`,
    );
    assert.ok(
      !cleanWithNumber.includes("every gate command has a corresponding row"),
      "the number form must not claim the coverage check ran",
    );
  });

  it("states the fabrication limit IN the rendered audit, even when there are no concerns", () => {
    // A clean audit line that just says "no concerns" would read as "this
    // evidence is verified". It is not, and never can be: the reader must be
    // told what was NOT checked, in the same breath as what was.
    const shown = helpers.formatGateEvidence(
      {
        landedState: "landed",
        detail: "merged clean, all required checks read green before merging",
        gateResults: CMDS.map((command) => ({ command, result: "exit 0, clean" })),
      },
      CMDS,
    );
    assert.ok(shown.includes("MECHANICAL EVIDENCE AUDIT"), "the audit line must render even when clean");
    assert.ok(
      /fabricat/i.test(shown),
      `the audit must state that fabrication is undetectable, got: ${shown.slice(0, 400)}`,
    );
  });

  it("both the review and the landing call sites pass the command LIST, so coverage is actually checked", () => {
    assert.ok(
      source.includes("${formatGateEvidence(build, gateCmds(b))}"),
      "the review prompt must pass gateCmds(b) (the list) so the coverage check runs",
    );
    assert.ok(
      /formatGateEvidence\(r\.land, gateCmdsFor\(r\.issue\)/.test(source),
      "the landing renderer must pass this item's command list, not just a count",
    );
  });

  it("the wave's returned payload surfaces the audit concerns as their own field", () => {
    // The point of #2686: evidence nothing is obliged to look at is barely
    // better than no evidence. The concerns must be a first-class field, not
    // buried inside a rendered string a summariser may skip.
    assert.ok(source.includes("gateEvidenceConcerns"), "the wave return must carry a gateEvidenceConcerns field");
    assert.ok(
      /counts: \{[^}]*gateEvidenceConcerns/s.test(source),
      "counts must include gateEvidenceConcerns so a non-zero value is visible at a glance",
    );
  });

  it("the retro is handed the full merge-phase concerns and told to act on them", () => {
    // The retro is the only phase that runs AFTER landing and has an action
    // channel (issue comments, follow-up issues). It cannot un-merge anything
    // — say so — but it can make an under-evidenced landing a named, durable
    // item instead of a ratio nobody reads.
    assert.ok(
      source.includes("${gateAuditFacts"),
      "the retro prompt must interpolate the wave-wide gate-evidence concerns",
    );
    assert.ok(
      /MERGE-PHASE GATE-EVIDENCE AUDIT/.test(source),
      "the retro prompt must label the audit section so the agent cannot skim past it",
    );
    assert.ok(
      /cannot un-merge/i.test(source),
      "the retro prompt must state plainly that it cannot un-merge a landed commit",
    );
  });
});

// ── #2686 Gap 2: the OVER-COMPLETE banner's accuracy is coupled to a config
// convention nothing enforced ────────────────────────────────────────────────
// `gateCmds` splits on `&&`. A gate written with `;` instead under-counts, and
// then every honest report renders as a false OVER-COMPLETE. Latent today —
// every BRINK-CONFIG.md gate is `&&`-chained — and #2672 refused `maxItems`
// precisely because of that under-count. This lint makes the coupling explicit
// instead of latent. It READS BRINK-CONFIG.md; it never writes it (the lessons
// phase owns a per-wave PR against that file).
describe("BRINK-CONFIG.md gate strings stay &&-chained (#2686 Gap 2)", () => {
  const CONFIG_MD = join(__dirname, "..", ".claude", "skills", "autonomous-pump", "BRINK-CONFIG.md");
  const config = readFileSync(CONFIG_MD, "utf8");

  // Only the lines that DEFINE a gate or the CACHE prefix — not every
  // backticked command in the file's prose.
  const gateLines = config
    .split("\n")
    .map((line) => {
      const m = line.match(/^- \*\*([^*]+)\*\*: `([^`]+)`/) ?? line.match(/^- (CACHE prefix): `([^`]+)`/);
      if (!m) return null;
      const [, label, command] = m;
      return /gate|cache prefix/i.test(label) ? { label, command } : null;
    })
    .filter(Boolean);

  it("finds the gate definitions it is meant to lint — a zero-match scan would pass vacuously", () => {
    // House rule: a glob/scan that matches nothing exits green forever.
    assert.ok(
      gateLines.length >= 3,
      `expected at least 3 gate/CACHE definitions in BRINK-CONFIG.md, found ${gateLines.length}: ${gateLines.map((g) => g.label).join(", ")}`,
    );
  });

  it("no gate command is `;`-joined — `gateCmds` splits on `&&` and would under-count it", () => {
    const offenders = gateLines.filter((g) => g.command.includes(";"));
    assert.deepEqual(
      offenders.map((g) => `${g.label}: ${g.command}`),
      [],
      "a `;`-joined gate makes gateCmds under-count its steps, which lowers the schema floor AND renders every honest report as a false OVER-COMPLETE. Rewrite the gate with `&&`, or change gateCmds and this lint together.",
    );
  });

  it("every gate definition splits into at least one non-empty leg", () => {
    for (const { label, command } of gateLines) {
      const legs = command.split("&&").map((s) => s.trim()).filter(Boolean);
      assert.ok(legs.length >= 1, `${label} produced no gate legs`);
    }
  });
});

// ── #2673: the #2665 probe record is a one-shot with no currency signal ──────
// The probe established by direct experiment that the harness rejects a short
// `gateResults` array. That record sits in SKILL.md with nothing to invalidate
// it: a CLI upgrade could silently drop enforcement and every downstream claim
// would keep reading as true.
//
// ⚠ THESE TESTS GUARD THE RECORD'S SHAPE, NOT ITS CURRENCY. No in-tree test can
// detect a harness that stopped enforcing `minItems` — that boundary is stated
// in this file's header and nothing below softens it. What they buy is that the
// version and the re-probe trigger cannot be quietly dropped from the record,
// so a reader can always tell WHICH harness answered.
describe("the #2665 probe record carries a version and a re-probe trigger (#2673)", () => {
  const skill = readFileSync(SKILL_MD, "utf8");

  it("records the observed CLI version in a machine-readable line", () => {
    const m = skill.match(/PROBED-CLI:\s*([0-9]+\.[0-9]+\.[0-9]+)/);
    assert.ok(m, "SKILL.md must carry a `PROBED-CLI: <x.y.z>` line so the record names WHICH harness answered");
    assert.ok(m[1].split(".").every((p) => /^[0-9]+$/.test(p)), `PROBED-CLI must parse as a version, got ${m[1]}`);
  });

  it("names the command that reads the running version, so the trigger is executable", () => {
    assert.ok(
      skill.includes("claude --version"),
      "the re-probe trigger must name `claude --version` — a trigger nobody can evaluate is not a trigger",
    );
  });

  it("states a re-probe trigger, not just a date", () => {
    assert.ok(
      /RE-PROBE TRIGGER/.test(skill),
      "SKILL.md must carry an explicit RE-PROBE TRIGGER heading for the #2665 record",
    );
  });

  it("keeps the boundary: no in-tree test can detect a harness that stops enforcing", () => {
    assert.ok(
      /cannot|can not/i.test(skill) && skill.includes("#2665"),
      "the record must keep stating what it cannot establish",
    );
  });

  it("the retro carries the version-drift duty, so the trigger has an actor once per wave", () => {
    assert.ok(
      source.includes("PROBED-CLI"),
      "pump.js's retro prompt must tell the agent to compare the running CLI version against SKILL.md's PROBED-CLI record",
    );
  });
});

// ── #2673 Gap 2: the BRINK-CONFIG.md house-rule handoff had no tracked home ──
// #2669 correctly did not edit BRINK-CONFIG.md (the lessons phase owns a
// per-wave PR against that file), but nothing carried the proposed rule
// forward — if the lessons agent misses the issue, the rule evaporates. A
// tracked handoff depends on an agent reading a tracker; a MECHANICAL one is
// interpolated into the lessons prompt every wave until the rule lands.
describe("pending house rules are handed to the lessons phase mechanically (#2673 Gap 2)", () => {
  it("pump.js declares PENDING_HOUSE_RULES and interpolates it into the lessons prompt", () => {
    assert.ok(/const PENDING_HOUSE_RULES = \[/.test(source), "pump.js must declare a PENDING_HOUSE_RULES list");
    assert.ok(
      source.includes("${pendingRulesBlock}"),
      "the lessons prompt must interpolate the pending house rules, not merely define them",
    );
  });

  it("carries #2669's proposed rule about probing the enforcer", () => {
    assert.ok(
      /a schema constraint is enforcement only if a validator honours it/.test(source),
      "the #2669/#2673 rule must be seeded into PENDING_HOUSE_RULES verbatim enough to be recognisable",
    );
    assert.ok(source.includes("#2673"), "the pending rule must cite its tracking issue");
  });

  it("tells the lessons agent to skip a pending rule already present in RULES", () => {
    // Otherwise the list re-proposes the same rule every wave forever.
    assert.ok(
      /already covered by an existing rule/i.test(source),
      "the lessons prompt must tell the agent to drop a pending rule that already landed",
    );
  });
});
