//! Symbolic name-reference + relocation representation for codegen chunks
//! (FG-4b, #808 — `docs/fine-grained-salsa-proposal.md` §5 + the
//! three-resolution-moments appendix).
//!
//! A per-container **chunk** ([`ContainerChunk`]) is the unit
//! [`crate::emit`] produces for one LIR container: its emitted bytecode plus
//! a table of *relocations*. Every name reference codegen must bake into
//! bytecode (today only `PushString`'s `NameId` operand) is written as an
//! unresolved placeholder ([`UNRESOLVED_NAME_ID`]) and recorded as a
//! [`Relocation`] carrying the *symbolic* name ([`NameRef`]) rather than a
//! resolved `NameId`. The assembly/link phase ([`ContainerChunk::link`])
//! resolves each symbol against the assembled story name table and patches
//! the two operand bytes in place.
//!
//! This is compile-link — the first of the three resolution moments in the
//! proposal's appendix — mirroring the runtime's existing symbolic-ref /
//! linker model (decision-log 2026-03-01) one layer earlier. Note the
//! division of labour: the chunk owns only *bytecode* name references
//! (patch sites). A container's own identity strings (its name, its
//! author-path) are assembly-table fields, resolved by the assembler as it
//! builds the story's name table — not chunk relocations.
//!
//! The types are deliberately **serializable-in-principle** — a [`NameRef`]
//! owns its string and a [`Relocation`] is self-contained (a byte offset +
//! a symbol, no transient pointers) — so a future dynamic-linking slice
//! (#717) can ship relocatable chunks without redesigning them. No
//! serialization is implemented here.
//!
//! History-independence (the FG-4d gate this representation exists to
//! satisfy): a chunk's relocation offsets are byte positions in its own
//! bytecode and its `NameRef`s are the source strings — both derived from
//! the container's content, never from allocation history. Resolving them
//! against a name table built in the deterministic container-walk order
//! yields byte-identical output whether the chunk was freshly emitted or
//! (in a future incremental world) re-linked from cache.

use brink_format::{ContainerDef, NameId};

use crate::CodegenError;

/// The placeholder written into a chunk's bytecode at every name-reference
/// operand before linking. `u16::MAX` is never a legitimate resolved
/// `NameId` in the stories this backend emits (name tables stay far below
/// 65 535 entries at game-corpus sizes), so an un-patched site is
/// detectable rather than silently valid.
pub const UNRESOLVED_NAME_ID: u16 = u16::MAX;

/// A symbolic reference to a name, resolved to a final [`NameId`] by the
/// link phase. Owns its string so the record is self-contained (the FG-4
/// appendix's "no transient pointers, serializable in principle").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRef {
    /// The referenced name, identified by its interned string. The content
    /// *is* the address, so this doubles as the content-addressed id the
    /// appendix calls for.
    Symbol(String),
}

impl NameRef {
    /// The referenced symbol's string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            NameRef::Symbol(s) => s,
        }
    }
}

/// A patch site in a chunk's bytecode: the little-endian `u16` `NameId`
/// operand at byte `offset` (currently always a `PushString` operand) must
/// be overwritten with the link-resolved id of [`name`](Self::name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    /// Byte offset of the 2-byte operand within the chunk's own bytecode.
    pub offset: u32,
    /// The symbolic name to resolve and patch in.
    pub name: NameRef,
}

/// One container's codegen output: a [`ContainerDef`] whose bytecode still
/// carries [`UNRESOLVED_NAME_ID`] placeholders at every [`Relocation`] site,
/// plus the relocation table itself. [`link`](Self::link) resolves the
/// symbols and yields a fully-patched `ContainerDef`.
#[derive(Debug, Clone)]
pub struct ContainerChunk {
    /// The container definition. `def.bytecode` holds placeholders at every
    /// relocation offset until [`link`](Self::link) runs.
    pub def: ContainerDef,
    /// Symbolic name-reference patch sites into `def.bytecode`, in emission
    /// order.
    pub relocations: Vec<Relocation>,
}

impl ContainerChunk {
    /// Resolve every relocation against `resolve` and patch the operand
    /// bytes in place, consuming the chunk and returning the linked
    /// [`ContainerDef`].
    ///
    /// `resolve` maps a symbol to its assembled [`NameId`] (a lookup into
    /// the story name table). A miss is an internal invariant violation —
    /// the writer interns every referenced symbol into the name table as it
    /// records the relocation — so it surfaces as a distinct
    /// [`CodegenError`] rather than being silently skipped.
    pub fn link(
        mut self,
        resolve: impl Fn(&str) -> Option<NameId>,
    ) -> Result<ContainerDef, CodegenError> {
        for reloc in &self.relocations {
            let id = resolve(reloc.name.as_str()).ok_or_else(|| {
                CodegenError::new(format!(
                    "codegen link: unresolved name reference {:?} at byte offset {}",
                    reloc.name.as_str(),
                    reloc.offset
                ))
            })?;
            let start = reloc.offset as usize;
            let len = self.def.bytecode.len();
            let slot = self.def.bytecode.get_mut(start..start + 2).ok_or_else(|| {
                CodegenError::new(format!(
                    "codegen link: relocation offset {} out of bounds for chunk bytecode (len {len})",
                    reloc.offset,
                ))
            })?;
            slot.copy_from_slice(&id.0.to_le_bytes());
        }
        Ok(self.def)
    }
}

