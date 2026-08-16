// Runs scripts/refresh-excluded-lockfiles.test.sh under node:test (#2667).
//
// WHY A WRAPPER. The real harness is bash, because it drives the real bash
// script against PATH-injected stubs — the same shape as
// scripts/setup-dev.test.sh, which is the precedent for proving timeout
// CONTROL FLOW rather than grepping for it. But CI runs exactly two
// script-test steps: `pnpm test:scripts` (`node --test scripts/*.test.mjs`)
// and `pnpm test:setup-dev` (that one bash file, under a step named
// "Unit tests (scripts/setup-dev.sh)"). Folding a second script into that
// step would make its name describe a subset of what it runs, which is the
// thing #2610/#2613 rule against. So the bash harness is picked up by the
// glob instead, through this wrapper, and CI needs no change.
//
// Run the harness directly with `pnpm test:refresh-lockfiles` when iterating —
// its own output names each assertion.

import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import assert from "node:assert/strict";

const here = dirname(fileURLToPath(import.meta.url));
const harness = join(here, "refresh-excluded-lockfiles.test.sh");

// The harness deliberately waits on real 1s timeouts firing against a stub
// that sleeps 30s, several times over; 120s is slack, not an expectation.
test("scripts/refresh-excluded-lockfiles.test.sh passes", { timeout: 120_000 }, () => {
  const result = spawnSync("bash", [harness], {
    cwd: resolve(here, ".."),
    encoding: "utf8",
    timeout: 110_000,
  });

  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;

  assert.equal(result.error, undefined, `could not run the harness: ${result.error?.message}`);
  assert.equal(result.status, 0, `refresh-excluded-lockfiles.test.sh failed:\n${output}`);

  // Non-vacuity: a harness that ran nothing would also exit 0.
  const passes = (output.match(/^ok - /gm) ?? []).length;
  assert.equal(passes >= 15, true, `expected the harness to report 15+ assertions, saw ${passes}:\n${output}`);
});
