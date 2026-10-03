//! OS-backed spell checking (`docs/desktop-ota-spec.md` Stage 4).
//!
//! The shell half of replacing Harper's spelling pass with the platform's
//! own. The motivation is not only quality: `brink-prose` is 6.15 MB gzipped
//! against the whole compiler's 2.61 MB, and OTA ships the bundle as one
//! archive, so every update pays for it whether or not the author writes a
//! sentence.
//!
//! **What this module can and cannot be tested on.** The native call is
//! macOS-only and cannot run in CI's Linux container or in a cloud session;
//! it is first exercised on the `macos-15` release runner, and its behaviour
//! is first seen by a person running the app. So the module is deliberately
//! shaped to make the *untestable* part as small as possible: the platform
//! call returns raw spans, and everything after it — slicing the word out,
//! honouring the project dictionary, capping the result — is pure, on this
//! side of the FFI, and covered by tests that run everywhere.
//!
//! Windows reports unavailable rather than wrong. `ISpellCheckerFactory` is
//! COM and would need `unsafe`, which this crate denies, and Windows is not
//! in the release matrix (`desktop-release.yml` builds macOS and Linux). A
//! platform that says "I cannot do this" costs an author nothing — the
//! frontend keeps using Harper there — while a platform that silently checks
//! badly costs them trust in every squiggle.

/// One misspelled span.
///
/// ⚠ **Offsets are UTF-16 code units, not bytes and not chars.** That is not
/// a concession to JavaScript, it is what all three sides already speak:
/// `NSString` is UTF-16, so `NSRange` comes back in those units; `CodeMirror`
/// indexes the same way; and `brink-prose` already converts to them through
/// `CharToUtf16` before returning. Converting here would mean converting
/// back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Misspelling {
    /// Start offset, in UTF-16 code units.
    pub start: usize,
    /// End offset (exclusive), in UTF-16 code units.
    pub end: usize,
    /// The word as it appears in the text, for dictionary actions.
    pub word: String,
    /// Replacements the platform suggests, best first. May be empty.
    pub suggestions: Vec<String>,
}

/// What a check produced.
///
/// `Unavailable` is a first-class answer rather than an error: on Linux and
/// Windows there is no native checker to consult, and that is a fact about
/// the platform rather than a failure of the call. The frontend falls back
/// to Harper; an error would read as something being broken.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
// `Checked` is only constructed by the macOS branch; off macOS the type is
// still the command's return shape, so the variant has to exist.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        dead_code,
        reason = "only the macOS branch constructs or reads this; off macOS it is still \
                  part of the command's return shape and of what the pure tests cover"
    )
)]
pub enum SpellcheckOutcome {
    /// No native checker on this platform (or it refused to start).
    Unavailable { reason: String },
    /// Checked; `misspellings` may be empty.
    Checked { misspellings: Vec<Misspelling> },
}

/// Most misspellings one call will report.
///
/// The repo's standing guard against unbounded growth, applied to a loop
/// whose bound is otherwise the document: a pathological paste — a base64
/// blob, a minified file — is every "word" misspelled, and neither the IPC
/// channel nor the squiggle layer wants a hundred thousand of them.
#[cfg_attr(
    not(target_os = "macos"),
    expect(
        dead_code,
        reason = "only the macOS branch constructs or reads this; off macOS it is still \
                  part of the command's return shape and of what the pure tests cover"
    )
)]
pub const MAX_MISSPELLINGS: usize = 2_000;

/// Slice a word out of a UTF-16 buffer.
///
/// Returns `None` for a range that runs off the end, which the platform
/// should never produce — but a silent `panic!` inside a `#[tauri::command]`
/// on a length mismatch would take the whole check down, and this crate
/// denies `panic` anyway.
#[cfg_attr(
    all(not(target_os = "macos"), not(test)),
    expect(
        dead_code,
        reason = "only the macOS branch constructs or reads this; off macOS it is still \
                  part of the command's return shape and of what the pure tests cover"
    )
)]
pub fn word_at(utf16: &[u16], start: usize, len: usize) -> Option<String> {
    let end = start.checked_add(len)?;
    let slice = utf16.get(start..end)?;
    Some(String::from_utf16_lossy(slice))
}

/// Drop anything the project's own dictionary already covers.
///
/// **Matched case-insensitively, deliberately.** An author who added
/// `Kaelen` means the name, and being strict would flag `KAELEN` in a shout
/// and `kaelen` in a compound — which is exactly the "every invented name is
/// a typo" failure the dictionary exists to prevent. The cost is that a
/// genuinely wrong casing goes unflagged, which is a style question no
/// spellchecker was going to settle.
#[cfg_attr(
    all(not(target_os = "macos"), not(test)),
    expect(
        dead_code,
        reason = "only the macOS branch constructs or reads this; off macOS it is still \
                  part of the command's return shape and of what the pure tests cover"
    )
)]
pub fn apply_ignore_list(found: Vec<Misspelling>, ignored: &[String]) -> Vec<Misspelling> {
    if ignored.is_empty() {
        return found;
    }
    let ignored: Vec<String> = ignored.iter().map(|word| word.to_lowercase()).collect();
    found
        .into_iter()
        .filter(|candidate| {
            let lowered = candidate.word.to_lowercase();
            !ignored.contains(&lowered)
        })
        .collect()
}

