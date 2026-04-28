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

    /// Smoke test: does `commands.trigger` from a `Update` system fire
    /// observers at all in this minimal app setup? Isolates the dispatch
    /// path from the rest of the flow machinery.
    #[derive(Resource, Default)]
    struct Hits(u32);

    #[test]
    fn smoke_commands_trigger_fires_observers() {
        let mut app = make_test_app();
        app.insert_resource(Hits::default());
        app.add_observer(
            |_: On<BrinkLineDelivered<()>>, mut h: ResMut<Hits>| {
                h.0 += 1;
            },
        );
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

        let (program, tables, ctx) = compile_test_story(
            "first line\nsecond line\n* [A] -> END\n* [B] -> END\n",
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
            rec.story_ended, 1,
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
        let mut app = make_test_app();
        install_recorder(&mut app);
        // Choose driver: when ChoiceToMake is set, picks that index and
        // records to the replay log on the matching entity. `BrinkGlobals`
        // is `Option` because it doesn't exist until the first fulfillment.
        #[derive(Resource, Default)]
        struct ChoiceToMake(Option<usize>);
        app.insert_resource(ChoiceToMake::default());
        app.add_systems(
            Update,
            |mut flows: Query<(
                &mut BrinkFlow<()>,
                &mut crate::replay::BrinkReplayLog<()>,
            )>,
             globals: Option<ResMut<crate::BrinkGlobals<()>>>,
             mut to_make: ResMut<ChoiceToMake>| {
                let Some(idx) = to_make.0.take() else { return };
                let Some(mut globals) = globals else { return };
                let Ok((mut flow, mut log)) = flows.single_mut() else { return };
                let _ = flow.choose_recording(&mut globals, &mut log, idx);
            },
        );

        // Content at root so the root-start flow reaches the choice
        // (named knots are not auto-entered by FlowStart::Root).
        let (program, tables, ctx) =
            compile_test_story("hi\n* [A] -> END\n* [B] -> END\n");
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

        let (program, tables, ctx) =
            compile_test_story("hello\n* [A] -> END\n* [B] -> END\n");
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
            let _ = programs.get_mut(&program_handle);
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
            .spawn(BrinkFlowRequest::<()>::builder().story(story.clone()).build())
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
            if let Some(slot) = programs.get_mut(&program_handle) {
                slot.program = program_v2;
            }
        }
        // Replace line-tables content too.
        {
            let mut tables = app
                .world_mut()
                .resource_mut::<Assets<crate::LineTablesAsset>>();
            if let Some(slot) = tables.get_mut(&line_tables_handle) {
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
}
