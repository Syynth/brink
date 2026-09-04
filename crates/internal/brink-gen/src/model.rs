//! The typed story model — structure tier plus the expressions tier.
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
//! 3. A choice set never runs out: it contains an **unconditional** sticky
//!    (`+`) choice, or it carries a fallback exit — printed as the sticky
//!    form `+ -> target`. A `* -> target` fallback is itself a once-only
//!    choice, consumed the first time it fires, so it protects exactly one
//!    exhaustion of the set (found by the generator: a once-only fallback
//!    ran out on the third visit, in brink and inklecate alike). A choice
//!    guarded by a condition may be hidden, so it never counts as the
//!    protecting choice.
//! 4. A choice body may fall through (no exit of its own) only when its
//!    choice set has a gather to fall into.
//!
//! # Expressions and scope
//!
//! 5. Every expression is well-typed against the declared globals and the
//!    temps in scope: arithmetic on ints, comparisons and `and`/`or`/`not`
//!    yielding bools, equality on any single type; `mod`'s divisor is a
//!    nonzero int literal. No mixed-type coercion — that is a future bait
//!    flag, not an accident.
//! 6. A temp is visible only to the items that follow its declaration in the
//!    same weave, and to the choice bodies and gather of that weave's tail.
//!    Temps are never declared inside a conditional branch (the other branch
//!    would not define them) and a choice body's temps are never read by the
//!    gather (the other bodies would not define them) — so every temp read
//!    is dominated by its declaration by construction.
//! 7. Conditional blocks hold content only (lines, assignments, nested
//!    blocks); diverts live in tails, so conditionals never affect
//!    termination.
//!
//! # Functions
//!
//! 8. A function (`=== function f(a, ref b) ===`) has a fixed position in
//!    [`Story::functions`]; its body is items only (no tail, so no divert
//!    and no choice — ink forbids both in a function) and ends in `~ return
//!    expr` when it returns a value; the body and the return are never
//!    both absent (inklecate rejects an empty function, "Expected at least
//!    one line within the knot"). A call is legal from flow code to any
//!    function, and from function `i`'s body only to functions `< i` — the
//!    call graph is a DAG by construction, so no call can recurse and every
//!    call terminates.
//! 9. Calls are typed like any expression: argument types match the
//!    parameters, a `ref` parameter's argument is a visible variable of that
//!    type (a global, a temp, or an enclosing function's own parameter, so
//!    the reference chain of `I096-nested-pass-by-reference` is reachable),
//!    and a call in expression position names a function that returns a
//!    value. A void function is called only as a statement (`~ f(x)`).
//!
//! [`validate`] checks every rule plus name uniqueness and reference
//! resolution; the strategies in [`crate::strategy`] construct stories that
//! satisfy them, and the crate's smoke property asserts every generated
//! story validates — so a validation failure is a generator bug, never a
//! filtered case.

use std::collections::BTreeSet;
use std::fmt;

/// A complete single-compilation story: the entry is knot 0's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Story {
    /// Global `VAR` declarations, printed before the entry divert.
    pub vars: Vec<VarDecl>,
    /// Knots in document order. Never empty.
    pub knots: Vec<Knot>,
    /// Functions, printed after the knots. Function `i` may call only
    /// functions `< i` (rule 8).
    pub functions: Vec<Function>,
}

/// A function: `=== function name(params) ===`, a body of items, and an
/// optional `~ return expr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Unique across the story (shares the namespace with knots and vars).
    pub name: String,
    pub params: Vec<Param>,
    /// Items only — no tail (rule 8).
    pub body: Vec<Item>,
    /// `~ return expr` closing the body; `None` for a void function.
    pub ret: Option<Expr>,
}

/// One function parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// Unique within the function; visible in its body like a temp.
    pub name: String,
    pub ty: Ty,
    /// `ref name` — the argument is a variable the body writes through to.
    pub by_ref: bool,
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

/// A run of items followed by exactly one tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weave {
    pub items: Vec<Item>,
    pub tail: Tail,
}

/// The type of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
    Str,
}

/// A literal value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i32),
    Bool(bool),
    /// Printable text with no quote or ink-significant characters.
    Str(String),
}

impl Literal {
    pub fn ty(&self) -> Ty {
        match self {
            Self::Int(_) => Ty::Int,
            Self::Bool(_) => Ty::Bool,
            Self::Str(_) => Ty::Str,
        }
    }
}

