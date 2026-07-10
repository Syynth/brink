//! Behavior-pinning snapshots for `line_contexts` and the fold passes (#463).
//!
//! These snapshots are the safety net for re-expressing `line_context` and
//! `folding`'s structural classification as views over the HIR projection:
//! they pin the exact per-line output (element, weave, tags, block-comment,
//! dialect) plus the structural folds and machinery/narrative runs, for a
//! fixture set covering every `LineElement`/`WeaveElement` case and every
//! known walk-order subtlety. They must remain byte-identical through the
//! refactor — any diff is a behavior change to investigate, not accept.

use std::fmt::Write as _;

use brink_ide::folding::{FoldKind, folding_ranges, machinery_and_narrative_folds};
use brink_ide::line_context::{
    LineContext, WeaveElement, line_contexts, line_contexts_with_dialect,
};
use brink_ir::{FileId, ResolvedDialect, hir};

#[expect(clippy::expect_used, reason = "test helper; preset always compiles")]
fn at_cue_dialect() -> ResolvedDialect {
    ResolvedDialect::compile(&brink_ir::DialogueDialect::default()).expect("at-cue preset compiles")
}

fn render_lines(source: &str, ctx: &[LineContext]) -> String {
    let src_lines: Vec<&str> = source.split('\n').collect();
    let mut out = String::new();
    for (i, c) in ctx.iter().enumerate() {
        let src = src_lines.get(i).copied().unwrap_or("");
        let weave = match c.weave.element {
            WeaveElement::TopLevel => "TopLevel".to_owned(),
            WeaveElement::ChoiceLine { sticky } => format!("ChoiceLine(sticky={sticky})"),
            WeaveElement::ChoiceBody => "ChoiceBody".to_owned(),
            WeaveElement::GatherContinuation => "GatherContinuation".to_owned(),
            WeaveElement::ConditionalBranch => "ConditionalBranch".to_owned(),
            WeaveElement::SequenceBranch => "SequenceBranch".to_owned(),
        };
        let mut extras = String::new();
        if c.has_tags {
            extras.push_str(" tags");
        }
        if c.block_comment {
            extras.push_str(" bc");
        }
        if let Some(d) = &c.dialect {
            let _ = write!(extras, " dialect={}/{:?}", d.kind, d.nature);
            if !d.attrs.is_empty() {
                let attrs: Vec<String> = d.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
                let _ = write!(extras, "[{}]", attrs.join(","));
            }
        }
        let _ = writeln!(
            out,
            "{i:>3} |{src}| {:?} w={}/{weave}{extras}",
            c.element, c.weave.depth
        );
    }
    out
}

fn render_structural_folds(source: &str) -> String {
    let parsed = brink_syntax::parse(source);
    let (hir, _, _) = hir::lower(FileId(0), &parsed.tree());
    let projection = brink_ide::hir_projection::project_hir_structural(&hir, source);
    let mut out = String::new();
    for r in folding_ranges(&hir, source, &projection) {
        let _ = writeln!(
            out,
            "{}..{} from_start={} kind={:?} text={:?}",
            r.start_line, r.end_line, r.from_line_start, r.kind, r.collapsed_text
        );
    }
    out
}

fn render_runs(source: &str, ctx: &[LineContext]) -> String {
    let parsed = brink_syntax::parse(source);
    let (hir, _, _) = hir::lower(FileId(0), &parsed.tree());
    let projection = brink_ide::hir_projection::project_hir_structural(&hir, source);
    let mut out = String::new();
    for r in machinery_and_narrative_folds(&projection, source, ctx) {
        let kind = match r.kind {
            FoldKind::Machinery => "Machinery",
            FoldKind::Narrative => "Narrative",
            FoldKind::Structural => "Structural",
        };
        let _ = writeln!(out, "{}..{} {kind}", r.start_line, r.end_line);
    }
    out
}

