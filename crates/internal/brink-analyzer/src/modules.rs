//! Import well-formedness + cross-module visibility enforcement (M-2,
//! docs/modules-spec.md §2/§4/§7).
//!
//! Runs in the whole-project pass, where every file's `IMPORT` list (HIR)
//! and the merged symbol index (each [`SymbolInfo`] now carrying its module
//! and effective visibility, §4) are both available. Four jobs:
//!
//! - **Import well-formedness**: self-import (`E090`), a name brought into
//!   scope twice (`E089`), and a bare `IMPORT { name } FROM mod` whose
//!   trailing segment names neither an item the (declared) module publicly
//!   exports nor a declared submodule of it (`E088`, dual-reading — see
//!   `known_module_names` and issue #1592).
//! - **Cross-module visibility**: a reference that resolves to a `#@private`
//!   definition outside the referrer's module is `E087`. Private is
//!   module-internal; the check keys off *visibility*, so it fires for a
//!   `#@private` def in an undeclared file referenced from another file just
//!   as for a declared module — but never for the pre-modules world, where
//!   every definition is `Public` (declaration-flips-default, §4).
//! - **Import-required resolution** (M-2c, §2 — "names cross module
//!   boundaries only via import"): a reference that resolves to a *public*
//!   definition in another **declared** module which the referring file did
//!   not `IMPORT` is `E025` (did-you-mean-`IMPORT` flavor). The restriction
//!   is keyed on the *target's* module being declared, so it never fires for
//!   the undeclared legacy soup — a plain multi-file `INCLUDE` project (no
//!   `#@module`) is one big default-public module and every cross-file bare
//!   reference keeps resolving byte-identically (§3). Only genuinely
//!   multi-*declared*-module projects are constrained.
//! - **Qualified ambiguity** (`E091`, §2): a `IMPORT mod` (qualified) whose
//!   module name also names a definition visible bare in the same file — so
//!   `mod.y` could mean either module-qualified access or field/member
//!   access on the definition. Fixed with an alias; flagged at the import.
//!
//! Compat: this pass only ever *adds* diagnostics, and every trigger
//! requires a `#@module`/`#@private`/`#@public`/`IMPORT` construct that no
//! strict-ink or existing brink-tier1 story contains — so the oracle and
//! tier1 corpus see nothing. In particular the `E025` import-required check
//! keys off a **declared** target module, absent from the entire pre-modules
//! corpus, so resolution stays byte-identical there.

use std::collections::{BTreeMap, BTreeSet};

use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolInfo,
    SymbolKind, Visibility,
};

/// The importable top-level kinds (modules-spec §2): "all top-level public
/// definitions — knots, functions, VARs, CONSTs, LISTs, STRUCTs". Stitches
/// are reachable only through the qualified form, so they are not part of
/// the bare-import export set validated here.
fn is_importable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Knot
            | SymbolKind::Variable
            | SymbolKind::Constant
            | SymbolKind::List
            | SymbolKind::Struct
    )
}

/// Per-file declared module name (`Some` only for a *declared* module,
/// shared across a multi-file module; `None` for an undeclared
/// stem-module), plus the public top-level exports per declared module (for
/// bare-import validation).
///
/// The module map is derived primarily from any *top-level* symbol the file
/// declares — every top-level symbol in a file shares that file's module by
/// construction — with a fallback to the file's own HIR `#@module(…)`
/// declaration for a file that declares no top-level symbols of its own
/// (only root content), which otherwise never appears in `index.symbols`
/// and would wrongly resolve to "no module" (`None`).
///
/// Locals (`Param`/`Temp`) are skipped: they carry `module: None` by design
/// (module-internal, never module-qualified — see `insert_local`), and
/// `index.symbols` is a `HashMap` whose iteration order is nondeterministic,
/// so a local iterated before the file's top-level symbols would randomly
/// poison the attribution to `None` and fire `E087` on same-module
/// self-references (issue #795).
fn file_modules_and_exports(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
) -> (
    BTreeMap<FileId, Option<String>>,
    BTreeMap<String, BTreeSet<String>>,
) {
    let mut file_module: BTreeMap<FileId, Option<String>> = BTreeMap::new();
    let mut declared_exports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for info in index.symbols.values() {
        // Locals never carry a module — attributing a file from one would
        // be order-dependent and wrong (issue #795, doc above).
        if matches!(info.kind, SymbolKind::Param | SymbolKind::Temp) {
            continue;
        }
        file_module.entry(info.file).or_insert(info.module.clone());
        if let Some(module) = &info.module
            && info.visibility == Visibility::Public
            && is_importable(info.kind)
        {
            declared_exports
                .entry(module.clone())
                .or_default()
                .insert(info.name.clone());
        }
    }
    for &(file_id, hir) in files {
        file_module
            .entry(file_id)
            .or_insert_with(|| hir.module.as_ref().map(|decl| decl.name.clone()));
    }
    (file_module, declared_exports)
}

