//! Issue #3185 (D7, `docs/debugger-spec.md` §3): named locals on the
//! runtime debug surface — a per-container temp slot->name table (D6's
//! wire framing, populated by codegen here) resolved, at runtime, to the
//! live values in a call frame's `temps`, exposed additively on
//! `DebugFrame::locals`.
//!
//! Proof shape, per the issue's own acceptance bar:
//!
//! 1. **A function (tunnel) with several live temps**, mid-execution — the
//!    snapshot is taken at a choice presentation, the one point the VM
//!    genuinely stops and cannot have run ahead: `debug_snapshot()` reads
//!    the VM's *live* position, and this runtime's `continue_single` can
//!    buffer/queue several already-produced lines internally before a
//!    caller has drained them (verified empirically while building this
//!    test — pausing on a specific line of *text* does not pin the VM's
//!    position the way it first appears to), so a choice — the one event
//!    that forces the VM to actually stop and wait — is the only boundary
//!    this test trusts.
//! 2. **Names bound to the right values** — asserted against the actual
//!    runtime `Value`s the VM computed, not against the table's own
//!    output.
//! 3. **A parameter** — `calc`'s `n` — is a named local exactly like a
//!    `~ temp`, because it's bound by the same kind of `DeclareTemp` slot.
//! 4. **A value of each kind the issue calls out**: int, float, string,
//!    list, divert target, struct.
//! 5. **A shadowing case**: two nested tunnel calls each declare their own
//!    temp named `x` with a different value — each call frame's `locals`
//!    must report *its own* frame's binding, proving locals are resolved
//!    per call frame, not leaked across frames that happen to reuse a
//!    slot number or a name.
//! 6. **Both surfaces** (`.ink` and `.brink`) — the pipeline converges at
//!    HIR (`CLAUDE.md`), so both exercise the same codegen/runtime path;
//!    the native fixture below covers the parameter+temps+shadowing shape
//!    with a narrower set of value kinds (the value-kind formatting itself
//!    is exercised in full on the `.ink` fixture above, and is
//!    surface-independent past HIR).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use brink_compiler::AnalysisOptions;
use brink_runtime::{DebugValue, DotNetRng, Story, link};

// ─── `.ink` fixture: parameter + one of each named value kind ──────────────

const INK_SRC: &str = "\
LIST Fruit = Apple, Pear, Banana\n\
STRUCT Point = #{x: int, y: int}\n\
\n\
-> start\n\
\n\
=== start ===\n\
-> calc(5) ->\n\
Done.\n\
-> END\n\
\n\
=== calc(n) ===\n\
~ temp doubled = n * 2\n\
~ temp ratio = 3.5\n\
~ temp label = \"hello\"\n\
~ temp fruits = (Apple, Pear)\n\
~ temp target = -> start\n\
~ temp p = Point#{x: 1, y: 2}\n\
* [Continue]\n\
    ->->\n\
";

