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
 *
 * ## Shape was checked; vocabulary was not (#2603)
 *
 * The generated shapes carry a placeholder message, so every `error` string
 * below used to be typed by hand — and two of them were typed from the *mock*.
 * Both auto-import doc-handle sites pinned `"unknown handle"` while production
 * (`crates/brink-web/src/editor/refactor.rs`) answers
 * `"unknown document handle"`, so those two cases asserted only that the mock
 * agreed with itself. That is the fourth instance of the class in three waves
 * (#2583's invented serde message, #2599's shadowed stub, #2602's invented
 * `entry file '...' not found`).
 *
 * #2603 closed it for the three document-handle ops: the fixture carries a
 * `messages` map produced by *running* the production ops in
 * `driven_messages()` and reading `error` back out of the payload they answer
 * with — see `productionMessage` below. A site that reads from it cannot
 * drift: changing the Rust wording restales the fixture, and the regenerated
 * fixture fails this file until the mock matches.
 *
 * ## The other ~28 were swept, and three of them were wrong (#2620)
 *
 * #2603 left every other `error:` string here hand-transcribed, and said so.
 * The sweep #2620 asked for found that the transcription had been WRONG at
 * three sites — the mock was answering strings production never emits, and
 * this file was pinning them:
 *
 * | site                            | this file said     | production says               |
 * | ------------------------------- | ------------------ | ----------------------------- |
 * | `rename_file` (missing file)    | `file not loaded`  | `file 'ghost.ink' not found`  |
 * | `delete_symbol` (missing file)  | `file not loaded`  | `source knot not found`       |
 * | `delete_symbol` (missing knot)  | `symbol not found` | `source knot not found`       |
 *
 * `rename_file` has no wasm-level "loaded" guard at all (it delegates to
 * `brink_ide::file_rename`, whose error is `file '{0}' not found`), and
 * `delete_symbol` maps BOTH an unloaded path and a missing KNOT onto the same
 * `MoveError::SourceNotFound` — true only when the knot itself is missing. A
 * missing STITCH inside a knot that DOES exist is a different variant,
 * `MoveError::StitchNotFound` (`stitch '<name>' not found in knot`); that case
 * was undriven at the time of this sweep and is now its own driven site
 * (`delete_symbol (missing stitch in existing knot)`, #2627) rather than
 * folded into this table. A fourth site — `resolve_code_action`'s
 * "no change" case — pinned the right *wording* against an input production
 * ACCEPTS (`FormatKnot` reindents `TWO_KNOTS` and answers `ok: true`), so the
 * parity claim was against a question production answers differently; it now
 * drives `SortKnots` over a single-knot file, which genuinely is a no-op.
 *
 * Rather than re-transcribe, every site below now reads its wording from
 * `driven_messages()`. No `error:` string in the arrays below is typed,
 * except the one mock-only serde abbreviation noted further down (anchored as
 * a genuine prefix of the driven production string, never an invention).
 *
 * ## A refusal that is MISSING is invisible to all of the above (#2641)
 *
 * Everything described so far — shape, then vocabulary, then driving — checks
 * a refusal the mock actually emits. None of it can see an op that does not
 * refuse at all. `delete_symbol` located its target with one `lines.findIndex`
 * over the whole file, so `delete_symbol(p, "two", "b")` (stitch `b` lives
 * under `one`) and `delete_symbol(p, "ghost", "a")` (no such knot) both
 * answered `ok: true` and DELETED, where production answers
 * `MoveError::StitchNotFound` / `MoveError::SourceNotFound`. No amount of
 * driving refusal strings out of production detects that, because there is no
 * mock refusal to compare against — which makes it strictly harder to find
 * than the three wrong strings #2620 caught. The two cases are now driven
 * sites like any other, plus a dedicated behavioural block at the bottom of
 * this file asserting the mock leaves the source alone.
 *
 * ## Two more sites the sweep left behind (#2634, #2635)
 *
 * `rename_symbol` had the same shape of gap in a smaller way: production
 * refuses `symbol not found` when `declaration_offset` resolves nothing, and
 * the mock had no such branch, so renaming a knot that had been edited away
 * succeeded. `resolveCodeActionImpl`'s `file not loaded` was the opposite —
 * correct wording, no driver and no fixture key, i.e. #2621's "gap 2" (a real
 * site invisible to discovery) in its narrowest possible form. Both are driven
 * below. Two of `rename_symbol`'s literals deliberately get NO mock branch;
 * the reasoning is recorded at that call site and pinned by two Rust tests.
 *
 * ⚠ **What this does and does not guarantee.** Driving is still PER-SITE, not
 * automatic: nothing enumerates the (op, refusing-input) pairs, so each entry
 * is a driver someone wrote against an input someone chose, and a refusal site
 * nobody lists is invisible to every guard here (#2621 gap 2 — the #2577 wall,
 * one level up). Two strings are also still hand-written and are marked as
 * such: the mock-only serde abbreviation below (anchored as a *prefix* of the
 * driven production message, so it cannot become a fabrication) and the
 * `structuralRefusal`-family names in the source scan at the bottom of this
 * file, which are TypeScript identifiers rather than refusal vocabulary.
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
  /** Real refusal strings, read out of the production ops (#2603). */
  messages: Record<string, string>;
  /** The exact source text each Rust driver ran against (#2661). */
  sources: Record<string, string>;
  /** Each driven (op, input) pair's own `ok` flag and `error` (#2661). */
  acceptance: Record<string, { ok: boolean; error: string | null }>;
  /** Header lines production's promote/demote rewrites produced (#2661). */
  headers: Record<string, string>;
  /**
   * The symbols `file_symbols` reports for a named source — name/kind, nested
   * (#2662). Pins the header RECOGNIZER itself rather than an op built on it,
   * so a mock that resolves a knot for `delete_symbol` but hides it from the
   * Binder is red here even when every acceptance flag agrees.
   */
  outlines: Record<string, OutlineSymbol[]>;
  /**
   * The `new_source` left behind after a stitch REGION was deleted (#2684).
   * The half neither `acceptance` nor `outlines` can see: `delete_symbol`
   * answers `ok: true` whether or not `opensHeader` picked the right region
   * boundary, so a wrong answer is a successful op with the wrong content.
   */
  regions: Record<string, string>;
  /**
   * Values a FRESH production session is seeded with (#2663). `active_file`
   * is `"main.ink"`; the mock seeded `""`, and `update_source` writes into
   * `files[activePath]`, so the two wrote to different keys.
   */
  defaults: { active_file: string };
  /**
   * The `introduced_diagnostics` CODES a driven (op, input) pair reports
   * (review finding on #2662). `acceptance` only records the `ok`/`error`
   * pair, which is blind to a call that succeeds on both sides regardless of
   * whether a particular diagnostic fired — exactly `rename_symbol`'s
   * function-knot collision check, which answers `ok: true` either way.
   */
  diagnostics: Record<string, string[]>;
}

interface OutlineSymbol {
  name: string;
  kind: string;
  /**
   * Mirrors `DocumentSymbolJs.detail` — `"function"` for a function knot
   * (review finding on #2662). `null` rather than `undefined`: the fixture is
   * parsed JSON, and Rust's `outline_shape()` always emits the key (via
   * `serde_json::Value`'s `Index`, not `skip_serializing_if`), so a
   * non-function knot's `detail` comes through as an explicit `null`.
   */
  detail: string | null;
  children: OutlineSymbol[];
}

const fixture = JSON.parse(readFileSync(FIXTURE_PATH, "utf8")) as RefusalFixture;

/** The generated shape with its placeholder message swapped for a real one. */
function refusalShape(name: string, error: string): Record<string, unknown> {
  const shape = fixture.shapes[name];
  expect(shape, `fixture is missing the ${name} shape — regenerate it`).toBeDefined();
  return { ...shape!, error };
}

