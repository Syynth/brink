use std::collections::HashMap;

use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, RefKind, SymbolIndex, SymbolKind, SymbolManifest,
};
use rowan::TextRange;

/// Maps a reference's source range to the definition it resolved to.
pub type ResolutionMap = HashMap<TextRange, DefinitionId>;

/// Resolve all unresolved references across files.
pub fn resolve_refs(
    index: &SymbolIndex,
    files: &[(FileId, SymbolManifest)],
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();

    for (_file_id, manifest) in files {
        for uref in &manifest.unresolved {
            match uref.kind {
                RefKind::Divert => resolve_divert(index, uref, &mut map, &mut diagnostics),
                RefKind::Variable => resolve_variable(index, uref, &mut map, &mut diagnostics),
                RefKind::Function => resolve_function(index, uref, &mut map, &mut diagnostics),
                RefKind::List => resolve_list_ref(index, uref, &mut map, &mut diagnostics),
            }
        }
    }

    (map, diagnostics)
}

fn resolve_divert(
    index: &SymbolIndex,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Dotted path — try exact qualified lookup
    if path.contains('.') {
        let found = lookup_by_name(index, path, &[SymbolKind::Stitch, SymbolKind::Label]);
        if let Some(id) = found {
            map.insert(uref.range, id);
        } else {
            diagnostics.push(unresolved_diag(uref.range, path, DiagnosticCode::E024));
        }
        return;
    }

    // Single segment — ink's hierarchical resolution:
    // 1. Stitch in current knot
    if let Some(knot) = &uref.scope.knot {
        let qualified = format!("{knot}.{path}");
        if let Some(id) =
            lookup_by_name(index, &qualified, &[SymbolKind::Stitch, SymbolKind::Label])
        {
            map.insert(uref.range, id);
            return;
        }

        // Also check knot.stitch.label for labels in current stitch
        if let Some(stitch) = &uref.scope.stitch
            && let Some(id) = lookup_by_name(
                index,
                &format!("{knot}.{stitch}.{path}"),
                &[SymbolKind::Label],
            )
        {
            map.insert(uref.range, id);
            return;
        }
    }

    // 2. Knot at top level
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot]) {
        map.insert(uref.range, id);
        return;
    }

    // 3. Label anywhere in current knot (search by suffix)
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, knot, path)
    {
        map.insert(uref.range, id);
        return;
    }

    diagnostics.push(unresolved_diag(uref.range, path, DiagnosticCode::E024));
}

fn resolve_variable(
    index: &SymbolIndex,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try global variables / constants
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Variable, SymbolKind::Constant]) {
        map.insert(uref.range, id);
        return;
    }

    // Try list items by bare name (e.g. `myItem` matches `SomeList.myItem`)
    match lookup_list_item_bare(index, path) {
        BareItemResult::Unique(id) => {
            map.insert(uref.range, id);
            return;
        }
        BareItemResult::Ambiguous => {
            diagnostics.push(ambiguous_diag(uref.range, path));
            return;
        }
        BareItemResult::NotFound => {}
    }

    // Try qualified list item (e.g. `ListName.ItemName`)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, path, &[SymbolKind::ListItem])
    {
        map.insert(uref.range, id);
        return;
    }

    // Try list names
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::List]) {
        map.insert(uref.range, id);
        return;
    }

    // Try knots (ink allows knot references as variables, e.g. visit counts)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot]) {
        map.insert(uref.range, id);
        return;
    }

    // Try stitches in current knot scope
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_by_name(index, &format!("{knot}.{path}"), &[SymbolKind::Stitch])
    {
        map.insert(uref.range, id);
        return;
    }

    // Try labels
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, knot, path)
    {
        map.insert(uref.range, id);
        return;
    }

    diagnostics.push(unresolved_diag(uref.range, path, DiagnosticCode::E025));
}

fn resolve_function(
    index: &SymbolIndex,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try externals first
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::External]) {
        map.insert(uref.range, id);
        return;
    }

    // Try knots (ink allows knots as functions via tunnels)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot]) {
        map.insert(uref.range, id);
        return;
    }

    diagnostics.push(unresolved_diag(uref.range, path, DiagnosticCode::E025));
}

fn resolve_list_ref(
    index: &SymbolIndex,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try qualified list item (ListName.ItemName)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, path, &[SymbolKind::ListItem])
    {
        map.insert(uref.range, id);
        return;
    }

    // Try bare list item name
    match lookup_list_item_bare(index, path) {
        BareItemResult::Unique(id) => {
            map.insert(uref.range, id);
            return;
        }
        BareItemResult::Ambiguous => {
            diagnostics.push(ambiguous_diag(uref.range, path));
            return;
        }
        BareItemResult::NotFound => {}
    }

    // Try list name
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::List]) {
        map.insert(uref.range, id);
        return;
    }

    diagnostics.push(unresolved_diag(uref.range, path, DiagnosticCode::E025));
}

