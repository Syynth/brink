use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag};
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, LocalSymbol, Scope, SymbolIndex, SymbolInfo, SymbolKind,
    SymbolManifest, Visibility, VisibilityMark,
};

/// Apply declaration-flips-default (modules-spec §4) to an explicit
/// `#@private`/`#@public` override, returning the effective [`Visibility`]
/// and whether the override was *redundant* (restated the module default).
///
/// A declared module (`declared == true`) defaults private; an undeclared
/// stem-module defaults public.
fn effective_visibility(declared: bool, mark: Option<VisibilityMark>) -> (Visibility, bool) {
    let default = if declared {
        Visibility::Private
    } else {
        Visibility::Public
    };
    match mark {
        None => (default, false),
        Some(VisibilityMark::Private) => (Visibility::Private, default == Visibility::Private),
        Some(VisibilityMark::Public) => (Visibility::Public, default == Visibility::Public),
    }
}

/// A file's resolved module (M-1, docs/modules-spec.md §1/§5).
///
/// Computed upstream (in `brink-db`, where file stems, `#@module`
/// declarations, and the INCLUDE graph are all known) and threaded into
/// the symbol index so `DefinitionId` hashing can qualify names by
/// module. Only a **declared** module (`#@module(name)` present) qualifies
/// identity; an undeclared stem-module contributes nothing to the hash, so
/// the entire pre-modules corpus keeps byte-identical `DefinitionId`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModule {
    /// The module's name (stem for undeclared, `#@module` argument for
    /// declared).
    pub name: String,
    /// `true` when the file (or its INCLUDE head) carries `#@module`.
    pub declared: bool,
    /// The module's old name, from a `#@was(old_name)` on **any** file
    /// sharing this resolved module (M-3, docs/modules-spec.md §5) —
    /// aggregated by `brink-db::modules::resolve_modules` so every file in
    /// a multi-file module sees the same rename regardless of which file
    /// declared it. `None` for a module with no recorded rename, and
    /// always `None` for an undeclared stem-module (scoped to declared
    /// modules only — see `ModuleDecl::was`'s doc).
    pub was: Option<String>,
}

/// Map from file to its resolved module. Absent entries (and undeclared
/// modules) hash by bare name — byte-identical to the pre-modules scheme.
pub type ModuleMap = BTreeMap<FileId, ResolvedModule>;

/// Merge per-file symbol manifests into a unified symbol index.
///
/// Returns the index and any diagnostics (e.g. duplicate definitions).
/// Names hash by bare name (no module qualification) — the pre-modules
/// behavior. Use [`merge_manifests_with_modules`] to qualify identity by
/// declared module.
pub fn merge_manifests(files: &[(FileId, &SymbolManifest)]) -> (SymbolIndex, Vec<Diagnostic>) {
    merge_manifests_with_modules(files, &ModuleMap::new())
}

/// Merge per-file symbol manifests, qualifying `DefinitionId`s by each
/// file's **declared** module (M-1, docs/modules-spec.md §5).
///
/// A file whose [`ResolvedModule::declared`] is `true` hashes its symbol
/// names as `(module, name)`; every other file (undeclared stem-modules,
/// or files absent from `modules`) hashes by bare name, byte-identically
/// to [`merge_manifests`]. This is the byte-identity guarantee the whole
/// pre-modules corpus relies on.
pub fn merge_manifests_with_modules(
    files: &[(FileId, &SymbolManifest)],
    modules: &ModuleMap,
) -> (SymbolIndex, Vec<Diagnostic>) {
    let mut index = SymbolIndex::default();
    let mut diagnostics = Vec::new();

    for &(file_id, manifest) in files {
        // Only a *declared* module qualifies identity; undeclared
        // stem-modules (and files absent from `modules`) hash bare.
        let resolved = modules.get(&file_id).filter(|m| m.declared);
        let module = ModuleCtx {
            name: resolved.map(|m| m.name.as_str()),
            // M-3 (docs/modules-spec.md §5): the module's old name, if any
            // file sharing it declared `#@was` —
            // `brink-db::modules::resolve_modules` aggregates this onto
            // every file's `ResolvedModule` already, so a single per-file
            // lookup here sees it regardless of which file in a multi-file
            // module carried the directive.
            was: resolved.and_then(|m| m.was.as_deref()),
        };
        insert_file_symbols(&mut index, &mut diagnostics, file_id, module, manifest);
    }

    (index, diagnostics)
}

