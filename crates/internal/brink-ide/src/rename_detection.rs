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
//! by hand, same as any other rename.
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
    out.extend(diff_stitches(&old.stitches, &new.stitches));
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

/// Group qualified `parent.bare` declarations by their `parent` segment,
/// pairing each with its own bare tail. A declaration with no `.` (shouldn't
/// occur for stitches — [`SymbolManifest::stitches`] is always qualified —
/// but never assumed) is silently excluded rather than mis-grouped.
fn group_by_parent(
    decls: &[DeclaredSymbol],
) -> std::collections::BTreeMap<&str, Vec<(&str, &DeclaredSymbol)>> {
    let mut map: std::collections::BTreeMap<&str, Vec<(&str, &DeclaredSymbol)>> =
        std::collections::BTreeMap::new();
    for d in decls {
        if let Some((parent, bare)) = d.name.rsplit_once('.') {
            map.entry(parent).or_default().push((bare, d));
        }
    }
    map
}

/// The `Stitch`-only half of [`detect_undeclared_renames`] (review finding on
/// #1672 part 2, blocking): unlike every other `#@was`-eligible kind,
/// `SymbolManifest::stitches` names are *qualified* (`knot.stitch`,
/// [`SymbolManifest`]'s own doc comment) — a plain
/// [`diff_kind`]-style whole-name diff conflates a genuine stitch rename with
/// the cascade a *knot* rename produces on every one of its children's
/// qualified names, and a nested stitch's `#@was` always takes the **bare**
/// old name ([`crate::rename::was_directive_edit`]'s doc comment), not the
/// qualified one this would otherwise report.
///
/// Grouped by qualifier (the parent knot) instead: a name diff runs only
/// *within* a parent present in both `old` and `new` — a knot rename changes
/// the qualifier on both sides, so the vanished qualified name lands in the
/// old parent's now-vanished group and the appeared one lands in a group that
/// has no old-side counterpart to pair against, and neither group ever
/// produces a same-group 1:1 match. This also transitively covers "the
/// parent was itself detected as renamed" and "the parent already carries
/// `#@was`" — either way the qualifier changed (or never existed) on the old
/// side, so the parent's group is never present on both sides to compare.
fn diff_stitches(old: &[DeclaredSymbol], new: &[DeclaredSymbol]) -> Vec<RenameSuspicion> {
    let old_by_parent = group_by_parent(old);
    let new_by_parent = group_by_parent(new);

    let mut out = Vec::new();
    for (parent, old_members) in &old_by_parent {
        let Some(new_members) = new_by_parent.get(parent) else {
            // The parent qualifier doesn't exist on the new side at all —
            // whether because the parent knot was itself renamed, deleted,
            // or (via `#@was`) already migrated, there is nothing on this
            // side to pair a bare-name diff against. Skip rather than guess.
            continue;
        };

        let disappeared: Vec<&str> = old_members
            .iter()
            .map(|(bare, _)| *bare)
            .filter(|bare| !new_members.iter().any(|(nb, _)| nb == bare))
            .collect();
        let appeared: Vec<&(&str, &DeclaredSymbol)> = new_members
            .iter()
            .filter(|(bare, d)| d.was.is_none() && !old_members.iter().any(|(ob, _)| ob == bare))
            .collect();

        if let ([old_bare], [(new_bare, new_decl)]) = (disappeared.as_slice(), appeared.as_slice())
        {
            out.push(RenameSuspicion {
                kind: SymbolKind::Stitch,
                old_name: (*old_bare).to_owned(),
                new_name: (*new_bare).to_owned(),
                new_range: new_decl.range,
            });
        }
    }
    out
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
    fn a_renamed_stitch_is_reported_at_the_stitch_kind_with_its_bare_name() {
        // The reported names are the *bare* stitch names, not the qualified
        // `knot.stitch` form — a nested stitch's `#@was` always takes the
        // bare old name (`crate::rename::was_directive_edit`'s doc comment),
        // so `RenameSuspicion::old_name` must match what the directive it
        // suggests actually expects.
        let old = manifest_of("=== hub ===\n= market\nHi.\n-> DONE\n");
        let new = manifest_of("=== hub ===\n= plaza\nHi.\n-> DONE\n");

        let suspicions = detect_undeclared_renames(&old, &new);

        assert_eq!(
            suspicions,
            vec![RenameSuspicion {
                kind: SymbolKind::Stitch,
                old_name: "market".to_owned(),
                new_name: "plaza".to_owned(),
                new_range: new.stitches[0].range,
            }]
        );
    }

    // ── Review finding on #1672 part 2 (blocking): a pure knot rename
    // fabricates a bogus stitch suspicion. `SymbolManifest::stitches` names
    // are qualified `knot.stitch`, so renaming only the parent knot changes
    // every child stitch's qualified name too — a naive whole-name diff
    // reports that as a stitch rename, and the `#@was(hub.market)` it
    // suggests is invalid (nested-stitch `#@was` takes the bare name) ──────

    #[test]
    fn renaming_a_knot_alone_does_not_fabricate_a_stitch_suspicion_for_an_unchanged_child() {
        let old = manifest_of("=== hub ===\n= market\nHi.\n-> DONE\n");
        let new = manifest_of("=== plaza ===\n= market\nHi.\n-> DONE\n");

        let suspicions = detect_undeclared_renames(&old, &new);

        assert_eq!(
            suspicions,
            vec![RenameSuspicion {
                kind: SymbolKind::Knot,
                old_name: "hub".to_owned(),
                new_name: "plaza".to_owned(),
                new_range: new.knots[0].range,
            }],
            "only the knot rename may be reported — the stitch's qualifier changed purely as \
             a cascade of the knot rename, not a stitch rename of its own: {suspicions:?}"
        );
    }

    #[test]
    fn renaming_a_knot_via_an_existing_was_directive_still_does_not_fabricate_a_stitch_suspicion() {
        // Same shape as above, but the knot rename is already recorded via
        // `#@was(hub)` on the knot itself (so it isn't reported either) —
        // the child stitch's cascaded qualifier change must still not be
        // mistaken for a stitch rename of its own.
        let old = manifest_of("=== hub ===\n= market\nHi.\n-> DONE\n");
        let new = manifest_of("=== plaza ===\n#@was(hub)\n= market\nHi.\n-> DONE\n");

        assert!(
            detect_undeclared_renames(&old, &new).is_empty(),
            "a knot rename already recorded via #@was must not fabricate a stitch suspicion \
             for its unchanged child"
        );
    }

    #[test]
    fn a_knot_rename_and_a_simultaneous_stitch_rename_only_reports_the_knot() {
        // A knot rename and a stitch rename landing in the very same diff
        // step is inherently ambiguous from a `SymbolManifest` diff alone:
        // nothing distinguishes "the author renamed both" from "the author
        // renamed only the knot, and the stitch's qualified name changed as
        // a cascade of that". Per the module's own never-guess principle,
        // only the unambiguous knot rename is reported.
        let old = manifest_of("=== hub ===\n= market\nHi.\n-> DONE\n");
        let new = manifest_of("=== plaza ===\n= bazaar\nHi.\n-> DONE\n");

        let suspicions = detect_undeclared_renames(&old, &new);

        assert_eq!(
            suspicions,
            vec![RenameSuspicion {
                kind: SymbolKind::Knot,
                old_name: "hub".to_owned(),
                new_name: "plaza".to_owned(),
                new_range: new.knots[0].range,
            }],
            "the simultaneous stitch rename is ambiguous and must not be guessed at: \
             {suspicions:?}"
        );
    }
}
