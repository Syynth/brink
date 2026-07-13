//! `signature`/`infer_body`/`type_diagnostics` — the checker substrate
//! (typed-mode-spec §2, TM-1). **Advisory-only**: this module produces
//! inference *results* for later consumers (hover, TM-3 strict mode); it
//! changes no compiler behavior and the LIR/codegen/runtime never read it.
//!
//! ## The firewall
//!
//! `infer_body(A)` (here, [`body::infer_def_body`] driven by
//! [`infer_project`]) reads only `signature(B)` for every def `B` it calls —
//! never `B`'s body. Two kinds of "signature" satisfy that rule:
//!
//! - A **global** (`VAR`/`CONST`) reads its declaration-derived
//!   [`crate::Sig::value_type`] — the existing phase-0 stub, untouched by
//!   this module (see `crate::signature`'s `signature_is_declaration_derived_only`
//!   test, which this module must never make fail).
//! - A **callable** (knot/stitch) reads its entry in `known_sigs`: either
//!   another SCC's already-finalized [`InferredSig`], or — for a call
//!   *within* the SCC currently being solved — that SCC's current fixpoint
//!   estimate. Call-site-driven inference (letting a caller's argument
//!   types flow backward into a callee's params) is never done; only a
//!   callee's own already-computed signature flows forward into how the
//!   caller's argument expressions are typed.
//!
//! ## SCC fixpoint
//!
//! [`graph::topo_order`] batches every inferable def into strongly-connected
//! components, ordered so a component is solved only after every *other*
//! component it calls is finalized. A component may contain more than one
//! def (mutual recursion) — those solve together: seed every member with an
//! `Unknown` working signature, re-run [`body::infer_def_body`] across the
//! whole component, and repeat until no member's signature changes (or
//! [`MAX_SCC_ITERATIONS`] is hit — guard against unbounded growth, house
//! rule). This is the monomorphic Haskell-style binding-group solve spec §2
//! asks for.
//!
//! ## Laziness (perf)
//!
//! [`infer_project`] is not wired into `analysis_query`, `lir_query`,
//! `diagnostics_query`, or `story_data_query` — nothing in the existing
//! compile/IDE path calls it. The brink-db salsa wrapper
//! (`type_inference_query`) is therefore only ever computed when a consumer
//! explicitly asks for `infer_body`/`type_diagnostics`, which today is
//! nobody: this slice is pure substrate. See the PR's benchmark report for
//! the before/after warm/cold numbers this predicts (no measurable delta on
//! the existing paths).

mod body;
mod graph;
mod ty;

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::{Block, FileId, HirFile, Param, ResolutionMap, SymbolIndex, SymbolKind};
use rowan::TextRange;

pub use ty::{Ty, unify, unify_all};

use body::{BodyCtx, infer_def_body};
use graph::{CallGraph, topo_order};

/// `TextRange` has no `Ord` impl (ranges have no single natural total
/// order), so every `BTreeMap` keyed by a reference's source range in this
/// module uses this `(start, end)` `u32` pair instead.
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// Caps the number of re-solve rounds for one SCC batch (guard against
/// unbounded growth, house rule). Convergence is expected within a handful
/// of rounds for this finite, monomorphic, no-overloading type universe —
/// a genuinely pathological program that never stabilizes still terminates
/// with whatever partial signature this cap leaves it at, which is legal
/// (unresolved slots read as `Unknown`), not a hang.
const MAX_SCC_ITERATIONS: usize = 8;

/// A def's inferred signature: positional param types (declaration order)
/// plus a return type. The generalized, per-def result of a body's fixpoint
/// solve — what a *caller* reads (never the caller reading the callee's
/// body directly; that's the firewall).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InferredSig {
    pub params: Vec<Ty>,
    pub return_ty: Ty,
}

/// The full inferred picture of one def's body: params, every local
/// (params ∪ temps) by name, and the return type. A superset of
/// [`InferredSig`] — `signatures` is the firewall-facing projection,
/// `bodies` is what a hover/diagnostic consumer (TM-5) wants.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyTypes {
    pub params: Vec<(String, Ty)>,
    pub locals: BTreeMap<String, Ty>,
    pub return_ty: Ty,
}

/// The whole-project inference result (mirrors `AnalysisResult`'s shape:
/// one pure function over already-computed inputs, callable directly or
/// wrapped as a salsa query).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InferenceResult {
    /// Every inferable (knot/stitch) def's finalized signature.
    pub signatures: BTreeMap<DefinitionId, InferredSig>,
    /// Every inferable def's full body type picture.
    pub bodies: BTreeMap<DefinitionId, BodyTypes>,
}