/**
 * Every `fixture.messages` key that a case below has actually pulled through
 * `productionMessage`. Tracked by KEY, not by the string value it resolved
 * to — three driven messages currently share the identical value ("unknown
 * document handle"), so a value-keyed set would pass as soon as any one case
 * used it, silently tolerating another case being deleted. See the coverage
 * assertion below, which compares this against `Object.keys(fixture.messages)`.
 */
const consumedMessageKeys = new Set<string>();

/**
 * Production's own wording for a refusal site, read off the generated fixture
 * rather than transcribed (#2603).
 *
 * `driven_messages()` in `crates/brink-web/src/editor_refactor.rs` calls the
 * real op on a real `EditorSession` and stores the `error` it answered with,
 * so this is production's string by construction. Sites that use it are the
 * ones whose vocabulary is machine-checked; the rest are hand-copied, per the
 * warning in this file's header.
 */
function productionMessage(key: string): string {
  const message = fixture.messages[key];
  expect(
    message,
    `fixture has no driven message for "${key}" — add a driver to driven_messages() ` +
      "and regenerate with `BRINK_BLESS_REFUSAL_SHAPES=1 cargo test -p brink-web --lib refusal_shape`",
  ).toBeTypeOf("string");
  consumedMessageKeys.add(key);
  return message!;
}

/**
 * The one site whose mock wording is deliberately NOT production's, recorded
 * so the divergence is measured instead of asserted (#2620).
 *
 * serde_json's error for an unknown internally-tagged variant names every
 * known variant and a source line/column. The mock has no serde, so it emits a
 * short prefix of that. Pinning the abbreviation alone is how #2583's invented
 * serde message survived, so this also consumes the driven key and registers
 * the pair for {@link mockAbbreviations} — a dedicated case asserts the
 * abbreviation is a genuine PREFIX of what production answered, which fails the
 * moment the mock's wording becomes an invention rather than a truncation.
 */
const mockAbbreviations: Array<{ site: string; abbreviation: string; production: string }> = [];

function mockAbbreviationOf(site: string, key: string, abbreviation: string): string {
  mockAbbreviations.push({ site, abbreviation, production: productionMessage(key) });
  return abbreviation;
}

const MAIN = "=== hello ===\nHi.\n-> END\n";

/** Two knots, the first carrying stitches — enough shape for the reorder /
 *  move / promote / demote refusals to reach past their "not found" guards. */
const TWO_KNOTS =
  "=== one ===\nFirst.\n= a\nA.\n= b\nB.\n\n=== two ===\nSecond.\n= a\nOther A.\n";

/**
 * A single `function` knot (`=== function name() ===`, not `=== name ===`).
 * Its header carries a `function` segment between the `===` fence and the
 * declared name that a plain knot's never does — the shape the reviewer
 * flagged as unreached by every fixture above: `MAIN` and `TWO_KNOTS` are
 * both plain knots, so a guard/rewrite that forgot the `function` segment
 * still went green against them.
 */
const FUNCTION_KNOT = '=== function greet() ===\n~ return "hi"\n';

/** A plain knot with one stitch, plus a function knot: TWO top-level knots. */
const KNOT_AND_FUNCTION = '=== one ===\nFirst.\n= a\nA.\n\n=== function greet() ===\n~ return "hi"\n';

/** Two knots, neither carrying a stitch. */
const STITCHLESS_KNOTS = "=== one ===\nFirst.\n\n=== two ===\nSecond.\n";

/** A stitch whose name is already a top-level FUNCTION knot. */
const STITCH_SHADOWS_FUNCTION =
  '=== one ===\nFirst.\n= greet\nG.\n\n=== function greet() ===\n~ return "hi"\n';

/** A `VAR` beside a knot — the `extract_*` var-collision input. */
const VAR_AND_KNOT = "VAR score = 0\n\n=== one ===\nFirst.\nSecond.\n";

/** A knot body with a blank line: non-empty in offsets, empty in content. */
const BLANK_BODY = "=== one ===\nFirst.\n\nLast.\n";

/** A stitch carrying parameters, for the promote rewrite. */
const PARAM_STITCH = "=== one ===\nFirst.\n= deal(n)\nD.\n";

/**
 * A function knot whose declared name is a single letter that also occurs as
 * the FIRST character of the `function` keyword itself (review finding on
 * #2670): `line.indexOf(name)` for `name = "f"` finds the `f` of `function`
 * at offset 4, not the declared name at offset 13. `FUNCTION_KNOT`
 * (`greet`) and `PARAM_STITCH` (`deal`) both happen to use names with no
 * overlapping character in that position, so neither exercised this —
 * `parseOutline`'s nameStart bug was invisible to every existing fixture.
 */
const FUNCTION_KNOT_SHORT_NAME = '=== function f() ===\n~ return "hi"\n';

/** The offset of the real declared name `f` in {@link FUNCTION_KNOT_SHORT_NAME}
 *  — i.e. NOT the `f` inside `function`. */
const FUNCTION_KNOT_SHORT_NAME_OFFSET = FUNCTION_KNOT_SHORT_NAME.indexOf("f(");

/** A source whose very first character is a blank line (review finding on
 *  #2670): `snapToLines`' old `source.lastIndexOf("\n", l - 1) + 1` passes
 *  `l - 1 === -1` to `lastIndexOf`, which JS clamps to `0` — so it finds the
 *  leading `\n` itself and answers `start = 1` instead of `0`. */
const LEADING_BLANK_LINE = "\n=== a ===\nContent.\n";

/**
 * Five knots, none of them fenced `=== name ===` (#2662).
 *
 * Production's `knot_header` rule is
 * `"==" ~ "="* ~ INLINE_WS* ~ … ~ INLINE_WS* ~ ("==" ~ "="*)?`: two or MORE
 * `=`, zero or more spaces, a tolerated leading indent, and an optional
 * trailing fence of any width. So `== one ==`, `===two===`, `==== three
 * ====`, `  ==== four ====` and `=== five` are all ordinary top-level knots,
 * and `one` still owns its stitch `a` — driven, not asserted, in
 * `driven_outlines()`.
 *
 * The mock had two narrower answers and which applied depended on the op:
 * `parseOutline` wanted `^===\s+` (all five invisible), while
 * `delete_symbol`/`rename_symbol` matched `^\s*={2,3}\s*` inline (the first
 * two resolved, the rest did not). One source exercises both halves.
 *
 * `four` (indented) and `five` (no closing fence) are the two widenings
 * `KNOT_HEADER_PREFIX`'s `^\s*` and `KNOT_HEADER_RE`'s trailing
 * `(?:={2,})?` claim and nothing before them drove (review finding on
 * #2662) — every other case here uses a flush-left header with a closing
 * fence.
 */
const ALT_FENCES =
  "== one ==\nFirst.\n= a\nA.\n\n===two===\nSecond.\n\n==== three ====\nThird.\n\n  ==== four ====\nFourth.\n\n=== five\nFifth.\n";

/**
 * Three stitches, none of them `= name`, plus a `=>` line that is NOT a
 * header (#2684) — #2662's split one rung down.
 *
 * `parser/knot.rs` DOCUMENTS `stitch_header` as `"=" ~ !("=" | ">") ~
 * INLINE_WS+ ~ identifier`, but the code is `at_stitch` (`current() == EQ &&
 * nth(1) != EQ && nth(1) != GT`) then `p.skip_ws()`, which matches ZERO or
 * more. Driven in `driven_outlines()` / `driven_stitch_acceptance()`, not
 * read off the grammar: production reports a stitch for `= a`, `  = b`, `=c`,
 * `   =d`, `= e(n)` and `\t= h`, and none for `=> f` or `  => g`.
 *
 * The mock had three answers to "is this a stitch": `parseOutline` wanted
 * `^=\s+` (indent and the tight form both invisible), the `delete_symbol` /
 * `rename_symbol` guards wanted `^\s*=\s+` (indent fine, tight form
 * invisible), and `opensHeader` was a bare `^\s*=` that ended a region for
 * anything starting `=` — `=>` included, which production keeps running
 * through.
 *
 * `a` is the positive control: the one shape every family already resolved.
 * The indented `  = b` is LAST on purpose — production's regions are CST node
 * ranges, so an indented header's whitespace crosses the boundary and only a
 * flush-left boundary is comparable byte-for-byte against a line-based mock.
 */
