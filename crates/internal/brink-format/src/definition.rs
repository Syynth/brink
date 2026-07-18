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

/// Compute a deterministic hash of line content text.
///
/// Used by both the compiler codegen and the converter to populate
/// [`LineEntry::source_hash`]. The hash detects when source text has
/// changed across builds, enabling the regeneration workflow in the
/// internationalization pipeline.
pub fn content_hash(text: &str) -> u64 {
    #[cfg(feature = "std")]
    {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
    #[cfg(not(feature = "std"))]
    {
        // `std::collections::hash_map::DefaultHasher` isn't available
        // without `std`. This is a plain FNV-1a fallback: still
        // deterministic, but NOT bit-identical to the `std` path above —
        // nothing compares hashes produced by the two builds against each
        // other, so that's fine.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}

/// An externally-bound function definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalFnDef {
    pub id: DefinitionId,
    pub name: NameId,
    pub arg_count: u8,
    pub fallback: Option<DefinitionId>,
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
