//! The pure-callback **contract gate** (E119, `docs/stdlib-spec.md` §4/§4b),
//! built on the effects machinery (#859) in the `await_purity` (E105) pass
//! shape.
//!
//! Two verb families are gated, because one sitting ruled both:
//!
//! - NS-A4's `sort_by`/`sorted_by` comparators (issue #1110, §4b);
//! - the fn-value verb layer's pure quartet `map`/`filter`/`fold`/
//!   `filter_map` (issue #1679, §4) — pure·silent-required by the
//!   2026-07-18 ruling, which is what dissolves the eager/lazy question:
//!   with a pure callback, stage interleaving is unobservable by
//!   construction. The effectful spellings `each`/`map_each` (issue #1679
//!   slice 2) exist precisely so authors have a legal home for the effects
//!   this gate rejects, and are deliberately NOT gated here.
//!
//! [`callback_arg_index`] is the single roster; adding a verb there is all
//! that a new pure-callback verb needs.
//!
//! §4b (RULED 2026-07-18): "the comparator falls under the trio's
//! pure·silent rule plus the consistent-total-order LAW." A comparator that
//! writes a global, calls a host external, emits content, or touches the
//! tag channel makes *sorting itself* observable — and the number and order
//! of comparator invocations is an implementation detail of the sort, so
//! any such effect is inherently order-of-evaluation-dependent. A global
//! *read* is likewise banned by the trio rule (the same bound the E114
//! protocol contract enforces for registry `compare` impls): the order must
//! be a pure function of the two comparands.
//!
//! **Exceedance-only** (the E103/E108/E114 posture): the gate fires only on
//! a *proven* violation — a callback written as an inline `#fn(target)`
//! literal (ink/brink), or its native bare-name equivalent (issue #1887,
//! #1862's 2026-08-01 ruling: `map(items, double)`, no sigil) — whose
//! statically-named target's inferred row shows the effect.
//! A callback that arrives as an opaque value (a variable, a parameter, a
//! `bind(…)` result) is not provable here and passes; the VM's output
//! isolation and the `ComparatorEscaped`/fault machinery are the runtime
//! residual, exactly as gradual typing intends. Faults in the callback
//! are NOT flagged — the contract is pure·silent, deliberately not total
//! (F14: `sort_by`'s row is `⊕cmp` + the inconsistency fault; a comparator
//! may fault, and that fault is honest — §4 says the same of the trio).
//!
//! **The known hole (#1679/#1680):** an opaque function *value* is
//! unjudgeable here — "pure-required" is enforced only at the one site
//! where the callback's origin is syntactically visible. `Ty::Fn` does
//! carry an effect row since #1680 step 3 (`FnRow`, the creation-target
//! set §7's row table is keyed by), which is the substrate this gate would
//! need; what is missing is that no inferred type is threaded into this
//! pass, and effects-spec §6.1c's stratum question is unanswered. Still a
//! real gap in the ruling's coverage, not a design choice of this gate; it
//! is recorded on issue #1679 rather than papered over.
//!
//! **A second known hole, deferred rather than fixed (issue #1887's own
//! scope, bullet 2):** an inline lambda argument (`map(items, |x|
//! impure())`) is also not collected as a comparator site, for a
//! different, structural reason — a lambda literal has no `DefinitionId`
//! until LIR lowering mints one (issue #1727, itself parked pending a
//! design ruling), and this pass runs at HIR time, so there is nothing to
//! resolve an effect row against yet. Recorded on #1709 and #1887; see the
//! `_ => {}` arm in `collect_expr`'s `Expr::Call` match.
//!
//! Brink-only, same posture as the other effect passes: under strict-ink
//! the `#fn(…)` literal (and the verbs themselves) are already rejected by
//! the dialect gate; the native bare-name spelling (issue #1887) simply
//! does not arise on the ink surface at all — a bare name there is a
//! visit count, never a fn value, so this gate's bare-name `collect_expr`
//! arm is unreachable outside `ctx.native`.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Content, ContentPart, Diagnostic, DiagnosticCode, Expr, FileId, HirFile,
    ResolutionMap, Stmt, SymbolIndex,
};
use rowan::TextRange;

use crate::infer::EffectRow;

