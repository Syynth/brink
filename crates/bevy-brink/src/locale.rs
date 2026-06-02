//! `.inkl` locale overlays: loading, application, and global switching.
//!
//! A `.inkl` file is a translation overlay produced by `brink-intl`. This
//! module loads it as a [`LocaleAsset`], applies it to a story's base line
//! tables (via [`brink_runtime::apply_locale`]) to produce a localized
//! [`LineTablesAsset`], and wires up **global, event-driven** locale
//! switching:
//!
//! - [`BrinkCurrentLocale<M>`] — the resource holding the active locale
//!   (`None` = base/source language).
//! - [`SetBrinkLocale::set_brink_locale`] — the API: sets the resource and
//!   fires [`BrinkLocaleChanged<M>`].
//! - An observer on that event (plus a catch-up system for `.inkl`s that
//!   finish loading after a switch, plus a spawn-time read in
//!   `fulfill_flow_requests`) reconciles every flow's [`BrinkLocale`] to the
//!   current locale. The transcript then re-renders via the existing
//!   `Changed<BrinkLocale>` reactivity. No per-frame polling.
//! - [`BrinkLocaleOverride<M>`] — a marker that opts a flow out of global
//!   switching (its locale is set manually via [`apply_locale_overlay`]).

use std::collections::HashMap;
use std::marker::PhantomData;

use bevy_asset::{
    Asset, AssetEvent, AssetId, AssetLoader, Assets, Handle, LoadContext, io::Reader,
};
use bevy_ecs::component::Component;
use bevy_ecs::event::Event;
use bevy_ecs::message::MessageReader;
use bevy_ecs::observer::On;
use bevy_ecs::query::Without;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, Res, ResMut};
use bevy_ecs::world::World;
use bevy_log::warn;
use bevy_reflect::TypePath;
use brink_format::LocaleData;
use brink_runtime::{LocaleMode, RuntimeError, apply_locale};

use crate::asset::{BrinkProgram, LineTablesAsset, ProgramAsset};
use crate::line_tables::BrinkLocale;

// ── Asset + loader ──────────────────────────────────────────────────────────

/// A parsed `.inkl` locale overlay. Apply it to a story's base line tables
/// with [`apply_locale_overlay`] (or let the global locale machinery do it).
#[derive(Asset, TypePath)]
pub struct LocaleAsset {
    pub data: LocaleData,
}

/// Asset loader for `.inkl` (compiled locale overlay) files. Decodes via
/// [`brink_format::read_inkl`] into a [`LocaleAsset`].
#[derive(Default, TypePath)]
pub struct InklLoader;

/// Errors that can occur loading an `.inkl` file.
#[derive(Debug, thiserror::Error)]
pub enum InklLoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid .inkl: {0:?}")]
    Decode(brink_format::DecodeError),
}

impl From<brink_format::DecodeError> for InklLoaderError {
    fn from(err: brink_format::DecodeError) -> Self {
        Self::Decode(err)
    }
}

impl AssetLoader for InklLoader {
    type Asset = LocaleAsset;
    type Settings = ();
    type Error = InklLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let data = brink_format::read_inkl(&bytes)?;
        Ok(LocaleAsset { data })
    }

    fn extensions(&self) -> &[&str] {
        &["inkl"]
    }
}

// ── Apply helper (primitive) ──────────────────────────────────────────────

/// Apply a locale overlay to a story's base line tables, inserting the
/// resulting localized [`LineTablesAsset`] and returning its handle.
///
/// The building block for locale switching: point a flow's
/// [`BrinkLocale::handle`](crate::BrinkLocale) at the returned handle to
/// render that flow in this locale (the transcript re-renders automatically).
///
/// # Errors
/// Propagates [`apply_locale`] errors — notably
/// [`LocaleChecksumMismatch`](RuntimeError::LocaleChecksumMismatch) when the
/// overlay was built against a different `.inkb`.
pub fn apply_locale_overlay(
    program: &ProgramAsset,
    base: &LineTablesAsset,
    locale: &LocaleAsset,
    mode: LocaleMode,
    line_tables: &mut Assets<LineTablesAsset>,
) -> Result<Handle<LineTablesAsset>, RuntimeError> {
    let tables = apply_locale(&program.program, &locale.data, &base.tables, mode)?;
    Ok(line_tables.add(LineTablesAsset { tables }))
}

