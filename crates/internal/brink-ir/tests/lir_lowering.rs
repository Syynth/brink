#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::items_after_statements
)]

use brink_ir::lir;
use brink_ir::{FileId, HirFile, SymbolManifest};

// ─── Test harness ───────────────────────────────────────────────────

/// Parse ink source → HIR lower → analyze → LIR lower. Returns the full Program.
fn lower_ink(source: &str) -> lir::Program {
    let (program, _warnings) = lower_ink_with_warnings(source);
    program
}

/// Parse ink source → HIR lower → analyze → LIR lower. Returns program + warnings.
fn lower_ink_with_warnings(source: &str) -> (lir::Program, Vec<brink_ir::Diagnostic>) {
    let parsed = brink_syntax::parse(source);
    let tree = parsed.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, _diags) = brink_ir::hir::lower(file_id, &tree);

    // Normalize HIR (lift inline sequences/conditionals) — mirrors what
    // `lower_to_program` does internally so the test pipeline is consistent.
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    lir::lower_to_program(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
    )
}

/// Get the root container.
fn root(program: &lir::Program) -> &lir::Container {
    &program.root
}

/// Find a direct child of a container by name.
fn find_child<'a>(container: &'a lir::Container, name: &str) -> &'a lir::Container {
    container
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some(name))
        .unwrap_or_else(|| {
            let names: Vec<Option<&str>> = container
                .children
                .iter()
                .map(|c| c.name.as_deref())
                .collect();
            panic!("no child named {name:?}, available: {names:?}")
        })
}

/// Find a container by dot-separated path from root.
fn find_by_path<'a>(program: &'a lir::Program, path: &str) -> &'a lir::Container {
    if path.is_empty() {
        return &program.root;
    }
    let mut current = &program.root;
    for segment in path.split('.') {
        current = find_child(current, segment);
    }
    current
}

/// Find a global by checking if its name matches via the name table.
fn find_global<'a>(program: &'a lir::Program, name: &str) -> &'a lir::GlobalDef {
    program
        .globals
        .iter()
        .find(|g| program.name_table[g.name.0 as usize] == name)
        .unwrap_or_else(|| panic!("no global named {name:?}"))
}

/// Recursively count containers of a given kind in the tree.
fn count_kind(container: &lir::Container, kind: lir::ContainerKind) -> usize {
    let mut count = usize::from(container.kind == kind);
    for child in &container.children {
        count += count_kind(child, kind);
    }
    count
}

/// Count all containers in the tree (including the root itself).
fn count_all(container: &lir::Container) -> usize {
    1 + container.children.iter().map(count_all).sum::<usize>()
}

/// Extract text from `EmitContent` statements.
fn collect_text(stmts: &[lir::Stmt]) -> Vec<String> {
    let mut texts = Vec::new();
    for stmt in stmts {
        match stmt {
            lir::Stmt::EmitContent(content) => {
                let mut line = String::new();
                for part in &content.parts {
                    if let lir::ContentPart::Text(t) = part {
                        line.push_str(t);
                    }
                }
                if !line.is_empty() {
                    texts.push(line);
                }
            }
            lir::Stmt::EmitLine(emission) => match &emission.line {
                lir::RecognizedLine::Plain(s) => {
                    if !s.is_empty() {
                        texts.push(s.clone());
                    }
                }
                lir::RecognizedLine::Template { parts, .. } => {
                    let mut line = String::new();
                    for part in parts {
                        if let brink_format::LinePart::Literal(s) = part {
                            line.push_str(s);
                        }
                    }
                    if !line.is_empty() {
                        texts.push(line);
                    }
                }
            },
            _ => {}
        }
    }
    texts
}

/// Check if a statement list ends with a divert.
fn ends_with_divert(stmts: &[lir::Stmt]) -> bool {
    stmts
        .last()
        .is_some_and(|s| matches!(s, lir::Stmt::Divert(_)))
}

/// Recursively find any container matching a predicate.
fn find_any<'a>(
    container: &'a lir::Container,
    pred: &dyn Fn(&lir::Container) -> bool,
) -> Option<&'a lir::Container> {
    if pred(container) {
        return Some(container);
    }
    for child in &container.children {
        if let Some(found) = find_any(child, pred) {
            return Some(found);
        }
    }
    None
}

/// Collect all containers of a given kind from the tree.
fn collect_kind(container: &lir::Container, kind: lir::ContainerKind) -> Vec<&lir::Container> {
    let mut result = Vec::new();
    if container.kind == kind {
        result.push(container);
    }
    for child in &container.children {
        result.extend(collect_kind(child, kind));
    }
    result
}

// ─── Basic content ──────────────────────────────────────────────────

#[test]
fn minimal_story_has_root_container() {
    let p = lower_ink("Hello, world!\n");
    assert_eq!(p.root.kind, lir::ContainerKind::Root);
}

#[test]
fn root_content_emits_text() {
    let p = lower_ink("Hello, world!\n");
    let r = root(&p);
    let texts = collect_text(&r.body);
    assert_eq!(texts, vec!["Hello, world!"]);
}

#[test]
fn root_has_implicit_done() {
    let p = lower_ink("Hello!\n");
    let r = root(&p);
    assert!(
        ends_with_divert(&r.body),
        "root should end with implicit DONE"
    );
    if let Some(lir::Stmt::Divert(d)) = r.body.last() {
        assert!(
            matches!(d.target, lir::DivertTarget::Done),
            "root should end with DONE, not {:?}",
            std::mem::discriminant(&d.target)
        );
    }
}

#[test]
fn multiple_content_lines() {
    let p = lower_ink("Line one.\nLine two.\nLine three.\n");
    let r = root(&p);
    let texts = collect_text(&r.body);
    assert_eq!(texts, vec!["Line one.", "Line two.", "Line three."]);
}

// ─── Knots ──────────────────────────────────────────────────────────

#[test]
fn knot_creates_container() {
    let p = lower_ink("== greet ==\nHello!\n-> END\n");
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 1);
    let knot = find_child(&p.root, "greet");
    assert_eq!(knot.kind, lir::ContainerKind::Knot);
}

#[test]
fn knot_body_has_content() {
    let p = lower_ink("== greet ==\nWelcome.\n-> END\n");
    let knot = find_child(&p.root, "greet");
    let texts = collect_text(&knot.body);
    assert_eq!(texts, vec!["Welcome."]);
}

#[test]
fn knot_divert_to_end() {
    let p = lower_ink("== greet ==\nHi.\n-> END\n");
    let knot = find_child(&p.root, "greet");
    assert!(ends_with_divert(&knot.body));
    if let Some(lir::Stmt::Divert(d)) = knot.body.last() {
        assert!(matches!(d.target, lir::DivertTarget::End));
    }
}

#[test]
fn multiple_knots() {
    let p = lower_ink(
        "\
== alpha ==
First.
-> END

== beta ==
Second.
-> END
",
    );
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 2);
    let a = find_child(&p.root, "alpha");
    let b = find_child(&p.root, "beta");
    assert_eq!(collect_text(&a.body), vec!["First."]);
    assert_eq!(collect_text(&b.body), vec!["Second."]);
}

#[test]
fn root_divert_to_knot_resolves() {
    let p = lower_ink("-> greet\n== greet ==\nHi.\n-> END\n");
    let r = root(&p);
    let knot = find_child(&p.root, "greet");

    let has_divert_to_knot = r.body.iter().any(|stmt| {
        if let lir::Stmt::Divert(d) = stmt {
            matches!(d.target, lir::DivertTarget::Address(id) if id == knot.id)
        } else {
            false
        }
    });
    assert!(has_divert_to_knot, "root should divert to knot 'greet'");
}

// ─── Stitches ───────────────────────────────────────────────────────

#[test]
fn stitch_creates_container() {
    let p = lower_ink(
        "\
== tavern ==
= order
What'll it be?
-> END
",
    );
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Stitch), 1);
    let stitch = find_by_path(&p, "tavern.order");
    assert_eq!(stitch.kind, lir::ContainerKind::Stitch);
    assert_eq!(collect_text(&stitch.body), vec!["What'll it be?"]);
}

#[test]
fn knot_with_stitches_and_no_own_body() {
    let p = lower_ink(
        "\
== tavern ==
= order
Ordering.
-> END
= pay
Paying.
-> END
",
    );
    let _knot = find_child(&p.root, "tavern");
    let stitch_order = find_by_path(&p, "tavern.order");
    let stitch_pay = find_by_path(&p, "tavern.pay");

    assert_eq!(collect_text(&stitch_order.body), vec!["Ordering."]);
    assert_eq!(collect_text(&stitch_pay.body), vec!["Paying."]);
}

#[test]
fn stitch_is_child_of_knot() {
    let p = lower_ink(
        "\
== tavern ==
= order
Hi.
-> END
",
    );
    let knot = find_child(&p.root, "tavern");
    let stitch = find_child(knot, "order");
    assert_eq!(stitch.kind, lir::ContainerKind::Stitch);
}

// ─── Variables and constants ────────────────────────────────────────

#[test]
fn var_declaration_creates_mutable_global() {
    let p = lower_ink("VAR x = 5\n");
    let g = find_global(&p, "x");
    assert!(g.mutable);
    assert!(matches!(g.default, lir::ConstValue::Int(5)));
}

