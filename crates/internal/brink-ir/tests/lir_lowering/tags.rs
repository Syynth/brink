use crate::support::*;
use brink_ir::lir;

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
