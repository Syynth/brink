use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, RefKind, ResolutionMap, ResolvedRef, SymbolIndex,
    SymbolKind, SymbolManifest,
};

/// Resolve all unresolved references across files.
pub fn resolve_refs(
    index: &SymbolIndex,
    files: &[(FileId, &SymbolManifest)],
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();

    for &(file_id, manifest) in files {
        for uref in &manifest.unresolved {
            match uref.kind {
                RefKind::Divert => {
                    resolve_divert(index, file_id, uref, &mut map, &mut diagnostics);
                }
                RefKind::Variable => {
                    resolve_variable(index, file_id, uref, &mut map, &mut diagnostics);
                }
                RefKind::Function => {
                    resolve_function(index, file_id, uref, &mut map, &mut diagnostics);
                }
                RefKind::List => {
                    resolve_list_ref(index, file_id, uref, &mut map, &mut diagnostics);
                }
            }
        }
    }

    (map, diagnostics)
}

fn resolve_divert(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(id) = lookup_divert(index, uref) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
    } else {
        diagnostics.push(unresolved_diag(
            file_id,
            uref.range,
            &uref.path,
            DiagnosticCode::E024,
        ));
    }
}

fn lookup_divert(index: &SymbolIndex, uref: &brink_ir::UnresolvedRef) -> Option<DefinitionId> {
    let path = &uref.path;

    // Dotted path — try exact qualified lookup, then qualify with current knot
    if path.contains('.') {
        if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Stitch, SymbolKind::Label]) {
            return Some(id);
        }
        // Try qualifying with current knot scope (e.g., `a_package.forest` → `adventure.a_package.forest`)
        if let Some(knot) = &uref.scope.knot {
            let qualified = format!("{knot}.{path}");
            if let Some(id) =
                lookup_by_name(index, &qualified, &[SymbolKind::Stitch, SymbolKind::Label])
            {
                return Some(id);
            }
        }
        return None;
    }

    // Single segment — ink's hierarchical resolution:
    // 1. Stitch or label in current knot
    if let Some(knot) = &uref.scope.knot {
        let qualified = format!("{knot}.{path}");
        if let Some(id) =
            lookup_by_name(index, &qualified, &[SymbolKind::Stitch, SymbolKind::Label])
        {
            return Some(id);
        }
        // Label in current stitch (knot.stitch.label)
        if let Some(stitch) = &uref.scope.stitch
            && let Some(id) = lookup_by_name(
                index,
                &format!("{knot}.{stitch}.{path}"),
                &[SymbolKind::Label],
            )
        {
            return Some(id);
        }
    }

    // 2. Knot at top level
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot]) {
        return Some(id);
    }

    // 3. Top-level stitch (bare name, no parent knot)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Stitch]) {
        return Some(id);
    }

    // 4. Label anywhere in current knot (search by suffix)
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, knot, path)
    {
        return Some(id);
    }

    // 5. Top-level label — stored as bare name (visible from any scope)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Label]) {
        return Some(id);
    }

    // 6. Variable divert target (`VAR x = -> knot`, then `-> x`)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Variable]) {
        return Some(id);
    }

    // 7. Divert parameter in scope (`=== knot(-> x) ===` then `-> x`)
    lookup_local_in_scope(index, path, &uref.scope)
}

fn resolve_variable(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    if is_builtin_function(path) {
        return;
    }

    match lookup_variable(index, uref) {
        VarResult::Found(id) => {
            map.push(ResolvedRef {
                file: file_id,
                range: uref.range,
                target: id,
            });
        }
        VarResult::Ambiguous => {
            diagnostics.push(ambiguous_diag(file_id, uref.range, path));
        }
        VarResult::NotFound => {
            diagnostics.push(unresolved_diag(
                file_id,
                uref.range,
                path,
                DiagnosticCode::E025,
            ));
        }
    }
}

enum VarResult {
    Found(DefinitionId),
    Ambiguous,
    NotFound,
}