impl From<crate::InferredType> for Ty {
    fn from(t: crate::InferredType) -> Self {
        match t {
            crate::InferredType::Int => Ty::Int,
            crate::InferredType::Float => Ty::Float,
            crate::InferredType::Bool => Ty::Bool,
            crate::InferredType::String => Ty::String,
            crate::InferredType::Divert => Ty::Divert,
            // The initializer-derived stub records only "this was a list
            // literal", not which LIST it belongs to — conservatively
            // Unknown rather than guessing (legal; advisory-only slice).
            crate::InferredType::List => Ty::Unknown,
        }
    }
}

/// One inferable definition: its own id, declaring file, declared params,
/// and body.
struct Def<'a> {
    id: DefinitionId,
    file: FileId,
    params: &'a [Param],
    body: &'a Block,
}

/// Per-file resolution lookup: a `Path`'s range is only unique within its
/// own file, so resolutions must never be merged across files.
fn index_resolutions_by_file(
    resolutions: &ResolutionMap,
) -> BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>> {
    let mut by_file: BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>> = BTreeMap::new();
    for r in resolutions {
        by_file
            .entry(r.file)
            .or_default()
            .insert(range_key(r.range), r.target);
    }
    by_file
}

/// Declaration-derived global (VAR/CONST) types — read via `signature()`,
/// the firewall boundary for every non-callable reference in a body.
fn collect_globals(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
) -> BTreeMap<DefinitionId, Ty> {
    let mut globals: BTreeMap<DefinitionId, Ty> = BTreeMap::new();
    for (&id, info) in &index.symbols {
        if matches!(info.kind, SymbolKind::Variable | SymbolKind::Constant)
            && let Some(sig) = crate::signature::signature(id, index, files)
            && let Some(vt) = sig.value_type
        {
            globals.insert(id, Ty::from(vt));
        }
    }
    globals
}

/// Every inferable (knot/stitch) def in the project, resolved back to its
/// own `DefinitionId` via `(file, kind, qualified name)` — HIR `Knot`/
/// `Stitch` nodes carry only a bare `Name`, not their own id.
fn collect_defs<'a>(files: &[(FileId, &'a HirFile)], index: &SymbolIndex) -> Vec<Def<'a>> {
    let mut def_of: BTreeMap<(FileId, SymbolKind, String), DefinitionId> = BTreeMap::new();
    for (&id, info) in &index.symbols {
        def_of.insert((info.file, info.kind, info.name.clone()), id);
    }

    let mut defs: Vec<Def<'a>> = Vec::new();
    for &(file_id, hir) in files {
        for knot in &hir.knots {
            if let Some(&id) = def_of.get(&(file_id, SymbolKind::Knot, knot.name.text.clone())) {
                defs.push(Def {
                    id,
                    file: file_id,
                    params: &knot.params,
                    body: &knot.body,
                });
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(&id) = def_of.get(&(file_id, SymbolKind::Stitch, qualified)) {
                    defs.push(Def {
                        id,
                        file: file_id,
                        params: &stitch.params,
                        body: &stitch.body,
                    });
                }
            }
        }
    }
    defs.sort_by_key(|d| d.id);
    defs
}

/// Shared read-only context every pass over `defs` needs.
struct ProjectCtx<'a> {
    index: &'a SymbolIndex,
    globals: &'a BTreeMap<DefinitionId, Ty>,
    by_file: &'a BTreeMap<FileId, BTreeMap<(u32, u32), DefinitionId>>,
    inferable: &'a BTreeSet<DefinitionId>,
}

impl ProjectCtx<'_> {
    fn body_ctx<'a>(
        &'a self,
        def: &Def<'_>,
        known_sigs: &'a BTreeMap<DefinitionId, InferredSig>,
    ) -> BodyCtx<'a> {
        static EMPTY: BTreeMap<(u32, u32), DefinitionId> = BTreeMap::new();
        BodyCtx {
            resolution_by_range: self.by_file.get(&def.file).unwrap_or(&EMPTY),
            index: self.index,
            globals: self.globals,
            known_sigs,
            inferable: self.inferable,
        }
    }
}