/// The **position of the pure callback** in `name`'s argument row, for every
/// verb that takes one — `None` for anything else.
///
/// Two families share this gate because they share the ruling: the NS-A4
/// comparator pair (F0: the imperative in-place form and its functional
/// past-participle twin), and the fn-value verb layer's pure quartet
/// (`docs/stdlib-spec.md` §4, issue #1679), whose callbacks are
/// pure·silent-required by the same 2026-07-18 sitting. `fold`'s callback is
/// its *third* argument (`fold(a, init, f)`) — which is exactly why this is a
/// position lookup rather than a boolean.
///
/// The ruled effectful spellings (`each`, `map_each`, issue #1679 slice 2)
/// deliberately never appear here: their whole purpose is to permit the
/// effects this gate rejects.
fn callback_arg_index(name: &str) -> Option<usize> {
    match name {
        "sort_by" | "sorted_by" | "map" | "filter" | "filter_map" => Some(1),
        "fold" => Some(2),
        _ => None,
    }
}

/// The verb spellings this gate knows, as `&'static str` — the diagnostic
/// interns the name so [`ComparatorSite`] can stay `Copy`-cheap.
fn verb_name(name: &str) -> Option<&'static str> {
    match name {
        "sort_by" => Some("sort_by"),
        "sorted_by" => Some("sorted_by"),
        "map" => Some("map"),
        "filter" => Some("filter"),
        "fold" => Some("fold"),
        "filter_map" => Some("filter_map"),
        _ => None,
    }
}

/// Check every pure-callback verb call in `hir` (see
/// [`callback_arg_index`]) whose callback is a statically-named function —
/// an inline `#fn(target)` literal (ink/brink), or the sigil-free native
/// bare-name spelling (`.brink`, issue #1862/#1887: `map(items, double)`) —
/// against the whole-project effect rows. Returns an `E119` for each
/// callback whose row provably exceeds pure·silent.
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    rows: &BTreeMap<DefinitionId, EffectRow>,
) -> Vec<Diagnostic> {
    let by_range = range_map(file, resolutions);
    let is_fn_target = |range: TextRange| is_fn_target_ref(range, &by_range, index);
    let ctx = CollectCtx {
        native: hir.native,
        is_fn_target: &is_fn_target,
    };
    let mut sites = Vec::new();
    collect_sites(hir, &ctx, &mut sites);

    let mut out = Vec::new();
    for site in sites {
        let key = (
            site.target_range.start().into(),
            site.target_range.end().into(),
        );
        let Some(def) = by_range.get(&key) else {
            continue; // unresolved target — E079's problem, not ours
        };
        let Some(row) = rows.get(def) else {
            continue; // no row (an EXTERNAL etc.) — not provable here
        };
        if let Some(exceedance) = contract_exceedance(row, index) {
            let (role, requirement) = callback_role(site.verb);
            out.push(Diagnostic {
                file,
                range: site.call_range,
                message: format!(
                    "{}: `{}`'s {role} `{}` {} — {requirement}",
                    DiagnosticCode::E119.title(),
                    site.verb,
                    site.target_name,
                    exceedance,
                ),
                code: DiagnosticCode::E119,
            });
        }
    }
    out
}

/// Cheap structural scan: does any knot/stitch body in `hir` contain a
/// pure-callback verb call whose callback is an inline `#fn(…)` literal, or
/// (native files only) a bare-name reference that might be one? The
/// laziness gate for the whole-project pass — a project without such a
/// site never triggers effect inference here, mirroring the `#@effects`
/// and `await`-purity gates.
#[must_use]
pub fn hir_has_comparator_site(hir: &HirFile) -> bool {
    // No resolution map at this layer (this runs before the whole-project
    // effect table exists at all — it decides whether that table gets
    // built) — the laziness gate can only be structural. Accepting every
    // native bare-`Path` callback candidate unconditionally (never proving
    // it resolves to a function definition) is a deliberate
    // over-approximation: a false positive here just costs one wasted
    // `effects_project` run on a project that turns out to have no real
    // site, whereas a false negative would silently skip [`check`]
    // entirely (issue #1887 was exactly this: an under-approximating gate
    // made the whole pass native-blind).
    let is_fn_target = |_: TextRange| true;
    let ctx = CollectCtx {
        native: hir.native,
        is_fn_target: &is_fn_target,
    };
    let mut sites = Vec::new();
    collect_sites(hir, &ctx, &mut sites);
    !sites.is_empty()
}

