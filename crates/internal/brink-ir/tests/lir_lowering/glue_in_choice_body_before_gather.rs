use crate::support::*;
use brink_ir::lir;

// ─── Glue in choice body before gather ──────────────────────────────

/// Helper: check if a statement list contains a Glue emission.
fn has_glue(stmts: &[lir::Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        lir::StmtKind::EmitContent(c) => {
            c.parts.iter().any(|p| matches!(p, lir::ContentPart::Glue))
        }
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
        match &stmt.kind {
            lir::StmtKind::EmitContent(c) => {
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
            lir::StmtKind::EmitLine(e) => match &e.line {
                lir::RecognizedLine::Plain(s) => {
                    eprintln!("{pad}  stmt[{i}]: EmitLine(Plain({s:?}))");
                }
                lir::RecognizedLine::Template { .. } => {
                    eprintln!("{pad}  stmt[{i}]: EmitLine(Template)");
                }
            },
            lir::StmtKind::Divert(_) => eprintln!("{pad}  stmt[{i}]: Divert"),
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
