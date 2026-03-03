use core::fmt;

/// Tag discriminant stored in the high byte of a [`DefinitionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DefinitionTag {
    Container = 0x01,
    GlobalVar = 0x02,
    ListDef = 0x03,
    ListItem = 0x04,
    ExternalFn = 0x05,
    Label = 0x06,
}

impl DefinitionTag {
    /// Try to convert a raw `u8` into a known tag.
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Container),
            0x02 => Some(Self::GlobalVar),
            0x03 => Some(Self::ListDef),
            0x04 => Some(Self::ListItem),
            0x05 => Some(Self::ExternalFn),
            0x06 => Some(Self::Label),
            _ => None,
        }
    }
}

/// Mask for the 56-bit hash portion of a definition id.
const HASH_MASK: u64 = (1 << 56) - 1;

/// A tagged 64-bit identifier for any definition in a compiled story.
///
/// Layout: `[tag: 8 bits][hash: 56 bits]`
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefinitionId(u64);

impl DefinitionId {
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
        DefinitionTag::from_u8(byte).unwrap_or(DefinitionTag::Container)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        let id = DefinitionId::new(DefinitionTag::Container, 0xDEAD_BEEF);
        let raw = id.to_raw();
        let recovered = DefinitionId::from_raw(raw).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn tag_extraction() {
        for tag in [
            DefinitionTag::Container,
            DefinitionTag::GlobalVar,
            DefinitionTag::ListDef,
            DefinitionTag::ListItem,
            DefinitionTag::ExternalFn,
        ] {
            let id = DefinitionId::new(tag, 42);
            assert_eq!(id.tag(), tag);
        }
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
    fn line_id_equality() {
        let c = DefinitionId::new(DefinitionTag::Container, 1);
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
