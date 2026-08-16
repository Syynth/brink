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
      "minItems", "minLength", "enum", "description",
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
