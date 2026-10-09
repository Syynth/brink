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

/// Compile with debug info, as the studios do: a line's source is then
/// resolved from where it was emitted (#3670).
fn debug_story_from_source(src: &str) -> Res<Story<FastRng>> {
    let options = brink_analyzer::AnalysisOptions {
        emit_debug_info: true,
        ..brink_analyzer::AnalysisOptions::default()
    };
    let data =
        brink_compiler::compile_with_options("main.ink", |_p| Ok(src.to_owned()), options)?.data;
    let (program, line_tables) = brink_runtime::link(&data)?;
    Ok(Story::new(std::sync::Arc::new(program), line_tables))
}

/// A screenplay repeats its cues. Line-table dedup shares one entry
/// across every `@Rhodes: <>`, and that entry's location is the first
/// one's — a line resolved through it spanned back to the cue's first use
/// (#3670). Resolved from the emit site, each line covers only its own
/// cue and text.
#[test]
fn a_repeated_cue_does_not_drag_a_line_back_to_its_first_use() -> Res<()> {
    let src = "-> scene\n=== scene ===\n@Rhodes: <>\nSo much for calling me back, huh?\n@Jackie: <>\nI need a cigarette.\n@Rhodes: <>\nNeed's a strong word, you know.\n-> END\n";
    let mut story = debug_story_from_source(src)?;
    let mut seen = Vec::new();
    loop {
        match story.continue_single()? {
            Step::Line(line) => {
                let source = line.source.ok_or("every line carries a source")?;
                let covered =
                    src[source.range_start as usize..source.range_end as usize].to_owned();
                seen.push((line.text.trim().to_owned(), covered));
            }
            Step::End | Step::Done => break,
            other => return Err(format!("unexpected step: {other:?}").into()),
        }
    }
    let (text, covered) = seen.last().ok_or("the story played")?;
    assert_eq!(text, "@Rhodes: Need's a strong word, you know.");
    assert!(
        covered.contains("Need's a strong word") && covered.contains("@Rhodes"),
        "the line covers its own cue and text: {covered:?}"
    );
    assert!(
        !covered.contains("So much") && !covered.contains("cigarette"),
        "and nothing before them: {covered:?}"
    );
    Ok(())
}

/// The same for a choice: a repeated choice text's source is its own line.
/// (Choices take their source from the same line-ref fragments.)
#[test]
fn a_repeated_choice_text_has_its_own_source() -> Res<()> {
    // One knot, so both `Go on`s share a line-table scope — and an entry.
    let src = "-> first\n=== first ===\nOne.\n* [Go on]\n    Two.\n    * * [Go on] -> DONE\n";
    let mut story = debug_story_from_source(src)?;
    let first_choices = loop {
        if let Step::Choices(choices) = story.continue_single()? {
            break choices;
        }
    };
    story.choose(first_choices[0].index)?;
    let choices = loop {
        if let Step::Choices(choices) = story.continue_single()? {
            break choices;
        }
    };
    let source = choices[0]
        .source
        .clone()
        .ok_or("the choice carries a source")?;
    let line_start = src.find("* * [Go on]").ok_or("the second choice")?;
    assert!(
        source.range_start as usize >= line_start,
        "the second `Go on` is sourced at its own line, not the first: {:?}",
        &src[source.range_start as usize..source.range_end as usize]
    );
    Ok(())
}
