use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Write;

use crate::error::Xliff2Error;
use crate::model::inline::{CanReorder, Ec, Em, InlineElement, Mrk, Pc, Ph, Sc, Sm};

use super::extensions;

pub fn write_inline_elements<W: Write>(
    elements: &[InlineElement],
    w: &mut Writer<W>,
) -> Result<(), Xliff2Error> {
    for elem in elements {
        write_inline_element(elem, w)?;
    }
    Ok(())
}

fn write_inline_element<W: Write>(
    elem: &InlineElement,
    w: &mut Writer<W>,
) -> Result<(), Xliff2Error> {
    match elem {
        InlineElement::Text(text) => {
            w.write_event(Event::Text(BytesText::new(text)))?;
        }
        InlineElement::CData(text) => {
            w.write_event(Event::CData(quick_xml::events::BytesCData::new(text)))?;
        }
        InlineElement::Cp(hex) => {
            let mut elem = BytesStart::new("cp");
            elem.push_attribute(("hex", hex.as_str()));
            w.write_event(Event::Empty(elem))?;
        }
        InlineElement::Ph(ph) => write_ph(ph, w)?,
        InlineElement::Pc(pc) => write_pc(pc, w)?,
        InlineElement::Sc(sc) => write_sc(sc, w)?,
        InlineElement::Ec(ec) => write_ec(ec, w)?,
        InlineElement::Mrk(mrk) => write_mrk(mrk, w)?,
        InlineElement::Sm(sm) => write_sm(sm, w)?,
        InlineElement::Em(em) => write_em(em, w)?,
    }
    Ok(())
}

fn write_ph<W: Write>(ph: &Ph, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("ph");
    elem.push_attribute(("id", ph.id.as_str()));
    push_opt_attr(&mut elem, "dataRef", ph.data_ref.as_ref());
    push_opt_attr(&mut elem, "equiv", ph.equiv.as_ref());
    push_opt_attr(&mut elem, "disp", ph.disp.as_ref());
    push_opt_attr(&mut elem, "subType", ph.sub_type.as_ref());
    extensions::write_ext_attributes(&ph.extensions, &mut elem);
    w.write_event(Event::Empty(elem))?;
    Ok(())
}

fn write_pc<W: Write>(pc: &Pc, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("pc");
    elem.push_attribute(("id", pc.id.as_str()));
    push_opt_attr(&mut elem, "dataRefStart", pc.data_ref_start.as_ref());
    push_opt_attr(&mut elem, "dataRefEnd", pc.data_ref_end.as_ref());
    push_opt_attr(&mut elem, "subType", pc.sub_type.as_ref());
    extensions::write_ext_attributes(&pc.extensions, &mut elem);
    w.write_event(Event::Start(elem))?;

    extensions::write_ext_elements(&pc.extensions, w)?;
    write_inline_elements(&pc.content, w)?;

    w.write_event(Event::End(BytesEnd::new("pc")))?;
    Ok(())
}

fn write_sc<W: Write>(sc: &Sc, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("sc");
    elem.push_attribute(("id", sc.id.as_str()));
    push_opt_attr(&mut elem, "dataRef", sc.data_ref.as_ref());
    push_opt_attr(&mut elem, "subType", sc.sub_type.as_ref());
    push_opt_bool_attr(&mut elem, "canCopy", sc.can_copy);
    push_opt_bool_attr(&mut elem, "canDelete", sc.can_delete);
    push_opt_bool_attr(&mut elem, "canOverlap", sc.can_overlap);
    if let Some(cr) = sc.can_reorder {
        elem.push_attribute(("canReorder", can_reorder_str(cr)));
    }
    extensions::write_ext_attributes(&sc.extensions, &mut elem);
    w.write_event(Event::Empty(elem))?;
    Ok(())
}

fn write_ec<W: Write>(ec: &Ec, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("ec");
    if let Some(ref sr) = ec.start_ref {
        elem.push_attribute(("startRef", sr.as_str()));
    }
    if let Some(ref id) = ec.id {
        elem.push_attribute(("id", id.as_str()));
    }
    push_opt_bool_attr(&mut elem, "isolated", ec.isolated);
    push_opt_attr(&mut elem, "dataRef", ec.data_ref.as_ref());
    push_opt_attr(&mut elem, "subType", ec.sub_type.as_ref());
    push_opt_bool_attr(&mut elem, "canCopy", ec.can_copy);
    push_opt_bool_attr(&mut elem, "canDelete", ec.can_delete);
    push_opt_bool_attr(&mut elem, "canOverlap", ec.can_overlap);
    if let Some(cr) = ec.can_reorder {
        elem.push_attribute(("canReorder", can_reorder_str(cr)));
    }
    extensions::write_ext_attributes(&ec.extensions, &mut elem);
    w.write_event(Event::Empty(elem))?;
    Ok(())
}

fn write_mrk<W: Write>(mrk: &Mrk, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("mrk");
    elem.push_attribute(("id", mrk.id.as_str()));
    push_opt_bool_attr(&mut elem, "translate", mrk.translate);
    push_opt_attr(&mut elem, "type", mrk.mrk_type.as_ref());
    push_opt_attr(&mut elem, "ref", mrk.ref_.as_ref());
    push_opt_attr(&mut elem, "value", mrk.value.as_ref());
    extensions::write_ext_attributes(&mrk.extensions, &mut elem);
    w.write_event(Event::Start(elem))?;

    extensions::write_ext_elements(&mrk.extensions, w)?;
    write_inline_elements(&mrk.content, w)?;

    w.write_event(Event::End(BytesEnd::new("mrk")))?;
    Ok(())
}

fn write_sm<W: Write>(sm: &Sm, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("sm");
    elem.push_attribute(("id", sm.id.as_str()));
    push_opt_bool_attr(&mut elem, "translate", sm.translate);
    push_opt_attr(&mut elem, "type", sm.sm_type.as_ref());
    push_opt_attr(&mut elem, "ref", sm.ref_.as_ref());
    push_opt_attr(&mut elem, "value", sm.value.as_ref());
    extensions::write_ext_attributes(&sm.extensions, &mut elem);
    w.write_event(Event::Empty(elem))?;
    Ok(())
}

fn write_em<W: Write>(em: &Em, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("em");
    elem.push_attribute(("startRef", em.start_ref.as_str()));
    w.write_event(Event::Empty(elem))?;
    Ok(())
}

fn push_opt_attr(elem: &mut BytesStart, name: &str, value: Option<&String>) {
    if let Some(v) = value {
        elem.push_attribute((name, v.as_str()));
    }
}

fn push_opt_bool_attr(elem: &mut BytesStart, name: &str, value: Option<bool>) {
    if let Some(v) = value {
        elem.push_attribute((name, if v { "yes" } else { "no" }));
    }
}

fn can_reorder_str(cr: CanReorder) -> &'static str {
    match cr {
        CanReorder::Yes => "yes",
        CanReorder::No => "no",
        CanReorder::FirstNo => "firstNo",
    }
}