const ALT_STITCHES =
  "=== one ===\nFirst.\n= a\nA.\n=> x\nStill a.\n=c\nC.\n  = b\nB.\n\n=== two ===\nSecond.\n";

function sessionWith(files: Record<string, string>): EditorSession {
  const s = new EditorSession();
  for (const [path, source] of Object.entries(files)) s.update_file(path, source);
  return s;
}

/**
 * Every call site in the mock that answers with a *refused* structural op.
 *
 * Each `error` is read off the fixture's driven `messages` (#2603/#2620) — the
 * key names the Rust driver in `driven_op_messages()` that produced it, and the
 * `call` below MUST refuse for the same reason with the same inputs, or the
 * fixture pins production's answer to a different question.
 */
const structuralRefusals: Array<{ site: string; error: string; call: () => string }> = [
  {
    site: "rename_file (read-only library)",
    error: productionMessage("rename_file:read-only-mount"),
    call: () => {
      const s = new EditorSession();
      s.__mockMarkReadOnlyForTest("std/lib.ink", MAIN);
      return s.rename_file("std/lib.ink", "mine.ink");
    },
  },
  {
    // #2620: pinned `file not loaded` for two waves. Production has no such
    // guard here — `brink_ide::file_rename` answers `file '{0}' not found`.
    site: "rename_file (file not found)",
    error: productionMessage("rename_file:missing-file"),
    call: () => sessionWith({ "main.ink": MAIN }).rename_file("ghost.ink", "other.ink"),
  },
  {
    site: "rename_file (target exists)",
    error: productionMessage("rename_file:target-exists"),
    call: () =>
      sessionWith({ "main.ink": MAIN, "other.ink": MAIN }).rename_file("main.ink", "other.ink"),
  },
  {
    // #2620: pinned `file not loaded`. Production folds an unloaded path onto
    // `MoveError::SourceNotFound` — the same variant as a missing KNOT below,
    // but NOT the same as a missing STITCH inside a knot that does exist
    // (`delete_symbol (stitch not found, #2627)` further down) — those are a
    // different `MoveError` variant with different wording.
    site: "delete_symbol (file not loaded)",
    error: productionMessage("delete_symbol:missing-file"),
    call: () => sessionWith({ "main.ink": MAIN }).delete_symbol("ghost.ink", "hello", ""),
  },
  {
    // #2620: pinned `symbol not found`, a string this op never emits. This
    // case is a missing KNOT specifically — `MoveError::SourceNotFound`, the
    // same variant as the missing-file case above. A missing STITCH inside an
    // EXISTING knot is a different variant (`StitchNotFound`); see the
    // dedicated case below (#2627) rather than reading this one as covering
    // both.
    site: "delete_symbol (missing knot)",
    error: productionMessage("delete_symbol:missing-symbol"),
    call: () => sessionWith({ "main.ink": MAIN }).delete_symbol("main.ink", "nowhere", ""),
  },
  {
    // #2627 review: the two cases above fold onto `MoveError::SourceNotFound`
    // because the KNOT itself is missing. A missing STITCH inside a knot that
    // DOES exist is `MoveError::StitchNotFound` instead — `stitch '<name>'
    // not found in knot` (structural_move.rs:23) — and was previously
    // undriven, so this sub-case was invisible to the sweep.
    site: "delete_symbol (missing stitch in existing knot)",
    error: productionMessage("delete_symbol:missing-stitch-in-knot"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).delete_symbol("main.ink", "one", "nowhere"),
  },
  {
    // #2641, and the reason this file's whole mechanism needed a case it could
    // not previously express: the mock did not answer the WRONG WORDS here, it
    // answered `ok: true` and DELETED. `b` exists in `TWO_KNOTS` — under knot
    // `one` — so the old whole-file `lines.findIndex` matched it and removed
    // that region, while production's knot-scoped lookup answers
    // `MoveError::StitchNotFound`. Every guard built across
    // #2568/#2577/#2610/#2627 compares refusal wording, so a *missing* refusal
    // is invisible to all of them; this case exists to make it visible.
    site: "delete_symbol (stitch exists, but under another knot)",
    error: productionMessage("delete_symbol:stitch-under-wrong-knot"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).delete_symbol("main.ink", "two", "b"),
  },
  {
    // #2641 case 2. The `knotRe` guard #2627 added only ran on the
    // stitch-not-found branch, so a hit anywhere in the file meant the named
    // knot was never checked at all: `ghost` does not exist, yet the mock
    // deleted `a`. Production refuses at the knot lookup — `SourceNotFound`,
    // before the stitch is considered.
    site: "delete_symbol (named knot does not exist)",
    error: productionMessage("delete_symbol:stitch-under-missing-knot"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).delete_symbol("main.ink", "ghost", "a"),
  },
  {
    site: "extract_to_knot (file not loaded)",
    error: productionMessage("extract_to_knot:missing-file"),
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_knot("ghost.ink", 0, 4, "lifted"),
  },
  {
    site: "extract_to_knot (empty selection)",
    error: productionMessage("extract_to_knot:empty-selection"),
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_knot("main.ink", 4, 4, "lifted"),
  },
  {
    site: "extract_to_function (file not loaded)",
    error: productionMessage("extract_to_function:missing-file"),
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_function("ghost.ink", 0, 4, "lifted"),
  },
  {
    site: "extract_to_function (empty selection)",
    error: productionMessage("extract_to_function:empty-selection"),
    call: () => sessionWith({ "main.ink": MAIN }).extract_to_function("main.ink", 4, 4, "lifted"),
  },
  // The two sites PR #2564 already fixed — kept here so the guard covers every
  // structural refusal the mock can emit, not only the newly swept ones.
  {
    site: "rename_symbol (file not loaded)",
    error: productionMessage("rename_symbol:missing-file"),
    call: () => sessionWith({ "main.ink": MAIN }).rename_symbol("ghost.ink", "hello", "", "hi"),
  },
  {
    // #2634. The guard above is FAITHFUL — `rename_symbol` is the one op of
    // the three #2620 swept that really does emit `file not loaded` at the
    // wasm level (`editor/refactor.rs:478`). What was missing is this one:
    // production also refuses when `declaration_offset` resolves nothing, and
    // the mock proceeded, so renaming a knot that had been edited away
    // answered `ok: true`. `performSymbolRename` names exactly this case
    // ("'symbol not found' after the knot was edited away") as one it notifies
    // the author about, and the branch was unreachable under the mock.
    //
    // The op's other two literals get no mock counterpart, decided per string
    // (#2634's Ask): `no analysis` fires only for a non-source extension, which
    // has no outline and therefore no symbol menu
    // (`rename_symbol_says_no_analysis_only_for_a_non_source_extension`), and
    // `cannot rename this symbol` sits below the guard added here, unreachable
    // once a declaration resolved — its wording is already pinned through the
    // F2 road at `rename_symbol_at (cannot rename this symbol)` below
    // (`rename_symbol_answers_once_a_declaration_resolves`).
    site: "rename_symbol (symbol not found)",
    error: productionMessage("rename_symbol:missing-symbol"),
    call: () => sessionWith({ "main.ink": MAIN }).rename_symbol("main.ink", "nowhere", "", "hi"),
  },
  {
    site: "rename_symbol_at (cannot rename this symbol)",
    error: productionMessage("rename_symbol_at:unrenameable"),
    call: () => sessionWith({ "main.ink": MAIN }).rename_symbol_at("main.ink", 0, "hi"),
  },
  // The seven `dispatchSymbolAction` ops + the code-action resolver, added to
  // the mock by #2577. These had NO mock method at all before it, so the
  // studio suite could not reach them either way; they route through
  // `structuralRefusal` from their first line rather than growing an inline
  // literal to be swept later.
  {
    site: "reorder_stitch (file not loaded)",
    error: productionMessage("reorder_stitch:missing-file"),
    call: () => sessionWith({ "main.ink": MAIN }).reorder_stitch("ghost.ink", "hello", "a", 1),
  },
  {
    site: "reorder_stitch (stitch not found)",
    error: productionMessage("reorder_stitch:missing-stitch"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_stitch("main.ink", "one", "nowhere", 1),
  },
  {
    site: "reorder_knot (source knot not found)",
    error: productionMessage("reorder_knot:missing-knot"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_knot("main.ink", "nowhere", 1),
  },
  {
    site: "reorder_stitches (invalid reorder)",
    error: productionMessage("reorder_stitches:invalid-order"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_stitches("main.ink", "one", ["a"]),
  },
  {
    site: "reorder_knots (invalid reorder)",
    error: productionMessage("reorder_knots:invalid-order"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_knots("main.ink", ["one", "one"]),
  },
  {
    site: "move_stitch (destination knot not found)",
    error: productionMessage("move_stitch:missing-dest-knot"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).move_stitch("main.ink", "one", "a", "nope"),
  },
  {
    site: "move_stitch (name collision)",
    error: productionMessage("move_stitch:name-collision"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).move_stitch("main.ink", "one", "a", "two"),
  },
  {
    site: "promote_stitch (name collision with a top-level knot)",
    error: productionMessage("promote_stitch:name-collision"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).promote_stitch("main.ink", "one", "two"),
  },
  {
    site: "promote_stitch (stitch not found)",
    error: productionMessage("promote_stitch:missing-stitch"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).promote_stitch("main.ink", "one", "nowhere"),
  },
  {
    site: "demote_knot (illegal nesting)",
    error: productionMessage("demote_knot:illegal-nesting"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).demote_knot("main.ink", "one", "two"),
  },
  {
    site: "demote_knot (destination knot not found)",
    error: productionMessage("demote_knot:missing-dest-knot"),
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).demote_knot("main.ink", "two", "nope"),
  },
  {
    // The one deliberately-divergent wording left in this file. Production's
    // serde_json error names every known variant plus a line/column; the mock
    // has no serde and emits a truncation of it. `mockAbbreviationOf` still
    // consumes the driven key and registers the pair, so a dedicated case can
    // assert the truncation really is a prefix of production's answer rather
    // than an invention (the #2583 failure mode).
    site: "resolve_code_action (unknown variant, mock-only abbreviation)",
    error: mockAbbreviationOf(
      "resolve_code_action (unknown variant)",
      "resolve_code_action:unknown-variant",
      "invalid code-action data: unknown variant `Nonsense`",
    ),
    call: () => {
      const s = sessionWith({ "main.ink": TWO_KNOTS });
      s.set_active_file("main.ink");
      return s.resolve_code_action(JSON.stringify({ action: "Nonsense" }), 0);
    },
  },
  {
    // #2620: this used to drive `FormatKnot` over `TWO_KNOTS`, which production
    // ACCEPTS (it reindents and answers `ok: true`) — the wording was right but
    // the question was not one production refuses. `SortKnots` over a
    // single-knot file is a genuine no-op on both sides.
    site: "resolve_code_action (no change)",
    error: productionMessage("resolve_code_action:no-change"),
    call: () => {
      const s = sessionWith({ "main.ink": MAIN });
      s.set_active_file("main.ink");
      return s.resolve_code_action(JSON.stringify({ action: "SortKnots" }), 0);
    },
  },
  {
    // #2635 — #2621's "gap 2" made concrete. `resolveCodeActionImpl` refuses
    // `file not loaded` and production says the same thing
    // (`editor/code_actions.rs:102`), so unlike the three strings #2620 caught
    // the wording was never wrong. The site was simply UNDRIVEN: no fixture
    // key, no call site, invisible to every guard in this file.
    //
    // Both sides reach it the same way — the active path is not a loaded file.
    // Production's `active_path` starts as `main.ink` and the mock's as `""`,
    // and `set_active_file` refuses an unknown path on both, so a session that
    // loaded only `other.ink` is left pointing at nothing either way.
    site: "resolve_code_action (active path not loaded)",
    error: productionMessage("resolve_code_action:missing-file"),
    call: () =>
      sessionWith({ "other.ink": MAIN }).resolve_code_action(
        JSON.stringify({ action: "SortKnots" }),
        0,
      ),
  },
  {
    site: "resolve_code_action_doc (unknown document handle)",
    error: productionMessage("resolve_code_action_doc:unknown-handle"),
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
    error: productionMessage("rename_dir:missing-dir"),
    call: () => sessionWith({ "main.ink": MAIN }).rename_dir("ghost", "other"),
  },
  {
    site: "rename_dir (destination occupied)",
    error: productionMessage("rename_dir:destination-occupied"),
    call: () =>
      sessionWith({ "src/a.ink": MAIN, "dst/a.ink": MAIN }).rename_dir("src", "dst"),
  },
];

/** The doc-handle refusals, which answer with the `AutoImportJs` shape instead
 *  — a different Rust struct, so a different generated shape.
 *
 *  Their wording is read off the fixture's driven `messages` (#2603): these two
 *  are exactly the sites that drifted, pinning the mock's `"unknown handle"`
 *  against production's `"unknown document handle"`. */
const autoImportRefusals: Array<{ site: string; error: string; call: () => string }> = [
  {
    site: "auto_import_include_doc (unknown document handle)",
    error: productionMessage("auto_import_include_doc:unknown-handle"),
    call: () => sessionWith({ "main.ink": MAIN }).auto_import_include_doc(999, "other.ink"),
  },
  {
    site: "auto_import_apply_include_doc (unknown document handle)",
    error: productionMessage("auto_import_apply_include_doc:unknown-handle"),
    call: () => sessionWith({ "main.ink": MAIN }).auto_import_apply_include_doc(999, "other.ink"),
  },
  {
    // #2621: production fences a handle on a mounted stdlib file before it
    // attempts the include, and the mock modelled no fence at all — so the
    // ~1174-test studio suite could not reach this branch either way.
    //
    // Its sibling literal at the same op, `current file source unavailable`,
    // gets NO mock counterpart: it is a defensive `let ... else` that sits
    // AFTER `ensure_include` has already resolved the same source, so no input
    // reaches it (proved by
    // `removing_a_file_under_an_open_handle_refuses_before_the_source_guard`
    // in `crates/brink-web/src/editor_refactor.rs`). Mirroring an unreachable
    // branch would model a production answer nothing can produce — #2577's
    // lesson that a mock method nothing calls closes nothing.
    site: "auto_import_apply_include_doc (read-only mounted stdlib)",
    error: productionMessage("auto_import_apply_include_doc:read-only-mount"),
    call: () => {
      const s = new EditorSession();
      s.__mockMarkReadOnlyForTest("std/lib.ink", MAIN);
      const doc = s.open_document("std/lib.ink");
      return s.auto_import_apply_include_doc(doc, "other.ink");
    },
  },
];

/** The compile-channel refusal, which answers `CompileResult` — a fourth Rust
 *  struct again (no `safe`/`cross_file_edits`, no `path`; `warnings` always
 *  ships). Its shape was generated into the fixture by #2577 with no mock
 *  call site to check it against; `compile_project` (#2589) is that
 *  counterpart — the studio's real channel is `EditorSession::compile_project`
 *  (`crates/brink-web/src/editor/mod.rs`) -> `IdeSession::compile` ->
 *  `CompileEntryError::EntryNotFound`, and the mock's only reproducible
 *  failure mode mirrors that: `entry` not resolving to a loaded file. */
const compileRefusals: Array<{ site: string; error: string; call: () => string }> = [
  {
    site: "compile_project (entry file not found)",
    error: productionMessage("compile_project:missing-entry"),
    call: () => sessionWith({ "main.ink": MAIN }).compile_project("ghost.ink"),
  },
];

/**
 * The ACCEPTANCE half (#2661) — the one every guard above is blind to.
 *
 * Everything before this compares the WORDING of a refusal the mock emits. An
 * op that answers `ok: true` where production refuses has no wording to
 * compare, so it is invisible: `delete_symbol` (#2641) and `rename_symbol`
 * (#2634) were both found by reading the two implementations side by side, and
 * the audit that produced this table found seven more.
 *
 * `fixture.acceptance` is each (op, input) pair's own `ok` flag plus the
 * `error` beside it, harvested by CALLING the op on a real `EditorSession`
 * (`driven_outline_acceptance` / `driven_extract_acceptance` in
 * `crates/brink-web/src/editor_refactor.rs`). Each case below asks the mock the
 * same question and compares BOTH — so a mock that succeeds where production
 * refuses, refuses where production succeeds, or refuses for a different
 * reason, is red here.
 *
 * The inputs are the same question by construction: the offsets are derived
 * from the source text (`indexOf`, exactly as the Rust `at()` helper does), and
 * the source constants are asserted byte-identical to `fixture.sources` below.
 */
const acceptanceCases: Array<{ key: string; call: () => string }> = [
  // ── The outline ops. Most of these turn on one thing: production
  // resolves knots through `tree.knots()`, which yields a `function` knot
  // like any other, while the mock resolved them with a header regex that
  // could not see past the `function` segment.
  {
    key: "reorder_knot:function-knot",
    call: () => sessionWith({ "main.ink": FUNCTION_KNOT }).reorder_knot("main.ink", "greet", 1),
  },
  {
    key: "reorder_knots:function-knot-counted",
    call: () => sessionWith({ "main.ink": KNOT_AND_FUNCTION }).reorder_knots("main.ink", ["one"]),
  },
  {
    key: "reorder_knots:function-knot-permuted",
    call: () =>
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).reorder_knots("main.ink", ["greet", "one"]),
  },
  {
    // Production short-circuits to `Ok(source)` when the knot carries no
    // stitches, BEFORE the permutation is resolved — so even a nonsense
    // order is accepted rather than refused.
    key: "reorder_stitches:stitchless-knot",
    call: () =>
      sessionWith({ "main.ink": STITCHLESS_KNOTS }).reorder_stitches("main.ink", "one", ["nope"]),
  },
  {
    key: "reorder_stitch:function-knot",
    call: () =>
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).reorder_stitch("main.ink", "greet", "a", 1),
  },
  {
    key: "move_stitch:into-function-knot",
    call: () =>
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).move_stitch("main.ink", "one", "a", "greet"),
  },
  {
    key: "promote_stitch:collides-with-function-knot",
    call: () =>
      sessionWith({ "main.ink": STITCH_SHADOWS_FUNCTION }).promote_stitch(
        "main.ink",
        "one",
        "greet",
      ),
  },
  {
    key: "demote_knot:function-knot-source",
    call: () =>
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).demote_knot("main.ink", "greet", "one"),
  },
  {
    key: "demote_knot:function-knot-dest",
    call: () =>
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).demote_knot("main.ink", "one", "greet"),
  },
  {
    key: "reorder_stitch:accepted",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).reorder_stitch("main.ink", "one", "a", 1),
  },
  {
    key: "move_stitch:accepted",
    call: () => sessionWith({ "main.ink": TWO_KNOTS }).move_stitch("main.ink", "one", "b", "two"),
  },
  // ── The extract ops. `ExtractError` has EIGHT variants; the mock
  // modelled three, so five production refusals answered `ok: true`.
  {
    key: "extract_to_knot:crosses-header",
    call: () =>
      sessionWith({ "main.ink": TWO_KNOTS }).extract_to_knot(
        "main.ink",
        TWO_KNOTS.indexOf("B."),
        TWO_KNOTS.indexOf("Second."),
        "lifted",
      ),
  },
  {
    key: "extract_to_knot:knot-name-collision",
    call: () =>
      sessionWith({ "main.ink": MAIN }).extract_to_knot(
        "main.ink",
        MAIN.indexOf("Hi."),
        MAIN.indexOf("Hi.") + 3,
        "hello",
      ),
  },
  {
    key: "extract_to_knot:var-collision",
    call: () =>
      sessionWith({ "main.ink": VAR_AND_KNOT }).extract_to_knot(
        "main.ink",
        VAR_AND_KNOT.indexOf("First."),
        VAR_AND_KNOT.indexOf("First.") + 6,
        "score",
      ),
  },
  {
    key: "extract_to_knot:invalid-name",
    call: () =>
      sessionWith({ "main.ink": MAIN }).extract_to_knot(
        "main.ink",
        MAIN.indexOf("Hi."),
        MAIN.indexOf("Hi.") + 3,
        "1bad",
      ),
  },
  {
    key: "extract_to_knot:blank-selection",
    call: () =>
      sessionWith({ "main.ink": BLANK_BODY }).extract_to_knot(
        "main.ink",
        BLANK_BODY.indexOf("\n\n") + 1,
        BLANK_BODY.indexOf("\n\n") + 2,
        "lifted",
      ),
  },
  {
    key: "extract_to_knot:accepted",
    call: () =>
      sessionWith({ "main.ink": MAIN }).extract_to_knot(
        "main.ink",
        MAIN.indexOf("Hi."),
        MAIN.indexOf("Hi.") + 3,
        "lifted",
      ),
  },
  {
    // Review finding on #2670: a source whose first character is a blank
    // line, selected at [0, 1) — see LEADING_BLANK_LINE above.
    key: "extract_to_knot:leading-blank-line",
    call: () => sessionWith({ "main.ink": LEADING_BLANK_LINE }).extract_to_knot("main.ink", 0, 1, "lifted"),
  },
  {
    key: "extract_to_function:flow-control",
    call: () =>
      sessionWith({ "main.ink": MAIN }).extract_to_function(
        "main.ink",
        MAIN.indexOf("-> END"),
        MAIN.indexOf("-> END") + 6,
        "lifted",
      ),
  },
  {
    key: "extract_to_function:invalid-name",
    call: () =>
      sessionWith({ "main.ink": MAIN }).extract_to_function(
        "main.ink",
        MAIN.indexOf("Hi."),
        MAIN.indexOf("Hi.") + 3,
        "1bad",
      ),
  },
  {
    key: "extract_to_function:var-collision",
    call: () =>
      sessionWith({ "main.ink": VAR_AND_KNOT }).extract_to_function(
        "main.ink",
        VAR_AND_KNOT.indexOf("First."),
        VAR_AND_KNOT.indexOf("First.") + 6,
        "score",
      ),
  },
  {
    key: "extract_to_function:accepted",
    call: () =>
      sessionWith({ "main.ink": MAIN }).extract_to_function(
        "main.ink",
        MAIN.indexOf("Hi."),
        MAIN.indexOf("Hi.") + 3,
        "lifted",
      ),
  },
  // ── One knot-header vocabulary (#2662). Both mock families are driven
  // against ALT_FENCES on purpose: the four `parseOutline` ops saw NONE of
  // its five knots, and the two inline ops saw the first two but not the
  // rest. A case list covering only one family would read as though the
  // split had a single victim.
  {
    key: "reorder_knots:alt-fences",
    call: () =>
      sessionWith({ "main.ink": ALT_FENCES }).reorder_knots("main.ink", [
        "five",
        "four",
        "three",
        "two",
        "one",
      ]),
  },
  {
    key: "promote_stitch:alt-fence-terse",
    call: () => sessionWith({ "main.ink": ALT_FENCES }).promote_stitch("main.ink", "one", "a"),
  },
  {
    key: "delete_symbol:alt-fence-wide",
    call: () => sessionWith({ "main.ink": ALT_FENCES }).delete_symbol("main.ink", "three", ""),
  },
  {
    key: "rename_symbol:alt-fence-wide",
    call: () =>
      sessionWith({ "main.ink": ALT_FENCES }).rename_symbol("main.ink", "three", "", "renamed"),
  },
  {
    // Positive control: the `==` fence the inline family already resolved.
    key: "delete_symbol:alt-fence-terse",
    call: () => sessionWith({ "main.ink": ALT_FENCES }).delete_symbol("main.ink", "one", ""),
  },
  // ── One stitch-header vocabulary (#2684). Same both-families discipline
  // as the knot block above: `=c` was invisible to BOTH families, `  = b`
  // only to the outline family, and `= a` to neither.
  {
    key: "reorder_stitches:alt-stitches",
    call: () =>
      sessionWith({ "main.ink": ALT_STITCHES }).reorder_stitches("main.ink", "one", [
        "b",
        "c",
        "a",
      ]),
  },
  {
    key: "delete_symbol:alt-stitch-tight",
    call: () => sessionWith({ "main.ink": ALT_STITCHES }).delete_symbol("main.ink", "one", "c"),
  },
  {
    key: "rename_symbol:alt-stitch-tight",
    call: () =>
      sessionWith({ "main.ink": ALT_STITCHES }).rename_symbol("main.ink", "one", "c", "renamed"),
  },
  {
    key: "delete_symbol:alt-stitch-indented",
    call: () => sessionWith({ "main.ink": ALT_STITCHES }).delete_symbol("main.ink", "one", "b"),
  },
  {
    // Positive control: the flush-left `= a` shape every family resolved.
    key: "delete_symbol:alt-stitch-plain",
    call: () => sessionWith({ "main.ink": ALT_STITCHES }).delete_symbol("main.ink", "one", "a"),
  },
];

