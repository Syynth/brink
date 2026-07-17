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

use bevy_asset::{AssetLoader, LoadContext, io::Reader};
use bevy_reflect::TypePath;

use crate::asset::{BrinkStoryAsset, emit_story_assets};

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
}
