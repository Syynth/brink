//! Property tests for the `.brink` native lexer/CST (B0.5).
//!
//! The heart property (`docs/b0-sequencing.md` §B0.5's exit criterion): a
//! lossless lexer/CST round-trip — `source == parse(source).syntax().text()`
//! — for both well-formed grammar-shaped input and adversarial garbage
//! (mixed line endings, a leading BOM, unicode, unterminated
//! braces/blocks). Mirrors `brink-syntax/tests/proptest_syntax.rs`'s
//! structure and generator style, studied for this crate's own grammar.

use brink_syntax_native::{SyntaxKind, parse};
use proptest::prelude::*;

const NUM_CASES: u32 = 512;

// ── Leaf strategies ──────────────────────────────────────────────────

const KEYWORDS: &[&str] = &[
    "flow", "fn", "var", "const", "flags", "struct", "extern", "import", "use", "module", "return",
    "ref", "if", "match", "else", "as", "true", "false", "END", "DONE",
];

fn arb_ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
}

/// Prose text that avoids every structural character the grammar
/// recognizes: `{ } < > @ # | - [ ] ( ) : ;` and quotes.
fn arb_text() -> impl Strategy<Value = String> {
    "[a-z][a-zA-Z0-9 ,.!?]{0,29}"
}

fn arb_integer() -> impl Strategy<Value = String> {
    (0..10_000u32).prop_map(|n| n.to_string())
}

// ── Expression strategy (recursive, depth-bounded) ───────────────────

fn arb_expr() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        arb_integer(),
        Just("true".to_string()),
        Just("false".to_string()),
        arb_ident(),
    ];
    leaf.prop_recursive(3, 24, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|e| format!("-{e}")),
            inner.clone().prop_map(|e| format!("!{e}")),
            inner.clone().prop_map(|e| format!("({e})")),
            (inner.clone(), arb_infix_op(), inner.clone())
                .prop_map(|(l, op, r)| format!("{l} {op} {r}")),
            (arb_ident(), prop::collection::vec(inner, 0..=2))
                .prop_map(|(name, args)| format!("{name}({})", args.join(", "))),
        ]
    })
}

fn arb_infix_op() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("+"),
        Just("-"),
        Just("*"),
        Just("/"),
        Just("%"),
        Just("=="),
        Just("!="),
        Just("<"),
        Just(">"),
        Just("<="),
        Just(">="),
        Just("&&"),
    ]
}

// ── Declaration strategies ───────────────────────────────────────────

fn arb_param_list() -> impl Strategy<Value = String> {
    prop::collection::vec((prop::bool::ANY, arb_ident()), 0..=3).prop_map(|params| {
        let rendered: Vec<String> = params
            .into_iter()
            .map(|(is_ref, name)| if is_ref { format!("ref {name}") } else { name })
            .collect();
        format!("({})", rendered.join(", "))
    })
}

fn arb_content_line() -> impl Strategy<Value = String> {
    arb_text().prop_map(|t| format!("{t}\n"))
}

fn arb_interpolation_line() -> impl Strategy<Value = String> {
    (arb_text(), arb_ident(), arb_text())
        .prop_map(|(before, name, after)| format!("{before} {{{name}}} {after}\n"))
}

fn arb_divert_line() -> impl Strategy<Value = String> {
    arb_ident().prop_map(|name| format!("-> {name}\n"))
}

fn arb_tunnel_line() -> impl Strategy<Value = String> {
    arb_ident().prop_map(|name| format!("-> {name} ->\n"))
}

fn arb_return_line() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("return\n".to_string()),
        arb_ident().prop_map(|name| format!("return -> {name}\n")),
    ]
}

fn arb_var_line() -> impl Strategy<Value = String> {
    (arb_ident(), arb_expr()).prop_map(|(name, e)| format!("var {name} = {e}\n"))
}

fn arb_const_line() -> impl Strategy<Value = String> {
    (arb_ident(), arb_expr()).prop_map(|(name, e)| format!("const {name} = {e}\n"))
}

fn arb_annotation_line() -> impl Strategy<Value = String> {
    (arb_ident(), prop::collection::vec(arb_ident(), 0..=3)).prop_map(|(name, args)| {
        if args.is_empty() {
            format!("@[{name}]\n")
        } else {
            format!("@[{name}({})]\n", args.join(", "))
        }
    })
}

fn arb_choice_line() -> impl Strategy<Value = String> {
    (prop::bool::ANY, arb_text()).prop_map(|(sticky, text)| {
        let bullet = if sticky { "+" } else { "*" };
        format!("{bullet} {text}\n")
    })
}

