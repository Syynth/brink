//! Prose block elements — scene headings + `[slug]`, block cues, compact
//! cues, parentheticals, header-scoped stitch bodies, and per-flow header
//! tags (#1715; `docs/prose-dialect-spec.md` §8b/§8d).

use super::*;

fn first_node(root: &SyntaxNode, kind: SyntaxKind) -> SyntaxNode {
    // `assert!` + `expect`, not `unwrap_or_else(|| panic!(…))`: this
    // crate's `clippy.toml` exempts `unwrap`/`expect` in tests but
    // `clippy::panic` stays denied everywhere.
    let found = root.descendants().find(|n| n.kind() == kind);
    assert!(found.is_some(), "no {kind:?} node in tree");
    found.expect("asserted present just above")
}

fn tag_texts(node: &SyntaxNode) -> Vec<String> {
    node.children()
        .filter(|n| n.kind() == SyntaxKind::TAG)
        .map(|n| n.text().to_string().trim().to_owned())
        .collect()
}

// ── Scene headings (§8b.3) ───────────────────────────────────────────

#[test]
fn scene_heading_with_slug_and_tags_parses_in_the_ruled_line_order() {
    // The ruled spelling, verbatim from §8b.3: pattern, `[slug]`, tags.
    let src = "INT. MARKET SQUARE - NIGHT [market] #tense #act1\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let heading = first_node(&p.syntax(), SyntaxKind::SCENE_HEADING);
    let heading = ast::SceneHeading::cast(heading).expect("SCENE_HEADING casts");
    assert_eq!(
        heading.title().expect("title").text(),
        "INT. MARKET SQUARE - NIGHT"
    );
    assert_eq!(
        heading
            .slug()
            .expect("slug")
            .name_token()
            .expect("slug ident")
            .text(),
        "market"
    );
    assert_eq!(
        heading
            .tags()
            .map(|t| t.to_string().trim().to_owned())
            .collect::<Vec<_>>(),
        vec!["#tense".to_owned(), "#act1".to_owned()]
    );
}

#[test]
fn ext_prefix_and_a_slugless_heading_are_both_recognized() {
    let src = "EXT. COLD ALLEY - CONTINUOUS\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let heading = ast::SceneHeading::cast(first_node(&p.syntax(), SyntaxKind::SCENE_HEADING))
        .expect("SCENE_HEADING");
    assert_eq!(
        heading.title().expect("title").text(),
        "EXT. COLD ALLEY - CONTINUOUS"
    );
    assert!(
        heading.slug().is_none(),
        "no explicit slug: the address is inferred from the title (spec 3.3)"
    );
}

#[test]
fn a_bracket_in_the_middle_of_a_title_is_not_a_slug() {
    // The slug is recognized only at the tail of the heading; anything
    // else keeps its `[` as ordinary title text.
    let src = "INT. VAULT [b7] - NIGHT\n";
    let p = assert_lossless(src);
    let heading = ast::SceneHeading::cast(first_node(&p.syntax(), SyntaxKind::SCENE_HEADING))
        .expect("SCENE_HEADING");
    assert!(heading.slug().is_none());
    assert_eq!(
        heading.title().expect("title").text(),
        "INT. VAULT [b7] - NIGHT"
    );
}

#[test]
fn a_scene_title_with_an_escaped_hash_does_not_end_the_title_early() {
    // Issue #1738, mirrors `content.rs`'s
    // `a_tag_with_an_escaped_hash_does_not_end_the_tag_early` and
    // `a_cue_name_with_an_escaped_hash_does_not_end_the_name_early`: `#` is
    // one of the four members of the ruled, final inline escape set
    // (§8d.6), but before this fix `scene_title()` gave `\#` zero escape
    // treatment — a bare `HASH` always ended the title, even one
    // immediately preceded by a backslash.
    let src = "INT. MARKET \\#3\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let heading = ast::SceneHeading::cast(first_node(&p.syntax(), SyntaxKind::SCENE_HEADING))
        .expect("SCENE_HEADING");
    assert_eq!(
        heading.title().expect("title").text(),
        "INT. MARKET \\#3",
        "an escaped `#` must not end the title early, and the backslash is \
         not stripped — same precedent `\\{{` and `tag()`/`cue_name()` \
         already established"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::TAG),
        "the escaped `#` must not be reparsed as a trailing TAG"
    );
}

