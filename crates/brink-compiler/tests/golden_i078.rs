#![allow(clippy::unwrap_used, clippy::panic)]

//! Golden pipeline test for I078: `* [Option]\n    Text`
//!
//! Spells out the expected data structure at each compilation stage —
//! HIR, LIR, and JSON — then asserts the actual output matches.

use brink_ir::hir::{self, HirFile};
use brink_ir::lir::{self, ContainerKind};
use brink_ir::{FileId, SymbolManifest};
use serde_json::Value;

const SOURCE: &str = "*   [Option]\n    Text\n";

// ═══════════════════════════════════════════════════════════════════════
// Stage 1: HIR
// ═══════════════════════════════════════════════════════════════════════
//
// Expected HIR (ignoring source provenance pointers):
//
//   HirFile {
//     root_content: Block {
//       stmts: [
//         ChoiceSet {
//           choices: [
//             Choice {
//               is_sticky: false,
//               is_fallback: false,
//               label: None,
//               condition: None,
//               start_content: None,
//               bracket_content: Some(Content {
//                 parts: [Text("Option")],
//                 tags: [],
//               }),
//               inner_content: None,
//               divert: None,
//               tags: [],
//               body: Block {
//                 stmts: [
//                   Content(Content {
//                     parts: [Text("Text")],
//                     tags: [],
//                   }),
//                 ],
//               },
//             },
//           ],
//           gather: None,
//         },
//       ],
//     },
//     knots: [],
//     variables: [],
//     constants: [],
//     lists: [],
//     externals: [],
//     includes: [],
//   }

fn lower_hir(source: &str) -> (HirFile, SymbolManifest) {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (hir, manifest, diags) = brink_ir::hir::lower(file_id, &tree);
    assert!(
        diags.is_empty(),
        "HIR lowering should produce no diagnostics, got: {diags:?}"
    );
    (hir, manifest)
}

