//! D6 (`docs/debugger-spec.md` §2, issue #3184): recording
//! `(bytecode_offset, source_range)` pairs as `emit` walks the container
//! tree, and assembling them into a `brink_format::DebugInfoSection`.
//!
//! Gated on [`EmitOptions::emit_debug_info`], default `false` — the
//! ship-policy default (§1.2): a release compile never pays for this, and
//! never changes a single emitted byte (the byte-identical guarantee this
//! module exists to preserve).

use std::collections::HashMap;

use brink_format::{
    DEBUG_FLAG_IS_STMT, DEBUG_FLAG_PROLOGUE_END, DebugContainerTable, DebugEntry, DebugFileEntry,
    DebugInfoSection, DebugLocalEntry, FileSurface, NameId,
};
use brink_ir::{FileId, Provenance, lir};

/// Codegen-facing knobs for one `emit` call. `emit_debug_info` is the only
/// field today — a `struct` (not a bare bool parameter) so a future knob
/// (e.g. a D7 "populate locals" toggle) doesn't need another `emit_with_*`
/// overload.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmitOptions<'a> {
    /// Emit the `SectionKind::DebugInfo` section (`docs/debugger-spec.md`
    /// §2). `false` (the `Default`) reproduces today's `emit()` behavior
    /// byte-for-byte — this is what the ship-policy ruling (§1.2) and the
    /// oracle-safety guarantee both depend on.
    pub emit_debug_info: bool,
    /// Source text per file, for the `DebugInfo` file table's `source_hash`
    /// and `line_starts` (#3261). Only read when `emit_debug_info` is set,
    /// so a release compile never pays for gathering it.
    ///
    /// The text must be **exactly what the compiler consumed** — the hash
    /// is a staleness detector and any normalisation applied here but not
    /// by a later reader (or vice versa) turns it into a permanent false
    /// alarm.
    ///
    /// `None` (or a file missing from the map) means that file's entry gets
    /// `source_hash: 0` and no line index: the section is still valid and
    /// positions still resolve, but staleness cannot be detected and
    /// `file:line` lookups for that file are unavailable. Degrading rather
    /// than failing is deliberate — a debug artifact without the extras
    /// beats no debug artifact.
    pub debug_sources: Option<&'a std::collections::BTreeMap<brink_ir::FileId, String>>,
}

/// One recorded `(bytecode_offset, provenance)` pair for a single
/// container, before file-table interning. Statement-level only in v1
/// (`docs/debugger-spec.md` §2.1) — every entry this module produces sets
/// `DEBUG_FLAG_IS_STMT`.
pub(crate) struct RawDebugEntry {
    pub offset: u32,
    pub provenance: Provenance,
    /// This entry's `bytecode_offset` is the prologue-end landing point
    /// (§2.4) — at most one `true` per container.
    pub prologue_end: bool,
}

/// One recorded temp-slot declaration for a single container's `LocalsTable`
/// (`docs/debugger-spec.md` §3, D7/#3185). Produced from
/// [`brink_ir::lir::Param`] (function/knot/stitch parameters — bound by a
/// bare `DeclareTemp` opcode the caller emits directly, with no `lir::Stmt`
/// of its own, so no source-level declaring range exists at this layer) and
/// from [`brink_ir::lir::StmtKind::DeclareTemp`] (`~ temp` declarations,
/// which do carry a real declaring [`Provenance`] via `Stmt::provenance`).
pub(crate) struct RawLocal {
    pub slot: u16,
    pub name: NameId,
    /// `None` for parameters (no per-param source range in LIR — see
    /// above); `Some(stmt.provenance)` for a `~ temp` declaration.
    pub declaring_range: Option<Provenance>,
    /// [`brink_ir::lir::StmtKind::DeclareTemp`]'s `synthetic` — a
    /// compiler-minted temp (#3395) the debugger hides; always `false` for
    /// a parameter.
    pub synthetic: bool,
}

/// Per-`emit()`-call debug-info recording state, held alongside
/// [`crate::EmitState`] and threaded the same way. A dedicated struct
/// (rather than loose `Option` fields on `EmitState`) keeps the container
/// walk's debug bookkeeping out of the production emission path except at
/// its two call sites, both gated on `EmitState::debug` being `Some`
/// (`CLAUDE.md` "Instrumentation doesn't belong in the production path").
pub(crate) struct DebugCollector {
    /// One `Vec<RawDebugEntry>` per container, pushed in the same order as
    /// `EmitState::chunks` — i.e. lockstep with the eventual
    /// `StoryData::containers`, matching §2.2's `container_idx` contract.
    containers: Vec<Vec<RawDebugEntry>>,
    /// One `Vec<RawLocal>` per container, parallel to `containers` above
    /// (same push order, same lockstep contract).
    locals: Vec<Vec<RawLocal>>,
    files: FileTableBuilder,
}