#[test]
fn int_stays_an_ordinary_identifier_away_from_item_position() {
    // The heading prefix is a declared line shape, not a reserved word:
    // `INT` is still a perfectly good binding name.
    let src = "var INT = 1\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::SCENE_HEADING));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::VAR_DECL));
}

// ── Header-scoped stitch bodies (§8b.2) ──────────────────────────────

#[test]
fn a_heading_scopes_the_lines_below_it_without_braces() {
    let src = "INT. A [a]\nOne.\nTwo.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let stitch = ast::SceneStitch::cast(first_node(&p.syntax(), SyntaxKind::SCENE_STITCH))
        .expect("SCENE_STITCH");
    let body = stitch.body().expect("SCENE_BODY");
    let lines: Vec<_> = body
        .items()
        .filter(|n| n.kind() == SyntaxKind::CONTENT_LINE)
        .collect();
    assert_eq!(lines.len(), 2, "both lines belong to the heading's body");
}

#[test]
fn consecutive_headings_are_flat_siblings_never_nested() {
    // "scenes don't nest - as on a real page" (spec 8b.2).
    let src = "INT. A [a]\nOne.\nEXT. B [b]\nTwo.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let root = p.syntax();
    let stitches: Vec<_> = root
        .children()
        .filter(|n| n.kind() == SyntaxKind::SCENE_STITCH)
        .collect();
    assert_eq!(stitches.len(), 2, "two sibling stitches under SOURCE_FILE");
    for stitch in &stitches {
        let nested = stitch
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::SCENE_STITCH)
            .count();
        assert_eq!(nested, 1, "a scene stitch never contains another");
    }
}

#[test]
fn a_doc_comment_above_the_second_heading_keeps_flat_siblings() {
    // Review finding on #1715: `scene_stitch`'s body loop checked
    // `at_scene_heading` directly, which fails on a leading `///` token
    // (`DOC_COMMENT_OUTER` is not trivia), so the loop never broke and
    // `block::item` recursed into a *nested* stitch instead of the outer
    // dispatcher opening a flat sibling with its doc attached. Same
    // fixture as `consecutive_headings_are_flat_siblings_never_nested`,
    // with a `///` run above the second heading only.
    let src = "INT. A [a]\nOne.\n/// doc\nEXT. B [b]\nTwo.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let root = p.syntax();
    let stitches: Vec<_> = root
        .children()
        .filter(|n| n.kind() == SyntaxKind::SCENE_STITCH)
        .collect();
    assert_eq!(stitches.len(), 2, "two sibling stitches under SOURCE_FILE");
    for stitch in &stitches {
        let nested = stitch
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::SCENE_STITCH)
            .count();
        assert_eq!(nested, 1, "a scene stitch never contains another");
    }

    let second = ast::SceneStitch::cast(stitches[1].clone()).expect("SCENE_STITCH");
    assert!(
        second.doc().is_some(),
        "the `///` run attaches to the second (documented) heading, not the first"
    );
}

#[test]
fn a_heading_body_ends_at_the_enclosing_close() {
    // The other delimiter the ruling names: the enclosing close. The
    // `after` line must land back in the flow's own BLOCK, not the scene.
    let src = "flow f() {\n  INT. A [a]\n  Inside.\n}\nAfter.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let stitch = first_node(&p.syntax(), SyntaxKind::SCENE_STITCH);
    assert!(
        !stitch.text().to_string().contains("After."),
        "the scene body stops at the enclosing `}}`: {}",
        stitch.text()
    );
    let block = first_node(&p.syntax(), SyntaxKind::BLOCK);
    assert!(block.text().to_string().contains("Inside."));
}

