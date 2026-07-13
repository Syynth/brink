//! TM-2 inline type annotation resolution + `signature()` firewall wiring
//! (docs/typed-mode-spec.md §3).
//!
//! Two independent jobs, both consuming already-lowered HIR (never touching
//! `infer::body`'s `BodyCtx` — that rework is fenced off, #638):
//!
//! - [`resolve`]: turn a parsed [`brink_ir::TypeExpr`] into the checker's
//!   [`Ty`] universe, for `signature()` to carry as its firewall (annotation
//!   wins over inference — see `crate::signature::Sig`'s `param_annotations`/
//!   `return_annotation` fields, populated via this function).
//! - [`check`]: semantic diagnostics on the annotation *content* — unknown
//!   type names (`E061`) and `fn(...)` function types, which parse
//!   everywhere but type as reserved until T1c (`E062`). Runs only under
//!   the brink dialect (`finish_analysis` gates the call): under
//!   `strict-ink`, `dialect_gate` already rejects the annotation whole as
//!   extension syntax (`E051`), and content diagnostics on rejected syntax
//!   are noise (maintainer ruling 2026-07-13).
//!
//! [`mismatches`] is the third job: the annotation-vs-body-inference
//! diagnostic (`E063`), composing `signature()`'s annotations with
//! `infer_project`'s already-computed body-derived types — a pure consumer
//! of both public seams, touching neither's internals (per the fence: "no
//! changes to the FG query decomposition beyond consuming its public seam").

use std::collections::BTreeSet;

use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, Knot, Stitch, SymbolIndex, SymbolKind,
};

use crate::infer::{InferenceResult, Ty};

/// Recognized bare nominal leaf names (typed-mode-spec §3): everything except
/// the generic heads (`list`/`array`/`map`) and the reserved function-type
/// keyword (`fn`), which are grammar/semantic concerns of their own.
fn is_known_leaf(name: &str) -> bool {
    matches!(
        name,
        "int" | "float" | "bool" | "string" | "divert" | "void"
    )
}

/// Resolve a parsed type annotation into the checker's `Ty` universe.
///
/// Returns `None` for `void` (no `Ty` — return-position-only, handled
/// separately by callers that care), `fn(...)` (reserved until T1c — no
/// `Ty::Fn` variant exists, spec §2), and any name this function doesn't
/// recognize (an unknown leaf name, or a `list<L>` whose `L` isn't a
/// declared `LIST` — [`check`] is what reports these, not this function).
///
/// `struct_names` (TM-4b, docs/typed-mode-spec.md §6): a bare `Named` type
/// whose name is a declared `STRUCT` resolves to `Ty::Struct` — "declared
/// struct names join the TM-2 annotation type grammar", the same join
/// `list_names` gives `list<L>`. Checked after the fixed scalar-keyword set
/// so a struct can never shadow `int`/`float`/etc. (those names aren't
/// legal `STRUCT` identifiers by convention, but this ordering is the
/// unambiguous choice regardless).
#[must_use]
pub fn resolve(
    te: &brink_ir::TypeExpr,
    list_names: &BTreeSet<String>,
    struct_names: &BTreeSet<String>,
) -> Option<Ty> {
    match te {
        brink_ir::TypeExpr::Named { name, .. } => match name.as_str() {
            "int" => Some(Ty::Int),
            "float" => Some(Ty::Float),
            "bool" => Some(Ty::Bool),
            "string" => Some(Ty::String),
            "divert" => Some(Ty::Divert),
            _ if struct_names.contains(name) => Some(Ty::Struct(name.clone())),
            _ => None, // "void", or an unrecognized/unknown name
        },
        brink_ir::TypeExpr::Generic { name, args, .. } => match name.as_str() {
            "list" if args.len() == 1 => match &args[0] {
                brink_ir::TypeExpr::Named { name: l, .. } if list_names.contains(l) => {
                    Some(Ty::List(l.clone()))
                }
                _ => None,
            },
            "array" if args.len() == 1 => {
                resolve(&args[0], list_names, struct_names).map(|t| Ty::Array(Box::new(t)))
            }
            "map" if args.len() == 2 => {
                let k = resolve(&args[0], list_names, struct_names)?;
                let v = resolve(&args[1], list_names, struct_names)?;
                Some(Ty::Map(Box::new(k), Box::new(v)))
            }
            _ => None,
        },
        brink_ir::TypeExpr::Fn { .. } => None, // reserved until T1c
    }
}

