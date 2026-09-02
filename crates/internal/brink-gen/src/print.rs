//! The `.ink` printer — the model's dialect switch.
//!
//! Layout is deliberately plain: knot roots and stitches print in the
//! model's linear order, choice markers repeat with nesting depth (`*`,
//! `* *`, …), gathers mirror them (`-`, `- -`, …), and choice bodies indent
//! four spaces per level. Indentation is cosmetic in ink; the markers are
//! what carry structure, and they are the part a reader of a shrunk
//! counterexample needs to be able to trust.

use std::fmt::Write as _;

use crate::model::{Exit, Story, Tail, Weave};

/// Print a story as `.ink` source. Never fails; an invalid story prints
/// something, but only a [`crate::model::validate`]d one is guaranteed to
/// compile.
pub fn print_ink(story: &Story) -> String {
    let mut out = String::new();
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

fn print_weave(story: &Story, w: &Weave, depth: usize, out: &mut String) {
    let ind = indent(depth);
    for l in &w.lines {
        let _ = writeln!(out, "{ind}{}{}", l.text, if l.glue { " <>" } else { "" });
    }
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
                let _ = writeln!(out, "{ind}{m} [{}]", c.label);
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
    use crate::model::{Choice, Divert, Knot, Line, Stitch};

    #[test]
    fn prints_markers_by_depth_and_paths_by_name() {
        let leaf = |e: Exit| Weave {
            lines: vec![Line {
                text: "leaf".into(),
                glue: false,
            }],
            tail: Tail::Exit(e),
        };
        let story = Story {
            knots: vec![
                Knot {
                    name: "start".into(),
                    root: Weave {
                        lines: vec![Line {
                            text: "hello".into(),
                            glue: true,
                        }],
                        tail: Tail::Choices {
                            choices: vec![Choice {
                                sticky: false,
                                label: "go".into(),
                                body: Weave {
                                    lines: vec![],
                                    tail: Tail::Choices {
                                        choices: vec![Choice {
                                            sticky: true,
                                            label: "deeper".into(),
                                            body: Weave {
                                                lines: vec![],
                                                tail: Tail::FallThrough,
                                            },
                                        }],
                                        fallback: None,
                                        gather: Some(Box::new(Weave {
                                            lines: vec![],
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
}
