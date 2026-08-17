//! NS-A3 (issue #1109, docs/stdlib-spec.md §9.6): the protocol registry.
//!
//! A **CLOSED** set of compiler-declared protocols — `display`, `compare`,
//! `iterate` — that user `STRUCT` types may *implement* but never *declare*.
//! No bounds, no user generics, no user-defined protocols (#1090 guards the
//! door). Three concerns live here:
//!
//! - **The registry itself** ([`Protocol`]): each entry's method name,
//!   signature shape, and per-protocol **effect contract**
//!   (`display`/`compare`: pure·silent·total; `iterate`'s `next`:
//!   writes-receiver·silent·total). The set is closed by construction — the
//!   enum IS the registry.
//! - **Name reservation** ([`check_reserved_names`], F6 ruled 2026-07-19):
//!   the method names `display`/`compare`/`next` are reserved under the
//!   brink dialect; an author declaration of any callable or
//!   value-bindable kind is a hard `E113`, not an E035-lineage warning —
//!   a shadowed `display` would make interpolation untrustworthy (F1 routes
//!   both interpolation and `string()` through the display path).
//! - **Impl validation** ([`check_protocol_impls`]): a registered impl's
//!   declared shape is checked against the protocol's signature (`E115`)
//!   and its inferred effect row against the protocol's contract (`E114`,
//!   exceedance-only — the `E103`/`E108`/`E109` posture, riding NS-A2's
//!   `emits`/`tags`/`faults` row dimensions).
//!
//! ## v1 has no impl *spelling*
//!
//! The implementation spelling (attribute vs impl-block) is ⏳ for the
//! code-dialect sitting, and F6 reserves the method names themselves, so
//! the brink dialect cannot honestly host a source-level impl declaration
//! today. [`ProtocolImplDecl`] is therefore a *programmatic* registration
//! surface (the `HostManifest` precedent: project-level metadata supplied
//! beside the source, not invented syntax inside it) — the validation
//! machinery is real and fully exercised, and the future surface spelling
//! lowers into this same table. Consequences, all deliberate:
//!
//! - Structural `display` defaults (field-order rendering, in
//!   `brink-runtime::value_ops`) serve every struct — a user impl would
//!   *override* the default, and nothing can register one from source yet.
//! - `compare` has **no structural default** (§4b: field declaration order
//!   must not silently define semantics), so structs stay not-orderable at
//!   the ordering verbs (`NotOrderable`, since NS-A1) until a compare impl
//!   is registrable — wiring registered compares into the VM's ordering
//!   verbs is Wave A4's scope, alongside `sort`/`sort_by`/`sorted_by`.
//! - `iterate`'s v1 consumer is `for` over the closed builtin iterable set
//!   ([`iterate_element_ty`] is that unification point on the checker
//!   side); user iterables joining the verb ecosystem stays #1090-gated.

use brink_ir::{
    BlockStmt, Content, ContentPart, Diagnostic, DiagnosticCode, ElseBranch, Expr, FileId, HirFile,
    HostManifest, IfStmt, Knot, Name, Param, ResolutionMap, Stmt, StringPart, SymbolIndex,
    TypeExpr,
};

use crate::infer::{EffectRow, Ty};

/// One entry of the closed protocol registry (stdlib-spec §9.6). The enum
/// is the registry: adding an entry is a compiler change by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Protocol {
    /// `fn display(self: T): string`, row ⊆ pure·silent·total. Feeds the
    /// §1.6 display boundary; F1 (ruled 2026-07-19): BOTH interpolation
    /// and the `string()` conversion intrinsic dispatch through this one
    /// path (`brink-runtime::value_ops::stringify` is the runtime seam).
    Display,
    /// `fn compare(a: T, b: T): int`, row ⊆ pure·silent·total. Slots user
    /// types into the §4b ordering doctrine; no structural default.
    Compare,
    /// Pull-shaped iteration: `next(ref Self): Option[T]`, row ⊆
    /// writes-receiver·silent·total, laws attached ("every element once;
    /// `none` terminal and sticky" — property-harness enforced in
    /// `brink-runtime::iter`).
    Iterate,
}

impl Protocol {
    /// Every registry entry, in declaration order.
    pub const ALL: [Protocol; 3] = [Protocol::Display, Protocol::Compare, Protocol::Iterate];

    /// The protocol's reserved method name (F6): the name an impl answers
    /// to, and the name authors may not declare.
    #[must_use]
    pub fn method_name(self) -> &'static str {
        match self {
            Protocol::Display => "display",
            Protocol::Compare => "compare",
            Protocol::Iterate => "next",
        }
    }

    /// Human-readable name of the protocol itself (diagnostics).
    #[must_use]
    pub fn protocol_name(self) -> &'static str {
        match self {
            Protocol::Display => "display",
            Protocol::Compare => "compare",
            Protocol::Iterate => "iterate",
        }
    }

    /// Declared parameter count of the protocol method.
    #[must_use]
    pub fn arity(self) -> usize {
        match self {
            Protocol::Display | Protocol::Iterate => 1,
            Protocol::Compare => 2,
        }
    }

    /// Whether the receiver (first) parameter must be `ref`. Only
    /// `iterate`'s `next` mutates its receiver — that write is a `ref`
    /// param write, invisible to the *global* effect row, which is why one
    /// row bound ([`EffectRow::is_empty`]) serves all three contracts.
    #[must_use]
    pub fn receiver_is_ref(self) -> bool {
        matches!(self, Protocol::Iterate)
    }

    /// The contract phrase used in diagnostics.
    #[must_use]
    pub fn contract_phrase(self) -> &'static str {
        match self {
            Protocol::Display | Protocol::Compare => "pure\u{b7}silent\u{b7}total",
            Protocol::Iterate => "writes-receiver\u{b7}silent\u{b7}total",
        }
    }
}

