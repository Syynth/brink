//! `#@local` flow-private state, end to end through bevy-brink — with
//! **zero host policy code**.
//!
//! Two guards run the same story over one shared `BrinkGlobals` world.
//! The ink source marks `suspicion` (a VAR) and `encounter` (a knot)
//! flow-private with the `#@local` directive
//! (`docs/directive-annotations-spec.md`), so each guard remembers the
//! player privately — its own visit counts, its own suspicion — while
//! the unmarked `alarm_raised` stays world-shared and instantly visible
//! to both. The plugin is `BrinkPlugin::default()`: no `with_policy`,
//! no hand-written name list. The compiled program *is* the policy.
//!
//! Run with `cargo run -p bevy-brink --example flow_private_state`.
//!
//! Expected arc: Ashe is approached three times (Halt! → You again? →
//! ALARM); Brogan's first greeting is still "Halt!" (his private memory
//! is untouched by Ashe's), but after Ashe raises the world-shared
//! alarm, Brogan reacts to it immediately.

// A demo prints its story with `println!` and asserts its own setup with
// `expect` — the workspace denies both in production code, but a runnable
// example is exactly where they're the point.
#![allow(clippy::print_stdout, clippy::expect_used)]
#![expect(
    clippy::type_complexity,
    reason = "bevy systems have complex query tuples"
)]

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy_brink::{
    BrinkBindings, BrinkBindingsAppExt, BrinkChoicesPresented, BrinkContext, BrinkFlow,
    BrinkFlowRequest, BrinkGlobals, BrinkLineDelivered, BrinkLocale, BrinkPlugin, BrinkProgram,
    BrinkStoryAsset, FlowInstance, LineTablesAsset, ProgramAsset, Value, flow_context_view,
    runtime::{ContextAccess, StoryStatus},
};

/// The whole feature is in the ink: `#@local` marks the private state.
const STORY: &str = "\
VAR alarm_raised = false
#@local
VAR suspicion = 0

-> post

=== post ===
-> encounter ->
+ [Approach again] -> post
+ [Leave] -> DONE

=== encounter ===
#@local
{ alarm_raised:
    Don't move! The alarm is up!
    ->->
}
{ stopping:
- Halt! Who goes there?
  ~ suspicion = 1
- You again? State your business.
  ~ suspicion = 2
- Three times now. That's enough — ALARM!
  ~ suspicion = 3
  ~ alarm_raised = true
}
->->
";

#[derive(Component)]
struct Ashe;

#[derive(Component)]
struct Brogan;

fn main() {
    let mut app = App::new();
    app.add_plugins((AssetPlugin::default(), BrinkPlugin::<()>::default()));

    // A binding so `BrinkBindings<()>` exists as a resource; `approach`
    // reads it for the external-fn handler (the story calls none).
    app.bind_brink_fn::<(), _, _>("noop", |_args| Value::Int(0));

    // Narrate story text, labeled by the guard entity's `Name`. Mid-turn
    // text arrives as `BrinkLineDelivered`; a terminal `Line::Choices`
    // carries the accumulated text in its own field, so the choices
    // event is observed too (this story's greetings all land there).
    app.add_observer(|on: On<BrinkLineDelivered<()>>, names: Query<&Name>| {
        say(&names, on.event().entity, &on.event().text);
    });
    app.add_observer(|on: On<BrinkChoicesPresented<()>>, names: Query<&Name>| {
        say(&names, on.event().entity, &on.event().text);
    });

    let Some(story) = build_story(&mut app, STORY) else {
        println!("failed to compile story");
        return;
    };

    // Two guards, one shared world. Note: no policy anywhere.
    app.world_mut().spawn((
        Name::new("Ashe"),
        Ashe,
        BrinkFlowRequest::<()>::builder()
            .story(story.clone())
            .build(),
    ));
    app.world_mut().spawn((
        Name::new("Brogan"),
        Brogan,
        BrinkFlowRequest::<()>::builder().story(story).build(),
    ));
    app.update(); // fulfill both requests

    println!("\n-- you approach Ashe --");
    approach::<Ashe>(&mut app);
    println!("\n-- you approach Brogan (his memory is his own) --");
    approach::<Brogan>(&mut app);
    println!("\n-- back to Ashe --");
    approach::<Ashe>(&mut app);
    println!("\n-- Ashe, a third time --");
    approach::<Ashe>(&mut app);
    println!("\n-- Brogan again (the alarm is world-shared) --");
    approach::<Brogan>(&mut app);

    report(&mut app);
}

