//! The MODULE half of the §9.1 confinement ruling (issue #1844,
//! `docs/decision-log.md` 2026-07-31 "Conventions are annotated handlers",
//! item (4)):
//!
//! > **Pattern-claiming is confined to ONE module** — the conventions
//! > module named in `brink.toml`. `!name`-dispatched handlers stay legal
//! > anywhere precisely because they self-announce.
//!
//! #1838 (and #1847's follow-up) enforce the *placement* half — a claiming
//! `fn` must be a direct top-level child of its own file (`E112`). This
//! module enforces the other half: that file must be **the one** file
//! `brink.toml`'s `[project] conventions` key names. The asymmetry is the
//! whole point: a `!name`-dispatched line self-announces at the call site,
//! while a claiming pattern can silently reinterpret ordinary prose as a
//! call — so the auditability the ruling protects depends on every claim
//! living in one file a reader already knows to open.
//!
//! (Issue #2180: this key was named `elements` until the 2026-08-03
//! `@[element]`/`@[convention]` split ruling — `brink-project-config`
//! still accepts `elements` as a deprecated alias, but by the time a
//! `ProjectConfig` reaches this crate the two keys have already been
//! reconciled into one value, so this module never sees `elements`
//! specifically.)
//!
//! # Why this is a pure, caller-fed function
//!
//! Resolving `conventions` against real project/module identity is
//! `brink-db`'s job (`native_module_path`, `root_relative_key`,
//! `module_map_query`) — this crate stays dependency-free of that path
//! machinery, matching every other project-identity-gated check
//! (`native_strict_only_error`'s `is_native` flag is the precedent this
//! follows). [`conventions_module_diagnostics`]/
//! [`conventions_unconfigured_diagnostics`] themselves stay pure and
//! caller-fed exactly this way: the caller computes `is_conventions_module`
//! once per file and hands it in alongside the raw pointer string (for the
//! diagnostic message only).
//!
//! [`conventions_confinement_diagnostics`] (issue #2335) is the one
//! exception: a caller with no `ProjectDb` at all — the off-db analysis
//! road — has no db to ask "is this file the conventions module", so this
//! function does that resolution itself, via a small duplicated
//! [`native_module_path`] rather than an injected callback. See that
//! function's own doc for why duplication, not a dependency, is the
//! shape here.
//!
//! # What IS now enforced (issue #2289, part 2 of the 2026-08-05 ruling)
//!
//! An **unset `conventions` key** used to be silent here, on the reasoning
//! recorded just below (kept for history): "nothing is being confined to
//! yet, so a project that hasn't opted in stays exactly as permissive as it
//! always was." That reasoning did not survive part 1 of the same ruling —
//! conventions now claim across the WHOLE PROJECT, not just their own file —
//! so a `@[convention]` with no configured module names no module for the
//! declaration to belong to. It is a misconfiguration, not an opt-out, and
//! [`conventions_unconfigured_diagnostics`] reports it at `E169` exactly like
//! [`conventions_module_diagnostics`] reports one declared outside the
//! configured module. The caller (`brink_db::queries::analysis::
//! conventions_confinement_diagnostics_query`) now calls this sibling
//! instead of skipping the file whenever `opts.conventions` is `None`.
//!
//! # What is still NOT enforced here, and why
//!
//! - **A bare preset name** (`conventions = "screenplay"`). A preset points
//!   at a `std::conventions::*` module, not a project file — there is no
//!   path in the project tree to compare a claiming handler's own file
//!   against, and inventing a project-side "no file may claim" rule for
//!   this case is a bigger decision than this issue's slice covers (see
//!   the PR description's scope note). The caller skips this pass for a
//!   non-path-shaped pointer. This module still stays silent on it either
//!   way — but as of issue #1874 the bare name itself is no longer silently
//!   *accepted*: `AnalysisOptions::apply_project_config` (a different crate,
//!   a different check) validates it against the closed built-in-preset set
//!   and emits a `ConfigWarning` when it isn't recognized — `"screenplay"`
//!   is now a recognized name (issue #1720 shipped its authored source at
//!   `std/conventions/screenplay.brink`), though recognition there is only
//!   the validation verdict; this module's own confinement check still has
//!   nothing to confine a preset-shaped pointer against, unchanged.