/// Whether `name` is a reserved protocol method name (F6, ruled
/// 2026-07-19): `display`, `compare`, or `next`.
#[must_use]
pub fn is_reserved_protocol_name(name: &str) -> bool {
    Protocol::ALL.iter().any(|p| p.method_name() == name)
}

/// The element type `for` binds when iterating `iterable` — the checker
/// side of the closed builtin iterable set, unified under the registry
/// (stdlib-spec §9.6: "`for` is the only v1 consumer"). Arrays iterate
/// values; maps iterate **keys** in insertion order
/// (docs/t1b-surface-spec.md §2). Everything else is not iterable v1 —
/// `None` (the caller falls back to `Unknown`; the runtime faults
/// `NotIndexable`, conservatively carried in the `faults` row dimension).
#[must_use]
pub fn iterate_element_ty(iterable: &Ty) -> Option<Ty> {
    match iterable {
        Ty::Array(elem) => Some((**elem).clone()),
        Ty::Map(key, _) => Some((**key).clone()),
        // Ranges iterate their int elements (NS-A5, F7 — `for i in 0..n`;
        // the refinement bit is irrelevant to iteration: an empty range
        // runs zero times, emptiness is load-bearing).
        Ty::Range { .. } => Some(Ty::Int),
        _ => None,
    }
}

/// The value type bound by `for k, v in m`'s second binding (B2, issue
/// #1461, docs/stdlib-spec.md §5/§9's F10 ruling — two-binding map
/// iteration is the pair story `entries()` never got). Only maps have a
/// "value at key"; arrays and ranges iterate a single element with no
/// paired value, so they're not represented here at all — a caller
/// (`infer::body`'s `BlockStmt::For` arm) falls back to `Ty::Unknown` for
/// anything this returns `None` for, the same permissive-at-compile
/// posture [`iterate_element_ty`]'s own callers already rely on.
#[must_use]
pub fn iterate_val_ty(iterable: &Ty) -> Option<Ty> {
    match iterable {
        Ty::Map(_, val) => Some((**val).clone()),
        _ => None,
    }
}

// ─── F6: reserved-name declarations (E113) ──────────────────────────────

/// Check one file for author declarations of the reserved protocol method
/// names (`E113`, hard error). Brink-dialect-only — the caller
/// (`per_file_diagnostics`) gates the call, mirroring the annotation-
/// content precedent: under `strict-ink` there is no protocol registry and
/// vanilla ink identifiers stay untouched.
///
/// Covered declaration kinds: knots/stitches (including functions), their
/// params, `VAR`/`CONST`, `EXTERNAL`, body temps, `for`-loop variables, and
/// a lambda's own `|…|` param row (issue #1773) — every kind that can bind
/// a callable or a value (a fn-value in a temp named `display` would
/// capture call-position dispatch). A lambda's params are checked at any
/// expression depth the lambda literal can be reached from — see
/// [`walk_expr_for_lambdas`]. Deliberately *not* covered: `LIST`/`STRUCT`
/// type names and `LIST` members — type names aren't callable, and list
/// members are value-position-only vocabulary (`next` is plausible
/// narrative domain language); reserving them would over-reach F6's
/// rationale. Also not covered: a `temp`/for-loop variable/`as` binding
/// declared *inside* a lambda's own body — only the lambda's param row
/// itself is checked (asymmetric with `Expr::Fragment`'s block-capture
/// arm, which does check declarations via `walk_stmts`).
#[must_use]
pub fn check_reserved_names(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let mut push = |name: &Name, what: &str| {
            if is_reserved_protocol_name(&name.text) {
                out.push(Diagnostic {
                    file,
                    range: name.range,
                    code: DiagnosticCode::E113,
                    message: format!(
                        "`{}` is a reserved protocol method name (stdlib-spec \u{a7}9.6) and cannot name a {what}",
                        name.text
                    ),
                });
            }
        };
        for var in &hir.variables {
            push(&var.name, "VAR");
            walk_expr_for_lambdas(&var.value, &mut push);
        }
        for cst in &hir.constants {
            push(&cst.name, "CONST");
            walk_expr_for_lambdas(&cst.value, &mut push);
        }
        for ext in &hir.externals {
            push(&ext.name, "EXTERNAL");
        }
        for knot in &hir.knots {
            push(&knot.name, "knot or function");
            walk_params(&knot.params, &mut push);
            walk_stmts(&knot.body.stmts, &mut push);
            for stitch in &knot.stitches {
                push(&stitch.name, "stitch");
                walk_params(&stitch.params, &mut push);
                walk_stmts(&stitch.body.stmts, &mut push);
            }
        }
        walk_stmts(&hir.root_content.stmts, &mut push);
    }
    out
}

fn walk_params(params: &[Param], push: &mut impl FnMut(&Name, &str)) {
    for p in params {
        push(&p.name, "parameter");
    }
}

