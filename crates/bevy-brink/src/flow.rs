//! Per-flow mutable state: call stacks, output buffer, pending choices.

use std::marker::PhantomData;

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::system::Commands;
use brink_format::LineEntry;
use brink_runtime::{
    ContextAccess, DriveOutcome, ExternalFnHandler, FastRng, FlowInstance, Line, Program,
    RuntimeError, StepOutcome,
};
#[cfg(feature = "dev")]
use brink_runtime::{RecordingHandler, ReplayRecorder};

use crate::event::{BrinkChoicesPresented, BrinkLineDelivered, BrinkStoryEnded, BrinkTurnDone};

/// Result of advancing a flow one step via [`BrinkFlow::step_one`].
#[derive(Debug, Clone)]
pub enum Advance {
    /// A line was produced (and its observer event fired).
    Line(Line),
    /// The flow paused on a world-access query binding (`bind_brink_query`),
    /// which can't be resolved from a non-exclusive system. The plugin's
    /// resolver (gated on
    /// [`any_flow_awaiting_external`](crate::any_flow_awaiting_external))
    /// runs the query against the World; call `step_one` again afterward to
    /// resume. Drivers should stop advancing this flow until then.
    ///
    /// (From an exclusive `&mut World` context, prefer
    /// [`advance_flow`](crate::advance_flow), which resolves the query
    /// inline and never yields this.)
    AwaitingQuery,
}

/// A single live ink flow, attached to an entity. Holds the VM's per-flow
/// state: call stacks, output buffer, pending choices, and the accumulated
/// transcript.
///
/// Spawn one of these per active conversation. Systems advance the flow by
/// calling methods on it against the per-flow
/// [`BrinkContext`](crate::BrinkContext) component, the current
/// [`Program`] from `Assets<ProgramAsset>`, and the current line tables
/// from `Assets<LineTablesAsset>` (looked up via the entity's
/// [`BrinkLocale`](crate::BrinkLocale)).
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

    /// Select a choice against `context` — build one via
    /// [`flow_context_view`](crate::globals::flow_context_view) over the
    /// entity's [`BrinkContext`](crate::BrinkContext) and the marker's
    /// shared [`BrinkGlobals`](crate::BrinkGlobals).
    pub fn choose(
        &mut self,
        context: &mut (impl ContextAccess + ?Sized),
        index: usize,
    ) -> Result<(), RuntimeError> {
        self.inner.choose(context, index)
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
        context: &mut (impl ContextAccess + ?Sized),
        log: &mut crate::replay::BrinkReplayLog<M>,
        index: usize,
    ) -> Result<(), RuntimeError> {
        log.choices_made.push(index);
        self.inner.choose(context, index)
    }

    /// Control whether visibility enforcement is active. When `false`,
    /// private (`#@private`) definitions can be accessed, enabling
    /// "play from here" and similar dev-time workflows. Default is `true`.
    ///
    /// This mirrors [`FlowInstance::set_visibility_enforcement`] for
    /// Bevy-integrated flows.
    pub fn set_visibility_enforcement(&mut self, enforce: bool) {
        self.inner.set_visibility_enforcement(enforce);
    }

    /// Whether visibility enforcement is currently on for this flow.
    /// Returns `true` by default (private definitions are inaccessible).
    #[must_use]
    pub fn visibility_enforced(&self) -> bool {
        self.inner.visibility_enforced()
    }

    /// Step the VM by one line and queue the corresponding observer
    /// event ([`BrinkLineDelivered`], [`BrinkChoicesPresented`],
    /// [`BrinkTurnDone`], or [`BrinkStoryEnded`]) for the entity.
    ///
    /// Use this for typewriter-style UIs that animate one fragment at a
    /// time. For click-to-continue dialogue, use
    /// [`advance_until_terminal`](Self::advance_until_terminal).
    ///
    /// Returns [`Advance::AwaitingQuery`] if the flow paused on a
    /// world-access query binding — the plugin resolver runs it, then call
    /// `step_one` again. (Pass `bindings.handler()` so query bindings pause
    /// rather than fall back.)
    pub fn step_one(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        entity: Entity,
        commands: &mut Commands,
    ) -> Result<Advance, RuntimeError> {
        match self
            .inner
            .advance::<FastRng>(program, line_tables, context, handler, None)?
        {
            StepOutcome::Line(line) => {
                emit_event::<M>(&line, entity, commands);
                Ok(Advance::Line(line))
            }
            StepOutcome::AwaitingExternal => Ok(Advance::AwaitingQuery),
        }
    }

    /// Step the VM until reaching a terminal line ([`Line::Done`],
    /// [`Line::Choices`], or [`Line::End`]), queuing observer events
    /// for every line produced along the way.
    ///
    /// Delegates to the shared Layer-2 [`FlowInstance::drive`] op (F6.2):
    /// each call gets a fresh [`FlowInstance::LINE_LIMIT`] line budget (this
    /// method's own historical per-call cap — a caller re-invoking it across
    /// frames after an `AwaitingQuery` pause starts a new logical drive each
    /// time, so a fresh budget per call is the right semantics here; see
    /// [`advance_flow`](crate::advance_flow) for the case that shares one
    /// budget across a single call's internal resumes). Returns the terminal
    /// line, or [`Advance::AwaitingQuery`] if the flow paused on a
    /// world-access query binding (which a non-exclusive driver can't
    /// resolve — the plugin resolver handles it; resume by calling this
    /// again).
    pub fn advance_until_terminal(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        entity: Entity,
        commands: &mut Commands,
    ) -> Result<Advance, RuntimeError> {
        let mut budget = FlowInstance::LINE_LIMIT;
        let outcome = self.inner.drive::<FastRng>(
            program,
            line_tables,
            context,
            handler,
            None,
            &mut budget,
        )?;
        emit_drive_outcome::<M>(outcome, entity, commands)
    }

    /// Like [`step_one`](Self::step_one) but records every external the VM
    /// resolves *inline* (pure/command bindings) into `recorder` — the flow's
    /// [`BrinkReplayLog`](crate::BrinkReplayLog) recorder — by wrapping
    /// `handler` in a [`RecordingHandler`]. World-access/async externals park
    /// (`AwaitingQuery`) and are recorded out-of-band by the plugin resolver
    /// when it supplies their value.
    ///
    /// Use this (rather than [`step_one`](Self::step_one)) on the non-exclusive
    /// playback path when you want a hot-reload to replay faithfully: only
    /// externals captured here and at the resolve sites feed the reload
    /// re-walk. Available only with the `dev` feature.
    #[cfg(feature = "dev")]
    #[expect(clippy::too_many_arguments, reason = "recording adds the recorder")]
    pub fn step_one_recording(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        recorder: &mut ReplayRecorder,
        entity: Entity,
        commands: &mut Commands,
    ) -> Result<Advance, RuntimeError> {
        let recording = RecordingHandler::new(handler, recorder);
        match self
            .inner
            .advance::<FastRng>(program, line_tables, context, &recording, None)?
        {
            StepOutcome::Line(line) => {
                emit_event::<M>(&line, entity, commands);
                Ok(Advance::Line(line))
            }
            StepOutcome::AwaitingExternal => Ok(Advance::AwaitingQuery),
        }
    }

    /// Recording counterpart to
    /// [`advance_until_terminal`](Self::advance_until_terminal): steps to a
    /// terminal line — via the same shared [`FlowInstance::drive`] op, fresh
    /// [`FlowInstance::LINE_LIMIT`] budget per call — while recording every
    /// inline external into `recorder` (see
    /// [`step_one_recording`](Self::step_one_recording)). Available only with
    /// the `dev` feature.
    #[cfg(feature = "dev")]
    #[expect(clippy::too_many_arguments, reason = "recording adds the recorder")]
    pub fn advance_until_terminal_recording(
        &mut self,
        program: &Program,
        line_tables: &[Vec<LineEntry>],
        context: &mut (impl ContextAccess + ?Sized),
        handler: &dyn ExternalFnHandler,
        recorder: &mut ReplayRecorder,
        entity: Entity,
        commands: &mut Commands,
    ) -> Result<Advance, RuntimeError> {
        let recording = RecordingHandler::new(handler, recorder);
        let mut budget = FlowInstance::LINE_LIMIT;
        let outcome = self.inner.drive::<FastRng>(
            program,
            line_tables,
            context,
            &recording,
            None,
            &mut budget,
        )?;
        emit_drive_outcome::<M>(outcome, entity, commands)
    }
}

