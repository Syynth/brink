//! The `.ink` printer — the model's dialect switch.
//!
//! Layout is deliberately plain: `VAR` declarations first, then the entry
//! divert; knot roots and stitches print in the model's linear order, choice
//! markers repeat with nesting depth (`*`, `* *`, …), gathers mirror them
//! (`-`, `- -`, …), and choice bodies indent four spaces per level.
//! Indentation is cosmetic in ink; the markers are what carry structure, and
//! they are the part a reader of a shrunk counterexample needs to be able to
//! trust. Every binary expression prints fully parenthesized so no ink
//! precedence rule is ever relied on.

use std::fmt::Write as _;

use crate::model::{AssignOp, BinOp, Exit, Expr, Item, Literal, Part, Story, Tail, Weave};

/// Print a story as `.ink` source. Never fails; an invalid story prints
/// something, but only a [`crate::model::validate`]d one is guaranteed to
/// compile.
pub fn print_ink(story: &Story) -> String {
    let mut out = String::new();
    for v in &story.vars {
        let _ = writeln!(out, "VAR {} = {}", v.name, literal(&v.init));
    }
    if !story.vars.is_empty() {
        out.push('\n');
    }
    if let Some(first) = story.knots.first() {
        let _ = writeln!(out, "-> {}", first.name);
        out.push('\n');
    }
    for k in &story.knots {
        let _ = writeln!(out, "=== {} ===", k.name);
        print_weave(story, &k.root, 0, &mut out);
        for s in &k.stitches {
            out.push('\n');
            let _ = writeln!(out, "= {}", s.name);
            print_weave(story, &s.body, 0, &mut out);
        }
        out.push('\n');
    }
    out
}

/// Print an expression. Every binary node parenthesizes itself, so no ink
/// precedence rule is relied on — but a bare name or literal is NEVER
/// wrapped in parentheses: `(t0)` is ink's list-literal syntax ("the list
/// containing item `t0`"), and both inklecate and brink reject it as an
/// unknown list item (the generator found this on its first expressions
/// run). A unary operator therefore prefixes its operand directly; a nested
/// negation keeps its parentheses so `-(-x)` never reads as `--x`.
pub fn expr(e: &Expr) -> String {
    match e {
        Expr::Lit(l) => literal(l),
        Expr::Var(name) => name.clone(),
        Expr::Neg(inner) => match inner.as_ref() {
            Expr::Neg(_) => format!("-({})", expr(inner)),
            _ => format!("-{}", expr(inner)),
        },
        Expr::Not(inner) => match inner.as_ref() {
            // `not not (a op b)` is rejected by inklecate (it reads `-> not`
            // as a call target); one level of parentheses keeps the nesting
            // unambiguous in both compilers, mirroring the `Neg` arm.
            Expr::Not(_) => format!("not ({})", expr(inner)),
            _ => format!("not {}", expr(inner)),
        },
        Expr::Bin(l, op, r) => format!("({} {} {})", expr(l), binop(*op), expr(r)),
    }
}

fn literal(l: &Literal) -> String {
    match l {
        Literal::Int(n) => n.to_string(),
        Literal::Bool(b) => b.to_string(),
        Literal::Str(s) => format!("\"{s}\""),
    }
}

fn binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Mod => "mod",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
    }
}

fn indent(depth: usize) -> String {
    "    ".repeat(depth)
}

fn markers(marker: char, depth: usize) -> String {
    let mut m = String::new();
    for i in 0..=depth {
        if i > 0 {
            m.push(' ');
        }
        m.push(marker);
    }
    m
}

fn exit_text(story: &Story, e: Exit) -> String {
    match e {
        Exit::Divert(d) => format!("-> {}", story.path(d).unwrap_or_default()),
        Exit::End => "-> END".to_owned(),
        Exit::Done => "-> DONE".to_owned(),
    }
}

fn line_text(parts: &[Part]) -> String {
    let mut s = String::new();
    for p in parts {
        match p {
            Part::Text(t) => s.push_str(t),
            Part::Interp(e) => {
                let _ = write!(s, "{{{}}}", expr(e));
            }
            Part::Cond {
                cond,
                then,
                otherwise,
            } => match otherwise {
                Some(o) => {
                    let _ = write!(s, "{{{}:{then}|{o}}}", expr(cond));
                }
                None => {
                    let _ = write!(s, "{{{}:{then}}}", expr(cond));
                }
            },
        }
    }
    s
}

