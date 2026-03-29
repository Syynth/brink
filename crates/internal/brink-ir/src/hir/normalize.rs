//! HIR normalization pass: lift inline sequences/conditionals to block-level.
//!
//! This runs on a **cloned** HIR before LIR lowering. The stored HIR in the
//! project DB stays pristine — the LSP sees the original structure.
//!
//! The transform expands inline `InlineSequence` / `InlineConditional` content
//! parts into block-level `Sequence` / `Conditional` statements. Each branch
//! gets the surrounding text spliced in, producing complete content lines that
//! the recognizer can match as `Plain` or `Template`.

use super::types::{
    Block, CondBranch, Conditional, Content, ContentPart, HirFile, Sequence, SequenceType, Stmt,
    Tag,
};

// ─── Public entry point ─────────────────────────────────────────────

/// Normalize an entire HIR file by lifting inline sequences/conditionals
/// in all blocks (root, knot bodies, stitch bodies).
pub fn normalize_file(hir: &mut HirFile) {
    normalize_block(&mut hir.root_content);
    for knot in &mut hir.knots {
        normalize_block(&mut knot.body);
        for stitch in &mut knot.stitches {
            normalize_block(&mut stitch.body);
        }
    }
}

// ─── Block normalization ────────────────────────────────────────────

/// Walk a block's statements, lifting inline constructs to block-level
/// and recursing into contained blocks.
fn normalize_block(block: &mut Block) {
    let old_stmts = std::mem::take(&mut block.stmts);
    let mut new_stmts = Vec::with_capacity(old_stmts.len());

    let mut iter = old_stmts.into_iter().peekable();
    while let Some(stmt) = iter.next() {
        match stmt {
            Stmt::Content(content) => {
                // Check if the next stmt is EndOfLine — we absorb it into branches.
                let trailing_eol = matches!(iter.peek(), Some(Stmt::EndOfLine));

                match try_lift_inline(content, trailing_eol) {
                    Ok(lifted_stmts) => {
                        // Consume the EndOfLine we peeked at.
                        if trailing_eol {
                            let _ = iter.next();
                        }
                        new_stmts.extend(lifted_stmts);
                    }
                    Err(content) => {
                        // No inline construct — pass through.
                        new_stmts.push(Stmt::Content(content));
                    }
                }
            }
            // Recurse into contained blocks for all structural statements.
            Stmt::ChoiceSet(mut cs) => {
                for choice in &mut cs.choices {
                    normalize_block(&mut choice.body);
                }
                normalize_block(&mut cs.continuation);
                new_stmts.push(Stmt::ChoiceSet(cs));
            }
            Stmt::LabeledBlock(mut lb) => {
                normalize_block(&mut lb);
                new_stmts.push(Stmt::LabeledBlock(lb));
            }
            Stmt::Conditional(mut cond) => {
                for branch in &mut cond.branches {
                    normalize_block(&mut branch.body);
                }
                new_stmts.push(Stmt::Conditional(cond));
            }
            Stmt::Sequence(mut seq) => {
                for branch in &mut seq.branches {
                    normalize_block(branch);
                }
                new_stmts.push(Stmt::Sequence(seq));
            }
            other => new_stmts.push(other),
        }
    }

    block.stmts = new_stmts;

    // Recurse into any newly created Sequence/Conditional branches
    // (handles cartesian product from multiple inline constructs).
    for stmt in &mut block.stmts {
        match stmt {
            Stmt::Sequence(seq) => {
                for branch in &mut seq.branches {
                    normalize_block(branch);
                }
            }
            Stmt::Conditional(cond) => {
                for branch in &mut cond.branches {
                    normalize_block(&mut branch.body);
                }
            }
            _ => {}
        }
    }
}

// ─── Inline lifting ─────────────────────────────────────────────────

