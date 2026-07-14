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
    // unwrap: `lower_to_program` only returns `None` when the
    // residual-extension backstop fires (E053) — this helper's callers all
    // pass plain ink, so this should never be `None`.
    program.unwrap()
}

/// Parse ink source → HIR lower → analyze → LIR lower. Returns program + warnings.
fn lower_ink_with_warnings(source: &str) -> (Option<lir::Program>, Vec<brink_ir::Diagnostic>) {
    lower_ink_with_type_mode(source, lir::TypeMode::Gradual)
}

/// [`lower_ink_with_warnings`] with an explicit `types` policy — TM-4c
/// (#666) tests exercising the strict-only static-offset path need this;
/// every other caller gets the gradual default.
fn lower_ink_with_type_mode(
    source: &str,
    type_mode: lir::TypeMode,
) -> (Option<lir::Program>, Vec<brink_ir::Diagnostic>) {
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
    lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
        type_mode,
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

// ─── #673: collection/struct literal declaration defaults ───────────
//
// `eval_const_expr` (decls) previously had no arm for `ArrayLiteral`/
// `MapLiteral`/`StructLiteral` and fell through to `ConstValue::Null` with
// no diagnostic. These fixtures deliberately do NOT use the `VAR p = 0` +
// reassignment workaround idiom (`tests/tier1-brink/nested-index-
// assignment/story.ink`'s precedent) — the literal is the declaration's
// actual default.

#[test]
fn var_array_literal_default_folds_to_const_array_not_null() {
    let p = lower_ink("VAR arr = #[1, 2, 3]\n");
    let g = find_global(&p, "arr");
    match &g.default {
        lir::ConstValue::Array(items) => {
            assert_eq!(
                items,
                &[
                    lir::ConstValue::Int(1),
                    lir::ConstValue::Int(2),
                    lir::ConstValue::Int(3),
                ]
            );
        }
        other => panic!("expected ConstValue::Array, got {other:?} (silent Null regression)"),
    }
}

#[test]
fn var_map_literal_default_folds_to_const_map_not_null() {
    let p = lower_ink("VAR m = #{\"a\": 1, \"b\": 2}\n");
    let g = find_global(&p, "m");
    match &g.default {
        lir::ConstValue::Map(entries) => {
            assert_eq!(
                entries,
                &[
                    (
                        lir::ConstMapKey::Str("a".to_string()),
                        lir::ConstValue::Int(1)
                    ),
                    (
                        lir::ConstMapKey::Str("b".to_string()),
                        lir::ConstValue::Int(2)
                    ),
                ]
            );
        }
        other => panic!("expected ConstValue::Map, got {other:?} (silent Null regression)"),
    }
}

#[test]
fn const_array_literal_default_folds_to_const_array() {
    // The issue names both VAR and CONST declaration defaults.
    let p = lower_ink("CONST arr = #[9, 8]\n");
    let g = find_global(&p, "arr");
    assert!(!g.mutable);
    match &g.default {
        lir::ConstValue::Array(items) => {
            assert_eq!(items, &[lir::ConstValue::Int(9), lir::ConstValue::Int(8)]);
        }
        other => panic!("expected ConstValue::Array, got {other:?}"),
    }
}

#[test]
fn nested_array_literal_default_folds_recursively() {
    let p = lower_ink("VAR grid = #[#[1, 2], #[3, 4]]\n");
    let g = find_global(&p, "grid");
    match &g.default {
        lir::ConstValue::Array(items) => {
            assert_eq!(
                items,
                &[
                    lir::ConstValue::Array(vec![lir::ConstValue::Int(1), lir::ConstValue::Int(2)]),
                    lir::ConstValue::Array(vec![lir::ConstValue::Int(3), lir::ConstValue::Int(4)]),
                ]
            );
        }
        other => panic!("expected nested ConstValue::Array, got {other:?}"),
    }
}

#[test]
fn struct_literal_default_is_a_real_compile_error_not_silent_null() {
    let source =
        "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\nVAR p = Point#{x: 1.0, y: 2.0}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    // Lowering is still total (matches E055/E056/E073/E074's existing
    // "diagnostic doesn't stop `Some(program)`" convention) — the caller
    // above `lower_to_program` (brink-db's `lir_query`) is what turns an
    // Error-severity diagnostic into a blocked compile.
    let program = program.expect("lowering stays total; severity partitioning happens upstream");
    let g = find_global(&program, "p");
    assert!(
        matches!(g.default, lir::ConstValue::Null),
        "struct defaults have no ConstValue representation yet — expected Null, got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E075),
        "expected non-suppressible E075 for a struct literal VAR default, got {diagnostics:?}"
    );
}

#[test]
fn map_literal_default_with_non_scalar_key_is_a_real_compile_error() {
    // Float is not in the ratified map-key domain (int/string/bool) —
    // mid-story `MapNew` faults on this at runtime; a declaration default
    // has no runtime construction step to fault at, so this must be a
    // compile-time diagnostic, not a silently-dropped entry.
    let source = "VAR m = #{3.5: 1}\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "m");
    assert!(
        matches!(&g.default, lir::ConstValue::Map(entries) if entries.is_empty()),
        "expected the invalid-key entry to be dropped from the map, got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E076),
        "expected non-suppressible E076 for a non-scalar map key in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn array_literal_default_with_non_constant_element_is_a_real_compile_error() {
    // #679 review: a function call can never constant-fold — before E077
    // the element recursed into `eval_const_expr`'s catch-all and silently
    // became `Null`, #673's silent-Null bug one level down inside the
    // literal. Keyed off the source expr *kind* (a call), not the folded
    // result.
    let source = "VAR arr = #[f(), 2]\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice() == [lir::ConstValue::Null, lir::ConstValue::Int(2)]
        ),
        "the never-constant element still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a non-constant array element in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn map_literal_default_with_non_constant_value_is_a_real_compile_error() {
    // #679 review: same E077 story as the array-element test, for a map
    // *value*. (A never-constant map *key* is already E076 — it folds to
    // Null, outside the scalar key domain.)
    let source = "VAR m = #{\"a\": f()}\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "m");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Map(entries)
                if entries.as_slice()
                    == [(
                        lir::ConstMapKey::Str("a".to_string()),
                        lir::ConstValue::Null
                    )]
        ),
        "the never-constant value still folds to Null (now diagnosed, not silent), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected non-suppressible E077 for a non-constant map value in a VAR default, got {diagnostics:?}"
    );
}

