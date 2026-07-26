use std::collections::BTreeSet;

use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, Import, LocalSymbol, RefKind, ResolutionMap, ResolvedRef,
    Scope, SymbolIndex, SymbolInfo, SymbolKind, SymbolManifest, Visibility,
};

use crate::manifest::local_definition_id;

/// Per-file import coverage, shared verbatim by resolution ([`ImportScope`])
/// and the E025 import-required checker (`modules::import_covers`): the set
/// of modules imported **qualified** (`IMPORT mod`, which licenses
/// `module.name` access to any public export) and the `(module, name)` pairs
/// brought into scope by **bare** imports (`IMPORT { name } FROM mod`, which
/// is name-precise — it does *not* license every other export of `mod`).
///
/// A single source of truth for this distinction: an earlier version of
/// `ImportScope` collapsed every import to just its module name, so a bare
/// `IMPORT { other } FROM mod` was (wrongly) treated as importing *all* of
/// `mod`, silently disagreeing with `import_covers`'s name-precise gate.
#[must_use]
pub(crate) fn import_coverage_for_file(
    imports: &[Import],
) -> (BTreeSet<&str>, BTreeSet<(&str, &str)>) {
    let mut qualified = BTreeSet::new();
    let mut bare = BTreeSet::new();
    for import in imports {
        if import.bare {
            for item in &import.items {
                bare.insert((import.module.as_str(), item.name.as_str()));
            }
        } else {
            qualified.insert(import.module.as_str());
        }
    }
    (qualified, bare)
}

/// Per-file import context threaded into resolution (M-2d,
/// docs/modules-spec.md §2; issue #790) so a bare reference with multiple
/// cross-module candidates binds to the module *this file* actually imports —
/// "names cross module boundaries only via import" — rather than to the flat
/// duplicate-winner.
///
/// The default (empty) scope reproduces the pre-M-2d flat behavior exactly:
/// the entire strict-ink and single-module world has at most one candidate
/// per (name, kind), so [`lookup_by_name`]'s fast path returns it unchanged
/// and this context never influences the result. It only ever disambiguates
/// the genuinely-new case unlocked by relaxing the #784/#793 stopgap: two
/// *declared* modules publicly defining the same name, now coexisting in the
/// index instead of one being suppressed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportScope {
    /// The referring file's own **declared** module (`None` for an
    /// undeclared stem-module / the legacy world). A candidate declared in
    /// this same module is bare-visible without any import.
    pub file_module: Option<String>,
    /// Modules this file imports **qualified** (`IMPORT mod`) — licenses
    /// `module.name` access to any public export of that module.
    pub qualified_modules: BTreeSet<String>,
    /// `(module, name)` pairs this file imports **bare**
    /// (`IMPORT { name } FROM mod`) — name-precise, matching
    /// `modules::import_covers` exactly so resolution and the E025
    /// import-required diagnostic can never diverge. `BTreeSet` for
    /// determinism.
    pub bare_imports: BTreeSet<(String, String)>,
}

impl ImportScope {
    /// Build the scope for one file from its resolved (declared) module and
    /// its HIR `IMPORT` list.
    #[must_use]
    pub fn new(file_module: Option<String>, imports: &[Import]) -> Self {
        let (qualified, bare) = import_coverage_for_file(imports);
        Self {
            file_module,
            qualified_modules: qualified.into_iter().map(str::to_string).collect(),
            bare_imports: bare
                .into_iter()
                .map(|(module, name)| (module.to_string(), name.to_string()))
                .collect(),
        }
    }
}

/// How a candidate definition relates to the referring file's import scope.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Candidacy {
    /// Bare-visible without an import: the legacy world (`module == None`) or
    /// a definition in the referrer's own declared module.
    InScope,
    /// A public definition in a **declared** module this file imports.
    Imported,
    /// Neither — a cross-module definition this file has no line of sight to.
    Other,
}

