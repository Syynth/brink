use std::fmt::Write;

use brink_syntax::{SyntaxKind, parse};
use proptest::prelude::*;

// ── Constants ────────────────────────────────────────────────────────

const KEYWORDS: &[&str] = &[
    "INCLUDE", "EXTERNAL", "VAR", "CONST", "LIST", "temp", "return", "ref", "true", "false", "not",
    "and", "or", "mod", "has", "hasnt", "else", "function", "stopping", "cycle", "shuffle", "once",
    "DONE", "END", "TODO",
];

const NUM_CASES: u32 = 512;

// ── Leaf strategies ──────────────────────────────────────────────────

fn arb_ident() -> impl Strategy<Value = String> {
    "[a-zA-Z_][a-zA-Z0-9_]{0,7}"
        .prop_filter("must not be a keyword", |s| !KEYWORDS.contains(&s.as_str()))
}

/// Text content that avoids triggering parser-significant tokens.
/// Starts lowercase to avoid uppercase keywords (VAR, CONST, LIST, etc.).
/// Excludes structural characters: { } < > - # | \ / ~ = * + [ ] ( ) @
fn arb_text() -> impl Strategy<Value = String> {
    "[a-z][a-zA-Z0-9 ,.!?;:]{0,29}"
}

/// Text for choice content — also excludes [ and ] which delimit bracket content.
fn arb_choice_text() -> impl Strategy<Value = String> {
    "[a-z][a-zA-Z0-9 ,.!?;:]{0,19}"
}

fn arb_integer() -> impl Strategy<Value = String> {
    (0..10000u32).prop_map(|n| n.to_string())
}

fn arb_float() -> impl Strategy<Value = String> {
    (0..1000u32, 1..100u32).prop_map(|(whole, frac)| format!("{whole}.{frac}"))
}

fn arb_string_lit() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ,.!?]{0,20}".prop_map(|s| format!("\"{s}\""))
}

// ── Expression strategy (recursive, depth-bounded) ───────────────────

