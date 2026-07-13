//! TM-3 strict typed-mode policy (docs/typed-mode-spec.md §1/§4/§5/§9-step-3).
//!
//! `types = strict` is a project-level config option, orthogonal to (but
//! gated by) the T1b dialect (`docs/t1b-surface-spec.md` §1): strict typing
//! requires the brink dialect, since its annotation syntax (TM-2, spec §3)
//! is brink-extension syntax. Two jobs live here:
//!
//! - [`config_error`]: `types = strict` + `dialect = strict-ink` is a
//!   project-level config error (`E064`), reported once and skipping every
//!   other strict-mode check (there is nothing more useful to say about a
//!   project whose dialect already rejects the annotation syntax strict mode
//!   needs).
//! - [`check`]: the inference-driven strict diagnostics — Unknown-escape
//!   (`E065`) and Conflicted-escape (`E066`) over every inferable def's
//!   signature and body-local slots (spec §1: "Unknown escaping inference is
//!   a compile error"; the #627-landed `Ty::Conflicted` absorbing point is
//!   strict mode's payoff, spec's own words: "TM-3 (#619) is the slice that
//!   turns a Conflicted slot into a real strict-mode error"), plus wiring
//!   the already-landed advisory `annotations::mismatches` (`E063`) into
//!   production under strict (the inherited #640-round ruling: "TM-3's
//!   strict-policy wiring, which must run inference anyway, is where E063
//!   starts firing in production").
//!
//! A slot is exempted from Unknown-escape when an explicit, resolvable type
//! annotation is present (TM-2's "annotation = firewall" — the entire point
//! of annotating a boundary is to supply the concrete type inference alone
//! couldn't pin down, spec §5's own worked example: `#[]` is an `Unknown`
//! escape *unless* the binding is annotated). A `Conflicted` slot is never
//! exempted by an annotation — the body's own uses genuinely disagree with
//! each other, which no annotation can resolve (`annotations::mismatches`
//! already declines to compare against a `Conflicted`/`Unknown` body type
//! for the same reason, via [`Ty::is_unresolved`]).
//!
//! Coercion lattice (spec §4) and collection-literal joins (spec §5) need no
//! separate enforcement pass here: `infer::ty::unify` already implements the
//! lattice (`int -> float` directional, everything else structurally
//! mismatched joins to `Conflicted`), condition positions are already
//! inferred without forcing `bool` (`infer::body`'s module doc — the
//! int-truthiness idiom `{visited_knot: ...}` types as a clean concrete
//! `int`, never escapes), and a heterogeneous collection literal
//! (`#[1, "a"]`) already comes out `Array(Conflicted)` — this module's
//! recursive [`classify`] walk catches it precisely because it *is* the same
//! lattice, not a parallel implementation of it.
//!
//! ## Scope (see PR description for the full list)
//!
//! This slice does **not** implement: `VAR`/`CONST` cross-type-reassignment
//! detection (the inference substrate never joins a global's declaration-
//! derived type against its assignment sites — `infer::body`'s `observe`
//! only accumulates for `Param`/`Temp` locals; extending it is a `BodyCtx`
//! change, fenced off by #619 itself), the boundary-annotation-*required*
//! diagnostic (spec's "host-callable functions... and entry points require
//! explicit annotations" has no ratified, mechanically-checkable definition
//! of either term in the codebase today — inventing one here would be
//! unilateral architecture, not wiring), or the `int()`/`float()`/`string()`
//! pure conversion intrinsics (they don't exist yet; adding them is new
//! stdlib surface, not diagnostics wiring).

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Content, ContentPart, ElseBranch, FileId, HirFile, IfStmt, Stmt, SymbolIndex,
    SymbolKind, TypeExpr,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::{InferenceResult, Ty};

/// `types` project policy (docs/typed-mode-spec.md §1). `Gradual` (the
/// default) is today's behavior, byte-identical forever — `Unknown` unifies
/// with anything, annotations are optional seasoning, and none of this
/// module's checks run. `Strict` requires `dialect = brink` and turns on
/// [`config_error`]/[`check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypePolicy {
    #[default]
    Gradual,
    Strict,
}

