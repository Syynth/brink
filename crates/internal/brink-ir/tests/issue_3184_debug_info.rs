//! Issue #3184 (D6, `docs/debugger-spec.md` §2): `brink-codegen-inkb` emits
//! the `DebugInfo` section (`.inkb` tag `0x11`) from `Container`/`Stmt`
//! provenance (delivered by #3183/D5) when `EmitOptions::emit_debug_info` is
//! set.
//!
//! Proof shape, per the issue's own acceptance bar:
//!
//! 1. **Byte-identical when off** — the single most important test: a
//!    program emitted with the debug flag off must produce byte-identical
//!    `.inkb` output to calling the pre-existing, untouched `emit()` entry
//!    point.
//! 2. **Correctness of the map itself** — anchored against the `lir::
//!    Program`'s own `Stmt.provenance` (ground truth this test reads
//!    directly, independent of `debug_info`) and the raw source text
//!    (ground truth #2, mirroring #3213's "a recorded range must CONTAIN
//!    the text it is stamped on" invariant), not against the table's own
//!    output — a self-consistent wrong table must fail this.
//! 3. **Both surfaces** — `.ink` and `.brink`, mirroring #3183's own
//!    dual-frontend proof shape (different `ProvenanceResolver`/`raw`
//!    numbering per frontend, so a round-trip proof on one surface alone
//!    would not cover the other).
//! 4. **Structural invariants** that hold regardless of construct shape:
//!    every entry sets `IS_STMT`; exactly one entry per container sets
//!    `PROLOGUE_END`; entries are sorted ascending by `bytecode_offset`;
//!    `file_idx` 0 is always the reserved synthetic sentinel.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. The
// two uses here (`file_paths` handed to `lir::lower_to_program`) are
// single-entry maps read only by key, mirroring the same exemption
// `issue_3183_lir_provenance.rs` takes for the identical pattern.
#![allow(
    clippy::disallowed_types,
    reason = "single-entry file_paths map, read only by key — see file doc"
)]

use brink_codegen_inkb::EmitOptions;
use brink_format::{DEBUG_FLAG_IS_STMT, DEBUG_FLAG_PROLOGUE_END, FileSurface, StoryData};
use brink_ir::{FileId, HirFile, SymbolManifest, lir};

// ─── Fixture lowering (mirrors issue_3183_lir_provenance.rs's helpers) ──────

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

// ─── 1. Byte-identical when off — THE most important test ──────────────────

#[test]
fn byte_identical_when_debug_info_off_ink() {
    let program = lower_ink("VAR x = 0\n~ x = 5\n-> END\n", "story.ink");

    let via_emit = brink_codegen_inkb::emit(&program).expect("emit succeeds");
    let via_default_options =
        brink_codegen_inkb::emit_with_options(&program, EmitOptions::default())
            .expect("emit_with_options(default) succeeds");
    let via_explicit_off = brink_codegen_inkb::emit_with_options(
        &program,
        EmitOptions {
            emit_debug_info: false,
        },
    )
    .expect("emit_with_options(off) succeeds");

    assert!(via_emit.debug_info.is_none());
    assert!(via_default_options.debug_info.is_none());
    assert!(via_explicit_off.debug_info.is_none());
    assert_eq!(
        via_emit, via_default_options,
        "emit() must reproduce emit_with_options(default) field-for-field"
    );
    assert_eq!(
        via_emit, via_explicit_off,
        "emit() must reproduce emit_with_options(emit_debug_info: false) field-for-field"
    );

    let mut buf_a = Vec::new();
    brink_format::write_inkb(&via_emit, &mut buf_a);
    let mut buf_b = Vec::new();
    brink_format::write_inkb(&via_default_options, &mut buf_b);
    let mut buf_c = Vec::new();
    brink_format::write_inkb(&via_explicit_off, &mut buf_c);
    assert_eq!(
        buf_a, buf_b,
        ".inkb bytes must be byte-identical (default options)"
    );
    assert_eq!(
        buf_a, buf_c,
        ".inkb bytes must be byte-identical (explicit off)"
    );

    let index = brink_format::read_inkb_index(&buf_a).unwrap();
    assert!(
        !index
            .sections
            .iter()
            .any(|s| s.kind == brink_format::SectionKind::DebugInfo),
        "no DebugInfo section byte enters the offset table when off"
    );
}

