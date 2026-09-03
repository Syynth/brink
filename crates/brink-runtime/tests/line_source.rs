//! A delivered line's `source` (W7/#3300) spans every source line that
//! contributed text to it — a glue-joined line reads as one line in the
//! Player, and the editor highlights all of its source lines (feedback
//! 2026-09-02).

use brink_runtime::{FastRng, Step, Story};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn story_from_source(src: &str) -> Res<Story<FastRng>> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))?.data;
    let (program, line_tables) = brink_runtime::link(&data)?;
    Ok(Story::new(std::sync::Arc::new(program), line_tables))
}

#[test]
fn a_glue_joined_line_spans_all_its_source_lines() -> Res<()> {
    let src = "-> top\n=== top ===\nFirst part <>\nsecond part.\nPlain line.\n-> END\n";
    let mut story = story_from_source(src)?;
    let Step::Line(joined) = story.continue_single()? else {
        return Err("expected the joined line".into());
    };
    assert_eq!(joined.text.trim(), "First part second part.");
    let source = joined.source.ok_or("the joined line carries a source")?;
    let covered = &src[source.range_start as usize..source.range_end as usize];
    assert!(
        covered.starts_with("First part") && covered.contains("second part."),
        "the range must run from the first fragment to the last: {covered:?}"
    );
    let Step::Line(plain) = story.continue_single()? else {
        return Err("expected the plain line".into());
    };
    let source = plain.source.ok_or("the plain line carries a source")?;
    let covered = &src[source.range_start as usize..source.range_end as usize];
    assert!(
        covered.contains("Plain line.") && !covered.contains("second part"),
        "a single-source line keeps its own range: {covered:?}"
    );
    Ok(())
}
