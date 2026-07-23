//! Asset loader for `.ink` source files (dev mode).
//!
//! Compiles ink source at asset-load time, walking the transitive
//! `INCLUDE` graph asynchronously through Bevy's `AssetReader`. Hot-reload
//! "just works" because every INCLUDE'd file is fetched via
//! [`bevy_asset::LoadContext::read_asset_bytes`], which automatically
//! registers the file as a dependency — when any of them change, Bevy
//! re-runs the loader.
//!
//! Available only when the `dev` feature is enabled. Release builds
//! should pre-compile to `.inkb` and avoid carrying the compiler.
//!
//! ## Async/sync seam
//!
//! [`brink_compiler::compile`] is synchronous (it calls a `read_file`
//! closure for each file it discovers). Bevy's `AssetReader` is async,
//! and on web targets it has to be — there's no blocking filesystem.
//! We bridge the two by walking the INCLUDE graph ourselves first
//! (using [`brink_syntax::extract_includes`] to discover INCLUDEs from
//! cached source), pre-fetching every file via Bevy's async reader, and
//! then handing the compiler a closure that simply reads from the
//! in-memory cache.

use std::collections::HashMap;

use bevy_app::App;
use bevy_asset::{AssetLoader, Assets, Handle, LoadContext, io::Reader};
use bevy_reflect::TypePath;
use brink_project_config::ProjectConfig;

use crate::asset::{
    BrinkStoryAsset, LineTablesAsset, ProgramAsset, emit_story_assets, fresh_context,
};

/// Asset loader for `.ink` (source) files.
///
/// Reads the entry source, walks the `INCLUDE` graph asynchronously
/// through Bevy's `AssetReader`, compiles via [`brink_compiler::compile`],
/// links via [`brink_runtime::link`], and emits labeled subassets
/// (`#program`, `#line_tables`) just like [`InkbLoader`](crate::InkbLoader).
///
/// ## `brink.toml` discovery (#1029)
///
/// A `brink.toml` beside (or above) the entry asset supplies the
/// [`ProjectConfig`] (`dialect`/`types`) that gates T1b brink-extension
/// syntax — the same file the CLI discovers by walking the real
/// filesystem (`brink-project-config::load_from_entry`). Bevy's
/// `AssetReader` may be virtual or packed, so this loader re-implements
/// the walk-up over the async reader instead: [`load`](AssetLoader::load)
/// probes `brink.toml` beside the entry, then each ancestor directory in
/// turn, via [`LoadContext::read_asset_bytes`] — bounded at the asset
/// source root (never above it) and naturally finite (the entry path has
/// finitely many `/`-separated ancestors). A hit is registered as a load
/// dependency exactly like an `INCLUDE`, so editing `brink.toml` in dev
/// mode hot-reloads the story. A miss at every level leaves
/// `AnalysisOptions` at its default — byte-identical to pre-#1029
/// behavior.
///
/// [`override_config`](Self::override_config), set via
/// [`BrinkPlugin::with_config`](crate::BrinkPlugin::with_config) /
/// [`BrinkAssetsPlugin::with_config`](crate::BrinkAssetsPlugin::with_config),
/// is the programmatic escape hatch: when set, its fields win over
/// whatever the asset walk-up discovers (applied last via
/// `AnalysisOptions::apply_project_config`, mirroring the CLI's
/// "explicit call always wins over the file" precedence).
#[derive(Default, TypePath)]
pub struct InkLoader {
    /// Out-of-band [`ProjectConfig`] override (#1029). `None` (the
    /// default) means "the discovered `brink.toml` asset (if any) wins";
    /// `Some` fields win over the asset's, unset fields still fall
    /// through to whatever the asset (or the built-in default) supplies.
    pub override_config: Option<ProjectConfig>,
}

