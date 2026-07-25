# Prose dialect — DRAFT (sitting 1 checkpoint, 2026-07-25)

Status: **the ground-up prose rethink (#1351), sitting 1** — the rulings
below were made in-sitting with the maintainer; open threads are marked
⏳ and are the resumption points. This document supersedes the charter
§5 *direction* as drafted (charter §5 was first-pass; #1351 reopened it
in full). Grounding research referenced throughout: the studio-editor
rendering survey and the line-table/fragment survey, both recorded on
issue #1351.

## 1. Goals & north stars (RULED)

- **The writer.** The flagship user has a script-writing background
  (Scrivener, CELTX). Scrivener's model — each paragraph *is* a typed
  element, with per-type styling and keyboard behavior — is the UX
  target. Ink flattens all prose into undifferentiated text; the native
  dialect does not.
- **Fountain is the format posture**: plain-text conventions that read
  naturally *raw* (a Fountain file already looks like a screenplay in a
  bare editor) and decorate richly in CM6 without the text lying about
  itself.
- **Yarn Spinner is the markup-data posture**: inline markup delivered
  to the host as character-range attributes over the line.
- **Mechanism over bespoke.** The screenplay conventions are the
  flagship *preset*; the product is the *facility*. Projects adopt a
  preset, declare their own conventions, or ignore the layer entirely
  and write plain narrative. Ride the line between "this is for me" and
  a general facility.
- **Explicit format, editor-supplied ergonomics.** The marks are real
  and in the text; CM6 decorations are what keep them from *feeling*
  like syntax. No parser inference cleverness — classification is
  declared and deterministic.
- **Superset doctrine.** The new output/prose model is a **superset of
  current runtime behavior**: today's ink output (plain text lines +
  tags) is the *degenerate case* — an untyped narrative element with no
  spans — sitting unchanged inside the general model. Ink content keeps
  working; the general model generalizes rather than forks.

## 2. The two-layer model (RULED)

- **Block layer — elements.** A line/paragraph *is something*: cue,
  parenthetical, dialogue, action, scene heading, transition, plain
  narrative. Project-declared conventions; preset-able.
- **Inline layer — markup.** Spans *within* prose carry meaning:
  `<wave>text</wave>`, `<item id="lantern">the old lantern</item>`.
- Both layers are **wire-deep**: visible at compiler and runtime levels,
  not editor-only decoration. Both follow the same mechanism-vs-preset
  split.

This is Scrivener's paragraph-style/inline-formatting split expressed as
plain text.

## 3. The element layer