/// Classify a candidate against the referring file's import scope (M-2d).
fn classify(scope: &ImportScope, info: &SymbolInfo) -> Candidacy {
    match &info.module {
        // Undeclared stem-module / legacy soup — always bare-visible, so the
        // pre-modules corpus is untouched.
        None => Candidacy::InScope,
        Some(module) => {
            if scope.file_module.as_deref() == Some(module.as_str()) {
                Candidacy::InScope
            } else if info.visibility == Visibility::Public
                && (scope.qualified_modules.contains(module)
                    || scope
                        .bare_imports
                        .contains(&(module.clone(), info.name.clone())))
            {
                Candidacy::Imported
            } else {
                Candidacy::Other
            }
        }
    }
}

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

    let scope = ImportScope::default();
    for &(file_id, manifest) in files {
        let (file_map, file_diags) = resolve_file(index, &scope, file_id, manifest);
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
    scope: &ImportScope,
    file_id: FileId,
    manifest: &SymbolManifest,
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();
    let locals = &manifest.locals;

    for uref in &manifest.unresolved {
        match uref.kind {
            RefKind::Divert => {
                resolve_divert(
                    index,
                    scope,
                    locals,
                    file_id,
                    uref,
                    &mut map,
                    &mut diagnostics,
                );
            }
            RefKind::Variable => {
                resolve_variable(
                    index,
                    scope,
                    locals,
                    file_id,
                    uref,
                    &mut map,
                    &mut diagnostics,
                );
            }
            RefKind::Function => {
                resolve_function(
                    index,
                    scope,
                    locals,
                    file_id,
                    uref,
                    &mut map,
                    &mut diagnostics,
                );
            }
            RefKind::List => {
                resolve_list_ref(index, scope, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::Struct => {
                resolve_struct_ref(index, scope, file_id, uref, &mut map, &mut diagnostics);
            }
        }
    }

    (map, diagnostics)
}

fn resolve_divert(
    index: &SymbolIndex,
    scope: &ImportScope,
    locals: &[LocalSymbol],
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(id) = lookup_divert(index, scope, locals, uref) {
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
    scope: &ImportScope,
    locals: &[LocalSymbol],
    uref: &brink_ir::UnresolvedRef,
) -> Option<DefinitionId> {
    let path = &uref.path;

    // Dotted path — try exact qualified lookup, then qualify with current knot
    if path.contains('.') {
        if let Some(id) =
            lookup_by_name(index, scope, path, &[SymbolKind::Stitch, SymbolKind::Label])
        {
            return Some(id);
        }
        // Try qualifying with current knot scope (e.g., `a_package.forest` → `adventure.a_package.forest`)
        if let Some(knot) = &uref.scope.knot {
            let qualified = format!("{knot}.{path}");
            if let Some(id) = lookup_by_name(
                index,
                scope,
                &qualified,
                &[SymbolKind::Stitch, SymbolKind::Label],
            ) {
                return Some(id);
            }
        }
        return None;
    }

    // Single segment — ink's hierarchical resolution:
    // 1. Stitch or label in current knot
    if let Some(knot) = &uref.scope.knot {
        let qualified = format!("{knot}.{path}");
        if let Some(id) = lookup_by_name(
            index,
            scope,
            &qualified,
            &[SymbolKind::Stitch, SymbolKind::Label],
        ) {
            return Some(id);
        }
        // Label in current stitch (knot.stitch.label)
        if let Some(stitch) = &uref.scope.stitch
            && let Some(id) = lookup_by_name(
                index,
                scope,
                &format!("{knot}.{stitch}.{path}"),
                &[SymbolKind::Label],
            )
        {
            return Some(id);
        }
    }

    // 2. Knot at top level
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Knot]) {
        return Some(id);
    }

    // 3. Top-level stitch (bare name, no parent knot)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Stitch]) {
        return Some(id);
    }

    // 4. Label anywhere in current knot (search by suffix)
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, scope, knot, path)
    {
        return Some(id);
    }

    // 5. Top-level label — stored as bare name (visible from any scope)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Label]) {
        return Some(id);
    }

    // 6. Variable divert target (`VAR x = -> knot`, then `-> x`)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Variable]) {
        return Some(id);
    }

    // 7. Divert parameter in scope (`=== knot(-> x) ===` then `-> x`)
    lookup_local_in_scope(locals, path, &uref.scope)
}

