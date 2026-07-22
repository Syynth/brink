//! Module resolution (M-1, docs/modules-spec.md §1).
//!
//! Resolves every file to its module — the unit that qualifies
//! `DefinitionId` identity (§5). The rules:
//!
//! - **File-as-module by default**: an undeclared file's module is its
//!   file stem, and it does *not* qualify identity (byte-identical
//!   `DefinitionId`s to the pre-modules world).
//! - **`#@module(name)`** declares the module explicitly and opts the
//!   file into module-qualified identity.
//! - **INCLUDE glue**: an included file with no `#@module` of its own
//!   inherits its includer's module (name and declared-ness).
//! - **Stem collision**: an undeclared file whose stem equals some
//!   *declared* module's name is a compile error (`E085`) — the one
//!   footgun (accidental membership with mixed visibility defaults).
//!
//! This is a pure function of the per-file (stem, `#@module`) inputs and
//! the INCLUDE graph, so it is unit-testable in isolation and produces a
//! deterministic [`ModuleMap`] regardless of file iteration order.

use std::collections::BTreeMap;

use brink_analyzer::{ModuleMap, ResolvedModule};
use brink_ir::{Diagnostic, DiagnosticCode, FileId};
use rowan::TextRange;

use crate::include_graph::IncludeGraph;

/// Per-file resolution input: the file's stem (from its path) and its own
/// `#@module(name)` declaration, if any.
pub(crate) struct FileModuleInput {
    pub file: FileId,
    pub stem: String,
    pub declared: Option<String>,
    /// This file's own `#@was(old_name)`, if any (M-3, docs/modules-spec.md
    /// §5) — only meaningful alongside `declared`; ignored for an
    /// undeclared stem-module file (see `ModuleDecl::was`'s doc).
    pub was: Option<String>,
    /// `true` for a native `.brink` file whose `declared` module was derived
    /// from its filesystem path (B0.10b). Propagated onto the
    /// [`ResolvedModule`] so the analyzer can qualify identity like a
    /// declared module while defaulting the symbols public (charter "imports
    /// are naming only"). Always `false` for ink files.
    pub filesystem_derived: bool,
}

/// The file stem of a path: the final path segment with a trailing
/// `.ink` extension removed. `src/quest_3.ink` → `quest_3`.
pub(crate) fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.strip_suffix(".ink").unwrap_or(name)
}

/// Derive a native `.brink` file's **filesystem-derived module path**
/// (B0.10b, charter §13.2 / NF-3: "path on disk = path in language").
///
/// The `path` is taken **relative to the declared source root** (the native
/// discovery walk stores every file under a root-relative key, e.g.
/// `market/barter.brink`). Directory segments become `::`-separated module
/// walls and the file's own name — with its `.brink` extension stripped —
/// is the **leaf module** (charter §13.2: "directories = segments, files =
/// leaf modules"). `story::` is the absolute root of the whole project, so
/// it is prepended unconditionally:
///
/// - `barter.brink`            → `story::barter`
/// - `market/barter.brink`     → `story::market::barter`
/// - `npcs/quests/intro.brink` → `story::npcs::quests::intro`
///
/// This is the exact string folded into `DefinitionId` identity (M-1 hashing
/// in `brink-analyzer::manifest`), so it is **save-stability-critical** and
/// must stay byte-for-byte stable — the #719 defusal records absolute module
/// paths regardless of imports. The `::` separator is the ruled module-wall
/// spelling (charter §13.2 separator stratification: `::` crosses module
/// walls, `.` walks inside a module).
pub(crate) fn native_module_path(relative_path: &str) -> String {
    let without_ext = relative_path
        .strip_suffix(".brink")
        .unwrap_or(relative_path);
    let mut out = String::from("story");
    for segment in without_ext.split(['/', '\\']) {
        if segment.is_empty() || segment == "." {
            continue;
        }
        out.push_str("::");
        out.push_str(segment);
    }
    out
}

