//! The per-command `run_xxx` handlers for `brink ide`, plus the shared
//! mutation pipeline (preview / `--patch` / `--write`, safe-by-default
//! against newly-introduced diagnostics) that `rename`, `move-file`, and
//! `refactor *` all route through, and the story-graph / effects-diff
//! rendering helpers those handlers use.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use brink_ide::LineIndex;
use brink_ide::code_actions::{CodeActionKind, code_actions};
use brink_ide::document::{document_symbols, workspace_symbols};
use brink_ide::effects::EffectRowView;
use brink_ide::file_rename::rename_file;
use brink_ide::formatting::{format_region, sort_knots_in_source, sort_stitches_in_knot};
use brink_ide::hover::hover;
use brink_ide::line_context::line_contexts;
use brink_ide::line_convert::convert_element;
use brink_ide::navigation::find_references;
use brink_ide::rename::{FileEdit, rename};
use brink_ide::signature::signature_help;
use brink_ide::story_graph::{StoryEdgeKind, StoryGraph, StoryNodeKind, story_graph};
use brink_ide::structural_move::{
    demote_knot_to_stitch, move_stitch, promote_stitch_to_knot, reorder_knot, reorder_knots,
    reorder_stitch, reorder_stitches,
};
use brink_ir::{FileId, HirFile};

use super::commands::{
    Address, CommonOpts, ConvertTo, EffectsDiffOpts, Format, MutOpts, RefactorOp, kind_name,
};
use super::project::{
    DiagEntry, EditEntry, Loc, Project, SymEntry, doc_to_entry, file_diff, load_git_baseline,
    parse_at, print_tree, push_diff_line, resolve_fs_path, to_json,
};

// ── Commands ────────────────────────────────────────────────────────

