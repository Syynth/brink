//! The typed story model — structure tier.
//!
//! # Flow order and termination
//!
//! Every knot root and every stitch is a **flow**. Flows have a fixed linear
//! order — knot 0's root, then knot 0's stitches in order, then knot 1's
//! root, and so on — and [`Story::flow_index`] maps a [`Divert`] to its
//! position. The rules that make every story terminate:
//!
//! 1. A divert from flow `f` is **forward** if its target's index is
//!    greater than `f`. Forward diverts are legal anywhere.
//! 2. A divert whose target index is `<= f` is a **back-edge**. A back-edge
//!    is legal only inside the body of a once-only (`*`) choice — the choice
//!    is consumed the first time it is taken, so each back-edge fires at
//!    most once per playthrough.
//! 3. A choice set never runs out: it contains a sticky (`+`) choice, or it
//!    carries a fallback exit — printed as the **sticky** form `+ -> target`.
//!    A `* -> target` fallback is itself a once-only choice, consumed the
//!    first time it fires, so it protects exactly one exhaustion of the set
//!    (found by the generator: a once-only fallback ran out on the third
//!    visit, in brink and inklecate alike). Without this rule, exhausting
//!    every once-only choice would leave the runtime with nowhere to go.
//! 4. A choice body may fall through (no exit of its own) only when its
//!    choice set has a gather to fall into.
//!
//! [`validate`] checks all four plus name uniqueness and reference
//! resolution; the strategies in [`crate::strategy`] construct stories that
//! satisfy them, and the crate's smoke property asserts every generated
//! story validates — so a validation failure is a generator bug, never a
//! filtered case.

use std::fmt;

/// A complete single-compilation story: the entry is knot 0's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Story {
    /// Knots in document order. Never empty.
    pub knots: Vec<Knot>,
}

/// A knot: a root weave plus zero or more stitches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Knot {
    /// Unique across the story.
    pub name: String,
    pub root: Weave,
    pub stitches: Vec<Stitch>,
}

/// A stitch inside a knot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stitch {
    /// Unique within its knot.
    pub name: String,
    pub body: Weave,
}

/// A run of content lines followed by exactly one tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weave {
    pub lines: Vec<Line>,
    pub tail: Tail,
}

/// One content line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Printable text with no ink-significant characters (see
    /// [`crate::strategy`] for the alphabet).
    pub text: String,
    /// Trailing `<>` glue.
    pub glue: bool,
}

/// How a weave ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tail {
    /// Leave the weave.
    Exit(Exit),
    /// No exit of its own: control falls into the enclosing choice set's
    /// gather. Legal only for a choice body whose set has a gather.
    FallThrough,
    /// A choice point.
    Choices {
        /// Never empty.
        choices: Vec<Choice>,
        /// `+ -> target`: taken whenever no other choice is available. Always
        /// the sticky form — see rule 3 in the module doc.
        fallback: Option<Exit>,
        /// The gather the choice bodies fall into, and the weave that
        /// continues from it.
        gather: Option<Box<Weave>>,
    },
}

/// A weave exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Divert(Divert),
    End,
    Done,
}

/// A resolved divert target: a knot root or one of its stitches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divert {
    pub knot: usize,
    pub stitch: Option<usize>,
}

/// One choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// `+` (re-offerable) when true, `*` (once-only) when false.
    pub sticky: bool,
    /// Bracketed label: shown in the choice, not echoed as content.
    pub label: String,
    pub body: Weave,
}

/// A rule violation found by [`validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(pub String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

impl Story {
    /// Number of flows (knot roots + stitches) in linear order.
    pub fn flow_count(&self) -> usize {
        self.knots.iter().map(|k| 1 + k.stitches.len()).sum()
    }