/// Every declared `LIST` name in the project — `list<L>` is nominal per the
/// declaring `LIST` (spec §2/§3), so validating/resolving it needs project-
/// wide knowledge, same as every other cross-file lookup in this crate.
pub(crate) fn declared_list_names(index: &SymbolIndex) -> BTreeSet<String> {
    index
        .symbols
        .values()
        .filter(|s| s.kind == SymbolKind::List)
        .map(|s| s.name.clone())
        .collect()
}

/// Every declared `STRUCT` name in the project (TM-4b, docs/typed-mode-spec.md
/// §6) — mirrors [`declared_list_names`] exactly for the same reason: a
/// struct name is nominal, and joining the annotation grammar needs
/// project-wide knowledge.
pub(crate) fn declared_struct_names(index: &SymbolIndex) -> BTreeSet<String> {
    index
        .symbols
        .values()
        .filter(|s| s.kind == SymbolKind::Struct)
        .map(|s| s.name.clone())
        .collect()
}

/// Semantic diagnostics on annotation content: unknown type names (`E061`)
/// and reserved-until-T1c function types (`E062`). Unconditional in both
/// dialects (see module doc).
#[must_use]
pub fn check(files: &[(FileId, &HirFile)], index: &SymbolIndex) -> Vec<Diagnostic> {
    let list_names = declared_list_names(index);
    let struct_names = declared_struct_names(index);
    let mut out = Vec::new();
    for &(file, hir) in files {
        for v in &hir.variables {
            if let Some(te) = &v.annotation {
                check_one(te, &list_names, &struct_names, file, &mut out);
            }
        }
        for c in &hir.constants {
            if let Some(te) = &c.annotation {
                check_one(te, &list_names, &struct_names, file, &mut out);
            }
        }
        for knot in &hir.knots {
            check_knot(knot, file, &list_names, &struct_names, &mut out);
        }
    }
    out
}

fn check_knot(
    knot: &Knot,
    file: FileId,
    list_names: &BTreeSet<String>,
    struct_names: &BTreeSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    for p in &knot.params {
        if let Some(te) = &p.annotation {
            check_one(te, list_names, struct_names, file, out);
        }
    }
    if let Some(rt) = &knot.return_type {
        check_one(rt, list_names, struct_names, file, out);
    }
    for stitch in &knot.stitches {
        check_stitch(stitch, file, list_names, struct_names, out);
    }
}

fn check_stitch(
    stitch: &Stitch,
    file: FileId,
    list_names: &BTreeSet<String>,
    struct_names: &BTreeSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    for p in &stitch.params {
        if let Some(te) = &p.annotation {
            check_one(te, list_names, struct_names, file, out);
        }
    }
}

/// Check one type expression (and recursively, its generic args / fn
/// params+return) for unknown names / reserved fn-types.
fn check_one(
    te: &brink_ir::TypeExpr,
    list_names: &BTreeSet<String>,
    struct_names: &BTreeSet<String>,
    file: FileId,
    out: &mut Vec<Diagnostic>,
) {
    match te {
        brink_ir::TypeExpr::Named { name, range } => {
            // TM-4b (docs/typed-mode-spec.md §6): "declared struct names
            // join the TM-2 annotation type grammar... E061 no longer fires
            // for a declared name".
            if !is_known_leaf(name) && !struct_names.contains(name) {
                out.push(Diagnostic {
                    file,
                    range: *range,
                    message: format!(
                        "`{name}` is not a recognized type — expected int, float, bool, \
                         string, divert, void, list<L>, array<T>, map<K, V>, or a declared \
                         STRUCT name"
                    ),
                    code: DiagnosticCode::E061,
                });
            }
        }
        brink_ir::TypeExpr::Generic { name, args, range } => match name.as_str() {
            "list" => {
                let bad = match args.as_slice() {
                    [brink_ir::TypeExpr::Named { name: l, .. }] => !list_names.contains(l),
                    _ => true,
                };
                if bad {
                    out.push(Diagnostic {
                        file,
                        range: *range,
                        message: format!(
                            "`list<{}>` doesn't name a declared LIST",
                            args.first().map_or(String::new(), display_short)
                        ),
                        code: DiagnosticCode::E061,
                    });
                }
            }
            "array" | "map" => {
                for a in args {
                    check_one(a, list_names, struct_names, file, out);
                }
            }
            _ => {
                out.push(Diagnostic {
                    file,
                    range: *range,
                    message: format!("`{name}<...>` is not a recognized generic type"),
                    code: DiagnosticCode::E061,
                });
            }
        },
        brink_ir::TypeExpr::Fn { params, ret, range } => {
            out.push(Diagnostic {
                file,
                range: *range,
                message: "function types (`fn(...): R`) land with T1c — not usable yet".to_owned(),
                code: DiagnosticCode::E062,
            });
            for p in params {
                check_one(p, list_names, struct_names, file, out);
            }
            check_one(ret, list_names, struct_names, file, out);
        }
    }
}

