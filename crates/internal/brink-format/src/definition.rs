use alloc::string::String;
use alloc::vec::Vec;

use crate::counting::CountingFlags;
use crate::id::{DefinitionId, NameId};
use crate::line::LineContent;
use crate::value::{ShapeId, Value, ValueType};

/// A compiled container (knot, stitch, gather, or anonymous flow block).
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerDef {
    pub id: DefinitionId,
    /// The lexical scope this container belongs to.
    /// For scope containers (root, knot, stitch): `scope_id == id`.
    /// For child containers (gather, choice target, sequence, etc.): `scope_id` is
    /// the enclosing scope's `DefinitionId`.
    pub scope_id: DefinitionId,
    /// Human-readable name for scope-owning containers (root, knot, stitch).
    /// `None` for child containers.
    pub name: Option<NameId>,
    pub bytecode: Vec<u8>,
    pub counting_flags: CountingFlags,
    /// Sum of char values from the container's ink path string.
    /// Used to seed the RNG for shuffle sequences.
    pub path_hash: i32,
    /// Number of parameters this container declares (a parameterized knot,
    /// stitch, or function — e.g. `=== call(action, present) ===` has 2). The
    /// container's prologue binds them with that many leading `DeclareTemp`s.
    /// `0` for the vast majority of containers. Lets the runtime arity-check a
    /// host-directed entry (`choose_path_string_with_args`) or `call_function`.
    /// The converter reference pipeline leaves this `0` (inklecate's JSON does
    /// not expose it); only the brink compiler populates the true count.
    pub param_count: u8,
    /// Per-parameter name and mode metadata, in declared order (T1c,
    /// `docs/t1c-spec.md` §6). Empty for the vast majority of containers.
    ///
    /// Carried so the runtime can validate a **rehydrated function value**
    /// against the *current* signature: a `#fn`/closure saved before a
    /// recompile stores its bound params' names and modes, and on load/invoke
    /// they are checked against this table — a renamed or re-moded param is a
    /// defined fault, never a silent misbinding (spec §6). `len()` always
    /// equals [`param_count`](Self::param_count); both are kept (the count is
    /// the pre-T1c arity-check field, the metadata is additive). The converter
    /// reference pipeline leaves this empty.
    pub params: Vec<ParamMeta>,
    /// Compiled scope default: `true` for a flow-private (`#@local`) knot or
    /// stitch. Only ever set on scope-owning containers; subtree coverage of
    /// interior containers is resolved by the runtime at policy resolution
    /// (`docs/directive-annotations-spec.md`). The converter always emits
    /// `false` (inklecate has no flow-private concept).
    pub local: bool,
}

/// Name + mode of one declared parameter of a container (T1c,
/// `docs/t1c-spec.md` §6). See [`ContainerDef::params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamMeta {
    /// The parameter's interned name.
    pub name: NameId,
    /// `true` if declared `ref`, `false` for a by-value param.
    pub is_ref: bool,
}

/// Metadata for a single interpolation slot in a template line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotInfo {
    pub index: u8,
    pub name: String,
}

/// Source location of a line in the original `.ink` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub file: String,
    pub range_start: u32,
    pub range_end: u32,
}

/// One entry in a container's line table.
#[derive(Debug, Clone, PartialEq)]
pub struct LineEntry {
    pub content: LineContent,
    pub flags: crate::LineFlags,
    pub source_hash: u64,
    pub audio_ref: Option<String>,
    pub slot_info: Vec<SlotInfo>,
    pub source_location: Option<SourceLocation>,
}

/// A locale line entry — content + optional audio, no source metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct LocaleLineEntry {
    pub content: LineContent,
    pub audio_ref: Option<String>,
}

/// A per-scope locale line table.
#[derive(Debug, Clone, PartialEq)]
pub struct LocaleScopeTable {
    pub scope_id: DefinitionId,
    pub lines: Vec<LocaleLineEntry>,
}

/// Complete locale overlay data from a `.inkl` file.
#[derive(Debug, Clone, PartialEq)]
pub struct LocaleData {
    pub locale_tag: String,
    pub base_checksum: u32,
    pub line_tables: Vec<LocaleScopeTable>,
}

/// Per-scope line table, stored separately from [`ContainerDef`] for
/// locale overlay swapping (`.inkl`).
///
/// All containers within a lexical scope (knot, stitch, or root) share one
/// `ScopeLineTable`. `EmitLine(idx)` indices are scope-relative.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeLineTable {
    pub scope_id: DefinitionId,
    pub lines: Vec<LineEntry>,
}

