/// Errors produced by XLIFF 2.0 parsing, writing, and validation.
#[derive(Debug, thiserror::Error)]
pub enum Xliff2Error {
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("XML attribute error: {0}")]
    XmlAttribute(#[from] quick_xml::events::attributes::AttrError),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing required attribute `{attribute}` on <{element}>")]
    MissingAttribute { element: String, attribute: String },

    #[error("invalid value `{value}` for attribute `{attribute}` on <{element}>")]
    InvalidAttribute {
        element: String,
        attribute: String,
        value: String,
    },

    #[error("unexpected element <{found}>, expected <{expected}>")]
    UnexpectedElement { expected: String, found: String },

    #[error("missing required element <{child}> in <{parent}>")]
    MissingElement { parent: String, child: String },

    #[error("unexpected end of document")]
    UnexpectedEof,
}