fn resolve_variable(
    index: &SymbolIndex,
    scope: &ImportScope,
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

    match lookup_variable(index, scope, locals, uref) {
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
            // NS-A1 (`docs/stdlib-spec.md` §1.4): an otherwise-unresolved
            // bare `none` is the brink-dialect Option absence literal —
            // same "skip resolution, no diagnostic here" treatment as the
            // T1b stdlib call names in `resolve_function`. Every user
            // symbol interpretation above wins first (a LIST item, VAR,
            // temp, … named `none` shadows the literal, E035-warned at its
            // declaration); `strict-ink` rejection is the dialect gate's
            // job, and the bare-`none`-needs-context declaration rule is
            // E107 (`option_rules`).
            if path == "none" {
                return;
            }
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
    scope: &ImportScope,
    locals: &[LocalSymbol],
    uref: &brink_ir::UnresolvedRef,
) -> VarResult {
    let path = &uref.path;

    // 1. Locals (params/temps) in scope — they shadow globals
    if let Some(id) = lookup_local_in_scope(locals, path, &uref.scope) {
        return VarResult::Found(id);
    }

    // 2. Global variables / constants
    if let Some(id) = lookup_by_name(
        index,
        scope,
        path,
        &[SymbolKind::Variable, SymbolKind::Constant],
    ) {
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
        && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::ListItem])
    {
        return VarResult::Found(id);
    }

    // 5. List names
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::List]) {
        return VarResult::Found(id);
    }

    // 6. Knots and top-level stitches (visit counts)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Knot, SymbolKind::Stitch]) {
        return VarResult::Found(id);
    }

    // 7. Stitches in current knot scope
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_by_name(
            index,
            scope,
            &format!("{knot}.{path}"),
            &[SymbolKind::Stitch],
        )
    {
        return VarResult::Found(id);
    }

    // 8. Qualified stitch/label (e.g. `knot.stitch` or `knot.stitch.label` visit count)
    if path.contains('.') {
        if let Some(id) =
            lookup_by_name(index, scope, path, &[SymbolKind::Stitch, SymbolKind::Label])
        {
            return VarResult::Found(id);
        }
        // Try `knot.label` where label is stored as `knot.*.label` (label inside a stitch)
        if let Some((knot, label)) = path.split_once('.')
            && !label.contains('.')
            && let Some(id) = lookup_label_in_knot(index, scope, knot, label)
        {
            return VarResult::Found(id);
        }
    }

    // 9. Labels in current knot
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, scope, knot, path)
    {
        return VarResult::Found(id);
    }

    // 10. Labels at top level (no knot scope)
    if uref.scope.knot.is_none()
        && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Label])
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
        if let Some(id) = lookup_by_name(
            index,
            scope,
            head,
            &[SymbolKind::Variable, SymbolKind::Constant],
        ) {
            return VarResult::Found(id);
        }
    }

    VarResult::NotFound
}

