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
use brink_ir::hir::HirFile;
use brink_ir::{FileId, ParamInfo, SymbolIndex, SymbolKind};

use crate::annotations::{declared_list_names, resolve as resolve_annotation};
use crate::external_check::{InferredType, infer_literal_type};
use crate::infer::Ty;

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
        Ty::Array(_) | Ty::Map(_, _) | Ty::Unknown => None,
    }
}

/// TM-2 firewall rule, shared by `VAR` and `CONST`: if `annotation` resolves
/// to a representable [`InferredType`], it replaces `literal_type`; otherwise
/// the literal-inferred type stands.
fn value_type_with_annotation_override(
    literal_type: Option<InferredType>,
    annotation: Option<&brink_ir::TypeExpr>,
    list_names: &std::collections::BTreeSet<String>,
) -> Option<InferredType> {
    annotation
        .and_then(|ann| resolve_annotation(ann, list_names))
        .and_then(|ty| ty_to_inferred_type(&ty))
        .or(literal_type)
}

/// Compute the signature stub for one definition.
///
/// Reads the declaration only: the indexed `SymbolInfo` plus the declaring
/// file's HIR for the initializer expression and the `#@local` bit. Returns
/// `None` for an unknown definition id.
#[must_use]
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
    let mut is_local = false;
    let mut param_annotations = Vec::new();
    let mut return_annotation = None;
    if let Some(hir) = hir {
        // `list<L>` annotations resolve nominally against every declared
        // `LIST` in the project (spec §2/§3) — computed lazily, only when
        // this def actually has a knot/stitch/var to annotate below.
        let list_names = || declared_list_names(index);
        match info.kind {
            SymbolKind::Variable => {
                if let Some(v) = hir.variables.iter().find(|v| v.name.text == info.name) {
                    is_local = v.is_local;
                    // TM-2: annotation wins over the literal-inferred type.
                    value_type = value_type_with_annotation_override(
                        infer_literal_type(&v.value),
                        v.annotation.as_ref(),
                        &list_names(),
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
                    );
                }
            }
            SymbolKind::Knot => {
                if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
                    let names = list_names();
                    param_annotations = k
                        .params
                        .iter()
                        .map(|p| {
                            p.annotation
                                .as_ref()
                                .and_then(|a| resolve_annotation(a, &names))
                        })
                        .collect();
                    return_annotation = k
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_annotation(rt, &names));
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
                        param_annotations = s
                            .params
                            .iter()
                            .map(|p| {
                                p.annotation
                                    .as_ref()
                                    .and_then(|a| resolve_annotation(a, &names))
                            })
                            .collect();
                    }
                } else if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
                    let names = list_names();
                    param_annotations = k
                        .params
                        .iter()
                        .map(|p| {
                            p.annotation
                                .as_ref()
                                .and_then(|a| resolve_annotation(a, &names))
                        })
                        .collect();
                    return_annotation = k
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_annotation(rt, &names));
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
        is_local,
        param_annotations,
        return_annotation,
    }))
}