/// Find every `Expr::Lambda` reachable from `expr` — including nested
/// arbitrarily deep inside another expression (`f(|display| display)`), and
/// nested inside the lambda's *own* body (`|x| { let f = |display| display;
/// f() }`, via [`brink_ir::LambdaBody::all_exprs`], the "does this construct
/// occur anywhere inside" helper built for exactly this shape — issue
/// #1764) — and, for each one found, push its params exactly like
/// [`walk_params`] does for a top-level fn/knot/stitch param (issue #1773:
/// same reserved-name rule, same declaration-site treatment, regardless of
/// which kind of param row it sits on).
///
/// Mirrors the shape of `hir::visit::walk_expr` / this crate's other
/// hand-rolled expression collectors (e.g. `comparator_contract::
/// collect_expr`) rather than introducing a third — every `Expr` variant
/// that can hold a nested expression is descended; the only ones skipped
/// (`Int`/`Float`/`Bool`/`Null`/`Path`/`DivertTarget`/`ListLiteral`) are
/// leaves that can never contain a lambda literal.
fn walk_expr_for_lambdas(expr: &Expr, push: &mut impl FnMut(&Name, &str)) {
    match expr {
        Expr::Lambda(l) => {
            walk_params(&l.params, push);
            for e in l.body.all_exprs() {
                walk_expr_for_lambdas(e, push);
            }
        }
        Expr::Call(_path, args) => {
            for arg in args {
                walk_expr_for_lambdas(arg, push);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => walk_expr_for_lambdas(inner, push),
        Expr::Infix(ie) => {
            walk_expr_for_lambdas(&ie.lhs, push);
            walk_expr_for_lambdas(&ie.rhs, push);
        }
        Expr::String(s) => {
            for part in &s.parts {
                if let StringPart::Interpolation(e) = part {
                    walk_expr_for_lambdas(e, push);
                }
            }
        }
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                walk_expr_for_lambdas(e, push);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, v) in &m.entries {
                walk_expr_for_lambdas(k, push);
                walk_expr_for_lambdas(v, push);
            }
        }
        Expr::Index(idx) => {
            walk_expr_for_lambdas(&idx.base, push);
            walk_expr_for_lambdas(&idx.index, push);
        }
        Expr::StructLiteral(sl) => {
            for (_name, val) in &sl.fields {
                walk_expr_for_lambdas(val, push);
            }
        }
        Expr::FieldAccess(fa) => walk_expr_for_lambdas(&fa.base, push),
        // T1c `#fn(target, args…)`: the target is a static path, not an
        // `Expr` child (same shape as `Call`'s path) — only bound args
        // descend.
        Expr::FnLiteral(fl) => {
            for arg in &fl.args {
                walk_expr_for_lambdas(arg, push);
            }
        }
        Expr::RefArg(ra) => walk_expr_for_lambdas(&ra.operand, push),
        Expr::Range(r) => {
            walk_expr_for_lambdas(&r.start, push);
            walk_expr_for_lambdas(&r.end, push);
        }
        // Block-capture fragment (issue #1839): not constructible from
        // surface syntax, but it embeds real `Stmt`s the ordinary weave walk
        // already knows how to visit — reuse `walk_stmts` rather than
        // growing a second statement vocabulary here.
        Expr::Fragment(stmts) => walk_stmts(stmts, push),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => {}
    }
}

/// Recursive walk over weave-level statements, visiting every declaration
/// site a temp or loop variable can hide in (the `strict.rs`
/// `collect_temps_*` walk, extended to choice bodies and continuations) —
/// plus, additively, every position an `Expr` can sit in, so a lambda
/// literal reachable from any of them gets its params checked too (issue
/// #1773).
fn walk_stmts(stmts: &[Stmt], push: &mut impl FnMut(&Name, &str)) {
    for stmt in stmts {
        match stmt {
            Stmt::TempDecl(t) => {
                push(&t.name, "temp");
                if let Some(v) = &t.value {
                    walk_expr_for_lambdas(v, push);
                }
            }
            Stmt::Content(c) => walk_content(c, push),
            Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    // Guard-`as` binding (issue #1508) — same treatment as
                    // `Stmt::Conditional`'s `branch.binding` a few arms
                    // down: it's a declaration site a temp/loop variable
                    // can hide behind, per this function's own doc.
                    if let Some(binding) = &choice.binding {
                        push(binding, "binding");
                    }
                    if let Some(cond) = &choice.condition {
                        walk_expr_for_lambdas(cond, push);
                    }
                    // Native choice labels (`* Gold: {fmt(...)}`) lower
                    // interpolations into these three `Content` regions,
                    // not into `choice.body.stmts` — issue #1773 review: a
                    // lambda param reserved-name shadow in a choice label
                    // was still unreached without this.
                    for c in [
                        &choice.start_content,
                        &choice.bracket_content,
                        &choice.inner_content,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        walk_content(c, push);
                    }
                    walk_stmts(&choice.body.stmts, push);
                }
                walk_stmts(&cs.continuation.stmts, push);
            }
            Stmt::LabeledBlock(b) => walk_stmts(&b.stmts, push),
            Stmt::Conditional(c) => {
                for branch in &c.branches {
                    if let Some(binding) = &branch.binding {
                        push(binding, "binding");
                    }
                    if let Some(cond) = &branch.condition {
                        walk_expr_for_lambdas(cond, push);
                    }
                    walk_stmts(&branch.body.stmts, push);
                }
            }
            Stmt::Sequence(s) => {
                for branch in &s.branches {
                    walk_stmts(&branch.body.stmts, push);
                }
            }
            Stmt::LogicBlock(lb) => walk_block_stmts(&lb.stmts, push),
            Stmt::Divert(d) => {
                for arg in &d.target.args {
                    walk_expr_for_lambdas(arg, push);
                }
            }
            Stmt::TunnelCall(tc) => {
                for target in &tc.targets {
                    for arg in &target.args {
                        walk_expr_for_lambdas(arg, push);
                    }
                }
            }
            Stmt::ThreadStart(ts) => {
                for arg in &ts.target.args {
                    walk_expr_for_lambdas(arg, push);
                }
            }
            Stmt::Assignment(a) => {
                walk_expr_for_lambdas(&a.target, push);
                walk_expr_for_lambdas(&a.value, push);
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    walk_expr_for_lambdas(v, push);
                }
                for arg in &r.onwards_args {
                    walk_expr_for_lambdas(arg, push);
                }
            }
            Stmt::ExprStmt(e) | Stmt::AttachElement(e) => walk_expr_for_lambdas(e, push),
            Stmt::Await(a) => {
                if let Some(cond) = &a.condition {
                    walk_expr_for_lambdas(cond, push);
                }
            }
            Stmt::EndOfLine | Stmt::EndElementRun => {}
        }
    }
}

fn walk_content(content: &Content, push: &mut impl FnMut(&Name, &str)) {
    for part in &content.parts {
        walk_content_part(part, push);
    }
}

