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

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
}

/// The file stem of a path: the final path segment with a trailing
/// `.ink` extension removed. `src/quest_3.ink` → `quest_3`.
pub(crate) fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.strip_suffix(".ink").unwrap_or(name)
}

/// The **root-relative key** a path's identity is derived from, given a
/// database-registered root — `native_root` for a `.brink` file's module
/// identity (issue #1572), or `ink_root` for a `.ink` file's root-content
/// scope-path qualifier (issue #1696, extending #1572's mechanism to ink).
/// Generic over which root the caller passes: the strip logic itself has no
/// language-specific behavior.
///
/// [`native_module_path`] is contractually a function of a *root-relative*
/// key, and so is [`hir::root_content_scope_path`](brink_ir::hir::root_content_scope_path).
/// Native discovery (`brink_driver::discover_native`) already registers files
/// under exactly such keys, so `native_root` is `None` there and `path` is
/// returned untouched; ink's CLI discovery has no such `RealFs`-scoped tree,
/// so `ink_root` is registered explicitly by `prepare_driver` instead. A
/// long-lived editor session is the other case both roots serve: the LSP
/// keys `ProjectDb` by **absolute OS path** (it must — every path it holds
/// round-trips through a `file://` URI), so without the strip below every
/// identity it minted embedded the machine's directory layout
/// (`story::Users::…::market::barter`) and diverged from a real compile of the
/// same tree. Normalizing here, at the one place each identity function is
/// fed, is the house-rule-19a "normalize before you key" fix: the db keyspace
/// stays absolute (and collision-free across workspace roots) while identity
/// stays root-relative.
///
/// Both `root` and `path` are absolutized first, via [`std::path::absolute`]
/// (lexical `.`/`..` resolution against the process cwd — no filesystem
/// access, so it stays wasm-safe: it either succeeds with no I/O or fails
/// cleanly, it never touches disk), before the strip. A bare `Path::
/// strip_prefix` without this is unsound whenever `root` and `path` are
/// spelled with different qualifiers for the same file — e.g. `root` came
/// back absolute from `native_source_root`'s #1413 retry (a `brink.toml`
/// found only by walking up from an *absolutized* entry dir, because the
/// entry was relatively spelled and its cwd-relative walk alone came up
/// empty) while `path` is still `entry`'s raw relative spelling
/// (`prepare_driver` registers the ink entry key verbatim): `main.ink`,
/// `./main.ink`, and an absolute spelling of the same file would then strip
/// to three *different* keys (`main.ink`, `./main.ink`, and the
/// root-relative form) instead of agreeing — reopening the exact
/// CLI-vs-`brink-lsp` divergence #1696 exists to close (review finding on
/// #1706). Absolutizing both sides first means every spelling resolves to
/// the same real path before the strip ever runs.
///
/// When [`std::path::absolute`] errors on either side (only possible if the
/// process has no queryable cwd, e.g. wasm — see its doc), this falls back
/// to the raw, unabsolutized strip so wasm callers keep exactly their prior
/// (already root-relative-spelled) behavior instead of hard-erroring.
///
/// A `path` that does not live under `root` — even after absolutizing — is
/// returned unchanged (the original, non-absolutized string) rather than
/// mangled: a file outside the configured tree keeps whatever key it was
/// registered under, exactly as before this function existed.
pub(crate) fn root_relative_key<'a>(root: Option<&str>, path: &'a str) -> Cow<'a, str> {
    let Some(root) = root.filter(|r| !r.is_empty()) else {
        return Cow::Borrowed(path);
    };
    let root_abs = std::path::absolute(Path::new(root)).unwrap_or_else(|_| PathBuf::from(root));
    let path_abs = std::path::absolute(Path::new(path)).unwrap_or_else(|_| PathBuf::from(path));
    match path_abs.strip_prefix(&root_abs) {
        Ok(rel) => Cow::Owned(rel.to_string_lossy().into_owned()),
        Err(_) => Cow::Borrowed(path),
    }
}

