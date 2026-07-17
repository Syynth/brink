use crate::model::extensions::{ExtensionElement, ExtensionNode, Extensions};

/// The XLIFF 2.0 Metadata module namespace.
pub const METADATA_NS: &str = "urn:oasis:names:tc:xliff:metadata:2.0";

/// Typed representation of the XLIFF 2.0 Metadata module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub groups: Vec<MetaGroup>,
}

/// A group of metadata entries, optionally categorized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaGroup {
    pub id: Option<String>,
    pub category: Option<String>,
    pub entries: Vec<MetaEntry>,
}

/// A single metadata entry — either a key/value pair or a nested group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaEntry {
    Meta { key: String, value: String },
    Group(MetaGroup),
}

/// Extract typed metadata from generic extension storage.
pub fn extract_metadata(extensions: &Extensions) -> Option<Metadata> {
    let metadata_elem = extensions
        .elements
        .iter()
        .find(|e| e.namespace == METADATA_NS && e.local_name == "metadata")?;

    let groups = metadata_elem
        .children
        .iter()
        .filter_map(|child| match child {
            ExtensionNode::Element(e) if e.local_name == "metaGroup" => Some(parse_meta_group(e)),
            _ => None,
        })
        .collect();

    Some(Metadata { groups })
}

/// Store typed metadata into generic extension storage, replacing any existing metadata.
pub fn set_metadata(extensions: &mut Extensions, meta: Metadata) {
    extensions
        .elements
        .retain(|e| !(e.namespace == METADATA_NS && e.local_name == "metadata"));

    let children = meta
        .groups
        .into_iter()
        .map(|g| ExtensionNode::Element(meta_group_to_element(g)))
        .collect();

    extensions.elements.push(ExtensionElement {
        namespace: METADATA_NS.to_owned(),
        local_name: "metadata".to_owned(),
        attributes: Vec::new(),
        children,
    });
}

fn parse_meta_group(elem: &ExtensionElement) -> MetaGroup {
    let id = elem
        .attributes
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.clone());
    let category = elem
        .attributes
        .iter()
        .find(|(k, _)| k == "category")
        .map(|(_, v)| v.clone());

    let entries = elem
        .children
        .iter()
        .filter_map(|child| match child {
            ExtensionNode::Element(e) if e.local_name == "metaGroup" => {
                Some(MetaEntry::Group(parse_meta_group(e)))
            }
            ExtensionNode::Element(e) if e.local_name == "meta" => {
                let key = e
                    .attributes
                    .iter()
                    .find(|(k, _)| k == "type")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                let value = e
                    .children
                    .iter()
                    .filter_map(|n| match n {
                        ExtensionNode::Text(t) | ExtensionNode::CData(t) => Some(t.as_str()),
                        ExtensionNode::Element(_) => None,
                    })
                    .collect::<String>();
                Some(MetaEntry::Meta { key, value })
            }
            _ => None,
        })
        .collect();

    MetaGroup {
        id,
        category,
        entries,
    }
}