/// Pass 1: call-graph edges only. `known_sigs` is empty here — every call
/// resolves to `Unknown` and the resulting types are discarded — this pass
/// exists solely to discover which defs call which, which the SCC batching
/// (pass 2) needs before any real solving can start.
fn build_call_graph(defs: &[Def<'_>], ctx: &ProjectCtx<'_>) -> CallGraph {
    let no_sigs: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
    let mut graph = CallGraph::new();
    for d in defs {
        graph.add_node(d.id);
        let body_ctx = ctx.body_ctx(d, &no_sigs);
        let result = infer_def_body(d.params, d.body, &body_ctx);
        for callee in result.calls {
            graph.add_edge(d.id, callee);
        }
    }
    graph
}

/// Pass 2: solve every SCC batch in dependency order, mutually-recursive
/// batches by fixpoint (spec §2's SCC rule — see the module doc).
fn solve_batches(
    batches: Vec<BTreeSet<DefinitionId>>,
    by_id: &BTreeMap<DefinitionId, &Def<'_>>,
    ctx: &ProjectCtx<'_>,
) -> (
    BTreeMap<DefinitionId, InferredSig>,
    BTreeMap<DefinitionId, BodyTypes>,
) {
    let mut known_sigs: BTreeMap<DefinitionId, InferredSig> = BTreeMap::new();
    let mut bodies: BTreeMap<DefinitionId, BodyTypes> = BTreeMap::new();

    for batch in batches {
        for &id in &batch {
            known_sigs.entry(id).or_insert_with(|| {
                let param_count = by_id.get(&id).map_or(0, |d| d.params.len());
                InferredSig {
                    params: vec![Ty::Unknown; param_count],
                    return_ty: Ty::Unknown,
                }
            });
        }

        let mut last_round: BTreeMap<DefinitionId, body::BodyResult> = BTreeMap::new();
        for _round in 0..MAX_SCC_ITERATIONS {
            let mut round: BTreeMap<DefinitionId, body::BodyResult> = BTreeMap::new();
            let mut changed = false;
            for &id in &batch {
                let Some(&d) = by_id.get(&id) else { continue };
                let body_ctx = ctx.body_ctx(d, &known_sigs);
                let result = infer_def_body(d.params, d.body, &body_ctx);
                let new_sig = InferredSig {
                    params: result.params.iter().map(|(_, t)| t.clone()).collect(),
                    return_ty: result.return_ty.clone(),
                };
                if known_sigs.get(&id) != Some(&new_sig) {
                    changed = true;
                }
                known_sigs.insert(id, new_sig);
                round.insert(id, result);
            }
            last_round = round;
            if !changed {
                break;
            }
        }

        for (id, result) in last_round {
            bodies.insert(
                id,
                BodyTypes {
                    params: result.params,
                    locals: result.locals,
                    return_ty: result.return_ty,
                },
            );
        }
    }

    (known_sigs, bodies)
}

/// Infer types for every knot/stitch body across the whole project.
///
/// Pure function of already-computed inputs (`index`/`resolutions`, the
/// same shape `finish_analysis`/`signature` take) — safe to call directly in
/// tests, and the exact function `type_inference_query` wraps for salsa
/// memoization.
#[must_use]
pub fn infer_project(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> InferenceResult {
    let by_file = index_resolutions_by_file(resolutions);
    let globals = collect_globals(files, index);
    let defs = collect_defs(files, index);
    let inferable: BTreeSet<DefinitionId> = defs.iter().map(|d| d.id).collect();
    let by_id: BTreeMap<DefinitionId, &Def<'_>> = defs.iter().map(|d| (d.id, d)).collect();

    let ctx = ProjectCtx {
        index,
        globals: &globals,
        by_file: &by_file,
        inferable: &inferable,
    };

    let graph = build_call_graph(&defs, &ctx);
    let batches = topo_order(&graph);
    let (signatures, bodies) = solve_batches(batches, &by_id, &ctx);

    InferenceResult { signatures, bodies }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::lower;

    fn build(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) = crate::resolve(FileId(0), &manifest, &index);
        (hir, (*index).clone(), (*resolutions).clone())
    }

    fn sig_of<'a>(result: &'a InferenceResult, index: &SymbolIndex, name: &str) -> &'a InferredSig {
        let id = index
            .by_name
            .get(name)
            .and_then(|ids| ids.first())
            .copied()
            .expect("no def with this name");
        result
            .signatures
            .get(&id)
            .expect("no inferred signature for this def")
    }

    #[test]
    fn param_type_inferred_from_arithmetic_use() {
        // A knot whose param is used arithmetically against an int literal.
        let (hir, index, res) = build("=== heal(hp) ===\n~ temp x = hp + 1\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let sig = sig_of(&result, &index, "heal");
        assert_eq!(sig.params, vec![Ty::Int]);
    }

    #[test]
    fn param_type_inferred_from_comparison_with_float_literal() {
        let (hir, index, res) = build("=== spend(gold) ===\n{gold > 1.5:\n  ok\n}\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let sig = sig_of(&result, &index, "spend");
        assert_eq!(sig.params, vec![Ty::Float]);
    }

    #[test]
    fn unused_param_is_unknown_and_legal() {
        let (hir, index, res) = build("=== noop(x) ===\nHello.\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let sig = sig_of(&result, &index, "noop");
        assert_eq!(sig.params, vec![Ty::Unknown]);
    }

    #[test]
    fn return_type_inferred_from_return_statement() {
        let (hir, index, res) = build("=== function double(x) ===\n~ return x + x\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let sig = sig_of(&result, &index, "double");
        // `x` only ever appears added to itself — Unknown stays Unknown
        // under `unify(Unknown, Unknown) == Unknown`; the *return type*
        // still comes out Unknown too (nothing ever pins `x` concrete).
        assert_eq!(sig.return_ty, Ty::Unknown);
    }

    #[test]
    fn call_site_propagates_callee_param_type_to_caller_local() {
        let (hir, index, res) = build(
            "=== main ===\n~ temp v = 1\n~ use_it(v)\n-> DONE\n=== use_it(n) ===\n{n > 2.5:\n  big\n}\n-> DONE\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let use_it = sig_of(&result, &index, "use_it");
        assert_eq!(use_it.params, vec![Ty::Float]);
        // `main`'s own local `v` isn't a param, so we check it via `bodies`.
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let main_body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(main_body.locals.get("v"), Some(&Ty::Float));
    }

    #[test]
    #[expect(
        clippy::similar_names,
        reason = "ping/pong are the clearest names for this pair"
    )]
    fn mutual_recursion_params_stay_firewalled_to_each_defs_own_body() {
        // `ping` and `pong` call each other with an arithmetic expression
        // (`n - 1`), not a bare local — so nothing about the *callee's*
        // declared param type can flow backward onto the *caller's* own `n`
        // (call-site-driven inference is forbidden by the firewall). Each
        // def's own param type is pinned only by its own body's comparison:
        // `ping` compares `n` to an int literal, `pong` to a float literal.
        let (hir, index, res) = build(
            "=== function ping(n) ===\n{n > 0:\n  ~ return pong(n - 1)\n}\n~ return n\n\
             === function pong(n) ===\n{n > 0.5:\n  ~ return ping(n - 1)\n}\n~ return n\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let ping_sig = sig_of(&result, &index, "ping");
        let pong_sig = sig_of(&result, &index, "pong");
        assert_eq!(
            ping_sig.params,
            vec![Ty::Int],
            "ping's own body only compares n to an int"
        );
        assert_eq!(
            pong_sig.params,
            vec![Ty::Float],
            "pong's own body only compares n to a float"
        );
    }

    #[test]
    #[expect(
        clippy::similar_names,
        reason = "ping/pong are the clearest names for this pair"
    )]
    fn mutual_recursion_return_type_converges_by_fixpoint() {
        // `ping`'s return type is `unify(Float, pong's return type)`; `pong`'s
        // return type is exactly `ping`'s return type. Neither def has a
        // concrete return type on its own — round 0 sees the other's
        // `Unknown` placeholder — so this only converges to `Float` because
        // the batch is re-solved until stable (the SCC fixpoint), not in a
        // single pass.
        let (hir, index, res) = build(
            "=== function ping(n) ===\n{n == 0:\n  ~ return 0.0\n}\n~ return pong(n - 1)\n\
             === function pong(n) ===\n~ return ping(n)\n",
        );
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let ping_sig = sig_of(&result, &index, "ping");
        let pong_sig = sig_of(&result, &index, "pong");
        assert_eq!(ping_sig.return_ty, Ty::Float);
        assert_eq!(pong_sig.return_ty, Ty::Float);
    }

    #[test]
    fn intrinsic_len_types_int() {
        let (hir, index, res) =
            build("=== main ===\n~ temp arr = #[1, 2, 3]\n~ temp n = len(arr)\n-> DONE\n");
        let result = infer_project(&[(FileId(0), &hir)], &index, &res);
        let main_id = index
            .by_name
            .get("main")
            .and_then(|ids| ids.first())
            .copied()
            .expect("main");
        let body = result.bodies.get(&main_id).expect("main body");
        assert_eq!(body.locals.get("arr"), Some(&Ty::Array(Box::new(Ty::Int))));
        assert_eq!(body.locals.get("n"), Some(&Ty::Int));
    }

    #[test]
    fn determinism_same_input_same_output() {
        let src = "=== function fib(n) ===\n{n < 2.0:\n  ~ return n\n}\n~ return fib(n - 1) + fib(n - 2)\n";
        let (hir_a, index_a, res_a) = build(src);
        let (hir_b, index_b, res_b) = build(src);
        let a = infer_project(&[(FileId(0), &hir_a)], &index_a, &res_a);
        let b = infer_project(&[(FileId(0), &hir_b)], &index_b, &res_b);
        assert_eq!(a, b, "same input must infer identical types every run");
    }
}
