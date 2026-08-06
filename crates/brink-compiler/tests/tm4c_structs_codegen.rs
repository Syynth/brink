//! TM-4c (#666) end-to-end struct tests — construction/read/write through
//! COMPILED `.ink` source, via the public `brink_compiler::compile_with_options`
//! entry point (the concrete consumer path a CLI/library caller uses), then
//! executed through `brink_runtime::Story`. Mirrors `tm3_strict_policy.rs`'s
//! shape (dialect/types threaded through the same options struct) and
//! `driver.rs`'s compile-and-run helpers.
//!
//! Covers: construction (`RecordNew`, fields reordered into shape order),
//! field reads (`RecordGet`/`RecordGetDyn`), single-level field writes
//! (`RecordSet`/`RecordSetDyn`, take → `make_mut` → write-back), nested
//! structs in a collection, the strict/gradual static-offset matrix and its
//! required equivalence, the gradual construction-fault path, a
//! `SaveState` round-trip, and (issue #676) that construction-literal
//! initializers evaluate in **source** order end-to-end through real VM
//! execution, independent of the shape's declaration order.

#![allow(clippy::panic, clippy::unwrap_used)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_format::{Opcode, StoryData, Value};
use brink_runtime::{DotNetRng, RuntimeError, Story};

fn compile_mem(
    source: &str,
    dialect: Dialect,
    types: TypePolicy,
) -> Result<StoryData, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect,
        types: Some(types),
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
    .map(|output| output.data)
}

fn story_for(data: &StoryData) -> Story<DotNetRng> {
    let (program, line_tables) = brink_runtime::link(data).unwrap();
    Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables)
}

/// Drive `story` to a terminal line (`Done`/`End`), concatenating text.
/// Every fixture below is a single straight-line turn — no choices.
fn run_to_text(story: &mut Story<DotNetRng>) -> String {
    let lines = story.continue_maximally().unwrap();
    let mut out = String::new();
    for line in &lines {
        out.push_str(line.text());
    }
    out
}

/// Every opcode across every container's bytecode, decoded — used to prove
/// which record ops a compile actually emitted.
fn all_opcodes(data: &StoryData) -> Vec<Opcode> {
    let mut ops = Vec::new();
    for c in &data.containers {
        let mut offset = 0;
        while offset < c.bytecode.len() {
            match Opcode::decode(&c.bytecode, &mut offset) {
                Ok(op) => ops.push(op),
                Err(_) => break,
            }
        }
    }
    ops
}

const POINT_SRC: &str = "STRUCT Point = #{x: float, y: float}\n";

// NOTE: every fixture below declares a scalar placeholder default (`VAR p =
// 0`) and constructs/reassigns the real struct value in a statement
// afterward, rather than using a struct construction literal directly as the
// `VAR`'s declaration default. That was originally a required workaround —
// `eval_const_expr` (`brink-ir::lir::lower::decls`, the compile-time
// constant-folding path `VAR`/`CONST` defaults go through) silently compiled
// a struct/array/map literal default to `Value::Null` with no diagnostic.
// Fixed by #673 (a real, non-suppressible `E075` instead of a silent drop),
// then by #1530, which folds a well-formed construction literal into
// `lir::ConstValue::Record` so it *is* a legal declaration default — see
// `brink-ir/tests/lir_lowering/collection_struct_literal_declaration_
// defaults.rs`. The fixtures below keep the placeholder shape not because
// it's required, but because construct-then-mutate is what they exercise.

#[test]
fn construction_read_and_write_run_end_to_end() {
    let src = format!(
        "{POINT_SRC}VAR p = 0\n~ {{\n    p = Point#{{x: 1.0, y: 2.0}}\n    p.x = 9.0\n}}\n\
        {{p.x}} {{p.y}}\n-> DONE\n"
    );
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let text = run_to_text(&mut story);
    assert_eq!(text.trim(), "9 2");
}