/// Try to lift the first `InlineSequence` or `InlineConditional` from a
/// Content's parts into a block-level statement.
///
/// Returns `Ok(stmts)` with the replacement statements, or `Err(content)`
/// if no inline construct was found (caller passes through unchanged).
fn try_lift_inline(content: Content, trailing_eol: bool) -> Result<Vec<Stmt>, Content> {
    // Find the first inline construct.
    let inline_idx = content.parts.iter().position(|p| {
        matches!(
            p,
            ContentPart::InlineSequence(_) | ContentPart::InlineConditional(_)
        )
    });

    let Some(idx) = inline_idx else {
        return Err(content);
    };

    let prefix: Vec<ContentPart> = content.parts[..idx].to_vec();
    let suffix: Vec<ContentPart> = content.parts[idx + 1..].to_vec();
    let tags = &content.tags;
    let ptr = content.ptr;

    match &content.parts[idx] {
        ContentPart::InlineSequence(seq) => {
            let mut branches = Vec::with_capacity(seq.branches.len() + 1);
            for branch in &seq.branches {
                let mut b = branch.clone();
                splice_around(&mut b, &prefix, &suffix, tags, ptr);
                if trailing_eol {
                    b.stmts.push(Stmt::EndOfLine);
                }
                branches.push(b);
            }

            // `once` sequences exhaust their branches and then produce nothing.
            // When prefix/suffix text exists, it must still be emitted after
            // exhaustion. Add an extra "exhausted" branch with just prefix+suffix
            // and change to `stopping` so the last branch repeats forever.
            //
            // This is only valid for plain `once` (sequential). `shuffle | once`
            // would shuffle the extra branch into the pool — skip the conversion
            // and fall back to the existing inline sequence lowering for that case.
            let is_plain_once =
                seq.kind.contains(SequenceType::ONCE) && !seq.kind.contains(SequenceType::SHUFFLE);
            let kind = if is_plain_once && (!prefix.is_empty() || !suffix.is_empty()) {
                let mut exhausted = Block::default();
                splice_around(&mut exhausted, &prefix, &suffix, tags, ptr);
                if trailing_eol {
                    exhausted.stmts.push(Stmt::EndOfLine);
                }
                branches.push(exhausted);
                // Replace `once` with `stopping` so the exhausted branch repeats.
                (seq.kind & !SequenceType::ONCE) | SequenceType::STOPPING
            } else {
                seq.kind
            };

            Ok(vec![Stmt::Sequence(Sequence {
                ptr: seq.ptr,
                kind,
                branches,
                container_id: None,
            })])
        }
        ContentPart::InlineConditional(cond) => {
            let mut branches = Vec::with_capacity(cond.branches.len() + 1);
            for branch in &cond.branches {
                let mut body = branch.body.clone();
                splice_around(&mut body, &prefix, &suffix, tags, ptr);
                if trailing_eol {
                    body.stmts.push(Stmt::EndOfLine);
                }
                branches.push(CondBranch {
                    condition: branch.condition.clone(),
                    body,
                    container_id: None,
                });
            }

            // If no else branch exists and there's prefix/suffix text that
            // must be emitted even when all conditions are false, add an else
            // branch with just the surrounding text. Without this, text like
            // "A " in `A {cond:B}` would be lost when `cond` is false.
            let has_else = branches.iter().any(|b| b.condition.is_none());
            if !has_else && (!prefix.is_empty() || !suffix.is_empty()) {
                let mut else_body = Block::default();
                splice_around(&mut else_body, &prefix, &suffix, tags, ptr);
                if trailing_eol {
                    else_body.stmts.push(Stmt::EndOfLine);
                }
                branches.push(CondBranch {
                    condition: None,
                    body: else_body,
                    container_id: None,
                });
            }

            Ok(vec![Stmt::Conditional(Conditional {
                ptr: cond.ptr,
                kind: cond.kind.clone(),
                branches,
            })])
        }
        _ => unreachable!("position() matched only InlineSequence/InlineConditional"),
    }
}

// ─── Splice helper ──────────────────────────────────────────────────

