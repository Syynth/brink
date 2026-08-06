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
    merge_manifests_with_modules(files, &ModuleMap::new(), crate::Dialect::default(), false)
}

/// Merge per-file symbol manifests, qualifying `DefinitionId`s by each
/// file's **declared** module (M-1, docs/modules-spec.md §5).
///
/// A file whose [`ResolvedModule::declared`] is `true` hashes its symbol
/// names as `(module, name)`; every other file (undeclared stem-modules,
/// or files absent from `modules`) hashes by bare name, byte-identically
/// to [`merge_manifests`]. This is the byte-identity guarantee the whole
/// pre-modules corpus relies on.
///
/// `dialect` gates M-2d cross-module coexistence (issue #790, decision-log
/// "Cross-module name collisions" 2026-07-14, endgame clause): under
/// `Dialect::Brink`, a same-name/same-kind pair whose two owning files
/// declared *different* modules is no longer a duplicate — both are inserted
/// and coexist, and import-scoped resolution ([`crate::ImportScope`]) binds
/// each reference to the module its file imported. This relaxes the M-2c/#793
/// `E096` stopgap. A duplicate within one declared module, or involving any
/// undeclared/legacy file, still warns (`E022`/`E023`/`E026`) and drops the
/// later definition. `merge_manifests`'s empty `ModuleMap` can never produce
/// a declared-module pair, so its `Dialect::default()` is inert.
///
/// `is_native` (issue #1562 review finding) is the same widening
/// [`is_cross_declared_module_collision`]'s own doc describes: `true` lets
/// M-2d coexistence apply regardless of `dialect`, for callers with a real
/// `Language` classification of the project (`brink-db`'s `symbol_index_query`,
/// via `project_is_native`). `merge_manifests`'s `false` is byte-identical to
/// before this parameter existed.
pub fn merge_manifests_with_modules(
    files: &[(FileId, &SymbolManifest)],
    modules: &ModuleMap,
    dialect: crate::Dialect,
    is_native: bool,
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
        insert_file_symbols(
            &mut index,
            &mut diagnostics,
            file_id,
            module,
            manifest,
            dialect,
            is_native,
        );
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
    dialect: crate::Dialect,
    is_native: bool,
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
            insert_symbol(
                index,
                diagnostics,
                file_id,
                module,
                sym,
                kind,
                dup_code,
                dialect,
                is_native,
            );
        }
    }
    for local in &manifest.locals {
        insert_local(index, file_id, local);
    }

    push_transitive_aliases(index, module, manifest);
}

/// M-3 transitive alias entries (issue #1671, docs/modules-spec.md §5): a
/// `#@was` on a knot or stitch re-keys **every** stitch/label beneath it,
/// because their qualified names embed the container's name — but
/// [`insert_symbol`] mints only the container's *own* alias entry, so a
/// declared rename still lost every descendant's durable state. The loader
/// cannot recover this at load time (a `DefinitionId` is a hash; no path can
/// be derived from one), so it must be materialized here, where the
/// compiler still knows every descendant's qualified path.
///
/// Mirrors [`insert_symbol`]'s own-alias construction, additive and
/// independent per level — a knot renamed *simultaneously* with one of its
/// own stitches produces one edge per level (`old_knot.new_stitch...` and
/// `new_knot.old_stitch...`), not the doubly-old `old_knot.old_stitch...`
/// form, the same documented limitation [`insert_symbol`]'s module+name
/// case already carries. Table growth is bounded by the renamed container's
/// subtree size (docs/format-spec.md's alias-table section).
fn push_transitive_aliases(
    index: &mut SymbolIndex,
    module: ModuleCtx<'_>,
    manifest: &SymbolManifest,
) {
    // A knot rename re-keys every stitch and label qualified under its
    // (bare) name.
    for knot in &manifest.knots {
        let Some((old_name, _range)) = &knot.was else {
            continue;
        };
        if old_name == &knot.name {
            continue; // already diagnosed E095 at lowering; no bridge to mint
        }
        let new_prefix = format!("{}.", knot.name);
        let old_prefix = format!("{old_name}.");
        for (descendants, kind) in [
            (&manifest.stitches, SymbolKind::Stitch),
            (&manifest.labels, SymbolKind::Label),
        ] {
            push_prefix_bridged_aliases(index, module, descendants, kind, &new_prefix, &old_prefix);
        }
    }

    // A stitch rename re-keys every label qualified under it. `stitch.was`
    // is already fully qualified as `{knot}.{old_stitch}` (`lower_stitch`
    // qualifies it with the *current* knot name before storing — see
    // `Stitch::was`'s doc), matching `stitch.name`'s `{knot}.{new_stitch}`
    // shape, so only labels (never stitches — stitches don't nest) need a
    // bridge here.
    for stitch in &manifest.stitches {
        let Some((old_qualified, _range)) = &stitch.was else {
            continue;
        };
        if old_qualified == &stitch.name {
            continue;
        }
        let new_prefix = format!("{}.", stitch.name);
        let old_prefix = format!("{old_qualified}.");
        push_prefix_bridged_aliases(
            index,
            module,
            &manifest.labels,
            SymbolKind::Label,
            &new_prefix,
            &old_prefix,
        );
    }
}

