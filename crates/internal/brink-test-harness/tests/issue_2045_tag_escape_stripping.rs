//! Issue #2045 end-to-end: a *recognized* inline escape (`\< \{ \# \\`,
//! §8d.6) strips its backslash from a `#tag`'s materialized text, in
//! parity with `markup::escape`'s stripping for ordinary content — proven
//! through the real `.brink` pipeline, not just `hir::lower_native`'s
//! `lower_src` unit tests (`brink-ir/src/hir/lower_native/tests.rs`).
//!
//! `brink_test_harness::corpus::explore_from_brink_native` is the same
//! honest minimal native pipeline `b5_construction_e2e.rs` runs the
//! construction-literal corpus through: parse → `hir::lower_native` →
//! analyzer → LIR → codegen → link → explore. Unlike the `tier1-native`
//! golden-transcript corpus (`tests/tier1-native/`,
//! `brink_test_harness::corpus::run_native_transcript`), which only
//! concatenates `Step::Line`'s content and structurally discards tags, this
//! reads `StepRecord.tags` directly off the recorded `Episode` — the one
//! path in this harness that can actually observe a tag's text end to
//! end, which is why this is a new file rather than an extension of that
//! corpus.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

/// The tags recorded on the first step of a straight-line native
/// fixture's only episode.
fn tags(src: &str) -> Vec<String> {
    let episodes = explore_from_brink_native(src, &ExploreConfig::default())
        .unwrap_or_else(|e| panic!("native fixture must compile and play: {e}"));
    let episode = episodes.first().expect("one episode");
    episode.steps[0].tags.clone()
}

#[test]
fn a_recognized_hash_escape_in_a_tag_strips_its_backslash_end_to_end() {
    // #1738's own motivating example, playing all the way through the
    // real runtime: the content line's own `\#` already strips (parity
    // with `markup::escape`, unaffected by this issue) — this fixture
    // pins the trailing tag's `\#`, which #2045 now strips too.
    let out = tags(
        "\
flow main() {
  Hello \\# world #a \\#b
  -> END
}
",
    );
    assert_eq!(
        out,
        vec!["a #b".to_owned()],
        "a recognized `\\#` inside a tag's text must strip its backslash, \
         matching ordinary content's `markup::escape` behavior"
    );
}

#[test]
fn a_recognized_open_brace_escape_in_a_tag_strips_its_backslash_end_to_end() {
    // Issue #2045's own scope note: `\{` gets the identical treatment as
    // `\#`, not just the hash case.
    let out = tags(
        "\
flow main() {
  Hello. #tag \\{gold
  -> END
}
",
    );
    assert_eq!(out, vec!["tag {gold".to_owned()]);
}

#[test]
fn a_recognized_backslash_escape_in_a_tag_strips_its_backslash_end_to_end() {
    // Issue #2045 review finding: `\\` (a backslash escaping a backslash)
    // is the fourth member of the §8d.6 recognized set and was the one
    // actually broken by the prior run-parity-only reading — a bare `\\`
    // pair with nothing recognized following it never stripped, unlike
    // `markup::escape`'s greedy consumption for ordinary content, which
    // collapses the pair to one literal `\` regardless of what follows.
    let out = tags(
        "\
flow main() {
  Hello. #a\\\\b
  -> END
}
",
    );
    assert_eq!(out, vec!["a\\b".to_owned()]);
}
