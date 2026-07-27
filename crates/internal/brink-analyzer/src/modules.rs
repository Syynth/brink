//! Import well-formedness + cross-module visibility enforcement (M-2,
//! docs/modules-spec.md §2/§4/§7).
//!
//! Runs in the whole-project pass, where every file's `IMPORT` list (HIR)
//! and the merged symbol index (each [`SymbolInfo`] now carrying its module
//! and effective visibility, §4) are both available. Four jobs:
//!
//! - **Import well-formedness**: self-import (`E090`), a name brought into
//!   scope twice (`E089`), and a bare `IMPORT { name } FROM mod` naming a
//!   definition the (declared) module does not publicly export (`E088`).
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
    BTreeMap<FileId, BTreeSet<&'a str>>,
    BTreeMap<FileId, BTreeSet<(&'a str, &'a str)>>,
);

fn import_coverage<'a>(files: &'a [(FileId, &'a HirFile)]) -> ImportCoverage<'a> {
    let mut qualified: BTreeMap<FileId, BTreeSet<&str>> = BTreeMap::new();
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
    qualified: &BTreeMap<FileId, BTreeSet<&str>>,
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
            // Self-import: a module cannot import itself.
            if own_module.as_deref() == Some(import.module.as_str()) {
                diagnostics.push(Diagnostic {
                    file: file_id,
                    range: import.module_range,
                    message: format!("{}: `{}`", DiagnosticCode::E090.title(), import.module),
                    code: DiagnosticCode::E090,
                });
            }

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
                    // Unresolved import: the module is declared but does not
                    // publicly export this name. Only checked against
                    // *declared* modules (an undeclared stem-module's export
                    // set isn't visible to this pass) to stay sound.
                    if let Some(exports) = declared_exports.get(&import.module)
                        && !exports.contains(&item.name)
                    {
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
        Block, DiagnosticCode, FileId, HirFile, ModuleDecl, ResolvedRef, Scope, SymbolIndex,
        SymbolInfo, SymbolKind, Visibility,
    };
    use rowan::{TextRange, TextSize};

    use super::check;

    fn range(offset: u32, len: u32) -> TextRange {
        TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
    }

    fn hir_with_module(name: &str) -> HirFile {
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
            imports: Vec::new(),
            visibility: Vec::new(),
            was_directives: Vec::new(),
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
}
