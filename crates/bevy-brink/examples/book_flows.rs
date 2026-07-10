//! Compile-checked source for the Bevy book page
//! `docs/book/src/integrations/bevy/flows.md`.
//!
//! The book pulls the snippets shown on that page out of this file via
//! mdbook `{{#include …:anchor}}` directives, so the code readers see is
//! exactly the code that `cargo build --example book_flows` compiles. Keep
//! the regions between `// ANCHOR:` / `// ANCHOR_END:` readable as prose —
//! they are the documentation. `just book-test` builds this example and the
//! book together.
//!
//! Every runtime type these snippets name (`BrinkWorld`, `Choice`,
//! `RuntimeError`, …) is re-exported from `bevy_brink`, so a consumer needs
//! no direct `brink-runtime` dependency — this example imports from
//! `bevy_brink` alone to prove it.

// The observer snippet prints choices with `println!` — the workspace denies
// bare stdout prints in production code, but a docs example is exactly where
// showing a plain `println!` is the point.
#![allow(clippy::print_stdout)]

use bevy::prelude::*;
use bevy_brink::{
    BrinkBindings, BrinkBindingsAppExt, BrinkChoicesPresented, BrinkContext, BrinkFlow,
    BrinkFlowRequest, BrinkGlobals, BrinkLocale, BrinkPlugin, BrinkProgram, BrinkTranscript,
    BrinkWorld, Choice, ContextSeed, FlowStart, LineTablesAsset, ProgramAsset, RuntimeError, Value,
    digit_key_to_choice_index,
};

fn main() {
    let mut app = App::new();
    app.add_plugins((AssetPlugin::default(), BrinkPlugin::<()>::default()));

    // A binding so `BrinkBindings<()>` exists as a resource; `drive` reads it.
    app.bind_brink_fn::<(), _, _>("noop", |_args| Value::Int(0));

    register_choice_logger(&mut app);
    app.add_systems(Update, drive);
    // Inert here (there is no `dialogue.inkb` asset to load); registered only
    // so the request-spawning snippet is compiled as a real system.
    app.add_systems(Update, request_flow.run_if(|| false));

    app.update();
}

/// Spawn a flow via the request-component pattern.
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res by value"
)]
fn request_flow(mut commands: Commands, assets: Res<AssetServer>) {
    // ANCHOR: spawn_request
    commands.spawn(
        BrinkFlowRequest::<()>::builder()
            .story(assets.load("dialogue.inkb"))
            .start(FlowStart::Address("intro_scene".into())) // optional
            .seed(ContextSeed::FromGlobals) // optional
            .build(),
    );
    // ANCHOR_END: spawn_request
}

/// Commit a side conversation's world back into the shared save data.
#[expect(
    dead_code,
    reason = "included verbatim into the Bevy book; not exercised here"
)]
fn save(globals: &mut BrinkGlobals<()>, flow_ctx: &BrinkWorld, program: &ProgramAsset) {
    // ANCHOR: commit
    globals.commit_from(flow_ctx); // wholesale "save everything"
    globals.commit_progress(flow_ctx); // globals replace; visit/turn counts take the max
    globals.commit_globals_only(flow_ctx); // just variables; leave counts/RNG alone
    // "new game" reset:
    globals.commit_from(&program.initial_context);
    // ANCHOR_END: commit
}

/// Drive every non-paused flow to a terminal line from a normal system.
#[expect(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    reason = "bevy systems take Res by value and have complex query tuples"
)]
// ANCHOR: drive
fn drive(
    mut flows: Query<(Entity, &mut BrinkFlow<()>, &mut BrinkContext<()>,
                      &BrinkProgram<()>, &BrinkLocale<()>)>,
    programs: Res<Assets<ProgramAsset>>,
    tables: Res<Assets<LineTablesAsset>>,
    bindings: Res<BrinkBindings<()>>,
    mut commands: Commands,
) {
    for (entity, mut flow, mut ctx, prog, loc) in &mut flows {
        if flow.inner.has_pending_external() { continue; } // paused; resolver will resume it
        let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle))
            else { continue; };
        let handler = bindings.handler();
        let _ = flow.advance_until_terminal(
            &p.program, &t.tables, &mut ctx.inner, &handler, entity, &mut commands,
        );
        handler.flush(&mut commands); // emit any buffered command events
    }
}
// ANCHOR_END: drive

/// Select a pending choice.
#[expect(
    dead_code,
    reason = "included verbatim into the Bevy book; not exercised here"
)]
fn pick(
    flow: &mut BrinkFlow<()>,
    ctx: &mut BrinkContext<()>,
    index: usize,
) -> Result<(), RuntimeError> {
    // ANCHOR: choose
    flow.choose(&mut ctx.inner, index)?;
    // ANCHOR_END: choose
    Ok(())
}

/// Map a pressed digit key to a choice index.
#[expect(
    clippy::needless_pass_by_value,
    dead_code,
    reason = "included verbatim into the Bevy book; not exercised here"
)]
fn keyboard_pick(
    keys: Res<ButtonInput<KeyCode>>,
    choices: &[Choice],
    flow: &mut BrinkFlow<()>,
    ctx: &mut BrinkContext<()>,
) -> Result<(), RuntimeError> {
    // ANCHOR: digit_choose
    if let Some(idx) = digit_key_to_choice_index(&keys, choices.len()) {
        flow.choose(&mut ctx.inner, idx)?;
    }
    // ANCHOR_END: digit_choose
    Ok(())
}

/// React to a choices-presented event with an observer.
fn register_choice_logger(app: &mut App) {
    // ANCHOR: observer
    app.add_observer(|on: On<BrinkChoicesPresented<()>>| {
        for (i, choice) in on.event().choices.iter().enumerate() {
            println!("  [{}] {}", i + 1, choice.text);
        }
    });
    // ANCHOR_END: observer
}

/// Opt a flow into a whole-conversation transcript, then read it.
#[expect(
    dead_code,
    reason = "included verbatim into the Bevy book; not exercised here"
)]
fn show_transcript(mut commands: Commands, flow: Entity, transcript: &BrinkTranscript<()>) {
    // ANCHOR: transcript
    commands.entity(flow).insert(BrinkTranscript::<()>::default());
    // later, from a system that reads the component:
    let text = transcript.text(); // all lines joined with '\n'
    let lines = &transcript.lines; // Vec<(String, Vec<String>)> — (text, tags)
    render_conversation(&text, lines);
    // ANCHOR_END: transcript
}

/// Stand-in for the game's own transcript renderer.
fn render_conversation(_text: &str, _lines: &[(String, Vec<String>)]) {}