/// Every [`DefinitionId`] named as a statically-named-function callback
/// (`#fn(target)` literal, or the native bare-name spelling) of a
/// pure-callback verb call in `hir`, resolved through `resolutions` and
/// `index`. The salsa path (`brink-db`'s
/// `comparator_contract_diagnostics_query`) uses this to fetch exactly
/// those defs' memoized per-def effect rows — the incremental analogue of
/// the monolithic path handing [`check`] the whole-project
/// `effects_project` table (the `await_condition_callees` shape).
#[must_use]
pub fn comparator_callees(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> std::collections::BTreeSet<DefinitionId> {
    let by_range = range_map(file, resolutions);
    let is_fn_target = |range: TextRange| is_fn_target_ref(range, &by_range, index);
    let ctx = CollectCtx {
        native: hir.native,
        is_fn_target: &is_fn_target,
    };
    let mut sites = Vec::new();
    collect_sites(hir, &ctx, &mut sites);

    let mut out = std::collections::BTreeSet::new();
    for site in sites {
        let key = (
            site.target_range.start().into(),
            site.target_range.end().into(),
        );
        if let Some(&def) = by_range.get(&key) {
            out.insert(def);
        }
    }
    out
}

/// How to name the callback and what the diagnostic should demand of it.
/// The comparator pair keeps its original §4b wording verbatim; the quartet
/// gets §4's own.
///
/// §4 rules that the quartet's rejection "names both exits" — pure, or the
/// effectful spelling. Now that `each`/`map_each` ship (issue #1679 slice
/// 2), the message names them as the real advice they are.
fn callback_role(verb: &str) -> (&'static str, &'static str) {
    match verb {
        "sort_by" | "sorted_by" => (
            "comparator",
            "a comparator must be a pure, silent `fn(T, T): int` (stdlib-spec §4b: the order \
             must depend only on the two comparands)",
        ),
        _ => (
            "callback",
            "the callback must be pure and silent (stdlib-spec §4: the quartet is \
             pure-required, which is what makes iteration order unobservable) — make it pure, \
             or say `each`/`map_each`",
        ),
    }
}

/// Threaded through every `collect_*` walker: the two pieces of context
/// needed to recognize the native bare-name callback shape (issue #1887)
/// alongside the pre-existing `#fn(target)` literal shape.
struct CollectCtx<'a> {
    /// `hir.native` — a bare-name callback is a fn value only on the
    /// native surface (§2a); in ink the same shape is a knot's visit count
    /// and must never be collected as a comparator site.
    native: bool,
    /// Whether the `Expr::Path` at this range is *proven* to resolve to a
    /// statically-named function definition — the same "exceedance-only"
    /// posture [`contract_exceedance`] enforces for the row itself:
    /// [`hir_has_comparator_site`]'s structural pre-pass has no resolution
    /// map yet and always answers `true` (over-approximation, safe for a
    /// laziness gate); [`check`]/[`comparator_callees`] pass the real
    /// resolution+index lookup ([`is_fn_target_ref`]).
    is_fn_target: &'a dyn Fn(TextRange) -> bool,
}

/// `(start, end)` key for range-indexed lookups (`TextRange` has no `Ord`)
/// — the same shape [`range_map`] indexes by.
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// The real bare-name-is-a-fn-value predicate for [`check`]/
/// [`comparator_callees`]: `range` must resolve (through `by_range`) to a
/// definition that [`brink_ir::SymbolInfo::is_function_definition`] — the
/// same shared creation-site predicate `lir::lower::expr::lower_path` and
/// `InferPass::native_fn_value_target` gate their native bare-name
/// handling on, so this gate can never disagree with lowering about which
/// references are fn values. An opaque reference (a VAR/CONST global, a
/// param, a temp) answers `false` here — exactly the exceedance-only,
/// proven-only posture the module doc requires: an opaque value is not
/// provable and passes, never flagged.
fn is_fn_target_ref(
    range: TextRange,
    by_range: &BTreeMap<(u32, u32), DefinitionId>,
    index: &SymbolIndex,
) -> bool {
    by_range.get(&range_key(range)).is_some_and(|def| {
        index
            .symbols
            .get(def)
            .is_some_and(brink_ir::SymbolInfo::is_function_definition)
    })
}