#[test]
fn const_declaration_creates_immutable_global() {
    let p = lower_ink("CONST y = 10\n");
    let g = find_global(&p, "y");
    assert!(!g.mutable);
    assert!(matches!(g.default, lir::ConstValue::Int(10)));
}

#[test]
fn var_float_default() {
    let p = lower_ink("VAR f = 2.5\n");
    let g = find_global(&p, "f");
    if let lir::ConstValue::Float(v) = g.default {
        assert!((v - 2.5).abs() < 0.01);
    } else {
        panic!("expected Float default, got something else");
    }
}

#[test]
fn var_string_default() {
    let p = lower_ink("VAR name = \"hello\"\n");
    let g = find_global(&p, "name");
    assert!(matches!(&g.default, lir::ConstValue::String(s) if s == "hello"));
}

#[test]
fn var_bool_default() {
    let p = lower_ink("VAR flag = true\n");
    let g = find_global(&p, "flag");
    assert!(matches!(g.default, lir::ConstValue::Bool(true)));
}

#[test]
fn var_negative_default() {
    let p = lower_ink("VAR n = -42\n");
    let g = find_global(&p, "n");
    assert!(matches!(g.default, lir::ConstValue::Int(-42)));
}

// ─── Lists ──────────────────────────────────────────────────────────

#[test]
fn list_declaration() {
    let p = lower_ink("LIST colors = red, green, blue\n");
    assert_eq!(p.lists.len(), 1);
    assert_eq!(p.list_items.len(), 3);

    let ordinals: Vec<i32> = p.list_items.iter().map(|i| i.ordinal).collect();
    assert_eq!(ordinals, vec![1, 2, 3]);
}

#[test]
fn list_items_reference_origin() {
    let p = lower_ink("LIST mood = happy, sad, angry\n");
    let list_id = p.lists[0].id;
    for item in &p.list_items {
        assert_eq!(
            item.origin, list_id,
            "each list item should reference its origin list"
        );
    }
}

#[test]
fn list_explicit_ordinals() {
    let p = lower_ink("LIST rank = private = 1, corporal = 5, sergeant = 10\n");
    let ordinals: Vec<i32> = p.list_items.iter().map(|i| i.ordinal).collect();
    assert_eq!(ordinals, vec![1, 5, 10]);
}

#[test]
fn list_declaration_creates_global_variable() {
    // LIST declarations should produce a mutable global variable
    // initialized to the set of active (parenthesized) items.
    let p = lower_ink("LIST mood = (happy), sad, (excited)\n");

    // Should have a global named "mood"
    let g = find_global(&p, "mood");
    assert!(g.mutable, "list global should be mutable");

    // The global's ID should be a GlobalVar ($02_), not a ListDef ($03_)
    assert_eq!(
        g.id.tag(),
        brink_format::DefinitionTag::GlobalVar,
        "list global should have GlobalVar tag, got {:?}",
        g.id.tag()
    );

    // Default value should be a List with the active items
    if let lir::ConstValue::List { items, origins } = &g.default {
        assert_eq!(
            items.len(),
            2,
            "should have 2 active items (happy, excited)"
        );
        assert!(!origins.is_empty(), "should have origin list");
    } else {
        panic!(
            "list global default should be ConstValue::List, got {:?}",
            std::mem::discriminant(&g.default)
        );
    }
}

#[test]
fn list_no_active_items_creates_empty_global() {
    // LIST with no parenthesized items still creates a global
    let p = lower_ink("LIST colors = red, green, blue\n");

    let g = find_global(&p, "colors");
    assert!(g.mutable);
    if let lir::ConstValue::List { items, origins } = &g.default {
        assert!(items.is_empty(), "no active items means empty list");
        assert!(!origins.is_empty(), "should still track origin list");
    } else {
        panic!("expected List default");
    }
}

#[test]
fn list_global_referenced_in_expression() {
    // When code references a list variable, expr lowering should emit
    // GetGlobal with the GlobalVar ID, not the ListDef ID.
    let p = lower_ink("LIST mood = (happy), sad\n{mood}\n");

    let g = find_global(&p, "mood");
    let r = root(&p);

    fn expr_refs_global(expr: &lir::Expr, id: brink_format::DefinitionId) -> bool {
        matches!(expr, lir::Expr::GetGlobal(x) if *x == id)
    }

    let has_ref = r.body.iter().any(|s| match s {
        lir::Stmt::EmitContent(c) => c.parts.iter().any(
            |p| matches!(p, lir::ContentPart::Interpolation(expr) if expr_refs_global(expr, g.id)),
        ),
        lir::Stmt::ExprStmt(expr) => expr_refs_global(expr, g.id),
        _ => false,
    });
    assert!(
        has_ref,
        "expression should reference the GlobalVar ID, not the ListDef ID"
    );
}

#[test]
fn list_assignment_targets_global_var() {
    // `~ mood = happy` should assign to the GlobalVar ID
    let p = lower_ink("LIST mood = (happy), sad\n~ mood = sad\n");

    let g = find_global(&p, "mood");
    let r = root(&p);

    let has_assign_to_var = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::Assign {
            target: lir::AssignTarget::Global(id),
            ..
        } if *id == g.id)
    });
    assert!(
        has_assign_to_var,
        "assignment to list should target the GlobalVar ID"
    );
}

// ─── Externals ──────────────────────────────────────────────────────

#[test]
fn external_declaration() {
    let p = lower_ink("EXTERNAL multiply(a, b)\n");
    assert_eq!(p.externals.len(), 1);
    assert_eq!(p.externals[0].arg_count, 2);
}

#[test]
fn multiple_externals() {
    let p = lower_ink("EXTERNAL foo(x)\nEXTERNAL bar(a, b, c)\n");
    assert_eq!(p.externals.len(), 2);
    let arg_counts: Vec<u8> = p.externals.iter().map(|e| e.arg_count).collect();
    assert!(arg_counts.contains(&1));
    assert!(arg_counts.contains(&3));
}

// ─── Temp variables ─────────────────────────────────────────────────

#[test]
fn temp_decl_in_knot() {
    let p = lower_ink(
        "\
== func ==
~ temp x = 42
-> END
",
    );
    let knot = find_child(&p.root, "func");
    let has_temp = knot.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::DeclareTemp {
                slot: 0,
                value: Some(lir::Expr::Int(42)),
                ..
            }
        )
    });
    assert!(has_temp, "knot should have temp declaration at slot 0");
}

#[test]
fn params_occupy_first_temp_slots() {
    let p = lower_ink(
        "\
== func(a, b) ==
~ temp c = 0
-> END
",
    );
    let knot = find_child(&p.root, "func");
    assert_eq!(knot.params.len(), 2);
    assert_eq!(knot.params[0].slot, 0);
    assert_eq!(knot.params[1].slot, 1);
    assert_eq!(knot.temp_slot_count, 3);

    let has_temp_at_2 = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::DeclareTemp { slot: 2, .. }));
    assert!(has_temp_at_2, "temp 'c' should be at slot 2 (after params)");
}

// ─── Choices ────────────────────────────────────────────────────────

#[test]
fn choice_set_creates_containers() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  After A.
* Choice B
  After B.
- Gathered.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    assert!(count_kind(scene, lir::ContainerKind::ChoiceTarget) >= 2);
    assert!(count_kind(scene, lir::ContainerKind::Gather) >= 1);
}

#[test]
fn choice_set_in_knot_body() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  After A.
* Choice B
  After B.
- Gathered.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let has_choice_set = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::ChoiceSet(_)));
    assert!(has_choice_set, "knot should contain a ChoiceSet statement");
}

#[test]
fn choice_targets_have_body_content() {
    let p = lower_ink(
        "\
== scene ==
* First
  Content after first.
* Second
  Content after second.
- Gather point.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let choice_targets = collect_kind(scene, lir::ContainerKind::ChoiceTarget);
    assert_eq!(choice_targets.len(), 2);

    let any_has_content = choice_targets
        .iter()
        .any(|c| !collect_text(&c.body).is_empty());
    assert!(any_has_content, "choice targets should have body content");
}

#[test]
fn gather_has_content() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  A body.
* Choice B
  B body.
- Gathered here.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let gathers = collect_kind(scene, lir::ContainerKind::Gather);
    assert!(!gathers.is_empty(), "should have at least one gather");

    let gather_texts: Vec<String> = gathers.iter().flat_map(|g| collect_text(&g.body)).collect();
    assert!(
        gather_texts.iter().any(|t| t.contains("Gathered here")),
        "gather should contain its inline content, got: {gather_texts:?}"
    );
}