/// Print one guard's line, skipping the empty text of a bare choice echo.
fn say(names: &Query<&Name>, entity: Entity, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let who = names.get(entity).map_or("?", |n| n.as_str());
    println!("  [{who}] {text}");
}

/// Advance one guard's flow to its next terminal point; if it is parked
/// on the `[Approach again]` choice from a previous approach, take it.
fn approach<G: Component>(app: &mut App) {
    app.world_mut()
        .run_system_once(
            |mut flows: Query<
                (
                    Entity,
                    &mut BrinkFlow<()>,
                    &mut BrinkContext<()>,
                    &BrinkProgram<()>,
                    &BrinkLocale<()>,
                ),
                With<G>,
            >,
             mut globals: ResMut<BrinkGlobals<()>>,
             programs: Res<Assets<ProgramAsset>>,
             tables: Res<Assets<LineTablesAsset>>,
             bindings: Res<BrinkBindings<()>>,
             mut commands: Commands| {
                let Ok((entity, mut flow, mut ctx, prog, loc)) = flows.single_mut() else {
                    return;
                };
                let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle))
                else {
                    return;
                };
                let mut view = flow_context_view(&mut globals, &mut ctx);
                if flow.inner.status() == StoryStatus::WaitingForChoice {
                    let _ = flow.choose(&mut view, 0); // [Approach again]
                }
                let handler = bindings.handler();
                let _ = flow.advance_until_terminal(
                    &p.program,
                    &t.tables,
                    &mut view,
                    &handler,
                    entity,
                    &mut commands,
                );
                handler.flush(&mut commands);
            },
        )
        .expect("approach system runs");
}

/// Print the final state split: the world-shared alarm, each guard's
/// private suspicion, and the world's own untouched suspicion slot.
fn report(app: &mut App) {
    app.world_mut()
        .run_system_once(
            |mut guards: Query<(&Name, &mut BrinkContext<()>), With<BrinkFlow<()>>>,
             mut globals: ResMut<BrinkGlobals<()>>,
             programs: Res<Assets<ProgramAsset>>| {
                let Some(p) = programs.iter().next().map(|(_, p)| p) else {
                    return;
                };
                let alarm = p.program.global_index("alarm_raised").expect("declared");
                let suspicion = p.program.global_index("suspicion").expect("declared");

                println!("\n-- final state --");
                println!(
                    "  alarm_raised (shared world): {:?}",
                    globals.inner.global(alarm)
                );
                for (name, mut ctx) in &mut guards {
                    let view = flow_context_view(&mut globals, &mut ctx);
                    println!(
                        "  suspicion ({}, flow-private): {:?}",
                        name.as_str(),
                        view.global(suspicion)
                    );
                }
                println!(
                    "  suspicion (world slot, never written): {:?}",
                    globals.inner.global(suspicion)
                );
            },
        )
        .expect("report system runs");
}

/// Compile the inline story and register it as assets (dev-mode shape;
/// production ships pre-compiled `.inkb`).
fn build_story(app: &mut App, src: &str) -> Option<Handle<BrinkStoryAsset>> {
    let owned = src.to_string();
    let output = brink_compiler::compile("demo.ink", move |path| {
        if path == "demo.ink" {
            Ok(owned.clone())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no includes",
            ))
        }
    })
    .ok()?;
    let (program, tables) = brink_runtime::link(&output.data).ok()?;
    let (_, initial_context) = FlowInstance::new_at_root(&program);

    let world = app.world_mut();
    let program = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
        });
    let line_tables = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    Some(
        world
            .resource_mut::<Assets<BrinkStoryAsset>>()
            .add(BrinkStoryAsset {
                program,
                line_tables,
            }),
    )
}