fn arb_choice_point() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_choice_line(), 1..=4)
        .prop_map(|lines| format!("{{?\n{}}}\n", lines.join("")))
}

/// N-1: a choice line whose text is followed, on the same line, by a plain
/// divert — `* [text] -> target` (the exhibit-fogg-passage/manual-stitch-v1
/// shape).
fn arb_choice_line_with_inline_divert() -> impl Strategy<Value = String> {
    (prop::bool::ANY, arb_text(), arb_ident()).prop_map(|(sticky, text, target)| {
        let bullet = if sticky { "+" } else { "*" };
        format!("{bullet} [{text}] -> {target}\n")
    })
}

fn arb_choice_point_with_inline_divert() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_choice_line_with_inline_divert(), 1..=4)
        .prop_map(|lines| format!("{{?\n{}}}\n", lines.join("")))
}

/// N-1: a multiline choice body whose content line carries a divert after
/// prose on the same line — `* [text] {\n  prose -> target\n}` (the
/// sticky-choice shape).
fn arb_choice_line_with_body_divert() -> impl Strategy<Value = String> {
    (arb_text(), arb_text(), arb_ident()).prop_map(|(choice_text, prose, target)| {
        format!("* [{choice_text}] {{\n  {prose} -> {target}\n}}\n")
    })
}

fn arb_choice_point_with_body_divert() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_choice_line_with_body_divert(), 1..=3)
        .prop_map(|lines| format!("{{?\n{}}}\n", lines.join("")))
}

/// G-2: a choice line carrying trailing `{expr}` interpolation —
/// `* text {ident}` — rather than a nested `CHOICE_BODY`.
fn arb_choice_line_with_interpolation() -> impl Strategy<Value = String> {
    (arb_text(), arb_ident()).prop_map(|(text, ident)| format!("* {text} {{{ident}}}\n"))
}

fn arb_choice_point_with_interpolation() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_choice_line_with_interpolation(), 1..=4)
        .prop_map(|lines| format!("{{?\n{}}}\n", lines.join("")))
}

fn arb_alternation_inline() -> impl Strategy<Value = String> {
    (
        prop::sample::select(&["~", "&", "!", "|"][..]),
        prop::collection::vec(arb_text(), 2..=4),
    )
        .prop_map(|(marker, items)| format!("{{{marker} {}}}\n", items.join("|")))
}

fn arb_use_decl() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_ident(), 1..=3).prop_map(|segs| format!("use {};\n", segs.join("::")))
}

fn arb_body_line() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => arb_content_line(),
        2 => arb_interpolation_line(),
        2 => arb_divert_line(),
        1 => arb_tunnel_line(),
        1 => arb_return_line(),
        1 => arb_var_line(),
        1 => arb_annotation_line(),
        2 => arb_choice_point(),
        1 => arb_alternation_inline(),
        // N-1/G-2: real-narrative-content shapes the grammar-shaped
        // generators above never produced (see the respell-fixture README's
        // findings) — mixed into the general body/story fuzz so the "real
        // content, not grammar-shaped" gap the interim respelling exposed
        // gets ongoing property coverage, not just the dedicated tests
        // above.
        2 => arb_choice_point_with_inline_divert(),
        1 => arb_choice_point_with_body_divert(),
        1 => arb_choice_point_with_interpolation(),
    ]
}

fn arb_body() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_body_line(), 1..=6).prop_map(|lines| lines.join(""))
}

fn arb_flow_decl() -> impl Strategy<Value = String> {
    (arb_ident(), arb_param_list(), arb_body())
        .prop_map(|(name, params, body)| format!("flow {name}{params} {{\n{body}}}\n"))
}

fn arb_fn_decl() -> impl Strategy<Value = String> {
    (arb_ident(), arb_param_list(), arb_body())
        .prop_map(|(name, params, body)| format!("fn {name}{params} {{\n{body}}}\n"))
}

fn arb_story() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_body(),
        arb_flow_decl(),
        arb_fn_decl(),
        arb_var_line(),
        arb_const_line(),
        arb_use_decl(),
        prop::collection::vec(prop_oneof![arb_flow_decl(), arb_fn_decl()], 1..=3)
            .prop_map(|decls| decls.join("")),
    ]
}

// ── Helpers ───────────────────────────────────────────────────────────

fn has_node_kind(root: &brink_syntax_native::SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants().any(|node| node.kind() == kind)
}

fn has_error_nodes(root: &brink_syntax_native::SyntaxNode) -> bool {
    root.descendants()
        .any(|node| node.kind() == SyntaxKind::ERROR)
}

// ── Adversarial mutation strategies ───────────────────────────────────

/// Prepend a UTF-8 BOM.
fn with_bom(s: &str) -> String {
    format!("\u{FEFF}{s}")
}