/// Every module *name or name-prefix* this whole-project pass has real
/// visibility into (issue #1592): every module some file actually declares
/// itself as (`file_module`'s `Some` values), plus every `::`-joined
/// ancestor of those names.
///
/// The ancestor closure is the fix for the original silent no-op: a
/// directory that holds a declared submodule but is never itself the
/// module of any file (`story::market`, a pure container for
/// `story::market::barter`) never appears in `file_module` — nothing is
/// literally "module `story::market`" — so without this closure a bare
/// `use story::market::barter;` naming that container as `import.module`
/// could never be validated at all (neither confirmed nor refuted), and
/// `E088` stayed silent forever. `#@module(...)` accepts any non-empty
/// string, `::`-joined or not (`hir::lower::directive::module_directive_name`
/// places no structural constraint on it, and this crate's own
/// `native_use_dual_reading.rs` fixture declares
/// `#@module(story::market::barter)` from an `.ink` file), so this closure is
/// not an ink-specific no-op — a `::`-joined `#@module` fixture exercises it
/// exactly as a native `use` path does. The reason the oracle/tier1 corpus is
/// unaffected is the one stated in this module's top-level Compat doc: no
/// `#@module`/`IMPORT`/`use` construct appears anywhere in that corpus at
/// all, not any structural property of ink module names.
fn known_module_names(file_module: &BTreeMap<FileId, Option<String>>) -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    for module in file_module.values().flatten() {
        known.insert(module.clone());
        let mut segments: Vec<&str> = module.split("::").collect();
        while segments.len() > 1 {
            segments.pop();
            known.insert(segments.join("::"));
        }
    }
    known
}

/// Is the referring file inside the target's module?
///
/// For a **declared** target module, "same declared module name"; for an
/// undeclared stem-module (`None`), "the same file" (each undeclared file is
/// its own singleton module). Shared by the E087 and E025 cross-module
/// checks so they agree on the module boundary.
fn referrer_in_target_module(
    file_module: &BTreeMap<FileId, Option<String>>,
    target: &SymbolInfo,
    ref_file: FileId,
) -> bool {
    match &target.module {
        Some(tmod) => file_module.get(&ref_file).and_then(Option::as_ref) == Some(tmod),
        None => ref_file == target.file,
    }
}

/// Per-file import coverage: the set of modules imported qualified
/// (`IMPORT mod`) and the set of `(module, source_name)` pairs brought in by
/// bare imports (`IMPORT { name } FROM mod`). Together these decide whether a
/// cross-module public reference is licensed (M-2c, §2).
///
/// Keyed by [`FileId`]; a file with no imports simply has no entry.
/// `BTreeMap`/`BTreeSet` throughout — nothing here iterates in a way that
/// feeds output ordering, but the deterministic containers keep the pass
/// order-insensitive by construction.
type ImportCoverage<'a> = (
    BTreeMap<FileId, BTreeSet<String>>,
    BTreeMap<FileId, BTreeSet<(&'a str, &'a str)>>,
);

