use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use brink_format::{DefinitionId, DefinitionTag, NameId};
use rowan::TextRange;

use crate::FileId;
use crate::symbols::{ResolutionMap, SymbolIndex, SymbolInfo};

// ─── Resolution lookup ──────────────────────────────────────────────

/// O(1) lookup from `(FileId, TextRange)` to the resolved `DefinitionId`.
pub struct ResolutionLookup {
    map: HashMap<(FileId, TextRange), DefinitionId>,
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
    map: HashMap<String, NameId>,
    entries: Vec<String>,
}

impl NameTable {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
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
}

// ─── Id allocator ───────────────────────────────────────────────────

/// Allocates new `DefinitionId`s for containers not in the symbol index
/// (root, choice targets, unlabeled gathers).
pub struct IdAllocator {
    used: HashMap<String, DefinitionId>,
}

impl IdAllocator {
    pub fn new() -> Self {
        Self {
            used: HashMap::new(),
        }
    }

    /// Allocate a container id from a path string (e.g. `""`, `"knot.c0"`).
    pub fn alloc_container(&mut self, path: &str) -> DefinitionId {
        if let Some(&id) = self.used.get(path) {
            return id;
        }
        let hash = hash_path(path);
        let id = DefinitionId::new(DefinitionTag::Container, hash);
        self.used.insert(path.to_string(), id);
        id
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
    pub visible_temps: std::collections::HashSet<String>,
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
    pub fn temp_slot(&self, name: &str) -> Option<u16> {
        if self.visible_temps.contains(name) {
            self.temps.get(name)
        } else {
            None
        }
    }

    /// Look up a temp slot by name, bypassing visibility checks.
    /// Used for `DeclareTemp` lowering where the slot must exist even
    /// though the temp hasn't been marked visible yet.
    pub fn temp_slot_raw(&self, name: &str) -> Option<u16> {
        self.temps.get(name)
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
        self.ids.alloc_container(&path)
    }

    /// Look up a label's `DefinitionId` by qualifying it with the current scope.
    pub fn lookup_label_id(&self, label: &str) -> Option<DefinitionId> {
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
    slots: HashMap<String, u16>,
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
            target: DefinitionId::new(DefinitionTag::Container, 42),
        }];
        let lookup = ResolutionLookup::build(&refs);
        assert_eq!(
            lookup.resolve(FileId(0), TextRange::new(10.into(), 15.into())),
            Some(DefinitionId::new(DefinitionTag::Container, 42))
        );
        assert_eq!(
            lookup.resolve(FileId(1), TextRange::new(10.into(), 15.into())),
            None
        );
    }

    #[test]
    fn id_allocator_stable() {
        let mut alloc = IdAllocator::new();
        let a = alloc.alloc_container("knot.c0");
        let b = alloc.alloc_container("knot.c0");
        assert_eq!(a, b);
        let c = alloc.alloc_container("knot.c1");
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
