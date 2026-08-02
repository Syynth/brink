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
//! routes through the native pipeline with default `AnalysisOptions`.
//!
//! **`TypePolicy` is not neutral here** (issue #1882): the default options
//! above resolve to `TypePolicy::Gradual`, under which
//! `brink_analyzer::strict_diagnostics` returns nothing — so none of the
//! TM-3 strict pass runs against these fixtures, even though a real
//! `.brink` project with `dialect = "brink"` compiles under `Strict`. That
//! is deliberate for *this* file (a typing question must never fail a
//! transcript golden), and `tier1_native_strict.rs` is the sibling that
//! sweeps the same corpus under `types = strict` against a classified
//! baseline of findings. Adding a case here automatically adds it there.

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
/// hard-failed the compile with `E129`, so its signal is narrow for the
/// `flow`-attached declarations: an annotated `.brink` story compiles and
/// runs (dispatching to a `flow` target isn't implemented — see
/// `docs/prose-dialect-spec.md` §3.5b's Deferred list). The **`fn`**-
/// attached declaration now has a real behavioral signal too: the fixture's
/// `!radio TAC-2: All units report in.` line is the `!name` sigil dispatch
/// rewrite itself (issue #2004) — it dispatches by name to `radio`, binds
/// `chan`/`text` from the remainder, and lowers to one call whose return
/// value (`text`, unmodified) is `expected.txt`'s second line. Before #2004
/// that same line was ordinary, un-diagnosed prose (`!radio` parsed as
/// plain `TEXT`, per rule 20a's "establish the starting point empirically"
/// — reserved-and-ignored, not reserved-and-diagnosed); it could not have
/// reached this transcript line any other way, since `radio` is never
/// otherwise called with this exact captured text.
///
/// The `claims = "…"` half (issue #1838) is the part with a *behavioral*
/// signal, and it is this corpus's proof that natural-notation dispatch
/// actually reaches a reader: `interior` claims the `INT. MARKET SQUARE`
/// line, binds `place` to `MARKET SQUARE`, and the line lowers to one call
/// whose value is `expected.txt`'s third line. Before #1838 that same
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

