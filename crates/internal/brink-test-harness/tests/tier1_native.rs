//! Tier-1 **native** golden corpus (issue #1529).
//!
//! ⚠ **These goldens are self-referential, NOT oracle-derived.** Native
//! (`.brink`) source has no C# ink counterpart — inklecate never parsed
//! this syntax, so there is nothing for it to generate golden episodes
//! from. Each case under `tests/tier1-native/<name>/` is `story.brink`
//! (the native surface grammar) plus a hand-authored `expected.txt`
//! derived from tracing the case's own semantics against the shipped
//! native pipeline (parser → `hir::lower_native` → analyzer → LIR →
//! codegen → `brink_runtime::Story`), not against the C# runtime. A
//! passing case here proves "the compiler still produces what we
//! previously decided it should produce for this native construct," never
//! "this matches ink's reference behavior" — keep that distinction in any
//! report or doc that cites this corpus. The oracle ratchet
//! (`RATCHET_EPISODE_COUNT` in `oracle_snapshots.rs`) is a completely
//! separate number and must never be conflated with pass/fail counts here.
//!
//! Before this corpus, every native e2e proof lived only as ad-hoc
//! `#[test]` assertions in `crates/brink-compiler/tests/driver.rs` (the
//! `native_or_coalescing_*`/`native_as_binding_*` families) and a handful
//! of sibling integration-test files — real coverage, but invisible to
//! `corpus_report` (`cargo test -p brink-test-harness --test corpus_report
//! -- --nocapture`), which only walks `tests/tier{1,2,3}/` and
//! `tests_github/`. This corpus is `corpus_report`'s native counterpart,
//! reported in its own clearly-labeled section (see that test) so a
//! native regression surfaces where everyone already looks, without
//! polluting the oracle's CASES/EPISODES totals.
//!
//! Seeded (2026-07-26) with the native features that shipped this same
//! week: or-coalescing (issue #1460, short-circuit ruling #1471),
//! the `as` binding (issue #1475), UFCS calls — free-fn/auto-ref/prelude
//! desugar (issue #1482), `TypeName { … }` construction literals
//! (issue #1464), two-binding `for k, v in` map iteration (issue #1461),
//! the `@[was(…)]` module-rename annotation (issue #1286/#1355), and the
//! per-declaration `@[effects(…)]` annotation channel (issue #1563).
//! Complements, never replaces, `driver.rs`'s fine-grained unit-level
//! assertions on the same features.
//!
//! Mirrors `tier1_brink.rs`'s shape (that file's own module doc explains
//! why a hand-curated golden corpus is the right tool when there is no
//! oracle to diff against). Every case is a straight-line, choice-free
//! program — `compile_path` auto-detects the `.brink` extension and
//! routes through the native pipeline with default `AnalysisOptions`
//! (no `Dialect`/`TypePolicy` knob native cares about).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native")
}

