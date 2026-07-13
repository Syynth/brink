//! TM-3 strict typed-mode policy (docs/typed-mode-spec.md §1/§4/§5/§9-step-3).
//!
//! `types = strict` is a project-level config option, orthogonal to (but
//! gated by) the T1b dialect (`docs/t1b-surface-spec.md` §1): strict typing
//! requires the brink dialect, since its annotation syntax (TM-2, spec §3)
//! is brink-extension syntax. Three jobs live here:
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
//!   turns a Conflicted slot into a real strict-mode error"), the void-
//!   assignment check (`E067`, spec §3: "assigning a `void` call is an error
//!   in strict mode" — a `~ x = f()` / `~ temp x = f()` whose RHS *root* is a
//!   call resolving to a `void`-returning function; statement-position calls
//!   and calls nested in interpolation are never flagged), plus wiring the
//!   already-landed advisory `annotations::mismatches` (`E063`) into
//!   production under strict (the inherited #640-round ruling: "TM-3's
//!   strict-policy wiring, which must run inference anyway, is where E063
//!   starts firing in production").
//! - [`effective_severity`]: the policy-conditional severity lookup both of
//!   `brink-db`'s diagnostic-partitioning sites (`partition_diagnostics`'s
//!   two call sites, plus `lir_query`'s own LIR-diagnostic partition) must
//!   call instead of the raw [`brink_ir::DiagnosticCode::severity`] default —
//!   `E063` is `Warning` under `types = gradual` but `Error`-eligible under
//!   `types = strict` (the #640-round ruling this module's `check` doc above
//!   already cites); every other code's severity is policy-independent.
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