describe("the mock accepts exactly what production accepts (#2661)", () => {
  it("this file's source constants are byte-identical to the Rust drivers' (#2661)", () => {
    // Every driven answer in the fixture is evidence about the mock ONLY if
    // the mock was asked the same question. Before #2661 that identity was a
    // comment on both sides and nothing checked it.
    expect({
      ALT_FENCES,
      ALT_STITCHES,
      BLANK_BODY,
      FUNCTION_KNOT,
      KNOT_AND_FUNCTION,
      LEADING_BLANK_LINE,
      MAIN,
      PARAM_STITCH,
      STITCHLESS_KNOTS,
      STITCH_SHADOWS_FUNCTION,
      TWO_KNOTS,
      VAR_AND_KNOT,
    }).toEqual(fixture.sources);
  });

  it("every driven acceptance case has a call site here", () => {
    // Same omission guard the messages half carries: a driven (op, input) pair
    // nobody calls is a row parked in a fixture, not a guard.
    expect(acceptanceCases.map((c) => c.key).sort()).toEqual(Object.keys(fixture.acceptance).sort());
  });

  for (const { key, call } of acceptanceCases) {
    it(`${key}: the mock answers production's ok flag`, () => {
      const parsed = JSON.parse(call()) as { ok: boolean; error?: string };
      expect({ ok: parsed.ok, error: parsed.error ?? null }).toEqual(fixture.acceptance[key]);
    });
  }
});