/// One line-variant group (stage 1 of the shared-alternatives track,
/// issue #3273): the record that ties `dims.iter().product()` consecutive
/// [`LineEntry`]s in a scope's line table back to ONE authored source line
/// whose inline alternatives were enumerated at recognition time.
///
/// The entries themselves are ordinary whole-line entries — each with its
/// own `source_hash` and its own `audio_ref`, which is the point: VO is
/// associated per *rendered* line, and a translator sees whole lines. This
/// record exists so intl export and audio tooling can group them, and so
/// codegen's combo switch and the table agree on the layout.
///
/// Layout contract: variant `(i, j, …)` — one branch index per authored
/// alternative, in source order — lives at
/// `base + i * dims[1..].product() + j * dims[2..].product() + …`
/// (row-major, first alternative varies slowest). `dims` is never empty
/// and every dim is ≥ 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineVariantGroup {
    /// The scope whose [`ScopeLineTable`] holds this group's entries.
    pub scope_id: DefinitionId,
    /// Index of the group's first entry in that scope's `lines`.
    pub base: u32,
    /// Branch count per authored alternative, in source order.
    pub dims: Vec<u16>,
}

/// A global variable definition.
#[derive(Debug, Clone, PartialEq)]
pub struct GlobalVarDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub value_type: ValueType,
    pub default_value: Value,
    pub mutable: bool,
    /// Compiled scope default: `true` for a flow-private (`#@local`)
    /// variable, `false` for ordinary shared state. Consumed by the runtime
    /// as the base layer of `WorldPolicy` resolution
    /// (`docs/directive-annotations-spec.md`).
    pub local: bool,
}

/// A list (enum-like set) definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListDef {
    pub id: DefinitionId,
    pub name: NameId,
    /// `(item_name, ordinal)` pairs in declaration order.
    pub items: Vec<(NameId, i32)>,
}

/// A `STRUCT` shape definition (TM-4, `docs/typed-mode-spec.md` §6;
/// `StructShapes` section, `docs/format-spec.md` tag `0x0C`).
///
/// Closed shape: `fields` is the ordered set of declared field names — the
/// same order [`crate::value::Value::Record`]'s flat field vector follows,
/// and the order `RecordNew`/static `RecordGet`/`RecordSet` offsets index
/// into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructShapeDef {
    pub id: ShapeId,
    pub name: NameId,
    pub fields: Vec<NameId>,
}

/// One old→new `DefinitionId` rename record (M-3, `docs/modules-spec.md`
/// §5): the compiler emits one of these per `#@was(old_name)` directive —
/// on a module, one entry per definition the renamed module currently
/// owns; on a single definition, one entry for it. Rehydration
/// (`brink-runtime`'s `load_state`) consults the table **only on the miss
/// path**: a saved fn token, divert value, or visit-count key that the
/// current program doesn't recognize is looked up here before being
/// treated as genuinely gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AliasEntry {
    /// The identity a save from before the rename may still carry.
    pub old: DefinitionId,
    /// The definition's current identity.
    pub new: DefinitionId,
}

/// The capability-parameter slot carried by every call atom in a factored
/// effect row (T2-3, `docs/effects-spec.md` §11; ruled 2026-07-14,
/// `docs/t1d-spec.md` §7).
///
/// v1 populates every atom as [`CapabilityParam::Any`] — component-granular,
/// the whole capability unrefined. Path-granular refinement (#826) and the
/// instance-resolving handle parameter are later narrowing rungs; their
/// discriminants are reserved (the strict reader rejects them until a section
/// version graduates them), the same reservation discipline the projection
/// range segment follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapabilityParam {
    /// The whole capability, unrefined — the only value v1 ever emits.
    #[default]
    Any,
}

/// A single call atom in an effect row's direct part (`docs/effects-spec.md`
/// §2/§11): the name of an `EXTERNAL` binding (a call-kind), plus its
/// [capability-parameter slot](CapabilityParam) and a **reserved
/// handle-parameter slot**.
///
/// The handle-parameter slot (`docs/t1d-spec.md` §7) is where a
/// handle-parameterized atom (`Transform(@argN)`) will record which minted
/// handle bounds the capability; v1 leaves it `None` (the reserved wire byte is
/// `0`). Possession-bounded capabilities are the tier-2 security model — out of
/// scope for this slice, but the slot ships now so the row encoding need not
/// change to carry it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallAtom {
    /// The `EXTERNAL` binding name (interned into the story's `name_table`).
    pub name: NameId,
    /// Capability-parameter slot — v1 always [`CapabilityParam::Any`].
    pub capability: CapabilityParam,
    /// Reserved handle-parameter slot — v1 always `None`. A non-`None` value
    /// is never emitted in this section version.
    pub handle_param: Option<u8>,
}

