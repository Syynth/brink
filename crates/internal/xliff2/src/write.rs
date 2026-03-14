mod extensions;
mod inline;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use std::io::Write;

use crate::XLIFF_NS;
use crate::error::Xliff2Error;
use crate::model::{
    AppliesTo, Content, DataEntry, Document, File, Group, Ignorable, Note, OriginalData, Segment,
    Skeleton, State, SubUnit, Unit,
};

/// Serialize a `Document` to XLIFF 2.0 XML, writing to the given writer.
pub fn write_xliff<W: Write>(doc: &Document, writer: W) -> Result<(), Xliff2Error> {
    let mut w = Writer::new_with_indent(writer, b' ', 2);

    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut xliff = BytesStart::new("xliff");
    xliff.push_attribute(("xmlns", XLIFF_NS));
    xliff.push_attribute(("version", doc.version.as_str()));
    xliff.push_attribute(("srcLang", doc.src_lang.as_str()));
    if let Some(ref trg) = doc.trg_lang {
        xliff.push_attribute(("trgLang", trg.as_str()));
    }
    extensions::write_ext_attributes(&doc.extensions, &mut xliff);
    w.write_event(Event::Start(xliff))?;

    extensions::write_ext_elements(&doc.extensions, &mut w)?;

    for file in &doc.files {
        write_file(file, &mut w)?;
    }

    w.write_event(Event::End(BytesEnd::new("xliff")))?;
    Ok(())
}

/// Serialize a `Document` to an XLIFF 2.0 XML string.
pub fn to_string(doc: &Document) -> Result<String, Xliff2Error> {
    let mut buf = Vec::new();
    write_xliff(doc, &mut buf)?;
    String::from_utf8(buf).map_err(|e| Xliff2Error::Utf8(e.utf8_error()))
}

fn write_file<W: Write>(file: &File, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("file");
    elem.push_attribute(("id", file.id.as_str()));
    if let Some(ref orig) = file.original {
        elem.push_attribute(("original", orig.as_str()));
    }
    extensions::write_ext_attributes(&file.extensions, &mut elem);
    w.write_event(Event::Start(elem))?;

    extensions::write_ext_elements(&file.extensions, w)?;

    if let Some(ref skel) = file.skeleton {
        write_skeleton(skel, w)?;
    }

    write_notes(&file.notes, w)?;

    for group in &file.groups {
        write_group(group, w)?;
    }
    for unit in &file.units {
        write_unit(unit, w)?;
    }

    w.write_event(Event::End(BytesEnd::new("file")))?;
    Ok(())
}

fn write_skeleton<W: Write>(skel: &Skeleton, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("skeleton");
    if let Some(ref href) = skel.href {
        elem.push_attribute(("href", href.as_str()));
    }
    if let Some(ref content) = skel.content {
        w.write_event(Event::Start(elem))?;
        w.write_event(Event::Text(BytesText::new(content)))?;
        w.write_event(Event::End(BytesEnd::new("skeleton")))?;
    } else {
        w.write_event(Event::Empty(elem))?;
    }
    Ok(())
}

fn write_notes<W: Write>(notes: &[Note], w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    if notes.is_empty() {
        return Ok(());
    }
    w.write_event(Event::Start(BytesStart::new("notes")))?;
    for note in notes {
        write_note(note, w)?;
    }
    w.write_event(Event::End(BytesEnd::new("notes")))?;
    Ok(())
}

fn write_note<W: Write>(note: &Note, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("note");
    if let Some(ref id) = note.id {
        elem.push_attribute(("id", id.as_str()));
    }
    if let Some(ref cat) = note.category {
        elem.push_attribute(("category", cat.as_str()));
    }
    if let Some(pri) = note.priority {
        let s = pri.to_string();
        elem.push_attribute(("priority", s.as_str()));
    }
    if let Some(ref applies) = note.applies_to {
        let val = match applies {
            AppliesTo::Source => "source",
            AppliesTo::Target => "target",
        };
        elem.push_attribute(("appliesTo", val));
    }
    w.write_event(Event::Start(elem))?;
    w.write_event(Event::Text(BytesText::new(&note.content)))?;
    w.write_event(Event::End(BytesEnd::new("note")))?;
    Ok(())
}