/**
 * The recognizer itself, not an op built on it (#2662).
 *
 * Acceptance pins whether an op RUNS. It cannot see the studio's other
 * outline consumers — the Binder, the symbol menu and the story graph all
 * read `file_symbols`/`project_outline`, which is where a knot the ops
 * happily resolve can still be invisible. That was the shape of Gap A: the
 * mock had two answers to "is this a knot", and `== two ==` was a knot to
 * `delete_symbol` and absent from the outline.
 *
 * `fixture.outlines` is production's own `file_symbols` output, harvested by
 * `driven_outlines()` in `crates/brink-web/src/editor_refactor.rs`. Names and
 * kinds only: ranges are pinned by the #2670 offset guards below.
 */
describe("the outline reports exactly the symbols production reports (#2662)", () => {
  const outlineSources: Record<string, string> = {
    ALT_FENCES,
    ALT_STITCHES,
    KNOT_AND_FUNCTION,
    TWO_KNOTS,
  };

  it("every driven outline has a source here", () => {
    expect(Object.keys(outlineSources).sort()).toEqual(Object.keys(fixture.outlines).sort());
  });

  /**
   * Strip everything but the recognizer's answer: which symbols, nested, and
   * whether each is a function knot (`detail`) — the field `KNOT_AND_FUNCTION`
   * exists to control (review finding on #2662). Normalized to `null` rather
   * than left `undefined`: the fixture is JSON, where an absent Rust `Option`
   * serializes as an explicit `detail: null` (via `serde_json::Value`'s
   * `Index`, not `skip_serializing_if`, since `outline_shape` on the Rust
   * side always emits the key), and `toEqual` treats `undefined` and `null`
   * as different values.
   */
  function shape(symbols: OutlineSymbol[]): OutlineSymbol[] {
    return symbols.map((s) => ({
      name: s.name,
      kind: s.kind,
      detail: s.detail ?? null,
      children: shape(s.children),
    }));
  }

  for (const [name, source] of Object.entries(outlineSources)) {
    it(`${name}: file_symbols agrees with production`, () => {
      const symbols = JSON.parse(
        sessionWith({ "main.ink": source }).file_symbols("main.ink"),
      ) as OutlineSymbol[];
      expect(shape(symbols)).toEqual(fixture.outlines[name]);
    });
  }
});