/// The direct part of a factored effect row (`docs/effects-spec.md` §7/§11):
/// the atoms a definition (and everything it statically calls) may perform,
/// independent of any dispatch-cell narrowing. Mirrors the analyzer's flat
/// `EffectRow`, lowered to wire vocabulary (cells as [`DefinitionId`]s, call
/// kinds as [`CallAtom`]s).
///
/// Sets are stored as vectors already sorted/deduplicated by the producer so
/// the encoding is deterministic (the analyzer sources them from `BTreeSet`s).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the four bools are independent wire dimensions of one factored \
              effect row (opaque + the NS-A2 emits/tags/faults flags), not a \
              state machine in disguise — mirrors the analyzer's EffectRow"
)]
pub struct DirectEffects {
    /// Global cells this row may read.
    pub reads: Vec<DefinitionId>,
    /// Global cells this row may write.
    pub writes: Vec<DefinitionId>,
    /// Call-kind atoms this row may transitively perform.
    pub calls: Vec<CallAtom>,
    /// The pessimal top element (`docs/effects-spec.md` §3): this row performs
    /// a call whose effects inference cannot summarize.
    pub opaque: bool,
    /// NS-A2 (issue #1108, from #1087): the definition may produce content —
    /// narration/dialogue fragments a host renders (glue-only output counts;
    /// tag-only lines do NOT — those set [`Self::tags`]). Bool v1.
    pub emits: bool,
    /// NS-A2 (issue #1108, from #1087's second ruling): the definition may
    /// touch the tag channel. Independent of [`Self::emits`]. Bool v1.
    pub tags: bool,
    /// NS-A2 (issue #1108, from #1097): the definition may raise a
    /// turn-terminating fault. Bool v1 — per-fault-kind granularity is the
    /// reserved refinement and graduates via a section-version bump, the
    /// same reservation discipline the capability/handle slots follow.
    pub faults: bool,
}

/// A per-dispatch entry in a factored effect row (`docs/effects-spec.md` §7):
/// the row a call through a dispatch `cell` contributes, whether that dispatch
/// is runtime-**narrowable** (its cell is not in the entry's own write set),
/// and the **static fallback** row used when narrowing does not apply.
///
/// v1 emits none of these (call-through-value is inferred as opaque, folded
/// into the direct part) — but the encoding ships the structure now, because a
/// flat row structurally forecloses the §7 narrowing the host will do at
/// schedule-commit. The reader round-trips a populated dispatch list so writer
/// and reader stay paired (the #742 lesson).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchEntry {
    /// The dispatch cell whose live fn tokens the host may narrow against.
    pub cell: DefinitionId,
    /// Whether this dispatch is statically narrowable (`docs/effects-spec.md`
    /// §7 soundness gate: the cell is not in the entry's own write set).
    pub narrowable: bool,
    /// The static fallback row — the conservative join used when the host does
    /// not (or cannot) narrow.
    pub fallback: DirectEffects,
}