fn walk_content_part(part: &ContentPart, push: &mut impl FnMut(&Name, &str)) {
    match part {
        ContentPart::InlineConditional(c) => {
            for branch in &c.branches {
                if let Some(cond) = &branch.condition {
                    walk_expr_for_lambdas(cond, push);
                }
                walk_stmts(&branch.body.stmts, push);
            }
        }
        ContentPart::InlineSequence(s) => {
            for branch in &s.branches {
                walk_stmts(&branch.body.stmts, push);
            }
        }
        // A span can nest a conditional/sequence (§4.3), each with its own
        // statement bodies to walk.
        ContentPart::Span(span) => {
            for child in &span.children {
                walk_content_part(child, push);
            }
        }
        ContentPart::Interpolation(e) => walk_expr_for_lambdas(e, push),
        ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
    }
}

/// Logic-block statements (`~ { … }`): temps, `for`-loop variables, and
/// every nested block shape — plus, additively, every `Expr`-bearing
/// position (issue #1773; see [`walk_stmts`]'s doc).
fn walk_block_stmts(stmts: &[BlockStmt], push: &mut impl FnMut(&Name, &str)) {
    for stmt in stmts {
        match stmt {
            BlockStmt::TempDecl(t) => {
                push(&t.name, "temp");
                if let Some(v) = &t.value {
                    walk_expr_for_lambdas(v, push);
                }
            }
            BlockStmt::If(i) => walk_if(i, push),
            BlockStmt::While(w) => {
                if let Some(binding) = &w.binding {
                    push(binding, "binding");
                }
                walk_expr_for_lambdas(&w.condition, push);
                walk_block_stmts(&w.body, push);
            }
            BlockStmt::For(f) => {
                push(&f.var_name, "for-loop variable");
                if let Some(val_name) = &f.val_name {
                    push(val_name, "for-loop variable");
                }
                walk_expr_for_lambdas(&f.iterable, push);
                walk_block_stmts(&f.body, push);
            }
            BlockStmt::Assignment(a) => {
                walk_expr_for_lambdas(&a.target, push);
                walk_expr_for_lambdas(&a.value, push);
            }
            BlockStmt::Return(r) => {
                if let Some(v) = &r.value {
                    walk_expr_for_lambdas(v, push);
                }
                for arg in &r.onwards_args {
                    walk_expr_for_lambdas(arg, push);
                }
            }
            BlockStmt::ExprStmt(e) => walk_expr_for_lambdas(e, push),
            BlockStmt::Await(a) => {
                if let Some(cond) = &a.condition {
                    walk_expr_for_lambdas(cond, push);
                }
            }
            BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
        }
    }
}

fn walk_if(i: &IfStmt, push: &mut impl FnMut(&Name, &str)) {
    // B1b (issue #1475): the `as` binding declares a name, so it is a
    // reserved-protocol-name site exactly like a `temp` or a `for` variable.
    if let Some(binding) = &i.binding {
        push(binding, "binding");
    }
    walk_expr_for_lambdas(&i.condition, push);
    walk_block_stmts(&i.body, push);
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => walk_if(inner, push),
        Some(ElseBranch::Else(stmts)) => walk_block_stmts(stmts, push),
        None => {}
    }
}

// ─── Impl registration + validation (E114/E115) ─────────────────────────

/// One protocol impl registration: "`function` implements `protocol` for
/// the declared `STRUCT` named `type_name`". Programmatic v1 (see the
/// module doc) — the future source spelling lowers into this same shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolImplDecl {
    pub protocol: Protocol,
    /// The declared `STRUCT` name the impl attaches to.
    pub type_name: String,
    /// The declared function (knot with `is_function`) that implements the
    /// protocol method.
    pub function: String,
}

