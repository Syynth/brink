//! TM-2 inline type annotation grammar (docs/typed-mode-spec.md §3).
//!
//! `name: type` after params and `VAR`/`temp` declarations, `): type ===`
//! in the function-header return position. Superset grammar — parses
//! identically in both dialects; `strict-ink` rejection (E051) happens at
//! analysis (`brink-analyzer::dialect_gate`), never here. Nominal type names
//! are lowercase: `int`, `float`, `bool`, `string`, `divert`, `void`,
//! `list<L>`, `array<T>`, `map<K, V>`, `fn(T…): R`. Grammar accepts *any*
//! identifier as a type name or generic head — recognizing the fixed set,
//! rejecting unknown names, and flagging `fn(...)` as reserved-until-T1c are
//! semantic checks (`brink-analyzer`), not parser concerns.

use crate::SyntaxKind::{
    COLON, COMMA, GT, IDENTIFIER, L_PAREN, LT, R_PAREN, TYPE_ANNOTATION, TYPE_EXPR, TYPE_FN,
    TYPE_GENERIC, TYPE_NAME,
};

use super::Parser;
use super::expression::skip_balanced;

/// Returns `true` if the current token starts a type annotation (`: type`).
pub(crate) fn at_type_annotation(p: &Parser<'_, '_>) -> bool {
    p.current() == COLON
}

/// Parse `: type_expr`.
///
/// ```text
/// type_annotation = { ":" ~ type_expr }
/// ```
pub(crate) fn type_annotation(p: &mut Parser<'_, '_>) {
    p.start_node(TYPE_ANNOTATION);
    // `at_type_annotation` only peeks (`current()` skips trivia without
    // consuming it) — consume any pending trivia before the `:` ourselves,
    // so it nests inside this node rather than a sibling one.
    p.skip_ws();
    p.bump(); // COLON
    p.skip_ws();
    type_expr(p);
    p.finish_node();
}

/// Parse a type expression: a function type, a generic (`name<args>`), or a
/// bare name.
///
/// ```text
/// type_expr = { type_fn | type_name_or_generic }
/// ```
fn type_expr(p: &mut Parser<'_, '_>) {
    p.start_node(TYPE_EXPR);
    if p.at_depth_limit() {
        p.error("nesting depth limit exceeded".into());
        p.finish_node();
        return;
    }
    if p.at_kw_text("fn") {
        type_fn(p);
    } else if p.at_ident_or_keyword() {
        type_name_or_generic(p);
    } else {
        p.error("expected type".into());
    }
    p.finish_node();
}

/// Parse a bare type name, or a generic instantiation if `<` follows.
///
/// ```text
/// type_name_or_generic = { identifier ~ ("<" ~ type_expr ~ ("," ~ type_expr)* ~ ">")? }
/// ```
fn type_name_or_generic(p: &mut Parser<'_, '_>) {
    let checkpoint = p.checkpoint();
    p.start_node(IDENTIFIER);
    p.expect_ident_or_keyword();
    p.finish_node();
    p.skip_ws();

    if p.current() == LT {
        p.start_node_at(checkpoint, TYPE_GENERIC);
        p.bump(); // LT
        if p.at_depth_limit() {
            p.error("nesting depth limit exceeded".into());
            skip_balanced(p, LT, GT);
            p.finish_node();
            return;
        }
        p.depth += 1;
        p.skip_ws();
        type_expr(p);
        loop {
            p.skip_ws();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws();
            type_expr(p);
        }
        p.skip_ws();
        p.expect(GT);
        p.depth -= 1;
        p.finish_node();
    } else {
        p.start_node_at(checkpoint, TYPE_NAME);
        p.finish_node();
    }
}