fn import_coverage<'a>(files: &'a [(FileId, &'a HirFile)]) -> ImportCoverage<'a> {
    let mut qualified: BTreeMap<FileId, BTreeSet<String>> = BTreeMap::new();
    let mut bare: BTreeMap<FileId, BTreeSet<(&str, &str)>> = BTreeMap::new();
    for &(file_id, hir) in files {
        // Shared with `ImportScope` (`resolve.rs`) so resolution and this
        // E025 gate can never diverge on what an import covers (issue #790
        // review).
        let (file_qualified, file_bare) = crate::resolve::import_coverage_for_file(&hir.imports);
        if !file_qualified.is_empty() {
            qualified.insert(file_id, file_qualified);
        }
        if !file_bare.is_empty() {
            bare.insert(file_id, file_bare);
        }
    }
    (qualified, bare)
}

/// Does `ref_file` import `name` from declared module `module` — either by
/// bare-importing that exact name from it, or by importing the module
/// qualified (which licenses `module.name` access to any of its exports)?
fn import_covers(
    qualified: &BTreeMap<FileId, BTreeSet<String>>,
    bare: &BTreeMap<FileId, BTreeSet<(&str, &str)>>,
    ref_file: FileId,
    module: &str,
    name: &str,
) -> bool {
    if qualified
        .get(&ref_file)
        .is_some_and(|mods| mods.contains(module))
    {
        return true;
    }
    bare.get(&ref_file)
        .is_some_and(|pairs| pairs.contains(&(module, name)))
}

/// Is there a definition named `name` visible **bare** in `file_id` — a
/// top-level symbol in this file's own module? Used to detect the
/// qualified-import ambiguity (`E091`): a `IMPORT mod` collides when `mod` is
/// also such a definition.
///
/// Deterministic: `by_name` is a direct keyed lookup, and the candidate id
/// list is scanned membership-only (no order-dependent output).
fn symbol_visible_bare_in_file(
    index: &SymbolIndex,
    file_module: &BTreeMap<FileId, Option<String>>,
    file_id: FileId,
    name: &str,
) -> bool {
    let Some(ids) = index.by_name.get(name) else {
        return false;
    };
    ids.iter().any(|id| {
        index.symbols.get(id).is_some_and(|info| {
            // Bare-visible definitions are ordinary top-level names in the
            // same module; locals never participate in qualified access.
            !matches!(info.kind, SymbolKind::Param | SymbolKind::Temp)
                && referrer_in_target_module(file_module, info, file_id)
        })
    })
}

/// Cross-module reference gating (`E087` private / `E025` import-required).
///
/// Every resolved reference whose target lies outside the referrer's module
/// is gated: a `#@private` target is `E087` unconditionally; a *public*
/// target in another **declared** module is `E025` unless the referring file
/// imported it (bare name from that module, or the module qualified). A
/// same-module reference, or a public target in the undeclared legacy soup
/// (`module == None`), is always bare-legal and never flagged (§2/§3).
fn check_cross_module_refs(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    file_module: &BTreeMap<FileId, Option<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (qualified_imports, bare_imports) = import_coverage(files);

    for r in resolutions {
        let Some(target) = index.symbols.get(&r.target) else {
            continue;
        };
        // Locals are always same-file and module-internal — never a
        // cross-module concern.
        if matches!(target.kind, SymbolKind::Param | SymbolKind::Temp) {
            continue;
        }
        // A same-module reference is always bare-legal (§2) — nothing to
        // enforce.
        if referrer_in_target_module(file_module, target, r.file) {
            continue;
        }
        match target.visibility {
            // A `#@private` def referenced from outside its module: E087,
            // regardless of imports (private never crosses, §4).
            Visibility::Private => {
                diagnostics.push(Diagnostic {
                    file: r.file,
                    range: r.range,
                    message: format!("{}: `{}`", DiagnosticCode::E087.title(), target.name),
                    code: DiagnosticCode::E087,
                });
            }
            // A *public* def in another **declared** module (M-2c, §2): a
            // reference is legal only if this file imported it. Undeclared
            // target modules (`None`) are the permeable legacy soup and are
            // never gated, keeping the pre-modules corpus byte-identical.
            Visibility::Public => {
                if let Some(tmod) = &target.module
                    && !import_covers(
                        &qualified_imports,
                        &bare_imports,
                        r.file,
                        tmod,
                        &target.name,
                    )
                {
                    diagnostics.push(Diagnostic {
                        file: r.file,
                        range: r.range,
                        // Deliberately dialect-blind (issue #1590 companion
                        // finding): this pass reads only HIR, which never
                        // carries a native/ink frontend tag ("no dialect tag
                        // near HIR" — see `brink-db`'s `file_language` doc),
                        // so the message never spells out a concrete import
                        // statement — ink's `IMPORT { name } FROM mod` and
                        // native's `use mod::name;` differ. A consumer that
                        // *does* know the referring file's dialect
                        // (`brink-ide::import_fix::import_actions`, via
                        // `ProjectDb::is_native`) renders the concrete
                        // quick-fix syntax instead.
                        message: format!(
                            "unresolved cross-module reference `{name}` — import it from `{module}` (see modules-spec §2)",
                            name = target.name,
                            module = tmod,
                        ),
                        code: DiagnosticCode::E025,
                    });
                }
            }
        }
    }
}

/// Qualified module-vs-definition ambiguity (`E091`, §2): a `IMPORT mod`
/// (qualified) whose module name also names a definition visible bare in the
/// same file makes `mod.y` ambiguous. Flagged at the import's module token.
fn check_qualified_ambiguity(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    file_module: &BTreeMap<FileId, Option<String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for &(file_id, hir) in files {
        for import in &hir.imports {
            // Only the qualified form (`IMPORT mod`) introduces a module name
            // into value position where it can collide with a definition.
            if import.bare {
                continue;
            }
            if symbol_visible_bare_in_file(index, file_module, file_id, &import.module) {
                diagnostics.push(Diagnostic {
                    file: file_id,
                    range: import.module_range,
                    message: format!("{}: `{}`", DiagnosticCode::E091.title(), import.module),
                    code: DiagnosticCode::E091,
                });
            }
        }
    }
}

/// Run the M-2 import + visibility checks.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let (file_module, declared_exports) = file_modules_and_exports(files, index);
    let known_modules = known_module_names(&file_module);

    check_cross_module_refs(files, index, resolutions, &file_module, &mut diagnostics);
    check_qualified_ambiguity(files, index, &file_module, &mut diagnostics);

    // ── Import well-formedness (E088/E089/E090) ─────────────────────
    for &(file_id, hir) in files {
        if hir.imports.is_empty() {
            continue;
        }
        let own_module = file_module.get(&file_id).and_then(Option::clone);
        let mut seen_locals: BTreeSet<String> = BTreeSet::new();
        let mut seen_modules: BTreeSet<String> = BTreeSet::new();

        for import in &hir.imports {
            if import.bare {
                for item in &import.items {
                    // Duplicate local name across this file's imports.
                    if !seen_locals.insert(item.local_name().to_string()) {
                        diagnostics.push(Diagnostic {
                            file: file_id,
                            range: item.range,
                            message: format!(
                                "{}: `{}`",
                                DiagnosticCode::E089.title(),
                                item.local_name()
                            ),
                            code: DiagnosticCode::E089,
                        });
                    }

                    // Dual-reading (issue #1592): the trailing segment
                    // `item.name` may resolve as an item `import.module`
                    // publicly exports, or as a declared submodule
                    // `import.module::item.name` in its own right (Rust's
                    // `use` dual-reads its trailing segment; charter §13.2
                    // commits to that lineage). Both readings are checked
                    // independently and BOTH may hold at once — no
                    // precedence is needed between them (decided +
                    // documented at `resolve::import_coverage_for_file`,
                    // which is where the "resolves to a module" reading is
                    // actually *licensed*). This check only fires when
                    // NEITHER reading resolves.
                    let is_item = declared_exports
                        .get(&import.module)
                        .is_some_and(|exports| exports.contains(&item.name));
                    let full_path = format!("{}::{}", import.module, item.name);
                    let is_module = known_modules.contains(&full_path);

                    // Self-import via the prefix (review finding #1686,
                    // 2026-07-27): `own_module == import.module` is only a
                    // genuine self-import when this trailing segment does
                    // NOT itself resolve as a declared submodule. When it
                    // does (`is_module`), `import.module` is the
                    // *importing file's own module* legitimately importing
                    // one of its own declared **child** submodules
                    // (`story::market` writing `use story::market::barter;`
                    // to license `barter`'s exports bare) — required by the
                    // E025 import-required gate, not a self-import. That
                    // shape gets its own full-path check right below,
                    // exactly where it belongs; checked per item (not once
                    // per import) because whether it applies depends on
                    // this item's own dual-reading verdict.
                    if !is_module && own_module.as_deref() == Some(import.module.as_str()) {
                        diagnostics.push(Diagnostic {
                            file: file_id,
                            range: import.module_range,
                            message: format!(
                                "{}: `{}`",
                                DiagnosticCode::E090.title(),
                                import.module
                            ),
                            code: DiagnosticCode::E090,
                        });
                    }

                    // A trailing segment that resolves as a module names
                    // that module — from *this* declaration's own module,
                    // that is a self-import exactly as the qualified form's
                    // check above, just reached through the item-leaf
                    // shape (`use story::market::barter;` from inside
                    // `story::market::barter` itself).
                    if is_module && own_module.as_deref() == Some(full_path.as_str()) {
                        diagnostics.push(Diagnostic {
                            file: file_id,
                            range: item.range,
                            message: format!("{}: `{}`", DiagnosticCode::E090.title(), full_path),
                            code: DiagnosticCode::E090,
                        });
                    }

                    // Aliased trailing module segment (review finding #1686,
                    // 2026-07-27): `use story::market::barter as b;` where
                    // `barter` resolves as a **module**, not an item, has no
                    // representation to alias — `ImportItem.alias` renames
                    // one local binding, but a licensed module contributes
                    // its whole (unbounded, project-wide-determined) export
                    // set under their own names; there is no field to carry
                    // "these exports now come in under `b`" instead. Before
                    // this check, the alias was silently ignored: the
                    // submodule's exports still became bare-visible under
                    // their original names (the phantom `module::item`
                    // candidate in `resolve::import_coverage_for_file` does
                    // not know about aliases at all), while `b` bound
                    // nothing — with no diagnostic anywhere. This is the
                    // same "no `Import` shape for aliasing a whole module"
                    // gap `lower_native::import::lower_use_decl` already
                    // rejects loudly for the single-segment form
                    // (`use a as m;` → `E129`); reused here because it is
                    // structurally the same defect, only knowable once
                    // whole-project module data resolves the dual-reading
                    // (which is why it can't be caught at lowering time).
                    if is_module && item.alias.is_some() {
                        diagnostics.push(Diagnostic {
                            file: file_id,
                            range: item.range,
                            message: format!(
                                "{}: cannot alias imported module `{}`",
                                DiagnosticCode::E129.title(),
                                full_path
                            ),
                            code: DiagnosticCode::E129,
                        });
                    }

                    // Unresolved import: the trailing segment names neither
                    // a public export of the *declared* module `import.module`
                    // nor a declared submodule of it. Only checked when this
                    // pass has real visibility into `import.module` (via
                    // `known_modules`, which — unlike the old
                    // `declared_exports`-only guard — also covers a
                    // container module with no items of its own, closing
                    // the original silent no-op). `known_modules` is a
                    // superset of `declared_exports`'s keys by construction
                    // (every exporting module is a `file_module` value), so
                    // this single check subsumes the old guard. An
                    // undeclared/unknown module's export set genuinely
                    // isn't visible here, so it stays unchecked rather than
                    // false-flagged.
                    if !is_item && !is_module && known_modules.contains(&import.module) {
                        diagnostics.push(Diagnostic {
                            file: file_id,
                            range: item.range,
                            message: format!(
                                "{}: `{}` from `{}`",
                                DiagnosticCode::E088.title(),
                                item.name,
                                import.module
                            ),
                            code: DiagnosticCode::E088,
                        });
                    }
                }
            } else {
                // Self-import: a module cannot import itself (qualified
                // form — the whole path always names the module, no
                // trailing-segment dual-reading applies here).
                if own_module.as_deref() == Some(import.module.as_str()) {
                    diagnostics.push(Diagnostic {
                        file: file_id,
                        range: import.module_range,
                        message: format!("{}: `{}`", DiagnosticCode::E090.title(), import.module),
                        code: DiagnosticCode::E090,
                    });
                }
                // Qualified form: a repeated `IMPORT mod` is a duplicate.
                if !seen_modules.insert(import.module.clone()) {
                    diagnostics.push(Diagnostic {
                        file: file_id,
                        range: import.module_range,
                        message: format!("{}: `{}`", DiagnosticCode::E089.title(), import.module),
                        code: DiagnosticCode::E089,
                    });
                }
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use brink_format::{DefinitionId, DefinitionTag};
    use brink_ir::{
        Block, DiagnosticCode, FileId, HirFile, Import, ImportItem, ModuleDecl, ResolvedRef, Scope,
        SymbolIndex, SymbolInfo, SymbolKind, Visibility,
    };
    use rowan::{TextRange, TextSize};

    use super::check;

    fn range(offset: u32, len: u32) -> TextRange {
        TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
    }

    fn hir_with_module(name: &str) -> HirFile {
        hir_with_module_and_imports(name, Vec::new())
    }

    fn hir_with_module_and_imports(name: &str, imports: Vec<Import>) -> HirFile {
        HirFile {
            root_content: Block::default(),
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            structs: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
            module: Some(ModuleDecl {
                name: name.to_string(),
                range: range(0, 1),
                was: None,
            }),
            imports,
            visibility: Vec::new(),
            was_directives: Vec::new(),
            allow_scopes: Vec::new(),
            element_matches: Vec::new(),
        }
    }

    /// A bare `use module::item;` / `IMPORT { item } FROM module` with no
    /// alias — the shape `#1592`'s dual-reading applies to.
    fn bare_import(module: &str, item: &str) -> Import {
        Import {
            module: module.to_string(),
            module_range: range(0, 1),
            items: vec![ImportItem {
                name: item.to_string(),
                alias: None,
                range: range(1, 1),
            }],
            bare: true,
            range: range(0, 2),
        }
    }

    /// The aliased form (`use module::item as alias;` /
    /// `IMPORT { item AS alias } FROM module`) — the shape the #1686 review
    /// found silently dropped an alias-of-module diagnostic.
    fn bare_import_with_alias(module: &str, item: &str, alias: &str) -> Import {
        Import {
            module: module.to_string(),
            module_range: range(0, 1),
            items: vec![ImportItem {
                name: item.to_string(),
                alias: Some(alias.to_string()),
                range: range(1, 1),
            }],
            bare: true,
            range: range(0, 2),
        }
    }

    fn symbol(
        id: DefinitionId,
        kind: SymbolKind,
        name: &str,
        module: Option<&str>,
        scope: Option<Scope>,
    ) -> SymbolInfo {
        SymbolInfo {
            kind,
            file: FileId(0),
            range: range(0, 1),
            id,
            name: name.to_string(),
            params: Vec::new(),
            detail: None,
            scope,
            param_detail: None,
            module: module.map(str::to_string),
            visibility: Visibility::Private,
        }
    }

    /// Issue #795: a single file declaring `#@module(quest)` whose knot
    /// references a sibling knot bare must produce zero `E087`, no matter
    /// which of the file's symbols `index.symbols` (a `HashMap` with
    /// nondeterministic iteration order) happens to yield first. Locals
    /// carry `module: None` by design; before the fix, a local iterated
    /// ahead of the file's top-level symbols randomly poisoned the file's
    /// module attribution to `None`, flagging every same-module
    /// self-reference. Repeated fresh-HashMap runs cover the order space.
    #[test]
    fn single_file_declared_module_self_reference_never_e087() {
        for _ in 0..64 {
            let hir = hir_with_module("quest");
            let files = [(FileId(0), &hir)];

            let knot_id = DefinitionId::new(DefinitionTag::Address, 1);
            let sibling_id = DefinitionId::new(DefinitionTag::Address, 2);
            let temp_id = DefinitionId::new(DefinitionTag::LocalVar, 3);

            // Fresh HashMaps each iteration — fresh RandomState, fresh order.
            let mut index = SymbolIndex::default();
            index.symbols.insert(
                knot_id,
                symbol(knot_id, SymbolKind::Knot, "caller", Some("quest"), None),
            );
            index.symbols.insert(
                sibling_id,
                symbol(sibling_id, SymbolKind::Knot, "sibling", Some("quest"), None),
            );
            index.symbols.insert(
                temp_id,
                symbol(
                    temp_id,
                    SymbolKind::Temp,
                    "g",
                    None,
                    Some(Scope {
                        knot: Some("caller".to_string()),
                        stitch: None,
                    }),
                ),
            );
            for (name, id) in [("caller", knot_id), ("sibling", sibling_id), ("g", temp_id)] {
                index.by_name.entry(name.to_string()).or_default().push(id);
            }

            // `caller` (file 0, module quest) references `sibling` bare —
            // same declared module, must never be gated.
            let resolutions = vec![ResolvedRef {
                file: FileId(0),
                range: range(10, 7),
                target: sibling_id,
            }];

            let diagnostics = check(&files, &index, &resolutions);
            assert!(
                diagnostics.is_empty(),
                "same-module self-reference must produce no diagnostics, got {diagnostics:?}"
            );
        }
    }

    /// Issue #1590 companion finding: this pass never sees a native/ink
    /// frontend tag (HIR is dialect-blind by design — "no dialect tag near
    /// HIR"), so the `E025` message must not spell out a concrete import
    /// statement's syntax — ink's `IMPORT { name } FROM mod` reads wrong to a
    /// native `.brink` author, and there is no signal here to pick the other
    /// spelling instead. The message stays syntax-free; a consumer that does
    /// know the referring file's dialect (`brink-ide::import_fix`, via
    /// `ProjectDb::is_native`) renders the concrete quick-fix text.
    #[test]
    fn e025_message_never_hardcodes_a_concrete_import_statement() {
        let quest = hir_with_module("quest");
        let town = hir_with_module("town");
        let files = [(FileId(0), &quest), (FileId(1), &town)];

        let ambush_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            ambush_id,
            SymbolInfo {
                visibility: Visibility::Public,
                ..symbol(ambush_id, SymbolKind::Knot, "ambush", Some("quest"), None)
            },
        );
        index
            .by_name
            .entry("ambush".to_string())
            .or_default()
            .push(ambush_id);

        // `town` references `quest`'s public `ambush` bare, with no IMPORT/
        // `use` at all — E025 must fire (this is the same well-established
        // gate `check_cross_module_refs` exercises elsewhere in this test
        // module and in `brink-ide`'s `import_fix` tests).
        let resolutions = vec![ResolvedRef {
            file: FileId(1),
            range: range(10, 6),
            target: ambush_id,
        }];

        let diagnostics = check(&files, &index, &resolutions);
        let e025: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E025)
            .collect();
        assert_eq!(e025.len(), 1, "expected exactly one E025: {diagnostics:?}");
        assert!(
            !e025[0].message.contains("IMPORT"),
            "E025 message must not hardcode ink's IMPORT syntax: {:?}",
            e025[0].message
        );
    }

    /// Review finding #1686 (E088 guard widening): swapping the old
    /// `declared_exports.get(&import.module).is_some()` guard for
    /// `known_modules.contains(&import.module)` (needed so a pure-directory
    /// prefix like `story::market` is checked at all, dual-reading's whole
    /// point) is broader than just that pure-directory case — `E088` now
    /// also fires for a bare import naming an item of a **declared module
    /// that exports nothing publicly at all** (every top-level symbol
    /// `#@private`/unmarked-native-default-private), where before this PR
    /// it was silent (no `declared_exports` entry to check against, exactly
    /// like the pure-directory case, but for a different underlying
    /// reason). Pinned here so this widening — real and intentional, since
    /// `known_modules` is the correct superset for the pure-directory case
    /// — doesn't silently drift further. `E088`'s own diagnostic title
    /// ("names a definition the declared module does not export") already
    /// reads correctly for this case: a private item genuinely is not
    /// exported, so no wording change is needed alongside the widening.
    #[test]
    fn bare_import_from_a_declared_module_with_no_public_exports_is_e088() {
        let vault = hir_with_module("vault");
        let main = hir_with_module_and_imports("main", vec![bare_import("vault", "secret")]);
        let files = [(FileId(0), &vault), (FileId(1), &main)];

        // `vault` declares `secret`, but it's Private — never a
        // `declared_exports` entry, so `is_item` is false. `secret` also
        // isn't itself a declared module, so `is_module` is false too.
        let secret_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            secret_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Private,
                ..symbol(secret_id, SymbolKind::Knot, "secret", Some("vault"), None)
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e088: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E088)
            .collect();
        assert_eq!(
            e088.len(),
            1,
            "a bare import from a declared module that exports nothing publicly must diagnose \
             E088 (the known_modules-superset widening), even though the named item genuinely \
             exists (just private): {diagnostics:?}"
        );
    }

    // ── Issue #1592: dual-reading `use`/`IMPORT` trailing segments ──────

    /// A `story::market` prefix with **no file of its own** — a pure
    /// directory holding the declared submodule `story::market::barter` —
    /// is exactly the original silent no-op's fixture: before #1592,
    /// `declared_exports.get("story::market")` was `None` (nothing exports
    /// *from* a module no file ever declares), so the `E088` check could
    /// never fire either way, whether `barter` was a real submodule or a
    /// typo. Dual-reading must resolve this trailing segment as the module
    /// `story::market::barter` and license it — no `E088`.
    #[test]
    fn dual_reading_trailing_segment_resolving_to_a_submodule_is_not_e088() {
        let barter = hir_with_module("story::market::barter");
        let main = hir_with_module_and_imports(
            "story::main",
            vec![bare_import("story::market", "barter")],
        );
        let files = [(FileId(0), &barter), (FileId(1), &main)];

        let haggle_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            haggle_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    haggle_id,
                    SymbolKind::Knot,
                    "haggle",
                    Some("story::market::barter"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e088: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E088)
            .collect();
        assert!(
            e088.is_empty(),
            "a trailing segment naming a real submodule must not be E088: {diagnostics:?}"
        );
    }

    /// The mirror of the previous test: the same container-only
    /// `story::market` prefix, but the trailing segment names neither a
    /// declared submodule nor an export — this is the case #1592 requires
    /// to newly diagnose (previously silent for exactly the same structural
    /// reason: `story::market` had no `declared_exports` entry to check
    /// against).
    #[test]
    fn dual_reading_trailing_segment_resolving_to_neither_is_e088() {
        let barter = hir_with_module("story::market::barter");
        let main = hir_with_module_and_imports(
            "story::main",
            vec![bare_import("story::market", "nonexistent")],
        );
        let files = [(FileId(0), &barter), (FileId(1), &main)];

        let haggle_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            haggle_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    haggle_id,
                    SymbolKind::Knot,
                    "haggle",
                    Some("story::market::barter"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e088: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E088)
            .collect();
        assert_eq!(
            e088.len(),
            1,
            "a trailing segment naming neither an export nor a submodule must diagnose (the \
             retired silent no-op): {diagnostics:?}"
        );
    }

    /// Precedence decision (#1592, "decide and document"): when a trailing
    /// segment resolves as **both** a declared item of the parent module
    /// *and* a declared submodule in its own right, neither reading is
    /// suppressed — no `E088` fires, because the check only fires when
    /// NEITHER reading holds. (The *licensing* half of "both apply" — that
    /// the submodule's exports also become bare-visible alongside the item
    /// — is proved at the resolution level in
    /// `native_use_dual_reading.rs`, which is a whole-project concern this
    /// diagnostics-only pass can't observe.)
    #[test]
    fn dual_reading_both_item_and_submodule_neither_is_suppressed() {
        let market = hir_with_module("story::market");
        let barter = hir_with_module("story::market::barter");
        let main = hir_with_module_and_imports(
            "story::main",
            vec![bare_import("story::market", "barter")],
        );
        let files = [
            (FileId(0), &market),
            (FileId(1), &barter),
            (FileId(2), &main),
        ];

        let mut index = SymbolIndex::default();
        // `story::market` itself declares a public item literally named
        // `barter` — the "both" collision.
        let item_id = DefinitionId::new(DefinitionTag::Address, 1);
        index.symbols.insert(
            item_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    item_id,
                    SymbolKind::Knot,
                    "barter",
                    Some("story::market"),
                    None,
                )
            },
        );
        // `story::market::barter` is also a real, separately declared
        // submodule.
        let haggle_id = DefinitionId::new(DefinitionTag::Address, 2);
        index.symbols.insert(
            haggle_id,
            SymbolInfo {
                file: FileId(1),
                visibility: Visibility::Public,
                ..symbol(
                    haggle_id,
                    SymbolKind::Knot,
                    "haggle",
                    Some("story::market::barter"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e088: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E088)
            .collect();
        assert!(
            e088.is_empty(),
            "a trailing segment that resolves as both an item and a module must not diagnose: \
             {diagnostics:?}"
        );
    }

    /// The dual-reading module path also feeds self-import (`E090`): a file
    /// declaring `story::market::barter` that `use`s the leaf-item form of
    /// its own module (`use story::market::barter;`, parsed as
    /// `module: "story::market", items: [barter]`) is importing itself
    /// exactly as if it had written the qualified form directly — the
    /// pre-#1592 check only compared `own_module` against `import.module`
    /// (the *prefix*), so this shape was invisible to `E090` even though it
    /// is the same self-import.
    #[test]
    fn dual_reading_self_import_via_leaf_form_is_e090() {
        let barter = hir_with_module_and_imports(
            "story::market::barter",
            vec![bare_import("story::market", "barter")],
        );
        let files = [(FileId(0), &barter)];

        let haggle_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            haggle_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    haggle_id,
                    SymbolKind::Knot,
                    "haggle",
                    Some("story::market::barter"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e090: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E090)
            .collect();
        assert_eq!(
            e090.len(),
            1,
            "the leaf-item form naming this file's own module must self-import exactly as the \
             qualified form does: {diagnostics:?}"
        );
    }

    /// Review finding #1686 (BLOCKING E090 false positive): a **parent**
    /// module importing its own declared **child** submodule via the
    /// leaf-item shape (`use story::market::barter;` written from inside
    /// `story::market` itself, parsed as `module: "story::market", items:
    /// [barter]`) must NOT be flagged `E090` — this is exactly the import
    /// the `E025` import-required gate makes *mandatory* for
    /// `story::market` to reference `story::market::barter`'s exports bare,
    /// not a self-import. The pre-fix prefix check
    /// (`own_module == import.module`) could not distinguish this from a
    /// genuine self-import because it never consulted the item's own
    /// dual-reading verdict (`is_module`).
    #[test]
    fn parent_module_importing_its_own_declared_submodule_is_not_e090() {
        let barter = hir_with_module("story::market::barter");
        let market = hir_with_module_and_imports(
            "story::market",
            vec![bare_import("story::market", "barter")],
        );
        let files = [(FileId(0), &barter), (FileId(1), &market)];

        let haggle_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            haggle_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    haggle_id,
                    SymbolKind::Knot,
                    "haggle",
                    Some("story::market::barter"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        assert!(
            diagnostics.is_empty(),
            "a parent module importing its own declared child submodule must diagnose nothing \
             (no E090, and the submodule licenses `haggle` so no E088 either): {diagnostics:?}"
        );
    }

    /// Review finding #1686 (BLOCKING aliased trailing module segment): a
    /// trailing segment that both carries a local alias AND resolves as a
    /// declared **submodule** (`use story::market::barter as b;`) has no
    /// sound `Import` representation — aliasing an entire module's export
    /// set, not one name. Before this fix this was silently accepted:
    /// `story::market::barter`'s exports still became bare-visible under
    /// their own names (the phantom module candidate ignores aliases) while
    /// `b` bound nothing, with no diagnostic anywhere. Mirrors
    /// `lower_native::import::lower_use_decl`'s `E129` for the
    /// single-segment `use a as m;` module-alias shape.
    #[test]
    fn aliased_trailing_segment_resolving_to_a_submodule_is_e129() {
        let barter = hir_with_module("story::market::barter");
        let main = hir_with_module_and_imports(
            "story::main",
            vec![bare_import_with_alias("story::market", "barter", "b")],
        );
        let files = [(FileId(0), &barter), (FileId(1), &main)];

        let haggle_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            haggle_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    haggle_id,
                    SymbolKind::Knot,
                    "haggle",
                    Some("story::market::barter"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e129: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E129)
            .collect();
        assert_eq!(
            e129.len(),
            1,
            "aliasing a trailing segment that resolves as a module must diagnose E129, not \
             silently drop the alias: {diagnostics:?}"
        );
    }

    /// Ink dialect regression (#1592 "tests in both dialects"): `quest`
    /// exports something, but never `ambush`, and no file anywhere declares
    /// a module named `quest::ambush` — so neither dual-reading path
    /// resolves. This is *not* because ink module names are structurally
    /// flat (`#@module(...)` accepts any non-empty string, `::`-joined or
    /// not — see `known_module_names`'s doc, corrected by the #1686 review);
    /// it is simply that this fixture's corpus never declares that
    /// submodule. `E088` must keep firing exactly as it did before dual-
    /// reading landed, for this genuinely-unexported name.
    #[test]
    fn unexported_import_still_diagnoses_e088_with_dual_reading_in_place() {
        let quest = hir_with_module("quest");
        let town = hir_with_module_and_imports("town", vec![bare_import("quest", "ambush")]);
        let files = [(FileId(0), &quest), (FileId(1), &town)];

        // `quest` exports something, but never `ambush`.
        let other_id = DefinitionId::new(DefinitionTag::Address, 1);
        let mut index = SymbolIndex::default();
        index.symbols.insert(
            other_id,
            SymbolInfo {
                file: FileId(0),
                visibility: Visibility::Public,
                ..symbol(
                    other_id,
                    SymbolKind::Knot,
                    "guard_talk",
                    Some("quest"),
                    None,
                )
            },
        );

        let diagnostics = check(&files, &index, &Vec::new());
        let e088: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E088)
            .collect();
        assert_eq!(
            e088.len(),
            1,
            "an unexported ink import must still diagnose E088, unaffected by dual-reading: \
             {diagnostics:?}"
        );
    }
}
