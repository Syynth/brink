//! The compiled program as text — the `.inkt` dump behind Compiled Output
//! (`docs/studio-shell-spec.md` §4, `CompiledOutputDocument.tsx`).
//!
//! Compile-bound like [`crate::program`], and off the same memoized
//! `IdeSession::compile`: a project that has not changed since the Program
//! Explorer or the Player compiled it costs a `write_inkt` and nothing
//! else. The dump is produced here, on the worker, because it is the
//! compiler's own text format and the main thread never holds a
//! `StoryData`.
//!
//! `.inkt` is the compiler's textual interface, not a pretty-printer: the
//! same bytes `brink compile --emit inkt` writes, so what this shows and
//! what the intl pipeline reads are one artifact.

use brink_ide::session::IdeSession;

/// What Compiled Output shows.
#[derive(Debug, Clone)]
pub struct CompiledOutput {
    /// The file compiled from, once one was found.
    pub entry: Option<String>,
    pub status: CompiledStatus,
}

#[derive(Debug, Clone)]
pub enum CompiledStatus {
    /// Nothing names the story's start (see [`crate::play::entry_file`]).
    NoEntry,
    /// The project has errors, so there is no program. `code: message`
    /// each; Problems has the positions.
    Errors(Vec<String>),
    /// The `.inkt` text, and the byte count of the `.inkb` it came from —
    /// the dump is a reading of that file, so its size is worth saying.
    Ready { text: String, bytes: usize },
}

/// Compile `entry` and write its `.inkt` dump.
pub fn output(session: &mut IdeSession, entry: Option<&str>, files: &[String]) -> CompiledOutput {
    let Some(entry) = crate::play::entry_file(entry, files) else {
        return CompiledOutput {
            entry: None,
            status: CompiledStatus::NoEntry,
        };
    };
    let entry_name = Some(entry.to_owned());
    let errors = |messages: Vec<String>| CompiledOutput {
        entry: entry_name.clone(),
        status: CompiledStatus::Errors(messages),
    };
    let options = session.db().analysis_options().clone();
    let product = match session.compile(entry, &options) {
        Ok(product) => product,
        Err(e) => return errors(vec![e.to_string()]),
    };
    if !product.errors.is_empty() {
        return errors(
            product
                .errors
                .iter()
                .map(|d| format!("{}: {}", d.code.as_str(), d.message))
                .collect(),
        );
    }
    let Some(data) = product.story else {
        return errors(vec!["the compiler produced no story".to_owned()]);
    };
    let mut text = String::new();
    if let Err(e) = brink_format::write_inkt(&data, &mut text) {
        return errors(vec![format!("writing the .inkt dump: {e}")]);
    }
    let mut bytes = Vec::new();
    brink_format::write_inkb(&data, &mut bytes);
    CompiledOutput {
        entry: entry_name,
        status: CompiledStatus::Ready {
            text,
            bytes: bytes.len(),
        },
    }
}