/**
 * The stitch REGION, the half neither acceptance nor the outline can see
 * (#2684).
 *
 * `delete_symbol` answers `ok: true` whichever line `opensHeader` decides the
 * region ends at, so a wrong boundary is a *successful* op with the wrong
 * content — invisible to `acceptance`, and invisible to `outlines` because
 * the recognizer can be right while the region scan is not.
 *
 * The one pinned case carries both directions. Stitch `a`'s body holds a
 * `=> x` line, which production does NOT treat as a header (`at_stitch`
 * excludes a following `>`), and its region ends at the tight `=c`, which
 * production DOES. An `opensHeader` that says `true` too often stops early
 * and orphans `=> x\nStill a.`; one that says `false` too often runs past
 * `=c` and swallows the next stitch. Only the right vocabulary reproduces
 * production's string, so the widening cannot be bought either way.
 */
describe("a deleted stitch takes exactly production's region with it (#2684)", () => {
  const regionCalls: Record<string, () => string> = {
    "delete_symbol:alt-stitch-plain": () =>
      sessionWith({ "main.ink": ALT_STITCHES }).delete_symbol("main.ink", "one", "a"),
  };

  it("every driven region has a call site here", () => {
    expect(Object.keys(regionCalls).sort()).toEqual(Object.keys(fixture.regions).sort());
  });

  for (const [key, call] of Object.entries(regionCalls)) {
    it(`${key}: new_source agrees with production`, () => {
      const parsed = JSON.parse(call()) as { ok: boolean; new_source?: string };
      expect(parsed.ok).toBe(true);
      expect(parsed.new_source).toBe(fixture.regions[key]);
    });
  }
});

/**
 * Session-seed parity (#2663).
 *
 * Production's `EditorSession::new` seeds `active_path` with `"main.ink"`;
 * the mock seeded `""`. Both answer `file not loaded` for a session that has
 * loaded nothing, which is why #2635's driven `resolve_code_action` site
 * stayed green over the divergence — but `update_source` writes into
 * `files[activePath]`, so a mock session that never calls `set_active_file`
 * wrote to key `""` where production writes to `"main.ink"`.
 */
