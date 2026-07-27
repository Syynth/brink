//! End-to-end demonstration of #1504: two files with root-level weave
//! content miscompile into colliding container ids.
//!
//! This is the user-facing half of the analysis in
//! `docs/root-content-identity-findings.md`. The LIR-level acceptance tests
//! live in `brink-ir/tests/lir_lowering/root_content_definition_id_soundness.rs`.
//!
//! Reachable through the ordinary compiler entry point
//! (`brink_compiler::compile`) with a plain `INCLUDE` — no unusual flags,
//! no native dialect, no incremental session. Any ink project where the
//! entry file **and** an included file both carry root-level weave content
//! hits it.
//!
//! Both tests are **acceptance tests for the fix** and are `#[ignore]`d:
//! #1504 is labeled `needs-design` and the fix shape is blocked on the
//! FG-4d identity ruling. Un-ignoring them is the acceptance criterion.
//! Do not rewrite them to assert the current (wrong) behavior.
//!
//! #1673 added a codegen-boundary guard that refuses a `Program` containing
//! two containers with the same `DefinitionId` (see
//! `included_and_entry_root_weaves_trip_the_duplicate_definition_id_guard`
//! below). That guard now intercepts this exact shape *before* either
//! `#[ignore]`d test's own `compile_mem(...).unwrap()` — so running them
//! today (`cargo test -- --ignored`) panics on that `unwrap()` with the
//! guard's `E060` error rather than reaching the assertions they were
//! originally written to check. That's the intended effect of #1673: the
//! underlying #1504(a) id-derivation defect is unchanged, but it now fails
//! loudly at compile time instead of silently producing wrong player-visible
//! output. Both tests remain valid as acceptance tests for #1504(a) itself:
//! once the real fix removes the id collision, the guard has nothing to
//! fire on and both compiles succeed again, exactly as originally written.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::{BTreeMap, HashMap};

use brink_runtime::{DotNetRng, Line, Story};

/// Compile from an in-memory file system, mirroring `driver.rs`'s helper.
fn compile_mem(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_format::StoryData, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
    .map(|output| output.data)
}

/// The compiled program must not contain two containers with one id.
///
/// Measured on `origin/main` (commit 999581354): 8 containers, of which
/// three ids appear twice (`0x1779765f903c98e`, `0x1dde84850f175fb`,
/// `0x1ef2ee91775101d`).
#[test]
#[ignore = "known bug #1504(a); fix is blocked on the FG-4d identity ruling"]
fn included_and_entry_root_weaves_get_distinct_container_ids() {
    let files: HashMap<&str, &str> = HashMap::from([
        (
            "main.ink",
            "INCLUDE inc.ink\n* main one\n* main two\n- main gathered\n",
        ),
        ("inc.ink", "* inc one\n* inc two\n- inc gathered\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();

    let mut seen: BTreeMap<u64, usize> = BTreeMap::new();
    for container in &story.containers {
        *seen.entry(container.id.to_raw()).or_default() += 1;
    }
    let dupes: Vec<String> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, count)| format!("0x{id:x} appears {count}x"))
        .collect();

    assert!(
        dupes.is_empty(),
        "duplicate container ids in the compiled program: {dupes:#?}",
    );
}

/// #1673: until #1504(a) lands, this exact shape still produces two
/// containers with the same `DefinitionId` (proven by the `#[ignore]`d
/// `included_and_entry_root_weaves_get_distinct_container_ids` above). The
/// codegen-boundary uniqueness guard (`brink-codegen-inkb`, #1673) must
/// catch that at compile time — through the ordinary `brink_compiler::
/// compile` entry point, no unusual flags — instead of letting it reach the
/// runtime silently, which is what `choosing_an_included_files_choice_
/// runs_that_files_body` above demonstrates it does downstream of codegen
/// today.
///
/// Deliberately **not** `#[ignore]`d: this must be green both before and
/// after #1504(a) lands. Before: the guard fires on the collision. After:
/// the fix removes the collision the guard would have fired on, so
/// `compile_mem` starts succeeding here too — at which point this test
/// should be rewritten to assert `Ok`, mirroring the sibling test above.
#[test]
fn included_and_entry_root_weaves_trip_the_duplicate_definition_id_guard() {
    let files: HashMap<&str, &str> = HashMap::from([
        (
            "main.ink",
            "INCLUDE inc.ink\n* main one\n* main two\n- main gathered\n",
        ),
        ("inc.ink", "* inc one\n* inc two\n- inc gathered\n"),
    ]);

    let err = compile_mem("main.ink", &files)
        .expect_err("the #1504 collision should trip the #1673 codegen guard, not compile");

    // The real pipeline (`driver::compile_with_options`) routes a codegen
    // `Err` through the diagnostics query (`brink-db`'s `story_data`), which
    // wraps it as an `E060` `ResolvedDiagnostic` rather than surfacing the
    // raw `CompileError::Codegen` variant directly — see the `E060` entry in
    // `e0xx_diagnostics.rs`.
    let brink_compiler::CompileError::Diagnostics(diagnostics) = err else {
        panic!("expected CompileError::Diagnostics, got a different variant: {err:?}");
    };
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one diagnostic: {diagnostics:#?}"
    );
    assert_eq!(diagnostics[0].code, brink_compiler::DiagnosticCode::E060);
    assert!(
        diagnostics[0].message.contains("duplicate DefinitionId"),
        "expected the #1673 guard's message, got: {}",
        diagnostics[0].message
    );
}

/// The collision is observable as wrong output: the linker's address map is
/// last-write-wins (`brink-runtime/src/linker.rs:88`), so the entry file's
/// root-weave containers overwrite the included file's. Picking the
/// included file's first choice runs the **entry** file's first choice body.
///
/// Measured on `origin/main` (commit 999581354), choosing index 0 from the
/// `inc one` / `inc two` set yields `main one` + `MAIN-ONE-BODY`;
/// `INC-ONE-BODY` never executes.
#[test]
#[ignore = "known bug #1504(a); fix is blocked on the FG-4d identity ruling"]
fn choosing_an_included_files_choice_runs_that_files_body() {
    let files: HashMap<&str, &str> = HashMap::from([
        (
            "main.ink",
            "INCLUDE inc.ink\n* main one\n  MAIN-ONE-BODY\n* main two\n  MAIN-TWO-BODY\n- main gathered\n",
        ),
        (
            "inc.ink",
            "* inc one\n  INC-ONE-BODY\n* inc two\n  INC-TWO-BODY\n- inc gathered\n",
        ),
    ]);

    let data = compile_mem("main.ink", &files).unwrap();
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    // The first choice set is the included file's.
    let Line::Choices { choices, .. } = story.continue_single().unwrap() else {
        panic!("expected the included file's root choice set first");
    };
    assert_eq!(choices[0].text, "inc one");
    story.choose(0).unwrap();

    // Whatever ran must be the body of the choice the player was offered.
    let mut output = String::new();
    for _ in 0..4 {
        let line = story.continue_single().unwrap();
        output.push_str(line.text());
        if matches!(line, Line::Done { .. } | Line::End { .. }) {
            break;
        }
    }

    assert!(
        output.contains("INC-ONE-BODY"),
        "picking `inc one` ran the wrong choice body; got: {output:?}",
    );
    assert!(
        !output.contains("MAIN-ONE-BODY"),
        "picking `inc one` ran the entry file's body instead; got: {output:?}",
    );
}
