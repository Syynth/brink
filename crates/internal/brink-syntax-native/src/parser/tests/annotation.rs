//! Annotations `@[…]`. Family for #1198.
//!
//! No `brink-syntax` parity file exists for this family — annotations are
//! native-only (`AT_L_BRACKET` compound token, NS-A2 lineage,
//! `docs/directive-annotations-spec.md` §5b) — so these tests are scoped
//! directly against that spec's grammar rather than a mirrored ink-side
//! file. §5b's own literal example (`effects(reads(gold, hp), pure)`) is
//! used as the canonical multi-arg/nested fixture throughout.

use super::*;
use proptest::prelude::*;

// `MAX_DEPTH` is private to `parser::mod` but visible here as a descendant
// module — importing the real constant (instead of a hand-copied literal)
// means the depth-fuzz proptest below tracks it if it ever changes, rather
// than silently drifting out of sync with the guard it exercises.
use super::super::MAX_DEPTH;

#[test]
fn annotation_line_parses() {
    // `@[…]` annotations dispatch only through the prose-ground `body_line`
    // (`parser/block.rs`) — the code-ground `STMT_BLOCK` statement grammar
    // has no `AT_L_BRACKET` arm (`parser/stmt.rs::statement`) — so this
    // exercises `fn`'s `>{ }` prose override (charter §4, #1309) rather
    // than its now code-ground default.
    let src = "fn heal(hp) >{\n  @[effects(pure, silent, reads(gold, hp))]\n  var x = hp\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

// ── Structural helpers ────────────────────────────────────────────────

/// Build `@[a(a(a(...(a)...)))]\n` with `depth` levels of nested
/// paren-clauses (`depth` `(` and `depth` `)` characters) — reused by the
/// per-depth unit tests below and the depth-fuzzing proptest.
fn nested_annotation_src(depth: usize) -> String {
    format!("@[a{}{}]\n", "(a".repeat(depth), ")".repeat(depth))
}

/// Walk `depth` levels into the single-arg nested-paren-clause chain
/// `nested_annotation_src` builds, asserting every intermediate level is a
/// bare-ident arg wrapping exactly one further nested `ANNOTATION_ARGS`,
/// and the innermost (`depth`-th) level has no `nested_args` of its own.
/// Returns the leaf ident's text (always `"a"` for this builder, but
/// checked structurally rather than assumed).
fn walk_nested_chain(line: &crate::ast::AnnotationLine, depth: usize) -> String {
    let args = line.args().expect("top-level ANNOTATION_ARGS");
    let mut arg = args.args().next().expect("first ANNOTATION_ARG");
    for _ in 1..depth {
        let nested = arg.nested_args().expect("expected nested ANNOTATION_ARGS");
        arg = nested.args().next().expect("nested ANNOTATION_ARG");
    }
    assert!(
        arg.nested_args().is_none(),
        "innermost arg at depth {depth} must not have further nested args"
    );
    arg.name_token()
        .expect("leaf arg has an IDENT")
        .text()
        .to_string()
}

fn annotation_line_node(p: &Parse) -> crate::ast::AnnotationLine {
    find_child::<crate::ast::AnnotationLine>(&p.syntax())
        .or_else(|| {
            p.syntax()
                .descendants()
                .find_map(crate::ast::AnnotationLine::cast)
        })
        .expect("ANNOTATION_LINE")
}

// ── Exhaustive per-node unit tests ──────────────────────────────────────

#[test]
fn annotation_line_bare_name_no_parens() {
    let src = "@[local]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(line.name_token().unwrap().text(), "local");
    assert!(line.args().is_none(), "no `(` means no ANNOTATION_ARGS");
}

#[test]
fn annotation_args_empty_parens() {
    let src = "@[name()]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    assert_eq!(args.args().count(), 0);
}

#[test]
fn annotation_arg_bare_ident() {
    let src = "@[name(pure)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let arg = args.args().next().expect("ANNOTATION_ARG");
    assert_eq!(arg.name_token().unwrap().text(), "pure");
    assert!(arg.nested_args().is_none());
}

/// The spec's own §5b literal fixture, checked structurally end to end:
/// two top-level args, the first (`reads(gold, hp)`) recursing one level
/// with two bare-ident children, the second (`pure`) a plain leaf.
#[test]
fn annotation_arg_spec_effects_fixture() {
    let src = "@[effects(reads(gold, hp), pure)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(line.name_token().unwrap().text(), "effects");
    let args = line.args().expect("ANNOTATION_ARGS");
    let top: Vec<_> = args.args().collect();
    assert_eq!(top.len(), 2);

    assert_eq!(top[0].name_token().unwrap().text(), "reads");
    let reads_args = top[0].nested_args().expect("`reads(...)` nests");
    let reads_names: Vec<_> = reads_args
        .args()
        .map(|a| a.name_token().unwrap().text().to_string())
        .collect();
    assert_eq!(reads_names, vec!["gold".to_string(), "hp".to_string()]);

    assert_eq!(top[1].name_token().unwrap().text(), "pure");
    assert!(top[1].nested_args().is_none());
}

#[test]
fn annotation_arg_nested_one_level() {
    let src = nested_annotation_src(1);
    let p = assert_lossless(&src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(walk_nested_chain(&line, 1), "a");
}

#[test]
fn annotation_arg_nested_two_levels() {
    let src = nested_annotation_src(2);
    let p = assert_lossless(&src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(walk_nested_chain(&line, 2), "a");
}

#[test]
fn annotation_arg_nested_three_levels() {
    let src = nested_annotation_src(3);
    let p = assert_lossless(&src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(walk_nested_chain(&line, 3), "a");
}

/// A depth well within `MAX_DEPTH` (256) but far past what a hand-written
/// fixture would cover — proves the recursive-descent isn't secretly
/// capped lower than the documented guard.
#[test]
fn annotation_arg_nested_deep_but_within_limit() {
    let depth = 64;
    assert!(u32::try_from(depth).unwrap_or(u32::MAX) < MAX_DEPTH);
    let src = nested_annotation_src(depth);
    let p = assert_lossless(&src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(walk_nested_chain(&line, depth), "a");
}

#[test]
fn annotation_arg_integer_literal() {
    let src = "@[maxlen(80)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let arg = args.args().next().expect("ANNOTATION_ARG");
    let lit = find_child::<crate::ast::IntegerLit>(arg.syntax()).expect("INTEGER_LIT");
    assert_eq!(lit.value(), Some(80));
}

#[test]
fn annotation_arg_float_literal() {
    let src = "@[note(2.5)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let arg = args.args().next().expect("ANNOTATION_ARG");
    let lit = find_child::<crate::ast::FloatLit>(arg.syntax()).expect("FLOAT_LIT");
    assert!((lit.value().expect("float value") - 2.5).abs() < f64::EPSILON * 10.0);
}

#[test]
fn annotation_arg_string_literal() {
    let src = "@[note(\"hi there\")]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let arg = args.args().next().expect("ANNOTATION_ARG");
    let lit = find_child::<crate::ast::StringLit>(arg.syntax()).expect("STRING_LIT");
    // `STRING_TEXT` is a token, not a node — `has_node_kind` only walks
    // `descendants()` (nodes), so check the literal's rendered text instead.
    assert!(lit.syntax().text().to_string().contains("hi there"));
}

/// Issue #1349: the arg to `@[was(...)]` (native rename migration, the
/// closed #1286) must accept an *unquoted* `::`-separated module path, not
/// only the quoted-string form `lower_native`'s `module.rs` already
/// lowers. `story::old::path` must parse to a `PATH` (via
/// `AnnotationArg::path`), not an error.
#[test]
fn annotation_arg_unquoted_module_path() {
    let src = "@[was(story::old::path)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    assert_eq!(line.name_token().unwrap().text(), "was");
    let args = line.args().expect("ANNOTATION_ARGS");
    let arg = args.args().next().expect("ANNOTATION_ARG");

    // The path shape, not the bare-ident shape: no `name_token`, a `path`.
    assert!(arg.name_token().is_none(), "path arg has no bare IDENT");
    let path = arg.path().expect("ANNOTATION_ARG carries a PATH");
    let segments: Vec<_> = path.segments().map(|t| t.text().to_string()).collect();
    assert_eq!(segments, vec!["story", "old", "path"]);
    assert!(
        path.crosses_module_wall(),
        "`::`-separated, not `.`-separated"
    );
}

/// A single-segment path (no `::` at all) is indistinguishable from a bare
/// ident at the grammar level and must keep parsing as the existing
/// bare-ident arg shape — the new `nth(1) == COLON_COLON` lookahead must
/// not misfire on it.
#[test]
fn annotation_arg_single_segment_stays_bare_ident() {
    let src = "@[was(story)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let arg = args.args().next().expect("ANNOTATION_ARG");
    assert_eq!(arg.name_token().unwrap().text(), "story");
    assert!(arg.path().is_none());
}

/// Two path-shaped args in the same annotation, exercising the arg-list
/// comma loop with the new production (not just a single-arg fixture).
#[test]
fn annotation_args_multiple_unquoted_paths() {
    let src = "@[rename(story::old::a, story::old::b)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let paths: Vec<Vec<String>> = args
        .args()
        .map(|a| {
            a.path()
                .expect("ANNOTATION_ARG carries a PATH")
                .segments()
                .map(|t| t.text().to_string())
                .collect()
        })
        .collect();
    assert_eq!(
        paths,
        vec![
            vec!["story".to_string(), "old".to_string(), "a".to_string()],
            vec!["story".to_string(), "old".to_string(), "b".to_string()],
        ]
    );
}

#[test]
fn annotation_args_trailing_comma_allowed() {
    let src = "@[name(a, b,)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let names: Vec<_> = args
        .args()
        .map(|a| a.name_token().unwrap().text().to_string())
        .collect();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn annotation_args_multiline_list() {
    let src = "@[name(\n  a,\n  b\n)]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    assert_eq!(args.args().count(), 2);
}

#[test]
fn multiple_annotation_lines_stacked_before_declaration() {
    let src = "@[local]\n@[effects(pure)]\nflow f() {\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let lines: Vec<_> = p
        .syntax()
        .children()
        .filter(|n| n.kind() == SyntaxKind::ANNOTATION_LINE)
        .collect();
    assert_eq!(lines.len(), 2);
    let first = crate::ast::AnnotationLine::cast(lines[0].clone()).expect("AnnotationLine");
    let second = crate::ast::AnnotationLine::cast(lines[1].clone()).expect("AnnotationLine");
    assert_eq!(first.name_token().unwrap().text(), "local");
    assert_eq!(second.name_token().unwrap().text(), "effects");
    assert!(has_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL));
}

// ── Error recovery: malformed input must not panic, and must round-trip
//    losslessly (rowan's guarantee holds even for a syntactically invalid
//    tree, since the CST always contains every source byte somewhere) ───

#[test]
fn annotation_line_unclosed_bracket_no_newline_records_error() {
    // `]` never appears at all — falls off the end at EOF.
    let src = "@[name";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1);
    assert!(p.errors()[0].message.contains("R_BRACKET"));
}

#[test]
fn annotation_line_unclosed_bracket_before_newline_records_error() {
    // Well-formed args, but the line ends before the closing `]`.
    let src = "@[name(a, b)\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1);
    assert!(p.errors()[0].message.contains("R_BRACKET"));
    // Recovery still lands cleanly on the next line — no cascading error.
    assert!(p.errors()[0].message.contains("NEWLINE"));
}

#[test]
fn annotation_args_missing_closing_paren_before_bracket() {
    // `(a` never closes; the `]` is what `expect(R_PAREN)` chokes on.
    let src = "@[name(a]\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1);
    assert!(p.errors()[0].message.contains("R_PAREN"));
    // The line still closes cleanly — the `]` itself is consumed by
    // `annotation_line`'s own `expect(R_BRACKET)` right after.
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
}

/// Mismatched parens in a nested clause: the inner `reads(gold, hp)`
/// closes correctly, but the outer `effects(...)` never gets its own `)`
/// — only the trailing `]` is present. The inner clause must still parse
/// as a fully-formed nested `ANNOTATION_ARGS`; only the outer level
/// records an error.
#[test]
fn annotation_args_mismatched_nested_parens_recovers_inner() {
    let src = "@[effects(reads(gold, hp)]\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1, "errors: {:?}", p.errors());
    assert!(p.errors()[0].message.contains("R_PAREN"));

    let line = annotation_line_node(&p);
    let args = line.args().expect("outer ANNOTATION_ARGS");
    let reads = args.args().next().expect("`reads` ANNOTATION_ARG");
    assert_eq!(reads.name_token().unwrap().text(), "reads");
    let nested = reads.nested_args().expect("inner `(gold, hp)` still nests");
    let names: Vec<_> = nested
        .args()
        .map(|a| a.name_token().unwrap().text().to_string())
        .collect();
    assert_eq!(names, vec!["gold".to_string(), "hp".to_string()]);
}

#[test]
fn annotation_args_unexpected_token_recovers_as_error_node() {
    // A stray leading comma is not a valid argument start.
    let src = "@[name(, a)]\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1, "errors: {:?}", p.errors());
    assert!(
        p.errors()[0]
            .message
            .contains("unexpected token in annotation arguments")
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ERROR));
    // Recovery still finds the real argument that follows.
    let line = annotation_line_node(&p);
    let args = line.args().expect("ANNOTATION_ARGS");
    let names: Vec<_> = args
        .args()
        .map(|a| a.name_token().unwrap().text().to_string())
        .collect();
    assert_eq!(names, vec!["a".to_string()]);
}

#[test]
fn annotation_line_trailing_text_after_close_records_error() {
    let src = "@[name] extra text\n";
    let p = assert_lossless(src);
    assert_eq!(p.errors().len(), 1, "errors: {:?}", p.errors());
    assert!(p.errors()[0].message.contains("unexpected text after"));
}

#[test]
fn annotation_line_trailing_whitespace_only_no_error() {
    // Trailing whitespace with nothing meaningful after `]` is not an
    // error — only non-whitespace trailing content trips the diagnostic.
    let src = "@[name]   \n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
}

#[test]
fn annotation_line_excess_closers_recovers_without_panic() {
    // Adversarial: far more `)` than were ever opened, plus the `]`
    // itself gets swallowed into the trailing-text recovery sweep since
    // `annotation_args` was never entered (no `(` immediately follows the
    // name). Must still round-trip losslessly with no panic.
    let src = "@[name)))))]\n";
    let p = assert_lossless(src);
    assert!(!p.errors().is_empty());
    assert!(p.errors().iter().any(|e| e.message.contains("R_BRACKET")));
}

#[test]
fn lone_at_outside_bracket_is_plain_text_not_error_token() {
    // NOTE: `docs/directive-annotations-spec.md` §5b is explicit that "a
    // lone `@` in prose stays plain text", and that is exactly what this
    // asserts. The doc comment on `SyntaxKind::AT` in `syntax_kind.rs`
    // (fixed in this same wave) previously claimed a lone `@` "is emitted
    // as `ERROR_TOKEN`" — stale, contradicting `lex_punctuation`'s own doc
    // comment ("a lone `@` is `AT`... not otherwise meaningful
    // punctuation"). Actual behavior matches the spec.
    //
    // `has_node_kind` only walks `descendants()`, which yields nodes —
    // `ERROR_TOKEN` is a token kind (`SyntaxKind::is_token()`), so a node-
    // level absence check can never fail regardless of what the parser
    // emits. Use `has_token_kind` (`descendants_with_tokens()`) instead,
    // and positively assert the `AT` token itself is present with the
    // expected text — the absence check alone can't tell "no ERROR_TOKEN
    // because it's an AT token" from "no ERROR_TOKEN because nothing was
    // lexed at all".
    //
    // **Narrowed by #1715**: `@NAME` (sigil directly against the name) is
    // now the ruled block-cue spelling (`docs/prose-dialect-spec.md`
    // §8b.9), so the "stays plain text" promise holds for every *other*
    // `@` — a detached one, as here, and any `@` reached mid-line. See
    // `parser::tests::element::a_lone_at_in_prose_is_still_plain_text`
    // for the adjacency guard, and the cue tests beside it for the
    // claimed shape.
    let src = "@ name\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CUE));
    assert!(!has_token_kind(&p.syntax(), SyntaxKind::ERROR_TOKEN));
    let at_token = p
        .syntax()
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::AT)
        .expect("AT token");
    assert_eq!(at_token.text(), "@");
    assert_eq!(text_run_concat(&p.syntax()), "@ name");
}

#[test]
fn lone_at_inside_flow_body_is_plain_text() {
    // An `@` reached mid-line is prose, not a cue: only an `@` at
    // body-item position, directly against its name, claims a line
    // (#1715, see the test above).
    let src = "flow f() {\n  meet me @ dawn\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CUE));
    assert!(!has_token_kind(&p.syntax(), SyntaxKind::ERROR_TOKEN));
    let at_token = p
        .syntax()
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::AT)
        .expect("AT token");
    assert_eq!(at_token.text(), "@");
    assert!(text_run_concat(&p.syntax()).contains("@ dawn"));
}

/// `annotation_line` is reached only via `block::body_line`
/// (`parser/block.rs:161`), which serves both file scope and block bodies
/// — but every `@[…]` case above this point in the file is at file scope.
/// This is the family's only clean-parse, structurally-walked case in the
/// position annotations are actually meant to be used: inside a `flow`
/// body, immediately before a statement (spec §5b / `lexer/tests.rs:294`'s
/// shape).
#[test]
fn annotation_line_in_block_body_before_divert() {
    // Uses §5b's own canonical fixture (same order as
    // `annotation_arg_spec_effects_fixture` above), placed at the position
    // annotations are actually meant to be used: inside a flow body.
    let src = "flow f() {\n  @[effects(reads(gold, hp), pure)]\n  -> END\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let flow = find_child::<crate::ast::FlowDecl>(&p.syntax()).expect("FlowDecl");
    let body = expect_prose_body(flow.body());
    let items: Vec<_> = body.items().collect();
    let annotation_idx = items
        .iter()
        .position(|n| n.kind() == SyntaxKind::ANNOTATION_LINE)
        .expect("ANNOTATION_LINE is a direct child of the block");
    let divert_idx = items
        .iter()
        .position(|n| n.kind() == SyntaxKind::DIVERT_STMT)
        .expect("DIVERT_STMT is a direct child of the block");
    assert!(
        annotation_idx < divert_idx,
        "annotation must precede the statement it annotates"
    );

    let line = crate::ast::AnnotationLine::cast(items[annotation_idx].clone())
        .expect("AnnotationLine cast");
    assert_eq!(line.name_token().unwrap().text(), "effects");
    let args = line.args().expect("ANNOTATION_ARGS");
    let top: Vec<_> = args.args().collect();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].name_token().unwrap().text(), "reads");
    assert_eq!(top[1].name_token().unwrap().text(), "pure");
}