fn arb_expr() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        arb_integer(),
        arb_float(),
        arb_string_lit(),
        Just("true".to_string()),
        Just("false".to_string()),
        arb_ident(),
    ];

    leaf.prop_recursive(3, 32, 4, |inner| {
        prop_oneof![
            // Prefix
            inner.clone().prop_map(|e| format!("-{e}")),
            inner.clone().prop_map(|e| format!("!{e}")),
            inner.clone().prop_map(|e| format!("not {e}")),
            // Parenthesized
            inner.clone().prop_map(|e| format!("({e})")),
            // Infix
            (inner.clone(), arb_infix_op(), inner.clone())
                .prop_map(|(l, op, r)| format!("{l} {op} {r}")),
            // Function call
            (arb_ident(), inner).prop_map(|(name, arg)| format!("{name}({arg})")),
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
        Just("||"),
        Just("and"),
        Just("or"),
    ]
}

// ── Line strategies ──────────────────────────────────────────────────

fn arb_content_line() -> impl Strategy<Value = String> {
    (
        arb_text(),
        prop::option::of(arb_divert()),
        prop::option::of(arb_tag()),
    )
        .prop_map(|(text, divert, tag)| {
            let mut line = text;
            if let Some(d) = divert {
                let _ = write!(line, " {d}");
            }
            if let Some(t) = tag {
                let _ = write!(line, " {t}");
            }
            line.push('\n');
            line
        })
}

fn arb_divert() -> impl Strategy<Value = String> {
    arb_ident().prop_map(|name| format!("-> {name}"))
}

fn arb_tag() -> impl Strategy<Value = String> {
    "[a-z][a-zA-Z0-9 ]{0,10}".prop_map(|t| format!("# {t}"))
}

fn arb_choice_line() -> impl Strategy<Value = String> {
    (
        prop::sample::select(&[1usize, 2, 3][..]),
        prop::bool::ANY,
        arb_choice_text(),
        prop::option::of(arb_bracket_content()),
        prop::option::of(arb_divert()),
    )
        .prop_map(|(depth, sticky, text, bracket, divert)| {
            let bullet = if sticky { "+" } else { "*" };
            let bullets = bullet.repeat(depth);
            let mut line = format!("{bullets} {text}");
            if let Some(b) = bracket {
                let _ = write!(line, " {b}");
            }
            if let Some(d) = divert {
                let _ = write!(line, " {d}");
            }
            line.push('\n');
            line
        })
}

fn arb_bracket_content() -> impl Strategy<Value = String> {
    arb_choice_text().prop_map(|t| format!("[{t}]"))
}

fn arb_gather_line() -> impl Strategy<Value = String> {
    (
        prop::sample::select(&[1usize, 2, 3][..]),
        prop::option::of(arb_label()),
        arb_text(),
    )
        .prop_map(|(depth, label, text)| {
            let dashes = "- ".repeat(depth);
            let mut line = dashes;
            if let Some(l) = label {
                let _ = write!(line, "{l} ");
            }
            line.push_str(&text);
            line.push('\n');
            line
        })
}

fn arb_label() -> impl Strategy<Value = String> {
    arb_ident().prop_map(|name| format!("({name})"))
}

fn arb_logic_line() -> impl Strategy<Value = String> {
    prop_oneof![
        // ~ expr
        arb_expr().prop_map(|e| format!("~ {e}\n")),
        // ~ return expr
        arb_expr().prop_map(|e| format!("~ return {e}\n")),
        // ~ temp x = expr
        (arb_ident(), arb_expr()).prop_map(|(name, e)| format!("~ temp {name} = {e}\n")),
        // ~ x = expr
        (arb_ident(), arb_expr()).prop_map(|(name, e)| format!("~ {name} = {e}\n")),
    ]
}

fn arb_content_with_inline() -> impl Strategy<Value = String> {
    (arb_text(), arb_inline_logic(), prop::option::of(arb_text())).prop_map(
        |(before, inline, after)| {
            let mut line = before;
            line.push(' ');
            line.push_str(&inline);
            if let Some(a) = after {
                line.push(' ');
                line.push_str(&a);
            }
            line.push('\n');
            line
        },
    )
}

// ── Inline logic strategy ────────────────────────────────────────────

fn arb_inline_logic() -> impl Strategy<Value = String> {
    prop_oneof![
        // Bare expression: {x}
        arb_ident().prop_map(|e| format!("{{{e}}}")),
        // Conditional: {x: text}
        (arb_ident(), arb_text()).prop_map(|(cond, text)| format!("{{{cond}: {text}}}")),
        // Conditional with else: {x: yes | no}
        (arb_ident(), arb_text(), arb_text())
            .prop_map(|(cond, yes, no)| format!("{{{cond}: {yes}|{no}}}")),
        // Sequence: {a|b|c}
        prop::collection::vec(arb_text(), 2..=4)
            .prop_map(|items| format!("{{{}}}", items.join("|"))),
        // Annotated sequence: {&a|b|c}
        (
            prop::sample::select(&["&", "!", "~"][..]),
            prop::collection::vec(arb_text(), 2..=4),
        )
            .prop_map(|(ann, items)| format!("{{{ann}{}}}", items.join("|"))),
        // Keyword sequence: {stopping: a|b|c}
        (
            prop::sample::select(&["stopping", "cycle", "shuffle", "once"][..]),
            prop::collection::vec(arb_text(), 2..=4),
        )
            .prop_map(|(kw, items)| format!("{{{kw}:{}}}", items.join("|"))),
    ]
}

// ── Declaration strategies ───────────────────────────────────────────

fn arb_var_decl() -> impl Strategy<Value = String> {
    (arb_ident(), arb_simple_value()).prop_map(|(name, val)| format!("VAR {name} = {val}\n"))
}

fn arb_const_decl() -> impl Strategy<Value = String> {
    (arb_ident(), arb_simple_value()).prop_map(|(name, val)| format!("CONST {name} = {val}\n"))
}

fn arb_simple_value() -> impl Strategy<Value = String> {
    prop_oneof![arb_integer(), arb_float(), arb_string_lit(),]
}

fn arb_list_decl() -> impl Strategy<Value = String> {
    (arb_ident(), prop::collection::vec(arb_list_member(), 1..=4))
        .prop_map(|(name, members)| format!("LIST {name} = {}\n", members.join(", ")))
}

fn arb_list_member() -> impl Strategy<Value = String> {
    (arb_ident(), prop::bool::ANY).prop_map(
        |(name, on)| {
            if on { format!("({name})") } else { name }
        },
    )
}

fn arb_external_decl() -> impl Strategy<Value = String> {
    (arb_ident(), prop::collection::vec(arb_ident(), 0..=3))
        .prop_map(|(name, params)| format!("EXTERNAL {name}({})\n", params.join(", ")))
}

fn arb_include_stmt() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(|name| format!("INCLUDE {name}.ink\n"))
}

// ── Structure strategies ─────────────────────────────────────────────

fn arb_knot_header() -> impl Strategy<Value = String> {
    arb_ident().prop_map(|name| format!("=== {name} ===\n"))
}

fn arb_stitch_header() -> impl Strategy<Value = String> {
    arb_ident().prop_map(|name| format!("= {name}\n"))
}

fn arb_body() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_body_line(), 1..=5).prop_map(|lines| lines.join(""))
}

fn arb_body_line() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => arb_content_line(),
        2 => arb_choice_line(),
        1 => arb_gather_line(),
        2 => arb_logic_line(),
        1 => arb_content_with_inline(),
    ]
}

