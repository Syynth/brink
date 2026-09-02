//! proptest strategies over the model.
//!
//! # Skeleton, then decode
//!
//! The strategies never look at a generated value while generating: they
//! produce a **raw skeleton** — a plain tree of independent values in which a
//! divert target is just a small integer and a tail is a plain enum — and a
//! deterministic [`decode`] step turns that skeleton into a valid [`Story`],
//! resolving every raw target into the range the model's rules allow at its
//! site (forward flows anywhere; back-edges only inside once-only choice
//! bodies; fall-through only into a gather; a set that could run out gets a
//! fallback).
//!
//! That shape is what makes **shrinking** work. proptest shrinks each
//! component of an independent tree on its own — fewer knots, fewer lines,
//! a smaller target integer, a simpler tail — and the decoder keeps the
//! result valid by construction, so a counterexample shrinks all the way
//! down to a story a human can read. The alternative, `prop_flat_map`-ing
//! weaves against a generated layout, regenerates the inner values whenever
//! the outer ones shrink and stalls with a large story (the first version of
//! this module did exactly that).

use proptest::prelude::*;

use crate::model::{Choice, Divert, Exit, Knot, Line, Stitch, Story, Tail, Weave};

/// Biasing knobs — **data**, so a property names the profile it wants
/// (`docs/program-generator-spec.md` §4). The structure tier has size
/// bounds only; bait flags arrive with the later tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// Knots per story (at least 1).
    pub max_knots: usize,
    /// Stitches per knot.
    pub max_stitches: usize,
    /// Content lines per weave.
    pub max_lines: usize,
    /// Choices per choice set (at least 1).
    pub max_choices: usize,
    /// How deep choice sets may nest inside choice bodies (0 = none).
    pub max_choice_depth: usize,
}

impl Profile {
    /// The default profile: small enough that the harness's DFS explores
    /// every path in well under a second, large enough to nest.
    pub const DEFAULT: Self = Self {
        max_knots: 4,
        max_stitches: 2,
        max_lines: 3,
        max_choices: 3,
        max_choice_depth: 2,
    };
}

impl Default for Profile {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ─── Raw skeleton ────────────────────────────────────────────────────

/// A weave exit before resolution. `Forward`/`Backward` carry a raw index
/// the decoder maps into the legal range at the exit's site.
#[derive(Debug, Clone, Copy)]
pub enum RawExit {
    Forward(u8),
    Backward(u8),
    End,
    Done,
}

#[derive(Debug, Clone)]
pub enum RawTail {
    Exit(RawExit),
    FallThrough,
    Choices {
        choices: Vec<RawChoice>,
        fallback: Option<RawExit>,
        gather: Option<Box<RawWeave>>,
    },
}

#[derive(Debug, Clone)]
pub struct RawChoice {
    pub sticky: bool,
    pub label: String,
    pub body: RawWeave,
}

#[derive(Debug, Clone)]
pub struct RawWeave {
    pub lines: Vec<Line>,
    pub tail: RawTail,
}

#[derive(Debug, Clone)]
pub struct RawKnot {
    pub name: String,
    pub root: RawWeave,
    pub stitches: Vec<(String, RawWeave)>,
}

/// The unresolved story: independent values only.
#[derive(Debug, Clone)]
pub struct RawStory {
    pub knots: Vec<RawKnot>,
}

// ─── Leaf strategies ─────────────────────────────────────────────────

/// Characters that are never ink-significant in content position: no
/// `{}`, `#`, `|`, `\`, `[]`, `~`, `=`, `*`, `+`, `-`, `<`, `>`, `/`, and a
/// leading lowercase letter so a line can never read as a keyword, a
/// choice/gather marker, or a `TODO:`.
fn arb_text() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9 ,.!?;:]{0,29}".prop_map(|s| s.trim_end().to_owned())
}

/// Names are made unique by suffixing the flow's own indices — the base is
/// only for readability of a shrunk story.
fn arb_name_base() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,5}"
}

fn arb_line() -> impl Strategy<Value = Line> {
    (arb_text(), prop::bool::weighted(0.15)).prop_map(|(text, glue)| Line { text, glue })
}

fn arb_raw_exit() -> impl Strategy<Value = RawExit> {
    prop_oneof![
        4 => any::<u8>().prop_map(RawExit::Forward),
        2 => any::<u8>().prop_map(RawExit::Backward),
        1 => Just(RawExit::End),
        1 => Just(RawExit::Done),
    ]
}