#[test]
fn deeper_nesting_uses_the_general_flow_spelling_inside_a_scene_body() {
    // "deeper nesting uses the general `flow x { }` spelling, which
    // stays first-class in prose-ground" (spec 8b.2).
    let src = "INT. A [a]\nOne.\nflow inner() {\n  Two.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let stitch = first_node(&p.syntax(), SyntaxKind::SCENE_STITCH);
    assert!(
        stitch
            .descendants()
            .any(|n| n.kind() == SyntaxKind::FLOW_DECL),
        "the nested flow belongs to the scene's header-scoped body"
    );
}

#[test]
fn a_doc_comment_above_a_heading_attaches_to_the_stitch() {
    let src = "/// The market, after curfew.\nINT. A [a]\nOne.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let stitch = ast::SceneStitch::cast(first_node(&p.syntax(), SyntaxKind::SCENE_STITCH))
        .expect("SCENE_STITCH");
    assert!(
        stitch.doc().is_some(),
        "the `///` run wraps as the stitch's doc"
    );
}

// ── Cues: block and compact (§8b.9, §8d.4) ───────────────────────────

#[test]
fn a_block_cue_carries_its_extension_on_the_tag_channel() {
    // "Cue extensions ride the tag channel" (spec 8d.4) - no parsed `ext`
    // capture, no new payload machinery.
    let src = "@VENDOR #(v.o.)\nYou shouldn't be here.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let cue = first_node(&p.syntax(), SyntaxKind::CUE);
    assert_eq!(tag_texts(&cue), vec!["#(v.o.)".to_owned()]);
    let cue = ast::Cue::cast(cue).expect("CUE");
    assert_eq!(cue.name().expect("name").text(), "VENDOR");
}

#[test]
fn a_multi_word_cue_name_is_one_name() {
    let src = "@MARKET VENDOR\nHello.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cue = ast::Cue::cast(first_node(&p.syntax(), SyntaxKind::CUE)).expect("CUE");
    assert_eq!(cue.name().expect("name").text(), "MARKET VENDOR");
}

#[test]
fn a_balanced_brace_in_a_cue_name_does_not_swallow_the_enclosing_blocks_own_closer() {
    // #1786: `cue_name()` shared `content::tag()`'s pre-#1728 shape — an
    // unconditional stop at the first raw `R_BRACE`, regardless of an
    // earlier unpaired `{` in the same scan. Confirmed reachable exactly
    // like the `tag()` case: without the fix, this source's `}` (closing
    // the balanced `{gold}`) was mistaken for the flow's own closer, so
    // the flow's `BLOCK` ended right there, "coins." fell out to the top
    // level, and the flow's real closing `}` became a stray top-level
    // token — a parse error. Depth-tracking the same way `tag()` does
    // fixes it.
    let src = "flow f() {\n  @NAME {gold} coins.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let decl =
        ast::FlowDecl::cast(first_node(&p.syntax(), SyntaxKind::FLOW_DECL)).expect("FLOW_DECL");
    assert!(
        decl.body().is_some(),
        "the cue name's balanced brace must not swallow the flow's own body closer"
    );
    let cue = ast::Cue::cast(first_node(&p.syntax(), SyntaxKind::CUE)).expect("CUE");
    assert_eq!(cue.name().expect("name").text(), "NAME {gold} coins.");
}

#[test]
fn an_unbalanced_open_brace_in_a_cue_name_eats_the_enclosing_blocks_own_closer() {
    // The accepted tradeoff, pinned the same way #1728 pinned it for
    // `tag()` (`content.rs`'s
    // `an_unbalanced_open_brace_in_a_tag_eats_the_enclosing_blocks_own_closer`):
    // a raw, unescaped `{` left open inside a cue name is depth-balanced
    // the same as a matched one — the scan can't tell "unbalanced" from
    // "matches the closer" without a real grammar. So this fails to
    // parse: the name's `{` is counted, the very next `}` is consumed as
    // its match instead of stopping the name, and the flow body's own
    // closer is gone by the time EOF is reached.
    let src = "flow f() { @NAME { }\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected the unbalanced `{{` to consume the flow's own closer and error, got: {:?}",
        p.errors()
    );
}

