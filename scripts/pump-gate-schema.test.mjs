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
// ⚠ READ-ONLY HERE. The lessons phase owns a per-wave PR against this file, so
// nothing outside that phase edits it — but linting its gate strings (#2686
// gap 2) is a read, and the coupling it makes explicit is real: `gateCmds` is
// an `&&` split, so a `;`-joined gate would silently under-count.
const BRINK_CONFIG_MD = join(__dirname, "..", ".claude", "skills", "autonomous-pump", "BRINK-CONFIG.md");

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
  `${helpersSrc}\nreturn { gateFor, gateCmds, buildSchemaFor, mergeSchemaFor, formatGateEvidence, auditGateEvidence, formatEvidenceAudit, hasSemicolonChain, GATE_ROWS_CAP, GATE_CMD_CAP };`,
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
    // The call site must pass gateCmds(b).length so a shortfall renders a
    // banner instead of a falsely-complete-looking fraction.
    assert.ok(
      source.includes("<gate-output>") && source.includes("${formatGateEvidence(build, gateCmds(b).length)}"),
      "the <gate-output> block in the review prompt must call formatGateEvidence(build, gateCmds(b).length)",
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

// ── #2686 gap 1: the MERGE-phase evidence gets a READER ──────────────────────
// #2664 (PR #2680) made merge/fix `gateResults` required, structured and
// schema-enforced, and rendered it into the wave's return payload. Nothing was
// OBLIGED to read it. The BUILD rows are interpolated into an adversarial
// reviewer's prompt — an agent whose whole job is to disbelieve them; the MERGE
// rows, which cover the commit that actually LANDS ON MAIN, went to the
// wave-close payload and a bare `k/N` ratio in the retro. That is #2612's
// original hole one layer over: evidence that is collected and unread.
//
// ⚠ WHAT `auditGateEvidence` CAN AND CANNOT DETECT — the honest boundary, and
// the reason these tests are shaped the way they are:
//   CAN (mechanically):
//     • a gate command with NO matching row, even when `minItems` is satisfied
//       by padding — the failure mode the missing `maxItems` (#2672, ruled) is
//       deliberately open to;
//     • a row that SELF-DECLARES it did not run ("not run", "timed out",
//       "output lost") — honest at a park, a red flag on a landing;
//     • a row whose result carries no number at all (no counts, no exit
//       status) — #2686 gap 3's "bare 'passed'" shape;
//     • rows fewer than the gate's own command count.
//   CANNOT, EVER:
//     • tell a TRUE result from a FABRICATED one. "36 passed" for a command
//       never run audits perfectly clean, at BUILD and at MERGE alike. Only a
//       MISSING command is mechanically impossible. Nothing in this round
//       changes that, and the audit's own rendered output says so.
//   ALSO CANNOT:
//     • BLOCK a landing. The merge agent gates and merges inside one turn, so
//       every reader of its evidence is necessarily post-hoc. The reader added
//       here is the retro, whose lever is the tracker (flag it, comment, leave
//       the issue open), not the merge.
describe("merge-phase gate evidence has a mechanical auditor (#2686 gap 1)", () => {
  const CMDS = [
    "export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink CARGO_INCREMENTAL=0",
    "cargo fmt --all -- --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo nextest run --workspace",
  ];

  it("gives an honest, complete report ZERO flags — no false positives on real gate rows", () => {
    const audit = helpers.auditGateEvidence({
      gateResults: [
        { command: "export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink CARGO_INCREMENTAL=0 ...", result: "exit 0" },
        { command: "cargo fmt --all -- --check", result: "exit 0, 0 files reformatted" },
        { command: "cargo clippy --workspace --all-targets -- -D warnings", result: "exit 0, 0 warnings" },
        { command: "cargo nextest run --workspace", result: "1204 tests run: 1204 passed, 2 skipped" },
      ],
    }, CMDS);
    assert.equal(audit.flagged, false, `expected no flags, got ${JSON.stringify(audit)}`);
    assert.deepEqual(audit.missing, []);
    assert.deepEqual(audit.unrun, []);
    assert.deepEqual(audit.weak, []);
  });

  it("flags a gate command with NO matching row even when minItems is satisfied by padding", () => {
    // The exact hole the deliberately-absent `maxItems` (#2672) leaves open:
    // 4 rows for a 4-command gate, but clippy never ran and a preflight row
    // takes its place. `minItems` counts rows; only a matcher sees the gap.
    const audit = helpers.auditGateEvidence({
      gateResults: [
        { command: "df -h / (preflight)", result: "17G avail" },
        { command: "export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink ...", result: "exit 0" },
        { command: "cargo fmt --all -- --check", result: "exit 0, clean, 0 files changed" },
        { command: "cargo nextest run --workspace", result: "1204 passed" },
      ],
    }, CMDS);
    assert.equal(audit.flagged, true);
    assert.equal(audit.missing.length, 1, `expected exactly clippy missing, got ${JSON.stringify(audit.missing)}`);
    assert.ok(audit.missing[0].includes("clippy"), `expected the clippy command flagged, got ${audit.missing[0]}`);
  });

  it("flags rows that SELF-DECLARE they did not run — honest at a park, a red flag on a landing", () => {
    const audit = helpers.auditGateEvidence({
      gateResults: CMDS.map((command) => ({ command, result: "not run — merge aborted before the gate" })),
    }, CMDS);
    assert.equal(audit.unrun.length, 4, `every self-declared not-run row must be flagged, got ${JSON.stringify(audit.unrun)}`);
    assert.equal(audit.flagged, true);
    assert.deepEqual(audit.missing, [], "a parked report still covers every command — that is not a MISSING row");
  });

  it("does NOT read a real nextest summary's '2 skipped' as a skipped COMMAND", () => {
    // The obvious naive regex (/skipped/) would flag every green nextest run
    // in this repo. A false positive here trains the reader to ignore the
    // auditor, which is the same failure as having no auditor.
    const audit = helpers.auditGateEvidence({
      gateResults: CMDS.map((command) => ({ command, result: "1204 tests run: 1204 passed, 2 skipped, 0 failed" })),
    }, CMDS);
    assert.deepEqual(audit.unrun, [], `a nextest '2 skipped' tally must not read as an unrun command, got ${JSON.stringify(audit)}`);
    assert.equal(audit.flagged, false);
  });

  it("flags a result carrying no number at all — #2686 gap 3's bare 'passed', without a schema pattern", () => {
    // ⚠ RULED HERE, deliberately NOT as schema enforcement. #2686 gap 3 proposes
    // requiring a digit or exit-status token in `result` at the SCHEMA layer.
    // That would REJECT the tool call — and the schema's own description has
    // always named `"clean"` as a valid answer, so a digit requirement would
    // refuse honest rows and push an agent toward inventing a number to get
    // its call accepted. The pump is what every future wave lands through; a
    // constraint that rejects a valid merge is a worse bug than the one it
    // closes. So the mitigation lands as an ADVISORY flag a reader sees, not
    // as a rejection.
    const audit = helpers.auditGateEvidence({
      gateResults: CMDS.map((command) => ({ command, result: "passed" })),
    }, CMDS);
    assert.equal(audit.weak.length, 4, `a digitless result must be flagged as weak, got ${JSON.stringify(audit.weak)}`);
    assert.equal(audit.flagged, true);
  });

  it("counts rows against the gate's own command count, so a short array is flagged too", () => {
    const audit = helpers.auditGateEvidence({ gateResults: [{ command: "cargo nextest run --workspace", result: "1204 passed" }] }, CMDS);
    assert.equal(audit.rows, 1);
    assert.equal(audit.expected, 4);
    assert.equal(audit.flagged, true);
    assert.equal(audit.missing.length, 3, "the three unreported commands must all be named");
  });

  it("treats a missing/absent report as unevidenced rather than clean", () => {
    const audit = helpers.auditGateEvidence(null, CMDS);
    assert.equal(audit.rows, 0);
    assert.equal(audit.flagged, true, "no report at all must never audit as clean");
  });

  it("formatEvidenceAudit always states the limit it cannot cross — fabrication", () => {
    const clean = helpers.formatEvidenceAudit(helpers.auditGateEvidence({
      gateResults: CMDS.map((command) => ({ command, result: "exit 0, 12 passed" })),
    }, CMDS));
    assert.ok(/fabricat/i.test(clean), `a CLEAN audit must still disclaim fabrication, got: ${clean}`);
    assert.ok(!/^\s*$/.test(clean), "a clean audit must still render a line, not an empty string");

    const dirty = helpers.formatEvidenceAudit(helpers.auditGateEvidence({
      gateResults: [{ command: "cargo fmt --all -- --check", result: "clean" }],
    }, CMDS));
    assert.ok(dirty.includes("MECHANICAL AUDIT"), "the audit block must be labelled");
    assert.ok(/no matching row/i.test(dirty), "a missing command must be named in the rendered audit");
    assert.ok(/fabricat/i.test(dirty), "the rendered audit must disclaim fabrication in the flagged case too");
    // The matcher is a heuristic; saying so is what stops a false positive
    // being read as proof of dishonesty.
    assert.ok(/heuristic/i.test(dirty), "the rendered audit must name itself a heuristic");
  });

  it("the retro prompt carries the FULL merge evidence, not just the k/N ratio, and is told to disbelieve it", () => {
    // The reader. Without this the audit is exactly what #2664 was: structured
    // evidence nobody is obliged to look at.
    assert.ok(
      source.includes("MERGE-PHASE GATE EVIDENCE"),
      "the retro prompt must carry a MERGE-PHASE GATE EVIDENCE section",
    );
    assert.ok(
      source.includes("formatEvidenceAudit("),
      "the mechanical audit must be rendered somewhere, not just computed",
    );
    assert.ok(
      /\$\{mergeEvidence/.test(source),
      "the assembled merge evidence must be interpolated into the retro prompt",
    );
    // The instruction has to name what the reader should DO, or it is a
    // paragraph nobody acts on.
    assert.ok(
      /MERGE-PHASE GATE EVIDENCE[\s\S]{0,4000}trackerActions/.test(source),
      "the retro must be told to record what it found in trackerActions",
    );
  });

  it("the landing buckets carry the mechanical audit alongside the rendered rows", () => {
    const auditWirings = source.match(/gateAudit: /g) ?? [];
    assert.ok(auditWirings.length >= 3, `landed, awaitingChecks and parked must each carry gateAudit, found ${auditWirings.length}`);
    assert.ok(
      source.includes("landingsFlagged"),
      "the wave payload's counts must surface how many landings the audit flagged",
    );
  });
});

// ── #2686 gap 2: the OVER-COMPLETE banner's hidden coupling, made explicit ────
// `gateCmds` splits on `&&`. A gate written with `;` instead would report an
// expected count of 1, and then EVERY honest report renders the #2680
// OVER-COMPLETE banner as a false positive. Latent today — every BRINK-CONFIG
// gate is `&&`-chained — and #2672 refused `maxItems` precisely because the
// split under-counts. But "latent" was resting on a convention nothing checked.
//
// A `;`-joined gate is also a broken GATE independent of any counting: `;`
// does not fail fast, so a red first leg still runs the rest and the chain's
// exit status is the LAST command's. That is why this is a hard refusal at
// launch rather than a warning.
describe("gate strings are &&-chained, not ;-chained (#2686 gap 2)", () => {
  it("hasSemicolonChain distinguishes a ;-joined gate from an &&-joined one", () => {
    assert.equal(helpers.hasSemicolonChain("cargo fmt --check && cargo test"), false);
    assert.equal(helpers.hasSemicolonChain("cargo fmt --check; cargo test"), true);
    assert.equal(helpers.hasSemicolonChain("pnpm run test:scripts"), false);
  });

  it("every gate string in BRINK-CONFIG.md is &&-chained", () => {
    const config = readFileSync(BRINK_CONFIG_MD, "utf8");
    // Gate strings are the backticked span on a bullet whose bold label names
    // a gate ("Rust (default GATE)", "TS entries (gate override)", ...).
    const gates = [...config.matchAll(/^- \*\*[^*]*\b(?:GATE|gate override)\b[^*]*\*\*:\s*`([^`]+)`/gm)].map((m) => m[1]);
    assert.ok(
      gates.length >= 3,
      `expected to find BRINK-CONFIG.md's gate strings; found ${gates.length} — if the file's format changed, FIX THIS EXTRACTION rather than deleting the lint`,
    );
    const offenders = gates.filter((g) => helpers.hasSemicolonChain(g));
    assert.deepEqual(offenders, [], `;-joined gate strings under-count gateCmds AND do not fail fast: ${offenders.join(" | ")}`);
  });

  it("pump.js's own CACHE and GATE constants are &&-chained", () => {
    assert.equal(helpers.hasSemicolonChain(cache), false, "the CACHE prefix must not be ;-chained");
    assert.equal(helpers.hasSemicolonChain(gate), false, "the default GATE must not be ;-chained");
  });

  it("pump.js refuses to LAUNCH a batch whose gate override is ;-chained", () => {
    // A doc convention nothing enforces is how this coupling stayed latent.
    // The misgating guard is the precedent: refuse before any agent spawns.
    assert.ok(
      /hasSemicolonChain\(gateFor\(b\)\)/.test(source),
      "the guard must test each batch entry's FULL resolved gate (CACHE + override), the same string gateCmds splits",
    );
    assert.ok(
      /semicolon-chained gate/.test(source),
      "pump.js must return a named launch-time error for a ;-chained gate",
    );
  });
});

// ── #2686 gap 4: "complete work, incomplete evidence" is a first-class state ──
// PR #2660 (closing #2631) reached wave-close with a green gate and ZERO review
// because its build agent exhausted its turn budget mid-gate. The agent behaved
// correctly — it refused to open a PR on half-run evidence — and the wave had
// no routing for that state except a human noticing.
//
// ⚠ LIMIT, stated because it is load-bearing: this only covers the case where
// the agent still manages to RETURN. An agent that dies without ever calling
// StructuredOutput produces no result at all, and the retro's existing
// "NEVER RAN" row remains the only signal. Nothing here changes that.
describe("out-of-budget builds route as incomplete evidence, not as failures (#2686 gap 4)", () => {
  it("the BUILD prompt tells an out-of-budget agent what to do instead of vanishing", () => {
    assert.ok(
      /RUNNING OUT MID-GATE/.test(source),
      "the BUILD prompt must name the out-of-budget case explicitly",
    );
    assert.ok(
      /RUNNING OUT MID-GATE[\s\S]{0,1200}push/i.test(source),
      "the out-of-budget instruction must tell the agent to push its branch so the work survives",
    );
    assert.ok(
      /RUNNING OUT MID-GATE[\s\S]{0,1200}do NOT open the PR/i.test(source),
      "the out-of-budget instruction must keep the no-PR-on-half-run-evidence rule",
    );
  });

  it("the wave payload separates incomplete-evidence builds from outright failures", () => {
    assert.ok(source.includes("incompleteEvidence"), "the payload must carry an incompleteEvidence bucket");
    assert.ok(
      /incompleteEvidence[\s\S]{0,600}buildFailed/.test(source) || /buildFailed[\s\S]{0,600}incompleteEvidence/.test(source),
      "incompleteEvidence must sit alongside buildFailed, not replace it",
    );
    assert.ok(
      /re-queue|requeue/i.test(source),
      "the retro must be told these need finishing, not rebuilding from scratch",
    );
  });
});

// ── #2673: the #2665 probe record gets a version pin and a re-probe trigger ───
// The probe established BY DIRECT EXPERIMENT that the harness rejects a short
// `gateResults` array. That record now sits in SKILL.md with nothing to
// invalidate it: a CLI upgrade could silently drop enforcement and every
// downstream claim would keep reading as true.
//
// ⚠ NO IN-TREE TEST CAN DETECT A HARNESS THAT STOPS ENFORCING. That boundary is
// stated in this file's header and is NOT softened here. These tests check the
// RECORD and the TRIGGER's wiring — that a version is pinned beside the probe,
// that a re-probe condition is written down, and that a live phase is actually
// instructed to compare the running harness against it. Whether the retro agent
// obeys that instruction is the same trust the rest of the prompts run on.
describe("the #2665 harness probe carries a version pin and a re-probe trigger (#2673)", () => {
  const skill = readFileSync(SKILL_MD, "utf8");
  // Slice the probe SECTION, not "everything after the first mention of it" —
  // otherwise a semver or a trigger sentence anywhere later in the file would
  // satisfy these assertions and they would drift into vacuous truth.
  const sectionStart = skill.indexOf("## Harness enforcement of the build schema");
  assert.ok(sectionStart !== -1, "SKILL.md must keep the probe record as its own section");
  const rest = skill.slice(sectionStart + 3);
  const sectionEnd = rest.indexOf("\n## ");
  const record = sectionEnd === -1 ? rest : rest.slice(0, sectionEnd);

  it("SKILL.md pins the CLI version the probe was observed under", () => {
    assert.ok(
      /probed under|observed under|CLI version/i.test(record),
      "the probe record must name the CLI version it was observed under",
    );
    assert.ok(
      /\b\d+\.\d+\.\d+\b/.test(record),
      "the probe record must carry a concrete semver, not just a date",
    );
  });

  it("SKILL.md states a re-probe trigger, not just a version", () => {
    assert.ok(/RE-PROBE (?:TRIGGER|WHEN)/i.test(record), "the record must state WHEN to re-probe");
    assert.ok(
      /claude --version/.test(record),
      "the trigger must name the concrete command that reads the running harness version",
    );
  });

  it("a live phase is instructed to compare the running harness against the pinned version", () => {
    // A trigger nobody evaluates is a sentence. The retro runs once per wave
    // and is already this round's reader.
    assert.ok(
      /claude --version/.test(source),
      "some pump.js prompt must actually read the running harness version",
    );
    assert.ok(
      /claude --version[\s\S]{0,800}(?:SKILL\.md|#2665)/.test(source),
      "the version read must be compared against the SKILL.md probe record",
    );
  });

  it("the BRINK-CONFIG.md house-rule handoff has a tracked home the lessons phase reads", () => {
    // #2673 gap 2: #2669 correctly did NOT edit BRINK-CONFIG.md (the lessons
    // phase owns a per-wave PR against it), but nothing tracked the handoff —
    // so if the lessons agent misses the issue, the rule evaporates. That is
    // the exact failure mode #2665 was opened about. Parking the pending text
    // in SKILL.md and pointing the lessons PROMPT at it makes the handoff
    // survive in the file the agent is told to read.
    assert.ok(
      skill.includes("Pending BRINK-CONFIG.md house rules"),
      "SKILL.md must carry the pending-handoff section",
    );
    assert.ok(
      /a schema constraint is enforcement only if a validator honours it/i.test(skill),
      "the pending section must carry #2669's proposed rule text verbatim enough to paste",
    );
    assert.ok(
      source.includes("Pending BRINK-CONFIG.md house rules"),
      "the lessons prompt must point at the pending-handoff section by name",
    );
  });

  it("SKILL.md names itself as the home for this record, so a future issue does not re-hit the fence", () => {
    // #2673 gap 3: #2665 asked for the result in BRINK-CONFIG.md; the wave's
    // fence forbade touching that file, so it landed here instead.
    assert.ok(
      /this record's home is `?SKILL\.md`?/i.test(record),
      "the record must say SKILL.md is its home (#2673 gap 3)",
    );
  });
});
