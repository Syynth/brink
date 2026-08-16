/**
 * Mock ⇄ Rust refusal-shape parity (#2568).
 *
 * The studio's wasm mock (`src/__mocks__/brink-web.ts`) is the only thing 1000+
 * studio tests ever talk to. When it *understates* a payload, every one of those
 * tests is blind to bugs living in the fields it omits — which is literally how
 * #2543 shipped: `error_json` (`crates/brink-web/src/editor_refactor.rs`)
 * serializes the WHOLE `StructuralResultJs`, so a REFUSAL ships `safe: true`
 * with empty `cross_file_edits`/`introduced_diagnostics` (only `path`,
 * `new_source` and `error` carry `skip_serializing_if`), while the mock answered
 * `{ ok: false, error }` alone. Under the mock a refusal therefore read as
 * *unsafe* (`isSafeRename` → false, report shown, nothing committed); in
 * production it read as *safe* and was committed.
 *
 * PR #2564 fixed the two rename sites. This file sweeps the rest and pins the
 * contract so it cannot drift again.
 *
 * ## Why the expectations are not hand-written
 *
 * `crates/brink-web/fixtures/refusal-shapes.json` is GENERATED from the Rust
 * payloads themselves (`error_json`, `dir_error_json`, and an `AutoImportJs`
 * struct literal) by `refusal_shape::refusal_shape_fixture_matches_the_rust_payloads`
 * in `crates/brink-web/src/editor_refactor.rs`. That Rust test fails the moment
 * a field is added, renamed, or gains/loses `skip_serializing_if`, forcing a
 * regenerate; this file then fails until the mock matches. A hand-copied field
 * list would drift exactly the way the thing it guards drifted, so the list is
 * read off the Rust type instead — no field name below is typed by hand.
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";
import { isSafeRename } from "@brink-lang/editor";

import { EditorSession } from "../__mocks__/brink-web";

/**
 * The Rust crate owns the fixture; this is the read side. Resolved from this
 * file rather than `new URL(<literal>, import.meta.url)` — Vite statically
 * rewrites that pattern into a served asset URL, which `fileURLToPath` then
 * rejects. Same derivation `no-test-file-imports.test.ts` uses.
 */
const repoRoot = resolve(fileURLToPath(import.meta.url), "../../../../../");
const FIXTURE_PATH = resolve(repoRoot, "crates/brink-web/fixtures/refusal-shapes.json");

interface RefusalFixture {
  /** The refusal message baked into every generated shape below. */
  error: string;
  shapes: Record<string, Record<string, unknown>>;
}

const fixture = JSON.parse(readFileSync(FIXTURE_PATH, "utf8")) as RefusalFixture;

/** The generated shape with its placeholder message swapped for a real one. */
function refusalShape(name: string, error: string): Record<string, unknown> {
  const shape = fixture.shapes[name];
  expect(shape, `fixture is missing the ${name} shape — regenerate it`).toBeDefined();
  return { ...shape!, error };
}

const MAIN = "=== hello ===\nHi.\n-> END\n";

/** Two knots, the first carrying stitches — enough shape for the reorder /
 *  move / promote / demote refusals to reach past their "not found" guards. */
const TWO_KNOTS =
  "=== one ===\nFirst.\n= a\nA.\n= b\nB.\n\n=== two ===\nSecond.\n= a\nOther A.\n";

function sessionWith(files: Record<string, string>): EditorSession {
  const s = new EditorSession();
  for (const [path, source] of Object.entries(files)) s.update_file(path, source);
  return s;
}

