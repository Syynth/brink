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
//! ## Async/sync seam (#1360, the `brink-environment` producer consumer)
//!
//! [`brink_environment::Project::load`] (and the `compile` it feeds) is
//! synchronous — it reads through a [`brink_source_tree::SourceTree`], which
//! has no `async fn`. Bevy's `AssetReader` is async, and on web targets it
//! has to be — there's no blocking filesystem. We bridge the two by walking
//! the INCLUDE graph ourselves first (using [`brink_syntax::extract_includes`]
//! to discover INCLUDEs from cached source), pre-fetching every reachable
//! file via Bevy's async reader into an in-memory map, then handing that map
//! to the sync producer as a [`brink_source_tree::InMemory`] tree. The BFS
//! itself stays here (the mount's async reality); `brink.toml` discovery,
//! parsing, and override precedence (#1005/#1320) now live entirely in
//! [`brink_environment::Project::load`] — not re-implemented in this loader.

use std::collections::BTreeMap;

use bevy_app::App;
use bevy_asset::{AssetLoader, Assets, Handle, LoadContext, io::Reader};
use bevy_ecs::resource::Resource;
use bevy_reflect::TypePath;
use brink_environment::{OptionOverrides, Project};
use brink_project_config::ProjectConfig;
use brink_source_tree::InMemory;

use crate::asset::{
    BrinkStoryAsset, LineTablesAsset, ProgramAsset, emit_story_assets, fresh_context,
};

/// The `with_config` override [`BrinkAssetsPlugin::build`](crate::BrinkAssetsPlugin)
/// resolved [`InkLoader::override_config`](InkLoader) from, mirrored into a
/// resource so [`compile_story_inline`] — a freestanding function with only
/// `&mut App`, no `AssetLoader` instance to read a field off of — can see the
/// same value (#1380).
///
/// Inserted once, unconditionally, whenever `BrinkAssetsPlugin` builds, from
/// the exact `self.config` that also seeds `InkLoader` — so the two entry
/// points read the identical override and can never diverge on it. Both
/// entry points build their `OptionOverrides` through the one shared
/// [`overrides_from_config`] mapping fed into `Project::load`'s existing
/// seam, the same "reuse the resolution path, don't add a second one" fix
/// applied here that wired `InkLoader` itself.
/// `None` (the default, or no `BrinkAssetsPlugin` in the app at all) means
/// [`overrides_from_config`] falls through to [`OptionOverrides::default()`],
/// byte-identical to pre-#1380 behavior for a host that never calls
/// `with_config`.
#[derive(Resource, Debug, Clone, Default)]
pub(crate) struct BrinkOverrideConfig(pub(crate) Option<ProjectConfig>);

/// Build the [`OptionOverrides`] [`Project::load`] expects from a
/// `with_config` override — the exact `ProjectConfig` -> `OptionOverrides`
/// mapping both [`InkLoader::load`] and [`compile_story_inline`] need.
/// Extracted so the two entry points share one mapping rather than each
/// hand-rolling their own copy that could silently drift apart (#1380,
/// following the resolution shape #1553/#1559/#1417 used for the
/// IDE/LSP/wasm option-propagation gaps).
///
/// `config: None` (no override at all) yields
/// [`OptionOverrides::default()`] — every field unset, so `Project::load`
/// falls through entirely to the discovered `brink.toml` (or the built-in
/// default).
fn overrides_from_config(config: Option<&ProjectConfig>) -> OptionOverrides {
    OptionOverrides {
        dialect: config.and_then(|c| c.dialect),
        types: config.and_then(|c| c.types),
        lints: config.map(|c| c.lints.clone()).unwrap_or_default(),
        deny_warnings: config.and_then(|c| c.deny_warnings),
    }
}

