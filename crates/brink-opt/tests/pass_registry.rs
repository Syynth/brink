//! The pass registry's invariants, and proof that each negative control
//! actually edits an artifact.
//!
//! These run against a hand-built `StoryData` with no compiler in the loop —
//! `brink-opt` depends on `brink-format` and nothing else, and that stays true
//! for its tests. It is also what lets the corpus sweeps in `brink-test-harness`
//! assume "the control was applied": that fact is established here, without the
//! compiler, so a failure there is never ambiguous between "the control is
//! broken" and "the fence did not run it".

// Integration-test convention across this directory: helpers outside `#[test]`
// fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use brink_format::{
    DefinitionId, DefinitionTag, LineContent, LineEntry, LineFlags, ScopeLineTable, StoryData,
};
use brink_opt::{CONTROL_PREFIX, OptConfig, optimize};

/// A story with one scope holding one plain line. Enough for every control:
/// they all key on `line_tables`.
fn one_line_story(text: &str) -> StoryData {
    let content = LineContent::Plain(text.to_owned());
    StoryData {
        containers: Vec::new(),
        line_tables: vec![ScopeLineTable {
            scope_id: DefinitionId::new(DefinitionTag::Address, 1),
            lines: vec![LineEntry {
                flags: LineFlags::from_content(&content),
                content,
                source_hash: brink_format::content_hash(text),
                audio_ref: None,
                slot_info: Vec::new(),
                source_location: None,
            }],
        }],
        variables: Vec::new(),
        list_defs: Vec::new(),
        list_items: Vec::new(),
        externals: Vec::new(),
        addresses: Vec::new(),
        address_paths: Vec::new(),
        name_table: Vec::new(),
        list_literals: Vec::new(),
        literal_pool: Vec::new(),
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        alias_table: Vec::new(),
        effect_rows: Vec::new(),
        frame_shapes: Vec::new(),
        debug_info: None,
        line_variant_groups: Vec::new(),
        source_checksum: 0,
    }
}

/// The single line of `one_line_story`.
#[cfg(feature = "test-control")]
fn only_line(story: &StoryData) -> &LineEntry {
    &story.line_tables[0].lines[0]
}

/// The resident pass list, in run order (`docs/optimizer-spec.md` §8.2 —
/// the first real pass, `docs/optimizer-peephole.md`). Edited deliberately
/// whenever a pass lands or moves — that is the point of it, not an
/// obstacle.
#[test]
fn default_pass_set_is_the_resident_list() {
    let config = OptConfig::defaults();
    assert_eq!(
        config.passes.names(),
        vec![brink_opt::EmitLineNl::NAME, brink_opt::BinaryFusion::NAME],
        "the resident pass list, in run order"
    );
}

/// The guard that survives feature unification.
///
/// `cargo test --workspace` turns `test-control` on for every `brink-opt` in the
/// graph, so "the feature is off in release" is not a safety property. This test
/// has no `cfg` at all: it runs in both feature states and asserts the thing
/// that actually matters — that no *default* pass is a control.
#[test]
fn default_pass_set_contains_no_control_pass() {
    let names = OptConfig::defaults().passes.names();
    let leaked: Vec<_> = names
        .iter()
        .filter(|n| n.starts_with(CONTROL_PREFIX))
        .collect();
    assert!(
        leaked.is_empty(),
        "negative-control passes reached the default set: {leaked:?}"
    );
}

/// A story with none of the passes' input shapes (here: no containers, so
/// no `EmitLine`/`EmitNewline` pair) comes out untouched: every pass runs,
/// none reports a change, and before/after stats are identical.
#[test]
fn the_default_passes_leave_a_story_without_their_shapes_untouched() {
    let mut story = one_line_story("hello");
    let before = story.clone();
    let report = optimize(&mut story, &OptConfig::defaults());

    assert_eq!(report.passes.len(), 2, "both resident passes ran");
    assert!(!report.changed(), "nothing should have changed");
    assert_eq!(report.before, report.after, "stats must not move");
    assert_eq!(story, before, "the story must be untouched");
    assert_eq!(report.before.line_entries, 1, "the fixture has one line");
}

#[cfg(feature = "test-control")]
mod controls {
    use super::{OptConfig, one_line_story, only_line, optimize};
    use brink_format::LineContent;
    use brink_opt::control;

