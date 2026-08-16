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

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PUMP_JS = join(__dirname, "..", ".claude", "skills", "autonomous-pump", "pump.js");

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
  `${helpersSrc}\nreturn { gateFor, gateCmds, buildSchemaFor, formatGateEvidence };`,
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
});