fn display_short(te: &brink_ir::TypeExpr) -> String {
    match te {
        brink_ir::TypeExpr::Named { name, .. } => name.clone(),
        brink_ir::TypeExpr::Generic { name, .. } => format!("{name}<...>"),
        brink_ir::TypeExpr::Fn { .. } => "fn(...)".to_owned(),
    }
}

// ─── Signature-firewall mismatch (E063) ──────────────────────────────

/// Compare each def's annotated param/return types (`Sig`, declaration-only)
/// against the same def's body-inferred types (`InferenceResult`, from
/// `infer_project`/the composed `call_edges`→`solve_scc` path) and report a
/// disagreement. Advisory-only (`E063` is a warning) — severity policy for
/// strict mode is TM-3's call, not this one's.
///
/// A pure consumer of two already-public seams: never touches
/// `infer::body`'s internals, never re-solves anything.
#[must_use]
pub fn mismatches(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
) -> Vec<Diagnostic> {
    let list_names = declared_list_names(index);
    let struct_names = declared_struct_names(index);
    let mut out = Vec::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            check_def_mismatch(
                knot,
                file,
                index,
                &list_names,
                &struct_names,
                inference,
                &mut out,
            );
            for stitch in &knot.stitches {
                check_stitch_mismatch(
                    stitch,
                    file,
                    index,
                    &list_names,
                    &struct_names,
                    inference,
                    &mut out,
                );
            }
        }
    }
    out
}

pub(crate) fn def_id_for(
    index: &SymbolIndex,
    file: FileId,
    kind: SymbolKind,
    name: &str,
) -> Option<DefinitionId> {
    index
        .by_name
        .get(name)?
        .iter()
        .find(|id| {
            index
                .symbols
                .get(id)
                .is_some_and(|info| info.file == file && info.kind == kind)
        })
        .copied()
}

fn check_def_mismatch(
    knot: &Knot,
    file: FileId,
    index: &SymbolIndex,
    list_names: &BTreeSet<String>,
    struct_names: &BTreeSet<String>,
    inference: &InferenceResult,
    out: &mut Vec<Diagnostic>,
) {
    let Some(id) = def_id_for(index, file, SymbolKind::Knot, &knot.name.text) else {
        return;
    };
    let Some(inferred) = inference.signatures.get(&id) else {
        return;
    };
    for (i, p) in knot.params.iter().enumerate() {
        let Some(ann) = &p.annotation else { continue };
        let Some(ann_ty) = resolve(ann, list_names, struct_names) else {
            continue;
        };
        let Some(body_ty) = inferred.params.get(i) else {
            continue;
        };
        report_if_mismatched(ann, &ann_ty, body_ty, file, out);
    }
    if let Some(rt) = &knot.return_type
        && let Some(ann_ty) = resolve(rt, list_names, struct_names)
    {
        report_if_mismatched(rt, &ann_ty, &inferred.return_ty, file, out);
    }
}

fn check_stitch_mismatch(
    stitch: &Stitch,
    file: FileId,
    index: &SymbolIndex,
    list_names: &BTreeSet<String>,
    struct_names: &BTreeSet<String>,
    inference: &InferenceResult,
    out: &mut Vec<Diagnostic>,
) {
    let Some(id) = def_id_for(index, file, SymbolKind::Stitch, &stitch.name.text) else {
        return;
    };
    let Some(inferred) = inference.signatures.get(&id) else {
        return;
    };
    for (i, p) in stitch.params.iter().enumerate() {
        let Some(ann) = &p.annotation else { continue };
        let Some(ann_ty) = resolve(ann, list_names, struct_names) else {
            continue;
        };
        let Some(body_ty) = inferred.params.get(i) else {
            continue;
        };
        report_if_mismatched(ann, &ann_ty, body_ty, file, out);
    }
}