    /// Controls are never in the default set. Belt and braces alongside
    /// `default_pass_set_contains_no_control_pass`: that one checks the prefix,
    /// this one checks the actual names.
    #[test]
    fn control_passes_are_disjoint_from_the_default_set() {
        let defaults = OptConfig::defaults().passes.names();
        for name in control::ALL {
            assert!(
                !defaults.contains(&name),
                "{name} is in the default pass set"
            );
        }
    }

    /// Every control resolves to a pass set, and a typo does not silently
    /// produce an empty one (which would then pass every obligation).
    #[test]
    fn every_control_name_resolves_and_an_unknown_one_does_not() {
        for name in control::ALL {
            let set = control::pass_set(name).expect("known control");
            assert_eq!(set.names(), vec![name]);
        }
        assert!(
            control::pass_set("control:nope").is_none(),
            "an unknown control name must not resolve to an empty set"
        );
    }

    /// `retext` moves rendered text and leaves translation identity alone.
    ///
    /// This is one half of the pair that proves the trace oracle and the
    /// line-identity oracle are independently wired.
    #[test]
    fn retext_moves_content_and_leaves_the_hash() {
        let mut story = one_line_story("hello");
        let before = only_line(&story).clone();
        let report = optimize(&mut story, &control::config("control:retext"));

        assert!(report.changed(), "retext must report a change");
        let after = only_line(&story);
        assert_ne!(after.content, before.content, "content must move");
        assert_eq!(after.source_hash, before.source_hash, "hash must NOT move");
        assert_eq!(
            after.flags,
            brink_format::LineFlags::from_content(&after.content),
            "flags must be recomputed, or the output buffer can suppress the sentinel"
        );
    }

    /// `rehash` moves translation identity and leaves rendered text alone —
    /// the other half of the pair.
    #[test]
    fn rehash_moves_the_hash_and_leaves_content() {
        let mut story = one_line_story("hello");
        let before = only_line(&story).clone();
        let report = optimize(&mut story, &control::config("control:rehash"));

        assert!(report.changed(), "rehash must report a change");
        let after = only_line(&story);
        assert_eq!(after.content, before.content, "content must NOT move");
        assert_ne!(after.source_hash, before.source_hash, "hash must move");
    }

    /// `grow` accumulates, so it is not idempotent — which is exactly what it
    /// is for. `retext` cannot test the idempotence check because it assigns a
    /// constant.
    #[test]
    fn grow_accumulates_across_runs() {
        let mut story = one_line_story("hello");
        let config = control::config("control:grow");

        optimize(&mut story, &config);
        let once = only_line(&story).content.clone();
        optimize(&mut story, &config);
        let twice = only_line(&story).content.clone();

        assert_ne!(once, twice, "grow must not be idempotent");
        match (&once, &twice) {
            (LineContent::Plain(a), LineContent::Plain(b)) => {
                assert!(b.len() > a.len(), "the second run must lengthen the line");
            }
            _ => panic!("the fixture is plain content"),
        }
    }

    /// `drift` differs between two runs over the *same* input, without touching
    /// anything either semantic oracle reads.
    #[test]
    fn drift_differs_between_two_runs_of_the_same_input() {
        let config = control::config("control:drift");

        let mut first = one_line_story("hello");
        optimize(&mut first, &config);
        let mut second = one_line_story("hello");
        optimize(&mut second, &config);

        assert_ne!(
            only_line(&first).audio_ref,
            only_line(&second).audio_ref,
            "drift must produce different bytes on each run"
        );
        assert_eq!(
            only_line(&first).content,
            only_line(&second).content,
            "drift must not touch content"
        );
        assert_eq!(
            only_line(&first).source_hash,
            only_line(&second).source_hash,
            "drift must not touch the hash"
        );
    }

    /// A control with nothing to edit reports no change rather than claiming
    /// one — so the corpus sweeps can classify it `inert` instead of counting a
    /// false survivor.
    #[test]
    fn a_control_with_no_lines_reports_no_change() {
        let mut story = one_line_story("hello");
        story.line_tables.clear();
        for name in control::ALL {
            let report = optimize(&mut story, &control::config(name));
            assert!(!report.changed(), "{name} claimed a change with no lines");
        }
    }
}