#[test]
fn nested_array_literal_with_non_constant_element_propagates_e077() {
    // #679 review, nested case: the outer element is itself a literal (a
    // constant-foldable *kind*), so the outer check passes and the E077
    // must come from the recursion into the inner literal's own
    // per-element check — the hole must not reopen one level down.
    let source = "VAR grid = #[#[f()], #[2]]\n\n=== function f()\n~ return 1\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "grid");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice()
                    == [
                        lir::ConstValue::Array(vec![lir::ConstValue::Null]),
                        lir::ConstValue::Array(vec![lir::ConstValue::Int(2)]),
                    ]
        ),
        "the nested never-constant element still folds to Null (now diagnosed), got {:?}",
        g.default
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "expected E077 to propagate out of the nested literal, got {diagnostics:?}"
    );
}

#[test]
fn constant_infix_array_element_does_not_false_positive_e077() {
    // The E077 check recurses through Prefix/Infix — `1 + 2` and `-3` are
    // constant-foldable kinds and must not be flagged.
    let source = "VAR arr = #[1 + 2, -3]\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice() == [lir::ConstValue::Int(3), lir::ConstValue::Int(-3)]
        ),
        "constant infix/prefix elements fold for real, got {:?}",
        g.default
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "constant-foldable elements must not trip E077, got {diagnostics:?}"
    );
}

#[test]
fn const_reference_array_element_folds_and_does_not_false_positive_e077() {
    // A `CONST` reference is a `Path` — a constant-foldable kind. It must
    // resolve through `const_values` to the real value and must not trip
    // E077 (the check is keyed off the source expr kind, and `Path`
    // constness depends on resolution — deliberately not flagged).
    let source = "CONST SOME_CONST = 7\nVAR arr = #[SOME_CONST, 2]\n";
    let (program, diagnostics) = lower_ink_with_warnings(source);
    let program = program.expect("lowering stays total");
    let g = find_global(&program, "arr");
    assert!(
        matches!(
            &g.default,
            lir::ConstValue::Array(items)
                if items.as_slice() == [lir::ConstValue::Int(7), lir::ConstValue::Int(2)]
        ),
        "CONST-reference element folds to the constant's value, got {:?}",
        g.default
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E077),
        "a CONST-reference element must not trip E077, got {diagnostics:?}"
    );
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

/// Ink keywords are contextual — an external may be named after an operator
/// keyword (e.g. `has`, the `Has` list operator). This must lower to a proper
/// external symbol named "has", not be dropped with a misleading "missing name"
/// diagnostic (E010). Regression for keyword-named externals.
#[test]
fn external_keyword_name() {
    // HIR lowering must not emit E010 ("missing name") for the keyword name.
    let parsed = brink_syntax::parse("EXTERNAL has(item)\n");
    let (_hir, _manifest, hir_diags) = brink_ir::hir::lower(FileId(0), &parsed.tree());
    assert!(
        hir_diags.is_empty(),
        "keyword-named external should lower without diagnostics, got: {hir_diags:?}"
    );

    // …and the external survives end-to-end with the correct name and arity.
    let p = lower_ink("EXTERNAL has(item)\n");
    assert_eq!(p.externals.len(), 1);
    assert_eq!(p.externals[0].arg_count, 1);
    let name = &p.name_table[p.externals[0].name.0 as usize];
    assert_eq!(name, "has");
}