#[test]
fn byte_identical_when_debug_info_off_native() {
    let program = lower_native(
        "flow main() {\n  ~ let x = 0\n  ~ x = 5\n  -> END\n}\n",
        "scene.brink",
    );

    let via_emit = brink_codegen_inkb::emit(&program).expect("emit succeeds");
    let via_options = brink_codegen_inkb::emit_with_options(&program, EmitOptions::default())
        .expect("emit_with_options(default) succeeds");
    assert_eq!(via_emit, via_options);

    let mut buf_a = Vec::new();
    brink_format::write_inkb(&via_emit, &mut buf_a);
    let mut buf_b = Vec::new();
    brink_format::write_inkb(&via_options, &mut buf_b);
    assert_eq!(buf_a, buf_b);
}

// ─── 2 + 3. Correctness of the map, anchored against LIR provenance, both
//            surfaces ───────────────────────────────────────────────────────

/// Find the `DebugEntry` (if any, across every container) whose `file_idx`,
/// `range_start`/`range_len`, and `kind_token` match `provenance` exactly —
/// ground truth read from the `lir::Program` directly, never from
/// `debug_info`'s own output. `files` is `debug_info.files` (for resolving
/// `file_idx` back to a path/surface) and `expect_path`/`expect_surface`
/// are asserted on the *match*, not assumed.
fn find_matching_entry(
    story: &StoryData,
    provenance: brink_ir::Provenance,
) -> Option<(usize, usize, &brink_format::DebugEntry)> {
    let debug_info = story.debug_info.as_ref()?;
    let want_range_start = u32::from(provenance.range.start());
    let want_range_len = u32::from(provenance.range.len());
    let want_kind_token = provenance.kind.as_u32();
    for (ci, table) in debug_info.containers.iter().enumerate() {
        for (ei, entry) in table.entries.iter().enumerate() {
            if entry.range_start == want_range_start
                && entry.range_len == want_range_len
                && entry.kind_token == want_kind_token
            {
                return Some((ci, ei, entry));
            }
        }
    }
    None
}

#[test]
fn debug_info_entry_resolves_to_the_correct_source_text_ink() {
    let src = "VAR x = 0\n~ x = 5\n-> END\n";
    let program = lower_ink(src, "story.ink");
    let assign = program
        .root
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        .expect("root body should contain the `~ x = 5` assignment")
        .clone();

    let story = brink_codegen_inkb::emit_with_options(
        &program,
        EmitOptions {
            emit_debug_info: true,
        },
    )
    .expect("emit succeeds");

    let (_, _, entry) = find_matching_entry(&story, assign.provenance)
        .expect("an entry matching the Assign stmt's own provenance must exist");

    assert_eq!(entry.flags & DEBUG_FLAG_IS_STMT, DEBUG_FLAG_IS_STMT);

    let files = &story.debug_info.as_ref().unwrap().files;
    let file = &files[entry.file_idx as usize];
    assert_eq!(file.surface, FileSurface::Ink);
    assert_eq!(file.path, "story.ink");

    // Ground truth #2: independently slice the RAW SOURCE at the entry's own
    // range (never at the provenance's — a self-consistent wrong table that
    // copied provenance verbatim without codegen ever deriving it correctly
    // would still pass the provenance match above but must still slice back
    // to real text here).
    let start = entry.range_start as usize;
    let end = start + entry.range_len as usize;
    let text = &src[start..end];
    assert_eq!(
        text, "x = 5",
        "the DebugInfo entry's range must slice back to exactly the \
         assignment it maps to (#3213's containment invariant, applied here)"
    );
}

#[test]
fn debug_info_entry_resolves_to_the_correct_source_text_native() {
    let src = "flow main() {\n  ~ let x = 0\n  ~ x = 5\n  -> END\n}\n";
    let program = lower_native(src, "scene.brink");
    let main = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("a `main` flow container");
    let assign = main
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::Assign { .. }))
        .expect("main body should contain the `~ x = 5` assignment")
        .clone();

    let story = brink_codegen_inkb::emit_with_options(
        &program,
        EmitOptions {
            emit_debug_info: true,
        },
    )
    .expect("emit succeeds");

    let (_, _, entry) = find_matching_entry(&story, assign.provenance)
        .expect("an entry matching the Assign stmt's own provenance must exist");

    let files = &story.debug_info.as_ref().unwrap().files;
    let file = &files[entry.file_idx as usize];
    assert_eq!(file.surface, FileSurface::Native);
    assert_eq!(file.path, "scene.brink");

    let start = entry.range_start as usize;
    let end = start + entry.range_len as usize;
    let text = &src[start..end];
    assert_eq!(text, "x = 5");
}

