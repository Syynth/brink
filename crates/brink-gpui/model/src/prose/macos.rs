//! macOS's own spell checker (`NSSpellChecker`): the spelling on macOS,
//! and its quick grammar when the author picks it
//! (`docs/gpui-prose-checker-spec.md` §5, §6).
//!
//! It honours the words the author has taught macOS anywhere ("Learn
//! Spelling" in Mail, Pages, …), and it takes the project's words as a
//! per-document ignore list, so they never reach the author's own
//! dictionary — `brink.toml` stays the one place a project's words live.
//! The list matches case-insensitively, which is ruled acceptable for this
//! checker (decision log 2026-10-08).
//!
//! **Each span is its own string.** Measured: handing the checker the whole
//! file with a sub-range costs about 80 ms a call, a span copied into its
//! own `NSString` about 1.3 ms. And typing moves one span, so the span
//! cache below makes a keystroke one call, not one per span in the file.
//!
//! **Threads.** Every call here happens on the worker's thread, inside an
//! autorelease pool (that thread has none of its own). Apple does not
//! document `NSSpellChecker` as safe off the main thread; objc2 does not
//! mark it main-thread-only, and the probe behind the spec made over a
//! thousand calls from a background thread without incident. Spec §9 has
//! the fallback if that ever changes.
//!
//! **One checker per process, and its language is global.** Each open
//! project has its own worker thread and its own [`Spelling`], but they
//! share `sharedSpellChecker`, whose language is a setting on it, not an
//! argument to a check. Two projects in different Englishes would set it
//! under each other's checks, so a check holds [`CHECKER`] from setting
//! the language to its last question.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{NSArray, NSDictionary, NSRange, NSString, NSTextCheckingType, NSValue};

use super::{Lint16, MAX_FIXES, ProseFix, SPELLING};

/// How long one check may spend asking for suggestions it has not cached.
/// `guessesForWordRange` costs 5–26 ms a word; past this, a misspelling
/// gets the checker's single best correction (about 1 ms) and a later
/// check fills the rest in.
const SUGGESTION_BUDGET: Duration = Duration::from_millis(100);

/// Spans whose misspellings are remembered, and words whose suggestions
/// are. Bounds, not targets (CLAUDE.md, "Guard against unbounded growth").
const SPAN_CACHE: usize = 4096;
const GUESS_CACHE: usize = 2048;

/// Held for the whole of a check: the shared checker's language is set at
/// its start and must still be that language at its end.
static CHECKER: Mutex<()> = Mutex::new(());

/// The checker, and what it keeps between checks. Lives in the worker
/// loop for one project; dropping it closes the spell document.
pub(super) struct OsChecker {
    checker: Retained<NSSpellChecker>,
    /// This project's spell document — the scope of the ignore list.
    tag: isize,
    /// The ignore list as last handed to the checker.
    words: Vec<String>,
    /// The language as last applied (`en_US`, …); empty before the first.
    language: &'static str,
    /// Whether the last check asked for grammar too.
    grammar: bool,
    /// A span's text → what the checker found in it, relative to the span.
    spans: Recent<Scan>,
    /// A word → the checker's suggestions for it, in `language`.
    guesses: Recent<Vec<String>>,
}

/// One span's answer, in UTF-16 code units from the span's start.
#[derive(Debug, Clone, Default)]
struct Scan {
    /// Misspelled words. Their fixes are looked up per check, under the
    /// suggestion budget, so they are not part of the span's answer.
    misses: Vec<Miss>,
    /// Grammar findings, complete with their fixes.
    grammar: Vec<Lint16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Miss {
    start: u32,
    end: u32,
    word: String,
}

impl OsChecker {
    pub(super) fn new() -> Self {
        Self {
            checker: NSSpellChecker::sharedSpellChecker(),
            tag: NSSpellChecker::uniqueSpellDocumentTag(),
            words: Vec::new(),
            language: "",
            grammar: false,
            spans: Recent::new(SPAN_CACHE),
            guesses: Recent::new(GUESS_CACHE),
        }
    }