/// A global `VAR name = literal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    /// Unique across the story.
    pub name: String,
    pub init: Literal,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    /// `mod` — the divisor must be a nonzero int literal (rule 5).
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

/// A typed expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Lit(Literal),
    /// A global or a temp in scope, by name.
    Var(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Box<Expr>, BinOp, Box<Expr>),
    /// `f(args)` — a call to a value-returning function (rules 8–9).
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

/// How an assignment writes its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    /// `+=` — int targets only.
    Add,
    /// `-=` — int targets only.
    Sub,
}

/// One piece of a content line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// `{expr}` — printed value.
    Interp(Expr),
    /// `{cond: then|otherwise}` — the branches are plain text.
    Cond {
        cond: Expr,
        then: String,
        otherwise: Option<String>,
    },
}

/// One weave item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// A content line; `glue` prints a trailing `<>`.
    Line { parts: Vec<Part>, glue: bool },
    /// `~ target = value` / `~ target += value` / `~ target -= value`.
    Assign {
        target: String,
        op: AssignOp,
        value: Expr,
    },
    /// `~ temp name = init`.
    Temp { name: String, init: Expr },
    /// A multi-line conditional block. Rule 7: content only.
    Cond {
        cond: Expr,
        then: Vec<Item>,
        otherwise: Option<Vec<Item>>,
    },
    /// `~ f(args)` — a call to a void function as a statement (rule 9).
    Call { name: String, args: Vec<Expr> },
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
    /// `* {condition} [label]` — the choice is offered only when true.
    pub condition: Option<Expr>,
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

// ─── Validation ──────────────────────────────────────────────────────

/// A function's signature as the type checker sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnSig {
    pub name: String,
    /// `(type, by_ref)` per parameter.
    pub params: Vec<(Ty, bool)>,
    /// `None` for a void function.
    pub ret: Option<Ty>,
}

impl Function {
    /// The signature, with the return type inferred from `ret` against
    /// `vars` — the names visible at the return site: globals, parameters
    /// and the body's temps — and `funcs`, the functions callable from
    /// this one (those declared before it, rule 8).
    pub fn signature(&self, vars: &[(String, Ty)], funcs: &[FnSig]) -> Result<FnSig, Invalid> {
        let ret = match &self.ret {
            Some(e) => Some(type_of(e, vars, funcs)?),
            None => None,
        };
        Ok(FnSig {
            name: self.name.clone(),
            params: self.params.iter().map(|p| (p.ty, p.by_ref)).collect(),
            ret,
        })
    }
}

/// Names in scope at a point: globals plus the temps declared so far, and
/// the functions callable from here (rule 8).
#[derive(Clone)]
struct Scope {
    vars: Vec<(String, Ty)>,
    funcs: Vec<FnSig>,
}

impl Scope {
    fn lookup(&self, name: &str) -> Option<Ty> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
    }

    fn func(&self, name: &str) -> Option<&FnSig> {
        self.funcs.iter().find(|f| f.name == name)
    }
}

/// The type of `e` in `scope`, or the rule-5 violation. `funcs` are the
/// functions callable at this point.
pub fn type_of(e: &Expr, scope_vars: &[(String, Ty)], funcs: &[FnSig]) -> Result<Ty, Invalid> {
    let scope = Scope {
        vars: scope_vars.to_vec(),
        funcs: funcs.to_vec(),
    };
    type_in(e, &scope)
}

/// Check a call's arguments against `sig` (rule 9): arity, types, and a
/// visible variable of the right type for every `ref` parameter.
fn check_call(name: &str, args: &[Expr], scope: &Scope) -> Result<Option<Ty>, Invalid> {
    let Some(sig) = scope.func(name) else {
        return Err(Invalid(format!(
            "call to unknown or not-yet-callable function `{name}`"
        )));
    };
    if sig.params.len() != args.len() {
        return Err(Invalid(format!(
            "`{name}` takes {} argument(s), called with {}",
            sig.params.len(),
            args.len()
        )));
    }
    for ((pty, by_ref), arg) in sig.params.iter().zip(args) {
        if *by_ref {
            let Expr::Var(v) = arg else {
                return Err(Invalid(format!(
                    "`ref` argument to `{name}` is not a variable"
                )));
            };
            match scope.lookup(v) {
                Some(t) if t == *pty => {}
                Some(t) => {
                    return Err(Invalid(format!(
                        "`ref` argument `{v}` to `{name}` is {t:?}, parameter is {pty:?}"
                    )));
                }
                None => return Err(Invalid(format!("unresolved `ref` argument `{v}`"))),
            }
        } else {
            let t = type_in(arg, scope)?;
            if t != *pty {
                return Err(Invalid(format!(
                    "argument to `{name}` is {t:?}, parameter is {pty:?}"
                )));
            }
        }
    }
    Ok(sig.ret)
}