// ── Global, event-driven locale ─────────────────────────────────────────────

/// The active locale for story marker `M`. `None` = base/source language.
///
/// The game's single source of truth for "what language are we in." Switch
/// it with [`SetBrinkLocale::set_brink_locale`] (which also fires
/// [`BrinkLocaleChanged`] so flows reconcile). The plugin inserts this
/// (default `None`) automatically.
#[derive(Resource)]
pub struct BrinkCurrentLocale<M: Send + Sync + 'static = ()> {
    pub locale: Option<Handle<LocaleAsset>>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkCurrentLocale<M> {
    fn default() -> Self {
        Self {
            locale: None,
            _marker: PhantomData,
        }
    }
}

/// Fired when the global locale changes; reconciles all flows.
#[derive(Event)]
pub struct BrinkLocaleChanged<M: Send + Sync + 'static = ()> {
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkLocaleChanged<M> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

/// A flow's canonical **base** line tables (the `.inkb`'s `#line_tables`).
///
/// Inserted at fulfillment alongside [`BrinkLocale`]. Locale overlays always
/// apply to this base, never to an already-localized table, and reverting to
/// the base locale restores it.
#[derive(Component)]
pub struct BrinkBaseLocale<M: Send + Sync + 'static = ()> {
    pub handle: Handle<LineTablesAsset>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> BrinkBaseLocale<M> {
    #[must_use]
    pub fn new(handle: Handle<LineTablesAsset>) -> Self {
        Self {
            handle,
            _marker: PhantomData,
        }
    }
}

/// Marker: a flow carrying this is **excluded** from global locale reconcile.
/// Drive its [`BrinkLocale`] manually (e.g. a polyglot NPC) via
/// [`apply_locale_overlay`].
#[derive(Component, Default)]
pub struct BrinkLocaleOverride<M: Send + Sync + 'static = ()> {
    _marker: PhantomData<fn() -> M>,
}

/// Caches localized line tables per `(base, locale)` so all flows in a locale
/// share one [`LineTablesAsset`] rather than rebuilding it per flow.
#[derive(Resource)]
pub struct LocalizedTablesCache<M: Send + Sync + 'static = ()> {
    map: HashMap<(AssetId<LineTablesAsset>, AssetId<LocaleAsset>), Handle<LineTablesAsset>>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for LocalizedTablesCache<M> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            _marker: PhantomData,
        }
    }
}

/// [`Commands`] extension to switch the global locale.
pub trait SetBrinkLocale {
    /// Set the active locale for marker `M` (`None` = base) and fire
    /// [`BrinkLocaleChanged<M>`] so all non-override flows reconcile.
    fn set_brink_locale<M: Send + Sync + 'static>(&mut self, locale: Option<Handle<LocaleAsset>>);
}

impl SetBrinkLocale for Commands<'_, '_> {
    fn set_brink_locale<M: Send + Sync + 'static>(&mut self, locale: Option<Handle<LocaleAsset>>) {
        self.queue(move |world: &mut World| {
            world
                .get_resource_or_insert_with(BrinkCurrentLocale::<M>::default)
                .locale = locale;
            world.trigger(BrinkLocaleChanged::<M>::default());
        });
    }
}

