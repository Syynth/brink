use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, LocalSymbol, RefKind, ResolutionMap, ResolvedRef, Scope,
    SymbolIndex, SymbolKind, SymbolManifest,
};

use crate::manifest::local_definition_id;

/// Resolve all unresolved references across files.
///
/// Per-file concatenation of [`resolve_file`], preserving input file order.
/// The production orchestrator (`analyze_with_options`) drives the per-file
/// [`crate::resolve`] query directly; this whole-project wrapper survives as
/// a test convenience.
#[cfg(test)]
pub fn resolve_refs(
    index: &SymbolIndex,
    files: &[(FileId, &SymbolManifest)],
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();

    for &(file_id, manifest) in files {
        let (file_map, file_diags) = resolve_file(index, file_id, manifest);
        map.extend(file_map);
        diagnostics.extend(file_diags);
    }

    (map, diagnostics)
}

/// Resolve one file's unresolved references against the project-wide index.
///
/// Reads only the symbol index and this file's own manifest — never another
/// file's content. This is the per-file dependency seam the query pipeline
/// relies on (substrate spec §4, layer 2 — `resolve(FileId)`).
///
/// Local (param/temp) lookups read `manifest.locals` — this file's own
/// side table — rather than the project-wide index (issue #517): a knot's
/// body lives in exactly one file, so a local can never legitimately be
/// declared in one file and referenced from another. Scoping the lookup to
/// this file's own locals both restores correct behavior for cross-file
/// duplicate-scoped-locals (slice-A finding 4 — no more merged-index
/// aliasing) and lets the project-wide `resolution_index` drop locals
/// entirely, so a body edit that adds/removes a `~ temp` in file Y no
/// longer invalidates file X's `resolve` memo.
pub fn resolve_file(
    index: &SymbolIndex,
    file_id: FileId,
    manifest: &SymbolManifest,
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();
    let locals = &manifest.locals;

    for uref in &manifest.unresolved {
        match uref.kind {
            RefKind::Divert => {
                resolve_divert(index, locals, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::Variable => {
                resolve_variable(index, locals, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::Function => {
                resolve_function(index, locals, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::List => {
                resolve_list_ref(index, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::Struct => {
                resolve_struct_ref(index, file_id, uref, &mut map, &mut diagnostics);
            }
        }
    }

    (map, diagnostics)
}

fn resolve_divert(
    index: &SymbolIndex,
    locals: &[LocalSymbol],
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(id) = lookup_divert(index, locals, uref) {
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

fn lookup_divert(
    index: &SymbolIndex,
    locals: &[LocalSymbol],
    uref: &brink_ir::UnresolvedRef,
) -> Option<DefinitionId> {
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
    lookup_local_in_scope(locals, path, &uref.scope)
}

fn resolve_variable(
    index: &SymbolIndex,
    locals: &[LocalSymbol],
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    if is_builtin_function(path) {
        return;
    }

    match lookup_variable(index, locals, uref) {
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
fn lookup_variable(
    index: &SymbolIndex,
    locals: &[LocalSymbol],
    uref: &brink_ir::UnresolvedRef,
) -> VarResult {
    let path = &uref.path;

    // 1. Locals (params/temps) in scope — they shadow globals
    if let Some(id) = lookup_local_in_scope(locals, path, &uref.scope) {
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

    // 11. TM-4b resolution fallback (docs/typed-mode-spec.md §6): every
    // static dotted-path interpretation above (steps 3-10: list items,
    // lists, knots/stitches, labels — "ink's static dotted paths... resolved
    // first and win") has failed. If the path has more than one segment and
    // its *head* segment alone resolves to a local (param/temp) or a global
    // variable/constant, this is field access on that variable
    // (`p.x`/`p.x.y`) rather than an unresolved static path — the resolved
    // target is the head variable itself; the trailing segment(s) are field
    // names carried structurally by the HIR `Path`, not by this resolution.
    // Struct field-name validity (does `x` exist on `p`'s declared shape?) is
    // a separate construction-time concern (`brink-analyzer::structs`), not
    // resolution's. A single-segment path can never reach here having
    // already failed steps 1-2 above (which already check locals/globals for
    // the *whole* path), so this never fires for a bare variable reference.
    if let Some((head, _rest)) = path.split_once('.') {
        if let Some(id) = lookup_local_in_scope(locals, head, &uref.scope) {
            return VarResult::Found(id);
        }
        if let Some(id) = lookup_by_name(index, head, &[SymbolKind::Variable, SymbolKind::Constant])
        {
            return VarResult::Found(id);
        }
    }

    VarResult::NotFound
}

fn resolve_function(
    index: &SymbolIndex,
    locals: &[LocalSymbol],
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
        check_arity(index, file_id, uref, id, diagnostics);
        return;
    }

    // Try knots (ink allows knots as functions via tunnels)
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Knot]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        check_arity(index, file_id, uref, id, diagnostics);
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
    if let Some(id) = lookup_local_in_scope(locals, path, &uref.scope) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // T1b stdlib slice 1 (docs/t1b-surface-spec.md §5): `len`/`keys`/
    // `values`/`contains`/`push`/`insert`/`remove` with no matching user
    // symbol are the brink-dialect builtins, handled at LIR lowering —
    // same "skip resolution, no diagnostic here" treatment as
    // `is_builtin_function` above. Dialect-agnostic at this layer (an
    // author-defined symbol of the same name always wins regardless of
    // dialect, matched by the lookups above before this is reached);
    // `strict-ink` rejection of an unresolved use is a separate diagnostic
    // (`brink-analyzer::dialect_gate`, which — unlike this resolution pass —
    // does know the dialect).
    if is_t1b_stdlib_name(path) {
        return;
    }

    diagnostics.push(unresolved_diag(
        file_id,
        uref.range,
        path,
        DiagnosticCode::E025,
    ));
}

/// Check that the number of arguments at the call site matches the target's parameter count.
fn check_arity(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    target: DefinitionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(call_arg_count) = uref.arg_count else {
        return;
    };
    let Some(info) = index.symbols.get(&target) else {
        return;
    };
    let expected = info.params.len();
    if call_arg_count != expected {
        diagnostics.push(Diagnostic {
            file: file_id,
            range: uref.range,
            message: format!(
                "{}: `{}` expects {} argument(s), got {}",
                DiagnosticCode::E031.title(),
                uref.path,
                expected,
                call_arg_count,
            ),
            code: DiagnosticCode::E031,
        });
    }
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

/// Resolve a struct construction literal's leading shape name (`Name#{…}`,
/// TM-4b, docs/typed-mode-spec.md §6) against declared `SymbolKind::Struct`
/// symbols. Always a bare (undotted) name — the construction-literal grammar
/// only ever puts a single identifier before `#{` — so this is a direct
/// by-name lookup, no hierarchical/qualified fallback chain like diverts or
/// variables need.
fn resolve_struct_ref(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;
    if let Some(id) = lookup_by_name(index, path, &[SymbolKind::Struct]) {
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
        DiagnosticCode::E068,
    ));
}

// ─── Lookup helpers ─────────────────────────────────────────────────

/// Look up a local variable (param or temp) by bare name within the given
/// scope, among *this file's own* locals (issue #517 — locals never resolve
/// across files, since a knot's body lives in exactly one file).
///
/// A local matches if its name equals the bare name AND its scope is compatible:
/// same knot, and either same stitch or a knot-level param (stitch=None) which
/// is visible in all stitches. When multiple candidates match (e.g. a param and
/// a temp with the same name), picks the closest-preceding declaration.
fn lookup_local_in_scope(
    locals: &[LocalSymbol],
    bare_name: &str,
    scope: &Scope,
) -> Option<DefinitionId> {
    let mut best: Option<&LocalSymbol> = None;

    for local in locals {
        if local.name != bare_name {
            continue;
        }
        // Knot must match
        if local.scope.knot != scope.knot {
            continue;
        }
        // A knot-level local (stitch=None) is visible in all stitches.
        // A stitch-level local is only visible in that stitch.
        if local.scope.stitch.is_some() && local.scope.stitch != scope.stitch {
            continue;
        }
        // Pick closest-preceding by range start
        match best {
            Some(prev) if local.range.start() > prev.range.start() => {
                best = Some(local);
            }
            None => {
                best = Some(local);
            }
            _ => {}
        }
    }

    best.map(|local| local_definition_id(&local.scope, &local.name, local.kind))
}

/// Ink built-in functions that are resolved at LIR lowering, not by the symbol index.
pub(crate) fn is_builtin_function(name: &str) -> bool {
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

/// T1b stdlib slice 1 function names (`docs/t1b-surface-spec.md` §5) plus
/// the TM-3-completion pure conversion intrinsics `int`/`float`/`string`
/// (`docs/typed-mode-spec.md` §4, maintainer ruling 2026-07-13, issue #659,
/// "per the stdlib slice-1 pattern"): lowercase free functions,
/// brink-dialect-gated. Kept in sync by hand with `brink_ir`'s
/// LIR-lowering copy of this same list (`lir::lower::expr::
/// is_t1b_stdlib_name`) — the crates don't share a dependency edge for this
/// purpose in the analysis → codegen direction, mirroring the existing
/// `is_builtin_function`/`recognize_builtin` split for the classic uppercase
/// ink intrinsics above.
///
/// Unlike `is_builtin_function`, a name in this list is *not* unconditionally
/// treated as reserved: `resolve_function`'s lookup chain (externals, knots,
/// lists, variables, locals) always runs first, so an author-defined symbol
/// of the same name resolves normally — shadowing the builtin (§5's
/// ruling) — and only a resolution *failure* additionally checks this list
/// before falling back to the builtin (silently, no diagnostic) instead of
/// emitting E025.
pub(crate) fn is_t1b_stdlib_name(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "keys"
            | "values"
            | "contains"
            | "push"
            | "insert"
            | "remove"
            | "int"
            | "float"
            | "string"
    )
}

/// `pub(crate)`: reused by `signature.rs` (issue #712) to resolve a `#fn`
/// creation-site target to its declaring knot when computing a VAR/CONST
/// global's declaration-derived `fn(T…): R` type — the same "function knot
/// by bare name" lookup [`resolve_function`] itself does first, without
/// needing this pass's locals/scope machinery (a `#fn` target at global-
/// initializer position has no enclosing body to scope against).
pub(crate) fn lookup_by_name(
    index: &SymbolIndex,
    name: &str,
    kinds: &[SymbolKind],
) -> Option<DefinitionId> {
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
        uref_with_args(path, kind, knot, stitch, None)
    }

    fn uref_with_args(
        path: &str,
        kind: RefKind,
        knot: Option<&str>,
        stitch: Option<&str>,
        arg_count: Option<usize>,
    ) -> UnresolvedRef {
        UnresolvedRef {
            path: path.to_string(),
            range: range(900, path.len() as u32),
            kind,
            scope: Scope {
                knot: knot.map(String::from),
                stitch: stitch.map(String::from),
            },
            arg_count,
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
    fn duplicate_knot_emits_warning() {
        let mut m1 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);
        let m2 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);

        // Give m1 different range so they don't collide
        m1.knots[0].range = range(0, 5);

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        // Inklecate permits duplicate definitions — we warn but don't error.
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    #[test]
    fn cross_file_duplicate_knot_local_does_not_leak_across_files() {
        // #517 (finding-4 fix): file B has a duplicate knot name (`dup`,
        // warned E022) but does *not* declare its own `t`. Before the
        // locals split, `lookup_local_in_scope` searched the merged index
        // and would (wrongly) resolve B's reference against A's
        // same-scoped `t`, since the lookup never checked which file a
        // candidate came from. After the split, resolution reads only the
        // referencing file's own `manifest.locals`, so B's reference to an
        // undeclared `t` must fail to resolve instead of silently aliasing
        // A's declaration.
        let mut a = SymbolManifest::default();
        a.knots.push(brink_ir::DeclaredSymbol {
            name: "dup".to_string(),
            range: range(0, 3),
            params: Vec::new(),
            detail: None,
        });
        a.locals.push(LocalSymbol {
            name: "t".to_string(),
            range: range(10, 1),
            scope: Scope {
                knot: Some("dup".to_string()),
                stitch: None,
            },
            kind: SymbolKind::Temp,
            param_detail: None,
        });

        let mut b = SymbolManifest::default();
        b.knots.push(brink_ir::DeclaredSymbol {
            name: "dup".to_string(),
            range: range(100, 3),
            params: Vec::new(),
            detail: None,
        }); // duplicate name -> E022, not indexed
        b.unresolved
            .push(uref("t", RefKind::Variable, Some("dup"), None));

        let files = vec![(FileId(0), &a), (FileId(1), &b)];
        let (index, merge_diags) = merge_manifests(&files);
        assert_eq!(merge_diags.len(), 1, "duplicate knot should warn once");
        assert_eq!(merge_diags[0].code, DiagnosticCode::E022);

        let (resolutions, diags) = resolve_refs(&index, &files);
        assert!(
            resolutions.is_empty(),
            "B's reference to an undeclared local must not resolve, got {resolutions:?}"
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E025);
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

    // ── Arity checking ──────────────────────────────────────────────

    /// Make a manifest with a knot that has a specific number of params.
    fn make_manifest_with_params(
        knot_name: &str,
        param_count: usize,
        unresolved: Vec<UnresolvedRef>,
    ) -> SymbolManifest {
        let mut manifest = SymbolManifest::default();
        let r = range(0, knot_name.len() as u32);
        let params: Vec<brink_ir::ParamInfo> = (0..param_count)
            .map(|i| brink_ir::ParamInfo {
                name: format!("p{i}"),
                is_ref: false,
                is_divert: false,
            })
            .collect();
        manifest.knots.push(DeclaredSymbol {
            name: knot_name.to_string(),
            range: r,
            params,
            detail: Some("function".to_string()),
        });
        manifest.unresolved = unresolved;
        manifest
    }

    #[test]
    fn arity_match_no_warning() {
        // Call `greet(x)` where greet takes 1 param — no warning.
        let manifest = make_manifest_with_params(
            "greet",
            1,
            vec![uref_with_args(
                "greet",
                RefKind::Function,
                None,
                None,
                Some(1),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(resolutions.len(), 1);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for matching arity, got: {diags:?}"
        );
    }

    #[test]
    fn arity_mismatch_emits_e031() {
        // Call `greet(x, y)` where greet takes 1 param — E031 warning.
        let manifest = make_manifest_with_params(
            "greet",
            1,
            vec![uref_with_args(
                "greet",
                RefKind::Function,
                None,
                None,
                Some(2),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(
            resolutions.len(),
            1,
            "should still resolve despite arity mismatch"
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E031);
        assert!(diags[0].message.contains("expects 1"));
        assert!(diags[0].message.contains("got 2"));
    }

    #[test]
    fn arity_check_no_arg_count_no_warning() {
        // Non-function ref (arg_count=None) should never trigger arity check.
        let manifest =
            make_manifest_with_params("greet", 1, vec![uref("greet", RefKind::Divert, None, None)]);
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (_resolutions, diags) = resolve_refs(&index, &files);

        assert!(
            diags.is_empty(),
            "divert should not trigger arity check: {diags:?}"
        );
    }

    #[test]
    fn arity_mismatch_external() {
        // Call external with wrong arity.
        let mut manifest = SymbolManifest::default();
        let r = range(0, 5);
        manifest.externals.push(DeclaredSymbol {
            name: "print".to_string(),
            range: r,
            params: vec![brink_ir::ParamInfo {
                name: "msg".into(),
                is_ref: false,
                is_divert: false,
            }],
            detail: None,
        });
        manifest.unresolved.push(uref_with_args(
            "print",
            RefKind::Function,
            None,
            None,
            Some(3),
        ));
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(resolutions.len(), 1);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E031);
    }

    // ── TM-4b resolution fallback (docs/typed-mode-spec.md §6) ─────────
    //
    // End-to-end (parse -> HIR lower -> merge_manifests -> resolve_file)
    // rather than the hand-built `make_manifest` fixtures above: the
    // precedence claim is about how a *real* dotted `Path` — produced by
    // the actual parser/lowering pipeline, not a synthesized
    // `UnresolvedRef` — resolves, so the fixture needs the real pipeline to
    // be a faithful test of the claim.

    /// Parse -> HIR lower -> `merge_manifests` -> `resolve_file` (the real
    /// production pipeline). Each fixture below is written to produce
    /// exactly one resolvable reference (the LHS of `~ y = …` is an
    /// undeclared `y`, so it stays unresolved — diagnosed, not resolved —
    /// and `-> DONE` is handled specially at the HIR level, never a
    /// resolvable reference), so `resolutions` always has exactly one entry:
    /// the dotted/bare reference under test.
    fn build_real(src: &str) -> (SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (_hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let (index, _diag) = merge_manifests(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) = resolve_file(&index, FileId(0), &manifest);
        (index, resolutions)
    }

    /// The `SymbolKind` a reference spanning exactly `needle` (the first
    /// occurrence of that substring in `src`) resolved to. Ink's own
    /// lowering can register additional bookkeeping references beyond the
    /// one under test (e.g. an implicit fallthrough divert when a knot's
    /// only content is its first stitch) — matching by the reference's
    /// exact source span, rather than assuming `resolutions` has exactly
    /// one entry, is robust to that noise.
    fn resolved_kind_at(
        src: &str,
        index: &SymbolIndex,
        resolutions: &ResolutionMap,
        needle: &str,
    ) -> SymbolKind {
        // The *last* occurrence — every fixture below places the reference
        // under test after any same-named declaration.
        let start = src
            .rfind(needle)
            .expect("needle not found in fixture source");
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test fixture offsets fit in u32"
        )]
        let range = rowan::TextRange::new(
            rowan::TextSize::from(start as u32),
            rowan::TextSize::from((start + needle.len()) as u32),
        );
        let target = resolutions
            .iter()
            .find(|r| r.range == range)
            .expect("no resolution spanning the needle's exact range")
            .target;
        index
            .symbols
            .get(&target)
            .expect("resolved target missing from index")
            .kind
    }

    #[test]
    fn resolution_fallback_static_dotted_path_wins_over_a_colliding_variable_name() {
        // §6's own precedence claim: "ink's static dotted paths (knot.stitch,
        // List.Item) are resolved first and win". `knot` here is BOTH the
        // head of a real stitch path (`knot.stitch`) AND the name of a
        // declared variable — the static path must win, resolving `knot.x`
        // to the stitch, not falling back to field access on the variable.
        let src = "VAR knot = 0\n=== knot ===\n= x\nHello.\n-> DONE\n\
                   === main ===\n~ y = knot.x\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "knot.x"),
            SymbolKind::Stitch,
            "the static `knot.x` stitch path must win over the colliding `knot` variable"
        );
    }

    #[test]
    fn resolution_fallback_resolves_to_head_variable_when_no_static_path_matches() {
        // No knot/stitch/list/label named `p` or `p.x` exists — the fallback
        // resolves `p.x` to the variable `p` itself (field access on it),
        // not an unresolved reference.
        let src = "VAR p = 0\n=== main ===\n~ y = p.x\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "p.x"),
            SymbolKind::Variable
        );
    }

    #[test]
    fn resolution_fallback_resolves_to_head_param() {
        // Same fallback, but the head is a knot parameter rather than a
        // global — the fallback checks locals (params/temps) first, per
        // `lookup_variable`'s existing local-shadows-global ordering.
        let src = "=== main(p) ===\n~ y = p.x\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "p.x"),
            SymbolKind::Param
        );
    }

    #[test]
    fn resolution_fallback_does_not_apply_to_a_single_segment_path() {
        // A bare `p` (no dot at all) must resolve exactly as it always has
        // — the fallback is gated on `path.contains('.')` and must never
        // fire for an ordinary single-segment variable reference.
        let src = "VAR p = 0\n=== main ===\n~ y = p\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "p"),
            SymbolKind::Variable
        );
    }

    #[test]
    fn struct_literal_resolves_shape_name_to_the_declared_struct() {
        let src = "STRUCT Point = #{x: float}\n=== main ===\n~ p = Point#{x: 1.0}\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "Point"),
            SymbolKind::Struct
        );
    }

    #[test]
    fn struct_literal_unresolved_shape_name_is_e068() {
        let src = "=== main ===\n~ p = Bogus#{x: 1}\n-> DONE\n";
        let parsed = brink_syntax::parse(src);
        let (_hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let (index, _diag) = merge_manifests(&[(FileId(0), &manifest)]);
        let (resolutions, diags) = resolve_file(&index, FileId(0), &manifest);
        assert!(
            resolutions.is_empty(),
            "no resolution for an undeclared shape: {resolutions:?}"
        );
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E068),
            "{diags:?}"
        );
    }
}