fn type_in(e: &Expr, scope: &Scope) -> Result<Ty, Invalid> {
    match e {
        Expr::Lit(l) => Ok(l.ty()),
        Expr::Var(name) => scope
            .lookup(name)
            .ok_or_else(|| Invalid(format!("unresolved variable `{name}`"))),
        Expr::Neg(inner) => match type_in(inner, scope)? {
            Ty::Int => Ok(Ty::Int),
            t => Err(Invalid(format!("negation of {t:?}"))),
        },
        Expr::Not(inner) => match type_in(inner, scope)? {
            Ty::Bool => Ok(Ty::Bool),
            t => Err(Invalid(format!("`not` of {t:?}"))),
        },
        Expr::Call { name, args } => check_call(name, args, scope)?
            .ok_or_else(|| Invalid(format!("void function `{name}` used as a value"))),
        Expr::Bin(l, op, r) => {
            let lt = type_in(l, scope)?;
            let rt = type_in(r, scope)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul => {
                    if lt == Ty::Int && rt == Ty::Int {
                        Ok(Ty::Int)
                    } else {
                        Err(Invalid(format!("{op:?} on {lt:?} and {rt:?}")))
                    }
                }
                BinOp::Mod => {
                    if lt != Ty::Int {
                        return Err(Invalid(format!("mod on {lt:?}")));
                    }
                    match r.as_ref() {
                        Expr::Lit(Literal::Int(n)) if *n != 0 => Ok(Ty::Int),
                        _ => Err(Invalid("mod divisor must be a nonzero int literal".into())),
                    }
                }
                BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if lt == Ty::Int && rt == Ty::Int {
                        Ok(Ty::Bool)
                    } else {
                        Err(Invalid(format!("{op:?} on {lt:?} and {rt:?}")))
                    }
                }
                BinOp::Eq | BinOp::Ne => {
                    if lt == rt {
                        Ok(Ty::Bool)
                    } else {
                        Err(Invalid(format!("{op:?} on {lt:?} and {rt:?}")))
                    }
                }
                BinOp::And | BinOp::Or => {
                    if lt == Ty::Bool && rt == Ty::Bool {
                        Ok(Ty::Bool)
                    } else {
                        Err(Invalid(format!("{op:?} on {lt:?} and {rt:?}")))
                    }
                }
            }
        }
    }
}