/// Compute the [`LineTablesAsset`] handle a flow's [`BrinkLocale`] should
/// point at for the current locale.
///
/// Returns `Some(base)` when no locale is active; the cached/built localized
/// handle when the `.inkl` (and base) are loaded; or `None` (leave the flow's
/// current handle unchanged — it will catch up once the `.inkl` loads) when
/// the overlay isn't ready yet. Apply errors `warn!` and fall back to base.
fn reconcile_flow_locale<M: Send + Sync + 'static>(
    base_handle: &Handle<LineTablesAsset>,
    program: &ProgramAsset,
    current: Option<&Handle<LocaleAsset>>,
    locales: &Assets<LocaleAsset>,
    cache: &mut LocalizedTablesCache<M>,
    line_tables: &mut Assets<LineTablesAsset>,
) -> Option<Handle<LineTablesAsset>> {
    let Some(locale_handle) = current else {
        return Some(base_handle.clone());
    };

    let key = (base_handle.id(), locale_handle.id());
    if let Some(handle) = cache.map.get(&key) {
        return Some(handle.clone());
    }

    // Need both the overlay and the base tables loaded to build.
    let locale = locales.get(locale_handle)?;
    let base = line_tables.get(base_handle)?;
    let result = apply_locale(
        &program.program,
        &locale.data,
        &base.tables,
        LocaleMode::Overlay,
    );
    match result {
        Ok(tables) => {
            let handle = line_tables.add(LineTablesAsset { tables });
            cache.map.insert(key, handle.clone());
            Some(handle)
        }
        Err(err) => {
            warn!("brink: locale overlay failed to apply ({err}); staying on base");
            Some(base_handle.clone())
        }
    }
}

#[expect(
    clippy::type_complexity,
    reason = "bevy query tuple for flow locale reconcile"
)]
fn reconcile_all_flows<M: Send + Sync + 'static>(
    current: &BrinkCurrentLocale<M>,
    programs: &Assets<ProgramAsset>,
    locales: &Assets<LocaleAsset>,
    line_tables: &mut Assets<LineTablesAsset>,
    cache: &mut LocalizedTablesCache<M>,
    flows: &mut Query<
        (&BrinkProgram<M>, &BrinkBaseLocale<M>, &mut BrinkLocale<M>),
        Without<BrinkLocaleOverride<M>>,
    >,
) {
    for (prog, base, mut active) in flows.iter_mut() {
        let Some(program) = programs.get(&prog.handle) else {
            continue;
        };
        if let Some(handle) = reconcile_flow_locale(
            &base.handle,
            program,
            current.locale.as_ref(),
            locales,
            cache,
            line_tables,
        ) {
            active.handle = handle;
        }
    }
}

/// Observer (registered by the plugin) that reconciles every non-override
/// flow's locale when [`BrinkLocaleChanged`] fires.
#[expect(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    reason = "bevy systems take params by value and have complex query tuples"
)]
pub fn on_locale_changed<M: Send + Sync + 'static>(
    _on: On<BrinkLocaleChanged<M>>,
    current: Res<BrinkCurrentLocale<M>>,
    programs: Res<Assets<ProgramAsset>>,
    locales: Res<Assets<LocaleAsset>>,
    mut line_tables: ResMut<Assets<LineTablesAsset>>,
    mut cache: ResMut<LocalizedTablesCache<M>>,
    mut flows: Query<
        (&BrinkProgram<M>, &BrinkBaseLocale<M>, &mut BrinkLocale<M>),
        Without<BrinkLocaleOverride<M>>,
    >,
) {
    reconcile_all_flows(
        &current,
        &programs,
        &locales,
        &mut line_tables,
        &mut cache,
        &mut flows,
    );
}