/// `types = strict` + `dialect != brink` is a project-level config error —
/// there is no single offending span, so this reports once, attached to the
/// first file in the project (mirroring how a whole-project condition with
/// no natural per-construct site has to pick *some* file to carry it).
/// `None` when the project has no files at all (nothing to attach to) or the
/// dialect is already `brink` (no error).
#[must_use]
pub fn config_error(
    dialect: crate::Dialect,
    first_file: Option<FileId>,
) -> Option<brink_ir::Diagnostic> {
    if dialect == crate::Dialect::Brink {
        return None;
    }
    let file = first_file?;
    Some(brink_ir::Diagnostic {
        file,
        range: TextRange::new(0.into(), 0.into()),
        message: "types = strict requires dialect = brink — strict typing's annotation syntax \
                   is a brink-dialect extension (docs/typed-mode-spec.md §1); set \
                   `dialect = brink` or drop back to `types = gradual`"
            .to_owned(),
        code: brink_ir::DiagnosticCode::E064,
    })
}

/// The strict-mode diagnostics that need a full `InferenceResult`:
/// Unknown-escape (`E065`), Conflicted-escape (`E066`), and — the inherited
/// #640-round ruling — auto-wiring `annotations::mismatches` (`E063`) into
/// production. Callers only reach this once [`config_error`] has confirmed
/// `dialect = brink`.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = check_escapes(files, index, inference);
    out.extend(annotations::mismatches(files, index, inference));
    out
}

/// Unknown-escape (`E065`) + Conflicted-escape (`E066`) over every inferable
/// def's params, return type (function knots only — an ordinary knot has no
/// return-value concept), and temps.
#[must_use]
fn check_escapes(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<brink_ir::Diagnostic> {
    let list_names = annotations::declared_list_names(index);
    let mut out = Vec::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            let kind = match knot.ptr {
                brink_ir::ContainerPtr::Knot(_) => SymbolKind::Knot,
                brink_ir::ContainerPtr::Stitch(_) => SymbolKind::Stitch,
            };
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
                check_def(
                    id,
                    file,
                    &knot.name.text,
                    knot.name.range,
                    knot.is_function,
                    knot.return_type.as_ref(),
                    &knot.params,
                    &knot.body,
                    &list_names,
                    inference,
                    &mut out,
                );
            }
            for stitch in &knot.stitches {
                let qualified = format!("{}.{}", knot.name.text, stitch.name.text);
                if let Some(id) =
                    annotations::def_id_for(index, file, SymbolKind::Stitch, &qualified)
                {
                    check_def(
                        id,
                        file,
                        &qualified,
                        stitch.name.range,
                        false,
                        None,
                        &stitch.params,
                        &stitch.body,
                        &list_names,
                        inference,
                        &mut out,
                    );
                }
            }
        }
    }
    out
}

#[expect(clippy::too_many_arguments, reason = "internal helper, not public API")]
fn check_def(
    id: DefinitionId,
    file: FileId,
    def_label: &str,
    name_range: TextRange,
    is_function: bool,
    return_type: Option<&TypeExpr>,
    params: &[brink_ir::Param],
    body: &Block,
    list_names: &std::collections::BTreeSet<String>,
    inference: &InferenceResult,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    let Some(sig) = inference.signatures.get(&id) else {
        return;
    };
    let Some(body_types) = inference.bodies.get(&id) else {
        return;
    };

    // Params: an explicit, resolvable annotation supplies the concrete type
    // — TM-2's firewall — and exempts the slot from Unknown-escape *only*.
    // It never exempts Conflicted-escape: the body's own uses genuinely
    // disagree with each other, which no annotation can resolve (mirrors
    // `annotations::mismatches`' `is_unresolved()` treatment of Conflicted).
    for (i, p) in params.iter().enumerate() {
        let annotated = p
            .annotation
            .as_ref()
            .is_some_and(|ann| annotations::resolve(ann, list_names).is_some());
        let ty = sig.params.get(i).unwrap_or(&Ty::Unknown);
        emit_escape(
            file,
            def_label,
            &format!("parameter `{}`", p.name.text),
            p.name.range,
            ty,
            annotated,
            out,
        );
    }

    // Return type: only function knots carry return-value semantics; a
    // `void`-annotated function never needs a concrete return type either.
    if is_function {
        let is_void = return_type
            .is_some_and(|rt| matches!(rt, TypeExpr::Named { name, .. } if name == "void"));
        let annotated =
            is_void || return_type.is_some_and(|rt| annotations::resolve(rt, list_names).is_some());
        if !is_void {
            emit_escape(
                file,
                def_label,
                "return type",
                name_range,
                &sig.return_ty,
                annotated,
                out,
            );
        }
    }

    // Temps: an explicit ascription (`~ temp x: T = ...`) exempts the slot
    // the same way a param annotation does (Unknown-escape only, per above).
    let param_names: std::collections::BTreeSet<&str> =
        params.iter().map(|p| p.name.text.as_str()).collect();
    let temp_decls = collect_temps(body, list_names);
    for (name, ty) in &body_types.locals {
        if param_names.contains(name.as_str()) {
            continue; // already checked above, positionally + annotation-aware
        }
        let decl = temp_decls.get(name);
        let annotated = decl.is_some_and(|d| d.annotation_ty.is_some());
        let range = decl.map_or(name_range, |d| d.range);
        emit_escape(
            file,
            def_label,
            &format!("temp `{name}`"),
            range,
            ty,
            annotated,
            out,
        );
    }
}

