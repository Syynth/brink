//! Issue #2197: the stdlib mount (#2080) collides with a project's own
//! same-named definitions through the REAL production compile path.
//!
//! Every other native fixture this crate runs (`tier1_native.rs`,
//! `tier1_native_strict.rs`, `corpus_report`'s native section) compiles
//! through `brink_compiler::compile_path`, whose own doc says plainly:
//! "Bypasses `Environment` entirely, so a real consumer should use
//! `brink_environment::compile(&Environment)` instead." The stdlib mount
//! (`brink_environment`'s `mount_stdlib`, #2080) only exists inside
//! `Environment` — so the entire 5,608-episode oracle ratchet, and every
//! `tests/tier1-native/` golden, is structurally blind to it. This file
//! closes that specific gap by compiling through the real path (what
//! `brink play`/`brink compile` actually run) and asserting the collision
//! is fixed, not merely that the compile no longer errors.
//!
//! ⚠ Rule 20a: verified this test FAILS on the pre-fix code (reverting the
//! `lookup_container_id`/`lookup_global`/`lookup_label_id` file-scoping in
//! `brink-ir` reproduces the exact `[E060] internal codegen error: duplicate
//! DefinitionId … assigned to two different containers, at paths
//! "scene_entered" and "scene_entered"` failure this test's first assertion
//! (`compile(&env)` returning `Ok`) would catch — see this PR's description
//! for the reverted-and-reran transcript.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use brink_environment::{OptionOverrides, Project};
use brink_source_tree::InMemory;

/// The golden fixture this test reuses verbatim (issue #1720/#2092): it
/// declares its OWN `extern scene_entered` + no-op `fn scene_entered`
/// fallback, plus its own `heading`/`transition`/`cue`/`parenthetical`
/// convention handlers — deliberately the SAME four handlers (and the
/// same `scene_entered` extern/fallback pair) the shipped
/// `std/conventions/screenplay.brink` preset ships, per that file's own
/// header comment. Reading it from disk (rather than duplicating the text
/// inline) keeps this test byte-for-byte in sync with the checked-in
/// fixture and its `tier1_native.rs` coverage.
fn fixture_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native")
        .join("conventions-screenplay-preset")
        .join("story.brink");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn expected_transcript() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native")
        .join("conventions-screenplay-preset")
        .join("expected.txt");
    brink_test_harness::corpus::load_golden_transcript(&path, "conventions-screenplay-preset")
        .expect("golden transcript must be present and non-vacuous")
}

/// Link a compiled `StoryData` and run it to completion, returning the
/// concatenated output text. Mirrors `brink_test_harness::corpus::
/// run_native_transcript`'s own drive loop (that helper takes a `&Path` and
/// always compiles via `compile_path`, so it can't be reused directly for
/// output already compiled through `Environment`).
fn run_to_completion(data: &brink_format::StoryData) -> String {
    let (program, line_tables) = brink_runtime::link(data).expect("link");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );
    let mut out = String::new();
    let mut line_count = 0usize;
    loop {
        match story.continue_single().expect("runtime step") {
            brink_runtime::Step::Line(line) => out.push_str(&line.text),
            brink_runtime::Step::Done
            | brink_runtime::Step::End
            | brink_runtime::Step::Suspended => break,
            brink_runtime::Step::Choices(_) => {
                panic!("conventions-screenplay-preset must stay choice-free")
            }
        }
        line_count += 1;
        assert!(
            line_count < brink_runtime::FlowInstance::LINE_LIMIT,
            "exceeded FlowInstance::LINE_LIMIT without reaching a terminal step"
        );
    }
    out
}

/// Every container in `data` whose author-facing name is `name`, as
/// `(DefinitionId, scope_id)` pairs — `ContainerDef::name` is only set for
/// scope-owning containers (root/knot/stitch), which is exactly the
/// knot-level identity this test cares about.
fn containers_named<'a>(
    data: &'a brink_format::StoryData,
    name: &str,
) -> Vec<&'a brink_format::ContainerDef> {
    data.containers
        .iter()
        .filter(|c| {
            c.name.is_some_and(|n| {
                data.name_table
                    .get(usize::from(n.0))
                    .is_some_and(|s| s == name)
            })
        })
        .collect()
}