/// Plugin system: when the current locale's `.inkl` finishes loading (or is
/// hot-reloaded) *after* a switch/spawn, reconcile so flows pick it up. Reads
/// asset events; no-ops when nothing relevant loaded.
#[expect(
    clippy::needless_pass_by_value,
    clippy::type_complexity,
    reason = "bevy systems take params by value and have complex query tuples"
)]
pub fn catch_up_loaded_locales<M: Send + Sync + 'static>(
    mut events: MessageReader<AssetEvent<LocaleAsset>>,
    current: Res<BrinkCurrentLocale<M>>,
    programs: Res<Assets<ProgramAsset>>,
    locales: Res<Assets<LocaleAsset>>,
    mut line_tables: ResMut<Assets<LineTablesAsset>>,
    mut cache: ResMut<LocalizedTablesCache<M>>,
    mut flows: Query<
        (&BrinkProgram<M>, &BrinkBaseLocale<M>, &mut BrinkLocale<M>),
        Without<BrinkLocaleOverride<M>>,
    >,
) {
    let current_id = current.locale.as_ref().map(Handle::id);
    // Always drain the reader; only reconcile if the *current* locale loaded.
    let relevant = events.read().any(|ev| match ev {
        AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::LoadedWithDependencies { id } => Some(*id) == current_id,
        _ => false,
    });
    if !relevant {
        return;
    }
    reconcile_all_flows(
        &current,
        &programs,
        &locales,
        &mut line_tables,
        &mut cache,
        &mut flows,
    );
}

