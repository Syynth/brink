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

use crate::external_check::{InferredType, infer_literal_type};

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
    /// initializer is one.
    pub value_type: Option<InferredType>,
    /// Marked flow-private via a `#@local` directive (knots, stitches, VARs).
    pub is_local: bool,
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
    if let Some(hir) = hir {
        match info.kind {
            SymbolKind::Variable => {
                if let Some(v) = hir.variables.iter().find(|v| v.name.text == info.name) {
                    value_type = infer_literal_type(&v.value);
                    is_local = v.is_local;
                }
            }
            SymbolKind::Constant => {
                if let Some(c) = hir.constants.iter().find(|c| c.name.text == info.name) {
                    value_type = infer_literal_type(&c.value);
                }
            }
            SymbolKind::Knot => {
                if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
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
                    }
                } else if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
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
    }))
}
