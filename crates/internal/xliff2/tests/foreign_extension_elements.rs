//! #1824: `read_inline_content`'s catch-alls silently discard translator
//! text carried inside an XML element the reader does not recognize.
//!
//! `read_inline_content` (`crates/internal/xliff2/src/read/inline.rs`)
//! already matches every element in XLIFF 2.0's closed inline-markup
//! vocabulary (Core, §5.4-5.6: `<ph>`, `<pc>`, `<sc>`/`<ec>`, `<mrk>`,
//! `<sm>`/`<em>`; plus `<cp>`, the code-point escape). Anything else
//! encountered inside a `<source>`/`<target>` is by construction a
//! TMS-authored extension element (XLIFF 2.0 §10, Extensions) — brink's own
//! exporter never emits an inline element outside that closed set. Before
//! this fix:
//!
//! - An unrecognized `Event::Start` routed to `skip_element`, which
//!   discards every byte of text nested inside it — the exact "same class
//!   of bug as #1799/#1811/#1812" (silent drop of translator work), just
//!   one crate down from where #1821 closed it in `brink-intl`.
//! - An unrecognized `Event::Empty` hit a bare `_ => {}` no-op — safe by
//!   construction (an empty element is self-closing, so it can never carry
//!   character data), but undocumented as such, which this file also pins.
//!
//! These tests read a raw, hand-written TMS-shaped `.xlf` string through
//! `xliff2::read::read_xliff` — the exact function `brink compile-locale`
//! and `brink regenerate-xliff` call on a translator-returned file
//! (`crates/brink-cli/src/main.rs::run_compile_locale`/`run_regenerate_xliff`)
//! — so this exercises the real XML-reading boundary, not a hand-built
//! `Document`.

use xliff2::{InlineElement, SubUnit};

/// Parse `xml` and return the `<target>` inline elements of its first
/// segment. Returns `Err` (rather than unwrapping/panicking, per the house
/// rule that a `panic!`/`unwrap_or_else(|| panic!)` in a test *helper* is
/// not exempt from `clippy::panic`) so every caller — a `#[test]` fn, where
/// `.unwrap()` is the house convention — does its own unwrapping.
fn target_elements(xml: &str) -> Result<Vec<InlineElement>, String> {
    let doc = xliff2::read::read_xliff(xml).map_err(|e| e.to_string())?;
    let SubUnit::Segment(seg) = &doc.files[0].units[0].sub_units[0] else {
        return Err("expected a <segment> sub-unit".to_owned());
    };
    let target = seg
        .target
        .as_ref()
        .ok_or_else(|| "expected a <target>".to_owned())?;
    Ok(target.elements.clone())
}

/// A single foreign wrapper element (`<mq:comment>`, a memoQ-style QA
/// annotation) directly wraps translator text with no other structure.
/// This is the minimal case: one `Event::Start` catch-all, one run of text.
#[test]
fn text_inside_a_single_unknown_wrapper_survives() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root">
    <unit id="u1">
      <segment state="translated">
        <source>Hello world</source>
        <target><mq:comment id="c1">le monde</mq:comment></target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    assert_eq!(
        target_elements(xml).unwrap(),
        vec![InlineElement::Text("le monde".to_owned())],
        "text wrapped in a single unrecognized extension element must survive, \
         not vanish into skip_element"
    );
}

/// Nested depth, CDATA, and mixed known/unknown siblings all in one
/// `<target>`: `<mq:comment>` (unknown) wraps `<mq:reason>` (unknown,
/// nested one level deeper) containing a CDATA section, followed by a
/// sibling text run inside the same `<mq:comment>`; the comment is
/// followed by an unrecognized *empty* element (`<mq:flag/>`, the
/// `Event::Empty` catch-all) and then more plain text.
///
/// Every one of translator's text fragments — "Bonjour ", the CDATA
/// reason, "le monde", and "!" — must reach `elements`. Adjacent `Text`
/// nodes across a splice boundary are not expected to merge (matching
/// `elements_to_parts`'s foreign-`<mrk>` precedent in #1821, which also
/// leaves spliced-in text as separate `Literal` parts) — only nodes that
/// were already adjacent *before* a wrapper started coalesce.
#[test]
fn nested_depth_cdata_and_mixed_known_unknown_siblings_all_survive() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root">
    <unit id="u1">
      <segment state="translated">
        <source>Hello world</source>
        <target>Bonjour <mq:comment id="c1"><mq:reason><![CDATA[QA <flag>]]></mq:reason>le monde</mq:comment><mq:flag id="f1"/>!</target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    assert_eq!(
        target_elements(xml).unwrap(),
        vec![
            InlineElement::Text("Bonjour ".to_owned()),
            InlineElement::CData("QA <flag>".to_owned()),
            InlineElement::Text("le monde!".to_owned()),
        ],
        "nested unknown elements, CDATA, an unknown empty-element sibling, \
         and surrounding known text must all survive together"
    );
}

/// A *known* inline element (`<ph>`) nested inside an unknown wrapper must
/// still decode as itself, not get flattened into opaque skipped bytes —
/// proving the fix recurses through `read_inline_content` (which still
/// dispatches on recognized names) rather than just harvesting raw text.
#[test]
fn known_element_nested_inside_unknown_wrapper_still_decodes() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root">
    <unit id="u1">
      <segment state="translated">
        <source>Hello world</source>
        <target><mq:comment id="c1">before <ph id="ph1" equiv="*"/> after</mq:comment></target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    assert_eq!(
        target_elements(xml).unwrap(),
        vec![
            InlineElement::Text("before ".to_owned()),
            InlineElement::Ph(xliff2::Ph {
                id: "ph1".to_owned(),
                data_ref: None,
                equiv: Some("*".to_owned()),
                disp: None,
                sub_type: None,
                extensions: xliff2::Extensions::default(),
            }),
            InlineElement::Text(" after".to_owned()),
        ],
        "a known <ph> nested inside an unrecognized wrapper must still \
         decode as InlineElement::Ph, not be swallowed as opaque text"
    );
}

/// The `Event::Empty` catch-all in isolation: an unrecognized *self-closing*
/// element carries attributes only (XML forbids character content on an
/// empty element), so there is no text it could ever lose. This pins that
/// reasoning down and proves it does not error or disturb its siblings.
#[test]
fn unknown_empty_element_alone_is_ignored_without_error() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root">
    <unit id="u1">
      <segment state="translated">
        <source>Hello world</source>
        <target>Bonjour <mq:flag id="f1" severity="low"/>le monde</target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    assert_eq!(
        target_elements(xml).unwrap(),
        vec![InlineElement::Text("Bonjour le monde".to_owned())],
        "an unrecognized empty element must be ignored without error, and \
         must not split or drop the surrounding text"
    );
}

/// Two levels of unknown-wrapper nesting (`<mq:a><mq:b>text</mq:b></mq:a>`)
/// proves the recursion is not hardcoded to a single level.
#[test]
fn two_levels_of_unknown_nesting_survive() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<xliff version="2.0" srcLang="en" trgLang="fr"
       xmlns="urn:oasis:names:tc:xliff:document:2.0"
       xmlns:mq="urn:tms:memoq:extensions:1.0">
  <file id="root">
    <unit id="u1">
      <segment state="translated">
        <source>Hello world</source>
        <target><mq:a id="a1"><mq:b id="b1">deep text</mq:b></mq:a></target>
      </segment>
    </unit>
  </file>
</xliff>"#;

    assert_eq!(
        target_elements(xml).unwrap(),
        vec![InlineElement::Text("deep text".to_owned())],
        "text nested two levels deep inside unknown wrappers must survive"
    );
}