    /// The linear-order index of a divert target, or `None` if the target
    /// does not exist.
    pub fn flow_index(&self, d: Divert) -> Option<usize> {
        let mut ix = 0;
        for (ki, k) in self.knots.iter().enumerate() {
            if ki == d.knot {
                return match d.stitch {
                    None => Some(ix),
                    Some(si) if si < k.stitches.len() => Some(ix + 1 + si),
                    Some(_) => None,
                };
            }
            ix += 1 + k.stitches.len();
        }
        None
    }

    /// The divert that names the flow at linear-order `index`.
    pub fn flow_at(&self, index: usize) -> Option<Divert> {
        let mut ix = 0;
        for (ki, k) in self.knots.iter().enumerate() {
            if index == ix {
                return Some(Divert {
                    knot: ki,
                    stitch: None,
                });
            }
            for si in 0..k.stitches.len() {
                if index == ix + 1 + si {
                    return Some(Divert {
                        knot: ki,
                        stitch: Some(si),
                    });
                }
            }
            ix += 1 + k.stitches.len();
        }
        None
    }

    /// The `knot` / `knot.stitch` path a divert prints as.
    pub fn path(&self, d: Divert) -> Option<String> {
        let k = self.knots.get(d.knot)?;
        Some(match d.stitch {
            None => k.name.clone(),
            Some(si) => format!("{}.{}", k.name, k.stitches.get(si)?.name),
        })
    }
}

/// Check every rule in the module doc. `Ok(())` means the story is a
/// well-formed, terminating structure-tier program.
pub fn validate(story: &Story) -> Result<(), Invalid> {
    if story.knots.is_empty() {
        return Err(Invalid("story has no knots".into()));
    }
    let mut seen = std::collections::BTreeSet::new();
    for k in &story.knots {
        if !seen.insert(k.name.as_str()) {
            return Err(Invalid(format!("duplicate knot name `{}`", k.name)));
        }
        let mut seen_s = std::collections::BTreeSet::new();
        for s in &k.stitches {
            if !seen_s.insert(s.name.as_str()) {
                return Err(Invalid(format!(
                    "duplicate stitch name `{}` in knot `{}`",
                    s.name, k.name
                )));
            }
        }
    }
    let mut flow = 0;
    for k in &story.knots {
        validate_weave(story, &k.root, flow, false, false)?;
        flow += 1;
        for s in &k.stitches {
            validate_weave(story, &s.body, flow, false, false)?;
            flow += 1;
        }
    }
    Ok(())
}

fn validate_exit(story: &Story, e: Exit, flow: usize, may_go_back: bool) -> Result<(), Invalid> {
    if let Exit::Divert(d) = e {
        let Some(target) = story.flow_index(d) else {
            return Err(Invalid(format!("unresolved divert {d:?} from flow {flow}")));
        };
        if target <= flow && !may_go_back {
            return Err(Invalid(format!(
                "back-edge to flow {target} from flow {flow} outside a once-only choice body"
            )));
        }
    }
    Ok(())
}