/// Parse `fn(type_expr, …): type_expr` — a function type (T1c reserved:
/// parses in the superset grammar per typed-mode-spec §3, but
/// `brink-analyzer` flags any use as "function types land with T1c").
///
/// ```text
/// type_fn = { "fn" ~ "(" ~ (type_expr ~ ("," ~ type_expr)*)? ~ ")" ~ ":" ~ type_expr }
/// ```
fn type_fn(p: &mut Parser<'_, '_>) {
    p.start_node(TYPE_FN);
    p.bump(); // "fn" (IDENT, contextual)
    p.skip_ws();
    p.expect(L_PAREN);
    if p.at_depth_limit() {
        p.error("nesting depth limit exceeded".into());
        skip_balanced(p, L_PAREN, R_PAREN);
        p.finish_node();
        return;
    }
    p.depth += 1;
    p.skip_ws();
    if p.current() != R_PAREN {
        type_expr(p);
        loop {
            p.skip_ws();
            if !p.eat(COMMA) {
                break;
            }
            p.skip_ws();
            if p.current() == R_PAREN {
                break; // trailing comma
            }
            type_expr(p);
        }
    }
    p.skip_ws();
    p.expect(R_PAREN);
    p.depth -= 1;
    p.skip_ws();
    p.expect(COLON);
    p.skip_ws();
    type_expr(p);
    p.finish_node();
}

#[cfg(test)]
mod tests {
    use crate::SyntaxKind;
    use crate::parse;

    fn dump(src: &str) -> String {
        let parsed = parse(src);
        format!("{:#?}", parsed.syntax())
    }

    #[test]
    fn param_annotation_parses_type_generic_and_type_annotation_nodes() {
        let out = dump("=== function heal(ref hp: int, amount: int): int ===\n~ return hp\n");
        assert!(out.contains("TYPE_ANNOTATION"), "{out}");
        assert!(out.contains("TYPE_NAME"), "{out}");
    }

    #[test]
    fn var_annotation_parses() {
        let out = dump("VAR gold: int = 100\n");
        assert!(out.contains("TYPE_ANNOTATION"), "{out}");
        assert_eq!(parse("VAR gold: int = 100\n").errors().len(), 0);
    }

    #[test]
    fn temp_ascription_parses() {
        let out = dump("~ temp name: string = who\n");
        assert!(out.contains("TYPE_ANNOTATION"), "{out}");
    }

    #[test]
    fn generic_list_type_parses() {
        let out = dump("VAR w: list<Weathers> = sunny\n");
        assert!(
            out.contains(&format!("{:?}", SyntaxKind::TYPE_GENERIC)),
            "{out}"
        );
    }

    #[test]
    fn generic_map_type_parses_two_args() {
        let out = dump("VAR m: map<string, int> = 0\n");
        assert!(
            out.contains(&format!("{:?}", SyntaxKind::TYPE_GENERIC)),
            "{out}"
        );
    }

    #[test]
    fn fn_type_parses_reserved_syntax() {
        let out = dump("VAR cb: fn(int, int): bool = 0\n");
        assert!(out.contains(&format!("{:?}", SyntaxKind::TYPE_FN)), "{out}");
    }

    #[test]
    fn void_return_type_parses_as_type_name() {
        let out = dump("=== function noop(): void ===\n~ return\n");
        assert!(out.contains("TYPE_NAME"), "{out}");
    }

    #[test]
    fn unknown_type_name_still_parses_no_error() {
        // Grammar accepts any identifier; the "unknown type name" check is a
        // semantic diagnostic, not a parse error.
        let parsed = parse("VAR p: Frobnicator = 0\n");
        assert_eq!(parsed.errors().len(), 0, "{:?}", parsed.errors());
    }

    #[test]
    fn no_annotation_still_parses_plain_declarations() {
        assert_eq!(parse("VAR gold = 100\n").errors().len(), 0);
        assert_eq!(parse("~ temp name = who\n").errors().len(), 0);
        assert_eq!(
            parse("=== heal(hp, amount) ===\n~ return hp\n")
                .errors()
                .len(),
            0
        );
    }
}
