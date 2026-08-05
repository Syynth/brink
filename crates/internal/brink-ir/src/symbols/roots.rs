//! Module-namespace **root identity** (docs/decision-log.md 2026-08-04
//! "`std::` and libraries are PEER ROOTS of `story::`, not children of it";
//! docs/modules-spec.md §4; issue #2245, generalized to a set by #2251).
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
//! reference site, "is this a reserved peer root?" collapses to one check:
//! does the module's leading segment appear in [`RESERVED_ROOTS`]? Before
//! this module existed, that check was reinvented identically in two
//! places — `brink-analyzer::resolve` and `brink-ir::lir::lower::decls` —
//! because `brink-ir` cannot depend on `brink-analyzer` (the reverse edge
//! is the real one: `brink-analyzer` already depends on `brink-ir`, per
//! this crate's own `symbols` module doc — "so that `brink-ir::lir` can
//! consume the resolved index without depending on `brink-analyzer`").
//! Defining the check here, in the substrate both already share, removes
//! the duplication rather than merely keeping it in sync by hand.
//!
//! # A set, not a single constant (#2251)
//!
//! #2245's fix (PR #2250) hardcoded exactly one peer root as a single
//! `&str` constant, even though the ruling above was stated generally
//! ("`std::` — and every future mounted library"). A single constant
//! cannot answer a question about a second peer without either
//! re-deriving the "which branch does this root take" logic by hand at
//! every call site (recreating the duplication #2245 deleted) or silently
//! falling through to the `story` branch — the original #2245 defect,
//! recurring. [`RESERVED_ROOTS`] is the set every such call site now
//! consults, so a second mounted library is a one-line data change here,
//! not a re-derivation anywhere else.
//!
//! This intentionally does **not** add a per-root visibility *policy* type
//! (an enum, a trait, a `ReservedRoot { name, policy }` struct) — that half
//! of #2251's ask is deliberately deferred, not delivered here. Note that
//! the membership check below is not policy-neutral: every consumer that
//! calls [`is_reserved_root_module`] now applies std's bare-name-fallback
//! exclusion to *any* member of [`RESERVED_ROOTS`], with no per-root
//! opt-out. That is a real decision, baked in by generalizing a single
//! `std`-specific check into a set-membership test — it is just not a
//! *differing* policy per root, because there is only one data point
//! (`std`) to generalize a difference from. A future root that needs
//! different visibility behavior than `std` still needs the policy type;
//! this module only gives it a name to add to the set.

/// The standard library's reserved peer-root name — the one entry in
/// [`RESERVED_ROOTS`] that exists today (`brink_environment::mount_stdlib`'s
/// `std/…` source-key convention, turned into a module path by
/// `brink_db::modules::native_module_path`). Kept as its own public
/// constant rather than folded away into an unlabeled `RESERVED_ROOTS[0]`
/// index: today the only consumer that means "the std root" specifically,
/// not "any reserved root", is this module's own tests
/// (`reserved_roots_contains_exactly_std_today`), but that distinction —
/// naming one particular root versus testing set membership — is real even
/// with a single entry, and a public name is cheaper to keep than to add
/// back once a second root exists and something legitimately needs to
/// single `std` out from its peers.
pub const STD_ROOT: &str = "std";

/// The full set of reserved peer-root names (#2251, generalizing #2245's
/// single [`STD_ROOT`] constant). A structural constant, not a
/// project-config lookup — only `std` exists today; a future library
/// mount (any new `brink_environment::mount_*` producer) adds its own
/// entry here, so every consumer below (`native_module_path`,
/// [`is_reserved_root_module`], and the `Candidacy::Other` exclusion
/// sites in `brink-analyzer::resolve` / `brink-ir::lir::lower::decls`)
/// picks it up as data rather than needing a new hardcoded branch.
pub const RESERVED_ROOTS: &[&str] = &[STD_ROOT];

/// True when `module`'s leading `::`-segment names any reserved peer root
/// in [`RESERVED_ROOTS`] — `std` itself/its submodules today
/// (`std::conventions::screenplay`, …), and every future mounted library
/// once its root joins the set.
///
/// Generalizes the single-root `is_std_module` check #2245 shipped: every
/// caller of that function actually meant "is this a mounted-library
/// candidate, not project-owned" (the bare-name-fallback exclusion policy
/// applies identically to any reserved root, not specifically to `std`),
/// so the check generalizes along with the constant it was built on.
#[must_use]
pub fn is_reserved_root_module(module: &str) -> bool {
    RESERVED_ROOTS
        .iter()
        .any(|root| module_root_is(module, root))
}

/// Pure root-identity check for one candidate root: `module`'s leading
/// `::`-segment is exactly `root` (not merely a textual prefix — see the
/// `stdlib`-vs-`std` test below).
fn module_root_is(module: &str, root: &str) -> bool {
    module
        .strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_root_itself_and_its_submodules() {
        assert!(is_reserved_root_module("std"));
        assert!(is_reserved_root_module("std::conventions::screenplay"));
    }

    #[test]
    fn rejects_a_project_module_even_when_textually_similar() {
        assert!(!is_reserved_root_module("story::std"));
        assert!(!is_reserved_root_module(
            "story::std::conventions::screenplay"
        ));
        assert!(!is_reserved_root_module("story"));
        assert!(!is_reserved_root_module("story::stdlib"));
        // A same-prefixed but distinct sibling name must not match —
        // `strip_prefix` alone (without the boundary check) would wrongly
        // accept this.
        assert!(!is_reserved_root_module("stdlib"));
    }

    /// #2251: `module_root_is` takes `root` as a parameter rather than
    /// hardcoding `std` — proven here with a root name that is not `std`
    /// at all. This is the evidence that a second [`RESERVED_ROOTS`] entry
    /// needs no new branch in [`is_reserved_root_module`] or
    /// `module_root_is`, only a new entry in the set.
    #[test]
    fn root_identity_check_is_root_agnostic() {
        assert!(module_root_is("gizmo", "gizmo"));
        assert!(module_root_is("gizmo::widgets", "gizmo"));
        assert!(!module_root_is("gizmolib", "gizmo"));
        assert!(!module_root_is("story::gizmo", "gizmo"));
    }

    /// Documents today's actual set (one library) so a reader of the test
    /// file — not just the doc comment — sees exactly what is mounted
    /// without needing to cross-reference `brink_environment`.
    #[test]
    fn reserved_roots_contains_exactly_std_today() {
        assert_eq!(RESERVED_ROOTS, &[STD_ROOT]);
    }
}
