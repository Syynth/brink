//! `signature(def)` — per-declaration signature summary.
//!
//! Phase-0 **stub** of the `signature(DefId)` query (scripting-substrate
//! spec §4, layer 2): it carries only what is already derivable from the
//! declaration itself — name, kind, declared params, the initializer-inferred
//! type (the same inference as [`crate::external_check::infer_value_meta`]),
//! and the `#@local` flow-private bit. No checking, no body analysis. The
//! future type checker reads this as its firewall unit.

use std::sync::Arc;

use brink_format::DefinitionId;
use brink_ir::hir::{Expr, HirFile};
use brink_ir::{FileId, ParamInfo, SymbolIndex, SymbolKind};

use crate::annotations::{
    declared_list_names, declared_struct_names, resolve as resolve_annotation,
};
use crate::external_check::{InferredType, infer_literal_type};
use crate::infer::Ty;
use crate::resolve::lookup_by_name;

/// Per-declaration signature summary (phase-0 stub).
///
/// Everything here is declaration-derived: a body edit that doesn't touch
/// the declaration line(s) must not change a `Sig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sig {
    /// Canonical/qualified name, as indexed (e.g. `knot.stitch`).
    pub name: String,
    /// Declaration kind.
    pub kind: SymbolKind,
    /// Declared parameters (knots, stitches, externals).
    pub params: Vec<ParamInfo>,
    /// Type inferred from the initializer literal (VAR/CONST only), if the
    /// initializer is one. TM-2 (docs/typed-mode-spec.md §3): when the
    /// `VAR`/`CONST` also carries a `: type` annotation and that annotation
    /// resolves to a representable [`InferredType`], the annotation
    /// *replaces* this value — annotation wins over inference (the
    /// firewall rule) — so every existing consumer of `value_type`
    /// (notably `infer::collect_globals`) picks up the annotated type
    /// automatically, with no seam change.
    pub value_type: Option<InferredType>,
    /// A VAR/CONST's declaration-derived function-value type, `fn(T…): R`
    /// (T1c follow-up, issue #712, docs/t1c-spec.md §4) — `None` for every
    /// other symbol kind and for a VAR/CONST that isn't fn-typed. Kept
    /// separate from `value_type` rather than widening it: `InferredType`
    /// has no `Fn` form (by design — it predates T1c and stays the narrow,
    /// wire-adjacent domain `infer_value_meta`/hover share), so a `Ty::Fn`
    /// would otherwise be silently dropped exactly like the `Array`/`Map`/
    /// `Struct` gap `ty_to_inferred_type` already documents. Populated two
    /// ways, annotation winning per the TM-2 firewall (same rule as
    /// `value_type`'s override):
    /// - an explicit `: fn(T…): R` annotation on the VAR/CONST itself, or
    /// - a bare `#fn(target, args…)` initializer, in which case the type is
    ///   the bound prefix consumed from `target`'s *own* declaration-derived
    ///   signature (`param_annotations`/`return_annotation` — a second,
    ///   single-level `signature()` call, never the target's body). An
    ///   unannotated target param/return reads as `Ty::Unknown` in the row,
    ///   same conservative fallback declaration-derived typing always uses.
    pub fn_type: Option<Ty>,
    /// Marked flow-private via a `#@local` directive (knots, stitches, VARs).
    pub is_local: bool,
    /// TM-2 (docs/typed-mode-spec.md §3): declared param type annotations,
    /// positional (parallel to `params`) — `None` per-slot for an
    /// unannotated param, or one whose annotation doesn't resolve to a `Ty`
    /// (`void`, `fn(...)`, an unrecognized name — `crate::check_annotations`
    /// reports those separately). Knots/stitches only; empty for other
    /// symbol kinds.
    pub param_annotations: Vec<Option<Ty>>,
    /// TM-2: the function-header return type annotation (`): type ===`),
    /// resolved. `None` when absent, `void`, or unresolved. Knots only —
    /// `= stitch` headers have no return-type grammar position.
    pub return_annotation: Option<Ty>,
}

