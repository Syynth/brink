//! Transcript comparison harness for the test corpus.
//!
//! Walks `tests/tier1/`, and for each test case with `transcript.txt` and
//! `mode = "runtime"` in `metadata.toml`, converts and runs the story,
//! comparing output against the expected transcript.

use std::path::{Path, PathBuf};

use brink_converter::convert;
use brink_json::InkJson;
use brink_runtime::{DotNetRng, StepResult, Story};

/// Format text with per-line tags inserted after each tagged line.
///
/// Tags for a line are inserted immediately after that line's trailing `\n`
/// (on a new line). If the tagged line has no trailing `\n` (last line),
/// tags are appended directly (no newline separator), matching the old
/// flat-tags formatting behaviour.
fn format_text_with_tags(text: &str, tags: &[Vec<String>], output: &mut String) {
    use std::fmt::Write;
    if tags.iter().all(Vec::is_empty) {
        output.push_str(text);
        return;
    }
    let mut line_start = 0;
    let mut line_idx = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            let line = &text[line_start..i];
            output.push_str(line);
            if let Some(lt) = tags.get(line_idx)
                && !lt.is_empty()
            {
                let _ = write!(output, "\n# tags: {}", lt.join(", "));
            }
            output.push('\n');
            line_start = i + 1;
            line_idx += 1;
        }
    }
    // Final segment (no trailing \n).
    let remaining = &text[line_start..];
    output.push_str(remaining);
    if let Some(lt) = tags.get(line_idx)
        && !lt.is_empty()
    {
        let _ = write!(output, "# tags: {}", lt.join(", "));
    }
}

/// Run a story from an ink.json file with the given choice inputs.
fn run_story_from_json(json_str: &str, inputs: &[usize]) -> Result<String, String> {
    use std::fmt::Write;
    let ink: InkJson =
        serde_json::from_str(json_str).map_err(|e| format!("json parse error: {e}"))?;
    let data = convert(&ink).map_err(|e| format!("convert error: {e}"))?;
    let program = brink_runtime::link(&data).map_err(|e| format!("link error: {e}"))?;
    let mut story = Story::<DotNetRng>::new(&program);
    let mut output = String::new();
    let mut input_idx = 0;

    // Safety limit to prevent infinite loops.
    let mut steps = 0;
    let max_steps = 10_000;

    loop {
        steps += 1;
        if steps > max_steps {
            return Err(format!("exceeded {max_steps} steps — likely infinite loop"));
        }

        match story
            .step(&program)
            .map_err(|e| format!("runtime error: {e}"))?
        {
            StepResult::Done { text, tags } | StepResult::Ended { text, tags } => {
                format_text_with_tags(&text, &tags, &mut output);
                break;
            }
            StepResult::Choices {
                text,
                choices,
                tags,
            } => {
                format_text_with_tags(&text, &tags, &mut output);
                if choices.is_empty() {
                    return Err("no choices available".into());
                }

                // No more inputs — stop here (transcript ends before
                // these choices are displayed/selected).
                if input_idx >= inputs.len() {
                    break;
                }

                // Format choices to match the transcript format.
                output.push('\n');
                for choice in &choices {
                    let trimmed = choice.text.trim();
                    let _ = writeln!(output, "{}: {trimmed}", choice.index + 1);
                }
                output.push_str("?> ");

                let choice_idx = inputs[input_idx];
                input_idx += 1;
                if choice_idx >= choices.len() {
                    return Err(format!(
                        "choice index {choice_idx} out of range (only {} choices)",
                        choices.len()
                    ));
                }
                story
                    .choose(choice_idx)
                    .map_err(|e| format!("choose error: {e}"))?;
            }
        }
    }

    Ok(output)
}

/// Parse input.txt to get choice indices.
fn parse_inputs(path: &Path) -> Vec<usize> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| l.trim().parse::<usize>().ok())
        .collect()
}

/// Check if metadata.toml has mode = "runtime" and no [skip] section.
fn is_runtime_test(metadata_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(metadata_path) else {
        return false;
    };
    content.contains("mode = \"runtime\"") && !content.contains("[skip]")
}

/// Find all test cases in a directory tree.
fn find_test_cases(base: &Path) -> Vec<PathBuf> {
    let mut cases = Vec::new();
    if !base.is_dir() {
        return cases;
    }
    walk_dir(base, &mut cases);
    cases.sort();
    cases
}

fn walk_dir(dir: &Path, cases: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Check if this directory is a test case.
            let json_path = path.join("story.ink.json");
            let transcript_path = path.join("transcript.txt");
            if json_path.exists() && transcript_path.exists() {
                cases.push(path.clone());
            }
            walk_dir(&path, cases);
        }
    }
}

/// Run the corpus for a given tier directory and print results.
#[expect(clippy::print_stderr, clippy::unwrap_used)]
fn run_corpus(tier: &str) {
    let corpus_base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/{tier}"));

    let test_cases = find_test_cases(&corpus_base);
    if test_cases.is_empty() {
        eprintln!("WARNING: no test cases found in {}", corpus_base.display());
        return;
    }

    let mut passed: i32 = 0;
    let mut failed: i32 = 0;
    let mut skipped: i32 = 0;
    let mut failures: Vec<String> = Vec::new();

    for case_dir in &test_cases {
        let case_name = case_dir
            .strip_prefix(&corpus_base)
            .unwrap_or(case_dir)
            .display()
            .to_string();

        // Check metadata
        let metadata_path = case_dir.join("metadata.toml");
        if !is_runtime_test(&metadata_path) {
            skipped += 1;
            continue;
        }

        let json_path = case_dir.join("story.ink.json");
        let transcript_path = case_dir.join("transcript.txt");
        let input_path = case_dir.join("input.txt");

        let json_str = std::fs::read_to_string(&json_path).unwrap();
        let expected = std::fs::read_to_string(&transcript_path).unwrap();
        let inputs = parse_inputs(&input_path);

        let result = std::panic::catch_unwind(|| run_story_from_json(&json_str, &inputs));
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                let msg = e
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| e.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic");
                Err(format!("panic: {msg}"))
            }
        };
        match result {
            Ok(actual) => {
                // Normalize: trim trailing whitespace and ensure consistent line endings.
                let actual_normalized = actual.trim_end();
                let expected_normalized = expected.trim_end();

                if actual_normalized == expected_normalized {
                    passed += 1;
                } else {
                    failed += 1;
                    failures.push(format!(
                        "FAIL: {case_name}\n  expected: {expected_normalized:?}\n  actual:   {actual_normalized:?}",
                    ));
                }
            }
            Err(e) => {
                failed += 1;
                failures.push(format!("ERROR: {case_name}: {e}"));
            }
        }
    }

    let total = passed + failed + skipped;
    eprintln!("\n=== Corpus Results ({tier}) ===");
    eprintln!("Total: {total}, Passed: {passed}, Failed: {failed}, Skipped: {skipped}");

    if !failures.is_empty() {
        eprintln!("\nFailures:");
        for f in &failures {
            eprintln!("  {f}");
        }
    }

    // Don't assert all pass — this is a spike. Just report.
    let rate = if passed + failed > 0 {
        (f64::from(passed) / f64::from(passed + failed)) * 100.0
    } else {
        0.0
    };
    eprintln!("\nPass rate: {passed}/{} ({rate:.0}%)", passed + failed);
}

#[test]
fn corpus_tier1() {
    run_corpus("tier1");
}

#[test]
fn corpus_tier2() {
    run_corpus("tier2");
}

#[test]
fn corpus_tier3() {
    run_corpus("tier3");
}