// ─── Lookup helpers ─────────────────────────────────────────────────

fn lookup_by_name(index: &SymbolIndex, name: &str, kinds: &[SymbolKind]) -> Option<DefinitionId> {
    let ids = index.by_name.get(name)?;
    for id in ids {
        if let Some(info) = index.symbols.get(id)
            && kinds.contains(&info.kind)
        {
            return Some(*id);
        }
    }
    None
}

/// Result of a bare list item lookup.
enum BareItemResult {
    /// Exactly one match.
    Unique(DefinitionId),
    /// Multiple matches across different lists — caller must qualify.
    Ambiguous,
    /// No match found.
    NotFound,
}

/// Look up a list item by its bare (unqualified) name.
/// Searches all `ListName.ItemName` entries for a suffix match.
/// Returns `Ambiguous` if multiple lists contain an item with this name.
fn lookup_list_item_bare(index: &SymbolIndex, bare_name: &str) -> BareItemResult {
    let suffix = format!(".{bare_name}");
    let mut found: Option<DefinitionId> = None;
    for (name, ids) in &index.by_name {
        if name.ends_with(&suffix) {
            for id in ids {
                if let Some(info) = index.symbols.get(id)
                    && info.kind == SymbolKind::ListItem
                {
                    if found.is_some() {
                        return BareItemResult::Ambiguous;
                    }
                    found = Some(*id);
                }
            }
        }
    }
    match found {
        Some(id) => BareItemResult::Unique(id),
        None => BareItemResult::NotFound,
    }
}

/// Look up a label within a knot scope. Searches for `knot.label` and
/// `knot.*.label` patterns.
fn lookup_label_in_knot(index: &SymbolIndex, knot: &str, label: &str) -> Option<DefinitionId> {
    // Try knot.label
    let direct = format!("{knot}.{label}");
    if let Some(id) = lookup_by_name(index, &direct, &[SymbolKind::Label]) {
        return Some(id);
    }

    // Try knot.*.label (any stitch within this knot)
    let suffix = format!(".{label}");
    let prefix = format!("{knot}.");
    for (name, ids) in &index.by_name {
        if name.starts_with(&prefix) && name.ends_with(&suffix) && name.matches('.').count() == 2 {
            for id in ids {
                if let Some(info) = index.symbols.get(id)
                    && info.kind == SymbolKind::Label
                {
                    return Some(*id);
                }
            }
        }
    }
    None
}

fn ambiguous_diag(range: TextRange, path: &str) -> Diagnostic {
    Diagnostic {
        range,
        message: format!(
            "{}: `{path}` — qualify with the list name (e.g., `ListName.{path}`)",
            DiagnosticCode::E027.title(),
        ),
        code: DiagnosticCode::E027,
    }
}

fn unresolved_diag(range: TextRange, path: &str, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        range,
        message: format!("{}: `{path}`", code.title()),
        code,
    }
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test helper ranges")]
mod tests {
    use brink_ir::{DeclaredSymbol, Scope, UnresolvedRef};
    use rowan::TextRange;
    use rowan::TextSize;

    use super::*;
    use crate::manifest::merge_manifests;

    fn range(offset: u32, len: u32) -> TextRange {
        TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
    }