fn compiled_ink(src: &str) -> brink_format::StoryData {
    let options = AnalysisOptions {
        emit_debug_info: true,
        // The fixture's `STRUCT`/`#{...}` construction literal is a brink
        // extension (`E051` under the `StrictInk` default) — this flag
        // gates *syntax*, not the ink-vs-native frontend (that's the
        // `.ink`/`.brink` file extension, driven below by
        // `compile_with_options`'s own dispatch), so the fixture stays
        // parsed by `brink-syntax` (the `.ink`-compat frontend) throughout.
        dialect: brink_compiler::Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("story.ink", |_p| Ok(src.to_owned()), options)
        .unwrap_or_else(|e| panic!("fixture must compile cleanly: {e:?}"))
        .data
}

fn story_for(data: &brink_format::StoryData) -> Story<DotNetRng> {
    let (program, line_tables) = link(data).unwrap_or_else(|e| panic!("link failed: {e:?}"));
    Story::new(Arc::new(program), line_tables)
}

/// Drive `story` forward until it presents choices — the only point this
/// runtime's `continue_single` is guaranteed to have actually stopped (see
/// this file's module doc). Panics if the story ends or errors first.
fn run_to_choices(story: &mut Story<DotNetRng>) {
    loop {
        match story.continue_single().expect("VM step") {
            brink_runtime::Step::Choices(_) => return,
            brink_runtime::Step::Line(_) | brink_runtime::Step::Done => {}
            other => panic!("expected to reach a choice before {other:?}"),
        }
    }
}

/// Find a named local on a frame's locals by name, panicking with the full
/// frame contents on a miss (so a failure is diagnosable, not a bare "not
/// found").
fn local<'a>(locals: &'a [brink_runtime::DebugLocal], name: &str) -> &'a brink_runtime::DebugLocal {
    locals.iter().find(|l| l.name == name).unwrap_or_else(|| {
        let names: Vec<&str> = locals.iter().map(|l| l.name.as_str()).collect();
        panic!("no local named {name:?} — frame locals were {names:?}")
    })
}

#[test]
fn ink_tunnel_reports_named_locals_of_every_value_kind() {
    let data = compiled_ink(INK_SRC);
    let mut story = story_for(&data);
    run_to_choices(&mut story);

    let snap = story.debug_snapshot();
    let frame = snap
        .call_stack
        .iter()
        .find(|f| f.kind == "tunnel")
        .expect("calc's tunnel frame must be on the call stack at the choice");
    let locals = frame
        .locals
        .as_ref()
        .expect("DebugInfo was requested, frame locals must be Some");

    // Parameter.
    assert!(matches!(local(locals, "n").value, DebugValue::Int(5)));
    // Int.
    assert!(matches!(
        local(locals, "doubled").value,
        DebugValue::Int(10)
    ));
    // Float.
    match local(locals, "ratio").value {
        DebugValue::Float(f) => assert!((f - 3.5).abs() < f32::EPSILON),
        ref other => panic!("expected Float, got {other:?}"),
    }
    // String.
    match &local(locals, "label").value {
        DebugValue::Str(s) => assert_eq!(s, "hello"),
        other => panic!("expected Str, got {other:?}"),
    }
    // List.
    match &local(locals, "fruits").value {
        DebugValue::List(members) => {
            let mut m = members.clone();
            m.sort();
            assert_eq!(m, vec!["Fruit.Apple".to_string(), "Fruit.Pear".to_string()]);
        }
        other => panic!("expected List, got {other:?}"),
    }
    // Divert target.
    match &local(locals, "target").value {
        DebugValue::DivertTarget(Some(path)) => assert_eq!(path, "start"),
        other => panic!("expected DivertTarget(Some(\"start\")), got {other:?}"),
    }
    // Struct.
    match &local(locals, "p").value {
        DebugValue::Struct { name, fields } => {
            assert_eq!(name.as_deref(), Some("Point"));
            let mut named: Vec<(&str, &DebugValue)> =
                fields.iter().map(|(n, v)| (n.as_str(), v)).collect();
            named.sort_by_key(|(n, _)| *n);
            assert_eq!(named.len(), 2);
            assert_eq!(named[0].0, "x");
            assert!(matches!(named[0].1, DebugValue::Int(1)));
            assert_eq!(named[1].0, "y");
            assert!(matches!(named[1].1, DebugValue::Int(2)));
        }
        other => panic!("expected Struct, got {other:?}"),
    }

    // The slot each local reports must actually index a real `temps` entry
    // on the frame (the runtime, not just the table, agrees the slot is
    // live) — cross-checked against the frame's own `temps` count.
    for l in locals {
        assert!(
            (l.slot as usize) < frame.temps,
            "local {:?} claims slot {} but frame only has {} temps",
            l.name,
            l.slot,
            frame.temps
        );
    }
}