fn arb_raw_weave(p: Profile, depth_left: usize) -> BoxedStrategy<RawWeave> {
    let lines = prop::collection::vec(arb_line(), 0..=p.max_lines);
    let tail = if depth_left == 0 {
        prop_oneof![
            3 => arb_raw_exit().prop_map(RawTail::Exit),
            2 => Just(RawTail::FallThrough),
        ]
        .boxed()
    } else {
        let choice = (
            arb_text(),
            prop::bool::weighted(0.5),
            arb_raw_weave(p, depth_left - 1),
        )
            .prop_map(|(label, sticky, body)| RawChoice {
                sticky,
                label,
                body,
            });
        let choices = prop::collection::vec(choice, 1..=p.max_choices.max(1));
        let fallback = prop::option::weighted(0.4, arb_raw_exit());
        let gather =
            prop::option::weighted(0.5, arb_raw_weave(p, depth_left - 1).prop_map(Box::new));
        prop_oneof![
            3 => arb_raw_exit().prop_map(RawTail::Exit),
            2 => Just(RawTail::FallThrough),
            3 => (choices, fallback, gather).prop_map(|(choices, fallback, gather)| RawTail::Choices {
                choices,
                fallback,
                gather,
            }),
        ]
        .boxed()
    };
    (lines, tail)
        .prop_map(|(lines, tail)| RawWeave { lines, tail })
        .boxed()
}

fn arb_raw_knot(p: Profile) -> impl Strategy<Value = RawKnot> {
    let stitch = (arb_name_base(), arb_raw_weave(p, p.max_choice_depth));
    (
        arb_name_base(),
        arb_raw_weave(p, p.max_choice_depth),
        prop::collection::vec(stitch, 0..=p.max_stitches),
    )
        .prop_map(|(name, root, stitches)| RawKnot {
            name,
            root,
            stitches,
        })
}

/// A raw skeleton under `profile`.
pub fn arb_raw_story(profile: Profile) -> impl Strategy<Value = RawStory> {
    prop::collection::vec(arb_raw_knot(profile), 1..=profile.max_knots.max(1))
        .prop_map(|knots| RawStory { knots })
}

// ─── Decode ──────────────────────────────────────────────────────────

/// Where a weave sits, for resolving its raw exits.
#[derive(Clone, Copy)]
struct Site {
    flow: usize,
    flow_count: usize,
    may_go_back: bool,
    may_fall_through: bool,
}

fn resolve_exit(raw: RawExit, site: Site, table: &[Divert]) -> Exit {
    let forward_count = site.flow_count.saturating_sub(site.flow + 1);
    match raw {
        RawExit::End => Exit::End,
        RawExit::Done => Exit::Done,
        RawExit::Forward(n) => {
            if forward_count == 0 {
                Exit::End
            } else {
                Exit::Divert(table[site.flow + 1 + usize::from(n) % forward_count])
            }
        }
        RawExit::Backward(n) => {
            if site.may_go_back {
                Exit::Divert(table[usize::from(n) % (site.flow + 1)])
            } else if forward_count == 0 {
                Exit::End
            } else {
                // Not allowed to go back here: read the raw index forward.
                Exit::Divert(table[site.flow + 1 + usize::from(n) % forward_count])
            }
        }
    }
}

fn decode_weave(raw: &RawWeave, site: Site, table: &[Divert]) -> Weave {
    let lines = raw.lines.clone();
    let tail = match &raw.tail {
        RawTail::Exit(e) => Tail::Exit(resolve_exit(*e, site, table)),
        RawTail::FallThrough => {
            if site.may_fall_through {
                Tail::FallThrough
            } else {
                // No gather to fall into: the weave ends explicitly instead.
                Tail::Exit(resolve_exit(RawExit::Forward(0), site, table))
            }
        }
        RawTail::Choices {
            choices,
            fallback,
            gather,
        } => {
            let has_gather = gather.is_some();
            let decoded: Vec<Choice> = choices
                .iter()
                .map(|c| {
                    let body_site = Site {
                        may_go_back: site.may_go_back || !c.sticky,
                        may_fall_through: has_gather,
                        ..site
                    };
                    Choice {
                        sticky: c.sticky,
                        label: c.label.clone(),
                        body: decode_weave(&c.body, body_site, table),
                    }
                })
                .collect();
            // A fallback fires from a normal flow position: never a back-edge.
            let fallback_site = Site {
                may_go_back: false,
                ..site
            };
            let mut fallback = fallback.map(|f| resolve_exit(f, fallback_site, table));
            // Rule 3: a set with no sticky choice must carry a fallback.
            if !decoded.iter().any(|c| c.sticky) && fallback.is_none() {
                fallback = Some(Exit::End);
            }
            // The gather continues at the enclosing weave's own site.
            let gather = gather
                .as_ref()
                .map(|g| Box::new(decode_weave(g, site, table)));
            Tail::Choices {
                choices: decoded,
                fallback,
                gather,
            }
        }
    };
    Weave { lines, tail }
}

