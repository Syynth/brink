//! proptest strategies over the model.
//!
//! The strategies build a story in two flat-mapped stages — first the flow
//! **layout** (how many knots, how many stitches each), then every weave
//! generated against that layout with its own flow index in hand — so every
//! divert is chosen from a set the model's rules already allow (forward
//! targets anywhere; back-edges only inside once-only choice bodies). The
//! result validates by construction; `tests/smoke.rs` asserts it anyway.
//!
//! Shrinking happens on the model: proptest shrinks the layout (fewer
//! knots/stitches), then each weave (fewer lines, fewer choices, simpler
//! tails), and the printer re-emits — the counterexample stays a story.

use proptest::prelude::*;
use proptest::strategy::Union;

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

/// The flow layout: stitch counts per knot. Its length is the knot count.
fn arb_layout(p: Profile) -> impl Strategy<Value = Vec<usize>> {
    prop::collection::vec(0..=p.max_stitches, 1..=p.max_knots.max(1))
}

/// Everything a weave strategy needs to know about where it sits.
#[derive(Clone, Copy)]
struct Site {
    /// This weave's own flow index.
    flow: usize,
    /// Total flows in the story.
    flow_count: usize,
    /// Inside a once-only choice body: back-edges are legal.
    may_go_back: bool,
    /// Inside a choice body whose set has a gather: fall-through is legal.
    may_fall_through: bool,
    /// Remaining nesting budget for choice sets.
    depth_left: usize,
}

/// A divert to any flow strictly after `site.flow`, if one exists.
fn arb_forward(site: Site) -> Option<BoxedStrategy<Divert>> {
    let lo = site.flow + 1;
    if lo >= site.flow_count {
        return None;
    }
    Some((lo..site.flow_count).prop_map(placeholder).boxed())
}

/// A divert to any flow at or before `site.flow`.
fn arb_backward(site: Site) -> BoxedStrategy<Divert> {
    (0..=site.flow).prop_map(placeholder).boxed()
}

/// The strategies work in flow indices: a placeholder [`Divert`] carries the
/// flow index in `knot` until [`resolve`] rewrites it against the finished
/// layout.
fn placeholder(flow_ix: usize) -> Divert {
    Divert {
        knot: flow_ix,
        stitch: None,
    }
}

fn arb_exit(site: Site) -> BoxedStrategy<Exit> {
    let terminal = prop_oneof![Just(Exit::End), Just(Exit::Done)].boxed();
    let mut arms: Vec<(u32, BoxedStrategy<Exit>)> = vec![(1, terminal)];
    if let Some(fwd) = arb_forward(site) {
        arms.push((4, fwd.prop_map(Exit::Divert).boxed()));
    }
    if site.may_go_back {
        arms.push((2, arb_backward(site).prop_map(Exit::Divert).boxed()));
    }
    Union::new_weighted(arms).boxed()
}

fn arb_weave(p: Profile, site: Site) -> BoxedStrategy<Weave> {
    let lines = prop::collection::vec(arb_line(), 0..=p.max_lines);
    let mut tails: Vec<(u32, BoxedStrategy<Tail>)> =
        vec![(3, arb_exit(site).prop_map(Tail::Exit).boxed())];
    if site.may_fall_through {
        tails.push((2, Just(Tail::FallThrough).boxed()));
    }
    if site.depth_left > 0 {
        tails.push((3, arb_choices(p, site).boxed()));
    }
    (lines, Union::new_weighted(tails))
        .prop_map(|(lines, tail)| Weave { lines, tail })
        .boxed()
}

