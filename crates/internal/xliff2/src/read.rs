mod extensions;
mod inline;

use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::events::attributes::Attributes;

use crate::error::Xliff2Error;
use crate::model::extensions::Extensions;
use crate::model::{
    AppliesTo, Content, DataEntry, Document, File, Group, Ignorable, Note, OriginalData, Segment,
    Skeleton, State, SubUnit, Unit,
};

/// Parse an XLIFF 2.0 document from a string.
pub fn read_xliff(xml: &str) -> Result<Document, Xliff2Error> {
    read_xliff_bytes(xml.as_bytes())
}

/// Parse an XLIFF 2.0 document from bytes.
pub fn read_xliff_bytes(xml: &[u8]) -> Result<Document, Xliff2Error> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    // Skip XML declaration and find <xliff>
    let doc = loop {
        match reader.read_event()? {
            Event::Start(e) if local_name(&e) == "xliff" => {
                break read_xliff_element(&e.attributes(), &mut reader)?;
            }
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    };

    Ok(doc)
}

fn read_xliff_element(
    attrs: &Attributes,
    reader: &mut Reader<&[u8]>,
) -> Result<Document, Xliff2Error> {
    let mut version = None;
    let mut src_lang = None;
    let mut trg_lang = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "version" => version = Some(val.to_owned()),
            "srcLang" => src_lang = Some(val.to_owned()),
            "trgLang" => trg_lang = Some(val.to_owned()),
            "xmlns" => {}
            k if k.starts_with("xmlns:") => {}
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let version = version.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "xliff".to_owned(),
        attribute: "version".to_owned(),
    })?;
    let src_lang = src_lang.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "xliff".to_owned(),
        attribute: "srcLang".to_owned(),
    })?;

    let mut files = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = local_name(&e);
                match name.as_str() {
                    "file" => files.push(read_file(&e.attributes(), reader)?),
                    _ => {
                        extensions::read_ext_element_into(
                            &name,
                            &e.attributes(),
                            reader,
                            &mut ext,
                        )?;
                    }
                }
            }
            Event::End(e) if local_name_end(&e) == "xliff" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(Document {
        version,
        src_lang,
        trg_lang,
        files,
        extensions: ext,
    })
}

fn read_file(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<File, Xliff2Error> {
    let mut id = None;
    let mut original = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "original" => original = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "file".to_owned(),
        attribute: "id".to_owned(),
    })?;

    let mut notes = Vec::new();
    let mut skeleton = None;
    let mut groups = Vec::new();
    let mut units = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let name = local_name(&e);
                match name.as_str() {
                    "notes" => notes = read_notes(reader)?,
                    "skeleton" => skeleton = Some(read_skeleton(&e.attributes(), reader)?),
                    "group" => groups.push(read_group(&e.attributes(), reader)?),
                    "unit" => units.push(read_unit(&e.attributes(), reader)?),
                    _ => {
                        extensions::read_ext_element_into(
                            &name,
                            &e.attributes(),
                            reader,
                            &mut ext,
                        )?;
                    }
                }
            }
            Event::Empty(e) => {
                let name = local_name(&e);
                match name.as_str() {
                    "skeleton" => skeleton = Some(read_skeleton_empty(&e.attributes())?),
                    _ => extensions::collect_empty_ext_element(&name, &e.attributes(), &mut ext)?,
                }
            }
            Event::End(e) if local_name_end(&e) == "file" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(File {
        id,
        original,
        notes,
        skeleton,
        groups,
        units,
        extensions: ext,
    })
}

fn read_skeleton(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Skeleton, Xliff2Error> {
    let mut href = None;
    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "href" {
            href = Some(val.to_owned());
        }
    }

    let mut content = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(e) => content.push_str(&e.unescape()?),
            Event::CData(e) => {
                content.push_str(std::str::from_utf8(&e)?);
            }
            Event::End(e) if local_name_end(&e) == "skeleton" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    let content = if content.is_empty() {
        None
    } else {
        Some(content)
    };

    Ok(Skeleton { href, content })
}

fn read_skeleton_empty(attrs: &Attributes) -> Result<Skeleton, Xliff2Error> {
    let mut href = None;
    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "href" {
            href = Some(val.to_owned());
        }
    }
    Ok(Skeleton {
        href,
        content: None,
    })
}