/// Adversarial in-body interaction, never exercised elsewhere in this file:
/// `annotation_line`'s consume-to-`NEWLINE` trailing-text sweep runs after
/// its own `expect(R_BRACKET)`, with no awareness of the enclosing block's
/// `R_BRACE`. On a one-line block body, that sweep eats the block's closing
/// `}` as "unexpected text after `]`" — the `}` is not a `NEWLINE`, so
/// nothing stops the sweep there. Pinning down the actual (buggy-looking)
/// behavior: the annotation line's own trailing-text error fires, the `}`
/// ends up inside the `ANNOTATION_LINE` node (not the `BLOCK`), and
/// `braced_item_list` then reports its own `expected R_BRACE` error at EOF
/// since the brace was already consumed. Losslessness still holds — every
/// byte is somewhere in the tree — but the block is left syntactically
/// unterminated. Reported as-is per this issue's recovery-only scope; not
/// fixed here.
#[test]
fn annotation_line_in_single_line_block_eats_closing_brace() {
    let src = "flow f() { @[local] }\n";
    let p = assert_lossless(src);

    assert_eq!(p.errors().len(), 2, "errors: {:?}", p.errors());
    assert!(
        p.errors()[0].message.contains("unexpected text after"),
        "errors: {:?}",
        p.errors()
    );
    assert!(
        p.errors()[1].message.contains("R_BRACE"),
        "errors: {:?}",
        p.errors()
    );

    let flow = find_child::<crate::ast::FlowDecl>(&p.syntax()).expect("FlowDecl");
    let body = expect_prose_body(flow.body());
    let annotation_node = body
        .items()
        .find(|n| n.kind() == SyntaxKind::ANNOTATION_LINE)
        .expect("ANNOTATION_LINE is still a direct child of the block");
    // The `}` byte is folded into the ANNOTATION_LINE's own text (the
    // trailing-text sweep swallowed it) rather than terminating BLOCK.
    assert!(annotation_node.text().to_string().contains('}'));
}