/// Hierarchical variable lookup — returns the first match in priority order.
fn lookup_variable(index: &SymbolIndex, uref: &brink_ir::UnresolvedRef) -> VarResult {
    let path = &uref.path;

    // 1. Locals (params/temps) in scope — they shadow globals
    if let Some(id) = lookup_local_in_scope(index, path, &uref.scope) {
        return VarResult::Found(id);
    }

    // 2. Global variables / constants
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Variable, SymbolKind::Constant]) {
        return VarResult::Found(id);
    }

    // 3. List items by bare name
    match lookup_list_item_bare(index, path) {
        BareItemResult::Unique(id) => return VarResult::Found(id),
        BareItemResult::Ambiguous => return VarResult::Ambiguous,
        BareItemResult::NotFound => {}
    }

    // 4. Qualified list item (ListName.ItemName)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, path, &[SymbolKind::ListItem])
    {
        return VarResult::Found(id);
    }

    // 5. List names
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::List]) {
        return VarResult::Found(id);
    }

    // 6. Knots and top-level stitches (visit counts)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot, SymbolKind::Stitch]) {
        return VarResult::Found(id);
    }

    // 7. Stitches in current knot scope
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_by_name(index, &format!("{knot}.{path}"), &[SymbolKind::Stitch])
    {
        return VarResult::Found(id);
    }

    // 8. Qualified stitch/label (e.g. `knot.stitch` or `knot.stitch.label` visit count)
    if path.contains('.') {
        if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Stitch, SymbolKind::Label]) {
            return VarResult::Found(id);
        }
        // Try `knot.label` where label is stored as `knot.*.label` (label inside a stitch)
        if let Some((knot, label)) = path.split_once('.')
            && !label.contains('.')
            && let Some(id) = lookup_label_in_knot(index, knot, label)
        {
            return VarResult::Found(id);
        }
    }

    // 9. Labels in current knot
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, knot, path)
    {
        return VarResult::Found(id);
    }

    // 10. Labels at top level (no knot scope)
    if uref.scope.knot.is_none()
        && let Some(id) = lookup_by_name(index, path, &[SymbolKind::Label])
    {
        return VarResult::Found(id);
    }

    VarResult::NotFound
}

fn resolve_function(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Built-in functions don't need resolution — they're handled at LIR lowering.
    if is_builtin_function(path) {
        return;
    }

    // Try externals first
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::External]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try knots (ink allows knots as functions via tunnels)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try list names (ink allows `list(n)` as type conversion)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::List]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try variables (ink allows calling a variable holding a function ref)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Variable]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try locals (temps/params used as function names, e.g. `{storyletFunction(args)}`)
    if let Some(id) = lookup_local_in_scope(index, path, &uref.scope) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    diagnostics.push(unresolved_diag(
        file_id,
        uref.range,
        path,
        DiagnosticCode::E025,
    ));
}

fn resolve_list_ref(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try qualified list item (ListName.ItemName)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, path, &[SymbolKind::ListItem])
    {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try bare list item name
    match lookup_list_item_bare(index, path) {
        BareItemResult::Unique(id) => {
            map.push(ResolvedRef {
                file: file_id,
                range: uref.range,
                target: id,
            });
            return;
        }
        BareItemResult::Ambiguous => {
            diagnostics.push(ambiguous_diag(file_id, uref.range, path));
            return;
        }
        BareItemResult::NotFound => {}
    }

    // Try list name
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::List]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    diagnostics.push(unresolved_diag(
        file_id,
        uref.range,
        path,
        DiagnosticCode::E025,
    ));
}

// ─── Lookup helpers ─────────────────────────────────────────────────

/// Look up a local variable (param or temp) by bare name within the given scope.
///
/// A local matches if its name equals the bare name AND its scope is compatible:
/// same knot, and either same stitch or a knot-level param (stitch=None) which
/// is visible in all stitches. When multiple candidates match (e.g. a param and
/// a temp with the same name), picks the closest-preceding declaration.
fn lookup_local_in_scope(
    index: &SymbolIndex,
    bare_name: &str,
    scope: &brink_ir::Scope,
) -> Option<DefinitionId> {
    let ids = index.by_name.get(bare_name)?;
    let mut best: Option<(DefinitionId, rowan::TextRange)> = None;

    for &id in ids {
        let info = index.symbols.get(&id)?;
        if !matches!(info.kind, SymbolKind::Param | SymbolKind::Temp) {
            continue;
        }
        let Some(sym_scope) = &info.scope else {
            continue;
        };
        // Knot must match
        if sym_scope.knot != scope.knot {
            continue;
        }
        // A knot-level local (stitch=None) is visible in all stitches.
        // A stitch-level local is only visible in that stitch.
        if sym_scope.stitch.is_some() && sym_scope.stitch != scope.stitch {
            continue;
        }
        // Pick closest-preceding by range start
        match &best {
            Some((_, prev_range)) if info.range.start() > prev_range.start() => {
                best = Some((id, info.range));
            }
            None => {
                best = Some((id, info.range));
            }
            _ => {}
        }
    }

    best.map(|(id, _)| id)
}

