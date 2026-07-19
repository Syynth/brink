# NS-D findings — questions for the maintainer (issue #1132)

Status: **open questions, phone-rulable.** Each item: the question,
the options, a recommendation with a one-line why. Companion to
ns-d-book-outline.md; rulings here shape the outline before
ratification.

---

**F1. How much ink does the book teach?**
The native surface is what brink "documents, teaches, and prefers"
(charter §1), but ink is permanently supported and today's users are
ink users.
- (a) Full ink part — parallel chapters for the compat frontend.
- (b) Native-only main text + one "The ink frontend" chapter
  (conformance, gradual posture, the Unknown seam) + a "brink for
  ink authors" concept-map appendix.
- (c) Native-only; ink material moves out of the book entirely to
  docs/.
**Recommendation: (b).** One chapter + one appendix serves both the
migrating ink author and the compat-frontend user without splitting
the book's identity; (a) doubles maintenance on a surface that's
deliberately frozen, (c) orphans real users.

**F2. Where does the Bevy integration guide live?**
- (a) Book chapters teach (status quo, kept per migration map);
  docs/bevy-brink.md remains the spec.
- (b) Consolidate everything into docs/bevy-brink.md; book gets a
  pointer.
**Recommendation: (a).** It's the book-teaches/spec-rules split
already working; the existing bevy chapters are among the book's
best example-led material.

**F3. Does the stdlib get reference-manual chapters or
inline-with-concepts treatment?**
- (a) A reference-manual part: one chapter per std module
  (std::seq, std::map, …), exhaustive verb tables.
- (b) Inline-with-concepts (Option taught inside collections use,
  verbs taught where their concept lives) + per-module signature
  tables as each chapter's "details" section + a lookup appendix
  that indexes verbs → chapters.
- (c) Inline only, no tables.
**Recommendation: (b).** §10c says teach by concept; but the rows
and signatures are exactly what the IDE shows (closers §9.4 chose
display notation so "docs and IDE agree"), so the tables must exist
somewhere findable. (a) would recreate the glyph-list failure mode
at module granularity.

**F4. How does the book handle the transitional brink dialect?**
- (a) Teach the brink dialect as "the language" now; re-teach when
  `.brink` lands (two full passes over every chapter).
- (b) Write everything `.brink`-first now, with placeholder
  spellings throughout; no compiling examples until B-track.
- (c) Split by class: class-A chapters teach concepts with
  *compiling* brink-dialect examples plus a standard "Current
  spelling" callout naming the ruled native form; class-B chapters
  draft `.brink`-first with `brink,proposed` fences excluded from
  CI.
**Recommendation: (c).** It keeps the CI invariant (every unmarked
example compiles) *and* the phasing promise in #1132 ("starts now …
re-spells as B-track lands"); (a) wastes a full rewrite, (b)
abandons the compile-checked discipline that already saved the book
once (the Story::new rot).

**F5. Wave-0 truth-sync: patch the live book now, or let the
rewrite absorb the staleness?**
Today's book contradicts RULED state in at least three places:
types.md says gradual is the default (NS-A9 flipped it), effects.md
teaches the deprecated `#@effects` colon spelling (E110), and
falsy-`none` behavior (superseded by F27) may be documented.
- (a) Surgical patches now, ahead of any rewrite.
- (b) Absorb into the chapter rewrites, live with the window.
**Recommendation: (a).** "The book never contradicts a RULED spec
section" is only a rule if it's enforced when inconvenient; the
patches are small and independent of outline ratification.

**F6. Book-example CI mechanism: generalize now?**
Three bespoke per-chapter tests exist
(book_{effects,function_values,path_projections}_examples.rs);
the rewrite adds ~10 more example-bearing chapters.
- (a) Keep adding one test file per chapter.
- (b) One generic walker over docs/book/src keyed on fence info
  strings (`ink`, `ink,error`, `text` output-match,
  `brink,proposed` skip-and-log), chapters opt in by containing
  fences.
**Recommendation: (b), as NS-D's first infra task.** Coverage by
convention scales to a chapter-per-issue wave; per-chapter files
make CI coverage an opt-in someone forgets.

**F7. Does the introduction's identity reframe happen now or with
B0?**
The intro currently opens "brink is a toolchain for inkle's ink."
The reframe (brink is a language; ink is its compat frontend) is
public-messaging, not just docs.
- (a) Reframe now, with brink-dialect examples and a "where the
  native surface is" honesty note.
- (b) Reframe when the first `.brink` chapter can compile; until
  then the current intro gains a short "where brink is going"
  section pointing at the charter.
**Recommendation: (b).** An intro that leads with a syntax nothing
can run undermines the book's own compiled-examples credibility;
the pointer section captures intent without overclaiming.

**F8. Does a glyph table exist at all?**
§10c: teach by concept, not glyph list — but §10 caveat (c) admits
the mark vocabulary grows, and lookup is a real need (the
bare-diff/grep experience is a named concern).
- (a) No glyph table anywhere.
- (b) One "Syntax at a glance" appendix, reference-only, every row
  linking to the owning concept chapter; never a teaching device.
**Recommendation: (b).** §10c is about teaching *order*, not about
banning lookup; placement as an appendix satisfies both.

**F9. The Iteration chapter's callback spelling (concrete class-A
tension).**
The trio (`map`/`filter`/`fold`) is class-A semantics, but its
natural spelling is lambdas — ruled for native, not implemented in
the brink dialect, whose shipped mechanism is partial-application
function values (whose book chapter says "never 'closure' or
'lambda'", now superseded on native).
- (a) Demote Iteration to class B; wait for lambdas.
- (b) Ship it class-A now: compiling examples use named functions /
  function values, a Current-spelling box shows the ruled
  `|x| …` form; swap examples when lambdas land.
**Recommendation: (b).** The chapter's real content is the purity
requirement, the dissolved eager/lazy question, and the naming law
— none of which depend on the callback's spelling; waiting couples
Track-A teaching to a B-track wave for cosmetic reasons.

**F10. Do the Concepts/architecture/pipeline and Contributing
chapters stay in the book?**
They're contributor/implementer-facing in a book that's now
author-first.
- (a) Keep in the book (back matter), as outlined.
- (b) Move to docs/, book links out.
**Recommendation: (a).** They're accurate, cheap keeps, and the
book is also the embedder's manual; relocation is churn with no
reader benefit. Low stakes — happy either way.

---

Also surfaced while auditing, not questions (recorded for the
wave-0 patch list): dialect/stdlib.md's "no method-call syntax"
paragraph contradicts the ruled UFCS posture and the chapter is
slated retire; cli/compile.md's `--types` default text is stale
post NS-A9; the charter's own numbering gap (no §12; §13.1's
"§12.8" dangles) is already flagged in stdlib-spec's preamble —
the book should cite charter sections by name+number to survive
the fix.
