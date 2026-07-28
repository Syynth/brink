//! M-3 renames (`docs/modules-spec.md` §5): `#@was` end-to-end save/rename/
//! load coverage — the flagship module-rename case (fn token, divert value,
//! and visit count together) plus the plain knot-rename case (the
//! pre-existing silent save-break `#@was` retrofits).

use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, Story};

/// Compile brink-dialect source (`#@module`/`#@was` and T1c `#fn`/type
/// annotations are brink-only extensions — see `docs/directive-annotations-spec.md`
/// and `docs/modules-spec.md` §3).
#[expect(clippy::unwrap_used)]
fn compile(src: &str) -> brink_format::StoryData {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options("main.ink", |_p| Ok(src.to_owned()), options)
        .unwrap()
        .data
}

#[expect(clippy::unwrap_used)]
fn link_story(data: &brink_format::StoryData) -> Story<DotNetRng> {
    let (program, line_tables) = brink_runtime::link(data).unwrap();
    Story::new(Arc::new(program), line_tables)
}

/// Run to the next terminal line (Done/Choices/End), collecting text.
#[expect(clippy::unwrap_used)]
fn run_to_terminal(story: &mut Story<DotNetRng>) -> String {
    let mut out = String::new();
    for line in story.continue_maximally().unwrap() {
        match line {
            Line::Text { text, .. }
            | Line::Done { text, .. }
            | Line::End { text, .. }
            | Line::Suspended { text, .. } => {
                out.push_str(&text);
            }
            Line::Choices { text, .. } => out.push_str(&text),
        }
    }
    out
}

/// Flagship end-to-end case (issue #776 / modules-spec §5 §8): save a flow
/// carrying a fn token, a divert-target value, and a visit count — all
/// living in a **declared module** — then rename the module via `#@was`,
/// recompile, and load. Every one of the three saved-state shapes M-3
/// targets must rebind deterministically, with a clean `LoadReport`.
#[test]
fn module_rename_rebinds_fn_token_divert_value_and_visit_count() {
    let v1_src = r"
#@module(quest_3)
VAR target = -> ambush
VAR greeter = #fn(greet)

-> ambush

=== ambush ===
Ambush!
-> DONE

=== function greet(): int ===
~ return 42

=== reader ===
{READ_COUNT(-> ambush)}
-> DONE
";
    let v1_data = compile(v1_src);
    let mut v1 = link_story(&v1_data);
    run_to_terminal(&mut v1);

    let save = v1.save_state();
    assert_eq!(
        save.visits.len(),
        1,
        "ambush should have accrued exactly one visit: {:?}",
        save.visits
    );

    // V2: the module renamed quest_3 -> quest_4, `#@was(quest_3)` recording
    // it. Every declaration keeps its bare name — only the module changed,
    // so every one of its `DefinitionId`s (ambush's address, greet's fn
    // token) shifted along with it. The entry point now goes straight to
    // `reader` (never re-visiting `ambush`), so the printed visit count is
    // purely the *loaded* value, not conflated with a fresh visit.
    let v2_src = r"
#@module(quest_4)
#@was(quest_3)
VAR target = -> ambush
VAR greeter = #fn(greet)

-> reader

=== ambush ===
Ambush!
-> DONE

=== function greet(): int ===
~ return 42

=== reader ===
visits={READ_COUNT(-> ambush)}
divert={target == -> ambush:same|different}
~ temp g = greeter()
fn={g}
-> DONE
";
    let v2_data = compile(v2_src);
    let mut v2 = link_story(&v2_data);

    let report = v2.load_state(&save);
    assert!(
        report.is_clean(),
        "module rename with #@was must rebind cleanly: {report:?}"
    );

    let output = run_to_terminal(&mut v2);
    assert!(
        output.contains("visits=1"),
        "loaded visit count must rebind to the renamed module's ambush: {output:?}"
    );
    assert!(
        output.contains("divert=same"),
        "loaded divert-target value must rebind to the renamed module's ambush: {output:?}"
    );
    assert!(
        output.contains("fn=42"),
        "loaded fn token must rebind and dispatch to the renamed module's greet: {output:?}"
    );
}

