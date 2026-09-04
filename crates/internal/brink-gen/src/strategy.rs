//! proptest strategies over the model.
//!
//! # Skeleton, then decode
//!
//! The strategies never look at a generated value while generating: they
//! produce a **raw skeleton** — a plain tree of independent values in which a
//! divert target is just a small integer, a variable reference is an index,
//! an operator is a byte — and a deterministic [`decode`] step turns that
//! skeleton into a valid [`Story`], resolving every raw value into the range
//! the model's rules allow at its site: forward flows anywhere, back-edges
//! only inside once-only choice bodies, fall-through only into a gather, a
//! set that could run out gets a fallback, an expression decoded against the
//! type its position needs and the names in scope there.
//!
//! That shape is what makes **shrinking** work. proptest shrinks each
//! component of an independent tree on its own — fewer knots, fewer items,
//! a smaller target integer, a simpler tail, a shallower expression — and
//! the decoder keeps the result valid by construction, so a counterexample
//! shrinks all the way down to a story a human can read. The alternative,
//! `prop_flat_map`-ing weaves against a generated layout, regenerates the
//! inner values whenever the outer ones shrink and stalls with a large story
//! (the first version of this module did exactly that).

use proptest::prelude::*;

use crate::model::{
    AssignOp, BinOp, Choice, Divert, Exit, Expr, FlowKind, FnSig, Function, Item, Knot, Literal,
    Param, Part, Stitch, Story, Tail, Ty, VarDecl, Weave,
};

/// Biasing knobs — **data**, so a property names the profile it wants
/// (`docs/program-generator-spec.md` §4). Size bounds only so far; bait
/// flags arrive with the later tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// Knots per story (at least 1).
    pub max_knots: usize,
    /// Stitches per knot.
    pub max_stitches: usize,
    /// Items (lines, assignments, temps, conditional blocks) per weave.
    pub max_items: usize,
    /// Choices per choice set (at least 1).
    pub max_choices: usize,
    /// How deep choice sets may nest inside choice bodies (0 = none).
    pub max_choice_depth: usize,
    /// Global `VAR` declarations.
    pub max_vars: usize,
    /// Expression nesting depth (0 = literals and variables only).
    pub max_expr_depth: usize,
    /// Whether content lines may carry inline conditionals
    /// (`{cond: a|b}`). Off for the respell route, whose emitter does not
    /// yet spell them (`hir::emit_native`'s refused shapes).
    pub inline_conditionals: bool,
    /// Functions per story (0 = none). Function `i` may call only
    /// functions `< i` (`crate::model` rule 8).
    pub max_functions: usize,
    /// Parameters per function.
    pub max_params: usize,
    /// Tunnel knots per story (0 = none): entered by `-> t ->`, left by
    /// `->->` (`crate::model` rule 10).
    pub max_tunnels: usize,
    /// Thread knots per story (0 = none): entered by `<- t`, left by
    /// `-> DONE` (rule 11).
    pub max_threads: usize,
}

impl Profile {
    /// The default profile: small enough that the harness's DFS explores
    /// every path in well under a second, large enough to nest.
    pub const DEFAULT: Self = Self {
        max_knots: 4,
        max_stitches: 2,
        max_items: 3,
        max_choices: 3,
        max_choice_depth: 2,
        max_vars: 3,
        max_expr_depth: 2,
        inline_conditionals: true,
        max_functions: 2,
        max_params: 2,
        max_tunnels: 2,
        max_threads: 1,
    };

    /// Structure only — no variables, so no expressions can be decoded
    /// beyond literals: the shape the first tier shipped with.
    pub const STRUCTURE: Self = Self {
        max_vars: 0,
        max_functions: 0,
        max_tunnels: 0,
        max_threads: 0,
        ..Self::DEFAULT
    };

    /// The `plain_ink` differential profile (`docs/program-generator-spec.md`
    /// §6, issue #3379): the stories `tests/inkjs_differential.rs` replays
    /// through inkjs. Today it IS [`Self::DEFAULT`] — every construct the
    /// generator emits is plain ink, so the whole model is admissible — but
    /// the differential names this profile rather than the default so that a
    /// native-only construct (a `.brink`-surface feature the reference
    /// cannot run) lands behind a knob here, not in the differential by
    /// accident.
    pub const PLAIN_INK: Self = Self::DEFAULT;