/// One entry in the `EffectRows` `DefinitionId → row` table (T2-3,
/// `docs/effects-spec.md` §11, `docs/format-v4-rfc.md` §2 `EffectRows`
/// reservation): a factored effect row for one definition.
///
/// Every knot/stitch ships one — the per-container row is the host's
/// resume-scheduling estimate (`docs/effects-spec.md` §12.1: a flow resumes
/// from wherever it parked). The row is additive metadata; the runtime does
/// not consume it yet (`sleep`/narrowing are the future clients), so a story
/// carrying rows is byte-identical in behavior to one without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRowEntry {
    /// The definition (knot/stitch) this row summarizes.
    pub def: DefinitionId,
    /// The freeze bit (#882, `docs/effects-spec.md` §10 sitting-2 ruling;
    /// corroborated on `main` by `docs/modules-spec.md` §4 boundary rule 1/2
    /// and the 2026-07-14 "Modules & visibility rulings" decision-log entry —
    /// `docs/effects-spec.md` itself never merged past `docs/effects-skeleton`).
    ///
    /// `true` for every def by default: the row is a legitimate host
    /// **entry point** (host-callable by path/name — `begin_function_eval`,
    /// entry lookup, play-from-here). `false` for a `#@private` definition:
    /// **not an entry** — host-facing name lookup on it is refused (the
    /// load-error class effects-spec §10 rules) — but the row itself is not
    /// dropped from the table. `#@private` hides the *name*, not the *cell*:
    /// a private knot/stitch can still be captured as a first-class fn-value
    /// token that a *public* path holds and later calls through, and the
    /// dispatch-narrowing machinery (§7) resolves such a token by
    /// `DefinitionId`, not by name — so the row must stay resolvable in this
    /// table regardless of `is_entry`. This is unconditional (never a
    /// reachability computation over whether some public path actually holds
    /// such a token today): proving that would need whole-program fn-value
    /// capture analysis, which conservative-total rows do not attempt.
    /// Dev-tooling (play-from-here, `brink ide` effects-diff) is the
    /// documented visibility override (modules-spec §4 rule 3) and may read
    /// a non-entry row directly from this table; only *host* semantic lookup
    /// respects `is_entry`.
    pub is_entry: bool,
    /// The direct part — atoms independent of dispatch narrowing.
    pub direct: DirectEffects,
    /// Per-dispatch entries (empty in v1).
    pub dispatches: Vec<DispatchEntry>,
}

/// The name-keyed **frame shape** for one `await` site
/// (`docs/flow-suspension-spec.md` §4/§11): the static description of which
/// locals cross the park at that site, so the runtime knows what to
/// spill on park and restore on wake.
///
/// Emitted into the `FrameShapes` [`StoryData`](crate::StoryData) section
/// (`.inkb` tag `0x10`,
/// `.inkt` `(frame_shapes …)`). The shape is **name-keyed** — the runtime
/// spills/restores crossing locals by name, riding the same rehydration
/// machinery as `#@was`/saves (spec §7), so a frame survives recompiles
/// without instruction offsets (spec §2/§3).
///
/// **Reserved-through-fence**: the FS-3c compiler slice lands this section's
/// encoding (writer + reader + round-trips) but does not yet *emit* a
/// non-empty table — the E052 `await` lowering fence keeps `await` from
/// producing any `StoryData`. First emission rides the continuation-splitting
/// codegen when the fence drops (FS-3r), the same reserved-then-materialized
/// discipline `StructShapes` followed. `frame_shapes` is therefore empty for
/// every story compiled today (and for all converter output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameShapeDef {
    /// The `await` site's stable identity — the [`DefinitionId`] of the
    /// synthesized resume/continuation container (spec §11.1: stable identity
    /// = module + enclosing def + site index). This is both the wake-policy
    /// site id and the container the runtime enters from its top on resume.
    pub site: DefinitionId,
    /// The name-keyed crossing locals, in stable declared order (spec §4).
    /// Each entry is the local's interned [`NameId`] (into the story's
    /// `name_table`); the runtime's frame record is keyed by these names.
    /// Values are not stored here — the shape is static; the live values live
    /// in the save-time `SuspendedFlow.frame` (`docs/flow-suspension-spec.md`
    /// §2, FS-1).
    pub slots: Vec<NameId>,
}

/// A single list item definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItemDef {
    pub id: DefinitionId,
    pub origin: DefinitionId,
    pub ordinal: i32,
    pub name: NameId,
}

/// An address pointing to a specific byte offset within a container.
///
/// Addresses are used for divert targets, visit tracking, and any definition
/// that maps to a position within a container. A "primary" address has
/// `byte_offset == 0` and the same `id` as its `container_id`, functioning
/// like the old `Container` tag. Intra-container addresses have non-zero
/// offsets and distinct IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressDef {
    pub id: DefinitionId,
    pub container_id: DefinitionId,
    pub byte_offset: u32,
}

/// Maps a qualified author path (e.g. `knot`, `knot.stitch`, `knot.label`,
/// `knot.stitch.label`) to the [`DefinitionId`] it addresses.
///
/// This is the source of truth for path → address lookup
/// ([`Program::find_address`](../../brink_runtime/struct.Program.html#method.find_address)):
/// the linker resolves each `target` through its address map. The compiler
/// emits one entry per scope container (knot/stitch) and per author-labeled
/// gather/choice. `path` indexes the name table; `target` is the addressed
/// container/label id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressPath {
    pub path: NameId,
    pub target: DefinitionId,
}