fn validate_weave(
    story: &Story,
    w: &Weave,
    flow: usize,
    may_go_back: bool,
    may_fall_through: bool,
) -> Result<(), Invalid> {
    for l in &w.lines {
        if l.text.is_empty() {
            return Err(Invalid(format!("empty content line in flow {flow}")));
        }
    }
    match &w.tail {
        Tail::Exit(e) => validate_exit(story, *e, flow, may_go_back),
        Tail::FallThrough => {
            if may_fall_through {
                Ok(())
            } else {
                Err(Invalid(format!(
                    "fall-through with no gather to fall into (flow {flow})"
                )))
            }
        }
        Tail::Choices {
            choices,
            fallback,
            gather,
        } => {
            if choices.is_empty() {
                return Err(Invalid(format!("empty choice set in flow {flow}")));
            }
            if !choices.iter().any(|c| c.sticky) && fallback.is_none() {
                return Err(Invalid(format!(
                    "choice set in flow {flow} can run out: no sticky choice and no fallback"
                )));
            }
            if let Some(fb) = fallback {
                validate_exit(story, *fb, flow, false)?;
            }
            for c in choices {
                if c.label.is_empty() {
                    return Err(Invalid(format!("empty choice label in flow {flow}")));
                }
                // A back-edge stays legal deeper inside a once-only body
                // (the outer once-only choice already bounds it).
                let back = may_go_back || !c.sticky;
                validate_weave(story, &c.body, flow, back, gather.is_some())?;
            }
            match gather {
                Some(g) => validate_weave(story, g, flow, may_go_back, may_fall_through),
                None => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> Line {
        Line {
            text: s.to_owned(),
            glue: false,
        }
    }

    fn exit_weave(e: Exit) -> Weave {
        Weave {
            lines: vec![line("hello")],
            tail: Tail::Exit(e),
        }
    }

    fn two_knots() -> Story {
        Story {
            knots: vec![
                Knot {
                    name: "a".into(),
                    root: exit_weave(Exit::Divert(Divert {
                        knot: 1,
                        stitch: None,
                    })),
                    stitches: vec![],
                },
                Knot {
                    name: "b".into(),
                    root: exit_weave(Exit::End),
                    stitches: vec![Stitch {
                        name: "s".into(),
                        body: exit_weave(Exit::Done),
                    }],
                },
            ],
        }
    }

    #[test]
    fn flow_order_and_paths() {
        let s = two_knots();
        assert_eq!(s.flow_count(), 3);
        let bs = Divert {
            knot: 1,
            stitch: Some(0),
        };
        assert_eq!(s.flow_index(bs), Some(2));
        assert_eq!(s.flow_at(2), Some(bs));
        assert_eq!(s.path(bs).as_deref(), Some("b.s"));
        assert_eq!(
            s.flow_index(Divert {
                knot: 1,
                stitch: Some(1)
            }),
            None
        );
    }

    #[test]
    fn forward_story_validates() {
        assert_eq!(validate(&two_knots()), Ok(()));
    }

    #[test]
    fn back_edge_outside_once_only_is_rejected() {
        let mut s = two_knots();
        s.knots[1].root = exit_weave(Exit::Divert(Divert {
            knot: 0,
            stitch: None,
        }));
        let err = validate(&s).expect_err("back-edge must be rejected");
        assert!(err.0.contains("back-edge"), "{err}");
    }

    #[test]
    fn back_edge_inside_once_only_is_accepted() {
        let mut s = two_knots();
        s.knots[1].root = Weave {
            lines: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: false,
                    label: "again".into(),
                    body: exit_weave(Exit::Divert(Divert {
                        knot: 0,
                        stitch: None,
                    })),
                }],
                fallback: Some(Exit::End),
                gather: None,
            },
        };
        assert_eq!(validate(&s), Ok(()));
    }

    #[test]
    fn choice_set_that_can_run_out_is_rejected() {
        let mut s = two_knots();
        s.knots[1].root = Weave {
            lines: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: false,
                    label: "once".into(),
                    body: exit_weave(Exit::End),
                }],
                fallback: None,
                gather: None,
            },
        };
        let err = validate(&s).expect_err("must be rejected");
        assert!(err.0.contains("run out"), "{err}");
    }

    #[test]
    fn fall_through_needs_a_gather() {
        let mut s = two_knots();
        s.knots[1].root = Weave {
            lines: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: true,
                    label: "on".into(),
                    body: Weave {
                        lines: vec![line("x")],
                        tail: Tail::FallThrough,
                    },
                }],
                fallback: None,
                gather: None,
            },
        };
        let err = validate(&s).expect_err("must be rejected");
        assert!(err.0.contains("fall-through"), "{err}");
        if let Tail::Choices { gather, .. } = &mut s.knots[1].root.tail {
            *gather = Some(Box::new(exit_weave(Exit::End)));
        }
        assert_eq!(validate(&s), Ok(()));
    }
}