/// Validate registered protocol impls: shape against the protocol's
/// signature (`E115`) and inferred effect row against the protocol's
/// contract (`E114`). Returns all diagnostics; an impl that fails a shape
/// check is not row-checked (the `E102`-before-`E103` posture — don't
/// stack a second diagnostic on an impl that can't even be resolved).
///
/// Effect rows are computed via [`crate::infer::effects_project`] only
/// when at least one impl passes shape validation — an impl-free project
/// (today: every project) never pays for effect inference here.
///
/// Diagnostics carry the impl function's declaration range where
/// resolvable, else the file-start range of the first file (registration
/// is not a source construct yet, so there is no registration site to
/// point at).
#[must_use]
pub fn check_protocol_impls(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    host_manifest: Option<&HostManifest>,
    impls: &[ProtocolImplDecl],
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if impls.is_empty() {
        return out;
    }

    let struct_names: std::collections::BTreeSet<&str> = files
        .iter()
        .flat_map(|(_, hir)| hir.structs.iter())
        .map(|s| s.name.text.as_str())
        .collect();

    // Shape-validated impls, with the declaring knot located for row
    // lookup and diagnostic placement.
    let mut checked: Vec<(&ProtocolImplDecl, FileId, &Knot)> = Vec::new();
    let mut seen: std::collections::BTreeSet<(Protocol, &str)> = std::collections::BTreeSet::new();

    for decl in impls {
        let Some((file, knot)) = find_function(files, &decl.function) else {
            out.push(registration_error(
                files,
                format!(
                    "protocol impl `{}` for `{}`: `{}` is not a declared function",
                    decl.protocol.protocol_name(),
                    decl.type_name,
                    decl.function
                ),
            ));
            continue;
        };
        let at = |message: String| Diagnostic {
            file,
            range: knot.name.range,
            code: DiagnosticCode::E115,
            message,
        };

        // NS-A8 (docs/tower-mini-spec.md T4, issue #1114): tower kinds can
        // NEVER implement registry protocols — `compare` would contradict
        // the ruled not-orderable posture, and `display`/`iterate` would
        // shadow compiler-owned behavior. Checked before (and regardless
        // of) the STRUCT lookup, so a user STRUCT named `vec3` cannot
        // smuggle an impl in under a tower name — tower type names are
        // global, like `int`.
        if crate::infer::TowerTy::from_name(&decl.type_name).is_some() {
            out.push(Diagnostic {
                file,
                range: knot.name.range,
                code: DiagnosticCode::E118,
                message: format!(
                    "protocol impl `{}` for `{}`: numeric-tower kinds are compiler-known and cannot implement registry protocols{}",
                    decl.protocol.protocol_name(),
                    decl.type_name,
                    if decl.protocol == Protocol::Compare {
                        " (tower values are not orderable — tower-mini-spec T4)"
                    } else {
                        ""
                    }
                ),
            });
            continue;
        }

        if !struct_names.contains(decl.type_name.as_str()) {
            out.push(at(format!(
                "protocol impl `{}` for `{}`: the type is not a declared STRUCT (only user struct types may implement registry protocols)",
                decl.protocol.protocol_name(),
                decl.type_name
            )));
            continue;
        }
        if !seen.insert((decl.protocol, decl.type_name.as_str())) {
            out.push(at(format!(
                "duplicate protocol impl: `{}` for `{}` is already registered",
                decl.protocol.protocol_name(),
                decl.type_name
            )));
            continue;
        }
        if let Some(message) = shape_error(decl, knot) {
            out.push(at(message));
            continue;
        }
        checked.push((decl, file, knot));
    }

    if checked.is_empty() {
        return out;
    }

    // Contract enforcement over the inferred rows (NS-A2 substrate). One
    // whole-project inference serves every impl, the
    // `whole_project_diagnostics` effects posture.
    let rows = crate::infer::effects_project(files, index, resolutions, host_manifest);
    for (decl, file, knot) in checked {
        let Some(def_id) = index.by_name.get(&decl.function).and_then(|ids| {
            ids.iter()
                .copied()
                .find(|id| index.symbols.get(id).is_some_and(|info| info.file == file))
        }) else {
            continue;
        };
        let Some(row) = rows.get(&def_id) else {
            continue;
        };
        if let Some(message) = contract_error(decl.protocol, &decl.type_name, row, index) {
            out.push(Diagnostic {
                file,
                range: knot.name.range,
                code: DiagnosticCode::E114,
                message,
            });
        }
    }
    out
}

/// Locate a declared function knot by name across the project's files.
fn find_function<'a>(files: &[(FileId, &'a HirFile)], name: &str) -> Option<(FileId, &'a Knot)> {
    files.iter().find_map(|&(file, hir)| {
        hir.knots
            .iter()
            .find(|k| k.is_function && k.name.text == name)
            .map(|k| (file, k))
    })
}

/// Signature-shape validation against the protocol's declared form. Arity
/// and `ref`-ness are structural (always checkable); type annotations are
/// checked only where present — an unannotated param is the gradual
/// posture, accepted (TM-2's annotation-wins/inference-fills split).
fn shape_error(decl: &ProtocolImplDecl, knot: &Knot) -> Option<String> {
    let proto = decl.protocol;
    if knot.params.len() != proto.arity() {
        return Some(format!(
            "protocol impl `{}` for `{}`: `{}` takes {} parameter(s), but the protocol method `{}` declares {}",
            proto.protocol_name(),
            decl.type_name,
            knot.name.text,
            knot.params.len(),
            proto.method_name(),
            proto.arity()
        ));
    }
    for (i, param) in knot.params.iter().enumerate() {
        let want_ref = i == 0 && proto.receiver_is_ref();
        if param.is_ref != want_ref {
            return Some(format!(
                "protocol impl `{}` for `{}`: parameter `{}` must {} `ref` (the protocol method is `{}`)",
                proto.protocol_name(),
                decl.type_name,
                param.name.text,
                if want_ref { "be" } else { "not be" },
                signature_phrase(proto),
            ));
        }
        // Receiver params (all of display's/next's, both of compare's)
        // must be the implementing type where annotated.
        if let Some(TypeExpr::Named { name, .. }) = &param.annotation
            && name != &decl.type_name
        {
            return Some(format!(
                "protocol impl `{}` for `{}`: parameter `{}` is annotated `{}`, but the receiver of a protocol impl must be the implementing type",
                proto.protocol_name(),
                decl.type_name,
                param.name.text,
                name
            ));
        }
    }
    let want_return = match proto {
        Protocol::Display => Some("string"),
        Protocol::Compare => Some("int"),
        // `next` returns `Option[T]` — not expressible in the TM-2
        // annotation grammar yet, so no return check v1.
        Protocol::Iterate => None,
    };
    if let (Some(want), Some(TypeExpr::Named { name, .. })) = (want_return, &knot.return_type)
        && name != want
    {
        return Some(format!(
            "protocol impl `{}` for `{}`: return type is annotated `{}`, but `{}` returns `{}`",
            proto.protocol_name(),
            decl.type_name,
            name,
            signature_phrase(proto),
            want
        ));
    }
    None
}

fn signature_phrase(proto: Protocol) -> &'static str {
    match proto {
        Protocol::Display => "display(self: T): string",
        Protocol::Compare => "compare(a: T, b: T): int",
        Protocol::Iterate => "next(ref self): Option[T]",
    }
}

