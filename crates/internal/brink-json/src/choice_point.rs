use bitflags::bitflags;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::Path;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Hash, Eq, Ord)]
    pub struct ChoicePointFlags: u32 {
        /// `HasCondition` 0x1: Set if the story should pop a value from the
        /// evaluation stack in order to determine whether a choice instance
        /// should be created at all.
        const HAS_CONDITION = 0b0000_0001;

        /// `HasStartContent` 0x2: According to square bracket notation, is there
        /// any leading content before any square brackets? If so, this
        /// content should be popped from the evaluation stack.
        const HAS_START_CONTENT = 0b0000_0010;

        /// `HasChoiceOnlyContent` 0x4: According to square bracket notation,
        /// is there any content after the choice text? If so, this content
        /// should be popped from the evaluation stack.
        const HAS_CHOICE_ONLY_CONTENT = 0b0000_0100;

        /// `IsInvisibleDefault` 0x8: When this is enabled, the choice isn't
        /// provided to the game (isn't presented to the player), and instead
        /// is automatically followed if there are no other choices generated.
        const IS_INVISIBLE_DEFAULT = 0b0000_1000;

        /// `OnceOnly` 0x10: Defaults to true. This is the difference between
        /// the * and + choice bullets in ink. If once only (*), the choice is
        /// only displayed if its target container's read count is zero.
        const ONCE_ONLY = 0b0001_0000;
    }
}

/// Generates an instance of a Choice. Its exact behaviour depends on its
/// flags. It doesn't contain any text itself, since choice text is generated
/// at runtime and added to the evaluation stack. When a `ChoicePoint` is
/// encountered, it pops content off the evaluation stack according to its
/// flags, which indicate which texts are needed.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoicePoint {
    /// The path when chosen is the target path of a Container of content, and
    /// is assigned when calling `ChooseChoiceIndex`.
    pub target: Path,

    /// The flags indicating the choice's behavior
    pub flags: ChoicePointFlags,
}

impl Serialize for ChoicePoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("*", &self.target)?;
        map.serialize_entry("flg", &self.flags.bits())?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for ChoicePoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ChoicePointHelper {
            #[serde(rename = "*")]
            target: String,
            #[serde(rename = "flg")]
            flags: u32,
        }

        let helper = ChoicePointHelper::deserialize(deserializer)?;
        Ok(ChoicePoint {
            target: helper.target,
            flags: ChoicePointFlags::from_bits_truncate(helper.flags),
        })
    }
}