#[test]
fn at_l_bracket_only_the_adjacent_pair_opens_annotation() {
    // A space between `@` and `[` must NOT compound into `AT_L_BRACKET` —
    // only the immediately-adjacent pair does (spec §5b, lexer comment on
    // `lex_punctuation`'s `b'@'` arm).
    let src = "@ [name]\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
}

// ── Adversarial fuzz: the recursive-descent depth limit specifically ────

#[test]
fn annotation_args_extreme_nesting_hits_depth_limit_without_panic() {
    // Comfortably past `MAX_DEPTH` (256) — the obvious DoS shape for a
    // recursive-descent paren-clause grammar. Must not panic/stack
    // overflow, must still round-trip losslessly, and must record the
    // depth-limit diagnostic.
    let depth = usize::try_from(MAX_DEPTH).unwrap_or(256) + 64;
    let src = nested_annotation_src(depth);
    let p = assert_lossless(&src);
    assert!(
        p.errors()
            .iter()
            .any(|e| e.message.contains("maximum nesting depth exceeded")),
        "errors: {:?}",
        p.errors()
    );
}

#[test]
fn annotation_args_many_unmatched_openers_stays_linear_without_panic() {
    // No idents between the parens at all, so `annotation_arg` never
    // recurses (an `(` alone matches none of its arms) — every stray `(`
    // is instead consumed one at a time by `error_recover`'s zero-progress
    // fallback. This exercises the *other* DoS shape: pathological input
    // that never even reaches the depth guard, and must still terminate
    // in time linear in input length.
    let src = format!("@[name{}]\n", "(".repeat(2000));
    let p = assert_lossless(&src);
    assert!(!p.errors().is_empty());
}

