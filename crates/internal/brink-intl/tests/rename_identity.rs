#![allow(clippy::panic, clippy::unwrap_used)]

//! Characterization tests for **identity under rename** — the observable
//! behavior `docs/design/definition-identity-proposals.md` analyzes for
//! issue #1442 (`needs-design`).
//!
//! These tests pin *today's* behavior, including the parts that are the
//! problem. They are evidence for the design writeup, not a statement that
//! the current behavior is desirable: when a maintainer rules on the
//! identity model, tests 1, 2 and 4 below are expected to **flip**, and
//! flipping them is the signal that the ruling landed. Test 3 is a positive
//! control and should keep passing under any option in the writeup.
//!
//! Nothing here asserts an intended design. See the design doc for the
//! options and the two rulings they hang on.

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

/// **4.** The consequence for translators: regeneration matches scopes by id
/// only and never consults the compiled alias table, so a *declared* rename
/// still orphans every translation — including the knot's own, whose
/// migration edge is sitting right there in `StoryData::alias_table`.
#[test]
fn declared_rename_still_orphans_every_translation() {
    let before = compile_story(BEFORE);
    let after = compile_story(KNOT_RENAMED);

    // A fully translated locale file for the pre-rename story.
    let mut translated: LinesJson = brink_intl::export_lines(&before, 0);
    for scope in &mut translated.scopes {
        for line in &mut scope.lines {
            line.content = Some(ContentJson::Plain("TRANSLATED".to_owned()));
        }
    }
    let translated_count = translated
        .scopes
        .iter()
        .flat_map(|s| &s.lines)
        .filter(|l| l.content.is_some())
        .count();
    assert!(translated_count > 0, "fixture must have translated lines");

    let regenerated =
        brink_intl::regenerate_lines(&brink_intl::export_lines(&after, 0), &translated);

    let surviving = regenerated
        .scopes
        .iter()
        .flat_map(|s| &s.lines)
        .filter(|l| l.content.is_some())
        .count();
    assert_eq!(
        surviving, 0,
        "every translation is orphaned by the rename even though \
         `StoryData::alias_table` names the knot's old id"
    );
    assert!(
        !after.alias_table.is_empty(),
        "the migration edge exists — regeneration just never looks at it"
    );
}
