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
- **Enforcement (issue #1844, `E169`):** when `elements` names a
  project-relative path, a pattern-claiming `@[element(claims =
  "…")]` handler declared in any *other* file is a compile error — the
  module half of the §9.1 confinement asymmetry (`docs/decision-log.md`
  2026-07-31 item 4). An unset `elements` key, or one naming a built-in
  preset rather than a path, enforces nothing yet (no project file to
  confine against) — see `E169`'s own doc for the exact boundary.

### 3.5 Conventions are authored as a brink module (RULED, sitting 4 addendum)

> ⚠ **SUPERSEDED IN PART, 2026-07-31** — see `docs/decision-log.md`
> "Conventions are annotated handlers: the declarative element surface is
> subsumed by the annotation surface (§9.1 settled)". The module form,
> the `brink.toml` pointer, presets-as-modules, the purity/determinism
> gate, and data-as-generated-interchange all STAND. What changes: the
> well-known entry is `fn conventions()` which **registers handlers in
> order**, not `fn conventions() -> Conventions`; the `Conventions`
> **type does not exist**; and §9.1's owed "types shaped for extension
> ergonomics" is dissolved rather than designed.
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
  name entry (mirroring `flow main()`); the portable-regex subset
  validated at marshal with module-pointing errors; the editor
  re-evaluation loop; sequencing (native construction literals ride
  the #1103 build; the module may be brink-dialect until then). The
  `brink.toml` pointer itself (§3.4's `elements = "conventions.brink"`
  — corrected here from a stale `conventions = …` spelling that never
  matched §3.4) is **no longer owed for parsing**: `brink-project-
  config` parses `[project] elements` and `brink-analyzer` carries it
  onto `AnalysisOptions` (issue #1844), which also enforces the
  confinement half of item (4) below (`E169`) for a path-shaped
  pointer. Resolving it against an *evaluated* `fn conventions()`
  registry (the dispatch-consuming half, not just the confinement
  check) has its injection point built (issue #1863:
  `hir::lower_native::lower_with_conventions` accepts an already-ordered
  external handler set, joined on `DefinitionId` by
  `brink_analyzer::conventions_registry`) but not yet live-wired to a
  real evaluator — that is #1840's own job.

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

### 3.7 Consecutive lines & blocks (RULED — closed by §8d.2)
Multiple content lines under one attachment (a multi-line speech) form a
**block**. Emit **per-line** (the finer granularity — pacing stays a host
beat) with a **baked block id** in element data; block-level delivery is
API sugar / host aggregation over the id, robust under control flow
(id-matching, not positional). Screenplay `(CONT'D)` is derivable (same
speaker, new block) — an editor/preset concern, not wire.

The one open question this section carried — *which* runs get a block id
— was closed by **§8d.2: block id is universal**, every run of same-element
adjacent content lines carries one, and §9.5 has recorded it closed since
sitting 5. The 🔶 marker on this header was stale and is retired here
(issue #1715). The wire half — the `block_id` field itself — rides the
format bump (#1683) and the `Step`/`OutputLine` contract (#1684).

### 3.5b The `@[element]` annotation surface (RULED, sitting 4 addenda 2–3)

> ⚠ **EXTENDED 2026-07-31** (same decision-log entry). This is now the
> **only** element mechanism — preset elements are annotated handlers
> too, matched by natural-notation pattern instead of `!name` dispatch.
> Added: `block` capture (the following run as a `content` param,
> terminated by a blank line or any element-level line; the handler
> WRAPS it). Reversed: "zero comptime" as a restriction, and the role
> boundary that reserved attachment/chains to the declarative side.

The **second authoring surface**, and the one that ships first:

```brink
@[element(args = "^(?<chan>[A-Z0-9-]+): (?<text>.+)$")]
@[style(chan = "channel", line = "radio")]
fn radio(chan: string, text: content) {
    host::radio_ping(chan)
    > [{chan}] {text}
}
```

```
!radio TAC-2: All units report in.
```

- **Sigil + name dispatch (RULED, addendum 3).** Annotation elements
  live behind the `!` sigil: `!name args…`. The first identifier
  dispatches **by name** to the annotated fn (fn name, or an alias
  given by `@[element(name = "alias")]`); the `args` pattern parses
  only the remainder,
  binding captures to params. Rationale: without a sigil, a user
  pattern can silently claim natural prose — declared, but *invisible
  at the use site*; the sigil makes every rewritten line
  self-announcing (the explicit-format posture applied to macros).
  Name dispatch **dissolves the match-ordering problem entirely**:
  duplicate names are ordinary duplicate-definition errors, and an
  unmatched remainder is a targeted diagnostic naming both the line
  and the handler's pattern. (Fountain's `!`-forces-plain-action
  inversion noted and accepted — "good point of reference, not
  married to it.")
- **Pattern power proportional to auditability.** Natural-notation
  pattern claiming (lines that don't announce themselves — `INT.`
  headings, `@` cues) is *confined*, not banned: the declarative side it
  was originally reserved for is **dissolved** (`docs/decision-log.md`
  2026-07-31), so a claim is now an ordinary annotated handler spelled
  `@[element(claims = "…")]` — still one centralized, auditable place,
  now readable source rather than an interpreted table. **Landed by issue
  #1838**: a claimed line is matched, its named captures bind the
  handler's params by name (checked in *both* directions — `E160` for a
  capture with no param, `E167` for a param with no capture, since every
  argument of the rewrite comes from a capture), and the line lowers to
  exactly one call. Only a **top-level `fn`** may claim (the rewrite is an
  expression call; `E112` otherwise — including a `fn` declared inside a
  `module { … }` block, issue #1847: it reads as un-nested by
  `flow`/`fn`-depth alone, but the dispatch table only ever scans the
  file's direct declarations, so admitting it would silently register
  nothing to claim with), only a wholly literal prose line or scene
  heading is a candidate, and a claiming handler never claims inside its
  own body (§3.5's staging rule). Confinement to the
  `brink.toml`-named conventions module is not yet enforced — single-file
  lowering has no project identity to check against, and v1 dispatch is
  file-local, so a claim is visible in the file it affects.
- **Dispatch order (interim, issue #1848).** When a file declares more
  than one claiming handler and more than one could match the same line,
  the earlier-declared one wins: top-level declaration order,
  first-match-wins. **This is explicitly not the permanent rule** — item
  (5) of the ruling above says a `fn conventions()` well-known entry will
  eventually *register* handlers in order (issue #1840), and that
  registration order, not a claiming `fn`'s textual position in the file,
  becomes authoritative once it lands. Two claiming patterns that can
  both match the same line get no diagnostic today except the narrowest
  provable case — `E168` fires when two patterns are byte-identical (an
  identical pattern matches an identical input set, so the later one is
  certainly dead). A genuine overlap between two *different* patterns
  (one a strict subset of the other, a shared alternation branch, …) is
  real and more common, and is the case "pattern power proportional to
  auditability" above most wants surfaced. `E170` (issue #1859) now
  covers the provable slice of this: the later pattern's language
  **subsumed** by the earlier one's, proven from a set of witness
  strings generated from the later pattern's structure. Subsumption,
  not mere overlap, is the deliberate bar — a pair that overlaps
  without one subsuming the other (a shared alternation branch where
  each also matches lines the other doesn't, two prefixes that
  compete but don't nest) is not flagged, because the later handler in
  that case is genuinely live for the lines only it matches, not dead
  code.
- **Zero comptime** / the rewrite is exactly **one call** / the body
  is arbitrary *runtime* code within its declared effect row —
  unchanged (addendum 2).
- **Two surfaces, one value**; role boundary (content/call-shaped
  only; attachment, chains, structure stay declarative) — unchanged.
- **Capture contract** — unchanged: named captures bind params by
  name (compile-checked); `string` = literal/stringified;
  **`content`** = first-class value via the existing fragment-capture
  path, **translation-resident and measurable**; prose-bodied
  handlers (`>{ }`) are once-translated parameterized templates. A
  `claims = "…"` handler's captured param declaring a numeric/struct/
  generic/`fn` type (anything a plain string capture could never
  satisfy, `content` excepted — see `E171`'s own doc for why `content`
  stays exempt) is now a targeted diagnostic (`E171`, issue #1849) at
  the declaration, naming the deferred-coercion reason, rather than
  silence — the pre-#1849 state. The generic form of this mismatch
  (an ordinary direct call's arguments checked against the callee's
  declared parameter types) only appears once #1864 lands direct-call
  argument type-checking, which does not exist yet; the coercion gap
  itself stays deferred (see below), only the silence around it is
  closed.
- **`block` capture (RULED, 2026-07-31 sitting, issue #1839).**
  `@[element(args = "…", block)]` declares that the handler captures the
  run **following** its matched line into a trailing `content`-typed
  param, terminated by a blank line or any element-level line — the
  handler WRAPS the captured run. That is the whole of what the ruling
  itself says. `E166`, the declaration surface's own static check, adds
  two implementation-level requirements the ruling's own wording does not
  state — recorded here so they live somewhere other than a private
  helper's doc comment: the qualifying `content`-typed parameter must be
  the declaration's **last** parameter, and its name must not collide
  with one of `args`' own named capture groups (a capture and the block
  receiver cannot be the same param) —
  `crates/internal/brink-ir/src/hir/lower_native/annotation.rs`'s
  `has_block_content_param`. Declaration-surface only, matching #1719's
  own scope for `element`/`style`: the `!name`/natural-notation dispatch
  rewrite that would actually match a line, find the block's terminator,
  and call the handler with the captured run is issue #1838's scope, not
  delivered here. `content` is now a resolvable annotation type name
  (`Ty::Content`, issue #1846 — see the Deferred bullet below), so a
  conforming `block` declaration compiles cleanly under `dialect = brink`
  on its own `content`-typed parameter; it is still **not usable
  end-to-end** — nothing dispatches to it yet, since the `!name` sigil
  rewrite that would actually bind a captured run to that param remains
  issue #1838's scope.
- **`@[style]` — declared editor presentation (RULED, addenda 3–4).**
  A companion annotation mapping captures (and `line` = the whole
  line; `dispatch` = the `!name` prefix) to style values, drawn from
  a **built-in presentation vocabulary** — a closed,
  LSP-semantic-token-style set every conforming editor implements
  natively, *no plugin or CSS required*: alignment
  (`left`/`center`/`right`), emphasis (`bold`, `italic`, `dim`,
  `mono`), case (`uppercase`), and **`conceal`** (riding the shipped
  hidden-span / atomic-range machinery — also the declared spelling
  for hiding the dispatch prefix). Raw color remains a basic
  theme-overridable default; any other name is a **custom hook**
  emitting a stable `brink-*` class for host CSS. The conventions
  value gains the same style column — and with it **the screenplay
  preset becomes self-describing**: transitions declare
  `[right, uppercase]`, cues `[uppercase]`, replacing the editor's
  hardcoded screenplay CSS; a bare token-conforming editor renders
  screenplay correctly with zero configuration. Degradation stated up
  front: the full set renders in the CM6 package; plain LSP carries
  emphasis/color via standard semantic-token modifiers but has no
  alignment/conceal. **Editor-presentation only** — buffer
  decoration, firmly distinct from the runtime markup layer (output
  styling = the handler emits markup spans). Considered-deferred:
  indent-level tokens.
- **Tooling transparency (no invisible expansion)** — unchanged, and
  half-landed by #1838: `brink_ir::HirFile::element_matches` records
  every claimed line as `(line range, matched kind, handler name + its
  declaration range, the claiming annotation's range, captures as spans,
  disposition)`. `LineContext` carries the matched handler (fn + source
  location) and capture bindings as spans (now with their style hooks);
  hover shows the handler's signature and body; the explain-match query
  answers is/isn't-matched + what bound — those consumers ride the held
  editor track and read this record rather than re-running the match.
- **Deferred**: numeric capture coercion (issue #1849 added `E171`, a
  declaration-time diagnostic for a `claims` handler's non-`string`
  captured param, so the gap is loud rather than silent — the coercion
  itself, and binding a non-`string` capture to a real typed value,
  remain unbuilt); context injection (handlers
  reading attachment data); `Option` params for optional captures;
  binding a `content`-typed param to an actual captured `FragmentRef`/prose
  block (issue #1846 gave `content` a resolvable `Ty` in the native type
  system — the ruled `fn radio(chan: string, text: content)` example's
  `text` param, and `block`'s own trailing receiver param, now compile —
  but nothing yet *dispatches* a captured run into one; that binding rides
  the `!name` sigil rewrite below, issue #1838's scope); the `!name` sigil
  dispatch rewrite itself (matching
  a content line, binding captures, lowering to a call — issue #1719
  delivered the `@[element]`/`@[style]` **declaration surface**, and issue
  #1839 widened it with the `block` clause's own declaration-surface
  contract (`E166`, see the bullet above): parsing, portable-regex
  validation, the captures-bind-params-by-name compile check, and the
  block-receiver structural check, all in
  `brink_ir::hir::lower_native::annotation`; nothing dispatches yet, for
  either);
  cross-file dispatch-name resolution and the duplicate-dispatch-name
  check (v1 validates one declaration at a time — and #1838's claiming
  dispatch is file-local for the same reason); block capture (issue
  #1839) and `fn conventions()` registration + comptime (issue #1840 —
  **blocked on four rulings**, sized in
  `docs/conventions-comptime-sizing.md`: the identity a registered handler
  carries across the comptime boundary, whether a comptime fault fails the
  build or degrades to an empty convention set, the diagnostic floor while
  epic #452's instruction→range carrier stays `needs-design`, and — sharpest
  — what `register`'s own effect row is, since an `EXTERNAL` `register` lands
  in `EffectRow.calls` and makes the ruling's canonical `@[effects(pure)] fn
  conventions()` fail its own fence with `E103`); **multi-token style
  values** — issue #1719's `@[style(key = "value")]` value is a single
  presentation token today (one `StyleToken` per key), not the
  space-separated list this section's own screenplay preset describes
  (`[right, uppercase]`, `[uppercase]`); a `key = "a b"` value lowers to
  one `StyleToken::Custom("a b")` rather than two tokens.
- **Staging**: v1 = built-in screenplay preset + `!`-dispatched
  annotations (zero comptime); the §3.5 conventions-module evaluation
  arrives later for authoring full custom presets.

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

**Both halves landed** — freeform in PR #1732, manifest validation in
issue #1733. The vocabulary is the host capability manifest's `markup`
key (`docs/host-capability-manifest.md` § "Markup vocabulary"), an array
of `{ name, attrs }` span kinds. An empty/absent `markup` section leaves
markup freeform, including for a project whose manifest declares only
`externals`/`types` — declaring a span kind is the *only* thing that
turns checking on. An undeclared tag is `E164`, an undeclared attribute
on a declared kind is `E165`; both default to `Warning`, which is what
makes them `[lints]`-configurable and `@[allow(…)]`-suppressible (only
`Warning`-base codes are — a hard-error code can be neither). Attribute
*values* are unchecked: they are static text by construction, so only
the attribute name is vocabulary. Implemented in
`brink_analyzer::markup_check`, wired into `per_file_diagnostics`.

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
  with any other pending line-table growth at the time — the intl
  spec's own "future recognizers" section that this note originally
  meant to batch with (inline conditionals → `Select`, sequences as
  slots) was **retracted** by issue #1667: neither ships, and
  `LinePart::Select` stays target-side only. Inline conditionals/
  sequences needed **no** format bump at all — `hir::normalize_file`
  already lifted them into independently-recognized branches on
  2026-03-15, the actual #1667 gap was a missing `Text`-part merge at
  the splice seam (`normalize.rs::extend_merging_text`), a pure HIR-level
  fix. This span work is still the next thing that would need a real
  format bump.
  **RESOLVED by #1716** — this claim was correct and stands, despite
  §4.5's implementation note below having briefly claimed otherwise: the
  bump landed for real, `.inkb` `VERSION` 5 → 6 and `.inkl` version
  1 → 2 (`docs/format-spec.md` § Versioning). See §4.5.
- **The recognizer is the chokepoint**: marked-up lines must be
  *admitted to line recognition* or they shred into per-run entries and
  stop being single translation units. (Inline conditionals/sequences
  used to shred the same way for a narrower reason than "never lifted"
  — issue #1667 found the lifting already existed and fixed the
  seam-merge gap that kept it from ever matching `Plain`/`Template`;
  span admission for the prose round is a separate, still-open concern.
  Choice display/bracket/inner text with an inline conditional/sequence
  is a related gap #1667 did *not* fix — `normalize_file` never walks
  choice display text, only choice bodies — filed as a follow-up.)
  **Partially closed by #1716** — a span whose descendants are only
  Text/Interpolation/Span is now admitted; a span nesting an inline
  conditional/sequence still falls back to the flattening path (no data
  loss, just no wire `LinePart::Span` for that shape). See §4.5.
- **Public API**: `Line` grows a structured span surface (additive).
  Prefer structural parts over byte-range offsets — the runtime trims
  and collapses whitespace after assembly, so ranges are a trap.
  **Not yet landed by #1716** — `Line::Text` still resolves a span to
  its children's concatenated text with the tag stripped; the
  structured surface this bullet describes is still deferred to the
  §7/§9.1 `Step`/`Part` redesign. See §4.5.

### 4.5 Implementation status (#1716, 2026-07-29)

§4/§8b.11/§8d.3/§8d.6 landed, matching #1715's block-element grammar's
staging shape but going further — not "grammar only, everything E129": a
span in ordinary prose lowers and ships, because a grammar-only landing
here would have *regressed* previously-accepted content (any existing line
containing a bare `<letter…` sequence used to compile as literal text; a
grammar addition without lowering would have turned it into a hard parse
error, violating the superset doctrine, §1). What shipped:

- **Grammar** (`brink-syntax-native`): blunt lexing (§4.1) reuses existing
  tokens (`LT`/`SLASH`/`IDENT`) — no new lexer tokens, since `GLUE`/
  `THREAD` are already distinct compound tokens. Nesting doctrine (§4.3)
  enforced structurally: `span`'s recursive body scan is handed the exact
  `stop` set its caller was given, so a fragment-scope boundary reached
  before the matching close tag makes the span unrepresentable as closed,
  not merely checked. Escape set (§8d.6) final.
- **HIR** (`brink-ir`): `hir::ContentPart::Span { name, attrs, children }`,
  recursive. Every `ContentPart` consumer (reference-tracking walkers,
  type inference, the symbol index, several `brink-analyzer` passes, the
  heap-size estimator, the editor's HIR projection) recurses into a span's
  children instead of treating it as opaque.
- **Wire + hash-transparency** (`brink-format`, `brink-ir`'s
  `lir::lower::recognize`): `LinePart::Span` landed as a new `u8`
  part-tag (`PART_SPAN`) **and a real `.inkb`/`.inkl` version bump** —
  `.inkb` `VERSION` 5 → 6, `.inkl` version 1 → 2
  (`docs/format-spec.md` § Versioning). This section's earlier "a new
  `LinePart` tag is a version bump (v6)" claim was correct; an
  intermediate draft of this note wrongly claimed the #1519 "one-bump
  rule" precedent (`VAL_VEC2`/`VAL_WEIGHTED`) covered `PART_SPAN` too,
  but that precedent only excuses materializing a tag the v4 RFC had
  already *pre-reserved* — `PART_SPAN` was never reserved, so
  introducing it is its own one-bump event, same as v5's `AliasTable`.
  Coordinated with #1683 (the v6 bump manifest): this PR lands only the
  `Span` payload, and `VERSION` 6 stays open to absorb #1683's remaining
  payloads (element data, block id, choice captured environment)
  without a further bump. Hash-transparency is proven end-to-end
  (native source → HIR → LIR → `source_hash`), not merely asserted.
- **Span admission, partially closed**: a span whose descendants
  (recursively) are only Text/Interpolation/Span *is* admitted to line
  recognition — a markup-only line with zero interpolations
  (`Hello <wave>world</wave>`) is a single translation-unit `Template`
  line, not shredded. What's still open, per this section's own prior
  note: a span nesting an inline conditional/sequence is *not* admitted
  (falls back to the flattening `EmitContent` path — no data loss, just
  no wire `LinePart::Span` for that one shape) — a narrower remainder of
  the "span admission… separate, still-open concern" bullet above, not
  the whole thing.
- **Runtime**: `Line::Text`'s current flat-string shape resolves a span to
  its children's concatenated text, tag stripped — correct for today's
  API, and additive groundwork for the structured `Part::Span` surface
  this section's last bullet still wants once §7/§9.1's `Step`/`Part`
  redesign lands.
- **Intl**: `lines.json` (`PartJson::Span`) round-trips a span fully.
  XLIFF export (`xliff_convert.rs`) maps a span to a real inline code —
  non-empty `children` → a paired `<pc>`, the childless point-marker
  shape (§8b.11) → a standalone `<ph>` (XLIFF 2.0 core has no literal
  `<x/>`; `<ph>` is its standalone-code element) — with `name`/`attrs`
  carried in `originalData`, so a translated `.xlf` reconstructs the
  exact span structure on import, not just its flattened text.
  **Resolved by #1734** — see below.

Deliberately not attempted **by #1716/#1732**: manifest validation of
markup vocabulary (§4.2's second half); the `Step`/`Part` structured
runtime surface (§7/§9.1); a real XLIFF `<pc>`/`<x/>` inline-code mapping
for spans.

**Manifest validation has since landed** (issue #1733) — that item's
stated blocker ("needs the host capability manifest facility, which is a
separate track") was already stale when written: `brink_ir::HostManifest`
and its `AnalysisOptions`/`IdeSession`/`EditorHandle.setHostManifest`
registration path shipped with the Tier 1 + closed Tier 2 MVP
(`docs/host-capability-manifest.md` § "Implementation status"), so the
`markup` section layered onto an existing facility rather than waiting on
one. See §4.2 above.

**The XLIFF inline-code mapping has since landed too** (issue #1734,
`xliff_convert.rs`'s `push_part_inline`/`elements_to_parts`) — real
`<pc>`/`<ph>` mapping, described above. The only item still open is the
`Step`/`Part` structured runtime surface (§7/§9.1).

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
10. **Choice typing: RULED (sitting 5) — cue-only.** The cue above a
    choice block types **all** its options as that speaker's dialogue
    options; quotes carry **no** typing semantics (quotes are just
    prose). No cue → options are plain action-choices (want PC
    dialogue options? write the PC's cue — explicit, like everything
    else). Non-spoken options need no special rule: an all-bracketed
    option (`* [Slip away] -> alley`) delivers nothing via the
    existing `[]` anatomy, so nothing is "spoken" regardless of the
    cue. The quote-based lean (c) is superseded.
11. **Point markers** (`<pause/>`, `<sfx name="bell"/>`) named as an
    explicit span use case — in-text events, mapping to XLIFF `<x/>`.
    **Superseded by #1734/§4.5** — XLIFF 2.0 core has no literal `<x/>`;
    point markers map to a standalone `<ph subType="brink:x">` instead
    (`<pc>` is the mapping for non-empty spans). See §4.5.
12. **Parking is not a Step** (the questline case, §8c): `until`
    parks surface at the `FlowInstance::advance` layer
    (`AwaitingExternal`-family), never as a `Step` variant.
13. Yarn's persisted-line-ID localization model added to the
    translation-round-2 agenda as a genuine fork vs `source_hash`
    (hybrid opt-in is the candidate).

### 8b-i. Implementation status — the block-element grammar (#1715)

The *grammar* half of §8b/§8d is built, in `brink-syntax-native`
(`parser/element.rs`, `SyntaxKind::SCENE_STITCH`/`SCENE_HEADING`/
`SCENE_TITLE`/`SCENE_SLUG`/`SCENE_BODY`/`CUE`/`CUE_NAME`/`COMPACT_CUE`/
`PARENTHETICAL`):

- the heading pattern with trailing `[slug]` then tags, in that line
  order (§8b.3), and the header-scoped stitch body (§8b.2) — flat
  siblings, delimited by the next heading or the enclosing close;
- the block cue, the compact cue `@NAME: text` as a second declared
  pattern beside it (§8b.9), and cue extensions on the tag channel
  (§8d.4);
- parentheticals, gated on a live cue chain so G-1's `(label)`
  content-line spelling is untouched outside dialogue;
- trailing `#tag`s on a `flow` header line (§8b.4).

**One shape lowers; the rest deliberately do not.** Issue #1838 landed
natural-notation dispatch, so a **scene heading** whose text an
`@[element(claims = "…")]` handler matches lowers to exactly one call on
that handler (`brink_ir::hir::lower_native::element`) — the first time any
of this grammar reaches output. Everything else stays staged: element
roles/attachment are §3.6/§8b.7–8 → issue #1717; the built-in preset is
#1720; per-flow tag *APIs* are #474, whose iceboxed authoring surface this
grammar supplies. (The conventions `lower:` column this paragraph used to
name is **dissolved** — see `docs/decision-log.md` 2026-07-31.) Until those
land, `hir::lower_native` reports every unclaimed shape as not-yet-lowered
(`E129`) rather than reading it as ordinary prose or dropping it.

The **lyrics element stays dropped** (§8b.1): there is no `LYRICS` shape
in the grammar, and the `~` conflict died with it.

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

## 8d. Sitting 5 — rapid closure rulings (2026-07-25)

1. **Choice typing: cue-only** (see §8b.10, superseding the lean).
2. **Block id is universal** (RULED): every run of same-element
   adjacent content lines carries one; hosts aggregate or ignore.
3. **Centered = `<center>` markup span** (RULED). Not an element.
4. **Cue extensions ride the tag channel** (RULED): `@VENDOR #(v.o.)`
   — no parsed `ext` capture, no new payload machinery; the cue
   line's tags attach with the cue's data, and the export mapping may
   translate known tags to Fountain extensions.
5. **Fused `until cond -> target`: deferred.** Two-line is the
   canonical v1 spelling; the sugar can land later without breaking
   anything.
6. **Escape set final** (RULED): `\<` `\{` `\#` `\\` inline,
   `\!` `\@` line-start; backslash before anything else is a
   compile error.
7. **The output enum is `Step`** (RULED — §7's naming ⏳ closed).
8. **No baked scene fields in the output format** (RULED): element
   data stays an **open map** that conventions (declarative captures)
   and handlers produce; time-of-day, or anything else a project
   wants, is preset-configurable data — never a privileged field.
9. Context injection and numeric capture coercion: deferrals stand.

### The complement-pass page (everything applied)

```
INT. MARKET SQUARE - NIGHT [market] #act1

The square is empty. A single lantern gutters against the dark.

@VENDOR #(v.o.)
(hushed)
You shouldn't be here after dark. The gates closed an hour ago.

@KID: Says who? <i>You?</i>

@VENDOR: The curfew, kid. <sfx name="bell"/> That.

!radio TAC-2: All units, market square sweep in five.

{?
  @KID
  * "I was just leaving."[] I muttered, backing away.
  * "Make me."
    The vendor's eyes narrow. Bad start.
  * [Slip into the alley] -> alley_escape
}
The bell tolls again. <pause/> Somewhere above, a door slams.

~ until patrol_started
-> alley_escape

EXT. COLD ALLEY - CONTINUOUS [alley_escape]

<center>LATER</center>

Cold brick. Distant bells.
```

Reading notes: the heading is a header-scoped stitch with slug + tag;
the cue's extension is a tag; block cue and compact cue coexist; the
`!radio` line dispatches by name to its handler (rendered per its
`@[style]`); all three options are KID's dialogue-choices (the
bracketed one delivers nothing — `[]` anatomy); the rejoin is the
next line after `}`; the park then the invisible divert; arrival at
the second heading fires `scene_entered` (the cut, by decoration);
`<center>` is a span. Every mark is real; the editor supplies the
grace.

## 9. Open threads ⏳ (the resumption points)

1. **The `std::conventions` types** design pass (the later-stage
   module-authoring surface, §3.5) — the last substantial prose-round
   design item.
2. **Translation, round 2** — ~~element data in XLIFF~~ (**closed by
   ruling, not by delivery** — decision-log 2026-07-26: v1 XLIFF export
   carries content and markup spans only; element kind and data are
   *never* exported, living in the base `.inkb` shared across every
   locale. What #1734 delivered is the `<pc>`/`<x/>` inline-code mapping
   for `LinePart::Span`, §4.5 — the span round-trip, not element data.
   An earlier phrasing here credited #1734 in a way that read as
   "element data shipped"), per-locale budgets, the bump batching,
   scene-title localization under call-lowering, the Yarn
   persisted-line-ID fork.
3. **Per-path export design** (§2b.4) — Fountain/FDX/VO-sheet
   renderers; export mapping incl. tag→extension translation (§8d.4).
4. **Terminal cluster**: #1450 closed (PR #1468); #1448 landed
   (PR #1500); #1449 is un-deferred and partly delivered (PR #1513
   folds the episode-builder terminal handling into a single
   classifier; the Step/OutputLine runtime redesign itself remains).
   The runtime half is #1520, written up in
   `docs/design/yield-time-terminal-classifier.md` — it needs two
   rulings before code moves: **R1, where the classification surfaces**
   (a `Line`-shaped change now vs. folding into `Step`, per §7/§8d.7)
   and **R2, whether the `RanOutOfContent` fault moves to the same
   `continue`** (a ratchet-moving change that would retire #1522's
   extra-step allowance).

   ✅ **RULED 2026-08-01 — this thread is closed.** **R1: #1520 folds into
   #1684** (the classifier's output *is* `Step`'s variants; no interim shape,
   no standalone refactor) — so **#1684 now carries zero outstanding rulings**.
   **R2 (#1574): NO** — `RanOutOfContent` keeps the deferred fault, brink does
   not adopt `Story.cs`'s raise-on-discovery behavior, and `did_safe_exit()`
   remains how callers distinguish a real `-> DONE`. ⚠ Consequence: the
   `oracle.rs:227` and #1522 extra-step allowances are **PERMANENT**, and the
   divergence from `Story.cs`'s `!canContinue` branch is **intentional** — stop
   describing either as pending. The ran-out-of-content **message** does split
   into four C#-matched variants (#1993), an axis independent of fault timing.

   Historical note (superseded by the above): **R2 was split out to #1574** on 2026-07-26,
   so this is no longer "two rulings on one issue" — #1520 owns R1, #1574
   owns R2, and **neither is ruled** (no decision-log entry touches this
   cluster after 2026-07-25). Note also that **#1573/PR #1577 shipped a
   weak form of R1 option (b)** unruled, promoting `did_safe_exit` to
   production API; the design doc pre-empts the misreading — that exposes
   today's already-computed bit as-is and is *not* the R1 ruling. R1
   option (c) — fold #1520 into #1684 — would leave zero rulings
   outstanding on this cluster. Separately: **#1684 itself is blocked on
   implementation only, not on either ruling**, and carries its own
   ratchet hazard (see #1684).
5. **Deferred details**: context injection; numeric capture coercion;
   fused-`until` sugar; indent-level style tokens; which-blocks
   question closed by §8d.2.
6. **Editor implications (NS-T)** — #1350/#1131 stay held; the bridge
   features (§2b.3), the park decoration (§8c), the built-in token
   vocabulary (§3.5b) are their incoming scope.

   ⚠ **Scope of the hold, ruled 2026-08-01:** it covers editor *frontend*
   work — CM6, token rendering, the live renderer, `fmt`. It does **not**
   cover the classification/explain-match query family, which is emitted from
   `brink-db`/`brink-ide` and is **compiler work wearing editor's clothes**
   (#2006). Holding compiler-side queries behind the compiler track would
   hold them behind themselves.

   ⚠ **Why they are held, restated 2026-08-01 (maintainer).** The hold is
   **deliberate sequencing — the compiler work finishes before the editor
   work starts** — *not* an unlifted blocker waiting on this document. The
   original 2026-07-25 rationale ("don't classify tokens against a surface
   that will shift") has in fact been overtaken: that surface landed
   2026-07-28 (#1715 closed, #1716 landed, #1717 closed, escape set final
   per §8d.6). A review on 2026-08-01 read the bare "stay held" above as
   stale and concluded the hold should be lifted. It should not. This note
   exists so the next reader does not repeat that inference.

   **Also ruled 2026-08-01: take the free part now.** Register `.brink`
   *and* gate `semantic_tokens_full`/`_range` on `db.is_native` in the same
   change — switching on the native diagnostics, hover, go-to-definition,
   rename and cross-file scope that **already work** and that no client
   currently requests. Real native token classification stays held. The two
   halves must ship together: `parse_query` is unconditionally the *ink*
   parser with no dialect gate, so registering alone would light up a live
   bug (the server emitting ink-misclassified tokens over native source),
   latent today only because nothing asks.

## Migration notes — breaking changes for authors

### Escape sequences in inline markup (§8d.6, landed PR #1732)

**Status:** Breaking change, effective immediately on 2026-07-29, landed via
PR #1732 (part of #1716). The ruling is permanent per §8d.6, applied here as
a sanctioned exception to §1's superset doctrine (§8d.6 explicitly sanctions
it).

**What changed:** The **inline** escape set is now **strictly finite**:
`\<` `\{` `\#` `\\` are the only valid inline escape sequences. A backslash
before any other character in inline position — including spaces,
punctuation, emoticons, path separators — is now a compile error. §8d.6 also
rules `\!` and `\@` as **line-start** escapes — a disjoint set, legal as the
first item `content::content_line`/`content_line_else_boundary` scans,
guarding a literal leading `!`/`@` from the sigils those characters carry
there (`@NAME` cue dispatch, §8b.9; the reserved `!name` annotation-element
sigil, §3.5b). That is the true start of a physical line for an ordinary
content line, but also right after a compact cue's `@NAME:` prefix
(`element::cue_line`'s `COMPACT_CUE` arm calls `content_line` directly for
the fused dialogue line, reusing the same entry point). **Implemented, issue
#1744**: `parser::markup::at_line_start_escape`/`line_start_escape`
(`crates/internal/brink-syntax-native/src/parser/markup.rs`), wired into
`content::content_line`/`content_line_else_boundary`. A `\!`/`\@` anywhere
else in a line remains the ordinary "backslash before anything else" error.

**Who is affected:** Authors with existing `.brink` prose files containing a
bare backslash where inline markup grammar applies (inside flow bodies,
inside prose blocks, inside choice text, inside block elements). Common
patterns that now fail:

- Windows paths: `C:\Users\Documents` → compile error
- Emoticons: `\o/` → compile error
- Escaped punctuation outside the four-char set: `\-` or `\.` or `\;` → compile error

**How to fix:** Double the backslash. The doubled backslash becomes a valid
escape sequence `\\`, which renders as a single backslash in output:

```brink
C:\Users\Documents  → C:\\Users\\Documents  (renders as C:\Users\Documents)
\o/                 → \\o/                  (renders as \o/)
\-(dash)            → \\-(dash)             (renders as \-(dash))
```

**Rationale:** The final escape set (§8d.6) is ruled to prevent silent
data loss. A backslash before an unrecognized character was previously
ordinary literal text (and a following `#` or `{` was still parsed as a
tag or interpolation) — a real, ruled superset-doctrine exception (§8d.6
explicitly sanctions it), not a consequence of §1. Making the sequence a
hard error ensures authors notice and fix it correctly, rather than
discovering the surprising split at runtime.