/// Ink built-in functions that are resolved at LIR lowering, not by the symbol index.
fn is_builtin_function(name: &str) -> bool {
    matches!(
        name,
        "TURNS_SINCE"
            | "CHOICE_COUNT"
            | "RANDOM"
            | "SEED_RANDOM"
            | "INT"
            | "FLOAT"
            | "FLOOR"
            | "CEILING"
            | "POW"
            | "MIN"
            | "MAX"
            | "LIST_COUNT"
            | "LIST_MIN"
            | "LIST_MAX"
            | "LIST_ALL"
            | "LIST_INVERT"
            | "LIST_RANGE"
            | "LIST_RANDOM"
            | "LIST_VALUE"
            | "LIST_FROM_INT"
            | "READ_COUNT"
            | "TURNS"
    )
}

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

    // Try knot.*.label (any stitch within this knot).
    // Collect all matches and pick the smallest `DefinitionId` for determinism,
    // since `HashMap` iteration order is not stable across processes.
    let suffix = format!(".{label}");
    let prefix = format!("{knot}.");
    let mut best: Option<DefinitionId> = None;
    for (name, ids) in &index.by_name {
        if name.starts_with(&prefix) && name.ends_with(&suffix) && name.matches('.').count() == 2 {
            for id in ids {
                if let Some(info) = index.symbols.get(id)
                    && info.kind == SymbolKind::Label
                {
                    best = Some(match best {
                        Some(prev) if prev.to_raw() <= id.to_raw() => prev,
                        _ => *id,
                    });
                }
            }
        }
    }
    best
}

fn ambiguous_diag(file: FileId, range: rowan::TextRange, path: &str) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: format!(
            "{}: `{path}` — qualify with the list name (e.g., `ListName.{path}`)",
            DiagnosticCode::E027.title(),
        ),
        code: DiagnosticCode::E027,
    }
}

fn unresolved_diag(
    file: FileId,
    range: rowan::TextRange,
    path: &str,
    code: DiagnosticCode,
) -> Diagnostic {
    Diagnostic {
        file,
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
                params: Vec::new(),
                detail: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in stitches {
            let r = range(offset, name.len() as u32);
            manifest.stitches.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in variables {
            let r = range(offset, name.len() as u32);
            manifest.variables.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &(list_name, items) in lists {
            let r = range(offset, list_name.len() as u32);
            manifest.lists.push(DeclaredSymbol {
                name: list_name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
            });
            offset += list_name.len() as u32 + 1;
            for &item in items {
                let qualified = format!("{list_name}.{item}");
                let r = range(offset, item.len() as u32);
                manifest.list_items.push(DeclaredSymbol {
                    name: qualified,
                    range: r,
                    params: Vec::new(),
                    detail: None,
                });
                offset += item.len() as u32 + 1;
            }
        }
        for &name in externals {
            let r = range(offset, name.len() as u32);
            manifest.externals.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in labels {
            let r = range(offset, name.len() as u32);
            manifest.labels.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
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
        let files = vec![(FileId(0), &manifest)];
        let (index, merge_diags) = merge_manifests(&files);
        let (resolutions, resolve_diags) = resolve_refs(&index, &files);

        assert!(merge_diags.is_empty());
        assert!(resolve_diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].file, FileId(0));
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
        let files = vec![(FileId(0), &manifest)];
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
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        // The resolved ID should be for bedroom.look
        let info = index.symbols.get(&resolutions[0].target).unwrap();
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
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(resolutions.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E024);
    }

    #[test]
    fn duplicate_knot_is_silently_accepted() {
        let mut m1 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);
        let m2 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);

        // Give m2 different ranges so they don't collide
        m1.knots[0].range = range(0, 5);

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        // Inklecate permits duplicate definitions — no diagnostic emitted.
        assert!(diags.is_empty());
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
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let info = index.symbols.get(&resolutions[0].target).unwrap();
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
        let files = vec![(FileId(0), &manifest)];
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
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let info = index.symbols.get(&resolutions[0].target).unwrap();
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
        let files = vec![(FileId(0), &manifest)];
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
        let files = vec![(FileId(0), &manifest)];
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
        let files = vec![(FileId(0), &manifest)];
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
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let info = index.symbols.get(&resolutions[0].target).unwrap();
        assert_eq!(info.name, "Color.red");
    }
}
