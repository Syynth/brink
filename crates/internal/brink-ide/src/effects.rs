//! T2-4 (docs/effects-spec.md §10, issue #863): the author-facing *display*
//! of a definition's inferred effect row — the boring, stable rendering shared
//! by IDE hover and the `brink ide effects-diff` CLI subcommand (the ruled
//! drift-*visibility* tooling; there is no drift *policy* — spec §10).
//!
//! An [`EffectRow`] (`brink-analyzer::infer::effects`) is unordered sets over
//! the finite cells + call-kinds lattice — `{reads, writes, calls}` plus the
//! pessimal `opaque` top element (spec §2/§3). [`EffectRowView`] projects one
//! into name-resolved, alphabetically-sorted string lists so the display is
//! **deterministic** (house rule: never render a `HashMap`/id-ordered set where
//! order is observable — here the underlying sets are already ordered by
//! `DefinitionId`, but a *name*-sorted view is the stable author-facing order,
//! independent of id-allocation order across revisions, which is exactly what
//! `effects-diff` needs to compare two builds without spurious churn).
//!
//! The display is intentionally plain — no severity, no policy, no drift
//! verdict — because per the sitting-2 ruling the only *contract* is the
//! optional `#@effects` assertion (checked elsewhere: `brink-analyzer::
//! effects_assertions`, `E103`). Hover and `effects-diff` only ever *show* the
//! row.

use brink_analyzer::EffectRow;
use brink_format::DefinitionId;
use brink_ir::SymbolIndex;

/// A name-resolved, deterministically-ordered projection of an [`EffectRow`]
/// for display (IDE hover) and diffing (`brink ide effects-diff`).
///
/// `reads`/`writes` are the cell (global `VAR`/`CONST`) names the row touches;
/// `calls` are the `EXTERNAL` call-kind names. Each list is sorted
/// alphabetically — a stable author-facing order that does not depend on
/// `DefinitionId` allocation order (which can shift between revisions), so a
/// diff of two builds' views reflects real effect changes, not id churn.
///
/// `opaque` is the pessimal top element (spec §3): a call through a function
/// value or an unresolved callee, for which the row conservatively "touches
/// everything". When `opaque` is set the member lists are still populated with
/// whatever concrete atoms were also seen, but the display leads with the
/// opaque note since it dominates.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the analyzer EffectRow's independent dimension flags"
)]
pub struct EffectRowView {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub calls: Vec<String>,
    pub opaque: bool,
    /// NS-A2 (issue #1108): the definition may produce content (see
    /// `EffectRow::emits`).
    pub emits: bool,
    /// NS-A2: the definition may touch the tag channel.
    pub tags: bool,
    /// NS-A2: the definition may raise a turn-terminating fault.
    pub faults: bool,
}

impl EffectRowView {
    /// Project `row` into a display view, resolving cell `DefinitionId`s to
    /// their declared names through `index` and sorting every list
    /// alphabetically. An id with no entry in `index` (should not happen for a
    /// real global, but the row type does not guarantee it) falls back to its
    /// debug form — the same defensive fallback `effects_assertions`'
    /// exceedance message uses — so the display never silently drops an atom.
    #[must_use]
    pub fn from_row(row: &EffectRow, index: &SymbolIndex) -> Self {
        let name_of = |id: &DefinitionId| {
            index
                .symbols
                .get(id)
                .map_or_else(|| format!("{id:?}"), |info| info.name.clone())
        };
        let mut reads: Vec<String> = row.reads.iter().map(name_of).collect();
        let mut writes: Vec<String> = row.writes.iter().map(name_of).collect();
        let mut calls: Vec<String> = row.calls.iter().cloned().collect();
        reads.sort();
        writes.sort();
        calls.sort();
        Self {
            reads,
            writes,
            calls,
            // `is_pessimal`, not the intrinsic `opaque` bit: a row that still
            // carries a §6.1 row variable (issue #1680) is unbounded until a
            // caller instantiates it, and this view has no hole channel to
            // show one in.
            opaque: row.is_pessimal(),
            emits: row.emits,
            tags: row.tags,
            faults: row.faults,
        }
    }

    /// Whether the row lists nothing and is not opaque — a genuinely pure
    /// definition (the `#@effects(pure)` tooling-trust case, spec §10).
    #[must_use]
    pub fn is_pure(&self) -> bool {
        !self.opaque && self.reads.is_empty() && self.writes.is_empty() && self.calls.is_empty()
    }

    /// NS-A2: the empty row across every dimension — pure AND silent AND
    /// untagged AND total.
    #[must_use]
    pub fn is_empty_row(&self) -> bool {
        self.is_pure() && !self.emits && !self.tags && !self.faults
    }