/// External parameters may likewise be named after contextual keywords.
#[test]
fn external_keyword_params() {
    let p = lower_ink("EXTERNAL combine(and, or)\n");
    assert_eq!(p.externals.len(), 1);
    assert_eq!(p.externals[0].arg_count, 2);
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

// ─── T1b-2 (#570): LogicBlock/Index lower for real, no dialect gate needed ──
//
// `lower_ink_with_warnings` deliberately mirrors a caller that (like a
// suppressed dialect gate) never checks `brink_analyzer::analyze`'s
// diagnostics before lowering to LIR. Through T1b-1, a `LogicBlock`/`Index`/…
// HIR node reaching `lower_to_program` this way would panic via
// `debug_assert!` in debug builds and silently drop data (`None` /
// `lir::Expr::Null`) in release builds — caught by the (now-retired) E053
// backstop, which refused to produce a program at all (#572 review). T1b-2
// replaces that rejection with real lowering, so these HIR node kinds are no
// longer "residual" — this test now proves the opposite of its T1b-1
// version: the program lowers successfully and correctly.

#[test]
fn logic_block_lowers_without_a_dialect_gate_in_the_loop() {
    let (program, _diags) =
        lower_ink_with_warnings("Hello\n~ {\ntemp x = 0\nx = x + 1\n}\nWorld\n");
    let program = program.expect("LogicBlock should lower to a real program in T1b-2");
    assert!(!program.root.body.is_empty());
}

#[test]
fn index_expression_lowers_without_a_dialect_gate_in_the_loop() {
    let (program, _diags) = lower_ink_with_warnings("VAR a = 5\n~ x = a[0]\n");
    let program = program.expect("Index expression should lower to a real program in T1b-2");
    assert!(!program.root.body.is_empty());
}

// ─── #578: LogicBlock inside an un-lifted inline conditional/sequence ──────
//
// `hir::normalize_file` (called unconditionally by `lower_to_program`) lifts
// every `InlineConditional`/`InlineSequence` it finds in a block's own
// `Stmt::Content` parts into a top-level `Stmt::Conditional`/`Stmt::Sequence`
// — the safe path that reaches `lower_stmt` via `lower_block_with_children`,
// which explicitly dispatches `hir::Stmt::LogicBlock`. But normalization
// only walks `Block.stmts` (root/knot/stitch bodies, choice bodies,
// continuations) — it never touches a `Choice`'s own display text
// (`start_content`/`bracket_content`/`inner_content`), which LIR lowering
// feeds straight to `content::lower_content` regardless. A multiline
// branched conditional embedded in choice text (e.g. `* Pick {cond: - a:
// ~{...} - b: ...}`) therefore keeps its `InlineConditional` shape all the
// way to `content::lower_inline_block`, and if a branch contains a `~ { … }`
// T1b block, that reached `stmts::lower_stmt`'s `debug_assert!`-guarded
// "should be dispatched by lower_block_with_children" arm — panicking in
// debug builds, silently dropping the block's statements in release.
#[test]
fn logic_block_in_choice_text_inline_conditional_lowers_without_panicking() {
    let src = "VAR x = 1\n* Pick {x > 0:\n- true: ~ { x = x + 1 }\n- else: ~ { x = x - 1 }\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let program =
        program.expect("choice-text LogicBlock should lower to a real program, not panic/drop");

    // Prove the LogicBlock's `Assign` statements actually made it into the
    // lowered choice's display text (not silently dropped): find the
    // top-level ChoiceSet, dig into its one choice's `start_content`, and
    // confirm the `InlineConditional`'s branches carry `Stmt::Assign`.
    let Some(lir::Stmt::ChoiceSet(cs)) = program
        .root
        .body
        .iter()
        .find(|s| matches!(s, lir::Stmt::ChoiceSet(_)))
    else {
        panic!("expected a ChoiceSet in the root body");
    };
    assert_eq!(cs.choices.len(), 1);
    let start_content = cs.choices[0]
        .start_content
        .as_ref()
        .expect("choice should have start_content (the inline conditional's text)");
    let has_assign_in_branches = start_content.parts.iter().any(|p| {
        let lir::ContentPart::InlineConditional(cond) = p else {
            return false;
        };
        cond.branches
            .iter()
            .any(|b| b.body.iter().any(|s| matches!(s, lir::Stmt::Assign { .. })))
    });
    assert!(
        has_assign_in_branches,
        "expected the LogicBlock's Assign statements spliced into the \
         InlineConditional's branches, not dropped"
    );
}

#[test]
fn logic_block_second_inline_construct_on_a_content_line_lowers() {
    // Two inline logics on one content line: the first (`{x}`) is a plain
    // interpolation, so `lower_multiline_block_from_inline`'s "is it the
    // first inline logic AND promotable" check fails on it and the whole
    // line falls through to the generic content-parts path — the second
    // inline logic (the multiline conditional with LogicBlock branches)
    // becomes an un-lifted `ContentPart::InlineConditional` too, independent
    // of the choice-text case above.
    let src =
        "VAR x = 1\nHello {x} and {x > 0:\n- true: ~ { x = x + 1 }\n- else: ~ { x = x - 1 }\n}\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    let program = program.expect("should lower to a real program, not panic/drop");
    assert!(!program.root.body.is_empty());
}

// ─── #577: break/continue outside a loop is a targeted compile error ──────
//
// Previously `~ { break }`/`~ { continue }` with no enclosing `while`/`for`
// lowered unconditionally to `lir::Stmt::LogicBreak`/`LogicContinue`, and
// codegen's `container.rs` silently degraded the resulting unguarded jump
// to `Opcode::Nop` (`self.loop_stack.is_empty()`) instead of ever surfacing
// an error. `blocks::lower_block_stmt` now rejects it at LIR-lowering time
// (E057, Error severity) and skips emitting the statement — a real,
// non-suppressible compile error (`brink-db`'s `lir_query` gates `program:
// None` on any Error-severity LIR diagnostic, bypassing `// brink-disable-
// all`, which only covers analysis-phase diagnostics), not a cosmetic note.

fn find_e057(diags: &[brink_ir::Diagnostic]) -> Option<&brink_ir::Diagnostic> {
    diags
        .iter()
        .find(|d| d.code == brink_ir::DiagnosticCode::E057)
}

#[test]
fn break_outside_any_loop_emits_e057_error_and_is_not_lowered() {
    let (program, diags) = lower_ink_with_warnings("Hello\n~ {\nbreak\n}\n");
    let e057 = find_e057(&diags).expect("expected E057 for break outside a loop");
    assert_eq!(e057.code.severity(), brink_ir::Severity::Error);
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(
        !program
            .root
            .body
            .iter()
            .any(|s| matches!(s, lir::Stmt::LogicBreak)),
        "the unguarded break must not be lowered to a LogicBreak statement"
    );
}

#[test]
fn continue_outside_any_loop_emits_e057_error_and_is_not_lowered() {
    let (program, diags) = lower_ink_with_warnings("Hello\n~ {\ncontinue\n}\n");
    let e057 = find_e057(&diags).expect("expected E057 for continue outside a loop");
    assert_eq!(e057.code.severity(), brink_ir::Severity::Error);
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(
        !program
            .root
            .body
            .iter()
            .any(|s| matches!(s, lir::Stmt::LogicContinue)),
        "the unguarded continue must not be lowered to a LogicContinue statement"
    );
}

#[test]
fn break_after_a_while_loop_at_the_same_depth_still_errors() {
    // `loop_depth` must be decremented back to 0 on exiting the while body —
    // a break textually after the loop (sibling, not nested) is still an
    // error, proving the counter doesn't leak across sibling statements.
    let (_program, diags) =
        lower_ink_with_warnings("Hello\n~ {\ntemp x = 0\nwhile x < 3 {\nx = x + 1\n}\nbreak\n}\n");
    assert!(
        find_e057(&diags).is_some(),
        "expected E057 for a break textually after (not inside) the loop"
    );
}

#[test]
fn break_inside_if_inside_while_is_allowed() {
    // `if`/`else` nesting inside a loop body must not reset loop_depth —
    // break/continue reached through conditional nesting is still valid.
    let (program, diags) = lower_ink_with_warnings(
        "Hello\n~ {\ntemp x = 0\nwhile x < 3 {\nif x == 1 {\nbreak\n}\nx = x + 1\n}\n}\n",
    );
    assert!(
        find_e057(&diags).is_none(),
        "break nested in if-inside-while must not error: {diags:?}"
    );
    let program = program.expect("should lower to a real program");
    assert!(!program.root.body.is_empty());
}

#[test]
fn continue_inside_for_loop_is_allowed() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1, 2, 3]\n~ {\nfor v in a {\nif v == 2 {\ncontinue\n}\n}\n}\n",
    );
    assert!(find_e057(&diags).is_none(), "unexpected E057: {diags:?}");
    let program = program.expect("should lower to a real program");
    assert!(!program.root.body.is_empty());
}