fn print_items(items: &[Item], depth: usize, out: &mut String) {
    let ind = indent(depth);
    for item in items {
        match item {
            Item::Line { parts, glue } => {
                let _ = writeln!(
                    out,
                    "{ind}{}{}",
                    line_text(parts),
                    if *glue { " <>" } else { "" }
                );
            }
            Item::Assign { target, op, value } => {
                let op = match op {
                    AssignOp::Set => "=",
                    AssignOp::Add => "+=",
                    AssignOp::Sub => "-=",
                };
                let _ = writeln!(out, "{ind}~ {target} {op} {}", expr(value));
            }
            Item::Temp { name, init } => {
                let _ = writeln!(out, "{ind}~ temp {name} = {}", expr(init));
            }
            Item::Cond {
                cond,
                then,
                otherwise,
            } => {
                let _ = writeln!(out, "{ind}{{ {}:", expr(cond));
                print_items(then, depth + 1, out);
                if let Some(o) = otherwise {
                    let _ = writeln!(out, "{ind}- else:");
                    print_items(o, depth + 1, out);
                }
                let _ = writeln!(out, "{ind}}}");
            }
        }
    }
}

fn print_weave(story: &Story, w: &Weave, depth: usize, out: &mut String) {
    let ind = indent(depth);
    print_items(&w.items, depth, out);
    match &w.tail {
        Tail::Exit(e) => {
            let _ = writeln!(out, "{ind}{}", exit_text(story, *e));
        }
        Tail::FallThrough => {}
        Tail::Choices {
            choices,
            fallback,
            gather,
        } => {
            for c in choices {
                let m = markers(if c.sticky { '+' } else { '*' }, depth);
                match &c.condition {
                    Some(cond) => {
                        let _ = writeln!(out, "{ind}{m} {{{}}} [{}]", expr(cond), c.label);
                    }
                    None => {
                        let _ = writeln!(out, "{ind}{m} [{}]", c.label);
                    }
                }
                print_weave(story, &c.body, depth + 1, out);
            }
            // Fallbacks print sticky (`+ ->`): a `* ->` fallback is itself a
            // once-only choice and is consumed the first time it fires, so it
            // would only protect ONE exhaustion of the set (model rule 3).
            if let Some(fb) = fallback {
                let _ = writeln!(
                    out,
                    "{ind}{} {}",
                    markers('+', depth),
                    exit_text(story, *fb)
                );
            }
            if let Some(g) = gather {
                let _ = writeln!(out, "{ind}{}", markers('-', depth));
                print_weave(story, g, depth, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Choice, Divert, Knot, Stitch, VarDecl};

    fn text(s: &str) -> Item {
        Item::Line {
            parts: vec![Part::Text(s.to_owned())],
            glue: false,
        }
    }

    #[test]
    fn prints_markers_by_depth_and_paths_by_name() {
        let leaf = |e: Exit| Weave {
            items: vec![text("leaf")],
            tail: Tail::Exit(e),
        };
        let story = Story {
            vars: vec![],
            knots: vec![
                Knot {
                    name: "start".into(),
                    root: Weave {
                        items: vec![Item::Line {
                            parts: vec![Part::Text("hello".into())],
                            glue: true,
                        }],
                        tail: Tail::Choices {
                            choices: vec![Choice {
                                sticky: false,
                                condition: None,
                                label: "go".into(),
                                body: Weave {
                                    items: vec![],
                                    tail: Tail::Choices {
                                        choices: vec![Choice {
                                            sticky: true,
                                            condition: None,
                                            label: "deeper".into(),
                                            body: Weave {
                                                items: vec![],
                                                tail: Tail::FallThrough,
                                            },
                                        }],
                                        fallback: None,
                                        gather: Some(Box::new(Weave {
                                            items: vec![],
                                            tail: Tail::FallThrough,
                                        })),
                                    },
                                },
                            }],
                            fallback: Some(Exit::Divert(Divert {
                                knot: 1,
                                stitch: Some(0),
                            })),
                            gather: Some(Box::new(leaf(Exit::End))),
                        },
                    },
                    stitches: vec![],
                },
                Knot {
                    name: "next".into(),
                    root: leaf(Exit::Done),
                    stitches: vec![Stitch {
                        name: "inner".into(),
                        body: leaf(Exit::End),
                    }],
                },
            ],
        };
        let printed = print_ink(&story);
        let expected = "\
-> start

=== start ===
hello <>
* [go]
    + + [deeper]
    - -
+ -> next.inner
-
leaf
-> END

=== next ===
leaf
-> DONE

= inner
leaf
-> END

";
        assert_eq!(printed, expected);
    }

    #[test]
    fn nested_unary_operators_keep_one_level_of_parentheses() {
        let x = || Expr::Var("x".into());
        let b = || Expr::Var("b".into());
        assert_eq!(
            expr(&Expr::Neg(Box::new(Expr::Neg(Box::new(x()))))),
            "-(-x)"
        );
        assert_eq!(
            expr(&Expr::Not(Box::new(Expr::Not(Box::new(b()))))),
            "not (not b)"
        );
        assert_eq!(
            expr(&Expr::Not(Box::new(Expr::Not(Box::new(Expr::Bin(
                Box::new(b()),
                BinOp::And,
                Box::new(b())
            )))))),
            "not (not (b and b))"
        );
    }

    #[test]
    fn prints_vars_expressions_and_conditionals() {
        let n = || Expr::Var("n".into());
        let story = Story {
            vars: vec![
                VarDecl {
                    name: "n".into(),
                    init: Literal::Int(2),
                },
                VarDecl {
                    name: "s".into(),
                    init: Literal::Str("hi".into()),
                },
            ],
            knots: vec![Knot {
                name: "k".into(),
                root: Weave {
                    items: vec![
                        Item::Temp {
                            name: "t0".into(),
                            init: Expr::Bin(
                                Box::new(n()),
                                BinOp::Mod,
                                Box::new(Expr::Lit(Literal::Int(3))),
                            ),
                        },
                        Item::Assign {
                            target: "n".into(),
                            op: AssignOp::Add,
                            value: Expr::Neg(Box::new(Expr::Var("t0".into()))),
                        },
                        Item::Line {
                            parts: vec![
                                Part::Text("n is ".into()),
                                Part::Interp(n()),
                                Part::Cond {
                                    cond: Expr::Bin(
                                        Box::new(n()),
                                        BinOp::Gt,
                                        Box::new(Expr::Lit(Literal::Int(0))),
                                    ),
                                    then: " big".into(),
                                    otherwise: Some(" small".into()),
                                },
                            ],
                            glue: false,
                        },
                        Item::Cond {
                            cond: Expr::Not(Box::new(Expr::Bin(
                                Box::new(Expr::Var("s".into())),
                                BinOp::Eq,
                                Box::new(Expr::Lit(Literal::Str("hi".into()))),
                            ))),
                            then: vec![text("not hi")],
                            otherwise: Some(vec![text("hi")]),
                        },
                    ],
                    tail: Tail::Choices {
                        choices: vec![Choice {
                            sticky: true,
                            condition: Some(Expr::Bin(
                                Box::new(n()),
                                BinOp::Ge,
                                Box::new(Expr::Lit(Literal::Int(1))),
                            )),
                            label: "go".into(),
                            body: Weave {
                                items: vec![],
                                tail: Tail::Exit(Exit::End),
                            },
                        }],
                        fallback: Some(Exit::Done),
                        gather: None,
                    },
                },
                stitches: vec![],
            }],
        };
        let expected = "\
VAR n = 2
VAR s = \"hi\"

-> k

=== k ===
~ temp t0 = (n mod 3)
~ n += -t0
n is {n}{(n > 0): big| small}
{ not (s == \"hi\"):
    not hi
- else:
    hi
}
+ {(n >= 1)} [go]
    -> END
+ -> DONE

";
        assert_eq!(print_ink(&story), expected);
    }
}