fn read_notes(reader: &mut Reader<&[u8]>) -> Result<Vec<Note>, Xliff2Error> {
    let mut notes = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if local_name(&e) == "note" => {
                notes.push(read_note(&e.attributes(), reader)?);
            }
            Event::End(e) if local_name_end(&e) == "notes" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }
    Ok(notes)
}

fn read_note(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Note, Xliff2Error> {
    let mut id = None;
    let mut category = None;
    let mut priority = None;
    let mut applies_to = None;

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "category" => category = Some(val.to_owned()),
            "priority" => {
                priority = Some(
                    val.parse::<u8>()
                        .map_err(|_| Xliff2Error::InvalidAttribute {
                            element: "note".to_owned(),
                            attribute: "priority".to_owned(),
                            value: val.to_owned(),
                        })?,
                );
            }
            "appliesTo" => {
                applies_to = Some(match val {
                    "source" => AppliesTo::Source,
                    "target" => AppliesTo::Target,
                    _ => {
                        return Err(Xliff2Error::InvalidAttribute {
                            element: "note".to_owned(),
                            attribute: "appliesTo".to_owned(),
                            value: val.to_owned(),
                        });
                    }
                });
            }
            _ => {}
        }
    }

    let mut content = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(e) => content.push_str(&e.unescape()?),
            Event::CData(e) => content.push_str(std::str::from_utf8(&e)?),
            Event::End(e) if local_name_end(&e) == "note" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(Note {
        id,
        category,
        priority,
        applies_to,
        content,
    })
}

fn read_group(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Group, Xliff2Error> {
    let mut id = None;
    let mut name = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "name" => name = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "group".to_owned(),
        attribute: "id".to_owned(),
    })?;

    let mut notes = Vec::new();
    let mut groups = Vec::new();
    let mut units = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let ln = local_name(&e);
                match ln.as_str() {
                    "notes" => notes = read_notes(reader)?,
                    "group" => groups.push(read_group(&e.attributes(), reader)?),
                    "unit" => units.push(read_unit(&e.attributes(), reader)?),
                    _ => {
                        extensions::read_ext_element_into(&ln, &e.attributes(), reader, &mut ext)?;
                    }
                }
            }
            Event::Empty(e) => {
                let ln = local_name(&e);
                extensions::collect_empty_ext_element(&ln, &e.attributes(), &mut ext)?;
            }
            Event::End(e) if local_name_end(&e) == "group" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(Group {
        id,
        name,
        notes,
        groups,
        units,
        extensions: ext,
    })
}

fn read_unit(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Unit, Xliff2Error> {
    let mut id = None;
    let mut name = None;
    let mut ext = Extensions::default();

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "name" => name = Some(val.to_owned()),
            _ => extensions::collect_ext_attribute(key, val, &mut ext),
        }
    }

    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "unit".to_owned(),
        attribute: "id".to_owned(),
    })?;

    let mut notes = Vec::new();
    let mut sub_units = Vec::new();
    let mut original_data = None;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let ln = local_name(&e);
                match ln.as_str() {
                    "notes" => notes = read_notes(reader)?,
                    "originalData" => original_data = Some(read_original_data(reader)?),
                    "segment" => {
                        sub_units.push(SubUnit::Segment(read_segment(&e.attributes(), reader)?));
                    }
                    "ignorable" => {
                        sub_units
                            .push(SubUnit::Ignorable(read_ignorable(&e.attributes(), reader)?));
                    }
                    _ => {
                        extensions::read_ext_element_into(&ln, &e.attributes(), reader, &mut ext)?;
                    }
                }
            }
            Event::Empty(e) => {
                let ln = local_name(&e);
                extensions::collect_empty_ext_element(&ln, &e.attributes(), &mut ext)?;
            }
            Event::End(e) if local_name_end(&e) == "unit" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(Unit {
        id,
        name,
        notes,
        sub_units,
        original_data,
        extensions: ext,
    })
}

fn read_original_data(reader: &mut Reader<&[u8]>) -> Result<OriginalData, Xliff2Error> {
    let mut entries = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) if local_name(&e) == "data" => {
                entries.push(read_data_entry(&e.attributes(), reader)?);
            }
            Event::End(e) if local_name_end(&e) == "originalData" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }
    Ok(OriginalData { entries })
}

