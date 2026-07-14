//! M-2b: per-definition visibility (`#@private`) is compiled into
//! `StoryData.private_defs`, the format surface the runtime consults to refuse
//! host semantic access. See `docs/modules-spec.md` §4.
#![allow(clippy::unwrap_used)]

use brink_compiler::{AnalysisOptions, Dialect, compile_with_options};

fn brink_options() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

/// The `DefinitionId` of the global named `name`, or `None` if undeclared.
fn global_id(data: &brink_format::StoryData, name: &str) -> Option<brink_format::DefinitionId> {
    data.variables
        .iter()
        .find(|g| data.name_table[g.name.0 as usize] == name)
        .map(|g| g.id)
}

#[test]
fn private_var_lands_in_story_data() {
    // An undeclared stem-module defaults public; a per-definition `#@private`
    // overrides (declaration-flips-default, §4). `secret` is private, `shown`
    // stays public.
    let src = "#@private\n\
               VAR secret = 5\n\
               VAR shown = 1\n\
               == start ==\n\
               Hi\n\
               -> DONE\n";
    let data = compile_with_options("story.ink", |_p| Ok(src.to_owned()), brink_options())
        .unwrap()
        .data;

    let secret = global_id(&data, "secret").expect("secret declared");
    let shown = global_id(&data, "shown").expect("shown declared");

    assert!(
        data.private_defs.contains(&secret),
        "private VAR must be enumerated; private_defs = {:?}",
        data.private_defs
    );
    assert!(
        !data.private_defs.contains(&shown),
        "#@public VAR must not be enumerated"
    );

    // Determinism: the compiler emits the set sorted ascending by raw id.
    let mut sorted = data.private_defs.clone();
    sorted.sort_by_key(|id| id.to_raw());
    assert_eq!(data.private_defs, sorted);
}

#[test]
fn all_public_story_has_no_private_defs() {
    // The pre-modules world: no declared module, no `#@private` — the set is
    // empty and (per the writer) the `.inkb` Visibility section is omitted.
    let src = "VAR v = 1\n== start ==\nHi\n-> DONE\n";
    let data = compile_with_options("story.ink", |_p| Ok(src.to_owned()), brink_options())
        .unwrap()
        .data;
    assert!(data.private_defs.is_empty());
}
