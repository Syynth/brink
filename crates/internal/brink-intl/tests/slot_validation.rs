//! Structural re-import validation for slot indices (#1445).
//!
//! `compile_locale` must reject a translated line whose template references
//! a slot index that does not exist in the corresponding *base* line —
//! whether that index arrives via a bare `{slot n}` interpolation or via a
//! plural/keyword `Select` branch. Left unchecked, the bad index compiles
//! silently and resolves to empty/default text at playback instead of
//! failing at compile-locale time.
//!
//! Each failure mode below is exercised by hand-editing a real,
//! `xliff2`-serialized `.xlf` document — the actual re-import path
//! (`brink compile-locale`), not just the internal `LinesJson` model.

#![allow(clippy::unwrap_used)]

use brink_format::{read_inkb_index, write_inkb};
use brink_intl::{IntlError, compile_locale_xliff, generate_locale};
use xliff2::{Document, InlineElement, SubUnit};

/// A base story with a single root line carrying exactly two interpolation
/// slots (`name` at index 0, `count` at index 1).
const TWO_SLOT_SRC: &str = "\
VAR name = \"Alice\"
VAR count = 3
Hello {name}, you have {count} coins.
-> END
";

fn make_base() -> (Vec<u8>, brink_format::StoryData) {
    let data = brink_compiler::compile("story.ink", |_p| Ok(TWO_SLOT_SRC.to_owned()))
        .unwrap()
        .data;
    let mut inkb = Vec::new();
    write_inkb(&data, &mut inkb);
    (inkb, data)
}

/// Copy source → target on every segment, simulating a translator who
/// copied the source XLIFF and edited the target in place (the common
/// "translate in the same file" XLIFF workflow).
fn fill_targets(mut doc: Document) -> Document {
    for file in &mut doc.files {
        for unit in &mut file.units {
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su {
                    seg.target = Some(seg.source.clone());
                }
            }
        }
    }
    doc
}

/// Rewrite every `<ph id="{old}">` in a segment's *target* to `{new}` —
/// simulating a translator (or a hand-edited `.xlf`) fat-fingering the slot
/// number in a `Ph` id.
fn corrupt_target_ph_id(mut doc: Document, old: &str, new: &str) -> Document {
    for file in &mut doc.files {
        for unit in &mut file.units {
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su
                    && let Some(target) = &mut seg.target
                {
                    for el in &mut target.elements {
                        if let InlineElement::Ph(ph) = el
                            && ph.id == old
                        {
                            ph.id = new.to_string();
                        }
                    }
                }
            }
        }
    }
    doc
}

/// Round-trip a `Document` through real XLIFF XML text, as a genuine
/// re-import would (parse untrusted external `.xlf` bytes), instead of
/// handing the in-memory object straight to `compile_locale_xliff`.
fn through_xml(doc: &Document) -> Document {
    let xml = xliff2::write::to_string(doc).unwrap();
    xliff2::read::read_xliff(&xml).unwrap()
}

#[test]
fn slot_reference_out_of_range_rejected_on_reimport() {
    let (inkb, data) = make_base();
    let index = read_inkb_index(&inkb).unwrap();

    let doc = generate_locale(&data, index.checksum, "en", Some("es"));
    // Sanity: the base line's first slot ph is "s0".
    let xml = xliff2::write::to_string(&doc).unwrap();
    assert!(xml.contains("id=\"s0\""), "expected an s0 ph in: {xml}");

    // Translator's target references slot 7 — the base line has only two
    // slots (s0, s1).
    let doc = fill_targets(doc);
    let doc = corrupt_target_ph_id(doc, "s0", "s7");
    let doc = through_xml(&doc);

    let err = compile_locale_xliff(&inkb, &doc, "es").unwrap_err();
    assert!(
        matches!(
            err,
            IntlError::SlotIndexOutOfRange {
                slot: 7,
                slot_count: 2,
                ..
            }
        ),
        "expected SlotIndexOutOfRange{{slot: 7, slot_count: 2, ..}}, got {err:?}"
    );
    // The diagnostic must name the mismatch clearly.
    let msg = err.to_string();
    assert!(msg.contains('7'), "message should name the bad slot: {msg}");
    assert!(
        msg.contains('2'),
        "message should name the base slot count: {msg}"
    );
}

#[test]
fn select_branch_out_of_range_slot_rejected_on_reimport() {
    let (inkb, data) = make_base();
    let index = read_inkb_index(&inkb).unwrap();

    let doc = generate_locale(&data, index.checksum, "en", Some("fr"));
    let doc = fill_targets(doc);

    // A translator adds a plural `Select` branch for pluralization —
    // legitimate content the source line never had — but points it at
    // slot 9, which doesn't exist on a 2-slot base line.
    let mut select_variants = serde_json::Map::new();
    select_variants.insert(
        "cardinal:One".to_string(),
        serde_json::Value::String("piece".to_string()),
    );
    let select = brink_intl::SelectJson {
        slot: 9,
        variants: vec![select_variants],
        default: "pieces".to_string(),
    };
    let json = serde_json::to_string(&select).unwrap();
    let mut doc = doc;
    for file in &mut doc.files {
        for unit in &mut file.units {
            let mut touched = false;
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su
                    && let Some(target) = &mut seg.target
                {
                    target.elements.push(InlineElement::Ph(xliff2::Ph {
                        id: "sel0".to_string(),
                        data_ref: Some("dsel0".to_string()),
                        equiv: None,
                        disp: None,
                        sub_type: None,
                        extensions: xliff2::Extensions::default(),
                    }));
                    touched = true;
                }
            }
            if touched {
                unit.original_data = Some(xliff2::OriginalData {
                    entries: vec![xliff2::DataEntry {
                        id: "dsel0".to_string(),
                        content: json.clone(),
                    }],
                });
            }
        }
    }
    let doc = through_xml(&doc);

    let err = compile_locale_xliff(&inkb, &doc, "fr").unwrap_err();
    assert!(
        matches!(
            err,
            IntlError::SlotIndexOutOfRange {
                slot: 9,
                slot_count: 2,
                ..
            }
        ),
        "expected SlotIndexOutOfRange{{slot: 9, slot_count: 2, ..}}, got {err:?}"
    );
}

#[test]
fn reordered_in_range_slots_accepted() {
    let (inkb, data) = make_base();
    let index = read_inkb_index(&inkb).unwrap();

    let doc = generate_locale(&data, index.checksum, "en", Some("es"));
    let doc = fill_targets(doc);
    // Swap s0 <-> s1 in the target — a legitimate translation reordering
    // slots for target-language word order. `slot: 1` is exactly
    // `slot_count - 1`, the boundary valid index, and must not trip the
    // bounds check: only out-of-range indices are rejected.
    let doc = corrupt_target_ph_id(doc, "s0", "s_tmp");
    let doc = corrupt_target_ph_id(doc, "s1", "s0");
    let doc = corrupt_target_ph_id(doc, "s_tmp", "s1");
    let doc = through_xml(&doc);

    compile_locale_xliff(&inkb, &doc, "es")
        .expect("in-range, reordered slot references must compile");
}