/// Declared-module global rename (issue #785, gap 1): a VAR's own bare name
/// changes via `#@was(old_name)` while it lives in a **declared** module —
/// the case PR #782 disclosed as unrehydrated ("`SaveState.globals` is
/// name-keyed... that name carries no module qualifier"). Declared-module
/// identity is `(module, name)`-hashed, so a saved global whose name no
/// longer matches any current slot can't be reasoned about from the bare
/// name string alone; `SaveState::global_ids` round-trips the save-time
/// `DefinitionId` so the miss path can consult the alias table directly.
/// `anchor` is declared first specifically so `#@was(score)` (directly
/// above the renamed `points` declaration) sits *after* a real declaration
/// in the file, not in the file's pure leading directive run — otherwise
/// it would be mis-scanned as a file-level `#@was` (a module rename) rather
/// than a definition-level one (modules-spec §5's `is_file_leading_line`
/// placement rule).
#[test]
fn declared_module_var_rename_rebinds_by_saved_id() {
    let v1_src = r"
#@module(quest_5)
VAR anchor = 0
VAR score = 7

-> reader

=== reader ===
{score}
-> DONE
";
    let v1_data = compile(v1_src);
    let v1 = link_story(&v1_data);
    let save = v1.save_state();
    assert_eq!(
        save.globals.get("score"),
        Some(&brink_format::Value::Int(7)),
        "v1 should save score=7: {:?}",
        save.globals
    );

    // V2: same declared module (`quest_5`, unchanged) — only the VAR's own
    // name changed, `score` -> `points`, recorded with `#@was(score)`
    // directly on the renamed declaration. `points`' own default (99) is
    // deliberately different from the saved value (7) so the assertion
    // below proves the *loaded* value won, not a coincidental default.
    let v2_src = r"
#@module(quest_5)
VAR anchor = 0

#@was(score)
VAR points = 99

-> reader

=== reader ===
{points}
-> DONE
";
    let v2_data = compile(v2_src);
    let mut v2 = link_story(&v2_data);

    let report = v2.load_state(&save);
    assert!(
        report.is_clean(),
        "declared-module VAR rename with #@was must rebind cleanly: {report:?}"
    );

    let output = run_to_terminal(&mut v2);
    assert_eq!(
        output.trim(),
        "7",
        "loaded value must rebind from the renamed declared-module global: {output:?}"
    );
}

/// `Value::List` deep-rebind (issue #785, gap 2): a saved global holding a
/// `LIST` value — active items *and* origins, both `DefinitionId`s — must
/// rebind through a rename exactly like the `Array`/`Map`/`Record` cases
/// `rebind_value` already covered; before this fix `Value::List` fell
/// through to the catch-all `other => other.clone()` arm and its embedded
/// ids stayed stale. Exercised via a **module** rename (renames every
/// definition the module owns, including the `LIST`'s items and its own
/// list-definition id, in one alias-table sweep — the same mechanism the
/// flagship fn-token/divert-target/visit-count case above uses), so a
/// clean `LoadReport` and a correctly-printed list value together prove the
/// list's internal ids — not just its slot's top-level name — rebound.
#[test]
fn module_rename_deep_rebinds_list_value() {
    let v1_src = r"
#@module(quest_7)
LIST loot = sword, shield, potion
VAR held = (sword, potion)

-> reader

=== reader ===
{held}
-> DONE
";
    let v1_data = compile(v1_src);
    let v1 = link_story(&v1_data);
    let save = v1.save_state();

    let v2_src = r"
#@module(quest_8)
#@was(quest_7)
LIST loot = sword, shield, potion
VAR held = ()

-> reader

=== reader ===
{held}
-> DONE
";
    let v2_data = compile(v2_src);
    let mut v2 = link_story(&v2_data);

    let report = v2.load_state(&save);
    assert!(
        report.is_clean(),
        "module rename must deep-rebind a saved Value::List cleanly: {report:?}"
    );

    let output = run_to_terminal(&mut v2);
    assert_eq!(
        output.trim(),
        "sword, potion",
        "loaded LIST value's active items must rebind to the renamed module's list: {output:?}"
    );
}

