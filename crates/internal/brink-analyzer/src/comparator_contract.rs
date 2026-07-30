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
//! literal whose statically-named target's inferred row shows the effect.
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
//! Brink-only, same posture as the other effect passes: under strict-ink
//! the `#fn(…)` literal (and the verbs themselves) are already rejected by
//! the dialect gate.

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
/// [`callback_arg_index`]) whose callback is an inline `#fn(target)`
/// literal against the whole-project effect rows. Returns an `E119` for
/// each callback whose row provably exceeds
/// pure·silent.
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    rows: &BTreeMap<DefinitionId, EffectRow>,
) -> Vec<Diagnostic> {
    let by_range = range_map(file, resolutions);
    let mut sites = Vec::new();
    collect_sites(hir, &mut sites);

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
/// pure-callback verb call with an inline `#fn(…)` callback? The
/// laziness gate for the whole-project pass — a project without such a
/// site never triggers effect inference here, mirroring the `#@effects`
/// and `await`-purity gates.
#[must_use]
pub fn hir_has_comparator_site(hir: &HirFile) -> bool {
    let mut sites = Vec::new();
    collect_sites(hir, &mut sites);
    !sites.is_empty()
}

/// Every [`DefinitionId`] named as an inline `#fn(target)` callback of a
/// pure-callback verb call in `hir`, resolved through `resolutions`.
/// The salsa path (`brink-db`'s `comparator_contract_diagnostics_query`)
/// uses this to fetch exactly those defs' memoized per-def effect rows —
/// the incremental analogue of the monolithic path handing [`check`] the
/// whole-project `effects_project` table (the `await_condition_callees`
/// shape).
#[must_use]
pub fn comparator_callees(
    file: FileId,
    hir: &HirFile,
    resolutions: &ResolutionMap,
) -> std::collections::BTreeSet<DefinitionId> {
    let by_range = range_map(file, resolutions);
    let mut sites = Vec::new();
    collect_sites(hir, &mut sites);

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
/// and the inline `#fn` literal's target.
struct ComparatorSite {
    call_range: TextRange,
    verb: &'static str,
    target_range: TextRange,
    target_name: String,
}

fn collect_sites(hir: &HirFile, out: &mut Vec<ComparatorSite>) {
    collect_block(&hir.root_content, out);
    for knot in &hir.knots {
        collect_block(&knot.body, out);
        for stitch in &knot.stitches {
            collect_block(&stitch.body, out);
        }
    }
}

fn collect_block(block: &Block, out: &mut Vec<ComparatorSite>) {
    for stmt in &block.stmts {
        collect_stmt(stmt, out);
    }
}

fn collect_stmt(stmt: &Stmt, out: &mut Vec<ComparatorSite>) {
    match stmt {
        Stmt::TempDecl(t) => {
            if let Some(v) = &t.value {
                collect_expr(v, out);
            }
        }
        Stmt::Assignment(a) => {
            collect_expr(&a.target, out);
            collect_expr(&a.value, out);
        }
        Stmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_expr(v, out);
            }
            for a in &r.onwards_args {
                collect_expr(a, out);
            }
        }
        Stmt::ExprStmt(e) => collect_expr(e, out),
        Stmt::Await(a) => {
            if let Some(cond) = &a.condition {
                collect_expr(cond, out);
            }
        }
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                collect_block_stmt(bs, out);
            }
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                if let Some(cond) = &choice.condition {
                    collect_expr(cond, out);
                }
                collect_block(&choice.body, out);
            }
            collect_block(&cs.continuation, out);
        }
        Stmt::LabeledBlock(b) => collect_block(b, out),
        Stmt::Conditional(c) => collect_conditional(c, out),
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                collect_block(&branch.body, out);
            }
        }
        Stmt::Content(content) => collect_content(content, out),
        Stmt::Divert(_) | Stmt::TunnelCall(_) | Stmt::ThreadStart(_) | Stmt::EndOfLine => {}
    }
}

fn collect_conditional(c: &brink_ir::Conditional, out: &mut Vec<ComparatorSite>) {
    for branch in &c.branches {
        if let Some(cond) = &branch.condition {
            collect_expr(cond, out);
        }
        collect_block(&branch.body, out);
    }
}

fn collect_content(content: &Content, out: &mut Vec<ComparatorSite>) {
    for part in &content.parts {
        collect_content_part(part, out);
    }
}

fn collect_content_part(part: &ContentPart, out: &mut Vec<ComparatorSite>) {
    match part {
        ContentPart::Interpolation(e) => collect_expr(e, out),
        ContentPart::InlineConditional(c) => collect_conditional(c, out),
        ContentPart::InlineSequence(s) => {
            for branch in &s.branches {
                collect_block(&branch.body, out);
            }
        }
        // Presentational, not opaque (§4.3) — an interpolation inside a
        // span is still a real comparator site.
        ContentPart::Span(span) => {
            for child in &span.children {
                collect_content_part(child, out);
            }
        }
        ContentPart::Text(_) | ContentPart::Glue | ContentPart::Spring => {}
    }
}