/** Every call site in the mock that answers with a *refused* structural op. */
const structuralRefusals: Array<{ site: string; error: string; call: () => string }> = [
  {
    site: "rename_file (read-only library)",
    error: "cannot rename: file is part of the read-only library",
    call: () => {
      const s = new EditorSession();
      s.__mockMarkReadOnlyForTest("std/lib.ink", MAIN);
      return s.rename_file("std/lib.ink", "mine.ink");
    },
  },
  {
    site: "rename_file (file not loaded)",
    error: "file not loaded",
    call: () => sessionWith({ "main.ink": MAIN }).rename_file("ghost.ink", "other.ink"),
  },
  {
    site: "rename_file (target exists)",
    error: "a file already exists at 'other.ink'",
    call: () =>
      sessionWith({ "main.ink": MAIN, "other.ink": MAIN }).rename_file("main.ink", "other.ink"),
  },
  {
    site: "delete_symbol (file not loaded)",
    error: "file not loaded",
    call: () => sessionWith({ "main.ink": MAIN }).delete_symbol("ghost.ink", "hello", ""),
  },
  {
    site: "delete_symbol (symbol not found)",
    error: "symbol not found",
    call: () => sessionWith({ "main.ink": MAIN }).delete_symbol("main.ink", "nowhere", ""),
  },
  {
    site: "extract_to_knot (file not loaded)",
    error: "file not loaded",
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_knot("ghost.ink", 0, 4, "lifted"),
  },
  {
    site: "extract_to_knot (empty selection)",
    error: "empty selection: nothing to extract",
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_knot("main.ink", 4, 4, "lifted"),
  },
  {
    site: "extract_to_function (file not loaded)",
    error: "file not loaded",
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_function("ghost.ink", 0, 4, "lifted"),
  },
  {
    site: "extract_to_function (empty selection)",
    error: "empty selection: nothing to extract",
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_function("main.ink", 4, 4, "lifted"),
  },
  // The two sites PR #2564 already fixed — kept here so the guard covers every
  // structural refusal the mock can emit, not only the newly swept ones.
  {
    site: "rename_symbol (file not loaded)",
    error: "file not loaded",
    call: () => sessionWith({ "main.ink": MAIN }).rename_symbol("ghost.ink", "hello", "", "hi"),
  },
  {
    site: "rename_symbol_at (cannot rename this symbol)",
    error: "cannot rename this symbol",
    call: () => sessionWith({ "main.ink": MAIN }).rename_symbol_at("main.ink", 0, "hi"),
  },
  // The seven `dispatchSymbolAction` ops + the code-action resolver, added to
  // the mock by #2577. These had NO mock method at all before it, so the
  // studio suite could not reach them either way; they route through
  // `structuralRefusal` from their first line rather than growing an inline
  // literal to be swept later.
  {
    site: "reorder_stitch (file not loaded)",
    error: "file not loaded",
    call: () => sessionWith({ "main.ink": MAIN }).reorder_stitch("ghost.ink", "hello", "a", 1),
  },
  {
    site: "reorder_stitch (stitch not found)",
    error: "stitch 'nowhere' not found in knot",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_stitch("main.ink", "one", "nowhere", 1),
  },
  {
    site: "reorder_knot (source knot not found)",
    error: "source knot not found",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_knot("main.ink", "nowhere", 1),
  },
  {
    site: "reorder_stitches (invalid reorder)",
    error: "invalid reorder: list is not a permutation of the existing names",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_stitches("main.ink", "one", ["a"]),
  },
  {
    site: "reorder_knots (invalid reorder)",
    error: "invalid reorder: list is not a permutation of the existing names",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_knots("main.ink", ["one", "one"]),
  },
  {
    site: "move_stitch (destination knot not found)",
    error: "destination knot not found",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).move_stitch("main.ink", "one", "a", "nope"),
  },
  {
    site: "move_stitch (name collision)",
    error: "name collision: 'a' already exists in two",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).move_stitch("main.ink", "one", "a", "two"),
  },
  {
    site: "promote_stitch (name collision with a top-level knot)",
    error: "name collision: 'two' already exists in top-level knots",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).promote_stitch("main.ink", "one", "two"),
  },
  {
    site: "promote_stitch (stitch not found)",
    error: "stitch 'nowhere' not found in knot",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).promote_stitch("main.ink", "one", "nowhere"),
  },
  {
    site: "demote_knot (illegal nesting)",
    error: "illegal nesting: knot has sub-stitches and cannot be demoted",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).demote_knot("main.ink", "one", "two"),
  },
  {
    site: "demote_knot (destination knot not found)",
    error: "destination knot not found",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).demote_knot("main.ink", "two", "nope"),
  },
  {
    // MOCK-ONLY ABBREVIATION: production's serde_json error for an unknown
    // internally-tagged variant is `unknown variant \`Nonsense\`, expected one
    // of \`SortKnots\`, \`SortStitches\`, ...\` at line L column C. This pins the
    // mock's own (shorter) wording, not a claim that it matches production —
    // see the comment at the mock's `default:` arm in brink-web.ts.
    site: "resolve_code_action (unknown variant, mock-only wording)",
    error: "invalid code-action data: unknown variant `Nonsense`",
    call: () => {
      const s = sessionWith({ "main.ink": TWO_KNOTS });
      s.set_active_file("main.ink");
      return s.resolve_code_action(JSON.stringify({ action: "Nonsense" }), 0);
    },
  },
  {
    site: "resolve_code_action (no change)",
    error: "code action produced no change",
    call: () => {
      const s = sessionWith({ "main.ink": TWO_KNOTS });
      s.set_active_file("main.ink");
      return s.resolve_code_action(JSON.stringify({ action: "FormatKnot", knot: "one" }), 0);
    },
  },
  {
    site: "resolve_code_action_doc (unknown document handle)",
    error: "unknown document handle",
    call: () =>
      sessionWith({ "main.ink": TWO_KNOTS }).resolve_code_action_doc(
        999,
        JSON.stringify({ action: "SortKnots" }),
        0,
      ),
  },
];