/// For every `descendant` whose qualified name starts with `new_prefix`,
/// mint an [`brink_format::AliasEntry`] bridging its pre-rename identity
/// (`old_prefix` substituted for `new_prefix`) to its real, already-current
/// id. Shared by both [`push_transitive_aliases`] levels (knot→{stitch,
/// label}, stitch→label) — same substitution, different prefix pair.
fn push_prefix_bridged_aliases(
    index: &mut SymbolIndex,
    module: ModuleCtx<'_>,
    descendants: &[brink_ir::DeclaredSymbol],
    kind: SymbolKind,
    new_prefix: &str,
    old_prefix: &str,
) {
    let tag = kind.definition_tag();
    for sym in descendants {
        let Some(rest) = sym.name.strip_prefix(new_prefix) else {
            continue;
        };
        let old_qualified = format!("{old_prefix}{rest}");
        let old_hash = hash_qualified_name(module.name, &old_qualified, tag);
        let new_hash = hash_qualified_name(module.name, &sym.name, tag);
        index.aliases.push(brink_format::AliasEntry {
            old: DefinitionId::new(tag, old_hash),
            new: DefinitionId::new(tag, new_hash),
        });
    }
}

/// M-2d import-scoped coexistence (issue #790, decision-log "Cross-module
/// name collisions" 2026-07-14, endgame clause): two same-name/same-kind
/// definitions in *different* **declared** modules are no longer a collision
/// at all — they coexist in the index and import-scoped resolution
/// (`resolve.rs`, [`crate::ImportScope`]) binds each reference to the module
/// its file imported. This *relaxes* the M-2c/#793 `E096` stopgap, which had
/// hard-errored exactly this case as a temporary guard against silent
/// misbinding while the flat resolver was still authoritative.
///
/// Returns `true` when `existing`/`new_module` are such a cross-declared-
/// module pair (so the caller must let both coexist — no diagnostic, no
/// skip). A duplicate *within* one declared module, or involving any
/// undeclared/legacy file, returns `false` — the ordinary inklecate-compat
/// redefinition warning (`E022`/`E023`/`E026`) still applies and the later
/// definition is still dropped. The `Dialect::Brink` gate preserves
/// `strict-ink` byte-for-byte: `#@module` is brink-only syntax, so a
/// strict-ink story never has a declared module, and this always returns
/// `false` there — the whole compat corpus keeps its existing behavior.
///
/// `is_native` (issue #1562 review finding) widens the gate exactly as
/// [`crate::strict_diagnostics`]'s own `is_native` widens `E064`'s dialect
/// check: `dialect` is an ink-only axis — a native `.brink` project has no
/// dialect to be wrong about (every `.brink` module is always *declared*,
/// see `brink_db::modules::native_module_path`), so M-2d coexistence must
/// not depend on a client having declared `dialect: "brink"` in
/// `initializationOptions`. `true` lets cross-declared-module homonyms
/// coexist regardless of `dialect`'s value; `false` (every pure-API caller
/// with no `Language` classification of its own) is byte-identical to
/// before this parameter existed.
fn is_cross_declared_module_collision(
    dialect: crate::Dialect,
    is_native: bool,
    existing_module: Option<&str>,
    new_module: Option<&str>,
) -> bool {
    if dialect != crate::Dialect::Brink && !is_native {
        return false;
    }
    matches!(
        (existing_module, new_module),
        (Some(a), Some(b)) if a != b
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "internal helper threading file/module context + the per-kind duplicate code + dialect through one insertion; splitting further would just re-bundle these into an ad-hoc struct for no clarity gain"
)]
fn insert_symbol(
    index: &mut SymbolIndex,
    diagnostics: &mut Vec<Diagnostic>,
    file: FileId,
    module: ModuleCtx<'_>,
    sym: &brink_ir::DeclaredSymbol,
    kind: SymbolKind,
    dup_code: DiagnosticCode,
    dialect: crate::Dialect,
    is_native: bool,
) {
    // Duplicate handling. A same-name/same-kind definition already in the
    // index is normally an inklecate-compat redefinition: we warn and drop
    // the later one. The M-2d exception (issue #790): if *every* existing
    // same-kind definition sits in a **different declared module** from this
    // one, they are not duplicates at all — they coexist and import-scoped
    // resolution disambiguates them per referring file. Only a *true*
    // duplicate (same declared module, or the undeclared/legacy world, or
    // strict-ink where declared modules cannot exist) still warns-and-skips.
    if let Some(existing_ids) = index.by_name.get(&sym.name) {
        let true_duplicate = existing_ids
            .iter()
            .filter_map(|id| index.symbols.get(id))
            .filter(|info| info.kind == kind)
            .find(|existing| {
                !is_cross_declared_module_collision(
                    dialect,
                    is_native,
                    existing.module.as_deref(),
                    module.name,
                )
            });
        if let Some(_existing) = true_duplicate {
            diagnostics.push(Diagnostic {
                file,
                range: sym.range,
                message: format!("{}: `{}`", dup_code.title(), sym.name),
                code: dup_code,
            });
            return;
        }
        // Otherwise: only cross-declared-module homonyms exist — fall through
        // and insert this definition alongside them.
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
        || crate::resolve::is_t1b_stdlib_name(&sym.name)
        // NS-A1: `none` is the Option absence literal (variable-position,
        // not a call name, so it lives outside `is_t1b_stdlib_name`) —
        // shadowing it warns exactly like the call-form builtins.
        || sym.name == "none")
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
    use brink_format::{DefinitionId, DefinitionTag};
    use brink_ir::{DeclaredSymbol, DiagnosticCode, FileId, SymbolManifest};
    use rowan::{TextRange, TextSize};

    use super::{
        ModuleMap, ResolvedModule, hash_qualified_name, merge_manifests,
        merge_manifests_with_modules,
    };

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

    /// A declared symbol carrying a `#@was(old_name)` record — `old_name` is
    /// exactly what `DeclaredSymbol::was` stores (bare for a knot, already
    /// knot-qualified for a stitch — see `Stitch::was`'s doc).
    fn sym_with_was(name: &str, offset: u32, old_name: &str) -> DeclaredSymbol {
        DeclaredSymbol {
            was: Some((old_name.to_string(), range(offset, old_name.len() as u32))),
            ..sym(name, offset)
        }
    }

    fn bridge(old_name: &str, new_name: &str) -> (DefinitionId, DefinitionId) {
        let tag = DefinitionTag::Address;
        (
            DefinitionId::new(tag, hash_qualified_name(None, old_name, tag)),
            DefinitionId::new(tag, hash_qualified_name(None, new_name, tag)),
        )
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

    // ── M-2d cross-module coexistence (issue #790, decision-log
    // "Cross-module name collisions" 2026-07-14, endgame clause —
    // relaxes the #784/#793 E096 stopgap) ─────────────────────────────

    /// Two *declared* modules (different names) each defining `start` now
    /// **coexist** under `Dialect::Brink`: no diagnostic, and *both*
    /// definitions land in the index under the shared bare name so
    /// import-scoped resolution can bind each referring file to the module
    /// it imported. This is the E096 relaxation — the case #793 hard-errored
    /// now compiles.
    #[test]
    fn cross_declared_module_duplicate_coexists_under_brink() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));
        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let mut modules = ModuleMap::new();
        modules.insert(
            FileId(0),
            ResolvedModule {
                name: "quest".to_string(),
                declared: true,
                was: None,
            },
        );
        modules.insert(
            FileId(1),
            ResolvedModule {
                name: "town".to_string(),
                declared: true,
                was: None,
            },
        );

        let (index, diags) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::Brink, false);

        assert!(
            diags.is_empty(),
            "cross-declared-module homonyms coexist with no diagnostic (E096 relaxed), got {diags:?}"
        );

        // Both `start` definitions survive in the index under the shared bare
        // name — the raw material import-scoped resolution disambiguates.
        assert_eq!(index.by_name.get("start").map(Vec::len), Some(2));
        // …and they are genuinely distinct ids (module-qualified identity).
        let ids = index.by_name.get("start").expect("both starts present");
        assert_ne!(
            ids[0], ids[1],
            "the two modules' `start`s have distinct ids"
        );
        let modules_of: std::collections::BTreeSet<Option<&str>> = ids
            .iter()
            .filter_map(|id| index.symbols.get(id))
            .map(|info| info.module.as_deref())
            .collect();
        assert_eq!(
            modules_of,
            [Some("quest"), Some("town")].into_iter().collect(),
            "one `start` per declared module"
        );
    }

    /// Two files sharing the **same** declared module keep the ordinary
    /// within-module `E022` warning — never `E096` — even under
    /// `Dialect::Brink`.
    #[test]
    fn same_declared_module_duplicate_stays_e022_under_brink() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));
        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let mut modules = ModuleMap::new();
        for file in [FileId(0), FileId(1)] {
            modules.insert(
                file,
                ResolvedModule {
                    name: "quest".to_string(),
                    declared: true,
                    was: None,
                },
            );
        }

        let (_index, diags) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::Brink, false);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    /// A declared-module/undeclared-file collision (one side has no
    /// declared module) stays `E022` — the escalation requires *both*
    /// sides to be declared modules.
    #[test]
    fn declared_vs_undeclared_duplicate_stays_e022_under_brink() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));
        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let mut modules = ModuleMap::new();
        modules.insert(
            FileId(0),
            ResolvedModule {
                name: "quest".to_string(),
                declared: true,
                was: None,
            },
        );
        // FileId(1) absent from `modules` -> undeclared, hashes bare.

        let (_index, diags) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::Brink, false);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    /// Under `Dialect::StrictInk` (the default) with `is_native = false`, a
    /// cross-declared-module duplicate never escalates — `merge_manifests`'s
    /// inert default keeps the compat corpus untouched. See
    /// `cross_declared_module_duplicate_coexists_under_strict_ink_when_native`
    /// below for the native counterpart (issue #1562 review finding), where
    /// the same dialect instead coexists.
    #[test]
    fn cross_declared_module_duplicate_stays_e022_under_strict_ink() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));
        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let mut modules = ModuleMap::new();
        modules.insert(
            FileId(0),
            ResolvedModule {
                name: "quest".to_string(),
                declared: true,
                was: None,
            },
        );
        modules.insert(
            FileId(1),
            ResolvedModule {
                name: "town".to_string(),
                declared: true,
                was: None,
            },
        );

        let (_index, diags) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::StrictInk, false);

        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    /// Native counterpart of `cross_declared_module_duplicate_stays_e022_under_strict_ink`
    /// above (issue #1562 review finding): under the exact same
    /// `Dialect::StrictInk` (the default a client that never declares
    /// `initializationOptions.dialect` gets), `is_native = true` still lets
    /// the two declared-module `start`s coexist — a native `.brink` project
    /// has no dialect to be wrong about, so M-2d coexistence must not depend
    /// on a client having declared `dialect: "brink"`.
    #[test]
    fn cross_declared_module_duplicate_coexists_under_strict_ink_when_native() {
        let mut m1 = SymbolManifest::default();
        m1.knots.push(sym("start", 0));
        let mut m2 = SymbolManifest::default();
        m2.knots.push(sym("start", 100));

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let mut modules = ModuleMap::new();
        modules.insert(
            FileId(0),
            ResolvedModule {
                name: "quest".to_string(),
                declared: true,
                was: None,
            },
        );
        modules.insert(
            FileId(1),
            ResolvedModule {
                name: "town".to_string(),
                declared: true,
                was: None,
            },
        );

        let (index, diags) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::StrictInk, true);

        assert!(
            diags.is_empty(),
            "native cross-declared-module homonyms coexist regardless of dialect, got {diags:?}"
        );
        assert_eq!(index.by_name.get("start").map(Vec::len), Some(2));
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
        let (with_undeclared, _) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::default(), false);

        // (3) A file absent from the map must also stay bare.
        let (with_empty, _) = merge_manifests_with_modules(
            &files,
            &ModuleMap::new(),
            crate::Dialect::default(),
            false,
        );

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
        let (qualified, _) =
            merge_manifests_with_modules(&files, &modules, crate::Dialect::default(), false);

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

    // ── M-3 transitive aliases (issue #1671, docs/modules-spec.md §5)
    // ─────────────────────────────────────────────────────────────────

    /// `#@was` on a knot mints the knot's own alias entry *and* one per
    /// descendant re-keyed only because the knot's name changed — a stitch
    /// and a label nested directly under the knot, in this case.
    #[test]
    fn knot_rename_transitively_aliases_stitch_and_label() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym_with_was("plaza", 0, "hub"));
        manifest.stitches.push(sym("plaza.market", 10));
        manifest.labels.push(sym("plaza.done", 30));

        let files = vec![(FileId(0), &manifest)];
        let (index, _diags) = merge_manifests(&files);

        let expected = [
            bridge("hub", "plaza"),
            bridge("hub.market", "plaza.market"),
            bridge("hub.done", "plaza.done"),
        ];
        for (old, new) in expected {
            assert!(
                index.aliases.iter().any(|a| a.old == old && a.new == new),
                "missing bridge {old:?} -> {new:?}; aliases={:?}",
                index.aliases
            );
        }
        assert_eq!(
            index.aliases.len(),
            3,
            "one entry for the knot plus one per descendant; aliases={:?}",
            index.aliases
        );
    }

    /// `#@was` on a stitch (unrelated to any knot rename) mints the stitch's
    /// own entry plus one for a label nested under it — `Stitch::was` is
    /// already qualified with the enclosing (current) knot name, so the
    /// bridge lands on `{knot}.{old_stitch}.{label}` ->
    /// `{knot}.{new_stitch}.{label}`.
    #[test]
    fn stitch_rename_transitively_aliases_its_label() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("hub", 0));
        manifest
            .stitches
            .push(sym_with_was("hub.bazaar", 10, "hub.market"));
        manifest.labels.push(sym("hub.bazaar.done", 30));

        let files = vec![(FileId(0), &manifest)];
        let (index, _diags) = merge_manifests(&files);

        let expected = [
            bridge("hub.market", "hub.bazaar"),
            bridge("hub.market.done", "hub.bazaar.done"),
        ];
        for (old, new) in expected {
            assert!(
                index.aliases.iter().any(|a| a.old == old && a.new == new),
                "missing bridge {old:?} -> {new:?}; aliases={:?}",
                index.aliases
            );
        }
        assert_eq!(index.aliases.len(), 2, "aliases={:?}", index.aliases);
    }

    /// No `#@was` anywhere — no aliases minted at all, transitive or
    /// otherwise. (Guards against `push_transitive_aliases` firing on an
    /// empty-prefix false positive.)
    #[test]
    fn no_rename_no_aliases() {
        let mut manifest = SymbolManifest::default();
        manifest.knots.push(sym("hub", 0));
        manifest.stitches.push(sym("hub.market", 10));
        manifest.labels.push(sym("hub.market.done", 30));

        let files = vec![(FileId(0), &manifest)];
        let (index, _diags) = merge_manifests(&files);

        assert!(index.aliases.is_empty(), "aliases={:?}", index.aliases);
    }
}