fn resolve_function(
    index: &SymbolIndex,
    scope: &ImportScope,
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
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::External]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        check_arity(index, file_id, uref, id, diagnostics);
        return;
    }

    // Try knots (ink allows knots as functions via tunnels)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Knot]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        check_arity(index, file_id, uref, id, diagnostics);
        return;
    }

    // Try list names (ink allows `list(n)` as type conversion)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::List]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try variables (ink allows calling a variable holding a function ref)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Variable]) {
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

    // B3a UFCS-shaped callee (issue #1482, D1–D5 RULED 2026-07-26): every
    // static dotted-path interpretation above has failed, but the path's
    // *head* segment alone names a value in scope — this is method-call
    // syntax on that value (`g.greet(3)`), not an unresolved static path.
    // Exactly the TM-4b fallback `lookup_variable`'s step 11 already applies
    // to a dotted *value* reference (`p.x.y`), applied to the callee
    // position: the resolved target is the head value itself, and the
    // trailing segment is carried structurally by the HIR `Path`.
    //
    // Which of the two meanings that trailing segment has (a callable field
    // of the receiver's type, or a free function to desugar onto) is a
    // *type-directed* question this resolution pass cannot answer — so it
    // resolves the receiver and stays silent, and `brink-analyzer::ufcs`
    // owns the verdict and every diagnostic for the site (`E140`–`E143`).
    // Suppressing `E025` here is what keeps a legal method call
    // diagnostic-free and an illegal one from being reported twice.
    //
    // Inert for the ink corpus by construction: ink's own `FunctionCall`
    // lowering always builds a single-segment callee path, so no ink source
    // can reach this branch (see `ufcs`' module doc).
    if let Some((head, _rest)) = path.split_once('.')
        && let Some(id) = lookup_local_in_scope(locals, head, &uref.scope).or_else(|| {
            lookup_by_name(
                index,
                scope,
                head,
                &[SymbolKind::Variable, SymbolKind::Constant],
            )
        })
    {
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
    scope: &ImportScope,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try qualified list item (ListName.ItemName)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::ListItem])
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
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::List]) {
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
    scope: &ImportScope,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Struct]) {
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
            // T1c call forms (docs/t1c-spec.md §3): `call(f, args…)` (explicit
            // invocation) and `bind(f, args…)` (val-only currying) are
            // brink-dialect stdlib names so an unresolved use isn't E025 —
            // they lower to `CallValue`/`BindValue` and dispatch through a
            // function value. `bind` is effect-transparent (copies the value's
            // row); its typing rule (consume the head of the param row) is
            // wired into the checker (issue #733, `infer::body::
            // check_bind_value`) — under `types = strict` a known `Ty::Fn`
            // callee is statically checked (over-binding is `E063`); an
            // `Unknown`/`Conflicted` callee still escapes as `E065`/`E066`.
            // Gradual mode is unaffected: the runtime fault stays the
            // backstop for both `call` and `bind`, exactly as before.
            | "call"
            | "bind"
            // `char_at(s, i)` (T1b stdlib slice 1 completion, issue #857):
            // chars-indexed (Unicode scalar values, not bytes) single-
            // character-`String` read into `s`. VM-native, same
            // shadowing/dialect-gate machinery as the rest of this list.
            | "char_at"
            // NS-A1 (issue #1107, `docs/stdlib-spec.md` §§3-5 + §1.4): the
            // Option verb flips — text `find`, seq `index_of`/`min`/`max`/
            // `first`/`last`/`pop`, map `get`/`contains_value`/`clear` —
            // plus the `some(x)` Option constructor. Same slice-1 machinery
            // end to end (shadowable with E035, `strict-ink` rejection via
            // the dialect gate). The bare `none` literal resolves in
            // *variable* position (see `resolve_variable`), not here.
            | "find"
            | "index_of"
            | "min"
            | "max"
            | "first"
            | "last"
            | "pop"
            | "get"
            | "contains_value"
            | "clear"
            | "some"
            // NS-A6 (issue #1112, `docs/stdlib-spec.md` §7): the
            // `std::rand` draw verbs — every one an ordinary *write* to
            // the RNG state cell (`DefinitionId::RNG_CELL`) in the effect
            // row. `float` (nullary draw / unary conversion — the F4
            // arity split, resolved in-wave) and `int` (conversion only;
            // `int(range)` waits on A5's inhabited-range refinement) are
            // already listed above. Same slice-1 machinery end to end:
            // shadowable with E035, `strict-ink` rejection via the
            // dialect gate.
            | "chance"
            | "pick"
            | "shuffle"
            | "shuffled"
            | "seed"
            // NS-A5 (issue #1111, `docs/stdlib-spec.md` §7): the
            // inhabited-range validator `non_empty(r)` →
            // `Option[NonEmptyRange]`. Pure — no draw. `int(range)` (the
            // draw leg) needs no entry: `int` is already listed above and
            // the VM dispatches on the operand. Same slice-1 machinery:
            // shadowable with E035, `strict-ink` rejection via the
            // dialect gate.
            | "non_empty"
            // NS-A7 (issue #1113, `docs/stdlib-spec.md` §8): `Weighted[T]`
            // construction (`weighted(w1, v1, …)` — E120 refuses
            // statically-malformed tables), the `roll(w)` draw (an
            // RNG-cell write like the NS-A6 verbs), and the humble heap
            // (`heap_push`/`heap_pop`/`heap_peek` over ordinary arrays,
            // §4b ordering). Same slice-1 machinery: shadowable with
            // E035, `strict-ink` rejection via the dialect gate.
            | "weighted"
            | "roll"
            | "heap_push"
            | "heap_pop"
            | "heap_peek"
            // NS-A4 (issue #1110, `docs/stdlib-spec.md` §4b, F0): the
            // ordering verbs — imperative in-place `sort`/`sort_by` +
            // functional past-participle twins `sorted`/`sorted_by`.
            // Same slice-1 machinery: shadowable with E035, `strict-ink`
            // rejection via the dialect gate.
            | "sort"
            | "sort_by"
            | "sorted"
            | "sorted_by"
            // NS-A8 (issue #1114, `docs/tower-mini-spec.md`; ruled shape
            // `docs/stdlib-spec.md` §2b): the numeric tower — constructors
            // (`vec2(x, y)` … `mat4(c0, c1, c2, c3)`, matrices from
            // column vectors per T3's column-major pin), `dot`/`cross`,
            // and the tower-wide `clamp`/`lerp` (`min`/`max` are already
            // listed above — their two-arg call shape lowers to the tower
            // componentwise forms, the one-arg shape stays the NS-A1
            // array extremum). Same slice-1 machinery end to end:
            // shadowable with E035, `strict-ink` rejection via the
            // dialect gate. All pure.
            | "vec2"
            | "vec3"
            | "vec4"
            | "quat"
            | "mat2"
            | "mat3"
            | "mat4"
            | "dot"
            | "cross"
            | "clamp"
            | "lerp"
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
    scope: &ImportScope,
    name: &str,
    kinds: &[SymbolKind],
) -> Option<DefinitionId> {
    let ids = index.by_name.get(name)?;

    let mut first_match: Option<DefinitionId> = None;
    let mut first_in_scope: Option<DefinitionId> = None;
    let mut first_imported: Option<DefinitionId> = None;
    let mut multiple = false;

    for id in ids {
        let Some(info) = index.symbols.get(id) else {
            continue;
        };
        if !kinds.contains(&info.kind) {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(*id);
        } else {
            multiple = true;
        }
        match classify(scope, info) {
            Candidacy::InScope if first_in_scope.is_none() => first_in_scope = Some(*id),
            Candidacy::Imported if first_imported.is_none() => first_imported = Some(*id),
            _ => {}
        }
    }

    // Fast path (byte-identity guarantee): with zero or one candidate of the
    // requested kind — the entire strict-ink and single-module world — the
    // sole match is returned exactly as the pre-M-2d flat lookup did, so the
    // import scope never changes an existing corpus's resolution.
    if !multiple {
        return first_match;
    }

    // Multiple cross-module candidates (only reachable now that the #784/#793
    // stopgap is relaxed and same-name public defs coexist): the referrer's
    // own-module / legacy candidate wins, else an imported public one, else
    // fall back to the flat first-winner — which keeps `modules::check`'s
    // E025 import-required diagnostic (keyed off the resolved target) firing
    // for a genuinely un-imported cross-module reference, exactly as before.
    first_in_scope.or(first_imported).or(first_match)
}

