#![allow(clippy::unwrap_used, clippy::panic)]

//! Reviewer finding on #1732 (issue #1716): `add_template_line` collapsed
//! whitespace on top-level `LinePart::Literal` parts only — the
//! `other => other` fallback structurally ignored `LinePart::Span`, so a
//! literal nested *inside* a span never got whitespace-collapsed
//! (`<b>a  b</b>` shipped as `"a  b"` while `a  b` outside a span
//! collapsed to `"a b"`). This proves the fix: a span's children get the
//! same collapse treatment as top-level literals, recursively.

use brink_format::{CountingFlags, DefinitionId, DefinitionTag, LineContent, LinePart};
use brink_ir::lir;

fn root_id() -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, 1)
}

/// A placeholder provenance (issue #3183) — this fixture has no real
/// source text behind it.
fn test_provenance() -> brink_ir::Provenance {
    brink_ir::Provenance::synthetic(brink_ir::NodeClass::Stmt, rowan::TextRange::empty(0.into()))
}

/// A minimal `Program` whose root body emits a single recognized template
/// line built from `parts` — enough surface for `emit()` to walk without
/// hitting any other (irrelevant) codegen path.
fn program_with_template_line(parts: Vec<LinePart>) -> lir::Program {
    let emission = lir::ContentEmission {
        line: lir::RecognizedLine::Template {
            parts,
            slot_exprs: Vec::new(),
        },
        metadata: lir::LineMetadata {
            source_hash: 0,
            slot_info: Vec::new(),
            source_location: None,
        },
        tags: Vec::new(),
    };
    lir::Program {
        root: lir::Container {
            id: root_id(),
            provenance: test_provenance(),
            name: None,
            kind: lir::ContainerKind::Root,
            params: Vec::new(),
            body: vec![
                lir::Stmt::new(lir::StmtKind::EmitLine(emission), test_provenance()),
                lir::Stmt::new(lir::StmtKind::EndOfLine, test_provenance()),
            ],
            children: Vec::new(),
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
            labeled: false,
            inline: false,
            is_function: false,
            local: false,
        },
        globals: Vec::new(),
        lists: Vec::new(),
        list_items: Vec::new(),
        externals: Vec::new(),
        name_table: Vec::new(),
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        aliases: Vec::new(),
    }
}

#[test]
fn span_children_get_whitespace_collapsed_like_top_level_literals() {
    let parts = vec![LinePart::Span {
        name: "b".to_string(),
        attrs: Vec::new(),
        children: vec![LinePart::Literal("a  b".to_string())],
    }];
    let program = program_with_template_line(parts);
    let story = brink_codegen_inkb::emit(&program).expect("expected Ok");

    let line = &story.line_tables[0].lines[0];
    let LineContent::Template(emitted_parts) = &line.content else {
        panic!("expected Template content, got {:?}", line.content);
    };
    let LinePart::Span { children, .. } = &emitted_parts[0] else {
        panic!("expected Span part, got {:?}", emitted_parts[0]);
    };
    let LinePart::Literal(text) = &children[0] else {
        panic!("expected Literal child, got {:?}", children[0]);
    };
    assert_eq!(
        text, "a b",
        "literal nested inside a span must have its whitespace collapsed \
         the same way a top-level literal does"
    );
}

#[test]
fn nested_span_children_also_get_collapsed() {
    // Two levels of nesting — proves the recursion isn't just one level deep.
    let parts = vec![LinePart::Span {
        name: "outer".to_string(),
        attrs: Vec::new(),
        children: vec![LinePart::Span {
            name: "inner".to_string(),
            attrs: Vec::new(),
            children: vec![LinePart::Literal("x   y".to_string())],
        }],
    }];
    let program = program_with_template_line(parts);
    let story = brink_codegen_inkb::emit(&program).expect("expected Ok");

    let line = &story.line_tables[0].lines[0];
    let LineContent::Template(emitted_parts) = &line.content else {
        panic!("expected Template content, got {:?}", line.content);
    };
    let LinePart::Span { children, .. } = &emitted_parts[0] else {
        panic!("expected outer Span part, got {:?}", emitted_parts[0]);
    };
    let LinePart::Span {
        children: inner_children,
        ..
    } = &children[0]
    else {
        panic!("expected inner Span part, got {:?}", children[0]);
    };
    let LinePart::Literal(text) = &inner_children[0] else {
        panic!("expected Literal child, got {:?}", inner_children[0]);
    };
    assert_eq!(text, "x y");
}