/** The directory-move refusals, which answer `DirMoveResultJs` — a third Rust
 *  struct again (multi-file: `moved_files`, no `path`/`new_source`). Its shape
 *  was generated into the fixture by PR #2573 with no mock counterpart to check
 *  it against; `rename_dir` (#2577) is that counterpart. */
const dirMoveRefusals: Array<{ site: string; error: string; call: () => string }> = [
  {
    site: "rename_dir (no files under the directory)",
    error: "no files found under directory 'ghost'",
    call: () => sessionWith({ "main.ink": MAIN }).rename_dir("ghost", "other"),
  },
  {
    site: "rename_dir (destination occupied)",
    error: "a file already exists at 'dst/a.ink'",
    call: () =>
      sessionWith({ "src/a.ink": MAIN, "dst/a.ink": MAIN }).rename_dir("src", "dst"),
  },
];

/** The doc-handle refusals, which answer with the `AutoImportJs` shape instead
 *  — a different Rust struct, so a different generated shape. */
const autoImportRefusals: Array<{ site: string; error: string; call: () => string }> = [
  {
    site: "auto_import_include_doc (unknown handle)",
    error: "unknown handle",
    call: () => sessionWith({ "main.ink": MAIN }).auto_import_include_doc(999, "other.ink"),
  },
  {
    site: "auto_import_apply_include_doc (unknown handle)",
    error: "unknown handle",
    call: () => sessionWith({ "main.ink": MAIN }).auto_import_apply_include_doc(999, "other.ink"),
  },
];

describe("mock refusal payloads match the Rust structs (#2568)", () => {
  it("the generated fixture is present and carries the shapes this file reads", () => {
    // Cheap canary: a fixture regenerated after a Rust rename would drop a key
    // here rather than making every case below fail with a confusing diff.
    //
    // `CompileResult` (#2577) is generated but has no mock call site: the
    // mock's `compile_project` has no refusal path to check against. It is
    // pinned here anyway because the Rust-side discovery scan
    // (`every_refusal_struct_is_in_the_fixture`) found it — the fixture's job
    // is to cover every struct that CAN refuse, and a shape landing here ahead
    // of its mock counterpart is exactly how `DirMoveResultJs` waited for
    // `rename_dir`.
    expect(Object.keys(fixture.shapes).sort()).toEqual([
      "AutoImportJs",
      "CompileResult",
      "DirMoveResultJs",
      "StructuralResultJs",
    ]);
  });

  for (const { site, error, call } of structuralRefusals) {
    it(`${site} answers a full StructuralResult`, () => {
      expect(JSON.parse(call()) as unknown).toEqual(refusalShape("StructuralResultJs", error));
    });
  }

  for (const { site, error, call } of autoImportRefusals) {
    it(`${site} answers a full AutoImportResult`, () => {
      expect(JSON.parse(call()) as unknown).toEqual(refusalShape("AutoImportJs", error));
    });
  }

  for (const { site, error, call } of dirMoveRefusals) {
    it(`${site} answers a full DirMoveResult`, () => {
      expect(JSON.parse(call()) as unknown).toEqual(refusalShape("DirMoveResultJs", error));
    });
  }
});