/// The per-protocol effect contract (stdlib-spec §9.6), enforced over the
/// inferred row. Every v1 contract bounds the **global** row at empty
/// (see [`Protocol::receiver_is_ref`] for why `next`'s receiver write is
/// invisible here): no global reads — `display` runs at deferred
/// transcript-resolution time, after story state may have moved on, so a
/// state-reading impl would render differently at read time than at emit
/// time — no writes, no external calls, no emits, no tags, no faults, and
/// never opaque.
fn contract_error(
    proto: Protocol,
    type_name: &str,
    row: &EffectRow,
    index: &SymbolIndex,
) -> Option<String> {
    // Bool-granularity carve-out (v1): `next`'s mandatory `ref` receiver
    // makes NS-A2's inference mark EVERY iterate impl as conservatively
    // faulting (a `ref` param's deref can raise `ProjectionInvalidated`,
    // charged to the callee — `infer::body`'s ref-param rule), so
    // enforcing the `total` leg would reject every possible impl. Until
    // the reserved per-fault-kind row refinement can tell the sanctioned
    // receiver-deref fault from a real domain fault, iterate's contract
    // skips the `faults` dimension — under-enforcement, chosen over a
    // dead protocol, and called out in the registry docs.
    //
    // NS-A4 / **F29(a)** (ruled by delegation 2026-07-19, stdlib-spec §4b
    // — the symmetric carve-out, the post-A3 composition audit's C1/C2
    // finding): `display`/`compare` are judged on the **refined** faults
    // bit, not the conservative one. An impl whose row is provably total
    // — every charge site discharged by local type evidence
    // (`EffectRow::faults_refined`, invariant `refined → conservative`) —
    // does NOT inherit the conservative bit; the conservative union
    // applies only when the impl's own row is opaque (already a contract
    // violation above) or genuinely fault-bearing.
    let faults_exceed = row.faults_refined && !matches!(proto, Protocol::Iterate);
    if !row.is_pessimal()
        && row.reads.is_empty()
        && row.writes.is_empty()
        && row.calls.is_empty()
        && !row.emits
        && !row.tags
        && !faults_exceed
    {
        return None;
    }
    let mut parts = Vec::new();
    if row.is_pessimal() {
        parts.push(
            "calls through a function value or unresolved callee (unbounded row)".to_string(),
        );
    }
    let name_of = |id: &brink_format::DefinitionId| {
        index
            .symbols
            .get(id)
            .map_or_else(|| format!("{id:?}"), |info| info.name.clone())
    };
    if !row.reads.is_empty() {
        let names: Vec<String> = row.reads.iter().map(name_of).collect();
        parts.push(format!("reads {}", names.join(", ")));
    }
    if !row.writes.is_empty() {
        let names: Vec<String> = row.writes.iter().map(name_of).collect();
        parts.push(format!("writes {}", names.join(", ")));
    }
    if !row.calls.is_empty() {
        let names: Vec<String> = row.calls.iter().cloned().collect();
        parts.push(format!("calls {}", names.join(", ")));
    }
    if row.emits {
        parts.push("emits content".to_string());
    }
    if row.tags {
        parts.push("touches the tag channel".to_string());
    }
    if faults_exceed {
        parts.push("can raise a turn-terminating fault".to_string());
    }
    Some(format!(
        "protocol impl `{}` for `{type_name}` exceeds the {} contract: {}",
        proto.protocol_name(),
        proto.contract_phrase(),
        parts.join("; ")
    ))
}

fn registration_error(files: &[(FileId, &HirFile)], message: String) -> Diagnostic {
    Diagnostic {
        file: files.first().map_or(FileId(0), |&(f, _)| f),
        range: rowan::TextRange::empty(0.into()),
        code: DiagnosticCode::E115,
        message,
    }
}

#[cfg(test)]
mod tests {
    use brink_ir::SymbolManifest;
    use brink_ir::hir::HirFile;

    use super::*;

    fn lower(src: &str) -> (HirFile, SymbolManifest) {
        let parsed = brink_syntax::parse(src);
        let tree = parsed.tree();
        let (hir, manifest, diags) = brink_ir::hir::lower(FileId(0), &tree);
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        (hir, manifest)
    }

    fn reserved_diags(src: &str) -> Vec<Diagnostic> {
        let (hir, _manifest) = lower(src);
        check_reserved_names(&[(FileId(0), &hir)])
    }

    /// Native-frontend twin of [`reserved_diags`] — `Expr::Lambda` is minted
    /// only by `hir::lower_native` (the ink/brink-compat frontend has no
    /// lambda grammar), so a fixture exercising a lambda param must go
    /// through the native parser, mirroring `coalesce.rs`/
    /// `comparator_contract.rs`'s `build_native` test helpers.
    fn reserved_diags_native(src: &str) -> Vec<Diagnostic> {
        let parse = brink_syntax_native::parse(src);
        assert!(
            parse.errors().is_empty(),
            "fixture must parse cleanly: {:?}",
            parse.errors()
        );
        let tree = parse.tree();
        let (hir, _manifest, diags) = brink_ir::hir::lower_native::lower(FileId(0), &tree);
        assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
        check_reserved_names(&[(FileId(0), &hir)])
    }

    fn impl_diags(src: &str, impls: &[ProtocolImplDecl]) -> Vec<Diagnostic> {
        let (hir, manifest) = lower(src);
        let result = crate::analyze(&[(FileId(0), &hir, &manifest)]);
        check_protocol_impls(
            &[(FileId(0), &hir)],
            &result.index,
            &result.resolutions,
            None,
            impls,
        )
    }

    fn decl(protocol: Protocol, type_name: &str, function: &str) -> ProtocolImplDecl {
        ProtocolImplDecl {
            protocol,
            type_name: type_name.to_string(),
            function: function.to_string(),
        }
    }

    const POINT: &str = "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n";

    // ─── E113: reserved names (F6) ──────────────────────────────────

