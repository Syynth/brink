use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag, NameId};
use rowan::TextRange;

use crate::FileId;
use crate::determinism::{LookupMap, LookupSet};
use crate::symbols::{ResolutionMap, SymbolIndex, SymbolInfo};

use super::structs::{GlobalShapeMap, ShapeTable};

// ─── Resolution lookup ──────────────────────────────────────────────

/// O(1) lookup from `(FileId, TextRange)` to the resolved `DefinitionId`.
pub struct ResolutionLookup {
    map: LookupMap<(FileId, TextRange), DefinitionId>,
}

impl ResolutionLookup {
    pub fn build(resolutions: &ResolutionMap) -> Self {
        let map = resolutions
            .iter()
            .map(|r| ((r.file, r.range), r.target))
            .collect();
        Self { map }
    }

    pub fn resolve(&self, file: FileId, range: TextRange) -> Option<DefinitionId> {
        self.map.get(&(file, range)).copied()
    }
}

// ─── Name table ─────────────────────────────────────────────────────

/// Intern strings to `NameId`. Deduplicates identical strings.
pub struct NameTable {
    map: LookupMap<String, NameId>,
    entries: Vec<String>,
}

impl NameTable {
    pub fn new() -> Self {
        Self {
            map: LookupMap::new(),
            entries: Vec::new(),
        }
    }

    pub fn intern(&mut self, name: &str) -> NameId {
        if let Some(&id) = self.map.get(name) {
            return id;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "name table won't exceed u16::MAX"
        )]
        let id = NameId(self.entries.len() as u16);
        self.entries.push(name.to_string());
        self.map.insert(name.to_string(), id);
        id
    }

    pub fn into_entries(self) -> Vec<String> {
        self.entries
    }

    /// Rebuild a table pre-seeded with `entries` (first-occurrence order),
    /// so subsequent [`intern`](Self::intern) calls dedup against them and
    /// append after. The inverse of [`into_entries`](Self::into_entries) —
    /// the FG-4d link phase captures the decl+struct seed as a `Vec<String>`
    /// (a cutoff-friendly, `Eq`-able value) and reconstitutes the table here
    /// before merging the per-chunk local names, so the assembled ids are
    /// byte-identical to the single shared-table walk.
    pub fn from_entries(entries: Vec<String>) -> Self {
        let map = entries
            .iter()
            .enumerate()
            .map(|(i, s)| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "name table won't exceed u16::MAX"
                )]
                (s.clone(), NameId(i as u16))
            })
            .collect();
        Self { map, entries }
    }
}

/// The root container's `DefinitionId` — the hash-of-empty-path address every
/// lowering phase (prelude, per-chunk, assembly) uses as the `root_id`
/// fallback. Content-derived (a fixed hash), so a fresh [`IdAllocator`] in a
/// per-chunk salsa memo yields the same value as the whole-project walk
/// (FG-4d history-independence — `docs/fine-grained-salsa-proposal.md`
/// appendix).
#[must_use]
pub fn root_definition_id() -> DefinitionId {
    IdAllocator::new().alloc_address("")
}

// ─── Id allocator ───────────────────────────────────────────────────

/// Allocates new `DefinitionId`s for containers not in the symbol index
/// (root, choice targets, unlabeled gathers).
pub struct IdAllocator {
    used: LookupMap<String, DefinitionId>,
    /// Global counter for conditionals and sequences. Never resets —
    /// shared between the plan phase and lowering phase to ensure
    /// unique container paths across all sub-scopes.
    seq_counter: usize,
}

impl IdAllocator {
    pub fn new() -> Self {
        Self {
            used: LookupMap::new(),
            seq_counter: 0,
        }
    }

    /// Allocate an address id from a path string (e.g. `""`, `"knot.c0"`).
    pub fn alloc_address(&mut self, path: &str) -> DefinitionId {
        if let Some(&id) = self.used.get(path) {
            return id;
        }
        let hash = hash_path(path);
        let id = DefinitionId::new(DefinitionTag::Address, hash);
        self.used.insert(path.to_string(), id);
        id
    }

