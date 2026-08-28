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
    DebugInfoSection, FileSurface,
};
use brink_ir::{FileId, Provenance, lir};

/// Codegen-facing knobs for one `emit` call. `emit_debug_info` is the only
/// field today — a `struct` (not a bare bool parameter) so a future knob
/// (e.g. a D7 "populate locals" toggle) doesn't need another `emit_with_*`
/// overload.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmitOptions {
    /// Emit the `SectionKind::DebugInfo` section (`docs/debugger-spec.md`
    /// §2). `false` (the `Default`) reproduces today's `emit()` behavior
    /// byte-for-byte — this is what the ship-policy ruling (§1.2) and the
    /// oracle-safety guarantee both depend on.
    pub emit_debug_info: bool,
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
    files: FileTableBuilder,
}

impl DebugCollector {
    pub(crate) fn new() -> Self {
        Self {
            containers: Vec::new(),
            files: FileTableBuilder::new(),
        }
    }

    /// Push one container's raw entries (already offset-ordered by
    /// construction — the container walk records them in emission order,
    /// which is offset order) and intern every entry's file into the
    /// section-local file table, first-reference order (§2.3).
    pub(crate) fn push_container(&mut self, raw: Vec<RawDebugEntry>) {
        for entry in &raw {
            self.files.intern(entry.provenance.file);
        }
        self.containers.push(raw);
    }

    /// Finish collecting and produce the wire-shaped section. `program`
    /// resolves each interned `FileId` to its project-root-relative path
    /// and lets each file be classified by surface (§2.3).
    pub(crate) fn finish(self, program: &lir::Program) -> DebugInfoSection {
        let files = self.files.to_entries(program);
        let index_of = |file: FileId| -> u32 { self.files.index_of(file) };
        let containers = self
            .containers
            .into_iter()
            .map(|raw| {
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
                DebugContainerTable {
                    entries,
                    // D7's payload (docs/debugger-spec.md §3, issue #3185) —
                    // D6 ships the structural framing only.
                    locals: Vec::new(),
                }
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

    fn to_entries(&self, program: &lir::Program) -> Vec<DebugFileEntry> {
        self.order
            .iter()
            .map(|file| {
                if *file == FileId(u32::MAX) {
                    return DebugFileEntry {
                        surface: FileSurface::Synthetic,
                        path: String::new(),
                    };
                }
                let path = program.file_paths.get(file).cloned().unwrap_or_default();
                DebugFileEntry {
                    surface: surface_from_path(&path),
                    path,
                }
            })
            .collect()
    }
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