/// Compute a stable hash of source text — FNV-1a, 64-bit, over the UTF-8
/// bytes.
///
/// **Part of the wire contract** (`docs/format-spec.md`), not an
/// implementation detail. Hashes produced here are written into artifacts
/// ([`LineEntry::source_hash`] for the intl regeneration workflow;
/// [`DebugFileEntry::source_hash`] for the debugger's staleness check,
/// issue #3261) and compared later — potentially by a different binary, a
/// different toolchain, or a `no_std` build. So the algorithm is specified
/// and identical on every path, and must not change without treating it as
/// a format change.
///
/// This is why it is no longer `std`'s `DefaultHasher`: Rust documents that
/// hasher's algorithm as unspecified and subject to change between
/// releases, and the `no_std` fallback it used to sit beside was explicitly
/// not bit-identical to it. Both were fine while nothing compared hashes
/// across builds. Recording a hash in an artifact makes that exactly what
/// happens, and a silent algorithm change would make every comparison
/// report "changed" forever, with no obvious cause.
///
/// FNV-1a is a **change detector, not a proof**: it is not collision
/// resistant and is not a security primitive. A collision means changed
/// text is reported as unchanged — for the intl workflow, a line missed for
/// retranslation; for the debugger, a stale source accepted as fresh, which
/// is no worse than the silent wrong answer the check exists to replace.
#[must_use]
pub fn content_hash(text: &str) -> u64 {
    // FNV-1a 64-bit: offset basis, then per-byte xor-and-multiply by the
    // FNV prime. Written out rather than pulled from a crate so the wire
    // contract has no dependency that could revise it underneath us.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// An externally-bound function definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalFnDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub arg_count: u8,
    pub fallback: Option<DefinitionId>,
}

// ── DebugInfo (D6, `docs/debugger-spec.md` §2) ──────────────────────────────

/// `flags` bit 0: this entry marks a recommended stop location / the start
/// of a statement (`docs/debugger-spec.md` §2.1's DWARF-`is_stmt` design).
/// v1 sets this on every entry (statement-level rows only); a later
/// expression-level entry arrives with this bit unset, additively — no
/// version bump, no reader change.
pub const DEBUG_FLAG_IS_STMT: u8 = 0b0000_0001;

/// `flags` bit 1: this entry's own `bytecode_offset` is the prologue-end
/// landing point for a breakpoint set on the enclosing container
/// (`docs/debugger-spec.md` §2.4) — past any leading parameter-binding
/// `DeclareTemp`s / choice-output prologue bytes. At most one entry per
/// container carries this bit.
pub const DEBUG_FLAG_PROLOGUE_END: u8 = 0b0000_0010;

/// Bits 2–7 of `flags` are reserved. Per `docs/debugger-spec.md` §2.2's
/// explicit, ruled departure from this format's default strict-rejection
/// posture, a `DebugInfo` reader **must ignore** any reserved bit it does
/// not recognize rather than reject the entry — this constant exists so
/// callers can mask deliberately (e.g. a round-trip test asserting v1 never
/// sets a reserved bit) without hand-writing the mask twice.
pub const DEBUG_FLAG_RESERVED_MASK: u8 = !(DEBUG_FLAG_IS_STMT | DEBUG_FLAG_PROLOGUE_END);

/// Which frontend parsed a `DebugInfo` file-table entry's file
/// (`docs/debugger-spec.md` §2.3) — `KindToken::raw` is frontend-private
/// (two independent `ProvenanceResolver` numberings), so a reader must know
/// which resolver applies before interpreting an entry's `kind_token`.
/// Recorded once per file (not per entry) since surface is a property of
/// where the code came from, constant for every entry pointing at that
/// file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileSurface {
    /// The reserved sentinel file (index 0) — synthetic provenance
    /// (`Provenance::synthetic`'s `FileId(u32::MAX)`, §2.5). Never a real
    /// file; `path` is always empty for this surface.
    Synthetic = 0,
    /// Parsed by the `.ink` compatibility surface (`brink-syntax`).
    Ink = 1,
    /// Parsed by the `.brink` native surface (`brink-syntax-native`).
    Native = 2,
}