    /// Allocate the next sequential index for a conditional or sequence scope.
    /// This counter never resets when entering sub-scopes within a knot/stitch,
    /// ensuring unique paths like `b-0`, `b-1`, etc. It resets at knot/stitch
    /// boundaries via [`reset_seq_counter`], since scope paths are qualified
    /// by knot name (e.g., `"start.b-0"` can't collide with `"waited.b-0"`).
    pub fn next_seq_index(&mut self) -> usize {
        let idx = self.seq_counter;
        self.seq_counter += 1;
        idx
    }

    /// Reset the sequential index counter. Called at knot/stitch boundaries
    /// where the scope path prefix changes.
    pub fn reset_seq_counter(&mut self) {
        self.seq_counter = 0;
    }
}

/// Hash a path string using `DefaultHasher`, matching the converter/linker convention.
///
/// Collisions between container IDs and other definition types are already
/// impossible because `DefinitionId` encodes the tag in its top 8 bits.
fn hash_path(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

// ─── TM-4c struct-shape context ─────────────────────────────────────

/// The project's `types` policy, as seen by LIR lowering (TM-4c,
/// `docs/typed-mode-spec.md` §6/§1). A local, minimal mirror of
/// `brink-analyzer`'s `TypePolicy` — `brink-ir` sits *below*
/// `brink-analyzer` in the crate graph (`brink-analyzer` depends on
/// `brink-ir`, never the reverse), so it cannot name that type directly;
/// `brink-db`'s `lir_query` maps `TypePolicy` to this enum at the one call
/// site that threads it into [`lower_to_program`](super::lower_to_program).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TypeMode {
    #[default]
    Gradual,
    Strict,
}

/// Whole-program struct-shape data, built once before any container is
/// lowered and shared (read-only) by every [`LowerCtx`] — see
/// `lower::structs`' module doc for what each field is and the soundness
/// argument for why [`TypeMode::Strict`] gates static-offset emission.
pub struct StructCtx<'a> {
    pub shapes: &'a ShapeTable,
    pub global_shapes: &'a GlobalShapeMap,
    pub type_mode: TypeMode,
}

// ─── Lower context ──────────────────────────────────────────────────

