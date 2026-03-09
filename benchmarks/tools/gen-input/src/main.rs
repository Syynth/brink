use brink_converter::convert;
use brink_json::InkJson;
use brink_runtime::{DotNetRng, StepResult, Story};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const SEED: u64 = 42;
const MAX_CHOICES: usize = 5000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let story_path = args.get(1).map_or("benchmarks/stories/hanoi-10/story.ink.json", |s| s.as_str());

    let json_str = std::fs::read_to_string(story_path)
        .unwrap_or_else(|e| panic!("failed to read {story_path}: {e}"));
    let ink: InkJson = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("failed to parse JSON: {e}"));
    let data = convert(&ink)
        .unwrap_or_else(|e| panic!("failed to convert: {e}"));
    let program = brink_runtime::link(&data)
        .unwrap_or_else(|e| panic!("failed to link: {e}"));
    let mut story = Story::<DotNetRng>::new(&program);
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut choice_count = 0;

    loop {
        match story.continue_maximally() {
            Ok(StepResult::Done { .. } | StepResult::Ended { .. }) => break,
            Ok(StepResult::Choices { choices, .. }) => {
                if choice_count >= MAX_CHOICES {
                    break;
                }
                let idx = rng.random_range(0..choices.len());
                println!("{idx}");
                story.choose(idx).unwrap_or_else(|e| panic!("choose failed: {e}"));
                choice_count += 1;
            }
            Err(e) => panic!("runtime error: {e}"),
        }
    }

    eprintln!("Generated {choice_count} choice inputs (seed={SEED})");
}