/// Reconcile a single newly-spawned flow's locale against the current locale,
/// returning the handle its [`BrinkLocale`] should start at (base if no locale
/// is active or the overlay isn't loaded yet — `catch_up_loaded_locales` will
/// localize it once the `.inkl` loads). Used by `fulfill_flow_requests`.
pub(crate) fn initial_locale_handle<M: Send + Sync + 'static>(
    base_handle: &Handle<LineTablesAsset>,
    program: &ProgramAsset,
    current: Option<&BrinkCurrentLocale<M>>,
    locales: &Assets<LocaleAsset>,
    cache: &mut LocalizedTablesCache<M>,
    line_tables: &mut Assets<LineTablesAsset>,
) -> Handle<LineTablesAsset> {
    let current_locale = current.and_then(|c| c.locale.as_ref());
    reconcile_flow_locale(
        base_handle,
        program,
        current_locale,
        locales,
        cache,
        line_tables,
    )
    .unwrap_or_else(|| base_handle.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrinkFlowRequest;
    use crate::asset::BrinkStoryAsset;
    use crate::test_support::make_test_app;
    use bevy_app::App;
    use bevy_ecs::entity::Entity;
    use brink_format::LineContent;
    use brink_intl::ContentJson;
    use brink_runtime::FlowInstance;

    /// Compile `base_src`, round-trip through `.inkb` (so the program carries
    /// a real checksum), build an `es` overlay translating the first scope's
    /// first line, and stand up an app with all four assets inserted.
    /// Returns the app + the story and locale handles.
    fn setup(
        base_src: &str,
        translation: &str,
    ) -> (App, Handle<BrinkStoryAsset>, Handle<LocaleAsset>) {
        let owned = base_src.to_string();
        let out = brink_compiler::compile("t.ink", move |p| {
            if p == "t.ink" {
                Ok(owned.clone())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
        })
        .expect("compile");
        let data = out.data;

        let mut inkb = Vec::new();
        brink_format::write_inkb(&data, &mut inkb);
        let loaded = brink_format::read_inkb(&inkb).expect("read_inkb");
        let (program, base_tables) = brink_runtime::link(&loaded).expect("link");
        let checksum = brink_format::read_inkb_index(&inkb)
            .expect("index")
            .checksum;

        let mut lines = brink_intl::export_lines(&loaded, checksum);
        lines.scopes[0].lines[0].content = Some(ContentJson::Plain(translation.to_string()));
        let inkl_bytes = brink_intl::compile_locale(&inkb, &lines, "es").expect("compile_locale");
        let locale_data = brink_format::read_inkl(&inkl_bytes).expect("read_inkl");

        let mut app = make_test_app();
        let (_, initial_context) = FlowInstance::new_at_root(&program);
        let world = app.world_mut();
        let program_h = world
            .resource_mut::<Assets<ProgramAsset>>()
            .add(ProgramAsset {
                program,
                initial_context,
            });
        let base_h = world
            .resource_mut::<Assets<LineTablesAsset>>()
            .add(LineTablesAsset {
                tables: base_tables,
            });
        let story_h = world
            .resource_mut::<Assets<BrinkStoryAsset>>()
            .add(BrinkStoryAsset {
                program: program_h,
                line_tables: base_h,
            });
        let locale_h = world
            .resource_mut::<Assets<LocaleAsset>>()
            .add(LocaleAsset { data: locale_data });
        (app, story_h, locale_h)
    }

    /// True if any line in the flow's *active* line tables (what `BrinkLocale`
    /// currently points at) contains `needle`.
    fn active_text_contains(app: &App, flow: Entity, needle: &str) -> bool {
        let handle = app
            .world()
            .entity(flow)
            .get::<BrinkLocale<()>>()
            .expect("BrinkLocale")
            .handle
            .clone();
        let tables = &app
            .world()
            .resource::<Assets<LineTablesAsset>>()
            .get(&handle)
            .expect("active line tables")
            .tables;
        tables.iter().flatten().any(|e| match &e.content {
            LineContent::Plain(s) => s.contains(needle),
            LineContent::Template(_) => false,
        })
    }

    fn switch_to(app: &mut App, locale: Option<Handle<LocaleAsset>>) {
        app.world_mut()
            .resource_mut::<BrinkCurrentLocale<()>>()
            .locale = locale;
        app.world_mut().trigger(BrinkLocaleChanged::<()>::default());
        app.update();
    }

    #[test]
    fn global_switch_localizes_and_reverts() {
        let (mut app, story, locale) = setup("Hello world\n-> END\n", "[ES] Hola mundo\n");
        let flow = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill

        assert!(
            active_text_contains(&app, flow, "Hello world"),
            "starts on base"
        );
        assert!(!active_text_contains(&app, flow, "[ES]"));

        switch_to(&mut app, Some(locale));
        assert!(
            active_text_contains(&app, flow, "[ES] Hola mundo"),
            "switched to es"
        );

        switch_to(&mut app, None);
        assert!(
            active_text_contains(&app, flow, "Hello world"),
            "reverted to base"
        );
        assert!(!active_text_contains(&app, flow, "[ES]"));
    }

    #[test]
    fn override_flow_is_not_switched() {
        let (mut app, story, locale) = setup("Hello world\n-> END\n", "[ES] Hola mundo\n");
        let flow = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update();
        app.world_mut()
            .entity_mut(flow)
            .insert(BrinkLocaleOverride::<()>::default());

        switch_to(&mut app, Some(locale));
        assert!(
            active_text_contains(&app, flow, "Hello world"),
            "override flow stays on base"
        );
        assert!(!active_text_contains(&app, flow, "[ES]"));
    }

    #[test]
    fn flow_spawned_while_locale_set_starts_localized() {
        let (mut app, story, locale) = setup("Hello world\n-> END\n", "[ES] Hola mundo\n");
        // Set the locale BEFORE spawning any flow (overlay already loaded).
        app.world_mut()
            .resource_mut::<BrinkCurrentLocale<()>>()
            .locale = Some(locale);
        let flow = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update(); // fulfill reads the current locale at spawn

        assert!(
            active_text_contains(&app, flow, "[ES] Hola mundo"),
            "new flow starts localized"
        );
    }

    #[test]
    fn flows_share_cached_localized_tables() {
        let (mut app, story, locale) = setup("Hello world\n-> END\n", "[ES] Hola mundo\n");
        let a = app
            .world_mut()
            .spawn(
                BrinkFlowRequest::<()>::builder()
                    .story(story.clone())
                    .build(),
            )
            .id();
        let b = app
            .world_mut()
            .spawn(BrinkFlowRequest::<()>::builder().story(story).build())
            .id();
        app.update();

        switch_to(&mut app, Some(locale));
        let ha = app
            .world()
            .entity(a)
            .get::<BrinkLocale<()>>()
            .expect("a")
            .handle
            .clone();
        let hb = app
            .world()
            .entity(b)
            .get::<BrinkLocale<()>>()
            .expect("b")
            .handle
            .clone();
        assert_eq!(
            ha, hb,
            "both flows share one cached localized LineTablesAsset"
        );
    }
}