pub(super) fn run_def(addr: &Address, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
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

pub(super) fn run_references(
    addr: &Address,
    include_decl: bool,
    exists: bool,
    count: bool,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let sym = project.resolve(addr, opts.kind)?;
    // The definition offset is a valid query position: find_references resolves
    // the symbol there and collects every use (optionally including the decl).
    let refs = find_references(
        project.driver.db(),
        &project.analysis,
        sym.file,
        sym.range.start(),
        include_decl,
    );

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

pub(super) fn run_symbols(
    file: Option<&str>,
    search: Option<&str>,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
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

pub(super) fn run_unused(opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let db = project.driver.db();
    let mut unused: Vec<SymEntry> = project
        .analysis
        .index
        .symbols
        .values()
        .filter(|info| opts.kind.is_none_or(|k| k.matches(info.kind)))
        .filter(|info| {
            find_references(db, &project.analysis, info.file, info.range.start(), false).is_empty()
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

pub(super) fn run_check(opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let report = project
        .driver
        .collect_diagnostics(&project.analysis, Some(project.entry_id));

    // `diag_entry` resolves each diagnostic's actual severity via
    // `effective_severity` rather than trusting which of `report`'s two
    // buckets it came from (issue #1616) — `report.errors`/`report.warnings`
    // is a binary partition, so a `[lints]` code down-leveled to `Info`/
    // `Hint` still lands in `warnings` and must not render as `"warning"`.
    let mut diags: Vec<DiagEntry> = report
        .errors
        .iter()
        .chain(report.warnings.iter())
        // `filter_map`: a `[lints] allow` code yields no entry at all
        // (#3173).
        .filter_map(|d| project.diag_entry(d))
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

/// What a mutation does with its computed edits.
#[derive(Clone, Copy)]
pub(super) enum Mode<'a> {
    Preview,
    /// Emit a `git apply`-able patch — to stdout (`"-"`) or to a file path.
    Patch(&'a str),
    Write,
}

pub(super) fn run_rename(
    addr: &Address,
    new_name: &str,
    patch: Option<&str>,
    write: bool,
    unsafe_mode: bool,
    opts: &CommonOpts,
) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let sym = project.resolve(addr, opts.kind)?;
    let result = rename(
        project.driver.db(),
        &project.analysis,
        sym.file,
        sym.range.start(),
        new_name,
    )
    .ok_or_else(|| {
        format!(
            "'{}' cannot be renamed (a built-in or unresolved symbol)",
            sym.name
        )
    })?;
    if result.edits.is_empty() {
        return Err("rename produced no edits".to_string());
    }

    // Apply the edits in-memory; `emit_mutation` re-analyzes to gate on any new
    // diagnostic. Rename carries its fine-grained edits for a per-edit preview.
    let edited = project.apply_edits(&result.edits)?;
    let mode = match (patch, write) {
        (Some(dest), _) => Mode::Patch(dest),
        (None, true) => Mode::Write,
        (None, false) => Mode::Preview,
    };
    let mutation = Mutation {
        edited,
        edits: Some(result.edits),
    };
    emit_mutation(
        &project,
        &opts.entry,
        &mutation,
        &mode,
        opts.format,
        unsafe_mode,
    )
}

// ── Mutation pipeline (rename / move-file / refactor *) ──────────────

/// A computed mutation ready to emit: the new full source for every file it
/// touches, plus optional fine-grained edits for a richer preview.
pub(super) struct Mutation {
    /// path → new full source, for every file the operation changes.
    pub(super) edited: BTreeMap<String, String>,
    /// Fine-grained edits (rename) for a per-edit preview; `None` → diff preview.
    pub(super) edits: Option<Vec<FileEdit>>,
}

/// Emit a mutation through the requested mode, applying the safe-by-default
/// diagnostic gate. `preview` always informs (prints edits + introduced
/// diagnostics, exit 0); `--patch`/`--write` refuse on any newly-introduced
/// diagnostic unless `unsafe_mode`. Returns the process exit code.
pub(super) fn emit_mutation(
    project: &Project,
    entry: &Path,
    mutation: &Mutation,
    mode: &Mode,
    format: Format,
    unsafe_mode: bool,
) -> Result<ExitCode, String> {
    let introduced = project.introduced_diagnostics(entry, &mutation.edited, None)?;

    if !matches!(mode, Mode::Preview) && !introduced.is_empty() && !unsafe_mode {
        let mut err = io::stderr().lock();
        writeln!(
            err,
            "refusing: change introduces {} new diagnostic(s) (re-run with --unsafe to override):",
            introduced.len()
        )
        .map_err(|e| e.to_string())?;
        for d in &introduced {
            writeln!(
                err,
                "  {}[{}] {} {}",
                d.severity,
                d.code,
                d.location.display(),
                d.message
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(ExitCode::from(1));
    }

    let mut out = io::stdout().lock();
    match mode {
        Mode::Preview => {
            if let Some(edits) = &mutation.edits {
                let entries = project.edit_entries(edits);
                emit_rename_preview(&mut out, format, &entries, &introduced)?;
            } else {
                emit_diff_preview(&mut out, project, &mutation.edited, format, &introduced)?;
            }
        }
        Mode::Patch(dest) => {
            let diff = project.unified_diff(&mutation.edited)?;
            if *dest == "-" {
                write!(out, "{diff}").map_err(|e| e.to_string())?;
            } else {
                std::fs::write(dest, diff).map_err(|e| format!("{dest}: {e}"))?;
            }
        }
        Mode::Write => {
            for (path, src) in &mutation.edited {
                let fs_path = resolve_fs_path(entry, path);
                std::fs::write(&fs_path, src).map_err(|e| format!("{}: {e}", fs_path.display()))?;
            }
            writeln!(out, "wrote {} file(s)", mutation.edited.len()).map_err(|e| e.to_string())?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Preview a whole-file mutation as a unified diff (text) or
/// `{ diff, files, introducedDiagnostics, safe }` (JSON), plus the diagnostics
/// it would introduce.
fn emit_diff_preview(
    out: &mut impl Write,
    project: &Project,
    edited: &BTreeMap<String, String>,
    format: Format,
    introduced: &[DiagEntry],
) -> Result<(), String> {
    let diff = project.unified_diff(edited)?;
    match format {
        Format::Json => {
            let files: Vec<&String> = edited.keys().collect();
            let v = serde_json::json!({
                "diff": diff,
                "files": files,
                "introducedDiagnostics": introduced,
                "safe": introduced.is_empty(),
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            write!(out, "{diff}").map_err(|e| e.to_string())?;
            if !introduced.is_empty() {
                writeln!(
                    out,
                    "would introduce {} new diagnostic(s):",
                    introduced.len()
                )
                .map_err(|e| e.to_string())?;
                for d in introduced {
                    writeln!(
                        out,
                        "  {}[{}] {} {}",
                        d.severity,
                        d.code,
                        d.location.display(),
                        d.message
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    Ok(())
}

/// Render a rename preview — the edits plus any diagnostics it would introduce.
fn emit_rename_preview(
    out: &mut impl Write,
    format: Format,
    entries: &[EditEntry],
    introduced: &[DiagEntry],
) -> Result<(), String> {
    match format {
        Format::Json => {
            let v = serde_json::json!({
                "edits": entries,
                "introducedDiagnostics": introduced,
                "safe": introduced.is_empty(),
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            for e in entries {
                writeln!(out, "{}  {} -> {}", e.location.display(), e.old, e.new)
                    .map_err(|x| x.to_string())?;
            }
            if !introduced.is_empty() {
                writeln!(
                    out,
                    "would introduce {} new diagnostic(s):",
                    introduced.len()
                )
                .map_err(|x| x.to_string())?;
                for d in introduced {
                    writeln!(
                        out,
                        "  {}[{}] {} {}",
                        d.severity,
                        d.code,
                        d.location.display(),
                        d.message
                    )
                    .map_err(|x| x.to_string())?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn run_hover(addr: &Address, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let sym = project.resolve(addr, opts.kind)?;
    let db = project.driver.db();
    let source = db.source(sym.file).unwrap_or_default();
    let ids: Vec<FileId> = db.file_ids().collect();
    let project_files: Vec<(FileId, String, String)> = ids
        .iter()
        .filter_map(|&id| {
            Some((
                id,
                db.file_path(id)?.to_string(),
                db.source(id)?.to_string(),
            ))
        })
        .collect();
    let info = hover(
        &project.analysis,
        db,
        sym.file,
        source,
        sym.range.start(),
        &project_files,
    )
    .ok_or("no hover information for that symbol")?;

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let v = serde_json::json!({
                "content": info.content,
                "location": project.location_of(sym.file, sym.range),
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => writeln!(out, "{}", info.content.trim_end()).map_err(|e| e.to_string())?,
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn run_signature(at: &str, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let (file, line, col) = parse_at(at)?;
    let db = project.driver.db();
    let file_id = db
        .file_id(&file)
        .ok_or_else(|| format!("file not in project: {file}"))?;
    let source = db.source(file_id).unwrap_or_default();
    let offset = LineIndex::new(source).offset(line.saturating_sub(1), col.saturating_sub(1));
    let sig = signature_help(&project.analysis, source, u32::from(offset) as usize)
        .ok_or("no call signature at that position")?;

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let params: Vec<&str> = sig.parameters.iter().map(|p| p.label.as_str()).collect();
            let v = serde_json::json!({
                "label": sig.label,
                "documentation": sig.documentation,
                "parameters": params,
                "activeParameter": sig.active_parameter,
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            writeln!(out, "{}", sig.label).map_err(|e| e.to_string())?;
            if let Some(doc) = &sig.documentation {
                writeln!(out, "{doc}").map_err(|e| e.to_string())?;
            }
            if let Some(p) = sig.parameters.get(sig.active_parameter as usize) {
                writeln!(out, "active parameter: {}", p.label).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn run_graph(dot: bool, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let db = project.driver.db();
    let ids: Vec<FileId> = db.file_ids().collect();
    let files: Vec<(FileId, &HirFile)> = ids
        .iter()
        .filter_map(|&id| db.hir(id).map(|h| (id, h)))
        .collect();
    let graph = story_graph(&project.analysis, &files);

    let mut out = io::stdout().lock();
    if dot {
        write_graph_dot(&mut out, &graph)?;
    } else {
        match opts.format {
            Format::Json => {
                writeln!(out, "{}", to_json(&graph_json(&graph))?).map_err(|e| e.to_string())?;
            }
            Format::Text => {
                for n in &graph.nodes {
                    writeln!(out, "{} {}", node_kind_name(n.kind), n.id)
                        .map_err(|e| e.to_string())?;
                }
                for e in &graph.edges {
                    writeln!(out, "{} --{}-> {}", e.from, edge_kind_name(e.kind), e.to)
                        .map_err(|x| x.to_string())?;
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn run_lines(file: Option<&str>, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let db = project.driver.db();
    let file_id = match file {
        Some(f) => db
            .file_id(f)
            .ok_or_else(|| format!("file not in project: {f}"))?,
        None => project.entry_id,
    };
    let hir = db.hir(file_id).ok_or("no HIR for that file")?;
    let source = db.source(file_id).unwrap_or_default();
    let root = db
        .parse(file_id)
        .ok_or("no parse tree for that file")?
        .syntax();
    let projection = brink_ide::hir_projection::project_hir_structural(hir, source);
    let ctxs = line_contexts(source, &root, &projection);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let arr: Vec<_> = ctxs
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    serde_json::json!({
                        "line": i + 1,
                        "element": format!("{:?}", c.element),
                        "depth": c.weave.depth,
                    })
                })
                .collect();
            writeln!(out, "{}", to_json(&arr)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            for (i, c) in ctxs.iter().enumerate() {
                writeln!(out, "{}: {:?} depth={}", i + 1, c.element, c.weave.depth)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ── Mutating commands: move-file / refactor / actions ───────────────

pub(super) fn run_move_file(old: &str, new: &str, mode: &MutOpts) -> Result<ExitCode, String> {
    let project = Project::load(&mode.entry, &mode.lints.resolve())?;
    let session = project.ide_session();
    let result = rename_file(&session, old, new).map_err(|e| e.to_string())?;

    // A file move changes the file *set*: the old path is removed and `new`
    // appears with `result.new_source`. Inbound `INCLUDE` rewrites land on other
    // files; the moved file itself is covered by `new_source`.
    let new_source = result
        .new_source
        .ok_or("file rename produced no primary source")?;
    let mut edited = project.apply_edits(&result.cross_file_edits)?;
    edited.remove(old);
    edited.insert(new.to_string(), new_source);

    // The destination is a brand-new path, so the whole-project re-analysis in
    // the safety gate must read it. `introduced_diagnostics` already overlays the
    // edited map onto the on-disk read closure, so `new` resolves to its content.
    let mutation = Mutation {
        edited,
        edits: None,
    };
    emit_move_mutation(&project, &mode.entry, old, new, &mutation, mode)
}

/// Emit a file move. Like `emit_mutation`, but the diff/write must account for
/// the path change (delete `old`, create `new`) rather than an in-place edit.
fn emit_move_mutation(
    project: &Project,
    entry: &Path,
    old: &str,
    new: &str,
    mutation: &Mutation,
    mode: &MutOpts,
) -> Result<ExitCode, String> {
    let m = mode.flags.mode();
    let introduced = project.introduced_diagnostics(entry, &mutation.edited, Some(old))?;

    if !matches!(m, Mode::Preview) && !introduced.is_empty() && !mode.flags.unsafe_mode {
        let mut err = io::stderr().lock();
        writeln!(
            err,
            "refusing: move introduces {} new diagnostic(s) (re-run with --unsafe to override):",
            introduced.len()
        )
        .map_err(|e| e.to_string())?;
        for d in &introduced {
            writeln!(
                err,
                "  {}[{}] {} {}",
                d.severity,
                d.code,
                d.location.display(),
                d.message
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(ExitCode::from(1));
    }

    // Build the diff: a rename hunk for old→new, plus in-place hunks for the
    // inbound-include files.
    let db = project.driver.db();
    let old_src = db
        .file_id(old)
        .and_then(|id| db.source(id))
        .unwrap_or_default();
    let new_src = mutation
        .edited
        .get(new)
        .map(String::as_str)
        .unwrap_or_default();

    let mut out = io::stdout().lock();

    if let Mode::Write = m {
        // `old`/`new` are project-relative keys (matching how `entry` is
        // spelled for native discovery), not necessarily cwd-relative fs
        // paths — resolve both against the project's source root before
        // touching disk (#1295).
        let old_fs = resolve_fs_path(entry, old);
        let new_fs = resolve_fs_path(entry, new);
        if let Some(parent) = new_fs.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::rename(&old_fs, &new_fs)
            .map_err(|e| format!("{} -> {}: {e}", old_fs.display(), new_fs.display()))?;
        // The moved file's new content (outbound INCLUDE rewrites) is in `edited`
        // under `new`; the rename above just relocated the old bytes.
        for (path, src) in &mutation.edited {
            let fs_path = resolve_fs_path(entry, path);
            std::fs::write(&fs_path, src).map_err(|e| format!("{}: {e}", fs_path.display()))?;
        }
        writeln!(
            out,
            "moved {old} -> {new} ({} file(s) updated)",
            mutation.edited.len()
        )
        .map_err(|e| e.to_string())?;
        return Ok(ExitCode::SUCCESS);
    }

    // Preview / Patch: build the diff (rename hunk + inbound-include hunks).
    let mut diff = String::new();
    rename_diff(&mut diff, old, new, old_src, new_src);
    for (path, src) in &mutation.edited {
        if path == new {
            continue;
        }
        let old = db
            .file_id(path)
            .and_then(|id| db.source(id))
            .unwrap_or_default();
        file_diff(&mut diff, path, old, src);
    }
    match m {
        // A patch is always a raw diff, regardless of `--format`.
        Mode::Patch(dest) if dest != "-" => {
            std::fs::write(dest, diff).map_err(|e| format!("{dest}: {e}"))?;
        }
        Mode::Patch(_) => write!(out, "{diff}").map_err(|e| e.to_string())?,
        // Preview honors `--format`, matching `refactor` / `rename`.
        _ => match mode.format {
            Format::Json => {
                let files: Vec<&String> = mutation.edited.keys().collect();
                let v = serde_json::json!({
                    "diff": diff,
                    "files": files,
                    "introducedDiagnostics": introduced,
                    "safe": introduced.is_empty(),
                });
                writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
            }
            Format::Text => {
                write!(out, "{diff}").map_err(|e| e.to_string())?;
                emit_introduced(&mut out, &introduced)?;
            }
        },
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn run_refactor(op: &RefactorOp) -> Result<ExitCode, String> {
    match op {
        RefactorOp::SortKnots { file, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (id, source) = project.file_or_entry(file.as_deref())?;
            let new = sort_knots_in_source(&source);
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::SortStitches { knot, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (id, source) = project.knot_file(knot)?;
            let new = sort_stitches_in_knot(&source, knot);
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::Format { target, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (knot, stitch) = split_dotted(target);
            let (id, source) = project.knot_file(knot)?;
            let new = format_region(&source, knot, stitch);
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderKnot {
            knot,
            direction,
            mode,
        } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (id, source) = project.knot_file(knot)?;
            let new =
                reorder_knot(&source, knot, (*direction).into()).map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderStitch {
            target,
            direction,
            mode,
        } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (knot, stitch) = split_dotted(target);
            let stitch = stitch.ok_or("reorder-stitch needs KNOT.STITCH")?;
            let (id, source) = project.knot_file(knot)?;
            let new = reorder_stitch(&source, knot, stitch, (*direction).into())
                .map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderKnots { order, file, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (id, source) = project.file_or_entry(file.as_deref())?;
            let names = parse_order(order);
            let new = reorder_knots(&source, &names).map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::ReorderStitches { knot, order, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (id, source) = project.knot_file(knot)?;
            let names = parse_order(order);
            let new = reorder_stitches(&source, knot, &names).map_err(|e| e.to_string())?;
            project.emit_single(id, &source, new, mode)
        }
        RefactorOp::MoveStitch { target, dest, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (knot, stitch) = split_dotted(target);
            let stitch = stitch.ok_or("move-stitch needs KNOT.STITCH")?;
            let (id, source) = project.knot_file(knot)?;
            let result = move_stitch(&source, &project.analysis, id, knot, stitch, dest)
                .map_err(|e| e.to_string())?;
            project.emit_move_result(id, result, mode)
        }
        RefactorOp::PromoteStitch { target, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (knot, stitch) = split_dotted(target);
            let stitch = stitch.ok_or("promote-stitch needs KNOT.STITCH")?;
            let (id, source) = project.knot_file(knot)?;
            let result = promote_stitch_to_knot(&source, &project.analysis, id, knot, stitch)
                .map_err(|e| e.to_string())?;
            project.emit_move_result(id, result, mode)
        }
        RefactorOp::DemoteKnot { knot, dest, mode } => {
            let project = Project::load(&mode.entry, &mode.lints.resolve())?;
            let (id, source) = project.knot_file(knot)?;
            let result = demote_knot_to_stitch(&source, &project.analysis, id, knot, dest)
                .map_err(|e| e.to_string())?;
            project.emit_move_result(id, result, mode)
        }
        RefactorOp::ConvertLine { at, target, mode } => run_convert_line(at, *target, mode),
    }
}

fn run_convert_line(at: &str, target: ConvertTo, mode: &MutOpts) -> Result<ExitCode, String> {
    let project = Project::load(&mode.entry, &mode.lints.resolve())?;
    let (file, line, col) = parse_at(at)?;
    let db = project.driver.db();
    let id = db
        .file_id(&file)
        .ok_or_else(|| format!("file not in project: {file}"))?;
    let source = db.source(id).unwrap_or_default().to_string();
    let hir = db.hir(id).ok_or("no HIR for that file")?;
    let root = db.parse(id).ok_or("no parse tree for that file")?.syntax();
    let offset = LineIndex::new(&source).offset(line.saturating_sub(1), col.saturating_sub(1));
    let edit = convert_element(&source, hir, &root, u32::from(offset), target.into())
        .ok_or("that line cannot be converted to the requested type")?;
    let mut new = source.clone();
    new.replace_range(edit.from as usize..edit.to as usize, &edit.insert);
    project.emit_single(id, &source, new, mode)
}

pub(super) fn run_actions(at: &str, opts: &CommonOpts) -> Result<ExitCode, String> {
    let project = Project::load(&opts.entry, &opts.lints.resolve())?;
    let (file, line, col) = parse_at(at)?;
    let db = project.driver.db();
    let id = db
        .file_id(&file)
        .ok_or_else(|| format!("file not in project: {file}"))?;
    let source = db.source(id).unwrap_or_default();
    let offset = LineIndex::new(source).offset(line.saturating_sub(1), col.saturating_sub(1));
    let actions = code_actions(source, u32::from(offset) as usize);

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let arr: Vec<_> = actions
                .iter()
                .map(|a| serde_json::json!({ "title": a.title, "kind": action_kind_name(&a.kind) }))
                .collect();
            writeln!(out, "{}", to_json(&arr)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            for a in &actions {
                writeln!(out, "{}", a.title).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ── effects-diff (T2-4, #863, docs/effects-spec.md §10) ─────────────

/// One definition's effect-row change between a baseline and the head.
#[derive(serde::Serialize)]
struct EffectDiffEntry {
    /// `"knot spend"` / `"stitch hub.market"` — kind + qualified name, the
    /// stable key shared across the two builds.
    def: String,
    /// `"added"` / `"removed"` / `"changed"`.
    change: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    base: Option<EffectRowView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<EffectRowView>,
}

/// Diff every knot/stitch's inferred effect row against a baseline (another
/// entry file, or a git revision of the same project). Drift *visibility*
/// only — advisory, no policy (spec §10).
pub(super) fn run_effects_diff(opts: &EffectsDiffOpts) -> Result<ExitCode, String> {
    let head = Project::load(&opts.entry, &opts.lints.resolve())?;
    let head_rows = head.collect_effect_rows();

    let base_rows = match (opts.rev.as_deref(), opts.base.as_deref()) {
        (Some(rev), None) => {
            load_git_baseline(&opts.entry, rev, &opts.lints.resolve())?.collect_effect_rows()
        }
        (None, Some(base_entry)) => {
            Project::load(base_entry, &opts.lints.resolve())?.collect_effect_rows()
        }
        _ => return Err("provide exactly one of --rev <REV> or --base <FILE>".to_string()),
    };

    let entries = diff_effect_rows(&base_rows, &head_rows);
    let changed = entries.iter().filter(|e| e.change == "changed").count();
    let added = entries.iter().filter(|e| e.change == "added").count();
    let removed = entries.iter().filter(|e| e.change == "removed").count();

    let mut out = io::stdout().lock();
    match opts.format {
        Format::Json => {
            let v = serde_json::json!({
                "changed": changed,
                "added": added,
                "removed": removed,
                "entries": entries,
            });
            writeln!(out, "{}", to_json(&v)?).map_err(|e| e.to_string())?;
        }
        Format::Text => {
            write!(out, "{}", render_effects_diff_markdown(&entries)).map_err(|e| e.to_string())?;
        }
    }

    Ok(if opts.exit_code && !entries.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// Union-diff two `def → row` maps into per-definition change entries, in
/// deterministic key order (both maps are `BTreeMap`). Unchanged rows are
/// omitted.
fn diff_effect_rows(
    base: &BTreeMap<String, EffectRowView>,
    head: &BTreeMap<String, EffectRowView>,
) -> Vec<EffectDiffEntry> {
    let mut keys: BTreeSet<&String> = BTreeSet::new();
    keys.extend(base.keys());
    keys.extend(head.keys());

    let mut out = Vec::new();
    for key in keys {
        match (base.get(key), head.get(key)) {
            (None, Some(h)) => out.push(EffectDiffEntry {
                def: key.clone(),
                change: "added",
                base: None,
                head: Some(h.clone()),
            }),
            (Some(b), None) => out.push(EffectDiffEntry {
                def: key.clone(),
                change: "removed",
                base: Some(b.clone()),
                head: None,
            }),
            (Some(b), Some(h)) if b != h => out.push(EffectDiffEntry {
                def: key.clone(),
                change: "changed",
                base: Some(b.clone()),
                head: Some(h.clone()),
            }),
            _ => {}
        }
    }
    out
}

/// Render the diff as a CI-comment-friendly Markdown block. Empty diff → a
/// single reassuring line (spec §10: this is visibility, never a gate by
/// itself).
fn render_effects_diff_markdown(entries: &[EffectDiffEntry]) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "## Effect row diff");
    let _ = writeln!(s);
    if entries.is_empty() {
        let _ = writeln!(s, "No effect row changes.");
        return s;
    }
    let changed = entries.iter().filter(|e| e.change == "changed").count();
    let added = entries.iter().filter(|e| e.change == "added").count();
    let removed = entries.iter().filter(|e| e.change == "removed").count();
    let _ = writeln!(s, "_{changed} changed, {added} added, {removed} removed._");
    let _ = writeln!(s);
    for e in entries {
        let base_line = e
            .base
            .as_ref()
            .map_or("—".to_string(), EffectRowView::display_line);
        let head_line = e
            .head
            .as_ref()
            .map_or("—".to_string(), EffectRowView::display_line);
        let _ = writeln!(
            s,
            "- **{}** — {}: `{base_line}` → `{head_line}`",
            e.def, e.change
        );
    }
    s
}

fn action_kind_name(k: &CodeActionKind) -> &'static str {
    match k {
        CodeActionKind::QuickFix => "quickfix",
        CodeActionKind::Refactor => "refactor",
        CodeActionKind::Source => "source",
    }
}

/// Split `KNOT` / `KNOT.STITCH` into its parts (only the first dot is honored).
fn split_dotted(s: &str) -> (&str, Option<&str>) {
    match s.split_once('.') {
        Some((knot, stitch)) => (knot, Some(stitch)),
        None => (s, None),
    }
}

/// Parse a comma-separated permutation list, trimming whitespace.
fn parse_order(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Append a `git`-style file-rename diff (delete `old`, create `new`).
fn rename_diff(out: &mut String, old: &str, new: &str, old_src: &str, new_src: &str) {
    let old_lines: Vec<&str> = old_src.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new_src.split_inclusive('\n').collect();
    let _ = write!(
        out,
        "diff --git a/{old} b/{new}\nrename from {old}\nrename to {new}\n--- a/{old}\n+++ b/{new}\n@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    );
    for l in &old_lines {
        push_diff_line(out, '-', l);
    }
    for l in &new_lines {
        push_diff_line(out, '+', l);
    }
}

fn emit_introduced(out: &mut impl Write, introduced: &[DiagEntry]) -> Result<(), String> {
    if !introduced.is_empty() {
        writeln!(
            out,
            "would introduce {} new diagnostic(s):",
            introduced.len()
        )
        .map_err(|e| e.to_string())?;
        for d in introduced {
            writeln!(
                out,
                "  {}[{}] {} {}",
                d.severity,
                d.code,
                d.location.display(),
                d.message
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ── Story-graph rendering ───────────────────────────────────────────

fn node_kind_name(k: StoryNodeKind) -> &'static str {
    match k {
        StoryNodeKind::Knot => "knot",
        StoryNodeKind::Stitch => "stitch",
        StoryNodeKind::End => "end",
        StoryNodeKind::Done => "done",
    }
}

fn edge_kind_name(k: StoryEdgeKind) -> &'static str {
    match k {
        StoryEdgeKind::Divert => "divert",
        StoryEdgeKind::Choice => "choice",
        StoryEdgeKind::Tunnel => "tunnel",
        StoryEdgeKind::Thread => "thread",
    }
}

fn graph_json(graph: &StoryGraph) -> serde_json::Value {
    let nodes: Vec<_> = graph
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id,
                "name": n.name,
                "kind": node_kind_name(n.kind),
                "parent": n.parent,
            })
        })
        .collect();
    let edges: Vec<_> = graph
        .edges
        .iter()
        .map(|e| serde_json::json!({ "from": e.from, "to": e.to, "kind": edge_kind_name(e.kind) }))
        .collect();
    serde_json::json!({ "nodes": nodes, "edges": edges })
}

fn write_graph_dot(out: &mut impl Write, graph: &StoryGraph) -> Result<(), String> {
    writeln!(out, "digraph story {{").map_err(|e| e.to_string())?;
    for n in &graph.nodes {
        writeln!(
            out,
            "  {:?} [label={:?}];",
            n.id,
            format!("{} ({})", n.name, node_kind_name(n.kind))
        )
        .map_err(|e| e.to_string())?;
    }
    for e in &graph.edges {
        writeln!(
            out,
            "  {:?} -> {:?} [label={:?}];",
            e.from,
            e.to,
            edge_kind_name(e.kind)
        )
        .map_err(|x| x.to_string())?;
    }
    writeln!(out, "}}").map_err(|e| e.to_string())?;
    Ok(())
}
