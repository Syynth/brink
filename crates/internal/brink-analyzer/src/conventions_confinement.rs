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
//! follows). The caller computes `is_conventions_module` once per file and
//! hands it in alongside the raw pointer string (for the diagnostic
//! message only).
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

use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

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
}