#[test]
fn gather_includes_trailing_statements() {
    let p = lower_ink(
        "\
== scene ==
* Choice A
  A.
- Gather.
More content after gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let gathers = collect_kind(scene, lir::ContainerKind::Gather);
    assert!(!gathers.is_empty());

    let gather = &gathers[0];
    let texts = collect_text(&gather.body);
    assert!(
        texts.iter().any(|t| t.contains("More content")),
        "gather should include trailing statements from parent block, got: {texts:?}"
    );
    assert!(
        ends_with_divert(&gather.body),
        "gather should include trailing divert from parent block"
    );
}

#[test]
fn choice_set_has_gather_target() {
    let p = lower_ink(
        "\
== scene ==
* Alpha
  A.
* Beta
  B.
- Meet here.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let cs = knot.body.iter().find_map(|s| {
        if let lir::Stmt::ChoiceSet(cs) = s {
            Some(cs)
        } else {
            None
        }
    });
    assert!(cs.is_some(), "knot should have a ChoiceSet");
    let cs = cs.unwrap();
    assert!(
        cs.gather_target.is_some(),
        "ChoiceSet should have a gather target"
    );

    // The gather target should match a gather container's id
    let gather_id = cs.gather_target.unwrap();
    let gather_exists = find_any(&p.root, &|c| {
        c.id == gather_id && c.kind == lir::ContainerKind::Gather
    })
    .is_some();
    assert!(
        gather_exists,
        "gather_target should reference an existing gather container"
    );
}

#[test]
fn sticky_choice_flag() {
    let p = lower_ink(
        "\
== scene ==
+ Sticky choice
  Body.
- Done.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let choice = knot.body.iter().find_map(|s| {
        if let lir::Stmt::ChoiceSet(cs) = s {
            cs.choices.first()
        } else {
            None
        }
    });
    assert!(choice.is_some());
    assert!(choice.unwrap().is_sticky, "'+' choice should be sticky");
}

#[test]
fn once_only_choice_flag() {
    let p = lower_ink(
        "\
== scene ==
* Once-only choice
  Body.
- Done.
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let choice = knot.body.iter().find_map(|s| {
        if let lir::Stmt::ChoiceSet(cs) = s {
            cs.choices.first()
        } else {
            None
        }
    });
    assert!(choice.is_some());
    assert!(
        !choice.unwrap().is_sticky,
        "'*' choice should NOT be sticky"
    );
}

// ─── Choice inline divert folding ───────────────────────────────────

/// Choice with inline divert: choice target body starts with `ChoiceOutput`,
/// then `Divert`, then `EndOfLine` (the divert comes from the HIR body preamble).
#[test]
fn choice_inline_divert_in_target_body() {
    let p = lower_ink(
        "\
== scene ==
* Go somewhere -> other
- Gathered.
-> END
== other ==
Arrived.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let c0 = find_child(scene, "c-0");
    assert_eq!(c0.kind, lir::ContainerKind::ChoiceTarget);

    // Body should be: EmitLine("Go somewhere") or ChoiceOutput, Divert(other), EndOfLine, Divert(gather)
    assert!(
        matches!(
            &c0.body[0],
            lir::Stmt::EmitLine(_) | lir::Stmt::ChoiceOutput { .. }
        ),
        "first stmt should be EmitLine or ChoiceOutput with content, got {:?}",
        std::mem::discriminant(&c0.body[0])
    );
    assert!(
        matches!(&c0.body[1], lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(_))),
        "second stmt should be Divert to 'other'"
    );
    assert!(
        matches!(&c0.body[2], lir::Stmt::EndOfLine),
        "third stmt should be EndOfLine"
    );
}

/// Choice without inline divert: choice target body starts with `ChoiceOutput`,
/// then `EndOfLine` (no divert in preamble).
#[test]
fn choice_no_divert_endofline_in_target_body() {
    let p = lower_ink(
        "\
== scene ==
* Stay here
  Some body text.
- Gathered.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let c0 = find_child(scene, "c-0");

    // Body: EmitLine("Stay here") or ChoiceOutput, EndOfLine, EmitContent("Some body text."), EndOfLine, Divert(gather)
    assert!(
        matches!(
            &c0.body[0],
            lir::Stmt::EmitLine(_) | lir::Stmt::ChoiceOutput { .. }
        ),
        "first stmt should be EmitLine or ChoiceOutput"
    );
    assert!(
        matches!(&c0.body[1], lir::Stmt::EndOfLine),
        "second stmt should be EndOfLine"
    );
    assert!(
        matches!(
            &c0.body[2],
            lir::Stmt::EmitContent(_) | lir::Stmt::EmitLine(_)
        ),
        "third stmt should be EmitContent or EmitLine"
    );
}

/// Fallback choice (no content) with only a divert: no `ChoiceOutput`, body starts
/// with `Divert` then `EndOfLine`.
#[test]
fn fallback_choice_divert_only_in_target_body() {
    let p = lower_ink(
        "\
== scene ==
* [Visible choice] text
* -> other
- Gathered.
-> END
== other ==
Arrived.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    // c-1 is the fallback choice
    let c1 = find_child(scene, "c-1");

    // Fallback has no start/inner content → no ChoiceOutput.
    // Body: Divert(other), EndOfLine, Divert(gather)
    assert!(
        matches!(&c1.body[0], lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(_))),
        "first stmt should be Divert to 'other', got {:?}",
        std::mem::discriminant(&c1.body[0])
    );
    assert!(
        matches!(&c1.body[1], lir::Stmt::EndOfLine),
        "second stmt should be EndOfLine"
    );
}

/// `ChoiceOutput` is purely content — no divert, no newline. `Divert` and `EndOfLine`
/// are separate body stmts.
#[test]
fn choice_output_is_content_only() {
    let p = lower_ink(
        "\
== scene ==
* Hello world -> END
- Gathered.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let c0 = find_child(scene, "c-0");

    // Output should be EmitLine (recognized) or ChoiceOutput (fallback)
    match &c0.body[0] {
        lir::Stmt::EmitLine(emission) => {
            assert!(
                matches!(&emission.line, lir::RecognizedLine::Plain(s) if s == "Hello world"),
                "EmitLine should contain 'Hello world'"
            );
        }
        lir::Stmt::ChoiceOutput { content, .. } => {
            assert!(
                content
                    .parts
                    .iter()
                    .all(|p| matches!(p, lir::ContentPart::Text(_) | lir::ContentPart::Spring)),
                "ChoiceOutput should only contain text parts (Text or Spring)"
            );
        }
        other => panic!(
            "expected EmitLine or ChoiceOutput as first body stmt, got {:?}",
            std::mem::discriminant(other)
        ),
    }

    // The divert to END follows as a separate stmt
    assert!(
        matches!(&c0.body[1], lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::End)),
        "second stmt should be Divert to END"
    );
    assert!(
        matches!(&c0.body[2], lir::Stmt::EndOfLine),
        "third stmt should be EndOfLine"
    );
}

#[test]
fn interpolated_choice_text_is_recognized_as_template() {
    let p = lower_ink(
        "\
== scene ==
VAR name = \"Alice\"
* Hello {name}[ world.] goodbye.
- -> END
",
    );
    let scene = find_child(&p.root, "scene");

    // Find the ChoiceSet statement
    let choice_set = scene
        .body
        .iter()
        .find_map(|s| match s {
            lir::Stmt::ChoiceSet(cs) => Some(cs),
            _ => None,
        })
        .unwrap();

    let choice = &choice_set.choices[0];

    // Display (start + bracket) should be recognized as a Template
    assert!(
        matches!(
            choice.display_emission.as_ref().map(|e| &e.line),
            Some(lir::RecognizedLine::Template { .. })
        ),
        "display_emission should be Some(Template)"
    );

    // Output (start + inner) should be recognized as a Template
    assert!(
        matches!(
            choice.output_emission.as_ref().map(|e| &e.line),
            Some(lir::RecognizedLine::Template { .. })
        ),
        "output_emission should be Some(Template)"
    );
}

// ─── Nested choices ─────────────────────────────────────────────────

#[test]
fn nested_choices_create_nested_containers() {
    let p = lower_ink(
        "\
== scene ==
* Outer A
  ** Inner A1
     Deep.
  ** Inner A2
     Also deep.
  - Inner gather.
* Outer B
  B body.
- Outer gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let choice_targets = count_kind(scene, lir::ContainerKind::ChoiceTarget);
    assert!(
        choice_targets >= 4,
        "should have at least 4 choice targets (2 outer + 2 inner), got {choice_targets}"
    );
}

#[test]
fn nested_choice_bodies_have_content() {
    let p = lower_ink(
        "\
== scene ==
* Outer
  ** Inner choice
     Inner body text.
  - Inner gather.
- Outer gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let has_inner = collect_kind(scene, lir::ContainerKind::ChoiceTarget)
        .iter()
        .any(|c| {
            collect_text(&c.body)
                .iter()
                .any(|t| t.contains("Inner body"))
        });
    assert!(
        has_inner,
        "nested choice target should have inner body content"
    );
}

// ─── Diverts ────────────────────────────────────────────────────────

#[test]
fn divert_to_done() {
    let p = lower_ink("-> DONE\n");
    let r = root(&p);
    let has_done = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Done)));
    assert!(has_done, "should have a DONE divert");
}

#[test]
fn divert_to_end() {
    let p = lower_ink("-> END\n");
    let r = root(&p);
    let has_end = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::End)));
    assert!(has_end, "should have an END divert");
}

#[test]
fn divert_between_knots() {
    let p = lower_ink(
        "\
== start ==
-> middle

== middle ==
-> finish

== finish ==
The end.
-> END
",
    );
    let start = find_child(&p.root, "start");
    let middle = find_child(&p.root, "middle");

    let start_diverts_to_middle = start.body.iter().any(|s| {
        matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(id) if id == middle.id))
    });
    assert!(start_diverts_to_middle);
}

#[test]
fn divert_to_stitch() {
    let p = lower_ink(
        "\
== tavern ==
-> tavern.order

= order
One ale, please.
-> END
",
    );
    let knot = find_child(&p.root, "tavern");
    let stitch = find_child(knot, "order");

    let diverts_to_stitch = knot.body.iter().any(|s| {
        matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(id) if id == stitch.id))
    });
    assert!(diverts_to_stitch, "knot should divert to its stitch");
}

