//! Inline markup at the wire-recognition layer (#1716,
//! `docs/prose-dialect-spec.md` §4.4): a native `.brink` line carrying a
//! `<span>` is admitted to `try_recognize`'s `Template` path as a real
//! `LinePart::Span`, and — the ⚠⚠-ruled precondition — is
//! **hash-transparent**: markup normalizes out of `source_hash` exactly as
//! interpolation already does.
//!
//! Full pipeline (unlike `b07_native_body.rs`, which stops at HIR): native
//! parse → `lower_native::lower` → `normalize_file` → `brink_analyzer::analyze`
//! → `lir::lower_to_program_with_type_mode` → walk the resulting
//! `lir::Program` for the `EmitLine`/`RecognizedLine::Template` this fixture's
//! one content line produces.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. This
// file's one use (an always-empty `file_paths` map handed to
// `lower_to_program_with_type_mode` — this test never populates
// `SourceLocation`) has no order to leak; `crate::determinism::LookupMap`
// is `pub(crate)` and invisible to this external test-binary crate, so a
// file-level allow is the narrower fix — same precedent
// `tests/lir_lowering.rs` already established for the identical parameter.
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map, no order to leak — see file doc"
)]

use brink_ir::{FileId, HirFile, SymbolManifest, hir::lower_native, lir};

fn lower_native_to_program(source: &str) -> lir::Program {
    let parse = brink_syntax_native::parse(source);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let tree = parse.tree();
    let file_id = FileId(0);
    let (mut hir, manifest, diags) = lower_native::lower(file_id, &tree);
    assert!(diags.is_empty(), "fixture must lower cleanly: {diags:?}");
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let (program, _warnings) = lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &lir::UfcsLookup::new(),
            coalesce: &lir::CoalesceLookup::new(),
        },
    );
    program.expect("fixture should produce a program")
}

fn find_flow_f(program: &lir::Program) -> &lir::Container {
    program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("f"))
        .expect("flow `f` container")
}

fn find_root_template(program: &lir::Program) -> (Vec<brink_format::LinePart>, u64) {
    let root = find_flow_f(program);
    for stmt in &root.body {
        if let lir::Stmt::EmitLine(e) = stmt
            && let lir::RecognizedLine::Template { parts, .. } = &e.line
        {
            return (parts.clone(), e.metadata.source_hash);
        }
    }
    panic!(
        "no recognized Template line found ({} stmts in body)",
        root.body.len()
    );
}

#[test]
fn a_span_is_recognized_as_a_real_wire_line_part_span() {
    let program = lower_native_to_program(
        "flow f() {\n  He hands you <item id=\"lantern\">the lantern</item>.\n}\n",
    );
    let (parts, _hash) = find_root_template(&program);
    assert!(matches!(&parts[0], brink_format::LinePart::Literal(s) if s == "He hands you "));
    let brink_format::LinePart::Span {
        name,
        attrs,
        children,
    } = &parts[1]
    else {
        panic!("expected LinePart::Span, got {:?}", parts[1]);
    };
    assert_eq!(name, "item");
    assert_eq!(attrs, &vec![("id".to_string(), "lantern".to_string())]);
    assert_eq!(children.len(), 1);
    assert!(matches!(&children[0], brink_format::LinePart::Literal(s) if s == "the lantern"));
    assert!(matches!(&parts[2], brink_format::LinePart::Literal(s) if s == "."));
}

#[test]
fn span_hash_transparency_markup_normalizes_out_of_source_hash() {
    // The ⚠⚠-ruled precondition (§4.4): `Hello <wave>world</wave>` hashes
    // identically to `Hello world` — a translated line must not re-key the
    // moment an author bolds a word.
    let with_span = lower_native_to_program("flow f() {\n  Hello <wave>world</wave>\n}\n");
    let without_span = lower_native_to_program("flow f() {\n  Hello world\n}\n");

    let (_parts, hash_with_span) = find_root_template(&with_span);
    // The plain line recognizes as `Plain`, not `Template` — read its hash
    // off `EmitLine` directly.
    let root = find_flow_f(&without_span);
    let hash_without_span = root
        .body
        .iter()
        .find_map(|s| match s {
            lir::Stmt::EmitLine(e) => Some(e.metadata.source_hash),
            _ => None,
        })
        .expect("plain EmitLine");

    assert_eq!(
        hash_with_span, hash_without_span,
        "markup must normalize out of source_hash — a bolded word must not re-key the line"
    );
}

#[test]
fn a_point_marker_span_lowers_with_empty_children() {
    let program = lower_native_to_program("flow f() {\n  Bell tolls. <pause/> Door slams.\n}\n");
    let (parts, _hash) = find_root_template(&program);
    let span = parts
        .iter()
        .find_map(|p| match p {
            brink_format::LinePart::Span { name, children, .. } if name == "pause" => {
                Some(children)
            }
            _ => None,
        })
        .expect("expected a <pause/> LinePart::Span");
    assert!(span.is_empty());
}

#[test]
fn a_span_may_contain_interpolation_and_still_recognizes_as_template() {
    let program = lower_native_to_program("flow f(name) {\n  <b>hello {name}</b>\n}\n");
    let (parts, _hash) = find_root_template(&program);
    let brink_format::LinePart::Span { children, .. } = &parts[0] else {
        panic!("expected LinePart::Span, got {:?}", parts[0]);
    };
    assert!(matches!(&children[0], brink_format::LinePart::Literal(s) if s == "hello "));
    assert!(matches!(&children[1], brink_format::LinePart::Slot(0)));
}

/// Reviewer finding on #1732 (issue #1716): a line whose *entire* content is
/// a childless, point-marker span (§8b.11's `<pause/>` shape) with no
/// surrounding text used to be silently declined by `try_recognize_template`
/// (it requires ≥1 non-whitespace `Text` part, which a bare `<pause/>` line
/// has none of) and fall to `EmitContent`'s flattening path, which for a
/// childless span drops `name`/`attrs` and appends nothing — the whole line
/// vanished with no diagnostic. A `Span`'s mere presence must count as real
/// content, the same way `content_has_nonempty_text` already does, so this
/// now recognizes as a real wire `LinePart::Template` carrying the span.
#[test]
fn a_lone_point_marker_span_is_admitted_to_template_recognition() {
    let program = lower_native_to_program("flow f() {\n  <pause/>\n}\n");
    let (parts, _hash) = find_root_template(&program);
    assert_eq!(parts.len(), 1, "expected exactly one LinePart, got {parts:?}");
    let brink_format::LinePart::Span {
        name,
        attrs,
        children,
    } = &parts[0]
    else {
        panic!("expected LinePart::Span, got {:?}", parts[0]);
    };
    assert_eq!(name, "pause");
    assert!(attrs.is_empty());
    assert!(children.is_empty());
}