/// `DebugFrame::locals` is `None`, not `Some(vec![])`, when the compiled
/// story carries no `DebugInfo` at all — the "no debug info" case must be
/// distinguishable from "this frame genuinely has zero locals".
#[test]
fn no_debug_info_reports_locals_none_not_empty() {
    let options = AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink, // fixture needs STRUCT syntax; emit_debug_info stays false
        ..AnalysisOptions::default()
    };
    let data =
        brink_compiler::compile_with_options("story.ink", |_p| Ok(INK_SRC.to_owned()), options)
            .unwrap_or_else(|e| panic!("fixture must compile cleanly: {e:?}"))
            .data;
    let mut story = story_for(&data);
    run_to_choices(&mut story);

    let snap = story.debug_snapshot();
    let frame = snap
        .call_stack
        .iter()
        .find(|f| f.kind == "tunnel")
        .expect("calc's tunnel frame must be on the call stack at the choice");
    assert!(
        frame.locals.is_none(),
        "expected None with no DebugInfo, got {:?}",
        frame.locals
    );
}

// ─── `.brink` fixture: parameter + temps + shadowing across call frames ────
//
// Each tunnel presents its own one-choice `{? * [Continue] { ... } }` right
// after declaring its own `x` — a real yield point, so the debug snapshot
// taken there reflects that frame's declarations and no more (see this
// file's module doc for why a choice, not a line of text, is the boundary
// this test trusts).

const NATIVE_SRC: &str = "\
flow main() {\n\
  -> outer(1) ->\n\
  Done.\n\
  -> END\n\
}\n\
\n\
flow outer(n: int) {\n\
  ~ let x = 10\n\
  {?\n\
    * [Continue] {\n\
        -> inner(2) ->\n\
        return\n\
      }\n\
  }\n\
}\n\
\n\
flow inner(n: int) {\n\
  ~ let x = 99\n\
  {?\n\
    * [Continue] {\n\
        return\n\
      }\n\
  }\n\
}\n\
";

