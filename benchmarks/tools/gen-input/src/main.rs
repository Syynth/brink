use brink_runtime::{DotNetRng, Line, Story};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const SEED: u64 = 42;
const MAX_CHOICES: usize = 5000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let story_path = args.get(1).map_or("benchmarks/stories/hanoi-10/story.ink", |s| s.as_str());

    let data = brink_compiler::compile_path(std::path::Path::new(story_path))
        .unwrap_or_else(|e| panic!("failed to compile {story_path}: {e}"))
        .data;
    let (program, line_tables) = brink_runtime::link(&data)
        .unwrap_or_else(|e| panic!("failed to link: {e}"));
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut choice_count = 0;

    loop {
        let lines = match story.continue_maximally() {
            Ok(l) => l,
            Err(e) => panic!("runtime error: {e}"),
        };
        let last = lines.last();
        match last {
            Some(Line::Text { .. } | Line::Done { .. } | Line::End { .. }) | None => break,
            Some(Line::Choices { choices, .. }) => {
                if choice_count >= MAX_CHOICES {
                    break;
                }
                let idx = rng.random_range(0..choices.len());
                println!("{idx}");
                story.choose(idx).unwrap_or_else(|e| panic!("choose failed: {e}"));
                choice_count += 1;
            }
        }
    }

    eprintln!("Generated {choice_count} choice inputs (seed={SEED})");
}