#[cfg(test)]
mod tests {
    use brink_format::{CountingFlags, DefinitionId, DefinitionTag, Opcode};

    use super::{ContainerChunk, NameRef, Relocation, UNRESOLVED_NAME_ID};

    fn def_id() -> DefinitionId {
        DefinitionId::new(DefinitionTag::Address, 1)
    }

    /// A minimal `ContainerDef` whose bytecode is exactly `bytecode`.
    fn chunk_with(bytecode: Vec<u8>, relocations: Vec<Relocation>) -> ContainerChunk {
        ContainerChunk {
            def: brink_format::ContainerDef {
                id: def_id(),
                scope_id: def_id(),
                name: None,
                bytecode,
                counting_flags: CountingFlags::empty(),
                path_hash: 0,
                param_count: 0,
                params: Vec::new(),
                local: false,
            },
            relocations,
        }
    }

    /// The writer's placeholder → the reader's resolved id: a single
    /// `PushString` site round-trips through `link` to the byte-exact
    /// operand a resolved emit would have produced.
    #[test]
    fn push_string_relocation_round_trips() {
        // Writer: emit `PushString(UNRESOLVED)`; record a relocation at the
        // 2-byte operand.
        let mut bytecode = Vec::new();
        Opcode::PushString(UNRESOLVED_NAME_ID).encode(&mut bytecode);
        let offset = u32::try_from(bytecode.len() - 2).unwrap();
        let chunk = chunk_with(
            bytecode,
            vec![Relocation {
                offset,
                name: NameRef::Symbol("greeting".to_string()),
            }],
        );

        // The chunk is genuinely symbolic before linking: the operand is the
        // placeholder, not a resolved id.
        assert_eq!(
            &chunk.def.bytecode[offset as usize..offset as usize + 2],
            &[0xFF, 0xFF]
        );

        // Reader: resolve "greeting" -> NameId(7) and patch.
        let linked = chunk
            .link(|s| (s == "greeting").then_some(brink_format::NameId(7)))
            .expect("resolvable");

        // Byte-identical to a directly-resolved emit.
        let mut expected = Vec::new();
        Opcode::PushString(7).encode(&mut expected);
        assert_eq!(linked.bytecode, expected);
    }

    /// Multiple relocations in one chunk each patch their own site; an empty
    /// relocation table leaves bytecode untouched.
    #[test]
    fn multiple_and_empty_relocations() {
        let mut bytecode = Vec::new();
        Opcode::PushString(UNRESOLVED_NAME_ID).encode(&mut bytecode);
        let first = u32::try_from(bytecode.len() - 2).unwrap();
        Opcode::PushString(UNRESOLVED_NAME_ID).encode(&mut bytecode);
        let second = u32::try_from(bytecode.len() - 2).unwrap();

        let chunk = chunk_with(
            bytecode,
            vec![
                Relocation {
                    offset: first,
                    name: NameRef::Symbol("a".to_string()),
                },
                Relocation {
                    offset: second,
                    name: NameRef::Symbol("b".to_string()),
                },
            ],
        );
        let linked = chunk
            .link(|s| match s {
                "a" => Some(brink_format::NameId(3)),
                "b" => Some(brink_format::NameId(9)),
                _ => None,
            })
            .expect("resolvable");

        let mut expected = Vec::new();
        Opcode::PushString(3).encode(&mut expected);
        Opcode::PushString(9).encode(&mut expected);
        assert_eq!(linked.bytecode, expected);

        // No relocations => untouched.
        let untouched = chunk_with(vec![0xAB, 0xCD], Vec::new());
        let out = untouched.link(|_| None).expect("no work");
        assert_eq!(out.bytecode, vec![0xAB, 0xCD]);
    }

    /// A symbol the name table cannot resolve is a distinct error, never a
    /// silently-skipped patch.
    #[test]
    fn unresolved_symbol_is_an_error() {
        let mut bytecode = Vec::new();
        Opcode::PushString(UNRESOLVED_NAME_ID).encode(&mut bytecode);
        let offset = u32::try_from(bytecode.len() - 2).unwrap();
        let chunk = chunk_with(
            bytecode,
            vec![Relocation {
                offset,
                name: NameRef::Symbol("missing".to_string()),
            }],
        );
        let err = chunk.link(|_| None).expect_err("should fail");
        assert!(err.to_string().contains("missing"));
    }
}