    /// The subset the ink → `.brink` respeller emits today: structure only,
    /// no inline conditionals in content. `tests/equivalence.rs`'s
    /// `trace(P) = trace(respell(P))` property runs on it so the property
    /// is not vacuous while the emitter's supported shapes grow (#1951's
    /// holes, #1976's springs); widen it as they land.
    pub const RESPELLABLE: Self = Self {
        inline_conditionals: false,
        ..Self::STRUCTURE
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

/// An expression before typing: the decoder reads it against the type its
/// position needs. A `Var` is an index into the names of that type in
/// scope; a `Bin`'s byte selects an operator legal for the type.
#[derive(Debug, Clone)]
pub enum RawExpr {
    Lit(u8),
    Var(u8),
    Neg(Box<RawExpr>),
    Not(Box<RawExpr>),
    Bin(Box<RawExpr>, u8, Box<RawExpr>),
    /// A call in expression position: the byte indexes the callable
    /// functions returning the wanted type; the args decode against the
    /// callee's parameters (missing ones become literals, extras drop).
    Call(u8, Vec<RawExpr>),
}

#[derive(Debug, Clone)]
pub enum RawPart {
    Text(String),
    /// The byte picks the interpolated type.
    Interp(RawExpr, u8),
    Cond {
        cond: RawExpr,
        then: String,
        otherwise: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum RawItem {
    Line {
        parts: Vec<RawPart>,
        glue: bool,
    },
    /// `target` indexes the assignable names in scope; `op` picks the
    /// operator (compound ops only decode for int targets).
    Assign {
        target: u8,
        op: u8,
        value: RawExpr,
    },
    /// `ty` picks the temp's type.
    Temp {
        ty: u8,
        init: RawExpr,
    },
    Cond {
        cond: RawExpr,
        then: Vec<RawItem>,
        otherwise: Option<Vec<RawItem>>,
    },
    /// `~ f(args)`: the byte indexes the callable void functions.
    Call(u8, Vec<RawExpr>),
    /// `-> t ->`: the byte indexes the tunnel flows callable from here.
    TunnelCall(u8),
    /// `<- t`: the byte indexes the thread flows (plain-knot weaves only).
    Thread(u8),
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
    pub condition: Option<RawExpr>,
    pub label: String,
    pub body: RawWeave,
}

#[derive(Debug, Clone)]
pub struct RawWeave {
    pub items: Vec<RawItem>,
    pub tail: RawTail,
}

#[derive(Debug, Clone)]
pub struct RawKnot {
    pub name: String,
    pub root: RawWeave,
    pub stitches: Vec<(String, RawWeave)>,
}

/// A global before typing: `ty` picks the type, `init` the literal.
#[derive(Debug, Clone)]
pub struct RawVar {
    pub name: String,
    pub ty: u8,
    pub init: u8,
}

/// A function before typing: each parameter is `(type byte, by_ref)`; the
/// return, when present, is `(expr, type byte)`.
#[derive(Debug, Clone)]
pub struct RawFunction {
    pub name: String,
    pub params: Vec<(u8, bool)>,
    pub body: Vec<RawItem>,
    pub ret: Option<(RawExpr, u8)>,
}

/// The unresolved story: independent values only.
#[derive(Debug, Clone)]
pub struct RawStory {
    pub vars: Vec<RawVar>,
    pub knots: Vec<RawKnot>,
    pub functions: Vec<RawFunction>,
    /// Decoded after the plain knots, as tunnel knots.
    pub tunnels: Vec<RawKnot>,
    /// Decoded after the tunnels, as thread knots.
    pub threads: Vec<RawKnot>,
}

// ─── Leaf strategies ─────────────────────────────────────────────────

/// Characters that are never ink-significant in content position: no
/// `{}`, `#`, `|`, `\`, `[]`, `~`, `=`, `*`, `+`, `-`, `<`, `>`, `/`, `"`,
/// and a leading lowercase letter so a line can never read as a keyword, a
/// choice/gather marker, or a `TODO:`.
fn arb_text() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9 ,.!?;:]{0,29}".prop_map(|s| s.trim_end().to_owned())
}

/// Names are made unique by suffixing the flow's own indices — the base is
/// only for readability of a shrunk story.
fn arb_name_base() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,5}"
}

fn arb_raw_exit() -> impl Strategy<Value = RawExit> {
    prop_oneof![
        4 => any::<u8>().prop_map(RawExit::Forward),
        2 => any::<u8>().prop_map(RawExit::Backward),
        1 => Just(RawExit::End),
        1 => Just(RawExit::Done),
    ]
}