/// Shared context threaded through all lowering functions.
pub struct LowerCtx<'a> {
    pub file: FileId,
    pub resolutions: &'a ResolutionLookup,
    pub index: &'a SymbolIndex,
    pub temps: &'a TempMap,
    pub names: &'a mut NameTable,
    pub ids: &'a mut IdAllocator,
    /// Current container path prefix (e.g. `"knot"`, `"knot.stitch"`).
    pub scope_path: String,
    /// Child containers created during content lowering (inline sequences).
    /// Drained by the caller after each statement.
    pub pending_children: Vec<super::lir::Container>,
    /// Temps that have been declared so far in source order.
    /// Forward-referenced temps (used before declaration) should resolve as
    /// globals, matching inklecate's behavior.
    pub visible_temps: LookupSet<String>,
    /// Mapping from `FileId` to source file path, for populating `SourceLocation`.
    pub file_paths: &'a LookupMap<FileId, String>,
    /// The root container ID — used as fallback when a stamped container ID
    /// is missing (should not happen in well-formed HIR).
    pub root_id: brink_format::DefinitionId,
    /// The gather target of the enclosing choice set, if any. Set when
    /// lowering a choice body so that labeled containers within it can
    /// include an explicit `goto gather` — making them self-sufficient
    /// regardless of whether they're entered via `enter_container`
    /// (structured) or `goto` (ink divert).
    pub choice_gather_target: Option<brink_format::DefinitionId>,
    /// Next free temp slot for T1b block-scoped locals (explicit `temp`
    /// declarations inside `~ { … }`, `for` loop variables, and
    /// compiler-synthesized temps for indexed-assignment/for-loop
    /// desugaring). Shared by mutable reference across an entire frame
    /// (a knot body + all its stitches, or the whole root scope across
    /// files) — the same threading discipline as `ids` — so two block
    /// scopes that share a call frame never collide on a slot number.
    /// Seeded to the classic `TempMap`'s `total_slots()` for that frame.
    pub next_block_slot: &'a mut u16,
    /// Stack of open `~ { … }` lexical scopes. Each frame holds
    /// `(name, slot)` pairs for locals declared in that scope, innermost
    /// last. [`LowerCtx::temp_slot`] searches this stack (innermost first)
    /// before falling back to the classic flat `temps` map, so a
    /// block-scoped `temp` correctly shadows an outer temp of the same
    /// name (docs/t1b-surface-spec.md §2) without disturbing the outer
    /// slot's storage.
    pub block_scopes: Vec<Vec<(String, u16)>>,
    /// Every name ever declared via [`LowerCtx::declare_block_local`] in
    /// this frame — i.e. every T1b block-scoped `temp`/`for`-loop-variable
    /// name, whether or not its `~ { … }` block is still open. Unlike
    /// `block_scopes`, entries are never removed on `pop_block_scope`.
    ///
    /// Distinguishes, at the point `lower_path`/`lower_call_args` fall
    /// through to the "temp not currently visible" case, a genuine classic
    /// (non-block) forward reference (used before its declaring `~ temp`
    /// statement — inklecate-compat: emits a phantom `get_global` that
    /// faults at link time) from a block-scoped temp referenced *after* its
    /// own block has already closed (#680 RCA) — the latter is a real
    /// authoring error and gets its own diagnostic (E082) instead of
    /// silently resolving to the wrong global slot.
    pub block_scoped_temp_names: LookupSet<String>,
    /// Diagnostics produced during lowering — historically just warnings
    /// (the T1b block-scoped-temp shadow warning, E054), but also
    /// Error-severity ones now (E055/E056 mutator checks, E057
    /// break/continue-outside-loop, E058 mutator arity). Severity is read
    /// from `DiagnosticCode::severity()`, not from which list a diagnostic
    /// was pushed to — `brink-db`'s `lir_query` partitions this Vec by
    /// severity and refuses to hand back a `Program` (gates `program:
    /// None`, bypassing `// brink-disable-all` suppression entirely, unlike
    /// analysis-phase diagnostics) when any Error-severity one is present,
    /// so pushing here is a real, non-suppressible compile error, not a
    /// cosmetic note. Shared by mutable reference across an entire
    /// `lower_to_program` call — the same threading discipline as
    /// `ids`/`next_block_slot` — and returned alongside the program.
    pub diagnostics: &'a mut Vec<crate::Diagnostic>,
    /// Depth of `while`/`for` loop nesting the current T1b block-statement
    /// lowering position is inside — incremented/decremented around
    /// `while`/`for` body lowering in `blocks::lower_block_stmt`. Zero
    /// outside any loop; used to reject `break`/`continue` at depth 0
    /// (E057) instead of emitting an unguarded `LogicBreak`/`LogicContinue`
    /// that codegen has no jump target for (see #577 review).
    pub loop_depth: u32,
    /// TM-4c (`docs/typed-mode-spec.md` §6): the whole-program struct-shape
    /// data (shape table, `types` policy, global struct-typed VAR/CONST
    /// annotations) — shared, read-only, identical across every `LowerCtx`
    /// in a single `lower_to_program` call.
    pub structs: &'a StructCtx<'a>,
    /// TM-4c: temp slots (this frame only — reset per `LowerCtx`, exactly
    /// like `visible_temps`) whose declaring `TempDecl`'s TM-2 annotation
    /// names a declared struct — the temp-local half of the "compile-time
    /// known shape" story `expr::known_shape` chases (`structs::
    /// GlobalShapeMap` is the global half). Keyed by slot, not name, so a
    /// block-scoped shadow of an outer temp of the same name still maps to
    /// its own (correct) shape.
    pub temp_shapes: LookupMap<u16, String>,
}