    fn make_manifest(
        knots: &[&str],
        stitches: &[&str],
        variables: &[&str],
        lists: &[(&str, &[&str])],
        externals: &[&str],
        labels: &[&str],
        unresolved: Vec<UnresolvedRef>,
    ) -> SymbolManifest {
        let mut manifest = SymbolManifest::default();
        let mut offset = 0u32;

        for &name in knots {
            let r = range(offset, name.len() as u32);
            manifest.knots.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in stitches {
            let r = range(offset, name.len() as u32);
            manifest.stitches.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in variables {
            let r = range(offset, name.len() as u32);
            manifest.variables.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
            });
            offset += name.len() as u32 + 1;
        }
        for &(list_name, items) in lists {
            let r = range(offset, list_name.len() as u32);
            manifest.lists.push(DeclaredSymbol {
                name: list_name.to_string(),
                range: r,
            });
            offset += list_name.len() as u32 + 1;
            for &item in items {
                let qualified = format!("{list_name}.{item}");
                let r = range(offset, item.len() as u32);
                manifest.list_items.push(DeclaredSymbol {
                    name: qualified,
                    range: r,
                });
                offset += item.len() as u32 + 1;
            }
        }
        for &name in externals {
            let r = range(offset, name.len() as u32);
            manifest.externals.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in labels {
            let r = range(offset, name.len() as u32);
            manifest.labels.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
            });
            offset += name.len() as u32 + 1;
        }
        manifest.unresolved = unresolved;
        manifest
    }

    fn uref(path: &str, kind: RefKind, knot: Option<&str>, stitch: Option<&str>) -> UnresolvedRef {
        UnresolvedRef {
            path: path.to_string(),
            range: range(900, path.len() as u32),
            kind,
            scope: Scope {
                knot: knot.map(String::from),
                stitch: stitch.map(String::from),
            },
        }
    }

    #[test]
    fn single_knot_divert_resolves() {
        let manifest = make_manifest(
            &["start"],
            &[],
            &[],
            &[],
            &[],
            &[],
            vec![uref("start", RefKind::Divert, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, merge_diags) = merge_manifests(&files);
        let (resolutions, resolve_diags) = resolve_refs(&index, &files);

        assert!(merge_diags.is_empty());
        assert!(resolve_diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn qualified_knot_stitch_divert_resolves() {
        let manifest = make_manifest(
            &["kitchen"],
            &["kitchen.look_around"],
            &[],
            &[],
            &[],
            &[],
            vec![uref("kitchen.look_around", RefKind::Divert, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn stitch_local_divert_prefers_local_stitch() {
        let manifest = make_manifest(
            &["bedroom", "kitchen"],
            &["bedroom.look", "kitchen.look"],
            &[],
            &[],
            &[],
            &[],
            vec![uref("look", RefKind::Divert, Some("bedroom"), None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        // The resolved ID should be for bedroom.look
        let resolved_id = resolutions.values().next().unwrap();
        let info = index.symbols.get(resolved_id).unwrap();
        assert_eq!(info.name, "bedroom.look");
    }

    #[test]
    fn unresolved_divert_emits_diagnostic() {
        let manifest = make_manifest(
            &["start"],
            &[],
            &[],
            &[],
            &[],
            &[],
            vec![uref("nonexistent", RefKind::Divert, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(resolutions.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E024);
    }

    #[test]
    fn duplicate_knot_emits_diagnostic() {
        let mut m1 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);
        let m2 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);

        // Give m2 different ranges so they don't collide
        m1.knots[0].range = range(0, 5);

        let files = vec![(FileId(0), m1), (FileId(1), m2)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    #[test]
    fn list_item_bare_name_resolves() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[("Colors", &["red", "green", "blue"])],
            &[],
            &[],
            vec![uref("red", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let resolved_id = resolutions.values().next().unwrap();
        let info = index.symbols.get(resolved_id).unwrap();
        assert_eq!(info.name, "Colors.red");
    }

    #[test]
    fn end_done_not_in_unresolved() {
        // END/DONE are handled as DivertPath::End/Done at the HIR level,
        // so they never appear as UnresolvedRef entries. This test verifies
        // that the resolution pass doesn't get confused by them.
        let manifest = make_manifest(
            &["start"],
            &[],
            &[],
            &[],
            &[],
            &[],
            vec![], // No unresolved refs for END/DONE
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert!(resolutions.is_empty());
    }

    #[test]
    fn label_in_knot_resolves() {
        let manifest = make_manifest(
            &["meeting"],
            &[],
            &[],
            &[],
            &[],
            &["meeting.greet"],
            vec![uref("greet", RefKind::Divert, Some("meeting"), None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let resolved_id = resolutions.values().next().unwrap();
        let info = index.symbols.get(resolved_id).unwrap();
        assert_eq!(info.name, "meeting.greet");
    }

    #[test]
    fn external_function_resolves() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[],
            &["print_debug"],
            &[],
            vec![uref("print_debug", RefKind::Function, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn global_variable_resolves() {
        let manifest = make_manifest(
            &[],
            &[],
            &["player_name"],
            &[],
            &[],
            &[],
            vec![uref("player_name", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn ambiguous_bare_list_item_emits_diagnostic() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[("Fruit", &["red"]), ("Color", &["red"])],
            &[],
            &[],
            vec![uref("red", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(resolutions.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E027);
    }

    #[test]
    fn qualified_list_item_resolves_despite_ambiguity() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[("Fruit", &["red"]), ("Color", &["red"])],
            &[],
            &[],
            vec![uref("Color.red", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let resolved_id = resolutions.values().next().unwrap();
        let info = index.symbols.get(resolved_id).unwrap();
        assert_eq!(info.name, "Color.red");
    }
}
