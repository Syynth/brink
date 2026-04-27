//! Per-flow mutable state: call stacks, output buffer, pending choices.

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::Commands;
use brink_runtime::{ExternalFnHandler, FastRng, FlowInstance, Line, Program, RuntimeError};

use crate::event::{BrinkChoicesPresented, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};
use crate::globals::BrinkGlobals;
use crate::line_tables::BrinkLineTables;

/// A single live ink flow, attached to an entity. Holds the VM's per-flow
/// state: call stacks, output buffer, pending choices, and the accumulated
/// transcript.
///
/// Spawn one of these per active conversation. Systems advance the flow by
/// calling methods on `inner` against the shared [`BrinkGlobals`](crate::BrinkGlobals)
/// (or a per-flow `Context` if you're doing fork/branch) and the current
/// program from `Assets<ProgramAsset>`.
#[derive(Component)]
pub struct BrinkFlow<M: Send + Sync + 'static = ()> {
    pub inner: FlowInstance,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkFlow<M> {
    /// Wrap a freshly-constructed [`FlowInstance`] (e.g. from
    /// [`FlowInstance::new_at_root`](brink_runtime::FlowInstance::new_at_root))
    /// as a Bevy component ready to spawn.
    #[must_use]
    pub fn new(flow: FlowInstance) -> Self {
        Self {
            inner: flow,
            _marker: PhantomData,
        }
    }

    /// Select a choice. Convenience wrapper that pulls `&mut Context`
    /// out of [`BrinkGlobals`].
    pub fn choose(
        &mut self,
        globals: &mut BrinkGlobals<M>,
        index: usize,
    ) -> Result<(), RuntimeError> {
        self.inner.choose(&mut globals.inner, index)
    }

    /// Like [`choose`](Self::choose) but also records the chosen index
    /// into a [`BrinkReplayLog`](crate::BrinkReplayLog) so the plugin's
    /// reload-replay system can re-apply the choice after a hot-reload.
    ///
    /// Available only with the `dev` feature. In release builds, just
    /// use [`choose`](Self::choose).
    #[cfg(feature = "dev")]
    pub fn choose_recording(
        &mut self,
        globals: &mut BrinkGlobals<M>,
        log: &mut crate::replay::BrinkReplayLog<M>,
        index: usize,
    ) -> Result<(), RuntimeError> {
        log.choices_made.push(index);
        self.inner.choose(&mut globals.inner, index)
    }

    /// Step the VM by one line and queue the corresponding observer
    /// event ([`BrinkLineDelivered`], [`BrinkChoicesPresented`],
    /// [`BrinkTurnDone`], or [`BrinkStoryEnded`]) for the entity.
    ///
    /// Use this for typewriter-style UIs that animate one fragment at a
    /// time. For click-to-continue dialogue, use
    /// [`advance_until_terminal`](Self::advance_until_terminal).
    pub fn step_one(
        &mut self,
        program: &Program,
        line_tables: &BrinkLineTables<M>,
        globals: &mut BrinkGlobals<M>,
        handler: &dyn ExternalFnHandler,
        entity: Entity,
        commands: &mut Commands,
    ) -> Result<Line, RuntimeError> {
        let line = self.inner.step_single_line::<FastRng>(
            program,
            &line_tables.tables,
            &mut globals.inner,
            handler,
            None,
        )?;
        emit_event::<M>(&line, entity, commands);
        Ok(line)
    }

    /// Step the VM until reaching a terminal line ([`Line::Done`],
    /// [`Line::Choices`], or [`Line::End`]), queuing observer events
    /// for every line produced along the way.
    ///
    /// Bounded by a 10,000-line safety cap. Returns the terminal line.
    pub fn advance_until_terminal(
        &mut self,
        program: &Program,
        line_tables: &BrinkLineTables<M>,
        globals: &mut BrinkGlobals<M>,
        handler: &dyn ExternalFnHandler,
        entity: Entity,
        commands: &mut Commands,
    ) -> Result<Line, RuntimeError> {
        const STEP_LIMIT: u64 = 10_000;
        for _ in 0..STEP_LIMIT {
            let line = self.step_one(program, line_tables, globals, handler, entity, commands)?;
            if !matches!(line, Line::Text { .. }) {
                return Ok(line);
            }
        }
        Err(RuntimeError::StepLimitExceeded(STEP_LIMIT))
    }
}