/// Asset loader for `.ink` (source) files.
///
/// Reads the entry source, walks the `INCLUDE` graph asynchronously
/// through Bevy's `AssetReader`, lands the drained sources in a
/// [`brink_source_tree::InMemory`] tree, then goes through the
/// [`brink_environment`] producer: [`Project::load`] resolves `brink.toml`
/// and override precedence and freezes an `Environment`, and
/// [`brink_environment::compile`] compiles it. Links the result via
/// [`brink_runtime::link`] and emits labeled subassets (`#program`,
/// `#line_tables`) just like [`InkbLoader`](crate::InkbLoader).
///
/// ## `brink.toml` discovery (#1029, #1360, #1406)
///
/// A `brink.toml` beside (or above) the entry asset supplies the
/// [`ProjectConfig`] (`dialect`/`types`) that gates T1b brink-extension
/// syntax — the same file the CLI discovers by walking the real
/// filesystem (`brink-project-config::load_from_entry`). Bevy's
/// `AssetReader` may be virtual or packed, so [`load`](AssetLoader::load)
/// probes for it over the async reader itself: `brink.toml` beside the
/// entry, then each ancestor directory in turn, via
/// [`LoadContext::read_asset_bytes`] — bounded at the asset source root
/// (never above it) and naturally finite (the entry path has finitely many
/// `/`-separated ancestors). Every candidate found (not just the nearest)
/// is registered as a load dependency exactly like an `INCLUDE`, so editing
/// any of them in dev mode hot-reloads the story, and its text is landed at
/// its own key in the drained source map. Discovering *which* key governs,
/// parsing it, and applying `CLI/API > file > default` precedence is then
/// entirely [`Project::load`]'s job
/// (`brink_project_config::discover_from_entry_in_tree` over the same map,
/// walking the same bounded ancestor chain once more — but now as the
/// *only* place that walk makes a governing-candidate decision, see
/// [`probe_brink_toml`]'s doc) — this loader only supplies candidate bytes,
/// it does not re-implement the resolution policy. A miss at every level
/// leaves `AnalysisOptions` at its default — byte-identical to pre-#1029
/// behavior.
///
/// [`override_config`](Self::override_config), set via
/// [`BrinkPlugin::with_config`](crate::BrinkPlugin::with_config) /
/// [`BrinkAssetsPlugin::with_config`](crate::BrinkAssetsPlugin::with_config),
/// is the programmatic escape hatch: when set, its fields win over
/// whatever `brink.toml` supplies — passed through as
/// [`OptionOverrides`], the same `explicit call always wins over the file`
/// precedence [`Project::load`] applies for every mount (CLI included).
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
    /// The `brink-environment` producer failed to resolve the drained
    /// sources into an `Environment` — `INCLUDE` discovery (missing/circular
    /// include) or a malformed discovered `brink.toml` (bad TOML syntax, or
    /// a recognized key with a value outside its enum; unknown keys are
    /// warnings, never this). See `brink_environment::LoadError`.
    #[error("load environment: {0}")]
    Load(#[from] brink_environment::LoadError),
    #[error("compile: {0}")]
    Compile(#[from] brink_compiler::CompileError),
    #[error("link error: {0}")]
    Link(#[from] brink_runtime::RuntimeError),
}

/// Errors from [`compile_story_inline`].
#[derive(Debug, thiserror::Error)]
pub enum CompileStoryInlineError {
    /// The `brink-environment` producer failed to resolve the single-file
    /// tree into an `Environment` — most commonly an unresolvable `INCLUDE`
    /// (the tree has only `name`, so any `INCLUDE`'d path always misses),
    /// but also reachable via a malformed discovered `brink.toml` or a
    /// circular `INCLUDE`. See `brink_environment::LoadError`.
    ///
    /// The `#[error]` message carries the same authoring guidance the old
    /// (pre-#1372) read closure used to surface directly, since this is
    /// often the only diagnostic a caller sees — all three demo call sites
    /// (`demos/compound/src/ink_{doors,cameras,alarm}.rs`) `.expect()` on
    /// this function's result.
    #[error(
        "load environment: {0} (compile_story_inline compiles a single in-memory source; use InkLoader/AssetServer for multi-file stories)"
    )]
    Load(#[from] brink_environment::LoadError),
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
/// ## Goes through the same producer as [`InkLoader`] (#1372)
///
/// `name`/`source` are landed as the sole entry in a
/// [`brink_source_tree::InMemory`] tree, then handed to
/// [`brink_environment::Project::load`] → [`brink_environment::compile`] —
/// the exact same two-call seam [`InkLoader::load`] uses. This closes the
/// `brink.toml`-discovery half of the divergence #1360 left open: before,
/// this function called `brink_compiler::compile` directly, a second compile
/// path with its own (in this case: no) `brink.toml`/precedence resolution,
/// separate from `InkLoader`'s. Both entry points now run the identical
/// `resolve_options` codepath over their respective trees, so a future
/// change to *that* resolution logic can't silently diverge between them
/// again.
///
/// ## Picks up `with_config` too (#1380)
///
/// [`BrinkAssetsPlugin::build`](crate::BrinkAssetsPlugin) mirrors whatever
/// `with_config` override it resolved for [`InkLoader::override_config`]
/// into a [`BrinkOverrideConfig`] resource. This function reads that
/// resource back out of `app`'s `World` (absent — no `BrinkAssetsPlugin` in
/// the app at all — is treated the same as `None`) and runs it through the
/// same [`overrides_from_config`] mapping `InkLoader::load` uses, so an app
/// built with `BrinkPlugin::with_config(dialect = Brink)` now compiles
/// inline sources under `Brink` too, matching its `InkLoader`-loaded
/// assets — closing the divergence #1372 narrowed down to "one missing
/// override wire".
///
/// **Call this *after* `app.add_plugins(BrinkPlugin::<M>::with_config(...))`
/// / `BrinkAssetsPlugin::with_config(...)`, not before.** `BrinkOverrideConfig`
/// only exists in `app`'s `World` once `BrinkAssetsPlugin::build` has run,
/// and Bevy runs `Plugin::build` at `add_plugins` time, not at struct
/// construction. Calling `compile_story_inline` *before* that `add_plugins`
/// call finds no `BrinkOverrideConfig` resource yet — the same "absent"
/// case as no plugin at all — and silently compiles under
/// `OptionOverrides::default()` with no diagnostic, even though a
/// `with_config` override is sitting right there waiting to be installed.
/// This is the exact kind of silent divergence #1380 set out to kill,
/// relocated into a call-ordering footgun rather than eliminated; there is
/// no runtime check for it (a freestanding `fn(&mut App, ...)` can't tell
/// "no override was ever configured" apart from "the override exists but
/// its plugin hasn't built yet"). See
/// `compile_story_inline_before_plugin_add_silently_falls_back_to_default`
/// for the regression pinning this fallback.
///
/// `name` is the compiler's synthetic entry file name (also its `INCLUDE`
/// resolution root). Because the tree has exactly one key (`name`), `INCLUDE`
/// can never resolve here: a directive is still discovered by the same BFS
/// `InkLoader` uses, but always misses the single-key tree and surfaces as
/// [`CompileStoryInlineError::Load`] rather than silently doing nothing; a
/// story spanning multiple files needs [`InkLoader`]/`AssetServer::load`
/// instead, which walks the graph asynchronously. A `brink.toml` is
/// discovered the same way — never, for the same single-key reason — so
/// `AnalysisOptions` always resolves to its default here.
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
    let mut sources = BTreeMap::new();
    sources.insert(name.to_string(), source.to_string());
    let tree = InMemory::new(sources);
    let override_config = app
        .world()
        .get_resource::<BrinkOverrideConfig>()
        .and_then(|r| r.0.as_ref());
    let overrides = overrides_from_config(override_config);
    let env = Project::load(&tree, name, &overrides)?;
    let output = brink_environment::compile(&env)?;
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
/// filesystem. Every candidate found is read via
/// [`LoadContext::read_asset_bytes`], which registers it as a load
/// dependency, so hot-reload "just works" exactly like an `INCLUDE`.
///
/// Returns every candidate this probe found along the way — key + raw
/// text, **not** parsed, and no precedence applied. This loader's job
/// stops at supplying bytes; the caller lands every pair at its own key in
/// the drained source map, and [`Project::load`] (over the resulting
/// `SourceTree`) then runs its own discovery walk
/// (`brink_project_config::discover_from_entry_in_tree`) to decide which
/// candidate governs, parses it, and applies the `CLI/API > file >
/// default` precedence (#1360).
///
/// ## One walk, not two (#1406)
///
/// This probe deliberately does **not** stop at the first hit. #1360 left
/// the bounded ancestor walk-up performed twice — once here (fetching
/// bytes through the async `AssetReader`, stopping as soon as a candidate
/// was found) and again inside `Project::load` (`discover_from_entry_in_tree`,
/// walking the same ancestor chain over the resulting tree to decide which
/// candidate governs) — so this loader was quietly re-implementing the
/// same nearest-wins decision `discover_from_entry_in_tree` already makes,
/// with the two walks only ever agreeing because at most one candidate was
/// ever landed in the tree to find. Walking every ancestor to the asset
/// source root instead — landing every candidate that exists, not just
/// the nearest — means `Project::load`'s own walk is the **only** place
/// that decision is made; this probe just gathers raw bytes. Only the
/// nearest candidate is ever actually read as config (farther ones are
/// shadowed exactly as before — [`Project::load`]'s walk still stops at
/// the first hit it finds), so the extra reads never change which
/// candidate governs, just who decides.
///
/// A miss at every ancestor returns `Ok(vec![])` — not an error, matching
/// `brink-project-config`'s "missing config changes nothing" contract.
async fn probe_brink_toml(
    load_context: &mut LoadContext<'_>,
    entry_path: &str,
) -> Result<Vec<(String, String)>, InkLoaderError> {
    let mut found = Vec::new();
    for dir in ancestor_dirs(entry_path) {
        let candidate = if dir.is_empty() {
            brink_project_config::CONFIG_FILE_NAME.to_string()
        } else {
            format!("{dir}/{}", brink_project_config::CONFIG_FILE_NAME)
        };
        if let Ok(bytes) = load_context.read_asset_bytes(candidate.clone()).await {
            match String::from_utf8(bytes) {
                Ok(text) => found.push((candidate, text)),
                // Only the nearest candidate is load-relevant (it's the only
                // one `discover_from_entry_in_tree` can ever pick); an
                // undecodable *farther* ancestor is shadowed and must not
                // fail a load that would otherwise succeed.
                Err(err) if found.is_empty() => return Err(err.into()),
                Err(_) => {}
            }
        }
    }
    Ok(found)
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
        // automatic hot-reload). `BTreeMap` (not `HashMap`): it lands
        // directly in an `InMemory` `SourceTree` below, whose contract is a
        // deterministic key order.
        let mut sources: BTreeMap<String, String> = BTreeMap::new();
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

        // #1029/#1360/#1406: bounded ancestor walk-up for `brink.toml`
        // through the async AssetReader (see the module/struct docs). Only
        // the bytes are fetched here — every candidate found along the way,
        // not just the nearest — landing each pair at its own key in
        // `sources` lets `Project::load` below run the *sole* discovery
        // walk (over the resulting tree) that decides which candidate
        // governs, parses it, and applies precedence; this loader no longer
        // duplicates that decision.
        for (config_path, text) in probe_brink_toml(load_context, &entry_path).await? {
            sources.insert(config_path, text);
        }

        // The `CLI/API > file > default` precedence's explicit-override
        // side (#1005): `override_config` — set via
        // `BrinkPlugin::with_config` — wins over whatever `brink.toml` (just
        // landed above) supplies. `brink.toml` itself, and applying that
        // precedence, is entirely `Project::load`'s job. #1394's lint tier
        // and #1380's `compile_story_inline` wiring both go through this
        // same `overrides_from_config` mapping — no second implementation.
        let overrides = overrides_from_config(self.override_config.as_ref());

        // Land the drained map in a `SourceTree` and go through the
        // producer: `Project::load` resolves `brink.toml` + precedence and
        // freezes an `Environment`; `compile` is the pure function over it.
        // Both are synchronous — the only asynchrony (the BFS above) is
        // already behind us.
        let tree = InMemory::new(sources);
        let env = Project::load(&tree, &entry_path, &overrides)?;
        let output = brink_environment::compile(&env)?;
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

    /// #1372: `compile_story_inline` now goes through the same
    /// `Project::load` → `compile` producer seam as [`InkLoader`], instead of
    /// calling `brink_compiler::compile` directly. Pin the precedence
    /// consequence both entry points now share: with no `brink.toml`
    /// reachable (here, structurally — the tree has only the single inline
    /// source), `AnalysisOptions` resolves to its default (`StrictInk`),
    /// which rejects the brink-extension `#@private` form — the exact same
    /// fixture and expectation as
    /// `config_discovery_tests::missing_brink_toml_leaves_default_dialect_unchanged`
    /// for `InkLoader`.
    #[test]
    fn compile_story_inline_has_no_brink_toml_and_uses_default_dialect() {
        let mut app = crate::test_support::make_test_app();

        let err = compile_story_inline(
            &mut app,
            "inline.ink",
            "#@private\nVAR secret = 0\n-> END\n",
        )
        .expect_err("brink-extension syntax should be rejected under the default dialect");
        assert!(
            matches!(err, CompileStoryInlineError::Compile(_)),
            "got {err:?}"
        );
    }

    /// #1372: an `INCLUDE` in the inline source is followed through the same
    /// `Project::load` discovery BFS `InkLoader` uses, but the single-key
    /// tree can never satisfy it — surfaces as `CompileStoryInlineError::Load`
    /// (a `brink_environment::LoadError`, not `brink_compiler::CompileError`
    /// as it did before this function was rerouted through the producer).
    ///
    /// Also pins the authoring-guidance substring carried in `Load`'s
    /// `#[error]` message (the old read closure's hint, since #1372 turned
    /// this into the sole diagnostic every `.expect()`-ing demo call site
    /// sees) so it cannot silently rot away in a future edit.
    #[test]
    fn compile_story_inline_surfaces_load_error_for_unresolvable_include() {
        let mut app = crate::test_support::make_test_app();

        let err = compile_story_inline(&mut app, "inline.ink", "INCLUDE missing.ink\n-> END\n")
            .expect_err("an INCLUDE can never resolve in a single-key inline tree");
        assert!(
            matches!(err, CompileStoryInlineError::Load(_)),
            "got {err:?}"
        );
        assert!(
            err.to_string().contains(
                "compile_story_inline compiles a single in-memory source; use InkLoader/AssetServer for multi-file stories"
            ),
            "error message should carry authoring guidance, got: {err}"
        );
    }

    // ── `with_config` reaches `compile_story_inline` too (#1380) ───────
    //
    // `compile_story_inline_has_no_brink_toml_and_uses_default_dialect`
    // above pins the no-override baseline: `#@private` is rejected under
    // the default `StrictInk` dialect. These two tests prove a
    // `BrinkPlugin::with_config` override actually reaches this entry
    // point too (it didn't, pre-#1380 — see the removed "one override
    // channel is still divergent" doc note this PR replaces), each picking
    // a fixture whose outcome only the override — not the default policy
    // — can flip (house rule 19q).

    /// `dialect = Brink`, set via `BrinkPlugin::with_config`, must reach
    /// `compile_story_inline`'s `Project::load` call the same way it
    /// already reaches `InkLoader::load` — proven by compiling the same
    /// `#@private` brink-extension source the no-override baseline test
    /// rejects.
    #[test]
    fn compile_story_inline_reaches_plugin_with_config_dialect_override() {
        use brink_project_config::Dialect;

        let mut app = bevy_app::App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(
            crate::BrinkPlugin::<()>::default().with_config(ProjectConfig {
                dialect: Some(Dialect::Brink),
                ..ProjectConfig::default()
            }),
        );

        compile_story_inline(
            &mut app,
            "inline.ink",
            "#@private\nVAR secret = 0\n-> END\n",
        )
        .expect(
            "dialect = Brink override should reach compile_story_inline and permit \
                 brink-extension syntax the default dialect rejects",
        );
    }

    /// The `[lints] deny-warnings` tier (#1394's addition to the same
    /// `OptionOverrides` seam) reaches `compile_story_inline` too, not just
    /// `dialect`/`types` — a logic line with no effect (`~` alone,
    /// `DiagnosticCode::E014`, `Warning` by default) compiles cleanly with
    /// no override, but a `deny_warnings = true` override relevels it to a
    /// blocking `Error`.
    #[test]
    fn compile_story_inline_reaches_plugin_with_config_deny_warnings_override() {
        let mut app = bevy_app::App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(
            crate::BrinkPlugin::<()>::default().with_config(ProjectConfig {
                deny_warnings: Some(true),
                ..ProjectConfig::default()
            }),
        );

        let err = compile_story_inline(&mut app, "inline.ink", "Hello.\n~\n-> END\n")
            .expect_err("deny_warnings override should relevel E014 to a blocking error");
        assert!(
            matches!(err, CompileStoryInlineError::Compile(_)),
            "got {err:?}"
        );
    }

    /// Negative control for the two tests above (house rule 19q): the same
    /// `deny_warnings`-sensitive source compiles cleanly with no
    /// `with_config` override at all, so the override above is what
    /// actually flips the outcome, not something else about the fixture.
    #[test]
    fn compile_story_inline_without_config_leaves_e014_a_warning() {
        let mut app = crate::test_support::make_test_app();

        compile_story_inline(&mut app, "inline.ink", "Hello.\n~\n-> END\n")
            .expect("E014 is a Warning, never blocking, with no deny_warnings override");
    }

    /// An untyped function parameter, called with an argument — same fixture
    /// (and same reasoning) as `config_discovery_tests`'
    /// `UNTYPED_PARAM_SOURCE`: `dialect = Brink`'s own resolved-policy
    /// default is `Strict`, which rejects it with 2 diagnostics; only an
    /// explicit `types = Gradual` override compiles it.
    const UNTYPED_PARAM_SOURCE: &str =
        "=== function f(x) ===\n~ return x\n\n=== start ===\n{f(1)}\n-> END\n";

    /// `types = Gradual`, set via `BrinkPlugin::with_config` alongside
    /// `dialect = Brink`, must reach `compile_story_inline`'s `Project::load`
    /// call the same way it already reaches `InkLoader::load`
    /// (`plugin_with_config_types_reaches_ink_loader` in
    /// `config_discovery_tests`) — mirrored here since #1380's fix covers
    /// `compile_story_inline` too, not just `InkLoader` (house rule 19q: the
    /// dialect-keyed default alone, `Strict`, still rejects
    /// `UNTYPED_PARAM_SOURCE`, so only the `types` override can flip this
    /// outcome).
    #[test]
    fn compile_story_inline_reaches_plugin_with_config_types_override() {
        use brink_project_config::{Dialect, TypePolicy};

        let mut app = bevy_app::App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());
        app.add_plugins(
            crate::BrinkPlugin::<()>::default().with_config(ProjectConfig {
                dialect: Some(Dialect::Brink),
                types: Some(TypePolicy::Gradual),
                ..ProjectConfig::default()
            }),
        );

        compile_story_inline(&mut app, "inline.ink", UNTYPED_PARAM_SOURCE).expect(
            "types = Gradual override should reach compile_story_inline and permit the \
             untyped parameter the dialect-keyed Strict default rejects",
        );
    }

    /// A per-code `[lints]` override (not just the blanket `deny_warnings`
    /// knob already covered above) must reach `compile_story_inline` too —
    /// mirrors `plugin_override_lints_wins_over_conflicting_asset` /
    /// `plugin_with_config_dialect_reaches_ink_loader`'s `[lints]` coverage
    /// for `InkLoader`, but through `compile_story_inline`'s
    /// `BrinkOverrideConfig` channel.
    #[test]
    fn compile_story_inline_reaches_plugin_with_config_per_code_lint_override() {
        let mut app = bevy_app::App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());

        let mut lints = std::collections::BTreeMap::new();
        lints.insert("E014".to_owned(), brink_project_config::LintLevel::Deny);
        app.add_plugins(
            crate::BrinkPlugin::<()>::default().with_config(ProjectConfig {
                lints,
                ..ProjectConfig::default()
            }),
        );

        let err = compile_story_inline(&mut app, "inline.ink", "Hello.\n~\n-> END\n").expect_err(
            "a per-code `[lints] E014 = deny` override should relevel E014 \
                to a blocking error",
        );
        assert!(
            matches!(err, CompileStoryInlineError::Compile(_)),
            "got {err:?}"
        );
    }

    /// #1380 review finding: `compile_story_inline` reads
    /// `BrinkOverrideConfig` out of `app.world()` at call time, but that
    /// resource is only inserted once `BrinkAssetsPlugin::build` actually
    /// runs — which happens at `app.add_plugins(...)` time, not at
    /// `BrinkPlugin::with_config(...)` construction time. Calling
    /// `compile_story_inline` *before* `add_plugins` finds no
    /// `BrinkOverrideConfig` in the world at all — indistinguishable from
    /// "no `BrinkAssetsPlugin` ever added" — and silently falls back to
    /// `OptionOverrides::default()`, dropping the override with no
    /// diagnostic. This pins that fallback so it can't drift into something
    /// else (e.g. a panic) unnoticed; see `compile_story_inline`'s doc
    /// comment for the full hazard.
    #[test]
    fn compile_story_inline_before_plugin_add_silently_falls_back_to_default() {
        use brink_project_config::Dialect;

        let mut app = bevy_app::App::new();
        app.add_plugins(bevy_asset::AssetPlugin::default());

        // Deliberately NOT added yet -- `compile_story_inline` runs first.
        let pending_plugin = crate::BrinkPlugin::<()>::default().with_config(ProjectConfig {
            dialect: Some(Dialect::Brink),
            ..ProjectConfig::default()
        });

        let err = compile_story_inline(
            &mut app,
            "inline.ink",
            "#@private\nVAR secret = 0\n-> END\n",
        )
        .expect_err(
            "with no BrinkOverrideConfig resource yet inserted, the default StrictInk \
             dialect must still reject brink-extension syntax -- the override cannot \
             reach a call made before its plugin builds",
        );
        assert!(
            matches!(err, CompileStoryInlineError::Compile(_)),
            "got {err:?}"
        );

        // Adding the plugin after the fact doesn't retroactively help the
        // already-failed call above -- but confirms the override itself is
        // wired correctly (would reach a *subsequent* call), isolating the
        // failure above to ordering, not a broken override.
        app.add_plugins(pending_plugin);
        compile_story_inline(
            &mut app,
            "inline.ink",
            "#@private\nVAR secret = 0\n-> END\n",
        )
        .expect("once the plugin has built, the same override now reaches the same call");
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
    use brink_project_config::{Dialect, TypePolicy};

    use super::{InkLoader, ProjectConfig};
    use crate::asset::{BrinkStoryAsset, LineTablesAsset, ProgramAsset};

    /// A brink-extension form (`#@private`) that the default `StrictInk`
    /// dialect rejects (E051-class dialect-gate diagnostic,
    /// `brink-analyzer`'s `dialect_gate`) but `dialect = brink` compiles —
    /// the reachability proof #1029 calls for: a bevy story with
    /// `dialect = brink` in a sibling `brink.toml` compiles a
    /// brink-extension form that fails under the default.
    const BRINK_ONLY_SOURCE: &str = "#@private\nVAR secret = 0\n-> END\n";

    /// A logic line with no effect (`~` alone) — `DiagnosticCode::E014`,
    /// `Warning` by default (mirrors `brink_environment`'s own `E014_SOURCE`
    /// fixture, `crates/internal/brink-environment/src/lib.rs`). Compiles
    /// cleanly under the default policy; only relevels to a blocking `Error`
    /// (and so a `Failed` load) once `[lints]` denies it or sets
    /// `deny-warnings` (issue #1394).
    const E014_SOURCE: &str = "Hello.\n~\n-> END\n";

    /// An untyped function parameter (`f(x)`, no `: type` annotation) called
    /// with an argument: `dialect = Brink`'s own resolved-policy default is
    /// `Strict` (`AnalysisOptions::type_policy`), which rejects an untyped
    /// param with 2 diagnostics — only an explicit `types = gradual`
    /// override compiles it. Verified against this crate's own `brink`
    /// CLI: `brink compile fnreturn.ink --dialect brink` exits 1 with 2
    /// diagnostics; `--dialect brink --types gradual` exits 0.
    ///
    /// Replaces a prior `STRUCT NPC = ...; VAR npc = NPC#{...}` fixture that
    /// was meant to hit `E075` (struct literals as declaration defaults) but
    /// didn't — it compiled cleanly under `types = strict` too (verified the
    /// same way), so the `types` override test built on it was vacuous
    /// (house rule 19q; #1426 w52 review).
    const UNTYPED_PARAM_SOURCE: &str =
        "=== function f(x) ===\n~ return x\n\n=== start ===\n{f(1)}\n-> END\n";

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

    /// Same as [`make_memory_asset_app`], except the `.ink` loader is wired
    /// through the *real* [`crate::plugin::BrinkAssetsPlugin::with_config`]
    /// plugin-build path instead of a hand-constructed `InkLoader {
    /// override_config }`. Every other case in `config_discovery_tests`
    /// constructs `InkLoader` directly, which only proves
    /// `InkLoader::override_config`'s own behavior — not that
    /// `BrinkPlugin::with_config` / `BrinkAssetsPlugin::with_config`
    /// actually thread an override into it (`with_config` ->
    /// `with_config_option` -> `BrinkAssetsPlugin::build` ->
    /// `InkLoader { override_config }`). Use this builder for any test
    /// whose claim is specifically about `with_config`'s reachability.
    fn make_memory_asset_app_with_config(config: ProjectConfig) -> (bevy_app::App, Dir) {
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
            crate::plugin::BrinkAssetsPlugin::default().with_config(config),
        ));
        (app, dir)
    }

    /// Same as [`make_memory_asset_app_with_config`], but wired through
    /// [`crate::BrinkPlugin::with_config`] instead of
    /// [`crate::plugin::BrinkAssetsPlugin::with_config`] directly — proves
    /// the two-hop delegation (`BrinkPlugin::build` ->
    /// `BrinkAssetsPlugin::with_config_option` -> `InkLoader {
    /// override_config }`) a host adding the marker-parameterized plugin
    /// actually goes through, not just `BrinkAssetsPlugin` in isolation.
    fn make_memory_asset_app_via_brink_plugin_with_config(
        config: ProjectConfig,
    ) -> (bevy_app::App, Dir) {
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
            crate::BrinkPlugin::<()>::default().with_config(config),
        ));
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

    /// #1406 regression: with a `brink.toml` at *two* ancestor levels
    /// (conflicting settings), the nearest one must still govern even
    /// though [`super::probe_brink_toml`] now lands **both** candidates in
    /// the drained tree (it no longer stops at the first hit — see its doc
    /// comment). The farther, root-level `brink.toml` sets
    /// `dialect = "strict-ink"`, which alone would reject `BRINK_ONLY_SOURCE`
    /// (`#@private`) — only the nearer `stories/ch1/brink.toml`'s
    /// `dialect = "brink"` makes it compile, so a `Loaded` outcome here
    /// proves `Project::load`'s own walk (`discover_from_entry_in_tree`),
    /// not this probe, is still the sole "which candidate governs" decider.
    #[test]
    fn nearest_ancestor_brink_toml_shadows_a_farther_conflicting_one() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("stories/ch1/intro.ink"), BRINK_ONLY_SOURCE);
        dir.insert_asset_text(
            Path::new("stories/ch1/brink.toml"),
            "[project]\ndialect = \"brink\"\n",
        );
        dir.insert_asset_text(
            Path::new("brink.toml"),
            "[project]\ndialect = \"strict-ink\"\n",
        );

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("stories/ch1/intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    /// w-review regression: an *undecodable* farther ancestor `brink.toml`
    /// must not fail a load whose nearest, decodable candidate is the one
    /// that actually governs. On `main` (pre-guard), `probe_brink_toml`
    /// propagates `String::from_utf8`'s error for *every* candidate it
    /// reads, including shadowed ones — a non-UTF-8 root-level
    /// `brink.toml` that `discover_from_entry_in_tree` would never even
    /// look at (the nearer `stories/ch1/brink.toml` shadows it) turns a
    /// load that used to succeed into a `Failed` one. This fails on that
    /// code path and only passes once the probe skips a decode error on a
    /// candidate found *after* a decodable one already landed.
    #[test]
    fn undecodable_farther_ancestor_brink_toml_does_not_fail_the_load() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("stories/ch1/intro.ink"), BRINK_ONLY_SOURCE);
        dir.insert_asset_text(
            Path::new("stories/ch1/brink.toml"),
            "[project]\ndialect = \"brink\"\n",
        );
        // Invalid UTF-8 (a lone continuation byte) -- must never be decoded
        // as config text, since the nearer candidate above already governs.
        dir.insert_asset(Path::new("brink.toml"), vec![0x80_u8]);

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
                ..ProjectConfig::default()
            }),
        });

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    // ── `[lints]` re-level, observable through the bevy load path (#1394) ──
    //
    // A served `brink.toml`'s `[lints]`/`deny-warnings` table was already
    // reaching `Project::load` before this issue — `resolve_options` there
    // applies the discovered file's config unconditionally, regardless of
    // `OptionOverrides` (see `sibling_brink_toml_lints_deny_relevels_...`
    // below, both regression guards for that pre-existing file tier, not
    // new coverage). What `InkLoader` actually dropped was narrower: a
    // `BrinkPlugin::with_config` override's `lints`/`deny_warnings` never
    // reached `OptionOverrides`, so it could never win over (or supply, with
    // no `brink.toml` present) the file. `plugin_override_lints_wins_over_conflicting_asset`
    // and `plugin_override_deny_warnings_relevels_warning_to_failed_load`
    // below are the tests that actually exercise the fixed seam. These
    // mirror `brink_environment`'s own `[lints]` tests (`E014_SOURCE`, a
    // Warning by default that only blocks compilation once denied), but
    // drive them through the real `AssetServer`/`InkLoader` seam this crate
    // owns.

    #[test]
    fn e014_source_loads_by_default_with_no_lints_table() {
        // Baseline: E014 is a Warning, never blocking, with no `[lints]`
        // anywhere -- the contrast case for the two `Failed` tests below.
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    #[test]
    fn sibling_brink_toml_lints_deny_relevels_warning_to_failed_load() {
        // A served brink.toml's `[lints] E014 = "deny"` re-levels the
        // Warning to a blocking Error -- observable as a `Failed` load
        // through the bevy path, exactly as it already blocks the CLI.
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);
        dir.insert_asset_text(Path::new("brink.toml"), "[lints]\nE014 = \"deny\"\n");

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    #[test]
    fn sibling_brink_toml_deny_warnings_relevels_warning_to_failed_load() {
        // Same re-level, via the `deny-warnings = true` blanket knob rather
        // than a per-code entry.
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);
        dir.insert_asset_text(Path::new("brink.toml"), "[lints]\ndeny-warnings = true\n");

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    #[test]
    fn plugin_override_lints_wins_over_conflicting_asset() {
        // The asset explicitly allows E014 (so, alone, the load would
        // succeed); the plugin override denies it -- override must win,
        // same `override > file > default` precedence as dialect/types.
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);
        dir.insert_asset_text(Path::new("brink.toml"), "[lints]\nE014 = \"allow\"\n");

        let mut lints = std::collections::BTreeMap::new();
        lints.insert("E014".to_owned(), brink_project_config::LintLevel::Deny);
        app.register_asset_loader(InkLoader {
            override_config: Some(ProjectConfig {
                lints,
                ..ProjectConfig::default()
            }),
        });

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    #[test]
    fn plugin_override_deny_warnings_relevels_warning_to_failed_load() {
        // No `brink.toml` at all -- alone, E014 stays a non-blocking
        // Warning (see `e014_source_loads_by_default_with_no_lints_table`).
        // The plugin override's blanket `deny_warnings` knob must still
        // relevel it to a blocking Error on its own, the same way a served
        // `brink.toml`'s `deny-warnings = true` does
        // (`sibling_brink_toml_deny_warnings_relevels_warning_to_failed_load`),
        // proving the `.deny_warnings` field (not just `.lints`) actually
        // reaches `OptionOverrides` through `InkLoader::load`.
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);

        app.register_asset_loader(InkLoader {
            override_config: Some(ProjectConfig {
                deny_warnings: Some(true),
                ..ProjectConfig::default()
            }),
        });

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    // ── unknown/non-overridable `[lints]` codes warn, not drop (#1416) ──
    //
    // `AnalysisOptions::apply_lint_overrides` (`brink-analyzer`) already
    // rejects a code that isn't a real `DiagnosticCode`, or names one whose
    // *base* severity isn't `Warning`, returning a `ConfigWarning` instead of
    // silently merging it — `resolve_options` (`brink-environment`) loops
    // those through `tracing::warn!` unconditionally, regardless of whether
    // a `brink.toml` was even discovered (see its own doc comment). Since
    // `bevy_log`'s macros are `tracing`'s own, re-exported verbatim, and
    // `LogPlugin` installs a process-wide `tracing` subscriber, that
    // `tracing::warn!` call already reaches a bevy author's console in any
    // real app -- the CLI/`brink.toml` mounts rely on the exact same
    // ambient-dispatch mechanism, just with `tracing_subscriber::fmt`
    // instead of `LogPlugin` as the installed subscriber. What was actually
    // untested (house rule 9) is that `BrinkPlugin::with_config`'s
    // `override_config.lints` -- forwarded into `OptionOverrides` by #1394 --
    // reaches that channel too, and that an invalid entry doesn't take a
    // valid sibling entry down with it. `CapturingSubscriber` below installs
    // itself as the *global* `tracing` default (not a thread-local one,
    // which `InkLoader::load` -- driven off the asset IO task pool thread --
    // would never see) so the warnings loop above is observed no matter
    // which pool thread runs the compile.
    struct CapturingSubscriber {
        messages: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl tracing::field::Visit for CapturedMessage {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }

    struct CapturedMessage(String);

    impl tracing::Subscriber for CapturingSubscriber {
        // Global (process-wide, whole-test-binary-lifetime) subscriber, so
        // this must not accept every `trace!`/`debug!`/`info!` from every
        // crate (bevy_asset, bevy_ecs, brink-*) in every other test running
        // concurrently -- that would tax unrelated tests and let their
        // events contaminate this test's substring assertions. Only the
        // warnings this test actually cares about need capturing.
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() <= tracing::Level::WARN
        }

        fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
            Some(tracing::level_filters::LevelFilter::WARN)
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut captured = CapturedMessage(String::new());
            event.record(&mut captured);
            self.messages.lock().unwrap().push(captured.0);
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    /// Install (once per test binary) a process-wide capturing subscriber
    /// and return the shared buffer every event's formatted message lands
    /// in. A single global install is required -- `tracing` only allows
    /// setting the global default once -- so every test that calls this
    /// shares one growing buffer; that's fine here because each test only
    /// asserts its own uniquely-spelled codes appear somewhere in it, never
    /// that the buffer is otherwise empty.
    fn captured_warnings() -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        static MESSAGES: std::sync::OnceLock<std::sync::Arc<std::sync::Mutex<Vec<String>>>> =
            std::sync::OnceLock::new();
        let messages = MESSAGES.get_or_init(|| {
            let messages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let subscriber = CapturingSubscriber {
                messages: std::sync::Arc::clone(&messages),
            };
            // Ignore a "someone already set it" error: nothing else in this
            // crate's test binary installs a global subscriber, so in
            // practice this always wins on first call.
            let _ = tracing::subscriber::set_global_default(subscriber);
            messages
        });
        std::sync::Arc::clone(messages)
    }

    #[test]
    fn plugin_override_unknown_and_non_overridable_lint_codes_warn_but_valid_entry_still_applies() {
        let warnings = captured_warnings();

        let mut lints = std::collections::BTreeMap::new();
        // Not a real `DiagnosticCode` -- never parses.
        lints.insert(
            "E9999_TYPO".to_owned(),
            brink_project_config::LintLevel::Deny,
        );
        // A real code, but its base severity is `Error`, not `Warning` --
        // never overridable (mirrors `brink-analyzer`'s own
        // `apply_lint_overrides_rejects_non_overridable_code` unit test).
        lints.insert("E001".to_owned(), brink_project_config::LintLevel::Deny);
        // The valid entry: must still apply and relevel E014 to a blocking
        // Error, proving the two invalid siblings above don't take it down
        // with them.
        lints.insert("E014".to_owned(), brink_project_config::LintLevel::Deny);

        // Routed through `BrinkAssetsPlugin::with_config` (not a
        // hand-registered `InkLoader`) so this test actually proves the
        // `BrinkPlugin::with_config` wiring path, not just
        // `InkLoader::override_config` in isolation.
        let (mut app, dir) = make_memory_asset_app_with_config(ProjectConfig {
            lints,
            ..ProjectConfig::default()
        });
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);

        let joined = warnings.lock().unwrap().join("\n");
        // Assert the exact message text `validate_lint_code`
        // (`brink-analyzer/src/lib.rs`) emits for each rejection class, not
        // just the code substring -- a plain `contains("E001")` can't tell
        // "not a recognized diagnostic code" apart from "not overridable",
        // so it would pass identically even if the two rejection paths were
        // swapped.
        assert!(
            joined.contains("[lints] `E9999_TYPO` is not a recognized diagnostic code"),
            "an unknown lint code must warn with the 'not a recognized diagnostic code' \
             message (not silently drop); captured: {joined}"
        );
        assert!(
            joined.contains("[lints] `E001` is not overridable"),
            "a non-overridable lint code must warn with the 'not overridable' message \
             (not silently drop); captured: {joined}"
        );
    }

    // ── plugin-level `with_config` coverage across all four knobs (#1426) ──
    //
    // The tests above prove `[lints]` reaches through `with_config` at the
    // plugin level. `dialect`/`types`/`deny_warnings` previously only had
    // *loader*-level coverage (`InkLoader { override_config }` constructed
    // by hand, e.g. `plugin_override_wins_over_conflicting_asset` above) —
    // never proven to actually reach `InkLoader` through
    // `BrinkAssetsPlugin::with_config` / `BrinkPlugin::with_config`
    // themselves (`with_config` -> `with_config_option` ->
    // `BrinkAssetsPlugin::build` -> `InkLoader { override_config }`). Each
    // test below picks a fixture whose default-policy outcome the override
    // must actually flip (house rule: a value the default would already
    // produce proves nothing).

    #[test]
    fn plugin_with_config_dialect_reaches_ink_loader() {
        // Default dialect (StrictInk) rejects `#@private`
        // (`missing_brink_toml_leaves_default_dialect_unchanged` above) --
        // only an explicit `dialect = Brink` override flips that.
        let (mut app, dir) = make_memory_asset_app_with_config(ProjectConfig {
            dialect: Some(Dialect::Brink),
            ..ProjectConfig::default()
        });
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    #[test]
    fn plugin_with_config_types_reaches_ink_loader() {
        // `dialect = Brink` alone resolves `types` to its dialect-keyed
        // default, `Strict` (`AnalysisOptions::type_policy`), which rejects
        // `UNTYPED_PARAM_SOURCE`'s untyped function parameter -- only an
        // explicit `types = Gradual` override (on top of the same
        // `dialect = Brink`) makes it compile. See
        // `plugin_with_config_types_absent_leaves_strict_default_rejecting`
        // for the negative control proving the default (no `types`
        // override) still fails the same source.
        let (mut app, dir) = make_memory_asset_app_with_config(ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: Some(TypePolicy::Gradual),
            ..ProjectConfig::default()
        });
        dir.insert_asset_text(Path::new("intro.ink"), UNTYPED_PARAM_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    #[test]
    fn plugin_with_config_types_absent_leaves_strict_default_rejecting() {
        // Negative control for `plugin_with_config_types_reaches_ink_loader`
        // (house rule 19q): same `UNTYPED_PARAM_SOURCE`, same `dialect =
        // Brink`, but no `types` override -- the dialect-keyed default
        // (`Strict`) still rejects it, so the override in the sibling test
        // is what actually flips the outcome, not something else about the
        // fixture or the dialect setting.
        let (mut app, dir) = make_memory_asset_app_with_config(ProjectConfig {
            dialect: Some(Dialect::Brink),
            types: None,
            ..ProjectConfig::default()
        });
        dir.insert_asset_text(Path::new("intro.ink"), UNTYPED_PARAM_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    #[test]
    fn plugin_with_config_deny_warnings_reaches_ink_loader() {
        // `E014_SOURCE` loads cleanly under the default policy
        // (`e014_source_loads_by_default_with_no_lints_table` above) --
        // only an explicit `deny_warnings = true` override relevels its
        // Warning to a blocking Error.
        let (mut app, dir) = make_memory_asset_app_with_config(ProjectConfig {
            deny_warnings: Some(true),
            ..ProjectConfig::default()
        });
        dir.insert_asset_text(Path::new("intro.ink"), E014_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
    }

    #[test]
    fn brink_plugin_with_config_delegates_to_ink_loader() {
        // The two-hop path a real app actually uses (`BrinkPlugin<M>`, not
        // `BrinkAssetsPlugin` standalone) -- proves `BrinkPlugin::build`'s
        // `with_config_option` delegation actually carries the override
        // through, using the same dialect fixture as
        // `plugin_with_config_dialect_reaches_ink_loader`.
        let (mut app, dir) = make_memory_asset_app_via_brink_plugin_with_config(ProjectConfig {
            dialect: Some(Dialect::Brink),
            ..ProjectConfig::default()
        });
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_loaded(&mut app, &handle);
    }

    /// [`BrinkConfigWarnings`](crate::BrinkConfigWarnings)'s plugin-level
    /// wiring (#1426): `BrinkAssetsPlugin::build` inserts it eagerly, so a
    /// host never needs a real story load (or a `tracing` subscriber) to
    /// read a rejected `with_config` lint code.
    #[test]
    fn plugin_with_config_invalid_lint_code_reaches_brink_config_warnings_resource() {
        let mut lints = std::collections::BTreeMap::new();
        lints.insert(
            "E9999_TYPO".to_owned(),
            brink_project_config::LintLevel::Deny,
        );
        let (app, _dir) = make_memory_asset_app_with_config(ProjectConfig {
            lints,
            ..ProjectConfig::default()
        });

        let warnings = app.world().resource::<crate::BrinkConfigWarnings>();
        assert_eq!(warnings.0.len(), 1);
        assert!(
            warnings.0[0].contains("E9999_TYPO") && warnings.0[0].contains("not a recognized"),
            "unexpected resource contents: {warnings:?}"
        );
    }

    /// Additional coverage found while investigating #1436 — not one of the
    /// two gaps #1430's review thread actually named (a *served*
    /// `brink.toml`'s `[lints]` rejections and unknown-key warnings on the
    /// bevy loader path; see #1625, filed as the follow-up, for those).
    /// This closes a different, narrower gap: the sibling of
    /// [`plugin_with_config_invalid_lint_code_reaches_brink_config_warnings_resource`]
    /// above only ever exercised the *unknown-code* rejection class
    /// (`E9999_TYPO`, "not a recognized diagnostic code") against
    /// [`BrinkConfigWarnings`](crate::BrinkConfigWarnings) — the
    /// *non-overridable-code* class ("not overridable", e.g. `E001`, a
    /// real code whose default severity isn't `Warning`) had this exact
    /// resource-channel reachability covered only by
    /// `config_warnings::tests::non_overridable_code_surfaces_a_message_naming_it`,
    /// which calls `BrinkConfigWarnings::from_config` directly and so
    /// proves nothing about `with_config`'s own plugin-build wiring — the
    /// same gap class `plugin_with_config_invalid_lint_code_reaches_brink_config_warnings_resource`
    /// closed for the unknown-code class. `validate_lint_code`
    /// (`brink-analyzer`) returns a structurally different `ConfigWarning`
    /// per rejection class, so covering one does not prove the other is
    /// wired the same way.
    #[test]
    fn plugin_with_config_non_overridable_lint_code_reaches_brink_config_warnings_resource() {
        let mut lints = std::collections::BTreeMap::new();
        // A real `DiagnosticCode`, but its base severity is `Error`, not
        // `Warning` -- never overridable (mirrors
        // `apply_lint_overrides_rejects_non_overridable_code` in
        // `brink-analyzer`, and this file's own
        // `plugin_override_unknown_and_non_overridable_lint_codes_warn_but_valid_entry_still_applies`,
        // which proves the same code on the `tracing::warn!` channel, not
        // this resource).
        lints.insert("E001".to_owned(), brink_project_config::LintLevel::Deny);
        let (app, _dir) = make_memory_asset_app_with_config(ProjectConfig {
            lints,
            ..ProjectConfig::default()
        });

        let warnings = app.world().resource::<crate::BrinkConfigWarnings>();
        assert_eq!(warnings.0.len(), 1);
        assert!(
            warnings.0[0].contains("E001") && warnings.0[0].contains("not overridable"),
            "unexpected resource contents: {warnings:?}"
        );
    }

    /// Additional coverage found while investigating #1436 — not one of the
    /// two gaps #1430's review thread actually named (see the doc comment
    /// on `plugin_with_config_non_overridable_lint_code_reaches_brink_config_warnings_resource`
    /// above for those, and #1625, filed as the follow-up). This closes a
    /// different gap: `compile_story_inline` shares the exact same
    /// `Project::load` seam `InkLoader::load` uses (#1380's own doc
    /// comment: "the exact same two-call seam `InkLoader::load` uses") —
    /// so a `with_config` lint-code rejection must warn through
    /// `compile_story_inline`'s call too, not just the asset-loader path
    /// `plugin_override_unknown_and_non_overridable_lint_codes_warn_but_valid_entry_still_applies`
    /// already covers. That test (and the `BrinkConfigWarnings`-resource
    /// tests above) only ever drove the rejection through
    /// `AssetServer::load` -> `InkLoader::load` -> `Project::load`;
    /// `compile_story_inline`'s own `Project::load` call
    /// (`source_loader.rs`, `compile_story_inline`) had no coverage proving
    /// it reaches the same `tracing::warn!` channel.
    #[test]
    fn compile_story_inline_invalid_lint_code_warns_via_tracing() {
        let captured = captured_warnings();

        let mut lints = std::collections::BTreeMap::new();
        lints.insert(
            "E9998_INLINE_TYPO".to_owned(),
            brink_project_config::LintLevel::Deny,
        );
        let (mut app, _dir) = make_memory_asset_app_with_config(ProjectConfig {
            lints,
            ..ProjectConfig::default()
        });

        // The rejected code is never applied (it's invalid, not merged),
        // so this trivial, diagnostic-free source still compiles cleanly --
        // the point is to observe the warning, not a failed compile.
        crate::compile_story_inline(&mut app, "intro.ink", "-> END\n")
            .expect("E9998_INLINE_TYPO is rejected, not applied, so nothing blocks the compile");

        let joined = captured.lock().unwrap().join("\n");
        assert!(
            joined.contains("[lints] `E9998_INLINE_TYPO` is not a recognized diagnostic code"),
            "an invalid with_config lint code must warn through \
             compile_story_inline's own Project::load call too, not just \
             InkLoader's; captured: {joined}"
        );
    }

    /// Issue #1382 sweep finding: a *second* `BrinkPlugin<M>` registration's
    /// `with_config` override used to vanish with no trace at all once
    /// `BrinkAssetsPlugin` already existed (added by an earlier marker's
    /// plugin) — `BrinkPlugin::with_config`'s own doc comment already
    /// documented the precedence rule ("only the plugin that ends up adding
    /// `BrinkAssetsPlugin` applies its config"), but neither the
    /// `tracing::warn!` channel nor [`BrinkConfigWarnings`] ever recorded
    /// that a *later* marker's whole `ProjectConfig` was the one that lost —
    /// exactly the silent-drop pattern this issue swept for, just under a
    /// different name than `resolve_*_options`. Two distinct marker types
    /// reproduce a real multi-story app rather than a synthetic double
    /// registration: `MarkerA`'s plugin (no override) is the one that adds
    /// `BrinkAssetsPlugin`, so `MarkerB`'s `with_config` has nothing left to
    /// land in.
    #[test]
    fn second_marker_with_config_drop_is_diagnosed_not_silent() {
        struct MarkerA;
        struct MarkerB;

        let captured = captured_warnings();

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
            // `MarkerA`'s plugin has no override and is the one that ends up
            // adding `BrinkAssetsPlugin` (first in registration order).
            crate::BrinkPlugin::<MarkerA>::default(),
            // `MarkerB`'s override arrives after `BrinkAssetsPlugin` already
            // exists, so it must be diagnosed rather than silently dropped.
            crate::BrinkPlugin::<MarkerB>::default().with_config(ProjectConfig {
                dialect: Some(Dialect::Brink),
                ..ProjectConfig::default()
            }),
        ));

        let warnings = app.world().resource::<crate::BrinkConfigWarnings>();
        assert!(
            warnings
                .0
                .iter()
                .any(|w| w.contains("MarkerB") && w.contains("ignored")),
            "a second marker's dropped `with_config` override must be recorded \
             in `BrinkConfigWarnings`, not silently discarded; got: {warnings:?}"
        );

        let joined = captured.lock().unwrap().join("\n");
        assert!(
            joined.contains("MarkerB") && joined.contains("ignored"),
            "the same drop must also reach the tracing::warn! channel (the \
             'warn, never silently drop' rule every other mount's config \
             resolution already follows); captured: {joined}"
        );

        // Prove the drop, not just its announcement (house rule 19t): the
        // shared `InkLoader` this app ends up with must still be running
        // under the strict-ink default, not `MarkerB`'s `dialect = brink`
        // override -- if a future change ever let a later marker's config
        // reach `InkLoader` after all, this would start failing (green,
        // while the warning above kept firing as a lie) unless it's pinned
        // down here too.
        dir.insert_asset_text(Path::new("intro.ink"), BRINK_ONLY_SOURCE);
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("intro.ink");
        wait_for_failed(&mut app, &handle);
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

    /// #1360 regression: the migrated loader still walks a multi-file
    /// `INCLUDE` graph correctly through the real `AssetServer` /
    /// `MemoryAssetReader`, including a parent-traversing (`../`) include
    /// that exercises [`super::resolve_include_path`]'s `..`-segment
    /// normalization, alongside a same-directory include. Before #1360 this
    /// path went through `brink_compiler::compile_with_options`'s read
    /// closure; it now goes through `brink_source_tree::InMemory` ->
    /// `brink_environment::Driver::discover`, so a producer-side change to
    /// include-graph keying could silently break multi-file loads without
    /// this test catching it.
    #[test]
    fn multi_file_include_graph_loads_through_asset_server() {
        let (mut app, dir) = make_memory_asset_app();
        dir.insert_asset_text(
            Path::new("stories/ch1/intro.ink"),
            "INCLUDE ../shared/util.ink\nINCLUDE local.ink\n-> END\n",
        );
        // Parent traversal: "../shared/util.ink" from "stories/ch1/" must
        // resolve to "stories/shared/util.ink", not "stories/ch1/../shared/util.ink".
        dir.insert_asset_text(Path::new("stories/shared/util.ink"), "VAR shared_var = 1\n");
        // Same-directory include, resolved relative to the entry's own dir.
        dir.insert_asset_text(Path::new("stories/ch1/local.ink"), "VAR local_var = 2\n");

        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<BrinkStoryAsset>("stories/ch1/intro.ink");
        wait_for_loaded(&mut app, &handle);
    }
}
