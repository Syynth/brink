//! Systems that drive flow execution and emit events.

use bevy_asset::Assets;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::MessageWriter;
use bevy_ecs::system::{Query, Res, ResMut};
use bevy_log::error;
use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult, FastRng, StoryStatus};

use crate::asset::{BrinkProgram, ProgramAsset};
use crate::event::BrinkLineMessage;
use crate::flow::BrinkFlow;
use crate::globals::BrinkGlobals;
use crate::line_tables::BrinkLineTables;

/// Default external-function handler — every external call falls back to its
/// in-story fallback container. Consumers wanting real bindings will get a
/// proper handler-registry resource in a follow-up.
struct FallbackHandler;

impl ExternalFnHandler for FallbackHandler {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Fallback
    }
}

/// Advance every active [`BrinkFlow<M>`] by one VM line per tick.
///
/// For each entity that has both a `BrinkFlow<M>` and a `BrinkProgram<M>`,
/// looks up the program from `Assets<ProgramAsset>` (skipping if not yet
/// loaded), calls [`brink_runtime::FlowInstance::step_single_line`] against
/// the shared `BrinkGlobals<M>` and `BrinkLineTables<M>`, and writes one
/// [`BrinkLineMessage<M>`] per line produced.
///
/// Flows that have hit `StoryStatus::WaitingForChoice` or `StoryStatus::Ended`
/// are skipped — the consumer must call `BrinkFlow::inner.choose(...)` to
/// move past a choice point.
#[expect(
    clippy::needless_pass_by_value,
    reason = "bevy systems take Res<T>/ResMut<T> by value — this is the required signature"
)]
pub fn advance_flows<M: Send + Sync + 'static>(
    mut flows: Query<(Entity, &mut BrinkFlow<M>, &BrinkProgram<M>)>,
    mut globals: ResMut<BrinkGlobals<M>>,
    line_tables: Res<BrinkLineTables<M>>,
    programs: Res<Assets<ProgramAsset>>,
    mut messages: MessageWriter<BrinkLineMessage<M>>,
) {
    for (entity, mut flow, brink_program) in &mut flows {
        let Some(program_asset) = programs.get(&brink_program.handle) else {
            continue;
        };

        match flow.inner.status() {
            StoryStatus::WaitingForChoice | StoryStatus::Ended => continue,
            StoryStatus::Active | StoryStatus::Done => {}
        }

        match flow.inner.step_single_line::<FastRng>(
            &program_asset.program,
            &line_tables.tables,
            &mut globals.inner,
            &FallbackHandler,
            None,
        ) {
            Ok(line) => {
                messages.write(BrinkLineMessage::<M>::new(entity, line));
            }
            Err(err) => {
                error!("flow advance error on entity {entity:?}: {err}");
            }
        }
    }
}
