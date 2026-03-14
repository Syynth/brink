use std::collections::HashSet;

use crate::model::{
    Content, Document, File, Group, InlineElement, Note, OriginalData, SubUnit, Unit,
};

/// A single validation issue found in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

/// Validate a document for XLIFF 2.0 structural correctness.
/// Returns all issues found, not just the first.
pub fn validate(doc: &Document) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if doc.version != "2.0" && doc.version != "2.1" {
        errors.push(ValidationError {
            path: "xliff".to_owned(),
            message: format!(
                "version must be \"2.0\" or \"2.1\", got \"{}\"",
                doc.version
            ),
        });
    }

    if doc.src_lang.is_empty() {
        errors.push(ValidationError {
            path: "xliff".to_owned(),
            message: "srcLang must not be empty".to_owned(),
        });
    }

    if doc.files.is_empty() {
        errors.push(ValidationError {
            path: "xliff".to_owned(),
            message: "document must contain at least one <file>".to_owned(),
        });
    }

    let mut file_ids = HashSet::new();
    for file in &doc.files {
        if !file_ids.insert(&file.id) {
            errors.push(ValidationError {
                path: format!("file[@id='{}']", file.id),
                message: format!("duplicate file id \"{}\"", file.id),
            });
        }
        validate_file(file, &mut errors);
    }

    errors
}

fn validate_file(file: &File, errors: &mut Vec<ValidationError>) {
    let path = format!("file[@id='{}']", file.id);

    validate_notes(&file.notes, &path, errors);

    let mut unit_ids = HashSet::new();
    let mut group_ids = HashSet::new();

    for group in &file.groups {
        if !group_ids.insert(&group.id) {
            errors.push(ValidationError {
                path: format!("{path}/group[@id='{}']", group.id),
                message: format!("duplicate group id \"{}\"", group.id),
            });
        }
        validate_group(group, &path, &mut unit_ids, errors);
    }

    for unit in &file.units {
        if !unit_ids.insert(&unit.id) {
            errors.push(ValidationError {
                path: format!("{path}/unit[@id='{}']", unit.id),
                message: format!("duplicate unit id \"{}\"", unit.id),
            });
        }
        validate_unit(unit, &path, errors);
    }
}

fn validate_group<'a>(
    group: &'a Group,
    parent_path: &str,
    unit_ids: &mut HashSet<&'a String>,
    errors: &mut Vec<ValidationError>,
) {
    let path = format!("{parent_path}/group[@id='{}']", group.id);

    validate_notes(&group.notes, &path, errors);

    for g in &group.groups {
        validate_group(g, &path, unit_ids, errors);
    }

    for unit in &group.units {
        if !unit_ids.insert(&unit.id) {
            errors.push(ValidationError {
                path: format!("{path}/unit[@id='{}']", unit.id),
                message: format!("duplicate unit id \"{}\"", unit.id),
            });
        }
        validate_unit(unit, &path, errors);
    }
}

fn validate_unit(unit: &Unit, parent_path: &str, errors: &mut Vec<ValidationError>) {
    let path = format!("{parent_path}/unit[@id='{}']", unit.id);

    validate_notes(&unit.notes, &path, errors);

    let has_segment = unit
        .sub_units
        .iter()
        .any(|su| matches!(su, SubUnit::Segment(_)));
    if !has_segment {
        errors.push(ValidationError {
            path: path.clone(),
            message: "unit must contain at least one <segment>".to_owned(),
        });
    }

    let mut sub_ids = HashSet::new();
    for (i, su) in unit.sub_units.iter().enumerate() {
        let (sub_id, sub_path) = match su {
            SubUnit::Segment(seg) => (&seg.id, format!("{path}/segment[{i}]")),
            SubUnit::Ignorable(ign) => (&ign.id, format!("{path}/ignorable[{i}]")),
        };
        if let Some(id) = sub_id
            && !sub_ids.insert(id)
        {
            errors.push(ValidationError {
                path: sub_path.clone(),
                message: format!("duplicate segment/ignorable id \"{id}\""),
            });
        }

        match su {
            SubUnit::Segment(seg) => {
                validate_inline_codes(&seg.source, &format!("{sub_path}/source"), errors);
                if let Some(ref target) = seg.target {
                    validate_inline_codes(target, &format!("{sub_path}/target"), errors);
                }
                if let Some(ref od) = unit.original_data {
                    validate_data_refs(&seg.source, od, &format!("{sub_path}/source"), errors);
                    if let Some(ref target) = seg.target {
                        validate_data_refs(target, od, &format!("{sub_path}/target"), errors);
                    }
                }
            }
            SubUnit::Ignorable(ign) => {
                validate_inline_codes(&ign.source, &format!("{sub_path}/source"), errors);
                if let Some(ref target) = ign.target {
                    validate_inline_codes(target, &format!("{sub_path}/target"), errors);
                }
            }
        }
    }
}