#[test]
fn an_unbalanced_open_brace_in_a_cue_name_with_a_colon_inside_it_eats_the_enclosing_blocks_own_closer()
 {
    // #1851 widens the same accepted tradeoff pinned just above by
    // `an_unbalanced_open_brace_in_a_cue_name_eats_the_enclosing_blocks_own_closer`
    // from R_BRACE to COLON: once COLON is depth-guarded like R_BRACE, a
    // genuinely unbalanced, unescaped `{` left open in a name swallows not
    // just a later `}` but a later `:` too, along with everything after it
    // up to EOF, since nothing at depth > 0 can stop the scan anymore. So
    // this — balanced-looking at a glance, but the name's own `{` is never
    // closed — fails to parse: the R_BRACE that would close the block is
    // consumed as part of the still-open name scan, and the scan runs off
    // the end of input looking for a match.
    let src = "flow f() { @NAME {x: y }\n";
    let p = assert_lossless(src);
    assert!(
        !p.errors().is_empty(),
        "expected the unbalanced `{{` to swallow the `:` and the flow's own closer, got: {:?}",
        p.errors()
    );
}

#[test]
fn a_cue_name_with_an_escaped_open_brace_does_not_swallow_the_enclosing_blocks_own_closer() {
    // `\{` is the literal-brace escape (#1716/PR #1732), not a
    // metacharacter, so it must not count as a depth-opener — otherwise
    // the escaped brace would swallow the enclosing flow's own same-line
    // closer exactly like the unbalanced-raw-brace case above.
    let src = "flow f() { @NAME \\{ }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn a_cue_name_with_an_escaped_backslash_before_a_real_brace_counts_the_brace() {
    // #1852: `\\{` is an escaped backslash (producing literal `\`), followed
    // by a real interpolation-opening brace. The carve-out that excludes
    // `\{` from the depth counter must not fire for `\\{`, because the
    // backslash is itself escaped. Without the fix, the brace is not counted,
    // so the matching `}` ends the cue name prematurely, and the text after
    // it becomes stray content — a parse error.
    let src = "flow f() { @NAME \\\\{ } coins. }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn a_cue_name_with_a_colon_inside_braces_does_not_terminate_the_name() {
    // #1851: `cue_name()` has a COLON stop that mis-parses `@NAME {a:b}`
    // — the colon inside the braces should not terminate the cue name
    // because the braces create an interpolation context. The depth counter
    // should guard the COLON check the same way it guards the R_BRACE check.
    // Without the fix, the cue name ends at the colon, and the text after
    // it becomes stray content — a parse error.
    let src = "flow f() { @NAME {a:b} }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn a_cue_name_with_an_escaped_hash_does_not_end_the_name_early() {
    // Issue #1738, mirrors `content.rs`'s
    // `a_tag_with_an_escaped_hash_does_not_end_the_tag_early`: `#` is one of
    // the four members of the ruled, final inline escape set (§8d.6), but
    // before this fix `cue_name()` gave `\#` zero escape treatment — a bare
    // `HASH` always ended the name, even one immediately preceded by a
    // backslash.
    let src = "@NAME \\#not a tag\nHello.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let cue = ast::Cue::cast(first_node(&p.syntax(), SyntaxKind::CUE)).expect("CUE");
    assert_eq!(
        cue.name().expect("name").text(),
        "NAME \\#not a tag",
        "an escaped `#` must not end the name early, and the backslash is \
         not stripped — same precedent `\\{{` already established"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::TAG),
        "the escaped `#` must not be reparsed as a trailing TAG"
    );
}

