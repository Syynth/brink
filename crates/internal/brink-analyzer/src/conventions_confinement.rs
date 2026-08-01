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
//! `brink.toml`'s `[project] elements` key names. The asymmetry is the
//! whole point: a `!name`-dispatched line self-announces at the call site,
//! while a claiming pattern can silently reinterpret ordinary prose as a
//! call — so the auditability the ruling protects depends on every claim
//! living in one file a reader already knows to open.
//!
//! # Why this is a pure, caller-fed function
//!
//! Resolving `elements` against real project/module identity is
//! `brink-db`'s job (`native_module_path`, `root_relative_key`,
//! `module_map_query`) — this crate stays dependency-free of that path
//! machinery, matching every other project-identity-gated check
//! (`native_strict_only_error`'s `is_native` flag is the precedent this
//! follows). The caller computes `is_conventions_module` once per file and
//! hands it in alongside the raw pointer string (for the diagnostic
//! message only).
//!
//! # What is NOT enforced here, and why
//!
//! - **An unset `elements` key.** No conventions module is configured, so
//!   there is nothing to confine claiming *to* — every existing project
//!   without this key stays exactly as permissive as it was before this
//!   check existed. The caller is responsible for skipping this pass
//!   entirely in that case (never calling it with a meaningless `pointer`).
//! - **A bare preset name** (`elements = "screenplay"`). A preset points at
//!   a `std::conventions::*` module, not a project file — there is no path
//!   in the project tree to compare a claiming handler's own file against,
//!   and inventing a project-side "no file may claim" rule for this case
//!   is a bigger decision than this issue's slice covers (see the PR
//!   description's scope note). The caller skips this pass for a
//!   non-path-shaped pointer.

use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile};

/// Diagnose every claiming handler declared in `hir` when this file is not
/// the project's configured conventions module.
///
/// `pointer` is the raw `[project] elements` string as written in
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
                "`{name}` claims prose with `@[element(claims = \"…\")]`, but pattern-claiming \
                 handlers may only be declared in the project's configured conventions module \
                 (`brink.toml`'s `[project] elements = \"{pointer}\"`) — move `{name}` there",
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

    const CLAIMING_SRC: &str = "@[element(claims = \"^INT\\\\. (?<place>.+)$\")]\n\
        fn interior(place: content) {\n  return place;\n}\n\
        flow main() {\n  INT. MARKET SQUARE\n}\n";

    #[test]
    fn no_claim_handlers_is_always_silent() {
        let hir = build_native("flow main() {\n  hi\n}\n");
        let diags =
            conventions_module_diagnostics(FileId(0), &hir, /* is_conventions_module */ false, "conventions.brink");
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
        assert!(
            diags[0].message.contains("conventions.brink"),
            "{diags:?}"
        );
        // Anchored on the `@[element(…)]` annotation line, matching E112's
        // own placement-diagnostic anchor — never the handler's `fn` body.
        assert_eq!(diags[0].range, hir.claim_handlers[0].annotation);
    }

    #[test]
    fn one_diagnostic_per_declared_handler() {
        let src = "@[element(claims = \"^A$\")]\nfn a() {\n  return \"a\";\n}\n\
                   @[element(claims = \"^B$\")]\nfn b() {\n  return \"b\";\n}\n\
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
        let src = "@[element(claims = \"^INT\\\\. (?<place>.+)$\")]\n\
                   fn interior(place: content) {\n  return place;\n}\n\
                   flow main() {\n  hi\n}\n";
        let hir = build_native(src);
        assert!(hir.element_matches.is_empty(), "{:?}", hir.element_matches);
        assert_eq!(hir.claim_handlers.len(), 1, "{:?}", hir.claim_handlers);
        let diags = conventions_module_diagnostics(FileId(0), &hir, false, "conventions.brink");
        assert_eq!(diags.len(), 1, "{diags:?}");
    }
}
