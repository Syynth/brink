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

pub fn write_ext_elements<W: Write>(
    ext: &Extensions,
    w: &mut Writer<W>,
) -> Result<(), Xliff2Error> {
    for element in &ext.elements {
        write_ext_element(element, w)?;
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
            }
        }
        w.write_event(Event::End(BytesEnd::new(qualified)))?;
    }
    Ok(())
}
