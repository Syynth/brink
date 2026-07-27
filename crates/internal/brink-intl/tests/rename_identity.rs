#![allow(clippy::panic, clippy::unwrap_used)]

//! Characterization tests for **identity under rename** — the observable
//! behavior `docs/design/definition-identity-proposals.md` analyzes for
//! issue #1442.
//!
//! Tests 1–3 pin *today's* behavior, including the parts that are still the
//! problem: R1 (ruled 2026-07-27, PR #1670) kept identity name-derived with
//! `#@was` as the sole migration edge, so id churn under a rename is by
//! design and the shallow rename net is tracked separately as #1671.
//!
//! Test 4 **flipped** when #1442 landed: regeneration and locale compilation
//! now consult the compiled alias table, so a *declared* rename carries its
//! own translations across. Test 5 pins the residue — the transitive gap
//! #1671 owns — and test 6 covers the `compile-locale` half.
//!
//! See `docs/design/definition-identity-proposals.md` for the analysis and
//! `docs/intl-spec.md` for the resulting matching rules.

use std::collections::BTreeSet;

use brink_intl::{ContentJson, LinesJson};

fn compile_story(src: &str) -> brink_format::StoryData {
    // `#@was` is a brink-dialect extension (`E051` under strict ink), so the
    // rename cases need `Dialect::Brink`.
    let options = brink_compiler::AnalysisOptions {
        dialect: brink_compiler::Dialect::Brink,
        ..brink_compiler::AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("story.ink", |_p| Ok(src.to_owned()), options)
        .unwrap()
        .data
}

/// The same story, serialized as the `.inkb` bytes `compile-locale` consumes
/// — the real base artifact, alias-table section and all.
fn compile_inkb(src: &str) -> Vec<u8> {
    let data = compile_story(src);
    let mut inkb = Vec::new();
    brink_format::write_inkb(&data, &mut inkb);
    inkb
}

/// A knot with one stitch. Both are lexical scopes, so both get their own
/// line table — i.e. both are translation units in the XLIFF export.
const BEFORE: &str = "\
== hub ==
Welcome to the hub.
+ [Shop] The stalls are busy.
- You move on.
-> END

= market
Fish, mostly.
-> END
";

/// [`BEFORE`] with the *knot* renamed and the rename declared with `#@was`.
/// Nothing else changed — every line of prose is byte-identical.
const KNOT_RENAMED: &str = "\
== plaza ==
#@was(hub)
Welcome to the hub.
+ [Shop] The stalls are busy.
- You move on.
-> END

= market
Fish, mostly.
-> END
";

/// [`BEFORE`] with the *stitch* renamed and the rename declared with `#@was`.
const STITCH_RENAMED: &str = "\
== hub ==
Welcome to the hub.
+ [Shop] The stalls are busy.
- You move on.
-> END

= bazaar
#@was(market)
Fish, mostly.
-> END
";

/// Scope ids of every exported translation unit, as the hex strings that
/// become XLIFF `brink:scope-id` / `<unit id>` prefixes.
fn scope_ids(story: &brink_format::StoryData) -> BTreeSet<String> {
    brink_intl::export_lines(story, 0)
        .scopes
        .into_iter()
        .map(|s| s.id)
        .collect()
}

/// The scope id of the named scope (`"hub"`, `"hub.market"`, …).
fn scope_id_of(story: &brink_format::StoryData, name: &str) -> String {
    brink_intl::export_lines(story, 0)
        .scopes
        .into_iter()
        .find(|s| s.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no exported scope named `{name}`"))
        .id
}

fn hex(id: brink_format::DefinitionId) -> String {
    format!("0x{:016x}", id.to_raw())
}

/// **1.** A pure rename churns the id of *every* scope beneath it, so every
/// XLIFF unit id under the renamed knot changes — which is the churn #1442
/// was filed about, and which PR #1594's `{scope_id}:{index}` scheme did not
/// fix (a `DefinitionId` is a hash of the qualified name).
#[test]
fn knot_rename_churns_every_scope_id_beneath_it() {
    let before = scope_ids(&compile_story(BEFORE));
    let after = scope_ids(&compile_story(KNOT_RENAMED));

    assert_eq!(
        before.len(),
        2,
        "expected the knot and its stitch: {before:?}"
    );
    assert_eq!(
        after.len(),
        2,
        "expected the knot and its stitch: {after:?}"
    );
    assert!(
        before.is_disjoint(&after),
        "renaming `hub` -> `plaza` re-keys both scopes; before={before:?} after={after:?}"
    );
}

/// **2.** `#@was` on the knot mints exactly **one** alias entry — the knot's
/// own. The stitch's pre-rename id is not aliased, even though the stitch was
/// not renamed and its own qualified name only changed because its *parent*
/// did. The rename net is therefore shallow while the churn is transitive:
/// the stitch's saved visit count and its translations have no migration
/// path at all.
#[test]
fn knot_was_aliases_the_knot_but_not_its_stitch() {
    let before = compile_story(BEFORE);
    let after = compile_story(KNOT_RENAMED);

    let old_knot = scope_id_of(&before, "hub");
    let new_knot = scope_id_of(&after, "plaza");
    let old_stitch = scope_id_of(&before, "hub.market");

    let aliases: Vec<(String, String)> = after
        .alias_table
        .iter()
        .map(|a| (hex(a.old), hex(a.new)))
        .collect();

    assert!(
        aliases.contains(&(old_knot.clone(), new_knot.clone())),
        "`#@was(hub)` must alias the knot's own id; aliases={aliases:?}"
    );
    assert!(
        !aliases.iter().any(|(old, _)| old == &old_stitch),
        "today the stitch `hub.market` -> `plaza.market` gets no alias entry, \
         so its durable state is unrecoverable; aliases={aliases:?}"
    );
    assert_eq!(
        aliases.len(),
        1,
        "one declaration renamed, one alias entry — the relation is not \
         transitive; aliases={aliases:?}"
    );
}

/// **3.** Positive control: a *stitch-level* `#@was` does work, because the
/// HIR lowering qualifies the old name with the enclosing knot before it is
/// hashed. So the gap in test 2 is specifically the **transitive** case (an
/// ancestor renamed), not a broken `#@was`.
#[test]
fn stitch_was_aliases_the_qualified_pre_rename_id() {
    let before = compile_story(BEFORE);
    let after = compile_story(STITCH_RENAMED);

    let old_stitch = scope_id_of(&before, "hub.market");
    let new_stitch = scope_id_of(&after, "hub.bazaar");
    let aliases: Vec<(String, String)> = after
        .alias_table
        .iter()
        .map(|a| (hex(a.old), hex(a.new)))
        .collect();

    assert!(
        aliases.contains(&(old_stitch.clone(), new_stitch.clone())),
        "`#@was(market)` on the stitch must alias `hub.market` -> `hub.bazaar`; \
         aliases={aliases:?}"
    );
    // The knot itself is untouched, so its id is stable across this edit.
    assert_eq!(scope_id_of(&before, "hub"), scope_id_of(&after, "hub"));
}

/// **3b.** …and a stitch-level `#@was` is *not* an author-side workaround for
/// the transitive gap in test 2. Declaring `#@was(market)` on an unrenamed
/// stitch inside a renamed knot qualifies the old name with the knot's
/// **current** name (`plaza.market`), which is the stitch's new id — a no-op
/// self-edge, not the `hub.market -> plaza.market` bridge an author would
/// expect. So #1671 is the only path for a renamed subtree; there is nothing
/// to hand-write today.
#[test]
fn stitch_was_cannot_bridge_an_ancestor_rename() {
    const KNOT_RENAMED_STITCH_REDECLARED: &str = "\
== plaza ==
#@was(hub)
Welcome to the hub.
-> END

= market
#@was(market)
Fish, mostly.
-> END
";
    let before = compile_story(BEFORE);
    let after = compile_story(KNOT_RENAMED_STITCH_REDECLARED);

    let aliases: Vec<(String, String)> = after
        .alias_table
        .iter()
        .map(|a| (hex(a.old), hex(a.new)))
        .collect();

    assert_eq!(
        aliases,
        vec![(scope_id_of(&before, "hub"), scope_id_of(&after, "plaza"))],
        "the knot's edge is the only one minted; the stitch's `#@was` \
         resolves to its own new id and contributes nothing"
    );
    assert!(
        !aliases
            .iter()
            .any(|(old, _)| old == &scope_id_of(&before, "hub.market")),
        "no edge names the stitch's pre-rename id; aliases={aliases:?}"
    );
}

/// A fully translated locale file for `story`: every exported line gets the
/// same marker translation, so "did this survive" is a simple count.
fn fully_translated(story: &brink_format::StoryData) -> LinesJson {
    let mut translated: LinesJson = brink_intl::export_lines(story, 0);
    for scope in &mut translated.scopes {
        for line in &mut scope.lines {
            line.content = Some(ContentJson::Plain("TRANSLATED".to_owned()));
        }
    }
    assert!(
        translated
            .scopes
            .iter()
            .flat_map(|s| &s.lines)
            .any(|l| l.content.is_some()),
        "fixture must have translated lines"
    );
    translated
}

/// How many lines of `lines` carry a translation, within the scope whose id
/// is `scope_id`.
fn translated_in_scope(lines: &LinesJson, scope_id: &str) -> usize {
    lines
        .scopes
        .iter()
        .filter(|s| s.id == scope_id)
        .flat_map(|s| &s.lines)
        .filter(|l| l.content.is_some())
        .count()
}

/// **4.** #1442: regeneration now consults `StoryData::alias_table`, so the
/// renamed knot's own translations survive a *declared* rename instead of
/// being dropped on the floor. This test previously asserted the opposite.
#[test]
fn declared_rename_carries_the_renamed_scopes_translations_across() {
    let before = compile_story(BEFORE);
    let after = compile_story(KNOT_RENAMED);
    assert!(
        !after.alias_table.is_empty(),
        "`#@was(hub)` must mint the migration edge this test exercises"
    );

    let translated = fully_translated(&before);
    let old_knot = scope_id_of(&before, "hub");
    let expected = translated_in_scope(&translated, &old_knot);
    assert!(expected > 0, "the knot must have translated lines");

    let regenerated = brink_intl::regenerate_lines(
        &brink_intl::export_lines(&after, 0),
        &translated,
        &after.alias_table,
    );

    let new_knot = scope_id_of(&after, "plaza");
    assert_eq!(
        translated_in_scope(&regenerated, &new_knot),
        expected,
        "`#@was(hub)` must rebind `hub`'s translations onto `plaza`"
    );
}

/// **5.** The residue, owned by #1671: `#@was` on the knot mints exactly one
/// alias entry (test 2), so the *stitch* beneath it — re-keyed only because
/// its parent's name changed — has no edge to rebind through and its
/// translations are still orphaned. Alias-awareness can only carry what the
/// alias table records.
#[test]
fn transitive_rename_still_orphans_the_stitch() {
    let before = compile_story(BEFORE);
    let after = compile_story(KNOT_RENAMED);

    let translated = fully_translated(&before);
    let old_stitch = scope_id_of(&before, "hub.market");
    assert!(
        translated_in_scope(&translated, &old_stitch) > 0,
        "the stitch must have translated lines"
    );

    let regenerated = brink_intl::regenerate_lines(
        &brink_intl::export_lines(&after, 0),
        &translated,
        &after.alias_table,
    );

    let new_stitch = scope_id_of(&after, "plaza.market");
    assert_eq!(
        translated_in_scope(&regenerated, &new_stitch),
        0,
        "the stitch has no alias entry (see #1671), so it cannot rebind"
    );
}

/// **6.** The `compile-locale` half of #1442: a stale locale file carrying
/// pre-rename scope ids used to be a hard `IntlError::ScopeNotInBase`. It now
/// rebinds through the base `.inkb`'s own alias table and compiles.
#[test]
fn compile_locale_rebinds_a_stale_locale_file_through_the_alias_table() {
    let before = compile_story(BEFORE);
    let after_inkb = compile_inkb(KNOT_RENAMED);

    // Only the renamed knot — the stitch has no alias edge (test 5), and an
    // unrebindable scope is still `ScopeNotInBase`.
    let mut translated = fully_translated(&before);
    let old_knot = scope_id_of(&before, "hub");
    translated.scopes.retain(|s| s.id == old_knot);
    assert_eq!(translated.scopes.len(), 1);

    let inkl = brink_intl::compile_locale(&after_inkb, &translated, "es")
        .expect("a declared rename must not orphan the locale file");
    let locale = brink_format::read_inkl(&inkl).unwrap();

    let after = compile_story(KNOT_RENAMED);
    let new_knot = scope_id_of(&after, "plaza");
    let bound: Vec<String> = locale.line_tables.iter().map(|t| hex(t.scope_id)).collect();
    assert_eq!(
        bound,
        vec![new_knot],
        "the overlay must be keyed on the post-rename id"
    );
}