/// `brink_compiler::compile*` (the `read_file`-callback entry points) only
/// drive the `.ink`-surface frontend (`brink-syntax`); the `.brink` native
/// surface dispatches on the entry path's own extension
/// (`brink_driver::is_native`) and discovers via `RealFs` off real disk —
/// `read_file` is unused for a native entry (`driver.rs`'s own doc) — so,
/// like `crates/brink-cli/tests/debug_info_flag_cli.rs`'s
/// `project_dir`/`fs::write` pattern, the fixture is written to a real temp
/// file and compiled through `compile_path_with_options`.
fn compiled_native(src: &str) -> brink_format::StoryData {
    let dir = std::env::temp_dir().join(format!("brink-issue-3185-locals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create temp dir: {e}"));
    let entry = dir.join("story.brink");
    std::fs::write(&entry, src).unwrap_or_else(|e| panic!("write fixture: {e}"));

    let options = AnalysisOptions {
        emit_debug_info: true,
        dialect: brink_compiler::Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let data = brink_compiler::compile_path_with_options(&entry, options)
        .unwrap_or_else(|e| panic!("native fixture must compile cleanly: {e:?}"))
        .data;
    let _ = std::fs::remove_dir_all(&dir);
    data
}

#[test]
fn native_nested_tunnels_report_per_frame_shadowed_locals() {
    let data = compiled_native(NATIVE_SRC);
    let mut story = story_for(&data);

    // First choice: inside `outer`, before it tunnels into `inner`.
    run_to_choices(&mut story);
    let snap = story.debug_snapshot();
    let outer_frame = snap
        .call_stack
        .iter()
        .find(|f| f.kind == "tunnel")
        .expect("outer's tunnel frame must be on the call stack");
    let outer_locals = outer_frame.locals.as_ref().expect("DebugInfo requested");
    assert!(matches!(local(outer_locals, "n").value, DebugValue::Int(1)));
    assert!(matches!(
        local(outer_locals, "x").value,
        DebugValue::Int(10)
    ));

    // Choose "Continue" — enters `-> inner(2) ->`.
    story.choose(0).expect("choose outer's Continue");

    // Second choice: inside `inner`, tunneled from `outer` — both tunnel
    // frames are live on the call stack.
    run_to_choices(&mut story);
    let snap = story.debug_snapshot();
    let tunnel_frames: Vec<&brink_runtime::DebugFrame> = snap
        .call_stack
        .iter()
        .filter(|f| f.kind == "tunnel")
        .collect();
    assert_eq!(
        tunnel_frames.len(),
        2,
        "outer + inner tunnel frames must both be live; got {:?}",
        snap.call_stack.iter().map(|f| f.kind).collect::<Vec<_>>()
    );

    let inner_locals = tunnel_frames
        .iter()
        .find_map(|f| {
            let locals = f.locals.as_ref()?;
            locals
                .iter()
                .any(|l| l.name == "n" && matches!(l.value, DebugValue::Int(2)))
                .then_some(locals)
        })
        .expect("no tunnel frame reports n == 2 (inner's own parameter)");
    assert!(
        matches!(local(inner_locals, "x").value, DebugValue::Int(99)),
        "inner frame's own `x` must read 99, not leak outer's binding"
    );

    let outer_locals_still = tunnel_frames
        .iter()
        .find_map(|f| {
            let locals = f.locals.as_ref()?;
            locals
                .iter()
                .any(|l| l.name == "n" && matches!(l.value, DebugValue::Int(1)))
                .then_some(locals)
        })
        .expect("no tunnel frame reports n == 1 (outer's own parameter) anymore");
    assert!(
        matches!(local(outer_locals_still, "x").value, DebugValue::Int(10)),
        "outer frame's `x` must still read 10 while inner is on the stack \
         — shadowing must not corrupt the enclosing frame's own binding"
    );
}

// ── #3395: a lift-order hoist temp reaches the snapshot flagged `synthetic` ──
//
// The wire row (`DebugLocalEntry::synthetic`, pinned in brink-ir's
// `issue_3185_locals_table.rs`) must survive resolution into the frame's
// `DebugLocal`s, and an authored temp beside it must NOT be flagged — the
// studio's locals views filter on exactly this bit.

#[test]
fn hoisted_lift_order_temp_is_reported_synthetic_beside_an_authored_one() {
    let src = "\
VAR n = 0\n\
-> k\n\
=== function bump() ===\n\
~ n = n + 1\n\
~ return n\n\
=== k ===\n\
~ temp mine = 4\n\
{bump()}{n == 1:yes|no}\n\
* [Continue]\n\
    -> END\n\
";
    let data = compiled_ink(src);
    let mut story = story_for(&data);
    run_to_choices(&mut story);

    let snap = story.debug_snapshot();
    let frame = snap
        .call_stack
        .iter()
        .find(|f| f.location.as_deref() == Some("k"))
        .unwrap_or_else(|| {
            let locations: Vec<Option<&str>> = snap
                .call_stack
                .iter()
                .map(|f| f.location.as_deref())
                .collect();
            panic!("k's frame must be on the call stack at the choice: {locations:?}")
        });
    let locals = frame
        .locals
        .as_ref()
        .expect("DebugInfo was requested, frame locals must be Some");

    let mine = local(locals, "mine");
    assert!(!mine.synthetic, "an authored `~ temp` is never synthetic");
    assert!(matches!(mine.value, DebugValue::Int(4)));

    let hoisted: Vec<&brink_runtime::DebugLocal> = locals
        .iter()
        .filter(|l| l.name.starts_with("$lift"))
        .collect();
    assert_eq!(
        hoisted.len(),
        1,
        "one hoisted prefix interpolation: {locals:?}"
    );
    assert!(
        hoisted[0].synthetic,
        "the hoisted temp must be flagged synthetic"
    );
}