/// Fire the per-line observer event for every [`Line`] a
/// [`FlowInstance::drive`] call produced, then report the result as an
/// [`Advance`]: the last (terminal) line for [`DriveOutcome::Terminal`], or
/// [`Advance::AwaitingQuery`] for [`DriveOutcome::AwaitingExternal`] (having
/// still fired events for whatever lines were produced before the pause).
///
/// Shared by [`BrinkFlow::advance_until_terminal`] and
/// [`BrinkFlow::advance_until_terminal_recording`].
fn emit_drive_outcome<M: Send + Sync + 'static>(
    outcome: DriveOutcome,
    entity: Entity,
    commands: &mut Commands,
) -> Result<Advance, RuntimeError> {
    match outcome {
        DriveOutcome::Terminal(lines) => {
            let mut iter = lines.into_iter();
            // `drive`'s `Terminal` variant is documented to always be
            // non-empty with a terminal last line — this `ok_or` is an
            // unreachable-in-practice guard, not a real failure mode.
            let mut last = iter.next().ok_or(RuntimeError::CallStackUnderflow)?;
            emit_event::<M>(&last, entity, commands);
            for line in iter {
                emit_event::<M>(&line, entity, commands);
                last = line;
            }
            Ok(Advance::Line(last))
        }
        DriveOutcome::AwaitingExternal(lines) => {
            for line in &lines {
                emit_event::<M>(line, entity, commands);
            }
            Ok(Advance::AwaitingQuery)
        }
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
        Line::Done { text, tags } => {
            commands.trigger(BrinkTurnDone::<M>::new(entity, text.clone(), tags.clone()));
        }
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
    use crate::BrinkFlowRequest;
    use crate::test_support::{add_story_assets, compile_test_story, compile_test_story_brink, make_test_app};
    use bevy_app::Update;
    use bevy_asset::Assets;
    use bevy_ecs::prelude::*;

    /// Recorder for observer events; tests assert against its contents.
    ///
    /// Note: terminal `Line` variants (`Done`, `Choices`, `End`) carry
    /// accumulated text in their own `text` field, not as a separate
    /// preceding `Line::Text`. So we capture text from every event and
    /// expose it as `all_text` for tests that just want "did this string
    /// appear anywhere."
    #[derive(Resource, Default)]
    struct EventRecorder {
        text_lines: Vec<String>,
        choices_text: Vec<String>,
        choices_presentations: Vec<Vec<String>>,
        turn_done_text: Vec<String>,
        story_ended_text: Vec<String>,
        turn_done: u32,
        story_ended: u32,
        #[cfg(feature = "dev")]
        flow_resets: u32,
    }

    impl EventRecorder {
        /// Wipe all recorded events. Useful for "what happened *after*
        /// this point" tests (e.g. re-deliveries during hot-reload).
        fn clear(&mut self) {
            *self = Self::default();
        }

        fn all_text(&self) -> String {
            let mut s = String::new();
            for t in &self.text_lines {
                s.push_str(t);
            }
            for t in &self.choices_text {
                s.push_str(t);
            }
            for t in &self.turn_done_text {
                s.push_str(t);
            }
            for t in &self.story_ended_text {
                s.push_str(t);
            }
            s
        }
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
                rec.choices_text.push(trigger.event().text.clone());
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
            |trigger: On<BrinkTurnDone<()>>, mut rec: ResMut<EventRecorder>| {
                rec.turn_done_text.push(trigger.event().text.clone());
                rec.turn_done += 1;
            },
        );
        app.add_observer(
            |trigger: On<BrinkStoryEnded<()>>, mut rec: ResMut<EventRecorder>| {
                rec.story_ended_text.push(trigger.event().text.clone());
                rec.story_ended += 1;
            },
        );
        #[cfg(feature = "dev")]
        app.add_observer(
            |_: On<crate::BrinkFlowReset<()>>, mut rec: ResMut<EventRecorder>| {
                rec.flow_resets += 1;
            },
        );
    }

    /// Resource that, when set to `true`, causes `advance_driver_system`
    /// (scheduled in `Update`) to advance every flow this tick. Set
    /// `true`, run `app.update()`, then read the recorder.
    #[derive(Resource, Default)]
    struct DriveAdvance(bool);

    /// Drive a single advance pass and flush its triggers via the
    /// regular Update schedule (`RunSystemOnce` doesn't reliably flush
    /// queued observer triggers in 0.18 test contexts).
    fn run_advance(app: &mut bevy_app::App) {
        app.world_mut().resource_mut::<DriveAdvance>().0 = true;
        app.update();
    }

    #[expect(
        clippy::needless_pass_by_value,
        clippy::type_complexity,
        reason = "bevy systems take Res by value and have complex query tuples"
    )]
    fn advance_driver_system(
        mut flows: Query<(
            Entity,
            &mut BrinkFlow<()>,
            &mut crate::BrinkContext<()>,
            &crate::BrinkProgram<()>,
            &crate::BrinkLocale<()>,
        )>,
        globals: Option<ResMut<crate::BrinkGlobals<()>>>,
        programs: Res<Assets<crate::ProgramAsset>>,
        line_tables_assets: Res<Assets<crate::LineTablesAsset>>,
        mut drive: ResMut<DriveAdvance>,
        mut commands: Commands,
    ) {
        if !drive.0 {
            return;
        }
        drive.0 = false;
        let Some(mut globals) = globals else {
            eprintln!("[advance] no BrinkGlobals yet");
            return;
        };
        eprintln!("[advance] running");
        let count = flows.iter().count();
        eprintln!("[advance] {count} flow(s)");
        for (entity, mut flow, mut ctx, brink_program, locale) in &mut flows {
            let Some(program_asset) = programs.get(&brink_program.handle) else {
                eprintln!("[advance] no program asset");
                continue;
            };
            let Some(lt_asset) = line_tables_assets.get(&locale.handle) else {
                eprintln!("[advance] no line tables asset");
                continue;
            };
            let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
            let result = flow.advance_until_terminal(
                &program_asset.program,
                &lt_asset.tables,
                &mut view,
                &brink_runtime::FallbackHandler,
                entity,
                &mut commands,
            );
            eprintln!(
                "[advance] result={:?}, status={:?}",
                result.is_ok(),
                flow.inner.status()
            );
        }
    }

    /// Smoke test: does `commands.trigger` from a `Update` system fire
    /// observers at all in this minimal app setup? Isolates the dispatch
    /// path from the rest of the flow machinery.
    #[derive(Resource, Default)]
    struct Hits(u32);

    #[test]
    fn smoke_commands_trigger_fires_observers() {
        let mut app = make_test_app();
        app.insert_resource(Hits::default());
        app.add_observer(|_: On<BrinkLineDelivered<()>>, mut h: ResMut<Hits>| {
            h.0 += 1;
        });
        app.add_systems(Update, |mut commands: Commands| {
            commands.trigger(BrinkLineDelivered::<()>::new(
                Entity::PLACEHOLDER,
                "hello".into(),
                vec![],
            ));
        });

        app.update();
        let hits = app.world().resource::<Hits>().0;
        assert!(hits >= 1, "trigger from system did not reach observer");
    }

    /// Root-start flow walks text content and reaches a Choices line.
    /// Content is at root (not under `=== knot ===`) because `FlowStart::Root`
    /// runs the file's root container — it does not auto-enter a named knot.
    /// The Choices event itself carries any accumulated text in its `text`
    /// field, so we check for the strings via `all_text()`.
    #[test]
    fn advance_until_terminal_fires_text_then_choices() {
        let mut app = make_test_app();
        install_recorder(&mut app);

        let (program, tables, ctx) =
            compile_test_story("first line\nsecond line\n* [A] -> END\n* [B] -> END\n");
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
        let all = rec.all_text();
        assert!(
            all.contains("first line"),
            "expected 'first line' in events; got text_lines={:?} choices_text={:?}",
            rec.text_lines,
            rec.choices_text,
        );
        assert!(
            all.contains("second line"),
            "expected 'second line' in events; got {all:?}",
        );
        assert_eq!(
            rec.choices_presentations.len(),
            1,
            "should have hit exactly one choice presentation"
        );
        assert_eq!(rec.choices_presentations[0].len(), 2, "two choices");
    }

    #[test]
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
            rec.story_ended,
            1,
            "should have fired StoryEnded once; rec={:?}",
            (&rec.text_lines, rec.turn_done, rec.story_ended)
        );
        // The "goodbye" text is delivered as part of the End event's own
        // text field, not as a preceding Line::Text — terminal lines
        // bundle accumulated content.
        assert!(
            rec.all_text().contains("goodbye"),
            "expected 'goodbye' somewhere in events; story_ended_text={:?} text_lines={:?}",
            rec.story_ended_text,
            rec.text_lines,
        );
    }

    /// `choose_recording` records into the replay log and forwards to
    /// `inner.choose`. Drives a flow to a choice, picks a choice, asserts
    /// the choice index lands in the replay log.
    #[test]
    #[cfg(feature = "dev")]
    fn choose_recording_appends_to_replay_log() {
        // Choose driver: when ChoiceToMake is set, picks that index and
        // records to the replay log on the matching entity.
        #[derive(Resource, Default)]
        struct ChoiceToMake(Option<usize>);
        // Reads the replay log back out via a one-tick driver system.
        #[derive(Resource, Default)]
        struct LogReader(Vec<usize>);

        let mut app = make_test_app();
        install_recorder(&mut app);
        app.insert_resource(ChoiceToMake::default());
        app.add_systems(
            Update,
            |mut flows: Query<(
                &mut BrinkFlow<()>,
                &mut crate::BrinkContext<()>,
                &mut crate::replay::BrinkReplayLog<()>,
            )>,
             globals: Option<ResMut<crate::BrinkGlobals<()>>>,
             mut to_make: ResMut<ChoiceToMake>| {
                let Some(mut globals) = globals else { return };
                let Some(idx) = to_make.0.take() else { return };
                let Ok((mut flow, mut ctx, mut log)) = flows.single_mut() else {
                    return;
                };
                let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
                let _ = flow.choose_recording(&mut view, &mut log, idx);
            },
        );

        // Content at root so the root-start flow reaches the choice
        // (named knots are not auto-entered by FlowStart::Root).
        let (program, tables, ctx) = compile_test_story("hi\n* [A] -> END\n* [B] -> END\n");
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
        app.insert_resource(LogReader::default());
        app.add_systems(
            Update,
            |flows: Query<&crate::replay::BrinkReplayLog<()>>, mut out: ResMut<LogReader>| {
                if let Ok(log) = flows.single() {
                    out.0.clone_from(&log.choices_made);
                }
            },
        );
        app.update();
        let recorded = app.world().resource::<LogReader>().0.clone();
        assert_eq!(recorded, vec![1]);
    }

    /// Test that `set_visibility_enforcement` and `visibility_enforced`
    /// control access to private definitions. With enforcement on (default),
    /// private knots are rejected. With enforcement off, they're allowed.
    #[test]
    fn visibility_enforcement_controls_private_access() {
        let (program, tables, ctx) = compile_test_story_brink(
            "#@private\nVAR secret = 5\n\
             == start ==\nPublic\n-> DONE\n\
             == private_knot ==\n#@private\nSecret\n-> DONE\n",
        );
        let mut app = make_test_app();
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        app.update();

        // Get a mutable reference to the BrinkFlow and test enforcement
        let mut flow = app
            .world_mut()
            .get_mut::<BrinkFlow<()>>(entity)
            .expect("entity should have BrinkFlow");

        // By default, visibility enforcement should be on
        assert!(
            flow.visibility_enforced(),
            "visibility enforcement should be on by default"
        );

        // Disable visibility enforcement
        flow.set_visibility_enforcement(false);
        assert!(
            !flow.visibility_enforced(),
            "visibility enforcement should be off after set_visibility_enforcement(false)"
        );

        // Re-enable it
        flow.set_visibility_enforcement(true);
        assert!(
            flow.visibility_enforced(),
            "visibility enforcement should be back on after set_visibility_enforcement(true)"
        );
    }

    /// Sanity test: when only the asset event fires (with content
    /// unchanged), `replay_on_reload` correctly fires `BrinkFlowReset`
    /// and re-delivers the current page. Doesn't exercise any failure
    /// modes that depend on the new program differing from the old —
    /// see `hot_reload_with_new_content_*` for those.
    #[test]
    #[cfg(feature = "dev")]
    fn hot_reload_redelivers_current_page_events() {
        let mut app = make_test_app();
        install_recorder(&mut app);

        let (program, tables, ctx) = compile_test_story("hello\n* [A] -> END\n* [B] -> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        // Tick 1: fulfill request. Tick 2: drive advance to the choice.
        app.update();
        run_advance(&mut app);
        app.update();

        let before = app.world().resource::<EventRecorder>();
        eprintln!(
            "[before-reload] text_lines={:?} choices={} resets={}",
            before.text_lines,
            before.choices_presentations.len(),
            before.flow_resets,
        );
        assert_eq!(
            before.choices_presentations.len(),
            1,
            "test setup: should have reached the choice page before reload"
        );

        // Reset the recorder so post-reload assertions see only events
        // delivered as a result of the simulated hot-reload.
        app.world_mut().resource_mut::<EventRecorder>().clear();

        // Simulate the hot-reload: file watcher would normally re-issue
        // the asset; here we just mutate the program asset in place,
        // which queues `AssetEvent::Modified` and triggers `replay_on_reload`.
        let program_handle = app
            .world()
            .entity(entity)
            .get::<crate::BrinkProgram<()>>()
            .expect("entity should have BrinkProgram after fulfillment")
            .handle
            .clone();
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<bevy_asset::Assets<crate::ProgramAsset>>();
            // Bevy 0.19: `Assets::get_mut` returns a change-tracking `AssetMut`
            // guard that only queues `AssetEvent::Modified` if the value is
            // actually dereferenced mutably (see the migration note on
            // `AssetMut`). We're simulating "file watcher re-issued the same
            // content", so force that dereference without changing anything.
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                let _ = &mut *slot;
            }
        }

        // Tick: asset_events system propagates the queued event into the
        // MessageReader, then replay_on_reload sees it and rebuilds.
        app.update();
        // Second tick: any deferred triggers from the reload propagate.
        app.update();

        let after = app.world().resource::<EventRecorder>();
        eprintln!(
            "[after-reload] text_lines={:?} choices_text={:?} \
             choices_presentations={} turn_done={} story_ended={} \
             resets={}",
            after.text_lines,
            after.choices_text,
            after.choices_presentations.len(),
            after.turn_done,
            after.story_ended,
            after.flow_resets,
        );

        assert_eq!(
            after.flow_resets, 1,
            "expected exactly one BrinkFlowReset after the reload"
        );
        assert!(
            !after.all_text().is_empty() || !after.choices_presentations.is_empty(),
            "expected SOME post-reload events; got nothing — this is \
             the 'blank page after reload' symptom"
        );
        assert_eq!(
            after.choices_presentations.len(),
            1,
            "post-replay current page should be the choice again \
             (no choices recorded → replay log empty → flow restarts \
             at root → walks to the same Choices)"
        );
    }

    /// "Render harness" — mirrors what `play_story.rs` does for the
    /// visual example: a `PageText`/`PendingChoices`/`Banner` triple
    /// driven by observers, plus a render function that produces the
    /// same string the example would display.
    ///
    /// Tests that use this can call `simulate_render(&app)` to get the
    /// exact rendered output without any windowing.
    #[cfg(feature = "dev")]
    mod render_harness {
        use super::*;
        use std::fmt::Write as _;

        #[derive(Resource, Default)]
        pub struct PageText(pub String);

        #[derive(Resource, Default)]
        pub struct PendingChoices(pub Vec<String>);

        #[derive(Resource, Default)]
        pub struct Banner(pub String);

        pub fn install(app: &mut bevy_app::App) {
            app.insert_resource(PageText::default());
            app.insert_resource(PendingChoices::default());
            app.insert_resource(Banner::default());
            app.add_observer(
                |trigger: On<BrinkLineDelivered<()>>, mut page: ResMut<PageText>| {
                    page.0.push_str(&trigger.event().text);
                },
            );
            app.add_observer(
                |trigger: On<BrinkChoicesPresented<()>>,
                 mut page: ResMut<PageText>,
                 mut choices: ResMut<PendingChoices>| {
                    page.0.push_str(&trigger.event().text);
                    choices.0 = trigger
                        .event()
                        .choices
                        .iter()
                        .map(|c| c.text.clone())
                        .collect();
                },
            );
            app.add_observer(
                |trigger: On<BrinkTurnDone<()>>, mut page: ResMut<PageText>| {
                    page.0.push_str(&trigger.event().text);
                },
            );
            app.add_observer(
                |trigger: On<BrinkStoryEnded<()>>,
                 mut page: ResMut<PageText>,
                 mut banner: ResMut<Banner>| {
                    page.0.push_str(&trigger.event().text);
                    banner.0 = "Story ended.".to_string();
                },
            );
            app.add_observer(
                |_: On<crate::BrinkFlowReset<()>>,
                 mut page: ResMut<PageText>,
                 mut choices: ResMut<PendingChoices>,
                 mut banner: ResMut<Banner>| {
                    page.0.clear();
                    choices.0.clear();
                    banner.0 = "Reloaded.".to_string();
                },
            );
        }

        /// Produce the string the `play_story` example would render, given
        /// the current state of the page/choices/banner resources.
        pub fn render(app: &bevy_app::App) -> String {
            let page = &app.world().resource::<PageText>().0;
            let choices = &app.world().resource::<PendingChoices>().0;
            let banner = &app.world().resource::<Banner>().0;
            let mut out = String::new();
            if !banner.is_empty() {
                let _ = writeln!(out, "[banner] {banner}");
            }
            if choices.is_empty() {
                if page.is_empty() {
                    out.push_str("(press SPACE to begin)");
                } else {
                    out.push_str(page);
                }
            } else {
                out.push_str(page);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                for (i, c) in choices.iter().enumerate() {
                    let _ = write!(out, "\n  [{}] {}", i + 1, c);
                }
            }
            out
        }
    }

    /// Simulate a "real" hot-reload where the program asset's content
    /// is replaced with a freshly-compiled version. Mirrors the path
    /// the file watcher takes (mod the loader replay) — the `AssetId`
    /// is stable but the contents change.
    ///
    /// Returns what the `play_story` UI would render before vs. after.
    /// This exposes the line-tables-not-refreshed bug if it exists:
    /// the rendered text after reload should reflect the new story
    /// content, not the old.
    #[test]
    #[cfg(feature = "dev")]
    fn hot_reload_with_new_content_renders_new_text() {
        use bevy_asset::Assets;

        let mut app = make_test_app();
        install_recorder(&mut app);
        render_harness::install(&mut app);

        // First version of the story: simple choice.
        let (program_v1, tables_v1, ctx_v1) =
            compile_test_story("hello\n* [yes] -> END\n* [no] -> END\n");
        let story = add_story_assets(&mut app, program_v1, tables_v1, ctx_v1);
        let entity = app
            .world_mut()
            .spawn(
                BrinkFlowRequest::<()>::builder()
                    .story(story.clone())
                    .build(),
            )
            .id();

        app.update();
        run_advance(&mut app);
        app.update();

        let rendered_before = render_harness::render(&app);
        eprintln!("[BEFORE RELOAD]\n{rendered_before}\n");

        // Compile a SECOND version of the story with completely
        // different text. Replace the contents of the existing
        // ProgramAsset and LineTablesAsset slots — this is closest to
        // what the file watcher / labeled-subasset reload does.
        let (program_v2, tables_v2, _ctx_v2) =
            compile_test_story("BRAND NEW WORDS\n* [foo] -> END\n* [bar] -> END\n");

        let program_handle = app
            .world()
            .entity(entity)
            .get::<crate::BrinkProgram<()>>()
            .expect("entity should have BrinkProgram")
            .handle
            .clone();
        let line_tables_handle = {
            let bundle_handle = story.clone();
            let stories = app.world().resource::<Assets<crate::BrinkStoryAsset>>();
            stories.get(&bundle_handle).unwrap().line_tables.clone()
        };

        // Replace program content (also fires AssetEvent::Modified).
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<Assets<crate::ProgramAsset>>();
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                slot.program = program_v2;
            }
        }
        // Replace line-tables content too.
        {
            let mut tables = app
                .world_mut()
                .resource_mut::<Assets<crate::LineTablesAsset>>();
            if let Some(mut slot) = tables.get_mut(&line_tables_handle) {
                slot.tables = tables_v2;
            }
        }

        // Two ticks: first lets asset_events propagate, second flushes
        // any deferred triggers from replay.
        app.update();
        app.update();

        let rendered_after = render_harness::render(&app);
        eprintln!("[AFTER RELOAD]\n{rendered_after}\n");

        // Hard assertion: the rendered output must contain the new
        // story's text, not the old story's text.
        assert!(
            rendered_after.contains("BRAND NEW WORDS"),
            "expected new story content in rendered output;\n\
             got: {rendered_after:?}\n\
             this is the 'blank page' / 'wrong text' bug"
        );
        assert!(
            !rendered_after.contains("hello"),
            "old story content should not appear after reload; got: {rendered_after:?}"
        );
        assert!(
            rendered_after.contains("foo"),
            "new choice 'foo' should be in rendered output; got: {rendered_after:?}"
        );
    }

    /// End-to-end Recorded-mode replay (#173/#189): a query-gated branch
    /// resolved during live play is recorded and replayed *faithfully* on
    /// hot-reload — even though the live World state has since changed, the
    /// reload re-walk uses the recorded value (it re-runs no externals), so the
    /// branch it took during play is the branch it shows after reload.
    #[test]
    #[cfg(feature = "dev")]
    #[expect(clippy::too_many_lines, reason = "end-to-end replay test")]
    fn hot_reload_replays_recorded_query_branch() {
        use crate::{BrinkBindingsAppExt, BrinkQueryInput, Value, advance_flow};
        use bevy_asset::Assets;

        // A world-access binding whose value flips between play and reload.
        #[derive(Resource)]
        struct SwitchState(bool);
        #[expect(
            clippy::needless_pass_by_value,
            reason = "bevy system params are taken by value"
        )]
        fn get_switch(In((_e, _args)): In<BrinkQueryInput>, state: Res<SwitchState>) -> Value {
            Value::Bool(state.0)
        }

        let mut app = make_test_app();
        install_recorder(&mut app);
        render_harness::install(&mut app);
        app.insert_resource(SwitchState(true));
        app.bind_brink_query::<(), _, _>("get_switch", get_switch);

        // The query gate is *after* a choice, so the test exercises both
        // recorded-choice replay and recorded-query fidelity in one walk.
        //
        // F6.2 semantic-flip note: the choice is `+` (sticky), not `*`
        // (once-only). A `*` choice's "already used" flag is an interior
        // container visit count — under F6.2's shared-by-default `World`,
        // that count is NOT reset before a hot-reload re-walk (see
        // `BrinkReplayLog`'s "known limitation" doc in `replay.rs`: the
        // shared `World` can't be time-traveled back to "before this flow's
        // own prior play" the way a pre-F6.2 private per-flow `World` could
        // be). A `*` choice here would therefore be silently skipped on
        // replay (already "used" by the live playthrough moments earlier)
        // — a real, orthogonal consequence of the shared-world model, not a
        // regression in replay/query fidelity, which is what this test is
        // actually about. `+` sidesteps it cleanly.
        let (program, tables, ctx) = compile_test_story(
            "EXTERNAL get_switch(n)\nStart.\n+ [go] -> after\n\
             === after ===\nSwitch is {get_switch(1): ON|OFF}.\n-> END\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill

        // Drive one line at a time to the choice via the exclusive driver
        // (this is the recording path).
        let advance_to_terminal = |app: &mut bevy_app::App| -> Line {
            loop {
                let line = advance_flow::<()>(app.world_mut(), entity).expect("advance");
                if line.is_terminal() {
                    return line;
                }
            }
        };

        let at_choice = advance_to_terminal(&mut app);
        assert!(
            matches!(at_choice, Line::Choices { .. }),
            "expected the choice page; got {at_choice:?}"
        );

        // Pick [go], recording the choice, then advance through the gate —
        // get_switch(1) resolves to true (SwitchState) and is recorded.
        {
            type ChooseState = bevy_ecs::system::SystemState<(
                Query<
                    'static,
                    'static,
                    (
                        &'static mut BrinkFlow<()>,
                        &'static mut crate::BrinkContext<()>,
                        &'static mut crate::replay::BrinkReplayLog<()>,
                    ),
                >,
                ResMut<'static, crate::BrinkGlobals<()>>,
            )>;
            let mut state: ChooseState = bevy_ecs::system::SystemState::new(app.world_mut());
            let (mut q, mut globals) = state.get_mut(app.world_mut()).expect("system params");
            let (mut flow, mut ctx, mut log) = q.get_mut(entity).expect("flow components");
            let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
            flow.choose_recording(&mut view, &mut log, 0)
                .expect("choose");
        }
        let ended = advance_to_terminal(&mut app);
        assert!(
            ended.text().contains("Switch is ON."),
            "live play should take the ON branch; got {:?}",
            ended.text()
        );

        // The query result must have been recorded.
        let recorded = app
            .world()
            .entity(entity)
            .get::<crate::replay::BrinkReplayLog<()>>()
            .expect("replay log")
            .recorder
            .len();
        assert_eq!(
            recorded, 1,
            "get_switch(1) should be the one recorded external"
        );

        // Flip the live World state: a *live* re-query would now answer OFF.
        app.world_mut().resource_mut::<SwitchState>().0 = false;

        // Simulate a hot-reload: touching the program asset fires
        // AssetEvent::Modified, which drives replay_on_reload over the same
        // (unchanged) program — so any divergence is purely the handler's.
        let program_handle = app
            .world()
            .entity(entity)
            .get::<crate::BrinkProgram<()>>()
            .expect("BrinkProgram")
            .handle
            .clone();
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<Assets<crate::ProgramAsset>>();
            // Bevy 0.19: `Assets::get_mut` returns a change-tracking `AssetMut`
            // guard that only queues `AssetEvent::Modified` if the value is
            // actually dereferenced mutably (see the migration note on
            // `AssetMut`). We're simulating "file watcher re-issued the same
            // content", so force that dereference without changing anything.
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                let _ = &mut *slot;
            }
        }
        app.update(); // asset_events → replay_on_reload rebuilds + replays
        app.update(); // flush deferred triggers from replay

        // The reload re-walk replayed the recorded `true`, so the page shows
        // the ON branch — not OFF (what a live re-query would now produce) and
        // not a fallback approximation.
        let rendered_after = render_harness::render(&app);
        assert!(
            rendered_after.contains("Switch is ON."),
            "reload should replay the recorded ON branch; got: {rendered_after:?}"
        );
        assert!(
            !rendered_after.contains("OFF"),
            "reload must not re-query live (which would now be OFF); got: {rendered_after:?}"
        );
    }

    /// End-to-end Recorded-mode replay on the *non-exclusive* `step_one` path:
    /// `step_one_recording` captures an inline pure binding while the plugin's
    /// `resolve_pending_externals` records an out-of-band world query — both, in
    /// VM order, replay faithfully on hot-reload even after the live state flips.
    #[test]
    #[cfg(feature = "dev")]
    #[expect(clippy::too_many_lines, reason = "end-to-end replay test")]
    fn hot_reload_replays_recorded_externals_on_non_exclusive_path() {
        use crate::{
            BrinkBindings, BrinkBindingsAppExt, BrinkLocale, BrinkProgram, BrinkQueryInput, Value,
        };
        use bevy_asset::Assets;

        #[derive(Resource)]
        struct SwitchState(bool);
        #[derive(Resource, Default)]
        struct DriveOn(bool);
        #[expect(
            clippy::needless_pass_by_value,
            reason = "bevy system params are taken by value"
        )]
        fn get_switch(In((_e, _a)): In<BrinkQueryInput>, s: Res<SwitchState>) -> Value {
            Value::Bool(s.0)
        }

        // Non-exclusive recording driver (gated by DriveOn so it doesn't run
        // during the simulated reload): advance each non-parked flow to terminal
        // via `step_one_recording`; the plugin's resolver records + resolves the
        // parked query out-of-band.
        #[expect(
            clippy::type_complexity,
            clippy::needless_pass_by_value,
            reason = "bevy system params"
        )]
        fn driver(
            mut flows: Query<(
                Entity,
                &mut BrinkFlow<()>,
                &mut crate::BrinkContext<()>,
                &mut crate::replay::BrinkReplayLog<()>,
                &BrinkProgram<()>,
                &BrinkLocale<()>,
            )>,
            globals: Option<ResMut<crate::BrinkGlobals<()>>>,
            programs: Res<Assets<crate::ProgramAsset>>,
            tables: Res<Assets<crate::LineTablesAsset>>,
            bindings: Res<BrinkBindings<()>>,
            drive: Res<DriveOn>,
            mut commands: Commands,
        ) {
            if !drive.0 {
                return;
            }
            let Some(mut globals) = globals else {
                return;
            };
            for (entity, mut flow, mut ctx, mut log, prog, loc) in &mut flows {
                if flow.inner.has_pending_external() {
                    continue;
                }
                let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle))
                else {
                    continue;
                };
                let handler = bindings.handler();
                let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
                let _ = flow.advance_until_terminal_recording(
                    &p.program,
                    &t.tables,
                    &mut view,
                    &handler,
                    &mut log.recorder,
                    entity,
                    &mut commands,
                );
                handler.flush(&mut commands);
            }
        }

        let mut app = make_test_app();
        install_recorder(&mut app);
        render_harness::install(&mut app);
        app.insert_resource(SwitchState(true));
        app.init_resource::<DriveOn>();
        app.bind_brink_fn::<(), _, _>("dbl", |a| {
            a.first().and_then(Value::as_int).unwrap_or(0) * 2
        });
        app.bind_brink_query::<(), _, _>("get_switch", get_switch);
        app.add_systems(Update, driver);

        let (program, tables, ctx) = compile_test_story(
            "EXTERNAL dbl(x)\nEXTERNAL get_switch(n)\n\
             Val {dbl(20)} switch {get_switch(1): ON|OFF}.\n-> END\n",
        );
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        // Drive across frames: query parks, the resolver resolves it, then the
        // driver resumes to the end.
        app.world_mut().resource_mut::<DriveOn>().0 = true;
        for _ in 0..8 {
            app.update();
        }

        // Both externals were recorded, in VM order (pure dbl, then the query).
        let len = app
            .world()
            .entity(entity)
            .get::<crate::replay::BrinkReplayLog<()>>()
            .expect("replay log")
            .recorder
            .len();
        assert_eq!(len, 2, "dbl + get_switch should both be recorded");

        // Stop driving and flip the live state: a re-run would now answer OFF.
        app.world_mut().resource_mut::<DriveOn>().0 = false;
        app.world_mut().resource_mut::<SwitchState>().0 = false;

        let program_handle = app
            .world()
            .entity(entity)
            .get::<BrinkProgram<()>>()
            .expect("BrinkProgram")
            .handle
            .clone();
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<Assets<crate::ProgramAsset>>();
            // Bevy 0.19: `Assets::get_mut` returns a change-tracking `AssetMut`
            // guard that only queues `AssetEvent::Modified` if the value is
            // actually dereferenced mutably (see the migration note on
            // `AssetMut`). We're simulating "file watcher re-issued the same
            // content", so force that dereference without changing anything.
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                let _ = &mut *slot;
            }
        }
        app.update();
        app.update();

        let rendered = render_harness::render(&app);
        assert!(
            rendered.contains("Val 40 switch ON."),
            "reload should replay recorded dbl=40 + get_switch=ON; got: {rendered:?}"
        );
        assert!(
            !rendered.contains("OFF"),
            "reload must not re-run externals live (which would be OFF); got: {rendered:?}"
        );
    }

    /// A `bind_brink_task` external recorded during play replays *synchronously*
    /// on hot-reload: the reload re-walk serves the recorded value through
    /// `ReplayHandler`, so no task is spawned and the branch reproduces.
    #[test]
    #[cfg(feature = "dev")]
    #[expect(clippy::too_many_lines, reason = "end-to-end replay test")]
    fn hot_reload_replays_recorded_task_result() {
        use crate::{BrinkBindings, BrinkBindingsAppExt, BrinkLocale, BrinkProgram, Value};
        use bevy_asset::Assets;

        #[derive(Resource, Default)]
        struct DriveOn(bool);

        #[expect(
            clippy::type_complexity,
            clippy::needless_pass_by_value,
            reason = "bevy system params"
        )]
        fn driver(
            mut flows: Query<(
                Entity,
                &mut BrinkFlow<()>,
                &mut crate::BrinkContext<()>,
                &mut crate::replay::BrinkReplayLog<()>,
                &BrinkProgram<()>,
                &BrinkLocale<()>,
            )>,
            globals: Option<ResMut<crate::BrinkGlobals<()>>>,
            programs: Res<Assets<crate::ProgramAsset>>,
            tables: Res<Assets<crate::LineTablesAsset>>,
            bindings: Res<BrinkBindings<()>>,
            drive: Res<DriveOn>,
            mut commands: Commands,
        ) {
            if !drive.0 {
                return;
            }
            let Some(mut globals) = globals else {
                return;
            };
            for (entity, mut flow, mut ctx, mut log, prog, loc) in &mut flows {
                if flow.inner.has_pending_external() {
                    continue;
                }
                let (Some(p), Some(t)) = (programs.get(&prog.handle), tables.get(&loc.handle))
                else {
                    continue;
                };
                let handler = bindings.handler();
                let mut view = crate::globals::flow_context_view(&mut globals, &mut ctx);
                let _ = flow.advance_until_terminal_recording(
                    &p.program,
                    &t.tables,
                    &mut view,
                    &handler,
                    &mut log.recorder,
                    entity,
                    &mut commands,
                );
                handler.flush(&mut commands);
            }
        }

        let mut app = make_test_app();
        install_recorder(&mut app);
        render_harness::install(&mut app);
        app.init_resource::<DriveOn>();
        app.bind_brink_task::<(), _, _>("roll", |args: Vec<Value>| async move {
            let n = args.first().and_then(Value::as_int).unwrap_or(0);
            Value::Int(n * 2)
        });
        app.add_systems(Update, driver);

        let (program, tables, ctx) =
            compile_test_story("EXTERNAL roll(n)\nRolled {roll(21)}.\n-> END\n");
        let story = add_story_assets(&mut app, program, tables, ctx);
        let entity = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();

        // Drive until the task resolves (poll_brink_tasks records its value).
        app.world_mut().resource_mut::<DriveOn>().0 = true;
        let mut played = false;
        for _ in 0..200 {
            app.update();
            if render_harness::render(&app).contains("Rolled 42.") {
                played = true;
                break;
            }
        }
        assert!(played, "task should resolve to 42 during play");
        let len = app
            .world()
            .entity(entity)
            .get::<crate::replay::BrinkReplayLog<()>>()
            .expect("replay log")
            .recorder
            .len();
        assert_eq!(len, 1, "roll should be recorded once");

        // Reload: the recorded value is replayed inline, so no task is spawned.
        app.world_mut().resource_mut::<DriveOn>().0 = false;
        let program_handle = app
            .world()
            .entity(entity)
            .get::<BrinkProgram<()>>()
            .expect("BrinkProgram")
            .handle
            .clone();
        {
            let mut programs = app
                .world_mut()
                .resource_mut::<Assets<crate::ProgramAsset>>();
            // Bevy 0.19: `Assets::get_mut` returns a change-tracking `AssetMut`
            // guard that only queues `AssetEvent::Modified` if the value is
            // actually dereferenced mutably (see the migration note on
            // `AssetMut`). We're simulating "file watcher re-issued the same
            // content", so force that dereference without changing anything.
            if let Some(mut slot) = programs.get_mut(&program_handle) {
                let _ = &mut *slot;
            }
        }
        app.update();
        app.update();

        let rendered = render_harness::render(&app);
        assert!(
            rendered.contains("Rolled 42."),
            "reload should replay the recorded task value synchronously; got: {rendered:?}"
        );
        assert!(
            app.world()
                .entity(entity)
                .get::<crate::BrinkPendingTask<()>>()
                .is_none(),
            "reload should not spawn a task (the value is replayed)"
        );
    }
}
