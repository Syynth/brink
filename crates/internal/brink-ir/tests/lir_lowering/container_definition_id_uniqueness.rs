use crate::support::*;
use brink_ir::lir;

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
    let mut seen: std::collections::BTreeMap<brink_format::DefinitionId, Vec<&str>> =
        std::collections::BTreeMap::new();
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

    let mut seen: std::collections::BTreeMap<brink_format::DefinitionId, Vec<&str>> =
        std::collections::BTreeMap::new();
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
        matches!(&s.kind, lir::StmtKind::EmitLine(e) if matches!(&e.line, lir::RecognizedLine::Template { .. }))
    });
    assert!(
        has_template,
        "content with non-whitespace text between slots should be Template",
    );
}
