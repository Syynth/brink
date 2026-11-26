use std::fmt::Write;

use brink_syntax::{SyntaxKind, parse};
use proptest::prelude::*;

// ── Constants ────────────────────────────────────────────────────────

const KEYWORDS: &[&str] = &[
    "INCLUDE", "EXTERNAL", "VAR", "CONST", "LIST", "temp", "return", "ref", "true", "false", "not",
    "and", "or", "mod", "has", "hasnt", "else", "function", "stopping", "cycle", "shuffle", "once",
    "DONE", "END", "TODO",
    // T1b contextual block keywords (docs/t1b-surface-spec.md §2) are soft —
    // legal identifiers everywhere except at block-statement-start position.
    // Excluded here purely so the generic `arb_ident()` generator used
    // throughout this file doesn't occasionally land on the one position
    // where they shadow, which would be a spurious property-test failure,
    // not a parser bug.
    "if", "while", "for", "break", "continue", "in",
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
            (arb_ident(), inner.clone()).prop_map(|(name, arg)| format!("{name}({arg})")),
            // T1b §3: array sigil literal `#[…]`
            prop::collection::vec(inner.clone(), 0..=3)
                .prop_map(|items| format!("#[{}]", items.join(", "))),
            // T1b §3: map sigil literal `#{…}`
            prop::collection::vec((arb_ident(), inner.clone()), 0..=3).prop_map(|entries| {
                let body = entries
                    .iter()
                    .map(|(k, v)| format!("\"{k}\": {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("#{{{body}}}")
            }),
            // T1b §4: postfix indexing `base[index]`
            (inner.clone(), inner.clone()).prop_map(|(base, idx)| format!("{base}[{idx}]")),
            // T1c §2: `#fn(target, args…)` function-value creation literal —
            // joins the `#[…]`/`#{…}` sigil family. The target is always a
            // bare name (never an expression, per the grammar), so this
            // doesn't recurse on `inner` for the target position, only for
            // the bound args.
            (arb_ident(), prop::collection::vec(inner, 0..=2))
                .prop_map(|(name, args)| format_fn_literal(&name, &args)),
        ]
    })
}

/// Render `#fn(target, args…)` — a bare `#fn(target)` with no trailing
/// comma when `args` is empty (matching the grammar's zero-bound-args shape,
/// e.g. `#fn(double)`), `#fn(target, a, b, …)` otherwise.
fn format_fn_literal(name: &str, args: &[String]) -> String {
    if args.is_empty() {
        format!("#fn({name})")
    } else {
        format!("#fn({name}, {})", args.join(", "))
    }
}

/// `#fn(target, args…)` — same recursive-arg shape as the `arb_expr` arm
/// above, generated standalone (T1c, docs/t1c-spec.md §2) so it can be fuzzed
/// as a top-level statement, not only nested inside a larger expression.
fn arb_fn_literal() -> impl Strategy<Value = String> {
    (arb_ident(), prop::collection::vec(arb_expr(), 0..=3))
        .prop_map(|(name, args)| format_fn_literal(&name, &args))
}

/// `~ temp f = #fn(target, args…)` — a `#fn` literal in the one statement
/// position every other `arb_*_line` strategy above already covers for
/// ordinary expressions (`arb_logic_line`'s `~ temp x = expr` arm).
fn arb_fn_literal_temp_decl() -> impl Strategy<Value = String> {
    (arb_ident(), arb_fn_literal()).prop_map(|(name, fl)| format!("~ temp {name} = {fl}\n"))
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
        // ~ x[idx] = expr — T1b §4 indexed assignment
        (arb_ident(), arb_expr(), arb_expr())
            .prop_map(|(name, idx, e)| format!("~ {name}[{idx}] = {e}\n")),
    ]
}

// ── T1b multi-line `~ { … }` block strategy (docs/t1b-surface-spec.md §2) ────