/// `annotated`: whether an explicit, resolvable annotation/ascription is
/// present for this slot — exempts an `Unknown` classification (the
/// annotation supplies the type TM-1 alone couldn't pin down) but never a
/// `Conflicted` one (a genuine body-internal contradiction, which no
/// annotation heals).
fn emit_escape(
    file: FileId,
    def_label: &str,
    slot_label: &str,
    range: TextRange,
    ty: &Ty,
    annotated: bool,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match classify(ty) {
        Escape::Clean => {}
        Escape::Unknown if annotated => {}
        Escape::Unknown => out.push(brink_ir::Diagnostic {
            file,
            range,
            message: format!(
                "`{def_label}`'s {slot_label} escapes strict inference as Unknown — \
                 annotate or restructure"
            ),
            code: brink_ir::DiagnosticCode::E065,
        }),
        Escape::Conflicted => out.push(brink_ir::Diagnostic {
            file,
            range,
            message: format!(
                "`{def_label}`'s {slot_label} is Conflicted under strict types — its uses \
                 disagree on its type (observed as `{}`)",
                ty.display()
            ),
            code: brink_ir::DiagnosticCode::E066,
        }),
    }
}

enum Escape {
    Clean,
    Unknown,
    Conflicted,
}

/// Recursively classify a type as clean, an Unknown-escape, or a
/// Conflicted-escape — `Conflicted` wins whenever both appear inside the
/// same `Array`/`Map` nesting (it is the stronger diagnosis: a genuine
/// contradiction, not merely an unconstrained slot).
fn classify(ty: &Ty) -> Escape {
    match ty {
        Ty::Conflicted => Escape::Conflicted,
        Ty::Unknown => Escape::Unknown,
        Ty::Array(elem) => classify(elem),
        Ty::Map(k, v) => match (classify(k), classify(v)) {
            (Escape::Conflicted, _) | (_, Escape::Conflicted) => Escape::Conflicted,
            (Escape::Unknown, _) | (_, Escape::Unknown) => Escape::Unknown,
            (Escape::Clean, Escape::Clean) => Escape::Clean,
        },
        Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Divert | Ty::List(_) => Escape::Clean,
    }
}

/// A temp declaration's own name span plus its resolved ascription type
/// (`None` if unascribed or the ascription doesn't resolve — same "silent,
/// not binding" contract [`annotations::resolve`] documents).
struct TempDecl {
    range: TextRange,
    annotation_ty: Option<Ty>,
}

/// Walk one def's body collecting every `~ temp name[: type] = expr`
/// declaration by bare name (last declaration wins on a shadowed name — this
/// is diagnostic-only positioning, not a binding scope resolution). Mirrors
/// `infer::body`'s and `dialect_gate`'s own recursive shapes: `Stmt`-level
/// nesting (`ChoiceSet`/`Conditional`/`Sequence`/`LabeledBlock`/inline
/// content) plus the closed T1b `~ { … }` `BlockStmt` tree, which needs its
/// own hand-recursion (see `dialect_gate`'s module doc on why).
fn collect_temps(
    body: &Block,
    list_names: &std::collections::BTreeSet<String>,
) -> BTreeMap<String, TempDecl> {
    let mut out = BTreeMap::new();
    collect_temps_block(body, list_names, &mut out);
    out
}

fn collect_temps_block(
    block: &Block,
    list_names: &std::collections::BTreeSet<String>,
    out: &mut BTreeMap<String, TempDecl>,
) {
    for stmt in &block.stmts {
        collect_temps_stmt(stmt, list_names, out);
    }
}

