use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::events::attributes::Attributes;

use crate::error::Xliff2Error;
use crate::model::extensions::Extensions;
use crate::model::inline::{CanReorder, Ec, Em, InlineElement, Mrk, Pc, Ph, Sc, Sm};

use super::extensions;

/// Read inline content until the given end tag is reached.
///
/// # Element vocabulary
///
/// The `Event::Start`/`Event::Empty` matches below are exhaustive over
/// XLIFF 2.0's *closed* inline-markup vocabulary (Core, §5.4-§5.6):
/// `<ph>`, `<pc>`, `<sc>`/`<ec>`, `<mrk>`, `<sm>`/`<em>`, and `<cp>` (the
/// standalone code-point escape, `Event::Empty` only — `<cp>` has no
/// content model to speak of, so a `<cp>…</cp>` form is not legal XLIFF and
/// is not handled as a `Start`). Nothing else is a member of that
/// vocabulary; XLIFF 2.0 has no extensibility point that adds a *new*
/// inline element name to `<source>`/`<target>` content — mixing in
/// anything else is only legal through the Extensions mechanism (§10), and
/// brink's own exporter (`push_part_inline`) never emits an element name
/// outside this set. So an element name reaching the catch-all below is,
/// by construction, TMS-authored extension markup: a QA flag, a
/// terminology annotation, a reviewer comment — the same category as the
/// foreign, non-brink-marked `<mrk>` that #1821 taught `elements_to_parts`
/// to splice through rather than drop or reject.
///
/// # Catch-all disposition (#1824)
///
/// - **`Event::Start`** (a name with a body): recurse into it with this
///   same function, then splice its children directly into `elements`.
///   This is [`skip_element`]'s replacement — `skip_element` discarded
///   every byte of text nested inside an unrecognized element, which is
///   translator work with no second copy. Splicing keeps that text (and
///   any further-nested known/unknown elements, at any depth) while
///   dropping only the wrapper's own name and attributes — exactly what
///   #1821's foreign-`<mrk>` arm does one layer up in `brink-intl`, and
///   for the same reason: the wrapper is not brink content, but what it
///   wraps is.
/// - **`Event::Empty`** (a self-closing name, attributes only): still
///   ignored. This one is a deliberate no-op, not an oversight — an empty
///   element is, by XML's own grammar, incapable of carrying character
///   data, so there is no text this arm could ever discard. It is the
///   direct analog of the foreign `<sc>`/`<ec>`/`<sm>`/`<em>` "ignore"
///   arms in `elements_to_parts`, which rest on the identical premise.
pub fn read_inline_content(
    reader: &mut Reader<&[u8]>,
    end_tag: &str,
) -> Result<Vec<InlineElement>, Xliff2Error> {
    let mut elements = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Text(e) => {
                let text = e.decode()?;
                if !text.is_empty() {
                    push_text(&mut elements, &text);
                }
            }
            // quick-xml 0.39 emits entity references (`&amp;`, `&#65;`, …) as
            // standalone events. Resolve and merge them into the surrounding text
            // so `a &amp; b` round-trips as a single `Text("a & b")`.
            Event::GeneralRef(e) => {
                let text = super::resolve_general_ref(&e)?;
                push_text(&mut elements, &text);
            }
            Event::CData(e) => {
                let text = std::str::from_utf8(&e)?.to_owned();
                if !text.is_empty() {
                    elements.push(InlineElement::CData(text));
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
                    // Not a known XLIFF 2.0 inline element — TMS extension
                    // markup wrapping content brink still must not lose.
                    // Recurse on the wrapper's own end tag and splice its
                    // children in place (#1824); see the doc comment above.
                    _ => elements.extend(read_inline_content(reader, &name)?),
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
                    "cp" => elements.push(read_cp_empty(&e.attributes())?),
                    // Not a known XLIFF 2.0 inline element, and self-closing:
                    // an empty element cannot carry character data (XML's
                    // own grammar forbids it), so there is no text to lose
                    // by ignoring it (#1824).
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

/// Append `text` to the inline element list, merging into a trailing text node so
/// that adjacent text and resolved entity references coalesce into one element.
fn push_text(elements: &mut Vec<InlineElement>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(InlineElement::Text(prev)) = elements.last_mut() {
        prev.push_str(text);
    } else {
        elements.push(InlineElement::Text(text.to_owned()));
    }
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

fn read_cp_empty(attrs: &Attributes) -> Result<InlineElement, Xliff2Error> {
    let mut hex = None;
    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "hex" {
            hex = Some(val.to_owned());
        }
    }
    let hex = hex.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "cp".to_owned(),
        attribute: "hex".to_owned(),
    })?;
    Ok(InlineElement::Cp(hex))
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