fn assert_case(name: &str) {
    let dir = corpus_dir().join(name);
    let expected =
        brink_test_harness::corpus::load_golden_transcript(&dir.join("expected.txt"), name)
            .expect("golden transcript must be present and non-vacuous");
    let actual = brink_test_harness::corpus::run_native_transcript(&dir.join("story.brink"))
        .unwrap_or_else(|e| panic!("case {name}: {e}"));
    assert_eq!(
        actual, expected,
        "case {name}: output mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

/// B1 or-coalescing (`docs/stdlib-spec.md` §1.6a, issue #1460): the
/// collapse form (`some(v) or fallback` / `none or fallback`), a chained
/// `none or none or 7`, and the short-circuit ruling (#1471) — `bump()`
/// only runs, and `counter` only advances, when the left-hand side is
/// `none`.
#[test]
fn or_coalescing() {
    assert_case("or-coalescing");
}

/// B1b the `as` binding (issue #1475): `if EXPR as NAME { … }` binds the
/// *unwrapped* payload inside the success arm and falls through to the
/// story's own fallback when the condition is `none`.
#[test]
fn as_binding() {
    assert_case("as-binding");
}

/// B3a UFCS resolution (issue #1482): one story exercising all three
/// desugar verdicts — `hp.bump(amount)` with `bump`'s `ref` first
/// parameter (`FreeFnAutoRef`, mutates the global through the call),
/// `n.double()` against a plain by-value free function
/// (`FreeFnDesugar`), and `m.len()` against the T1b/NS stdlib prelude
/// (`PreludeDesugar`).
#[test]
fn ufcs_desugar_forms() {
    assert_case("ufcs");
}

/// B5 construction literals (issue #1464): `TypeName { field: value, … }`
/// against an unregistered type name falls through to the declared-struct
/// reading (`Expr::StructLiteral`) — construct, read, and use a field
/// value end to end. Also covers the registered-`flags` variant
/// (`Flags { calm }`), so this corpus case exercises a real registry
/// construction target, not just the unregistered fall-through (`Map { … }`
/// gets only incidental coverage elsewhere, via `for-k-v`/`ufcs`).
#[test]
fn construction_literal() {
    assert_case("construction-literal");
}

/// B2 two-binding `for k, v in m` (issue #1461): desugars to a container
/// snapshot plus a `LogicWhile` binding `k`/`v` each iteration. Uses a
/// non-alphabetical insertion order (`z`, `a`, `m`) and accumulates both
/// `k` (into a string) and `v` (summed) so the golden pins the key
/// binding and iteration order, not just that the loop runs to
/// completion — a case that only summed `v` would still pass with a
/// garbage `k` binding.
#[test]
fn for_k_v() {
    assert_case("for-k-v");
}

/// The `@[was("old::path")]` file-level module-rename annotation
/// (issue #1286/#1355) parses and lowers cleanly on a real compile. The
/// annotation's full DefinitionId-continuity-across-a-rename semantics
/// are a project/db-layer concern covered by `brink-ir`/`brink-db`'s own
/// tests, not by this single-compile transcript corpus — this case's
/// signal is narrower: the annotation must never regress a clean compile.
#[test]
fn annotations_was() {
    assert_case("annotations-was");
}

/// The `@[allow(Exxx, …)]` source-level suppression annotation (issue
/// #1161) at all three attachment points a native file offers: a top-level
/// `var`, a top-level `flow` (alongside an `@[effects(…)]` on the same
/// declaration), and a nested `flow` (the `Stitch` level).
///
/// The signal here is deliberately narrow — **compile-and-run**, exactly
/// like `annotations_was` above. Every one of these lines was a hard `E111`
/// ("unknown annotation name") compile failure before this landed, so a
/// running transcript is a real regression guard; but a transcript can
/// never show a *suppressed* diagnostic, and this corpus is choice-free by
/// construction (`run_native_transcript` rejects a story that presents
/// choices), which rules out driving the native `E151` dead-end lint from
/// here. The suppression semantics themselves — including the
/// source-`allow`-beats-project-`deny` ruling and the `E153`/`E154`/`E155`
/// rejections — are pinned end-to-end in `brink-compiler`'s
/// `tests/e0xx_diagnostics.rs`.
#[test]
fn annotations_allow() {
    assert_case("annotations-allow");
}

/// Per-declaration `@[effects(…)]` annotations (issue #1563) on a top-level
/// `fn`, a top-level `flow`, and a nested `flow` (the `Stitch` level). Until
/// this landed, every one of these lines hard-failed the compile with
/// `E129`, so the case's primary signal is exactly that: an annotated
/// `.brink` story compiles and runs at all. The assertions themselves are
/// exceedance-only and satisfied here, so a clean transcript also pins that
/// a *correct* assertion stays silent — `brink-db`'s
/// `t2_2_effects_assertions.rs` owns the exceeding-assertion direction.
#[test]
fn annotations_effects() {
    assert_case("annotations-effects");
}

/// The `@[element(…)]` surface, in both its spellings.
///
/// The `args = "…"` declaration half (issue #1719) sits at the same three
/// attachment points `annotations_effects` covers: a top-level `fn`
/// (captures binding both its params), a top-level `flow` (a capture-free
/// pattern), and a nested `flow` (the `Stitch` level, with the optional
/// `name = "…"` alias clause). Until that landed, every one of these lines
/// hard-failed the compile with `E129`, so its signal is narrow: an
/// annotated `.brink` story compiles and runs. The `!name` sigil dispatch
/// rewrite `args` exists to eventually drive is still not implemented —
/// see `docs/prose-dialect-spec.md` §3.5b's Deferred list.
///
/// The `claims = "…"` half (issue #1838) is the part with a *behavioral*
/// signal, and it is this corpus's proof that natural-notation dispatch
/// actually reaches a reader: `interior` claims the `INT. MARKET SQUARE`
/// line, binds `place` to `MARKET SQUARE`, and the line lowers to one call
/// whose value is the transcript's second line. Before #1838 that same
/// line was a scene heading with no HIR lowering at all (`E129`, a failed
/// compile) — so `expected.txt`'s `-- inside MARKET SQUARE --` cannot be
/// produced by any other path, and the line beneath it pins that the
/// heading claimed only its own line, never the header-scoped run below.
///
/// `radio` and `interior` both now carry a `content`-typed parameter
/// (issue #1846, `docs/prose-dialect-spec.md` §3.5b's capture contract) —
/// `radio(chan: string, text: content)` is the exact signature #1719 ruled
/// and #1846 unblocked (before this landed, `content` tripped `E061` like
/// any unrecognized name, so this fixture could not compile at all).
/// `interior` gained a second claimed heading (`INT. OLD MILL`) so this
/// fixture proves a `content`-typed param survives *two* distinct claim
/// dispatches with different captured text, not just one. `content`'s own
/// binding to a genuine captured `FragmentRef` (rather than today's plain
/// string argument) is issue #1839's dispatch-mechanism scope, not
/// delivered here — see that issue and the compile-level sibling test
/// immediately below for what this slice does and does not prove.
#[test]
fn annotations_element() {
    assert_case("annotations-element");
}

/// Compile-level sibling to `annotations_element` (issue #1846), mirroring
/// `inline_tag_embedded_brace_reaches_story_data`'s pattern: `assert_case`
/// only proves the *transcript* matches, which can't distinguish "this text
/// reached `StoryData`'s line table" from "this text was computed some
/// other way at runtime". This compiles+links the same fixture directly and
/// inspects `line_tables`, pinning two things a `content`-typed param must
/// not regress:
///
/// - ordinary, unclaimed content lines that sit alongside the two
///   `content`-typed handlers (`"The stalls are shuttered."`) still reach
///   their own translatable `LineEntry` with a real `SourceLocation` —
///   `content` typing a claim handler's param must not disturb sibling
///   lines' normal line-table residency;
/// - the claim-dispatched call embedding a `content`-typed handler's
///   return value (`radio(...)`, composed via `Feed: {radio(...)}.`) still
///   reaches its own `Template` line entry with its `SlotInfo` intact —
///   the existing display-position fragment-composition machinery
///   (`brink-codegen-inkb::content::emit_slot_expr`) is untouched by
///   `content` becoming a resolvable param type.
///
/// What this does **not** prove: that the captured span itself (`"MARKET
/// SQUARE"`, `"OLD MILL"`) becomes its own line-table entry — today
/// `hir::lower_native::element::try_claim` binds every capture as a plain
/// `Expr::String` literal regardless of the receiving param's declared
/// type, so a `content`-typed capture is not yet translation-resident the
/// way the capture contract ultimately requires. Closing that gap needs a
/// captured-run-to-`FragmentRef` binding this issue explicitly leaves to
/// #1839 ("the dispatch mechanism") — flagged on that issue's thread
/// rather than built here.
#[test]
fn annotations_element_content_param_reaches_story_data() {
    let path = corpus_dir().join("annotations-element").join("story.brink");
    let output = brink_compiler::compile_path(&path)
        .unwrap_or_else(|e| panic!("compile annotations-element: {e:?}"));
    let (_program, line_tables) =
        brink_runtime::link(&output.data).expect("link annotations-element");
    let all_lines = format!("{line_tables:?}");
    assert!(
        all_lines.contains("The stalls are shuttered."),
        "an ordinary content line beside the content-typed handlers must \
         still reach its own line-table entry: {all_lines}"
    );
    assert!(
        all_lines.contains(r#"name: "radio(...)""#),
        "the display-position call composing a content-typed handler's \
         return value must still carry its slot info: {all_lines}"
    );
}

/// NG-D array/sequence literals (issue #1490, RULED 2026-07-27:
/// `[1, 2, 3]`). Three functions: one builds a three-element array bound to
/// a `let` and sums it by iterating with `for` (proving `Expr::ArrayLiteral`
/// reaches real element values, not just an empty shape), one builds `[]`
/// and iterates zero times (proving the empty form compiles and runs, not
/// just parses), and one writes the array literal directly in the `for …
/// in` head (`for x in [4, 5, 6] { … }`, the most idiomatic spelling of
/// this feature) so that position is covered end-to-end too, not just via a
/// path binding. Neither reads the ink-json oracle — there is nothing there
/// to compare against, since `[…]` has no ink counterpart; this corpus is
/// the only end-to-end proof this construct actually plays.
#[test]
fn array_literal() {
    assert_case("array-literal");
}

/// Lambda **lifting** (issue #1709) meeting the fn-value verb layer's pure
/// trio (`map`/`filter`/`fold`, issue #1679). Issue #1685 landed lambdas as
/// far as HIR; LIR lowering then raised an `E052` codegen fence, so a
/// writer could not actually hand a lambda literal to the trio at runtime —
/// `#fn(named_function)` was the only fn-value spelling that reached those
/// ops. This case is the end-to-end proof that gap is closed: a
/// zero-capture lambda into each of `map`/`filter`/`fold`, a **capturing**
/// lambda (`|x| x * factor`, by-value per the 2026-07-19 ruling), the three
/// verbs composed, and a braced-body lambda with its own `let` and trailing
/// value expression. It also carries the three capture shapes the free-name
/// walk used to drop silently (issue #1709 review): a captured local read
/// as a call's callee, as a UFCS method-call receiver, and through a bare
/// dotted field access.
#[test]
fn lambda_verbs() {
    assert_case("lambda-verbs");
}

/// Reviewer finding on #1732 (issue #1716): a line whose *entire* content is
/// a childless, point-marker inline-markup span (§8b.11's `<pause/>` shape)
/// on its own, with no surrounding text, used to be declined by
/// `try_recognize_template` (it required ≥1 non-whitespace `Text` part) and
/// fall to `EmitContent`'s flattening, which for a childless span appends
/// nothing at all — the line vanished with no diagnostic, no wire
/// `LinePart::Span`, and (as this fixture proves end-to-end) no visible
/// line in the transcript either: `story.brink`'s middle `<pause/>` line
/// used to leave `"Bell tolls.\nDoor slams."`, collapsing two paragraphs
/// into one. Now it's admitted to `Template` recognition and produces its
/// own (visibly empty) line, matching `expected.txt`'s blank middle line —
/// `crates/internal/brink-ir/tests/markup_wire_recognition.rs`'s
/// `a_lone_point_marker_span_is_admitted_to_template_recognition` pins the
/// same fix at the wire-structure layer (the `LinePart::Span` itself, which
/// this transcript-level fixture can't distinguish from any other way of
/// producing an empty line).
#[test]
fn inline_markup_point_marker() {
    assert_case("inline-markup-point-marker");
}

/// A `#`-tag whose own raw text embeds a balanced `{…}` brace pair,
/// end-to-end (issue #1787 — filed from review of #1728/PR #1777, whose fix
/// to `content::tag()`'s brace-depth counter had only parser-unit coverage,
/// `crates/internal/brink-syntax-native/src/parser/tests/content.rs`, never
/// a fixture pinning it at the level a writer actually experiences).
/// `#sound {clang} in the tower.` is entirely the tag's own raw text (a
/// content line's tags are always trailing, so nothing on that source line
/// re-enters real prose grammar after the `#`) — before #1777, `tag()`
/// stopped unconditionally at the *first* `}`, leaving `in the tower.` and
/// the rest of the source unconsumed at the point the flow body expected
/// its own closer, which hard-fails compilation (reproduced locally against
/// this exact fixture: 4 diagnostics, "prevented compilation"). With the
/// fix, the balanced brace doesn't end the tag early, the line finishes
/// normally, and the following `Door creaks.` content line — proof the rest
/// of the flow's body was never swallowed — plays untouched. This is a
/// choice-free straight-line story, so the tag's own text isn't observable
/// in `run_native_transcript`'s output (`Line::Text`'s `tags` field is
/// dropped by that helper — only the printed prose is), but a story that
/// merely *compiles* clean here already isn't proof enough by itself
/// (`assert_case` alone is only half the guard); the discriminating signal
/// is the *second* line's text surviving intact, which is exactly what the
/// pre-fix unconditional-first-`}` stop would have destroyed.
#[test]
fn inline_tag_embedded_brace() {
    assert_case("inline-tag-embedded-brace");
}

/// Compile-level sibling to `inline_tag_embedded_brace` (review of #1787):
/// `assert_case` runs through `run_native_transcript`, whose `Line::Text {
/// text, .. }` arm discards the `tags` field entirely — so that golden
/// fixture only pins that the *following* content line survives, never that
/// the tag's own text (braces included) actually reached `StoryData`. This
/// asserts that directly, compiling the exact same fixture through
/// `brink_compiler::compile_path` and linking it, then checking the tag's
/// full raw text for the whole string, unbroken by the embedded brace.
/// Precedent: `crates/brink-compiler/tests/driver.rs`'s
/// `local_directive_reaches_story_data`, which pins a different tag's text
/// the same way (`format!("{:?}", story.line_tables)`, `.contains(..)`).
#[test]
fn inline_tag_embedded_brace_reaches_story_data() {
    let path = corpus_dir()
        .join("inline-tag-embedded-brace")
        .join("story.brink");
    let output = brink_compiler::compile_path(&path)
        .unwrap_or_else(|e| panic!("compile inline-tag-embedded-brace: {e:?}"));
    let (_program, line_tables) =
        brink_runtime::link(&output.data).expect("link inline-tag-embedded-brace");
    let all_lines = format!("{line_tables:?}");
    assert!(
        all_lines.contains("sound {clang} in the tower."),
        "tag's own raw text, embedded brace included, must reach StoryData's \
         line tables intact: {all_lines}"
    );
}

/// Every `tests/tier1-native/` case directory is exercised by a `#[test]`
/// above — a directory with no matching test would silently never run.
#[test]
fn every_case_directory_has_a_test() {
    let known = [
        "or-coalescing",
        "as-binding",
        "ufcs",
        "construction-literal",
        "for-k-v",
        "annotations-was",
        "annotations-effects",
        "annotations-allow",
        "annotations-element",
        "array-literal",
        "lambda-verbs",
        "inline-markup-point-marker",
        "inline-tag-embedded-brace",
    ];
    let mut found: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-native")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected: Vec<String> = known.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "tests/tier1-native/ directories and this test's `known` list have drifted"
    );
}
