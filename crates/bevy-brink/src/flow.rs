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