// ─── Assignments ────────────────────────────────────────────────────

#[test]
fn assignment_to_global() {
    let p = lower_ink("VAR x = 0\n~ x = 5\n");
    let r = root(&p);
    let has_assign = r.body.iter().any(|s| matches!(s, lir::Stmt::Assign { .. }));
    assert!(has_assign, "root should have an assignment statement");
}

#[test]
fn assignment_with_operator() {
    let p = lower_ink("VAR score = 0\n~ score += 10\n");
    let r = root(&p);
    let has_assign = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                op: brink_ir::AssignOp::Add,
                ..
            }
        )
    });
    assert!(has_assign, "should have += assignment");
}

// ─── Expressions ────────────────────────────────────────────────────

#[test]
fn interpolation_in_content() {
    let p = lower_ink("VAR name = \"world\"\nHello {name}!\n");
    let r = root(&p);
    // Interpolations are now recognized as templates (phase 3).
    let has_template = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "content with interpolation should be recognized as Template"
    );
}

#[test]
fn infix_expression_in_assignment() {
    let p = lower_ink("VAR x = 0\n~ x = 2 + 3\n");
    let r = root(&p);
    let has_infix = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::Infix(_, brink_ir::InfixOp::Add, _),
                ..
            }
        )
    });
    assert!(has_infix, "assignment should have infix Add expression");
}

#[test]
fn prefix_negate() {
    let p = lower_ink("VAR x = 0\n~ x = -x\n");
    let r = root(&p);
    let has_prefix = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::Prefix(brink_ir::PrefixOp::Negate, _),
                ..
            }
        )
    });
    assert!(has_prefix, "assignment should have prefix negate");
}

#[test]
fn boolean_not() {
    let p = lower_ink("VAR flag = true\n~ flag = not flag\n");
    let r = root(&p);
    let has_not = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::Prefix(brink_ir::PrefixOp::Not, _),
                ..
            }
        )
    });
    assert!(has_not, "assignment should have prefix not");
}

// ─── Conditionals ───────────────────────────────────────────────────

#[test]
fn block_conditional() {
    let p = lower_ink(
        "\
VAR x = true
{
    - x:
        Yes.
    - else:
        No.
}
",
    );
    let r = root(&p);
    let has_cond = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(has_cond, "should have a Conditional statement");
}

#[test]
fn conditional_branch_count() {
    let p = lower_ink(
        "\
VAR x = 1
{
    - x == 1:
        One.
    - x == 2:
        Two.
    - else:
        Other.
}
",
    );
    let r = root(&p);
    let cond = r.body.iter().find_map(|s| {
        if let lir::Stmt::Conditional(c) = s {
            Some(c)
        } else {
            None
        }
    });
    assert!(cond.is_some());
    assert_eq!(cond.unwrap().branches.len(), 3, "should have 3 branches");
}

#[test]
fn conditional_else_has_no_condition() {
    let p = lower_ink(
        "\
VAR x = 1
{
    - x == 1:
        One.
    - else:
        Other.
}
",
    );
    let r = root(&p);
    let cond = r.body.iter().find_map(|s| {
        if let lir::Stmt::Conditional(c) = s {
            Some(c)
        } else {
            None
        }
    });
    let cond = cond.unwrap();
    assert!(
        cond.branches.last().unwrap().condition.is_none(),
        "else branch should have no condition"
    );
}

// ─── Sequences ──────────────────────────────────────────────────────

#[test]
fn stopping_sequence() {
    let p = lower_ink(
        "\
{stopping:
    - First time.
    - Every other time.
}
",
    );
    let r = root(&p);
    // Root body now has EnterContainer pointing at a sequence wrapper child.
    let has_enter = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::EnterContainer(_)));
    assert!(has_enter, "root should have EnterContainer for sequence");

    let seq_child = r
        .children
        .iter()
        .find(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(
        seq_child.is_some(),
        "root should have a Sequence child container"
    );
    let seq_child = seq_child.unwrap();

    let has_seq = seq_child.body.iter().any(
        |s| matches!(s, lir::Stmt::Sequence(seq) if seq.kind == brink_ir::SequenceType::STOPPING),
    );
    assert!(
        has_seq,
        "sequence container should have a Stopping sequence"
    );
}

#[test]
fn cycle_sequence() {
    let p = lower_ink(
        "\
{cycle:
    - A.
    - B.
    - C.
}
",
    );
    let r = root(&p);
    let seq_child = r
        .children
        .iter()
        .find(|c| c.kind == lir::ContainerKind::Sequence)
        .expect("root should have a Sequence child container");

    let seq = seq_child.body.iter().find_map(|s| {
        if let lir::Stmt::Sequence(s) = s {
            Some(s)
        } else {
            None
        }
    });
    assert!(seq.is_some());
    let seq = seq.unwrap();
    assert_eq!(seq.kind, brink_ir::SequenceType::CYCLE);
    assert_eq!(seq.branches.len(), 3);
}

// ─── Inline content elements ────────────────────────────────────────

#[test]
fn inline_conditional_in_content() {
    // After normalization, the inline conditional is lifted to a block-level
    // Conditional with recognized content in each branch.
    let p = lower_ink("VAR happy = true\nI'm {happy:very|not} pleased.\n");
    let r = root(&p);
    let has_block_cond = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(
        has_block_cond,
        "inline conditional should be lifted to block-level Conditional"
    );

    // Each branch should have recognized content (EmitLine or EmitContent).
    let cond = r
        .body
        .iter()
        .find_map(|s| {
            if let lir::Stmt::Conditional(c) = s {
                Some(c)
            } else {
                None
            }
        })
        .expect("should have Conditional");
    assert_eq!(cond.branches.len(), 2);
}

#[test]
fn inline_sequence_lifted_produces_recognized_lines() {
    let p = lower_ink("{stopping:a fine|a good} day\n");
    let r = root(&p);
    // After normalization, should be a block-level Sequence.
    let has_seq = r
        .children
        .iter()
        .any(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(
        has_seq,
        "inline sequence should be lifted to a Sequence container"
    );
}

#[test]
fn inline_conditional_lifted_produces_recognized_lines() {
    let p = lower_ink("VAR f = true\n{f:Hello|Hi} world\n");
    let r = root(&p);
    let has_cond = r
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(
        has_cond,
        "inline conditional should be lifted to block-level Conditional"
    );
}

#[test]
fn inline_sequence_with_interpolation() {
    let p = lower_ink("VAR n = \"x\"\n{&Hello {n}|Hi {n}}\n");
    let r = root(&p);
    // After lift, the sequence branches should contain content with interpolation.
    let has_seq = r
        .children
        .iter()
        .any(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(
        has_seq,
        "inline sequence with interpolation should be lifted"
    );
}

#[test]
fn cartesian_two_sequences() {
    let p = lower_ink("{a|b} and {x|y}\n");
    let r = root(&p);
    // After normalization: outer Sequence wrapping inner Sequences.
    // The outer should be a Sequence container.
    let seq_count = r
        .children
        .iter()
        .filter(|c| c.kind == lir::ContainerKind::Sequence)
        .count();
    assert!(
        seq_count >= 1,
        "cartesian product should produce nested Sequence containers, found {seq_count}"
    );
}

#[test]
fn empty_branch_preserves_surrounding() {
    let p = lower_ink("{a||c} fine\n");
    let r = root(&p);
    // Should have a Sequence with 3 branches, middle branch gets " fine" only.
    let has_seq = r
        .children
        .iter()
        .any(|c| c.kind == lir::ContainerKind::Sequence);
    assert!(has_seq, "empty branch sequence should be lifted");
}

#[test]
fn complex_branch_with_divert() {
    let p = lower_ink(
        "\
== start ==
It's {stopping:
    - a fine
    - a good -> END
} day.
-> END
",
    );
    // This exercises normalization of block-level sequences that may
    // already exist (the multiline {stopping:} is already block-level
    // in HIR). The suffix " day." should still appear after the sequence.
    let start = find_child(&p.root, "start");
    // Just verify it compiles and has structure.
    assert!(
        !start.body.is_empty() || !start.children.is_empty(),
        "start container should have content"
    );
}

#[test]
fn glue_in_content() {
    let p = lower_ink("Hello<>\n, world!\n");
    let r = root(&p);
    let has_glue = r.body.iter().any(|s| {
        if let lir::Stmt::EmitContent(c) = s {
            c.parts.iter().any(|p| matches!(p, lir::ContentPart::Glue))
        } else {
            false
        }
    });
    assert!(has_glue, "content should have Glue element");
}

// ─── Builtin functions ──────────────────────────────────────────────

#[test]
fn builtin_random_recognized() {
    let p = lower_ink("VAR x = 0\n~ x = RANDOM(1, 10)\n");
    let r = root(&p);
    let has_builtin = r.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::CallBuiltin {
                    builtin: lir::BuiltinFn::Random,
                    ..
                },
                ..
            }
        )
    });
    assert!(has_builtin, "RANDOM should be recognized as builtin");
}

