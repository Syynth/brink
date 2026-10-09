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
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Mutex, Once, PoisonError, mpsc};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::NSSpellChecker;
use objc2_foundation::{
    NSArray, NSDictionary, NSNumber, NSOrthography, NSRange, NSString, NSTextCheckingResult,
    NSTextCheckingType, NSValue,
};

use super::{
    Lint16, MAX_FIXES, ModelCheck, ModelChunk, ModelReport, ModelState, ProseFix, SPELLING,
};

/// How long one check may spend asking for suggestions it has not cached.
/// `guessesForWordRange` costs 5–26 ms a word; past this, a misspelling
/// gets the checker's single best correction (about 1 ms) and a later
/// check fills the rest in.
const SUGGESTION_BUDGET: Duration = Duration::from_millis(100);

/// Spans whose misspellings are remembered, and words whose suggestions
/// are. Bounds, not targets (CLAUDE.md, "Guard against unbounded growth").
const SPAN_CACHE: usize = 4096;
const GUESS_CACHE: usize = 2048;
/// Spans whose grammar-model findings are remembered.
const MODEL_CACHE: usize = 2048;

/// The option that makes a check wait for the grammar model (macOS 27's
/// `NSTextCheckingWaitForAllGrammarCheckingResultsKey`). Named by its
/// value, not linked: the symbol does not exist before macOS 27, and an
/// unknown option is ignored there — the canary then finds no model and
/// the command is not offered, which is the right answer for that Mac.
const WAIT_FOR_MODEL: &str = "WaitForAllGrammarCheckingResults";

/// A sentence only the model gets right (the quick rules pass it), with
/// the fix it offers. Measured on macOS 27.0.1.
const CANARY: &str = "There is three apples on the table.";
const CANARY_FIX: &str = "are";
/// How long the canary waits. A Mac that has run it before answers from
/// the system's cache in milliseconds; a fresh one takes seconds.
const CANARY_WAIT: Duration = Duration::from_secs(30);

/// Model requests in flight at once, per project. Eight concurrent ones
/// took 4.8 s together in the probe, so concurrency helps; this is on-device
/// compute on the author's battery, so it is capped.
const MODEL_IN_FLIGHT: usize = 4;
/// A model request not answered by then is counted as failed.
const MODEL_WAIT: Duration = Duration::from_secs(60);
/// Prose per model request, in bytes.
pub(super) const MODEL_CHUNK: usize = 2048;

/// [`ModelState`], process-wide: 0 unknown, 1 available, 2 unavailable.
static MODEL: AtomicU8 = AtomicU8::new(0);
static PROBE: Once = Once::new();
/// Model-check ids, process-wide, so an answer for a project since closed
/// can never be taken for one of the next project's.
static JOBS: AtomicU64 = AtomicU64::new(1);

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
    /// A span's text → what the grammar model found in it, relative to the
    /// span: filled by "Check Grammar with Apple Intelligence" (spec §7),
    /// read by every later check while the span's text is unchanged.
    model: Recent<Vec<Lint16>>,
    /// Model checks under way, oldest first.
    jobs: Vec<Job>,
    /// Model requests sent and not yet answered.
    in_flight: usize,
}