/// A file's resolved module, bundled for the `insert_*` helpers below
/// (keeps their argument count under clippy's limit — `module` and
/// `module_was` are always passed together).
#[derive(Clone, Copy)]
struct ModuleCtx<'a> {
    /// The **declared** module's current name; `None` for an undeclared
    /// stem-module (identity hashes bare).
    name: Option<&'a str>,
    /// The module's old name from a `#@was` (M-3, docs/modules-spec.md
    /// §5), if any file sharing this module recorded one.
    was: Option<&'a str>,
}

/// Insert every symbol declared in one file's manifest, qualifying named
/// declarations by `module` (M-1). Locals stay unqualified (`LocalVar`,
/// container-scoped, never serialized).
fn insert_file_symbols(
    index: &mut SymbolIndex,
    diagnostics: &mut Vec<Diagnostic>,
    file_id: FileId,
    module: ModuleCtx<'_>,
    manifest: &SymbolManifest,
) {
    use DiagnosticCode::{E022, E023, E026};
    use SymbolKind::{Constant, External, Knot, Label, List, ListItem, Stitch, Struct, Variable};

    let groups: [(&[brink_ir::DeclaredSymbol], SymbolKind, DiagnosticCode); 9] = [
        (&manifest.knots, Knot, E022),
        (&manifest.stitches, Stitch, E022),
        (&manifest.variables, Variable, E023),
        (&manifest.constants, Constant, E023),
        (&manifest.lists, List, E023),
        (&manifest.structs, Struct, E023),
        (&manifest.externals, External, E023),
        (&manifest.labels, Label, E022),
        (&manifest.list_items, ListItem, E026),
    ];
    for (syms, kind, dup_code) in groups {
        for sym in syms {
            insert_symbol(index, diagnostics, file_id, module, sym, kind, dup_code);
        }
    }
    for local in &manifest.locals {
        insert_local(index, file_id, local);
    }
}