/// Compile-level sibling to `annotations_element`, mirroring
/// `inline_tag_embedded_brace_reaches_story_data`'s pattern: `assert_case`
/// only proves the *transcript* matches, which can't distinguish "this text
/// reached `StoryData`'s line table" from "this text was computed some
/// other way at runtime". This compiles+links the same fixture directly and
/// inspects `line_tables` to pin that reachability directly, independent of
/// transcript equality.
///
/// Note this is **not** a regression test for `content`-typed params
/// specifically: `hir::lower_native::element::try_claim` binds every
/// capture as a plain `Expr::String` literal regardless of the receiving
/// param's declared type (issue #1846 only made `content` a resolvable
/// `Ty`, it did not change how captures are bound), so both assertions
/// below hold identically whether `radio`/`interior`'s params are typed
/// `content` or left untyped. What this pins is StoryData/line-table
/// reachability for a claim-dispatched fixture in general:
///
/// - an ordinary, unclaimed content line that sits alongside the claimed
///   headings (`"The stalls are shuttered."`) still reaches its own
///   translatable `LineEntry` with a real `SourceLocation`;
/// - the claim-dispatched call embedding a handler's return value
///   (`radio(...)`, composed via `Feed: {radio(...)}.`) still reaches its
///   own `Template` line entry with its `SlotInfo` intact — the existing
///   display-position fragment-composition machinery
///   (`brink-codegen-inkb::content::emit_slot_expr`) is untouched.
///
/// What this does **not** prove: that the captured span itself (`"MARKET
/// SQUARE"`, `"OLD MILL"`) becomes its own line-table entry — today
/// `try_claim`'s plain-`Expr::String` binding means a `content`-typed
/// capture is not yet translation-resident the way the capture contract
/// ultimately requires. Closing that gap needs a captured-run-to-
/// `FragmentRef` binding, issue #1838's dispatch-mechanism scope, not
/// delivered here.
#[test]
fn annotations_element_reaches_story_data() {
    let path = corpus_dir().join("annotations-element").join("story.brink");
    let output = brink_compiler::compile_path(&path)
        .unwrap_or_else(|e| panic!("compile annotations-element: {e:?}"));
    let (_program, line_tables) =
        brink_runtime::link(&output.data).expect("link annotations-element");
    let all_lines = format!("{line_tables:?}");
    assert!(
        all_lines.contains("The stalls are shuttered."),
        "an ordinary content line beside the claimed headings must \
         still reach its own line-table entry: {all_lines}"
    );
    assert!(
        all_lines.contains(r#"name: "radio(...)""#),
        "the display-position call composing a claim handler's return \
         value must still carry its slot info: {all_lines}"
    );
}

/// Value-carrying `return <expr>` at prose-body position (issue #1973):
/// `fn double(x) >{ return x * 2 }` overrides the `fn`'s default
/// code-ground body to prose-ground, and `return x * 2` is the
/// content-ground `return_stmt` grammar this issue's fix taught to parse a
/// trailing value expression instead of leaving it as dangling,
/// unreachable content (the old shape raised `E033`). `flow main()` calls
/// `double(21)` from display position (`Doubled: {double(21)}`) so the
/// returned value must actually reach the transcript, not just compile —
/// before the fix, this exact source failed to compile at all (`return x *
/// 2` left `* 2` as unparsed dangling content past the bare `return`).
#[test]
fn prose_return_value() {
    assert_case("prose-return-value");
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

/// Native bare-name fn values (RULED 2026-08-01, issue #1862): a
/// statically-named function in expression position **is** a fn value on
/// the `.brink` surface, with no sigil (`map(items, double)`), while a call
/// keeps its parentheses (`double(4)`).
///
/// Before this landed the bare name lowered to the knot's **visit count**,
/// so every reference below compiled clean and reached the runtime as an
/// `int` — the case therefore pins a silent mis-compile, not just a missing
/// feature. Covers the reference reaching `map`/`filter`/`fold`, crossing a
/// call boundary into a plain parameter and being invoked there, held in a
/// `let` and called through it, losing to a same-named local (shadowing),
/// and mixing with a lambda literal in one expression.
#[test]
fn fn_value_bare_name() {
    assert_case("fn-value-bare-name");
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

/// Issue #1996 (`docs/prose-dialect-spec.md` §4.1, RULED 2026-08-01): a
/// hyphenated tag name (`<fade-in>`) end to end — parses, lowers, and runs
/// through the full native pipeline with the tag stripped and its content
/// surviving, exactly like any other span (`inline_markup_point_marker`'s
/// unhyphenated sibling). `in` is a reserved keyword (`KW_IN`) elsewhere in
/// native code, so this also proves the hyphen-continuation leniency
/// (`markup::is_name_segment`) doesn't just satisfy the parser's own unit
/// tests but reaches a real compiled-and-run story.
#[test]
fn inline_markup_hyphenated_name() {
    assert_case("inline-markup-hyphenated-name");
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

/// Issue #1903: end-to-end cover for a native file whose `root_content` is
/// the synthesized entry divert — it compiles and runs through the native
/// pipeline with #1903's `root_content` walk active.
///
/// ⚠ **This case does NOT exercise #1903's type checking, and cannot.**
/// `hir::lower_native::entry_root_content` makes a `.brink` file's
/// `root_content` either empty or a *single synthesized `Divert`* to
/// `main` — never user statements — so native root content holds nothing
/// type-bearing for the strict walk to check. #1903's fix only does real
/// work on the **ink** frontend, whose `root_content` is populated from the
/// file's literal top-level weave.
///
/// The discriminating tests (one that fails with the fix reverted, plus one
/// pinning the native asymmetry above) are
/// `brink_analyzer::strict::tests::ink_root_content_declared_temp_init_is_checked`
/// and `::native_root_content_holds_no_type_bearing_statements`.
#[test]
fn root_content_typed_strict() {
    assert_case("root-content-typed-strict");
}

/// Directive-shaped `#@…` tags (issue #1835): a `.brink` file with a
/// `#@was("old_name")` trailing tag (a name with a native `@[was(…)]`
/// annotation counterpart) and a `#@private` trailing tag (a name with no
/// native equivalent) compiles and runs clean end to end — `E172` is
/// `Warning`-severity and non-blocking (`DiagnosticCode::severity`), so
/// neither line's compile fails and both still print as ordinary content.
/// The diagnostic itself isn't observable through this transcript
/// (`run_native_transcript`'s `Line::Text` arm drops the `tags` field
/// entirely, the same reason `inline_tag_embedded_brace`'s module doc
/// gives) — `directive_like_tag_warns_e172` immediately below is the
/// compile-level sibling that inspects `CompileOutput::warnings` directly.
#[test]
fn directive_like_tag() {
    assert_case("directive-like-tag");
}

/// Compile-level sibling to `directive_like_tag`: inspects
/// `CompileOutput::warnings` directly to pin that both the recognized
/// (`was`) and unrecognized (`private`) `#@…` names each raise their own
/// `E172`, with the message naming the right guidance for each — the
/// native `@[was(…)]` annotation equivalent for the first, and an explicit
/// "no native equivalent" for the second.
#[test]
fn directive_like_tag_warns_e172() {
    let path = corpus_dir().join("directive-like-tag").join("story.brink");
    let output = brink_compiler::compile_path(&path)
        .unwrap_or_else(|e| panic!("compile directive-like-tag: {e:?}"));
    let e172s: Vec<_> = output
        .warnings
        .iter()
        .filter(|w| w.code == brink_ir::DiagnosticCode::E172)
        .collect();
    assert_eq!(
        e172s.len(),
        2,
        "expected exactly two E172 warnings: {:?}",
        output.warnings
    );
    assert!(
        e172s.iter().any(|w| w.message.contains("@[was(")),
        "expected the `was` tag's message to name the native `@[was(…)]` equivalent: {e172s:?}"
    );
    assert!(
        e172s
            .iter()
            .any(|w| w.message.contains("no `private` equivalent")),
        "expected the `private` tag's message to say it has no native equivalent: {e172s:?}"
    );
}

/// Review of #1953 ("DOCUMENTED CONTRACT WITH NO TEST"): `@[allow(E172)]`
/// is the stated justification for `E172`'s `Warning` severity in
/// `DiagnosticCode::E172`'s doc comment, in `severity()`'s comment, in the
/// issue's changeset, and in `docs/diagnostics/E172.md`'s Fix section, but
/// nothing exercised it — suppression is applied downstream of
/// `hir::lower_native` (`brink_ir::suppressions::apply_suppressions` via
/// `brink-db::partition_diagnostics` merging `HirFile::allow_scopes`),
/// invisible to the `lower_native` unit tests, which only see diagnostics
/// as raised, pre-suppression. Writes a scratch `.brink` file (not the
/// on-disk `directive-like-tag` fixture, which has no `@[allow(…)]`) and
/// compiles it through `compile_path`, the same entry point
/// `directive_like_tag_warns_e172` above uses, asserting an
/// `@[allow(E172)]` above the flow removes both tags' warnings from
/// `CompileOutput::warnings`.
#[test]
fn directive_like_tag_allow_e172_suppresses_the_warning() {
    // Native `.brink` discovery reads straight off disk
    // (`brink_driver::Driver::discover_native` → `RealFs`), bypassing any
    // in-memory `read_file` callback entirely (see
    // `crates/brink-compiler/tests/e0xx_diagnostics.rs`'s `compile_native`
    // doc comment for the same constraint) — so this needs a real scratch
    // file on disk rather than `brink_compiler::compile`'s virtual-source
    // entry point.
    let src = "\
@[allow(E172)]
flow main() {
  Hi there. #@was(\"old_name\")
  Bye. #@private
  -> END
}
";
    let dir = std::env::temp_dir().join(format!(
        "brink-tier1-native-e172-allow-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    std::fs::write(dir.join("main.brink"), src).expect("write scratch story.brink");
    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();
    let output = result.unwrap_or_else(|e| panic!("compile directive-like-tag-allow: {e:?}"));
    let e172s: Vec<_> = output
        .warnings
        .iter()
        .filter(|w| w.code == brink_ir::DiagnosticCode::E172)
        .collect();
    assert!(
        e172s.is_empty(),
        "expected @[allow(E172)] above the flow to suppress both tags' warnings: {e172s:?}"
    );
}

/// Issue #1991: `~ stmt` — the ruled content-ground line escape into code
/// (charter §8.2, RULED 2026-07-23 "Native interleaving & body-dialect
/// spelling": ink's logic line, kept). Before this landed, `~ n = 5` at
/// prose-body position compiled clean (zero diagnostics) and printed the
/// literal text `~ n = 5` to the player while silently never performing
/// the assignment — this fixture's `expected.txt` ("Value is 9.") is only
/// reachable if all three logic lines (a plain assignment, a compound
/// `+=` assignment, and a bare function call for its side effect) actually
/// ran: `n` starts at `0`, `~ n = 5` sets it to `5`, `~ n += 3` to `8`,
/// then `~ bump()` (a code-ground `fn` whose own body does `n += 1;`) to
/// `9`. With the parser/lowering fix reverted, this case's transcript
/// reads `~ n = 5\n~ n += 3\n~ bump()\nValue is 0.` instead — a different
/// mismatch for every one of the three lines, not just a missing feature.
///
/// Extended by issue #1972 with a fourth logic-line shape the same
/// `TILDE` dispatch didn't originally cover: `~ let m = n + 1` (a
/// content-ground temp declaration). "Temp is 10." is only reachable if
/// `m` is actually bound to `n + 1` (`n` is `9` at that point) and
/// interpolated — with the #1972 grammar/lowering reverted, this fixture
/// does not silently swallow the statement: `logic_line`'s dispatch falls
/// through to `expr_stmt_line`, which routes `let` into `expr::expression`
/// (`crates/internal/brink-syntax-native/src/parser/expr.rs`'s atom
/// fallback); `KW_LET` is not an expression starter there, so it raises
/// `"expected an expression, found KW_LET"`, plus one `error_recover` per
/// leftover token on the line (issue #1991's recovery loop — it does not
/// let the tokens through as prose). `brink-db` maps that `ParseSeverity::Error`
/// to the non-suppressible `DiagnosticCode::E037`, so the fixture fails to
/// *compile* rather than misrendering at runtime — a different failure mode
/// than #1991's assignment/bare-call case, but this test still fails
/// without the fix (it just fails at `compile_path`, not at the
/// transcript-diff step).
///
/// Extended again by issue #1972's second slice with a `~{ … }`
/// multi-statement logic block: `let k = m + 2; n = k; bump();` — "Now is
/// 13." is only reachable if all three statements inside the block
/// actually ran (`m` is `10` at that point, so `k = 12`, `n = k = 12`, then
/// `bump()`'s `n += 1;` makes it `13`). With the `L_BRACE` dispatch arm in
/// `logic_line` reverted, `~{` falls through to `expr_stmt_line`'s
/// `expr::expression`, whose `STMT_BLOCK` atom case (blocks-as-values)
/// lowers the block's statements for their diagnostics but has no `Expr`
/// representation for the block's own value (`expr::lower_expr`'s
/// `STMT_BLOCK` arm doc) — always a loud `E129`, never a silent drop, so
/// this fixture fails to compile without the fix too, the same posture as
/// the temp-decl case above.
#[test]
fn logic_line_escape() {
    assert_case("logic-line-escape");
}

/// Issue #1992: `> text` — the code-ground line escape into prose (charter
/// §8.2, RULED 2026-07-23, the mirror image of #1991 above at the opposite
/// ground). Before this landed, `>` had no dispatch in
/// `stmt::statement()`'s code-ground per-statement loop at all, so `>
/// [{chan}] {text}` at statement position inside a `fn`'s default
/// code-ground body was a parse error (`expected an expression, found GT`)
/// rather than a lowering gap.
#[test]
fn prose_line_escape() {
    assert_case("prose-line-escape");
}

/// Issue #1992 review finding F1: a `> text` split must not fragment a
/// code-ground body's T1b lexical scope. `prose-line-escape`'s own
/// `bump_and_announce` case deliberately uses the global `var n` around its
/// split, so it never exercises this — here `x` is a `let`-declared local,
/// read (via `{x}` interpolation) in the `Content` sitting *between* the
/// two split `LogicBlock` runs and written (`x += 1;`) in the second run,
/// so both the READ half (a block-scoped read after the first run's scope
/// would have been wrongly popped, misdiagnosed as E082) and the WRITE
/// half (a write that would otherwise miss its temp slot and fall through
/// to a phantom `AssignTarget::Global`) are covered by one case. Confirmed
/// to fail — E082 misdiagnosis / wrong or panicking output — with
/// `mark_split_logic_block_scopes` and `lower_logic_block`'s scope-aware
/// push/pop (`brink-ir/src/hir/lower_native/body.rs`,
/// `brink-ir/src/lir/lower/blocks.rs`) reverted.
#[test]
fn prose_line_escape_shares_scope_across_the_split() {
    assert_case("prose-line-escape-scope");
}

/// Every `tests/tier1-native/` case directory is exercised by a `#[test]`
/// above — a directory with no matching test would silently never run.
#[test]
fn every_case_directory_has_a_test() {
    let known = [
        "annotations-allow",
        "annotations-effects",
        "annotations-element",
        "annotations-was",
        "array-literal",
        "as-binding",
        "construction-literal",
        "directive-like-tag",
        "fn-value-bare-name",
        "for-k-v",
        "inline-markup-hyphenated-name",
        "inline-markup-point-marker",
        "inline-tag-embedded-brace",
        "lambda-verbs",
        "logic-line-escape",
        "or-coalescing",
        "prose-line-escape",
        "prose-line-escape-scope",
        "prose-return-value",
        "root-content-typed-strict",
        "ufcs",
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