    /// Check `spans` — each a span's text and where it starts in the file,
    /// in UTF-16 — and answer `(spelling, grammar)` as lints in the file's
    /// UTF-16. The grammar is the checker's quick rules plus whatever its
    /// model has cached for these sentences (spec §2.1), and only when
    /// `grammar` asks for it.
    pub(super) fn check(
        &mut self,
        spans: &[(&str, u32)],
        dictionary: &[String],
        dialect: Option<&str>,
        grammar: bool,
    ) -> (Vec<Lint16>, Vec<Lint16>) {
        // A poisoned lock guards nothing but the language, which this
        // check sets again first thing.
        let _checker = CHECKER.lock().unwrap_or_else(PoisonError::into_inner);
        autoreleasepool(|_| {
            self.configure(dictionary, dialect, grammar);
            let deadline = Instant::now() + SUGGESTION_BUDGET;
            let mut spelling = Vec::new();
            let mut found = Vec::new();
            for &(text, at) in spans {
                let scan = if let Some(scan) = self.spans.get(text) {
                    scan.clone()
                } else {
                    let scan = self.scan(text);
                    self.spans.insert(text.to_owned(), scan.clone());
                    scan
                };
                for miss in scan.misses {
                    let fixes = self.fixes(&miss.word, deadline);
                    spelling.push(Lint16 {
                        start: at + miss.start,
                        end: at + miss.end,
                        kind: SPELLING.to_owned(),
                        // Harper's wording, so the two studios read alike.
                        message: format!("Did you mean to spell `{}` this way?", miss.word),
                        fixes,
                    });
                }
                found.extend(scan.grammar.into_iter().map(|lint| Lint16 {
                    start: at + lint.start,
                    end: at + lint.end,
                    ..lint
                }));
            }
            (spelling, found)
        })
    }

    /// Hand the checker this project's words and English. The language is
    /// the shared checker's, so it is set again on every check (another
    /// project's check may have moved it); it is a setter, nearly free.
    /// The caches answer for one configuration and are dropped when it
    /// changes.
    fn configure(&mut self, dictionary: &[String], dialect: Option<&str>, grammar: bool) {
        let language = language_for(dialect);
        self.checker.setAutomaticallyIdentifiesLanguages(false);
        // `false` when the language is not installed; the checker then
        // keeps the one it had, which is all that could be done anyway.
        let _ = self.checker.setLanguage(&NSString::from_str(language));
        if language != self.language {
            self.language = language;
            self.spans.clear();
            self.guesses.clear();
        }
        if grammar != self.grammar {
            self.grammar = grammar;
            self.spans.clear();
        }
        if dictionary != self.words.as_slice() {
            let words: Vec<Retained<NSString>> =
                dictionary.iter().map(|w| NSString::from_str(w)).collect();
            self.checker.setIgnoredWords_inSpellDocumentWithTag(
                &NSArray::from_retained_slice(&words),
                self.tag,
            );
            self.words = dictionary.to_vec();
            self.spans.clear();
        }
    }

