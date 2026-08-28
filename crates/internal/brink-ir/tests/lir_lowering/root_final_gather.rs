// ─── Root weave terminus: implicit final gather + `-> DONE` (#1448) ───
//
// inklecate appends `Gather(null, 1)` + `-> DONE` to the *root* weave
// (`FlowBase.SplitWeaveAndSubFlowContent`, `FlowBase.cs:69-72`) so a branch
// that runs out of root-weave content ends the flow instead of faulting.
// Brink mirrors that with a synthesized `g-final` gather container.
//
// The guard rail is as important as the fix: a knot/stitch/tunnel/function
// whose content runs out is a genuine authoring error that C# ink reports,
// so the terminus must never be attached outside the root scope.

use brink_ir::lir;

use crate::support::*;

/// Does this container's body end with `-> DONE`?
fn ends_with_done(container: &lir::Container) -> bool {
    matches!(
        container.body.last().map(|s| &s.kind),
        Some(lir::StmtKind::Divert(d)) if matches!(d.target, lir::DivertTarget::Done)
    )
}

/// Does this container's body end with a divert to `target`?
fn ends_with_divert_to(container: &lir::Container, target: brink_format::DefinitionId) -> bool {
    matches!(
        container.body.last().map(|s| &s.kind),
        Some(lir::StmtKind::Divert(d)) if matches!(d.target, lir::DivertTarget::Address(a) if a == target)
    )
}

/// Every `g-final` container in the tree, in walk order.
fn final_gathers(container: &lir::Container) -> Vec<&lir::Container> {
    let mut out = Vec::new();
    if container.name.as_deref() == Some("g-final") {
        out.push(container);
    }
    for child in &container.children {
        out.extend(final_gathers(child));
    }
    out
}

#[test]
fn root_weave_loose_end_diverts_to_synthesized_final_gather() {
    let p = lower_ink("* one\n* two\n- gathered\n");
    let r = root(&p);

    let terminus = find_child(r, "g-final");
    assert_eq!(terminus.kind, lir::ContainerKind::Gather);
    assert!(!terminus.labeled);
    assert!(terminus.children.is_empty());
    assert_eq!(
        terminus.body.len(),
        1,
        "the terminus is exactly one `-> DONE`"
    );
    assert!(ends_with_done(terminus));

    // The root weave's loose end (the `- gathered` continuation) now diverts
    // into it instead of running off the end of the container.
    let gather = find_child(r, "g-0");
    assert!(
        ends_with_divert_to(gather, terminus.id),
        "root weave's outermost loose end should divert to the terminus"
    );
}

#[test]
fn root_weave_already_terminated_gets_no_final_gather() {
    // An authored `-> DONE` is not a loose end — synthesizing a terminus here
    // would emit an unreachable container.
    let done = lower_ink("* one\n* two\n- gathered\n-> DONE\n");
    assert!(
        final_gathers(root(&done)).is_empty(),
        "no terminus when the root weave already ends in `-> DONE`"
    );

    let end = lower_ink("* one\n* two\n- gathered\n-> END\n");
    assert!(
        final_gathers(root(&end)).is_empty(),
        "no terminus when the root weave already ends in `-> END`"
    );
}

#[test]
fn root_weave_diverting_elsewhere_gets_no_final_gather() {
    // An authored divert to another knot is not a loose end either — the
    // author took control elsewhere, so synthesizing a terminus here would
    // emit an unreachable container. This exercises the `DivertTarget::
    // Address(_)` arm of `ends_terminal`, distinct from the `Done`/`End`
    // arms covered above.
    let p = lower_ink("* one\n* two\n- gathered\n-> k\n\n=== k ===\nX\n-> DONE\n");
    assert!(
        final_gathers(root(&p)).is_empty(),
        "no terminus when the root weave's loose end already diverts elsewhere"
    );
}

#[test]
fn standalone_gather_wrapper_descends_to_inner_loose_end() {
    // `- (gather) …` lowers to an *inline* wrapper container, entered with
    // `EnterContainer` and therefore not itself a loose end — but the choice
    // set inside it diverts (clearing the container stack) into a
    // continuation gather that is. Regression shape taken from
    // `tests/tier3/misc/visit-count-bug-due-to-nested-containers`.
    let p = lower_ink("- (gather) {gather}\n* choice\n- {gather}\n");
    let r = root(&p);

    let terminus = find_child(r, "g-final");
    assert!(ends_with_done(terminus));

    let wrapper = find_child(r, "gather");
    assert!(
        wrapper.inline,
        "standalone gather lowers to an inline wrapper"
    );
    assert!(
        !ends_with_divert_to(wrapper, terminus.id),
        "the inline wrapper returns to the root body; it must not be patched"
    );

    let inner = find_child(wrapper, "g-1");
    assert!(
        ends_with_divert_to(inner, terminus.id),
        "the wrapper's continuation gather is the real loose end"
    );
}

