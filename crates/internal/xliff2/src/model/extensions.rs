/// Generic storage for XLIFF 2.0 extension elements and attributes.
///
/// The XLIFF 2.0 spec allows extension elements and attributes (from non-XLIFF namespaces)
/// on virtually every element. This type preserves them for round-trip fidelity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extensions {
    pub elements: Vec<ExtensionElement>,
    pub attributes: Vec<ExtensionAttribute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionAttribute {
    pub namespace: String,
    pub local_name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionElement {
    pub namespace: String,
    pub local_name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<ExtensionNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionNode {
    Element(ExtensionElement),
    Text(String),
}
