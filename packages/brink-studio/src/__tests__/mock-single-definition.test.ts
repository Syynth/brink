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
//
// ⚠ THIS GUARD IS NOT THE PRIMARY DEFENCE, and should not be read as one.
// `tsc` catches every shadowing shape as TS2393/TS2300, including several this
// scanner cannot see (a duplicated arrow-property member, a class expression),
// and `pnpm --filter @brink-lang/studio typecheck` runs in CI. What this adds
// is a diagnosis in the suite that observes the *symptom*: the vitest run
// happily executes a shadowed mock and reports the consequence as an unrelated
// assertion failure somewhere else, which is how the incident actually
// presented. Treat a red here as a pointer, and `tsc` as the gate.

const MOCK_PATH = resolve(fileURLToPath(import.meta.url), "../../__mocks__/brink-web.ts");

interface Definition {
  readonly className: string;
  /** Accessors are keyed separately: a legal `get x`/`set x` pair is not a duplicate. */
  readonly method: string;
  readonly line: number;
}

/**
 * Blanks out comments and string/template literals, preserving line structure
 * and brace balance, so the scanner below cannot be fooled by a commented-out
 * method sitting above its live replacement, or by a `class { … }` inside a
 * template literal. Replaces content with spaces rather than deleting it, so
 * reported line numbers still match the file a reader opens.
 */
function blankNonCode(source: string): string {
  const out = source.split("");
  let mode: "code" | "line" | "block" | '"' | "'" | "`" = "code";
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    const next = source[i + 1];
    if (mode === "code") {
      if (c === "/" && next === "/") mode = "line";
      else if (c === "/" && next === "*") mode = "block";
      else if (c === '"' || c === "'" || c === "`") mode = c;
      else continue;
      // Reached only for `/`, `"`, `'` or a backtick — never a newline.
      out[i] = " ";
      continue;
    }
    // Inside a non-code run: blank everything but newlines, then look for the end.
    if (c === "\\") {
      out[i] = " ";
      if (next !== undefined && next !== "\n") out[i + 1] = " ";
      i++;
      continue;
    }
    if (c !== "\n") out[i] = " ";
    if (mode === "line" && c === "\n") mode = "code";
    else if (mode === "block" && c === "*" && next === "/") {
      out[i + 1] = " ";
      i++;
      mode = "code";
    } else if ((mode === '"' || mode === "'" || mode === "`") && c === mode) mode = "code";
    else if ((mode === '"' || mode === "'") && c === "\n") mode = "code";
  }
  return out.join("");
}

/**
 * Collects class-body member definitions, keyed by the class that encloses
 * them. A line scanner rather than a parser — it must agree with what a reader
 * sees, and the mock is plain, uniformly-formatted class bodies — but it tracks
 * brace depth so that a member is only recognised at a class body's own depth.
 * Without that, an ordinary two-space-indented call statement inside a
 * top-level function further down the file is read as a member of whatever
 * class was declared last; this file has top-level functions both before and
 * after its classes, so that vector is live, not hypothetical.
 *
 * Not exported: sharing it would require importing this `.test.ts` file, which
 * re-registers its tests in the importer (CLAUDE.md § Rules, #2516). The
 * self-tests below live in this same file and need no export.
 */