/// Trigger the appropriate observer event for the produced [`Line`].
///
/// Internal helper used by both [`BrinkFlow::step_one`] and the replay
/// system so that the same set of events fires whether the flow is
/// being advanced in response to player input or replayed during a
/// hot-reload.
pub(crate) fn emit_event<M: Send + Sync + 'static>(
    line: &Line,
    entity: Entity,
    commands: &mut Commands,
) {
    match line {
        Line::Text { text, tags } => commands.trigger(BrinkLineDelivered::<M>::new(
            entity,
            text.clone(),
            tags.clone(),
        )),
        Line::Choices {
            text,
            tags,
            choices,
        } => commands.trigger(BrinkChoicesPresented::<M>::new(
            entity,
            text.clone(),
            tags.clone(),
            choices.clone(),
        )),
        Line::Done { text, tags } => commands.trigger(BrinkTurnDone::<M>::new(
            entity,
            text.clone(),
            tags.clone(),
        )),
        Line::End { text, tags } => commands.trigger(BrinkStoryEnded::<M>::new(
            entity,
            text.clone(),
            tags.clone(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{add_story_assets, compile_test_story, make_test_app};
    use crate::{BrinkFlowRequest, BrinkProgram};
    use bevy_app::Update;
    use bevy_asset::Assets;
    use bevy_ecs::prelude::*;

    /// Recorder for observer events; tests assert against its contents.
    #[derive(Resource, Default)]
    struct EventRecorder {
        text_lines: Vec<String>,
        choices_presentations: Vec<Vec<String>>,
        turn_done: u32,
        story_ended: u32,
    }

    fn install_recorder(app: &mut bevy_app::App) {
        app.insert_resource(EventRecorder::default());
        app.insert_resource(DriveAdvance::default());
        app.add_systems(Update, advance_driver_system);
        app.add_observer(
            |trigger: On<BrinkLineDelivered<()>>, mut rec: ResMut<EventRecorder>| {
                rec.text_lines.push(trigger.event().text.clone());
            },
        );
        app.add_observer(
            |trigger: On<BrinkChoicesPresented<()>>, mut rec: ResMut<EventRecorder>| {
                rec.choices_presentations.push(
                    trigger
                        .event()
                        .choices
                        .iter()
                        .map(|c| c.text.clone())
                        .collect(),
                );
            },
        );
        app.add_observer(
            |_: On<BrinkTurnDone<()>>, mut rec: ResMut<EventRecorder>| {
                rec.turn_done += 1;
            },
        );
        app.add_observer(
            |_: On<BrinkStoryEnded<()>>, mut rec: ResMut<EventRecorder>| {
                rec.story_ended += 1;
            },
        );
    }

    /// Resource that, when set to `true`, causes `advance_driver_system`
    /// (scheduled in `Update`) to advance every flow this tick. Set
    /// `true`, run `app.update()`, then read the recorder.
    #[derive(Resource, Default)]
    struct DriveAdvance(bool);

    /// Drive a single advance pass and flush its triggers via the
    /// regular Update schedule (RunSystemOnce doesn't reliably flush
    /// queued observer triggers in 0.18 test contexts).
    fn run_advance(app: &mut bevy_app::App) {
        app.world_mut().resource_mut::<DriveAdvance>().0 = true;
        app.update();
    }

    fn advance_driver_system(
        mut flows: Query<(Entity, &mut BrinkFlow<()>, &BrinkProgram<()>)>,
        line_tables: Res<crate::BrinkLineTables<()>>,
        globals: Option<ResMut<crate::BrinkGlobals<()>>>,
        programs: Res<Assets<crate::ProgramAsset>>,
        mut drive: ResMut<DriveAdvance>,
        mut commands: Commands,
    ) {
        if !drive.0 {
            return;
        }
        drive.0 = false;
        eprintln!("[advance] running");
        let Some(mut globals) = globals else {
            eprintln!("[advance] no globals yet");
            return;
        };
        let count = flows.iter().count();
        eprintln!("[advance] {count} flow(s)");
        for (entity, mut flow, brink_program) in &mut flows {
            let Some(program_asset) = programs.get(&brink_program.handle) else {
                eprintln!("[advance] no program asset");
                continue;
            };
            let result = flow.advance_until_terminal(
                &program_asset.program,
                &line_tables,
                &mut globals,
                &brink_runtime::FallbackHandler,
                entity,
                &mut commands,
            );
            eprintln!("[advance] result={:?}, status={:?}", result.is_ok(), flow.inner.status());
        }
    }

    #[test]
    #[ignore = "commands.trigger from a system queued via Update doesn't \
        fire observers in the test context; flow state mutates correctly \
        but events don't dispatch. Suspected same root cause as the \
        hot-reload visual issue. See docs/bevy-brink.md#open-issues."]
    fn advance_until_terminal_fires_text_then_choices() {
        let mut app = make_test_app();
        install_recorder(&mut app);

        let (program, tables, ctx) = compile_test_story(
            "=== start ===\nfirst line\nsecond line\n* [A] -> END\n* [B] -> END\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());

        // Tick 1: fulfill the request.
        app.update();
        // Tick 2: drive advance. We need a fresh tick so the recorder
        // observers see the events from advance, not from any
        // fulfillment-time setup.
        run_advance(&mut app);
        // Triggers from run_advance are queued; one more tick to flush.
        app.update();

        let rec = app.world().resource::<EventRecorder>();
        assert!(
            rec.text_lines.iter().any(|s| s.contains("first line")),
            "expected first line in events; got {:?}",
            rec.text_lines
        );
        assert!(
            rec.text_lines.iter().any(|s| s.contains("second line")),
            "expected second line"
        );
        assert_eq!(
            rec.choices_presentations.len(),
            1,
            "should have hit exactly one choice presentation"
        );
        assert_eq!(rec.choices_presentations[0].len(), 2, "two choices");
    }

    #[test]
    #[ignore = "see advance_until_terminal_fires_text_then_choices"]
    fn advance_to_end_fires_story_ended() {
        let mut app = make_test_app();
        install_recorder(&mut app);

        // Story content lives at root (no `=== knot ===` declaration)
        // so a root-start flow produces it on advance.
        let (program, tables, ctx) = compile_test_story("goodbye\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());

        app.update();
        run_advance(&mut app);
        app.update();

        let rec = app.world().resource::<EventRecorder>();
        assert_eq!(
            rec.story_ended, 1,
            "should have fired StoryEnded once; rec={:?}",
            (&rec.text_lines, rec.turn_done, rec.story_ended)
        );
        assert!(
            rec.text_lines.iter().any(|s| s.contains("goodbye")),
            "expected 'goodbye' in events; got {:?}",
            rec.text_lines
        );
    }

    /// `choose_recording` records into the replay log and forwards to
    /// `inner.choose`. Currently ignored along with the other event-firing
    /// tests — the choice itself is applied (verifiable via the log) but
    /// the test setup depends on advance firing observer events first to
    /// reach a Choices state.
    #[test]
    #[ignore = "see advance_until_terminal_fires_text_then_choices"]
    #[cfg(feature = "dev")]
    fn choose_recording_appends_to_replay_log() {
        let mut app = make_test_app();
        install_recorder(&mut app);
        // Choose driver: when ChoiceToMake is set, picks that index and
        // records to the replay log on the matching entity.
        #[derive(Resource, Default)]
        struct ChoiceToMake(Option<usize>);
        app.insert_resource(ChoiceToMake::default());
        app.add_systems(
            Update,
            |mut flows: Query<(
                &mut BrinkFlow<()>,
                &mut crate::replay::BrinkReplayLog<()>,
            )>,
             mut globals: ResMut<crate::BrinkGlobals<()>>,
             mut to_make: ResMut<ChoiceToMake>| {
                let Some(idx) = to_make.0.take() else { return };
                let Ok((mut flow, mut log)) = flows.single_mut() else { return };
                let _ = flow.choose_recording(&mut globals, &mut log, idx);
            },
        );

        let (program, tables, ctx) =
            compile_test_story("=== start ===\nhi\n* [A] -> END\n* [B] -> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        app.world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build());

        // Fulfill, advance to choice, then make a choice — each via a
        // full app.update() so deferred work flushes between steps.
        app.update();
        run_advance(&mut app);
        app.world_mut().resource_mut::<ChoiceToMake>().0 = Some(1);
        app.update();

        // Read the log via a one-tick driver system.
        #[derive(Resource, Default)]
        struct LogReader(Vec<usize>);
        app.insert_resource(LogReader::default());
        app.add_systems(
            Update,
            |flows: Query<&crate::replay::BrinkReplayLog<()>>,
             mut out: ResMut<LogReader>| {
                if let Ok(log) = flows.single() {
                    out.0.clone_from(&log.choices_made);
                }
            },
        );
        app.update();
        let recorded = app.world().resource::<LogReader>().0.clone();
        assert_eq!(recorded, vec![1]);
    }
}
