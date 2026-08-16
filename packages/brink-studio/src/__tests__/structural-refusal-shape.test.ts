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
    expect(Object.keys(fixture.shapes).sort()).toEqual([
      "AutoImportJs",
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