fn read_data_entry(
    attrs: &Attributes,
    reader: &mut Reader<&[u8]>,
) -> Result<DataEntry, Xliff2Error> {
    let mut id = None;
    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "id" {
            id = Some(val.to_owned());
        }
    }
    let id = id.ok_or_else(|| Xliff2Error::MissingAttribute {
        element: "data".to_owned(),
        attribute: "id".to_owned(),
    })?;

    let mut content = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(e) => content.push_str(&e.unescape()?),
            Event::CData(e) => content.push_str(std::str::from_utf8(&e)?),
            Event::End(e) if local_name_end(&e) == "data" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    Ok(DataEntry { id, content })
}

fn read_segment(attrs: &Attributes, reader: &mut Reader<&[u8]>) -> Result<Segment, Xliff2Error> {
    let mut id = None;
    let mut state = None;
    let mut sub_state = None;

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        match key {
            "id" => id = Some(val.to_owned()),
            "state" => {
                state = Some(parse_state(val)?);
            }
            "subState" => sub_state = Some(val.to_owned()),
            _ => {}
        }
    }

    let mut source = None;
    let mut target = None;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let ln = local_name(&e);
                match ln.as_str() {
                    "source" => source = Some(read_content(&e.attributes(), reader, "source")?),
                    "target" => target = Some(read_content(&e.attributes(), reader, "target")?),
                    _ => skip_element(reader)?,
                }
            }
            Event::End(e) if local_name_end(&e) == "segment" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    let source = source.ok_or_else(|| Xliff2Error::MissingElement {
        parent: "segment".to_owned(),
        child: "source".to_owned(),
    })?;

    Ok(Segment {
        id,
        state,
        sub_state,
        source,
        target,
    })
}

fn read_ignorable(
    attrs: &Attributes,
    reader: &mut Reader<&[u8]>,
) -> Result<Ignorable, Xliff2Error> {
    let mut id = None;

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "id" {
            id = Some(val.to_owned());
        }
    }

    let mut source = None;
    let mut target = None;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let ln = local_name(&e);
                match ln.as_str() {
                    "source" => source = Some(read_content(&e.attributes(), reader, "source")?),
                    "target" => target = Some(read_content(&e.attributes(), reader, "target")?),
                    _ => skip_element(reader)?,
                }
            }
            Event::End(e) if local_name_end(&e) == "ignorable" => break,
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }

    let source = source.ok_or_else(|| Xliff2Error::MissingElement {
        parent: "ignorable".to_owned(),
        child: "source".to_owned(),
    })?;

    Ok(Ignorable { id, source, target })
}

fn read_content(
    attrs: &Attributes,
    reader: &mut Reader<&[u8]>,
    end_tag: &str,
) -> Result<Content, Xliff2Error> {
    let mut lang = None;

    for attr in attrs.clone() {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let val = std::str::from_utf8(&attr.value)?;
        if key == "xml:lang" {
            lang = Some(val.to_owned());
        }
    }

    let elements = inline::read_inline_content(reader, end_tag)?;

    Ok(Content { lang, elements })
}

fn parse_state(val: &str) -> Result<State, Xliff2Error> {
    match val {
        "initial" => Ok(State::Initial),
        "translated" => Ok(State::Translated),
        "reviewed" => Ok(State::Reviewed),
        "final" => Ok(State::Final),
        _ => Err(Xliff2Error::InvalidAttribute {
            element: "segment".to_owned(),
            attribute: "state".to_owned(),
            value: val.to_owned(),
        }),
    }
}

fn skip_element(reader: &mut Reader<&[u8]>) -> Result<(), Xliff2Error> {
    let mut depth = 1u32;
    loop {
        match reader.read_event()? {
            Event::Start(_) => depth += 1,
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => return Err(Xliff2Error::UnexpectedEof),
            _ => {}
        }
    }
    Ok(())
}

/// Extract local name from a start event, stripping any namespace prefix.
fn local_name(e: &quick_xml::events::BytesStart) -> String {
    let name = e.name();
    let full = String::from_utf8_lossy(name.as_ref());
    strip_prefix(&full)
}

/// Extract local name from an end event, stripping any namespace prefix.
fn local_name_end(e: &quick_xml::events::BytesEnd) -> String {
    let name = e.name();
    let full = String::from_utf8_lossy(name.as_ref());
    strip_prefix(&full)
}

fn strip_prefix(name: &str) -> String {
    match name.find(':') {
        Some(pos) => name[pos + 1..].to_owned(),
        None => name.to_owned(),
    }
}
