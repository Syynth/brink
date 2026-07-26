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

## 2b. Product vision — the interactive screenplay (RULED, sitting 3)

A screenplay is a record of one performance; the weave is a machine for
many. The screenplay tradition has no vocabulary for "the reader
decides" — so the product is neither "screenplay with choices bolted
on" nor "ink with prettier prose":

**The document is linear dramatic prose on an interactive skeleton —
and the two never wear each other's clothes.**

1. **Branches read as screenplay; structure reads as structure.**
   Within any linear run, the text is screenplay — full conventions, no
   compromise. The interactive skeleton (choice points, transitions-as-
   diverts, scene boundaries) is its own visual register, made
   beautiful by the editor, never disguised as pseudo-screenplay
   elements. (**No-costume ruled in principle**; one deliberate
   aesthetic pass over choice-point syntax against the conventions is
   reserved for the syntax round — they must *naturally complement*.)
2. **Choices are typed prose.** An option's text is element-typed by
   the same conventions as any line: a dialogue-choice carries
   `speaker: <PC>`; an action-choice is imperative narrative; a cue
   above a choice block types the options as that speaker's dialogue
   options (the Telltale pattern, via chain rules). `Choice.element`
   is the same machinery, not a special case. **The ink `[]` choice
   anatomy is re-ratified unchanged** — it already answers
   spoken-vs-summary (option text vs delivered text) per choice.