#[test]
fn many_stacked_annotation_lines_no_error() {
    // Linear-not-exponential sanity check for the *stacking* shape (as
    // opposed to nesting): each top-level annotation line enters and
    // exits its own depth scope cleanly, so many of them in a row must
    // never trip the single shared nesting-depth counter.
    let count = 200;
    let mut src = String::new();
    for i in 0..count {
        use std::fmt::Write as _;
        let _ = writeln!(src, "@[a{i}]");
    }
    let p = assert_lossless(&src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(
        count_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE),
        count
    );
}

// ── Proptest: arbitrary-depth nested paren-clause round-trip ────────────
//
// Bounded per CLAUDE.md's "guard against unbounded growth" rule — this is
// a *generator* bound (the range below), distinct from the parser's own
// `MAX_DEPTH` guard that `annotation_args_extreme_nesting_...` exercises
// directly. The range deliberately straddles `MAX_DEPTH` so the same
// property test covers both "well-formed, well within the limit" and
// "well-formed but past the limit" without a second proptest block.

const NUM_CASES: u32 = 200;

// Keywords lex as their own `KW_*` token, not `IDENT` — an annotation arg
// name/nested-clause name must avoid them or the generated fixture isn't
// actually testing the annotation grammar (mirrors
// `tests/proptest_native.rs`'s own `KEYWORDS` filter, duplicated here per
// this issue's "put a proptest generator in your own family file" scoping
// rather than touching that shared file this wave).
const KEYWORDS: &[&str] = &[
    "flow", "fn", "var", "const", "let", "flags", "struct", "extern", "import", "use", "module",
    "return", "ref", "if", "match", "else", "while", "for", "in", "until", "break", "continue",
    "as", "or", "true", "false", "END", "DONE",
];

fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,5}".prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
}

/// One argument, recursively: a bare ident, or `ident(args...)` nesting
/// further. Depth and breadth are both explicitly bounded (`prop_recursive`
/// depth 6, size cap 40, ~3-wide branching) — comfortably under
/// `MAX_DEPTH`, so every case this generates is expected to parse with
/// zero errors.
fn arb_nested_arg() -> impl Strategy<Value = String> {
    let leaf = arb_ident();
    leaf.prop_recursive(6, 40, 3, |inner| {
        (arb_ident(), prop::collection::vec(inner, 1..=3))
            .prop_map(|(name, children)| format!("{name}({})", children.join(", ")))
    })
}

fn arb_annotation_line_with_nested_args() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_nested_arg(), 1..=3)
        .prop_map(|args| format!("@[effects({})]\n", args.join(", ")))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(NUM_CASES))]

    /// Well-formed, arbitrary-but-bounded-depth nested paren-clauses always
    /// round-trip losslessly and parse with zero errors.
    #[test]
    fn arb_nested_annotation_args_round_trip_clean(src in arb_annotation_line_with_nested_args()) {
        let p = parse(&src);
        prop_assert_eq!(p.syntax().text().to_string(), src);
        prop_assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    }

    /// Straddles `MAX_DEPTH`: for depth strictly under the limit the
    /// balanced-paren chain must parse clean; for any depth (including
    /// well past the limit) it must always round-trip losslessly and must
    /// never panic — the depth-limit DoS-shape property, generalized
    /// across the whole boundary rather than one hand-picked value.
    #[test]
    fn arb_nested_depth_straddling_max_depth_never_panics(depth in 1usize..320) {
        let src = nested_annotation_src(depth);
        let p = parse(&src);
        prop_assert_eq!(p.syntax().text().to_string(), src);
        if u32::try_from(depth).unwrap_or(u32::MAX) < MAX_DEPTH {
            prop_assert!(p.errors().is_empty(), "depth {depth} errors: {:?}", p.errors());
        }
    }

    /// Pure fuzz: arbitrary printable-ASCII garbage between `@[` and the
    /// line end must never panic and must always round-trip losslessly,
    /// regardless of how malformed it is.
    #[test]
    fn arb_annotation_line_garbage_never_panics(body in "[ -~]{0,40}") {
        let src = format!("@[{body}\n");
        let p = parse(&src);
        prop_assert_eq!(p.syntax().text().to_string(), src);
    }
}
