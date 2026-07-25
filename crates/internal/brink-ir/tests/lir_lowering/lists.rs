use crate::support::*;
use brink_ir::lir;

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