/// Downcast a resolved [`Ty`] to [`InferredType`] where the two universes
/// overlap (`Ty`'s scalar leaves plus nominal `list<L>`) — the annotation-
/// wins substitution for `Sig::value_type`, which predates TM-2 and only
/// has room for `InferredType`'s narrower domain. `array<T>`/`map<K, V>`
/// annotations have no `InferredType` representation; those `VAR`s keep
/// their literal-inferred `value_type` (a strict-mode gap TM-3 owns, not a
/// silent drop — the full `Ty` is still available via `param_annotations`/
/// `return_annotation` for knots/stitches, and `crate::resolve_annotation`
/// directly for any `TypeExpr`).
fn ty_to_inferred_type(ty: &Ty) -> Option<InferredType> {
    match ty {
        Ty::Int => Some(InferredType::Int),
        Ty::Float => Some(InferredType::Float),
        Ty::Bool => Some(InferredType::Bool),
        Ty::String => Some(InferredType::String),
        Ty::Divert => Some(InferredType::Divert),
        Ty::List(_) => Some(InferredType::List),
        // `Conflicted` (#627): a genuine type conflict has no representable
        // `InferredType` any more than `Unknown` does — this stub is a
        // gradual/advisory consumer, so it reads both the same way.
        // `Struct` (TM-4b): no `InferredType` representation exists for a
        // nominal struct shape either — same gap as `Array`/`Map`, not a
        // silent drop (the full `Ty::Struct` is still available via
        // `param_annotations`/`return_annotation`/`resolve_annotation`).
        // `Fn` (T1c): a function-value type has no `InferredType`
        // representation either — same gap as `Array`/`Map`/`Struct`, not a
        // silent drop.
        Ty::Array(_)
        | Ty::Map(_, _)
        | Ty::Struct(_)
        | Ty::Fn(..)
        | Ty::Unknown
        | Ty::Conflicted => None,
    }
}

/// TM-2 firewall rule, shared by `VAR` and `CONST`: if `annotation` resolves
/// to a representable [`InferredType`], it replaces `literal_type`; otherwise
/// the literal-inferred type stands.
fn value_type_with_annotation_override(
    literal_type: Option<InferredType>,
    annotation: Option<&brink_ir::TypeExpr>,
    list_names: &std::collections::BTreeSet<String>,
    struct_names: &std::collections::BTreeSet<String>,
) -> Option<InferredType> {
    annotation
        .and_then(|ann| resolve_annotation(ann, list_names, struct_names))
        .and_then(|ty| ty_to_inferred_type(&ty))
        .or(literal_type)
}

/// A VAR/CONST's declaration-derived `fn(T…): R` type ([`Sig::fn_type`],
/// T1c follow-up, issue #712) — `None` when neither an `fn(...)`
/// annotation nor a `#fn(...)` initializer applies.
fn declared_fn_type(
    value: &Expr,
    annotation: Option<&brink_ir::TypeExpr>,
    index: &SymbolIndex,
    files: &[(FileId, &HirFile)],
    list_names: &std::collections::BTreeSet<String>,
    struct_names: &std::collections::BTreeSet<String>,
) -> Option<Ty> {
    // Annotation wins over inference (TM-2 firewall) — same rule as
    // `value_type_with_annotation_override`, but reading the full `Ty`
    // straight from `resolve_annotation` instead of downcasting through
    // `InferredType` (which has no `Fn` form — see `Sig::fn_type`'s doc).
    if let Some(ann) = annotation
        && let Some(ty @ Ty::Fn(..)) = resolve_annotation(ann, list_names, struct_names)
    {
        return Some(ty);
    }

    let Expr::FnLiteral(fl) = value else {
        return None;
    };
    // `#fn` targets are always a single, statically-named function knot —
    // never a stitch (T1c-1's own note: "every stitch target rejects until
    // stitch-functions exist"), never a local (a global initializer has no
    // enclosing body to scope a temp/param against). This is exactly
    // `resolve::resolve_function`'s own first-tried bucket for a bare
    // name, so it agrees with the real resolver for every target that
    // isn't already an E079 error; an E079 target (wrong kind, or no
    // "function" detail) falls through to `None` — the pre-existing
    // Unknown-escape fallback, not a new failure mode.
    let target_name = fl
        .target
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    let target_def = lookup_by_name(index, &target_name, &[SymbolKind::Knot])?;
    let target_info = index.symbols.get(&target_def)?;
    if target_info.detail.as_deref() != Some("function") {
        return None;
    }
    // `signature()` is only ever handed a *narrowed* `files` slice by
    // `brink-db`'s per-def `signature_query` (FG-2, single declaring file
    // only — never a whole-project scan, by design). A cross-file `#fn`
    // target's HIR simply isn't in that slice; calling `signature()`
    // anyway would silently return a Sig with empty `param_annotations`
    // (indistinguishable from "declares zero params"), fabricating a wrong
    // arity instead of the honest "can't determine" `None`. The
    // whole-project caller (`infer::collect_globals`, which always passes
    // every file) never hits this guard.
    if !files.iter().any(|&(id, _)| id == target_info.file) {
        return None;
    }
    // Single-level recursion into the *target's* declaration-derived
    // signature only (never its body) — `#fn` targets are always knots, so
    // this never re-enters the `Variable`/`Constant` arms that call
    // `declared_fn_type` in the first place.
    let target_sig = signature(target_def, index, files)?;
    let remaining: Vec<Ty> = target_sig
        .param_annotations
        .iter()
        .skip(fl.args.len())
        .map(|a| a.clone().unwrap_or(Ty::Unknown))
        .collect();
    let ret = target_sig.return_annotation.clone().unwrap_or(Ty::Unknown);
    Some(Ty::Fn(remaining, Box::new(ret)))
}