function collectMethodDefinitions(rawSource: string): Definition[] {
  const source = blankNonCode(rawSource);
  // A class-body member: `name(`, an accessor, or an arrow-property member,
  // optionally preceded by member modifiers. Quoted names count — `"name"()`
  // shadows `name()` just as silently.
  const MEMBER =
    /^\s*(?:(?:private|public|protected|static|async|readonly|override|declare)\s+)*(?:(get|set)\s+)?(?:"([^"]+)"|'([^']+)'|([A-Za-z_$][\w$]*))\s*(?:\(|=\s*(?:async\s*)?\()/;
  const CLASS = /(?:^|[\s=(])(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)?/;
  const NOT_A_MEMBER = new Set([
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "return",
    "constructor",
    "function",
    "typeof",
    "await",
    "new",
    "do",
  ]);

  const definitions: Definition[] = [];
  // Stack of open class bodies: the depth at which their members sit.
  const openClasses: { name: string; bodyDepth: number }[] = [];
  let depth = 0;
  let pendingClass: string | null = null;

  source.split("\n").forEach((line, index) => {
    const startsClass = CLASS.exec(line);
    // `class` with no name (a class expression) still opens a body that must be
    // tracked, so its members are not misattributed to an enclosing class.
    if (startsClass) pendingClass = startsClass[1] ?? "<anonymous>";

    const enclosing = openClasses[openClasses.length - 1];
    if (enclosing !== undefined && depth === enclosing.bodyDepth && pendingClass === null) {
      const member = MEMBER.exec(line);
      const name = member?.[2] ?? member?.[3] ?? member?.[4];
      // A TS overload signature (`foo(a: string): void;`) is a DECLARATION, and
      // several of them legally share one name. A definition opens a body (`{`)
      // or is an arrow property (`=>`). Where the two are ambiguous, prefer
      // missing a duplicate over reddening CI on legal code — `tsc` is the gate.
      const declarationOnly =
        /;\s*$/.test(line) && !line.includes("{") && !line.includes("=>");
      if (member && name !== undefined && !NOT_A_MEMBER.has(name) && !declarationOnly) {
        const accessor = member[1];
        definitions.push({
          className: enclosing.name,
          method: accessor === undefined ? name : `${accessor} ${name}`,
          line: index + 1,
        });
      }
    }

    for (const char of line) {
      if (char === "{") {
        depth++;
        if (pendingClass !== null) {
          openClasses.push({ name: pendingClass, bodyDepth: depth });
          pendingClass = null;
        }
      } else if (char === "}") {
        const top = openClasses[openClasses.length - 1];
        if (top !== undefined && top.bodyDepth === depth) openClasses.pop();
        depth--;
      }
    }
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

  it("detects the historical bad state in main's own file", () => {
    // The strongest anti-vacuity check available: the detector must report the
    // real incident when run over the real file as it stood while main was red.
    const beforeRepair = [
      "class EditorSession {",
      "  /** Document-handle variant. */",
      "  resolve_code_action_doc(doc: number, dataJson: string, offset: number): string {",
      "    return this.resolveCodeActionImpl(d.path, dataJson, offset);",
      "  }",
      "  private resolveCodeActionImpl(path: string): string {",
      '    return "";',
      "  }",
      "  code_actions_doc(_doc: number): string {",
      '    return "[]";',
      "  }",
      "  resolve_code_action_doc(doc: number, _dataJson: string, _offset: number): string {",
      '    return EditorSession.structuralRefusal("unknown handle");',
      "  }",
      "}",
    ].join("\n");

    expect(findDuplicates(collectMethodDefinitions(beforeRepair))).toEqual([
      "EditorSession.resolve_code_action_doc defined 2× (lines 3, 12)",
    ]);
  });

  // Each of these was a FALSE POSITIVE in this guard's first draft, found by
  // the review of PR #2599 by running the detector over hand-built inputs.
  // Legal TypeScript must not redden CI.
  const legal: readonly (readonly [string, string])[] = [
    [
      "a get/set accessor pair",
      ["class S {", "  get activePath(): string {", '    return "";', "  }", "  set activePath(v: string) {", "    this.p = v;", "  }", "}"].join("\n"),
    ],
    [
      "overload signatures above their implementation",
      ["class S {", "  code_actions(offset: number): string;", "  code_actions(doc: number, offset: number): string;", "  code_actions(a: number, b?: number): string {", '    return "[]";', "  }", "}"].join("\n"),
    ],
    [
      "a block-commented-out method above its replacement",
      ["class S {", "  /*", "  resolve_code_action_doc(doc: number): string {", '    return "old";', "  }", "  */", "  resolve_code_action_doc(doc: number): string {", '    return "new";', "  }", "}"].join("\n"),
    ],
    [
      "repeated call statements in a top-level function after a class",
      ["class S {", "  initWasm(): void {}", "}", "function boot(): void {", "  initWasm();", "  initWasm();", "}"].join("\n"),
    ],
    [
      "a class inside a template literal",
      ["class S {", "  real(): void {}", "}", "const src = `", "class Fake {", "  dup(): void {}", "  dup(): void {}", "}", "`;"].join("\n"),
    ],
    [
      "the same method name in two different classes",
      ["class EditorSession {", "  free(): void {}", "}", "class Story {", "  free(): void {}", "}"].join("\n"),
    ],
    [
      "a nested class expression with a same-named member",
      ["class Outer {", "  build(): unknown {", "    const Inner = class {", "      build(): void {}", "    };", "    return Inner;", "  }", "}"].join("\n"),
    ],
  ];

  it.each(legal)("does not flag legal code: %s", (_name, source) => {
    expect(findDuplicates(collectMethodDefinitions(source))).toEqual([]);
  });

  // Shapes the scanner CANNOT see. Recorded so the blind spots are stated
  // rather than discovered — `tsc` reports every one as TS2393/TS2300, which is
  // why the header names typecheck as the gate and this guard as the pointer.
  it("documents its blind spots: duplicate arrow-property members without a call form", () => {
    const shadowed = ["class S {", "  dup = async (): Promise<void> => {};", "  dup = async (): Promise<void> => {};", "}"].join("\n");
    // This one the scanner DOES catch (the `= (` alternative)...
    expect(findDuplicates(collectMethodDefinitions(shadowed))).toHaveLength(1);

    // ...but a non-function duplicate property has no call form at all.
    const propertyShadow = ["class S {", '  dup = "a";', '  dup = "b";', "}"].join("\n");
    expect(findDuplicates(collectMethodDefinitions(propertyShadow))).toEqual([]);
  });
});