use brink_ir::symbols::{RESERVED_ROOTS, is_reserved_root_module};
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

use crate::manifest::ModuleMap;

/// Whether a `[project] conventions` pointer names a project-relative path
/// to a `.brink` conventions module, as opposed to a bare built-in preset
/// name (docs/prose-dialect-spec.md §3.4's "either shape" pointer
/// mechanism). A path either contains a directory separator or ends in the
/// `.brink` extension; a bare word (no separator, no extension) is a
/// preset name.
///
/// Shared by two callers so the "is this a path or a preset" classification
/// can never drift between them: `brink-db`'s
/// `conventions_confinement_diagnostics_query` (this module's own `E169`
/// caller) skips the module-confinement check entirely for a preset-shaped
/// pointer — a preset has no project file to compare a claiming handler's
/// own module against (see this module's doc above) — and
/// [`crate::AnalysisOptions::apply_project_config`]'s preset-name
/// validation (issue #1874) skips its closed-set check entirely for a
/// path-shaped pointer — rejecting a valid custom-module path here would
/// break the exact case #1844's confinement rule is built around.
///
/// Renamed from `is_path_shaped_elements_pointer` by issue #2180.
#[must_use]
pub fn is_path_shaped_conventions_pointer(pointer: &str) -> bool {
    pointer.contains('/')
        || pointer.contains('\\')
        || std::path::Path::new(pointer)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("brink"))
}

/// A native `.brink` file's module path, derived **purely** from its
/// root-relative key. DELIBERATELY duplicates `brink_db::modules::
/// native_module_path` byte-for-byte rather than sharing it: `brink-db`
/// already depends on `brink-analyzer` (for [`ModuleMap`]/`ResolvedModule`),
/// so the reverse dependency this module's own
/// [`is_path_shaped_conventions_pointer`] precedent relies on cannot run in
/// this direction, and lowering `brink-analyzer` to sit *under* `brink-db`
/// is a bigger structural change than this issue's slice covers. See
/// `brink_db::modules::native_module_path`'s own doc for the full
/// path-to-module derivation this mirrors, including the peer-root exception
/// for [`RESERVED_ROOTS`] (decision-log 2026-08-04, "`std::` and libraries
/// are PEER ROOTS of `story::`").
///
/// The one caller that needs it, [`conventions_confinement_diagnostics`]
/// (issue #2335's off-db road), never applies a `ProjectDb::native_root`
/// offset before calling this — every current off-db caller of that function
/// (`IdeSession`'s producers: `brink-web`'s `EditorSession`, the CLI's
/// `ide_session()`) registers files under already root-relative keys and
/// never declares a root, so `brink_db::modules::root_relative_key`'s own
/// no-root branch (return the path unchanged) is exactly what skipping it
/// here reproduces. `brink-lsp`'s `analysis_loop` is the one production
/// caller of `analyze_with_modules` that *does* declare a native root
/// (its files are keyed by absolute OS path) — this function's parity with
/// the db road is unverified for that caller and is not this issue's scope.
fn native_module_path(relative_path: &str) -> String {
    native_module_path_in(RESERVED_ROOTS, relative_path)
}

/// [`native_module_path`], parameterized over the reserved-root set instead
/// of hardcoding [`RESERVED_ROOTS`] — same rationale as `brink-db`'s own
/// `native_module_path_in` (a test needs a root-agnostic exercise the real,
/// one-member `RESERVED_ROOTS` constant cannot provide by itself).
fn native_module_path_in(roots: &[&str], relative_path: &str) -> String {
    let without_ext = relative_path
        .strip_suffix(".brink")
        .unwrap_or(relative_path);
    let mut segments = without_ext
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty() && *segment != ".");

    let Some(first) = segments.next() else {
        return String::from("story");
    };

    let mut out = if roots.contains(&first) {
        first.to_string()
    } else {
        format!("story::{first}")
    };
    for segment in segments {
        out.push_str("::");
        out.push_str(segment);
    }
    out
}