#[test]
fn a_cue_immediately_followed_by_the_enclosing_blocks_own_closer_still_stops_there() {
    // Guard against over-correcting, the same way `content.rs`'s
    // `a_tag_immediately_followed_by_the_enclosing_blocks_own_closer_still_stops_there`
    // guards `tag()`: with no `{` opened inside the cue name's own text,
    // depth stays zero and the very first `}` — here the flow body's own
    // closer — must still terminate the name, exactly as before this fix.
    let src = "flow f() { @NAME }\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn a_cue_name_containing_a_balanced_alternation_brace_does_not_end_early() {
    // Same defect as `a_balanced_brace_in_a_cue_name_does_not_swallow_the_enclosing_blocks_own_closer`,
    // alternation-shaped brace instead of interpolation — parity with
    // `content.rs`'s `a_tag_containing_a_balanced_alternation_brace_does_not_end_early`.
    let src = "flow f() {\n  @NAME {gold|silver} coins.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert_eq!(count_node_kind(&p.syntax(), SyntaxKind::FLOW_DECL), 1);
}

#[test]
fn the_compact_cue_fuses_a_name_and_one_dialogue_line() {
    // Spec 8b.9: a SECOND declared pattern beside the block cue, not a
    // rewrite of it - so it gets its own node kind.
    let src = "@KID: Says who?\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let compact = ast::CompactCue::cast(first_node(&p.syntax(), SyntaxKind::COMPACT_CUE))
        .expect("COMPACT_CUE");
    assert_eq!(compact.name().expect("name").text(), "KID");
    assert_eq!(
        compact.line().expect("fused line").to_string().trim(),
        "Says who?"
    );
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::CUE),
        "the compact form is its own shape, not a CUE plus a line"
    );
}

#[test]
fn a_compact_cue_line_keeps_interpolation_and_trailing_tags() {
    let src = "@KID: I have {gold} coins. #beat\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let compact = first_node(&p.syntax(), SyntaxKind::COMPACT_CUE);
    assert!(has_node_kind(&compact, SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&compact, SyntaxKind::TAG));
}

// ── `!name` sigil dispatch (§3.5b, issue #2004) ──────────────────────

#[test]
fn a_bang_name_line_parses_as_a_bang_dispatch() {
    let src = "!radio TAC-2: All units report in.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let dispatch = ast::BangDispatch::cast(first_node(&p.syntax(), SyntaxKind::BANG_DISPATCH))
        .expect("BANG_DISPATCH");
    assert_eq!(dispatch.name().expect("name").text(), "radio");
    assert_eq!(
        dispatch.line().expect("fused remainder").to_string().trim(),
        "TAC-2: All units report in."
    );
}

#[test]
fn a_bang_dispatch_line_keeps_interpolation_and_trailing_tags() {
    let src = "!radio I have {gold} coins. #beat\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let dispatch = first_node(&p.syntax(), SyntaxKind::BANG_DISPATCH);
    assert!(has_node_kind(&dispatch, SyntaxKind::INTERPOLATION));
    assert!(has_node_kind(&dispatch, SyntaxKind::TAG));
}

#[test]
fn a_bang_not_immediately_followed_by_an_ident_is_still_plain_text() {
    // Adjacency-guarded the same way `@NAME` is (`at_cue`'s own doc): a
    // gap between `!` and the name means this is ordinary exclamation-mark
    // prose, not a dispatch attempt.
    let src = "flow f() {\n  ! Wait, listen.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::BANG_DISPATCH));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

#[test]
fn an_escaped_bang_composes_with_bang_dispatch_and_stays_plain_text() {
    // §8d.6's line-start escape (`\!`, issue #1744/#1978) must still win
    // over the sigil this issue adds — composition, not a collision.
    let src = "flow f() {\n  \\!radio still just prose.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(
        !has_node_kind(&p.syntax(), SyntaxKind::BANG_DISPATCH),
        "an escaped `\\!` must never open a BANG_DISPATCH"
    );
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ESCAPE));
}

