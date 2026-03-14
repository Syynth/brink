use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Write;

use crate::error::Xliff2Error;
use crate::model::extensions::{ExtensionElement, ExtensionNode, Extensions};

pub fn write_ext_attributes(ext: &Extensions, elem: &mut BytesStart) {
    for attr in &ext.attributes {
        let qualified = format!("{}:{}", attr.namespace, attr.local_name);
        elem.push_attribute((qualified.as_str(), attr.value.as_str()));
    }
}

/// Write extension elements without indentation to preserve round-trip fidelity.
/// Extension element trees contain their own whitespace from the original document;
/// the indent writer must not inject additional whitespace.
pub fn write_ext_elements<W: Write>(
    ext: &Extensions,
    w: &mut Writer<W>,
) -> Result<(), Xliff2Error> {
    for element in &ext.elements {
        // Serialize the entire extension element tree into a raw buffer
        // using a non-indenting writer, then write through the main writer
        // with just the leading indentation.
        let mut raw_buf = Vec::new();
        {
            let mut raw_writer = Writer::new(&mut raw_buf);
            write_ext_element(element, &mut raw_writer)?;
        }
        w.write_indent()?;
        w.get_mut().write_all(&raw_buf)?;
    }
    Ok(())
}

fn write_ext_element<W: Write>(
    element: &ExtensionElement,
    w: &mut Writer<W>,
) -> Result<(), Xliff2Error> {
    let qualified = if element.namespace.is_empty() {
        element.local_name.clone()
    } else {
        format!("{}:{}", element.namespace, element.local_name)
    };

    let mut elem = BytesStart::new(qualified.clone());
    for (key, value) in &element.attributes {
        elem.push_attribute((key.as_str(), value.as_str()));
    }

    if element.children.is_empty() {
        w.write_event(Event::Empty(elem))?;
    } else {
        w.write_event(Event::Start(elem))?;
        for child in &element.children {
            match child {
                ExtensionNode::Element(e) => write_ext_element(e, w)?,
                ExtensionNode::Text(text) => {
                    w.write_event(Event::Text(BytesText::new(text)))?;
                }
                ExtensionNode::CData(text) => {
                    w.write_event(Event::CData(quick_xml::events::BytesCData::new(text)))?;
                }
            }
        }
        w.write_event(Event::End(BytesEnd::new(qualified)))?;
    }
    Ok(())
}