/// Replace `\n` line endings with a mix of `\n`, `\r\n`, and bare `\r`,
/// deterministically by position (adversarial mixed line endings).
fn mixed_line_endings(s: &str, pattern: u8) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut count = 0u8;
    for ch in s.chars() {
        if ch == '\n' {
            match (pattern.wrapping_add(count)) % 3 {
                0 => out.push('\n'),
                1 => out.push_str("\r\n"),
                _ => out.push('\r'),
            }
            count = count.wrapping_add(1);
        } else {
            out.push(ch);
        }
    }
    out
}

/// Truncate the source at a byte-safe boundary to simulate an unterminated
/// brace/block/string.
fn truncated(s: &str, cut_ratio: u32) -> String {
    let target = (s.len() as u64 * u64::from(cut_ratio) / 100) as usize;
    let mut end = target.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// Interleave some unicode noise (accented letters, CJK, emoji, combining
/// marks) into prose text positions.
fn arb_unicode_noise() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just("héllo"),
            Just("wörld"),
            Just("日本語"),
            Just("🎉🔥"),
            Just("e\u{0301}"), // combining acute accent
            Just("\u{200B}"),  // zero-width space
        ],
        1..=5,
    )
    .prop_map(|parts| parts.join(" "))
}

// ── Property tests ───────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(NUM_CASES))]

    // ── Lossless round-trip: well-formed grammar-shaped input ─────────

    #[test]
    fn content_line_roundtrip(input in arb_content_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn interpolation_line_roundtrip(input in arb_interpolation_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn divert_line_roundtrip(input in arb_divert_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn tunnel_line_roundtrip(input in arb_tunnel_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn return_line_roundtrip(input in arb_return_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn var_line_roundtrip(input in arb_var_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn annotation_line_roundtrip(input in arb_annotation_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn choice_point_roundtrip(input in arb_choice_point()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    // ── N-1: inline diverts in content position ────────────────────────

    #[test]
    fn choice_point_with_inline_divert_roundtrip(input in arb_choice_point_with_inline_divert()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn choice_point_with_inline_divert_no_errors(input in arb_choice_point_with_inline_divert()) {
        let parsed = parse(&input);
        prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
    }

    #[test]
    fn choice_point_with_inline_divert_produces_divert_stmt(input in arb_choice_point_with_inline_divert()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::DIVERT_STMT));
        // And the divert's target is a real PATH node, not text swallowed
        // into the enclosing CHOICE_INNER_CONTENT's TEXT run.
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::DIVERT_TARGET));
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::PATH));
    }

    #[test]
    fn choice_point_with_body_divert_roundtrip(input in arb_choice_point_with_body_divert()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn choice_point_with_body_divert_no_errors(input in arb_choice_point_with_body_divert()) {
        let parsed = parse(&input);
        prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
    }

    #[test]
    fn choice_point_with_body_divert_produces_divert_stmt_inside_content_line(input in arb_choice_point_with_body_divert()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        let content_line = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CONTENT_LINE);
        prop_assert!(content_line.is_some(), "no CONTENT_LINE found, tree: {:#?}", root);
        let content_line = content_line.expect("checked above");
        prop_assert!(has_node_kind(&content_line, SyntaxKind::DIVERT_STMT));
        prop_assert!(has_node_kind(&content_line, SyntaxKind::TEXT));
    }

    // ── G-2: choice-line `{expr}` interpolation ─────────────────────────

    #[test]
    fn choice_point_with_interpolation_roundtrip(input in arb_choice_point_with_interpolation()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn choice_point_with_interpolation_no_errors(input in arb_choice_point_with_interpolation()) {
        let parsed = parse(&input);
        prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
    }

    #[test]
    fn choice_point_with_interpolation_produces_interpolation_not_choice_body(input in arb_choice_point_with_interpolation()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(has_node_kind(&root, SyntaxKind::INTERPOLATION));
        // None of these generated choice lines have any nested-content
        // braces beyond the trailing `{ident}` — a spurious CHOICE_BODY
        // would mean G-2 regressed.
        prop_assert!(!has_node_kind(&root, SyntaxKind::CHOICE_BODY));
    }

    #[test]
    fn alternation_inline_roundtrip(input in arb_alternation_inline()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn use_decl_roundtrip(input in arb_use_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn flow_decl_roundtrip(input in arb_flow_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn fn_decl_roundtrip(input in arb_fn_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn story_roundtrip(input in arb_story()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    // ── No ERROR nodes for well-formed input ──────────────────────────

    #[test]
    fn flow_decl_no_errors(input in arb_flow_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node for input: {:?}\nTree:\n{:#?}", input, root,
        );
        prop_assert!(parsed.errors().is_empty(), "errors: {:?}", parsed.errors());
    }

    #[test]
    fn choice_point_no_errors(input in arb_choice_point()) {
        let parsed = parse(&input);
        prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
    }

    #[test]
    fn annotation_line_no_errors(input in arb_annotation_line()) {
        let parsed = parse(&input);
        prop_assert!(parsed.errors().is_empty(), "input: {:?}\nerrors: {:?}", input, parsed.errors());
    }

    // ── Root is SOURCE_FILE ────────────────────────────────────────────

    #[test]
    fn root_is_source_file(input in arb_story()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().kind(), SyntaxKind::SOURCE_FILE);
    }

    // ── Expected node kinds ─────────────────────────────────────────────

    #[test]
    fn flow_produces_flow_decl(input in arb_flow_decl()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::FLOW_DECL));
    }

    #[test]
    fn fn_produces_fn_decl(input in arb_fn_decl()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::FN_DECL));
    }

    #[test]
    fn choice_point_produces_choice_point_node(input in arb_choice_point()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::CHOICE_POINT));
    }

    // ── Adversarial: lossless round-trip and no panic, no hang ─────────
    // The heart property under mutation. These inputs are NOT required to
    // parse error-free — only to round-trip losslessly and never panic.

    #[test]
    fn bom_prefixed_input_roundtrips(input in arb_story()) {
        let mutated = with_bom(&input);
        let parsed = parse(&mutated);
        prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
    }

    #[test]
    fn mixed_line_endings_roundtrip(input in arb_story(), pattern in 0u8..3) {
        let mutated = mixed_line_endings(&input, pattern);
        let parsed = parse(&mutated);
        prop_assert_eq!(parsed.syntax().text().to_string(), mutated.clone());
        // Every byte still accounted for even though `\r`/`\r\n`/`\n` are mixed.
        prop_assert_eq!(parsed.syntax().text().len(), rowan::TextSize::of(mutated.as_str()));
    }

    #[test]
    fn truncated_input_never_panics_and_roundtrips(input in arb_flow_decl(), cut in 0u32..100) {
        let mutated = truncated(&input, cut);
        let parsed = parse(&mutated);
        prop_assert_eq!(parsed.syntax().text().to_string(), mutated);
    }

    #[test]
    fn unicode_noise_in_content_roundtrips(before in arb_text(), noise in arb_unicode_noise(), after in arb_text()) {
        let input = format!("{before} {noise} {after}\n");
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn unicode_noise_inside_flow_body_roundtrips(name in arb_ident(), noise in arb_unicode_noise()) {
        let input = format!("flow {name}() {{\n  {noise}\n}}\n");
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn unterminated_annotation_never_panics(name in arb_ident(), arg in arb_ident()) {
        // Missing the closing `]` and/or `)`.
        let input = format!("@[{name}({arg}\n");
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn unterminated_choice_point_never_panics(input in prop::collection::vec(arb_choice_line(), 1..=3)) {
        let src = format!("{{?\n{}", input.join(""));
        let parsed = parse(&src);
        prop_assert_eq!(parsed.syntax().text().to_string(), src);
    }

    #[test]
    fn unterminated_flow_body_never_panics(name in arb_ident(), body in arb_body()) {
        let src = format!("flow {name}() {{\n{body}");
        let parsed = parse(&src);
        prop_assert_eq!(parsed.syntax().text().to_string(), src);
    }

    #[test]
    fn unterminated_string_never_panics(text in "[a-zA-Z0-9 ]{0,20}") {
        let src = format!("var x = \"{text}\n");
        let parsed = parse(&src);
        prop_assert_eq!(parsed.syntax().text().to_string(), src);
    }

    /// Fully arbitrary byte-garbage input restricted to valid UTF-8
    /// (any `String` proptest generates is guaranteed valid UTF-8) drawn
    /// from a wide alphabet including every structural character the
    /// grammar recognizes, unpaired brackets, and raw control characters —
    /// the parser must never panic or hang on any of it.
    #[test]
    fn arbitrary_garbage_never_panics(input in "\\PC{0,120}") {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn arbitrary_structural_soup_never_panics(
        input in prop::collection::vec(
            prop::sample::select(&[
                "{", "}", "(", ")", "[", "]", "@", "::", ".", "-", ">", "<", "|", "~", "&", "!",
                "?", "\"", "\n", " ", "flow", "fn", "if", "match", "else", "return", "var",
            ][..]),
            0..40,
        )
    ) {
        let src = input.join("");
        let parsed = parse(&src);
        prop_assert_eq!(parsed.syntax().text().to_string(), src);
    }
}