impl FileSurface {
    pub(crate) fn from_u8(tag: u8) -> Result<Self, crate::opcode::DecodeError> {
        match tag {
            0 => Ok(Self::Synthetic),
            1 => Ok(Self::Ink),
            2 => Ok(Self::Native),
            _ => Err(crate::opcode::DecodeError::InvalidFileSurface(tag)),
        }
    }
}

/// One entry in the `DebugInfo` section's section-local file table
/// (`docs/debugger-spec.md` §2.3). Index 0 is always the reserved synthetic
/// sentinel (§2.5) — `surface = Synthetic`, `path = ""` — real files start
/// at index 1. Paths are project-root-relative (`root_relative_key`), not
/// process-cwd-relative or absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugFileEntry {
    pub surface: FileSurface,
    pub path: String,
    /// [`content_hash`] of this file's text **exactly as the compiler
    /// consumed it** (#3261) — no normalisation of line endings,
    /// whitespace or encoding on either side. A reader that hashes
    /// differently-normalised text will see a spurious mismatch, so the
    /// contract is the raw bytes that were compiled.
    ///
    /// Lets a consumer detect that the source it is measuring against is
    /// not the source this program was built from, and answer
    /// `StaleSource` instead of a confidently wrong address. That failure
    /// applies to byte ranges every bit as much as to line numbers —
    /// offsets shift on every inserted character.
    ///
    /// Per-file, deliberately: one dirty file then degrades debugging in
    /// that file alone, where a whole-program checksum degrades
    /// everything.
    ///
    /// `0` for the reserved synthetic sentinel at index 0, which names no
    /// real file.
    pub source_hash: u64,
    /// Byte offset of the start of each line in this file, ascending, with
    /// `line_starts[0] == 0` (#3261). Length is the file's line count.
    ///
    /// Carrying this means the engine can answer `file:line` **without
    /// being handed source text at all** — which is what a remote debugger
    /// frontend (DAP's `setBreakpoints` is file + line) needs, and what
    /// keeps line↔byte conversion to one implementation instead of one per
    /// consumer. Line indexing is 0-based here; a UI showing 1-based line
    /// numbers converts at its own edge.
    ///
    /// Empty for the synthetic sentinel, and legitimately empty for an
    /// empty file.
    pub line_starts: Vec<u32>,
}

/// One row in a container's `DebugInfo` entry table
/// (`docs/debugger-spec.md` §2.2): maps a bytecode offset (within that
/// container's own bytecode) to the source range it was lowered from.
/// Entries for one container are sorted ascending by `bytecode_offset` so a
/// reader can floor-lookup via binary search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugEntry {
    /// Byte offset within the owning container's own bytecode.
    pub bytecode_offset: u32,
    /// Index into the section's file table (§2.3) — not the compiler's
    /// project-wide `FileId`.
    pub file_idx: u32,
    /// Absolute source byte offset within the file at `file_idx`.
    pub range_start: u32,
    /// Length in bytes of the source range (`range_end = range_start +
    /// range_len`).
    pub range_len: u32,
    /// `KindToken::as_u32()` verbatim (class in the high 16 bits, raw in
    /// the low 16) — `brink-format` carries this opaquely; interpreting it
    /// needs `file_table[file_idx].surface` to pick the right
    /// `ProvenanceResolver` (§2.3), which is `brink-ir`'s job, not this
    /// crate's (no dependency edge from `brink-format` to `brink-ir`).
    pub kind_token: u32,
    /// `DEBUG_FLAG_IS_STMT` / `DEBUG_FLAG_PROLOGUE_END`, plus reserved bits
    /// a reader must tolerate (never reject on) — see those constants' docs.
    pub flags: u8,
}

/// One row in a container's `DebugInfo` locals table
/// (`docs/debugger-spec.md` §3): a VM temp slot's declared name, and
/// optionally the source range it was declared at (for slot-reuse
/// disambiguation). **D7's payload** (`docs/debugger-spec.md` §3, issue
/// #3185) — D6 emits the structural framing (an empty `locals` per
/// container) but does not populate real entries; the wire shape ships now
/// so D7 adds data without a layout change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLocalEntry {
    /// Matches the `u16` operand `DeclareTemp`/`GetTemp`/`SetTemp` use.
    pub slot: u16,
    pub name: String,
    /// The declaring range, if known: `(file_idx, range_start, range_len)`,
    /// the same triple shape entries use (§2.2).
    pub declaring_range: Option<(u32, u32, u32)>,
    /// A temp the compiler minted rather than the author (issue #3395: the
    /// lift-order hoist's `$liftN` temps — `docs/debugger-spec.md` §3).
    /// The studio's locals view hides these rows; the value is real and
    /// still resolvable by slot for tooling that wants it. Wire: bit 1 of
    /// the row's flags byte (section version 2), `false` for every row a
    /// version-1 writer produced.
    pub synthetic: bool,
}