describe("a fresh mock session is seeded the way production is (#2663)", () => {
  it("active_file() starts at production's default", () => {
    expect(new EditorSession().active_file()).toBe(fixture.defaults.active_file);
  });

  it("update_source on an untouched session writes at that key", () => {
    // The consequence the default actually has. Before #2663 this landed at
    // `""`, so a later `set_active_file("main.ink")` refused (the file was
    // never loaded under that name) where production's would succeed.
    const s = new EditorSession();
    s.update_source(MAIN);
    const paths = (JSON.parse(s.list_files()) as { path: string }[]).map((f) => f.path);
    expect(paths).toEqual([fixture.defaults.active_file]);
    expect(s.set_active_file(fixture.defaults.active_file)).toBe(true);
  });
});

/**
 * The half acceptance cannot see: an op that ACCEPTS on both sides but
 * rewrites the header differently (#2661).
 *
 * Production's promote/demote header rewrites are name-agnostic — they strip
 * the `=` fences and keep whatever is between them — so a function knot keeps
 * its `function` segment and a parameterised stitch keeps its `(n)` inside the
 * new fences. The mock interpolated the declared name into a regex instead,
 * which is exactly the trap PR #2658's own fix fell into: `={2,3}\s*<name>`
 * does not match `=== function greet() ===`.
 *
 * The expected strings are read off `fixture.headers`, which is production's
 * own `new_source` (`driven_header_rewrites` in
 * `crates/brink-web/src/editor_refactor.rs`) — not typed here.
 */
describe("promote/demote rewrite the header the way production does (#2661)", () => {
  it("demoting a function knot keeps its `function` segment", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).demote_knot("main.ink", "greet", "one"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(true);
    expect(parsed.new_source).toContain(fixture.headers["demote_knot:function-knot"]);
    // And the knot fence is gone — a rewrite that matched nothing would leave
    // `=== function greet() ===` sitting inside another knot's body.
    expect(parsed.new_source).not.toContain("=== function greet() ===");
  });

  it("promoting a parameterised stitch keeps the params inside the fences", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": PARAM_STITCH }).promote_stitch("main.ink", "one", "deal"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(true);
    expect(parsed.new_source).toContain(fixture.headers["promote_stitch:parameterised"]);
  });

  it("every driven header rewrite has a call site here", () => {
    expect(Object.keys(fixture.headers).sort()).toEqual([
      "demote_knot:function-knot",
      "promote_stitch:parameterised",
    ]);
  });
});

/**
 * The half `acceptance` cannot see, and the reason the fixture carries a
 * `diagnostics` map at all (review finding on #2662).
 *
 * `rename_symbol`'s collision check (`knotHeaderFor(newName)`) was widened to
 * carry the `function` segment, on the claim that a function knot already
 * holding the new name is a duplicate knot definition in production too — but
 * the rename itself answers `ok: true` on both sides regardless of whether
 * that collision fires, so no `acceptance` case could tell a fired E022 from
 * a silently skipped check. This drives the same call the mock makes and
 * compares the diagnostic CODES against `fixture.diagnostics`, which is
 * production's own answer (`driven_diagnostics()` in
 * `crates/brink-web/src/editor_refactor.rs`), not typed here.
 */
describe("rename_symbol's collision check counts a function knot (#2662 review)", () => {
  it("every driven diagnostics case has a call site here", () => {
    expect(Object.keys(fixture.diagnostics).sort()).toEqual([
      "rename_symbol:collides-with-function-knot",
    ]);
  });

  it("renaming a knot onto an existing function knot's name introduces the diagnostic production does", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": KNOT_AND_FUNCTION }).rename_symbol("main.ink", "one", "", "greet"),
    ) as { ok: boolean; introduced_diagnostics: Array<{ code: string }> };
    expect(parsed.ok).toBe(true);
    expect(parsed.introduced_diagnostics.map((d) => d.code)).toEqual(
      fixture.diagnostics["rename_symbol:collides-with-function-knot"],
    );
  });
});