#[test]
fn builtin_turns_since() {
    let p = lower_ink(
        "\
VAR t = 0
== scene ==
~ t = TURNS_SINCE(-> scene)
-> END
",
    );
    let knot = find_child(&p.root, "scene");
    let has_turns = knot.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::CallBuiltin {
                    builtin: lir::BuiltinFn::TurnsSince,
                    ..
                },
                ..
            }
        )
    });
    assert!(has_turns, "TURNS_SINCE should be recognized as builtin");
}

// ─── Counting flags ─────────────────────────────────────────────────

#[test]
fn knots_have_empty_counting_flags_by_default() {
    let p = lower_ink("== greet ==\nHi.\n-> END\n");
    let knot = find_child(&p.root, "greet");
    assert!(
        knot.counting_flags.is_empty(),
        "knots should have empty counting flags by default (VISITS added only when referenced)"
    );
}

#[test]
fn visit_count_reference_sets_flag() {
    let p = lower_ink(
        "\
== scene ==
-> END

== check ==
{scene > 0: Already visited.}
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    assert!(
        scene
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "referenced container should have VISITS flag"
    );
}

#[test]
fn variable_divert_target_gets_visit_flags() {
    let program = lower_ink(
        "\
VAR x = -> here
-> there
== there ==
-> x
== here ==
Here.
-> DONE
",
    );
    let here = find_by_path(&program, "here");
    assert!(
        here.counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "container targeted by variable divert must have VISITS flag"
    );
    assert!(
        here.counting_flags
            .contains(brink_format::CountingFlags::TURNS),
        "container targeted by variable divert must have TURNS flag"
    );
}

#[test]
fn variable_tunnel_target_gets_visit_flags() {
    let program = lower_ink(
        "\
VAR x = -> tunnel
-> x ->
== tunnel ==
->->
",
    );
    let tunnel = find_by_path(&program, "tunnel");
    assert!(
        tunnel
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "container targeted by variable tunnel must have VISITS flag"
    );
}

#[test]
fn divert_target_expr_gets_visit_flags() {
    let program = lower_ink(
        "\
~ temp x = -> target
-> x
== target ==
Done.
-> DONE
",
    );
    let target = find_by_path(&program, "target");
    assert!(
        target
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "container whose address is taken in an expr must have VISITS flag"
    );
}

#[test]
fn labeled_gather_with_visits_gets_count_start_only() {
    let program = lower_ink(
        "\
== scene ==
- (loop)
{loop} times.
{loop < 3: -> loop}
-> DONE
",
    );
    let scene = find_by_path(&program, "scene");
    // Find the gather container with the label
    let gather = scene
        .children
        .iter()
        .find(|c| c.labeled)
        .expect("should have a labeled gather child");
    assert!(
        gather
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS),
        "labeled gather referenced by visit count should have VISITS"
    );
    assert!(
        gather
            .counting_flags
            .contains(brink_format::CountingFlags::COUNT_START_ONLY),
        "labeled gather with VISITS should have COUNT_START_ONLY for self-goto loops"
    );
}

// ─── Container counts and structure ─────────────────────────────────

#[test]
fn empty_program_has_only_root() {
    let p = lower_ink("");
    assert_eq!(count_all(&p.root), 1);
    assert_eq!(p.root.kind, lir::ContainerKind::Root);
}

#[test]
fn name_table_contains_definitions() {
    let p = lower_ink("VAR score = 0\nLIST colors = red, green\n");
    assert!(
        p.name_table.iter().any(|n| n == "score"),
        "name table should contain 'score'"
    );
    assert!(
        p.name_table.iter().any(|n| n == "colors"),
        "name table should contain 'colors'"
    );
}

#[test]
fn container_count_knots_stitches() {
    let p = lower_ink(
        "\
Start.
-> knot_a

== knot_a ==
= stitch_1
One.
-> END
= stitch_2
Two.
-> END

== knot_b ==
Three.
-> END
",
    );
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Root), 1);
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 2);
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Stitch), 2);
}

// ─── Knot parameters ───────────────────────────────────────────────

#[test]
fn knot_with_params() {
    let p = lower_ink(
        "\
== greet(name) ==
Hello.
-> END
",
    );
    let knot = find_child(&p.root, "greet");
    assert_eq!(knot.params.len(), 1);
    assert_eq!(knot.params[0].slot, 0);
    assert!(!knot.params[0].is_ref);
}

#[test]
fn knot_with_ref_param() {
    let p = lower_ink(
        "\
== modify(ref x) ==
~ x = 10
-> END
",
    );
    let knot = find_child(&p.root, "modify");
    assert_eq!(knot.params.len(), 1);
    assert!(knot.params[0].is_ref);
}

// ─── Tunnel calls ───────────────────────────────────────────────────

#[test]
fn tunnel_call_statement() {
    let p = lower_ink(
        "\
== start ==
-> helper ->
Done.
-> END

== helper ==
Helping.
->->
",
    );
    let start = find_child(&p.root, "start");
    let has_tunnel = start
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::TunnelCall(_)));
    assert!(has_tunnel, "should have a TunnelCall statement");
}

// ─── Thread starts ──────────────────────────────────────────────────

#[test]
fn thread_start_statement() {
    let p = lower_ink(
        "\
== main ==
<- background
Main content.
-> END

== background ==
Background.
-> DONE
",
    );
    let knot = find_child(&p.root, "main");
    let has_thread = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::ThreadStart(_)));
    assert!(has_thread, "should have a ThreadStart statement");
}

// ─── Return statement ───────────────────────────────────────────────

#[test]
fn return_from_function() {
    let p = lower_ink(
        "\
== function double(x) ==
~ return x * 2
",
    );
    let knot = find_child(&p.root, "double");
    let has_return = knot
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Return { value: Some(_), .. }));
    assert!(has_return, "function should have a Return statement");
}

// ─── Tags ───────────────────────────────────────────────────────────

#[test]
fn content_tags() {
    let p = lower_ink("Hello. # greeting # friendly\n");
    let r = root(&p);
    let tag_sets: Vec<&Vec<Vec<lir::ContentPart>>> = r
        .body
        .iter()
        .filter_map(|s| match s {
            lir::Stmt::EmitContent(c) if !c.tags.is_empty() => Some(&c.tags),
            lir::Stmt::EmitLine(e) if !e.tags.is_empty() => Some(&e.tags),
            _ => None,
        })
        .collect();
    assert!(!tag_sets.is_empty(), "content should have tags");
    // Extract text from each tag's parts
    let tag_texts: Vec<String> = tag_sets
        .iter()
        .flat_map(|tags| {
            tags.iter().map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| {
                        if let lir::ContentPart::Text(t) = p {
                            Some(t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<String>()
            })
        })
        .collect();
    assert!(tag_texts.iter().any(|t| t.contains("greeting")));
    assert!(tag_texts.iter().any(|t| t.contains("friendly")));
}

// ─── Complex integration scenarios ──────────────────────────────────

#[test]
fn full_story_structure() {
    let p = lower_ink(
        "\
VAR visited_inn = false

-> town_square

== town_square ==
You stand in the town square.
* [Go to the inn] -> inn
* [Go to the market] -> market

== inn ==
~ visited_inn = true
The inn is warm and cozy.
* Order a drink
  You order an ale.
* Sit by the fire
  The fire crackles.
- The innkeeper nods.
-> town_square

== market ==
{visited_inn: The innkeeper waves from across the square.}
Stalls line the street.
-> END
",
    );

    // Structural assertions
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Root), 1);
    assert_eq!(count_kind(&p.root, lir::ContainerKind::Knot), 3);
    assert!(count_kind(&p.root, lir::ContainerKind::ChoiceTarget) >= 4);
    assert!(count_kind(&p.root, lir::ContainerKind::Gather) >= 1);

    // Globals
    assert_eq!(p.globals.len(), 1);
    let visited = find_global(&p, "visited_inn");
    assert!(matches!(visited.default, lir::ConstValue::Bool(false)));
    assert!(visited.mutable);

    // Root diverts to town_square
    let r = root(&p);
    let town = find_child(&p.root, "town_square");
    let root_diverts = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::Address(id) if id == town.id))
    });
    assert!(root_diverts, "root should divert to town_square");

    // Inn has assignment to visited_inn
    let inn = find_child(&p.root, "inn");
    let has_assign = inn
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Assign { .. }));
    assert!(has_assign, "inn should assign visited_inn = true");

    // Market has a block-level conditional (inline conditional was lifted by normalization)
    let market = find_child(&p.root, "market");
    let has_cond = market
        .body
        .iter()
        .any(|s| matches!(s, lir::Stmt::Conditional(_)));
    assert!(
        has_cond,
        "market should have block-level conditional for visited_inn"
    );
}

#[test]
fn multiple_choice_sets_cascade_gathers() {
    let p = lower_ink(
        "\
== scene ==
* A
  A body.
* B
  B body.
- First gather.
* C
  C body.
* D
  D body.
- Second gather.
-> END
",
    );
    let scene = find_child(&p.root, "scene");
    let gather_count = count_kind(scene, lir::ContainerKind::Gather);
    assert!(
        gather_count >= 2,
        "should have at least 2 gathers, got {gather_count}"
    );

    // One gather should contain -> END
    let gathers = collect_kind(scene, lir::ContainerKind::Gather);
    let any_gather_has_end = gathers.iter().any(|g| {
        g.body.iter().any(
            |s| matches!(s, lir::Stmt::Divert(d) if matches!(d.target, lir::DivertTarget::End)),
        )
    });
    assert!(
        any_gather_has_end,
        "one gather should contain the -> END divert"
    );
}