/// `calls`: whether a call may appear (off when the profile has no
/// functions, so the skeleton carries no dead entropy).
fn arb_raw_expr(depth: usize, calls: bool) -> BoxedStrategy<RawExpr> {
    let leaf = prop_oneof![
        2 => any::<u8>().prop_map(RawExpr::Lit),
        3 => any::<u8>().prop_map(RawExpr::Var),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    let inner = arb_raw_expr(depth - 1, calls);
    let call_weight = u32::from(calls) * 2;
    prop_oneof![
        3 => leaf,
        1 => inner.clone().prop_map(|e| RawExpr::Neg(Box::new(e))),
        1 => inner.clone().prop_map(|e| RawExpr::Not(Box::new(e))),
        4 => (inner.clone(), any::<u8>(), inner.clone())
            .prop_map(|(l, op, r)| RawExpr::Bin(Box::new(l), op, Box::new(r))),
        call_weight => (any::<u8>(), prop::collection::vec(inner, 0..=2))
            .prop_map(|(f, args)| RawExpr::Call(f, args)),
    ]
    .boxed()
}

fn arb_expr_for(p: Profile) -> BoxedStrategy<RawExpr> {
    arb_raw_expr(p.max_expr_depth, p.max_functions > 0)
}

fn arb_raw_part(p: Profile) -> impl Strategy<Value = RawPart> {
    // Weight 0 removes the arm without a second strategy type
    // (`prop_oneof!` rejects an all-zero table, and the text arm keeps it
    // positive).
    let cond_weight = u32::from(p.inline_conditionals);
    prop_oneof![
        4 => arb_text().prop_map(RawPart::Text),
        2 => (arb_expr_for(p), any::<u8>()).prop_map(|(e, t)| RawPart::Interp(e, t)),
        cond_weight => (arb_expr_for(p), arb_text(), prop::option::weighted(0.5, arb_text()))
            .prop_map(|(cond, then, otherwise)| RawPart::Cond { cond, then, otherwise }),
    ]
}

/// `in_cond`: inside a conditional branch — no temps, no nested blocks.
fn arb_raw_item(p: Profile, in_cond: bool) -> BoxedStrategy<RawItem> {
    let line = (
        prop::collection::vec(arb_raw_part(p), 1..=3),
        prop::bool::weighted(0.15),
    )
        .prop_map(|(parts, glue)| RawItem::Line { parts, glue });
    let assign = (any::<u8>(), any::<u8>(), arb_expr_for(p))
        .prop_map(|(target, op, value)| RawItem::Assign { target, op, value });
    let call_weight = u32::from(p.max_functions > 0);
    let call = (
        any::<u8>(),
        prop::collection::vec(arb_expr_for(p), 0..=p.max_params),
    )
        .prop_map(|(f, args)| RawItem::Call(f, args));
    let tunnel_weight = u32::from(p.max_tunnels > 0);
    let tunnel = any::<u8>().prop_map(RawItem::TunnelCall);
    let thread_weight = u32::from(p.max_threads > 0);
    let thread = any::<u8>().prop_map(RawItem::Thread);
    if in_cond {
        return prop_oneof![
            4 => line,
            2 => assign,
            call_weight => call,
            tunnel_weight => tunnel,
            thread_weight => thread,
        ]
        .boxed();
    }
    let temp = (any::<u8>(), arb_expr_for(p)).prop_map(|(ty, init)| RawItem::Temp { ty, init });
    let branch = || prop::collection::vec(arb_raw_item(p, true), 1..=2);
    let cond = (
        arb_expr_for(p),
        branch(),
        prop::option::weighted(0.5, branch()),
    )
        .prop_map(|(cond, then, otherwise)| RawItem::Cond {
            cond,
            then,
            otherwise,
        });
    prop_oneof![
        4 => line,
        2 => assign,
        1 => temp,
        1 => cond,
        call_weight => call,
        tunnel_weight => tunnel,
        thread_weight => thread,
    ]
    .boxed()
}

fn arb_raw_weave(p: Profile, depth_left: usize) -> BoxedStrategy<RawWeave> {
    let items = prop::collection::vec(arb_raw_item(p, false), 0..=p.max_items);
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
            prop::option::weighted(0.3, arb_expr_for(p)),
            arb_raw_weave(p, depth_left - 1),
        )
            .prop_map(|(label, sticky, condition, body)| RawChoice {
                sticky,
                condition,
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
    (items, tail)
        .prop_map(|(items, tail)| RawWeave { items, tail })
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

fn arb_raw_var() -> impl Strategy<Value = RawVar> {
    (arb_name_base(), any::<u8>(), any::<u8>()).prop_map(|(name, ty, init)| RawVar {
        name,
        ty,
        init,
    })
}

fn arb_raw_function(p: Profile) -> impl Strategy<Value = RawFunction> {
    let param = (any::<u8>(), prop::bool::weighted(0.3));
    (
        arb_name_base(),
        prop::collection::vec(param, 0..=p.max_params),
        prop::collection::vec(arb_raw_item(p, false), 0..=p.max_items),
        prop::option::weighted(0.6, (arb_expr_for(p), any::<u8>())),
    )
        .prop_map(|(name, params, body, ret)| RawFunction {
            name,
            params,
            body,
            ret,
        })
}

/// A raw skeleton under `profile`.
pub fn arb_raw_story(profile: Profile) -> impl Strategy<Value = RawStory> {
    (
        prop::collection::vec(arb_raw_var(), 0..=profile.max_vars),
        prop::collection::vec(arb_raw_knot(profile), 1..=profile.max_knots.max(1)),
        prop::collection::vec(arb_raw_function(profile), 0..=profile.max_functions),
        prop::collection::vec(arb_raw_knot(profile), 0..=profile.max_tunnels),
        prop::collection::vec(arb_raw_knot(profile), 0..=profile.max_threads),
    )
        .prop_map(|(vars, knots, functions, tunnels, threads)| RawStory {
            vars,
            knots,
            functions,
            tunnels,
            threads,
        })
}

// ─── Decode ──────────────────────────────────────────────────────────

/// Where a weave sits, for resolving its raw exits: its flow index, the
/// range `first..end` of flows of its own kind (diverts never leave the
/// kind — rules 10–11), and what its position allows.
#[derive(Clone, Copy)]
struct Site {
    flow: usize,
    first: usize,
    end: usize,
    kind: FlowKind,
    may_go_back: bool,
    may_fall_through: bool,
}

impl Site {
    /// The exit a flow of this kind takes when it has nothing else to do.
    fn default_exit(self) -> Exit {
        match self.kind {
            FlowKind::Knot => Exit::End,
            FlowKind::Tunnel => Exit::TunnelReturn,
            FlowKind::Thread => Exit::Done,
        }
    }
}

/// Names in scope while decoding: globals first, then parameters (inside
/// a function) and temps as declared; plus the functions callable from
/// here (rule 8: every function from flow code, only earlier ones from a
/// function body).
#[derive(Clone, Default)]
struct Env {
    vars: Vec<(String, Ty)>,
    funcs: Vec<FnSig>,
    /// Tunnel flows callable from here (rule 10's DAG already applied).
    tunnels: Vec<Divert>,
    /// Thread flows startable from here (plain-knot weaves only).
    threads: Vec<Divert>,
}

impl Env {
    /// The `n`-th entry of `list`, wrapping; `None` when empty.
    fn pick_flow(list: &[Divert], n: u8) -> Option<Divert> {
        if list.is_empty() {
            None
        } else {
            Some(list[usize::from(n) % list.len()])
        }
    }

    /// The `n`-th callable function whose return type is `ret` (`None`
    /// for a void function), wrapping.
    fn pick_fn(&self, ret: Option<Ty>, n: u8) -> Option<&FnSig> {
        let matching: Vec<&FnSig> = self.funcs.iter().filter(|f| f.ret == ret).collect();
        if matching.is_empty() {
            None
        } else {
            Some(matching[usize::from(n) % matching.len()])
        }
    }

    /// The `n`-th name of type `ty`, wrapping.
    fn pick(&self, ty: Ty, n: u8) -> Option<&str> {
        let of_ty: Vec<&str> = self
            .vars
            .iter()
            .filter(|(_, t)| *t == ty)
            .map(|(name, _)| name.as_str())
            .collect();
        if of_ty.is_empty() {
            None
        } else {
            Some(of_ty[usize::from(n) % of_ty.len()])
        }
    }

    /// The `n`-th name of any type, wrapping.
    fn pick_any(&self, n: u8) -> Option<(&str, Ty)> {
        if self.vars.is_empty() {
            None
        } else {
            let (name, ty) = &self.vars[usize::from(n) % self.vars.len()];
            Some((name.as_str(), *ty))
        }
    }
}

const WORDS: [&str; 8] = [
    "alpha", "beta", "gamma", "delta", "echo", "fox", "golf", "hotel",
];

fn ty_of(byte: u8) -> Ty {
    match byte % 3 {
        0 => Ty::Int,
        1 => Ty::Bool,
        _ => Ty::Str,
    }
}

fn literal(ty: Ty, n: u8) -> Literal {
    match ty {
        Ty::Int => Literal::Int(i32::from(n % 21)),
        Ty::Bool => Literal::Bool(n.is_multiple_of(2)),
        Ty::Str => Literal::Str(WORDS[usize::from(n) % WORDS.len()].to_owned()),
    }
}

/// A small positive int literal for `mod` divisors and `*` operands, so
/// generated arithmetic stays far from overflow.
fn small_positive(raw: &RawExpr, salt: u8) -> Expr {
    let n = match raw {
        RawExpr::Lit(n) | RawExpr::Var(n) => *n,
        _ => salt,
    };
    Expr::Lit(Literal::Int(1 + i32::from(n % 9)))
}

/// The index a raw argument contributes when a `ref` parameter needs a
/// variable picked: its own leaf byte, or `salt` for a compound.
fn ref_index(raw: Option<&RawExpr>, salt: u8) -> u8 {
    match raw {
        Some(RawExpr::Lit(n) | RawExpr::Var(n) | RawExpr::Call(n, _)) => *n,
        _ => salt,
    }
}

/// Decode the arguments of a call to `sig`: one per parameter, typed for
/// it; a `ref` parameter needs a visible variable of its type and yields
/// `None` when there is none (the call cannot be made).
fn decode_args(sig: &FnSig, raw: &[RawExpr], env: &Env) -> Option<Vec<Expr>> {
    let mut args = Vec::with_capacity(sig.params.len());
    for (i, (ty, by_ref)) in sig.params.iter().enumerate() {
        let salt = u8::try_from(i).unwrap_or(u8::MAX);
        let raw_arg = raw.get(i);
        if *by_ref {
            let name = env.pick(*ty, ref_index(raw_arg, salt))?;
            args.push(Expr::Var(name.to_owned()));
        } else {
            args.push(match raw_arg {
                Some(r) => decode_expr(r, *ty, env),
                None => Expr::Lit(literal(*ty, salt)),
            });
        }
    }
    Some(args)
}

fn decode_expr(raw: &RawExpr, want: Ty, env: &Env) -> Expr {
    match raw {
        RawExpr::Lit(n) => Expr::Lit(literal(want, *n)),
        RawExpr::Call(f, args) => env
            .pick_fn(Some(want), *f)
            .and_then(|sig| {
                decode_args(sig, args, env).map(|args| Expr::Call {
                    name: sig.name.clone(),
                    args,
                })
            })
            // No callable function of this type (or no variable for a
            // `ref` parameter): read the byte as a literal instead.
            .unwrap_or_else(|| Expr::Lit(literal(want, *f))),
        RawExpr::Var(n) => env.pick(want, *n).map_or_else(
            || Expr::Lit(literal(want, *n)),
            |name| Expr::Var(name.to_owned()),
        ),
        RawExpr::Neg(inner) => {
            if want == Ty::Int {
                Expr::Neg(Box::new(decode_expr(inner, Ty::Int, env)))
            } else {
                decode_expr(inner, want, env)
            }
        }
        RawExpr::Not(inner) => {
            if want == Ty::Bool {
                Expr::Not(Box::new(decode_expr(inner, Ty::Bool, env)))
            } else {
                decode_expr(inner, want, env)
            }
        }
        RawExpr::Bin(l, op, r) => match want {
            Ty::Int => match op % 4 {
                0 => Expr::Bin(
                    Box::new(decode_expr(l, Ty::Int, env)),
                    BinOp::Add,
                    Box::new(decode_expr(r, Ty::Int, env)),
                ),
                1 => Expr::Bin(
                    Box::new(decode_expr(l, Ty::Int, env)),
                    BinOp::Sub,
                    Box::new(decode_expr(r, Ty::Int, env)),
                ),
                2 => Expr::Bin(
                    Box::new(decode_expr(l, Ty::Int, env)),
                    BinOp::Mul,
                    Box::new(small_positive(r, *op)),
                ),
                _ => Expr::Bin(
                    Box::new(decode_expr(l, Ty::Int, env)),
                    BinOp::Mod,
                    Box::new(small_positive(r, *op)),
                ),
            },
            Ty::Bool => {
                let (bin, operand) = match op % 8 {
                    0 => (BinOp::Eq, ty_of(op / 8)),
                    1 => (BinOp::Ne, ty_of(op / 8)),
                    2 => (BinOp::Lt, Ty::Int),
                    3 => (BinOp::Gt, Ty::Int),
                    4 => (BinOp::Le, Ty::Int),
                    5 => (BinOp::Ge, Ty::Int),
                    6 => (BinOp::And, Ty::Bool),
                    _ => (BinOp::Or, Ty::Bool),
                };
                Expr::Bin(
                    Box::new(decode_expr(l, operand, env)),
                    bin,
                    Box::new(decode_expr(r, operand, env)),
                )
            }
            // No binary operator yields a string: read the left operand.
            Ty::Str => decode_expr(l, Ty::Str, env),
        },
    }
}

fn resolve_exit(raw: RawExit, site: Site, table: &[Divert]) -> Exit {
    let forward_count = site.end.saturating_sub(site.flow + 1);
    let forward = |n: u8| {
        if forward_count == 0 {
            site.default_exit()
        } else {
            Exit::Divert(table[site.flow + 1 + usize::from(n) % forward_count])
        }
    };
    match raw {
        RawExit::End => Exit::End,
        // A tunnel never leaves by `-> DONE` (rule 10): its `Done` reads
        // as the return.
        RawExit::Done => match site.kind {
            FlowKind::Tunnel => Exit::TunnelReturn,
            FlowKind::Knot | FlowKind::Thread => Exit::Done,
        },
        RawExit::Forward(n) => forward(n),
        RawExit::Backward(n) => {
            if site.may_go_back {
                let span = site.flow + 1 - site.first;
                Exit::Divert(table[site.first + usize::from(n) % span])
            } else {
                // Not allowed to go back here: read the raw index forward.
                forward(n)
            }
        }
    }
}

/// Decode items in order, extending `env` with each temp. Returns the
/// decoded items (raw items with nothing valid to decode into are dropped).
fn decode_items(
    raw: &[RawItem],
    env: &mut Env,
    in_cond: bool,
    temp_counter: &mut usize,
) -> Vec<Item> {
    let mut out = Vec::new();
    for item in raw {
        match item {
            RawItem::Line { parts, glue } => {
                let parts = parts
                    .iter()
                    .map(|p| match p {
                        RawPart::Text(t) => Part::Text(t.clone()),
                        RawPart::Interp(e, ty) => Part::Interp(decode_expr(e, ty_of(*ty), env)),
                        RawPart::Cond {
                            cond,
                            then,
                            otherwise,
                        } => Part::Cond {
                            cond: decode_expr(cond, Ty::Bool, env),
                            then: then.clone(),
                            otherwise: otherwise.clone(),
                        },
                    })
                    .collect();
                out.push(Item::Line { parts, glue: *glue });
            }
            RawItem::Assign { target, op, value } => {
                if let Some((name, ty)) = env.pick_any(*target) {
                    let op = if ty == Ty::Int {
                        [AssignOp::Set, AssignOp::Add, AssignOp::Sub][usize::from(op % 3)]
                    } else {
                        AssignOp::Set
                    };
                    out.push(Item::Assign {
                        target: name.to_owned(),
                        op,
                        value: decode_expr(value, ty, env),
                    });
                }
            }
            RawItem::Temp { ty, init } => {
                if in_cond {
                    continue;
                }
                let ty = ty_of(*ty);
                let name = format!("t{temp_counter}");
                *temp_counter += 1;
                out.push(Item::Temp {
                    name: name.clone(),
                    init: decode_expr(init, ty, env),
                });
                env.vars.push((name, ty));
            }
            RawItem::TunnelCall(n) => {
                if let Some(d) = Env::pick_flow(&env.tunnels, *n) {
                    out.push(Item::TunnelCall(d));
                }
            }
            RawItem::Thread(n) => {
                if let Some(d) = Env::pick_flow(&env.threads, *n) {
                    out.push(Item::Thread(d));
                }
            }
            RawItem::Call(f, args) => {
                if let Some(sig) = env.pick_fn(None, *f)
                    && let Some(args) = decode_args(sig, args, env)
                {
                    out.push(Item::Call {
                        name: sig.name.clone(),
                        args,
                    });
                }
            }
            RawItem::Cond {
                cond,
                then,
                otherwise,
            } => {
                if in_cond {
                    continue;
                }
                let mut branch_env = env.clone();
                let then = decode_items(then, &mut branch_env, true, temp_counter);
                let otherwise = otherwise.as_ref().and_then(|o| {
                    let mut branch_env = env.clone();
                    let items = decode_items(o, &mut branch_env, true, temp_counter);
                    (!items.is_empty()).then_some(items)
                });
                if then.is_empty() {
                    continue;
                }
                out.push(Item::Cond {
                    cond: decode_expr(cond, Ty::Bool, env),
                    then,
                    otherwise,
                });
            }
        }
    }
    out
}

fn decode_weave(
    raw: &RawWeave,
    site: Site,
    table: &[Divert],
    env: &Env,
    temp_counter: &mut usize,
) -> Weave {
    let mut env = env.clone();
    let items = decode_items(&raw.items, &mut env, false, temp_counter);
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
            // Rule 10: a tunnel's choices are once-only, so re-entry through
            // several call sites cannot re-offer them.
            let in_tunnel = site.kind == FlowKind::Tunnel;
            let decoded: Vec<Choice> = choices
                .iter()
                .map(|c| {
                    let sticky = c.sticky && !in_tunnel;
                    let body_site = Site {
                        may_go_back: site.may_go_back || !sticky,
                        may_fall_through: has_gather,
                        ..site
                    };
                    Choice {
                        sticky,
                        condition: c.condition.as_ref().map(|e| decode_expr(e, Ty::Bool, &env)),
                        label: c.label.clone(),
                        body: decode_weave(&c.body, body_site, table, &env, temp_counter),
                    }
                })
                .collect();
            // A fallback fires from a normal flow position: never a back-edge.
            let fallback_site = Site {
                may_go_back: false,
                ..site
            };
            let mut fallback = fallback.map(|f| resolve_exit(f, fallback_site, table));
            // Rule 3: without an unconditional sticky choice, carry a fallback.
            let protected = decoded.iter().any(|c| c.sticky && c.condition.is_none());
            if !protected && fallback.is_none() {
                fallback = Some(Exit::End);
            }
            // The gather continues at the enclosing weave's own site and
            // sees only the temps declared before the choice point.
            let gather = gather
                .as_ref()
                .map(|g| Box::new(decode_weave(g, site, table, &env, temp_counter)));
            Tail::Choices {
                choices: decoded,
                fallback,
                gather,
            }
        }
    };
    Weave { items, tail }
}

/// Decode the functions in order: function `i` decodes against the
/// signatures of functions `< i` only, so the call graph is a DAG (rule 8).
/// Returns the functions and their signatures.
fn decode_functions(
    raw: &[RawFunction],
    global_vars: &[(String, Ty)],
) -> (Vec<Function>, Vec<FnSig>) {
    let mut functions: Vec<Function> = Vec::with_capacity(raw.len());
    let mut sigs: Vec<FnSig> = Vec::with_capacity(raw.len());
    for (fi, f) in raw.iter().enumerate() {
        let params: Vec<Param> = f
            .params
            .iter()
            .enumerate()
            .map(|(pi, (ty, by_ref))| Param {
                name: format!("p{pi}"),
                ty: ty_of(*ty),
                by_ref: *by_ref,
            })
            .collect();
        let mut env = Env {
            vars: global_vars.to_vec(),
            funcs: sigs.clone(),
            tunnels: Vec::new(),
            threads: Vec::new(),
        };
        env.vars
            .extend(params.iter().map(|p| (p.name.clone(), p.ty)));
        let mut temps = 0;
        let mut body = decode_items(&f.body, &mut env, false, &mut temps);
        let ret = f
            .ret
            .as_ref()
            .map(|(e, ty)| decode_expr(e, ty_of(*ty), &env));
        if body.is_empty() && ret.is_none() {
            // Rule 8: never an empty function.
            body.push(Item::Line {
                parts: vec![Part::Text("noop".to_owned())],
                glue: false,
            });
        }
        let function = Function {
            name: format!("f{fi}_{}", f.name),
            params,
            body,
            ret,
        };
        sigs.push(FnSig {
            name: function.name.clone(),
            params: function.params.iter().map(|p| (p.ty, p.by_ref)).collect(),
            ret: f.ret.as_ref().map(|(_, ty)| ty_of(*ty)),
        });
        functions.push(function);
    }
    (functions, sigs)
}

/// The flow table of a story under decode: every knot root and stitch in
/// linear order, each kind's contiguous range within it, and the flows a
/// tunnel call or thread may name.
struct Layout {
    table: Vec<Divert>,
    ranges: Vec<(FlowKind, usize, usize)>,
    tunnel_flows: Vec<Divert>,
    thread_flows: Vec<Divert>,
}

impl Layout {
    fn of(all: &[(&RawKnot, FlowKind)]) -> Self {
        let mut table = Vec::new();
        let mut ranges: Vec<(FlowKind, usize, usize)> = Vec::new();
        for (ki, (k, kind)) in all.iter().enumerate() {
            let start = table.len();
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
            match ranges.last_mut() {
                Some((rk, _, end)) if rk == kind => *end = table.len(),
                _ => ranges.push((*kind, start, table.len())),
            }
        }
        let of_kind = |kind: FlowKind| -> Vec<Divert> {
            table
                .iter()
                .copied()
                .filter(|d| all[d.knot].1 == kind)
                .collect()
        };
        let tunnel_flows = of_kind(FlowKind::Tunnel);
        let thread_flows = of_kind(FlowKind::Thread);
        Self {
            table,
            ranges,
            tunnel_flows,
            thread_flows,
        }
    }

    /// `first..end` of the flows of `kind` (empty when there are none).
    fn range_of(&self, kind: FlowKind) -> (usize, usize) {
        self.ranges
            .iter()
            .find(|(k, _, _)| *k == kind)
            .map_or((0, 0), |(_, s, e)| (*s, *e))
    }

    /// The decode environment for knot `ki` of `kind` — rules 10–11: a
    /// tunnel calls only later tunnels; only a plain knot starts a thread.
    fn env_for(&self, globals: &Env, kind: FlowKind, ki: usize) -> Env {
        Env {
            tunnels: match kind {
                FlowKind::Tunnel => self
                    .tunnel_flows
                    .iter()
                    .copied()
                    .filter(|d| d.knot > ki)
                    .collect(),
                FlowKind::Knot | FlowKind::Thread => self.tunnel_flows.clone(),
            },
            threads: match kind {
                FlowKind::Knot => self.thread_flows.clone(),
                FlowKind::Tunnel | FlowKind::Thread => Vec::new(),
            },
            ..globals.clone()
        }
    }
}

/// Turn a raw skeleton into a valid [`Story`]: names made unique, every
/// exit resolved into the legal range for its site, every expression typed
/// for its position against the names in scope, every rule of
/// `crate::model` satisfied by construction.
pub fn decode(raw: &RawStory) -> Story {
    let vars: Vec<VarDecl> = raw
        .vars
        .iter()
        .enumerate()
        .map(|(i, v)| VarDecl {
            name: format!("v{i}_{}", v.name),
            init: literal(ty_of(v.ty), v.init),
        })
        .collect();
    let global_vars: Vec<(String, Ty)> =
        vars.iter().map(|v| (v.name.clone(), v.init.ty())).collect();
    let (functions, sigs) = decode_functions(&raw.functions, &global_vars);
    let globals = Env {
        vars: global_vars,
        funcs: sigs,
        tunnels: Vec::new(),
        threads: Vec::new(),
    };
    let all: Vec<(&RawKnot, FlowKind)> = raw
        .knots
        .iter()
        .map(|k| (k, FlowKind::Knot))
        .chain(raw.tunnels.iter().map(|k| (k, FlowKind::Tunnel)))
        .chain(raw.threads.iter().map(|k| (k, FlowKind::Thread)))
        .collect();
    let layout = Layout::of(&all);
    // Pass 2: decode every weave against its own flow index; temps are
    // numbered per flow.
    let mut flow = 0;
    let knots = all
        .iter()
        .enumerate()
        .map(|(ki, (k, kind))| {
            let (first, end) = layout.range_of(*kind);
            let root_site = Site {
                flow,
                first,
                end,
                kind: *kind,
                may_go_back: false,
                may_fall_through: false,
            };
            let env = layout.env_for(&globals, *kind, ki);
            let table = &layout.table;
            flow += 1;
            let mut temps = 0;
            let root = decode_weave(&k.root, root_site, table, &env, &mut temps);
            let stitches = k
                .stitches
                .iter()
                .enumerate()
                .map(|(si, (base, body))| {
                    let site = Site { flow, ..root_site };
                    flow += 1;
                    let mut temps = 0;
                    Stitch {
                        name: format!("{base}_s{si}"),
                        body: decode_weave(body, site, table, &env, &mut temps),
                    }
                })
                .collect();
            let name = match kind {
                FlowKind::Knot => format!("{}_k{ki}", k.name),
                FlowKind::Tunnel => format!("{}_t{ki}", k.name),
                FlowKind::Thread => format!("{}_th{ki}", k.name),
            };
            Knot {
                name,
                kind: *kind,
                root,
                stitches,
            }
        })
        .collect();
    Story {
        vars,
        knots,
        functions,
    }
}

/// A story under `profile`: validates by construction.
pub fn arb_story_with(profile: Profile) -> impl Strategy<Value = Story> {
    arb_raw_story(profile).prop_map(|raw| decode(&raw))
}

/// A story under [`Profile::DEFAULT`].
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

        /// The structure-only profile validates too.
        #[test]
        fn structure_profile_validates(story in arb_story_with(Profile::STRUCTURE)) {
            prop_assert!(story.vars.is_empty());
            prop_assert_eq!(validate(&story), Ok(()));
        }

        /// The profile's size bounds hold.
        #[test]
        fn generated_stories_respect_profile(story in arb_story()) {
            let p = Profile::DEFAULT;
            let of = |kind: FlowKind| story.knots.iter().filter(|k| k.kind == kind).count();
            prop_assert!(of(FlowKind::Knot) >= 1 && of(FlowKind::Knot) <= p.max_knots);
            prop_assert!(of(FlowKind::Tunnel) <= p.max_tunnels);
            prop_assert!(of(FlowKind::Thread) <= p.max_threads);
            prop_assert!(story.functions.len() <= p.max_functions);
            prop_assert!(story.vars.len() <= p.max_vars);
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
        // lines. Shrinking can delete but not relocate, so a stitch — or,
        // since the tunnels/threads tier, a tunnel or thread knot — that
        // hosts the offending set legitimately survives next to the entry
        // knot (which cannot be deleted).
        assert!(
            minimal.knots.len() <= 2 && line_count <= 12,
            "expected a minimal one-knot story, got {} knots / {line_count} lines:\n{printed}",
            minimal.knots.len()
        );
    }
}