fn range_map(file: FileId, resolutions: &ResolutionMap) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| ((r.range.start().into(), r.range.end().into()), r.target))
        .collect()
}

/// The proven-exceedance judgment: `Some(description)` when the row shows
/// a pure·silent violation. Opaque rows and fault-bearing rows pass — see
/// the module doc (exceedance-only; faults are allowed by F14).
fn contract_exceedance(row: &EffectRow, index: &SymbolIndex) -> Option<String> {
    let name_of = |id: &DefinitionId| {
        index
            .symbols
            .get(id)
            .map_or_else(|| format!("{id:?}"), |info| info.name.clone())
    };
    let mut parts = Vec::new();
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
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

/// One pure-callback site: the call's diagnostic anchor, the verb spelling,
/// and the target of the callback — an inline `#fn` literal (ink/brink) or
/// a native bare-name reference (issue #1887).
struct ComparatorSite {
    call_range: TextRange,
    verb: &'static str,
    target_range: TextRange,
    target_name: String,
}

/// Issues #1769/#2085: this pass used to start only from `root_content` +
/// knot/stitch bodies, silently skipping every file-level `VAR`/`CONST`
/// initializer expression — both a direct pure-callback misuse written
/// straight in an initializer (`VAR bad = sort_by(xs, #fn(spy))`, #1769) and
/// one nested inside a decl-default lambda's own body
/// (`const doIt = || map(xs, spy)`, #2085, legal since #1774's ruling). The
/// other six passes in this "hand-rolled initializer recursion" family
/// (`coalesce`, `contains_domain`, `conversions`, `map_keys`, `structs`,
/// `range_refinement`) already carry this same two-loop mirror in their own
/// entry points; this is `comparator_contract`'s copy of it. See this
/// function's own doc below (and the PR that added these two loops) for why
/// this stays a hand-rolled mirror rather than a switch to
/// `hir::visit::visit_with_decl_initializers` — the shared entry point this
/// family should eventually consolidate onto.
fn collect_sites(hir: &HirFile, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    collect_block(&hir.root_content, ctx, out);
    for knot in &hir.knots {
        collect_block(&knot.body, ctx, out);
        for stitch in &knot.stitches {
            collect_block(&stitch.body, ctx, out);
        }
    }
    for var in &hir.variables {
        collect_expr(&var.value, ctx, out);
    }
    for c in &hir.constants {
        collect_expr(&c.value, ctx, out);
    }
}

fn collect_block(block: &Block, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    for stmt in &block.stmts {
        collect_stmt(stmt, ctx, out);
    }
}

fn collect_stmt(stmt: &Stmt, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    match stmt {
        Stmt::TempDecl(t) => {
            if let Some(v) = &t.value {
                collect_expr(v, ctx, out);
            }
        }
        Stmt::Assignment(a) => {
            collect_expr(&a.target, ctx, out);
            collect_expr(&a.value, ctx, out);
        }
        Stmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_expr(v, ctx, out);
            }
            for a in &r.onwards_args {
                collect_expr(a, ctx, out);
            }
        }
        Stmt::ExprStmt(e) => collect_expr(e, ctx, out),
        Stmt::Await(a) => {
            if let Some(cond) = &a.condition {
                collect_expr(cond, ctx, out);
            }
        }
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                collect_block_stmt(bs, ctx, out);
            }
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                if let Some(cond) = &choice.condition {
                    collect_expr(cond, ctx, out);
                }
                collect_block(&choice.body, ctx, out);
            }
            collect_block(&cs.continuation, ctx, out);
        }
        Stmt::LabeledBlock(b) => collect_block(b, ctx, out),
        Stmt::Conditional(c) => collect_conditional(c, ctx, out),
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                collect_block(&branch.body, ctx, out);
            }
        }
        Stmt::Content(content) => collect_content(content, ctx, out),
        Stmt::Divert(_) | Stmt::TunnelCall(_) | Stmt::ThreadStart(_) | Stmt::EndOfLine => {}
    }
}

fn collect_conditional(
    c: &brink_ir::Conditional,
    ctx: &CollectCtx<'_>,
    out: &mut Vec<ComparatorSite>,
) {
    for branch in &c.branches {
        if let Some(cond) = &branch.condition {
            collect_expr(cond, ctx, out);
        }
        collect_block(&branch.body, ctx, out);
    }
}