/// `body_ty` disagrees with `ann_ty` when the body implies something
/// concrete that isn't the annotation itself and isn't absorbed by it
/// (`Unknown` never disagrees — an unused/unconstrained slot is silent, not
/// a mismatch; `unify(ann, body) == ann` covers the one legal directional
/// coercion, `int` annotated but body only ever compares against `int`
/// literals promoted to `float` nowhere, etc.). `Conflicted` (#627) reads
/// the same as `Unknown` here too: E063 is gradual/advisory (never wired
/// into `finish_analysis`), and reporting a *conflicted* slot specifically
/// is strict mode's TM-3 (#619) job, not this diagnostic's — see
/// [`Ty::is_unresolved`].
fn report_if_mismatched(
    te: &brink_ir::TypeExpr,
    ann_ty: &Ty,
    body_ty: &Ty,
    file: FileId,
    out: &mut Vec<Diagnostic>,
) {
    if body_ty.is_unresolved() || body_ty == ann_ty {
        return;
    }
    if &crate::infer::unify(ann_ty, body_ty) == ann_ty {
        return;
    }
    out.push(Diagnostic {
        file,
        range: te.range(),
        message: format!(
            "annotated type `{}` disagrees with the type inferred from usage (`{}`)",
            ann_ty.display(),
            body_ty.display()
        ),
        code: DiagnosticCode::E063,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::ResolutionMap;
    use brink_ir::hir::lower;

    fn build(src: &str) -> (HirFile, SymbolIndex) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        (hir, (*index).clone())
    }

    /// Like [`build`], but also computes real resolutions — needed by the
    /// `mismatches()` tests: `infer_project` resolves body references (e.g.
    /// `hp` inside a knot body back to its own param) via the resolution
    /// map, same as `infer::tests::build`'s helper does.
    fn build_with_resolutions(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) = crate::resolve(FileId(0), &manifest, &index);
        (hir, (*index).clone(), (*resolutions).clone())
    }

    // ── resolve() ───────────────────────────────────────────────────

    #[test]
    fn resolve_recognizes_scalar_leaves() {
        let (hir, _index) = build("VAR a: int = 1\nVAR b: float = 1.0\nVAR c: bool = true\n");
        let a = hir.variables[0].annotation.as_ref().expect("a annotation");
        let b = hir.variables[1].annotation.as_ref().expect("b annotation");
        let c = hir.variables[2].annotation.as_ref().expect("c annotation");
        let empty = BTreeSet::new();
        assert_eq!(resolve(a, &empty, &empty), Some(Ty::Int));
        assert_eq!(resolve(b, &empty, &empty), Some(Ty::Float));
        assert_eq!(resolve(c, &empty, &empty), Some(Ty::Bool));
    }

    #[test]
    fn resolve_array_and_map_generics() {
        let (hir, _index) = build("VAR a: array<int> = 0\nVAR m: map<string, int> = 0\n");
        let a = hir.variables[0].annotation.as_ref().expect("a");
        let m = hir.variables[1].annotation.as_ref().expect("m");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(a, &empty, &empty),
            Some(Ty::Array(Box::new(Ty::Int)))
        );
        assert_eq!(
            resolve(m, &empty, &empty),
            Some(Ty::Map(Box::new(Ty::String), Box::new(Ty::Int)))
        );
    }

    #[test]
    fn resolve_list_generic_needs_declared_list_name() {
        let (hir, _index) = build("VAR w: list<Weathers> = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(te, &empty, &empty),
            None,
            "Weathers isn't declared here"
        );
        let declared: BTreeSet<String> = ["Weathers".to_string()].into_iter().collect();
        assert_eq!(
            resolve(te, &declared, &empty),
            Some(Ty::List("Weathers".to_string()))
        );
    }

    #[test]
    fn resolve_void_and_fn_and_unknown_are_none() {
        let (hir, _index) =
            build("VAR v: void = 0\nVAR f: fn(int): int = 0\nVAR u: Frobnicator = 0\n");
        let empty = BTreeSet::new();
        for v in &hir.variables {
            let te = v.annotation.as_ref().expect("annotation");
            assert_eq!(resolve(te, &empty, &empty), None, "{v:?}");
        }
    }

    #[test]
    fn resolve_recognizes_declared_struct_name() {
        // TM-4b: "declared struct names join the TM-2 annotation type
        // grammar" — a bare `Named` type whose name is a declared `STRUCT`
        // resolves to `Ty::Struct`, same join `list_names` gives `list<L>`.
        let (hir, _index) = build("STRUCT Point = #{x: float}\nVAR p: Point = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(te, &empty, &empty),
            None,
            "Point isn't in struct_names here"
        );
        let declared: BTreeSet<String> = ["Point".to_string()].into_iter().collect();
        assert_eq!(
            resolve(te, &empty, &declared),
            Some(Ty::Struct("Point".to_string()))
        );
    }

    // ── check() ─────────────────────────────────────────────────────

    #[test]
    fn check_flags_unknown_type_name() {
        let (hir, index) = build("VAR p: Frobnicator = 0\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_flags_fn_type_as_reserved() {
        let (hir, index) = build("VAR cb: fn(int, int): bool = 0\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E062);
    }

    #[test]
    fn check_accepts_known_scalar_and_generic_types() {
        let (hir, index) =
            build("VAR a: int = 1\nVAR b: array<float> = 0\nVAR c: map<string, bool> = 0\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_accepts_void_return_type() {
        let (hir, index) = build("=== function noop(): void ===\n~ return\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_accepts_declared_list_name() {
        let (hir, index) = build("LIST Weathers = sunny, rainy\nVAR w: list<Weathers> = sunny\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// TM-4b: "E061 no longer fires for a declared [struct] name".
    #[test]
    fn check_accepts_declared_struct_name() {
        let (hir, index) = build("STRUCT Point = #{x: float}\nVAR p: Point = 0\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_flags_undeclared_struct_name_still() {
        // A name that isn't a known scalar, generic head, or declared
        // struct still flags E061 — TM-4b only widens the accepted set.
        let (hir, index) = build("VAR w: NotAStruct = 0\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_flags_undeclared_list_name() {
        let (hir, index) = build("VAR w: list<Nope> = 0\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_flags_param_and_return_type_annotations() {
        let (hir, index) = build("=== function heal(hp: Bogus): AlsoBogus ===\n~ return hp\n");
        let diags = check(&[(FileId(0), &hir)], &index);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E061));
    }

    // ── mismatches() ────────────────────────────────────────────────

    #[test]
    fn mismatches_flags_annotation_disagreeing_with_body_inference() {
        // `hp` is annotated `string` but the body only ever compares it
        // against an int literal — body inference derives `int`.
        let (hir, index, res) =
            build_with_resolutions("=== heal(hp: string) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
    }

    #[test]
    fn mismatches_is_silent_when_annotation_and_inference_agree() {
        let (hir, index, res) =
            build_with_resolutions("=== heal(hp: int) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_is_silent_when_body_never_constrains_the_param() {
        // Annotated `int`, body never uses `hp` at all — body infers
        // `Unknown`, which never disagrees (spec: "unresolved -> Unknown,
        // which is LEGAL").
        let (hir, index, res) = build_with_resolutions("=== heal(hp: int) ===\nHello.\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_is_silent_for_the_legal_int_to_float_coercion() {
        // Annotated `float`, body only ever compares against an int literal
        // — `unify(Float, Int) == Float`, the one legal directional
        // coercion (spec §4) — not a disagreement.
        let (hir, index, res) =
            build_with_resolutions("=== heal(hp: float) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_is_silent_when_body_is_conflicted() {
        // #627 ruling: `Conflicted` reads exactly like `Unknown` to this
        // gradual/advisory consumer — reporting a *conflicted* slot
        // specifically is strict mode's TM-3 (#619) job, not E063's. `hp`
        // is compared against both an int and a string literal (a genuine
        // conflict), annotated `int`; this must stay silent, unchanged from
        // the pre-#627 behavior where the same body inferred `Unknown`.
        let (hir, index, res) = build_with_resolutions(
            "=== heal(hp: int) ===\n{hp > 1:\n  ok\n}\n{hp == \"x\":\n  no\n}\n-> DONE\n",
        );
        let inference = crate::infer_project(&[(FileId(0), &hir)], &index, &res);
        // Confirm the fixture actually exercises `Conflicted`, not some
        // other path, before asserting on `mismatches`' silence.
        let heal_id = index
            .by_name
            .get("heal")
            .and_then(|ids| ids.first())
            .copied()
            .expect("heal");
        let sig = inference
            .signatures
            .get(&heal_id)
            .expect("inferred signature for heal");
        assert_eq!(sig.params, vec![Ty::Conflicted], "fixture sanity check");

        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