/// One non-nesting statement inside a block body.
fn arb_block_simple_stmt() -> impl Strategy<Value = String> {
    prop_oneof![
        (arb_ident(), arb_expr()).prop_map(|(name, e)| format!("temp {name} = {e}")),
        (arb_ident(), arb_expr()).prop_map(|(name, e)| format!("{name} = {e}")),
        (arb_ident(), arb_expr(), arb_expr())
            .prop_map(|(name, idx, e)| format!("{name}[{idx}] = {e}")),
        arb_expr().prop_map(|e| format!("return {e}")),
        Just("return".to_string()),
        Just("break".to_string()),
        Just("continue".to_string()),
        arb_ident().prop_map(|name| format!("{name}()")),
    ]
}

/// One statement inside a block body — a simple statement, or one level of
/// `if`/`if-else`/`while`/`for` wrapping a simple statement.
fn arb_block_stmt() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => arb_block_simple_stmt(),
        1 => (arb_expr(), arb_block_simple_stmt())
            .prop_map(|(cond, s)| format!("if {cond} {{\n{s}\n}}")),
        1 => (arb_expr(), arb_block_simple_stmt(), arb_block_simple_stmt())
            .prop_map(|(cond, s1, s2)| format!("if {cond} {{\n{s1}\n}} else {{\n{s2}\n}}")),
        1 => (arb_expr(), arb_block_simple_stmt())
            .prop_map(|(cond, s)| format!("while {cond} {{\n{s}\n}}")),
        1 => (arb_ident(), arb_expr(), arb_block_simple_stmt())
            .prop_map(|(var, iter, s)| format!("for {var} in {iter} {{\n{s}\n}}")),
    ]
}

fn arb_block_logic_line() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_block_stmt(), 1..=4)
        .prop_map(|stmts| format!("~ {{\n{}\n}}\n", stmts.join("\n")))
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

// ── TM-2 type annotation strategy (depth-bounded, docs/typed-mode-spec.md
// §3) ──────────────────────────────────────────────────────────────────

fn arb_type_leaf() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("int".to_string()),
        Just("float".to_string()),
        Just("bool".to_string()),
        Just("string".to_string()),
        Just("divert".to_string()),
        Just("void".to_string()),
        // Exercises the "unknown type name" surface too. `fn` is excluded:
        // it's a contextual keyword in type position (starts a `fn(...):
        // ...` function type — see `parser::types::type_fn`), so landing on
        // it bare would be a spurious property-test failure, not a bug.
        arb_ident().prop_filter("must not be the `fn` contextual keyword", |s| s != "fn"),
    ]
}

fn arb_type_expr() -> impl Strategy<Value = String> {
    arb_type_leaf().prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            inner.clone().prop_map(|t| format!("Array<{t}>")),
            (inner.clone(), inner.clone()).prop_map(|(k, v)| format!("Map<{k}, {v}>")),
            inner.clone().prop_map(|t| format!("List<{t}>")),
            prop::collection::vec(inner.clone(), 0..=3).prop_map(|params| format!(
                "fn({}): {}",
                params.join(", "),
                "int"
            )),
        ]
    })
}

fn arb_var_decl_typed() -> impl Strategy<Value = String> {
    (arb_ident(), arb_type_expr(), arb_simple_value())
        .prop_map(|(name, ty, val)| format!("VAR {name}: {ty} = {val}\n"))
}

/// #641: CONST mirrors VAR's typed-declaration strategy.
fn arb_const_decl_typed() -> impl Strategy<Value = String> {
    (arb_ident(), arb_type_expr(), arb_simple_value())
        .prop_map(|(name, ty, val)| format!("CONST {name}: {ty} = {val}\n"))
}

fn arb_temp_decl_typed() -> impl Strategy<Value = String> {
    (arb_ident(), arb_type_expr(), arb_ident())
        .prop_map(|(name, ty, val)| format!("~ temp {name}: {ty} = {val}\n"))
}

/// A deliberately narrow body generator for the TM-2 knot-header tests
/// below — `arb_body()` (via `arb_content_with_inline`) can produce
/// content-line/inline-logic combinations that are a pre-existing,
/// unrelated generator edge case (not a TM-2 regression); these tests only
/// need *some* well-formed body to follow a typed header, not full body
/// coverage (already exercised by `story_roundtrip` et al.).
fn arb_typed_test_body() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_content_line(), 1..=3).prop_map(|lines| lines.join(""))
}