fn collect_content(content: &Content, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    for part in &content.parts {
        collect_content_part(part, ctx, out);
    }
}

fn collect_content_part(part: &ContentPart, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    match part {
        ContentPart::Interpolation(e) => collect_expr(e, ctx, out),
        ContentPart::InlineConditional(c) => collect_conditional(c, ctx, out),
        ContentPart::InlineSequence(s) => {
            for branch in &s.branches {
                collect_block(&branch.body, ctx, out);
            }
        }
        // Presentational, not opaque (§4.3) — an interpolation inside a
        // span is still a real comparator site.
        ContentPart::Span(span) => {
            for child in &span.children {
                collect_content_part(child, ctx, out);
            }
        }
        ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
    }
}

fn collect_block_stmt(bs: &BlockStmt, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    match bs {
        BlockStmt::TempDecl(t) => {
            if let Some(v) = &t.value {
                collect_expr(v, ctx, out);
            }
        }
        BlockStmt::Assignment(a) => {
            collect_expr(&a.target, ctx, out);
            collect_expr(&a.value, ctx, out);
        }
        BlockStmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_expr(v, ctx, out);
            }
            for a in &r.onwards_args {
                collect_expr(a, ctx, out);
            }
        }
        BlockStmt::ExprStmt(e) => collect_expr(e, ctx, out),
        BlockStmt::Await(a) => {
            if let Some(cond) = &a.condition {
                collect_expr(cond, ctx, out);
            }
        }
        BlockStmt::While(w) => {
            collect_expr(&w.condition, ctx, out);
            for s in &w.body {
                collect_block_stmt(s, ctx, out);
            }
        }
        BlockStmt::If(i) => collect_if(i, ctx, out),
        BlockStmt::For(f) => {
            collect_expr(&f.iterable, ctx, out);
            for s in &f.body {
                collect_block_stmt(s, ctx, out);
            }
        }
        BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
    }
}