/// Result of a bare list item lookup.
///
/// `pub(crate)` (issue #628): the phase-0 `Sig` stub's list-literal type
/// inference (`external_check::resolve_list_item_name`) reuses this exact
/// bare-name resolution — the declaring LIST is always a project-global
/// lookup, never locally scoped, so that stub can call straight in without
/// threading an `ImportScope` through `signature()`.
pub(crate) enum BareItemResult {
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
pub(crate) fn lookup_list_item_bare(index: &SymbolIndex, bare_name: &str) -> BareItemResult {
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
fn lookup_label_in_knot(
    index: &SymbolIndex,
    scope: &ImportScope,
    knot: &str,
    label: &str,
) -> Option<DefinitionId> {
    // Try knot.label
    let direct = format!("{knot}.{label}");
    if let Some(id) = lookup_by_name(index, scope, &direct, &[SymbolKind::Label]) {
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
    use brink_ir::{DeclaredSymbol, ImportItem, Scope, UnresolvedRef};
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
                visibility: None,
                was: None,
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
                visibility: None,
                was: None,
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
                visibility: None,
                was: None,
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
                visibility: None,
                was: None,
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
                    visibility: None,
                    was: None,
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
                visibility: None,
                was: None,
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
                visibility: None,
                was: None,
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
            visibility: None,
            was: None,
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
            visibility: None,
            was: None,
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
            visibility: None,
            was: None,
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
            visibility: None,
            was: None,
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
        let (resolutions, _diag) =
            resolve_file(&index, &ImportScope::default(), FileId(0), &manifest);
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
        let (resolutions, diags) =
            resolve_file(&index, &ImportScope::default(), FileId(0), &manifest);
        assert!(
            resolutions.is_empty(),
            "no resolution for an undeclared shape: {resolutions:?}"
        );
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E068),
            "{diags:?}"
        );
    }

    // ── M-2d import-scoped lookup (issue #790) ────────────────────────

