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
//! ## Why it is best-effort
//!
//! Not every HIR expression carries source provenance. `Expr::Infix` is a
//! bare `(Box<Expr>, InfixOp, Box<Expr>)` tuple with no `Provenance` of its
//! own, and the scalar literal variants (`Int`, `Float`, `Bool`, `Null`)
//! carry none either — only `Path`s (their `range`) and the
//! `Provenance`-carrying extension shapes do. So this function returns the
//! **union of every range reachable in the subtree**, or `None` when the
//! subtree contains not a single ranged node (`5 or 9`).
//!
//! Two consequences a consumer must respect, both of which the side-table
//! producer already handles by recording nothing rather than guessing:
//!
//! - A `None` span cannot be keyed at all.
//! - Two *distinct* nodes can share a span — most importantly an `or`-chain
//!   and its own left spine, since a trailing literal fallback contributes
//!   no range (`some(a) or f() or 99` spans exactly what `some(a) or f()`
//!   does). A producer keying nodes by this span must therefore detect the
//!   collision and drop **both** entries; a verdict for the wrong node is a
//!   miscompile, an absent verdict is only a missed optimization.
//!
//! This is deliberately *not* a diagnostic anchor — `brink-analyzer`'s own
//! `expr_anchor` helpers stay the anchor policy (leftmost meaningful token).
//! This is an identity key, and it wants maximal coverage, not tightness.

use rowan::TextRange;

use crate::hir::types::{Expr, StringPart};

/// The union of every source range reachable inside `expr`, or `None` when
/// the subtree carries no source provenance at all.
///
/// See the module doc for the contract: this is an **identity key** for
/// side-table lookups, best-effort by construction, and callers must treat
/// both `None` and a collision between two distinct nodes as "no verdict".
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
        Expr::Infix(lhs, _, rhs) => {
            cover(&mut span, expr_span(lhs));
            cover(&mut span, expr_span(rhs));
        }
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
    fn an_infix_spans_the_union_of_its_ranged_operands() {
        let src = "flow main() {\n  {a or b}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let span = expr_span(&e).expect("both operands are paths");
        assert_eq!(text(src, span), "a or b");
    }

    #[test]
    fn a_trailing_scalar_literal_contributes_nothing() {
        // The collision the module doc warns about, demonstrated: the whole
        // chain spans exactly what its left spine spans, because `99` has
        // no range of its own. A side-table producer must poison this key,
        // not guess which node it meant.
        let src = "flow main() {\n  {a or b or 99}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let whole = expr_span(&e).expect("chain has ranged operands");
        let covered = text(src, whole);
        assert!(
            covered.starts_with("a or b") && !covered.contains("99"),
            "the trailing literal must contribute nothing, got {covered:?}"
        );
        let Expr::Infix(inner, _, _) = &e else {
            panic!("expected a left-associative chain, got {e:?}");
        };
        // The collision itself: the chain and its own left spine are
        // indistinguishable by this key.
        assert_eq!(expr_span(inner), Some(whole));
    }

    #[test]
    fn a_call_spans_its_callee_and_arguments() {
        let src = "flow main() {\n  {f(a, b)}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        let span = expr_span(&e).expect("call has a callee range");
        assert_eq!(text(src, span), "f(a, b");
    }

    #[test]
    fn an_all_literal_subtree_has_no_span() {
        let src = "flow main() {\n  {5 or 9}\n  -> END\n}\n";
        let e = first_logic_expr(src);
        assert_eq!(expr_span(&e), None);
    }
}