impl DebugCollector {
    pub(crate) fn new() -> Self {
        Self {
            containers: Vec::new(),
            locals: Vec::new(),
            files: FileTableBuilder::new(),
        }
    }

    /// Push one container's raw entries (already offset-ordered by
    /// construction — the container walk records them in emission order,
    /// which is offset order) plus its raw locals, and intern every
    /// referenced file (from entries and from any local's declaring range)
    /// into the section-local file table, first-reference order (§2.3).
    pub(crate) fn push_container(&mut self, raw: Vec<RawDebugEntry>, locals: Vec<RawLocal>) {
        for entry in &raw {
            self.files.intern(entry.provenance.file);
        }
        for local in &locals {
            if let Some(range) = local.declaring_range {
                self.files.intern(range.file);
            }
        }
        self.containers.push(raw);
        self.locals.push(locals);
    }

    /// Finish collecting and produce the wire-shaped section. `program`
    /// resolves each interned `FileId` to its project-root-relative path
    /// and lets each file be classified by surface (§2.3). `errors` is
    /// `EmitState::errors` (#3219 review): an interned `FileId` missing from
    /// `program.file_paths` is a defect in the LIR fed to codegen — the same
    /// class of thing `CodegenError` exists for — and must be surfaced
    /// there, not silently defaulted to an empty path (which
    /// `FileTableBuilder::to_entries` used to do, misclassifying the entry
    /// as `FileSurface::Ink` in the process — worse than a crash, since it
    /// routes a resolver lookup to the wrong `ProvenanceResolver` instead of
    /// failing loudly).
    pub(crate) fn finish(
        self,
        program: &lir::Program,
        sources: Option<&std::collections::BTreeMap<FileId, String>>,
        errors: &mut Vec<crate::CodegenError>,
    ) -> DebugInfoSection {
        let files = self.files.to_entries(program, sources, errors);
        let index_of = |file: FileId| -> u32 { self.files.index_of(file) };
        // Resolve a `NameId` against the program's name table — falls back
        // to an empty name (never panics, per `CLAUDE.md`'s deny-`unwrap`/
        // `expect`/`panic` posture) on an out-of-range id, which should not
        // happen: every `NameId` on a `Param`/`DeclareTemp` is interned into
        // this same table during LIR lowering.
        let name_of = |id: NameId| -> String {
            program
                .name_table
                .get(id.0 as usize)
                .cloned()
                .unwrap_or_default()
        };
        let containers = self
            .containers
            .into_iter()
            .zip(self.locals)
            .map(|(raw, raw_locals)| {
                let entries = raw
                    .into_iter()
                    .map(|e| {
                        let mut flags = DEBUG_FLAG_IS_STMT;
                        if e.prologue_end {
                            flags |= DEBUG_FLAG_PROLOGUE_END;
                        }
                        let range = e.provenance.range;
                        DebugEntry {
                            bytecode_offset: e.offset,
                            file_idx: index_of(e.provenance.file),
                            range_start: u32::from(range.start()),
                            range_len: u32::from(range.len()),
                            kind_token: e.provenance.kind.as_u32(),
                            flags,
                        }
                    })
                    .collect();
                // D7's payload (docs/debugger-spec.md §3, issue #3185):
                // slot -> name (+ optional declaring range) for every
                // parameter and `~ temp` declared directly in this
                // container's own body (nested child containers — branch
                // bodies, gathers, choice targets — get their own table
                // when they're walked in turn, per §2.2's per-container
                // lockstep framing).
                let locals = raw_locals
                    .into_iter()
                    .map(|l| DebugLocalEntry {
                        slot: l.slot,
                        name: name_of(l.name),
                        declaring_range: l.declaring_range.map(|p| {
                            (
                                index_of(p.file),
                                u32::from(p.range.start()),
                                u32::from(p.range.len()),
                            )
                        }),
                        synthetic: l.synthetic,
                    })
                    .collect();
                DebugContainerTable { entries, locals }
            })
            .collect();
        DebugInfoSection { files, containers }
    }
}

/// Interns `FileId`s into the section-local file table (§2.3) in
/// first-reference order, seeded with the reserved synthetic sentinel at
/// index 0 (§2.5) regardless of whether anything ends up referencing it.
struct FileTableBuilder {
    order: Vec<FileId>,
    index: HashMap<FileId, u32>,
}

impl FileTableBuilder {
    fn new() -> Self {
        let mut b = Self {
            order: Vec::new(),
            index: HashMap::new(),
        };
        // Index 0 is always the synthetic sentinel — `Provenance::synthetic`
        // stamps `FileId(u32::MAX)` (`brink-ir/src/provenance.rs`).
        b.order.push(FileId(u32::MAX));
        b.index.insert(FileId(u32::MAX), 0);
        b
    }

