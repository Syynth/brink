# The native studio's prose checker on macOS

**Status:** designed 2026-10-08; not yet built. Ruled the same day (decision
log, "Native studio prose checking on macOS: OS spelling, Harper grammar
while typing, Apple Intelligence grammar on request"). §12 lists what is
still open. Everything here is about `crates/brink-gpui`. The web studio
keeps Harper and is not touched.

## 1. Summary

What gets checked is unchanged: content spans minus the machinery nested in
them (`model/src/prose.rs`). What changes is which checker does which job:

| Job | macOS | Windows, Linux |
|---|---|---|
| Spelling, while typing | **the OS checker** (`NSSpellChecker`) | Harper (as today) |
| Grammar, while typing | a setting: **Harper** (default), **macOS quick grammar**, or **off** | a setting: **Harper** (default) or **off** |
| Grammar, on request | **"Check Grammar with Apple Intelligence"** on a selection, from the editor's context menu. Offered only where Apple Intelligence is available. | not offered |

Apple's grammar model is the strongest checker measured (§2.2), but each new
sentence costs it 2–5 s of on-device compute. So it never runs on its own:
the author asks for it on the text they care about. Everything that runs
while typing costs milliseconds.

## 2. What was measured

Everything below comes from a probe run on 2026-10-08: an Apple M2 on macOS
27.0.1, presumably with Apple Intelligence enabled (not checked), using
`objc2-app-kit` 0.3.2 (already in the gpui lockfile). The machine was busy
with other builds (load average 10–25), so absolute times are rough. Every
call was made from a background thread, as the worker would make it.

### 2.1 API behaviour

| Fact | Consequence |
|---|---|
| `checkString:range:types:…` works from a non-main thread. objc2 does not mark `NSSpellChecker` main-thread-only, but Apple does not document it as thread-safe either. | The worker can own the checker. The risk is noted in §9. |
| Passing the **whole file** with a sub-range costs about **80 ms per call**. Copying the span into **its own string** costs about **1.3 ms**. | Each span becomes its own `NSString`. Never pass the file with a range. |
| Asking for `Grammar` alone returns **nothing**. `Spelling \| Grammar` returns both. | Grammar requests always carry the spelling bit, and the spelling half is dropped when only grammar is wanted. |
| Apple's grammar model runs **only** when the request carries `NSTextCheckingWaitForAllGrammarCheckingResultsKey` (new in macOS 27; its value is `"WaitForAllGrammarCheckingResults"`). Synchronous calls and plain async calls never compute it, and never start it in the background (polled for 30 s). Without it you get the **quick** rule-based grammar. | The on-request command must ask for it explicitly. Nothing else ever triggers it. |
| Model results are **cached system-wide, across processes, per sentence**. Once a sentence has been computed, a synchronous call returns its model findings in about 2 ms, even when the sentence sits inside a new paragraph. | The on-request command can read its results back on the worker thread with a cheap synchronous call (§7.3). "macOS quick grammar" while typing also shows them once computed (§6). |
| Cold model grammar costs 2.4–5.4 s per request. One 10-sentence paragraph took 6.6 s. Eight concurrent requests took 4.8 s in total. | Big selections are chunked and run a few at a time (§7.2). |
| The model runs in `TGOnDeviceInferenceProviderService`, an Apple Intelligence extension. None of the spell or inference processes had network sockets open during cold checks. | It is on-device. It presumably needs Apple Intelligence to be available and turned on (§8). |
| `setIgnoredWords:inSpellDocumentWithTag:` takes a word list for one document tag, and writes nothing to the user's own dictionary. It matches **case-insensitively** ("KAELEN" and "kaelen" both pass for `Kaelen`) and handles possessives (`Kaelen's`), but not plurals (`Kaelens` is flagged). | It carries the project words. Case-insensitive matching is ruled acceptable (§5.3). |
| The per-call `NSTextCheckingOrthographyKey` option does **not** change the spelling dialect. `setAutomaticallyIdentifiesLanguages(false)` plus `setLanguage("en_GB")` does: "colour" passes. | Dialect is global state on the shared checker (§5.3). |
| `guessesForWordRange` costs 5–26 ms per word. `correctionForWordRange` returns one suggestion in about 1 ms. Requesting `NSTextCheckingTypeCorrection` in the check added nothing. | Suggestions need a cache and a budget (§5.2). |
| Grammar results carry `NSGrammarRange`, `NSGrammarUserDescription`, `NSGrammarCorrections`, and on macOS 27 also `NSGrammarSystemCategory` ("Verb Form", "Word Usage", "Apostrophe", …). Quick results also carry `NSGrammarIssueType` and a confidence score; model results came back without them. | Each one maps directly onto `ProseLint` (§7.4). |

### 2.2 How the grammar compares

The corpus has 77 cases: 53 that an author would want flagged, 21 pieces of
fiction prose that should pass untouched, and 3 fragments of the kind an
interpolation leaves behind. A sentence counts as "caught" if the checker
flagged anything in it. I eyeballed the fixes, but not rigorously.

| Category (n) | macOS quick | Apple Intelligence | Harper |
|---|---|---|---|
| Subject–verb agreement (7) | 0 | **7** | 2 |
| Confusables: their/they're, its/it's, then/than, should of… (13) | 2 | **13** | 9 |
| Articles (3) | 2 | 3 | 3 |
| Verb form / tense (5) | 0 | **5** | 2 |
| Pronoun case (2) | 0 | **2** | 0 |
| Comparatives, less/fewer (3) | 0 | **3** | 2 |
| Double negative (1) | 0 | 1 | 0 |
| Missing word (2) | 0 | 2 | 1 |
| Doubled word (2) | 2 | 2 | 2 |
| Apostrophes (2) | 0 | 2 | 1 |
| Run-on / comma splice (2) | 0 | 1 | 0 |
| Usage: alot, intensive purposes… (4) | 3 | 3 | 3 |
| Punctuation and spacing (3) | 0 | **0** | **3** |
| Capitalisation (2) | 0 | **0** | 1 |
| Adverb ("runs real quick") (1) | 0 | 0 | 0 |
| Spelling (1) | 1 | 1 | 1 |
| **Total caught (53)** | **10** | **45** | **30** |
| **False positives on fiction prose (21)** | **0** | **0** | **4** |
| False positives on fragments (3) | 0 | 0 | 0 |

What the numbers mean:

- **Apple's model is much the stronger grammar checker**, and it is quiet on
  fiction. It left alone sentence fragments ("Silence. Then footsteps."),
  dialect ("Ain't nobody goin' nowhere"), archaic forms ("Thou art"),
  em-dash interruptions, and second-person interactive fiction. Harper
  flagged `goin'`, `...` (asking for `…`), `OK` (asking for `okay`), and
  `Who's` in dialogue.
- **Harper keeps the edge on mechanics.** Neither macOS tier said anything
  about `Hello ,world`, `Wait..`, a double space, or a lowercase `i`.
- **Without the model, macOS grammar is weak**: 10 of 53 caught. That is why
  Harper is the default grammar while typing.
- Both produce some odd fixes. macOS suggested `dog's` for "The dogs barks",
  and `I go` for "Kaelen and me goes". Harper read "could loose" as a missing
  preposition, and offered to lowercase `Who's`. All of these are offered as
  suggestions, never applied automatically, so this is acceptable.

## 3. What does not change

- Prose ranges: `prose_ranges`, `read_ranges`, interpolation subtraction.
- The `[prose]` schema: `enable`, `dialect`, `dictionary`. `[prose] enable =
  false` still switches everything off for the project, including the menu
  command. The grammar choice is an app setting, not project config (§6.1).
- The project dictionary: symbols plus `[prose] dictionary` in `brink.toml`.
  "Add to dictionary" keeps writing `brink.toml`.
- How lints are shown: HINT squiggles on top of the compiler's diagnostics,
  `prose.<kind>` codes, the Problems prose bucket off by default, fixes in the
  squiggle's `data` for the hover card.
- `QueryKind::Prose { path }` → `QueryResult::Prose(Vec<ProseLint>)`.
- The web studio, `brink-prose`, and its wasm artifact.

## 4. Shape

### 4.1 Where the code lives

`model/src/prose.rs` becomes a module directory:

```
model/src/prose/
  mod.rs        span extraction, Units, project_dictionary, ProseLint,
                check(): run the engines, merge their findings (§4.3)
  harper.rs     today's body of check(): CheckRequest → brink_prose::check,
                with its results split into spelling and grammar
  macos.rs      NSSpellChecker: spelling, quick grammar, and the on-request
                model check (cfg(target_os = "macos"))
```

`mod.rs` keeps the part that decides *what* to check. The engines decide
*how*. Each takes the file's text plus UTF-16 span ranges, the word list and
the dialect, and returns UTF-16 lints. So `Units` and the byte↔UTF-16
conversion stay where they are. `NSString` ranges are UTF-16 too, so the
macOS engine needs no extra conversion.

`brink-prose` stays linked on every platform, because it is the default
grammar on macOS too. The macOS engine is an addition there:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-foundation = { version = "0.3", features = [...] }
objc2-app-kit = { version = "0.3", default-features = false, features = ["std", "NSSpellChecker", "block2"] }
block2 = "0.6"
```

### 4.2 Who owns what

The **worker loop** owns a `ProseEngines` value, created on first use and
dropped on `Request::Open`. It holds:

- the macOS engine (macOS only): the spell-document tag
  (`uniqueSpellDocumentTag`, closed with `closeSpellDocumentWithTag` on drop),
  the word list and language last applied to it, the span cache (§5.1), the
  suggestion cache (§5.2), the model-results store and the in-flight model
  jobs (§7);
- the grammar-while-typing choice, sent by the app (§6.1);
- a clone of the worker's own `Request` sender, which the model command's
  completion handlers report back through (§7.3).

Harper keeps its own thread-local cache inside `brink_prose`, as today.

### 4.3 Merging findings

For each span, `check()` puts together:

1. **Spelling:** macOS on macOS, Harper's `Spelling` kind elsewhere.
2. **Model grammar:** what the on-request command found for this span's
   exact text, if anything (§7.3). macOS only.
3. **Grammar while typing:** Harper's non-`Spelling` kinds, macOS's quick
   grammar, or nothing, depending on the setting.

Overlaps are settled in one direction each:

- A while-typing grammar finding that overlaps a **model** finding is
  dropped. When the author has asked for the strong check, its answer wins.
- A Harper finding that overlaps a **macOS spelling** finding is dropped, but
  its fixes go **first** in the spelling finding's list (capped at
  `MAX_FIXES`). This covers Harper's spelling-shaped kinds (`Typo`,
  `BoundaryError`) without showing two squiggles on one word, and keeps its
  context-aware fix (`alot` → `a lot`) ahead of the dictionary's guesses.

## 5. Spelling on macOS

### 5.1 The check

For each prose span:

1. Look the span's text up in the **span cache**, keyed by
   `(span text, language, word-list generation, grammar mode)`. On a hit,
   reuse its findings, moved to the span's current offset. Typing changes one
   span, so a keystroke costs one ObjC call, not one per span.
2. On a miss, copy the span into its own `NSString` and call `checkString`
   over the whole of it. The types are `Spelling`, or `Spelling | Grammar`
   when the grammar setting is macOS (§6). Decode the results and store them.

The whole check runs inside one `objc2::rc::autoreleasepool`. The worker
thread has no pool of its own, so without one every check would leak its
autoreleased objects.

The span cache is a bounded LRU (CLAUDE.md, "Guard against unbounded growth").
4096 entries is a reasonable start. It is cleared when the word list, the
language or the grammar setting changes.

### 5.2 Suggestions

A spelling finding becomes `kind: "Spelling"`, with fixes from the
**suggestion cache**: `(word, language) → Vec<String>`, bounded LRU. Each
check fills misses until it has spent **100 ms** on `guessesForWordRange`.
Any finding still unfilled gets `correctionForWordRange` (about 1 ms) as its
single fix, and a later check fills in the rest. Misspellings repeat, both
across re-checks of the same file and in names typed the same wrong way, so
the steady-state cost is one `guesses` call per newly typed misspelling.

### 5.3 Dictionary and dialect

- **Words:** `setIgnoredWords:inSpellDocumentWithTag:` with the result of
  `project_dictionary`, set again only when the list changes. It is per tag,
  so nothing reaches the user's `~/Library/Spelling`. Words the author has
  taught macOS elsewhere ("Learn Spelling" in Mail, Pages, …) are also
  accepted. That is new behaviour, and probably welcome.
- **Case:** the list matches case-insensitively. That is ruled acceptable for
  this checker, so no second filter is applied on top. Harper, which still
  receives the same words for its grammar rules, keeps its literal matching;
  that has no visible effect, because Harper's spelling findings are not
  shown on macOS.
- **Dialect:** `setAutomaticallyIdentifiesLanguages(false)` and
  `setLanguage(...)` on the shared checker, mapped American → `en_US`,
  British → `en_GB`, Canadian → `en_CA`, Australian → `en_AU`. This is
  **process-wide** state: every open project has its own worker thread, and
  all of them share `sharedSpellChecker`. So a check holds a process-wide
  lock from setting the language to its last call, and sets the language
  again every time, since it is a cheap setter. (Found in step 2: parallel
  tests in two dialects set it under each other's checks. Two project
  windows would do the same.) gpui draws its own text and has no
  `NSTextView` to share it with. The probe showed `en_CA` accepting "colour"
  and flagging "realise", which is correct Canadian spelling. Harper gets the
  same dialect, as today.

## 6. Grammar while typing

### 6.1 The setting

`AppSettings.prose_grammar` (`shell/src/settings.rs`, the platform config
file), one of:

| Value | Grammar while typing | Platforms |
|---|---|---|
| `"harper"` (default) | Harper's non-`Spelling` kinds | all |
| `"macos"` | macOS quick grammar, plus model results already in the system cache | macOS |
| `"off"` | none | all |

It is an **app** setting rather than a `[prose]` key. It picks between
checkers on *this machine*, and a collaborator on Linux has no macOS checker
to pick. `[prose]` stays a description of the manuscript.

It is shown in a new App-scope Settings section, **Spelling & Grammar**
(registered beside Appearance and Keymap). That section also says which
spelling checker is in use, and whether "Check Grammar with Apple
Intelligence" is available here, with the reason when it is not (§8). The
Project-scope Prose section keeps `enable`, dialect and dictionary, with a
line pointing to the App section.

The app sends the value to the worker with a new
`Request::SetProseOptions { grammar }` on startup and whenever it changes.
Open editors then re-check their prose.

### 6.2 Each value

- **Harper:** `brink_prose::check` exactly as today, keeping only lints whose
  `kind != "Spelling"`. Harper still runs its spelling rule internally, which
  wastes a little time. A later `brink-prose` option could switch it off;
  that changes the shared wasm crate's request shape, so it is not part of
  this work.
- **macOS:** the span-cache check already requests `Spelling | Grammar`
  (§5.1), so this mode costs nothing extra. Sentences the author has run
  through the model command keep their model findings across launches, since
  the system cache answers for them.
- **Off:** spelling only, plus anything the model command finds.

## 7. "Check Grammar with Apple Intelligence"

### 7.1 Where it appears

An item in the editor's text menu (`app/src/editor_menu.rs`, `text_menu`), at
the end of group 3, enabled only when `click.has_selection`. The one menu
builder serves Script tabs and the manuscript alike, so Write gets it too.
It is native-only, like New Stitch… and Write's Go to. The menus ruling
allows those.

It is **not shown** where the model is unavailable (§8), or when
`[prose] enable` is off. A greyed-out item the author can never enable would
only advertise a feature they cannot have.

### 7.2 What it checks

The selection's byte range, intersected with the file's prose ranges. So
selecting across a knot header or an interpolation checks only the prose
pieces. If there are no prose pieces, the status bar says "No prose in the
selection" and nothing runs.

The pieces are grouped into **chunks of about 2 KB**, joined by a blank line
so each keeps its own sentences. One `requestCheckingOfString` goes out per
chunk, with types `Spelling | Grammar` and options
`{ WaitForAllGrammarCheckingResults: YES }`. At most **4 chunks are in
flight**; the rest queue. A select-all over a long file therefore takes a
while, and that is the author's choice. The status bar shows "Checking
grammar… (n of m)" until the job finishes. A second command while one is
running joins the queue.

### 7.3 How results come back

The model can take seconds, so this is not a `Query`, which the worker
answers inside its loop. It is a fire-and-report request:

1. The app sends `Request::CheckGrammar { path, range }` and shows the status.
2. The worker computes the chunks, sends the first requests, and returns to
   its loop.
3. Each completion handler runs "in an arbitrary context", per the header.
   It **makes no ObjC calls**. It sends `Request::GrammarChunkDone { job,
   chunk }` on the worker's own request sender, and returns.
4. On `GrammarChunkDone`, the worker re-checks each span text in that chunk
   with a **synchronous** call. The system cache now holds the model's
   results, so this takes about 2 ms per span, and decoding happens on the
   worker thread like every other ObjC call. It keeps the grammar findings,
   relative to each span, in the **model-results store**: span text →
   findings, bounded LRU, for the session's lifetime.
5. When the job's last chunk is in, the worker sends
   `Response::GrammarChecked { path, found, timed_out }`.
6. The app clears the status ("Grammar: 3 suggestions", or "No grammar
   issues found") and re-checks that path's prose. A Script tab calls
   `refresh_prose`. The manuscript **drops that path's `ProseCache` entry**
   first: its cache is keyed by the text, which has not changed, so it would
   otherwise keep serving the lints from before the command.

Each later check merges the store's findings for any span whose text is
unchanged (§4.3). Editing a span changes its text, so its model findings
disappear until the command is run again. The store is keyed by the exact
span text, so a finding from before an edit can never land on the edited
text.

A chunk that has not finished after **60 s** is counted as failed, and
`timed_out` tells the author some of the selection went unchecked. In the
probe, 2 of 77 cold requests passed 20 s while the machine was loaded.

### 7.4 Decoding a model result

| `NSTextCheckingResult` | `ProseLint` |
|---|---|
| Grammar, one per entry in `grammarDetails` | range = `result.range.location + NSGrammarRange`; `kind` = `NSGrammarSystemCategory` with spaces removed (`VerbForm`, `WordUsage`, …), falling back to `"Grammar"`; message = `NSGrammarUserDescription`; fixes = `NSGrammarCorrections` as `ProseFix::Replace`, at most `MAX_FIXES` |
| Spelling | dropped here. The while-typing check already reports it. |
| Any other type (orthography, type 1, comes back on some inputs) | dropped |

A grammar correction replaces the whole `NSGrammarRange`. Every correction in
the probe was a whole-range replacement (`"late,"` → `"late, but"`, `"is
more"` → `"is"`), so `ProseFix::Replace` covers them.

The same decoder serves the quick grammar of §6.2. Quick results carry the
older keys as well (`NSGrammarIssueType`, a confidence score). The category
is there on macOS 27, and the fallbacks above cover it missing.

## 8. Availability

- **macOS older than 27:** the `WaitForAllGrammarCheckingResults` key does not
  exist there, and `NSGrammarSystemCategory` may not either. Look the key up
  with `dlsym` rather than linking it, or the binary will not load on older
  systems. When it is missing, the command is not offered. Spelling and the
  while-typing grammar work as usual.
- **Apple Intelligence unavailable or off** (Intel Macs, unsupported regions,
  the user's choice): the probe could not test this. The likely result is
  that model requests return quickly with quick results only. So once per
  launch, in the background, the macOS engine runs a **canary**: "There is
  three apples on the table." with the model option and a 30 s timeout. The
  command is offered only once the canary comes back with a `Verb Form`
  finding. On a Mac that has run it before, the system cache answers in
  milliseconds. On a fresh Mac it takes a few seconds, during which the
  command is not yet offered.
- **What the author sees:** Settings ▸ Spelling & Grammar says "Apple
  Intelligence grammar: available", or "not available on this Mac (needs
  macOS 27 and Apple Intelligence)".

## 9. Threads

- All ObjC calls happen on the worker thread, inside an autorelease pool.
- Completion handlers make no ObjC calls; they only send on a channel.
- Apple does not document `NSSpellChecker`'s thread-safety for synchronous
  calls off the main thread. The probe made over a thousand without
  incident, but that is evidence, not a guarantee. If it breaks, the
  fallback is to issue the synchronous calls from the main thread via gpui's
  foreground executor, which would cost a millisecond per changed span there.

## 10. Testing

- **Pure parts, every platform:** merging and overlap rules (§4.3), span-cache
  invalidation, chunking, the model-results store and its bounds, and result
  decoding, all tested against a fake engine behind a small trait that
  exists for tests only. (CLAUDE.md: instrumentation wraps production types;
  it does not thread through them.)
- **The setting:** with `"harper"`, Harper's `Spelling` lints never appear;
  with `"off"`, no grammar kind appears.
- **macOS, deterministic:** a `#[cfg(target_os = "macos")]` test that
  "recieve" comes back as Spelling at the right UTF-16 range, that an ignored
  word passes in any case, and that `en_GB` accepts "colour". Spelling does
  not depend on Apple Intelligence.
- **macOS, environment-dependent:** model-grammar tests are `#[ignore]` and
  run by hand. Their result depends on the machine, the OS version, and what
  the system cache already holds.
- **Through a real consumer:** the gpui headless harness, with a project
  containing "There is three apples." Select it, run the command, and check
  that a `VerbForm` HINT appears after `GrammarChecked`, and that it goes
  away when the sentence is edited.
- CI does not build `brink-gpui`, so all of the above is a local gate. Record
  that in each slice's PR.

## 11. Risks

| Risk | Mitigation |
|---|---|
| `NSSpellChecker` off the main thread breaks on some OS update. | §9 fallback. The while-typing check is one call per changed span, so moving it is cheap. |
| The private keys and categories change in a later macOS. | Every key is read as optional. A missing category falls back to `"Grammar"`. A missing model key hides the command. |
| The system cache evicts a sentence between the model call and the synchronous read-back (§7.3, step 4). | That span's model findings are missing, and the author sees fewer suggestions than the model found. Rare, since the read-back happens within milliseconds. Not worth a second decode path. |
| The two studios disagree about the same file. | Accepted when this was ruled. The Problems codes differ (`prose.VerbForm` vs `prose.Agreement`), which matters only to someone comparing the two side by side. |

## 12. Open questions

- **"Learn Spelling".** Should the squiggle menu also offer macOS's own
  "Learn Spelling" (this Mac, every app) beside "Add to dictionary" (this
  project, `brink.toml`)? The default is no: one dictionary, kept with the
  manuscript.
- **App setting vs project config** for the grammar choice (§6.1). This is
  the design's choice, not a ruling yet.

## 13. Slices

1. **Seam and setting:** the `prose/` module directory with `harper.rs`
   holding today's code, `AppSettings.prose_grammar` (`harper` / `off`),
   `Request::SetProseOptions`, and the merge in `check()`. No behaviour change
   by default.
2. **macOS spelling:** `macos.rs` with the span cache, suggestions,
   dictionary, dialect, and the overlap rules against Harper.
3. **macOS quick grammar** as the third setting value.
4. **The model command:** availability lookups, canary, menu item,
   `CheckGrammar` / `GrammarChunkDone` / `GrammarChecked`, the
   model-results store, status-bar progress, and the manuscript's
   `ProseCache` drop.
5. **Settings ▸ Spelling & Grammar**, plus the pointer from Project ▸ Prose.
   Also fix the stale comment in `settings_prose.rs` that says the native
   studio has no prose checker.