    /// Ask the checker about one span.
    fn scan(&self, text: &str) -> Scan {
        let string = NSString::from_str(text);
        let whole = NSRange::new(0, string.length());
        // Grammar is never asked for alone: on its own the checker answers
        // nothing at all (spec §2.1).
        let types = if self.grammar {
            NSTextCheckingType::Spelling.0 | NSTextCheckingType::Grammar.0
        } else {
            NSTextCheckingType::Spelling.0
        };
        // SAFETY: no options dictionary is passed, so there is no generic
        // to get wrong, and the word count is not wanted — the binding
        // accepts a null pointer there.
        let results = unsafe {
            self.checker
                .checkString_range_types_options_inSpellDocumentWithTag_orthography_wordCount(
                    &string,
                    whole,
                    types,
                    None,
                    self.tag,
                    None,
                    std::ptr::null_mut(),
                )
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut scan = Scan::default();
        for result in &results {
            let range = result.range();
            if result.resultType() == NSTextCheckingType::Spelling {
                if let Some(miss) = miss(&units, range) {
                    scan.misses.push(miss);
                }
            } else if result.resultType() == NSTextCheckingType::Grammar {
                let details = result
                    .grammarDetails()
                    .map(|d| d.to_vec())
                    .unwrap_or_default();
                scan.grammar.extend(
                    details
                        .iter()
                        .filter_map(|d| grammar_lint(&units, range, d)),
                );
            }
            // Anything else (an orthography result comes back on some
            // inputs) is not a finding.
        }
        scan
    }

    /// A misspelling's fixes: the cached suggestions, fresh ones while the
    /// check's budget lasts, and past it the single best correction.
    fn fixes(&mut self, word: &str, deadline: Instant) -> Vec<ProseFix> {
        let found = if let Some(guesses) = self.guesses.get(word) {
            guesses.clone()
        } else if Instant::now() < deadline {
            let guesses = self.guess(word);
            self.guesses.insert(word.to_owned(), guesses.clone());
            guesses
        } else {
            // Not cached: the next check with budget to spare asks properly.
            self.correction(word).into_iter().collect()
        };
        found
            .into_iter()
            .take(MAX_FIXES)
            .map(ProseFix::Replace)
            .collect()
    }

    fn guess(&self, word: &str) -> Vec<String> {
        let string = NSString::from_str(word);
        self.checker
            .guessesForWordRange_inString_language_inSpellDocumentWithTag(
                NSRange::new(0, string.length()),
                &string,
                Some(&NSString::from_str(self.language)),
                self.tag,
            )
            .map(|guesses| guesses.iter().map(|g| g.to_string()).collect())
            .unwrap_or_default()
    }

    fn correction(&self, word: &str) -> Option<String> {
        let string = NSString::from_str(word);
        self.checker
            .correctionForWordRange_inString_language_inSpellDocumentWithTag(
                NSRange::new(0, string.length()),
                &string,
                &NSString::from_str(self.language),
                self.tag,
            )
            .map(|c| c.to_string())
    }
}

impl Drop for OsChecker {
    fn drop(&mut self) {
        self.checker.closeSpellDocumentWithTag(self.tag);
    }
}

/// A spelling result as a [`Miss`]; `None` for a range that does not fit
/// the span, which the checker should never send.
fn miss(units: &[u16], range: NSRange) -> Option<Miss> {
    let end = range.location.checked_add(range.length)?;
    Some(Miss {
        word: String::from_utf16_lossy(units.get(range.location..end)?),
        start: u32::try_from(range.location).ok()?,
        end: u32::try_from(end).ok()?,
    })
}

/// One entry of a grammar result's details as a lint, relative to the span
/// (spec §7.4). Every key is read as optional: they are AppKit's, several
/// arrived only with macOS 27, and a missing one should cost a detail of
/// the lint, not the lint. `None` only when no range in the span is left.
fn grammar_lint(
    units: &[u16],
    sentence: NSRange,
    detail: &NSDictionary<NSString, AnyObject>,
) -> Option<Lint16> {
    let get = |key: &str| detail.objectForKey(&NSString::from_str(key));
    let text = |key: &str| {
        get(key)
            .and_then(|o| o.downcast::<NSString>().ok())
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
    };
    // `NSGrammarRange` is relative to the sentence the result covers; a
    // detail without one is about the whole sentence.
    let (start, len) = match get("NSGrammarRange")
        .and_then(|o| o.downcast::<NSValue>().ok())
        .and_then(|v| v.get_range())
    {
        Some(inner) => (sentence.location.checked_add(inner.location)?, inner.length),
        None => (sentence.location, sentence.length),
    };
    let end = start.checked_add(len)?;
    if len == 0 || end > units.len() {
        return None;
    }
    let fixes = get("NSGrammarCorrections")
        .and_then(|o| o.downcast::<NSArray>().ok())
        .map(|corrections| {
            corrections
                .iter()
                .filter_map(|c| c.downcast::<NSString>().ok())
                .map(|c| ProseFix::Replace(c.to_string()))
                .take(MAX_FIXES)
                .collect()
        })
        .unwrap_or_default();
    Some(Lint16 {
        start: u32::try_from(start).ok()?,
        end: u32::try_from(end).ok()?,
        // "Word Usage" → `WordUsage`: a kind is half of a `prose.<kind>`
        // code, and a code has no spaces.
        kind: text("NSGrammarSystemCategory")
            .map_or_else(|| "Grammar".to_owned(), |c| c.split_whitespace().collect()),
        message: text("NSGrammarUserDescription").unwrap_or_else(|| "Grammar".to_owned()),
        fixes,
    })
}

/// `[prose] dialect` as the checker's language. The spellings are
/// `ProseDialect::as_str`'s; anything else is the default, American, as
/// Harper reads it.
fn language_for(dialect: Option<&str>) -> &'static str {
    match dialect {
        Some("british") => "en_GB",
        Some("canadian") => "en_CA",
        Some("australian") => "en_AU",
        _ => "en_US",
    }
}

/// A bounded cache by string key: two generations, the older dropped
/// whole when the newer fills, and a hit in the older moved to the newer.
/// Least-recently-used in effect, without the bookkeeping, and nothing is
/// ever iterated, so the hash order cannot reach an answer.
struct Recent<V> {
    now: HashMap<String, V>,
    before: HashMap<String, V>,
    cap: usize,
}

impl<V> Recent<V> {
    fn new(cap: usize) -> Self {
        Self {
            now: HashMap::new(),
            before: HashMap::new(),
            cap,
        }
    }

    fn get(&mut self, key: &str) -> Option<&V> {
        if !self.now.contains_key(key)
            && let Some(value) = self.before.remove(key)
        {
            self.insert(key.to_owned(), value);
        }
        self.now.get(key)
    }

    fn insert(&mut self, key: String, value: V) {
        if self.now.len() >= self.cap {
            self.before = std::mem::take(&mut self.now);
        }
        self.now.insert(key, value);
    }

    fn clear(&mut self) {
        self.now.clear();
        self.before.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_and_words(lints: &[Lint16], text: &str) -> Vec<(String, String)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        lints
            .iter()
            .map(|l| {
                (
                    l.kind.clone(),
                    String::from_utf16_lossy(&units[l.start as usize..l.end as usize]),
                )
            })
            .collect()
    }