fn collect_temps_stmt(
    stmt: &Stmt,
    list_names: &std::collections::BTreeSet<String>,
    out: &mut BTreeMap<String, TempDecl>,
) {
    match stmt {
        Stmt::TempDecl(t) => {
            let annotation_ty = t
                .annotation
                .as_ref()
                .and_then(|te| annotations::resolve(te, list_names));
            out.insert(
                t.name.text.clone(),
                TempDecl {
                    range: t.name.range,
                    annotation_ty,
                },
            );
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                collect_temps_block(&choice.body, list_names, out);
                if let Some(c) = &choice.start_content {
                    collect_temps_content(c, list_names, out);
                }
                if let Some(c) = &choice.bracket_content {
                    collect_temps_content(c, list_names, out);
                }
                if let Some(c) = &choice.inner_content {
                    collect_temps_content(c, list_names, out);
                }
            }
            collect_temps_block(&cs.continuation, list_names, out);
        }
        Stmt::LabeledBlock(b) => collect_temps_block(b, list_names, out),
        Stmt::Conditional(c) => {
            for branch in &c.branches {
                collect_temps_block(&branch.body, list_names, out);
            }
        }
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                collect_temps_block(branch, list_names, out);
            }
        }
        Stmt::Content(c) => collect_temps_content(c, list_names, out),
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                collect_temps_block_stmt(bs, list_names, out);
            }
        }
        Stmt::Divert(_)
        | Stmt::TunnelCall(_)
        | Stmt::ThreadStart(_)
        | Stmt::Assignment(_)
        | Stmt::Return(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine => {}
    }
}

fn collect_temps_content(
    content: &Content,
    list_names: &std::collections::BTreeSet<String>,
    out: &mut BTreeMap<String, TempDecl>,
) {
    for part in &content.parts {
        match part {
            ContentPart::InlineConditional(c) => {
                for branch in &c.branches {
                    collect_temps_block(&branch.body, list_names, out);
                }
            }
            ContentPart::InlineSequence(s) => {
                for branch in &s.branches {
                    collect_temps_block(branch, list_names, out);
                }
            }
            ContentPart::Interpolation(_)
            | ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring => {}
        }
    }
}

fn collect_temps_block_stmt(
    bs: &BlockStmt,
    list_names: &std::collections::BTreeSet<String>,
    out: &mut BTreeMap<String, TempDecl>,
) {
    match bs {
        BlockStmt::TempDecl(t) => {
            let annotation_ty = t
                .annotation
                .as_ref()
                .and_then(|te| annotations::resolve(te, list_names));
            out.insert(
                t.name.text.clone(),
                TempDecl {
                    range: t.name.range,
                    annotation_ty,
                },
            );
        }
        BlockStmt::If(i) => collect_temps_if(i, list_names, out),
        BlockStmt::While(w) => {
            for s in &w.body {
                collect_temps_block_stmt(s, list_names, out);
            }
        }
        BlockStmt::For(f) => {
            for s in &f.body {
                collect_temps_block_stmt(s, list_names, out);
            }
        }
        BlockStmt::Assignment(_)
        | BlockStmt::Return(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_) => {}
    }
}