describe("mock refusal payloads match the Rust structs (#2568)", () => {
  it("the generated fixture is present and carries the shapes this file reads", () => {
    // Cheap canary: a fixture regenerated after a Rust rename would drop a key
    // here rather than making every case below fail with a confusing diff.
    expect(Object.keys(fixture.shapes).sort()).toEqual([
      "AutoImportJs",
      "CompileResult",
      "DirMoveResultJs",
      "StructuralResultJs",
    ]);
  });

  it("the driven refusal messages are present and every one has a call site here (#2603/#2620)", () => {
    // Same canary for the vocabulary half: a renamed key would otherwise make
    // `productionMessage` fail once per site with no hint of the cause. This
    // list is the ONLY hand-typed thing left about the messages — it names
    // Rust driver KEYS, never refusal wording.
    expect(Object.keys(fixture.messages).sort()).toEqual([
      "auto_import_apply_include_doc:read-only-mount",
      "auto_import_apply_include_doc:unknown-handle",
      "auto_import_include_doc:unknown-handle",
      "compile_project:missing-entry",
      "delete_symbol:missing-file",
      "delete_symbol:missing-stitch-in-knot",
      "delete_symbol:missing-symbol",
      "delete_symbol:stitch-under-missing-knot",
      "delete_symbol:stitch-under-wrong-knot",
      "demote_knot:illegal-nesting",
      "demote_knot:missing-dest-knot",
      "extract_to_function:empty-selection",
      "extract_to_function:missing-file",
      "extract_to_knot:empty-selection",
      "extract_to_knot:missing-file",
      "move_stitch:missing-dest-knot",
      "move_stitch:name-collision",
      "promote_stitch:missing-stitch",
      "promote_stitch:name-collision",
      "rename_dir:destination-occupied",
      "rename_dir:missing-dir",
      "rename_file:missing-file",
      "rename_file:read-only-mount",
      "rename_file:target-exists",
      "rename_symbol:missing-file",
      "rename_symbol:missing-symbol",
      "rename_symbol_at:unrenameable",
      "reorder_knot:missing-knot",
      "reorder_knots:invalid-order",
      "reorder_stitch:missing-file",
      "reorder_stitch:missing-stitch",
      "reorder_stitches:invalid-order",
      "resolve_code_action:missing-file",
      "resolve_code_action:no-change",
      "resolve_code_action:unknown-variant",
      "resolve_code_action_doc:unknown-handle",
    ]);

    // A driven message with no site exercising it is a string parked in a
    // fixture, not a guard: it would keep passing while the mock said anything
    // it liked. Tracked by KEY (`consumedMessageKeys`, populated by
    // `productionMessage` as the arrays above are built), not by the string
    // VALUE it resolved to — several driven messages share an identical value
    // ("unknown document handle", "source knot not found"), so comparing values
    // would pass as soon as any one case used it, even with the others deleted.
    expect([...consumedMessageKeys].sort()).toEqual(Object.keys(fixture.messages).sort());
  });

  it("the one mock-only wording is a truncation of production's, not an invention (#2620)", () => {
    // #2583 shipped a serde message that was simply made up. An abbreviation
    // that is a genuine prefix of the driven production string cannot be: the
    // moment the mock's wording stops being a truncation — because someone
    // reworded it, or because serde's own text changed — this goes red.
    expect(mockAbbreviations.length).toBeGreaterThan(0);
    for (const { site, abbreviation, production } of mockAbbreviations) {
      expect(
        production.startsWith(abbreviation),
        `${site}: the mock says ${JSON.stringify(abbreviation)}, which is not a prefix of ` +
          `production's driven ${JSON.stringify(production)} — either fix the mock or, if the ` +
          "divergence is now more than a truncation, stop calling it an abbreviation",
      ).toBe(true);
      // And a truncation, not an equality dressed up as one — if they match
      // exactly the site should read from `productionMessage` like every other.
      expect(
        production.length,
        `${site}: the mock's wording now equals production's — drop the abbreviation ` +
          "and read the driven message directly",
      ).toBeGreaterThan(abbreviation.length);
    }
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

  for (const { site, error, call } of compileRefusals) {
    it(`${site} answers a full CompileResult`, () => {
      expect(JSON.parse(call()) as unknown).toEqual(refusalShape("CompileResult", error));
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
 * The behavioural half of #2641, stated in the direction that matters.
 *
 * The two driven cases above already fail against the pre-#2641 mock — a
 * successful `{ ok: true, new_source, ... }` is not the refusal shape. But
 * shape-equality states the failure as "wrong keys", when the defect is
 * "content was DELETED that production would have left alone". These assert
 * that directly, so the regression they guard reads as itself: a mock that
 * reintroduces the whole-file `findIndex` deletes `= b`/`= a` here and goes
 * red on the `toContain`, not on a key diff.
 *
 * Both calls name a stitch that exists SOMEWHERE in the file, which is the
 * whole point — a scan not scoped to the named knot finds it.
 */
describe("delete_symbol does not delete across a knot boundary (#2641)", () => {
  it("refuses, and removes nothing, when the stitch lives under a different knot", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": TWO_KNOTS }).delete_symbol("main.ink", "two", "b"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(false);
    // Nothing was rewritten: a refusal carries no `new_source` at all. The
    // op is pure on both sides, so this — not the session's contents — is
    // where the deletion would have shown up.
    expect(parsed.new_source).toBeUndefined();
  });

  it("refuses, and removes nothing, when the named knot does not exist", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": TWO_KNOTS }).delete_symbol("main.ink", "ghost", "a"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(false);
    expect(parsed.new_source).toBeUndefined();
  });

  it("still deletes a stitch that really is under the named knot", () => {
    // The guard above must not have been bought by refusing everything: the
    // ordinary case still computes a deletion.
    const parsed = JSON.parse(
      sessionWith({ "main.ink": TWO_KNOTS }).delete_symbol("main.ink", "one", "b"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(true);
    expect(parsed.new_source).not.toContain("= b");
    // The identically-named stitch under `two` survives, and so does `two`.
    expect(parsed.new_source).toContain("=== two ===");
  });
});

/**
 * The behavioural half of #2634: the mock must not report a rename of a
 * symbol that is not there as having happened.
 */
describe("rename_symbol refuses a symbol the file does not declare (#2634)", () => {
  it("refuses a knot that is not declared", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": MAIN }).rename_symbol("main.ink", "nowhere", "", "hi"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(false);
    expect(parsed.new_source).toBeUndefined();
  });

  it("refuses a stitch that exists only under a different knot", () => {
    // `declaration_offset` looks the stitch up inside the named knot only, so
    // `b` (under `one`) is not found when `two` is named — the same scoping
    // #2641 fixed in `delete_symbol`.
    const parsed = JSON.parse(
      sessionWith({ "main.ink": TWO_KNOTS }).rename_symbol("main.ink", "two", "b", "hi"),
    ) as { ok: boolean };
    expect(parsed.ok).toBe(false);
  });

  it("still renames a knot and a stitch that really are declared", () => {
    const knot = JSON.parse(
      sessionWith({ "main.ink": TWO_KNOTS }).rename_symbol("main.ink", "one", "", "uno"),
    ) as { ok: boolean; new_source?: string };
    expect(knot.ok).toBe(true);
    expect(knot.new_source).toContain("=== uno ===");

    const stitch = JSON.parse(
      sessionWith({ "main.ink": TWO_KNOTS }).rename_symbol("main.ink", "one", "b", "bee"),
    ) as { ok: boolean; new_source?: string };
    expect(stitch.ok).toBe(true);
    expect(stitch.new_source).toContain("= bee");
  });

  it("does not refuse a function knot — the guard must see past the `function` segment", () => {
    // `KnotHeader::name()` returns the bare name for a function header too
    // (pinned production-side by `ast/tests/decl.rs::function_knot_header`),
    // so `brink_ide::rename::declaration_offset` resolves `greet` here just
    // like it would a plain knot. A guard regex that only matches
    // `={2,3}\s*<name>` — skipping the `function` keyword in between — would
    // find no declaration and wrongly answer `symbol not found`.
    const parsed = JSON.parse(
      sessionWith({ "main.ink": FUNCTION_KNOT }).rename_symbol("main.ink", "greet", "", "hail"),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(true);
    expect(parsed.new_source).toContain("=== function hail() ===");
  });
});

/**
 * `parseOutline`'s name-offset regression (review finding on #2670).
 *
 * The widened `KNOT_HEADER_RE` (`(?:function\s+)?`) fixed WHICH knots
 * `parseOutline` sees, but `nameStart = offset + line.indexOf(name)` was
 * still wrong for any of them: `line.indexOf(name)` finds the FIRST
 * occurrence of the name's characters anywhere in the line, not the
 * matched declaration. For `=== function f() ===` that is the `f` inside
 * `function` itself (offset 4), not the declared name (offset 13). Every
 * existing fixture (`FUNCTION_KNOT` -> `greet`, `PARAM_STITCH` -> `deal`)
 * happened to use a name that shares no such prefix with `function`, so
 * none of them could catch it — this fixture's whole reason to exist is
 * that its name starts with the same letter `function` does.
 *
 * This feeds `file_symbols`, `project_outline`, `story_graph` node spans,
 * and `rename_symbol_at` (the F2 road) — a caret placed on the real name
 * would answer `cannot rename this symbol` under the old offset.
 */
describe("parseOutline reports the real name offset, not the first character match (#2670 review)", () => {
  it("file_symbols reports the knot's name span at the declared name", () => {
    const symbols = JSON.parse(
      sessionWith({ "main.ink": FUNCTION_KNOT_SHORT_NAME }).file_symbols("main.ink"),
    ) as Array<{ name: string; start: number; end: number }>;
    expect(symbols).toHaveLength(1);
    expect(symbols[0]!.name).toBe("f");
    expect(symbols[0]!.start).toBe(FUNCTION_KNOT_SHORT_NAME_OFFSET);
    expect(symbols[0]!.end).toBe(FUNCTION_KNOT_SHORT_NAME_OFFSET + 1);
  });

  it("rename_symbol_at resolves a caret on the real name", () => {
    const parsed = JSON.parse(
      sessionWith({ "main.ink": FUNCTION_KNOT_SHORT_NAME }).rename_symbol_at(
        "main.ink",
        FUNCTION_KNOT_SHORT_NAME_OFFSET,
        "hail",
      ),
    ) as { ok: boolean; new_source?: string };
    expect(parsed.ok).toBe(true);
    expect(parsed.new_source).toContain("=== function hail() ===");
  });

  it("rename_symbol_at does NOT resolve a caret sitting on the `function` keyword", () => {
    // The old, wrong offset (4) lived inside the `function` keyword, so a
    // caret there used to resolve as if it were on the declared name. It
    // is not the declaration and must refuse like any other non-symbol
    // offset.
    const keywordOffset = FUNCTION_KNOT_SHORT_NAME.indexOf("function");
    const parsed = JSON.parse(
      sessionWith({ "main.ink": FUNCTION_KNOT_SHORT_NAME }).rename_symbol_at(
        "main.ink",
        keywordOffset,
        "hail",
      ),
    ) as { ok: boolean };
    expect(parsed.ok).toBe(false);
  });
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
   *  a fifth helper means adding it here — the arrays above list SITES, this
   *  lists the seams they are required to route through. */
  const REFUSAL_HELPERS = [
    "structuralRefusal",
    "autoImportRefusal",
    "dirMoveRefusal",
    "compileRefusal",
  ];

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