#[test]
fn a_lone_at_in_prose_is_still_plain_text() {
    // `SyntaxKind::AT`'s standing promise: the cue sigil is tight, so a
    // detached `@` never claims a line.
    let src = "flow f() {\n  @ home tomorrow\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CUE));
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::COMPACT_CUE));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::CONTENT_LINE));
}

#[test]
fn an_annotation_line_is_not_a_cue() {
    // `@[` is its own token; the two channels cannot collide.
    let src = "@[effects(pure)]\nflow f() {\n  Hi.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::CUE));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::ANNOTATION_LINE));
}

// ── Parentheticals and the chain rule ────────────────────────────────

#[test]
fn a_parenthetical_after_a_cue_is_a_delivery_line() {
    let src = "@VENDOR\n(hushed)\nYou shouldn't be here.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let paren = ast::Parenthetical::cast(first_node(&p.syntax(), SyntaxKind::PARENTHETICAL))
        .expect("PARENTHETICAL");
    assert_eq!(paren.text(), "hushed");
}

#[test]
fn a_parenthetical_after_that_cues_dialogue_is_still_a_delivery_line() {
    // The inventory's chain rule: "(...) line, chain: after cue or
    // dialogue" - the chain survives the dialogue lines under a cue.
    let src = "@VENDOR\nYou shouldn't be here.\n(muttering)\nNot after dark.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::PARENTHETICAL));
}

#[test]
fn a_blank_line_breaks_the_chain_so_a_label_line_stays_a_label() {
    // `brink_ir::dialect`: "blank lines always break a chain".
    let src = "@VENDOR\nYou shouldn't be here.\n\n(loop_back)\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PARENTHETICAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn a_bare_label_line_outside_any_chain_is_untouched() {
    // The shipped G-1 spelling, exactly as
    // `tests/tier1-brink-respell/labeled-mid-flow-gather` writes it.
    let src = "flow f() {\n  (look_around)\n  You look around.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PARENTHETICAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn a_parenthetical_must_fill_its_whole_line() {
    // In a live chain, `(label) text` is still G-1's labeled content line
    // - only a `(...)` that fills the line is a delivery.
    let src = "@VENDOR\n(spot_here) I trudge on.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PARENTHETICAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn a_braced_body_starts_a_fresh_chain() {
    // A cue outside a nested body must not make a `(label)` line inside
    // it a parenthetical.
    let src = "@VENDOR\nflow inner() {\n  (loop_back)\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PARENTHETICAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn a_scene_heading_breaks_the_chain() {
    let src = "@VENDOR\nHello.\nINT. A [a]\n(loop_back)\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(!has_node_kind(&p.syntax(), SyntaxKind::PARENTHETICAL));
    assert!(has_node_kind(&p.syntax(), SyntaxKind::LABEL));
}

#[test]
fn a_multi_word_parenthetical_carries_trailing_tags() {
    let src = "@VENDOR\n(under his breath) #beat\nHello.\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let paren = first_node(&p.syntax(), SyntaxKind::PARENTHETICAL);
    assert_eq!(tag_texts(&paren), vec!["#beat".to_owned()]);
    assert_eq!(
        ast::Parenthetical::cast(paren)
            .expect("PARENTHETICAL")
            .text(),
        "under his breath"
    );
}

// ── Tags on declarations (§8b.4) ─────────────────────────────────────

#[test]
fn a_flow_header_carries_trailing_tags_before_its_body() {
    let src = "flow market(gold) #act1 #tense {\n  Hi.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let decl =
        ast::FlowDecl::cast(first_node(&p.syntax(), SyntaxKind::FLOW_DECL)).expect("FLOW_DECL");
    assert_eq!(
        decl.tags()
            .map(|t| t.to_string().trim().to_owned())
            .collect::<Vec<_>>(),
        vec!["#act1".to_owned(), "#tense".to_owned()]
    );
    assert!(
        decl.body().is_some(),
        "the tag text must not swallow the body brace"
    );
}