/// One container's `DebugInfo` table (`docs/debugger-spec.md` §2.2): the
/// `DebugInfo` section's Nth `DebugContainerTable` describes the container
/// at `StoryData::containers[N]` — addressed by the same `container_idx`
/// the runtime's `ContainerPosition` uses, lockstep with the `Containers`
/// section, no `DefinitionId` lookup needed on the read path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugContainerTable {
    /// Sorted ascending by `bytecode_offset`; covers the container's full
    /// address range with no gaps (§2.2's coverage guarantee).
    pub entries: Vec<DebugEntry>,
    /// D7's payload (see [`DebugLocalEntry`]) — empty until D7 lands.
    pub locals: Vec<DebugLocalEntry>,
}

/// The `DebugInfo` section (`docs/debugger-spec.md` §2, `.inkb` tag
/// `0x11`): bytecode-offset → source-range map, plus the section-local file
/// table it's keyed against. Carried on [`crate::StoryData::debug_info`] as
/// `Option` — `None` when not requested (dev/studio compiles and an
/// explicit CLI debug flag opt in; release export never does, §1.2 ship
/// policy) — distinct from the other "always present, possibly empty"
/// section types, because presence here tracks *whether debug info was
/// requested*, not merely whether any entry was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugInfoSection {
    /// Index 0 is always the reserved synthetic sentinel (§2.5).
    pub files: Vec<DebugFileEntry>,
    /// One table per container, in the same order and count as
    /// [`crate::StoryData::containers`].
    pub containers: Vec<DebugContainerTable>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_deterministic() {
        let a = content_hash("Hello, world!");
        let b = content_hash("Hello, world!");
        assert_eq!(a, b);
    }

    // ── #3261: the hash is a wire contract, so pin the DIGESTS ──────────
    //
    // `content_hash_deterministic` above hashes the same string twice in
    // one process. That proves same-run stability — which is NOT the
    // property anything depends on. These hashes are written into
    // artifacts and compared later, possibly by a different binary or
    // toolchain, so what matters is that the algorithm itself never moves.
    // A same-process round trip cannot see an algorithm change; hard-coded
    // digests can, and are the reason `std`'s `DefaultHasher` (documented
    // as unspecified between Rust releases) is no longer used here.

    #[test]
    fn content_hash_matches_the_canonical_fnv_1a_64_vectors() {
        // Published FNV-1a 64-bit test vectors — independent of this
        // implementation, so they prove the ALGORITHM is right rather than
        // merely self-consistent. If these fail, the function is no longer
        // FNV-1a and `docs/format-spec.md` is lying.
        assert_eq!(content_hash("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(content_hash("foobar"), 0x8594_4171_f739_67e8);
        // The empty string is the bare offset basis.
        assert_eq!(content_hash(""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn content_hash_digests_are_pinned_for_representative_source_text() {
        // Including non-ASCII, because brink is a narrative language: em
        // dashes and curly quotes are the normal case, and a change that
        // hashed chars instead of UTF-8 bytes would pass ASCII-only tests.
        assert_eq!(content_hash("Hello, world!"), 0x38d1_3341_4498_7bf4);
        assert_eq!(content_hash("some text"), 0x15b9_e594_d5d3_b704);
        assert_eq!(
            content_hash("The vendor — she of the curly quotes — said \u{201c}no\u{201d}."),
            0x9de0_0913_8091_d4e5
        );
    }

    #[test]
    fn content_hash_distinguishes_texts_that_differ_only_late() {
        // A rolling hash that failed to mix would collide on these; the
        // detector would then miss exactly the small edits it exists for.
        assert_ne!(content_hash("chapter one"), content_hash("chapter onf"));
        assert_ne!(
            content_hash("a long line of prose"),
            content_hash("a long line of prosf")
        );
    }

    #[test]
    fn content_hash_non_zero_for_non_empty() {
        assert_ne!(content_hash("some text"), 0);
        assert_ne!(content_hash("x"), 0);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        assert_ne!(content_hash("hello"), content_hash("world"));
    }
}
