//! Issue #3185 (D7, `docs/debugger-spec.md` §3): codegen populates the
//! `DebugInfo` `LocalsTable` D6 shipped as structural framing only (empty
//! `locals` per container) — one row per declared parameter and per
//! top-level `~ temp`/`let`, slot->name (+ declaring range for `~ temp`,
//! `None` for a parameter — no per-param source range exists in LIR).
//!
//! Mirrors `issue_3184_debug_info.rs`'s fixture-lowering helpers and dual-
//! frontend shape (verbatim copy — this crate's tests do not share a
//! non-`.test.` support module for it, matching that file's own precedent).
//! Runtime-level proof (names bound to live *values* on the debug snapshot,
//! across both surfaces, covering every value kind the issue names) lives
//! in `crates/brink-runtime/tests/issue_3185_locals_debug_snapshot.rs` —
//! this file pins the wire-level table codegen actually emits, independent
//! of how the runtime later resolves it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(
    clippy::disallowed_types,
    reason = "single-entry file_paths map, read only by key — see issue_3184_debug_info.rs's identical exemption"
)]

use brink_codegen_inkb::EmitOptions;
use brink_format::StoryData;
use brink_ir::{FileId, HirFile, SymbolManifest, lir};

fn lower_ink(source: &str, path: &str) -> lir::Program {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, diags) = brink_ir::hir::lower(file_id, &tree);
    assert!(
        diags.is_empty(),
        "unexpected ink lowering diagnostics: {diags:?}"
    );
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let file_paths = std::collections::HashMap::from([(file_id, path.to_string())]);
    let (program, diags) = lir::lower_to_program(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &file_paths,
    );
    assert!(diags.is_empty(), "unexpected LIR diagnostics: {diags:?}");
    program.expect("plain ink source always lowers to a program")
}

fn lower_native(source: &str, path: &str) -> lir::Program {
    let parsed = brink_syntax_native::parse(source);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let file_id = FileId(0);
    let (hir, manifest, diags) = brink_ir::hir::lower_native::lower(file_id, &parsed.tree());
    assert!(
        diags.is_empty(),
        "unexpected native lowering diagnostics: {diags:?}"
    );

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let analysis_opts = brink_analyzer::AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        ..Default::default()
    };
    let analysis = brink_analyzer::analyze_with_options(&files_for_analysis, &analysis_opts);
    assert!(
        analysis.diagnostics.is_empty(),
        "unexpected analysis diagnostics: {:?}",
        analysis.diagnostics
    );

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let file_paths = std::collections::HashMap::from([(file_id, path.to_string())]);
    let (program, diags) = lir::lower_to_program(
        &files_for_lir,
        &analysis.index,
        &analysis.resolutions,
        &file_paths,
    );
    assert!(diags.is_empty(), "unexpected LIR diagnostics: {diags:?}");
    program.expect("well-formed native source always lowers to a program")
}

fn emit_with_debug_info(program: &lir::Program) -> StoryData {
    brink_codegen_inkb::emit_with_options(
        program,
        EmitOptions {
            emit_debug_info: true,
            debug_sources: None,
        },
    )
    .expect("emit_with_options(debug_info: true) succeeds")
}

/// Index of the container named `name` (a scope container — root/knot/
/// stitch — the only kind with `ContainerDef::name` set) in `data.containers`.
fn container_idx_named(data: &StoryData, name: &str) -> usize {
    data.containers
        .iter()
        .position(|c| {
            c.name
                .and_then(|n| data.name_table.get(n.0 as usize))
                .is_some_and(|n| n == name)
        })
        .unwrap_or_else(|| panic!("no container named {name:?} in {:?}", data.containers))
}

fn local_named<'a>(
    entries: &'a [brink_format::DebugLocalEntry],
    name: &str,
) -> &'a brink_format::DebugLocalEntry {
    entries
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no local named {name:?} in {entries:?}"))
}

// ─── 1. Parameter: slot->name, no declaring range ───────────────────────────

