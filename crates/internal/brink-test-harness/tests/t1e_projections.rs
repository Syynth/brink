//! T1e-2 path-projection corpus wing (`docs/t1e-spec.md` §7, tracking #828).
//!
//! Unlike `tests/tier{1,2,3}/`, this corpus has no C# oracle — vanilla ink
//! has no path refs (`docs/t1e-spec.md` §7: "Oracle ratchet byte-identical").
//! Each test here compiles a small brink-dialect story through the real
//! pipeline (`brink_compiler::compile_with_options` → `brink_runtime::link`
//! → `Story`) and drives it to observe the ratified semantics directly:
//! overlap write-through, snapshot-at-creation, invalidation faults, save
//! mid-call with a live projection, and projection through `#fn`. A
//! property test at the bottom proves the RMW-equivalence law (spec §7):
//! `heal(ref npc.hp, k)` ≡ manually reading, adding `k`, and writing back.
//!
//! Every fixture declares `STRUCT`/`VAR` at true file scope (before any
//! `=== knot ===` header) with a scalar placeholder default (a struct
//! construction literal is not a legal `VAR` declaration default, `E075` —
//! the real value is assigned in a knot body instead), then enters its
//! entry knot explicitly via `choose_path_string` — the same shape
//! `tier1_brink.rs`'s save/load fn-value tests already use.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::LineEntry;
use brink_runtime::{DotNetRng, Line, RuntimeError, Story};
use proptest::prelude::*;

/// Compile `source` (brink dialect) and link it to a runnable program —
/// same shape `tier1_brink.rs::compile_and_link` uses.
fn compile_and_link(source: &str) -> (Arc<brink_runtime::Program>, Vec<Vec<LineEntry>>) {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let output = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        },
    )
    .expect("compile");
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    (Arc::new(program), tables)
}

/// Compile, link, jump straight into the `entry` knot (bypassing root
/// fallthrough entirely), and drain to the terminal line's text.
fn run_entry(source: &str, entry: &str) -> String {
    let (program, tables) = compile_and_link(source);
    let mut story = Story::<DotNetRng>::new(program, tables);
    story
        .choose_path_string(entry)
        .unwrap_or_else(|e| panic!("goto {entry}: {e:?}"));
    run_to_end(&mut story)
}

/// Drain a story to its terminal line, concatenating text.
fn run_to_end(story: &mut Story<DotNetRng>) -> String {
    let mut out = String::new();
    loop {
        match story.continue_single().expect("runtime error") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Choices { text, .. } => {
                out.push_str(&text);
                break;
            }
        }
    }
    out
}

/// Compile, link, jump into `entry`, and drive one line at a time until a
/// `RuntimeError` surfaces (the turn-terminating-fault contract) — returns
/// the error.
fn run_entry_until_fault(source: &str, entry: &str) -> RuntimeError {
    let (program, tables) = compile_and_link(source);
    let mut story = Story::<DotNetRng>::new(program, tables);
    story
        .choose_path_string(entry)
        .unwrap_or_else(|e| panic!("goto {entry}: {e:?}"));
    loop {
        match story.continue_single() {
            Ok(Line::Text { .. }) => {}
            Ok(Line::Done { .. } | Line::End { .. } | Line::Choices { .. }) => {
                panic!("expected a ProjectionInvalidated fault, but the story ran to completion")
            }
            Err(e) => return e,
        }
    }
}

const NPC_STRUCT: &str = "STRUCT NPC = #{hp: int, name: string}\nVAR npc = 0\n\n";
const HEAL: &str = "\n=== function heal(ref hp, k) ===\n~ hp = hp + k\n";

// ── Overlap write-through (spec §1(3)) ─────────────────────────────────

#[test]
fn overlapping_projections_write_through_immediately() {
    // Two separate `ref npc.hp` projections into the same root cell: a
    // write through the first is visible to a read through the second —
    // "every write applies to the root cell at the moment it happens"
    // (spec §1(3)), deterministic without any aliasing check.
    let src = format!(
        "{NPC_STRUCT}=== main ===\n\
         ~ npc = NPC#{{hp: 10, name: \"x\"}}\n\
         ~ heal(ref npc.hp, 5)\n~ heal(ref npc.hp, 7)\n{{npc.hp}}\n-> END\n{HEAL}"
    );
    let out = run_entry(&src, "main");
    assert_eq!(out, "22\n");
}

// ── Snapshot-at-creation (spec §1(1)) ──────────────────────────────────

#[test]
fn index_snapshot_at_creation_ignores_later_mutation_of_the_index_var() {
    // `ref inventory[idx]` captures the *value* of `idx` at `ref` creation
    // (spec §1(1)) — mutating `idx` afterward must not retarget an
    // already-created projection. T1e permits `ref` only in argument
    // position (standalone is E097), so the snapshot is proved at a real
    // call site: `bump`'s `ref x` param binds `ref inventory[idx]` with
    // `idx == 0` *at that call*; `idx` is reassigned to `2` only after.
    let src = "VAR inventory = 0\n\n\
               === main ===\n\
               ~ inventory = #[10, 20, 30]\n\
               ~ temp idx = 0\n\
               ~ bump(ref inventory[idx], 100)\n\
               ~ idx = 2\n\
               {inventory[0]} {inventory[1]} {inventory[2]}\n-> END\n\n\
               === function bump(ref x, k) ===\n~ x = x + k\n";
    let out = run_entry(src, "main");
    // Only `inventory[0]` moves — the snapshot fixed the index at 0, the
    // later `idx = 2` reassignment has no effect on the already-live ref.
    assert_eq!(out, "110 20 30\n");
}

