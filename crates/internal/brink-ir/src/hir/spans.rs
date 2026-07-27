//! Best-effort source spans for HIR expression trees (issue #1492).
//!
//! ## Why this exists
//!
//! `brink-analyzer` records typing verdicts *beside* the HIR, in a
//! `(node → payload)` side table keyed by `(FileId, TextRange)` (the
//! `SideTable`/`NodeKey` pattern issue #1482 shipped). LIR lowering is one
//! of that table's ruled consumers — it must be able to reconstruct the
//! **same** key for the **same** node, from the same HIR, without depending
//! on `brink-analyzer` (`brink-ir` does not, and must not, depend on it).
//!
//! [`expr_span`] is that shared key derivation: one function, in the crate
//! both sides already depend on, so the two can never disagree about what
//! range identifies an expression.
//!
//! ## Own provenance first, subtree union as the fallback
//!
//! An expression that carries its own [`crate::Provenance`] — every
//! extension shape, and (since issue #1517) [`crate::InfixExpr`] — is keyed
//! by **that node's own range**, so it is separately addressable from every
//! other node in the tree.
//!
//! The variants with no provenance of their own — `Prefix`/`Postfix`,
//! `Call`, `String` interpolation, `Path`/`ListLiteral`, and the scalar
//! literals (`Int`, `Float`, `Bool`, `Null`) — fall back to the **union of
//! every range reachable in the subtree**, or `None` when the subtree
//! contains not a single ranged node. A consumer keying one of *those*
//! shapes still inherits the old caveats: a `None` span cannot be keyed at
//! all, and a wrapper can share a span with the operand it wraps (`-x`
//! spans exactly what `x` does).
//!
//! ### The collision issue #1517 removed
//!
//! Before infix nodes carried provenance, this function's union was the
//! *only* identity available for them, and a left-associative chain shared
//! it with its own left spine whenever the trailing operand contributed no
//! range (`some(a) or f() or 99` spanned exactly what `some(a) or f()`
//! did). The two ruled side-table consumers had to detect that collision
//! and drop both entries. They no longer do: an infix node's own range
//! strictly contains its left operand's, so a chain root and its spine are
//! always distinct keys.
//!
//! This is deliberately *not* a diagnostic anchor — `brink-analyzer`'s own
//! `expr_anchor` helpers stay the anchor policy (leftmost meaningful token).
//! This is an identity key, and it wants maximal coverage, not tightness.

use rowan::TextRange;

use crate::hir::types::{Expr, StringPart};

