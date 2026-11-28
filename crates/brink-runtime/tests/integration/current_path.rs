//! `Story::current_path` (#3389 follow-up, ruled 2026-09-02): the knot or
//! `knot.stitch` the story is executing in — ink's `currentPathString`
//! without the weave indices — as a QUERY the host reads around each line.

use brink_runtime::{FastRng, Step, Story};

type Res<T> = Result<T, Box<dyn std::error::Error>>;
/// `(text, path before the continue, path after it)`.
type Seen = (String, Option<String>, Option<String>);

fn story_from_source(src: &str) -> Res<Story<FastRng>> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))?.data;
    let (program, line_tables) = brink_runtime::link(&data)?;
    Ok(Story::new(std::sync::Arc::new(program), line_tables))
}

/// Every delivered line paired with the path read BEFORE and AFTER the
/// continue that delivered it.
fn walk(story: &mut Story<FastRng>) -> Res<Vec<Seen>> {
    let mut seen = Vec::new();
    loop {
        let before = story.current_path();
        let step = story.continue_single()?;
        let after = story.current_path();
        match step {
            Step::Line(line) => seen.push((line.text.trim().to_owned(), before, after)),
            Step::End | Step::Done | Step::Choices(_) | Step::Suspended => break,
        }
    }
    Ok(seen)
}

#[test]
fn current_path_reports_where_the_story_is_like_ink() -> Res<()> {
    let src = "-> top\n=== top ===\nOne.\n-> inner\n= inner\nTwo.\n-> elsewhere\n=== elsewhere ===\nThree.\n-> END\n";
    let mut story = story_from_source(src)?;
    assert_eq!(
        story.current_path(),
        None,
        "the root scope is no named container"
    );
    let seen = walk(&mut story)?;
    let texts: Vec<&str> = seen.iter().map(|(t, ..)| t.as_str()).collect();
    assert_eq!(texts, vec!["One.", "Two.", "Three."]);
    // AFTER a line the VM sits at the start of the NEXT content — ink's
    // `currentPathString` semantics.
    let after: Vec<Option<&str>> = seen.iter().map(|(_, _, a)| a.as_deref()).collect();
    assert_eq!(
        after,
        vec![Some("top.inner"), Some("elsewhere"), Some("elsewhere")]
    );
    // BEFORE a line is therefore where that line comes from (the first
    // line of a run from the root reads None).
    let before: Vec<Option<&str>> = seen.iter().map(|(_, b, _)| b.as_deref()).collect();
    assert_eq!(before, vec![None, Some("top.inner"), Some("elsewhere")]);
    Ok(())
}

#[test]
fn current_path_after_choose_path_string() -> Res<()> {
    let src = "=== a ===\nA line.\n-> DONE\n=== b ===\nB line.\nStill b.\n-> DONE\n";
    let mut story = story_from_source(src)?;
    story.choose_path_string("b")?;
    let _ = story.continue_single()?;
    assert_eq!(story.current_path().as_deref(), Some("b"));
    Ok(())
}

/// After `choose`, the frame holds only the chosen branch — an unnamed
/// choice body. The story is still in the knot that offered the choice,
/// and the query says so (found live 2026-09-02: the first line after a
/// choice read as "nowhere", which hid the knot change that followed).
#[test]
fn current_path_after_a_choice_is_the_offering_knot() -> Res<()> {
    let src = "-> hall\n=== hall ===\nA voice.\n* [Answer]\n    Wrong.\n    -> hub\n=== hub ===\nBack in the hub.\n-> END\n";
    let mut story = story_from_source(src)?;
    let _ = walk(&mut story)?; // runs to the choice point
    assert_eq!(story.current_path().as_deref(), Some("hall"));
    story.choose(0)?;
    assert_eq!(
        story.current_path().as_deref(),
        Some("hall"),
        "the chosen branch's body belongs to the knot that offered it"
    );
    let seen = walk(&mut story)?;
    let before: Vec<Option<&str>> = seen
        .iter()
        .map(|(t, b, _)| (t, b))
        .map(|(_, b)| b.as_deref())
        .collect();
    let texts: Vec<&str> = seen.iter().map(|(t, ..)| t.as_str()).collect();
    assert_eq!(texts, vec!["Wrong.", "Back in the hub."]);
    assert_eq!(before, vec![Some("hall"), Some("hub")]);
    Ok(())
}

/// Every external takes its ink fallback body — a peek never calls the host.
struct Fallback;
impl brink_runtime::ExternalFnHandler for Fallback {
    fn call(&self, _name: &str, _args: &[brink_format::Value]) -> brink_runtime::ExternalResult {
        brink_runtime::ExternalResult::Fallback
    }
}

/// A speculation forked from the live story answers the same query over
/// its own forked position (peek, ruled 2026-09-03): it starts where the
/// story is, moves with the fork's own advances, and the live story never
/// moves with it.
#[test]
fn a_speculation_reports_its_own_current_path() -> Res<()> {
    let src = "-> hall\n=== hall ===\nA voice.\n* [Answer]\n    -> hub\n=== hub ===\nBack in the hub.\n-> END\n";
    let mut story = story_from_source(src)?;
    let _ = walk(&mut story)?; // at the choice point, in `hall`
    let handler = Fallback;
    let mut fork = story.speculate();
    assert_eq!(fork.current_path().as_deref(), Some("hall"));
    fork.choose(0)?;
    assert_eq!(fork.current_path().as_deref(), Some("hall"));
    let step = fork.advance(brink_runtime::Budget::default(), &handler)?;
    let brink_runtime::SpeculationStep::Step(Step::Line(line)) = step else {
        return Err("expected the hub's line".into());
    };
    assert_eq!(line.text.trim(), "Back in the hub.");
    assert_eq!(fork.current_path().as_deref(), Some("hub"));
    // The live story did not move.
    assert_eq!(story.current_path().as_deref(), Some("hall"));
    Ok(())
}
