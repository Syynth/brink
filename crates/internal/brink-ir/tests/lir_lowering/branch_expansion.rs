use crate::support::*;
use brink_ir::lir;

// ─── Branch expansion for inline conditionals/sequences ──────────────
//
// The 2026-03-15 decision-log ruling (issue #1667): inline conditionals/
// sequences in content lines expand at compile time into the cartesian
// product of complete lines, each independently recognized as Plain/
// Template (or falling back to EmitContent) and getting its own LineEntry
// — the runtime selects a line rather than assembling one from parts.
//
// `hir::normalize_file` (added the same day as the ruling, commit
// 4e1cf774b) already does the *structural* half: it lifts an inline
// conditional/sequence out of its `Stmt::Content` and splices the
// surrounding prefix/suffix text into each branch, producing one
// `Stmt::Conditional`/`Stmt::Sequence` with the branch bodies living in
// their own child containers (each independently reaching
// `lir::lower::stmts`'s `hir::Stmt::Content` recognition arm). What it
// never did was *merge* the spliced prefix/branch/suffix `ContentPart`s —
// they stayed three separate adjacent `Text` entries, so
// `recognize::try_recognize`'s Phase 1 (`parts.len() == 1`) could never
// match and every branch fell back to `EmitContent`, which still emits one
// line-table entry *per fragment*. That's the actual gap this issue closes
// (`normalize.rs`'s new `extend_merging_text`) — not the absence of branch
// lifting itself.

/// Find the `n`th `ConditionalBranch`/`SequenceBranch` child container
/// under `root`, in encounter order (matches source branch order — see
/// `container.rs::lower_choice_with_child`/the `Stmt::Conditional`/
/// `Stmt::Sequence` lowering in `mod.rs`, which push branch containers in
/// `cond.branches`/`seq.branches` iteration order).
fn branch_bodies(program: &lir::Program) -> Vec<&[lir::Stmt]> {
    // A conditional's branch containers are direct children of the
    // enclosing scope, but a sequence's live one level deeper — under its
    // own `ContainerKind::Sequence` wrapper container (the addressable
    // container that owns the shared `CurrentVisitCount` state) — so this
    // walks the whole tree rather than assuming a fixed depth.
    fn walk<'a>(c: &'a lir::Container, out: &mut Vec<&'a [lir::Stmt]>) {
        if matches!(
            c.kind,
            lir::ContainerKind::ConditionalBranch | lir::ContainerKind::SequenceBranch
        ) {
            out.push(c.body.as_slice());
        }
        for child in &c.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(root(program), &mut out);
    out
}

fn plain_text(stmts: &[lir::Stmt]) -> Option<&str> {
    stmts.iter().find_map(|s| {
        if let lir::Stmt::EmitLine(e) = s
            && let lir::RecognizedLine::Plain(s) = &e.line
        {
            return Some(s.as_str());
        }
        None
    })
}

fn has_emit_content(stmts: &[lir::Stmt]) -> bool {
    stmts.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)))
}

#[test]
fn if_else_conditional_branches_recognize_as_independent_plain_lines() {
    let program = lower_ink("VAR x = true\nIt was {x: sunny|rainy} today.\n");
    let bodies = branch_bodies(&program);
    assert_eq!(
        bodies.len(),
        2,
        "if/else conditional should have 2 branches"
    );
    let texts: Vec<&str> = bodies.iter().map(|b| plain_text(b).unwrap_or_else(|| {
        panic!("branch should recognize as a single Plain line, not EmitContent (has_emit_content={})", has_emit_content(b))
    })).collect();
    assert_eq!(texts, vec!["It was sunny today.", "It was rainy today."]);
}