use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::{
    Block, BlockStmt, Content, ContentPart, ElseBranch, Expr, FileId, HirFile, IfStmt, Path,
    ResolutionMap, Stmt, SymbolIndex, SymbolKind, TypeExpr,
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

/// The severity a diagnostic code should actually be reported at, given the
/// project's `types` policy — the single seam every diagnostic-partitioning
/// site must call instead of the raw [`brink_ir::DiagnosticCode::severity`]
/// default (the #640-round ruling: "TM-3's strict-policy wiring, which must
/// run inference anyway, is where E063 starts firing in production", i.e.
/// `E063` — annotation-vs-inference mismatch — is advisory (`Warning`) under
/// `types = gradual` but error-eligible under `types = strict`). Every other
/// code's severity is policy-independent and this simply defers to
/// [`brink_ir::DiagnosticCode::severity`].
#[must_use]
pub fn effective_severity(code: brink_ir::DiagnosticCode, types: TypePolicy) -> brink_ir::Severity {
    if code == brink_ir::DiagnosticCode::E063 && types == TypePolicy::Strict {
        brink_ir::Severity::Error
    } else {
        code.severity()
    }
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
/// Unknown-escape (`E065`), Conflicted-escape (`E066`), void-assignment
/// (`E067`), and — the inherited #640-round ruling — auto-wiring
/// `annotations::mismatches` (`E063`) into production. Callers only reach
/// this once [`config_error`] has confirmed `dialect = brink`.
///
/// `resolutions`: the project's full resolution map — the void-assignment
/// pass needs it to resolve a call-site's `Path` back to the def it targets
/// (the same range→`DefinitionId` lookup `infer::body` builds its own
/// per-file projection of).
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    resolutions: &ResolutionMap,
) -> Vec<brink_ir::Diagnostic> {
    let mut out = check_escapes(files, index, inference);
    out.extend(annotations::mismatches(files, index, inference));
    out.extend(check_void_assignments(files, index, resolutions));
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

// ── Void-assignment (E067, docs/typed-mode-spec.md §3) ────────────────

/// `(start, end)` `u32` pair — `TextRange` has no `Ord` impl, so every
/// `BTreeMap` keyed by a source range in this module uses this instead
/// (mirrors `infer::mod`'s own `range_key`).
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// `~ x = f()` / `~ temp x = f()` where `f`'s resolved def is a `void`-
/// returning function is a compile error under strict (spec §3: "assigning a
/// `void` call is an error in strict mode"). Only the assignment/temp-decl's
/// RHS *root* expression is checked — a statement-position call (`~ f()`) or
/// a call nested inside interpolation is never flagged, since neither
/// assigns the (nonexistent) result anywhere.
#[must_use]
fn check_void_assignments(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<brink_ir::Diagnostic> {
    let void_defs = collect_void_defs(files, index);
    if void_defs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &(file, hir) in files {
        let resolution_by_range = resolution_index(resolutions, file);
        for knot in &hir.knots {
            check_void_block(file, &knot.body, &void_defs, &resolution_by_range, &mut out);
            for stitch in &knot.stitches {
                check_void_block(
                    file,
                    &stitch.body,
                    &void_defs,
                    &resolution_by_range,
                    &mut out,
                );
            }
        }
    }
    out
}

/// Every function knot whose `): void ===` return annotation resolves to
/// `void`, by `DefinitionId`. Stitches never carry `return_type` (only
/// `Knot` does — see the field's doc comment), so only `hir.knots` entries
/// with `is_function` set are candidates, mirroring `check_escapes`' own
/// def-id lookup (`kind` tracks `knot.ptr`, since a top-level stitch
/// promoted to knot status is indexed under `SymbolKind::Stitch`, #626).
fn collect_void_defs(files: &[(FileId, &HirFile)], index: &SymbolIndex) -> BTreeSet<DefinitionId> {
    let mut out = BTreeSet::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            if !knot.is_function {
                continue;
            }
            let is_void = knot
                .return_type
                .as_ref()
                .is_some_and(|rt| matches!(rt, TypeExpr::Named { name, .. } if name == "void"));
            if !is_void {
                continue;
            }
            let kind = match knot.ptr {
                brink_ir::ContainerPtr::Knot(_) => SymbolKind::Knot,
                brink_ir::ContainerPtr::Stitch(_) => SymbolKind::Stitch,
            };
            if let Some(id) = annotations::def_id_for(index, file, kind, &knot.name.text) {
                out.insert(id);
            }
        }
    }
    out
}

/// This file's own reference resolutions, projected to a range-keyed lookup
/// (mirrors `infer::mod`'s `index_resolutions_by_file`, narrowed to one file
/// at a time — a `Path`'s range is only unique within its own file).
fn resolution_index(
    resolutions: &ResolutionMap,
    file: FileId,
) -> BTreeMap<(u32, u32), DefinitionId> {
    resolutions
        .iter()
        .filter(|r| r.file == file)
        .map(|r| (range_key(r.range), r.target))
        .collect()
}

fn check_void_block(
    file: FileId,
    block: &Block,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    for stmt in &block.stmts {
        check_void_stmt(file, stmt, void_defs, resolution_by_range, out);
    }
}

fn check_void_stmt(
    file: FileId,
    stmt: &Stmt,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match stmt {
        Stmt::TempDecl(t) => {
            if let Some(value) = &t.value {
                check_void_root(file, value, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Assignment(a) => {
            check_void_root(file, &a.value, void_defs, resolution_by_range, out);
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &cs.choices {
                check_void_block(file, &choice.body, void_defs, resolution_by_range, out);
                if let Some(c) = &choice.start_content {
                    check_void_content(file, c, void_defs, resolution_by_range, out);
                }
                if let Some(c) = &choice.bracket_content {
                    check_void_content(file, c, void_defs, resolution_by_range, out);
                }
                if let Some(c) = &choice.inner_content {
                    check_void_content(file, c, void_defs, resolution_by_range, out);
                }
            }
            check_void_block(file, &cs.continuation, void_defs, resolution_by_range, out);
        }
        Stmt::LabeledBlock(b) => check_void_block(file, b, void_defs, resolution_by_range, out),
        Stmt::Conditional(c) => {
            for branch in &c.branches {
                check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Sequence(s) => {
            for branch in &s.branches {
                check_void_block(file, branch, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Content(c) => check_void_content(file, c, void_defs, resolution_by_range, out),
        Stmt::LogicBlock(lb) => {
            for bs in &lb.stmts {
                check_void_block_stmt(file, bs, void_defs, resolution_by_range, out);
            }
        }
        Stmt::Divert(_)
        | Stmt::TunnelCall(_)
        | Stmt::ThreadStart(_)
        | Stmt::Return(_)
        | Stmt::ExprStmt(_)
        | Stmt::EndOfLine => {}
    }
}

fn check_void_content(
    file: FileId,
    content: &Content,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    for part in &content.parts {
        match part {
            ContentPart::InlineConditional(c) => {
                for branch in &c.branches {
                    check_void_block(file, &branch.body, void_defs, resolution_by_range, out);
                }
            }
            ContentPart::InlineSequence(s) => {
                for branch in &s.branches {
                    check_void_block(file, branch, void_defs, resolution_by_range, out);
                }
            }
            ContentPart::Interpolation(_)
            | ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring => {}
        }
    }
}

fn check_void_block_stmt(
    file: FileId,
    bs: &BlockStmt,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    match bs {
        BlockStmt::TempDecl(t) => {
            if let Some(value) = &t.value {
                check_void_root(file, value, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::Assignment(a) => {
            check_void_root(file, &a.value, void_defs, resolution_by_range, out);
        }
        BlockStmt::If(i) => check_void_if(file, i, void_defs, resolution_by_range, out),
        BlockStmt::While(w) => {
            for s in &w.body {
                check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::For(f) => {
            for s in &f.body {
                check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
            }
        }
        BlockStmt::Return(_)
        | BlockStmt::ExprStmt(_)
        | BlockStmt::Break(_)
        | BlockStmt::Continue(_) => {}
    }
}

fn check_void_if(
    file: FileId,
    i: &IfStmt,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    for s in &i.body {
        check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
    }
    match &i.else_branch {
        Some(ElseBranch::ElseIf(inner)) => {
            check_void_if(file, inner, void_defs, resolution_by_range, out);
        }
        Some(ElseBranch::Else(stmts)) => {
            for s in stmts {
                check_void_block_stmt(file, s, void_defs, resolution_by_range, out);
            }
        }
        None => {}
    }
}

/// If `expr`'s root is `Expr::Call(path, _)` resolving to a def in
/// `void_defs`, push `E067`. Anything else (a non-call root, or a call that
/// doesn't resolve to a known void def) is silently clean — this is a root-
/// position-only check, never a recursive expression walk (a void call
/// buried inside e.g. `1 + f()` is a type error `infer::body` would already
/// have caught as a non-numeric operand, not this diagnostic's job).
fn check_void_root(
    file: FileId,
    expr: &Expr,
    void_defs: &BTreeSet<DefinitionId>,
    resolution_by_range: &BTreeMap<(u32, u32), DefinitionId>,
    out: &mut Vec<brink_ir::Diagnostic>,
) {
    let Expr::Call(path, _) = expr else {
        return;
    };
    let Some(&def_id) = resolution_by_range.get(&range_key(path.range)) else {
        return;
    };
    if !void_defs.contains(&def_id) {
        return;
    }
    out.push(brink_ir::Diagnostic {
        file,
        range: path.range,
        message: format!(
            "`{}` returns void — its result cannot be assigned (docs/typed-mode-spec.md §3)",
            path_display(path)
        ),
        code: brink_ir::DiagnosticCode::E067,
    });
}

/// Dotted display name for a call target's `Path` (e.g. `knot.stitch`).
fn path_display(path: &Path) -> String {
    path.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
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
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(diags[0].message.contains('x'));
    }

    #[test]
    fn annotated_unused_param_is_exempt_from_unknown_escape() {
        let (hir, index, res) = build("=== noop(x: int) ===\nHello.\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(diags.is_empty(), "annotation supplies the type: {diags:?}");
    }

    #[test]
    fn unconstrained_empty_array_temp_escapes_as_unknown() {
        // spec §5's own worked example.
        let (hir, index, res) = build("=== main ===\n~ temp x = #[]\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
    }

    #[test]
    fn annotated_empty_array_temp_is_exempt() {
        // spec §5: "if unconstrained, that's an Unknown escape -> annotate
        // the binding" — following that advice must silence the error.
        let (hir, index, res) = build("=== main ===\n~ temp x: array<int> = #[]\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(diags.is_empty(), "ascription supplies the type: {diags:?}");
    }

    #[test]
    fn unannotated_function_return_escapes_as_unknown() {
        let (hir, index, res) = build("=== function noop() ===\nHello.\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E065);
        assert!(diags[0].message.contains("return"));
    }

    #[test]
    fn void_annotated_function_return_is_exempt() {
        let (hir, index, res) = build("=== function noop(): void ===\n~ return\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn non_function_knot_return_is_never_checked() {
        // An ordinary knot has no return-value concept at all — never flagged
        // regardless of whether the body ever exercises `~ return`.
        let (hir, index, res) = build("=== main ===\nHello.\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── check(): Conflicted-escape ─────────────────────────────────

    #[test]
    fn genuinely_disjoint_param_uses_escape_as_conflicted() {
        let (hir, index, res) = build(
            "=== conflict_case(hp) ===\n{hp > 5:\n  ok\n}\n{hp == \"no\":\n  no\n}\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
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
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
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
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
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
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn int_to_float_join_survives_strict_with_no_escape() {
        let (hir, index, res) = build("=== spend(gold) ===\n{gold > 1.5:\n  ok\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
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
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
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
        let diags_f = check(&[(FileId(0), &hir_f)], &index_f, &inference_f, &res_f);

        let (hir_r, index_r, res_r) = build(reversed);
        let inference_r = crate::infer_project(&[(FileId(0), &hir_r)], &index_r, &res_r);
        let diags_r = check(&[(FileId(0), &hir_r)], &index_r, &inference_r, &res_r);

        assert_eq!(codes(&diags_f), vec![DiagnosticCode::E066]);
        assert_eq!(codes(&diags_r), vec![DiagnosticCode::E066]);
    }

    #[test]
    fn clean_strict_project_compiles_with_no_strict_diagnostics() {
        let (hir, index, res) = build(
            "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── effective_severity ──────────────────────────────────────────

    #[test]
    fn effective_severity_e063_is_warning_under_gradual() {
        assert_eq!(
            effective_severity(DiagnosticCode::E063, TypePolicy::Gradual),
            brink_ir::Severity::Warning
        );
    }

    #[test]
    fn effective_severity_e063_is_error_under_strict() {
        assert_eq!(
            effective_severity(DiagnosticCode::E063, TypePolicy::Strict),
            brink_ir::Severity::Error
        );
    }

    #[test]
    fn effective_severity_other_codes_are_policy_independent() {
        // A code with no strict-conditional carve-out keeps its default
        // severity regardless of policy — only E063 is ever conditioned.
        for policy in [TypePolicy::Gradual, TypePolicy::Strict] {
            assert_eq!(
                effective_severity(DiagnosticCode::E065, policy),
                DiagnosticCode::E065.severity()
            );
            assert_eq!(
                effective_severity(DiagnosticCode::E022, policy),
                DiagnosticCode::E022.severity()
            );
        }
    }

    // ── check(): void-assignment (E067) ────────────────────────────

    #[test]
    fn void_assigned_to_temp_is_e067() {
        let (hir, index, res) = build(
            "=== function noop(): void ===\n~ return\n\
             === main ===\n~ temp x = noop()\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn void_assigned_to_var_is_e067() {
        let (hir, index, res) = build(
            "VAR gold = 0\n=== function noop(): void ===\n~ return\n\
             === main ===\n~ gold = noop()\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn void_call_in_statement_position_is_clean() {
        let (hir, index, res) = build(
            "=== function noop(): void ===\n~ return\n\
             === main ===\n~ noop()\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "statement-position void call must never be flagged: {diags:?}"
        );
    }

    #[test]
    fn non_void_call_assigned_is_clean_of_e067() {
        let (hir, index, res) = build(
            "=== function give(): int ===\n~ return 5\n\
             === main ===\n~ temp x: int = give()\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = check(&[(FileId(0), &hir)], &index, &inference, &res);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E067),
            "{diags:?}"
        );
    }

    #[test]
    fn void_assignment_never_checked_under_gradual() {
        // `check`'s void-assignment pass is unconditional — it's
        // `finish_analysis` that gates the whole `strict::check` call behind
        // `opts.types == TypePolicy::Strict`. Exercise that real gate (not
        // `check` directly) to prove a void assignment stays silent under
        // gradual, matching this module's "byte-identical forever" contract.
        let parsed = brink_syntax::parse(
            "=== function noop(): void ===\n~ return\n\
             === main ===\n~ temp x = noop()\n-> DONE\n",
        );
        let (hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let opts = crate::AnalysisOptions {
            dialect: crate::Dialect::Brink,
            ..crate::AnalysisOptions::default()
        };
        let result = crate::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::E067),
            "gradual must never surface E067: {:?}",
            result.diagnostics
        );
    }
}