fn insert_symbol(
    index: &mut SymbolIndex,
    diagnostics: &mut Vec<Diagnostic>,
    file: FileId,
    module: ModuleCtx<'_>,
    sym: &brink_ir::DeclaredSymbol,
    kind: SymbolKind,
    dup_code: DiagnosticCode,
) {
    // Skip duplicates of the same kind — inklecate permits redefinition but we warn.
    if let Some(existing_ids) = index.by_name.get(&sym.name) {
        let has_dup = existing_ids
            .iter()
            .any(|id| index.symbols.get(id).is_some_and(|info| info.kind == kind));
        if has_dup {
            diagnostics.push(Diagnostic {
                file,
                range: sym.range,
                message: format!("{}: `{}`", dup_code.title(), sym.name),
                code: dup_code,
            });
            return;
        }
    }

    let tag = kind.definition_tag();
    let hash = hash_qualified_name(module.name, &sym.name, tag);
    let id = DefinitionId::new(tag, hash);

    // Effective visibility (declaration-flips-default, modules-spec §4): a
    // *declared* module (`module.name.is_some()`) defaults private; an
    // undeclared stem-module defaults public. A `#@private`/`#@public`
    // override flips that; restating the default is a redundant-override
    // warning (`E092`).
    let declared = module.name.is_some();
    let (visibility, redundant) = effective_visibility(declared, sym.visibility);
    if redundant {
        diagnostics.push(Diagnostic {
            file,
            range: sym.range,
            message: format!("{}: `{}`", DiagnosticCode::E092.title(), sym.name),
            code: DiagnosticCode::E092,
        });
    }

    index.symbols.insert(
        id,
        SymbolInfo {
            kind,
            file,
            range: sym.range,
            id,
            name: sym.name.clone(),
            params: sym.params.clone(),
            detail: sym.detail.clone(),
            scope: None,
            param_detail: None,
            module: module.name.map(str::to_string),
            visibility,
        },
    );
    index.by_name.entry(sym.name.clone()).or_default().push(id);

    // M-3 (docs/modules-spec.md §5): compiled alias-table entries. Additive
    // and independent — a definition-level `#@was` and a module-level
    // `#@was` on the same symbol both produce an entry (each aliasing a
    // *different* stale identity to the same current `id`); the rarer case
    // of both renamed **simultaneously** (old module + old name together)
    // is a known gap, not covered by either entry alone.
    if let Some((old_name, _range)) = &sym.was {
        let old_hash = hash_qualified_name(module.name, old_name, tag);
        index.aliases.push(brink_format::AliasEntry {
            old: DefinitionId::new(tag, old_hash),
            new: id,
        });
    }
    if let Some(old_module) = module.was {
        let old_hash = hash_qualified_name(Some(old_module), &sym.name, tag);
        index.aliases.push(brink_format::AliasEntry {
            old: DefinitionId::new(tag, old_hash),
            new: id,
        });
    }

    // Warn if the symbol name shadows a built-in function — the classic
    // uppercase ink intrinsics (`is_builtin_function`) or a T1b stdlib
    // slice 1 / TM-3-completion lowercase free function
    // (`is_t1b_stdlib_name`, docs/t1b-surface-spec.md §5 +
    // docs/typed-mode-spec.md §4 — "an author-defined function with the
    // same name shadows the builtin, with a warning diagnostic"). Fired
    // dialect-agnostically here, same as the existing uppercase check: under
    // `strict-ink` the T1b names aren't reserved at all, so the warning is
    // merely informational (harmless, matches existing precedent for the
    // uppercase set, which also doesn't consult dialect).
    if matches!(
        kind,
        SymbolKind::Knot | SymbolKind::Variable | SymbolKind::Constant | SymbolKind::External
    ) && (crate::resolve::is_builtin_function(&sym.name)
        || crate::resolve::is_t1b_stdlib_name(&sym.name))
    {
        diagnostics.push(Diagnostic {
            file,
            range: sym.range,
            message: format!("{}: `{}`", DiagnosticCode::E035.title(), sym.name),
            code: DiagnosticCode::E035,
        });
    }
}

fn insert_local(index: &mut SymbolIndex, file: FileId, local: &LocalSymbol) {
    let id = local_definition_id(&local.scope, &local.name, local.kind);

    index.symbols.insert(
        id,
        SymbolInfo {
            kind: local.kind,
            file,
            range: local.range,
            id,
            name: local.name.clone(),
            params: Vec::new(),
            detail: None,
            scope: Some(local.scope.clone()),
            param_detail: local.param_detail.clone(),
            // Locals are never module-qualified and always module-internal.
            module: None,
            visibility: Visibility::Private,
        },
    );
    index
        .by_name
        .entry(local.name.clone())
        .or_default()
        .push(id);
}

/// Hash a bare (unqualified) symbol name. Equivalent to
/// `hash_qualified_name(None, name, tag)` — retained for the local-var
/// path, whose scope-qualified names are never module-qualified in M-1.
fn hash_name(name: &str, tag: DefinitionTag) -> u64 {
    hash_qualified_name(None, name, tag)
}

/// Hash a symbol name, optionally qualified by its **declared** module
/// (M-1, docs/modules-spec.md §5).
///
/// The identity gate: `hash_qualified_name(None, name, tag)` writes the
/// hasher in the exact order the pre-modules `hash_name` did — `tag` then
/// `name` — so every undeclared stem-module symbol hashes to a
/// byte-identical `DefinitionId`. A declared module folds its name in
/// *before* the symbol name; `str`'s `Hash` impl self-delimits (a `0xff`
/// sentinel after the bytes), so `(module="ab", name="c")` can never
/// collide with `(module="a", name="bc")`.
fn hash_qualified_name(module: Option<&str>, name: &str, tag: DefinitionTag) -> u64 {
    let mut hasher = DefaultHasher::new();
    tag.hash(&mut hasher);
    if let Some(module) = module {
        module.hash(&mut hasher);
    }
    name.hash(&mut hasher);
    hasher.finish()
}