/// Run the confinement/unconfigured `E169` checks
/// ([`conventions_module_diagnostics`]/[`conventions_unconfigured_diagnostics`])
/// for a caller with no live `ProjectDb` to ask per-file — the off-db
/// analysis road (issue #2335: `IdeSnapshot::analyze`/`session.analysis()`,
/// reached through [`crate::analyze_with_modules`], this function's one
/// caller).
///
/// Mirrors `brink_db::queries::analysis::conventions_confinement_diagnostics_query`
/// file for file:
///
/// - a file with no declared claim handler is skipped immediately (lazy,
///   matching the db query's own doc: it never even reads `modules` for
///   such a file);
/// - a file whose resolved module is an [`is_reserved_root_module`] peer
///   (the mounted `std::…` tree) is exempt entirely (issue #2251's
///   exemption, folded into the db query on 2026-08-05) — a project that
///   configures its OWN conventions module must not have the *mounted*
///   preset's handlers flagged as living outside it;
/// - an entirely unset `conventions` pointer diagnoses every declared
///   handler as unconfigured (issue #2289 part 2's ruling: this is a
///   misconfiguration, not an opt-out);
/// - a bare preset name (not [`is_path_shaped_conventions_pointer`]) stays
///   silent — a preset names a `std::conventions::*` module, not a project
///   file, so there is nothing in `modules` to confine against (see that
///   function's own doc);
/// - a path-shaped pointer that resolves (via [`native_module_path`]) to no
///   real module in `modules` stays silent too, `tracing::warn!`-logged —
///   the same "warn, never silently drop" channel the db road uses for a
///   typo'd/moved/deleted pointer, so it never misfires `E169` against every
///   claiming handler in the project (including the real intended module)
///   for a config problem that is not their fault.
///
/// An entirely **empty** `modules` map opts a caller out of this whole check
/// (issue #2335 review finding), matching the established "empty `ModuleMap`
/// is inert" contract every other module-identity-gated behavior in
/// [`crate::analyze_with_modules`] already honors ([`crate::analyze_with_options`]'s
/// own doc: "An empty map reproduces `analyze_with_options` exactly"). A
/// real `IdeSession`/`brink-lsp` caller with a native claim handler always
/// has a non-empty map — native module identity is a pure, always-computed
/// function of the file's path — so this can only trigger for a
/// module-blind harness that compiles one in-memory source string with no
/// path at all (`brink_test_harness::corpus::compile_and_explore_from_brink_native`,
/// whose own doc records exactly this "no path-derived identity to qualify
/// by" tradeoff). Without this guard, that harness — used by
/// `issue_2092_scene_entered_extern.rs` to drive the *real* shipped
/// `std/conventions/screenplay.brink` source directly, outside the real
/// stdlib-mounting pipeline that would otherwise classify it as a
/// [`is_reserved_root_module`] peer — misfired unconfigured `E169` on the
/// preset's own handlers, which is not a real editor-path finding: nothing
/// about a genuinely module-blind compile can tell "this claim handler is
/// misplaced" from "there is no place for it to even be placed".
#[must_use]
pub fn conventions_confinement_diagnostics(
    files: &[(FileId, &HirFile)],
    modules: &ModuleMap,
    conventions: Option<&str>,
) -> Vec<Diagnostic> {
    if modules.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for &(file_id, hir) in files {
        if hir.claim_handlers.is_empty() {
            continue;
        }
        if modules
            .get(&file_id)
            .is_some_and(|m| is_reserved_root_module(&m.name))
        {
            continue;
        }
        let Some(pointer) = conventions else {
            out.extend(conventions_unconfigured_diagnostics(file_id, hir));
            continue;
        };
        if !is_path_shaped_conventions_pointer(pointer) {
            continue;
        }
        let expected_module = native_module_path(pointer);
        if !modules.values().any(|m| m.name == expected_module) {
            tracing::warn!(
                "[project] conventions = \"{pointer}\" does not match any file in the project \
                 (expected module `{expected_module}`) — conventions-module confinement (E169) \
                 is skipped until this is fixed"
            );
            continue;
        }
        let is_conventions_module = modules
            .get(&file_id)
            .is_some_and(|m| m.name == expected_module);
        out.extend(conventions_module_diagnostics(
            file_id,
            hir,
            is_conventions_module,
            pointer,
        ));
    }
    out
}

