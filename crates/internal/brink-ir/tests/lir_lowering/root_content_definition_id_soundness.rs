// ─── Root-content DefinitionId soundness (#1504) ─────────────────────
//
// `lower_root_content_chunks` lowers *every* file's root-level weave under
// the same empty scope path (`mod.rs:487` hands `String::new()` to
// `make_ctx` unconditionally), and `IdAllocator::alloc_address`
// (`context.rs:392`) is a pure hash of that path. So the first choice of
// file A's root weave and the first choice of file B's root weave both
// hash the path `c-0` and receive the **same** `DefinitionId`.
//
// `DefinitionId` is the address key (`linker.rs:88`, last-write-wins) and
// the save key for visit/turn counts (`save.rs:113`), so this is a
// miscompile, not merely a cosmetic clash — see
// `docs/root-content-identity-findings.md` for the full analysis and the
// end-to-end demonstration in
// `brink-compiler/tests/issue_1504_root_content_identity.rs`.
//
// Both tests below are **acceptance tests for the fix**, not
// characterization tests: they assert the behavior brink should have, and
// are `#[ignore]`d because the fix shape is blocked on the FG-4d identity
// ruling (#1504 is labeled `needs-design`, and #1442's owner comment asks
// for one answer across #1504/#1442/`@[was]`). Un-ignoring them is the
// acceptance criterion. Do **not** convert them into assertions of the
// current (wrong) behavior — that would bless the bug.

use std::collections::BTreeMap;

use brink_ir::lir;

use crate::support::*;

/// Every container id in the tree, paired with its name, in walk order.
fn collect_ids(container: &lir::Container, out: &mut Vec<(brink_format::DefinitionId, String)>) {
    out.push((
        container.id,
        container.name.as_deref().unwrap_or("(anon)").to_string(),
    ));
    for child in &container.children {
        collect_ids(child, out);
    }
}

/// The single `g-final` terminus id, if one was synthesized.
fn terminus_id(program: &lir::Program) -> Option<brink_format::DefinitionId> {
    fn walk(c: &lir::Container) -> Option<brink_format::DefinitionId> {
        if c.name.as_deref() == Some("g-final") {
            return Some(c.id);
        }
        c.children.iter().find_map(walk)
    }
    walk(root(program))
}

/// #1504(a): two files' root weaves must not share container ids.
///
/// Measured on `origin/main` (commit 999581354): the 8-container tree
/// carries three collided ids — `c-0`, `c-1` and `g-0` each appear twice,
/// once per file.
///
/// Uses [`lower_ink_files_with_paths`] rather than [`lower_ink_files`] —
/// the recommended fix (`docs/root-content-identity-findings.md`,
/// "Recommended shape") qualifies a root-content scope path by the owning
/// file's identity. `lower_ink_files` always hands lowering an *empty*
/// `file_paths` map (`support.rs`), so under that fix both files would
/// still resolve to the same (absent) qualifier and this test would keep
/// failing for a reason unrelated to the bug it documents. Distinct real
/// paths give the fix something to key on once it lands.
#[test]
#[ignore = "known bug #1504(a): root-content scope paths are unqualified by \
            file, so two files' root weaves collide; fix is blocked on the \
            FG-4d identity ruling"]
fn root_content_ids_are_distinct_across_files() {
    let p = lower_ink_files_with_paths(&[
        ("alpha.ink", "* alpha one\n* alpha two\n- alpha gathered\n"),
        ("beta.ink", "* beta one\n* beta two\n- beta gathered\n"),
    ]);

    let mut ids = Vec::new();
    collect_ids(root(&p), &mut ids);

    let mut seen: BTreeMap<brink_format::DefinitionId, Vec<String>> = BTreeMap::new();
    for (id, name) in &ids {
        seen.entry(*id).or_default().push(name.clone());
    }
    let collisions: Vec<String> = seen
        .iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(id, names)| format!("{id:?} -> {names:?}"))
        .collect();

    assert!(
        collisions.is_empty(),
        "two files' root weaves share DefinitionIds: {collisions:#?}",
    );
}

/// #1504(b): the synthesized root terminus address must be content-derived.
///
/// `attach_root_final_gather` keys it `#root-terminus.{file_id}`
/// (`mod.rs:1957`) — the only `alloc_address` call in `brink-ir` keyed by
/// `FileId` rather than a scope path. This test proves that sensitivity at
/// the LIR-lowering level: [`lower_ink_files`] mints each source's `FileId`
/// strictly from its position in the `sources` slice (`support.rs`), so
/// prepending an extra source shifts the entry's numeric id and moves the
/// terminus address, even though the entry's own text is byte-identical.
///
/// This does **not** prove the same thing happens on any real user action —
/// see `docs/root-content-identity-findings.md`'s "Correcting the framing
/// of (b)" section: `discover`'s BFS always seeds the entry first, so an
/// ordinary from-scratch compile always mints the entry `FileId(0)`
/// regardless of how many files it `INCLUDE`s. The reachable form of this
/// bug is editor/LSP file-registration order, not `INCLUDE` count, and is
/// covered separately by
/// `brink-driver`'s `root_content_ids_agree_between_discover_and_editor_order`
/// (`crates/internal/brink-driver/src/discover.rs`).
#[test]
#[ignore = "known bug #1504(b): the terminus address is keyed by FileId, so \
            its position in the file list moves it; fix is blocked on the \
            FG-4d identity ruling"]
fn root_terminus_address_is_independent_of_file_id_assignment() {
    // Same entry file, same root weave — only the number of unrelated
    // files ahead of it in the lowered file list differs.
    let entry = "* one\n* two\n- gathered\n";
    let unrelated = "=== helper ===\nhelper text\n-> DONE\n";

    let one_include = terminus_id(&lower_ink_files(&[unrelated, entry]));
    let two_includes = terminus_id(&lower_ink_files(&[unrelated, unrelated, entry]));

    assert!(one_include.is_some(), "expected a synthesized terminus");
    assert_eq!(
        one_include, two_includes,
        "the root terminus address moved when the entry's FileId assignment shifted",
    );
}