#[test]
fn struct_shapes_table_is_populated_and_ordered_by_declaration() {
    // Since #1530, a well-formed construction literal IS a legal `VAR`
    // declaration default (it folds into `lir::ConstValue::Record`) —
    // `build_shape_table` populates every declared `STRUCT` unconditionally
    // from the declarations themselves (`hir_file.structs`), independent of
    // any construction site, so this test never needed one; `A` is
    // constructed in an ordinary `temp` instead, matching the established
    // "construct after declaration" pattern.
    let src = "STRUCT A = #{v: int}\nSTRUCT B = #{v: int}\nSTRUCT C = #{v: int}\n\
        ~ temp a = A#{v: 1}\nHello.\n-> DONE\n";
    let data = compile_mem(src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    assert_eq!(data.struct_shapes.len(), 3);
    let names: Vec<&str> = {
        let mut by_id: Vec<_> = data.struct_shapes.iter().collect();
        by_id.sort_by_key(|s| s.id.0);
        by_id
            .iter()
            .map(|s| data.name_table[s.name.0 as usize].as_str())
            .collect()
    };
    assert_eq!(names, ["A", "B", "C"], "shape ids follow declaration order");
}

#[test]
fn nested_struct_in_array_reads_back_correctly() {
    let src = format!(
        "{POINT_SRC}VAR pts = 0\n~ pts = #[Point#{{x: 1.0, y: 2.0}}, Point#{{x: 3.0, y: 4.0}}]\n\
        {{pts[1].x}}\n-> DONE\n"
    );
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let text = run_to_text(&mut story);
    assert_eq!(text.trim(), "3");
}

#[test]
fn cow_sharing_law_temp_copy_does_not_mutate_source() {
    let src = format!(
        "{POINT_SRC}VAR p = 0\n~ p = Point#{{x: 1.0, y: 2.0}}\n~ temp q = p\n~ q.x = 9.0\n\
        {{p.x}} {{q.x}}\n-> DONE\n"
    );
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let text = run_to_text(&mut story);
    assert_eq!(
        text.trim(),
        "1 9",
        "mutating q must never affect p (value semantics)"
    );
}

#[test]
fn strict_and_gradual_produce_equivalent_output_for_well_formed_program() {
    // The strict/gradual matrix's required equivalence (typed-mode-spec §6):
    // the *same* well-formed source compiles under both policies with
    // equivalent observable semantics — even though strict emits static
    // `RecordGet`/`RecordSet` offset ops here (the VAR is struct-typed) and
    // gradual emits the by-name `RecordGetDyn`/`RecordSetDyn` forms.
    //
    // Issue #1877 (review correction): `p`'s declaration default was
    // originally a bare `0` — a genuine, previously-uncaught `Point` vs
    // `int` mismatch against its own `: Point` annotation, incidental to
    // what this test verifies (strict/gradual codegen equivalence). Before
    // #1877, strict mode never checked a VAR's initializer against its own
    // annotation at all. Fixed to a well-typed `Point#{...}` default — the
    // very next statement overwrites it before any read, so the observable
    // behavior (and the `RecordGet`/`RecordSet` vs `RecordGetDyn`/
    // `RecordSetDyn` codegen split this test exists to prove) is unchanged.
    let src = format!(
        "{POINT_SRC}VAR p: Point = Point#{{x: 0.0, y: 0.0}}\n~ p = Point#{{x: 1.0, y: 2.0}}\n\
        ~ p.y = 5.0\n{{p.x}} {{p.y}}\n-> DONE\n"
    );
    let gradual = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let strict = compile_mem(&src, Dialect::Brink, TypePolicy::Strict).unwrap();

    let gradual_text = run_to_text(&mut story_for(&gradual));
    let strict_text = run_to_text(&mut story_for(&strict));
    assert_eq!(gradual_text, strict_text);
    assert_eq!(gradual_text.trim(), "1 5");

    // And the bytecode really did take different op forms — proving the
    // equivalence is meaningful (not just "both compiled the same way").
    let gradual_ops = all_opcodes(&gradual);
    let strict_ops = all_opcodes(&strict);
    assert!(
        gradual_ops
            .iter()
            .any(|op| matches!(op, Opcode::RecordGetDyn(_) | Opcode::RecordSetDyn(_))),
        "gradual should use by-name field ops"
    );
    assert!(
        !gradual_ops
            .iter()
            .any(|op| matches!(op, Opcode::RecordGet(_) | Opcode::RecordSet(_))),
        "gradual must never emit static-offset ops, even with an annotation"
    );
    assert!(
        strict_ops
            .iter()
            .any(|op| matches!(op, Opcode::RecordGet(_) | Opcode::RecordSet(_))),
        "strict + known shape should use static-offset field ops"
    );
}

#[test]
fn gradual_construction_field_mismatch_faults_at_runtime() {
    // Missing declared field `y` — under `types = gradual` this is a
    // construction fault at runtime (value-model-spec §11c), not a compile
    // error (compare `tm3_strict_policy.rs`'s style — under strict this
    // would already be E069, a compile error, checked separately below).
    let src = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0}}\nHello.\n-> DONE\n");
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let err = story.continue_maximally().unwrap_err();
    assert!(
        matches!(err, RuntimeError::InvalidShapeId(_)),
        "expected a turn-terminating construction fault, got {err:?}"
    );
}

#[test]
fn strict_construction_field_mismatch_is_a_compile_error() {
    let src = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0}}\nHello.\n-> DONE\n");
    let result = compile_mem(&src, Dialect::Brink, TypePolicy::Strict);
    assert!(
        result.is_err(),
        "missing field under strict must be a compile error (E069)"
    );
}

// ── TM-5 (#621) corpus wing growth: the extra-field half of the same rule ─
//
// Spec §6: "Missing/extra fields at construction: compile error (strict) /
// construction fault (gradual)." The tests above only exercise the
// *missing*-field half via `Point#{x: 1.0}`; these two round out the
// *extra*-field half (`z` isn't a declared `Point` field) end-to-end,
// mirroring the missing-field tests' shape exactly.

#[test]
fn gradual_construction_extra_field_faults_at_runtime() {
    let src = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0, y: 2.0, z: 3.0}}\nHello.\n-> DONE\n");
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let err = story.continue_maximally().unwrap_err();
    assert!(
        matches!(err, RuntimeError::InvalidShapeId(_)),
        "expected a turn-terminating construction fault, got {err:?}"
    );
}