/// Compute the signature stub for one definition.
///
/// Reads the declaration only: the indexed `SymbolInfo` plus the declaring
/// file's HIR for the initializer expression and the `#@local` bit. Returns
/// `None` for an unknown definition id.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "TM-4b (docs/typed-mode-spec.md §6) threads a second declared-name \
              set (struct_names, alongside the pre-existing list_names) through \
              every existing per-kind annotation-resolution branch below — the \
              same shape the function already had, just wider, not a new \
              structural concern worth splitting out"
)]
pub fn signature(
    def: DefinitionId,
    index: &SymbolIndex,
    files: &[(FileId, &HirFile)],
) -> Option<Arc<Sig>> {
    let info = index.symbols.get(&def)?;
    let hir = files
        .iter()
        .find(|&&(id, _)| id == info.file)
        .map(|&(_, hir)| hir);

    let mut value_type = None;
    let mut fn_type = None;
    let mut is_local = false;
    let mut param_annotations = Vec::new();
    let mut return_annotation = None;
    if let Some(hir) = hir {
        // `list<L>`/declared-struct annotations resolve nominally against
        // every declared `LIST`/`STRUCT` in the project (spec §2/§3, TM-4b
        // §6) — computed lazily, only when this def actually has a
        // knot/stitch/var to annotate below.
        let list_names = || declared_list_names(index);
        let struct_names = || declared_struct_names(index);
        match info.kind {
            SymbolKind::Variable => {
                if let Some(v) = hir.variables.iter().find(|v| v.name.text == info.name) {
                    is_local = v.is_local;
                    // TM-2: annotation wins over the literal-inferred type.
                    value_type = value_type_with_annotation_override(
                        infer_literal_type(&v.value),
                        v.annotation.as_ref(),
                        &list_names(),
                        &struct_names(),
                    );
                    // T1c follow-up (issue #712): `Ty::Fn` has no
                    // `InferredType` home, so it's carried on its own field
                    // — see `Sig::fn_type`.
                    fn_type = declared_fn_type(
                        &v.value,
                        v.annotation.as_ref(),
                        index,
                        files,
                        &list_names(),
                        &struct_names(),
                    );
                }
            }
            SymbolKind::Constant => {
                if let Some(c) = hir.constants.iter().find(|c| c.name.text == info.name) {
                    // TM-2: annotation wins over the literal-inferred type
                    // (same firewall rule as VAR — see the `Variable` arm).
                    value_type = value_type_with_annotation_override(
                        infer_literal_type(&c.value),
                        c.annotation.as_ref(),
                        &list_names(),
                        &struct_names(),
                    );
                    // T1c follow-up (issue #712) — see the `Variable` arm.
                    fn_type = declared_fn_type(
                        &c.value,
                        c.annotation.as_ref(),
                        index,
                        files,
                        &list_names(),
                        &struct_names(),
                    );
                }
            }
            SymbolKind::Knot => {
                if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
                    let names = list_names();
                    let s_names = struct_names();
                    param_annotations = k
                        .params
                        .iter()
                        .map(|p| {
                            p.annotation
                                .as_ref()
                                .and_then(|a| resolve_annotation(a, &names, &s_names))
                        })
                        .collect();
                    return_annotation = k
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_annotation(rt, &names, &s_names));
                }
            }
            SymbolKind::Stitch => {
                // Indexed stitch names are qualified (`knot.stitch`); HIR
                // nests stitches under their knot by bare name. A top-level
                // stitch is promoted to knot status during HIR lowering.
                if let Some((knot_name, stitch_name)) = info.name.split_once('.') {
                    if let Some(s) = hir
                        .knots
                        .iter()
                        .find(|k| k.name.text == knot_name)
                        .and_then(|k| k.stitches.iter().find(|s| s.name.text == stitch_name))
                    {
                        is_local = s.is_local;
                        let names = list_names();
                        let s_names = struct_names();
                        param_annotations = s
                            .params
                            .iter()
                            .map(|p| {
                                p.annotation
                                    .as_ref()
                                    .and_then(|a| resolve_annotation(a, &names, &s_names))
                            })
                            .collect();
                    }
                } else if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
                    let names = list_names();
                    let s_names = struct_names();
                    param_annotations = k
                        .params
                        .iter()
                        .map(|p| {
                            p.annotation
                                .as_ref()
                                .and_then(|a| resolve_annotation(a, &names, &s_names))
                        })
                        .collect();
                    return_annotation = k
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_annotation(rt, &names, &s_names));
                }
            }
            _ => {}
        }
    }

    Some(Arc::new(Sig {
        name: info.name.clone(),
        kind: info.kind,
        params: info.params.clone(),
        value_type,
        fn_type,
        is_local,
        param_annotations,
        return_annotation,
    }))
}
