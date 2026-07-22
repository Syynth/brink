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
    let src = "fn heal(hp) {\n  @[effects(pure, silent, reads(gold, hp))]\n  var x = hp\n}\n";
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
    // asserts. This directly contradicts the doc comment on
    // `SyntaxKind::AT` in `syntax_kind.rs` ("is emitted as `ERROR_TOKEN`")
    // — that comment is stale (the lexer's own `lex_punctuation` doc
    // comment already says the opposite: "a lone `@` is `AT`... not
    // otherwise meaningful punctuation"). This is a documentation nit, not
    // a parser bug: the actual behavior matches the spec, so nothing here
    // needs `#[ignore]` — only the `AT` doc comment (out of this file's
    // scope) is wrong. Filed as a scope note on #1198.
    let src = "@name\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ERROR_TOKEN));
    assert_eq!(text_run_concat(&p.syntax()), "@name");
}

#[test]
fn lone_at_inside_flow_body_is_plain_text() {
    let src = "flow f() {\n  @oops\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
    assert!(text_run_concat(&p.syntax()).contains("@oops"));
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
    "flow", "fn", "var", "const", "flags", "struct", "extern", "import", "use", "module", "return",
    "ref", "if", "match", "else", "as", "true", "false", "END", "DONE",
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