fn write_group<W: Write>(group: &Group, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("group");
    elem.push_attribute(("id", group.id.as_str()));
    if let Some(ref name) = group.name {
        elem.push_attribute(("name", name.as_str()));
    }
    extensions::write_ext_attributes(&group.extensions, &mut elem);
    w.write_event(Event::Start(elem))?;

    extensions::write_ext_elements(&group.extensions, w)?;
    write_notes(&group.notes, w)?;

    for g in &group.groups {
        write_group(g, w)?;
    }
    for unit in &group.units {
        write_unit(unit, w)?;
    }

    w.write_event(Event::End(BytesEnd::new("group")))?;
    Ok(())
}

fn write_unit<W: Write>(unit: &Unit, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("unit");
    elem.push_attribute(("id", unit.id.as_str()));
    if let Some(ref name) = unit.name {
        elem.push_attribute(("name", name.as_str()));
    }
    extensions::write_ext_attributes(&unit.extensions, &mut elem);
    w.write_event(Event::Start(elem))?;

    extensions::write_ext_elements(&unit.extensions, w)?;
    write_notes(&unit.notes, w)?;

    if let Some(ref od) = unit.original_data {
        write_original_data(od, w)?;
    }

    for su in &unit.sub_units {
        match su {
            SubUnit::Segment(seg) => write_segment(seg, w)?,
            SubUnit::Ignorable(ign) => write_ignorable(ign, w)?,
        }
    }

    w.write_event(Event::End(BytesEnd::new("unit")))?;
    Ok(())
}

fn write_original_data<W: Write>(od: &OriginalData, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    w.write_event(Event::Start(BytesStart::new("originalData")))?;
    for entry in &od.entries {
        write_data_entry(entry, w)?;
    }
    w.write_event(Event::End(BytesEnd::new("originalData")))?;
    Ok(())
}

fn write_data_entry<W: Write>(entry: &DataEntry, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("data");
    elem.push_attribute(("id", entry.id.as_str()));
    w.write_event(Event::Start(elem))?;
    w.write_event(Event::Text(BytesText::new(&entry.content)))?;
    w.write_event(Event::End(BytesEnd::new("data")))?;
    Ok(())
}

fn write_segment<W: Write>(seg: &Segment, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("segment");
    if let Some(ref id) = seg.id {
        elem.push_attribute(("id", id.as_str()));
    }
    if let Some(ref state) = seg.state {
        let val = match state {
            State::Initial => "initial",
            State::Translated => "translated",
            State::Reviewed => "reviewed",
            State::Final => "final",
        };
        elem.push_attribute(("state", val));
    }
    if let Some(ref sub) = seg.sub_state {
        elem.push_attribute(("subState", sub.as_str()));
    }
    w.write_event(Event::Start(elem))?;

    write_content("source", &seg.source, w)?;
    if let Some(ref target) = seg.target {
        write_content("target", target, w)?;
    }

    w.write_event(Event::End(BytesEnd::new("segment")))?;
    Ok(())
}

fn write_ignorable<W: Write>(ign: &Ignorable, w: &mut Writer<W>) -> Result<(), Xliff2Error> {
    let mut elem = BytesStart::new("ignorable");
    if let Some(ref id) = ign.id {
        elem.push_attribute(("id", id.as_str()));
    }
    w.write_event(Event::Start(elem))?;

    write_content("source", &ign.source, w)?;
    if let Some(ref target) = ign.target {
        write_content("target", target, w)?;
    }

    w.write_event(Event::End(BytesEnd::new("ignorable")))?;
    Ok(())
}

fn write_content<W: Write>(
    tag: &str,
    content: &Content,
    w: &mut Writer<W>,
) -> Result<(), Xliff2Error> {
    // Inline content must not be indented — whitespace is significant.
    // Build the entire <source>...</source> or <target>...</target> as raw bytes
    // and write them via the indent writer's inner writer to avoid indentation
    // artifacts inside content elements.
    let mut raw_buf = Vec::new();
    {
        let mut raw_writer = Writer::new(&mut raw_buf);
        let mut elem = BytesStart::new(tag);
        if let Some(ref lang) = content.lang {
            elem.push_attribute(("xml:lang", lang.as_str()));
        }
        raw_writer.write_event(Event::Start(elem))?;
        inline::write_inline_elements(&content.elements, &mut raw_writer)?;
        raw_writer.write_event(Event::End(BytesEnd::new(tag)))?;
    }

    // Write indentation for the opening tag, then the raw content
    w.write_indent()?;
    w.get_mut().write_all(&raw_buf)?;
    Ok(())
}