impl<'a> LowerCtx<'a> {
    /// Resolve a HIR path at the given range. Returns the resolved `SymbolInfo`.
    pub fn resolve_path(&self, range: TextRange) -> Option<&'a SymbolInfo> {
        let id = self.resolutions.resolve(self.file, range)?;
        self.index.symbols.get(&id)
    }

    /// Resolve a HIR path to its `DefinitionId`.
    pub fn resolve_id(&self, range: TextRange) -> Option<DefinitionId> {
        self.resolutions.resolve(self.file, range)
    }

    /// Look up a name in the temp map for the current scope.
    /// Only returns a slot if the temp has been declared (is visible).
    ///
    /// Checks open T1b block scopes first (innermost frame first, most
    /// recent declaration within a frame first) so a block-scoped `temp`
    /// correctly shadows an outer temp of the same name; outside any block
    /// (`block_scopes` empty) this is exactly the pre-T1b lookup.
    pub fn temp_slot(&self, name: &str) -> Option<u16> {
        if let Some(slot) = self.lookup_block_local(name) {
            return Some(slot);
        }
        if self.visible_temps.contains(name) {
            self.temps.get(name)
        } else {
            None
        }
    }

    /// Search the open block-scope stack for `name`, innermost first.
    fn lookup_block_local(&self, name: &str) -> Option<u16> {
        for frame in self.block_scopes.iter().rev() {
            if let Some(&(_, slot)) = frame.iter().rev().find(|(n, _)| n == name) {
                return Some(slot);
            }
        }
        None
    }

    /// Open a new T1b lexical block scope (`~ { … }`, or an `if`/`while`/
    /// `for` body nested within one). Must be paired with
    /// [`pop_block_scope`](Self::pop_block_scope).
    pub fn push_block_scope(&mut self) {
        self.block_scopes.push(Vec::new());
    }

    /// Close the innermost T1b lexical block scope. Locals declared in it
    /// stop shadowing once popped.
    pub fn pop_block_scope(&mut self) {
        self.block_scopes.pop();
    }

    /// Allocate a fresh temp slot for a T1b block-scoped local (explicit or
    /// compiler-synthesized). Never reused/deduped by name — that's exactly
    /// what makes shadowing safe.
    pub fn alloc_block_slot(&mut self) -> u16 {
        let slot = *self.next_block_slot;
        *self.next_block_slot += 1;
        slot
    }

    /// Whether `name` is already visible as a temp/param — either an open
    /// T1b block scope (innermost first) or the classic flat scope (knot/
    /// stitch params + `~ temp` declarations seen so far in source order).
    /// The shadow-warning check (`docs/t1b-surface-spec.md` §2, E054) uses
    /// this: declaring a block-scoped `temp` with a name that's already
    /// visible — from an enclosing block *or* an outer classic temp — is a
    /// shadow.
    pub fn is_name_visible(&self, name: &str) -> bool {
        self.lookup_block_local(name).is_some() || self.visible_temps.contains(name)
    }

    /// Bind `name` to `slot` in the innermost open block scope.
    ///
    /// Every T1b local is declared while lowering a `~ { … }`/`if`/`while`/
    /// `for` body, all of which push a scope first, so `block_scopes` is
    /// never empty in practice; self-heals (opens a scope) rather than
    /// using a denied `expect`/`unwrap` if that invariant is ever violated.
    pub fn declare_block_local(&mut self, name: String, slot: u16) {
        self.block_scoped_temp_names.insert(name.clone());
        if self.block_scopes.is_empty() {
            self.block_scopes.push(Vec::new());
        }
        if let Some(frame) = self.block_scopes.last_mut() {
            frame.push((name, slot));
        }
    }

    /// Look up a temp slot by name, bypassing visibility checks.
    /// Used for `DeclareTemp` lowering where the slot must exist even
    /// though the temp hasn't been marked visible yet.
    pub fn temp_slot_raw(&self, name: &str) -> Option<u16> {
        self.temps.get(name)
    }

    /// TM-4c: record that temp `slot` was declared with a TM-2 annotation
    /// naming struct shape `shape_name`.
    pub fn set_temp_shape(&mut self, slot: u16, shape_name: String) {
        self.temp_shapes.insert(slot, shape_name);
    }

    /// TM-4c: if `annotation` is a `Named` type naming a declared struct,
    /// record it as `slot`'s known shape (`set_temp_shape`) — the one call
    /// every `TempDecl` lowering site (classic and block-scoped) makes right
    /// after allocating/resolving the temp's own slot, so `expr::known_shape`
    /// can later chase a struct-typed `temp`'s reads/writes to a static
    /// offset under `types = strict`.
    pub fn record_temp_annotation(&mut self, slot: u16, annotation: Option<&crate::hir::TypeExpr>) {
        if let Some(crate::hir::TypeExpr::Named { name, .. }) = annotation
            && self.structs.shapes.get(name).is_some()
        {
            self.set_temp_shape(slot, name.clone());
        }
    }

    /// TM-4c: the declared struct shape name for temp `slot`, if its
    /// `TempDecl` carried a struct-typed TM-2 annotation.
    pub fn temp_shape(&self, slot: u16) -> Option<&str> {
        self.temp_shapes.get(&slot).map(String::as_str)
    }

    /// TM-4c: the declared struct shape name for a resolved global `VAR`/
    /// `CONST`, if its TM-2 annotation named a declared struct.
    pub fn global_shape(&self, id: DefinitionId) -> Option<&str> {
        self.structs.global_shapes.get(&id).map(String::as_str)
    }

    /// Qualify a label name with the current scope path.
    pub fn qualify_label(&self, label: &str) -> String {
        if self.scope_path.is_empty() {
            label.to_string()
        } else {
            format!("{}.{label}", self.scope_path)
        }
    }

    /// Allocate a `DefinitionId` for a sequence wrapper container.
    pub fn alloc_sequence_id(&mut self, counter: usize) -> DefinitionId {
        let path = if self.scope_path.is_empty() {
            format!("s-{counter}")
        } else {
            format!("{}.s-{counter}", self.scope_path)
        };
        self.ids.alloc_address(&path)
    }

    /// Look up an address `DefinitionId` by qualifying a label name with the current scope.
    pub fn lookup_address_id(&self, label: &str) -> Option<DefinitionId> {
        let qualified = self.qualify_label(label);
        self.index
            .by_name
            .get(&qualified)
            .and_then(|ids| ids.first())
            .copied()
    }
}

