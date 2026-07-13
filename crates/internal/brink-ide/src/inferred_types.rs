//! Shared plumbing for TM-5 (#621) inferred-type IDE surfacing.
//!
//! Hover and inlay hints both need to answer "which knot/stitch definition
//! encloses this param/temp?" so they can key
//! [`brink_db::ProjectDb::infer_body`] / `inferred_signature` — the
//! FG-narrowed per-def seam (docs/typed-mode-spec.md §9 TM-5) — instead of
//! the whole-project `type_inference()` (never call that per keystroke).

use brink_analyzer::AnalysisResult;
use brink_format::DefinitionId;
use brink_ir::{SymbolInfo, SymbolKind};

/// The enclosing knot/stitch `DefinitionId` for a param/temp [`SymbolInfo`],
/// derived from its `Scope` (knot/stitch names) — the same qualified name
/// `infer_body`/`inferred_signature` key their result maps by. `None` for a
/// global (no scope) or an orphaned scope (shouldn't happen for a resolved
/// symbol, but fails safe rather than panicking).
///
/// Deterministic even if a duplicate-declaration diagnostic left two ids
/// under the same qualified name: picks the lowest `DefinitionId`, never
/// `HashMap` iteration order (CLAUDE.md's determinism rule).
pub(crate) fn enclosing_callable(
    analysis: &AnalysisResult,
    info: &SymbolInfo,
) -> Option<DefinitionId> {
    let scope = info.scope.as_ref()?;
    let knot = scope.knot.as_deref()?;
    let qualified = match &scope.stitch {
        Some(stitch) => format!("{knot}.{stitch}"),
        None => knot.to_owned(),
    };
    analysis
        .index
        .by_name
        .get(&qualified)?
        .iter()
        .copied()
        .filter(|id| {
            analysis.index.symbols.get(id).is_some_and(|sym| {
                sym.file == info.file && matches!(sym.kind, SymbolKind::Knot | SymbolKind::Stitch)
            })
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::enclosing_callable;
    use crate::session::IdeSession;
    use brink_ir::SymbolKind;

    #[test]
    fn finds_the_enclosing_knot_for_a_param() {
        let src = "=== function heal(hp) ===\n~ return hp\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let param = analysis
            .index
            .symbols
            .values()
            .find(|s| s.kind == SymbolKind::Param && s.name == "hp")
            .expect("param indexed");
        let heal = analysis
            .index
            .by_name
            .get("heal")
            .and_then(|ids| ids.first())
            .copied()
            .expect("heal indexed");

        assert_eq!(enclosing_callable(analysis, param), Some(heal));
    }

    #[test]
    fn finds_the_enclosing_stitch_for_a_temp() {
        let src = "=== hub ===\n= market\n~ temp x = 1\n{x}\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let temp = analysis
            .index
            .symbols
            .values()
            .find(|s| s.kind == SymbolKind::Temp && s.name == "x")
            .expect("temp indexed");
        let market = analysis
            .index
            .by_name
            .get("hub.market")
            .and_then(|ids| ids.first())
            .copied()
            .expect("hub.market indexed");

        assert_eq!(enclosing_callable(analysis, temp), Some(market));
    }

    #[test]
    fn returns_none_for_a_global_symbol() {
        let src = "VAR gold = 100\n-> END\n";
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");

        let gold = analysis
            .index
            .symbols
            .values()
            .find(|s| s.name == "gold")
            .expect("gold indexed");
        assert_eq!(enclosing_callable(analysis, gold), None);
    }
}
