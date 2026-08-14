import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, join } from "node:path";
import { describe, expect, it } from "vitest";

// Directory-level guard for the main-guard invariant (#2478).
//
// Two rounds of "script X has no main-guard/export seam" happened one at a
// time — #2452 named `ensure-cli-sidecar.mjs`, then #2468 had to separately
// name its sibling `ensure-wasm.mjs` after #2452 shipped without it. The
// invariant behind both ("every packages/brink-desktop/scripts/*.mjs
// reachable from a package.json script must be inert on import and only act
// behind a main-guard") lived only as prose in docs/desktop-shell-spec.md's
// "The `dev` preflight pair" section and as pointer comments in the two
// scripts. Nothing enforced it, so a third preflight script would have
// landed unguarded exactly as `ensure-wasm.mjs` did.
//
// This file enumerates the directory rather than naming files, so a third
// script is checked the moment it lands, without a human remembering to ask
// for it. `ensure-wasm.test.ts` / `ensure-cli-sidecar.test.ts` keep their
// own `describe("the main-guard")` blocks: those spawn a child node and
// prove the runtime behaviour (inert on import; still acts standalone) for
// the two scripts we have. This scan is the cheap structural net underneath
// them that no new file can slip past.
//
// SCOPE (settled deliberately, see the PR for #2478): this covers
// `packages/brink-desktop/scripts/` only — the directory the issue names and
// the one "The `dev` preflight pair" governs. The repo ROOT also has a
// `scripts/check-wasm-pkg.mjs` (#2479) that carries the identical idiom, but
// it is a different package's tooling, is exercised by Node's built-in test
// runner (`pnpm test:scripts`) rather than Vitest, and is not part of the
// `dev` preflight pair. Widening this invariant to the repo root is a real
// question and is NOT ruled on here; it is raised on #2478 instead of being
// silently answered by a test reaching across the package fence.

const scriptsDir = resolve(fileURLToPath(import.meta.url), "../../../scripts");

// ⚠ A scan that matches nothing passes forever. This roster pins the exact
// set the scan is expected to find, so an empty or mis-rooted `scriptsDir`
// fails loudly instead of vacuously reporting zero violations. Adding a
// preflight script means adding its name here — one line — while the guard
// assertions below apply to it automatically.
const EXPECTED_SCRIPTS = ["ensure-cli-sidecar.mjs", "ensure-wasm.mjs"];

// The exact idiom both scripts use verbatim, and the one named in #2478.
// The leading `process.argv[1] &&` is load-bearing, not decorative: with no
// script path at all — `node --input-type=module -e "await import(...)"`,
// which is precisely how `ensure-wasm.test.ts` proves the module is inert —
// `argv[1]` is undefined and `pathToFileURL(undefined)` throws, so a guard
// written without it makes importing the module fail instead of doing
// nothing. Requiring the whole line is therefore stricter on purpose.
const MAIN_GUARD_PATTERN =
  /if\s*\(\s*process\.argv\[1\]\s*&&\s*import\.meta\.url\s*===\s*pathToFileURL\(process\.argv\[1\]\)\.href\s*\)/;

// "exports its core logic as a named function (not only top-level
// imperative code)" — the second item of #2478's fix shape.
const NAMED_EXPORT_PATTERN =
  /^export\s+(?:async\s+function|function|const|class)\s+\w+/m;

function listPreflightScripts(dir: string): string[] {
  return readdirSync(dir)
    .filter((name) => name.endsWith(".mjs") && !name.endsWith(".test.mjs"))
    .sort();
}

function hasMainGuard(source: string): boolean {
  return MAIN_GUARD_PATTERN.test(source);
}

function hasNamedExport(source: string): boolean {
  return NAMED_EXPORT_PATTERN.test(source);
}

describe("packages/brink-desktop/scripts/*.mjs carry the main-guard (#2478)", () => {
  it("scans a directory that actually holds the known preflight scripts", () => {
    // Exact set, not `length > 0`: this is the assertion that stops the
    // whole file from passing green over an empty match.
    expect(listPreflightScripts(scriptsDir)).toEqual(EXPECTED_SCRIPTS);
  });

  for (const file of listPreflightScripts(scriptsDir)) {
    describe(file, () => {
      const source = readFileSync(join(scriptsDir, file), "utf8");

      it("exports its core logic as a named binding", () => {
        expect(hasNamedExport(source)).toBe(true);
      });

      it("carries the import.meta.url === pathToFileURL(process.argv[1]).href main-guard", () => {
        expect(hasMainGuard(source)).toBe(true);
      });
    });
  }

  // The two predicates above decide pass/fail for every real script, but
  // against already-compliant files they only ever run in the true
  // direction. These pin the false direction directly, so the check is
  // known to be capable of failing rather than assumed to be.
  describe("hasMainGuard", () => {
    it("rejects a script whose side effect runs at import time", () => {
      // The shape `ensure-wasm.mjs` had before #2468: logic exported, but
      // nothing gating the standalone-run call from a plain `import`.
      const unguarded = ["export function ensureWasm() {}", "ensureWasm();"].join(
        "\n",
      );
      expect(hasMainGuard(unguarded)).toBe(false);
    });

    it("rejects a near-miss guard that omits the process.argv[1] check", () => {
      // Not pedantry: without the presence check this line throws under
      // `node -e`, where `argv[1]` is undefined — so the module is not
      // inert on import, which is the property the guard exists to give.
      const nearMiss = [
        "export function ensureWasm() {}",
        "if (import.meta.url === pathToFileURL(process.argv[1]).href) {",
        "  ensureWasm();",
        "}",
      ].join("\n");
      expect(hasMainGuard(nearMiss)).toBe(false);
    });

    it("accepts the real guard line", () => {
      const guarded = [
        "export function ensureWasm() {}",
        "if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {",
        "  ensureWasm();",
        "}",
      ].join("\n");
      expect(hasMainGuard(guarded)).toBe(true);
    });
  });

  describe("hasNamedExport", () => {
    it("rejects a script that is only top-level imperative code", () => {
      expect(hasNamedExport("run();\nprocess.exit(0);")).toBe(false);
    });

    it("rejects a default-only export, which gives the tests no named seam", () => {
      expect(hasNamedExport("export default function () {}")).toBe(false);
    });

    it("accepts a named function export", () => {
      expect(hasNamedExport("export function ensureWasm() {}")).toBe(true);
    });
  });
});