#[test]
fn parameter_gets_a_locals_entry_with_no_declaring_range_ink() {
    let program = lower_ink(
        "-> calc(3)\n=== calc(n) ===\nGot {n}.\n-> END\n",
        "story.ink",
    );
    let data = emit_with_debug_info(&program);
    let idx = container_idx_named(&data, "calc");
    let table = &data
        .debug_info
        .as_ref()
        .expect("debug info requested")
        .containers[idx];
    let n = local_named(&table.locals, "n");
    assert_eq!(n.slot, 0, "calc's sole parameter is slot 0");
    assert_eq!(
        n.declaring_range, None,
        "a parameter has no per-param source range in LIR (Param carries no Provenance)"
    );
}

#[test]
fn parameter_gets_a_locals_entry_with_no_declaring_range_native() {
    let program = lower_native(
        "flow main() {\n  -> calc(3)\n}\nflow calc(n: int) {\n  Got {n}.\n  -> END\n}\n",
        "story.brink",
    );
    let data = emit_with_debug_info(&program);
    let idx = container_idx_named(&data, "calc");
    let table = &data
        .debug_info
        .as_ref()
        .expect("debug info requested")
        .containers[idx];
    let n = local_named(&table.locals, "n");
    assert_eq!(n.slot, 0);
    assert_eq!(n.declaring_range, None);
}

// ─── 2. `~ temp`/`let`: slot->name WITH a declaring range that actually ────
//    contains the declaration's own source text — ground truth #2, mirroring
//    #3213/#3184's "a recorded range must CONTAIN the text it is stamped
//    on" invariant, not read back against the table's own output.

#[test]
fn declared_temp_gets_a_locals_entry_whose_range_contains_its_own_text_ink() {
    let src = "-> calc\n=== calc ===\n~ temp doubled = 4\nValue: {doubled}\n-> END\n";
    let program = lower_ink(src, "story.ink");
    let data = emit_with_debug_info(&program);
    let idx = container_idx_named(&data, "calc");
    let table = &data
        .debug_info
        .as_ref()
        .expect("debug info requested")
        .containers[idx];
    let doubled = local_named(&table.locals, "doubled");
    let (file_idx, range_start, range_len) = doubled
        .declaring_range
        .expect("a `~ temp` declaration has a real declaring Provenance");
    assert_eq!(
        file_idx, 1,
        "the only real file referenced is section-local index 1 (index 0 is the synthetic sentinel)"
    );
    let start = range_start as usize;
    let end = start + range_len as usize;
    let text = &src[start..end];
    assert!(
        text.contains("temp doubled = 4"),
        "declaring range {start}..{end} = {text:?} must contain the declaration text"
    );
}

#[test]
fn declared_temp_gets_a_locals_entry_whose_range_contains_its_own_text_native() {
    let src = "flow calc() {\n  ~ let doubled = 4\n  Value: {doubled}\n  -> END\n}\n";
    let program = lower_native(src, "story.brink");
    let data = emit_with_debug_info(&program);
    let idx = container_idx_named(&data, "calc");
    let table = &data
        .debug_info
        .as_ref()
        .expect("debug info requested")
        .containers[idx];
    let doubled = local_named(&table.locals, "doubled");
    let (file_idx, range_start, range_len) = doubled
        .declaring_range
        .expect("a `~ let` declaration has a real declaring Provenance");
    assert_eq!(file_idx, 1);
    let start = range_start as usize;
    let end = start + range_len as usize;
    let text = &src[start..end];
    assert!(
        text.contains("let doubled = 4"),
        "declaring range {start}..{end} = {text:?} must contain the declaration text"
    );
}

// ─── 3. A container with neither params nor temps still gets a present, ───
//    empty `locals` (not a missing/absent row) — the coverage guarantee
//    `DebugContainerTable` already gives entries (§2.4) applies the same
//    way to `locals`: every container has a row, some are just empty.

#[test]
fn container_with_no_params_or_temps_gets_an_empty_locals_row() {
    let program = lower_ink("-> plain\n=== plain ===\nHello.\n-> END\n", "story.ink");
    let data = emit_with_debug_info(&program);
    let idx = container_idx_named(&data, "plain");
    let table = &data
        .debug_info
        .as_ref()
        .expect("debug info requested")
        .containers[idx];
    assert!(
        table.locals.is_empty(),
        "expected no locals, got {:?}",
        table.locals
    );
}