// ── Invalidation faults (spec §1(2)) ───────────────────────────────────

#[test]
fn shrunk_array_projection_faults_projection_invalidated_on_write() {
    // Shrink the array via reassignment *between* creating the projection
    // (the call-argument evaluation) and writing through it (the callee's
    // own `x = x + k`) — the write walks against the array's *current*
    // (now-shrunk) value, per spec §4: "reads walk the snapshot path
    // against the root's current value".
    let src = "VAR items = 0\n\n\
               === main ===\n\
               ~ items = #[1, 2, 3]\n\
               ~ temp idx = 2\n\
               ~ shrink_then_bump(ref items[idx], 1)\n\
               Unreachable.\n-> END\n\n\
               === function shrink_then_bump(ref x, k) ===\n\
               ~ items = #[1, 2]\n~ x = x + k\n";
    let err = run_entry_until_fault(src, "main");
    assert!(
        matches!(err, RuntimeError::ProjectionInvalidated(_)),
        "expected ProjectionInvalidated, got {err:?}"
    );
}

#[test]
fn removed_struct_field_projection_faults_on_write() {
    // A field genuinely absent from the shape — `heal`'s `ref hp` binds
    // `ref npc.mana`, which `NPC` never declares (gradual mode: not
    // statically checked, so it reaches the runtime fault, spec §4/§6
    // "Unknown never disagrees" for gradual).
    let src = format!(
        "{NPC_STRUCT}=== main ===\n\
         ~ npc = NPC#{{hp: 10, name: \"x\"}}\n\
         ~ heal(ref npc.mana, 5)\nUnreachable.\n-> END\n{HEAL}"
    );
    let err = run_entry_until_fault(&src, "main");
    assert!(
        matches!(err, RuntimeError::ProjectionInvalidated(_)),
        "expected ProjectionInvalidated, got {err:?}"
    );
}

// ── Save/load mid-call with a live projection (spec §3) ────────────────

#[test]
fn save_load_mid_call_with_a_live_projection_reconciles_cleanly() {
    // A projection saved mid-call serializes like `VariablePointer` (spec
    // §3) — the root cell rehydrates via the ordinary global-name lookup,
    // no special-cased persistence path. Prove it end-to-end: save the
    // globals right after `setup` (before `heal` ever runs), reload into a
    // fresh story, then invoke — same outcome as running straight through.
    let src = format!(
        "{NPC_STRUCT}=== setup ===\n~ npc = NPC#{{hp: 1, name: \"x\"}}\nSet up.\n-> DONE\n\
         === invoke ===\n~ heal(ref npc.hp, 41)\n{{npc.hp}}\n-> END\n{HEAL}"
    );
    let (program, tables) = compile_and_link(&src);

    let mut direct = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
    direct.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut direct);
    direct.choose_path_string("invoke").expect("goto invoke");
    let direct_out = run_to_end(&mut direct);

    let mut src_story = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
    src_story.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut src_story);
    let saved = src_story.save_state();

    let mut loaded = Story::<DotNetRng>::new(program, tables);
    let report = loaded.load_state(&saved);
    assert!(
        report.is_clean(),
        "save round-trip should reconcile cleanly: {report:?}"
    );
    loaded.choose_path_string("invoke").expect("goto invoke");
    let loaded_out = run_to_end(&mut loaded);

    assert_eq!(
        direct_out, loaded_out,
        "save→load→invoke must equal direct invoke"
    );
    assert_eq!(loaded_out, "42\n");
}

// ── Projection through `#fn` (spec §2) ─────────────────────────────────

#[test]
fn projection_through_fn_value_ref_binding() {
    // `#fn(heal, ref npc.hp)` binds a real path projection into a
    // function value's env — the projection crosses the `#fn`/`Closure`
    // boundary exactly like a `VariablePointer` would (spec §2's grammar:
    // "`#fn(heal, ref party[leader].hp)`").
    let src = format!(
        "{NPC_STRUCT}=== main ===\n\
         ~ npc = NPC#{{hp: 1, name: \"x\"}}\n\
         ~ temp f = #fn(heal, ref npc.hp)\n\
         ~ temp r = f(9)\n\
         {{npc.hp}}\n-> END\n{HEAL}"
    );
    let out = run_entry(&src, "main");
    assert_eq!(out, "10\n");
}

// ── RMW-equivalence property (spec §7: "extends the lane-B law") ───────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// `heal(ref npc.hp, k)` (root-cell RMW through a projection) must
    /// produce exactly the value manual take/mutate/write-back
    /// (`npc.hp = npc.hp + k`, an ordinary field assignment) would — the
    /// spec §7 law: "RMW-through-projection ≡ manual take/mutate/write-back".
    #[test]
    fn projection_write_equals_manual_take_mutate_write_back(
        initial in -1000i32..1000,
        k in -1000i32..1000,
    ) {
        let via_projection = format!(
            "{NPC_STRUCT}=== main ===\n\
             ~ npc = NPC#{{hp: {initial}, name: \"x\"}}\n\
             ~ heal(ref npc.hp, {k})\n{{npc.hp}}\n-> END\n{HEAL}"
        );
        let via_manual = format!(
            "{NPC_STRUCT}=== main ===\n\
             ~ npc = NPC#{{hp: {initial}, name: \"x\"}}\n\
             ~ npc.hp = npc.hp + {k}\n{{npc.hp}}\n-> END\n"
        );

        let out1 = run_entry(&via_projection, "main");
        let out2 = run_entry(&via_manual, "main");

        prop_assert_eq!(out1, out2);
    }
}