#[test]
fn conditional_without_else_synthesizes_a_plain_empty_case() {
    // No else arm: the ruling still requires a complete cartesian product —
    // "condition false" is itself a distinct combination (contributes
    // nothing), not a line that silently doesn't exist.
    let program = lower_ink("VAR x = false\nA{x: B} C\n");
    let bodies = branch_bodies(&program);
    assert_eq!(
        bodies.len(),
        2,
        "no-else conditional should still get a synthesized else branch"
    );
    let texts: Vec<&str> = bodies
        .iter()
        .map(|b| {
            plain_text(b)
                .expect("both branches (including synthesized else) should recognize as Plain")
        })
        .collect();
    assert_eq!(texts, vec!["A B C", "A C"]);
}

#[test]
fn sequence_branches_recognize_as_independent_plain_lines() {
    let program = lower_ink("Roll {&one|two|three}!\n");
    let bodies = branch_bodies(&program);
    assert_eq!(bodies.len(), 3, "cycle sequence should have 3 branches");
    let texts: Vec<&str> = bodies
        .iter()
        .map(|b| plain_text(b).expect("sequence branch should recognize as Plain"))
        .collect();
    assert_eq!(texts, vec!["Roll one!", "Roll two!", "Roll three!"]);
}

#[test]
fn branch_with_interpolation_becomes_a_template_not_emit_content() {
    let program = lower_ink("VAR x = true\nVAR name = \"Ren\"\nHi {x: {name}|there}.\n");
    let bodies = branch_bodies(&program);
    assert_eq!(bodies.len(), 2);
    let true_body = bodies[0];
    let has_template = true_body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "an interpolation-bearing branch should recognize as Template, not fall back to EmitContent"
    );
    assert!(
        !has_emit_content(true_body),
        "should not have fallen back to EmitContent"
    );
}

#[test]
fn each_branch_gets_a_distinct_source_hash() {
    // Each combination is a distinct translatable/voiceable line — distinct
    // source_hash is how the line table (and downstream .xlf export) tells
    // them apart as separate translation units.
    let program = lower_ink("VAR x = true\nIt was {x: sunny|rainy} today.\n");
    let bodies = branch_bodies(&program);
    let hashes: Vec<u64> = bodies
        .iter()
        .map(|b| {
            b.iter()
                .find_map(|s| {
                    if let lir::Stmt::EmitLine(e) = s {
                        Some(e.metadata.source_hash)
                    } else {
                        None
                    }
                })
                .expect("branch should have a recognized EmitLine")
        })
        .collect();
    assert_ne!(
        hashes[0], hashes[1],
        "distinct branch text should hash differently"
    );
}

#[test]
fn branch_expansion_reachable_via_full_compile_and_codegen() {
    // Reachability check (not just LIR shape): compile through codegen to
    // StoryData and confirm the line table actually carries the composed,
    // independently-recognized Plain text — not just that LIR lowering
    // built the right shape in isolation.
    let src = "VAR x = true\nIt was {x: sunny|rainy} today.\n";
    let parsed = brink_syntax::parse(src);
    let file_id = brink_ir::FileId(0);
    let (mut hir, manifest, _diags) = brink_ir::hir::lower(file_id, &parsed.tree());
    brink_ir::hir::normalize_file(&mut hir);
    let files_for_analysis = vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);
    let files_for_lir = vec![(file_id, &hir)];
    let (program, _diags) = lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    );
    let program = program.expect("should lower to a real program");
    let story_data = brink_codegen_inkb::emit(&program).expect("codegen should succeed");
    let all_lines = story_data.line_tables.iter().flat_map(|t| t.lines.iter());
    let has_sunny = all_lines.clone().any(
        |e| matches!(&e.content, brink_format::LineContent::Plain(s) if s == "It was sunny today."),
    );
    let has_rainy = all_lines.clone().any(
        |e| matches!(&e.content, brink_format::LineContent::Plain(s) if s == "It was rainy today."),
    );
    assert!(
        has_sunny && has_rainy,
        "compiled StoryData's line table should carry both fully-composed \
         branch lines as independent Plain entries, not one shredded across \
         several EmitContent fragments"
    );
}
