//! HIR projection — re-exported from `brink_ir::hir::projection` since
//! #3064 B2 (the core moved upstream so `brink-db`'s per-segment salsa
//! queries can build projection fragments; `brink-db` cannot depend on
//! this crate). Everything downstream keeps importing from here; the one
//! piece that stays local is [`project_hir`], whose signature takes the
//! analyzer's `AnalysisResult` (a type the IR crate deliberately doesn't
//! know).

use std::collections::BTreeMap;

use brink_analyzer::AnalysisResult;
use brink_format::DefinitionId;
use brink_ir::FileId;
use brink_ir::hir::HirFile;

pub use brink_ir::hir::projection::*;

/// Project a file's HIR onto its source ranges.
#[must_use]
pub fn project_hir(
    hir: &HirFile,
    source: &str,
    analysis: &AnalysisResult,
    file: FileId,
) -> Projection {
    // Prebuild the range-keyed identity maps (§6.2). Ranges are unique per
    // declaration identifier / reference site, so no scope logic is needed.
    let mut decl_ids: BTreeMap<(u32, u32), DefinitionId> = BTreeMap::new();
    for info in analysis.index.symbols.values() {
        if info.file == file {
            decl_ids.insert(range_key(info.range), info.id);
        }
    }
    let mut ref_targets: BTreeMap<(u32, u32), DefinitionId> = BTreeMap::new();
    for r in &analysis.resolutions {
        if r.file == file {
            ref_targets.insert(range_key(r.range), r.target);
        }
    }

    project_with_maps(hir, source, &decl_ids, &ref_targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::IdeSession;
    use rowan::TextRange;

    fn project(src: &str) -> Projection {
        let mut session = IdeSession::new();
        session.update_and_analyze("main.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let db = session.db();
        let file = db.file_ids().next().expect("one file");
        let hir = db.hir(file).expect("hir");
        project_hir(hir, src, analysis, file)
    }

    #[test]
    fn named_symbols_carry_def_ids_and_refs_carry_targets() {
        let src = "\
=== start ===
Hello {name}.
* [Go] -> hub
=== hub ===
-> DONE
VAR name = \"x\"
";
        let p = project(src);

        // Knot declarations carry a def_id (the range-join against SymbolIndex).
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.def_id.is_some()),
            "a knot span carries a def_id"
        );

        // The `-> hub` divert resolves to a target, and that target is exactly
        // some projected knot's def_id — the range-join, end to end.
        let hub_target = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Divert)
            .find_map(|s| s.target_id)
            .expect("a resolved divert target");
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.def_id == Some(hub_target)),
            "the divert's target_id matches a projected knot's def_id"
        );

        // The VAR decl is projected from the flat vec, with a def_id.
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::VarDecl && s.def_id.is_some()),
            "VAR decl span with def_id"
        );
    }

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ──────
    //
    // The IDE pipeline (parse → HIR → analyze → project) must not crash on
    // annotated sources — this is a superset-grammar/HIR extension the
    // projection layer never inspects (it walks knots/params/vars by their
    // pre-existing shape), so the assertion here is really "this whole
    // pipeline runs to completion and still finds the same spans a
    // consumer (hover, go-to-def) would have found without annotations".

    #[test]
    fn ide_pipeline_does_not_crash_on_annotated_sources_and_still_projects_spans() {
        let src = "\
VAR gold: int = 100
CONST max_gold: int = 999
LIST Weathers = sunny, (rainy)
=== function heal(ref hp: int, amount: float): bool ===
~ temp bonus: string = \"none\"
~ return true
= aftermath
~ temp w: List<Weathers> = sunny
-> DONE
";
        let p = project(src);

        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::VarDecl && s.def_id.is_some()),
            "annotated VAR decl still projects with a def_id"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::ConstDecl && s.def_id.is_some()),
            "annotated CONST decl still projects with a def_id"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.def_id.is_some()),
            "the function knot with param/return annotations still projects"
        );
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::Param),
            "annotated params still project"
        );
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::TempDecl),
            "annotated/ascribed temps still project"
        );
    }

    #[test]
    fn ide_pipeline_does_not_crash_on_reserved_and_unknown_type_annotations() {
        // `fn(...)` (reserved until T1c) and an unrecognized name both
        // produce analyzer diagnostics (E062/E061), not a panic anywhere
        // in the pipeline.
        let src = "VAR cb: fn(int): bool = 0\nVAR p: Frobnicator = 0\nCONST bad: Frobnicator = 0\n";
        let p = project(src);
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::VarDecl && s.def_id.is_some()),
            "VAR decls with reserved/unknown annotations still project"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::ConstDecl && s.def_id.is_some()),
            "CONST decl with an unknown annotation still projects"
        );
    }

    #[test]
    fn inline_conditionals_sequences_and_includes_are_covered() {
        let src = "\
INCLUDE other.ink
=== start ===
Take the {red|blue} pill.
{ready: Go now.}
-> go
=== go ===
-> DONE
";
        let p = project(src);
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::Include),
            "INCLUDE span projected"
        );
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::SequenceBranch),
            "inline sequence {{red|blue}} → SequenceBranch container"
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::ConditionalBranch),
            "inline conditional {{ready: ...}} → ConditionalBranch container"
        );
    }

    #[test]
    fn containers_and_per_line_stack() {
        let src = "\
=== start ===
Hello.
* [Go]
  Nested body.
- gather
";
        let p = project(src);

        // Knot + choice containers present, with handles.
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Knot && s.handle.is_some())
        );
        assert!(
            p.spans
                .iter()
                .any(|s| s.kind == SpanKind::Choice && s.handle.is_some())
        );

        // Every line's stack is depth-ordered (outermost first).
        for stack in &p.lines {
            assert!(
                stack
                    .containers
                    .windows(2)
                    .all(|w| w[0].depth <= w[1].depth),
                "line stack must be depth-ordered: {stack:?}"
            );
        }

        // The choice body line sits inside both the knot and the choice.
        let deepest = p
            .lines
            .iter()
            .map(|l| l.containers.len())
            .max()
            .unwrap_or(0);
        assert!(deepest >= 2, "a line should be inside knot + choice");
    }

    #[test]
    fn choice_containers_carry_stickiness_and_weave_depth() {
        let src = "\
=== start ===
* Once
* * Nested once
+ Sticky
- gathered
-> END
";
        let p = project(src);
        let choices: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Choice)
            .collect();
        assert_eq!(choices.len(), 3);
        assert_eq!(
            (choices[0].sticky, choices[0].weave_depth),
            (Some(false), Some(1))
        );
        assert_eq!(
            (choices[1].sticky, choices[1].weave_depth),
            (Some(false), Some(2)),
            "nested choice carries its sigil depth"
        );
        assert_eq!(
            (choices[2].sticky, choices[2].weave_depth),
            (Some(true), Some(1)),
            "`+` choice is sticky"
        );
        let gather = p
            .spans
            .iter()
            .find(|s| s.kind == SpanKind::Gather)
            .expect("gather container");
        assert_eq!(gather.weave_depth, Some(1));
    }

    #[test]
    fn inline_choice_set_reports_sigil_depth() {
        // Choices inside a conditional arm form an Inline choice set with
        // `cs.depth == 0` — their weave depth comes from the literal sigils
        // (#478), the depth Tab/Enter transitions need to rebuild prefixes.
        let src = "\
=== start ===
{ ready:
    * [Go now]
    * * [Deeper]
- else:
    Not ready.
}
-> END
";
        let p = project(src);
        let choices: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Choice)
            .collect();
        assert_eq!(choices.len(), 2);
        assert_eq!(
            choices[0].weave_depth,
            Some(1),
            "single-sigil inline choice: {choices:?}"
        );
        assert_eq!(
            choices[1].weave_depth,
            Some(2),
            "double-sigil inline choice: {choices:?}"
        );
    }

    #[test]
    fn terminal_diverts_project_divert_terminal_spans() {
        let src = "\
=== start ===
-> DONE
=== other ===
-> END
";
        let p = project(src);
        let terminals: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::DivertTerminal)
            .collect();
        assert_eq!(
            terminals.len(),
            2,
            "-> DONE and -> END each project a DivertTerminal span"
        );
        assert!(
            !p.spans.iter().any(|s| s.kind == SpanKind::Divert),
            "terminal diverts are not Divert reference spans"
        );
    }

    #[test]
    fn divert_statements_project_stmt_spans_but_expressions_do_not() {
        // `-> hub` is a divert statement (DivertStmt + a Divert target ref);
        // `~ temp x = -> hub` holds a divert-target *expression* — a Divert
        // reference only, no statement span. Line views classify from the
        // statement span, so the temp line stays logic, not a divert.
        let src = "\
=== start ===
~ temp x = -> hub
-> hub
=== hub ===
-> DONE
";
        let p = project(src);
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::DivertStmt)
                .count(),
            1,
            "only the standalone divert is a statement span"
        );
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Divert)
                .count(),
            2,
            "both the expression and the statement carry target references"
        );
    }

    #[test]
    fn assignments_and_returns_project_logic_spans() {
        let src = "\
VAR gold = 0
=== start ===
~ gold = 1
-> DONE
=== function f ===
~ return 0
";
        let p = project(src);
        let logic = p.spans.iter().filter(|s| s.kind == SpanKind::Logic).count();
        assert_eq!(logic, 2, "one assignment + one return");
    }

    #[test]
    fn choice_tags_project_tag_spans() {
        let src = "\
=== start ===
* Choice # tagged
-> DONE
";
        let p = project(src);
        assert!(
            p.spans.iter().any(|s| s.kind == SpanKind::Tag),
            "choice-level tags project Tag spans"
        );
    }

    #[test]
    fn bare_labeled_gather_projects_its_container() {
        // `- (g)` with an empty continuation: the label range alone is the
        // gather extent (block_extent is None), so the container must still
        // be emitted.
        let src = "\
=== start ===
* Choice
- (g)
";
        let p = project(src);
        let gather = p
            .spans
            .iter()
            .find(|s| s.kind == SpanKind::Gather)
            .expect("bare labeled gather projects a container");
        assert_eq!(gather.weave_depth, Some(1));
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test fixture byte offsets are a few dozen bytes, far below u32::MAX"
    )]
    fn conditional_arm_prose_projects_content_spans() {
        // Issue #981: prose inside a multi-line conditional arm (the ink
        // `{ Flag: ... - else: ... }` switch/branch shape) got no `content`
        // span at all — only the whole-construct `Conditional` span covered
        // it. Top-level prose outside the conditional already projects
        // `Content` spans; arm prose must too, with byte-exact ranges.
        let src = "\
=== start ===
{ Flag:
Some dialogue prose inside the arm.
- else:
Wait here.
}
-> DONE
";
        let p = project(src);

        let arm_1_text = "Some dialogue prose inside the arm.";
        let arm_1_start = src.find(arm_1_text).expect("fixture contains arm 1 text");
        let arm_1_range = TextRange::new(
            (arm_1_start as u32).into(),
            ((arm_1_start + arm_1_text.len()) as u32).into(),
        );

        let arm_2_text = "Wait here.";
        let arm_2_start = src.find(arm_2_text).expect("fixture contains arm 2 text");
        let arm_2_range = TextRange::new(
            (arm_2_start as u32).into(),
            ((arm_2_start + arm_2_text.len()) as u32).into(),
        );

        let content_ranges: Vec<TextRange> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Content)
            .map(|s| s.range)
            .collect();

        assert!(
            content_ranges.contains(&arm_1_range),
            "the first arm's prose must project its own Content span at {arm_1_range:?}; got {content_ranges:?}"
        );
        assert!(
            content_ranges.contains(&arm_2_range),
            "the else arm's prose must project its own Content span at {arm_2_range:?}; got {content_ranges:?}"
        );

        // Should-not-change: the construct-extent Conditional span itself,
        // and the top-level prose outside it (the greeting line if any),
        // stay exactly as before — this fix only adds arm-content spans, it
        // does not touch the construct span's own range.
        let cond_spans: Vec<_> = p
            .spans
            .iter()
            .filter(|s| s.kind == SpanKind::Conditional)
            .collect();
        assert_eq!(cond_spans.len(), 1, "one construct-extent Conditional span");
        let expected_cond_start = src
            .find("{ Flag:")
            .expect("fixture contains the conditional open");
        let expected_cond_end = src
            .find('}')
            .expect("fixture contains the conditional close")
            + 1;
        assert_eq!(
            cond_spans[0].range,
            TextRange::new(
                (expected_cond_start as u32).into(),
                (expected_cond_end as u32).into()
            ),
            "the Conditional construct span's own range is unchanged by this fix"
        );
    }

    #[test]
    fn construct_extent_spans_are_body_gated() {
        // A statement-level conditional and a body inline sequence project
        // construct-extent spans; an inline sequence in a choice's bracket
        // text projects its container (rails) but NOT a construct span
        // (never scaffold).
        let src = "\
=== start ===
{ ready:
Go.
- else:
Wait.
}
Take the {red|blue} pill.
* [Take the {big|small} dose]
-> DONE
";
        let p = project(src);
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Conditional)
                .count(),
            1,
            "the multiline conditional projects one construct-extent span"
        );
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::Sequence)
                .count(),
            1,
            "only the body inline sequence projects a construct span — the \
             choice-bracket one is gated out"
        );
        assert_eq!(
            p.spans
                .iter()
                .filter(|s| s.kind == SpanKind::SequenceBranch)
                .count(),
            2,
            "both inline sequences still project rail containers"
        );
        // Construct spans are not containers: no handle, absent from stacks.
        assert!(
            p.spans
                .iter()
                .filter(|s| matches!(s.kind, SpanKind::Conditional | SpanKind::Sequence))
                .all(|s| s.handle.is_none() && !s.kind.is_container())
        );
    }
}