// ── Story strategy ───────────────────────────────────────────────────

fn arb_story() -> impl Strategy<Value = String> {
    prop_oneof![
        // Flat body
        arb_body(),
        // Declarations + body
        (arb_declarations(), arb_body()).prop_map(|(decls, body)| format!("{decls}{body}")),
        // Single knot
        (arb_knot_header(), arb_body()).prop_map(|(header, body)| format!("{header}{body}")),
        // Knot with stitches
        (
            arb_knot_header(),
            arb_body(),
            prop::collection::vec(
                (arb_stitch_header(), arb_body()).prop_map(|(h, b)| format!("{h}{b}")),
                1..=3,
            ),
        )
            .prop_map(|(knot, body, stitches)| { format!("{knot}{body}{}", stitches.join("")) }),
        // Multiple knots
        prop::collection::vec(
            (arb_knot_header(), arb_body()).prop_map(|(h, b)| format!("{h}{b}")),
            2..=4,
        )
        .prop_map(|knots| knots.join("")),
    ]
}

fn arb_declarations() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_declaration(), 1..=3).prop_map(|decls| decls.join(""))
}

fn arb_declaration() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_var_decl(),
        arb_const_decl(),
        arb_list_decl(),
        arb_external_decl(),
        arb_include_stmt(),
    ]
}

// ── Helper functions ─────────────────────────────────────────────────

fn has_error_nodes(root: &brink_syntax::SyntaxNode) -> bool {
    root.descendants()
        .any(|node| node.kind() == SyntaxKind::ERROR)
}

fn has_node_kind(root: &brink_syntax::SyntaxNode, kind: SyntaxKind) -> bool {
    root.descendants().any(|node| node.kind() == kind)
}

// ── Property tests ───────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(NUM_CASES))]

    // ── Lossless round-trip ──────────────────────────────────────────

    #[test]
    fn content_line_roundtrip(input in arb_content_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn choice_line_roundtrip(input in arb_choice_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn gather_line_roundtrip(input in arb_gather_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn logic_line_roundtrip(input in arb_logic_line()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn content_with_inline_roundtrip(input in arb_content_with_inline()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn var_decl_roundtrip(input in arb_var_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn const_decl_roundtrip(input in arb_const_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn list_decl_roundtrip(input in arb_list_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn external_decl_roundtrip(input in arb_external_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn include_stmt_roundtrip(input in arb_include_stmt()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn story_roundtrip(input in arb_story()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    // ── No ERROR nodes ───────────────────────────────────────────────

    #[test]
    fn content_line_no_errors(input in arb_content_line()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn choice_line_no_errors(input in arb_choice_line()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn gather_line_no_errors(input in arb_gather_line()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn logic_line_no_errors(input in arb_logic_line()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn var_decl_no_errors(input in arb_var_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn const_decl_no_errors(input in arb_const_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn list_decl_no_errors(input in arb_list_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn external_decl_no_errors(input in arb_external_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn include_stmt_no_errors(input in arb_include_stmt()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            !has_error_nodes(&root),
            "ERROR node found in CST for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    // ── Root is SOURCE_FILE ──────────────────────────────────────────

    #[test]
    fn root_is_source_file(input in arb_story()) {
        let parsed = parse(&input);
        prop_assert_eq!(
            parsed.syntax().kind(),
            SyntaxKind::SOURCE_FILE,
            "root node should be SOURCE_FILE for input: {:?}",
            input,
        );
    }

    // ── Expected node kinds ──────────────────────────────────────────

    #[test]
    fn knot_produces_knot_def(
        name in arb_ident(),
        body in arb_body(),
    ) {
        let input = format!("=== {name} ===\n{body}");
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::KNOT_DEF),
            "KNOT_DEF not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn var_produces_var_decl(input in arb_var_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::VAR_DECL),
            "VAR_DECL not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn const_produces_const_decl(input in arb_const_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::CONST_DECL),
            "CONST_DECL not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn list_produces_list_decl(input in arb_list_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::LIST_DECL),
            "LIST_DECL not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn external_produces_external_decl(input in arb_external_decl()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::EXTERNAL_DECL),
            "EXTERNAL_DECL not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn include_produces_include_stmt(input in arb_include_stmt()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::INCLUDE_STMT),
            "INCLUDE_STMT not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn choice_produces_choice_node(input in arb_choice_line()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::CHOICE),
            "CHOICE not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }

    #[test]
    fn gather_produces_gather_node(input in arb_gather_line()) {
        let parsed = parse(&input);
        let root = parsed.syntax();
        prop_assert!(
            has_node_kind(&root, SyntaxKind::GATHER),
            "GATHER not found for input: {:?}\nTree:\n{:#?}",
            input, root,
        );
    }
}