fn meta_group_to_element(group: MetaGroup) -> ExtensionElement {
    let mut attributes = Vec::new();
    if let Some(id) = group.id {
        attributes.push(("id".to_owned(), id));
    }
    if let Some(cat) = group.category {
        attributes.push(("category".to_owned(), cat));
    }

    let children = group
        .entries
        .into_iter()
        .map(|entry| match entry {
            MetaEntry::Meta { key, value } => ExtensionNode::Element(ExtensionElement {
                namespace: METADATA_NS.to_owned(),
                local_name: "meta".to_owned(),
                attributes: vec![("type".to_owned(), key)],
                children: vec![ExtensionNode::Text(value)],
            }),
            MetaEntry::Group(g) => ExtensionNode::Element(meta_group_to_element(g)),
        })
        .collect();

    ExtensionElement {
        namespace: METADATA_NS.to_owned(),
        local_name: "metaGroup".to_owned(),
        attributes,
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_metadata() {
        let meta = Metadata {
            groups: vec![MetaGroup {
                id: Some("g1".to_owned()),
                category: Some("tool".to_owned()),
                entries: vec![
                    MetaEntry::Meta {
                        key: "tool-id".to_owned(),
                        value: "brink".to_owned(),
                    },
                    MetaEntry::Group(MetaGroup {
                        id: None,
                        category: Some("nested".to_owned()),
                        entries: vec![MetaEntry::Meta {
                            key: "version".to_owned(),
                            value: "1.0".to_owned(),
                        }],
                    }),
                ],
            }],
        };

        let mut ext = Extensions::default();
        set_metadata(&mut ext, meta.clone());
        let extracted = extract_metadata(&ext);
        assert_eq!(extracted, Some(meta));
    }

    /// A `<mta:meta>` value delivered as a CDATA section (permitted by the XLIFF 2 spec,
    /// and something external TMS round-trips may produce) must be extracted just like
    /// plain text, not silently dropped.
    #[test]
    fn round_trip_metadata_cdata_value() {
        let ext = Extensions {
            elements: vec![ExtensionElement {
                namespace: METADATA_NS.to_owned(),
                local_name: "metadata".to_owned(),
                attributes: Vec::new(),
                children: vec![ExtensionNode::Element(ExtensionElement {
                    namespace: METADATA_NS.to_owned(),
                    local_name: "metaGroup".to_owned(),
                    attributes: vec![("category".to_owned(), "tool".to_owned())],
                    children: vec![ExtensionNode::Element(ExtensionElement {
                        namespace: METADATA_NS.to_owned(),
                        local_name: "meta".to_owned(),
                        attributes: vec![("type".to_owned(), "tool-id".to_owned())],
                        children: vec![ExtensionNode::CData("brink <tool>".to_owned())],
                    })],
                })],
            }],
            attributes: Vec::new(),
        };

        let extracted = extract_metadata(&ext).expect("metadata element present");
        assert_eq!(
            extracted,
            Metadata {
                groups: vec![MetaGroup {
                    id: None,
                    category: Some("tool".to_owned()),
                    entries: vec![MetaEntry::Meta {
                        key: "tool-id".to_owned(),
                        value: "brink <tool>".to_owned(),
                    }],
                }],
            }
        );
    }

    /// A meta value split across adjacent Text and `CData` nodes (as the reader may
    /// produce when mixed content includes an entity-decoded run alongside a CDATA
    /// section) must be concatenated in document order.
    #[test]
    fn round_trip_metadata_mixed_text_and_cdata_value() {
        let ext = Extensions {
            elements: vec![ExtensionElement {
                namespace: METADATA_NS.to_owned(),
                local_name: "metadata".to_owned(),
                attributes: Vec::new(),
                children: vec![ExtensionNode::Element(ExtensionElement {
                    namespace: METADATA_NS.to_owned(),
                    local_name: "metaGroup".to_owned(),
                    attributes: Vec::new(),
                    children: vec![ExtensionNode::Element(ExtensionElement {
                        namespace: METADATA_NS.to_owned(),
                        local_name: "meta".to_owned(),
                        attributes: vec![("type".to_owned(), "note".to_owned())],
                        children: vec![
                            ExtensionNode::Text("prefix-".to_owned()),
                            ExtensionNode::CData("cdata-part".to_owned()),
                            ExtensionNode::Text("-suffix".to_owned()),
                        ],
                    })],
                })],
            }],
            attributes: Vec::new(),
        };

        let extracted = extract_metadata(&ext).expect("metadata element present");
        assert_eq!(
            extracted,
            Metadata {
                groups: vec![MetaGroup {
                    id: None,
                    category: None,
                    entries: vec![MetaEntry::Meta {
                        key: "note".to_owned(),
                        value: "prefix-cdata-part-suffix".to_owned(),
                    }],
                }],
            }
        );
    }
}