/// One "Check Grammar with Apple Intelligence": the spans it covers, in
/// chunks of about [`MODEL_CHUNK`], and how far it has got.
struct Job {
    id: u64,
    path: String,
    chunks: Vec<Vec<String>>,
    dictionary: Vec<String>,
    dialect: Option<String>,
    /// Chunks sent, and chunks answered (or given up on).
    sent: usize,
    done: usize,
    found: usize,
    timed_out: bool,
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
        // The first project to check prose finds out, once per launch,
        // whether the grammar model answers here.
        probe_model();
        Self {
            checker: NSSpellChecker::sharedSpellChecker(),
            tag: NSSpellChecker::uniqueSpellDocumentTag(),
            words: Vec::new(),
            language: "",
            grammar: false,
            spans: Recent::new(SPAN_CACHE),
            guesses: Recent::new(GUESS_CACHE),
            model: Recent::new(MODEL_CACHE),
            jobs: Vec::new(),
            in_flight: 0,
        }
    }

    /// Check `spans` — each a span's text and where it starts in the file,
    /// in UTF-16 — and answer `(spelling, grammar, model)` as lints in the
    /// file's UTF-16. `grammar` is the checker's quick rules plus whatever
    /// its model has cached for these sentences (spec §2.1), and only when
    /// asked for; `model` is what "Check Grammar with Apple Intelligence"
    /// found in spans whose text has not changed since.
    pub(super) fn check(
        &mut self,
        spans: &[(&str, u32)],
        dictionary: &[String],
        dialect: Option<&str>,
        grammar: bool,
    ) -> (Vec<Lint16>, Vec<Lint16>, Vec<Lint16>) {
        // A poisoned lock guards nothing but the language, which this
        // check sets again first thing.
        let _checker = CHECKER.lock().unwrap_or_else(PoisonError::into_inner);
        autoreleasepool(|_| {
            self.configure(dictionary, dialect, grammar);
            let deadline = Instant::now() + SUGGESTION_BUDGET;
            let mut spelling = Vec::new();
            let mut found = Vec::new();
            let mut model = Vec::new();
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
                found.extend(scan.grammar.into_iter().map(|lint| shift(lint, at)));
                if let Some(seen) = self.model.get(text) {
                    model.extend(seen.iter().cloned().map(|lint| shift(lint, at)));
                }
            }
            (spelling, found, model)
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
            self.model.clear();
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

    /// Ask the checker about one span, with or without grammar as the
    /// setting has it.
    fn scan(&self, text: &str) -> Scan {
        scan_text(&self.checker, self.tag, text, self.grammar)
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

impl OsChecker {
    /// Start a model check of `spans` (each a whole span's text), for
    /// `path`. Answers at once only when there is nothing to do; otherwise
    /// [`OsChecker::model_chunk_done`] answers once every chunk is in.
    pub(super) fn start_model_check(
        &mut self,
        path: &str,
        spans: Vec<String>,
        dictionary: &[String],
        dialect: Option<&str>,
        report: &ModelReport,
    ) -> Option<ModelCheck> {
        if model_state() != ModelState::Available {
            return Some(ModelCheck::Unavailable);
        }
        let chunks = super::chunks(spans, MODEL_CHUNK);
        if chunks.is_empty() {
            return Some(ModelCheck::NoProse);
        }
        self.jobs.push(Job {
            id: JOBS.fetch_add(1, Ordering::Relaxed),
            path: path.to_owned(),
            chunks,
            dictionary: dictionary.to_vec(),
            dialect: dialect.map(str::to_owned),
            sent: 0,
            done: 0,
            found: 0,
            timed_out: false,
        });
        self.pump(report);
        None
    }

    /// One chunk answered (or given up on). Its spans are checked again —
    /// synchronously, from the system's cache, which now holds the model's
    /// findings — and kept; the next chunk goes out. Answers `(path,
    /// outcome)` when that was its job's last chunk, and nothing for a job
    /// this project does not have (one from a project since closed).
    pub(super) fn model_chunk_done(
        &mut self,
        done: ModelChunk,
        report: &ModelReport,
    ) -> Option<(String, ModelCheck)> {
        let index = self.jobs.iter().position(|job| job.id == done.job)?;
        self.in_flight = self.in_flight.saturating_sub(1);
        if done.answered {
            let (spans, dictionary, dialect) = {
                let job = &self.jobs[index];
                (
                    job.chunks.get(done.chunk).cloned().unwrap_or_default(),
                    job.dictionary.clone(),
                    job.dialect.clone(),
                )
            };
            let found = {
                let _checker = CHECKER.lock().unwrap_or_else(PoisonError::into_inner);
                autoreleasepool(|_| {
                    self.configure(&dictionary, dialect.as_deref(), self.grammar);
                    let mut found = 0;
                    for text in spans {
                        let grammar = scan_text(&self.checker, self.tag, &text, true).grammar;
                        found += grammar.len();
                        self.model.insert(text, grammar);
                    }
                    found
                })
            };
            self.jobs[index].found += found;
        } else {
            self.jobs[index].timed_out = true;
        }
        self.jobs[index].done += 1;
        let finished = (self.jobs[index].done == self.jobs[index].chunks.len()).then(|| {
            let job = self.jobs.remove(index);
            (
                job.path,
                ModelCheck::Checked {
                    found: job.found,
                    timed_out: job.timed_out,
                },
            )
        });
        self.pump(report);
        finished
    }

    /// Send chunks, oldest job first, while fewer than
    /// [`MODEL_IN_FLIGHT`] are out. Each one's answer comes back through
    /// `report`, from a thread that waits for it — at most
    /// [`MODEL_WAIT`], so a request the system never answers still ends.
    fn pump(&mut self, report: &ModelReport) {
        let _checker = CHECKER.lock().unwrap_or_else(PoisonError::into_inner);
        autoreleasepool(|_| {
            while self.in_flight < MODEL_IN_FLIGHT {
                let Some(index) = self.jobs.iter().position(|job| job.sent < job.chunks.len())
                else {
                    break;
                };
                let (id, chunk, text, dictionary, dialect) = {
                    let job = &mut self.jobs[index];
                    let chunk = job.sent;
                    job.sent += 1;
                    (
                        job.id,
                        chunk,
                        job.chunks[chunk].join("\n\n"),
                        job.dictionary.clone(),
                        job.dialect.clone(),
                    )
                };
                self.configure(&dictionary, dialect.as_deref(), self.grammar);
                let (tx, rx) = mpsc::channel();
                ask_model(&self.checker, self.tag, &text, tx);
                self.in_flight += 1;
                let waiter = report.clone();
                let waited = std::thread::Builder::new()
                    .name("brink-grammar-model".to_owned())
                    .spawn(move || {
                        let answered = rx.recv_timeout(MODEL_WAIT).is_ok();
                        waiter(ModelChunk {
                            job: id,
                            chunk,
                            answered,
                        });
                    });
                if waited.is_err() {
                    // No thread to wait with: report the chunk unanswered
                    // now rather than leave its job open forever.
                    report(ModelChunk {
                        job: id,
                        chunk,
                        answered: false,
                    });
                }
            }
        });
    }
}

impl Drop for OsChecker {
    fn drop(&mut self) {
        self.checker.closeSpellDocumentWithTag(self.tag);
    }
}

/// A lint relative to a span, moved to where the span starts.
fn shift(lint: Lint16, at: u32) -> Lint16 {
    Lint16 {
        start: at + lint.start,
        end: at + lint.end,
        ..lint
    }
}

/// Ask `checker` about one span's text.
fn scan_text(checker: &NSSpellChecker, tag: isize, text: &str, grammar: bool) -> Scan {
    let string = NSString::from_str(text);
    let whole = NSRange::new(0, string.length());
    // Grammar is never asked for alone: on its own the checker answers
    // nothing at all (spec §2.1).
    let types = if grammar {
        NSTextCheckingType::Spelling.0 | NSTextCheckingType::Grammar.0
    } else {
        NSTextCheckingType::Spelling.0
    };
    // SAFETY: no options dictionary is passed, so there is no generic
    // to get wrong, and the word count is not wanted — the binding
    // accepts a null pointer there.
    let results = unsafe {
        checker.checkString_range_types_options_inSpellDocumentWithTag_orthography_wordCount(
            &string,
            whole,
            types,
            None,
            tag,
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

/// Ask the grammar model about `text`, and wait for all of it. `done`
/// hears once the answer is in; the handler runs "in an arbitrary
/// context", per AppKit's header, so it makes no ObjC calls — the answer is
/// read back on the asking thread, from the system's cache (spec §7.3).
fn ask_model(checker: &NSSpellChecker, tag: isize, text: &str, done: mpsc::Sender<()>) {
    let string = NSString::from_str(text);
    let key = NSString::from_str(WAIT_FOR_MODEL);
    let yes = NSNumber::new_bool(true);
    let options: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[&*key], &[yes.as_ref() as &AnyObject]);
    let handler = RcBlock::new(
        move |_: isize,
              _: NonNull<NSArray<NSTextCheckingResult>>,
              _: NonNull<NSOrthography>,
              _: isize| {
            let _ = done.send(());
        },
    );
    // SAFETY: the options dictionary holds one NSNumber, the type its key
    // documents; the handler takes exactly the four arguments the binding
    // declares and touches none of them.
    unsafe {
        checker
            .requestCheckingOfString_range_types_options_inSpellDocumentWithTag_completionHandler(
                &string,
                NSRange::new(0, string.length()),
                NSTextCheckingType::Spelling.0 | NSTextCheckingType::Grammar.0,
                Some(&options),
                tag,
                Some(&handler),
            );
    }
}

/// Whether the grammar model answers on this Mac, as far as is known.
pub(super) fn model_state() -> ModelState {
    match MODEL.load(Ordering::Acquire) {
        1 => ModelState::Available,
        2 => ModelState::Unavailable,
        _ => ModelState::Unknown,
    }
}

/// Find out, once per launch and off every thread that matters, whether
/// the grammar model answers here (spec §8).
pub(super) fn probe_model() {
    PROBE.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("brink-grammar-probe".to_owned())
            .spawn(|| {
                let answers = canary();
                MODEL.store(if answers { 1 } else { 2 }, Ordering::Release);
            });
        if spawned.is_err() {
            MODEL.store(2, Ordering::Release);
        }
    });
}

/// Ask the model about [`CANARY`]: it answers here if it offers
/// [`CANARY_FIX`]. A Mac without it (no Apple Intelligence, or a macOS
/// before 27) gets the quick rules' answer, which passes the sentence.
fn canary() -> bool {
    autoreleasepool(|_| {
        let checker = NSSpellChecker::sharedSpellChecker();
        let tag = NSSpellChecker::uniqueSpellDocumentTag();
        let english = |checker: &NSSpellChecker| {
            checker.setAutomaticallyIdentifiesLanguages(false);
            let _ = checker.setLanguage(&NSString::from_str("en_US"));
        };
        let (tx, rx) = mpsc::channel();
        {
            let _checker = CHECKER.lock().unwrap_or_else(PoisonError::into_inner);
            english(&checker);
            ask_model(&checker, tag, CANARY, tx);
        }
        let answers = rx.recv_timeout(CANARY_WAIT).is_ok() && {
            let _checker = CHECKER.lock().unwrap_or_else(PoisonError::into_inner);
            english(&checker);
            scan_text(&checker, tag, CANARY, true)
                .grammar
                .iter()
                .any(|lint| {
                    lint.fixes
                        .contains(&ProseFix::Replace(CANARY_FIX.to_owned()))
                })
        };
        checker.closeSpellDocumentWithTag(tag);
        answers
    })
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
        let (spelling, grammar, _) = checker.check(&[(text, 7)], &[], None, true);
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
        let (_, off, _) = checker.check(&[(text, 0)], &[], None, false);
        assert!(off.is_empty(), "spelling only: {off:?}");
        // Same text, now with grammar: the span cache must not answer for
        // the spelling-only check.
        let (_, on, _) = checker.check(&[(text, 0)], &[], None, true);
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
