use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::events::attributes::Attributes;

use crate::error::Xliff2Error;
use crate::model::extensions::Extensions;
use crate::model::inline::{CanReorder, Ec, Em, InlineElement, Mrk, Pc, Ph, Sc, Sm};

use super::extensions;

/// Read inline content until the given end tag is reached.
pub fn read_inline_content(
    reader: &mut Reader<&[u8]>,
    end_tag: &str,
) -> Result<Vec<InlineElement>, Xliff2Error> {
    let mut elements = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Text(e) => {
                let text = e.unescape()?.into_owned();
                if !text.is_empty() {
                    elements.push(InlineElement::Text(text));
                }
            }
            Event::CData(e) => {
                let text = std::str::from_utf8(&e)?.to_owned();
                if !text.is_empty() {
                    elements.push(InlineElement::Text(text));
                }
            }
            Event::Start(e) => {
                let name = super::local_name(&e);
                match name.as_str() {
                    "ph" => {
                        elements.push(InlineElement::Ph(read_ph_start(&e.attributes(), reader)?));
                    }
                    "pc" => {
                        elements.push(InlineElement::Pc(read_pc(&e.attributes(), reader)?));
                    }
                    "sc" => {
                        elements.push(InlineElement::Sc(read_sc_start(&e.attributes(), reader)?));
                    }
                    "ec" => {
                        elements.push(InlineElement::Ec(read_ec_start(&e.attributes(), reader)?));
                    }
                    "mrk" => {
                        elements.push(InlineElement::Mrk(read_mrk(&e.attributes(), reader)?));
                    }
                    "sm" => {
                        elements.push(InlineElement::Sm(read_sm_start(&e.attributes(), reader)?));
                    }
                    "em" => {
                        elements.push(InlineElement::Em(read_em_start(&e.attributes(), reader)?));
                    }
                    _ => super::skip_element(reader)?,
                }
            }
            Event::Empty(e) => {
                let name = super::local_name(&e);
                match name.as_str() {
                    "ph" => elements.push(InlineElement::Ph(read_ph_empty(&e.attributes())?)),
                    "sc" => elements.push(InlineElement::Sc(read_sc_empty(&e.attributes())?)),
                    "ec" => elements.push(InlineElement::Ec(read_ec_empty(&e.attributes())?)),
                    "sm" => elements.push(InlineElement::Sm(read_sm_empty(&e.attributes())?)),
                    "em" => elements.push(InlineElement::Em(read_em_empty(&e.attributes())?)),
                    _ => {}
                }
            }
            Event::End(e) if super::local_name_end(&e) == end_tag => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(elements)
}

fn read_ph_attrs(attrs: &Attributes) -> Result<Ph, Xliff2Error> {
    let mut id = None;
    let mut data_ref = None;
    let mut equiv = None;
    let mut disp = None;
    let mut sub_type = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "dataRef" => data_ref = Some(val.to_owned()),
            "equiv" => equiv = Some(val.to_owned()),
            "disp" => disp = Some(val.to_owned()),
            "subType" => sub_type = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "ph".to_owned(),
        attribute: "id".to_owned(),
    })?;

    Ok(Ph {
        id,
        data_ref,
        equiv,
        disp,
        sub_type,
        extensions: ext,
    })
}

fn read_ph_empty(attrs: &Attributes) -> Result<Ph, Xliff2Error> {
    read_ph_attrs(attrs)
}

fn read_ph_start(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Ph, Xliff2Error> {
    let ph = read_ph_attrs(attrs)?;
    // <ph> should not have child elements in practice, but consume until end tag
    super::skip_element(reader)?;
    Ok(ph)
}

fn read_pc(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Pc, Xliff2Error> {
    let mut id = None;
    let mut data_ref_start = None;
    let mut data_ref_end = None;
    let mut sub_type = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "dataRefStart" => data_ref_start = Some(val.to_owned()),
            "dataRefEnd" => data_ref_end = Some(val.to_owned()),
            "subType" => sub_type = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "pc".to_owned(),
        attribute: "id".to_owned(),
    })?;

    let content = read_inline_content(reader, "pc")?;

    Ok(Pc {
        id,
        data_ref_start,
        data_ref_end,
        sub_type,
        content,
        extensions: ext,
    })
}

fn read_sc_attrs(attrs: &Attributes) -> Result<Sc, Xliff2Error> {
    let mut id = None;
    let mut data_ref = None;
    let mut sub_type = None;
    let mut can_copy = None;
    let mut can_delete = None;
    let mut can_overlap = None;
    let mut can_reorder = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "dataRef" => data_ref = Some(val.to_owned()),
            "subType" => sub_type = Some(val.to_owned()),
            "canCopy" => can_copy = Some(parse_yes_no(val)?),
            "canDelete" => can_delete = Some(parse_yes_no(val)?),
            "canOverlap" => can_overlap = Some(parse_yes_no(val)?),
            "canReorder" => can_reorder = Some(parse_can_reorder(val)?),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "sc".to_owned(),
        attribute: "id".to_owned(),
    })?;

    Ok(Sc {
        id,
        data_ref,
        sub_type,
        can_copy,
        can_delete,
        can_overlap,
        can_reorder,
        extensions: ext,
    })
}