/// Diagnose every claiming handler declared in `hir` when this file is not
/// the project's configured conventions module.
///
/// `pointer` is the raw `[project] conventions` string as written in
/// `brink.toml` (already established by the caller to be path-shaped, not
/// a preset name) — used only to name the expected file in the diagnostic
/// message, per the issue's own instruction: "enforce it with a diagnostic
/// naming the file the handler should live in."
#[must_use]
pub fn conventions_module_diagnostics(
    file_id: FileId,
    hir: &HirFile,
    is_conventions_module: bool,
    pointer: &str,
) -> Vec<Diagnostic> {
    if is_conventions_module || hir.claim_handlers.is_empty() {
        return Vec::new();
    }
    hir.claim_handlers
        .iter()
        .map(|handler| Diagnostic {
            file: file_id,
            range: handler.annotation,
            code: DiagnosticCode::E169,
            message: format!(
                "`{name}` claims prose with `@[convention(claims = \"…\", order = …)]`, but \
                 pattern-claiming handlers may only be declared in the project's configured \
                 conventions module (`brink.toml`'s `[project] conventions = \"{pointer}\"`) — move \
                 `{name}` there",
                name = handler.name.text,
            ),
        })
        .collect()
}

/// Diagnose every claiming handler declared in `hir` when the project has
/// **no** conventions module configured at all (issue #2289, part 2 of the
/// 2026-08-05 ruling — see this module's own doc, "What IS now enforced").
///
/// The caller calls this instead of [`conventions_module_diagnostics`]
/// specifically when `[project] conventions` is entirely unset — there is
/// no `pointer` string to name a destination module with, so the message
/// tells the author to configure one rather than to move the handler
/// somewhere specific.
#[must_use]
pub fn conventions_unconfigured_diagnostics(file_id: FileId, hir: &HirFile) -> Vec<Diagnostic> {
    hir.claim_handlers
        .iter()
        .map(|handler| Diagnostic {
            file: file_id,
            range: handler.annotation,
            code: DiagnosticCode::E169,
            message: format!(
                "`{name}` claims prose with `@[convention(claims = \"…\", order = …)]`, but no \
                 conventions module is configured for this project — set `brink.toml`'s \
                 `[project] conventions = \"…\"` to the module `{name}` should live in",
                name = handler.name.text,
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower_native;

    use crate::manifest::ResolvedModule;

    fn build_native(src: &str) -> HirFile {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, _manifest, _diags) = lower_native::lower(FileId(0), &parsed.tree());
        hir
    }

    const CLAIMING_SRC: &str = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
        fn interior(place: content) {\n  return place;\n}\n\
        flow main() {\n  INT. MARKET SQUARE\n}\n";

    #[test]
    fn no_claim_handlers_is_always_silent() {
        let hir = build_native("flow main() {\n  hi\n}\n");
        let diags = conventions_module_diagnostics(
            FileId(0),
            &hir,
            /* is_conventions_module */ false,
            "conventions.brink",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn claim_handler_in_the_conventions_module_is_silent() {
        let hir = build_native(CLAIMING_SRC);
        let diags = conventions_module_diagnostics(
            FileId(0),
            &hir,
            /* is_conventions_module */ true,
            "conventions.brink",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn claim_handler_outside_the_conventions_module_is_e169() {
        let hir = build_native(CLAIMING_SRC);
        let diags = conventions_module_diagnostics(
            FileId(0),
            &hir,
            /* is_conventions_module */ false,
            "conventions.brink",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E169);
        assert!(diags[0].message.contains("interior"), "{diags:?}");
        assert!(diags[0].message.contains("conventions.brink"), "{diags:?}");
        // Anchored on the `@[element(…)]` annotation line, matching E112's
        // own placement-diagnostic anchor — never the handler's `fn` body.
        assert_eq!(diags[0].range, hir.claim_handlers[0].annotation);
    }

    #[test]
    fn one_diagnostic_per_declared_handler() {
        let src = "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\
                   @[convention(claims = \"^B$\", order = 20)]\nfn b() {\n  return \"b\";\n}\n\
                   flow main() {\n  hi\n}\n";
        let hir = build_native(src);
        assert_eq!(hir.claim_handlers.len(), 2, "{:?}", hir.claim_handlers);
        let diags = conventions_module_diagnostics(FileId(0), &hir, false, "conventions.brink");
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E169));
    }

    #[test]
    fn a_handler_that_never_wins_a_claim_is_still_diagnosed() {
        // `interior`'s pattern matches nothing in this file's own body —
        // `element_matches` would be empty — but the declaration itself is
        // what the confinement rule cares about (the module doc's
        // reasoning for why this can't be derived from `element_matches`).
        let src = "@[convention(claims = \"^INT\\\\. (?<place>.+)$\", order = 10)]\n\
                   fn interior(place: content) {\n  return place;\n}\n\
                   flow main() {\n  hi\n}\n";
        let hir = build_native(src);
        assert!(hir.element_matches.is_empty(), "{:?}", hir.element_matches);
        assert_eq!(hir.claim_handlers.len(), 1, "{:?}", hir.claim_handlers);
        let diags = conventions_module_diagnostics(FileId(0), &hir, false, "conventions.brink");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn no_claim_handlers_is_silent_even_when_unconfigured() {
        let hir = build_native("flow main() {\n  hi\n}\n");
        let diags = conventions_unconfigured_diagnostics(FileId(0), &hir);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn claim_handler_with_no_configured_module_is_e169() {
        // Issue #2289 part 2: an unset `conventions` key no longer opts a
        // project out of confinement — a declared claim handler with no
        // module to belong to is a misconfiguration and must error.
        let hir = build_native(CLAIMING_SRC);
        let diags = conventions_unconfigured_diagnostics(FileId(0), &hir);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E169);
        assert!(diags[0].message.contains("interior"), "{diags:?}");
        assert!(
            diags[0]
                .message
                .contains("no conventions module is configured"),
            "{diags:?}"
        );
        assert_eq!(diags[0].range, hir.claim_handlers[0].annotation);
    }

    #[test]
    fn one_diagnostic_per_declared_handler_when_unconfigured() {
        let src = "@[convention(claims = \"^A$\", order = 10)]\nfn a() {\n  return \"a\";\n}\n\
                   @[convention(claims = \"^B$\", order = 20)]\nfn b() {\n  return \"b\";\n}\n\
                   flow main() {\n  hi\n}\n";
        let hir = build_native(src);
        assert_eq!(hir.claim_handlers.len(), 2, "{:?}", hir.claim_handlers);
        let diags = conventions_unconfigured_diagnostics(FileId(0), &hir);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E169));
    }

    // ── `native_module_path` (duplicated from `brink_db::modules`, issue
    // #2335) — same example table as that crate's own tests, so the two
    // copies staying in sync is at least mechanically checkable. ─────────

    #[test]
    fn native_module_path_derives_purely_from_relative_path() {
        assert_eq!(native_module_path("barter.brink"), "story::barter");
        assert_eq!(
            native_module_path("market/barter.brink"),
            "story::market::barter"
        );
        assert_eq!(
            native_module_path("npcs/quests/intro.brink"),
            "story::npcs::quests::intro"
        );
        assert_eq!(
            native_module_path("market\\barter.brink"),
            "story::market::barter"
        );
        assert_eq!(native_module_path("./main.brink"), "story::main");
    }

    #[test]
    fn native_module_path_roots_a_std_mounted_key_as_a_peer_of_story() {
        assert_eq!(
            native_module_path("std/conventions/screenplay.brink"),
            "std::conventions::screenplay"
        );
        assert_eq!(native_module_path("std.brink"), "std");
    }

    #[test]
    fn native_module_path_in_generalizes_to_a_second_reserved_root() {
        let roots = &["gizmo"];
        assert_eq!(
            native_module_path_in(roots, "gizmo/leaf.brink"),
            "gizmo::leaf"
        );
        assert_eq!(
            native_module_path_in(roots, "market/barter.brink"),
            "story::market::barter"
        );
    }

    // ── `conventions_confinement_diagnostics` (issue #2335) ──────────────

    fn resolved_module(name: &str) -> ResolvedModule {
        ResolvedModule {
            name: name.to_owned(),
            declared: true,
            was: None,
        }
    }

    #[test]
    fn correctly_placed_handler_is_silent() {
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let mut modules = ModuleMap::new();
        modules.insert(FileId(0), resolved_module("story::conventions"));
        let diags =
            conventions_confinement_diagnostics(&files, &modules, Some("conventions.brink"));
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn misplaced_handler_with_a_configured_module_is_e169() {
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let mut modules = ModuleMap::new();
        // This file (id 0) is `story::other`, not the configured
        // `story::conventions` — nothing in `modules` even names the file
        // the pointer resolves to other than this mismatch.
        modules.insert(FileId(0), resolved_module("story::other"));
        modules.insert(FileId(1), resolved_module("story::conventions"));
        let diags =
            conventions_confinement_diagnostics(&files, &modules, Some("conventions.brink"));
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E169);
        assert!(diags[0].message.contains("conventions.brink"), "{diags:?}");
    }

    #[test]
    fn unset_conventions_reports_unconfigured() {
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let mut modules = ModuleMap::new();
        modules.insert(FileId(0), resolved_module("story::main"));
        let diags = conventions_confinement_diagnostics(&files, &modules, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0]
                .message
                .contains("no conventions module is configured"),
            "{diags:?}"
        );
    }

    #[test]
    fn preset_shaped_pointer_stays_silent() {
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let mut modules = ModuleMap::new();
        modules.insert(FileId(0), resolved_module("story::main"));
        let diags = conventions_confinement_diagnostics(&files, &modules, Some("screenplay"));
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_completely_empty_modules_map_opts_out_of_the_whole_check() {
        // The module-blind harness case (issue #2335 review finding):
        // `brink_test_harness::corpus::compile_and_explore_from_brink_native`
        // compiles one in-memory source string with no path-derived module
        // identity at all, so it can never tell a misplaced handler from a
        // handler with nowhere to belong. An entirely empty `modules` map
        // must not misfire `E169`, even with an unset `conventions` pointer
        // and a real claim handler present.
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let modules = ModuleMap::new();
        assert!(conventions_confinement_diagnostics(&files, &modules, None).is_empty());
        assert!(
            conventions_confinement_diagnostics(&files, &modules, Some("conventions.brink"))
                .is_empty()
        );
    }

    #[test]
    fn unresolvable_pointer_stays_silent() {
        // Path-shaped, but no file in `modules` resolves to that module —
        // a typo'd/moved/deleted `[project] conventions` target must not
        // misfire E169 against every claiming handler in the project.
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let mut modules = ModuleMap::new();
        modules.insert(FileId(0), resolved_module("story::somewhere_else"));
        let diags =
            conventions_confinement_diagnostics(&files, &modules, Some("conventions.brink"));
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_mounted_reserved_root_file_is_exempt_even_when_unconfigured() {
        // Issue #2251's peer-root exemption: `brink-environment` mounts
        // `std::conventions::screenplay` into every compiled project
        // regardless of whether that project's own `brink.toml` ever names
        // it — without this exemption, every project with no `conventions`
        // key would suddenly fail to compile against the mounted preset's
        // own handlers.
        let hir = build_native(CLAIMING_SRC);
        let files = [(FileId(0), &hir)];
        let mut modules = ModuleMap::new();
        modules.insert(FileId(0), resolved_module("std::conventions::screenplay"));
        let diags = conventions_confinement_diagnostics(&files, &modules, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_claim_handlers_is_silent_regardless_of_configuration() {
        let hir = build_native("flow main() {\n  hi\n}\n");
        let files = [(FileId(0), &hir)];
        let modules = ModuleMap::new();
        assert!(conventions_confinement_diagnostics(&files, &modules, None).is_empty());
        assert!(
            conventions_confinement_diagnostics(&files, &modules, Some("conventions.brink"))
                .is_empty()
        );
    }
}
