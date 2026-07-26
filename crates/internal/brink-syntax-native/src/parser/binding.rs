//! The `as` binding (B1b, issue #1475) — one construct, both condition
//! positions.
//!
//! Ruled `docs/decision-log.md` 2026-07-26 ("The `as` binding: one
//! construct, both condition positions, `{if}` spelling"): the statement
//! form (`if EXPR as NAME { … }`, `while EXPR as NAME { … }`,
//! `parser/control_flow.rs`) and the template form (`{if EXPR as NAME: …
//! else: …}`, `parser/family.rs`) are the SAME `if` construct in brink's
//! two condition positions, so they share this one grammar rule and one
//! node kind ([`AS_BINDING`]) rather than forking into two binding
//! spellings.
//!
//! The binding is a **suffix of the condition head**, never an expression
//! operator: `expr::expression` never sees `as`, so `KW_AS` cannot be
//! absorbed mid-expression, and the "the binding is the ENTIRE condition"
//! v1 restriction falls out of the grammar shape instead of needing a
//! precedence rule. What the grammar can't refuse on its own is a *trailing*
//! `&&`/`||`/`or` after the binding (`if find(x) as s && s.sharp { … }`) —
//! that's [`reject_composition`]'s clear parse error, per the ruling's
//! "composition with `&&`/`||` is a parse/analysis error with a clear
//! diagnostic". The mirror-image spelling (`if a && find(x) as s { … }`,
//! where the binding lands on a boolean sub-expression) is not a parse
//! failure at all — it parses as a binding over `a && find(x)` — and is
//! rejected downstream by `brink-ir`'s `E140`, which can see the bound
//! expression's shape.

use crate::SyntaxKind::{AMP_AMP, AS_BINDING, IDENT, KW_AS, KW_OR, PIPE};

use super::Parser;

/// Whether an `as` binding follows the just-parsed condition head.
pub(crate) fn at_as_binding(p: &Parser<'_, '_>) -> bool {
    p.at(KW_AS)
}

/// `as NAME` — parsed as a trailing sibling of the condition head, inside
/// the construct that binds it (`IF_STMT`/`WHILE_STMT`/
/// `CONDITIONAL_BLOCK`/`CHOICE_GUARD`).
///
/// Call sites must check [`at_as_binding`] first; this unconditionally
/// consumes the `as` keyword.
pub(crate) fn as_binding(p: &mut Parser<'_, '_>) {
    p.start_node(AS_BINDING);
    p.expect(KW_AS);
    p.expect(IDENT);
    p.finish_node();
    reject_composition(p);
}

/// The v1 whole-condition restriction: an operator directly after the
/// binding means the author tried to compose `as` with `&&`/`||`, which
/// let-chains would admit later but v1 does not. Diagnose it by name —
/// the fallback would be `expected L_BRACE, found AMP_AMP`, which says
/// nothing about the actual rule.
///
/// `||` is two adjacent `PIPE` tokens (the lexer has no compound token for
/// it — `expr::infix_expression`'s own `is_double_pipe` check), and a
/// single `PIPE` here can only be a mis-typed one, so both are caught.
fn reject_composition(p: &mut Parser<'_, '_>) {
    p.skip_ws();
    if matches!(p.current(), AMP_AMP | PIPE | KW_OR) {
        p.error(
            "the `as` binding must be the entire condition — it cannot be composed with \
             `&&`/`||`/`or` (issue #1475, `docs/decision-log.md` 2026-07-26)"
                .into(),
        );
    }
}