// ─── #581: collection mutator arity mismatch is a targeted compile error ──
//
// `push`/`insert`/`remove` called with the wrong argument count used to
// share the generic warning-severity E031 with ordinary function-call arity
// checking, and — because E031 never blocked compilation — the malformed
// mutator statement silently vanished from the lowered bytecode (nothing
// pushed to `out`, `try_lower_mutator_stmt` still returned `true`). E058 is
// Error-severity and names the expected signature.

fn find_e058(diags: &[brink_ir::Diagnostic]) -> Option<&brink_ir::Diagnostic> {
    diags
        .iter()
        .find(|d| d.code == brink_ir::DiagnosticCode::E058)
}

#[test]
fn push_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ push(a)\n");
    let e058 = find_e058(&diags).expect("expected E058 for push with 1 argument");
    assert_eq!(e058.code.severity(), brink_ir::Severity::Error);
    assert!(
        e058.message.contains("push(container, value)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn insert_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ insert(a, 0)\n");
    let e058 = find_e058(&diags).expect("expected E058 for insert with 2 arguments");
    assert!(
        e058.message
            .contains("insert(container, key_or_index, value)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn remove_wrong_arity_emits_e058_naming_the_signature() {
    let (_program, diags) = lower_ink_with_warnings("VAR a = #[1]\n~ remove(a, 0, 1)\n");
    let e058 = find_e058(&diags).expect("expected E058 for remove with 3 arguments");
    assert!(
        e058.message.contains("remove(container, key_or_index)"),
        "message should name the expected signature: {}",
        e058.message
    );
}

#[test]
fn mutator_correct_arity_no_e058() {
    let (program, diags) = lower_ink_with_warnings(
        "VAR a = #[1]\n~ {\npush(a, 2)\ninsert(a, 0, 9)\nremove(a, 0)\n}\n",
    );
    assert!(find_e058(&diags).is_none(), "unexpected E058: {diags:?}");
    let program = program.expect("should lower to a real program");
    assert!(!program.root.body.is_empty());
}

#[test]
fn pure_function_call_arity_still_uses_e031_not_e058() {
    // Ordinary (non-mutator) function-call arity checking is untouched by
    // #581 — only push/insert/remove route through E058.
    let (_program, diags) =
        lower_ink_with_warnings("~ temp x = f(1)\n== function f(a, b) ==\n~ return a + b\n");
    assert!(
        find_e058(&diags).is_none(),
        "pure function arity mismatch must not use E058: {diags:?}"
    );
}

// ─── #585: nested choice inside an un-lifted inline conditional (sibling
// of #578, live-reproduced) ─────────────────────────────────────────────
//
// `stmts::lower_stmt`'s `ChoiceSet`/`LabeledBlock`/`Conditional`/`Sequence`
// arm is reached the exact same way #578's `LogicBlock` arm was: a
// multiline conditional embedded in a *choice's own* display/bracket/inner
// text (`content::lower_inline_block`'s doc comment) keeps its
// `InlineConditional` shape all the way to LIR lowering instead of being
// lifted to a top-level `Stmt::Conditional` — but unlike `LogicBlock`, a
// nested choice inside one of that inline conditional's branches can't be
// "properly routed" in place: a `ChoiceSet` needs an addressable child
// container for the runtime to divert into on selection, and
// `lower_inline_block` (unlike `lower_block_with_children`) has no way to
// hand a child container back to its caller. Before this fix that reached
// `lower_stmt`'s `debug_assert!(false, …)` arm — a panic in debug builds,
// a silent drop in release. It now emits a real, non-suppressible E059
// compile error and drops just the malformed nested statement (not the
// whole program).

fn find_e059(diags: &[brink_ir::Diagnostic]) -> Option<&brink_ir::Diagnostic> {
    diags
        .iter()
        .find(|d| d.code == brink_ir::DiagnosticCode::E059)
}

#[test]
fn nested_choice_in_choice_text_inline_conditional_emits_e059_not_panic() {
    // The reproducing input: a top-level choice ("Pick") whose own display
    // text is a multiline `{x > 0: ... }` conditional, one branch of which
    // contains a *nested* `*` choice — never lifted out of choice text by
    // HIR normalization (`content::lower_inline_block`'s doc comment).
    let src =
        "VAR x = 1\n* Pick {x > 0:\n- true: * nested\n    -> END\n- else: text\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e059 = find_e059(&diags).expect("expected E059 for the nested un-lifted choice");
    assert_eq!(e059.code.severity(), brink_ir::Severity::Error);
    assert!(
        e059.message.contains("nested choice"),
        "message should name the offending construct: {}",
        e059.message
    );
    // `lower_to_program` stays total (like E057/E058) — it still returns a
    // program, just without the malformed nested statement.
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(!program.root.body.is_empty());
}

#[test]
fn nested_sequence_in_choice_text_inline_conditional_emits_e059_not_panic() {
    // A multiline sequence nested the same way as the `ChoiceSet` case
    // above — proves the `Sequence` sub-arm (which, unlike `ChoiceSet`,
    // carries its own `AstPtr`) is exercised too, not just asserted by
    // analogy.
    let src = "VAR x = 1\n* Pick {x > 0:\n- true:\n{stopping:\n- one\n- two\n}\n- else: text\n}\n    -> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e059 = find_e059(&diags).expect("expected E059 for the nested un-lifted sequence");
    assert_eq!(e059.code.severity(), brink_ir::Severity::Error);
    let program = program.expect("lower_to_program is total — it still returns Some");
    assert!(!program.root.body.is_empty());
}

// ─── TM-4c (#666): structs LIR + codegen ────────────────────────────────
//
// Construction, field reads, and single-level field writes all lower for
// real now (TM-4b/#665 only landed grammar+HIR+analyzer, diagnostics-only).
// E072 (the old "reject everything" backstop) is retired; E073 (unresolved
// shape at construction) and E074 (chained/mixed field-write projection)
// are its narrower TM-4c replacements — still real, non-suppressible
// diagnostics, mirroring the E053-backstop discipline.

fn find_diag(
    diags: &[brink_ir::Diagnostic],
    code: brink_ir::DiagnosticCode,
) -> Option<&brink_ir::Diagnostic> {
    diags.iter().find(|d| d.code == code)
}

/// A `RecordNew`'s decoded parts: `(shape_id, fields, prelude)` — see
/// `lir::Expr::RecordNew`'s own doc for what `fields`/`prelude` mean.
type RecordNewParts<'a> = (
    u32,
    &'a [lir::Expr],
    &'a [(u16, brink_format::NameId, lir::Expr)],
);

fn find_record_new(expr: &lir::Expr) -> Option<RecordNewParts<'_>> {
    match expr {
        lir::Expr::RecordNew {
            shape_id,
            fields,
            prelude,
        } => Some((*shape_id, fields, prelude)),
        _ => None,
    }
}