    #[test]
    fn knot_named_display_is_reserved() {
        let diags = reserved_diags("== display ==\nHello.\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn function_named_compare_is_reserved() {
        let diags = reserved_diags("=== function compare(a, b) ===\n~ return 0\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn stitch_named_next_is_reserved() {
        let diags = reserved_diags("== knot ==\n= next\nHello.\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn var_const_external_named_reserved() {
        let diags = reserved_diags("VAR display = 1\nCONST compare = 2\nEXTERNAL next(x)\n");
        assert_eq!(diags.len(), 3, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E113));
    }

    #[test]
    fn param_named_display_is_reserved() {
        let diags = reserved_diags("=== function f(display) ===\n~ return display\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn temp_and_for_var_in_logic_block_are_reserved() {
        let src = "== k ==\n~ {\n    temp next = 1\n    for display in #[1, 2] {\n        next = next + display\n    }\n}\n-> DONE\n";
        let diags = reserved_diags(src);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E113));
    }

    #[test]
    fn weave_level_temp_named_next_is_reserved() {
        let diags = reserved_diags("== k ==\n~ temp next = 1\n{next}\n-> DONE\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn list_members_and_type_names_are_not_reserved() {
        // Deliberate carve-outs (see `check_reserved_names`'s doc): LIST
        // members are value-position narrative vocabulary; LIST/STRUCT
        // *type* names aren't callable.
        let diags = reserved_diags("LIST steps = intro, next, outro\n");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn lambda_param_named_display_is_reserved() {
        // Issue #1773: same shadowing shape as `param_named_display_is_reserved`
        // (a top-level fn/knot/stitch param), but the binding site is a
        // lambda's own `|…|` param row instead. Same name, same file — must
        // get the identical E113 answer.
        let diags = reserved_diags_native("var f = |display| display\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn lambda_param_named_display_in_choice_label_is_reserved() {
        // Issue #1773 review finding: native choice labels (`* Gold: {…}`)
        // lower interpolations into `choice.start_content` /
        // `bracket_content` / `inner_content`, NOT into `choice.body.stmts`
        // — so a lambda param shadow reachable only through one of those
        // three `Content` regions was still unreached without walking them.
        let diags =
            reserved_diags_native("flow f() {\n  {?\n    * Gold: {fmt(|display| 0)}\n  }\n}\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E113);
    }

    #[test]
    fn ordinary_names_stay_clean() {
        let diags = reserved_diags(
            "VAR score = 1\n== k ==\n~ temp shown = score\n{shown}\n-> DONE\n=== function render(p) ===\n~ return \"x\"\n",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ─── E115: impl shape validation ────────────────────────────────

    #[test]
    fn well_formed_display_impl_is_clean() {
        let src = format!("{POINT}=== function render(p: Point): string ===\n~ return \"P\"\n");
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unknown_function_is_e115() {
        let diags = impl_diags(POINT, &[decl(Protocol::Display, "Point", "nope")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("not a declared function"));
    }

    #[test]
    fn non_struct_type_is_e115() {
        let src = "=== function render(p) ===\n~ return \"x\"\n";
        let diags = impl_diags(src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("not a declared STRUCT"));
    }

    #[test]
    fn wrong_arity_is_e115() {
        let src = format!("{POINT}=== function render(p, extra) ===\n~ return \"x\"\n");
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("parameter"));
    }

    #[test]
    fn display_receiver_must_not_be_ref() {
        let src = format!("{POINT}=== function render(ref p) ===\n~ return \"x\"\n");
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
    }

    #[test]
    fn next_receiver_must_be_ref() {
        let src = format!("{POINT}=== function step(p) ===\n~ return 0\n");
        let diags = impl_diags(&src, &[decl(Protocol::Iterate, "Point", "step")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("ref"));
    }

    #[test]
    fn contradicting_param_annotation_is_e115() {
        let src = format!("{POINT}=== function render(p: int) ===\n~ return \"x\"\n");
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("annotated"));
    }

    #[test]
    fn contradicting_return_annotation_is_e115() {
        let src =
            format!("{POINT}=== function cmp(a: Point, b: Point): string ===\n~ return \"x\"\n");
        let diags = impl_diags(&src, &[decl(Protocol::Compare, "Point", "cmp")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("return"));
    }

    #[test]
    fn duplicate_registration_is_e115() {
        let src = format!(
            "{POINT}=== function render(p) ===\n~ return \"x\"\n=== function render2(p) ===\n~ return \"y\"\n"
        );
        let diags = impl_diags(
            &src,
            &[
                decl(Protocol::Display, "Point", "render"),
                decl(Protocol::Display, "Point", "render2"),
            ],
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E115);
        assert!(diags[0].message.contains("duplicate"));
    }

    // ─── E118: tower kinds can never implement protocols (NS-A8) ────

    #[test]
    fn compare_for_tower_kind_is_e118() {
        // T4 (docs/tower-mini-spec.md): the tower is NOT orderable —
        // registering `compare` for a tower kind must be impossible.
        let src = "=== function cmp(a, b) ===\n~ return 0\n";
        for kind in ["vec2", "vec3", "vec4", "quat", "mat2", "mat3", "mat4"] {
            let diags = impl_diags(src, &[decl(Protocol::Compare, kind, "cmp")]);
            assert_eq!(diags.len(), 1, "{kind}: {diags:?}");
            assert_eq!(diags[0].code, DiagnosticCode::E118, "{kind}");
            assert!(diags[0].message.contains("not orderable"), "{kind}");
        }
    }

    #[test]
    fn display_and_iterate_for_tower_kind_are_e118() {
        let src = "=== function render(p) ===\n~ return \"x\"\n";
        for proto in [Protocol::Display, Protocol::Iterate] {
            let diags = impl_diags(src, &[decl(proto, "vec3", "render")]);
            assert_eq!(diags.len(), 1, "{proto:?}: {diags:?}");
            assert_eq!(diags[0].code, DiagnosticCode::E118, "{proto:?}");
        }
    }

    #[test]
    fn tower_rejection_wins_over_a_shadowing_struct() {
        // A user STRUCT named `vec3` cannot smuggle a compare impl in
        // under the tower name — tower type names are global, like `int`.
        let src = "STRUCT vec3 = #{\n    v: float,\n}\n=== function cmp(a, b) ===\n~ return 0\n";
        let diags = impl_diags(src, &[decl(Protocol::Compare, "vec3", "cmp")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E118);
    }

    // ─── E114: effect-contract enforcement (needs NS-A2's rows) ─────

    #[test]
    fn global_write_exceeds_display_contract() {
        let src = format!(
            "{POINT}VAR seen = 0\n=== function render(p) ===\n~ seen = seen + 1\n~ return \"x\"\n"
        );
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
        assert!(
            diags[0].message.contains("writes seen"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn global_read_exceeds_display_contract() {
        // Display runs at deferred transcript-resolution time — a
        // state-reading impl would render differently at read time than
        // at emit time, so reads are outside the contract too.
        let src = format!("{POINT}VAR mood = 1\n=== function render(p) ===\n~ return mood\n");
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
        assert!(
            diags[0].message.contains("reads mood"),
            "{}",
            diags[0].message
        );
    }

    #[test]
    fn emitting_impl_exceeds_silent() {
        let src = format!("{POINT}=== function render(p) ===\nLoud line.\n~ return \"x\"\n");
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "render")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
        assert!(diags[0].message.contains("emits"), "{}", diags[0].message);
    }

    #[test]
    fn faulting_impl_exceeds_total() {
        // `min` over a *float* array carries the §4b ordering fault
        // unconditionally (mode-independent rows: dev NaN-fault / prod
        // pinned order — the checker doesn't know modes exist), so the
        // charge is NOT discharged (F29's carve-out only covers provably
        // NaN-free element types) and breaks the `total` leg.
        let src = format!(
            "{POINT}=== function cmp(a, b) ===\n~ temp lowest = min(#[1.0, 2.0])\n~ return 0\n"
        );
        let diags = impl_diags(&src, &[decl(Protocol::Compare, "Point", "cmp")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
        assert!(diags[0].message.contains("fault"), "{}", diags[0].message);
    }

    // ─── F29(a) — the symmetric faults carve-out (ruled by delegation
    // 2026-07-19, stdlib-spec §4b): a display/compare impl whose inferred
    // row is PROVABLY total does not inherit the conservative faults bit;
    // the conservative union applies only when the impl's own row is
    // opaque or genuinely fault-bearing. ─────────────────────────────────

    #[test]
    fn f29_provably_total_impl_is_not_rejected_for_conservative_faults() {
        // `min(#[1, 2])`/`len(#[1, 2])` carry the *conservative* faults
        // bit (bool v1 — the wrong-type/NotOrderable paths exist in
        // general) but are provably total here: int-array arguments
        // discharge the charge (F29), so the impl's refined row is
        // faults-free and E114 must NOT fire.
        let src = format!(
            "{POINT}=== function cmp(a, b) ===\n~ temp lowest = min(#[1, 2])\n~ temp n = len(#[1, 2])\n~ return 0\n"
        );
        let diags = impl_diags(&src, &[decl(Protocol::Compare, "Point", "cmp")]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn f29_opaque_impl_keeps_the_conservative_union() {
        // A call through a function value escapes the static call graph —
        // the row is opaque, and F29's carve-out explicitly does NOT
        // apply ("the conservative union applies only when the impl's own
        // row is opaque or fault-bearing"). E114 names the opaque escape.
        let src = format!(
            "{POINT}=== function helper() ===\n~ return 1\n\n=== function shape(self) ===\n~ temp f = #fn(helper)\n~ temp n = call(f)\n~ return \"p\"\n"
        );
        let diags = impl_diags(&src, &[decl(Protocol::Display, "Point", "shape")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
    }

    #[test]
    fn f29_value_dependent_fault_still_rejects() {
        // Indexing is value-dependent (OOB) — never discharged; the
        // refined bit stays set and the contract still rejects.
        let src = format!(
            "{POINT}=== function cmp(a, b) ===\n~ temp arr = #[1, 2]\n~ temp x = arr[5]\n~ return 0\n"
        );
        let diags = impl_diags(&src, &[decl(Protocol::Compare, "Point", "cmp")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
        assert!(diags[0].message.contains("fault"), "{}", diags[0].message);
    }

    #[test]
    fn pure_compare_impl_is_clean() {
        let src = format!("{POINT}=== function cmp(a: Point, b: Point): int ===\n~ return 0\n");
        let diags = impl_diags(&src, &[decl(Protocol::Compare, "Point", "cmp")]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn pure_next_impl_with_ref_receiver_is_clean() {
        // The `ref` receiver marks the row as conservatively faulting
        // (`ProjectionInvalidated` — infer::body's ref-param rule); the
        // iterate contract's bool-granularity carve-out must not reject
        // the only shape an impl can legally have.
        let src =
            format!("{POINT}=== function step(ref p) ===\n~ p.x = p.x + 1.0\n~ return some(p.x)\n");
        let diags = impl_diags(&src, &[decl(Protocol::Iterate, "Point", "step")]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn next_impl_writing_a_global_still_exceeds() {
        // The faults carve-out is faults-only: a global write inside a
        // `next` impl is outside writes-receiver·silent·total regardless.
        let src = format!(
            "{POINT}VAR steps = 0\n=== function step(ref p) ===\n~ steps = steps + 1\n~ return some(p.x)\n"
        );
        let diags = impl_diags(&src, &[decl(Protocol::Iterate, "Point", "step")]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E114);
        assert!(
            diags[0].message.contains("writes steps"),
            "{}",
            diags[0].message
        );
    }

    // ─── iterate: the closed builtin iterable set ───────────────────

    #[test]
    fn iterate_element_types_cover_the_closed_set() {
        assert_eq!(
            iterate_element_ty(&Ty::Array(Box::new(Ty::Int))),
            Some(Ty::Int)
        );
        assert_eq!(
            iterate_element_ty(&Ty::Map(Box::new(Ty::String), Box::new(Ty::Int))),
            Some(Ty::String),
            "maps iterate keys"
        );
        assert_eq!(iterate_element_ty(&Ty::Int), None);
        assert_eq!(iterate_element_ty(&Ty::String), None);
        assert_eq!(iterate_element_ty(&Ty::List("Mood".into())), None);
    }
}