fn validate_notes(notes: &[Note], parent_path: &str, errors: &mut Vec<ValidationError>) {
    let mut note_ids = HashSet::new();
    for note in notes {
        if let Some(ref id) = note.id
            && !note_ids.insert(id)
        {
            errors.push(ValidationError {
                path: format!("{parent_path}/note[@id='{id}']"),
                message: format!("duplicate note id \"{id}\""),
            });
        }
        if let Some(pri) = note.priority
            && !(1..=10).contains(&pri)
        {
            errors.push(ValidationError {
                path: format!("{parent_path}/note"),
                message: format!("note priority must be 1-10, got {pri}"),
            });
        }
    }
}

fn validate_inline_codes(content: &Content, path: &str, errors: &mut Vec<ValidationError>) {
    let mut span_code_ids = HashSet::new();
    let mut annotation_ids = HashSet::new();
    collect_code_and_annotation_ids(&content.elements, &mut span_code_ids, &mut annotation_ids);
    check_ref_pairing(
        &content.elements,
        &span_code_ids,
        &annotation_ids,
        path,
        errors,
    );
}

fn collect_code_and_annotation_ids<'a>(
    elements: &'a [InlineElement],
    span_code_ids: &mut HashSet<&'a str>,
    annotation_ids: &mut HashSet<&'a str>,
) {
    for elem in elements {
        match elem {
            InlineElement::Sc(sc) => {
                span_code_ids.insert(&sc.id);
            }
            InlineElement::Sm(sm) => {
                annotation_ids.insert(&sm.id);
            }
            InlineElement::Pc(pc) => {
                collect_code_and_annotation_ids(&pc.content, span_code_ids, annotation_ids);
            }
            InlineElement::Mrk(mrk) => {
                collect_code_and_annotation_ids(&mrk.content, span_code_ids, annotation_ids);
            }
            _ => {}
        }
    }
}

fn check_ref_pairing(
    elements: &[InlineElement],
    span_code_ids: &HashSet<&str>,
    annotation_ids: &HashSet<&str>,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    for elem in elements {
        match elem {
            InlineElement::Ec(ec)
                if ec
                    .start_ref
                    .as_deref()
                    .is_some_and(|sr| !span_code_ids.contains(sr)) =>
            {
                errors.push(ValidationError {
                    path: path.to_owned(),
                    message: format!(
                        "<ec> startRef \"{}\" has no matching <sc>",
                        ec.start_ref.as_deref().unwrap_or_default()
                    ),
                });
            }
            InlineElement::Em(em) if !annotation_ids.contains(em.start_ref.as_str()) => {
                errors.push(ValidationError {
                    path: path.to_owned(),
                    message: format!("<em> startRef \"{}\" has no matching <sm>", em.start_ref),
                });
            }
            InlineElement::Pc(pc) => {
                check_ref_pairing(&pc.content, span_code_ids, annotation_ids, path, errors);
            }
            InlineElement::Mrk(mrk) => {
                check_ref_pairing(&mrk.content, span_code_ids, annotation_ids, path, errors);
            }
            _ => {}
        }
    }
}

fn validate_data_refs(
    content: &Content,
    od: &OriginalData,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    let data_ids: HashSet<&str> = od.entries.iter().map(|e| e.id.as_str()).collect();
    check_data_refs(&content.elements, &data_ids, path, errors);
}

fn check_data_refs(
    elements: &[InlineElement],
    data_ids: &HashSet<&str>,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    for elem in elements {
        match elem {
            InlineElement::Ph(ph) => {
                check_single_data_ref(
                    ph.data_ref.as_deref(),
                    "ph",
                    "dataRef",
                    data_ids,
                    path,
                    errors,
                );
            }
            InlineElement::Sc(sc) => {
                check_single_data_ref(
                    sc.data_ref.as_deref(),
                    "sc",
                    "dataRef",
                    data_ids,
                    path,
                    errors,
                );
            }
            InlineElement::Ec(ec) => {
                check_single_data_ref(
                    ec.data_ref.as_deref(),
                    "ec",
                    "dataRef",
                    data_ids,
                    path,
                    errors,
                );
            }
            InlineElement::Pc(pc) => {
                check_single_data_ref(
                    pc.data_ref_start.as_deref(),
                    "pc",
                    "dataRefStart",
                    data_ids,
                    path,
                    errors,
                );
                check_single_data_ref(
                    pc.data_ref_end.as_deref(),
                    "pc",
                    "dataRefEnd",
                    data_ids,
                    path,
                    errors,
                );
                check_data_refs(&pc.content, data_ids, path, errors);
            }
            InlineElement::Mrk(mrk) => {
                check_data_refs(&mrk.content, data_ids, path, errors);
            }
            _ => {}
        }
    }
}

fn check_single_data_ref(
    data_ref: Option<&str>,
    tag: &str,
    attr: &str,
    data_ids: &HashSet<&str>,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(dr) = data_ref
        && !data_ids.contains(dr)
    {
        errors.push(ValidationError {
            path: path.to_owned(),
            message: format!("<{tag}> {attr} \"{dr}\" not found in <originalData>"),
        });
    }
}