/// A native `.brink` file's module path, derived **purely** from its
/// root-relative key (decision-log 2026-07-22 "Native module identity: pure
/// function of the root-relative path"; charter §13.2: path on disk = path
/// in language) — see [`root_relative_key`] for how a caller that
/// keys by absolute path obtains one. Directory segments become
/// `::`-separated module walls, the file (`.brink` stripped) is the leaf, and
/// `story::` is the absolute root:
///
/// - `barter.brink`            → `story::barter`
/// - `market/barter.brink`     → `story::market::barter`
/// - `npcs/quests/intro.brink` → `story::npcs::quests::intro`
///
/// This string is folded into `DefinitionId` identity (a **declared**,
/// always-qualifying module), so it is save-key-critical and must stay a
/// pure function of the path — nothing else (no `FileId`, no discovery
/// order, no other file) may enter it. Unlike ink modules, native modules
/// never flow through `resolve_modules`: they have no `#@module` inheritance
/// and no INCLUDE graph, so their identity is this function and nothing more.
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
            },
            None => ResolvedModule {
                name: input.stem.clone(),
                declared: false,
                was: None,
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

    #[test]
    fn native_module_path_derives_purely_from_relative_path() {
        // Charter §13.2: path on disk = path in language; `story::` is the
        // absolute root, `::` crosses walls, the file is the leaf module.
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

    /// Issue #1572: with no declared root — every compile path, where
    /// `discover_native` already keys root-relative — the key is the path,
    /// untouched.
    #[test]
    fn root_relative_key_is_identity_without_a_root() {
        assert_eq!(
            root_relative_key(None, "market/barter.brink"),
            "market/barter.brink"
        );
        // An empty root is treated as "no root", not as a prefix that
        // matches everything.
        assert_eq!(
            root_relative_key(Some(""), "market/barter.brink"),
            "market/barter.brink"
        );
    }

    /// Issue #1572: an absolute-keyed consumer (the LSP) that declares its
    /// tree root gets exactly the key `discover_native` would have produced —
    /// so `native_module_path` mints compile-identical module identity.
    #[test]
    fn root_relative_key_strips_a_declared_root() {
        let root = "/home/dev/game";
        assert_eq!(
            root_relative_key(Some(root), "/home/dev/game/market/barter.brink"),
            "market/barter.brink"
        );
        assert_eq!(
            native_module_path(&root_relative_key(
                Some(root),
                "/home/dev/game/market/barter.brink"
            )),
            native_module_path("market/barter.brink"),
            "the whole point: absolute-keyed identity must equal compile identity"
        );
        // A trailing separator on the root is the same root.
        assert_eq!(
            root_relative_key(Some("/home/dev/game/"), "/home/dev/game/main.brink"),
            "main.brink"
        );
    }

    /// Review finding on #1706: `native_source_root`'s #1413 absolutized
    /// retry can hand back an ABSOLUTE root for an entry that was itself
    /// registered under a relative spelling (`prepare_driver` registers the
    /// ink entry key verbatim, never resolved against `root`). A bare
    /// `Path::strip_prefix` without absolutizing both sides first cannot
    /// match an absolute root against a relative path even when they name
    /// the same real file, so `main.ink` and `./main.ink` used to strip to
    /// two different keys (each left unchanged) once `root` was absolute.
    /// Uses the real process cwd as `root` — no filesystem access, no
    /// `chdir`, no file needs to exist — precisely because
    /// [`std::path::absolute`] resolves a relative `path` against cwd
    /// lexically; the full `native_source_root`-driven, real-`brink.toml`
    /// version of this same scenario lives at
    /// `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`'s
    /// `root_content_ids_are_stable_when_brink_toml_lives_above_the_entry_dir`.
    #[test]
    fn root_relative_key_absolutizes_a_relative_path_against_an_absolute_root() {
        let cwd = std::env::current_dir().expect("process must have a cwd");
        let root = cwd.to_string_lossy().into_owned();

        assert_eq!(root_relative_key(Some(&root), "main.ink"), "main.ink");
        assert_eq!(root_relative_key(Some(&root), "./main.ink"), "main.ink");
        assert_eq!(
            root_relative_key(Some(&root), "sub/main.ink"),
            "sub/main.ink"
        );
    }

    /// A path outside the declared root keeps whatever key it was registered
    /// under — never a mangled partial strip. `Path::strip_prefix` matches
    /// whole components, so a root that is only a *textual* prefix of the
    /// path (`…/game` vs `…/game-assets`) is not a match either.
    #[test]
    fn root_relative_key_leaves_paths_outside_the_root_alone() {
        assert_eq!(
            root_relative_key(Some("/home/dev/game"), "/elsewhere/stray.brink"),
            "/elsewhere/stray.brink"
        );
        assert_eq!(
            root_relative_key(Some("/home/dev/game"), "/home/dev/game-assets/a.brink"),
            "/home/dev/game-assets/a.brink"
        );
    }

    fn input(file: u32, stem: &str, declared: Option<&str>) -> FileModuleInput {
        FileModuleInput {
            file: FileId(file),
            stem: stem.to_string(),
            declared: declared.map(str::to_string),
            was: None,
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