/// Resolve a `RecordNew` field to the actual initializer expression it was
/// built from: the well-formed path's `fields` entries are `GetTemp` reads
/// of a `prelude` slot (#676 source-order staging), so this chases that
/// indirection back to the staged expression; the fault path's `fields`
/// entries are already the raw initializer, returned as-is.
fn resolve_field<'a>(
    field: &'a lir::Expr,
    prelude: &'a [(u16, brink_format::NameId, lir::Expr)],
) -> &'a lir::Expr {
    match field {
        lir::Expr::GetTemp(slot, _) => prelude
            .iter()
            .find(|(s, _, _)| s == slot)
            .map_or(field, |(_, _, e)| e),
        _ => field,
    }
}

fn declare_temp_value(stmt: &lir::Stmt) -> Option<&lir::Expr> {
    match stmt {
        lir::Stmt::DeclareTemp { value: Some(v), .. } => Some(v),
        _ => None,
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "exact literal from source (1.0/2.0), not a computed value"
)]
fn struct_literal_lowers_to_record_new_in_shape_order() {
    let src = "STRUCT Point = #{x: float, y: float}\n~ temp p = Point#{y: 2.0, x: 1.0}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct constructs lower for real under TM-4c");
    assert_eq!(program.struct_shapes.len(), 1);
    assert_eq!(program.struct_shapes[0].fields.len(), 2);

    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp p := Point#{...} should be a DeclareTemp");
    let (shape_id, fields, prelude) =
        find_record_new(value).expect("Point#{...} should lower to RecordNew");
    assert_eq!(shape_id, program.struct_shapes[0].id);
    // `fields` is reordered into shape decl order (x, y) despite being
    // written y, x — each entry is a `GetTemp` read of a `prelude` slot,
    // resolved back to its staged value here.
    assert!(matches!(
        resolve_field(&fields[0], prelude),
        lir::Expr::Float(f) if *f == 1.0
    ));
    assert!(matches!(
        resolve_field(&fields[1], prelude),
        lir::Expr::Float(f) if *f == 2.0
    ));
    // `prelude` itself is staged in **source** order (#676): y (2.0) first,
    // then x (1.0) — the author's left-to-right order, not shape order.
    assert_eq!(prelude.len(), 2, "one staged slot per supplied initializer");
    assert!(matches!(prelude[0].2, lir::Expr::Float(f) if f == 2.0));
    assert!(matches!(prelude[1].2, lir::Expr::Float(f) if f == 1.0));
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "exact literal from source (1.0/2.0/3.0), not a computed value"
)]
fn duplicate_field_still_stages_every_initializer_no_silent_drop() {
    // #675's LIR-level defense-in-depth: `structs::check_duplicates` (E084)
    // is what normally stops this from compiling at all, but this test
    // proves lowering itself never silently drops an initializer's side
    // effect even under suppression — both `x` initializers get staged
    // into `prelude` (hence both would still be evaluated at runtime),
    // even though only the *last* one's value (2.0, last-wins) ends up
    // placed in the record. `E084` itself is an analyzer diagnostic this
    // LIR-only harness never surfaces — covered instead by
    // `brink-analyzer`'s own unit tests and the `e0xx_diagnostics` pipeline
    // suite.
    let src = "STRUCT Point = #{x: float, y: float}\n\
        ~ temp p = Point#{x: 1.0, x: 2.0, y: 3.0}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct constructs still lower for real under TM-4c");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp p := Point#{...} should be a DeclareTemp");
    let (_shape_id, fields, prelude) =
        find_record_new(value).expect("Point#{...} should lower to RecordNew");
    assert_eq!(
        prelude.len(),
        3,
        "every supplied initializer is staged, including the shadowed duplicate"
    );
    // The winning (last-wins) `x` initializer's value (2.0) is what
    // actually lands in the record at offset 0.
    assert!(matches!(
        resolve_field(&fields[0], prelude),
        lir::Expr::Float(f) if *f == 2.0
    ));
}