/// Check every rule in the module doc. `Ok(())` means the story is a
/// well-formed, terminating, well-typed program.
pub fn validate(story: &Story) -> Result<(), Invalid> {
    if story.knots.is_empty() {
        return Err(Invalid("story has no knots".into()));
    }
    let mut names = BTreeSet::new();
    for v in &story.vars {
        if !names.insert(v.name.as_str()) {
            return Err(Invalid(format!("duplicate VAR `{}`", v.name)));
        }
    }
    for k in &story.knots {
        if !names.insert(k.name.as_str()) {
            return Err(Invalid(format!("duplicate name `{}`", k.name)));
        }
        let mut seen_s = BTreeSet::new();
        for s in &k.stitches {
            if !seen_s.insert(s.name.as_str()) {
                return Err(Invalid(format!(
                    "duplicate stitch name `{}` in knot `{}`",
                    s.name, k.name
                )));
            }
        }
    }
    for f in &story.functions {
        if !names.insert(f.name.as_str()) {
            return Err(Invalid(format!("duplicate name `{}`", f.name)));
        }
    }
    let global_vars: Vec<(String, Ty)> = story
        .vars
        .iter()
        .map(|v| (v.name.clone(), v.init.ty()))
        .collect();
    // Functions: each body sees globals + its params and may call only
    // earlier functions (rule 8), so signatures are checked in order.
    let mut sigs: Vec<FnSig> = Vec::new();
    for (i, f) in story.functions.iter().enumerate() {
        if f.body.is_empty() && f.ret.is_none() {
            return Err(Invalid(format!("function `{}` is empty", f.name)));
        }
        let mut seen_p = BTreeSet::new();
        for p in &f.params {
            if !seen_p.insert(p.name.as_str()) || global_vars.iter().any(|(n, _)| *n == p.name) {
                return Err(Invalid(format!(
                    "parameter `{}` of `{}` duplicates a visible name",
                    p.name, f.name
                )));
            }
        }
        let mut scope = Scope {
            vars: global_vars.clone(),
            funcs: sigs.clone(),
        };
        scope
            .vars
            .extend(f.params.iter().map(|p| (p.name.clone(), p.ty)));
        validate_items(&f.body, &mut scope, false, usize::MAX - i)?;
        let sig = f.signature(&scope.vars, &sigs)?;
        sigs.push(sig);
    }
    let globals = Scope {
        vars: global_vars,
        funcs: sigs,
    };
    let mut flow = 0;
    for k in &story.knots {
        validate_weave(story, &k.root, flow, false, false, &globals)?;
        flow += 1;
        for s in &k.stitches {
            validate_weave(story, &s.body, flow, false, false, &globals)?;
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

fn expect_ty(e: &Expr, want: Ty, scope: &Scope, what: &str) -> Result<(), Invalid> {
    let got = type_in(e, scope)?;
    if got == want {
        Ok(())
    } else {
        Err(Invalid(format!("{what} must be {want:?}, got {got:?}")))
    }
}

/// Validate items in order, extending `scope` with each temp. `in_cond` is
/// true inside a conditional branch, where temps may not be declared.
fn validate_items(
    items: &[Item],
    scope: &mut Scope,
    in_cond: bool,
    flow: usize,
) -> Result<(), Invalid> {
    for item in items {
        match item {
            Item::Line { parts, .. } => {
                if parts.is_empty() {
                    return Err(Invalid(format!("empty content line in flow {flow}")));
                }
                for p in parts {
                    match p {
                        Part::Text(t) => {
                            if t.is_empty() {
                                return Err(Invalid("empty text part".into()));
                            }
                        }
                        Part::Interp(e) => {
                            type_in(e, scope)?;
                        }
                        Part::Cond { cond, then, .. } => {
                            expect_ty(cond, Ty::Bool, scope, "inline condition")?;
                            if then.is_empty() {
                                return Err(Invalid("empty inline-conditional branch".into()));
                            }
                        }
                    }
                }
            }
            Item::Assign { target, op, value } => {
                let Some(tt) = scope.lookup(target) else {
                    return Err(Invalid(format!("assignment to unknown `{target}`")));
                };
                expect_ty(value, tt, scope, "assigned value")?;
                if *op != AssignOp::Set && tt != Ty::Int {
                    return Err(Invalid(format!("{op:?} on a {tt:?} target")));
                }
            }
            Item::Temp { name, init } => {
                if in_cond {
                    return Err(Invalid(format!(
                        "temp `{name}` declared inside a conditional branch"
                    )));
                }
                if scope.lookup(name).is_some() {
                    return Err(Invalid(format!("temp `{name}` shadows a visible name")));
                }
                let t = type_in(init, scope)?;
                scope.vars.push((name.clone(), t));
            }
            Item::Call { name, args } => {
                if check_call(name, args, scope)?.is_some() {
                    return Err(Invalid(format!(
                        "value-returning `{name}` called as a statement"
                    )));
                }
            }
            Item::Cond {
                cond,
                then,
                otherwise,
            } => {
                expect_ty(cond, Ty::Bool, scope, "block condition")?;
                if then.is_empty() {
                    return Err(Invalid("empty conditional block".into()));
                }
                let mut inner = scope.clone();
                validate_items(then, &mut inner, true, flow)?;
                if let Some(o) = otherwise {
                    if o.is_empty() {
                        return Err(Invalid("empty else branch".into()));
                    }
                    let mut inner = scope.clone();
                    validate_items(o, &mut inner, true, flow)?;
                }
            }
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
    scope: &Scope,
) -> Result<(), Invalid> {
    let mut scope = scope.clone();
    validate_items(&w.items, &mut scope, false, flow)?;
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
            let protected = choices.iter().any(|c| c.sticky && c.condition.is_none());
            if !protected && fallback.is_none() {
                return Err(Invalid(format!(
                    "choice set in flow {flow} can run out: no unconditional sticky choice and no fallback"
                )));
            }
            if let Some(fb) = fallback {
                validate_exit(story, *fb, flow, false)?;
            }
            for c in choices {
                if c.label.is_empty() {
                    return Err(Invalid(format!("empty choice label in flow {flow}")));
                }
                if let Some(cond) = &c.condition {
                    expect_ty(cond, Ty::Bool, &scope, "choice condition")?;
                }
                // A back-edge stays legal deeper inside a once-only body
                // (the outer once-only choice already bounds it).
                let back = may_go_back || !c.sticky;
                validate_weave(story, &c.body, flow, back, gather.is_some(), &scope)?;
            }
            match gather {
                Some(g) => validate_weave(story, g, flow, may_go_back, may_fall_through, &scope),
                None => Ok(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Item {
        Item::Line {
            parts: vec![Part::Text(s.to_owned())],
            glue: false,
        }
    }

    fn exit_weave(e: Exit) -> Weave {
        Weave {
            items: vec![text("hello")],
            tail: Tail::Exit(e),
        }
    }

    fn two_knots() -> Story {
        Story {
            functions: vec![],
            vars: vec![VarDecl {
                name: "n".into(),
                init: Literal::Int(0),
            }],
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

    fn int(n: i32) -> Expr {
        Expr::Lit(Literal::Int(n))
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
            items: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: false,
                    condition: None,
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
            items: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: false,
                    condition: None,
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
    fn conditioned_sticky_choice_does_not_protect_the_set() {
        let mut s = two_knots();
        s.knots[1].root = Weave {
            items: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: true,
                    condition: Some(Expr::Bin(
                        Box::new(Expr::Var("n".into())),
                        BinOp::Gt,
                        Box::new(int(0)),
                    )),
                    label: "maybe".into(),
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
            items: vec![],
            tail: Tail::Choices {
                choices: vec![Choice {
                    sticky: true,
                    condition: None,
                    label: "on".into(),
                    body: Weave {
                        items: vec![text("x")],
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

    #[test]
    fn expressions_are_type_checked() {
        let scope = vec![("n".to_owned(), Ty::Int), ("ok".to_owned(), Ty::Bool)];
        assert_eq!(
            type_of(
                &Expr::Bin(
                    Box::new(Expr::Var("n".into())),
                    BinOp::Add,
                    Box::new(int(1))
                ),
                &scope,
                &[]
            ),
            Ok(Ty::Int)
        );
        assert_eq!(
            type_of(
                &Expr::Bin(Box::new(Expr::Var("n".into())), BinOp::Lt, Box::new(int(1))),
                &scope,
                &[]
            ),
            Ok(Ty::Bool)
        );
        assert!(
            type_of(
                &Expr::Bin(
                    Box::new(Expr::Var("ok".into())),
                    BinOp::Add,
                    Box::new(int(1))
                ),
                &scope,
                &[]
            )
            .is_err()
        );
        assert!(
            type_of(
                &Expr::Bin(
                    Box::new(Expr::Var("n".into())),
                    BinOp::Mod,
                    Box::new(int(0))
                ),
                &scope,
                &[]
            )
            .is_err()
        );
        assert!(type_of(&Expr::Var("nope".into()), &scope, &[]).is_err());
    }

    #[test]
    fn temps_are_scoped_to_what_follows_them() {
        let mut s = two_knots();
        // Read before declaration → rejected.
        s.knots[0].root = Weave {
            items: vec![
                Item::Line {
                    parts: vec![Part::Interp(Expr::Var("t0".into()))],
                    glue: false,
                },
                Item::Temp {
                    name: "t0".into(),
                    init: int(1),
                },
            ],
            tail: Tail::Exit(Exit::End),
        };
        assert!(validate(&s).is_err());
        // Declaration then read → accepted, and the temp feeds an assignment.
        s.knots[0].root = Weave {
            items: vec![
                Item::Temp {
                    name: "t0".into(),
                    init: int(1),
                },
                Item::Assign {
                    target: "n".into(),
                    op: AssignOp::Add,
                    value: Expr::Var("t0".into()),
                },
                Item::Line {
                    parts: vec![
                        Part::Text("n is ".into()),
                        Part::Interp(Expr::Var("n".into())),
                    ],
                    glue: false,
                },
            ],
            tail: Tail::Exit(Exit::End),
        };
        assert_eq!(validate(&s), Ok(()));
        // A temp inside a conditional branch → rejected.
        s.knots[0].root = Weave {
            items: vec![Item::Cond {
                cond: Expr::Lit(Literal::Bool(true)),
                then: vec![Item::Temp {
                    name: "t1".into(),
                    init: int(1),
                }],
                otherwise: None,
            }],
            tail: Tail::Exit(Exit::End),
        };
        assert!(validate(&s).is_err());
    }
}