/// Splice prefix/suffix text around a branch block's content.
///
/// Handles these cases:
/// - **Single Content stmt**: parts = prefix + original + suffix, merge tags
/// - **Empty block**: create new Content with prefix + suffix
/// - **Multiple stmts, first is Content**: prepend prefix to first Content's parts
/// - **Multiple stmts, last is Content**: append suffix to last Content's parts
/// - **No Content stmts** (e.g., just Divert): insert new Content at position 0
fn splice_around(
    block: &mut Block,
    prefix: &[ContentPart],
    suffix: &[ContentPart],
    tags: &[Tag],
    ptr: Option<brink_syntax::ast::SyntaxNodePtr>,
) {
    let has_prefix = !prefix.is_empty();
    let has_suffix = !suffix.is_empty();

    if !has_prefix && !has_suffix && tags.is_empty() {
        return;
    }

    // Empty block — create a new Content with prefix + suffix.
    if block.stmts.is_empty() {
        let mut parts = prefix.to_vec();
        parts.extend_from_slice(suffix);
        if !parts.is_empty() || !tags.is_empty() {
            block.stmts.push(Stmt::Content(Content {
                ptr,
                parts,
                tags: tags.to_vec(),
            }));
        }
        return;
    }

    // Single Content stmt — splice into it directly.
    if block.stmts.len() == 1
        && let Stmt::Content(ref mut c) = block.stmts[0]
    {
        let mut new_parts = prefix.to_vec();
        new_parts.append(&mut c.parts);
        new_parts.extend_from_slice(suffix);
        c.parts = new_parts;
        c.tags.extend_from_slice(tags);
        if c.ptr.is_none() {
            c.ptr = ptr;
        }
        return;
    }

    // Multiple stmts — find first and last Content to splice prefix/suffix.
    let first_content_idx = block
        .stmts
        .iter()
        .position(|s| matches!(s, Stmt::Content(_)));
    let last_content_idx = block
        .stmts
        .iter()
        .rposition(|s| matches!(s, Stmt::Content(_)));

    if let (Some(first), Some(last)) = (first_content_idx, last_content_idx) {
        // Prepend prefix to first Content.
        if has_prefix && let Stmt::Content(ref mut c) = block.stmts[first] {
            let mut new_parts = prefix.to_vec();
            new_parts.append(&mut c.parts);
            c.parts = new_parts;
            c.tags.extend_from_slice(tags);
            if c.ptr.is_none() {
                c.ptr = ptr;
            }
        } else if !tags.is_empty()
            && let Stmt::Content(ref mut c) = block.stmts[first]
        {
            c.tags.extend_from_slice(tags);
        }
        // Append suffix to last Content.
        if has_suffix && let Stmt::Content(ref mut c) = block.stmts[last] {
            c.parts.extend_from_slice(suffix);
        }
    } else {
        // No Content stmts at all — insert a new Content at position 0.
        let mut parts = prefix.to_vec();
        parts.extend_from_slice(suffix);
        if !parts.is_empty() || !tags.is_empty() {
            block.stmts.insert(
                0,
                Stmt::Content(Content {
                    ptr,
                    parts,
                    tags: tags.to_vec(),
                }),
            );
        }
    }
}

#[cfg(test)]
#[expect(clippy::panic, clippy::items_after_statements)]
mod tests {
    use brink_syntax::ast::{AstNode, SyntaxNodePtr};

    use super::super::types::*;
    use super::normalize_file;

    // ─── Helpers ────────────────────────────────────────────────────

    fn dummy_ptr() -> SyntaxNodePtr {
        let parsed = brink_syntax::parse("hello\n");
        let root = parsed.tree().syntax().clone();
        SyntaxNodePtr::from_node(&root)
    }

    fn dummy_tag_ptr() -> brink_syntax::ast::AstPtr<brink_syntax::ast::Tag> {
        let parsed = brink_syntax::parse("hello #tag\n");
        let root = parsed.tree().syntax().clone();
        // Find the Tag node in the tree.
        fn find_tag(
            node: &brink_syntax::SyntaxNode,
        ) -> Option<brink_syntax::ast::AstPtr<brink_syntax::ast::Tag>> {
            use brink_syntax::ast::AstNode;
            if let Some(tag) = brink_syntax::ast::Tag::cast(node.clone()) {
                return Some(brink_syntax::ast::AstPtr::new(&tag));
            }
            for child in node.children() {
                if let Some(ptr) = find_tag(&child) {
                    return Some(ptr);
                }
            }
            None
        }
        find_tag(&root).expect("should find Tag node in 'hello #tag'")
    }

    fn dummy_choice_ptr() -> brink_syntax::ast::AstPtr<brink_syntax::ast::Choice> {
        let parsed = brink_syntax::parse("* choice\n");
        let root = parsed.tree().syntax().clone();
        fn find_choice(
            node: &brink_syntax::SyntaxNode,
        ) -> Option<brink_syntax::ast::AstPtr<brink_syntax::ast::Choice>> {
            use brink_syntax::ast::AstNode;
            if let Some(choice) = brink_syntax::ast::Choice::cast(node.clone()) {
                return Some(brink_syntax::ast::AstPtr::new(&choice));
            }
            for child in node.children() {
                if let Some(ptr) = find_choice(&child) {
                    return Some(ptr);
                }
            }
            None
        }
        find_choice(&root).expect("should find Choice node in '* choice'")
    }