// ─── 4. Structural invariants (construct-shape-independent) ────────────────

fn assert_structural_invariants(story: &StoryData) {
    let debug_info = story.debug_info.as_ref().expect("debug info requested");
    assert_eq!(
        debug_info.files.first().map(|f| f.surface),
        Some(FileSurface::Synthetic),
        "file_idx 0 is always the reserved synthetic sentinel"
    );
    assert_eq!(debug_info.files[0].path, "");
    assert_eq!(
        debug_info.containers.len(),
        story.containers.len(),
        "one DebugInfo table per container, lockstep with Containers"
    );
    for (ci, table) in debug_info.containers.iter().enumerate() {
        assert!(
            !table.entries.is_empty(),
            "container {ci} has zero entries — violates the coverage guarantee"
        );
        let mut prev = 0u32;
        let mut prologue_end_count = 0;
        for (ei, entry) in table.entries.iter().enumerate() {
            assert!(
                entry.bytecode_offset >= prev,
                "container {ci} entry {ei}: offsets must be sorted ascending"
            );
            prev = entry.bytecode_offset;
            assert_eq!(
                entry.flags & DEBUG_FLAG_IS_STMT,
                DEBUG_FLAG_IS_STMT,
                "container {ci} entry {ei}: v1 sets IS_STMT on every entry"
            );
            if entry.flags & DEBUG_FLAG_PROLOGUE_END != 0 {
                prologue_end_count += 1;
            }
        }
        assert_eq!(
            prologue_end_count, 1,
            "container {ci}: exactly one entry must carry PROLOGUE_END"
        );
    }
}

#[test]
fn debug_info_structural_invariants_hold_ink() {
    let program = lower_ink(
        "VAR x = 0\n\
         == function addup(a, b) ==\n\
         ~ return a + b\n\
         -> END\n",
        "story.ink",
    );
    let story = brink_codegen_inkb::emit_with_options(
        &program,
        EmitOptions {
            emit_debug_info: true,
        },
    )
    .expect("emit succeeds");
    assert_structural_invariants(&story);

    // At least one container (the parameterized function) must have a
    // params-prologue entry: offset 0, `Container.provenance` (not any
    // `Stmt`'s), not itself the PROLOGUE_END landing point.
    let debug_info = story.debug_info.as_ref().unwrap();
    let has_param_prologue_entry = debug_info.containers.iter().any(|t| {
        t.entries
            .first()
            .is_some_and(|e| e.bytecode_offset == 0 && e.flags & DEBUG_FLAG_PROLOGUE_END == 0)
            && t.entries.len() > 1
    });
    assert!(
        has_param_prologue_entry,
        "expected the parameterized `add` function's container to carry a \
         leading non-PROLOGUE_END entry at offset 0 for its DeclareTemp \
         parameter binding"
    );
}

#[test]
fn debug_info_structural_invariants_hold_native() {
    let program = lower_native(
        "flow main() {\n  ~ let x = 0\n  ~ x = 5\n  -> END\n}\n",
        "scene.brink",
    );
    let story = brink_codegen_inkb::emit_with_options(
        &program,
        EmitOptions {
            emit_debug_info: true,
        },
    )
    .expect("emit succeeds");
    assert_structural_invariants(&story);
}

// ─── .inkt dump parity ──────────────────────────────────────────────────────

#[test]
fn debug_info_survives_inkt_round_trip() {
    let program = lower_ink("VAR x = 0\n~ x = 5\n-> END\n", "story.ink");
    let story = brink_codegen_inkb::emit_with_options(
        &program,
        EmitOptions {
            emit_debug_info: true,
        },
    )
    .expect("emit succeeds");
    assert!(story.debug_info.is_some());

    let mut text = String::new();
    brink_format::write_inkt(&story, &mut text).expect("write_inkt succeeds");
    assert!(
        text.contains("(debug_info"),
        ".inkt dump must render the DebugInfo section, got:\n{text}"
    );

    let recovered = brink_format::read_inkt(&text).expect("read_inkt succeeds");
    assert_eq!(story.debug_info, recovered.debug_info);
}