// ── TM-4b structs (docs/typed-mode-spec.md §6) ────────────────────────

/// A struct field name distinct from the shape/base identifiers used
/// alongside it in the same generated snippet — avoids a field literally
/// named `fn` (a contextual type keyword) tripping up unrelated grammar,
/// mirroring `arb_type_leaf`'s own filter.
fn arb_field_name() -> impl Strategy<Value = String> {
    arb_ident().prop_filter("must not be the `fn` contextual keyword", |s| s != "fn")
}

/// `STRUCT Name = #{field: type, …}` — single-line body (the multi-line
/// case is covered by hand-written unit tests; the property-test generator
/// stays single-line so `arb_field_name`/`arb_type_leaf` combinations don't
/// need newline-aware joining).
fn arb_struct_decl() -> impl Strategy<Value = String> {
    (
        arb_ident(),
        prop::collection::vec((arb_field_name(), arb_type_leaf()), 0..=4),
    )
        .prop_map(|(name, fields)| {
            let body = fields
                .iter()
                .map(|(f, t)| format!("{f}: {t}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("STRUCT {name} = #{{{body}}}\n")
        })
}

/// `Name#{field: expr, …}` — struct construction literal, generated as a
/// `~` assignment so it parses in expression position.
fn arb_struct_literal() -> impl Strategy<Value = String> {
    (
        arb_ident(),
        prop::collection::vec((arb_field_name(), arb_simple_value()), 0..=4),
    )
        .prop_map(|(shape, fields)| {
            let body = fields
                .iter()
                .map(|(f, v)| format!("{f}: {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("~ p = {shape}#{{{body}}}\n")
        })
}

/// `~ x = Name#{…}.field` — postfix field access after a struct literal
/// (the unambiguous new grammar; a bare `ident.ident` chain stays a `PATH`,
/// exercised separately by the existing dotted-path generators).
fn arb_field_access() -> impl Strategy<Value = String> {
    (arb_ident(), arb_field_name(), arb_field_name()).prop_map(
        |(shape, init_field, access_field)| {
            format!("~ x = {shape}#{{{init_field}: 1}}.{access_field}\n")
        },
    )
}

fn arb_knot_header_typed() -> impl Strategy<Value = String> {
    (
        arb_ident(),
        prop::collection::vec((arb_ident(), arb_type_expr()), 0..=3),
        arb_type_expr(),
    )
        .prop_map(|(name, params, ret)| {
            let params = params
                .iter()
                .map(|(n, t)| format!("{n}: {t}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("=== function {name}({params}): {ret} ===\n")
        })
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
    fn block_logic_line_roundtrip(input in arb_block_logic_line()) {
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

    // ── TM-2 type annotations (docs/typed-mode-spec.md §3) ────────────

    #[test]
    fn var_decl_typed_roundtrip(input in arb_var_decl_typed()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn var_decl_typed_no_errors(input in arb_var_decl_typed()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn var_decl_typed_produces_type_annotation(input in arb_var_decl_typed()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::TYPE_ANNOTATION));
    }

    #[test]
    fn const_decl_typed_roundtrip(input in arb_const_decl_typed()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn const_decl_typed_no_errors(input in arb_const_decl_typed()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn const_decl_typed_produces_type_annotation(input in arb_const_decl_typed()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::TYPE_ANNOTATION));
    }

    #[test]
    fn temp_decl_typed_roundtrip(input in arb_temp_decl_typed()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn temp_decl_typed_no_errors(input in arb_temp_decl_typed()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn knot_header_typed_roundtrip((header, body) in (arb_knot_header_typed(), arb_typed_test_body())) {
        let input = format!("{header}{body}");
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn knot_header_typed_no_errors((header, body) in (arb_knot_header_typed(), arb_typed_test_body())) {
        let input = format!("{header}{body}");
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    /// Never panics on any type-annotated input — extends the grammar fuzz
    /// coverage (docs/t1b-surface-spec.md §6 pattern) to TM-2's superset
    /// grammar: the parser must never crash, in either dialect (the parser
    /// itself is dialect-agnostic — dialect gating happens at analysis).
    #[test]
    fn type_annotated_input_never_panics(input in prop_oneof![
        arb_var_decl_typed(),
        arb_const_decl_typed(),
        arb_temp_decl_typed(),
        (arb_knot_header_typed(), arb_typed_test_body()).prop_map(|(h, b)| format!("{h}{b}")),
    ]) {
        let _ = parse(&input);
    }

    // ── TM-4b structs (docs/typed-mode-spec.md §6) ────────────────────

    #[test]
    fn struct_decl_roundtrip(input in arb_struct_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn struct_decl_no_errors(input in arb_struct_decl()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn struct_decl_produces_struct_decl_node(input in arb_struct_decl()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::STRUCT_DECL));
    }

    #[test]
    fn struct_literal_roundtrip(input in arb_struct_literal()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn struct_literal_no_errors(input in arb_struct_literal()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn struct_literal_produces_struct_literal_node(input in arb_struct_literal()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::STRUCT_LITERAL));
    }

    #[test]
    fn field_access_roundtrip(input in arb_field_access()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn field_access_no_errors(input in arb_field_access()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn field_access_produces_field_access_expr_node(input in arb_field_access()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::FIELD_ACCESS_EXPR));
    }

    /// Never panics on any TM-4b struct input, in either dialect — extends
    /// the grammar fuzz coverage (docs/t1b-surface-spec.md §6 pattern,
    /// docs/typed-mode-spec.md §6) the same way TM-2's
    /// `type_annotated_input_never_panics` does.
    #[test]
    fn struct_input_never_panics(input in prop_oneof![
        arb_struct_decl(),
        arb_struct_literal(),
        arb_field_access(),
    ]) {
        let _ = parse(&input);
    }

    // ── T1c (docs/t1c-spec.md §2/§9/§11): `#fn(target, args…)` function-value
    // creation literal — joins the `#[…]`/`#{…}` sigil family, so it gets the
    // same lossless-roundtrip / no-errors / node-kind / never-panics coverage
    // those literals already have. "Grammar fuzzing extends to `#fn` in both
    // dialects" (spec §9): the parser is dialect-agnostic by construction
    // (dialect gating happens at analysis, matching TM-2's and TM-4b's own
    // `never_panics` precedent), so this fuzzes it once, at the parser layer.

    #[test]
    fn fn_literal_temp_decl_roundtrip(input in arb_fn_literal_temp_decl()) {
        let parsed = parse(&input);
        prop_assert_eq!(parsed.syntax().text().to_string(), input);
    }

    #[test]
    fn fn_literal_temp_decl_no_errors(input in arb_fn_literal_temp_decl()) {
        let parsed = parse(&input);
        prop_assert!(
            parsed.errors().is_empty(),
            "parse errors for input: {:?}\nerrors: {:?}",
            input, parsed.errors(),
        );
    }

    #[test]
    fn fn_literal_temp_decl_produces_fn_literal_node(input in arb_fn_literal_temp_decl()) {
        let parsed = parse(&input);
        prop_assert!(has_node_kind(&parsed.syntax(), SyntaxKind::FN_LITERAL));
    }

    /// Never panics on any `#fn` input, standalone or nested inside a larger
    /// expression (`arb_expr`'s own `#fn` arm, exercised transitively via
    /// every `arb_logic_line`/`arb_block_stmt`/`arb_story` fuzz run above) —
    /// extends the grammar fuzz coverage the same way TM-2's
    /// `type_annotated_input_never_panics` and TM-4b's `struct_input_never_panics`
    /// do. The parser itself is dialect-agnostic, so this covers both
    /// dialects by construction — dialect gating (E051 under strict-ink) is
    /// an analysis-layer concern, never a parse-layer one.
    #[test]
    fn fn_literal_input_never_panics(input in prop_oneof![
        arb_fn_literal(),
        arb_fn_literal_temp_decl(),
    ]) {
        let _ = parse(&input);
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
    fn block_logic_line_no_errors(input in arb_block_logic_line()) {
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