// ─── Multi-file root content: the terminus is the entry file's (#1502) ───
//
// C# guards the implicit gather with `if (isRootStory)`: an INCLUDEd file is
// parsed as `Story(isInclude: true)` and its root content becomes a nested
// weave *container* in the root, whose trailing gather is entered by divert
// and therefore genuinely runs out of content. Brink lowers root content one
// chunk per file, so the terminus must be attached to the last chunk — the
// entry file's — and to no other, or an included file's trailing weave ends
// the story silently in the wrong place.

/// The root-level gather container whose body emits `text`.
fn root_gather_emitting<'a>(r: &'a lir::Container, text: &str) -> &'a lir::Container {
    r.children
        .iter()
        .find(|c| {
            c.kind == lir::ContainerKind::Gather
                && collect_text(&c.body).iter().any(|t| t.contains(text))
        })
        .unwrap_or_else(|| panic!("no root-level gather emitting {text:?}"))
}

#[test]
fn included_file_trailing_weave_keeps_running_out_of_content() {
    // Included file first, entry file last — `topological_order`'s order.
    let p = lower_ink_files(&[
        "* one\n* two\n- gathered in the included file\n",
        "INCLUDE included.ink\nBack in the entry file.\n",
    ]);
    let r = root(&p);

    assert!(
        final_gathers(r).is_empty(),
        "an INCLUDEd file's trailing weave gets no terminus — C# reports it as \
         `ran out of content`, and silently ending the flow there is worse"
    );
    let gather = root_gather_emitting(r, "gathered in the included file");
    assert!(
        !ends_with_divert(&gather.body),
        "the included file's loose end stays loose"
    );
}

#[test]
fn entry_file_trailing_weave_still_gets_the_terminus() {
    // The #1448 fix must keep working once the project has INCLUDEs: the
    // entry file is the root story, so its own loose end is terminated.
    let p = lower_ink_files(&[
        "A line from the included file.\n",
        "INCLUDE prelude.ink\n* one\n* two\n- gathered in the entry file\n",
    ]);
    let r = root(&p);

    let terminus = find_child(r, "g-final");
    assert!(ends_with_done(terminus));
    let gather = root_gather_emitting(r, "gathered in the entry file");
    assert!(
        ends_with_divert_to(gather, terminus.id),
        "the entry file's loose end diverts to the terminus"
    );
}

#[test]
fn only_the_entry_files_trailing_weave_is_terminated() {
    // Both files end in a loose weave: exactly one terminus, and it belongs
    // to the entry file's weave.
    let p = lower_ink_files(&[
        "* one\n* two\n- gathered in the included file\n",
        "INCLUDE included.ink\n* three\n* four\n- gathered in the entry file\n",
    ]);
    let r = root(&p);

    let all = final_gathers(r);
    assert_eq!(all.len(), 1, "exactly one terminus for the whole program");

    let terminus = find_child(r, "g-final");
    assert!(ends_with_divert_to(
        root_gather_emitting(r, "gathered in the entry file"),
        terminus.id
    ));
    assert!(
        !ends_with_divert(&root_gather_emitting(r, "gathered in the included file").body),
        "the included file's loose end must not be diverted to the entry \
         file's terminus either — that would splice one file's weave onto \
         another's tail"
    );
}

#[test]
fn knot_weave_loose_end_is_left_erroring() {
    // Root scope ONLY. A knot that runs out of content is a genuine error in
    // ink (`divert-choice`, `varying-choice`, I092 all depend on it), so no
    // terminus may be synthesized for it — here the root content is a bare
    // divert, so nothing at all should be added.
    let p = lower_ink("-> k\n\n=== k ===\n* one\n* two\n- gathered\n");
    assert!(
        final_gathers(root(&p)).is_empty(),
        "a knot's loose end must keep running out of content"
    );
}

#[test]
fn root_terminus_does_not_leak_into_a_sibling_knot() {
    // Root weave has a loose end *and* a knot with its own loose end: exactly
    // one terminus, hanging off the root, and the knot's gather untouched.
    let p = lower_ink("* one\n* two\n- gathered\n\n=== k ===\n* three\n* four\n- knot gathered\n");
    let r = root(&p);

    let all = final_gathers(r);
    assert_eq!(all.len(), 1, "exactly one terminus for the whole program");

    let terminus = find_child(r, "g-final");
    assert_eq!(terminus.id, all[0].id);

    let knot = find_child(r, "k");
    assert!(
        final_gathers(knot).is_empty(),
        "the knot subtree gets no terminus"
    );
    let knot_gather = find_child(knot, "g-0");
    assert!(
        !ends_with_divert(&knot_gather.body),
        "the knot's loose end stays loose"
    );
}