fn arb_choices(p: Profile, site: Site) -> impl Strategy<Value = Tail> {
    let inner = Site {
        depth_left: site.depth_left - 1,
        ..site
    };
    // The gather is chosen first: whether it exists decides whether the
    // choice bodies may fall through.
    prop::option::weighted(0.5, Just(())).prop_flat_map(move |has_gather| {
        let body_site = Site {
            may_fall_through: has_gather.is_some(),
            ..inner
        };
        let choice =
            (arb_text(), prop::bool::weighted(0.5)).prop_flat_map(move |(label, sticky)| {
                let site_for_body = Site {
                    may_go_back: body_site.may_go_back || !sticky,
                    ..body_site
                };
                arb_weave(p, site_for_body).prop_map(move |body| Choice {
                    sticky,
                    label: label.clone(),
                    body,
                })
            });
        let choices = prop::collection::vec(choice, 1..=p.max_choices.max(1));
        // A fallback never goes back: it is taken from a normal flow position.
        let fallback = prop::option::weighted(
            0.4,
            arb_exit(Site {
                may_go_back: false,
                ..site
            }),
        );
        let gather = if has_gather.is_some() {
            // The gather continues at the enclosing weave's own site.
            arb_weave(p, inner).prop_map(|w| Some(Box::new(w))).boxed()
        } else {
            Just(None).boxed()
        };
        (choices, fallback, gather).prop_map(|(choices, fallback, gather)| {
            // Rule 3: a set with no sticky choice must carry a fallback.
            let fallback = if choices.iter().any(|c| c.sticky) {
                fallback
            } else {
                Some(fallback.unwrap_or(Exit::End))
            };
            Tail::Choices {
                choices,
                fallback,
                gather,
            }
        })
    })
}

fn resolve_exit(e: &mut Exit, table: &[Divert]) {
    if let Exit::Divert(d) = e
        && let Some(real) = table.get(d.knot)
    {
        *d = *real;
    }
}

fn resolve_weave(w: &mut Weave, table: &[Divert]) {
    match &mut w.tail {
        Tail::Exit(e) => resolve_exit(e, table),
        Tail::FallThrough => {}
        Tail::Choices {
            choices,
            fallback,
            gather,
        } => {
            for c in choices.iter_mut() {
                resolve_weave(&mut c.body, table);
            }
            if let Some(fb) = fallback {
                resolve_exit(fb, table);
            }
            if let Some(g) = gather {
                resolve_weave(g, table);
            }
        }
    }
}

/// Rewrite every placeholder divert (a flow index stored in `knot`) into a
/// real `(knot, stitch)` pair against the finished layout.
fn resolve(story: &mut Story) {
    let table: Vec<Divert> = (0..story.flow_count())
        .filter_map(|ix| story.flow_at(ix))
        .collect();
    for k in &mut story.knots {
        resolve_weave(&mut k.root, &table);
        for s in &mut k.stitches {
            resolve_weave(&mut s.body, &table);
        }
    }
}

/// A structure-tier story under `profile`: validates by construction.
pub fn arb_story_with(profile: Profile) -> impl Strategy<Value = Story> {
    arb_layout(profile).prop_flat_map(move |layout| {
        let flow_count: usize = layout.iter().map(|s| 1 + s).sum();
        let mut flow = 0;
        let knots: Vec<BoxedStrategy<Knot>> = layout
            .iter()
            .enumerate()
            .map(|(ki, &n_stitches)| {
                let root_site = Site {
                    flow,
                    flow_count,
                    may_go_back: false,
                    may_fall_through: false,
                    depth_left: profile.max_choice_depth,
                };
                flow += 1;
                let stitch_sites: Vec<Site> = (0..n_stitches)
                    .map(|_| {
                        let s = Site { flow, ..root_site };
                        flow += 1;
                        s
                    })
                    .collect();
                let stitches: Vec<BoxedStrategy<Stitch>> = stitch_sites
                    .into_iter()
                    .enumerate()
                    .map(|(si, site)| {
                        (arb_name_base(), arb_weave(profile, site))
                            .prop_map(move |(base, body)| Stitch {
                                name: format!("{base}_s{si}"),
                                body,
                            })
                            .boxed()
                    })
                    .collect();
                (arb_name_base(), arb_weave(profile, root_site), stitches)
                    .prop_map(move |(base, root, stitches)| Knot {
                        name: format!("{base}_k{ki}"),
                        root,
                        stitches,
                    })
                    .boxed()
            })
            .collect();
        knots.prop_map(|knots| {
            let mut story = Story { knots };
            resolve(&mut story);
            story
        })
    })
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
}
