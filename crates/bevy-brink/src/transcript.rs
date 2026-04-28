//! Auto-rendered per-flow transcript component.
//!
//! Attach a [`BrinkTranscript<M>`] to a fulfilled flow entity (alongside
//! the usual [`BrinkFlow`](crate::BrinkFlow), [`BrinkProgram`](crate::BrinkProgram),
//! [`BrinkLocale`](crate::BrinkLocale)) and the plugin will keep it
//! in sync with `flow.inner.transcript()` — re-rendering whenever
//! the flow grows, the locale handle changes, or the line-tables
//! asset content changes (hot-reload).
//!
//! This is purely a convenience: consumers can read structural output
//! parts directly via `flow.inner.transcript()` and call
//! `brink_runtime::transcript::render_transcript` themselves.

use std::marker::PhantomData;

use bevy_asset::{AssetEvent, Assets};
use bevy_ecs::change_detection::{DetectChanges, Ref};
use bevy_ecs::component::Component;
use bevy_ecs::message::MessageReader;
use bevy_ecs::system::{Query, Res};

use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::flow::BrinkFlow;
use crate::line_tables::BrinkLocale;

/// Cached, locale-resolved view of a flow's transcript.
///
/// Inserted by the consumer (opt-in) on a flow entity. The plugin's
/// `refresh_transcripts<M>` system re-renders `lines` when:
///
/// - The flow's `transcript_len()` differs from `cached_len` (the flow
///   has produced new output since the last refresh).
/// - The `BrinkLocale<M>` handle on this entity changed (locale swap).
/// - An `AssetEvent::Modified<LineTablesAsset>` fired since last refresh
///   (the current locale's content was hot-reloaded).
///
/// Each entry in `lines` is `(text, tags)` for one resolved output line,
/// as produced by [`brink_runtime::transcript::render_transcript`].
#[derive(Component)]
pub struct BrinkTranscript<M: Send + Sync + 'static = ()> {
    pub lines: Vec<(String, Vec<String>)>,
    cached_len: usize,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkTranscript<M> {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            cached_len: 0,
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkTranscript<M> {
    /// Concatenate every line's text with `\n` between entries.
    /// `render_transcript` returns lines stripped of their trailing
    /// newline, so we re-insert one between consecutive entries.
    #[must_use]
    pub fn text(&self) -> String {
        // Capacity hint: sum of line lengths + (n-1) separators.
        let n = self.lines.len();
        let total: usize = self.lines.iter().map(|(s, _)| s.len()).sum();
        let mut out = String::with_capacity(total + n.saturating_sub(1));
        for (i, (text, _)) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(text);
        }
        out
    }
}

/// Plugin-managed system: re-render `BrinkTranscript<M>` for any flow
/// entity whose transcript has grown, whose locale handle changed, or
/// whose locale's `LineTablesAsset` content was hot-reloaded.
#[expect(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    reason = "bevy systems take Res/Query by value and have complex query tuples"
)]
pub fn refresh_transcripts<M: Send + Sync + 'static>(
    mut events: MessageReader<AssetEvent<LineTablesAsset>>,
    mut flows: Query<(
        &BrinkFlow<M>,
        &BrinkProgram<M>,
        Ref<BrinkLocale<M>>,
        &mut BrinkTranscript<M>,
    )>,
    programs: Res<Assets<ProgramAsset>>,
    line_tables: Res<Assets<LineTablesAsset>>,
) {
    // If any LineTablesAsset content changed this tick, every flow
    // potentially needs a re-render against the new tables. We don't
    // route by handle — there's typically one locale per marker.
    let any_locale_modified = events
        .read()
        .any(|ev| matches!(ev, AssetEvent::Modified { .. }));

    for (flow, program_h, locale_h, mut transcript) in &mut flows {
        let current_len = flow.inner.transcript_len();
        let locale_changed = locale_h.is_changed();
        let needs_refresh =
            current_len != transcript.cached_len || locale_changed || any_locale_modified;
        if !needs_refresh {
            continue;
        }

        let Some(program_asset) = programs.get(&program_h.handle) else {
            continue;
        };
        let Some(lt_asset) = line_tables.get(&locale_h.handle) else {
            continue;
        };

        transcript.lines = brink_runtime::transcript::render_transcript(
            flow.inner.transcript(),
            &program_asset.program,
            &lt_asset.tables,
            None,
            flow.inner.fragments(),
        );
        transcript.cached_len = current_len;
    }
}