#[test]
fn a_paren_less_flow_header_carries_tags_too() {
    let src = "flow market #act1 {\n  Hi.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let decl =
        ast::FlowDecl::cast(first_node(&p.syntax(), SyntaxKind::FLOW_DECL)).expect("FLOW_DECL");
    assert_eq!(decl.tags().count(), 1);
    assert!(decl.body().is_some());
}

#[test]
fn header_tags_do_not_swallow_a_body_dialect_selector() {
    let src = "flow market #act1 ~{\n  let x = 1;\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    assert!(has_node_kind(&p.syntax(), SyntaxKind::STMT_BLOCK));
}

#[test]
fn a_flow_word_starting_a_prose_line_is_still_prose() {
    // Finding #5's firewall, re-checked after widening `at_flow_decl` to
    // accept a `#` third token.
    let src = "flow f() {\n  flow gently #1 and the river bends.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let inner = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FLOW_DECL)
        .count();
    assert_eq!(inner, 1, "the prose line must not parse as a second flow");
}

#[test]
fn a_prose_line_with_an_interpolation_brace_is_still_prose() {
    // Review finding on #1715: a bare "any `{` on the line" lookahead
    // mistook a prose line's own interpolation brace for a declaration
    // body opener. `header_tags_precede_a_body` must require the `{` to
    // be the *last* non-trivia token on the header line.
    //
    // Tightened for #1728: this used to stop short of `assert_lossless` +
    // zero-errors because falling through to prose meant the line's `#1`
    // became an ordinary content-line tag, and `content::tag()`'s
    // free-text scan unconditionally stopped at the first literal `}` it
    // met — including the interpolation's own closer — fooling the
    // enclosing flow's own closing brace. #1728 fixed that scan (it now
    // tracks brace depth so a balanced `}` never terminates the tag
    // early), so this line must now parse completely clean.
    let src = "flow f() {\n  flow gently #1 and {gold} coins.\n  The river bends.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let inner = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FLOW_DECL)
        .count();
    assert_eq!(inner, 1, "the prose line must not parse as a second flow");
}

#[test]
fn a_prose_line_with_an_alternation_brace_is_still_prose() {
    // Same firewall, alternation form of the same review finding. See the
    // interpolation test above — tightened for #1728 the same way.
    let src = "flow f() {\n  flow gently #1 and {river|stream} bends.\n}\n";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());
    let inner = p
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::FLOW_DECL)
        .count();
    assert_eq!(inner, 1, "the prose line must not parse as a second flow");
}

// ── The spec's own complement-pass page ──────────────────────────────

#[test]
fn the_complement_pass_page_parses_cleanly() {
    // docs/prose-dialect-spec.md 8d, "the complement-pass page", trimmed
    // to the shapes this slice owns (no markup spans, no `!name`
    // annotation elements - those are #1716/#1719).
    let src = "\
INT. MARKET SQUARE - NIGHT [market] #act1

The square is empty. A single lantern gutters against the dark.

@VENDOR #(v.o.)
(hushed)
You shouldn't be here after dark. The gates closed an hour ago.

@KID: Says who?

@VENDOR: The curfew, kid. That.

{?
  * \"I was just leaving.\"[] I muttered, backing away.
  * [Slip into the alley] -> alley_escape
}
The bell tolls again.

-> alley_escape

EXT. COLD ALLEY - CONTINUOUS [alley_escape]

Cold brick. Distant bells.
-> DONE
";
    let p = assert_lossless(src);
    assert!(p.errors().is_empty(), "errors: {:?}", p.errors());

    let root = p.syntax();
    assert_eq!(
        root.children()
            .filter(|n| n.kind() == SyntaxKind::SCENE_STITCH)
            .count(),
        2
    );
    assert!(has_node_kind(&root, SyntaxKind::CUE));
    assert!(has_node_kind(&root, SyntaxKind::COMPACT_CUE));
    assert!(has_node_kind(&root, SyntaxKind::PARENTHETICAL));
    assert!(has_node_kind(&root, SyntaxKind::SCENE_SLUG));
    assert!(has_node_kind(&root, SyntaxKind::CHOICE_POINT));
}
