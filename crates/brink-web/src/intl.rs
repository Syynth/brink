//! The localization round trip, in wasm (`docs/desktop-ota-spec.md` Stage 1).
//!
//! These three functions are the whole reason the `brink-cli` **sidecar**
//! existed. The desktop shell shipped a native `brink-cli` binary inside its
//! signed `.app` purely so `export-xliff` had somewhere to run, which coupled
//! the desktop app's intl features to a second copy of the compiler core —
//! and that coupling is what made an over-the-air web-bundle update unsafe.
//! `.inkb`'s container check is exact-match, so a wasm updated ahead of the
//! sidecar would emit artifacts the sidecar's reader refuses, silently, with
//! nothing else looking wrong. See the spec and `docs/decision-log.md`
//! (2026-09-14) for why the boundary is deleted rather than gated.
//!
//! The port is thin because the operations were already pure. The CLI's
//! wrappers are `fs::read` → a `brink_intl`/`xliff2` call → `fs::write`, with
//! no traversal and nothing subprocess-shaped, so all that moves is where the
//! bytes come from and go: the host reads and writes through its own file
//! APIs and hands these functions the contents.
//!
//! Every function takes `.inkb` bytes rather than a path. The CLI's
//! `export-xliff` also accepts a non-`.inkb` input — it compiles the source
//! in memory and passes checksum `0`, because there is no header to read one
//! out of. That branch has no analogue here: the only producer on this side
//! is a compile that just returned `.inkb` bytes, so the real checksum is
//! always available.
//!
//! ⚠ That is an OBSERVABLE change for the desktop shell, which passes its
//! entry `.ink` today and therefore takes the source branch: its exported
//! `.xlf` carries `brink:checksum="0x00000000"`, and through wasm it will
//! carry the artifact's real CRC. Nothing reads the attribute back —
//! `compile_locale` stamps the `.inkl`'s `base_checksum` from the base
//! `.inkb` it is handed (`brink-intl/src/compile.rs:105`), never from the
//! document, and the runtime's check (`brink-runtime/src/locale.rs:33`)
//! compares that stamp against the program. So the attribute is provenance
//! only, and the real value is the useful one: it names which artifact a
//! translator's file was cut from, which `0` cannot.

use wasm_bindgen::prelude::*;

// The logic lives in plain `Result<_, String>` functions and the
// `#[wasm_bindgen]` entry points do nothing but map the error. That is not
// ceremony: `JsError::new` is a wasm import stub that PANICS when called on a
// native target, so anything constructing one is unreachable under
// `cargo test -p brink-web --lib` — a `JsError`-returning signature makes
// every failure path untestable. Splitting the seam keeps the error paths
// covered by the same suite that covers the happy ones, which matters here
// because `panic` has no test carve-out in this repo and a wasm panic is an
// unrecoverable trap for the embedder rather than a catchable exception.

/// Reads `.inkb` bytes into a `StoryData`.
///
/// `StoryData::source_checksum` is the CRC the reader lifts straight out of
/// the header index (`inkb/read.rs:95`) rather than anything recomputed, so
/// it is the same value the header carries — and it is what binds an `.xlf`
/// to the exact artifact it was generated from.
fn read_story(story_bytes: &[u8]) -> Result<brink_format::StoryData, String> {
    brink_format::read_inkb(story_bytes).map_err(|e| format!("decode error: {e}"))
}

fn export_xliff_inner(
    story_bytes: &[u8],
    src_lang: &str,
    trg_lang: Option<&str>,
) -> Result<String, String> {
    let data = read_story(story_bytes)?;
    let doc = brink_intl::generate_locale(&data, data.source_checksum, src_lang, trg_lang);
    xliff2::write::to_string(&doc).map_err(|e| format!("xliff write error: {e}"))
}

fn compile_locale_inner(
    base_bytes: &[u8],
    xliff_text: &str,
    locale: &str,
) -> Result<Vec<u8>, String> {
    let doc = xliff2::read::read_xliff(xliff_text).map_err(|e| format!("xliff read error: {e}"))?;
    brink_intl::compile_locale_xliff(base_bytes, &doc, locale)
        .map_err(|e| format!("compile-locale error: {e}"))
}

fn regenerate_xliff_inner(
    base_bytes: &[u8],
    existing_xliff: &str,
    src_lang: &str,
) -> Result<String, String> {
    let data = read_story(base_bytes)?;
    let existing_doc =
        xliff2::read::read_xliff(existing_xliff).map_err(|e| format!("xliff read error: {e}"))?;
    let merged =
        brink_intl::regenerate_locale(&data, data.source_checksum, src_lang, &existing_doc)
            .map_err(|e| format!("regenerate error: {e}"))?;
    xliff2::write::to_string(&merged).map_err(|e| format!("xliff write error: {e}"))
}

/// Generates an XLIFF 2 document for `story_bytes`, as XML text.
///
/// `trg_lang` is optional: omitted, the document carries source text only,
/// which is the shape a translator is handed first.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the wasm ABI forces it: wasm-bindgen does not implement \
              OptionFromWasmAbi for &str, so an optional string argument must \
              be owned. Clippy's suggested Option<&String> does not compile \
              here either. The owned value is consumed immediately — \
              export_xliff_inner takes Option<&str>."
)]
#[wasm_bindgen]
pub fn export_xliff(
    story_bytes: &[u8],
    src_lang: &str,
    trg_lang: Option<String>,
) -> Result<String, JsError> {
    export_xliff_inner(story_bytes, src_lang, trg_lang.as_deref()).map_err(|e| JsError::new(&e))
}