/// Resolve every file's module and detect stem collisions (`E085`).
///
/// Returns the [`ModuleMap`] consumed by
/// [`brink_analyzer::symbol_index_with_modules`] and any collision
/// diagnostics. Deterministic: inputs are processed in `FileId` order and
/// INCLUDE inheritance propagates first-declared-parent-wins along a
/// bounded fixpoint (at most one pass per file).
pub(crate) fn resolve_modules(
    inputs: &[FileModuleInput],
    graph: &IncludeGraph,
) -> (ModuleMap, Vec<Diagnostic>) {
    // Seed: a declared file is fixed; an undeclared file starts as its
    // stem-module (subject to INCLUDE inheritance below).
    let mut resolved: BTreeMap<FileId, ResolvedModule> = BTreeMap::new();
    for input in inputs {
        let module = match &input.declared {
            Some(name) => ResolvedModule {
                name: name.clone(),
                declared: true,
                was: input.was.clone(),
                filesystem_derived: input.filesystem_derived,
            },
            None => ResolvedModule {
                name: input.stem.clone(),
                declared: false,
                was: None,
                filesystem_derived: false,
            },
        };
        resolved.insert(input.file, module);
    }

    // INCLUDE inheritance: an undeclared file inherits the first (in
    // `FileId` order) of its includers whose resolved module is declared.
    // Bounded by the file count — a declared module propagates at most one
    // hop per pass down an include chain, so `inputs.len()` passes suffice
    // and the loop can never run unbounded (guard-against-growth rule).
    for _ in 0..inputs.len() {
        let mut changed = false;
        for input in inputs {
            if input.declared.is_some() {
                continue; // fixed — own declaration wins.
            }
            if resolved.get(&input.file).is_some_and(|m| m.declared) {
                continue; // already inherited a declared module.
            }
            let mut parents: Vec<FileId> = graph.included_by(input.file).to_vec();
            parents.sort_unstable();
            for parent in parents {
                if let Some(parent_module) = resolved.get(&parent)
                    && parent_module.declared
                {
                    let inherited = parent_module.clone();
                    resolved.insert(input.file, inherited);
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // M-3 (docs/modules-spec.md §5): a `#@was` declared on any file of a
    // multi-file module (several files independently carrying the same
    // `#@module(name)` — the merge case, not INCLUDE, which already
    // propagates the whole `ResolvedModule` including `was` above) applies
    // to the whole module. Aggregate by name so every file sees it
    // regardless of which file declared the directive. Deterministic:
    // `resolved` is a `BTreeMap<FileId, _>`, so `.values()` iterates in
    // `FileId` order — the first file (in that order) with a `was` for a
    // given module name wins if more than one disagrees (undiagnosed edge
    // case, same "first wins" discipline INCLUDE inheritance already uses).
    let mut was_by_name: BTreeMap<String, String> = BTreeMap::new();
    for module in resolved.values() {
        if module.declared
            && let Some(was) = &module.was
        {
            was_by_name
                .entry(module.name.clone())
                .or_insert_with(|| was.clone());
        }
    }
    for module in resolved.values_mut() {
        if module.declared
            && module.was.is_none()
            && let Some(was) = was_by_name.get(&module.name)
        {
            module.was = Some(was.clone());
        }
    }

    // Stem collision (`E085`): the set of declared module names, versus
    // every still-undeclared file whose stem lands in that set.
    let declared_names: std::collections::BTreeSet<&str> = resolved
        .values()
        .filter(|m| m.declared)
        .map(|m| m.name.as_str())
        .collect();

    let mut diagnostics = Vec::new();
    for input in inputs {
        let module = &resolved[&input.file];
        if !module.declared && declared_names.contains(module.name.as_str()) {
            diagnostics.push(Diagnostic {
                file: input.file,
                // No `#@module` directive to point at — anchor at the file
                // start.
                range: TextRange::default(),
                message: format!("{}: `{}`", DiagnosticCode::E085.title(), module.name),
                code: DiagnosticCode::E085,
            });
        }
    }

    (resolved, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(file: u32, stem: &str, declared: Option<&str>) -> FileModuleInput {
        FileModuleInput {
            file: FileId(file),
            stem: stem.to_string(),
            declared: declared.map(str::to_string),
            was: None,
            filesystem_derived: false,
        }
    }

    #[test]
    fn stem_helper_strips_dir_and_ext() {
        assert_eq!(file_stem("src/quest_3.ink"), "quest_3");
        assert_eq!(file_stem("story.ink"), "story");
        assert_eq!(file_stem("a/b/c.ink"), "c");
        assert_eq!(file_stem("noext"), "noext");
    }

    #[test]
    fn native_module_path_derives_from_relative_path() {
        // Charter §13.2 / NF-3: path on disk = path in language; files are
        // leaf modules; `story::` is the absolute root; `::` crosses walls.
        assert_eq!(native_module_path("barter.brink"), "story::barter");
        assert_eq!(
            native_module_path("market/barter.brink"),
            "story::market::barter"
        );
        assert_eq!(
            native_module_path("npcs/quests/intro.brink"),
            "story::npcs::quests::intro"
        );
        // Backslash separators and stray `.`/empty segments normalize away.
        assert_eq!(
            native_module_path("market\\barter.brink"),
            "story::market::barter"
        );
        assert_eq!(native_module_path("./main.brink"), "story::main");
    }

    #[test]
    fn undeclared_file_is_stem_module_not_qualifying() {
        let inputs = vec![input(0, "story", None)];
        let (map, diags) = resolve_modules(&inputs, &IncludeGraph::new());
        assert!(diags.is_empty());
        let m = &map[&FileId(0)];
        assert_eq!(m.name, "story");
        assert!(!m.declared);
    }

    #[test]
    fn declared_module_qualifies() {
        let inputs = vec![input(0, "story", Some("quest"))];
        let (map, diags) = resolve_modules(&inputs, &IncludeGraph::new());
        assert!(diags.is_empty());
        let m = &map[&FileId(0)];
        assert_eq!(m.name, "quest");
        assert!(m.declared);
    }

    #[test]
    fn included_file_inherits_declared_module() {
        // File 0 declares module `quest` and INCLUDEs file 1 (undeclared).
        let inputs = vec![input(0, "head", Some("quest")), input(1, "part", None)];
        let mut graph = IncludeGraph::new();
        graph.update(FileId(0), vec![FileId(1)]);
        let (map, diags) = resolve_modules(&inputs, &graph);
        assert!(diags.is_empty());
        let m = &map[&FileId(1)];
        assert_eq!(m.name, "quest", "included file inherits includer's module");
        assert!(m.declared);
    }

    #[test]
    fn inheritance_propagates_down_a_chain() {
        // 0 (decl quest) -> 1 -> 2, both undeclared.
        let inputs = vec![
            input(0, "head", Some("quest")),
            input(1, "mid", None),
            input(2, "leaf", None),
        ];
        let mut graph = IncludeGraph::new();
        graph.update(FileId(0), vec![FileId(1)]);
        graph.update(FileId(1), vec![FileId(2)]);
        let (map, _diags) = resolve_modules(&inputs, &graph);
        assert_eq!(map[&FileId(2)].name, "quest");
        assert!(map[&FileId(2)].declared);
    }

    #[test]
    fn undeclared_include_of_undeclared_stays_stem() {
        let inputs = vec![input(0, "head", None), input(1, "part", None)];
        let mut graph = IncludeGraph::new();
        graph.update(FileId(0), vec![FileId(1)]);
        let (map, diags) = resolve_modules(&inputs, &graph);
        assert!(diags.is_empty());
        assert_eq!(map[&FileId(1)].name, "part");
        assert!(!map[&FileId(1)].declared);
    }

    #[test]
    fn stem_collision_with_declared_module_is_e085() {
        // File 0 declares module `quest`; file 1 is an undeclared file
        // whose stem is *also* `quest` — the forbidden footgun.
        let inputs = vec![input(0, "head", Some("quest")), input(1, "quest", None)];
        let (_map, diags) = resolve_modules(&inputs, &IncludeGraph::new());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E085);
        assert_eq!(diags[0].file, FileId(1));
    }

    #[test]
    fn no_collision_when_same_name_is_declared() {
        // Two files both declaring the same module `quest` merge — no
        // collision (multi-file module).
        let inputs = vec![input(0, "a", Some("quest")), input(1, "b", Some("quest"))];
        let (map, diags) = resolve_modules(&inputs, &IncludeGraph::new());
        assert!(
            diags.is_empty(),
            "same declared module is a merge, not a clash"
        );
        assert!(map[&FileId(0)].declared && map[&FileId(1)].declared);
    }
}