    fn text(s: &str) -> ContentPart {
        ContentPart::Text(s.to_string())
    }

    fn mk_content(parts: Vec<ContentPart>) -> Content {
        Content {
            ptr: Some(dummy_ptr()),
            parts,
            tags: Vec::new(),
        }
    }

    fn mk_content_with_tags(parts: Vec<ContentPart>, tags: Vec<Tag>) -> Content {
        Content {
            ptr: Some(dummy_ptr()),
            parts,
            tags,
        }
    }

    fn mk_inline_seq(kind: SequenceType, branches: Vec<Vec<ContentPart>>) -> ContentPart {
        let ptr = dummy_ptr();
        ContentPart::InlineSequence(Sequence {
            ptr,
            kind,
            branches: branches
                .into_iter()
                .map(|parts| Block {
                    label: None,
                    stmts: if parts.is_empty() {
                        Vec::new()
                    } else {
                        vec![Stmt::Content(Content {
                            ptr: Some(ptr),
                            parts,
                            tags: Vec::new(),
                        })]
                    },
                    container_id: None,
                })
                .collect(),
            container_id: None,
        })
    }

    fn mk_inline_cond(branches: Vec<(Option<Expr>, Vec<ContentPart>)>) -> ContentPart {
        let ptr = dummy_ptr();
        ContentPart::InlineConditional(Conditional {
            ptr,
            kind: CondKind::InitialCondition,
            branches: branches
                .into_iter()
                .map(|(condition, parts)| CondBranch {
                    condition,
                    body: Block {
                        label: None,
                        stmts: if parts.is_empty() {
                            Vec::new()
                        } else {
                            vec![Stmt::Content(Content {
                                ptr: Some(ptr),
                                parts,
                                tags: Vec::new(),
                            })]
                        },
                        container_id: None,
                    },
                    container_id: None,
                })
                .collect(),
        })
    }

    fn mk_tag(s: &str) -> Tag {
        Tag {
            parts: vec![ContentPart::Text(s.to_string())],
            ptr: dummy_tag_ptr(),
        }
    }

    fn mk_block(stmts: Vec<Stmt>) -> Block {
        Block { label: None, stmts, container_id: None }
    }

    fn mk_hir(stmts: Vec<Stmt>) -> HirFile {
        HirFile {
            root_content: mk_block(stmts),
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
        }
    }