fn read_sc_empty(attrs: &Attributes) -> Result<Sc, Xliff2Error> {
    read_sc_attrs(attrs)
}

fn read_sc_start(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Sc, Xliff2Error> {
    let sc = read_sc_attrs(attrs)?;
    super::skip_element(reader)?;
    Ok(sc)
}

fn read_ec_attrs(attrs: &Attributes) -> Result<Ec, Xliff2Error> {
    let mut start_ref = None;
    let mut id = None;
    let mut isolated = None;
    let mut data_ref = None;
    let mut sub_type = None;
    let mut can_copy = None;
    let mut can_delete = None;
    let mut can_overlap = None;
    let mut can_reorder = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "startRef" => start_ref = Some(val.to_owned()),
            "id" => id = Some(val.to_owned()),
            "isolated" => isolated = Some(parse_yes_no(val)?),
            "dataRef" => data_ref = Some(val.to_owned()),
            "subType" => sub_type = Some(val.to_owned()),
            "canCopy" => can_copy = Some(parse_yes_no(val)?),
            "canDelete" => can_delete = Some(parse_yes_no(val)?),
            "canOverlap" => can_overlap = Some(parse_yes_no(val)?),
            "canReorder" => can_reorder = Some(parse_can_reorder(val)?),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    Ok(Ec {
        start_ref,
        id,
        isolated,
        data_ref,
        sub_type,
        can_copy,
        can_delete,
        can_overlap,
        can_reorder,
        extensions: ext,
    })
}

fn read_ec_empty(attrs: &Attributes) -> Result<Ec, Xliff2Error> {
    read_ec_attrs(attrs)
}

fn read_ec_start(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Ec, Xliff2Error> {
    let ec = read_ec_attrs(attrs)?;
    super::skip_element(reader)?;
    Ok(ec)
}

fn read_mrk(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Mrk, Xliff2Error> {
    let mut id = None;
    let mut translate = None;
    let mut mrk_type = None;
    let mut ref_ = None;
    let mut value = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "translate" => translate = Some(parse_yes_no(val)?),
            "type" => mrk_type = Some(val.to_owned()),
            "ref" => ref_ = Some(val.to_owned()),
            "value" => value = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "mrk".to_owned(),
        attribute: "id".to_owned(),
    })?;

    let content = read_inline_content(reader, "mrk")?;

    Ok(Mrk {
        id,
        translate,
        mrk_type,
        ref_,
        value,
        content,
        extensions: ext,
    })
}

fn read_sm_attrs(attrs: &Attributes) -> Result<Sm, Xliff2Error> {
    let mut id = None;
    let mut translate = None;
    let mut sm_type = None;
    let mut ref_ = None;
    let mut value = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "translate" => translate = Some(parse_yes_no(val)?),
            "type" => sm_type = Some(val.to_owned()),
            "ref" => ref_ = Some(val.to_owned()),
            "value" => value = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "sm".to_owned(),
        attribute: "id".to_owned(),
    })?;

    Ok(Sm {
        id,
        translate,
        sm_type,
        ref_,
        value,
        extensions: ext,
    })
}

fn read_sm_empty(attrs: &Attributes) -> Result<Sm, Xliff2Error> {
    read_sm_attrs(attrs)
}

fn read_sm_start(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Sm, Xliff2Error> {
    let sm = read_sm_attrs(attrs)?;
    super::skip_element(reader)?;
    Ok(sm)
}

fn read_em_attrs(attrs: &Attributes) -> Result<Em, Xliff2Error> {
    let mut start_ref = None;

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "startRef" {
            start_ref = Some(val.to_owned());
        }
    }

    let start_ref = start_ref.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "em".to_owned(),
        attribute: "startRef".to_owned(),
    })?;

    Ok(Em { start_ref })
}

fn read_em_empty(attrs: &Attributes) -> Result<Em, Xliff2Error> {
    read_em_attrs(attrs)
}

fn read_em_start(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Em, Xliff2Error> {
    let em = read_em_attrs(attrs)?;
    super::skip_element(reader)?;
    Ok(em)
}

fn parse_yes_no(val: &str) -> Result<bool, Xliff2Error> {
    match val {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(Xliff2Error::InvalidAttribute {
            element: String::new(),
            attribute: String::new(),
            value: val.to_owned(),
        }),
    }
}

fn parse_can_reorder(val: &str) -> Result<CanReorder, Xliff2Error> {
    match val {
        "yes" => Ok(CanReorder::Yes),
        "no" => Ok(CanReorder::No),
        "firstNo" => Ok(CanReorder::FirstNo),
        _ => Err(Xliff2Error::InvalidAttribute {
            element: String::new(),
            attribute: "canReorder".to_owned(),
            value: val.to_owned(),
        }),
    }
}