/// Render the full pinned surface for one fixture: base line contexts,
/// dialect line contexts (at-cue preset), structural folds, and the
/// machinery/narrative runs for both classification paths.
fn render_fixture(source: &str) -> String {
    let parsed = brink_syntax::parse(source);
    let (hir, _, _) = hir::lower(FileId(0), &parsed.tree());
    let base = line_contexts(&hir, source, &parsed.syntax());
    let dialect = line_contexts_with_dialect(&hir, source, &parsed.syntax(), &at_cue_dialect());

    let mut out = String::new();
    out.push_str("== LINES (base) ==\n");
    out.push_str(&render_lines(source, &base));
    out.push_str("== LINES (at-cue dialect) ==\n");
    out.push_str(&render_lines(source, &dialect));
    out.push_str("== STRUCTURAL FOLDS ==\n");
    out.push_str(&render_structural_folds(source));
    out.push_str("== RUNS (base) ==\n");
    out.push_str(&render_runs(source, &base));
    out.push_str("== RUNS (at-cue dialect) ==\n");
    out.push_str(&render_runs(source, &dialect));
    out
}

macro_rules! snap {
    ($name:ident, $src:expr) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(render_fixture($src));
        }
    };
}

// ── Headers, narrative, blanks ──────────────────────────────────────

snap!(
    headers_and_narrative,
    "\
=== my_knot ===
Some narrative text.

= my_stitch
More text here.
-> END
"
);

snap!(
    function_knot_with_params_and_return,
    "\
== function damage(weapon, bonus) ==
~ temp roll = weapon + bonus
~ return roll
"
);

// ── Choices ─────────────────────────────────────────────────────────

snap!(
    choices_nested_and_sticky,
    "\
=== start ===
* Once-only choice
  Body text here.
* * Nested choice
+ Sticky choice
- gathered
-> END
"
);

snap!(
    choice_with_label_condition_and_tags,
    "\
=== start ===
* (opt) {true} Labeled choice # tagged
  Body line.
-> END
"
);

snap!(
    choice_inline_divert_on_choice_line,
    "\
=== start ===
* [Go] -> hub
* [Stay]
  Fine.
=== hub ===
-> END
"
);

snap!(
    choice_body_blank_lines,
    "\
=== start ===
* Choice one
  First body line.

  Second body line after a blank.
* Choice two

-> END
"
);

snap!(
    choice_bracket_inline_alternatives,
    "\
=== start ===
* [Take the {red|blue} pill]
* [Take the {big|small} dose]
-> END
"
);

snap!(
    choice_nested_inline_logic_in_bracket,
    "\
=== start ===
* [take {a: {b|c}}]
* [drop {d: {e|f}}]
-> END
"
);

// ── Gathers ─────────────────────────────────────────────────────────

snap!(
    gathers_after_choices,
    "\
=== start ===
* [Go back]
- (labeled) Continuation text.
* Another
- bare gather text
* Deeper
* * Nested
- - (deep) nested gather
-
-> END
"
);

snap!(
    labeled_gather_with_inline_divert_continuation,
    "\
=== start ===
* Choice
- (g) -> next
=== next ===
-> END
"
);

snap!(
    labeled_gather_bare_with_empty_continuation,
    "\
=== start ===
* Choice
- (g)
"
);

snap!(
    top_level_labeled_block_with_divert,
    "\
=== start ===
- (top) -> next
=== next ===
-> END
"
);

// ── Diverts, tunnels, threads ───────────────────────────────────────

snap!(
    divert_variants,
    "\
=== start ===
-> hub
=== hub ===
-> tunnel ->
<- threaded
-> DONE
=== tunnel ===
->->
=== threaded ===
-> END
"
);

// ── Logic ───────────────────────────────────────────────────────────

snap!(
    logic_variants,
    "\
VAR gold = 0
=== start ===
~ temp x = 1
~ gold = gold + x
~ do_thing()
-> END
=== function do_thing ===
~ return 0
"
);

// ── Declarations ────────────────────────────────────────────────────

snap!(
    declarations,
    "\
INCLUDE a.ink
INCLUDE b.ink
VAR health = 100
CONST MAX = 10
LIST moods = happy, sad,
    angry
EXTERNAL play_sound(name)
=== start ===
-> END
"
);

// ── Comments, tags ──────────────────────────────────────────────────

snap!(
    comments_and_tags,
    "\
// A line comment
/* a block
   comment spanning
   lines */
/// doc line one
/// doc line two
=== start ===
# standalone_tag
Narrative with a trailing tag. # trailing
-> END
"
);

// ── Conditionals & sequences ────────────────────────────────────────

snap!(
    conditional_if_else_routing,
    "\
=== start ===
{
    - get_variable(16) == 2: -> leave
    - else: -> busy
}
=== leave ===
-> END
=== busy ===
-> END
"
);

