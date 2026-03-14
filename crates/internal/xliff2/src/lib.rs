pub mod error;
pub mod model;
pub mod modules;
pub mod read;
pub mod validate;
pub mod write;

pub use error::Xliff2Error;
pub use model::{
    AppliesTo, CanReorder, Content, DataEntry, Document, Ec, Em, ExtensionAttribute,
    ExtensionElement, ExtensionNode, Extensions, File, Group, Ignorable, InlineElement, Mrk, Note,
    OriginalData, Pc, Ph, Sc, Segment, Skeleton, Sm, State, SubUnit, Unit,
};

/// The XLIFF 2.0 core namespace URI.
pub const XLIFF_NS: &str = "urn:oasis:names:tc:xliff:document:2.0";