fn collect_block_stmt(bs: &BlockStmt, out: &mut Vec<ComparatorSite>) {
    match bs {
        BlockStmt::TempDecl(t) => {
            if let Some(v) = &t.value {
                collect_expr(v, out);
            }
        }
        BlockStmt::Assignment(a) => {
            collect_expr(&a.target, out);
            collect_expr(&a.value, out);
        }
        BlockStmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_expr(v, out);
            }
            for a in &r.onwards_args {
                collect_expr(a, out);
            }
        }
        BlockStmt::ExprStmt(e) => collect_expr(e, out),
        BlockStmt::Await(a) => {
            if let Some(cond) = &a.condition {
                collect_expr(cond, out);
            }
        }
        BlockStmt::While(w) => {
            collect_expr(&w.condition, out);
            for s in &w.body {
                collect_block_stmt(s, out);
            }
        }
        BlockStmt::If(i) => collect_if(i, out),
        BlockStmt::For(f) => {
            collect_expr(&f.iterable, out);
            for s in &f.body {
                collect_block_stmt(s, out);
            }
        }
        BlockStmt::Break(_) | BlockStmt::Continue(_) => {}
    }
}

fn collect_if(i: &brink_ir::IfStmt, out: &mut Vec<ComparatorSite>) {
    collect_expr(&i.condition, out);
    for s in &i.body {
        collect_block_stmt(s, out);
    }
    match &i.else_branch {
        Some(brink_ir::ElseBranch::Else(stmts)) => {
            for s in stmts {
                collect_block_stmt(s, out);
            }
        }
        Some(brink_ir::ElseBranch::ElseIf(nested)) => collect_if(nested, out),
        None => {}
    }
}

fn collect_expr(expr: &Expr, out: &mut Vec<ComparatorSite>) {
    match expr {
        Expr::Call(path, args) => {
            let name = path.segments.last().map_or("", |seg| seg.text.as_str());
            if path.segments.len() == 1
                && let Some(idx) = callback_arg_index(name)
                && let Some(verb) = verb_name(name)
                && let Some(Expr::FnLiteral(fnl)) = args.get(idx)
            {
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
            for a in args {
                collect_expr(a, out);
            }
        }
        Expr::Prefix(_, inner) | Expr::Postfix(inner, _) => collect_expr(inner, out),
        Expr::Infix(ie) => {
            collect_expr(&ie.lhs, out);
            collect_expr(&ie.rhs, out);
        }
        Expr::Index(idx) => {
            collect_expr(&idx.base, out);
            collect_expr(&idx.index, out);
        }
        Expr::FieldAccess(fa) => collect_expr(&fa.base, out),
        Expr::Range(r) => {
            collect_expr(&r.start, out);
            collect_expr(&r.end, out);
        }
        Expr::ArrayLiteral(a) => {
            for e in &a.elements {
                collect_expr(e, out);
            }
        }
        Expr::MapLiteral(m) => {
            for (k, v) in &m.entries {
                collect_expr(k, out);
                collect_expr(v, out);
            }
        }
        Expr::StructLiteral(sl) => {
            for (_, v) in &sl.fields {
                collect_expr(v, out);
            }
        }
        // An `#fn(…)` literal's *bound args* are evaluated at creation, so
        // a comparator site nested in one is real; the target is a name,
        // not an expression.
        Expr::FnLiteral(fnl) => {
            for a in &fnl.args {
                collect_expr(a, out);
            }
        }
        Expr::RefArg(r) => collect_expr(&r.operand, out),
        // A lambda's whole body (issue #1685, #1764). Unlike the sibling
        // initializer walks this collector *has* a statement vocabulary, so
        // a braced body's statements go through `collect_block_stmt` — the
        // same arm every code-ground block gets — rather than the flattened
        // `LambdaBody::all_exprs`.
        //
        // **No reachable case today, and so no regression test** (audited
        // under #1764, same finding as `range_refinement`): this gate fires
        // only on an *inline* `#fn(target)` callback, and the two shapes are
        // surface-disjoint. `Expr::Lambda` is minted only by
        // `hir::lower_native`, `Expr::FnLiteral` only by `hir::lower` (the
        // native surface has no `#fn` grammar; the ink/brink surface has no
        // lambda grammar), so a `#fn` literal cannot occur inside a lambda
        // body at all. This descends anyway so the gap cannot silently
        // reopen the day the surfaces converge; it is a no-op until then.
        Expr::Lambda(l) => match &l.body {
            brink_ir::LambdaBody::Expr(e) => collect_expr(e, out),
            brink_ir::LambdaBody::Block { stmts, tail } => {
                for s in stmts {
                    collect_block_stmt(s, out);
                }
                if let Some(t) = tail {
                    collect_expr(t, out);
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
    }
}