    #[test]
    fn a_misspelling_lands_where_it_is_in_the_file() {
        let mut spelling = OsChecker::new();
        // Two spans of one file; the second starts at UTF-16 unit 20.
        let file = "Rain fell. 🌊 She… recieve the letter.";
        let units: Vec<u16> = file.encode_utf16().collect();
        let at = 14;
        let second = String::from_utf16_lossy(&units[at..]);
        let lints = spelling
            .check(&[("Rain fell.", 0), (&second, at as u32)], &[], None, false)
            .0;
        assert_eq!(
            kinds_and_words(&lints, file),
            vec![("Spelling".to_owned(), "recieve".to_owned())]
        );
        let fixes = &lints[0].fixes;
        assert!(
            fixes.contains(&ProseFix::Replace("receive".to_owned())),
            "the checker's suggestion comes with it: {fixes:?}"
        );
    }

    #[test]
    fn the_projects_words_pass_in_any_case() {
        let mut spelling = OsChecker::new();
        let text = "Kaelen nodded. KAELEN shouted. kaelen whispered.";
        let without = spelling.check(&[(text, 0)], &[], None, false).0;
        assert!(
            !without.is_empty(),
            "an invented name is flagged by default"
        );
        let with = spelling
            .check(&[(text, 0)], &["Kaelen".to_owned()], None, false)
            .0;
        assert!(
            with.is_empty(),
            "and passes once it is the project's: {with:?}"
        );
    }

    #[test]
    fn the_dialect_picks_the_english() {
        let mut spelling = OsChecker::new();
        let text = "The colour of the harbour.";
        let us = spelling.check(&[(text, 0)], &[], Some("american"), false).0;
        assert_eq!(us.len(), 2, "American flags both: {us:?}");
        let gb = spelling.check(&[(text, 0)], &[], Some("british"), false).0;
        assert!(gb.is_empty(), "British accepts them: {gb:?}");
        let back = spelling.check(&[(text, 0)], &[], None, false).0;
        assert_eq!(
            back.len(),
            2,
            "and the cache did not keep the British answer"
        );
    }

    /// The quick rules, which answer synchronously. The system's model
    /// cache may answer instead for a sentence it has seen (spec §2.1), so
    /// these pin where a finding is and what it offers, not its wording.
    #[test]
    fn grammar_comes_back_where_it_is_with_its_fix() {
        let mut checker = OsChecker::new();
        let text = "She ate a apple. He opened the the door.";
        let (spelling, grammar) = checker.check(&[(text, 7)], &[], None, true);
        assert!(spelling.is_empty(), "nothing misspelled: {spelling:?}");
        let units: Vec<u16> = text.encode_utf16().collect();
        let found: Vec<(String, Vec<ProseFix>)> = grammar
            .iter()
            .map(|l| {
                let (a, b) = (l.start as usize - 7, l.end as usize - 7);
                (String::from_utf16_lossy(&units[a..b]), l.fixes.clone())
            })
            .collect();
        assert!(
            found.contains(&("a".to_owned(), vec![ProseFix::Replace("an".to_owned())])),
            "the article, at the span's offset: {found:?}"
        );
        assert!(
            found.contains(&(
                "the the".to_owned(),
                vec![ProseFix::Replace("the".to_owned())]
            )),
            "the doubled word: {found:?}"
        );
        assert!(
            grammar
                .iter()
                .all(|l| l.kind != SPELLING && !l.kind.contains(' ')),
            "a category, as a code can carry it: {grammar:?}"
        );
    }

    #[test]
    fn grammar_is_asked_for_only_when_wanted() {
        let mut checker = OsChecker::new();
        let text = "She ate a apple.";
        let (_, off) = checker.check(&[(text, 0)], &[], None, false);
        assert!(off.is_empty(), "spelling only: {off:?}");
        // Same text, now with grammar: the span cache must not answer for
        // the spelling-only check.
        let (_, on) = checker.check(&[(text, 0)], &[], None, true);
        assert!(
            !on.is_empty(),
            "the cache did not keep the spelling-only answer"
        );
    }

    #[test]
    fn the_cache_stays_within_its_bound_and_keeps_what_is_used() {
        let mut cache = Recent::new(2);
        cache.insert("a".to_owned(), 1);
        cache.insert("b".to_owned(), 2);
        cache.insert("c".to_owned(), 3); // `a` and `b` age into `before`
        assert_eq!(cache.get("a"), Some(&1), "an older hit is moved forward");
        cache.insert("d".to_owned(), 4); // `now` was full: `b` is dropped
        assert_eq!(cache.get("b"), None);
        assert!(
            cache.now.len() + cache.before.len() <= 4,
            "at most two generations"
        );
    }
}
