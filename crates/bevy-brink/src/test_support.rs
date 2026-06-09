//! Shared test helpers used by inline `#[cfg(test)]` modules across
//! the crate. Compiled only under `cfg(test)`.
//!
//! Provides:
//! - `compile_test_story`: compile a small ink source into the trio of
//!   (`Program`, line tables, fresh `Context`) needed to set up asset
//!   state in a test.
//! - `make_test_app` / `add_story_assets`: build a minimal Bevy `App`
//!   wired with `BrinkPlugin`, plus directly insert pre-built story
//!   assets so tests don't have to round-trip through the file-watcher
//!   loaders.

#![cfg(test)]

use bevy_app::App;
use bevy_asset::{AssetPlugin, Assets, Handle};
use brink_runtime::{Context, Program};

use crate::asset::{BrinkStoryAsset, LineTablesAsset, ProgramAsset, fresh_context};

/// Compile an inline ink source and return the (`Program`, line tables,
/// fresh `Context`) tuple needed to build a `BrinkStoryAsset` in a test.
///
/// Panics on any failure — tests should provide valid ink sources.
/// (`expect` is allowed in tests via `clippy.toml`'s `allow-expect-in-tests`.)
pub fn compile_test_story(source: &str) -> (Program, Vec<Vec<brink_format::LineEntry>>, Context) {
    let output = brink_compiler::compile("test.ink", |path| {
        if path == "test.ink" {
            Ok(source.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("unexpected include: {path}"),
            ))
        }
    })
    .expect("test fixture should compile");
    let (program, tables) = brink_runtime::link(&output.data).expect("test fixture should link");
    let initial_context = fresh_context(&program);
    (program, tables, initial_context)
}

/// Build an `App` with the minimum plugins needed to exercise
/// `BrinkPlugin<()>`'s systems without spinning up a full Bevy game.
pub fn make_test_app() -> App {
    let mut app = App::new();
    app.add_plugins(AssetPlugin::default());
    app.add_plugins(crate::BrinkPlugin::<()>::default());
    app
}

/// Insert pre-built story assets directly into `Assets<...>` and
/// return a `Handle<BrinkStoryAsset>` pointing at them.
///
/// Lets tests bypass the loaders entirely so they can focus on the
/// fulfillment / replay logic.
pub fn add_story_assets(
    app: &mut App,
    program: Program,
    tables: Vec<Vec<brink_format::LineEntry>>,
    initial_context: Context,
) -> Handle<BrinkStoryAsset> {
    let world = app.world_mut();
    let program_handle = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
        });
    let tables_handle = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    world
        .resource_mut::<Assets<BrinkStoryAsset>>()
        .add(BrinkStoryAsset {
            program: program_handle,
            line_tables: tables_handle,
        })
}

/// Host-side coverage for the runtime `Story` additions that back the
/// `brink-web` external-binding foundation (Track A). These exercise the
/// runtime contract directly (no Bevy, no wasm); the JS marshaling is covered
/// by `brink-web`'s `wasm-bindgen-test` suite. Lives here because the runtime
/// crate can't depend on the compiler (cycle), and this crate already has
/// `compile_test_story`.
mod runtime_story_api {
    use super::compile_test_story;
    use brink_format::Value;
    use brink_runtime::{ExternalFnHandler, ExternalResult, FastRng, Line, Story};

