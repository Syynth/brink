//! `brink ide` — scriptable brink-ide queries (epic #289).
//!
//! Phase 1: the project loader + name/`--at` addressing + output framework, plus
//! the two headline read-queries `def` and `references`. The CLI drives the same
//! `brink-ide` engine the LSP and studio use, via a `brink_driver::Driver` that
//! discovers the project from an entry `.ink` (following `INCLUDE`s) — identical
//! to `brink compile`. See `docs/cli-ide-inventory.md`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use brink_analyzer::AnalysisResult;
use brink_driver::Driver;
use brink_ide::LineIndex;
use brink_ide::navigation::find_references;
use brink_ir::FileId;
use brink_ir::symbols::{SymbolInfo, SymbolKind};
use clap::{Args, Subcommand, ValueEnum};
use rowan::TextRange;

// ── CLI surface ─────────────────────────────────────────────────────

/// Scriptable IDE queries over an ink project (epic #289).
///
/// Address a symbol by its qualified name (`intro`, `intro.evidence`,
/// `Colors.Red`) — the same dotted paths ink uses. Output is human-readable by
/// default, or `--format json` for scripting. Exit codes: 0 ok, 1 query-false,
/// 2 usage error.
#[derive(Subcommand)]
pub enum IdeCommand {
    /// Print where a symbol is defined.
    #[command(after_help = "\
Examples:
  brink ide def intro --entry main.ink
  brink ide def intro.evidence -e main.ink --format json
  brink ide def gold -e main.ink --kind variable")]
    Def {
        /// Qualified symbol name (knot / knot.stitch / List.Item / var / …).
        symbol: String,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// List the references to a symbol across the project.
    #[command(after_help = "\
Examples:
  brink ide references gold -e main.ink
  brink ide references intro --exists -e main.ink   # exit 0 if used, 1 if not
  brink ide references gold --count -e main.ink --format json")]
    References {
        /// Qualified symbol name.
        symbol: String,
        /// Include the declaration site in the results.
        #[arg(long)]
        include_decl: bool,
        /// Print nothing; exit 0 if the symbol is referenced, 1 if not.
        #[arg(long)]
        exists: bool,
        /// Print only the number of references.
        #[arg(long)]
        count: bool,
        #[command(flatten)]
        opts: CommonOpts,
    },
}

/// Options shared by every `brink ide` query.
#[derive(Args)]
pub struct CommonOpts {
    /// Entry-point `.ink` file; `INCLUDE`s are followed to build the project.
    #[arg(long, short = 'e', value_name = "FILE")]
    entry: PathBuf,
    /// Disambiguate when a name matches multiple symbol kinds.
    #[arg(long, value_enum, value_name = "KIND")]
    kind: Option<KindFilter>,
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

/// The symbol kinds addressable from the CLI (mirrors `SymbolKind`).
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum KindFilter {
    Knot,
    Stitch,
    Variable,
    Constant,
    List,
    ListItem,
    External,
    Label,
    Param,
    Temp,
}

impl KindFilter {
    fn matches(self, k: SymbolKind) -> bool {
        matches!(
            (self, k),
            (Self::Knot, SymbolKind::Knot)
                | (Self::Stitch, SymbolKind::Stitch)
                | (Self::Variable, SymbolKind::Variable)
                | (Self::Constant, SymbolKind::Constant)
                | (Self::List, SymbolKind::List)
                | (Self::ListItem, SymbolKind::ListItem)
                | (Self::External, SymbolKind::External)
                | (Self::Label, SymbolKind::Label)
                | (Self::Param, SymbolKind::Param)
                | (Self::Temp, SymbolKind::Temp)
        )
    }
}

fn kind_name(k: SymbolKind) -> &'static str {
    match k {
        SymbolKind::Knot => "knot",
        SymbolKind::Stitch => "stitch",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::List => "list",
        SymbolKind::ListItem => "list-item",
        SymbolKind::External => "external",
        SymbolKind::Label => "label",
        SymbolKind::Param => "param",
        SymbolKind::Temp => "temp",
    }
}

// ── Entry point ─────────────────────────────────────────────────────

/// Run a `brink ide` subcommand. Returns the process exit code.
pub fn run(cmd: &IdeCommand) -> ExitCode {
    let result = match cmd {
        IdeCommand::Def { symbol, opts } => run_def(symbol, opts),
        IdeCommand::References {
            symbol,
            include_decl,
            exists,
            count,
            opts,
        } => run_references(symbol, *include_decl, *exists, *count, opts),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            let _ = writeln!(io::stderr(), "error: {msg}");
            ExitCode::from(2)
        }
    }
}

