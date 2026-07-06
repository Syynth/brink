/**
 * Dialect conformance corpus (#368): the same JSON fixture
 * (`tests/dialect_fixtures/at_cue.json`) that pins the Rust classifier
 * (`brink-ide`'s `line_context.rs` dialect_tests) also pins the TS
 * interpreter (`@brink-lang/editor`'s `dialect.ts`) — the anti-drift gate
 * the dialect spec requires. Each case classifies one already-trimmed line
 * in isolation (or, for `chain_after` cases, simulates the one-line-back
 * chain context the Rust `apply_dialect` chain pass reproduces) and checks
 * the result against `expect` (`null` = must NOT classify — negative
 * fixtures).
 */

import { describe, expect, it } from "vitest";
import { AT_CUE_DIALECT, ResolvedDialect } from "@brink-lang/editor";
// Imported directly (resolveJsonModule) rather than read via node:fs — this
// file's tsconfig has no @types/node, and a direct JSON import is the more
// idiomatic path anyway: the same fixture file both interpreters (Rust's
// `dialect_conformance.rs`, this suite) are pinned against, with Vite/vitest
// handling the module resolution.
import fixture from "../../../../tests/dialect_fixtures/at_cue.json";

interface FixtureCase {
  id: string;
  description: string;
  line: string;
  chain_after?: string;
  chain_after_attrs?: Record<string, string>;
  expect: { kind: string; attrs?: Record<string, string> } | null;
}

const cases = fixture.cases as FixtureCase[];

describe(`dialect conformance corpus: ${fixture.dialect}`, () => {
  const dialect = ResolvedDialect.compile(AT_CUE_DIALECT);

  it("loads a non-empty corpus", () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  for (const c of cases) {
    it(`${c.id}: ${c.description}`, () => {
      if (c.chain_after !== undefined) {
        // Simulate the one-line-back chain context directly against the
        // dialect's chain rules (mirrors the Rust chain pass in
        // `line_context.rs`'s `apply_dialect`): a chain rule fires when the
        // immediately preceding line's dialect kind is in `after`.
        const rule = dialect.chainRuleAfter(c.chain_after);
        if (c.expect === null) {
          expect(rule).toBeNull();
          return;
        }
        expect(rule).not.toBeNull();
        expect(rule!.becomes).toBe(c.expect.kind);
        const carriedAttrs = c.expect.attrs ?? {};
        const carryNames = rule!.carry ?? [];
        const carried: Record<string, string> = {};
        for (const name of carryNames) {
          const value = c.chain_after_attrs?.[name];
          if (value !== undefined) carried[name] = value;
        }
        expect(carried).toEqual(carriedAttrs);
        return;
      }

      const match = dialect.classify(c.line, 0);
      if (c.expect === null) {
        expect(match).toBeNull();
        return;
      }
      expect(match).not.toBeNull();
      expect(match!.kind).toBe(c.expect.kind);
      const attrs = Object.fromEntries(match!.attrs);
      expect(attrs).toEqual(c.expect.attrs ?? {});
    });
  }
});