#[test]
fn strict_construction_extra_field_is_a_compile_error() {
    let src = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0, y: 2.0, z: 3.0}}\nHello.\n-> DONE\n");
    let result = compile_mem(&src, Dialect::Brink, TypePolicy::Strict);
    assert!(
        result.is_err(),
        "extra field under strict must be a compile error (E070)"
    );
}

#[test]
fn save_state_round_trips_a_struct_valued_global() {
    let src = format!("{POINT_SRC}VAR p = 0\n~ p = Point#{{x: 1.0, y: 2.0}}\nHello.\n-> DONE\n");
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let _ = story.continue_maximally().unwrap();

    let save = story.save_state();
    let record = save
        .globals
        .get("p")
        .expect("the struct-valued global `p` should be in the save");
    assert!(matches!(record, Value::Record { .. }));

    // Through JSON, matching the wasm boundary's own round-trip contract.
    let json = serde_json::to_string(&save).unwrap();
    let restored: brink_format::SaveState = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, save);

    // And a fresh story reconciles it back correctly.
    let mut story2 = story_for(&data);
    let report = story2.load_state(&restored);
    assert!(report.unknown_globals.is_empty());
    assert_eq!(story2.variable("p"), Some(record));
}

// ── RMW equivalence property ─────────────────────────────────────────────
//
// `p.field = expr` (the take → `make_mut` → write-back sugar this PR adds)
// must be observably indistinguishable from manually performing the same
// read-modify-write by hand: read the old record, build a brand-new one
// with every field copied except the written one, then reassign it
// wholesale. Swept across several representative values (not just one) —
// the closest a proptest-free property test gets to the issue's requested
// fuzz coverage without a new `proptest` dev-dependency for this one crate.

fn rmw_sugar_output(new_y: f64) -> String {
    let src = format!(
        "{POINT_SRC}VAR p = 0\n~ p = Point#{{x: 1.0, y: 2.0}}\n~ p.y = {new_y}\n{{p.x}} {{p.y}}\n-> DONE\n"
    );
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    run_to_text(&mut story_for(&data))
}

fn rmw_manual_output(new_y: f64) -> String {
    // The hand-written reference: no field-assignment sugar at all — read
    // `p.x` out first, then reconstruct and reassign the whole record.
    let src = format!(
        "{POINT_SRC}VAR p = 0\n~ p = Point#{{x: 1.0, y: 2.0}}\n\
        ~ temp old_x = p.x\n~ p = Point#{{x: old_x, y: {new_y}}}\n{{p.x}} {{p.y}}\n-> DONE\n"
    );
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    run_to_text(&mut story_for(&data))
}

#[test]
fn single_level_field_write_is_observably_equivalent_to_manual_read_modify_write() {
    for new_y in [0.0, 1.0, -3.5, 42.0, 100.25] {
        let sugar = rmw_sugar_output(new_y);
        let manual = rmw_manual_output(new_y);
        assert_eq!(
            sugar, manual,
            "p.y = {new_y} must match the manual take/reconstruct/write-back reference"
        );
    }
}

// ── Source-order evaluation (issue #676) ──────────────────────────────
//
// Construction-literal initializers evaluate in the order the author wrote
// them, not the shape's declaration order — codegen reorders only the
// already-evaluated *values* into shape offsets afterward (decision-log
// "Struct construction literals: source-order evaluation, duplicate field
// is a compile error", 2026-07-14). `Point`'s shape declares `x` before
// `y`; the fixture below writes the literal with `y` first — each
// initializer a side-effecting function call appending to a shared log —
// proving through actual VM execution (not just LIR inspection) that `y`'s
// call fires before `x`'s.

#[test]
fn construction_literal_initializers_evaluate_in_source_order() {
    let src = format!(
        "VAR log = \"\"\n{POINT_SRC}\
        ~ temp p = Point#{{y: mark_y(), x: mark_x()}}\n\
        {{log}}\n-> DONE\n\n\
        === function mark_y() ===\n~ log = log + \"y\"\n~ return 2.0\n\n\
        === function mark_x() ===\n~ log = log + \"x\"\n~ return 1.0\n"
    );
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let text = run_to_text(&mut story);
    assert_eq!(
        text.trim(),
        "yx",
        "y is written first in source, so mark_y() must fire before mark_x() \
         even though x is declared first in Point's shape"
    );
}

#[test]
fn construction_literal_values_land_at_the_right_field_despite_source_reordering() {
    // Companion to the effect-order test above: the *values* placed into
    // the record must still land at their declared field regardless of
    // source order — decoupling evaluation order from placement must never
    // also scramble placement itself.
    let src = format!("{POINT_SRC}~ temp p = Point#{{y: 2.0, x: 1.0}}\n{{p.x}} {{p.y}}\n-> DONE\n");
    let data = compile_mem(&src, Dialect::Brink, TypePolicy::Gradual).unwrap();
    let mut story = story_for(&data);
    let text = run_to_text(&mut story);
    assert_eq!(text.trim(), "1 2");
}
