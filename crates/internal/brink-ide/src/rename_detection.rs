//! Undeclared-rename detection (issue #1672 part 2, docs/modules-spec.md
//! §5).
//!
//! `crate::rename` (part 1) writes `#@was` automatically when a rename goes
//! through the IDE's own refactor (F2 / `brink ide rename`). This module
//! covers the residual case the R1 ruling on #1672 calls out explicitly: a
//! rename that never passes through that machinery — a hand edit, a `sed`,
//! a merge — still leaves the identity hole `#@was` exists to close, and
//! the author may not even know the directive exists.
//!
//! [`detect_undeclared_renames`] diffs the current [`SymbolManifest`]
//! against the file's previous one (the caller's job to keep one around —
//! an editor session across keystrokes, an LSP backend across
//! `didChange`s) and, for an unambiguous 1:1 shape, reports a
//! [`RenameSuspicion`] the caller can turn into a question: *"`hub`
//! disappeared and `plaza` appeared — did you rename it?"*
//!
//! This is explicitly **not** the fuzzy load-time rematching
//! docs/modules-spec.md §5 rejects ("silent-garbage risk"): that rejection
//! is of *silently* guessing an identity mapping *at load* (rehydrating a
//! save against a possibly-wrong target with no one to confirm it). This
//! runs at *authoring* time, decides nothing on its own, and only ever
//! *asks* — accepting the suggestion is still the author writing `#@was`
//! (by hand, or via the companion quick-fix), same as any other rename.
//!
//! Deliberately conservative: a kind (Knot, Stitch, Variable, Constant,
//! List — the same set [`crate::rename::was_directive_edit`] stamps) only
//! reports when it loses **exactly one** declared name and gains **exactly
//! one**, in the same file, in the same diff step. Two names disappearing
//! alongside two appearing is not a rename shape this can disambiguate
//! (which old name became which new one?), so it is silently skipped
//! rather than guessed at — the false-negative side of the same
//! never-guess principle above. A rename that never passes through brink
//! tooling *at all* (so this diff never runs) is the residual gap
//! docs/modules-spec.md §5 and issue #1672 both call out as something to
//! document, not solve.

use brink_ir::{DeclaredSymbol, SymbolKind, SymbolManifest};
use rowan::TextRange;

/// A suspected undeclared rename: `old_name` (of kind `kind`) was declared
/// in the previous manifest and no longer is; `new_name` is the sole
/// newly-declared name of the same kind that doesn't already carry its own
/// `#@was` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameSuspicion {
    /// The kind shared by both the vanished and the new declaration.
    pub kind: SymbolKind,
    /// The name that was declared before and no longer is.
    pub old_name: String,
    /// The name newly declared in its place.
    pub new_name: String,
    /// The new declaration's own name range — where a caller anchors the
    /// question (a diagnostic, a hint, a quick-fix).
    pub new_range: TextRange,
}

/// Compare `old` and `new` — the same file's manifest before and after an
/// edit — and report every unambiguous 1:1 rename-shaped diff. See the
/// module doc for the exact conservatism rule.
#[must_use]
pub fn detect_undeclared_renames(
    old: &SymbolManifest,
    new: &SymbolManifest,
) -> Vec<RenameSuspicion> {
    let mut out = Vec::new();
    out.extend(diff_kind(SymbolKind::Knot, &old.knots, &new.knots));
    out.extend(diff_kind(SymbolKind::Stitch, &old.stitches, &new.stitches));
    out.extend(diff_kind(
        SymbolKind::Variable,
        &old.variables,
        &new.variables,
    ));
    out.extend(diff_kind(
        SymbolKind::Constant,
        &old.constants,
        &new.constants,
    ));
    out.extend(diff_kind(SymbolKind::List, &old.lists, &new.lists));
    out
}