/// Compiles a translated XLIFF document against its base `.inkb` into `.inkl`
/// overlay bytes for `locale`.
///
/// ⚠ The overlay carries a `base_checksum` matching the artifact it was built
/// against, so an `.inkl` is invalidated by ANY byte change to that `.inkb`.
/// That is why optimization has to precede localization, and it is equally
/// why this belongs on the same side of the wire as the compile that produced
/// the base (`docs/desktop-ota-spec.md`).
#[wasm_bindgen]
pub fn compile_locale(
    base_bytes: &[u8],
    xliff_text: &str,
    locale: &str,
) -> Result<Vec<u8>, JsError> {
    compile_locale_inner(base_bytes, xliff_text, locale).map_err(|e| JsError::new(&e))
}

/// Merges an existing XLIFF document forward onto a newer `.inkb`, returning
/// the regenerated XML.
///
/// This is the "the story changed, keep the translations that still apply"
/// operation: entries whose source text still matches carry their targets
/// over, and everything else is re-emitted untranslated.
#[wasm_bindgen]
pub fn regenerate_xliff(
    base_bytes: &[u8],
    existing_xliff: &str,
    src_lang: &str,
) -> Result<String, JsError> {
    regenerate_xliff_inner(base_bytes, existing_xliff, src_lang).map_err(|e| JsError::new(&e))
}

#[cfg(test)]
mod tests {
    use super::{compile_locale_inner, export_xliff_inner, regenerate_xliff_inner};

    /// Compiles a story through the real wasm entry point and returns its
    /// `.inkb` bytes — the same bytes the desktop shell would hand these
    /// functions, rather than a hand-built fixture.
    fn story_bytes(source: &str) -> Vec<u8> {
        let json = crate::compile(source);
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(v["ok"], true, "{json}");
        v["story_bytes"]
            .as_array()
            .expect("story_bytes array")
            .iter()
            .map(|b| u8::try_from(b.as_u64().expect("byte")).expect("in range"))
            .collect()
    }

    const STORY: &str = "The lantern gutters.\nNot even close.\n-> END\n";

    #[test]
    fn export_carries_the_story_prose_into_a_parseable_xliff() {
        let xml = export_xliff_inner(&story_bytes(STORY), "en", None).expect("export");

        // Parseable by the same reader `compile_locale` would use, and
        // actually carrying the prose — not an empty well-formed shell.
        let doc = xliff2::read::read_xliff(&xml).expect("re-read the exported document");
        assert!(!doc.files.is_empty(), "exported document has no files");
        assert!(
            xml.contains("The lantern gutters."),
            "source text missing from the export:\n{xml}"
        );
    }

    #[test]
    fn export_honours_a_target_language() {
        let bytes = story_bytes(STORY);
        let without = export_xliff_inner(&bytes, "en", None).expect("export");
        let with = export_xliff_inner(&bytes, "en", Some("fr")).expect("export");
        assert_ne!(
            without, with,
            "trgLang made no difference to the exported document"
        );
        assert!(with.contains("fr"), "target language missing:\n{with}");
    }

    #[test]
    fn regenerate_merges_an_existing_document_forward() {
        let bytes = story_bytes(STORY);
        let existing = export_xliff_inner(&bytes, "en", None).expect("export");
        let merged = regenerate_xliff_inner(&bytes, &existing, "en").expect("regenerate");
        assert!(
            merged.contains("The lantern gutters."),
            "regenerated document lost the source text:\n{merged}"
        );
    }

    /// The exported document names the artifact it was cut from. The CLI's
    /// source-input branch emits `0x00000000` here because it has no header
    /// to read; going through `.inkb` bytes there is always a real one, and
    /// that difference is observable in a file authors hand to translators —
    /// so it is pinned rather than left to drift back.
    #[test]
    fn export_carries_the_artifacts_real_checksum_not_zero() {
        let bytes = story_bytes(STORY);
        let data = super::read_story(&bytes).expect("read the artifact back");
        assert_ne!(
            data.source_checksum, 0,
            "the compiled artifact has no checksum, so this test proves nothing"
        );

        let xml = export_xliff_inner(&bytes, "en", None).expect("export");
        let expected = format!("0x{:08x}", data.source_checksum);
        assert!(
            xml.contains(&expected),
            "export did not carry the artifact checksum {expected}:\n{xml}"
        );
        assert!(
            !xml.contains("0x00000000"),
            "export fell back to the CLI's source-branch placeholder:\n{xml}"
        );
    }

    /// Host-facing entry points must surface malformed input as an error the
    /// caller can catch. `panic` has no test carve-out in this repo and a
    /// wasm panic is an unrecoverable trap for the embedder, so this pins the
    /// behaviour rather than assuming it.
    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        assert!(export_xliff_inner(b"not an inkb artifact", "en", None).is_err());
        assert!(regenerate_xliff_inner(b"not an inkb artifact", "<xliff/>", "en").is_err());
        assert!(
            compile_locale_inner(&story_bytes(STORY), "not xliff at all", "fr").is_err(),
            "a malformed xliff document should be an error"
        );
    }
}