#[test]
fn list_variable_default_references_items() {
    let p = lower_ink("LIST mood = (happy), sad, (excited)\n");
    assert_eq!(p.lists.len(), 1);
    assert_eq!(p.list_items.len(), 3);

    let ordinals: Vec<i32> = p.list_items.iter().map(|i| i.ordinal).collect();
    assert_eq!(ordinals, vec![1, 2, 3]);
}

#[test]
fn divert_with_arguments() {
    let p = lower_ink("-> greet(42)\n\n== greet(name) ==\nHello.\n-> END\n");
    let r = root(&p);
    let divert = r.body.iter().find_map(|s| {
        if let lir::Stmt::Divert(d) = s
            && matches!(d.target, lir::DivertTarget::Address(_))
        {
            return Some(d);
        }
        None
    });
    assert!(divert.is_some(), "should have a divert with args");
    assert!(
        !divert.unwrap().args.is_empty(),
        "divert should have arguments"
    );
}

#[test]
fn expr_statement() {
    let p = lower_ink(
        "\
EXTERNAL do_something()
~ do_something()
",
    );
    let r = root(&p);
    let has_expr_stmt = r.body.iter().any(|s| matches!(s, lir::Stmt::ExprStmt(_)));
    assert!(
        has_expr_stmt,
        "should have an ExprStmt for the function call"
    );
}

#[test]
fn choice_body_content_in_conditional_branch() {
    let program = lower_ink(
        "\
== scene(x) ==
{true:
    + A choice
        Body content.
        -> END
}
->->
",
    );
    let scene = find_by_path(&program, "scene");
    let choice_target = scene
        .children
        .iter()
        .flat_map(|c| std::iter::once(c).chain(c.children.iter()))
        .find(|c| c.kind == lir::ContainerKind::ChoiceTarget)
        .expect("should have a choice target");
    // Choice target should have body content (not just the choice output)
    let has_end_divert = choice_target.body.iter().any(|s| {
        if let lir::Stmt::Divert(d) = s {
            matches!(d.target, lir::DivertTarget::End)
        } else {
            false
        }
    });
    assert!(
        has_end_divert,
        "choice target should contain -> END from the choice body"
    );
}

// ─── READ_COUNT builtin ─────────────────────────────────────────────

#[test]
fn builtin_read_count_with_divert_target() {
    // READ_COUNT(-> knot) should lower to CallBuiltin { builtin: ReadCount, args: [DivertTarget] }
    let p = lower_ink(
        "\
VAR t = 0
== knot ==
~ t = READ_COUNT(-> knot)
-> END
",
    );
    let knot = find_child(&p.root, "knot");
    let has_read_count = knot.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::CallBuiltin {
                    builtin: lir::BuiltinFn::ReadCount,
                    ..
                },
                ..
            }
        )
    });
    assert!(
        has_read_count,
        "READ_COUNT(-> knot) should be recognized as a ReadCount builtin, not null"
    );
}

#[test]
fn builtin_read_count_with_variable() {
    // READ_COUNT(x) where x is a variable holding a divert target
    // should lower to CallBuiltin { builtin: ReadCount, args: [GetGlobal] }
    let p = lower_ink(
        "\
VAR x = -> knot
VAR t = 0
== knot ==
~ t = READ_COUNT(x)
-> END
",
    );
    let knot = find_child(&p.root, "knot");
    let has_read_count = knot.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::CallBuiltin {
                    builtin: lir::BuiltinFn::ReadCount,
                    ..
                },
                ..
            }
        )
    });
    assert!(
        has_read_count,
        "READ_COUNT(x) should be recognized as a ReadCount builtin, not null"
    );
}

// ─── Call through variable ──────────────────────────────────────────

#[test]
fn call_through_global_variable() {
    let prog = lower_ink(
        "\
VAR s = -> knot
~ s()

== function knot ==
~ return 1
",
    );

    // The root body should contain an ExprStmt with CallVariable
    let has_call_var = root(&prog)
        .body
        .iter()
        .any(|stmt| matches!(stmt, lir::Stmt::ExprStmt(lir::Expr::CallVariable { .. })));
    assert!(
        has_call_var,
        "call through global variable should produce CallVariable"
    );
}

#[test]
fn call_through_temp_variable() {
    let prog = lower_ink(
        "\
== function run ==
~ temp s = -> helper
~ return s()

== function helper ==
~ return 42
",
    );

    let run = find_by_path(&prog, "run");
    let has_call_var_temp = run.body.iter().any(|stmt| {
        matches!(
            stmt,
            lir::Stmt::Return {
                value: Some(lir::Expr::CallVariableTemp { .. }),
                ..
            }
        )
    });
    assert!(
        has_call_var_temp,
        "call through temp variable should produce CallVariableTemp"
    );
}

// ─── Pattern recognizer tests ───────────────────────────────────────

#[test]
fn plain_text_recognized() {
    let program = lower_ink("Hello, world!\n");
    let body = &root(&program).body;
    assert!(
        matches!(&body[0], lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello, world!")),
        "plain text should be recognized as EmitLine(Plain(...))"
    );
}

#[test]
fn plain_text_source_hash() {
    let program = lower_ink("Hello\n");
    let body = &root(&program).body;
    if let lir::Stmt::EmitLine(emission) = &body[0] {
        assert_eq!(
            emission.metadata.source_hash,
            brink_format::content_hash("Hello"),
            "source_hash should match content_hash of the text"
        );
    } else {
        panic!(
            "expected EmitLine, got {:?}",
            std::mem::discriminant(&body[0])
        );
    }
}

#[test]
fn plain_text_with_tag_recognized() {
    let program = lower_ink("Hello #tag\n");
    let body = &root(&program).body;
    if let lir::Stmt::EmitLine(emission) = &body[0] {
        assert!(
            matches!(&emission.line, lir::RecognizedLine::Plain(s) if s == "Hello "),
            "text before tag should be plain"
        );
        assert_eq!(emission.tags.len(), 1, "should have one tag");
    } else {
        panic!("expected EmitLine for plain text with tag");
    }
}

fn find_template(body: &[lir::Stmt]) -> Option<(Vec<brink_format::LinePart>, usize)> {
    body.iter().find_map(|s| {
        if let lir::Stmt::EmitLine(e) = s
            && let lir::RecognizedLine::Template { parts, slot_exprs } = &e.line
        {
            return Some((parts.clone(), slot_exprs.len()));
        }
        None
    })
}

#[test]
fn interpolation_recognized_as_template() {
    let program = lower_ink("VAR name = \"world\"\nHello, {name}!\n");
    let body = &root(&program).body;
    let (parts, slot_count) = find_template(body).expect("should be recognized as Template");
    assert_eq!(slot_count, 1, "one slot expression");
    assert_eq!(parts.len(), 3, "literal + slot + literal");
    assert!(matches!(&parts[0], brink_format::LinePart::Literal(s) if s == "Hello, "));
    assert!(matches!(&parts[1], brink_format::LinePart::Slot(0)));
    assert!(matches!(&parts[2], brink_format::LinePart::Literal(s) if s == "!"));
}

#[test]
fn multiple_interpolations_recognized() {
    let program = lower_ink("VAR x = 1\nVAR y = 2\n{x} and {y}\n");
    let body = &root(&program).body;
    let (parts, slot_count) =
        find_template(body).expect("multiple interpolations should be recognized as Template");
    assert_eq!(slot_count, 2, "two slot expressions");
    assert!(matches!(&parts[0], brink_format::LinePart::Slot(0)));
    assert!(matches!(&parts[1], brink_format::LinePart::Literal(s) if s == " and "));
    assert!(matches!(&parts[2], brink_format::LinePart::Slot(1)));
}

#[test]
fn interpolation_only_not_recognized_as_template() {
    // Single interpolation with no surrounding text should NOT be a Template —
    // it falls through to EmitContent, which uses emit_value (correctly
    // suppresses null/void results).
    let program = lower_ink("VAR x = 1\n{x}\n");
    let body = &root(&program).body;
    let has_template = find_template(body).is_some();
    assert!(
        !has_template,
        "slot-only content {{x}} should NOT be recognized as Template"
    );
    // Should be EmitContent instead.
    let has_emit_content = body.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)));
    assert!(
        has_emit_content,
        "slot-only content should fall through to EmitContent"
    );
}

#[test]
fn glue_not_recognized() {
    let program = lower_ink("Hello<>\n");
    let body = &root(&program).body;
    let has_emit_content = body.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)));
    assert!(
        has_emit_content,
        "content with glue should fall back to EmitContent"
    );
}

#[test]
fn glue_with_interpolation_not_recognized() {
    let program = lower_ink("VAR x = 1\nHello<>{x}\n");
    let body = &root(&program).body;
    let has_emit_content = body.iter().any(|s| matches!(s, lir::Stmt::EmitContent(_)));
    assert!(
        has_emit_content,
        "content with glue and interpolation should fall back to EmitContent"
    );
}

#[test]
fn multiple_plain_lines() {
    let program = lower_ink("Line one\nLine two\n");
    let body = &root(&program).body;
    let emit_lines: Vec<_> = body
        .iter()
        .filter(|s| matches!(s, lir::Stmt::EmitLine(_)))
        .collect();
    assert_eq!(
        emit_lines.len(),
        2,
        "two plain text lines should both be recognized"
    );
}

