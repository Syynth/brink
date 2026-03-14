use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::events::attributes::Attributes;

use crate::error::Xliff2Error;
use crate::model::extensions::{ExtensionAttribute, ExtensionElement, ExtensionNode, Extensions};

/// Collect an unknown attribute into extension storage.
/// Stores the full qualified name (e.g. `my:attr`, `xmlns:mtc`) as-is for round-trip fidelity.
pub fn collect_ext_attribute(key: &str, val: &str, ext: &mut Extensions) {
    if let Some(pos) = key.find(':') {
        let ns = &key[..pos];
        let local = &key[pos + 1..];
        ext.attributes.push(ExtensionAttribute {
            namespace: ns.to_owned(),
            local_name: local.to_owned(),
            value: val.to_owned(),
        });
    }
    // Non-namespaced unknown attributes are silently ignored (per XLIFF spec,
    // extension attributes must be namespace-qualified).
}

/// Read a full extension element (Start event already consumed) into extensions.
pub fn read_ext_element_into(
    local_name: &str,
    attrs: &Attributes,
    reader: &mut Reader<&[u8]>,
    ext: &mut Extensions,
) -> Result<(), Xliff2Error> {
    let element = read_ext_element(local_name, attrs, reader)?;
    ext.elements.push(element);
    Ok(())
}

/// Collect an empty extension element into extensions.
pub fn collect_empty_ext_element(
    local_name: &str,
    attrs: &Attributes,
    ext: &mut Extensions,
) -> Result<(), Xliff2Error> {
    let attributes = read_ext_attrs(attrs)?;
    ext.elements.push(ExtensionElement {
        namespace: String::new(),
        local_name: local_name.to_owned(),
        attributes,
        children: Vec::new(),
    });
    Ok(())
}

fn read_ext_element(
    local_name: &str,
    attrs: &Attributes,
    reader: &mut Reader<&[u8]>,
) -> Result<ExtensionElement, Xliff2Error> {
    let attributes = read_ext_attrs(attrs)?;
    let mut children = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Text(e) => {
                let text = e.unescape()?.into_owned();
                if !text.is_empty() {
                    // Merge adjacent text nodes (e.g. when XML comments between
                    // text are dropped, the surrounding text events should coalesce).
                    if let Some(ExtensionNode::Text(prev)) = children.last_mut() {
                        prev.push_str(&text);
                    } else {
                        children.push(ExtensionNode::Text(text));
                    }
                }
            }
            Event::CData(e) => {
                let text = std::str::from_utf8(&e)?.to_owned();
                if !text.is_empty() {
                    children.push(ExtensionNode::CData(text));
                }
            }
            Event::Start(e) => {
                let child_name = super::raw_name(&e);
                let child = read_ext_element(&child_name, &e.attributes(), reader)?;
                children.push(ExtensionNode::Element(child));
            }
            Event::Empty(e) => {
                let child_name = super::raw_name(&e);
                let child_attrs = read_ext_attrs(&e.attributes())?;
                children.push(ExtensionNode::Element(ExtensionElement {
                    namespace: String::new(),
                    local_name: child_name,
                    attributes: child_attrs,
                    children: Vec::new(),
                }));
            }
            Event::End(_) => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(ExtensionElement {
        namespace: String::new(),
        local_name: local_name.to_owned(),
        attributes,
        children,
    })
}

fn read_ext_attrs(attrs: &Attributes) -> Result<Vec<(String, String)>, Xliff2Error> {
    let mut result = Vec::new();
    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?.to_owned();
        let val = std::str::from_utf8(&attr.value)?.to_owned();
        // Preserve xmlns declarations for round-trip fidelity
        result.push((key, val));
    }
    Ok(result)
}