3. **Scene-grained is the golden path; the editor dissolves the
   granularity tension.** Linear leaves on a graph spine (the
   industry's own interactive-screenplay answer) is what the
   conventions serve best — headings-as-stitches, transitions-dressing-
   diverts. Conversation-grained weaves stay fully powered. The
   bridge is editorial (NS-T scope, gated on this round):
   **scrivenings-style inline-destination view** (select a choice, see
   its target rendered in place), **extract-to-stitch refactoring**
   (inline body → knot/stitch + auto-divert), **story-graph
   visualization**. Composition is the editor's job; the format stays
   honestly graph-shaped.
4. **Per-path export is in the vision.** Element-typed lines make any
   single playthrough renderable as a genuine linear screenplay —
   Fountain/FDX for table reads, per-character VO recording sheets —
   and the machinery is *adjacent to the intl pipeline* (the xliff
   exporter already walks scopes and renders line tables; a path
   exporter walks a path and renders to industry formats — the same
   extractor family, inheriting line-identity work for free).
   Consequence: the element set must map cleanly onto what
   Fountain/FDX can express.

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

### 3.5 Conventions are authored as a brink module (RULED, sitting 4 addendum)
**The authored form of project conventions is a `.brink` module** — a
code-ground-only module exporting a pure `fn conventions() ->
Conventions` (purity asserted via `@[effects(pure)]`, the determinism
gate; the proc-macro staging rule holds: the module cannot use the
conventions it defines). The producer compiles and evaluates it
(existing machinery: the compiler + `begin_function_eval`) and freezes
the resulting *value* into the `Environment` — the compiler never
consumes the module, only the value.

- **Presets ship as modules** (`std::conventions::screenplay`), so a
  project *imports and extends* a preset with ordinary value code
  (`screenplay::base()` then modify) instead of forking a JSON blob.
- **The data form survives only as generated interchange** — the
  resolved value serializes to the JSON shape the TS editor's dialect
  interpreter already reads (evaluated through wasm in the editor;
  `brink conventions export` for non-wasm embedders). It is never
  authored.
- The host **capability manifest stays data** — it is generated from
  engine code; nobody hand-writes it either way.
- The cast roster and PC identity live in the same module (`cast`,
  `pc`) — the "natural early tenant" arrives with the house.
- Design pass owed (⏳, spec §9.1): the `std::conventions` types
  shaped for extension ergonomics; the `fn conventions()` well-known-
  name entry (mirroring `flow main()`); the `brink.toml` pointer
  (`conventions = "conventions.brink"`); the portable-regex subset
  validated at marshal with module-pointing errors; the editor
  re-evaluation loop; sequencing (native construction literals ride
  the #1103 build; the module may be brink-dialect until then).

### 3.6 Element roles & attachment (RULED direction; mechanism leaned)
Elements declare a **role**:
- **attached-forward** (cue, parenthetical) — not content; data *about* the
  content that follows;
- **content** (dialogue, narrative, action);
- **structural** (scene heading — the §3.2/§3.3 exception).

**Element structure replaces glue ceremony** (RULED): the runtime proceeds
past a cue into its dialogue because the schema says so — the author never
writes glue to associate them. Glue demotes to what it honestly is: literal
text joining within content. Degenerate case intact: no schema → today's
newline/glue behavior.

**Mechanism (leaned): attachment resolves at compile time over the source
chain**, not at runtime over the emission stream. Attached-element lines
emit nothing; each content line's entry carries the attached data baked in
(dynamic payloads as slots). Consequences: control flow cannot tear an
attachment (source-static, not emission-order); a dangling attached element
(cue with nothing attachable) is a **compile diagnostic**, not a runtime
surprise.

### 3.7 Consecutive lines & blocks (🔶 under discussion)
Multiple content lines under one attachment (a multi-line speech) form a
**block**. Direction being worked: emit **per-line** (the finer granularity
— pacing stays a host beat) with a **baked block id** in element data;
block-level delivery is API sugar / host aggregation over the id, robust
under control flow (id-matching, not positional). Screenplay `(CONT'D)` is
derivable (same speaker, new block) — an editor/preset concern, not wire.

### 3.5b The `@[element]` annotation surface (RULED, sitting 4 addendum 2)

The **second authoring surface**, and the one that ships first:

```brink
@[element("^%% (?<chan>[A-Z0-9-]+): (?<text>.+)$")]
fn radio_line(chan: string, text: content) {
    host::radio_ping(chan)
    > [{chan}] {text}
}
```

A prose line matching the pattern **lowers to a call** to the annotated
fn. Properties, all RULED:

- **Zero comptime.** The annotation is static metadata; the rewrite is
  ordinary lowering; the body runs at *runtime* under its declared
  effect row — "arbitrary within the effect bounds required by the
  compiler," enforced by the existing row machinery. The disciplined
  member of the macro family: an expansion that can only ever be *one
  call* has no hygiene problem — the arbitrariness lives inside a
  function, where the language already governs it.
- **Two surfaces, one interface.** The producer sweeps annotations and
  folds them into the *same resolved conventions value* as the
  declarative side — editor, exporter, translator still read one value.
- **Role boundary**: annotations declare content/call-shaped elements
  only; **attachment, chains, and structure stay declarative**
  (preset/module) — those are facts about relationships between lines,
  not behaviors of one.
- **Capture contract**: named captures bind to parameters **by name**
  (arity/name mismatches are compile errors at the annotation).
  `string` param = literal when the capture is static, stringified at
  the call boundary when dynamic. **`content` param** = a first-class
  content value via the existing fragment-capture path
  (`BeginFragment…EndFragment → FragmentRef`); the captured prose
  compiles **through the normal line path**, so it is
  **translation-resident and measurable** like any authored line.
  Deferred: numeric capture coercion; context injection (a handler
  reading attachment data like `speaker`); `Option` params for
  optional captures fall out when wanted.
- **Prose-bodied handlers** compose: `@[element(…)] fn x(…) >{ … }` —
  the template is translated *once*, parameterized by captures.
- **Staging flip (amends §3.5's plan): v1 = built-in screenplay preset
  + annotations** — no comptime machinery at all; the
  conventions-module evaluation arrives later for authoring full
  custom presets. The maintainer's favorite surface ships first.
- **Tooling transparency (RULED: no invisible expansion).** The
  compiler exposes per-line classification metadata through the
  `LineContext`/ide query family: matched element kind, **the matching
  rule/handler** (fn + source location), **capture bindings as spans**,
  and the disposition. Hover on a matched line shows the handler's
  signature and body; an **explain-match query** answers "is this line
  matched, by what, what did the captures bind" — and on a miss, which
  declared patterns were attempted. Capture spans double as decoration
  ranges. Rides the existing wasm/ide query surface (NS-T consumes).
- **Match ordering ⏳ (rule owed)**: lean — declaration order, with
  overlap diagnostics when two patterns can match the same line.

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

## 5. Tooling: completions & succession (RULED doctrine)

**Harvest by default, declaration upgrades** — the freeform/manifest
pattern extended to editor intelligence:
- **Character names**: every `@NAME` cue in the project completes
  everywhere (harvest). An optional **cast roster** upgrades: typo
  validation (à la manifest), display name, editor color, voice ref.
  The roster is also a natural early tenant of the §3.5 module door.
- **Markup tags**: harvested span kinds/attr names complete; the host
  manifest upgrades to full vocabulary, attribute types/enums, hover
  docs.
- **Element kinds**: inherently declared (conventions file).

Consequences (RULED):
- **Harvest is a project-db index obligation** — cue payloads and span
  kinds are indexed project-wide (sibling of the symbol index), so
  completion crosses files.
- **Schemas are tooling-grade**: the manifest and conventions files
  carry editor-consumed fields the compiler ignores (descriptions,
  attr types, display metadata). A declaration format is a
  documentation format.
- **Succession rules live in the conventions file** — the editing-time
  dual of chain rules (`after cue: enter → dialogue, tab →
  parenthetical`), consumed by the editor's transition machinery,
  ignored by the compiler. This is what makes the Tab/Enter behavior
  convention-driven instead of hardcoded ink.

## 6. Display metrics & measurement (#362 becomes a consumer)

- **The host declares metrics in the manifest** (host-authored,
  generatable): per-element budgets (lines × width, or px + font
  metrics) and per-span-kind width behavior. The same declaration that
  validates `<wave>` says what it does to width.
- **Overflow is a lint** — delivered through the `[lints]` control
  plane: severity-configurable, deny-able, so "no line exceeds the
  dialogue box" can be a CI guarantee, not a QA pass. One measurement
  query serves both the live editor squiggle and the compile-time
  check.
- **Measurement tiers (RULED — no variable tracing):**
  1. **Static text measures always** — including *translated* line
     tables against the same budgets: the intl overflow check (German
     +30%) is the killer application and needs zero tracing.
  2. **Slots get opt-in declared allowances** (a width hint riding
     `slot_info`). The roster-bound case (a cue-name slot measured
     against the longest declared name) is a lookup, not tracing.
  3. **No hint → honestly unmeasurable** — a distinct, quieter
     diagnostic; never guessed.
  Dataflow/variable tracing is explicitly out (maintainer ruling).
- Escape hatch: a host-supplied measurer callback for exotic
  rendering; declarative metrics are the 90% case.
- Enumerable variants are measurement-critical: recognizer growth
  (#1446) is promoted from intl-nicety to measurement prerequisite.

## 7. Runtime output (RULED in substance, sitting 3 — naming ⏳)

**Break-compat is RULED**: no external consumers exist; in-repo
consumers (bevy, web, TUI) migrate in-PR; `@brink-lang/web` takes a
major. Don't break gratuitously — but compat holds no veto.

Proposed shape — **separate the fused axes** (content vs
why-we-stopped):

```rust
pub enum Step {                 // continue_single() -> Step
    Line(OutputLine),           // content — keep going
    Choices(Vec<Choice>),       // stopped: present these
    Done,                       // stopped: turn complete
    End,                        // stopped: story over
}
pub struct OutputLine {
    pub element: Element,       // kind ("narrative" degenerate) + resolved data
    pub parts: Vec<Part>,       // Text(String) | Span { kind, attrs, children }
    pub tags: Vec<String>,
}                               // .text() derived, never stored
pub struct Choice {
    pub parts: Vec<Part>,       // choice text is prose; markup legal
    pub tags: Vec<String>,      // element slot ⏳ (choices × elements)
}
```

- Terminals carry **no text** (prior `Line` events delivered it — the
  fused shape existed only for the old bundled API).
- `parts` is the measurement input and the locale-re-resolution
  product; the transcript keeps storing refs.
- `element.data` carries attachment-resolved payloads (+ block id per
  §3.7).
- `FlowInstance::advance` keeps `AwaitingExternal`, wrapping `Step`.
- Superset check: schema-less ink → `element: narrative,
  parts: [Text]` — information-identical to today.
- Naming (`Line`/`Step`/`StoryEvent`) ⏳.

## 8. Worked cases (abbreviated; spellings illustrative)

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

## 8b. Sitting 4 — concrete syntax rulings (2026-07-25)

The syntax round, held against docs/prose-element-inventory.md. RULED
unless marked:

1. **Lyrics element: dropped** (the `~` conflict dies with it).
2. **Header-scoped stitch bodies.** A scene-heading stitch is
   delimited by the next heading or the enclosing close — **amending
   charter §4's "braces are the universal body delimiter"** for preset
   heading-elements in prose-ground only. Consequences embraced:
   heading-stitches are **flat siblings** (scenes don't nest — as on a
   real page); deeper nesting uses the general `flow x { }` spelling,
   which remains first-class in prose-ground; this is a restoration of
   ink's own header-scoped stitch, not an invention.
3. **Slug spelling: trailing `[slug]` on the heading.**
   `INT. MARKET SQUARE - NIGHT [market] #tense #act1`. Rejected:
   `#x#` (tag-lexer clash), `{x}` (lexes as interpolation — headings
   get no carve-out). Line order: pattern, `[slug]`, tags.
4. **Tags on declarations**: trailing `#tag`s on header lines — both
   the heading spelling and `flow x #tag { }` — captured as
   **container-level per-flow tags**. This is the authoring surface
   #474 (per-flow tag APIs) was iceboxed waiting for.
5. **The conventions schema gains an *address capture* role** — the
   slug capture feeds structure/`DefinitionId`, unlike ordinary
   payload captures.
6. **Divert line-neutrality (native dialect).** Diverts are invisible
   to line assembly — they never end, join, or contribute to an output
   line; **glue is the only joiner**. Divert placement is therefore
   pure formatting: fmt normalizes to own-line (choice-line trailing
   diverts exempt — anatomy, not formatting). Ink dialect keeps its
   oracle-bound inline-divert joining untouched (superset doctrine).
7. **Transitions and scene entry are lowered host calls, not content
   lines.** Their runtime consumer is the *engine*, not the reader —
   so they ride the existing non-blocking command/extern machinery
   (journaled, effect-checked via the manifest). **Scene entry** = the
   scene-heading element's default lowering (`scene_entered(title,
   slug)` fires on entering the stitch — pure codegen, a call planted
   at the top of the body). **Written transitions** (`SMASH CUT TO:`)
   = a departure-site style call; the bare-scene-divert default cut
   needs no authored transition (the slugline implies the cut, as in
   real screenwriting). **Diverts remain absolutely invisible** — no
   annotations, no target-inference, no exceptions.
8. **The lowering column.** The conventions schema gains per-element
   `lower: content | call(name, args ← payload) | nothing`. v1 is
   declarative; the §3.5 comptime conventions-module later *computes*
   the same value through the already-open door. **Power boundary
   (RULED):** per-element, call/content/nothing only — arbitrary
   rewriting is a macro system and a separate future round.
9. **Compact cue `@NAME: text`** (the Yarn cross): cue + single
   dialogue line fused, as a second declared pattern beside the block
   cue. Accepted on the sample page.
10. **Choice typing: 🔶 lean (c)** — the cue above a choice block
    supplies the speaker; **quoted** option text marks a spoken
    (dialogue-)choice, unquoted/bracketed options are action-choices;
    mixed blocks legal; with no cue, quoted options attribute to the
    roster's PC. Recorded as lean, cheap to flip.
11. **Point markers** (`<pause/>`, `<sfx name="bell"/>`) named as an
    explicit span use case — in-text events, mapping to XLIFF `<x/>`.
12. **Parking is not a Step** (the questline case, §8c): `until`
    parks surface at the `FlowInstance::advance` layer
    (`AwaitingExternal`-family), never as a `Step` variant.
13. Yarn's persisted-line-ID localization model added to the
    translation-round-2 agenda as a genuine fork vs `source_hash`
    (hybrid opt-in is the candidate).

## 8c. Worked case: the gated questline (validation)

```
EXT. RUINED CHAPEL - DUSK [chapel_found]

You mark the chapel on your map. Somewhere below, something waits.

~ until quest.chapel_key_found
-> chapel_interior
```

Zero new machinery: `until` parks the flow (FlowSleep reactive wake,
built); the invisible divert fires on wake — possibly much later —
and arrival triggers the next scene's `scene_entered` lowering; the
parked flow is save-persistent (serializable coroutine); the suspend
dimension infers into the flow's effect row. The fused
`~ until cond -> target` spelling is optional sugar ⏳. Editor
decoration: the parked line renders as an *awaits: criteria → SCENE
TITLE* annotation via the display-name machinery.

## 9. Open threads ⏳ (the resumption points)

1. **Syntax round remainder** — cue extensions (`(V.O.)`/`(O.S.)` as
   parsed payload vs opaque), centered's fate (lean: span), the fused
   `until cond -> target` sugar call, escape-set finalization, the
   `std::conventions` types design (§3.5), the §3.5b **match-ordering
   rule** and deferred context-injection question, and the final
   complement-pass read over a full page.
2. **Choice typing** — flip-or-ratify the 🔶 lean (c) (§8b.10).
3. **Translation, round 2** — element data in XLIFF (speaker names
   translatable?), per-locale budgets, the bump batching, scene-title
   localization under call-lowering (§8b.7 rider), and the Yarn
   persisted-line-ID fork (§8b.13).
4. **Per-path export design** (§2b.4) — Fountain/FDX/VO-sheet
   renderers as intl-exporter siblings; the element↔Fountain mapping.
5. **Terminal cluster — parked**: #1448 / #1449 / #1450, deferred
   until the runtime/compiler window.
6. **§7 output naming** (`Line`/`Step`/`StoryEvent`) — shape ruled in
   substance; which element kinds form blocks (lean: id universal).
7. **Editor implications (NS-T)** — line-local classification vs the
   nesting-property dialect; the §2b.3 bridge features; the §8c park
   decoration. #1350/#1131 stay held on this round.
