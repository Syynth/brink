//! Story-level program generator — `docs/program-generator-spec.md`.
//!
//! Three layers, deliberately separate:
//!
//! - [`model`] — a **typed story model**. Every reference resolves by
//!   construction (declare-before-use is a property of the types, not of a
//!   filter), every expression is well-typed, and every story terminates by
//!   construction: flows are laid out in a fixed linear order, forward
//!   diverts go to a later flow, and a back-edge is legal only inside a
//!   once-only (`*`) choice body, so each revisit consumes a choice that can
//!   never be offered again (`model::validate` states the rules; the
//!   strategies obey them; the smoke properties prove it).
//! - [`print`] — the `.ink` printer. It is the *dialect switch*: a `.brink`
//!   printer over the same model is the spec's §7 follow-on, and nothing in
//!   the model knows which surface it will be printed to.
//! - [`strategy`] — proptest strategies that generate a **raw skeleton** of
//!   independent values and [`strategy::decode`] it into a valid story, so
//!   shrinking simplifies the story component by component and a
//!   counterexample arrives as a story a human can read.
//!
//! Tiers covered: **structure** (knots, stitches, diverts, choices —
//! sticky / once-only / conditioned — fallbacks, gathers, glue, text) and
//! **expressions** (global `VAR`s of int / bool / string, temps, assignments
//! including `+=`/`-=`, typed arithmetic / comparison / boolean expressions,
//! interpolation, inline conditionals, multi-line conditional blocks, choice
//! conditions). Functions, tunnels, threads, lists, sequences and externals
//! are the later tiers (#3370) and extend the model rather than replacing it.
//!
//! Consumers: the smoke properties in this crate's own `tests/`, and — as
//! they land — every equivalence property of
//! `docs/observable-semantics-spec.md` §4.1 (optimizer, `fmt`, respell,
//! auto-fix `Safe` fixers) plus the inkjs differential (#3379).

pub mod model;
pub mod print;
pub mod strategy;

pub use model::{
    AssignOp, BinOp, Choice, Divert, Exit, Expr, Item, Knot, Literal, Part, SeqKind, Stitch, Story,
    Tail, Ty, VarDecl, Weave,
};
pub use print::print_ink;
pub use strategy::{Profile, RawStory, arb_raw_story, arb_story, arb_story_with, decode};