/// Compute the `DefinitionId` for a scoped local (param/temp).
///
/// Scope-qualifies the hash so identically-named locals in different
/// containers get distinct ids: `knot.stitch.name`, `knot.name`, or bare
/// `name` for an unscoped local. Shared between [`insert_local`] (project-wide
/// merge, still used by `symbol_index`/hover/completion) and the per-file
/// local lookup in `resolve.rs` (issue #517) so both derive the identical id
/// for the same declaration.
///
/// Not file-qualified: two files that (pathologically) declare the same
/// scope-qualified local name still collide on one `DefinitionId` in the
/// *merged* index (slice-A finding 4, unchanged by this — a merged-index
/// consumer, e.g. hover, can only show one). Per-file resolution
/// (`resolve_file`) no longer goes through the merged index for locals, so
/// the collision cannot leak into resolution correctness.
pub(crate) fn local_definition_id(scope: &Scope, name: &str, kind: SymbolKind) -> DefinitionId {
    let tag = kind.definition_tag();
    let scope_prefix = match (&scope.knot, &scope.stitch) {
        (Some(k), Some(s)) => format!("{k}.{s}."),
        (Some(k), None) => format!("{k}."),
        _ => String::new(),
    };
    let qualified = format!("{scope_prefix}{name}");
    let hash = hash_name(&qualified, tag);
    DefinitionId::new(tag, hash)
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test helper ranges")]
mod tests {
    use brink_ir::{DeclaredSymbol, DiagnosticCode, FileId, SymbolManifest};
    use rowan::{TextRange, TextSize};

    use super::{ModuleMap, ResolvedModule, merge_manifests, merge_manifests_with_modules};

    fn range(offset: u32, len: u32) -> TextRange {
        TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
    }

    fn sym(name: &str, offset: u32) -> DeclaredSymbol {
        DeclaredSymbol {
            name: name.to_string(),
            range: range(offset, name.len() as u32),
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        }
    }

    #[test]
    fn duplicate_knot_emits_e022() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));

        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    #[test]
    fn duplicate_variable_emits_e023() {
        let mut m1 = SymbolManifest::default();
        m1.variables.push(sym("score", 0));

        let mut m2 = SymbolManifest::default();
        m2.variables.push(sym("score", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E023);
    }

    #[test]
    fn different_kind_same_name_no_warning() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("thing", 0));
        manifest.variables.push(sym("thing", 100));

        let files = vec![(FileId(0), &manifest)];
        let (_index, diags) = merge_manifests(&files);

        // A knot and a variable with the same name are different kinds — no duplicate.
        assert!(diags.is_empty(), "expected no diagnostics: {diags:?}");
    }

    #[test]
    fn builtin_name_shadow_emits_e035() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("RANDOM", 0));

        let files = vec![(FileId(0), &manifest)];
        let (_index, diags) = merge_manifests(&files);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E035);
    }

    #[test]
    fn non_builtin_name_no_shadow_warning() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("my_function", 0));

        let files = vec![(FileId(0), &manifest)];
        let (_index, diags) = merge_manifests(&files);

        assert!(diags.is_empty(), "expected no diagnostics: {diags:?}");
    }

    // ── M-1 identity gate (docs/modules-spec.md §5) ──────────────────

    fn sample_manifest() -> SymbolManifest {
        let mut m = SymbolManifest::default();
        m.knots.push(sym("start", 0));
        m.stitches.push(sym("start.middle", 20));
        m.variables.push(sym("score", 60));
        m.lists.push(sym("colors", 90));
        m.list_items.push(sym("colors.red", 110));
        m
    }

    /// THE critical gate: an undeclared stem-module (the entire pre-modules
    /// corpus) must hash to byte-identical `DefinitionId`s. Proven two ways:
    /// (1) qualifying with an *undeclared* module is equivalent to no
    /// qualification, and (2) the merge is bit-for-bit equal to the
    /// pre-modules `merge_manifests`.
    #[test]
    fn undeclared_module_definition_ids_are_byte_identical() {
        let manifest = sample_manifest();
        let files = vec![(FileId(0), &manifest)];

        // (1) Baseline — the pre-modules derivation (no module map).
        let (baseline, _) = merge_manifests(&files);

        // (2) An *undeclared* stem-module entry must not qualify.
        let mut modules = ModuleMap::new();
        modules.insert(
            FileId(0),
            ResolvedModule {
                name: "story".to_string(),
                declared: false,
                was: None,
            },
        );
        let (with_undeclared, _) = merge_manifests_with_modules(&files, &modules);

        // (3) A file absent from the map must also stay bare.
        let (with_empty, _) = merge_manifests_with_modules(&files, &ModuleMap::new());

        let mut base_ids: Vec<_> = baseline.symbols.keys().map(|id| id.to_raw()).collect();
        base_ids.sort_unstable();
        for other in [&with_undeclared, &with_empty] {
            let mut ids: Vec<_> = other.symbols.keys().map(|id| id.to_raw()).collect();
            ids.sort_unstable();
            assert_eq!(
                ids, base_ids,
                "undeclared / absent module must produce byte-identical DefinitionIds"
            );
        }
    }

    /// Known-good pinned ids: hardcoded raw `DefinitionId`s for the sample
    /// symbols under the bare (pre-modules) scheme. If any hashing input or
    /// order ever changes, these literals break — the tripwire that would
    /// have caught a silent regeneration of every checked-in `.inkb`.
    #[test]
    fn known_good_bare_definition_ids() {
        let manifest = sample_manifest();
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);

        let id_of = |name: &str| -> u64 {
            index
                .by_name
                .get(name)
                .and_then(|ids| ids.first())
                .map(|id| id.to_raw())
                .expect("symbol present")
        };

        // These are the raw ids produced by `hash(tag, name)` — the exact
        // derivation checked-in artifacts and saved games depend on. If any
        // hashing input or write order regresses, these literals break.
        assert_eq!(id_of("start"), 0x01e2_25d7_2013_19eb);
        assert_eq!(id_of("start.middle"), 0x015d_6dc8_e16e_aef6);
        assert_eq!(id_of("score"), 0x0293_4ea0_c935_6d8d);
        assert_eq!(id_of("colors"), 0x03ab_176e_5431_d3b4);
        assert_eq!(id_of("colors.red"), 0x04c5_e205_8cad_67b3);
    }

    /// A *declared* module qualifies identity — its ids differ from bare.
    #[test]
    fn declared_module_changes_definition_ids() {
        let manifest = sample_manifest();
        let files = vec![(FileId(0), &manifest)];
        let (bare, _) = merge_manifests(&files);

        let mut modules = ModuleMap::new();
        modules.insert(
            FileId(0),
            ResolvedModule {
                name: "quest".to_string(),
                declared: true,
                was: None,
            },
        );
        let (qualified, _) = merge_manifests_with_modules(&files, &modules);

        let bare_start = bare.by_name.get("start").and_then(|v| v.first()).copied();
        let q_start = qualified
            .by_name
            .get("start")
            .and_then(|v| v.first())
            .copied();
        assert!(bare_start.is_some() && q_start.is_some());
        assert_ne!(
            bare_start, q_start,
            "a declared module must qualify (change) the DefinitionId"
        );
        // Same tag byte though — only the hash portion moves.
        assert_eq!(
            bare_start.unwrap().tag(),
            q_start.unwrap().tag(),
            "qualification changes the hash, not the tag"
        );
    }
}