#[test]
fn collect_text_includes_recognized() {
    let program = lower_ink("Hello, world!\n");
    let texts = collect_text(&root(&program).body);
    assert_eq!(texts, vec!["Hello, world!"]);
}

// ─── Glue stripping recognition tests ──────────────────────────────

#[test]
fn glue_leading_recognized_as_plain() {
    // `<>Hello world` — leading glue should be stripped, interior recognized as Plain.
    let program = lower_ink("<>Hello world\n");
    let body = &root(&program).body;

    // Should be: EmitContent(Glue), EmitLine(Plain("Hello world")), EndOfLine
    let mut found_glue = false;
    let mut found_line = false;
    for stmt in body {
        match stmt {
            lir::Stmt::EmitContent(c)
                if c.parts.len() == 1 && matches!(c.parts[0], lir::ContentPart::Glue) =>
            {
                found_glue = true;
            }
            lir::Stmt::EmitLine(e) => {
                assert!(
                    matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello world"),
                    "expected Plain(\"Hello world\")"
                );
                found_line = true;
            }
            _ => {}
        }
    }
    assert!(found_glue, "should emit a Glue statement");
    assert!(found_line, "should emit an EmitLine statement");
}

#[test]
fn glue_trailing_recognized_as_plain() {
    // `Hello world<>` — trailing glue should be stripped, interior recognized as Plain.
    let program = lower_ink("Hello world<>\n");
    let body = &root(&program).body;

    let mut found_line = false;
    let mut found_trailing_glue = false;
    let mut line_pos = None;
    let mut glue_pos = None;
    for (i, stmt) in body.iter().enumerate() {
        match stmt {
            lir::Stmt::EmitLine(e) => {
                assert!(
                    matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello world"),
                    "expected Plain(\"Hello world\")"
                );
                found_line = true;
                line_pos = Some(i);
            }
            lir::Stmt::EmitContent(c)
                if c.parts.len() == 1 && matches!(c.parts[0], lir::ContentPart::Glue) =>
            {
                found_trailing_glue = true;
                glue_pos = Some(i);
            }
            _ => {}
        }
    }
    assert!(found_line, "should emit an EmitLine statement");
    assert!(found_trailing_glue, "should emit a trailing Glue statement");
    assert!(
        line_pos.unwrap() < glue_pos.unwrap(),
        "EmitLine should come before trailing Glue"
    );
}

#[test]
fn glue_both_ends_recognized_as_template() {
    // `<>Hello {x}<>` — both glues stripped, interior recognized as Template.
    let program = lower_ink("VAR x = 1\n<>Hello {x}<>\n");
    let body = &root(&program).body;

    let mut glue_count = 0;
    let mut found_template = false;
    for stmt in body {
        match stmt {
            lir::Stmt::EmitContent(c)
                if c.parts.len() == 1 && matches!(c.parts[0], lir::ContentPart::Glue) =>
            {
                glue_count += 1;
            }
            lir::Stmt::EmitLine(e) => {
                assert!(
                    matches!(&e.line, lir::RecognizedLine::Template { .. }),
                    "expected Template"
                );
                found_template = true;
            }
            _ => {}
        }
    }
    assert!(
        found_template,
        "should emit an EmitLine(Template) statement"
    );
    assert_eq!(
        glue_count, 2,
        "should emit two Glue statements (leading + trailing)"
    );
}

#[test]
fn interior_text_glue_text_merged() {
    // `Hello<>world` — interior glue between two text parts merges them.
    let program = lower_ink("Hello<>world\n");
    let body = &root(&program).body;

    let found_line = body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Helloworld"))
    });
    assert!(
        found_line,
        "interior Text-Glue-Text should merge into Plain(\"Helloworld\")"
    );
}

#[test]
fn no_glue_plain_still_works() {
    // Plain `Hello world` — no regression, still recognized as Plain.
    let program = lower_ink("Hello world\n");
    let body = &root(&program).body;

    let found_line = body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Plain(s) if s == "Hello world"))
    });
    assert!(found_line, "plain text should still be recognized as Plain");
}

// ─── Const folding for binary expressions ───────────────────────────