// ── Commands ────────────────────────────────────────────────────────

fn run_def(symbol: &str, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve_unique(symbol, opts.kind)?;
    let loc = project.location_of(sym.file, sym.range);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Text => writeln!(out, "{} {}", kind_name(sym.kind), loc.display()),
        Format::Json => writeln!(
            out,
            "{}",
            serde_json::json!({ "name": sym.name, "kind": kind_name(sym.kind), "location": loc })
        ),
    }
    .map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

fn run_references(
    symbol: &str,
    include_decl: bool,
    exists: bool,
    count: bool,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve_unique(symbol, opts.kind)?;
    // The definition offset is a valid query position: find_references resolves
    // the symbol there and collects every use (optionally including the decl).
    let refs = find_references(&project.analysis, sym.file, sym.range.start(), include_decl);

    if exists {
        return Ok(if refs.is_empty() {
            ExitCode::from(1)
        } else {
            ExitCode::SUCCESS
        });
    }

    let locs: Vec<Loc> = refs
        .iter()
        .map(|r| project.location_of(r.file, r.range))
        .collect();

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Text => {
            if count {
                writeln!(out, "{}", locs.len()).map_err(|e| e.to_string())?;
            } else {
                for loc in &locs {
                    writeln!(out, "{}", loc.display()).map_err(|e| e.to_string())?;
                }
            }
        }
        Format::Json => writeln!(
            out,
            "{}",
            serde_json::json!({
                "name": sym.name,
                "kind": kind_name(sym.kind),
                "count": locs.len(),
                "references": locs,
            })
        )
        .map_err(|e| e.to_string())?,
    }
    Ok(ExitCode::SUCCESS)
}

// ── Project loader ──────────────────────────────────────────────────

struct Project {
    driver: Driver,
    analysis: AnalysisResult,
}

impl Project {
    /// Discover + analyze the project rooted at `entry` (follows `INCLUDE`s),
    /// exactly like `brink compile`.
    fn load(entry: &Path) -> Result<Self, String> {
        let entry = entry.to_string_lossy().into_owned();
        let mut driver = Driver::new();
        driver
            .discover(&entry, |p| {
                std::fs::read_to_string(p)
                    .map_err(|e| io::Error::new(e.kind(), format!("{p}: {e}")))
            })
            .map_err(|e| format!("{e}"))?;
        let analysis = driver.analyze().clone();
        driver
            .db()
            .file_id(&entry)
            .ok_or_else(|| format!("entry file not found after discovery: {entry}"))?;
        Ok(Self { driver, analysis })
    }

    /// Resolve a qualified name to exactly one symbol, honoring `--kind`.
    fn resolve_unique(&self, name: &str, kind: Option<KindFilter>) -> Result<&SymbolInfo, String> {
        let ids = self.analysis.index.by_name.get(name);
        let mut hits: Vec<&SymbolInfo> = ids
            .into_iter()
            .flatten()
            .filter_map(|id| self.analysis.index.symbols.get(id))
            .filter(|s| kind.is_none_or(|k| k.matches(s.kind)))
            .collect();
        let db = self.driver.db();
        hits.sort_by_key(|s| {
            (
                db.file_path(s.file).unwrap_or_default().to_string(),
                s.range.start(),
            )
        });

        match hits.as_slice() {
            [] => Err(format!("no symbol named '{name}' in the project")),
            [one] => Ok(one),
            many => {
                let kinds: Vec<&str> = many.iter().map(|s| kind_name(s.kind)).collect();
                Err(format!(
                    "'{name}' is ambiguous (matches: {}); disambiguate with --kind",
                    kinds.join(", ")
                ))
            }
        }
    }

    fn location_of(&self, file: FileId, range: TextRange) -> Loc {
        let db = self.driver.db();
        let path = db.file_path(file).unwrap_or_default().to_string();
        let src = db.source(file).unwrap_or_default();
        let idx = LineIndex::new(src);
        let (line, col) = idx.line_col(range.start());
        Loc {
            path,
            line: line + 1,
            col: col + 1,
            byte_start: u32::from(range.start()),
            byte_end: u32::from(range.end()),
        }
    }
}

// ── Output location ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct Loc {
    path: String,
    line: u32,
    col: u32,
    byte_start: u32,
    byte_end: u32,
}

impl Loc {
    fn display(&self) -> String {
        format!("{}:{}:{}", self.path, self.line, self.col)
    }
}