fn collect_temps_if(
    i: &IfStmt,
    list_names: &std::collections::BTreeSet<String>,
    out: &mut BTreeMap<String, TempDecl>,
) {
    for s in &i.body {
        collect_temps_block_stmt(s, list_names, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => collect_temps_if(inner, list_names, out),
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                collect_temps_block_stmt(s, list_names, out);
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::{Diagnostic, DiagnosticCode, ResolutionMap, hir::lower};

    fn build(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) = crate::resolve(FileId(0), &manifest, &index);
        (hir, (*index).clone(), (*resolutions).clone())
    }

    fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
        let mut v: Vec<DiagnosticCode> = diags.iter().map(|d| d.code).collect();
        v.sort_by_key(|c| c.as_str());
        v
    }

    // ── config_error ────────────────────────────────────────────────

    #[test]
    fn config_error_fires_for_strict_ink_dialect() {
        let diag = config_error(crate::Dialect::StrictInk, Some(FileId(0)));
        assert!(diag.is_some());
        assert_eq!(diag.expect("checked above").code, DiagnosticCode::E064);
    }

    #[test]
    fn config_error_is_none_for_brink_dialect() {
        assert!(config_error(crate::Dialect::Brink, Some(FileId(0))).is_none());
    }

    #[test]
    fn config_error_is_none_with_no_files() {
        assert!(config_error(crate::Dialect::StrictInk, None).is_none());
    }

    // ── check(): Unknown-escape ────────────────────────────────────

    #[test]
    fn unused_param_escapes_as_unknown() {
        let (hir, index, res) = build("=== noop(x) ===\nHello.\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(diags[0].message.contains('x'));
    }

    #[test]
    fn annotated_unused_param_is_exempt_from_unknown_escape() {
        let (hir, index, res) = build("=== noop(x: int) ===\nHello.\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "annotation supplies the type: {diags:?}");
    }

    #[test]
    fn unconstrained_empty_array_temp_escapes_as_unknown() {
        // spec §5's own worked example.
        let (hir, index, res) = build("=== main ===\n~ temp x = #[]\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    #[test]
    fn annotated_empty_array_temp_is_exempt() {
        // spec §5: "if unconstrained, that's an Unknown escape -> annotate
        // the binding" — following that advice must silence the error.
        let (hir, index, res) = build("=== main ===\n~ temp x: array<int> = #[]\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "ascription supplies the type: {diags:?}");
    }

    #[test]
    fn unannotated_function_return_escapes_as_unknown() {
        let (hir, index, res) = build("=== function noop() ===\nHello.\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(diags[0].message.contains("return"));
    }

    #[test]
    fn void_annotated_function_return_is_exempt() {
        let (hir, index, res) = build("=== function noop(): void ===\n~ return\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_function_knot_return_is_never_checked() {
        // An ordinary knot has no return-value concept at all — never flagged
        // regardless of whether the body ever exercises `~ return`.
        let (hir, index, res) = build("=== main ===\nHello.\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── check(): Conflicted-escape ─────────────────────────────────

    #[test]
    fn genuinely_disjoint_param_uses_escape_as_conflicted() {
        let (hir, index, res) = build(
            "=== conflict_case(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn annotation_never_exempts_a_conflicted_slot() {
        // Annotating a genuinely conflicted param doesn't heal the body's
        // internal contradiction — Conflicted-escape still fires (E063 stays
        // silent for the same reason: `is_unresolved()` covers Conflicted).
        let (hir, index, res) = build(
            "=== conflict_case(hp: int) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    #[test]
    fn heterogeneous_array_literal_temp_escapes_as_conflicted() {
        // spec §5: `#[1, "a"]` is an error — the join lattice already
        // produces `Array(Conflicted)`; this module's recursive classify
        // catches it through the nesting.
        let (hir, index, res) = build("=== main ===\n~ temp x = #[1, \"a\"]\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E066);
    }

    // ── §4 coercion lattice survives strict (regression guards) ────

    #[test]
    fn condition_position_int_truthiness_survives_strict() {
        // `{visited_knot: ...}`-style int truthiness in condition position
        // must never escape — the type resolves cleanly to a concrete `int`.
        let (hir, index, res) = build("=== main ===\nVAR gold = 5\n{gold:\n  rich\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_to_float_join_survives_strict_with_no_escape() {
        let (hir, index, res) = build("=== spend(gold) ===\n{gold > 1.5:\n  ok\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(
            diags.is_empty(),
            "int->float directional join is clean: {diags:?}"
        );
    }

    // ── E063 wiring ──────────────────────────────────────────────────

    #[test]
    fn check_wires_in_e063_mismatches() {
        let (hir, index, res) = build("=== heal(hp: string) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E063),
            "{diags:?}"
        );
    }

    // ── determinism ──────────────────────────────────────────────────

    #[test]
    fn escape_diagnostics_are_order_independent() {
        let forward =
            "=== conflict_fwd(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n";
        let reversed =
            "=== conflict_rev(hp) ===\n{hp == \"no\":\n  no\n}\n{hp > 5:\n  ok\n}\n-> DONE\n";

        let (hir_f, index_f, res_f) = build(forward);
        let inference_f = crate::infer_project(&[(FileId(0), &hir_f)], &index_f, &res_f);
        let diags_f = check(&[(FileId(0), &hir_f)], &index_f, &inference_f);

        let (hir_r, index_r, res_r) = build(reversed);
        let inference_r = crate::infer_project(&[(FileId(0), &hir_r)], &index_r, &res_r);
        let diags_r = check(&[(FileId(0), &hir_r)], &index_r, &inference_r);

        assert_eq!(codes(&diags_f), vec![DiagnosticCode::E066]);
        assert_eq!(codes(&diags_r), vec![DiagnosticCode::E066]);
    }

    #[test]
    fn clean_strict_project_compiles_with_no_strict_diagnostics() {
        let (hir, index, res) = build(
            "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