#[test]
fn field_access_on_construction_literal_lowers_to_record_get() {
    let src = "STRUCT Point = #{x: float}\n~ temp v = Point#{x: 1.0}.x\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("field access lowers for real under TM-4c");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp v := Point#{...}.x should be a DeclareTemp");
    assert!(matches!(value, lir::Expr::RecordGet { base, .. } if find_record_new(base).is_some()));
}

#[test]
fn resolution_fallback_field_access_lowers_to_record_get_dyn() {
    // The ambiguous `p.x` shape (ordinary dotted `Path`, not the
    // `FieldAccessExpr` grammar) — `brink-analyzer`'s resolution fallback
    // resolves it to the variable `p` via a multi-segment path (TM-4b);
    // LIR lowering must produce the equivalent `RecordGet` chain, not
    // silently load `p` and drop `.x`. `p` holds an int here (gradual mode
    // never checks this statically), so `static_offset` must be `None` — a
    // by-name op, which faults cleanly at runtime instead of trusting an
    // unproven offset.
    let src = "VAR p = 0\n~ temp y = p.x\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("ambiguous-path field access lowers cleanly");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp y := p.x should be a DeclareTemp");
    match value {
        lir::Expr::RecordGet {
            base,
            static_offset,
            ..
        } => {
            assert!(matches!(**base, lir::Expr::GetGlobal(_)));
            assert_eq!(
                *static_offset, None,
                "gradual mode never emits static offsets"
            );
        }
        _ => panic!("expected RecordGet"),
    }
}

#[test]
fn ordinary_dotted_path_still_lowers_as_a_static_visit_count() {
    // A genuine static path (`knot.stitch`) must never be reinterpreted as
    // field access — only the TM-4b fallback case (resolved to
    // Variable/Constant/Param/Temp via a multi-segment path) is. A bare
    // knot/stitch reference used as a *value* (not a divert target) means
    // its visit count in ink semantics (`SymbolKind::Knot | Stitch | Label`
    // → `Expr::VisitCount`) — the key property under test is that it's
    // never a `RecordGet`.
    let src =
        "=== knot ===\n= stitch\nHello.\n-> DONE\n=== main ===\n~ temp x = knot.stitch\n-> DONE\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("ordinary dotted path must lower cleanly");
    let main_knot = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("the `main` knot should exist as a root child container");
    let value = main_knot
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp x := knot.stitch should be a DeclareTemp");
    assert!(
        matches!(value, lir::Expr::VisitCount(_)),
        "a static dotted path must never become a RecordGet"
    );
}

#[test]
fn unresolved_struct_shape_at_construction_emits_e073() {
    let src = "~ temp p = Bogus#{x: 1}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e073 = find_diag(&diags, brink_ir::DiagnosticCode::E073)
        .expect("expected E073 for an unresolved struct shape reaching LIR");
    assert_eq!(e073.code.severity(), brink_ir::Severity::Error);
    // `lower_to_program` stays total (like E053/E057/E058) — it still
    // returns a program; `brink-db`'s `lir_query` is what turns an
    // Error-severity LIR diagnostic into `program: None` for compilation
    // purposes, not `lower_to_program` itself.
    program.expect("lower_to_program is total — it still returns Some");
}

#[test]
fn chained_field_write_emits_e074() {
    let src = "STRUCT Inner = #{v: float}\nSTRUCT Outer = #{inner: Inner}\n\
        VAR o = Outer#{inner: Inner#{v: 1.0}}\n~ o.inner.v = 2.0\nHello.\n";
    let (_program, diags) = lower_ink_with_warnings(src);
    let e074 = find_diag(&diags, brink_ir::DiagnosticCode::E074)
        .expect("expected E074 for a chained field write (o.inner.v = ...)");
    assert_eq!(e074.code.severity(), brink_ir::Severity::Error);
}

