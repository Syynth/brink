//! The compiled program, for the Program Explorer: the structured model
//! (`brink_ide::program_model`), the lines table and the size report — one
//! compile, three readings of it, all plain data.
//!
//! Compile-bound, not session-bound (`docs/studio-shell-spec.md` §4): it
//! exists the moment a compile lands, with or without a running story.
//! The compile is the same memoized `IdeSession::compile` the play session
//! uses, under the same entry rule.

use brink_ide::program_model::ProgramModel;
use brink_ide::session::IdeSession;
use brink_intl::LinesJson;
use serde::Deserialize;

/// What the Program Explorer shows.
#[derive(Debug, Clone)]
pub struct ProgramReport {
    /// The file compiled from, once one was found.
    pub entry: Option<String>,
    pub status: ProgramStatus,
}

#[derive(Debug, Clone)]
pub enum ProgramStatus {
    /// Nothing names the story's start (see `play::entry_file`).
    NoEntry,
    /// The project has errors, so there is no program. `code: message`
    /// each; Problems has the positions.
    Errors(Vec<String>),
    Ready(Box<Program>),
}

/// One compile, three readings.
#[derive(Debug, Clone)]
pub struct Program {
    pub model: ProgramModel,
    pub lines: LinesJson,
    pub size: SizeReport,
}

/// `brink_ide::size_report::size_report_of`'s JSON, typed. Real on-disk
/// bytes from the file's own offset table.
#[derive(Debug, Clone, Deserialize)]
pub struct SizeReport {
    pub total: usize,
    /// An exact re-serialization without the `DebugInfo` section — what a
    /// release export produces.
    pub shipping: usize,
    pub debug: usize,
    pub header: usize,
    pub sections: Vec<SizeSection>,
    pub line_scopes: Vec<SizeScope>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SizeSection {
    pub kind: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SizeScope {
    /// `None` for the root scope.
    pub name: Option<String>,
    pub bytes: usize,
}

/// Compile and read the program.
pub fn report(session: &mut IdeSession, entry: Option<&str>, files: &[String]) -> ProgramReport {
    let Some(entry) = crate::play::entry_file(entry, files) else {
        return ProgramReport {
            entry: None,
            status: ProgramStatus::NoEntry,
        };
    };
    let entry_name = Some(entry.to_owned());
    let options = session.db().analysis_options().clone();
    let product = match session.compile(entry, &options) {
        Ok(product) => product,
        Err(e) => {
            return ProgramReport {
                entry: entry_name,
                status: ProgramStatus::Errors(vec![e.to_string()]),
            };
        }
    };
    if !product.errors.is_empty() {
        return ProgramReport {
            entry: entry_name,
            status: ProgramStatus::Errors(
                product
                    .errors
                    .iter()
                    .map(|d| format!("{}: {}", d.code.as_str(), d.message))
                    .collect(),
            ),
        };
    }
    let Some(data) = product.story else {
        return ProgramReport {
            entry: entry_name,
            status: ProgramStatus::Errors(vec!["the compiler produced no story".to_owned()]),
        };
    };
    let model = brink_ide::program_model::build(&data);
    let lines = brink_intl::export_lines(&data, data.source_checksum);
    // The size report measures the file, so the file is written.
    let mut bytes = Vec::new();
    brink_format::write_inkb(&data, &mut bytes);
    let size = brink_ide::size_report::size_report_of(&bytes)
        .and_then(|value| serde_json::from_value::<SizeReport>(value).map_err(|e| e.to_string()));
    let size = match size {
        Ok(size) => size,
        Err(e) => {
            return ProgramReport {
                entry: entry_name,
                status: ProgramStatus::Errors(vec![format!("size report: {e}")]),
            };
        }
    };
    ProgramReport {
        entry: entry_name,
        status: ProgramStatus::Ready(Box::new(Program { model, lines, size })),
    }
}
