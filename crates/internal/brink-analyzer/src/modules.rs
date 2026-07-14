//! Import well-formedness + cross-module visibility enforcement (M-2,
//! docs/modules-spec.md §2/§4/§7).
//!
//! Runs in the whole-project pass, where every file's `IMPORT` list (HIR)
//! and the merged symbol index (each [`SymbolInfo`] now carrying its module
//! and effective visibility, §4) are both available. Two jobs:
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
//!
//! Compat: this pass only ever *adds* diagnostics, and every trigger
//! requires a `#@private`/`#@public`/`IMPORT` construct that no strict-ink
//! or existing brink-tier1 story contains — so the oracle and tier1 corpus
//! see nothing.

use std::collections::{BTreeMap, BTreeSet};

use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, ResolutionMap, SymbolIndex, SymbolKind,
    Visibility,
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

/// Run the M-2 import + visibility checks.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Per-file declared module name (`Some` only for a *declared* module,
    // shared across a multi-file module; `None` for an undeclared
    // stem-module). Derived from any symbol the file declares — every symbol
    // in a file shares that file's module by construction.
    let mut file_module: BTreeMap<FileId, Option<String>> = BTreeMap::new();
    // Public top-level exports per declared module, for bare-import
    // validation.
    let mut declared_exports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for info in index.symbols.values() {
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

    // ── Cross-module visibility (E087) ──────────────────────────────
    for r in resolutions {
        let Some(target) = index.symbols.get(&r.target) else {
            continue;
        };
        // Locals are always same-file and module-internal — never a
        // cross-module concern.
        if matches!(target.kind, SymbolKind::Param | SymbolKind::Temp) {
            continue;
        }
        if target.visibility != Visibility::Private {
            continue;
        }
        // Is the referrer inside the target's module? For a declared module,
        // that is "same declared module name"; for an undeclared
        // stem-module (`None`), it is "the same file" (each undeclared file
        // is its own singleton module).
        let in_module = match &target.module {
            Some(tmod) => {
                file_module.get(&r.file).and_then(Option::as_ref) == Some(tmod)
            }
            None => r.file == target.file,
        };
        if !in_module {
            diagnostics.push(Diagnostic {
                file: r.file,
                range: r.range,
                message: format!("{}: `{}`", DiagnosticCode::E087.title(), target.name),
                code: DiagnosticCode::E087,
            });
        }
    }

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