#[test]
fn mixed_index_then_field_write_emits_e074() {
    // #674: `arr[i].field = v` (an `Index`-based root followed by a
    // `.field` write) now parses as a real `FIELD_ACCESS_EXPR` assignment
    // target (the grammar gap tracked in the NOTE this test replaces —
    // formerly a generic E015 parse error, PR #665/#668's pre-existing
    // gap). LIR still fences it off as a chained/mixed field write: this
    // pins that `E074` actually fires end-to-end through the parser, not
    // just by code-review inspection of `try_lower_field_assignment`'s
    // `hir::Expr::FieldAccess` branch (blocks.rs).
    let src = "VAR arr = #[1, 2, 3]\n~ arr[0].x = 2.0\nHello.\n";
    let (_program, diags) = lower_ink_with_warnings(src);
    let e074 = find_diag(&diags, brink_ir::DiagnosticCode::E074).expect(
        "expected E074 for a mixed index-then-field write (arr[0].x = ...), now reachable \
         through the parser",
    );
    assert_eq!(e074.code.severity(), brink_ir::Severity::Error);
}

#[test]
fn single_level_field_write_lowers_via_take_make_mut_write_back() {
    // #673: a struct literal is no longer a legal VAR *declaration default*
    // (that now emits a real E075 diagnostic, not a silent Null) — this
    // test's actual concern is the RMW field-write desugaring, so `p` gets
    // a scalar placeholder default (no TM-2 annotation here, so `types =
    // gradual`'s advisory-only E063 is the worst this scalar/struct
    // mismatch could trigger) and the real `Point` value is constructed via
    // assignment, same as `tests/tier1-brink/struct-construct-read-write/
    // story.ink`'s established pattern.
    let src = "STRUCT Point = #{x: float, y: float}\nVAR p = 0\n\
        ~ p = Point#{x: 1.0, y: 2.0}\n~ p.x = 9.0\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("single-level field write lowers for real");
    // The RMW desugaring produces a TakeGlobal(p) somewhere, feeding a
    // RecordSet, whose result is written back into the p global.
    let has_take_then_record_set = program.root.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                value: lir::Expr::RecordSet { base, .. },
                ..
            } if matches!(**base, lir::Expr::TakeTemp(..) | lir::Expr::TakeGlobal(_))
        )
    });
    assert!(
        has_take_then_record_set,
        "expected a TakeGlobal/TakeTemp feeding a RecordSet"
    );
    let writes_back_to_p = program.root.body.iter().any(|s| {
        matches!(
            s,
            lir::Stmt::Assign {
                target: lir::AssignTarget::Global(_),
                ..
            }
        )
    });
    assert!(
        writes_back_to_p,
        "expected a write-back Assign into a global"
    );
}

#[test]
fn gradual_construction_field_mismatch_uses_fault_sentinel_shape_id() {
    // Missing declared field `y` — under `types = gradual` (the default;
    // strict would already be E069, a compile error) this must still lower
    // to *something* the VM can execute deterministically: the
    // construction-fault sentinel `RecordNew` (see
    // `expr::lower_struct_literal`'s doc) rather than a stack-desyncing
    // partial construction.
    let src = "STRUCT Point = #{x: float, y: float}\n~ temp p = Point#{x: 1.0}\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("gradual mismatch still lowers, faulting at runtime instead");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp p := Point#{...} should be a DeclareTemp");
    let (shape_id, fields, prelude) =
        find_record_new(value).expect("mismatched construction should still be a RecordNew");
    assert_eq!(shape_id, u32::MAX, "sentinel shape id signals the fault");
    assert_eq!(
        fields.len(),
        1,
        "the one supplied initializer is still evaluated"
    );
    assert!(
        prelude.is_empty(),
        "the fault path's fields are already source order — no staging needed"
    );
}

#[test]
fn strict_mode_known_shape_field_read_uses_static_offset() {
    // #673: `VAR p: Point = Point#{...}` used to be exactly the pattern
    // `eval_const_expr` silently dropped to `Null` — now a real E075. `p`
    // gets the same `VAR p: Point = 0` + assignment shape
    // `tm4c_structs_codegen.rs`'s `strict_and_gradual_produce_equivalent_
    // output_for_well_formed_program` already establishes (a scalar
    // placeholder default under a struct annotation doesn't trip E063 —
    // that fixture already proves it compiles clean under strict): shape
    // resolution for the static-offset decision is driven purely by the
    // TM-2 annotation (`structs::build_global_shape_map` reads
    // `var.annotation`, never the default expression), so the placeholder
    // default doesn't affect what this test actually checks.
    let src = "STRUCT Point = #{x: float, y: float}\nVAR p: Point = 0\n\
        ~ p = Point#{x: 1.0, y: 2.0}\n~ temp v = p.y\nHello.\n";
    let (program, diags) = lower_ink_with_type_mode(src, lir::TypeMode::Strict);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct-typed VAR under strict lowers cleanly");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp v := p.y should be a DeclareTemp");
    match value {
        lir::Expr::RecordGet { static_offset, .. } => {
            assert_eq!(*static_offset, Some(1), "y is field offset 1 in Point");
        }
        _ => panic!("expected RecordGet"),
    }
}

#[test]
fn gradual_mode_never_emits_static_offset_even_with_annotation() {
    // Same source as the strict test above, but under `types = gradual`
    // (the default) — the annotation is "optional seasoning" there, never
    // enforced, so trusting it for a static offset would be unsound (see
    // `expr::static_offset_for`'s doc). Must fall back to the by-name op.
    // #673: same fixture rewrite as
    // `strict_mode_known_shape_field_read_uses_static_offset` above — a
    // struct literal is no longer legal as the declaration default itself
    // (real E075 now), so `p` gets a scalar placeholder default and the
    // annotation stays for this test's actual point (the annotation being
    // *ignored* under gradual).
    let src = "STRUCT Point = #{x: float, y: float}\nVAR p: Point = 0\n\
        ~ p = Point#{x: 1.0, y: 2.0}\n~ temp v = p.y\nHello.\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        diags
            .iter()
            .all(|d| d.code.severity() != brink_ir::Severity::Error),
        "{diags:?}"
    );
    let program = program.expect("struct-typed VAR under gradual lowers cleanly");
    let value = program
        .root
        .body
        .iter()
        .find_map(declare_temp_value)
        .expect("temp v := p.y should be a DeclareTemp");
    match value {
        lir::Expr::RecordGet { static_offset, .. } => {
            assert_eq!(*static_offset, None);
        }
        _ => panic!("expected RecordGet"),
    }
}