    /// The boring, stable one-line rendering (spec §10's "boring stable
    /// display: reads/writes/calls sets"):
    ///
    /// - a pure def → `pure`;
    /// - an opaque def → `opaque (calls through a function value; touches
    ///   every cell)`, with any concrete atoms appended after a `—`;
    /// - otherwise → the non-empty clauses joined by `; `, e.g.
    ///   `reads: gold, health; writes: alarm; calls: PlaySound`.
    ///
    /// NS-A2 (issue #1108): the emits/tags/faults dimensions join the line
    /// as bare markers after the state clauses; a fully-empty row renders
    /// `pure, silent, total` (the strongest statement the row makes).
    #[must_use]
    pub fn display_line(&self) -> String {
        if self.is_empty_row() {
            return "pure, silent, total".to_string();
        }
        let mut clauses = Vec::new();
        if !self.reads.is_empty() {
            clauses.push(format!("reads: {}", self.reads.join(", ")));
        }
        if !self.writes.is_empty() {
            clauses.push(format!("writes: {}", self.writes.join(", ")));
        }
        if !self.calls.is_empty() {
            clauses.push(format!("calls: {}", self.calls.join(", ")));
        }
        if self.emits {
            clauses.push("emits".to_string());
        }
        if self.tags {
            clauses.push("tags".to_string());
        }
        if self.faults {
            clauses.push("faults".to_string());
        }
        if self.opaque {
            let mut s = "opaque (calls through a function value; touches every cell)".to_string();
            if !clauses.is_empty() {
                s.push_str(" — ");
                s.push_str(&clauses.join("; "));
            }
            return s;
        }
        if clauses.is_empty() {
            // Pure state row that still emits/tags/faults was handled above;
            // reaching here means state-pure with no dimension set — covered
            // by is_empty_row, but keep the defensive arm total.
            return "pure, silent, total".to_string();
        }
        clauses.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::DefinitionTag;
    use brink_ir::symbols::SymbolInfo;

    fn cell(index: &mut SymbolIndex, n: u64, name: &str) -> DefinitionId {
        let id = DefinitionId::new(DefinitionTag::GlobalVar, n);
        index.symbols.insert(
            id,
            SymbolInfo {
                id,
                name: name.to_string(),
                kind: brink_ir::SymbolKind::Variable,
                file: brink_ir::FileId(0),
                range: rowan::TextRange::empty(rowan::TextSize::from(0)),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: None,
                visibility: brink_ir::symbols::Visibility::Public,
            },
        );
        id
    }

    #[test]
    fn pure_row_reads_as_pure() {
        let index = SymbolIndex::default();
        let view = EffectRowView::from_row(&EffectRow::default(), &index);
        assert!(view.is_pure());
        assert!(view.is_empty_row());
        // NS-A2: the empty row is the strongest statement across every
        // dimension — state purity, silence, totality.
        assert_eq!(view.display_line(), "pure, silent, total");
    }

    #[test]
    fn dimension_flags_render_as_bare_markers() {
        let index = SymbolIndex::default();
        let row = EffectRow {
            emits: true,
            tags: true,
            faults: true,
            ..Default::default()
        };
        let view = EffectRowView::from_row(&row, &index);
        assert!(view.is_pure(), "state-pure despite output/fault dimensions");
        assert!(!view.is_empty_row());
        assert_eq!(view.display_line(), "emits; tags; faults");
    }

    #[test]
    fn names_are_sorted_alphabetically_not_by_id() {
        let mut index = SymbolIndex::default();
        // Insert with ids whose numeric order is the reverse of name order, to
        // prove the view sorts by name, not id.
        let health = cell(&mut index, 1, "health");
        let gold = cell(&mut index, 2, "gold");
        let row = EffectRow {
            reads: [health, gold].into_iter().collect(),
            calls: ["PlaySound".to_string(), "Alarm".to_string()]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let view = EffectRowView::from_row(&row, &index);
        assert_eq!(view.reads, ["gold", "health"]);
        assert_eq!(view.calls, ["Alarm", "PlaySound"]);
        assert_eq!(
            view.display_line(),
            "reads: gold, health; calls: Alarm, PlaySound"
        );
    }

    #[test]
    fn writes_and_reads_render_in_fixed_clause_order() {
        let mut index = SymbolIndex::default();
        let gold = cell(&mut index, 1, "gold");
        let alarm = cell(&mut index, 2, "alarm");
        let row = EffectRow {
            reads: [gold].into_iter().collect(),
            writes: [alarm].into_iter().collect(),
            ..Default::default()
        };
        let view = EffectRowView::from_row(&row, &index);
        assert_eq!(view.display_line(), "reads: gold; writes: alarm");
    }

    #[test]
    fn opaque_row_leads_with_the_opaque_note() {
        let mut index = SymbolIndex::default();
        let gold = cell(&mut index, 1, "gold");
        let row = EffectRow {
            reads: [gold].into_iter().collect(),
            opaque: true,
            ..Default::default()
        };
        let view = EffectRowView::from_row(&row, &index);
        assert!(!view.is_pure());
        let line = view.display_line();
        assert!(line.starts_with("opaque"), "{line}");
        assert!(line.contains("reads: gold"), "{line}");
    }
}