/// Errors that can occur loading an `.ink` source file.
#[derive(Debug, thiserror::Error)]
pub enum InkLoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("read asset: {0}")]
    ReadAsset(#[from] bevy_asset::ReadAssetBytesError),
    #[error("source not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("entry path missing or non-UTF-8")]
    BadEntryPath,
    #[error("compile: {0}")]
    Compile(#[from] brink_compiler::CompileError),
    #[error("link error: {0}")]
    Link(#[from] brink_runtime::RuntimeError),
    /// A discovered `brink.toml` asset exists but is malformed: bad TOML
    /// syntax, or a recognized key (`dialect`/`types`) with a value
    /// outside its enum. Unknown keys are warnings (logged), never this —
    /// see `brink_project_config::ConfigError` (#1029).
    #[error("invalid {path}: {source}")]
    InvalidProjectConfig {
        path: String,
        #[source]
        source: brink_project_config::ConfigError,
    },
}

/// Errors from [`compile_story_inline`].
#[derive(Debug, thiserror::Error)]
pub enum CompileStoryInlineError {
    #[error("compile: {0}")]
    Compile(#[from] brink_compiler::CompileError),
    #[error("link error: {0}")]
    Link(#[from] brink_runtime::RuntimeError),
}

/// Compile an in-memory ink source string straight into story assets,
/// inserting them into `app`'s asset collections and returning the
/// resulting `Handle<BrinkStoryAsset>` (G3, issue #1060).
///
/// Collapses the four-step dance tests/tools otherwise hand-roll —
/// `brink_compiler::compile` → `brink_runtime::link` →
/// `FlowInstance::new_at_root` (only to obtain the initial context) →
/// hand-inserting `ProgramAsset` + `LineTablesAsset` + `BrinkStoryAsset`
/// into three `Assets<T>` resources — into one call, wrapping the same
/// [`emit_story_assets`]-adjacent logic [`InkLoader`] uses at asset-load
/// time, but synchronously and without the async `AssetServer`.
///
/// `name` is the compiler's synthetic entry file name (also its `INCLUDE`
/// resolution root). Since `source` is a single in-memory string, `INCLUDE`
/// is not supported here — any `INCLUDE` directive fails to resolve and
/// surfaces as a [`CompileStoryInlineError::Compile`]; a story spanning
/// multiple files needs [`InkLoader`]/`AssetServer::load` instead, which
/// walks the graph asynchronously.
///
/// `app` must already have `AssetPlugin` (or equivalent) installed, so
/// `Assets<ProgramAsset>`, `Assets<LineTablesAsset>`, and
/// `Assets<BrinkStoryAsset>` exist as resources — the same precondition
/// `BrinkPlugin` itself has.
pub fn compile_story_inline(
    app: &mut App,
    name: &str,
    source: &str,
) -> Result<Handle<BrinkStoryAsset>, CompileStoryInlineError> {
    let output = brink_compiler::compile(name, |path| {
        if path == name {
            Ok(source.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "{path}: compile_story_inline compiles a single in-memory source with no \
                     INCLUDE support; use InkLoader/AssetServer for multi-file stories"
                ),
            ))
        }
    })?;
    let (program, tables) = brink_runtime::link(&output.data)?;
    let initial_context = fresh_context(&program);

    let world = app.world_mut();
    let program_handle = world
        .resource_mut::<Assets<ProgramAsset>>()
        .add(ProgramAsset {
            program,
            initial_context,
            effect_rows: output.data.effect_rows,
        });
    let line_tables_handle = world
        .resource_mut::<Assets<LineTablesAsset>>()
        .add(LineTablesAsset { tables });
    Ok(world
        .resource_mut::<Assets<BrinkStoryAsset>>()
        .add(BrinkStoryAsset {
            program: program_handle,
            line_tables: line_tables_handle,
        }))
}

/// Resolve an `INCLUDE` path relative to the including file's directory.
///
/// String-based (uses `/`) to match `brink-db`'s WASM-safe resolver and
/// avoid platform separator issues. The joined path is normalized so `.`/`..`
/// segments collapse to a clean key (matching `brink-db::resolve_include_path`
/// system-wide; see docs/decision-log.md).
fn resolve_include_path(from_file: &str, include_path: &str) -> String {
    let joined = match from_file.rfind('/') {
        Some(i) => format!("{}/{include_path}", &from_file[..i]),
        None => include_path.to_string(),
    };
    let absolute = joined.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." if matches!(out.last(), Some(&s) if s != "..") => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

/// Ancestor directories of `entry_path`'s containing directory, nearest
/// first, ending at `""` (the asset source root). String-based (matches
/// [`resolve_include_path`]'s `/`-only convention). E.g.
/// `"stories/ch1/intro.ink"` yields `["stories/ch1", "stories", ""]`;
/// `"intro.ink"` (no directory) yields `[""]`.
///
/// Finite by construction — each step strictly shortens the remaining
/// prefix at a `/` boundary, so the walk-up this drives ([`probe_brink_toml`])
/// is naturally bounded by the entry path's depth (guard against
/// unbounded growth).
fn ancestor_dirs(entry_path: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut current = match entry_path.rfind('/') {
        Some(i) => &entry_path[..i],
        None => "",
    };
    loop {
        dirs.push(current.to_string());
        if current.is_empty() {
            break;
        }
        current = match current.rfind('/') {
            Some(i) => &current[..i],
            None => "",
        };
    }
    dirs
}

/// Bounded ancestor walk-up (#1029): probe for `brink.toml` beside the
/// entry asset, then each ancestor directory in turn (nearest first), up
/// to the asset source root — mirroring the CLI's `brink.toml` walk-up
/// (`brink_project_config::find_config`), but over the async `AssetReader`
/// since a bevy source tree may be virtual or packed rather than a real
/// filesystem. A hit is read via [`LoadContext::read_asset_bytes`], which
/// registers it as a load dependency, so hot-reload "just works" exactly
/// like an `INCLUDE`. A miss at every ancestor returns `None` — not an
/// error, matching `brink-project-config`'s "missing config changes
/// nothing" contract.
async fn probe_brink_toml(
    load_context: &mut LoadContext<'_>,
    entry_path: &str,
) -> Option<(String, Vec<u8>)> {
    for dir in ancestor_dirs(entry_path) {
        let candidate = if dir.is_empty() {
            brink_project_config::CONFIG_FILE_NAME.to_string()
        } else {
            format!("{dir}/{}", brink_project_config::CONFIG_FILE_NAME)
        };
        if let Ok(bytes) = load_context.read_asset_bytes(candidate.clone()).await {
            return Some((candidate, bytes));
        }
    }
    None
}

impl AssetLoader for InkLoader {
    type Asset = BrinkStoryAsset;
    type Settings = ();
    type Error = InkLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        // Read the entry source. AssetPath includes optional source +
        // label; for our purposes the underlying filesystem path is what
        // INCLUDE resolution needs.
        let entry_path = load_context
            .path()
            .path()
            .to_str()
            .ok_or(InkLoaderError::BadEntryPath)?
            .to_string();

        let mut entry_bytes = Vec::new();
        reader.read_to_end(&mut entry_bytes).await?;
        let entry_source = String::from_utf8(entry_bytes)?;

        // BFS the INCLUDE graph, fetching every transitive dep through
        // Bevy's async reader (which registers each as a dependency for
        // automatic hot-reload).
        let mut sources: HashMap<String, String> = HashMap::new();
        let mut queue: Vec<String> = brink_syntax::extract_includes(&entry_source)
            .into_iter()
            .map(|inc| resolve_include_path(&entry_path, &inc))
            .collect();
        sources.insert(entry_path.clone(), entry_source);

        while let Some(path) = queue.pop() {
            if sources.contains_key(&path) {
                continue;
            }
            let bytes = load_context.read_asset_bytes(path.clone()).await?;
            let source = String::from_utf8(bytes)?;
            for inc in brink_syntax::extract_includes(&source) {
                let resolved = resolve_include_path(&path, &inc);
                if !sources.contains_key(&resolved) {
                    queue.push(resolved);
                }
            }
            sources.insert(path, source);
        }

        // #1029: brink.toml discovery — bounded ancestor walk-up through
        // the async AssetReader (see the module/struct docs). A hit
        // supplies the *default* ProjectConfig; `override_config` (set via
        // `BrinkPlugin::with_config`) is applied afterward and wins.
        let mut options = brink_compiler::AnalysisOptions::default();
        if let Some((config_path, bytes)) = probe_brink_toml(load_context, &entry_path).await {
            let text = String::from_utf8(bytes)?;
            let (config, warnings) = brink_project_config::parse_str(&text).map_err(|source| {
                InkLoaderError::InvalidProjectConfig {
                    path: config_path.clone(),
                    source,
                }
            })?;
            for warning in &warnings {
                bevy_log::warn!("[{config_path}] {warning}");
            }
            options.apply_project_config(&config, false, false);
        }
        if let Some(override_config) = &self.override_config {
            options.apply_project_config(override_config, false, false);
        }

        // Compile from the cached sources (synchronous; closure reads
        // from the HashMap).
        let output = brink_compiler::compile_with_options(
            &entry_path,
            |p| {
                sources.get(p).cloned().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("{p}: not in pre-fetched source cache"),
                    )
                })
            },
            options,
        )?;
        let (program, tables) = brink_runtime::link(&output.data)?;
        Ok(emit_story_assets(
            load_context,
            program,
            tables,
            output.data.effect_rows,
        ))
    }

    fn extensions(&self) -> &[&str] {
        &["ink"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_with_directory_prefix() {
        assert_eq!(
            resolve_include_path("src/main.ink", "utils.ink"),
            "src/utils.ink"
        );
    }

    #[test]
    fn resolves_without_directory() {
        assert_eq!(resolve_include_path("story.ink", "other.ink"), "other.ink");
    }

    #[test]
    fn resolves_nested_directory() {
        assert_eq!(resolve_include_path("a/b/c.ink", "d.ink"), "a/b/d.ink");
    }

    #[test]
    fn normalizes_parent_traversal() {
        assert_eq!(resolve_include_path("a/b/c.ink", "../d.ink"), "a/d.ink");
        assert_eq!(resolve_include_path("a/b/c.ink", "../../d.ink"), "d.ink");
    }

    #[test]
    fn compile_story_inline_inserts_assets_and_returns_handle() {
        let mut app = crate::test_support::make_test_app();

        let handle = compile_story_inline(&mut app, "inline.ink", "VAR mood = 3\n-> END\n")
            .expect("inline source compiles and links");

        let world = app.world();
        let story = world
            .resource::<Assets<BrinkStoryAsset>>()
            .get(&handle)
            .expect("story asset inserted");
        let program_asset = world
            .resource::<Assets<ProgramAsset>>()
            .get(&story.program)
            .expect("program asset inserted");
        assert_eq!(program_asset.program.global_index("mood"), Some(0));
        assert!(
            world
                .resource::<Assets<LineTablesAsset>>()
                .get(&story.line_tables)
                .is_some(),
            "line tables asset inserted"
        );
    }

    #[test]
    fn compile_story_inline_surfaces_compile_error() {
        let mut app = crate::test_support::make_test_app();

        let err = compile_story_inline(&mut app, "broken.ink", "-> nowhere_knot\n")
            .expect_err("a divert to an undeclared knot should not compile");
        assert!(
            matches!(err, CompileStoryInlineError::Compile(_)),
            "got {err:?}"
        );
    }

    // ── ancestor_dirs (#1029 bounded walk-up) ───────────────────────────

    #[test]
    fn ancestor_dirs_nested_entry_climbs_to_root() {
        assert_eq!(
            ancestor_dirs("stories/ch1/intro.ink"),
            vec![
                "stories/ch1".to_string(),
                "stories".to_string(),
                String::new()
            ]
        );
    }

    #[test]
    fn ancestor_dirs_root_entry_is_just_root() {
        assert_eq!(ancestor_dirs("intro.ink"), vec![String::new()]);
    }

    #[test]
    fn ancestor_dirs_single_directory_level() {
        assert_eq!(
            ancestor_dirs("stories/intro.ink"),
            vec!["stories".to_string(), String::new()]
        );
    }
}

/// Integration tests for #1029 (`brink.toml` discovery through the async
/// `AssetReader`): the bounded ancestor walk-up, the plugin-override
/// precedence, and hot-reload — all driven through a real `AssetServer`
/// against an in-memory `AssetSource`
/// ([`bevy_asset::io::memory::Dir`]/`MemoryAssetReader`), mirroring
/// `bevy_asset`'s own `create_app` test pattern. In-memory (not a real
/// temp directory) so these tests are hermetic and can freely rewrite file
/// content for the hot-reload case without touching disk or a file
/// watcher.
#[cfg(test)]
mod config_discovery_tests {
    use std::path::Path;

    use bevy_app::TaskPoolPlugin;
    use bevy_asset::io::memory::{Dir, MemoryAssetReader};
    use bevy_asset::io::{AssetSourceBuilder, AssetSourceId};
    use bevy_asset::{AssetApp, AssetPlugin, AssetServer, LoadState};
    use brink_project_config::Dialect;

    use super::{InkLoader, ProjectConfig};
    use crate::asset::{BrinkStoryAsset, LineTablesAsset, ProgramAsset};

    /// A brink-extension form (`#@private`) that the default `StrictInk`
    /// dialect rejects (E051-class dialect-gate diagnostic,
    /// `brink-analyzer`'s `dialect_gate`) but `dialect = brink` compiles —
    /// the reachability proof #1029 calls for: a bevy story with
    /// `dialect = brink` in a sibling `brink.toml` compiles a
    /// brink-extension form that fails under the default.
    const BRINK_ONLY_SOURCE: &str = "#@private\nVAR secret = 0\n-> END\n";

    /// Build an `App` with an in-memory `AssetSource` and just enough
    /// registered (asset types + the dev-mode `InkLoader`) to drive a real
    /// `AssetServer::load` through [`InkLoader::load`] end to end, without
    /// pulling in all of `BrinkPlugin`'s systems.
    fn make_memory_asset_app() -> (bevy_app::App, Dir) {
        let mut app = bevy_app::App::new();
        let dir = Dir::default();
        let dir_clone = dir.clone();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: dir_clone.clone(),
                })
            }),
        )
        .add_plugins((
            TaskPoolPlugin::default(),
            AssetPlugin {
                watch_for_changes_override: Some(false),
                use_asset_processor_override: Some(false),
                ..Default::default()
            },
        ));
        app.init_asset::<BrinkStoryAsset>();
        app.init_asset::<ProgramAsset>();
        app.init_asset::<LineTablesAsset>();
        app.register_asset_loader(InkLoader::default());
        (app, dir)
    }

    /// Poll `app.update()` until `predicate` returns `Some`, bounded so a
    /// stuck load fails the test instead of hanging the suite (guard
    /// against unbounded growth).
    fn run_until<T>(
        app: &mut bevy_app::App,
        mut predicate: impl FnMut(&mut bevy_app::App) -> Option<T>,
    ) -> Option<T> {
        for _ in 0..2000 {
            app.update();
            let hit = predicate(app);
            if hit.is_some() {
                return hit;
            }
        }
        None
    }

    /// Poll until the handle reaches `Loaded`. Also stops early on `Failed`
    /// (so a genuine failure reports immediately instead of spinning out
    /// the whole bound) but never treats a *stale* `Loaded` as the answer
    /// to `wait_for_failed` below — each waiter polls for its own specific
    /// target state, which matters for the hot-reload test: right after
    /// `AssetServer::reload`, the handle briefly still reads its *previous*
    /// terminal state before the reload's spawned task lands.
    fn wait_for_loaded(app: &mut bevy_app::App, handle: &bevy_asset::Handle<BrinkStoryAsset>) {
        let state = run_until(app, |app| {
            match app.world().resource::<AssetServer>().load_state(handle) {
                LoadState::NotLoaded | LoadState::Loading => None,
                terminal => Some(terminal),
            }
        })
        .expect("asset load did not reach a terminal state within the bounded poll loop");
        assert!(
            matches!(state, LoadState::Loaded),
            "expected the load to succeed; got {state:?}"
        );
    }

    /// Poll until the handle reaches `Failed`, ignoring any `Loaded` seen
    /// along the way (see [`wait_for_loaded`]'s doc for why that matters
    /// post-reload).
    fn wait_for_failed(app: &mut bevy_app::App, handle: &bevy_asset::Handle<BrinkStoryAsset>) {
        run_until(app, |app| {
            matches!(
                app.world().resource::<AssetServer>().load_state(handle),
                LoadState::Failed(_)
            )
            .then_some(())
        })
        .expect("asset load did not reach Failed within the bounded poll loop");
    }

    #[test]
    fn missing_brink_toml_leaves_default_dialect_unchanged() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);
        // No brink.toml anywhere -- current (pre-#1029) behavior: default
        // AnalysisOptions (StrictInk), which rejects `#@private`.

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    #[test]
    fn sibling_brink_toml_sets_brink_dialect() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);
        dir.insert_asset_text(Path::new("brink.toml"), "[project]\ndialect = \"brink\"\n");

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    #[test]
    fn ancestor_brink_toml_found_via_bounded_walkup() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("stories/ch1/intro.ink"), BRINK_ONLY_SOURCE);
        // brink.toml sits two levels above the entry -- proves the walk-up
        // doesn't stop at the immediate sibling directory.
        dir.insert_asset_text(Path::new("brink.toml"), "[project]\ndialect = \"brink\"\n");

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("stories/ch1/intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    #[test]
    fn plugin_override_wins_over_conflicting_asset() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);
        // The asset explicitly sets strict-ink, which alone would reject
        // `#@private`.
        dir.insert_asset_text(
            Path::new("brink.toml"),
            "[project]\ndialect = \"strict-ink\"\n",
        );

        // Re-register the loader with a programmatic override that
        // disagrees with the discovered asset -- override must win
        // (#1029: override > asset > default).
        app.register_asset_loader(InkLoader {
            override_config: Some(ProjectConfig {
                dialect: Some(Dialect::Brink),
                types: None,
            }),
        });

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    #[test]
    fn hot_reload_picks_up_edited_brink_toml() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);
        dir.insert_asset_text(Path::new("brink.toml"), "[project]\ndialect = \"brink\"\n");

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);

        // Edit brink.toml back to strict-ink and force a reload -- this is
        // what the dev-mode file watcher does automatically when a
        // registered load dependency changes on disk; `reload` here drives
        // the same path deterministically without a real watcher.
        dir.insert_asset_text(
            Path::new("brink.toml"),
            "[project]\ndialect = \"strict-ink\"\n",
        );
        app.world().resource::<AssetServer>().reload("intro.ink");

        wait_for_failed(&mut app, &handle);
    }
}