#[test]
fn i078_hir() {
    let (hir, _) = lower_hir(SOURCE);

    // Root content has exactly one statement: a ChoiceSet
    assert_eq!(hir.root_content.stmts.len(), 1);
    let hir::Stmt::ChoiceSet(cs) = &hir.root_content.stmts[0] else {
        panic!("expected ChoiceSet, got {:?}", hir.root_content.stmts[0]);
    };

    // One choice, no explicit gather (empty continuation)
    assert_eq!(cs.choices.len(), 1);
    assert!(cs.continuation.label.is_none());
    assert!(cs.continuation.stmts.is_empty());

    let choice = &cs.choices[0];

    // * = once-only, not sticky, not fallback
    assert!(!choice.is_sticky);
    assert!(!choice.is_fallback);
    assert!(choice.label.is_none());
    assert!(choice.condition.is_none());
    assert!(choice.tags.is_empty());

    // No start content (nothing before `[`)
    assert!(choice.start_content.is_none());

    // Bracket content = "Option"
    let bracket = choice
        .bracket_content
        .as_ref()
        .expect("should have bracket_content");
    assert_eq!(bracket.parts.len(), 1);
    assert!(matches!(&bracket.parts[0], hir::ContentPart::Text(t) if t == "Option"));
    assert!(bracket.tags.is_empty());

    // No inner content (nothing after `]` on choice line)
    assert!(choice.inner_content.is_none());

    // Body = EndOfLine (choice line newline) + Content("Text") + EndOfLine
    assert_eq!(choice.body.stmts.len(), 3);
    assert!(matches!(&choice.body.stmts[0], hir::Stmt::EndOfLine));
    let hir::Stmt::Content(body_content) = &choice.body.stmts[1] else {
        panic!("expected Content in body, got {:?}", choice.body.stmts[1]);
    };
    assert!(matches!(&choice.body.stmts[2], hir::Stmt::EndOfLine));
    assert_eq!(body_content.parts.len(), 1);
    assert!(matches!(&body_content.parts[0], hir::ContentPart::Text(t) if t == "Text"));
    assert!(body_content.tags.is_empty());

    // No knots, vars, etc.
    assert!(hir.knots.is_empty());
    assert!(hir.variables.is_empty());
    assert!(hir.constants.is_empty());
    assert!(hir.lists.is_empty());
    assert!(hir.externals.is_empty());
    assert!(hir.includes.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// Stage 2: LIR
// ═══════════════════════════════════════════════════════════════════════
//
// Expected LIR (ignoring DefinitionId values):
//
//   Program {
//     root: Container {
//       name: None,
//       kind: Root,
//       params: [],
//       body: [
//         ChoiceSet(ChoiceSet {
//           choices: [
//             Choice {
//               is_sticky: false,
//               is_fallback: false,
//               condition: None,
//               start_content: None,
//               choice_only_content: Some(Content {
//                 parts: [Text("Option")],
//                 tags: [],
//               }),
//               inner_content: None,
//               target: <c-0 DefinitionId>,
//               tags: [],
//             },
//           ],
//           gather_target: Some(<g-0 DefinitionId>),
//         }),
//         Divert(Divert { target: Done, args: [] }),
//       ],
//       children: [
//         Container {
//           name: Some("c-0"),
//           kind: ChoiceTarget,
//           body: [
//             EmitContent(Content {
//               parts: [Text("Text")],
//               tags: [],
//             }),
//             Divert(Divert { target: Container(<g-0>), args: [] }),
//           ],
//           children: [],
//           counting_flags: VISITS | COUNT_START_ONLY,
//         },
//         Container {
//           name: Some("g-0"),
//           kind: Gather,
//           body: [
//             Divert(Divert { target: Done, args: [] }),
//           ],
//           children: [],
//           counting_flags: empty,
//         },
//       ],
//     },
//     globals: [],
//     lists: [],
//     list_items: [],
//     externals: [],
//   }

fn lower_lir(source: &str) -> lir::Program {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (hir, manifest, _) = brink_ir::hir::lower(file_id, &tree);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    lir::lower_to_program(&files_for_lir, &result.index, &result.resolutions)
}

#[test]
fn i078_lir() {
    let p = lower_lir(SOURCE);

    // ── Root container ──
    let root = &p.root;
    assert_eq!(root.kind, ContainerKind::Root);
    assert!(root.name.is_none());
    assert!(root.params.is_empty());

    // Root body: ChoiceSet + Divert(Done)
    assert_eq!(root.body.len(), 2);

    let lir::Stmt::ChoiceSet(cs) = &root.body[0] else {
        panic!("expected ChoiceSet as first stmt");
    };
    let lir::Stmt::Divert(done_divert) = &root.body[1] else {
        panic!("expected Divert as second stmt");
    };
    assert!(matches!(done_divert.target, lir::DivertTarget::Done));
    assert!(done_divert.args.is_empty());

    // ── ChoiceSet ──
    assert_eq!(cs.choices.len(), 1);
    assert!(
        cs.gather_target.is_some(),
        "every choice set has a gather target"
    );

    let choice = &cs.choices[0];
    assert!(!choice.is_sticky);
    assert!(!choice.is_fallback);
    assert!(choice.condition.is_none());
    assert!(choice.tags.is_empty());

    // No start content
    assert!(choice.start_content.is_none());

    // choice_only_content = "Option"
    let coc = choice
        .choice_only_content
        .as_ref()
        .expect("should have choice_only_content");
    assert_eq!(coc.parts.len(), 1);
    assert!(matches!(&coc.parts[0], lir::ContentPart::Text(t) if t == "Option"));
    assert!(coc.tags.is_empty());

    // No inner content
    assert!(choice.inner_content.is_none());

    // ── Children: c-0 and g-0 ──
    assert_eq!(root.children.len(), 2);

    let c0 = &root.children[0];
    assert_eq!(c0.name.as_deref(), Some("c-0"));
    assert_eq!(c0.kind, ContainerKind::ChoiceTarget);
    assert!(c0.children.is_empty());

    // c-0 body: EndOfLine (choice line) + EmitContent("Text") + EndOfLine + Divert(Container(g-0))
    assert_eq!(c0.body.len(), 4);

    assert!(matches!(&c0.body[0], lir::Stmt::EndOfLine));

    let lir::Stmt::EmitContent(text_content) = &c0.body[1] else {
        panic!("expected EmitContent in c-0 body");
    };
    assert_eq!(text_content.parts.len(), 1);
    assert!(matches!(&text_content.parts[0], lir::ContentPart::Text(t) if t == "Text"));
    assert!(text_content.tags.is_empty());

    assert!(matches!(&c0.body[2], lir::Stmt::EndOfLine));

    let lir::Stmt::Divert(gather_divert) = &c0.body[3] else {
        panic!("expected Divert to gather in c-0 body");
    };
    let gather_id = cs.gather_target.unwrap();
    assert!(
        matches!(gather_divert.target, lir::DivertTarget::Container(id) if id == gather_id),
        "c-0 should divert to g-0"
    );

    let g0 = &root.children[1];
    assert_eq!(g0.name.as_deref(), Some("g-0"));
    assert_eq!(g0.kind, ContainerKind::Gather);
    assert!(g0.children.is_empty());

    // g-0 body: Divert(Done)
    assert_eq!(g0.body.len(), 1);
    let lir::Stmt::Divert(g0_done) = &g0.body[0] else {
        panic!("expected Divert in g-0 body");
    };
    assert!(matches!(g0_done.target, lir::DivertTarget::Done));

    // choice.target == c-0's id
    assert_eq!(choice.target, c0.id);

    // ── No globals/lists/externals ──
    assert!(p.globals.is_empty());
    assert!(p.lists.is_empty());
    assert!(p.list_items.is_empty());
    assert!(p.externals.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════
// Stage 3: JSON (must match inklecate reference exactly)
// ═══════════════════════════════════════════════════════════════════════
//
// Expected JSON (from inklecate):
//
//   {
//     "inkVersion": 21,
//     "root": [
//       [
//         "ev", "str", "^Option", "/str", "/ev",
//         { "*": "0.c-0", "flg": 20 },
//         {
//           "c-0": ["\n", "^Text", "\n", { "->": "0.g-0" }, { "#f": 5 }],
//           "g-0": ["done", null]
//         }
//       ],
//       "done",
//       null
//     ],
//     "listDefs": {}
//   }
//
// Key observations:
//   - No outer container wrapper — bracket-only choice goes inline
//   - flg: 20 = ONCE_ONLY (0x10) | HAS_CHOICE_ONLY_CONTENT (0x04)
//     (NOT HAS_START_CONTENT — there is no start content)
//   - c-0 body starts with "\n" (newline after choice selection)
//   - c-0 has "#f": 5 = VISITS | COUNT_START_ONLY
//   - g-0 is just ["done", null]
//   - "*": "0.c-0" — path prefixed with "0." (root inner container index)

#[test]
fn i078_json() {
    let our_json = brink_compiler::compile_string_to_json(SOURCE).expect("should compile");

    let our_value: Value = serde_json::to_value(&our_json).unwrap();

    let ref_json = serde_json::json!({
        "inkVersion": 21,
        "root": [
            [
                "ev", "str", "^Option", "/str", "/ev",
                { "*": "0.c-0", "flg": 20 },
                {
                    "c-0": ["\n", "^Text", "\n", { "->": "0.g-0" }, { "#f": 5 }],
                    "g-0": ["done", null]
                }
            ],
            "done",
            null
        ],
        "listDefs": {}
    });

    if our_value != ref_json {
        let our_pretty = serde_json::to_string_pretty(&our_value).unwrap();
        let ref_pretty = serde_json::to_string_pretty(&ref_json).unwrap();

        std::fs::write("/tmp/brink_i078_ours.json", &our_pretty).unwrap();
        std::fs::write("/tmp/brink_i078_ref.json", &ref_pretty).unwrap();

        panic!(
            "I078 JSON mismatch.\n\
             Files written to /tmp/brink_i078_{{ours,ref}}.json\n\n\
             Expected:\n{ref_pretty}\n\n\
             Got:\n{our_pretty}"
        );
    }
}
