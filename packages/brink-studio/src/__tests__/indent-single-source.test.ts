/**
 * `[project] indent` has ONE value, and every component reads it (#3149,
 * ruled 2026-08-27: "everything that indents reads the same setting").
 *
 * The failure mode the ruling exists to prevent is disagreement — a
 * formatter writing two spaces under guides drawn every four looks like a
 * rendering glitch rather than a config mismatch, and the author cannot
 * tell which component is wrong. That bug is latent in any implementation
 * where a component keeps its own default.
 *
 * The Rust side pins its own half (`brink-fmt`'s
 * `the_default_indent_comes_from_the_project_config` asserts against
 * `brink_project_config::DEFAULT_INDENT` rather than a literal). This file
 * pins the half that crosses the language boundary, which nothing else can:
 * the editor's `DEFAULT_INDENT` mirror, and the fact that a configured
 * width actually reaches an editor view.
 */

import { describe, expect, it } from "vitest";
import { DEFAULT_INDENT } from "@brink-lang/editor";

describe("the indent default (#3149)", () => {
  it("matches the Rust DEFAULT_INDENT", async () => {
    // Read out of the Rust source rather than restated here: a test that
    // writes `4` on both sides proves only that this file agrees with
    // itself. If the constant moves in Rust and not here, this fails —
    // which is the entire point.
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    // `process.cwd()` is the package root under vitest (its config sets the
    // root there), so this resolves from a stable anchor rather than from
    // `import.meta.url`, which the transform rewrites.
    const lib = resolve(
      process.cwd(),
      "../../crates/internal/brink-project-config/src/lib.rs",
    );
    const source = readFileSync(lib, "utf8");
    const match = /pub const DEFAULT_INDENT: u8 = (\d+);/.exec(source);
    expect(match, "DEFAULT_INDENT not found in brink-project-config").not.toBeNull();
    expect(DEFAULT_INDENT).toBe(Number(match![1]));
  });

  it("is a usable indent width", () => {
    // Guards the regex above against matching something absurd and this
    // whole file passing for the wrong reason.
    expect(Number.isInteger(DEFAULT_INDENT)).toBe(true);
    expect(DEFAULT_INDENT).toBeGreaterThan(0);
    expect(DEFAULT_INDENT).toBeLessThanOrEqual(16);
  });
});
