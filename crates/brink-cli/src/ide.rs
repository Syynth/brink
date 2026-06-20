//! `brink ide` — scriptable brink-ide queries (epic #289).
//!
//! The project loader + name/`--at` addressing + output framework, plus the
//! read-queries `def`, `references`, `symbols`, `unused`, and `check`. The CLI
//! drives the same `brink-ide` engine the LSP and studio use, via a
//! `brink_driver::Driver` that discovers the project from an entry `.ink`
//! (following `INCLUDE`s) — identical to `brink compile`. Mutating refactors
//! (`rename`, …) land in a later slice. See `docs/cli-ide-inventory.md`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use brink_analyzer::AnalysisResult;
use brink_driver::Driver;
use brink_ide::LineIndex;
use brink_ide::document::{DocumentSymbol, document_symbols, workspace_symbols};
use brink_ide::navigation::{find_def_at_offset, find_references};
use brink_ir::symbols::{SymbolInfo, SymbolKind};
use brink_ir::{Diagnostic, FileId};
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
  brink ide def --at main.ink:7:5 -e main.ink   # the symbol under that cursor")]
    Def {
        #[command(flatten)]
        addr: Address,
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
        #[command(flatten)]
        addr: Address,
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
    /// List a file's outline, or search symbols across the project.
    #[command(after_help = "\
Examples:
  brink ide symbols -e main.ink                  # outline of the entry file
  brink ide symbols --file scenes/intro.ink -e main.ink
  brink ide symbols --search gold -e main.ink    # project-wide name search
  brink ide symbols --kind knot -e main.ink")]
    Symbols {
        /// Outline this file instead of the entry file.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        /// Project-wide substring search by name instead of an outline.
        #[arg(long, value_name = "QUERY")]
        search: Option<String>,
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// List declared symbols that have no references (dead-code lint).
    ///
    /// Exit 1 if any are found. Note: this is reference-based, not reachability
    /// — an entry knot reached implicitly (no `->`) can show up here.
    #[command(after_help = "\
Examples:
  brink ide unused -e main.ink
  brink ide unused --kind variable -e main.ink
  brink ide unused -e main.ink --format json")]
    Unused {
        #[command(flatten)]
        opts: CommonOpts,
    },
    /// Report project diagnostics (exit 1 if any error).
    #[command(after_help = "\
Examples:
  brink ide check -e main.ink
  brink ide check -e main.ink --format json")]
    Check {
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

/// How a query addresses its target — by qualified name or by cursor position.
#[derive(Args)]
pub struct Address {
    /// Qualified symbol name (knot / knot.stitch / List.Item / var / …).
    symbol: Option<String>,
    /// Address by cursor position instead: `FILE:LINE:COL` (1-based line & column).
    #[arg(long, value_name = "FILE:LINE:COL", conflicts_with = "symbol")]
    at: Option<String>,
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
        IdeCommand::Def { addr, opts } => run_def(addr, opts),
        IdeCommand::References {
            addr,
            include_decl,
            exists,
            count,
            opts,
        } => run_references(addr, *include_decl, *exists, *count, opts),
        IdeCommand::Symbols { file, search, opts } => {
            run_symbols(file.as_deref(), search.as_deref(), opts)
        }
        IdeCommand::Unused { opts } => run_unused(opts),
        IdeCommand::Check { opts } => run_check(opts),
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

fn run_def(addr: &Address, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve(addr, opts.kind)?;
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
    addr: &Address,
    include_decl: bool,
    exists: bool,
    count: bool,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let sym = project.resolve(addr, opts.kind)?;
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

fn run_symbols(
    file: Option<&str>,
    search: Option<&str>,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let mut out = io::stdout().lock();

    let entries: Vec<SymEntry> = if let Some(query) = search {
        // Project-wide name search (flat list).
        workspace_symbols(std::iter::once(&project.analysis), query)
            .into_iter()
            .filter(|s| opts.kind.is_none_or(|k| k.matches(s.kind)))
            .map(|s| SymEntry {
                name: s.name,
                kind: kind_name(s.kind).into(),
                detail: None,
                location: project.location_of(s.file, s.range),
                children: Vec::new(),
            })
            .collect()
    } else {
        // Outline of one file (default: the entry file). The full tree is kept;
        // `--kind` applies to search/unused, not the hierarchical outline.
        let db = project.driver.db();
        let file_id = match file {
            Some(f) => db
                .file_id(f)
                .ok_or_else(|| format!("file not in project: {f}"))?,
            None => project.entry_id,
        };
        let hir = db.hir(file_id).ok_or("no HIR for that file")?;
        let manifest = db.manifest(file_id).ok_or("no manifest for that file")?;
        let source = db.source(file_id).unwrap_or_default();
        document_symbols(hir, manifest, source)
            .iter()
            .map(|d| doc_to_entry(&project, file_id, d))
            .collect()
    };

    match opts.format {
        Format::Json => {
            writeln!(out, "{}", to_json(&entries)?).map_err(|e| e.to_string())?;
        }
        Format::Text => print_tree(&mut out, &entries, 0)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_unused(opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let mut unused: Vec<SymEntry> = project
        .analysis
        .index
        .symbols
        .values()
        .filter(|info| opts.kind.is_none_or(|k| k.matches(info.kind)))
        .filter(|info| {
            find_references(&project.analysis, info.file, info.range.start(), false).is_empty()
        })
        .map(|info| SymEntry {
            name: info.name.clone(),
            kind: kind_name(info.kind).into(),
            detail: None,
            location: project.location_of(info.file, info.range),
            children: Vec::new(),
        })
        .collect();
    unused.sort_by(|a, b| {
        (&a.location.path, a.location.byte_start).cmp(&(&b.location.path, b.location.byte_start))
    });

    let any = !unused.is_empty();
    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => writeln!(out, "{}", to_json(&unused)?).map_err(|e| e.to_string())?,
        Format::Text => {
            for e in &unused {
                writeln!(out, "{} {} {}", e.kind, e.name, e.location.display())
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(if any {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn run_check(opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry)?;
    let report = project
        .driver
        .collect_diagnostics(&project.analysis, Some(project.entry_id));

    let mut diags: Vec<DiagEntry> = report
        .errors
        .iter()
        .map(|d| project.diag_entry(d, "error"))
        .chain(
            report
                .warnings
                .iter()
                .map(|d| project.diag_entry(d, "warning")),
        )
        .collect();
    diags.sort_by(|a, b| {
        (&a.location.path, a.location.byte_start).cmp(&(&b.location.path, b.location.byte_start))
    });

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => writeln!(out, "{}", to_json(&diags)?).map_err(|e| e.to_string())?,
        Format::Text => {
            for d in &diags {
                writeln!(
                    out,
                    "{}[{}] {} {}",
                    d.severity,
                    d.code,
                    d.location.display(),
                    d.message
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(if report.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

// ── Project loader ──────────────────────────────────────────────────

struct Project {
    driver: Driver,
    analysis: AnalysisResult,
    entry_id: FileId,
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
        let entry_id = driver
            .db()
            .file_id(&entry)
            .ok_or_else(|| format!("entry file not found after discovery: {entry}"))?;
        Ok(Self {
            driver,
            analysis,
            entry_id,
        })
    }

    /// Resolve a query's target to a single symbol — by `--at FILE:LINE:COL`
    /// (cursor → the symbol there, resolving a reference to its definition) or
    /// by qualified name.
    fn resolve(&self, addr: &Address, kind: Option<KindFilter>) -> Result<&SymbolInfo, String> {
        if let Some(at) = &addr.at {
            let (file, line, col) = parse_at(at)?;
            let db = self.driver.db();
            let file_id = db
                .file_id(&file)
                .ok_or_else(|| format!("file not in project: {file}"))?;
            let src = db.source(file_id).unwrap_or_default();
            // `--at` is 1-based; LineIndex (like line_col) is 0-based.
            let offset = LineIndex::new(src).offset(line.saturating_sub(1), col.saturating_sub(1));
            find_def_at_offset(&self.analysis, file_id, offset)
                .ok_or_else(|| format!("no symbol at {at}"))
        } else if let Some(name) = &addr.symbol {
            self.resolve_unique(name, kind)
        } else {
            Err("provide a symbol name or --at FILE:LINE:COL".to_string())
        }
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

    fn diag_entry(&self, d: &Diagnostic, severity: &str) -> DiagEntry {
        DiagEntry {
            severity: severity.to_string(),
            code: d.code.as_str().to_string(),
            message: d.message.clone(),
            location: self.location_of(d.file, d.range),
        }
    }
}

/// Recursively convert a `brink-ide` outline node into an output entry.
fn doc_to_entry(project: &Project, file: FileId, d: &DocumentSymbol) -> SymEntry {
    SymEntry {
        name: d.name.clone(),
        kind: kind_name(d.kind).into(),
        detail: d.detail.clone(),
        location: project.location_of(file, d.range),
        children: d
            .children
            .iter()
            .map(|c| doc_to_entry(project, file, c))
            .collect(),
    }
}

fn print_tree(out: &mut impl Write, entries: &[SymEntry], depth: usize) -> Result<(), String> {
    for e in entries {
        let indent = "  ".repeat(depth);
        let detail = e
            .detail
            .as_deref()
            .map(|d| format!(" [{d}]"))
            .unwrap_or_default();
        writeln!(
            out,
            "{indent}{} {}{}  {}",
            e.kind,
            e.name,
            detail,
            e.location.display()
        )
        .map_err(|x| x.to_string())?;
        print_tree(out, &e.children, depth + 1)?;
    }
    Ok(())
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, String> {
    serde_json::to_string(v).map_err(|e| e.to_string())
}

/// Parse `FILE:LINE:COL` (line/col 1-based). The file may itself contain `:`,
/// so split the two numeric fields off the right.
fn parse_at(s: &str) -> Result<(String, u32, u32), String> {
    let mut parts = s.rsplitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(col), Some(line), Some(file)) if !file.is_empty() => {
            let line = line
                .parse::<u32>()
                .map_err(|_| format!("bad line in --at '{s}'"))?;
            let col = col
                .parse::<u32>()
                .map_err(|_| format!("bad column in --at '{s}'"))?;
            Ok((file.to_string(), line, col))
        }
        _ => Err(format!("--at must be FILE:LINE:COL, got '{s}'")),
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

/// An outline / search / unused entry (a symbol with an optional child list).
#[derive(serde::Serialize)]
struct SymEntry {
    name: String,
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    location: Loc,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<SymEntry>,
}

/// A diagnostic entry for `brink ide check`.
#[derive(serde::Serialize)]
struct DiagEntry {
    severity: String,
    code: String,
    message: String,
    location: Loc,
}
