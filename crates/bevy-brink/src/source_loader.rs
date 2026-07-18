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

use crate::asset::{
    BrinkStoryAsset, LineTablesAsset, ProgramAsset, emit_story_assets, fresh_context,
};

/// Asset loader for `.ink` (source) files.
///
/// Reads the entry source, walks the `INCLUDE` graph asynchronously
/// through Bevy's `AssetReader`, compiles via [`brink_compiler::compile`],
/// links via [`brink_runtime::link`], and emits labeled subassets
/// (`#program`, `#line_tables`) just like [`InkbLoader`](crate::InkbLoader).
#[derive(Default, TypePath)]
pub struct InkLoader;

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

        // Compile from the cached sources (synchronous; closure reads
        // from the HashMap).
        let output = brink_compiler::compile(&entry_path, |p| {
            sources.get(p).cloned().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("{p}: not in pre-fetched source cache"),
                )
            })
        })?;
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
}