// ─── Temp map ───────────────────────────────────────────────────────

/// Per-scope temp variable slot assignments.
#[derive(Debug, Clone, Default)]
pub struct TempMap {
    slots: LookupMap<String, u16>,
}

impl TempMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: String, slot: u16) {
        self.slots.insert(name, slot);
    }

    pub fn get(&self, name: &str) -> Option<u16> {
        self.slots.get(name).copied()
    }

    pub fn total_slots(&self) -> u16 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "temp count won't exceed u16::MAX"
        )]
        {
            self.slots.len() as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::ResolvedRef;

    #[test]
    fn name_table_deduplication() {
        let mut table = NameTable::new();
        let a = table.intern("hello");
        let b = table.intern("world");
        let c = table.intern("hello");
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(table.into_entries(), vec!["hello", "world"]);
    }

    #[test]
    fn resolution_lookup() {
        let refs = vec![ResolvedRef {
            file: FileId(0),
            range: TextRange::new(10.into(), 15.into()),
            target: DefinitionId::new(DefinitionTag::Address, 42),
        }];
        let lookup = ResolutionLookup::build(&refs);
        assert_eq!(
            lookup.resolve(FileId(0), TextRange::new(10.into(), 15.into())),
            Some(DefinitionId::new(DefinitionTag::Address, 42))
        );
        assert_eq!(
            lookup.resolve(FileId(1), TextRange::new(10.into(), 15.into())),
            None
        );
    }

    #[test]
    fn id_allocator_stable() {
        let mut alloc = IdAllocator::new();
        let a = alloc.alloc_address("knot.c0");
        let b = alloc.alloc_address("knot.c0");
        assert_eq!(a, b);
        let c = alloc.alloc_address("knot.c1");
        assert_ne!(a, c);
    }

    #[test]
    fn temp_map_slots() {
        let mut map = TempMap::new();
        map.insert("x".to_string(), 0);
        map.insert("y".to_string(), 1);
        assert_eq!(map.get("x"), Some(0));
        assert_eq!(map.get("y"), Some(1));
        assert_eq!(map.get("z"), None);
        assert_eq!(map.total_slots(), 2);
    }
}