/// Check `text` with the platform's spell checker.
///
/// `language` is a BCP-47 tag (`en_GB`, `en_US`); `None` lets the platform
/// use the author's own preference, which is the right default — their OS
/// dictionary is already set up the way they write.
#[cfg(target_os = "macos")]
pub fn check(text: &str, language: Option<&str>, ignored: &[String]) -> SpellcheckOutcome {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::NSString;

    let utf16: Vec<u16> = text.encode_utf16().collect();
    let Ok(length) = isize::try_from(utf16.len()) else {
        return SpellcheckOutcome::Unavailable {
            reason: "the document is too large to check".to_owned(),
        };
    };

    let checker = NSSpellChecker::sharedSpellChecker();
    let ns_text = NSString::from_str(text);
    let ns_language = language.map(NSString::from_str);

    let mut misspellings = Vec::new();
    let mut cursor: isize = 0;
    while cursor < length && misspellings.len() < MAX_MISSPELLINGS {
        let range = checker.checkSpellingOfString_startingAt(&ns_text, cursor);
        // NSNotFound, or a zero-length hit: nothing more to find.
        if range.length == 0 || range.location >= utf16.len() {
            break;
        }

        if let Some(word) = word_at(&utf16, range.location, range.length) {
            let suggestions = checker
                .guessesForWordRange_inString_language_inSpellDocumentWithTag(
                    range,
                    &ns_text,
                    ns_language.as_deref(),
                    0,
                )
                .map(|guesses| guesses.iter().map(|guess| guess.to_string()).collect())
                .unwrap_or_default();
            misspellings.push(Misspelling {
                start: range.location,
                end: range.location + range.length,
                word,
                suggestions,
            });
        }

        // The cursor MUST advance or this loop never ends. A platform that
        // returned the same range twice would otherwise hang the command.
        let Ok(next) = isize::try_from(range.location + range.length) else {
            break;
        };
        if next <= cursor {
            break;
        }
        cursor = next;
    }

    SpellcheckOutcome::Checked {
        misspellings: apply_ignore_list(misspellings, ignored),
    }
}

/// No native checker here — see the module note on why this is an answer
/// rather than an error.
#[cfg(not(target_os = "macos"))]
pub fn check(_text: &str, _language: Option<&str>, _ignored: &[String]) -> SpellcheckOutcome {
    SpellcheckOutcome::Unavailable {
        reason: "this platform has no OS spell checker wired up".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_ignore_list, word_at, Misspelling};

    fn misspelling(word: &str) -> Misspelling {
        Misspelling {
            start: 0,
            end: word.encode_utf16().count(),
            word: word.to_owned(),
            suggestions: Vec::new(),
        }
    }

    /// The offsets are UTF-16, and the only text where that is observable is
    /// text with astral characters — where bytes, chars and UTF-16 units all
    /// disagree. An emoji is two code units; getting this wrong shifts every
    /// squiggle after it.
    #[test]
    fn word_at_slices_in_utf16_units_not_bytes_or_chars() {
        let text = "a 🎭 bda";
        let utf16: Vec<u16> = text.encode_utf16().collect();
        // "🎭" is ONE char, FOUR bytes, TWO utf-16 units — so "bda" starts at
        // 5 in utf-16 units and would be at 4 counting chars.
        assert_eq!(word_at(&utf16, 5, 3).as_deref(), Some("bda"));
        assert_eq!(word_at(&utf16, 2, 2).as_deref(), Some("🎭"));
    }

    /// A range past the end must not panic: this crate denies `panic`, and a
    /// panic inside the command would take the whole check down over one bad
    /// span.
    #[test]
    fn word_at_refuses_a_range_past_the_end_rather_than_panicking() {
        let utf16: Vec<u16> = "short".encode_utf16().collect();
        assert_eq!(word_at(&utf16, 3, 99), None);
        assert_eq!(word_at(&utf16, 99, 1), None);
        assert_eq!(
            word_at(&utf16, 1, usize::MAX),
            None,
            "start + len overflows"
        );
    }

    /// The dictionary is what stops every invented name reading as a typo,
    /// so it matches in any casing — a shout and a sentence-initial use are
    /// the same name.
    #[test]
    fn the_project_dictionary_silences_a_word_in_any_casing() {
        let found = vec![
            misspelling("Kaelen"),
            misspelling("KAELEN"),
            misspelling("kaelen"),
        ];
        let kept = apply_ignore_list(found, &["kaelen".to_owned()]);
        assert!(
            kept.is_empty(),
            "one entry must cover every casing: {kept:?}"
        );
    }

    /// And it silences only what it names.
    #[test]
    fn the_project_dictionary_leaves_everything_else_alone() {
        let found = vec![misspelling("Kaelen"), misspelling("teh")];
        let kept = apply_ignore_list(found, &["Kaelen".to_owned()]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].word, "teh");
    }

    /// An empty dictionary is the common case and must not cost a pass.
    #[test]
    fn an_empty_dictionary_keeps_everything() {
        let found = vec![misspelling("teh"), misspelling("recieve")];
        assert_eq!(apply_ignore_list(found.clone(), &[]).len(), found.len());
    }
}
