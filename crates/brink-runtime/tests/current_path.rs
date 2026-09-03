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