snap!(
    conditional_pure_routing_machinery,
    "\
=== start ===
{ x > 5:
~ y = 1
- else:
~ y = 2
}
Hello.
-> END
"
);

snap!(
    conditional_narrative_arms,
    "\
=== start ===
{ busy:
Sorry, I'm quite busy today.
- else:
Come on in, take a seat.
}
-> END
"
);

snap!(
    sequence_multiline,
    "\
=== start ===
{ stopping:
- First visit text.
- Second visit text.
- Every other time.
}
-> END
"
);

snap!(
    inline_conditional_and_sequence_in_narrative,
    "\
=== start ===
Take the {red|blue} pill.
{visited: You were here before.}
You have {gold}
-> END
"
);

snap!(
    choices_inside_conditional_arm,
    "\
=== start ===
{ ready:
    * [Go now]
      Body inside the arm.
    * [Wait]
- else:
    Not ready yet.
}
-> END
"
);

snap!(
    conditional_inside_choice_body,
    "\
=== start ===
* Choice
  { lucky:
  You win.
  - else:
  You lose.
  }
- done
-> END
"
);

// ── Adversarial-review regressions (#463) ───────────────────────────
// Shapes where the span-replay initially diverged from the old walk;
// pinned at the old walk's output (verified by executing 9717e407's
// implementation side by side during review).

// Equal-extent tie: the conditional branch's extent is byte-identical to
// the single choice filling it — the choice (inner, emitted later) must
// win weave derivation: the divert is ChoiceBody, not ConditionalBranch.
snap!(
    equal_extent_choice_in_conditional_arm,
    "\
=== start ===
{ ready:
    * [Go] -> hub
}
=== hub ===
-> END
"
);

// Same tie via a gather: the continuation's extent collapses to exactly
// the nested choice set (`-` has no located content) — the body divert is
// ChoiceBody d1, not GatherContinuation.
snap!(
    equal_extent_nested_choice_after_gather,
    "\
=== start ===
* A
-
* B
  -> hub
=== hub ===
-> END
"
);

// A divert inside an inline conditional in choice bracket text is choice
// text: the choice line must stay Choice, not become Divert.
snap!(
    slot_inline_conditional_divert,
    "\
=== start ===
* [Go {ready: -> hub}]
=== hub ===
-> END
"
);

// A labeled gather following another is lowered as a LabeledBlock inside
// the first's continuation: its label applies in order, so the inline
// divert overwrites it (Divert), unlike a true continuation label.
snap!(
    labeled_block_in_continuation_with_divert,
    "\
=== start ===
* A
- (g) text
- (h) -> hub
=== hub ===
-> END
"
);

// Gather-line prose is ptr-less, so extents must still cover nested
// labels (block_extent includes Block.label): these keep their lexical
// weave (ChoiceBody d1 / GatherContinuation d1), never TopLevel d0.
snap!(
    nested_labeled_gather_inside_choice_body,
    "\
=== start ===
* A
- - (g1) deep gather
- (g2) shallow
-> END
"
);

snap!(
    labeled_block_after_bare_gather_text,
    "\
=== start ===
* A
- g text
- (h) more text
-> END
"
);

snap!(
    labeled_gather_between_gathers_weave,
    "\
=== k ===
* A
- gather text
- (opts)
content under opts
* B
- done
"
);

// A tag inside ptr-less inline-branch content never sets has_tags (the
// old walk's content.ptr gate); the host line's own trailing tag does.
snap!(
    tag_inside_inline_branch_content,
    "\
=== start ===
He said {mood: hi # tag} there.
You have it {now # t2|later # t3}.
Plain with tag. # real
-> END
"
);

// ── Dialect (at-cue preset) ─────────────────────────────────────────

snap!(
    dialect_cue_chain_and_break,
    "\
=== start ===
@Alice:<>
(warmly)<>
Hello there.
Still talking.

After the blank, no chain.
~ change_party_member(2, false)
-> END
"
);

snap!(
    dialect_cue_in_choice_body_and_conditional_arm,
    "\
=== start ===
* Choice
  @Alice:<>
  Choice-body line must not chain.
{ get_variable(17) >= 1:
    @Solstice:<>
    Hello, this is Sols.
- else:
    @Solstice:<>
    Hello?
}
-> END
"
);