fn collect_if(i: &brink_ir::IfStmt, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    collect_expr(&i.condition, ctx, out);
    for s in &i.body {
        collect_block_stmt(s, ctx, out);
    }
    match &i.else_branch {
        Some(brink_ir::ElseBranch::Else(stmts)) => {
            for s in stmts {
                collect_block_stmt(s, ctx, out);
            }
        }
        Some(brink_ir::ElseBranch::ElseIf(nested)) => collect_if(nested, ctx, out),
        None => {}
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one match arm per Expr variant; splitting would obscure the dispatch"
)]
fn collect_expr(expr: &Expr, ctx: &CollectCtx<'_>, out: &mut Vec<ComparatorSite>) {
    match expr {
        Expr::Call(path, args) => {
            let name = path.segments.last().map_or("", |seg| seg.text.as_str());
            if path.segments.len() == 1
                && let Some(idx) = callback_arg_index(name)
                && let Some(verb) = verb_name(name)
                && let Some(arg) = args.get(idx)
            {
                match arg {
                    Expr::FnLiteral(fnl) => {
                        out.push(ComparatorSite {
                            call_range: path.range,
                            verb,
                            target_range: fnl.target.range,
                            target_name: fnl
                                .target
                                .segments
                                .iter()
                                .map(|s| s.text.as_str())
                                .collect::<Vec<_>>()
                                .join("."),
                        });
                    }
                    // Native bare-name callback (issue #1887, #1862's
                    // 2026-08-01 ruling): `map(items, double)` — no `#fn`,
                    // no sigil, because `#` is already the tag sigil in
                    // native content position. Ink-blind by construction:
                    // `ctx.native` gates it exactly like
                    // `lir::lower::expr::lower_path`'s `MakeFnValue` arm
                    // and `InferPass::native_fn_value_target` do, and
                    // `ctx.is_fn_target` additionally proves the reference
                    // resolves to a real function definition — never an
                    // opaque VAR/param/temp/list-item, which stay
                    // unprovable and pass (the module doc's
                    // exceedance-only posture).
                    Expr::Path(p) if ctx.native && (ctx.is_fn_target)(p.range) => {
                        out.push(ComparatorSite {
                            call_range: path.range,
                            verb,
                            target_range: p.range,
                            target_name: p
                                .segments
                                .iter()
                                .map(|s| s.text.as_str())
                                .collect::<Vec<_>>()
                                .join("::"),
                        });
                    }
                    // Deliberately not collected — the known, DEFERRED
                    // hole issue #1887 (§"Scope", bullet 2) asked to be
                    // decided or explicitly deferred, not silently dropped:
                    // an inline lambda argument (`map(items, |x| impure())`)
                    // is *itself* a pure-callback candidate, structurally
                    // parallel to the `#fn(target)`/bare-name arms above,
                    // but a lambda literal has no `DefinitionId` until LIR
                    // lowering mints one (issue #1727, itself parked
                    // pending a design ruling) — so at HIR time, where this
                    // pass runs, there is no def to resolve an effect row
                    // against. Tracked on #1709 (the same structural gap
                    // recorded for the creation-site atom) and #1887.
                    // Not a regression: the pre-#1887 code dropped this
                    // shape too, just without a comment saying so.
                    _ => {}
                }
            }
            for a in args {
                collect_expr(a, ctx, out);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => collect_expr(inner, ctx, out),
        Expr::Infix(ie) => {
            collect_expr(&ie.lhs, ctx, out);
            collect_expr(&ie.rhs, ctx, out);
        }
        Expr::Index(idx) => {
            collect_expr(&idx.base, ctx, out);
            collect_expr(&idx.index, ctx, out);
        }
        Expr::FieldAccess(fa) => collect_expr(&fa.base, ctx, out),
        Expr::Range(r) => {
            collect_expr(&r.start, ctx, out);
            collect_expr(&r.end, ctx, out);
        }
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                collect_expr(e, ctx, out);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, v) in &m.entries {
                collect_expr(k, ctx, out);
                collect_expr(v, ctx, out);
            }
        }
        Expr::StructLiteral(sl) => {
            for (_, v) in &sl.fields {
                collect_expr(v, ctx, out);
            }
        }
        // An `#fn(…)` literal's *bound args* are evaluated at creation, so
        // a comparator site nested in one is real; the target is a name,
        // not an expression.
        Expr::FnLiteral(fnl) => {
            for a in &fnl.args {
                collect_expr(a, ctx, out);
            }
        }
        Expr::RefArg(r) => collect_expr(&r.operand, ctx, out),
        // A lambda's whole body (issue #1685, #1764). Unlike the sibling
        // initializer walks this collector *has* a statement vocabulary, so
        // a braced body's statements go through `collect_block_stmt` — the
        // same arm every code-ground block gets — rather than the flattened
        // `LambdaBody::all_exprs`.
        //
        // The `#fn(target)` literal shape still has **no reachable case
        // here** (audited under #1764, same finding as `range_refinement`):
        // `Expr::Lambda` is minted only by `hir::lower_native`,
        // `Expr::FnLiteral` only by `hir::lower` (the native surface has no
        // `#fn` grammar; the ink/brink surface has no lambda grammar), so a
        // `#fn` literal cannot occur inside a lambda body at all.
        //
        // The native bare-name shape (issue #1887) is different: both it
        // and `Expr::Lambda` are native-only, so a bare-name pure-callback
        // call CAN occur inside a lambda body (`filter(items, |x|
        // map(x.rest, impure))`) — this descent is what reaches it, via the
        // ordinary `Expr::Call` arm above once the walk gets to the tail/
        // body statements.
        Expr::Lambda(l) => match &l.body {
            brink_ir::LambdaBody::Expr(e) => collect_expr(e, ctx, out),
            brink_ir::LambdaBody::Block { stmts, tail } => {
                for s in stmts {
                    collect_block_stmt(s, ctx, out);
                }
                if let Some(t) = tail {
                    collect_expr(t, ctx, out);
                }
            }
        },
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Null
        | Expr::Path(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral(_) => {}
        // Block capture (issue #1839): the captured run is real weave-level
        // content, so it goes through `collect_stmt` — the same top-level
        // `Stmt` vocabulary the rest of this file's HIR walk already uses —
        // rather than the closed `BlockStmt` set `collect_block_stmt` owns.
        Expr::Fragment(stmts) => {
            for s in stmts {
                collect_stmt(s, ctx, out);
            }
        }
    }
}