#[test]
fn stdlib_mount_no_longer_collides_with_a_projects_own_scene_entered() {
    // ── Ground truth: compile the fixture ALONE (no mount at all) ──────
    // `brink_compiler::compile_path` never mounts the stdlib (#2080's mount
    // lives only inside `Environment`), so this is exactly what
    // `tier1_native.rs`'s own `assert_case` already exercises for this
    // fixture — the project's own `scene_entered` knot's `DefinitionId`,
    // completely uncontested by any std candidate.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native")
        .join("conventions-screenplay-preset");
    let isolated = brink_compiler::compile_path(&dir.join("story.brink")).expect(
        "the fixture must still compile in isolation (compile_path, no stdlib mount) — this is \
         the existing tier1_native.rs coverage, unrelated to #2197",
    );
    let isolated_scene_entered = containers_named(&isolated.data, "scene_entered");
    assert_eq!(
        isolated_scene_entered.len(),
        1,
        "in isolation there is exactly one `scene_entered` container: the project's own"
    );
    let project_owned_id = isolated_scene_entered[0].id;

    // ── The real production path: brink_environment::compile ───────────
    // A native entry's compilation universe is "tree is universe"
    // (`brink_environment`'s `collect_sources` doc) — every `.brink` key
    // joins, including the mounted `std/conventions/screenplay.brink`,
    // which declares its OWN `extern scene_entered` + `fn scene_entered`
    // fallback + its own `heading`/`transition`/`cue`/`parenthetical`,
    // colliding by bare name with this fixture's identical declarations.
    // Before the #2197 fix this hard-fails with `[E060] internal codegen
    // error: duplicate DefinitionId … assigned to two different
    // containers, at paths "scene_entered" and "scene_entered"`.
    let tree = InMemory::new(BTreeMap::from([(
        "story.brink".to_string(),
        fixture_source(),
    )]));
    let env = Project::load(&tree, "story.brink", &OptionOverrides::default())
        .expect("Environment::load must succeed for a plain native project");
    let mounted = brink_environment::compile(&env).unwrap_or_else(|e| {
        panic!(
            "compiling through the real production path (brink_environment::compile) must \
             succeed — the stdlib mount must not collide with the project's own \
             `scene_entered`/convention declarations: {e}"
        )
    });

    // ── Assert the CORRECT definition resolves, not merely that it built ──
    // Two `scene_entered` knots now coexist (the project's own + the
    // mounted std module's own), and they must be genuinely distinct
    // containers — never the collapsed-to-one-id shape that trips codegen's
    // `#1673` duplicate-`DefinitionId` guard.
    let mounted_scene_entered = containers_named(&mounted.data, "scene_entered");
    assert_eq!(
        mounted_scene_entered.len(),
        2,
        "expected exactly two `scene_entered` containers (the project's own + the mounted \
         std module's own) once the mount is compiled alongside the project, got: {:?}",
        mounted_scene_entered
            .iter()
            .map(|c| c.id)
            .collect::<Vec<_>>()
    );
    let mounted_ids: Vec<_> = mounted_scene_entered.iter().map(|c| c.id).collect();
    assert_ne!(
        mounted_ids[0], mounted_ids[1],
        "the project's own `scene_entered` and the mounted std module's own `scene_entered` \
         must hash to DIFFERENT DefinitionIds (module-qualified identity) — an identical hash \
         here is exactly the #2197 collision"
    );
    assert!(
        mounted_ids.contains(&project_owned_id),
        "the project's own `scene_entered` must keep the EXACT SAME DefinitionId it has when \
         compiled in isolation (save-key stability) — presence of the stdlib mount alongside it \
         must not change which id the project's own declaration resolves to. Isolated id: \
         {project_owned_id:?}, mounted ids: {mounted_ids:?}"
    );

    // ── End to end: the story must still run and produce the exact same ──
    // ── transcript as the isolated (unmounted) compile.                 ──
    let transcript = run_to_completion(&mounted.data);
    assert_eq!(
        transcript,
        expected_transcript(),
        "the mounted compile's transcript must match the golden `expected.txt` — proving the \
         project's own conventions/extern-fallback pair actually ran end to end, not just that \
         compilation succeeded"
    );
}