#[test]
fn const_fold_int_addition() {
    let program = lower_ink("VAR x = 2 + 3\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(5));
}

#[test]
fn const_fold_int_subtraction() {
    let program = lower_ink("VAR x = 10 - 4\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(6));
}

#[test]
fn const_fold_int_multiplication() {
    let program = lower_ink("VAR x = 3 * 7\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(21));
}

#[test]
fn const_fold_int_division() {
    let program = lower_ink("VAR x = 20 / 4\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(5));
}

#[test]
fn const_fold_int_modulo() {
    let program = lower_ink("VAR x = 7 % 3\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(1));
}

#[test]
fn const_fold_comparison_eq() {
    let program = lower_ink("VAR x = 5 == 5\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(true));
}

#[test]
fn const_fold_comparison_lt() {
    let program = lower_ink("VAR x = 3 < 5\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(true));
}

#[test]
fn const_fold_logical_and() {
    let program = lower_ink("VAR x = true && false\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(false));
}

#[test]
fn const_fold_logical_or() {
    let program = lower_ink("VAR x = false || true\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Bool(true));
}

#[test]
fn const_fold_string_concatenation() {
    let program = lower_ink("VAR x = \"hello\" + \" world\"\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::String("hello world".into()));
}

#[test]
fn const_fold_nested_arithmetic() {
    // (2 + 3) * 4 — depends on parser precedence, but the key test is
    // that nested infix expressions are recursively folded.
    let program = lower_ink("VAR x = 2 + 3 * 4\n{x}\n");
    let g = find_global(&program, "x");
    // 3 * 4 = 12, 2 + 12 = 14 (standard precedence)
    assert_eq!(g.default, lir::ConstValue::Int(14));
}

#[test]
fn const_fold_const_reference_in_binary() {
    let program = lower_ink("CONST a = 10\nVAR x = a + 5\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Int(15));
}

#[test]
fn const_fold_division_by_zero_yields_null() {
    let program = lower_ink("VAR x = 10 / 0\n{x}\n");
    let g = find_global(&program, "x");
    assert_eq!(g.default, lir::ConstValue::Null);
}

// ─── AUTHOR_WARNING handling ────────────────────────────────────────

#[test]
fn author_warning_does_not_panic() {
    // TODO: author warning — should be silently skipped without hitting
    // the debug_assert in lower_body_children.
    let program = lower_ink("TODO: fix this later\nHello\n");
    let body = &root(&program).body;
    // The TODO line is skipped, but "Hello" content should still be present.
    let texts = collect_text(body);
    assert!(
        texts.iter().any(|t| t.contains("Hello")),
        "content after AUTHOR_WARNING should be preserved"
    );
}

// ─── String interpolation in const context ──────────────────────────

#[test]
fn string_interpolation_in_const_emits_e030() {
    let source = "VAR name = \"world\"\nCONST greeting = \"hello {name}\"\n{greeting}\n";
    let (_program, warnings) = lower_ink_with_warnings(source);
    assert!(
        warnings
            .iter()
            .any(|w| w.code == brink_ir::DiagnosticCode::E030),
        "expected E030 warning for string interpolation in const, got: {warnings:?}"
    );
}

// ─── Template recognition: slot-only and whitespace-only ─────────────
//
// Templates should only be created when there is non-whitespace source
// text that corresponds to output content. Slot-only lines and lines
// where the only text between slots is whitespace should fall through
// to EmitContent, which uses emit_value (correctly suppresses null).

#[test]
fn slot_only_content_not_recognized_as_template() {
    // `{name}` — single interpolation, no surrounding text.
    // Should be EmitContent, NOT EmitLine(Template).
    let p = lower_ink("VAR name = \"world\"\n{name}\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        !has_template,
        "slot-only content `{{name}}` should NOT be recognized as Template; \
         should fall through to EmitContent",
    );
}

#[test]
fn whitespace_only_text_between_slots_not_recognized_as_template() {
    // `{x} {y}` — two interpolations with only whitespace between them.
    // No non-whitespace source text → should NOT be a template.
    let p = lower_ink("VAR x = 1\nVAR y = 2\n{x} {y}\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        !has_template,
        "content with only whitespace between slots should NOT be a Template",
    );
}

#[test]
fn text_with_interpolation_recognized_as_template() {
    // `Hello {name}!` — has non-whitespace text around the slot.
    // Should be recognized as a Template.
    let p = lower_ink("VAR name = \"world\"\nHello {name}!\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "content with non-whitespace text + interpolation should be Template",
    );
}

// ─── Container DefinitionId uniqueness ───────────────────────────────
//
// Every container in the LIR must have a unique DefinitionId. Collisions
// cause the linker to map multiple containers to the same ID, and the
// last-write-wins HashMap behavior silently resolves to the wrong
// container at runtime.

/// Collect all container `DefinitionId`s recursively.
fn collect_ids(container: &lir::Container, out: &mut Vec<(brink_format::DefinitionId, String)>) {
    let name = container.name.as_deref().unwrap_or("(anon)");
    out.push((container.id, name.to_string()));
    for child in &container.children {
        collect_ids(child, out);
    }
}

#[test]
fn no_definition_id_collisions_in_simple_story() {
    // Two gathers at the same scope, each containing a conditional.
    // Each conditional's branches must get unique IDs.
    let p = lower_ink(
        "\
=== start ===
* [A] -> gather_a
* [B] -> gather_b
- (gather_a)
  { true:
    branch a1
  - else:
    branch a2
  }
  -> DONE
- (gather_b)
  { true:
    branch b1
  - else:
    branch b2
  }
  -> DONE
",
    );

    let mut ids = Vec::new();
    collect_ids(&p.root, &mut ids);

    // Check for duplicates
    let mut seen: std::collections::HashMap<brink_format::DefinitionId, Vec<&str>> =
        std::collections::HashMap::new();
    let mut collisions = Vec::new();
    for (id, name) in &ids {
        seen.entry(*id).or_default().push(name.as_str());
    }
    for (id, names) in &seen {
        if names.len() > 1 {
            collisions.push(format!("{id:?} -> {names:?}"));
        }
    }
    assert!(
        collisions.is_empty(),
        "DefinitionId collisions found: {collisions:#?}",
    );
}

#[test]
fn no_definition_id_collisions_in_intercept_pattern() {
    // The TheIntercept pattern: nested choice sets with conditionals
    // at multiple gather points.
    let p = lower_ink(
        "\
VAR teacup = false
=== start ===
- greeting
    * [Take cup]
        ~ teacup = true
        took cup
    * [Leave it]
        left it
- middle text
    * [Agree]
        reply A
    * [Disagree]
        reply B
- { teacup:
    <>, with teacup
  }
  <>.
-
    * [Watch]
        watching
    * [Wait]
        waiting
- done
",
    );

    let mut ids = Vec::new();
    collect_ids(&p.root, &mut ids);

    let mut seen: std::collections::HashMap<brink_format::DefinitionId, Vec<&str>> =
        std::collections::HashMap::new();
    let mut collisions = Vec::new();
    for (id, name) in &ids {
        seen.entry(*id).or_default().push(name.as_str());
    }
    for (id, names) in &seen {
        if names.len() > 1 {
            collisions.push(format!("{id:?} -> {names:?}"));
        }
    }
    assert!(
        collisions.is_empty(),
        "DefinitionId collisions found: {collisions:#?}",
    );
}

#[test]
fn multiple_slots_with_real_text_recognized_as_template() {
    // `{x} and {y}` — has "and" (non-whitespace) between slots.
    // Should be recognized as a Template.
    let p = lower_ink("VAR x = 1\nVAR y = 2\n{x} and {y}\n");
    let r = root(&p);
    let has_template = r.body.iter().any(|s| {
        matches!(s, lir::Stmt::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "content with non-whitespace text between slots should be Template",
    );
}

// ─── Glue in choice body before gather ──────────────────────────────

/// Helper: check if a statement list contains a Glue emission.
fn has_glue(stmts: &[lir::Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        lir::Stmt::EmitContent(c) => c.parts.iter().any(|p| matches!(p, lir::ContentPart::Glue)),
        _ => false,
    })
}

/// Helper: recursively dump container tree structure for debugging.
fn dump_container(container: &lir::Container, indent: usize) {
    let pad = " ".repeat(indent);
    eprintln!(
        "{pad}[{:?}] {:?} ({} stmts, {} children)",
        container.kind,
        container.name,
        container.body.len(),
        container.children.len(),
    );
    for (i, stmt) in container.body.iter().enumerate() {
        match stmt {
            lir::Stmt::EmitContent(c) => {
                let parts_desc: Vec<String> = c
                    .parts
                    .iter()
                    .map(|p| match p {
                        lir::ContentPart::Text(t) => format!("Text({t:?})"),
                        lir::ContentPart::Glue => "Glue".to_string(),
                        _ => "Other".to_string(),
                    })
                    .collect();
                eprintln!("{pad}  stmt[{i}]: EmitContent({parts_desc:?})");
            }
            lir::Stmt::EmitLine(e) => match &e.line {
                lir::RecognizedLine::Plain(s) => {
                    eprintln!("{pad}  stmt[{i}]: EmitLine(Plain({s:?}))");
                }
                lir::RecognizedLine::Template { .. } => {
                    eprintln!("{pad}  stmt[{i}]: EmitLine(Template)");
                }
            },
            lir::Stmt::Divert(_) => eprintln!("{pad}  stmt[{i}]: Divert"),
            _ => eprintln!("{pad}  stmt[{i}]: <other>"),
        }
    }
    for child in &container.children {
        dump_container(child, indent + 2);
    }
}

#[test]
fn glue_at_end_of_choice_body_before_gather() {
    // This pattern appears in TheIntercept: glue at end of choice body content,
    // before a gather line. The glue should suppress the newline and join the
    // choice body text with the gather text.
    //
    // Key: the choice body uses tab indentation matching the original source.
    let p = lower_ink(
        "-> knot\n\n=== knot\n*\t[Talk]\n\t\"There was a young man.\"\n-\tHarris is not letting me off.\n\t\"You seriously entertained that possibility?\"\n \t* [Yes]\n \t\t\"Yes, I considered it. <>\n \t* [No]\n\t\"No. Not for a moment.\"\n\tI nod. \"<>\n*\t[Lie]\n\t\"I was quite certain, after a while. <>\n- \tHe seemed to know all about me.\"\n-> END\n",
    );

    let knot = find_by_path(&p, "knot");
    eprintln!("=== knot dump ===");
    dump_container(knot, 0);

    // Find choice target containers that have trailing glue
    // (c-0 is "Talk" which has no glue; c-1/c-2/c-3 are Yes/No/Lie which do)
    let choices = collect_kind(knot, lir::ContainerKind::ChoiceTarget);
    assert!(
        choices.len() >= 4,
        "expected at least 4 choice targets, got {}",
        choices.len()
    );

    // Yes (c-1), No (c-2), Lie (c-3) should have trailing glue
    for (i, choice) in choices[1..=3].iter().enumerate() {
        let choice_has_glue = has_glue(&choice.body);
        eprintln!(
            "choice[{}] ({:?}) has_glue={choice_has_glue}",
            i + 1,
            choice.name
        );
        assert!(
            choice_has_glue,
            "choice[{}] ({:?}) should have trailing glue in its body",
            i + 1,
            choice.name,
        );
    }
}

// ─── Temp scoping across choice/gather boundaries ─────────────────────

/// Return true if any expression in the container tree is `GetGlobal`.
fn has_get_global(container: &lir::Container) -> bool {
    fn in_expr(e: &lir::Expr) -> bool {
        match e {
            lir::Expr::GetGlobal(_) => true,
            lir::Expr::Prefix(_, inner) | lir::Expr::Postfix(inner, _) => in_expr(inner),
            lir::Expr::Infix(a, _, b) => in_expr(a) || in_expr(b),
            _ => false,
        }
    }
    fn in_content(c: &lir::Content) -> bool {
        c.parts.iter().any(|p| match p {
            lir::ContentPart::Interpolation(e) => in_expr(e),
            lir::ContentPart::InlineConditional(cond) => cond
                .branches
                .iter()
                .any(|b| b.condition.as_ref().is_some_and(in_expr) || in_stmts(&b.body)),
            _ => false,
        })
    }
    fn in_stmts(stmts: &[lir::Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            lir::Stmt::ExprStmt(e) => in_expr(e),
            lir::Stmt::Assign { value, .. } => in_expr(value),
            lir::Stmt::DeclareTemp { value, .. } => value.as_ref().is_some_and(in_expr),
            lir::Stmt::Conditional(c) => c
                .branches
                .iter()
                .any(|b| b.condition.as_ref().is_some_and(in_expr) || in_stmts(&b.body)),
            lir::Stmt::ChoiceSet(cs) => cs
                .choices
                .iter()
                .any(|ch| ch.condition.as_ref().is_some_and(in_expr)),
            lir::Stmt::EmitContent(c) => in_content(c),
            lir::Stmt::EmitLine(em) | lir::Stmt::EvalLine(em) => {
                if let lir::RecognizedLine::Template { slot_exprs, .. } = &em.line {
                    slot_exprs.iter().any(in_expr)
                } else {
                    false
                }
            }
            lir::Stmt::ChoiceOutput { content, emission } => {
                in_content(content)
                    || emission.as_ref().is_some_and(|em| {
                        if let lir::RecognizedLine::Template { slot_exprs, .. } = &em.line {
                            slot_exprs.iter().any(in_expr)
                        } else {
                            false
                        }
                    })
            }
            _ => false,
        })
    }
    in_stmts(&container.body) || container.children.iter().any(has_get_global)
}

#[test]
fn temp_visible_in_choice_body_after_gather() {
    // A temp declared in a gather continuation must be visible in the
    // next choice set's bodies. A program with no VAR declarations
    // should produce no globals and no GetGlobal expressions.
    // Multiple levels of choice+gather+labeled-block to match TheIntercept.
    let p = lower_ink(
        "\
-> test_knot
=== test_knot ===
 * [A]
   A.
 * [B]
   B.
- First gather.
 * [C]
   C.
 * [D]
   D.
- Second gather.
- (labeled)
  ~ temp saved = true
 * [Yes]
   -> DONE
 * [No]
   {saved:Saved was true.}
   -> DONE
",
    );

    assert!(
        p.globals.is_empty(),
        "program has no VAR — should have no globals"
    );
    assert!(
        !has_get_global(&p.root),
        "program has no VAR — should have no GetGlobal expressions",
    );
}