// ── #680 RCA: block-scoped temp read after its block closes (E082) ───────
//
// #680 was filed as "a `ref`-argument call co-occurring with a `temp` decl
// in the same `~ { }` block resolves to the wrong global slot
// (UnresolvedGlobal)". Root-causing the reporter's minimal repro against
// current `main` shows the `ref`-argument call is a red herring: it
// reproduces with *no* ref call at all, and does *not* reproduce for the
// literal "ref call + temp decl in the same block" shape alone. The actual
// trigger is reading a T1b block-scoped `temp` (`~ { … }`) from *outside*
// its own block — LIR lowering's fallback for "temp not currently visible"
// (`lower_path`'s `SymbolKind::Temp` arm, kept for inklecate-compat
// classic-temp forward-reference emulation) previously caught this case
// too, silently emitting a phantom hashed `GetGlobal`/`RefGlobal` id that
// was never registered as a real global — exactly the reported
// `UnresolvedGlobal` fault, with zero compile diagnostic. This matches the
// `tests/tier1-brink/annotations-mixed` fixture's own account of the bug
// (docs comment there: rewritten from a `~ { … }` block to standalone `~`
// logic lines "specifically to avoid tripping this bug").

#[test]
fn block_scoped_temp_read_after_block_closes_is_e082_not_a_silent_phantom_global() {
    let src = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n}\n{name}\n-> END\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e082 = find_diag(&diags, brink_ir::DiagnosticCode::E082)
        .expect("expected E082 for a block-scoped temp read after its block closed");
    assert_eq!(e082.code.severity(), brink_ir::Severity::Error);
    // `lower_to_program` is total — it still returns `Some` even with an
    // Error-severity diagnostic recorded (the `brink-db` `lir_query` layer
    // is what refuses to hand back a `Program`, not `lower_to_program`
    // itself, matching the E057/E059 precedent above).
    let program = program.expect("lower_to_program is total — it still returns Some");
    // The pre-fix behavior emitted `Expr::GetGlobal(<hash of "name">)` here
    // — a `DefinitionId` never present in `program.globals` (only `gold`
    // is). Confirm that phantom id is gone: `gold` is still the only
    // global, and it never masquerades as the temp's storage.
    assert_eq!(program.globals.len(), 1);
    assert_eq!(program.globals[0].id, find_global(&program, "gold").id);
}

#[test]
fn block_scoped_temp_passed_by_ref_after_block_closes_is_e082() {
    // Same defect, reached through `lower_call_args`'s `ref`-argument path
    // instead of `lower_path` — `name` is passed by `ref` to `heal` after
    // its declaring block has closed.
    let src = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n}\n~ heal(name)\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    let e082 = find_diag(&diags, brink_ir::DiagnosticCode::E082)
        .expect("expected E082 for an out-of-scope block temp passed by ref");
    assert_eq!(e082.code.severity(), brink_ir::Severity::Error);
    assert!(
        program.is_some(),
        "lower_to_program is total — it still returns Some"
    );
}

#[test]
fn ref_argument_call_with_temp_decl_in_the_same_block_compiles_clean() {
    // The issue's literal minimal repro shape — a `ref`-argument call
    // co-occurring with a `temp` decl in the same `~ { … }` block — with the
    // temp used only inside its own block (never read after the block
    // closes). This must compile with no E082 and resolve `gold`'s `ref`
    // argument to the real global slot, proving the fix doesn't regress the
    // shape #680 was originally filed against.
    let src = "VAR gold = 100\n~ {\n    temp x = 1\n    heal(gold)\n}\nDone.\n-> END\n\n\
               === function heal(ref hp) ===\n~ return hp\n";
    let (program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E082).is_none(),
        "temp used only within its own block must never trigger E082: {diags:?}"
    );
    let program = program.expect("well-formed program lowers cleanly");
    let gold_id = find_global(&program, "gold").id;
    let heal = find_child(&program.root, "heal");
    let call = program
        .root
        .body
        .iter()
        .find_map(|s| match s {
            lir::Stmt::ExprStmt(e @ lir::Expr::Call { .. }) => Some(e),
            _ => None,
        })
        .expect("heal(gold) should lower to an ExprStmt(Call)");
    match call {
        lir::Expr::Call { target, args } => {
            assert_eq!(*target, heal.id);
            assert_eq!(args.len(), 1);
            match &args[0] {
                lir::CallArg::RefGlobal(id) => assert_eq!(*id, gold_id),
                lir::CallArg::RefTemp(..) => panic!("expected RefGlobal(gold), got RefTemp"),
                lir::CallArg::Value(_) => panic!("expected RefGlobal(gold), got Value"),
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn block_scoped_temp_visible_for_the_rest_of_its_own_block_no_false_positive() {
    // Nested scopes (an `if` inside the block) must still see the outer
    // block's temp — E082 is only for reads *after* the declaring block has
    // fully closed, never for a live nested scope.
    let src = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n    if gold > 0 {\n        temp y = name\n    }\n}\nDone.\n-> END\n";
    let (_program, diags) = lower_ink_with_warnings(src);
    assert!(
        find_diag(&diags, brink_ir::DiagnosticCode::E082).is_none(),
        "a nested scope must still see the outer block's live temp: {diags:?}"
    );
}