    #[expect(
        clippy::needless_pass_by_value,
        reason = "test helper takes ownership of the produced lines"
    )]
    fn render(lines: Vec<Line>) -> String {
        lines.iter().map(Line::text).collect()
    }

    #[test]
    fn variable_get_set_by_name() {
        let (program, tables, _ctx) = compile_test_story("VAR mood = 1\nMood {mood}.\n-> END\n");
        let mut story = Story::<FastRng>::new(&program, tables);
        assert_eq!(story.variable("mood"), Some(&Value::Int(1)));
        assert!(story.set_variable("mood", Value::Int(5)));
        assert_eq!(story.variable("mood"), Some(&Value::Int(5)));
        // Unknown variable: read None, set is a no-op returning false.
        assert!(!story.set_variable("nope", Value::Int(0)));
        assert_eq!(story.variable("nope"), None);
    }

    #[test]
    fn set_variable_reflected_in_output() {
        let (program, tables, _ctx) = compile_test_story("VAR mood = 1\nMood {mood}.\n-> END\n");
        let mut story = Story::<FastRng>::new(&program, tables);
        story.set_variable("mood", Value::Int(7));
        let text = render(story.continue_maximally().expect("continues"));
        assert!(text.contains("Mood 7."), "got {text:?}");
    }

    #[test]
    fn rng_seed_is_deterministic() {
        let src = "{RANDOM(1, 1000)}\n-> END\n";
        let run = |seed: i32| {
            let (program, tables, _ctx) = compile_test_story(src);
            let mut story = Story::<FastRng>::new(&program, tables);
            story.set_rng_seed(seed);
            render(story.continue_maximally().expect("continues"))
        };
        assert_eq!(run(42), run(42), "same seed -> identical RANDOM output");
    }

    #[test]
    fn external_binding_resolves_via_continue_with() {
        struct Doubler;
        impl ExternalFnHandler for Doubler {
            fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
                if name == "double" {
                    let n = args.first().and_then(Value::as_int).unwrap_or(0);
                    ExternalResult::Resolved(Value::Int(n * 2))
                } else {
                    ExternalResult::Fallback
                }
            }
        }
        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL double(x)\nResult: {double(21)}.\n-> END\n");
        let mut story = Story::<FastRng>::new(&program, tables);
        let text = render(story.continue_maximally_with(&Doubler).expect("continues"));
        assert!(text.contains("Result: 42"), "got {text:?}");
    }

    #[test]
    fn save_load_round_trips_globals() {
        let src = "VAR mood = 1\nVAR who = \"a\"\nMood {mood} {who}.\n-> END\n";
        let (program, tables, _ctx) = compile_test_story(src);
        let mut s1 = Story::<FastRng>::new(&program, tables.clone());
        s1.set_variable("mood", Value::Int(9));
        s1.set_variable("who", Value::from("bob"));
        let save = s1.save_state();

        let mut s2 = Story::<FastRng>::new(&program, tables);
        let report = s2.load_state(&save);
        assert!(report.is_clean(), "clean load: {report:?}");
        assert_eq!(s2.variable("mood"), Some(&Value::Int(9)));
        assert_eq!(s2.variable("who").and_then(Value::as_str), Some("bob"));
    }

    #[test]
    fn load_reports_globals_the_program_lacks() {
        let (pa, ta, _) = compile_test_story("VAR foo = 1\n-> END\n");
        let mut sa = Story::<FastRng>::new(&pa, ta);
        sa.set_variable("foo", Value::Int(5));
        let save = sa.save_state();

        // A different story with no `foo`: the saved global is reported, not applied.
        let (pb, tb, _) = compile_test_story("VAR bar = 2\n-> END\n");
        let mut sb = Story::<FastRng>::new(&pb, tb);
        let report = sb.load_state(&save);
        assert_eq!(report.unknown_globals, vec!["foo".to_string()]);
        assert!(!report.is_clean());
    }

    #[test]
    fn visit_counts_survive_round_trip() {
        // Referencing {start} makes `start` a counted scope.
        let src = "-> start\n=== start ===\nVisits: {start}.\n-> END\n";
        let (program, tables, _) = compile_test_story(src);
        let mut s1 = Story::<FastRng>::new(&program, tables.clone());
        let _ = s1.continue_maximally().expect("continues");
        let save = s1.save_state();
        assert!(!save.visits.is_empty(), "a visit count should be recorded");

        let mut s2 = Story::<FastRng>::new(&program, tables);
        s2.load_state(&save);
        assert_eq!(
            s2.save_state().visits,
            save.visits,
            "visit counts round-trip"
        );
    }

    #[test]
    fn call_function_returns_value() {
        use brink_runtime::FallbackHandler;
        let src = "-> END\n=== function add(a, b) ===\n~ return a + b\n";
        let (program, tables, _) = compile_test_story(src);
        let mut story = Story::<FastRng>::new(&program, tables);
        let v = story
            .call_function("add", &[Value::Int(2), Value::Int(3)], &FallbackHandler)
            .expect("calls");
        assert_eq!(v, Value::Int(5));
    }

    #[test]
    fn call_function_resolves_external_via_handler() {
        struct Doubler;
        impl ExternalFnHandler for Doubler {
            fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
                if name == "dbl" {
                    let n = args.first().and_then(Value::as_int).unwrap_or(0);
                    ExternalResult::Resolved(Value::Int(n * 2))
                } else {
                    ExternalResult::Fallback
                }
            }
        }
        let src = "EXTERNAL dbl(x)\n-> END\n=== function scaled(n) ===\n~ return dbl(n) + 1\n";
        let (program, tables, _) = compile_test_story(src);
        let mut story = Story::<FastRng>::new(&program, tables);
        let v = story
            .call_function("scaled", &[Value::Int(10)], &Doubler)
            .expect("calls");
        assert_eq!(v, Value::Int(21), "dbl(10) + 1");
    }

    #[test]
    fn call_function_unknown_errors() {
        use brink_runtime::{FallbackHandler, RuntimeError};
        let (program, tables, _) = compile_test_story("-> END\n");
        let mut story = Story::<FastRng>::new(&program, tables);
        let err = story
            .call_function("nope", &[], &FallbackHandler)
            .unwrap_err();
        assert!(
            matches!(err, RuntimeError::FunctionNotFound(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn advance_with_surfaces_and_resumes_pending_external() {
        use brink_runtime::StepOutcome;
        // Defers `wait` (Pending) — the runtime pause/resume the async web path
        // is built on, exercised here without any JS.
        struct Pauser;
        impl ExternalFnHandler for Pauser {
            fn call(&self, name: &str, _args: &[Value]) -> ExternalResult {
                if name == "wait" {
                    ExternalResult::Pending
                } else {
                    ExternalResult::Fallback
                }
            }
        }
        let (program, tables, _) = compile_test_story("EXTERNAL wait(x)\nGot {wait(5)}.\n-> END\n");
        let mut story = Story::<FastRng>::new(&program, tables);

        let mut text = String::new();
        let mut parked = false;
        for _ in 0..50 {
            match story.advance_with(&Pauser).expect("advance") {
                StepOutcome::Line(line) => {
                    let terminal = line.is_terminal();
                    text.push_str(line.text());
                    if terminal {
                        break;
                    }
                }
                StepOutcome::AwaitingExternal => {
                    assert_eq!(story.pending_external_name(), Some("wait"));
                    assert_eq!(story.pending_external_args().to_vec(), vec![Value::Int(5)]);
                    story.resolve_external(Value::Int(7));
                    parked = true;
                }
            }
        }
        assert!(parked, "flow should have parked on the external");
        assert!(
            text.contains("Got 7."),
            "resolved value appears; got {text:?}"
        );
    }
}