### 3.1 Classification (RULED)
Static and declared: line-shape patterns (e.g. `@NAME` cues, `INT.`/
`EXT.` scene-heading prefixes) plus **chain rules** (dialogue is the
line after a cue/parenthetical) — the shipped at_cue dialect's exact
mechanism, promoted to first-class. Classification never depends on
runtime state. Element kinds are an open set (string-typed, as the
editor's `ElementType` already is).

### 3.2 Elements are annotative; structure only gets costumes (RULED)
Elements attach data to lines (speaker, delivery, scene metadata). They
do **not** create new structural kinds. The permitted exceptions are
elements serving as *spellings of structure that already exists*:
- a **scene heading may declare a stitch** (see §3.3);
- a **transition element dresses the divert that follows** (`CUT TO:`
  above `-> market` — the divert remains the structure).

### 3.3 Scene-heading slugs (RULED)
- **Explicit-optional, inferred-default**: a heading may spell its
  address explicitly (syntax TBD); otherwise a slug is inferred from the
  title.
- The **heading text is the display name**: the editor renders a divert
  to that stitch with the scene title (decoration only — the buffer
  stays honest), so transitions read like a screenplay.
- **Save-key note**: with an inferred slug the title is load-bearing
  (stitch names feed `DefinitionId`). Retitling is a *rename* — the
  `@[was]` migration machinery and rename tooling are the safety net,
  and the editor can offer the rename-safe path. Spelling the slug
  explicitly makes the title free to change.

### 3.4 Where conventions live (RULED)
- **`brink.toml` holds pointers, not content** — a manifest location,
  an elements location.
- **Element conventions are project-authored** → a dedicated
  project-side file referenced from `brink.toml`, with built-in presets
  nameable (`elements = "screenplay"` vs a path). The shipped dialect
  JSON is the precedent, and it already solved a real constraint: it is
  interpreted identically in Rust and TS (the editor needs the
  conventions too).
- **Markup vocabulary is host-authored** → it lives in the **host
  capability manifest**, same author as the externals section, and can
  be *generated* from engine code (a text-effect plugin auto-declaring
  its tags), like bindings can generate externals.
- The authorship test that produced this split: co-locate declarations
  only if hand-authored or generated by the same source.

### 3.5 Conventions as a brink module (deferred door, RULED)
The cool version — defining conventions in a `.brink` module — has a
real path via the Environment architecture: the compiler never consumes
a conventions *module*, only a resolved conventions *value* out of the
`Environment`; where the value came from is the producer's business.
v1 reads a data file. Later, the producer can compile-and-evaluate a
code-ground-only conventions module (the proc-macro staging rule: a
conventions module cannot use the conventions it defines) and freeze
the result into the same slot. **The conventions schema is therefore
defined as a value** so the module source can slot in later without any
consumer knowing. Deferred; the door stays open by construction.

## 4. The markup layer

### 4.1 Syntax (RULED)
- **XML tags**: `<name attr="v">content</name>`, self-closing allowed.
  v1 is **XML-only**; markdown-style emphasis sugar is deferred until
  the writer misses it (`<b>`/`<i>` come free as tags).
- **Blunt lexing**: `<` followed by a letter always opens a tag;
  `</ident` closes; `<>` (glue) and `<-` (splice) are lexically
  distinct — no letter after `<`.
- **Escapes**: `\<` `\{` `\#` `\\` produce literals; `\` before
  anything else is a **compile error**. The editor hides escapes in
  rendering and can auto-insert them — the format stays blunt, the
  editor supplies the grace.

### 4.2 Schema: freeform by default, manifest-validated (RULED)
Undeclared markup passes through freely (fast host iteration). When the
host manifest declares a markup vocabulary (span kinds + attributes),
the compiler validates tags against it with configurable severity —
exactly the externals-manifest pattern.

### 4.3 Nesting doctrine (RULED)
- **A tag must close in the same fragment scope it opened in.** Markup
  and logic nest freely inside each other; they can never partially
  overlap. (`<b>hello {name}</b>` ✓ · `{tired: <i>yawn</i>|Ready.}` ✓ ·
  `<b>hi {tired: there</b>|friend}` ✗ — compile error.)
- **Spans are line-scoped** — mechanically forced, not stylistic: the
  locale system swaps whole line vectors by index and hard-rejects
  count mismatches, so a span may never split or cross entries.
  Multi-line styling is the element layer's job.
- Spans are **presentational**; interpolation `{…}` remains the only
  dynamic channel (a span may *contain* interpolation).

### 4.4 Wire representation (RULED, grounded by the line-table survey)
- **Spans live in the line table**, as a genuinely nested
  `LinePart::Span { name, attrs, children }` inside
  `LineContent::Template` — *not* in the runtime fragment model.
  (Fragments are a flat `Vec<OutputPart>` filling value slots; right
  spirit — deferred, structural, locale-safe — wrong mechanism. A span
  over part of a line is not a value boundary. Recorded so this is not
  re-derived.)
- **Nesting-by-type**: with a recursive `Span` variant the decoder
  enforces balance structurally — a mangled translation (unbalanced
  inline codes, a classic TMS failure) becomes a *decode error*, not
  silent rendering corruption. The nesting doctrine is
  unrepresentable-to-violate at grammar, HIR, and wire.
- **Span hash-transparency (RULED — before any markup ships):** markup
  is normalized out of `source_hash` the way interpolations already
  are; `Hello <wave>world</wave>` hashes identically to
  `Hello world`. Markup is presentation; identity is the words.
  Deciding this later would be a mass translation-memory invalidation.
- **Format cost, acknowledged**: a new `LinePart` tag is a `.inkb`
  version bump (v6) + `.inkl` bump (decoders hard-reject unknown part
  tags; section 0x07 has no section-local version). **Batch the bump**
  with the intl spec's future recognizers (inline conditionals →
  `Select`, sequences as slots) — one bump, all the line-table growth.
- **The recognizer is the chokepoint**: marked-up lines must be
  *admitted to line recognition* or they shred into per-run entries and
  stop being single translation units. (Pre-existing: inline
  conditionals/sequences already shred today — the prose round elevates
  the priority of those future recognizers.)
- **Public API**: `Line` grows a structured span surface (additive).
  Prefer structural parts over byte-range offsets — the runtime trims
  and collapses whitespace after assembly, so ranges are a trap.

## 5. Worked cases (abbreviated; spellings illustrative)

```
INT. MARKET SQUARE - NIGHT          ← scene heading (stitch decl, §3.3)

The square is empty.                ← action/narrative

@VENDOR                             ← cue (speaker data)
(hushed)                            ← parenthetical (delivery)
You shouldn't be here after dark.   ← dialogue (chain rule)

He hands you <item id="lantern">the old lantern</item>.
It'll cost you <price>{gold_cost} coins</price>.
{haggled: You get a <b>discount</b>.|Full price, then.}

CUT TO:                             ← transition (dresses the divert)
-> market_square                    ← editor renders the display name
```

## 6. Open threads ⏳ (the resumption points)

1. **Runtime representation** — the general typed-line output shape of
   which today's `Line::Text { text, tags }` is the degenerate case
   (superset doctrine, §1). Element kind + data, structured content
   parts, where choices sit in the model.
2. **Translation, round 2** — element data in XLIFF (is a speaker name
   translatable?), the recognizer growth plan, the bump batching.
3. **Runtime output surface** — how span/element data reaches hosts
   (bevy, web) concretely.
4. **Choices × elements** — chain rules meeting the weave; what element
   a choice line carries; markup in choice text.
5. **Dynamic element payloads** — is `@{challenger}` (interpolated cue)
   legal? Classification stays static; is the *payload* allowed to be a
   fragment?
6. **Editor implications (NS-T)** — line-local classification vs the
   nesting-property dialect; #1350/#1131 stay held on this round.
7. Element-conventions file format details; explicit-slug spelling;
   escape-set finalization.
