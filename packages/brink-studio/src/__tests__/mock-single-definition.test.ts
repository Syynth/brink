import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// Guard against a class method being defined twice in the brink-web mock.
//
// THE INCIDENT (2026-08-16, on `main`): `EditorSession.resolve_code_action_doc`
// existed twice — a faithful implementation sharing `resolveCodeActionImpl`
// with its `resolve_code_action` sibling (#2583, for #2577), and, 400 lines
// further down in the doc-handle block, a stub that always refused (#2585, for
// #2578). JS class semantics make the LATER definition win, silently: the
// doc-handle op could no longer succeed at all under the mock, and its refusal
// vocabulary changed from "unknown document handle" to "unknown handle",
// reddening structural-refusal-shape.test.ts.
//
// It reached `main` because each PR was green against a `main` that did not yet
// contain the other — the collision only exists in the merge result, which no
// PR's CI ever evaluated. That is a property of the merge order, not of either
// change, so it will recur; hence a guard rather than a fix alone.
//
// TypeScript does flag this as TS2393, but only under `typecheck` — the vitest
// suite runs happily on the shadowed mock and reports the resulting wrong
// behaviour as an ordinary assertion failure elsewhere. This guard puts the
// diagnosis in the suite that observes the symptom.
//
// Scoped to this one file deliberately: a mock class is a hand-maintained
// mirror of a Rust surface, appended to by many separate PRs, which is what
// makes silent shadowing likely here and unremarkable elsewhere.

const MOCK_PATH = resolve(fileURLToPath(import.meta.url), "../../__mocks__/brink-web.ts");

interface Definition {
  readonly className: string;
  readonly method: string;
  readonly line: number;
}

/**
 * Collects class-body method definitions, keyed by the class that encloses
 * them. Deliberately a line scanner rather than a parser: it must agree with
 * what a reader sees, and the mock is plain, uniformly-formatted class bodies.
 *
 * Exported for the self-test below — a guard whose detector is never exercised
 * on a known-bad input is the vacuity this repo keeps re-earning.
 */
export function collectMethodDefinitions(source: string): Definition[] {
  // A class-body member at exactly two spaces of indentation: `name(`,
  // optionally preceded by member modifiers. Excludes control-flow keywords,
  // which can also appear as `  if (` at that indentation.
  const MEMBER = /^ {2}(?:(?:private|public|protected|static|async|get|set|readonly)\s+)*([A-Za-z_$][\w$]*)\s*\(/;
  const CLASS = /^(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)/;
  const NOT_A_MEMBER = new Set(["if", "for", "while", "switch", "catch", "return", "constructor"]);

  const definitions: Definition[] = [];
  let className: string | null = null;

  source.split("\n").forEach((line, index) => {
    const startsClass = CLASS.exec(line);
    if (startsClass) {
      className = startsClass[1] ?? null;
      return;
    }
    const member = MEMBER.exec(line);
    if (!member) return;
    const method = member[1];
    if (method === undefined || NOT_A_MEMBER.has(method) || className === null) return;
    definitions.push({ className, method, line: index + 1 });
  });

  return definitions;
}

function findDuplicates(definitions: readonly Definition[]): string[] {
  const byKey = new Map<string, number[]>();
  for (const { className, method, line } of definitions) {
    const key = `${className}.${method}`;
    const lines = byKey.get(key);
    if (lines === undefined) byKey.set(key, [line]);
    else lines.push(line);
  }
  return [...byKey.entries()]
    .filter(([, lines]) => lines.length > 1)
    .map(([key, lines]) => `${key} defined ${lines.length}× (lines ${lines.join(", ")})`)
    .sort();
}

describe("brink-web mock: one definition per method", () => {
  const source = readFileSync(MOCK_PATH, "utf8");
  const definitions = collectMethodDefinitions(source);

  it("finds the mock's methods at all", () => {
    // Without this, an over-tightened MEMBER pattern would report an empty set
    // and pass the duplicate check for the wrong reason.
    expect(definitions.length).toBeGreaterThan(100);
    expect(definitions.map((d) => `${d.className}.${d.method}`)).toContain(
      "EditorSession.resolve_code_action_doc",
    );
  });

  it("defines no class method twice", () => {
    expect(findDuplicates(definitions)).toEqual([]);
  });

  it("reports a duplicate when one exists", () => {
    // The self-test: the same detector, run over a shadowing pair shaped like
    // the real incident (two definitions of one method in one class body,
    // separated by unrelated members).
    const planted = [
      "class EditorSession {",
      "  resolve_code_action_doc(doc: number): string {",
      '    return "faithful";',
      "  }",
      "  inlay_hints_doc(): string {",
      '    return "[]";',
      "  }",
      "  resolve_code_action_doc(doc: number): string {",
      '    return "stub";',
      "  }",
      "}",
      "class Story {",
      "  free(): void {}",
      "}",
    ].join("\n");

    expect(findDuplicates(collectMethodDefinitions(planted))).toEqual([
      "EditorSession.resolve_code_action_doc defined 2× (lines 2, 8)",
    ]);
  });

  it("does not report the same method name in two different classes", () => {
    // `free`, `continue_single` and friends legitimately appear in several
    // mock classes; keying by class is what keeps those out of the report.
    const planted = ["class EditorSession {", "  free(): void {}", "}", "class Story {", "  free(): void {}", "}"].join(
      "\n",
    );

    expect(findDuplicates(collectMethodDefinitions(planted))).toEqual([]);
  });
});