#[cfg(test)]
mod tight_end_tests {
    use super::*;
    use brink_ir::LineIndex;

    #[test]
    fn playground_fixture_sweep_never_panics() {
        // Repro harness for the wasm `unreachable` seen live (#3054): run
        // the tight-end computation over every span of the real playground
        // fixture.
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../packages/brink-studio/src/stories/toppled-temple.ink.txt"
        );
        let source = std::fs::read_to_string(fixture).expect("fixture readable");
        let parse = brink_syntax::parse(&source);
        let (hir, _, _) = brink_ir::hir::lower(brink_ir::FileId(0), &parse.tree());
        let projection = super::project_hir_structural(&hir, &source);
        let idx = LineIndex::new(&source);
        for sp in &projection.spans {
            let _ = tight_container_end_line(&idx, &source, sp.range);
        }
    }

    #[test]
    fn tight_end_excludes_trailing_blanks_and_next_decls_docs() {
        // `roll`'s structural range runs to `spend_torch`'s header — the
        // tight end must stop at `~ return …` (line 1), before the blank
        // line and the NEXT function's `///` block (#3054 review).
        let source = "\
=== function roll(lo, hi) ===
~ return RANDOM(lo, hi)

/// Burn the torch down by n notches.
/// @param n {int}
=== function spend_torch(n) ===
~ torch = torch - n
";
        let idx = LineIndex::new(source);
        let header_of_next = source.find("=== function spend_torch").expect("fixture");
        let range = rowan::TextRange::new(
            rowan::TextSize::from(0),
            rowan::TextSize::from(u32::try_from(header_of_next).expect("fits")),
        );
        assert_eq!(tight_container_end_line(&idx, source, range), 1);

        // Whitespace-only tail (no doc block): still trimmed.
        let blank_tail_end = source.find("/// Burn").expect("fixture");
        let range = rowan::TextRange::new(
            rowan::TextSize::from(0),
            rowan::TextSize::from(u32::try_from(blank_tail_end).expect("fits")),
        );
        assert_eq!(tight_container_end_line(&idx, source, range), 1);
    }
}