/// The single-kind half of [`detect_undeclared_renames`]: a `BTreeSet` name
/// diff (deterministic — order never depends on hash iteration), gated to
/// the exactly-one-vanished/exactly-one-appeared shape.
fn diff_kind(
    kind: SymbolKind,
    old: &[DeclaredSymbol],
    new: &[DeclaredSymbol],
) -> Vec<RenameSuspicion> {
    let old_names: std::collections::BTreeSet<&str> = old.iter().map(|d| d.name.as_str()).collect();

    let disappeared: Vec<&str> = old_names
        .iter()
        .copied()
        .filter(|n| !new.iter().any(|d| d.name == *n))
        .collect();
    // A newly-declared name that already carries its own `#@was` has
    // already been migrated (by this same refactor or by hand) — it is not
    // "undeclared" anymore, so it's excluded from the appeared side.
    let appeared: Vec<&DeclaredSymbol> = new
        .iter()
        .filter(|d| d.was.is_none() && !old_names.contains(d.name.as_str()))
        .collect();

    match (disappeared.as_slice(), appeared.as_slice()) {
        ([old_name], [new_decl]) => vec![RenameSuspicion {
            kind,
            old_name: (*old_name).to_owned(),
            new_name: new_decl.name.clone(),
            new_range: new_decl.range,
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{RenameSuspicion, detect_undeclared_renames};
    use brink_ir::SymbolKind;

    fn manifest_of(src: &str) -> brink_ir::SymbolManifest {
        let mut s = crate::session::IdeSession::new();
        let id = s.update_and_analyze("t.ink", src.to_string());
        s.manifest(id).expect("manifest").clone()
    }

    #[test]
    fn a_vanished_knot_and_a_new_one_is_a_suspected_rename() {
        let old = manifest_of("=== hub ===\nHi.\n-> END\n");
        let new = manifest_of("=== plaza ===\nHi.\n-> END\n");

        let suspicions = detect_undeclared_renames(&old, &new);

        assert_eq!(
            suspicions,
            vec![RenameSuspicion {
                kind: SymbolKind::Knot,
                old_name: "hub".to_owned(),
                new_name: "plaza".to_owned(),
                new_range: new.knots[0].range,
            }]
        );
    }

    #[test]
    fn no_change_reports_nothing() {
        let old = manifest_of("=== hub ===\nHi.\n-> END\n");
        let new = manifest_of("=== hub ===\nHi.\n-> END\n");

        assert!(detect_undeclared_renames(&old, &new).is_empty());
    }

    #[test]
    fn a_brand_new_knot_with_no_vanished_counterpart_reports_nothing() {
        let old = manifest_of("=== hub ===\nHi.\n-> END\n");
        let new = manifest_of("=== hub ===\nHi.\n-> END\n=== plaza ===\nHi.\n-> END\n");

        assert!(
            detect_undeclared_renames(&old, &new).is_empty(),
            "an addition with no matching removal is not a rename shape"
        );
    }

    #[test]
    fn a_deleted_knot_with_no_new_counterpart_reports_nothing() {
        let old = manifest_of("=== hub ===\nHi.\n-> END\n=== plaza ===\nHi.\n-> END\n");
        let new = manifest_of("=== hub ===\nHi.\n-> END\n");

        assert!(
            detect_undeclared_renames(&old, &new).is_empty(),
            "a removal with no matching addition is not a rename shape"
        );
    }

    #[test]
    fn two_vanished_and_two_appeared_is_ambiguous_and_reports_nothing() {
        // Which old name became which new one? Not decidable from a name
        // diff alone — silently skip rather than guess (the "never guess"
        // principle this module shares with the §5 ruling it implements).
        let old = "=== hub ===\nHi.\n-> END\n=== market ===\nHi.\n-> END\n";
        let new = "=== plaza ===\nHi.\n-> END\n=== bazaar ===\nHi.\n-> END\n";

        assert!(detect_undeclared_renames(&manifest_of(old), &manifest_of(new)).is_empty());
    }

    #[test]
    fn a_new_declaration_that_already_carries_was_is_not_reported_again() {
        // Already migrated (by hand, or by a rename this diff didn't see
        // start-to-finish) — not "undeclared" anymore.
        let old = manifest_of("=== hub ===\nHi.\n-> END\n");
        let new = manifest_of("=== plaza ===\n#@was(hub)\nHi.\n-> END\n");

        assert!(detect_undeclared_renames(&old, &new).is_empty());
    }

    #[test]
    fn a_renamed_stitch_is_reported_at_the_stitch_kind() {
        let old = manifest_of("=== hub ===\n= market\nHi.\n-> DONE\n");
        let new = manifest_of("=== hub ===\n= plaza\nHi.\n-> DONE\n");

        let suspicions = detect_undeclared_renames(&old, &new);

        assert_eq!(
            suspicions,
            vec![RenameSuspicion {
                kind: SymbolKind::Stitch,
                old_name: "hub.market".to_owned(),
                new_name: "hub.plaza".to_owned(),
                new_range: new.stitches[0].range,
            }]
        );
    }

    #[test]
    fn a_knot_rename_and_a_stitch_rename_in_the_same_diff_are_both_reported() {
        let old = "=== hub ===\n= market\nHi.\n-> DONE\n";
        let new = "=== plaza ===\n= bazaar\nHi.\n-> DONE\n";

        let suspicions = detect_undeclared_renames(&manifest_of(old), &manifest_of(new));

        assert_eq!(suspicions.len(), 2, "one per kind: {suspicions:?}");
        assert!(
            suspicions.iter().any(|s| s.kind == SymbolKind::Knot
                && s.old_name == "hub"
                && s.new_name == "plaza")
        );
        assert!(suspicions.iter().any(|s| s.kind == SymbolKind::Stitch
            && s.old_name == "hub.market"
            && s.new_name == "plaza.bazaar"));
    }
}
