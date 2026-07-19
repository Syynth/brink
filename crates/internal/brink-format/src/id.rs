use core::fmt;

use alloc::format;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Tag discriminant stored in the high byte of a [`DefinitionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DefinitionTag {
    Address = 0x01,
    GlobalVar = 0x02,
    ListDef = 0x03,
    ListItem = 0x04,
    ExternalFn = 0x05,
    /// A `STRUCT` shape declaration (TM-4b, `docs/typed-mode-spec.md` §6).
    /// Compiler-side bookkeeping only — the analyzer's `SymbolIndex` needs a
    /// stable `DefinitionId` for a struct name like every other declared
    /// symbol (duplicate detection, goto-def, resolution), but this tag is
    /// never serialized to `.inkb`: the runtime-facing shape identity is the
    /// separate `ShapeId`/`StructShapes` space `brink-format::value` already
    /// reserves, which TM-4c's codegen populates once struct constructs
    /// lower to bytecode. Until then a `StructDef`-tagged id never reaches
    /// the linker.
    StructDef = 0x06,
    /// Params and temps — scoped to a container, not serialized in bytecode.
    LocalVar = 0x07,
}

impl DefinitionTag {
    /// Try to convert a raw `u8` into a known tag.
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Address),
            0x02 => Some(Self::GlobalVar),
            0x03 => Some(Self::ListDef),
            0x04 => Some(Self::ListItem),
            0x05 => Some(Self::ExternalFn),
            0x06 => Some(Self::StructDef),
            0x07 => Some(Self::LocalVar),
            _ => None,
        }
    }
}

/// Mask for the 56-bit hash portion of a definition id.
const HASH_MASK: u64 = (1 << 56) - 1;

/// A tagged 64-bit identifier for any definition in a compiled story.
///
/// Layout: `[tag: 8 bits][hash: 56 bits]`
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefinitionId(u64);

impl DefinitionId {
    /// The well-known cell id of the **`std::rand` RNG state cell** (NS-A6,
    /// `docs/stdlib-spec.md` §7, ruled 2026-07-18): RNG state is a named
    /// runtime state cell, and every draw is an ordinary *write* to it in a
    /// definition's effect row — no new row dimension. This constant is that
    /// cell's name in the `DefinitionId` space shared by the effect-row
    /// machinery (`brink-analyzer`'s harvest, `@[effects(…)]` assertions,
    /// the wake-condition purity gate) and the runtime's ground-truth
    /// recorder (`brink-runtime::effect_trace`).
    ///
    /// The cell is compiler-owned — no source declaration mints it — so its
    /// hash is a fixed, documented constant rather than a content hash. A
    /// collision with a real content-hashed `GlobalVar` would require a
    /// user `VAR`/`CONST` to hash to exactly this 56-bit value (probability
    /// 2⁻⁵⁶ per global; `hash_qualified_name` output is uniform), which is
    /// the same residual risk every pair of user globals already carries.
    ///
    /// The cell's *runtime* representation is not a `Value` slot: it is the
    /// `(rng_seed, previous_random)` pair `ContextAccess` has always carried
    /// (and saves have always round-tripped). This id names that state for
    /// the effect system; it never appears in a global table.
    pub const RNG_CELL: DefinitionId =
        DefinitionId(((DefinitionTag::GlobalVar as u64) << 56) | 0x00_5EED_0000_D1CE);

    /// Create a new id from a tag and a 56-bit hash.
    ///
    /// The hash is masked to 56 bits — upper bits are silently discarded.
    pub fn new(tag: DefinitionTag, hash: u64) -> Self {
        let raw = (u64::from(tag as u8) << 56) | (hash & HASH_MASK);
        Self(raw)
    }

    /// Extract the tag byte.
    pub fn tag(self) -> DefinitionTag {
        // SAFETY-equivalent: we only construct from known tags, so the unwrap
        // below is always valid. We use `unwrap_or` to satisfy the lint.
        let byte = (self.0 >> 56) as u8;
        // This should never fail for a validly-constructed id.
        DefinitionTag::from_u8(byte).unwrap_or(DefinitionTag::Address)
    }

    /// Extract the 56-bit hash.
    pub fn hash(self) -> u64 {
        self.0 & HASH_MASK
    }

    /// Return the raw `u64` representation.
    pub fn to_raw(self) -> u64 {
        self.0
    }

    /// Reconstruct from a raw `u64`, returning `None` if the tag byte is
    /// invalid.
    pub fn from_raw(raw: u64) -> Option<Self> {
        let byte = (raw >> 56) as u8;
        DefinitionTag::from_u8(byte)?;
        Some(Self(raw))
    }
}

impl Serialize for DefinitionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Serialize as "$tt_hhhhhhhhhhhhhh" — tag byte + 56-bit hash.
        serializer.serialize_str(&format!("{self}"))
    }
}

impl<'de> Deserialize<'de> for DefinitionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        // Parse "$tt_hhhhhhhhhhhhhh"
        if !s.starts_with('$') || s.len() != 18 || s.as_bytes()[3] != b'_' {
            return Err(serde::de::Error::custom(format!(
                "invalid DefinitionId: {s:?}"
            )));
        }
        let tag_byte = u8::from_str_radix(&s[1..3], 16).map_err(serde::de::Error::custom)?;
        let tag = DefinitionTag::from_u8(tag_byte).ok_or_else(|| {
            serde::de::Error::custom(format!("invalid tag byte: {tag_byte:#04x}"))
        })?;
        let hash = u64::from_str_radix(&s[4..], 16).map_err(serde::de::Error::custom)?;
        Ok(Self::new(tag, hash))
    }
}

impl fmt::Display for DefinitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${:02x}_{:014x}", self.tag() as u8, self.hash())
    }
}

impl fmt::Debug for DefinitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}({:#014x})", self.tag(), self.hash())
    }
}

/// An index into the story name table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NameId(pub u16);

/// A reference to a specific line within a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineId {
    pub container: DefinitionId,
    pub index: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_raw() {
        let id = DefinitionId::new(DefinitionTag::Address, 0xDEAD_BEEF);
        let raw = id.to_raw();
        let recovered = DefinitionId::from_raw(raw).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn tag_extraction() {
        for tag in [
            DefinitionTag::Address,
            DefinitionTag::GlobalVar,
            DefinitionTag::ListDef,
            DefinitionTag::ListItem,
            DefinitionTag::ExternalFn,
            DefinitionTag::StructDef,
            DefinitionTag::LocalVar,
        ] {
            let id = DefinitionId::new(tag, 42);
            assert_eq!(id.tag(), tag);
        }
    }

    #[test]
    fn struct_def_tag_roundtrips_through_from_u8() {
        assert_eq!(DefinitionTag::from_u8(0x06), Some(DefinitionTag::StructDef));
    }

    #[test]
    fn hash_masking() {
        // High bits beyond 56 should be discarded.
        let id = DefinitionId::new(DefinitionTag::ListDef, u64::MAX);
        assert_eq!(id.hash(), HASH_MASK);
        assert_eq!(id.tag(), DefinitionTag::ListDef);
    }

    #[test]
    fn invalid_tag_rejection() {
        // Forge a raw value with tag byte 0x00.
        let raw = 0x00_DEAD_BEEF_CAFE_u64;
        assert!(DefinitionId::from_raw(raw).is_none());

        // Tag byte 0xFF is also invalid.
        let raw = 0xFF_0000_0000_0000_u64;
        assert!(DefinitionId::from_raw(raw).is_none());
    }

    #[test]
    fn debug_format() {
        let id = DefinitionId::new(DefinitionTag::ExternalFn, 0xCAFE);
        let s = format!("{id:?}");
        assert!(s.contains("ExternalFn"));
        assert!(s.contains("0x"));
    }

    #[test]
    fn rng_cell_is_a_well_formed_global_var_id() {
        // NS-A6: the well-known `std::rand` cell id must be a valid,
        // round-trippable `GlobalVar`-tagged id — it flows through the same
        // effect-row wire section (`EffectRows`) as content-hashed ids.
        let id = DefinitionId::RNG_CELL;
        assert_eq!(id.tag(), DefinitionTag::GlobalVar);
        assert_eq!(id.hash(), 0x00_5EED_0000_D1CE);
        assert_eq!(DefinitionId::from_raw(id.to_raw()), Some(id));
        assert_eq!(
            id,
            DefinitionId::new(DefinitionTag::GlobalVar, 0x00_5EED_0000_D1CE)
        );
    }

    #[test]
    fn line_id_equality() {
        let c = DefinitionId::new(DefinitionTag::Address, 1);
        let a = LineId {
            container: c,
            index: 0,
        };
        let b = LineId {
            container: c,
            index: 0,
        };
        assert_eq!(a, b);
    }
}