    fn intern(&mut self, file: FileId) {
        if self.index.contains_key(&file) {
            return;
        }
        #[expect(clippy::cast_possible_truncation)]
        let idx = self.order.len() as u32;
        self.order.push(file);
        self.index.insert(file, idx);
    }

    /// Every raw entry's file passes through [`Self::intern`] (via
    /// `DebugCollector::push_container`) before this is ever called, so a
    /// miss here would mean a caller forgot that step — falling back to the
    /// sentinel index (never panicking, per `CLAUDE.md`'s deny-`unwrap`/
    /// `expect`/`panic` posture) rather than misattributing to another
    /// file.
    fn index_of(&self, file: FileId) -> u32 {
        self.index.get(&file).copied().unwrap_or(0)
    }

    /// `errors` receives a [`crate::CodegenError`] for every interned
    /// `FileId` that `program.file_paths` cannot resolve (#3219 review): a
    /// silent `unwrap_or_default()` used to land such a file at its
    /// already-assigned real index as `{surface: Ink, path: ""}` — a wrong
    /// answer stamped with unwarranted confidence, worse than failing,
    /// since a reader would route that file's entries through the ink
    /// `ProvenanceResolver` for a file that was never ink at all. The
    /// fallback shape here (`Synthetic`, empty path) is only ever reached
    /// alongside a pushed error, never silently.
    fn to_entries(
        &self,
        program: &lir::Program,
        sources: Option<&std::collections::BTreeMap<FileId, String>>,
        errors: &mut Vec<crate::CodegenError>,
    ) -> Vec<DebugFileEntry> {
        self.order
            .iter()
            .map(|file| {
                if *file == FileId(u32::MAX) {
                    return DebugFileEntry {
                        surface: FileSurface::Synthetic,
                        path: String::new(),
                        source_hash: 0,
                        line_starts: Vec::new(),
                    };
                }
                if let Some(path) = program.file_paths.get(file) {
                    // #3261: hash and line index, when the caller supplied
                    // this file's text. Absent text degrades to
                    // `source_hash: 0` + no index rather than failing —
                    // positions still resolve, only staleness detection and
                    // `file:line` lookup are unavailable.
                    let (source_hash, line_starts) = sources.and_then(|m| m.get(file)).map_or_else(
                        || (0, Vec::new()),
                        |text| (brink_format::content_hash(text), line_starts_of(text)),
                    );
                    DebugFileEntry {
                        surface: surface_from_path(path),
                        path: path.clone(),
                        source_hash,
                        line_starts,
                    }
                } else {
                    errors.push(crate::CodegenError::new(format!(
                        "codegen: DebugInfo file table references {file:?}, which has no \
                         entry in Program.file_paths — cannot resolve its path or surface \
                         for the debug-info section (#3219)"
                    )));
                    DebugFileEntry {
                        surface: FileSurface::Synthetic,
                        path: String::new(),
                        source_hash: 0,
                        line_starts: Vec::new(),
                    }
                }
            })
            .collect()
    }
}

/// Byte offset of the start of every line in `text` (#3261), ascending,
/// always beginning with 0.
///
/// Lines are split on `\n`; a `\r\n` file simply carries the `\r` as the
/// last byte of the preceding line, which is correct for offset purposes
/// and is why nothing here normalises. Normalising would silently break the
/// `source_hash` contract next to it, which is the raw bytes the compiler
/// consumed.
///
/// A trailing newline does NOT produce a final empty line entry: `"a\n"` is
/// one line, matching how every editor numbers it.
fn line_starts_of(text: &str) -> Vec<u32> {
    let mut starts = vec![0_u32];
    for (i, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            let next = i + 1;
            // A trailing newline does not open a new line: `"a\n"` is one
            // line, matching how every editor numbers it.
            if next < text.len()
                && let Ok(next) = u32::try_from(next)
            {
                starts.push(next);
            }
        }
    }
    starts
}

/// Classify a source file's frontend from its path — the same pure,
/// deterministic extension test `brink-db::queries::file_language` uses
/// (`.brink` case-insensitive → native, everything else → ink).
/// Duplicated here rather than shared: `brink-codegen-inkb` cannot depend
/// on `brink-db` (wrong dependency direction — `brink-db` depends on the
/// compiler crates, not the reverse), and this is a one-line pure
/// path-string predicate with nothing else worth extracting a shared crate
/// for.
fn surface_from_path(path: &str) -> FileSurface {
    let is_native = std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("brink"));
    if is_native {
        FileSurface::Native
    } else {
        FileSurface::Ink
    }
}