    /// A hand-built index with two `Knot`s named `ambush` in *different*
    /// declared modules (the coexistence unlocked by relaxing the #784/#793
    /// stopgap). Insertion order is `quest_a`, then `quest_b`, so the flat
    /// first-winner is always `quest_a` — the import scope is what makes
    /// `quest_b` reachable.
    fn two_module_ambush_index() -> (SymbolIndex, DefinitionId, DefinitionId) {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let mk = |index: &mut SymbolIndex, module: &str, hash: u64| {
            let id = DefinitionId::new(DefinitionTag::Address, hash);
            index.symbols.insert(
                id,
                SymbolInfo {
                    kind: SymbolKind::Knot,
                    file: FileId(0),
                    range: TextRange::default(),
                    id,
                    name: "ambush".to_string(),
                    params: Vec::new(),
                    detail: None,
                    scope: None,
                    param_detail: None,
                    module: Some(module.to_string()),
                    visibility: Visibility::Public,
                },
            );
            index
                .by_name
                .entry("ambush".to_string())
                .or_default()
                .push(id);
            id
        };
        let a = mk(&mut index, "quest_a", 0xA);
        let b = mk(&mut index, "quest_b", 0xB);
        (index, a, b)
    }

    #[test]
    fn import_scope_binds_each_importer_to_its_own_module() {
        let (index, a, b) = two_module_ambush_index();

        let scope_a = ImportScope {
            file_module: None,
            qualified_modules: ["quest_a".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope_a, "ambush", &[SymbolKind::Knot]),
            Some(a),
            "a file importing quest_a binds quest_a's ambush"
        );

        let scope_b = ImportScope {
            file_module: None,
            qualified_modules: ["quest_b".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope_b, "ambush", &[SymbolKind::Knot]),
            Some(b),
            "a file importing quest_b binds quest_b's ambush — not the flat first-winner"
        );
    }

    #[test]
    fn same_module_candidate_wins_over_imported_one() {
        let (index, a, b) = two_module_ambush_index();
        // A file *inside* quest_b that also imports quest_a: its own module's
        // `ambush` is bare-visible and wins over the imported homonym.
        let scope = ImportScope {
            file_module: Some("quest_b".to_string()),
            qualified_modules: ["quest_a".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(b),
            "own-module definition beats an imported homonym"
        );
        let _ = a;
    }

    #[test]
    fn default_scope_falls_back_to_flat_first_winner() {
        // Byte-identity guard: with no import context (the pre-M-2d world),
        // a multi-candidate lookup returns the flat first-inserted winner,
        // exactly as the old flat resolver did.
        let (index, a, _b) = two_module_ambush_index();
        assert_eq!(
            lookup_by_name(
                &index,
                &ImportScope::default(),
                "ambush",
                &[SymbolKind::Knot]
            ),
            Some(a),
            "no imports → flat first-winner, unchanged from pre-M-2d"
        );
    }

    /// `ImportScope` granularity regression (issue #790 review): a bare
    /// import must be name-precise, matching `modules::import_covers`
    /// exactly. `ImportScope::new` used to collapse every import to just its
    /// module name (`imports.iter().map(|i| i.module.clone())`), so a bare
    /// `IMPORT { other } FROM quest_a` wrongly counted as importing *all* of
    /// `quest_a` — including its unrelated public `ambush`.
    #[test]
    fn bare_import_grants_candidacy_only_for_its_own_named_item() {
        let (index, _a, b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[
                Import {
                    module: "quest_a".to_string(),
                    module_range: TextRange::default(),
                    items: vec![ImportItem {
                        name: "other".to_string(),
                        alias: None,
                        range: TextRange::default(),
                    }],
                    bare: true,
                    range: TextRange::default(),
                },
                Import {
                    module: "quest_b".to_string(),
                    module_range: TextRange::default(),
                    items: vec![ImportItem {
                        name: "ambush".to_string(),
                        alias: None,
                        range: TextRange::default(),
                    }],
                    bare: true,
                    range: TextRange::default(),
                },
            ],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(b),
            "bare-importing `other` from quest_a must not license quest_a's `ambush` — \
             only quest_b's `ambush` (actually bare-imported) is a candidate"
        );
    }

    /// A qualified `IMPORT mod` still licenses every public export of that
    /// module (unlike a bare import, which is name-precise) — the
    /// granularity fix must not regress this path.
    #[test]
    fn qualified_import_still_grants_candidacy_for_any_export() {
        let (index, a, _b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "quest_a".to_string(),
                module_range: TextRange::default(),
                items: Vec::new(),
                bare: false,
                range: TextRange::default(),
            }],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(a),
            "a qualified `IMPORT quest_a` still licenses quest_a's `ambush`"
        );
    }
}