/// Turn a raw skeleton into a valid [`Story`]: names made unique, every
/// exit resolved into the legal range for its site, every rule of
/// `crate::model` satisfied by construction.
pub fn decode(raw: &RawStory) -> Story {
    // Pass 1: the layout — flow table in linear order.
    let mut table = Vec::new();
    for (ki, k) in raw.knots.iter().enumerate() {
        table.push(Divert {
            knot: ki,
            stitch: None,
        });
        for si in 0..k.stitches.len() {
            table.push(Divert {
                knot: ki,
                stitch: Some(si),
            });
        }
    }
    let flow_count = table.len();
    // Pass 2: decode every weave against its own flow index.
    let mut flow = 0;
    let knots = raw
        .knots
        .iter()
        .enumerate()
        .map(|(ki, k)| {
            let root_site = Site {
                flow,
                flow_count,
                may_go_back: false,
                may_fall_through: false,
            };
            flow += 1;
            let root = decode_weave(&k.root, root_site, &table);
            let stitches = k
                .stitches
                .iter()
                .enumerate()
                .map(|(si, (base, body))| {
                    let site = Site { flow, ..root_site };
                    flow += 1;
                    Stitch {
                        name: format!("{base}_s{si}"),
                        body: decode_weave(body, site, &table),
                    }
                })
                .collect();
            Knot {
                name: format!("{}_k{ki}", k.name),
                root,
                stitches,
            }
        })
        .collect();
    Story { knots }
}

/// A structure-tier story under `profile`: validates by construction.
pub fn arb_story_with(profile: Profile) -> impl Strategy<Value = Story> {
    arb_raw_story(profile).prop_map(|raw| decode(&raw))
}

/// A structure-tier story under [`Profile::DEFAULT`].
pub fn arb_story() -> impl Strategy<Value = Story> {
    arb_story_with(Profile::DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::validate;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Every generated story satisfies the model's rules — a failure
        /// here is a generator bug, never a filtered case.
        #[test]
        fn generated_stories_validate(story in arb_story()) {
            prop_assert_eq!(validate(&story), Ok(()));
        }

        /// The profile's size bounds hold.
        #[test]
        fn generated_stories_respect_profile(story in arb_story()) {
            let p = Profile::DEFAULT;
            prop_assert!(!story.knots.is_empty() && story.knots.len() <= p.max_knots);
            for k in &story.knots {
                prop_assert!(k.stitches.len() <= p.max_stitches);
            }
        }
    }

    /// Shrinking quality: a property that rejects any nested choice set
    /// must shrink to a story with exactly the offending shape and
    /// nothing else — one knot, one choice, one nested set.
    #[test]
    fn counterexamples_shrink_to_the_offending_shape() {
        use proptest::strategy::ValueTree as _;
        use proptest::test_runner::{Config, TestRunner};

        fn has_nested_set(w: &Weave, depth: usize) -> bool {
            match &w.tail {
                Tail::Choices {
                    choices, gather, ..
                } => {
                    depth >= 1
                        || choices.iter().any(|c| has_nested_set(&c.body, depth + 1))
                        || gather.as_ref().is_some_and(|g| has_nested_set(g, depth))
                }
                _ => false,
            }
        }
        fn story_has_nested_set(s: &Story) -> bool {
            s.knots.iter().any(|k| {
                has_nested_set(&k.root, 0)
                    || k.stitches.iter().any(|st| has_nested_set(&st.body, 0))
            })
        }

        let mut runner = TestRunner::new(Config::default());
        let mut tree = None;
        // Find a failing value.
        for _ in 0..500 {
            let t = arb_story().new_tree(&mut runner).expect("new tree");
            if story_has_nested_set(&t.current()) {
                tree = Some(t);
                break;
            }
        }
        let mut tree = tree.expect("a nested choice set is generated within 500 tries");
        // Shrink while the failure persists (proptest's own loop, by hand).
        let mut budget = 10_000;
        loop {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if story_has_nested_set(&tree.current()) {
                if !tree.simplify() {
                    break;
                }
            } else if !tree.complicate() {
                break;
            }
        }
        let minimal = tree.current();
        assert!(story_has_nested_set(&minimal));
        let printed = crate::print::print_ink(&minimal);
        let line_count = printed.lines().filter(|l| !l.trim().is_empty()).count();
        // Measured: the shrinker lands on one knot, 9 printed lines (a
        // choice, its nested set with a fallback, the exits). Before the
        // skeleton-then-decode design, the same property stalled at ~220
        // lines. Shrinking can delete but not relocate, so a stitch that
        // hosts the offending set legitimately survives.
        assert!(
            minimal.knots.len() == 1 && line_count <= 12,
            "expected a minimal one-knot story, got {} knots / {line_count} lines:\n{printed}",
            minimal.knots.len()
        );
    }
}