    /// Extract the text parts from a Content stmt, concatenated.
    fn content_text(content: &Content) -> String {
        content
            .parts
            .iter()
            .filter_map(|p| {
                if let ContentPart::Text(s) = p {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    // ─── Tests ──────────────────────────────────────────────────────

    #[test]
    fn simple_sequence_lift() {
        // "It's " + {stopping: "a fine", "a good"} + " day."
        let content = mk_content(vec![
            text("It's "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("a fine")], vec![text("a good")]],
            ),
            text(" day."),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        // Should be a single Sequence stmt.
        assert_eq!(hir.root_content.stmts.len(), 1);
        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence, got {:?}", hir.root_content.stmts[0]);
        };
        assert_eq!(seq.kind, SequenceType::STOPPING);
        assert_eq!(seq.branches.len(), 2);

        // Branch 0: Content("It's a fine day.") + EndOfLine
        assert_eq!(seq.branches[0].stmts.len(), 2);
        let Stmt::Content(c0) = &seq.branches[0].stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c0), "It's a fine day.");
        assert!(matches!(seq.branches[0].stmts[1], Stmt::EndOfLine));

        // Branch 1: Content("It's a good day.") + EndOfLine
        let Stmt::Content(c1) = &seq.branches[1].stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c1), "It's a good day.");
        assert!(matches!(seq.branches[1].stmts[1], Stmt::EndOfLine));
    }

    #[test]
    fn simple_conditional_lift() {
        // "I'm " + {happy: "very", "not"} + " pleased."
        let cond_expr = Expr::Bool(true);
        let content = mk_content(vec![
            text("I'm "),
            mk_inline_cond(vec![
                (Some(cond_expr), vec![text("very")]),
                (None, vec![text("not")]),
            ]),
            text(" pleased."),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        assert_eq!(hir.root_content.stmts.len(), 1);
        let Stmt::Conditional(cond) = &hir.root_content.stmts[0] else {
            panic!("expected Conditional");
        };
        assert_eq!(cond.branches.len(), 2);

        let Stmt::Content(c0) = &cond.branches[0].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c0), "I'm very pleased.");

        let Stmt::Content(c1) = &cond.branches[1].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c1), "I'm not pleased.");
    }

    #[test]
    fn tag_propagation() {
        let content = mk_content_with_tags(
            vec![
                text("Hello "),
                mk_inline_seq(
                    SequenceType::CYCLE,
                    vec![vec![text("world")], vec![text("there")]],
                ),
            ],
            vec![mk_tag("greeting")],
        );
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence");
        };

        // Tags should be on the first content of each branch.
        let Stmt::Content(c0) = &seq.branches[0].stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(c0.tags.len(), 1);

        let Stmt::Content(c1) = &seq.branches[1].stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(c1.tags.len(), 1);
    }

    #[test]
    fn eol_absorption() {
        // Without trailing EOL — no EndOfLine in branches.
        let content = mk_content(vec![
            text("a "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("x")], vec![text("y")]],
            ),
            text(" b"),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content)]);
        normalize_file(&mut hir);

        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence");
        };
        // No EndOfLine since there was no trailing EOL.
        assert_eq!(seq.branches[0].stmts.len(), 1);
    }

    #[test]
    fn empty_branch_gets_prefix_suffix() {
        // "It's " + {stopping: "a", "", "c"} + " fine"
        let content = mk_content(vec![
            text("It's "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("a")], vec![], vec![text("c")]],
            ),
            text(" fine"),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence");
        };
        assert_eq!(seq.branches.len(), 3);

        // Branch 1 (empty) should still get "It's  fine".
        let Stmt::Content(c1) = &seq.branches[1].stmts[0] else {
            panic!("expected Content in empty branch");
        };
        assert_eq!(content_text(c1), "It's  fine");
    }

    #[test]
    fn no_inline_passes_through() {
        let content = mk_content(vec![text("Just plain text.")]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        // Should be unchanged: Content + EndOfLine.
        assert_eq!(hir.root_content.stmts.len(), 2);
        assert!(matches!(hir.root_content.stmts[0], Stmt::Content(_)));
        assert!(matches!(hir.root_content.stmts[1], Stmt::EndOfLine));
    }

    #[test]
    fn recursion_into_choice_body() {
        // A choice with an inline sequence in its body.
        let body_content = mk_content(vec![
            text("It's "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("a")], vec![text("b")]],
            ),
        ]);
        let choice = Choice {
            ptr: dummy_choice_ptr(),
            is_sticky: false,
            is_fallback: false,
            label: None,
            condition: None,
            start_content: Some(mk_content(vec![text("Pick")])),
            bracket_content: None,
            inner_content: None,
            tags: Vec::new(),
            body: mk_block(vec![Stmt::Content(body_content), Stmt::EndOfLine]),
            container_id: None,
        };
        let cs = ChoiceSet {
            choices: vec![choice],
            continuation: mk_block(vec![]),
            context: ChoiceSetContext::Weave,
            depth: 1,
            gather_id: None,
        };
        let mut hir = mk_hir(vec![Stmt::ChoiceSet(Box::new(cs))]);
        normalize_file(&mut hir);

        // The choice body should have been normalized.
        let Stmt::ChoiceSet(ref cs) = hir.root_content.stmts[0] else {
            panic!("expected ChoiceSet");
        };
        assert_eq!(cs.choices[0].body.stmts.len(), 1);
        assert!(matches!(cs.choices[0].body.stmts[0], Stmt::Sequence(_)));
    }

    #[test]
    fn recursion_into_conditional_branches() {
        let body_content = mk_content(vec![
            text("Hello "),
            mk_inline_seq(SequenceType::CYCLE, vec![vec![text("x")], vec![text("y")]]),
        ]);
        let cond = Conditional {
            ptr: dummy_ptr(),
            kind: CondKind::IfElse,
            branches: vec![CondBranch {
                condition: Some(Expr::Bool(true)),
                body: mk_block(vec![Stmt::Content(body_content), Stmt::EndOfLine]),
                container_id: None,
            }],
        };
        let mut hir = mk_hir(vec![Stmt::Conditional(cond)]);
        normalize_file(&mut hir);

        let Stmt::Conditional(ref c) = hir.root_content.stmts[0] else {
            panic!("expected Conditional");
        };
        // The branch body should have been normalized — Sequence instead of Content+EOL.
        assert_eq!(c.branches[0].body.stmts.len(), 1);
        assert!(matches!(c.branches[0].body.stmts[0], Stmt::Sequence(_)));
    }
}
