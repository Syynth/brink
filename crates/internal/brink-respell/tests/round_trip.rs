//! Round-trip verification for the `.brink` native-surface emitter
//! (issue #1178): `story.brink` → HIR → emit → reparse+relower → run.
//!
//! Entirely off the ink pipeline (both sides of the comparison are native
//! `.brink` source, run through the same `explore_from_brink_native` path)
//! — the oracle cannot move because this test never touches it.
//!
//! Verification is **episode-identity through the actual runtime**, not
//! HIR struct equality: the emitter's output is allowed to restructure
//! text (canonical body-then-stitches ordering, normalized whitespace,
//! `Stmt::EndOfLine` markers that carry no textual weight) as long as the
//! *compiled, executed* behavior is unchanged — exactly the differential
//! method `docs/b0-findings.md` NF-5 and this program's own exit criterion
//! describe. `brink_test_harness::corpus::explore_from_brink_native`
//! already performs the honest minimal native pipeline composition (parse
//! → `lower_native` → analyze → LIR → codegen → link → explore); this test
//! only supplies both sides of the diff.
//!
//! Fixture corpus: `tests/tier1-brink-respell/*/story.brink` — the same
//! hand-curated corpus (weave/choice/tunnel/thread/alternation-adjacent
//! semantics) `tests/tier1-brink-respell/README.md` describes as the NF-5
//! (c) seed this issue's emitter now has real machinery to regenerate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use brink_ir::FileId;
use brink_ir::hir::emit_native::emit_file;
use brink_ir::hir::lower_native;
use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/tier1-brink-respell")
        .canonicalize()
        .expect("tests/tier1-brink-respell must exist at the repo root")
}

fn explore_config() -> ExploreConfig {
    ExploreConfig {
        max_depth: 20,
        max_episodes: 200,
    }
}

/// Lower `src` through the native frontend, emit it back out, and assert
/// the emitted `.brink` plays episode-identical to the original.
fn round_trip_case(name: &str) {
    let dir = fixtures_root().join(name);
    let src = fs::read_to_string(dir.join("story.brink"))
        .unwrap_or_else(|e| panic!("{name}: read story.brink: {e}"));

    let parsed = brink_syntax_native::parse(&src);
    assert!(
        parsed.errors().is_empty(),
        "{name}: story.brink itself has parse errors: {:?}",
        parsed.errors()
    );
    let tree = parsed.tree();
    let (hir, _manifest, diags) = lower_native::lower(FileId(0), &tree);
    assert!(
        diags.is_empty(),
        "{name}: story.brink itself has lowering diagnostics: {diags:?}"
    );

    let emitted =
        emit_file(&hir).unwrap_or_else(|e| panic!("{name}: emitter refused this fixture: {e}"));

    let config = explore_config();
    let original_episodes = explore_from_brink_native(&src, &config)
        .unwrap_or_else(|e| panic!("{name}: exploring the original fixture failed: {e}"));
    let emitted_episodes = explore_from_brink_native(&emitted, &config).unwrap_or_else(|e| {
        panic!("{name}: exploring the emitted respelling failed: {e}\n--- emitted ---\n{emitted}")
    });

    assert_eq!(
        original_episodes, emitted_episodes,
        "{name}: round-trip episode mismatch\n--- emitted .brink ---\n{emitted}"
    );
}

#[test]
fn basic_tunnel() {
    round_trip_case("basic-tunnel");
}

#[test]
fn complex_flow_v1() {
    round_trip_case("complex-flow-v1");
}

#[test]
fn const_vars() {
    round_trip_case("const-vars");
}

#[test]
fn exhibit_fogg_passage() {
    round_trip_case("exhibit-fogg-passage");
}

#[test]
fn gather_basic() {
    round_trip_case("gather-basic");
}

#[test]
fn labeled_mid_flow_gather() {
    round_trip_case("labeled-mid-flow-gather");
}

#[test]
fn manual_stitch_v1() {
    round_trip_case("manual-stitch-v1");
}

#[test]
fn simple_glue() {
    round_trip_case("simple-glue");
}

#[test]
fn sticky_choice() {
    round_trip_case("sticky-choice");
}

#[test]
fn weave_options() {
    round_trip_case("weave-options");
}

/// NG-A/NG-B (issues #1487/#1488): the `: type` annotation grammar,
/// end-to-end. This is the case that proves `emit_native`'s annotation
/// spelling and the native parser agree — before the annotation grammar
/// landed, `emit_param` could already write `name: type` into a `.brink`
/// file the parser had no rule for.
#[test]
fn typed_annotations() {
    round_trip_case("typed-annotations");
}

/// Reviewer finding on #1732 (issue #1716): `emit_content_parts`'
/// `ContentPart::Text(t) => s.push_str(t)` used to push literal text
/// unescaped — safe before the markup grammar existed, but now a `Text`
/// containing e.g. `<b>` re-parses as a real `SPAN` on the way back in.
/// Likewise `emit_span` wrote attribute values raw, so a `"` or `\` in a
/// value emitted malformed source. This fixture exercises both: a span
/// attribute value containing an escaped quote (`the \"old\" lantern`) and
/// a content line containing every character in the escape set as a
/// literal (`\< \{ \# \\`, none of them opening a real span/interpolation/
/// tag/escape on the original parse) — proving `emit_native` escapes them
/// back out symmetrically rather than let any of them re-parse as live
/// syntax.
#[test]
fn inline_markup_escape_set() {
    round_trip_case("inline-markup-escape-set");
}

/// Issue #1744 (§8d.6): `\!`/`\@` as line-start escapes, plus the emitter
/// half — `emit_native::escape_leading_cue_sigil` must re-escape a
/// literal leading `@`+identifier or a real `CUE` would open on the
/// respelled source's next parse. See the fixture's own `manifest.toml`.
#[test]
fn line_start_cue_escape() {
    round_trip_case("line-start-cue-escape");
}

/// Issue #1975: the checked-in `story.brink` is `respell_ink_source`'s
/// mechanical output for `tests/tier2/conditional/ifelse-ext/story.ink`'s
/// `CondKind::IfElse` (3-way, independently-chained, no shared subject)
/// conditional, re-shaped into nested `{if …} else { {if …} else { … } }`
/// native syntax. This only proves the *emitted* nesting is itself legal,
/// episode-identical native source when re-parsed — native's own lowering
/// always reconstructs `CondKind::InitialCondition` from an `if`/`else if`
/// chain (never `IfElse`, see the fixture's own `manifest.toml`), so this
/// round trip alone doesn't exercise the new `CondKind::IfElse` emitter arm.
/// `ink_corpus_convert.rs::ifelse_ext_three_way_chain` is the differential
/// that does (it lowers the ink origin directly, constructing the real
/// `IfElse` HIR shape my fix re-shapes).
#[test]
fn else_if_chain() {
    round_trip_case("else-if-chain");
}