describe("a refused structural op is indistinguishable from an unsafe one (#2568)", () => {
  /**
   * The behavioural half of the guard, and the reason this is not hygiene.
   *
   * `isSafeRename` (`packages/ink-editor/src/breakage.ts`) is
   * `result.safe && result.introduced_diagnostics.length === 0`. Against a mock
   * that omits `safe`, EVERY refusal above answered `false` — i.e. the studio
   * suite saw a refusal as a *breakage report*, the one outcome production never
   * produces for it. Any consumer that only guards the unsafe path (and not
   * `ok`) therefore looked correct under test and was wrong in production.
   *
   * These assertions fail loudly against the pre-#2568 mock and are the shape
   * production actually emits.
   */
  for (const { site, call } of structuralRefusals) {
    it(`${site}: the editor's safety gate sees the production answer`, () => {
      const parsed = JSON.parse(call()) as Parameters<typeof isSafeRename>[0];
      expect(parsed.ok).toBe(false);
      // `safe` does not mean "the op happened" — `ok` does. Pinning it `true`
      // keeps the lie that hid #2543 visible instead of papering over it here;
      // making refusals report `safe: false` is #2544's production-side call.
      expect(isSafeRename(parsed)).toBe(true);
      expect(parsed.introduced_diagnostics).toEqual([]);
      expect(parsed.cross_file_edits).toEqual([]);
    });
  }
});

/**
 * Guards the *enumeration* above, not just the shapes it checks.
 *
 * The `structuralRefusals`/`autoImportRefusals` arrays list call sites by
 * hand. A NEW mock method that answers its own inline `{ ok: false, error }`
 * literal instead of routing through `structuralRefusal`/`autoImportRefusal`
 * turns nothing red here — the arrays above simply don't know it exists. That
 * is the exact recurrence vector #2568 was opened to close, so this reads the
 * mock's own source and asserts no `ok: false` literal exists outside the two
 * helpers, following the source-scanning precedent in
 * `no-test-file-imports.test.ts`.
 */
describe("no mock call site answers ok: false outside the refusal helpers (#2568)", () => {
  const mockPath = resolve(fileURLToPath(import.meta.url), "../../__mocks__/brink-web.ts");
  const mockSource = readFileSync(mockPath, "utf8");

  /**
   * Slices out a `private static <name>(...) { ... }` method body by brace
   * matching on indentation rather than regex brace-counting: every line
   * inside a class method body in this file is indented 4+ spaces, so the
   * first `\n  }` (exactly two spaces) after the opening brace is the
   * method's own close, not an inner literal's.
   */
  function extractMethodBody(source: string, name: string): string {
    const marker = `private static ${name}(`;
    const start = source.indexOf(marker);
    expect(start, `could not find ${name}(...) in the mock source`).toBeGreaterThanOrEqual(0);
    const braceOpen = source.indexOf("{", start);
    expect(braceOpen, `could not find ${name}'s opening brace`).toBeGreaterThan(start);
    const end = source.indexOf("\n  }", braceOpen);
    expect(end, `could not find the end of ${name}'s body`).toBeGreaterThan(braceOpen);
    return source.slice(braceOpen, end);
  }

  /** One named helper per Rust refusal struct the mock can answer with. Adding
   *  a fourth helper means adding it here — the arrays above list SITES, this
   *  lists the seams they are required to route through. */
  const REFUSAL_HELPERS = ["structuralRefusal", "autoImportRefusal", "dirMoveRefusal"];

  const helperBodies = REFUSAL_HELPERS.map((name) => ({
    name,
    body: extractMethodBody(mockSource, name),
  }));

  it.each(helperBodies)("$name still emits exactly one ok: false", ({ body }) => {
    // Guards the guard: if a helper refactor stopped emitting `ok: false`,
    // the "nothing outside the helpers" check below would pass vacuously.
    expect((body.match(/ok:\s*false/g) ?? []).length).toBe(1);
  });

  it("no ok: false literal exists in the mock outside those helper bodies", () => {
    // Excise the known-good bodies, then strip comments — the doc block
    // above these helpers quotes `{ ok: false, error }` as prose explaining
    // the history this file guards against, and a naive scan would flag its
    // own explanation as a violation of the invariant it documents.
    const withoutKnownBodies = helperBodies.reduce(
      (source, { body }) => source.replace(body, ""),
      mockSource,
    );
    const withoutComments = withoutKnownBodies
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");

    const strayMatches = withoutComments.match(/ok:\s*false/g) ?? [];
    expect(
      strayMatches,
      `found a raw \`ok: false\` outside ${REFUSAL_HELPERS.join("/")} — ` +
        "route the new site through one of those helpers instead of an inline literal",
    ).toEqual([]);
  });
});