/// The plain knot-rename case (modules-spec §5, §8): renaming a bare knot —
/// no module involved — is the pre-existing silent save-break `#@was`
/// retrofits. A save from before the rename must still rebind after it.
#[test]
fn knot_rename_rebinds_visit_count_and_divert_value() {
    let v1_src = r"
VAR target = -> old_name

-> old_name

=== old_name ===
Here.
-> DONE

=== reader ===
{READ_COUNT(-> old_name)}
-> DONE
";
    let v1_data = compile(v1_src);
    let mut v1 = link_story(&v1_data);
    run_to_terminal(&mut v1);

    let save = v1.save_state();
    assert_eq!(save.visits.len(), 1, "old_name should have one visit");

    let v2_src = r"
VAR target = -> new_name

-> reader

=== new_name ===
#@was(old_name)
Here.
-> DONE

=== reader ===
visits={READ_COUNT(-> new_name)}
divert={target == -> new_name:same|different}
-> DONE
";
    let v2_data = compile(v2_src);
    let mut v2 = link_story(&v2_data);

    let report = v2.load_state(&save);
    assert!(
        report.is_clean(),
        "knot rename with #@was must rebind cleanly: {report:?}"
    );

    let output = run_to_terminal(&mut v2);
    assert!(
        output.contains("visits=1"),
        "loaded visit count must rebind to the renamed knot: {output:?}"
    );
    assert!(
        output.contains("divert=same"),
        "loaded divert-target value must rebind to the renamed knot: {output:?}"
    );
}

/// The transitive case #1671 fixes: renaming a knot re-keys every stitch
/// beneath it too, because the stitch's qualified name embeds the knot's
/// name. A save taken while a *descendant stitch* was visited must still
/// rebind that stitch's own visit count after the knot's `#@was` rename —
/// not just the knot-level visit count the sibling test above covers.
#[test]
fn knot_rename_rebinds_descendant_stitch_visit_count_and_divert_value() {
    let v1_src = r"
VAR target = -> old_name.market

-> old_name.market

=== old_name ===
Here.
-> DONE

= market
Market.
-> DONE

=== reader ===
{READ_COUNT(-> old_name.market)}
-> DONE
";
    let v1_data = compile(v1_src);
    let mut v1 = link_story(&v1_data);
    run_to_terminal(&mut v1);

    let save = v1.save_state();
    assert_eq!(
        save.visits.len(),
        1,
        "old_name.market should have one visit"
    );

    let v2_src = r"
VAR target = -> new_name.market

-> reader

=== new_name ===
#@was(old_name)
Here.
-> DONE

= market
Market.
-> DONE

=== reader ===
visits={READ_COUNT(-> new_name.market)}
divert={target == -> new_name.market:same|different}
-> DONE
";
    let v2_data = compile(v2_src);
    let mut v2 = link_story(&v2_data);

    let report = v2.load_state(&save);
    assert!(
        report.is_clean(),
        "knot rename with #@was must transitively rebind a descendant stitch's visit count: {report:?}"
    );

    let output = run_to_terminal(&mut v2);
    assert!(
        output.contains("visits=1"),
        "loaded descendant stitch visit count must rebind to the renamed knot's stitch: {output:?}"
    );
    assert!(
        output.contains("divert=same"),
        "loaded divert-target value must rebind to the renamed knot's descendant stitch: {output:?}"
    );
}

/// Without `#@was`, the same knot rename is the pre-existing silent
/// save-break: the saved visit count and divert-target value are simply
/// orphaned under the old id — no crash, but no rebind either (this is the
/// regression M-3 fixes; pinning it down proves the `#@was` tests above are
/// actually exercising the fix, not a scenario that always worked).
#[test]
fn knot_rename_without_was_stays_orphaned() {
    let v1_src = r"
VAR target = -> old_name

-> old_name

=== old_name ===
Here.
-> DONE

=== reader ===
{READ_COUNT(-> old_name)}
-> DONE
";
    let v1_data = compile(v1_src);
    let mut v1 = link_story(&v1_data);
    run_to_terminal(&mut v1);
    let save = v1.save_state();

    let v2_src = r"
VAR target = -> new_name

-> reader

=== new_name ===
Here.
-> DONE

=== reader ===
visits={READ_COUNT(-> new_name)}
divert={target == -> new_name:same|different}
-> DONE
";
    let v2_data = compile(v2_src);
    let mut v2 = link_story(&v2_data);
    let _report = v2.load_state(&save);

    let output = run_to_terminal(&mut v2);
    assert!(
        output.contains("visits=0"),
        "without #@was the visit count stays orphaned under the old id: {output:?}"
    );
    assert!(
        output.contains("divert=different"),
        "without #@was the divert-target value stays the stale id: {output:?}"
    );
}