/// The node's own provenance range where it has one, else the union of
/// every source range reachable inside `expr` — `None` only when neither is
/// available.
///
/// See the module doc for the contract: this is an **identity key** for
/// side-table lookups. It is exact for a provenance-carrying node (every
/// extension shape, and [`Expr::Infix`]); for the remaining shapes it stays
/// best-effort, so a caller keying one of those must still treat `None` and
/// a collision with a wrapped operand as "no verdict".
#[must_use]
pub fn expr_span(expr: &Expr) -> Option<TextRange> {
    let mut span = None;
    match expr {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Null => {}
        Expr::Path(p) | Expr::DivertTarget(p) => cover(&mut span, Some(p.range)),
        Expr::ListLiteral(items) => {
            for p in items {
                cover(&mut span, Some(p.range));
            }
        }
        Expr::String(s) => {
            for part in &s.parts {
                if let StringPart::Interpolation(inner) = part {
                    cover(&mut span, expr_span(inner));
                }
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => cover(&mut span, expr_span(inner)),
        // An infix node's own range (issue #1517) — never the subtree
        // union, which is exactly what used to collide a chain with its own
        // left spine.
        Expr::Infix(ie) => return Some(ie.ptr.range),
        Expr::Call(path, args) => {
            cover(&mut span, Some(path.range));
            for a in args {
                cover(&mut span, expr_span(a));
            }
        }
        Expr::ArrayLiteral(a) => {
            cover(&mut span, Some(a.ptr.range));
            for e in &a.elements {
                cover(&mut span, expr_span(e));
            }
        }
        Expr::MapLiteral(m) => {
            cover(&mut span, Some(m.ptr.range));
            for (k, v) in &m.entries {
                cover(&mut span, expr_span(k));
                cover(&mut span, expr_span(v));
            }
        }
        Expr::Index(idx) => {
            cover(&mut span, Some(idx.ptr.range));
            cover(&mut span, expr_span(&idx.base));
            cover(&mut span, expr_span(&idx.index));
        }
        Expr::Range(r) => {
            cover(&mut span, Some(r.ptr.range));
            cover(&mut span, expr_span(&r.start));
            cover(&mut span, expr_span(&r.end));
        }
        Expr::StructLiteral(sl) => {
            cover(&mut span, Some(sl.ptr.range));
            for (_, v) in &sl.fields {
                cover(&mut span, expr_span(v));
            }
        }
        Expr::FieldAccess(fa) => {
            cover(&mut span, Some(fa.ptr.range));
            cover(&mut span, expr_span(&fa.base));
        }
        Expr::FnLiteral(fl) => {
            cover(&mut span, Some(fl.ptr.range));
            for a in &fl.args {
                cover(&mut span, expr_span(a));
            }
        }
        Expr::RefArg(ra) => {
            cover(&mut span, Some(ra.ptr.range));
            cover(&mut span, expr_span(&ra.operand));
        }
    }
    span
}

/// Widen `span` to also cover `next`, tolerating an absent side.
fn cover(span: &mut Option<TextRange>, next: Option<TextRange>) {
    let Some(next) = next else {
        return;
    };
    *span = Some(match *span {
        Some(current) => current.cover(next),
        None => next,
    });
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test-only assertions; see sibling test modules"
)]
mod tests {
    use super::*;
    use crate::FileId;
    use crate::hir::types::{HirFile, Stmt};

    /// Lower one native `flow` body and hand back its first statement's
    /// expression — the coalescing shapes this helper exists for are
    /// produced only by the native frontend (`InfixOp::Coalesce`).
    fn first_logic_expr(src: &str) -> Expr {
        let parse = brink_syntax_native::parse(src);
        assert!(parse.errors().is_empty(), "{:?}", parse.errors());
        let tree = parse.tree();
        let (hir, _manifest, _diag): (HirFile, _, _) =
            crate::hir::lower_native::lower(FileId(0), &tree);
        let knot = hir.knots.first().expect("one flow");
        for stmt in &knot.body.stmts {
            if let Stmt::Content(c) = stmt {
                for part in &c.parts {
                    if let crate::hir::types::ContentPart::Interpolation(e) = part {
                        return e.clone();
                    }
                }
            }
        }
        panic!("no inline expression found in {src}");
    }

    fn text(src: &str, range: TextRange) -> String {
        src[usize::from(range.start())..usize::from(range.end())].to_string()
    }

    #[test]
    fn scalar_literals_carry_no_span() {
        assert_eq!(expr_span(&Expr::Int(5)), None);
        assert_eq!(expr_span(&Expr::Bool(true)), None);
        assert_eq!(expr_span(&Expr::Null), None);
    }

    #[test]
    fn a_path_spans_itself() {
        let src = "flow main() {\n  {x}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let span = expr_span(&e).expect("path has a range");
        assert_eq!(text(src, span), "x");
    }

    #[test]
    fn an_infix_spans_its_own_node() {
        let src = "flow main() {\n  {a or b}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let span = expr_span(&e).expect("an infix node carries its own range");
        assert_eq!(text(src, span), "a or b");
    }

    /// The #1517 fix, pinned: a chain whose trailing operand carries no
    /// range of its own is still distinguishable from its own left spine,
    /// because the span now comes from the infix node's own provenance
    /// instead of the union of its subtree. This is the exact fixture that
    /// used to collide, and the reason both side-table consumers needed a
    /// collision-poisoning workaround.
    #[test]
    fn a_chain_and_its_left_spine_have_distinct_spans() {
        let src = "flow main() {\n  {a or b or 99}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let whole = expr_span(&e).expect("chain carries its own range");
        assert_eq!(text(src, whole), "a or b or 99");
        let Expr::Infix(root) = &e else {
            panic!("expected a left-associative chain, got {e:?}");
        };
        let inner = expr_span(&root.lhs).expect("the left spine is an infix too");
        // The stamped range is the CST node's real `text_range()`, which for a
        // non-terminal chain node includes the trailing whitespace trivia before
        // the next operator (brink-syntax-native's `expression_bp` runs
        // `skip_ws()` while the checkpoint-wrapped node is still open). That
        // trivia is part of the provenance on purpose — it must stay exact for
        // resolver round-trip and LIR/analyzer key agreement — so trim only in
        // the assertion, not the stamped range itself.
        assert_eq!(text(src, inner).trim_end(), "a or b");
        assert_ne!(inner, whole, "root and spine must be separately keyable");
    }

    /// The trailing literal itself is still span-less — #1517 gave infix
    /// nodes provenance, not the scalar literal variants. It no longer
    /// matters for keying, because nothing keys a bare literal.
    #[test]
    fn a_trailing_scalar_literal_still_carries_no_span_of_its_own() {
        let src = "flow main() {\n  {a or b or 99}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let Expr::Infix(root) = &e else {
            panic!("expected a left-associative chain, got {e:?}");
        };
        assert_eq!(expr_span(&root.rhs), None);
    }

    #[test]
    fn a_call_spans_its_callee_and_arguments() {
        let src = "flow main() {\n  {f(a, b)}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let span = expr_span(&e).expect("call has a callee range");
        assert_eq!(text(src, span), "f(a, b");
    }

    /// Before #1517 this subtree had no span at all (neither operand is
    /// ranged, and the infix node had nothing of its own), so it could not
    /// be keyed. The infix node's own provenance now keys it.
    #[test]
    fn an_all_literal_infix_is_keyed_by_the_operation_itself() {
        let src = "flow main() {\n  {5 or 9}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let span = expr_span(&e).expect("the operation carries its own range");
        assert_eq!(text(src, span), "5 or 9");
    }
}
