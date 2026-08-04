//! Module-namespace **root identity** (docs/decision-log.md 2026-08-04
//! "`std::` and libraries are PEER ROOTS of `story::`, not children of it";
//! docs/modules-spec.md §1/§4; issue #2245).
//!
//! `story::*` is the universe of what the project *author* provided.
//! `std::*` — and every future mounted library — is a top-level **peer**
//! of `story`, never a child of it: the module forest has several roots,
//! and a project's own tree is exactly one of them
//! (`brink_db::modules::native_module_path` mints `story::…` for an
//! ordinary project file and bare `std::…` for a file mounted under the
//! reserved `std/` key prefix — see that function's own doc).
//!
//! Once "which root is this?" is a structural fact about the module
//! *path itself* rather than a policy decided independently at each
//! reference site, "is this std?" collapses to one check: does the
//! module's root segment equal [`STD_ROOT`]? Before this module existed,
//! that check was reinvented identically in two places —
//! `brink-analyzer::resolve` and `brink-ir::lir::lower::decls` — because
//! `brink-ir` cannot depend on `brink-analyzer` (the reverse edge is the
//! real one: `brink-analyzer` already depends on `brink-ir`, per this
//! module's own crate's `symbols` doc — "so that `brink-ir::lir` can
//! consume the resolved index without depending on `brink-analyzer`").
//! Defining the check here, in the substrate both already share, removes
//! the duplication rather than merely keeping it in sync by hand.

/// The reserved peer-root name a mounted library's files are qualified
/// under (`brink_environment::mount_stdlib`'s `std/…` source-key
/// convention, turned into a module path by
/// `brink_db::modules::native_module_path`). A structural constant, not a
/// project-config lookup — only `std` exists today; a future library mount
/// would add its own reserved root alongside it.
pub const STD_ROOT: &str = "std";

/// True when `module` names the standard-library peer root — [`STD_ROOT`]
/// itself, or one of its submodules (`std::conventions::screenplay`, …).
///
/// A pure root-identity check: `module`'s leading `::`-segment is `std`.
/// Ties every consumer to the single [`STD_ROOT`] constant instead of a
/// hand-copied `"std::"` literal.
#[must_use]
pub fn is_std_module(module: &str) -> bool {
    module
        .strip_prefix(STD_ROOT)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_root_itself_and_its_submodules() {
        assert!(is_std_module("std"));
        assert!(is_std_module("std::conventions::screenplay"));
    }

    #[test]
    fn rejects_a_project_module_even_when_textually_similar() {
        assert!(!is_std_module("story::std"));
        assert!(!is_std_module("story::std::conventions::screenplay"));
        assert!(!is_std_module("story"));
        assert!(!is_std_module("story::stdlib"));
        // A same-prefixed but distinct sibling name must not match —
        // `strip_prefix` alone (without the boundary check) would wrongly
        // accept this.
        assert!(!is_std_module("stdlib"));
    }
}
