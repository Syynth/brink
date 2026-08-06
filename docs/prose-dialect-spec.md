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
> **2026-08-03 rename (issue #2180):** the `brink.toml` key this section
> describes was named `elements` until the 2026-08-03 ruling
> (`docs/decision-log.md`) split the annotation surface into `@[convention]`
> (claiming) and `@[element]` (`!name`-dispatched) — the key names a module
> of the *former*, so `elements` no longer matched what it pointed at. It is
> now `[project] conventions`. `brink-project-config` still accepts
> `elements` as a deprecated alias for a deprecation window (it still sets
> the same value, but emits a `ConfigWarning`) rather than hard-breaking
> every existing project's `brink.toml` outright. The rest of this section
> is rewritten in terms of the current name; historical PRs/issues it
> references may still say `elements` in their own text.
- **`brink.toml` holds pointers, not content** — a manifest location,
  a conventions location.
- **Element conventions are project-authored** → a dedicated
  project-side file referenced from `brink.toml`, with built-in presets
  nameable (`conventions = "screenplay"` vs a path). The shipped dialect
  JSON is the precedent, and it already solved a real constraint: it is
  interpreted identically in Rust and TS (the editor needs the
  conventions too).
  > **2026-08-02 caveat:** issue #1874 landed validation of a bare
  > preset-shaped `conventions` value against the closed built-in-preset set
  > (`brink-analyzer::AnalysisOptions::apply_project_config`). Since issue
  > #1720, `"screenplay"` — the exact form this section names as an
  > example — is a recognized name in that set
  > (`BUILTIN_CONVENTION_PRESETS`), but recognition is validation-only:
  > nothing downstream injects the preset's handlers yet (needs #2080/#1840),
  > so a recognized-but-not-yet-injectable name still produces a
  > `ConfigWarning`. A project-relative path pointer is unaffected.
- **Markup vocabulary is host-authored** → it lives in the **host
  capability manifest**, same author as the externals section, and can
  be *generated* from engine code (a text-effect plugin auto-declaring
  its tags), like bindings can generate externals.
- The authorship test that produced this split: co-locate declarations
  only if hand-authored or generated by the same source.
- **Enforcement (issue #1844, `E169`):** when `conventions` names a
  project-relative path, a pattern-claiming `@[convention(claims =
  "…", order = N)]` handler declared in any *other* file is a compile
  error — the module half of the §9.1 confinement asymmetry
  (`docs/decision-log.md` 2026-07-31 item 4). An unset `conventions` key, or
  one naming a built-in preset rather than a path, enforces nothing yet
  (no project file to confine against) — see `E169`'s own doc for the
  exact boundary.

### 3.5 Conventions are authored as a brink module (RULED, sitting 4 addendum)

> ⚠ **SUPERSEDED IN PART, 2026-07-31** — see `docs/decision-log.md`
> "Conventions are annotated handlers: the declarative element surface is
> subsumed by the annotation surface (§9.1 settled)". The module form,
> the `brink.toml` pointer, presets-as-modules, the purity/determinism
> gate, and data-as-generated-interchange all STAND. What changes: the
> well-known entry is `fn conventions()` which **registers handlers in
> order**, not `fn conventions() -> Conventions`; the `Conventions`
> **type does not exist**; and §9.1's owed "types shaped for extension
> ergonomics" is dissolved rather than designed. **Also corrected,
> 2026-08-01 (Q4):** `fn conventions()` is not pure — each `register(...)`
> call is a write to the named conventions-registry cell, so the function
> declares `@[effects(writes(conventions_registry))]`, not
> `@[effects(pure)]`; the earlier purity framing failed its own `E103`
> fence.
**The authored form of project conventions is a `.brink` module** — a
code-ground-only module exporting `fn conventions()`, which declares
`writes(conventions_registry)` (each `register(...)` call writes the
named conventions-registry cell, `docs/decision-log.md`'s 2026-08-01 Q4
ruling; the proc-macro staging rule holds: the module cannot use the
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
- ✅ **The §9.1 design pass is CLOSED — nothing on it is owed.** This bullet
  previously listed five open items; every one has since been settled, and
  leaving the list up caused the maintainer to be asked to re-design already-
  ruled work more than once. Retired 2026-08-02 with each item's disposition:
  - **`std::conventions` types** — **DISSOLVED** by the 2026-07-31 §9.1 ruling,
    in its own words: collapsing the two surfaces "removes the column, the
    chain-rule engine, the `Conventions` type, and §9.1's 'types shaped for
    extension ergonomics' — that last item stops being a data-structure design
    problem and becomes `use` plus a registration call." There is no type to
    design.
  - **The `fn conventions()` well-known-name entry** — **RULED** by §9.1 item 5:
    the well-known name survives and the module *registers handlers in order*
    (statement order is resolution order); the returned `Conventions` type does
    not survive.
  - **The portable-regex subset** — **RULED** (decision-log, dialect-declaration
    entry): "portable-regex core (JS ∩ Rust subset, CI-enforced) with affix
    sugar compiling to it." What remains is marshal-time *validation* with
    module-pointing errors — implementation, not design.
  - **The editor re-evaluation loop** — **RULED 2026-08-02**: the projection is
    cached on the conventions module's import closure; classification runs per
    keystroke against it; comptime is never per-keystroke.
  - **Sequencing** (native construction literals ride the #1103 build; the
    module may be brink-dialect until then) — a sequencing note, never a design
    question.

  The
  `brink.toml` pointer itself (§3.4's `conventions = "conventions.brink"`
  — renamed from `elements` by issue #2180's 2026-08-03 rename, itself a
  correction of a stale `conventions = …` spelling that, at the time,
  never matched §3.4's then-current `elements` key; `brink-project-config`
  still accepts `elements` as a deprecated alias) is **no longer owed for
  parsing**: `brink-project-config` parses `[project] conventions` (and
  its `elements` alias) and `brink-analyzer` carries it onto
  `AnalysisOptions` (issue #1844), which also enforces the confinement
  half of item (4) below (`E169`) for a path-shaped pointer.
  **Resolving it against the dispatch-consuming half is RULED and BUILT**
  (issue #2289, `docs/decision-log.md` 2026-08-05 — correcting the defect
  that survived #1844: confinement restricted *where a handler may be
  declared*, but nothing let it claim prose in any *other* file, so a
  correctly-declared conventions module claimed nothing where it mattered).
  There is no separate comptime-evaluated identity list to join against —
  `order` (issue #2164) already makes precedence a static property of the
  `@[convention]` declaration itself — so `brink_db::queries::analysis::
  external_claim_handlers_query` reads the configured conventions module's
  own declared handlers directly and `hir::lower_native::
  lower_with_conventions` merges them into every OTHER file's dispatch
  table, sorted by `order` together with that file's own local
  declarations. This is a full replacement of the earlier #1863 design
  (deleted by issue #2165 alongside the dissolved `fn conventions()`
  registration it was built for), not a revival of it: that design chained
  an injected handler after every local one because it had no real `order`
  to sort by; this one doesn't. An entirely unset `conventions` key is ALSO
  no longer silent (issue #2289 part 2): a declared `@[convention]` with no
  configured module names no module for the declaration to belong to, so it
  is `E169` too, not an opt-out.

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

- **Sigil + name dispatch (RULED, addendum 3; landed by issue #2004 for
  the plain, non-`block` case).** Annotation elements live behind the `!`
  sigil: `!name args…` — parsed as one `BANG_DISPATCH` node
  (`brink-syntax-native`'s `parser::element::at_bang_dispatch`/
  `bang_dispatch`), composing with `\!`'s line-start escape (§8d.6) by
  construction. The first identifier dispatches **by name** to the
  annotated fn (fn name, or an alias given by `@[element(name =
  "alias")]`); the `args` pattern parses only the remainder, binding
  captures to params (`brink_ir::hir::lower_native::element::
  try_dispatch`) — the same wholly-literal-remainder and
  captures-bind-params-by-name mechanics `claims` dispatch already uses.
  Rationale: without a sigil, a user pattern can silently claim natural
  prose — declared, but *invisible at the use site*; the sigil makes
  every rewritten line self-announcing (the explicit-format posture
  applied to macros). Name dispatch **dissolves the match-ordering
  problem entirely**: at most one handler can ever match a given `!name`
  line's own name, so there is no analogue of `claims`'s "two patterns
  match one line" question. Two *declarations* naming the same dispatch
  name is a distinct question, and is only an interim first-declared-wins
  today (`Elements::dispatch`'s own doc) — the ruled "duplicate names are
  ordinary duplicate-definition errors" is not yet a diagnostic; nor is
  the ruled "an unmatched remainder is a targeted diagnostic naming both
  the line and the handler's pattern" — an unmatched-name or
  unmatched-remainder `!name` line falls to the generic `E129`
  ("parses cleanly but has no HIR lowering yet") rather than a diagnostic
  that names the line and pattern, since no diagnostic code is
  pre-assigned for either yet (both remain open follow-up work). The
  target is a **top-level `fn` only**, matching `claims`'s own
  restriction (the rewrite is an expression call) — a `flow`'s own `args`
  clause still parses and validates but is not yet a live dispatch
  target. Dispatch is **file-local**, matching `claims`'s pre-#1863 scope
  (no cross-file dispatch-name resolution yet). **A `block`-declared
  handler's trailing receiver now dispatches for real (issue #1839)** —
  see the `block` capture bullet below. (Fountain's `!`-forces-plain-action
  inversion noted and accepted — "good point of reference, not
  married to it.")
- **Pattern power proportional to auditability.** Natural-notation
  pattern claiming (lines that don't announce themselves — `INT.`
  headings, `@` cues) is *confined*, not banned: the declarative side it
  was originally reserved for is **dissolved** (`docs/decision-log.md`
  2026-07-31), so a claim is now an ordinary annotated handler — spelled
  `@[convention(claims = "…", order = N)]` since issue #2164's 2026-08-03
  split of the annotation surface (formerly `@[element(claims = "…")]`,
  issue #1838) — still one centralized, auditable place, now readable
  source rather than an interpreted table. **Landed by issue #1838**: a
  claimed line is matched, its named captures bind the
  handler's params by name (checked in *both* directions — `E160` for a
  capture with no param, `E167` for a param with no capture, since every
  argument of the rewrite comes from a capture), and the line lowers to
  exactly one call. Only a **top-level `fn`** may claim (the rewrite is an
  expression call; `E112` otherwise — including a `fn` declared inside a
  `module { … }` block, issue #1847: it reads as un-nested by
  `flow`/`fn`-depth alone, but the dispatch table only ever scans the
  file's direct declarations, so admitting it would silently register
  nothing to claim with), only a wholly literal prose line, scene
  heading, a cue's `CUE_NAME`, or a chain-gated `PARENTHETICAL`'s
  delivery text is a candidate (issue #1720 widened the set to the
  latter two), and a claiming handler never claims inside its
  own body (§3.5's staging rule). Confinement to the
  `brink.toml`-named conventions module IS enforced (`E169`, issue #1844) —
  a claiming `fn` declared outside that one file is an error, and (issue
  #2289, `docs/decision-log.md` 2026-08-05) so is a `@[convention]` with no
  `conventions` key configured at all, since there is then no module for it
  to belong to. Claiming reach is project-WIDE, not file-local: the
  configured module's declared handlers claim prose in every file of the
  project, not merely the one that declares them — see item (4)'s own
  ruling text above and `hir::lower_native::element`'s module doc ("Cross-
  file claiming reach") for the mechanism.
- **Dispatch order (RULED, issue #2164, `docs/decision-log.md`
  2026-08-03).** When a module declares more than one claiming handler
  and more than one could match the same line, the lower-`order` one
  wins: `@[convention(claims = "…", order = N)]`'s `order` is a
  **required** bare integer, and the walk tries handlers in ascending
  `order`, first-match-wins. This retires the interim issue #1848 rule
  (top-level declaration order) and the `fn conventions()` registration
  idea it was standing in for — that mechanism was **dissolved**
  entirely (`docs/decision-log.md` 2026-08-03, "`fn conventions()` is
  DISSOLVED"), not merely superseded, so there is no comptime
  registration step to land. `order` has **no default** (`E178` if
  absent) and **no tie-break** (`E179`, naming both declarations, if two
  handlers in one module share a value) — precedence is total, explicit,
  and authored on the declaration itself, never inferred from position.
  Two claiming patterns that can
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
  `@[convention(claims = "…", order = N)]` handler's captured param declaring a numeric/struct/
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
- **`block` capture (RULED, 2026-07-31 sitting; DELIVERED, issue #1839).**
  `@[element(args = "…", block)]` / `@[convention(claims = "…", order =
  N, block)]` (the latter renamed from `@[element(claims = "…", block)]`
  by issue #2164) capture the run **following** the matched line into a trailing
  `content`-typed param, terminated by a blank line or any element-level
  line (any body item other than a plain `CONTENT_LINE`) — the handler
  WRAPS the captured run rather than tagging it. `E166`, the declaration
  surface's own static check, adds two implementation-level requirements
  the ruling's own wording does not state — recorded here so they live
  somewhere other than a private helper's doc comment: the qualifying
  `content`-typed parameter must be the declaration's **last** parameter,
  and its name must not collide with one of `args`'/`claims`' own named
  capture groups (a capture and the block receiver cannot be the same
  param) —
  `crates/internal/brink-ir/src/hir/lower_native/annotation.rs`'s
  `has_block_content_param`.
  The dispatch mechanism itself (`hir::lower_native::element::
  capture_block`, both `try_claim` and `try_dispatch`) builds the internal
  `hir::Expr::Fragment`/`lir::Expr::Fragment` node the 2026-08-01
  "Content-as-value" ruling adds (`docs/decision-log.md`) — the same
  `BeginFragment`…`EndFragment` → `Value::FragmentRef` machinery an
  ordinary call's display-position composition already uses
  (`brink-codegen-inkb::content::emit_slot_expr`), widened to hold an
  arbitrary captured statement run rather than one call's output.
  Interior lines lower through the ordinary `body::lower_items` dispatch
  loop, so a handler that would claim one of them still claims it — no
  special case, and each interior line still reaches its own line-table
  entry (`tests/tier1-native/annotations-element-block/`). Carried across
  the cross-file conventions-module injection join (issue #1863) since
  issue #2068: `ClaimHandlerDecl`, `ClaimHandlerCandidate`, and
  `ExternalClaimHandler` each thread a `block: bool` through the three
  join hops (`Elements::handler_decls()` →
  `conventions_registry::candidate_claim_handlers` →
  `conventions_registry::join_conventions_registry` →
  `element::collect`'s external branch), so an injected handler declared
  `block` in the project's conventions module still captures its block
  receiver when dispatched from another file
  (`hir::lower_native::element::ClaimHandler::block`'s own doc).
  **Empty-capture rendering (RULED, issue #2091):** a `block` receiver
  whose captured run is empty (e.g. a cue immediately followed by a
  parenthetical, terminating the capture at zero interior lines) still
  binds a real, present `Value::FragmentRef` to the `content`-typed
  param — but interpolating it alone on a template line now renders
  **nothing**, not a blank output line: `brink-runtime::output`'s
  `resolve_lines`/`take_first_line` suppress a resolved line when its
  text is empty, it carries no tags, and one of its parts interpolated a
  `content`-typed value that itself rendered empty. This is a read-time
  rendering decision only — the compiled line-table entry stays
  present-but-empty, so locale hot-swap is unaffected. The same
  suppression also fires for the ordinary display-position
  call-composition `FragmentRef` `emit_slot_expr` produces for any
  template slot whose expr is a function call (e.g. `{ f() }` where `f`
  returns empty and emits nothing) — the discriminator is structural
  (any `FragmentRef`-driven emptiness) and does not distinguish a
  `block` receiver from that case. **Extended to the string-capture and
  fragment-interior paths (issue #2147):** the fix above landed only in
  `resolve_lines`/`take_first_line`; `brink-runtime::output::resolve_parts`
  — `OutputBuffer::end_capture`'s resolution path (`Opcode::EndStringEval`,
  e.g. an unrecognized choice display or any `~ temp x = "..."`
  string-eval capture) and `OutputBuffer::resolve_fragment` (the resolver
  `ChoiceDisplay::Fragment` reads through) — did not suppress the same
  case until #2147 applied the identical per-line invariant there too,
  including when resolving a *nested* fragment's own multi-line interior.
  **Deliberately excluded:** a line
  that resolves empty for any other reason — a literal blank line, or a
  self-closing inline markup span (`<pause/>`) with no children — keeps
  its existing blank-beat behavior (`inline-markup-point-marker`
  fixture, issue #1716), a separate, already-settled question this rule
  does not touch.
- **`attach = StructName` — the declared attachment schema (RULED,
  `docs/decision-log.md` 2026-08-03 "The element output model"; issue
  #2178, split from #2164's item 2).** An optional clause on
  `@[convention(claims = "…", order = N)]` naming a declared `struct` —
  the schema of keys and types the handler attaches to the run that
  follows. Governing split: **declared** (this clause: which keys, what
  types — static, editor-readable, cacheable) vs. **computed** (the
  handler body: the actual values). Deliberately not a new declarative
  sub-language — a `struct` is already declarative, statically known,
  serialized, and understood by compiler + editor + host, so reusing it
  is the whole point. A handler can never attach a *computed key name*:
  nothing about a plain `struct` return type lets it invent one, since
  the struct's own declaration fixes the field names once and for all.
  Checked at the declaration (`E180`): the handler's own return-type
  annotation must name the same struct `attach` does, the same
  declaration-surface-only posture `E166`/`E171` already take for
  `block`/captured-parameter checks — real name resolution (does a
  struct of that name actually exist) is deliberately out of scope here.
  `ClaimHandlerDecl::attach` carries the schema name onward — the field a
  future NS-T projection (#2111, blocked on this landing) reads to
  surface a handler's declared output schema to the editor/host.
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
- **Tooling transparency (no invisible expansion)** — half-landed by
  #1838: `brink_ir::HirFile::element_matches` records every claimed line
  as `(line range, matched kind, handler name + its declaration range,
  the claiming annotation's range, captures as spans, disposition)`.
  `LineContext` carries the matched handler (fn + source location) and
  capture bindings as spans (now with their style hooks); hover shows
  the handler's signature and body. **The explain-match query is built**
  (issue #2113, shipped by PR #2309): `brink_ir::explain_match`/
  `ExplainMatchCache` compose #2112's classification walk and #2111's
  projection into is/isn't-matched + what bound (patterns attempted on a
  miss, other matches shadowed on a hit), reading this record rather
  than re-running the match — exposed to the wasm editor surface as
  `EditorSession::explain_match`/`explain_match_doc`. This is not a
  consumer "riding a held editor track": per the ruling recorded at
  item 6 below, this compiler-side query family was never covered by
  the 2026-08-01 hold in the first place, and the hold itself is lifted
  as of 2026-08-05 regardless.
- **Deferred**: numeric capture coercion (issue #1849 added `E171`, a
  declaration-time diagnostic for a `claims` handler's non-`string`
  captured param, so the gap is loud rather than silent — the coercion
  itself, and binding a non-`string` capture to a real typed value,
  remain unbuilt); context injection (handlers
  reading attachment data); `Option` params for optional captures;
  binding an ORDINARY (non-`block`) `content`-typed param to a genuine
  captured `FragmentRef` — `try_claim`/`try_dispatch` still bind every
  named-capture-group value as a plain `Expr::String` literal regardless
  of the receiving parameter's declared type (the ruled `fn radio(chan:
  string, text: content)` example's `text` param binds this way), so a
  `string` literal is accepted where `content` is declared and the
  captured span is not yet translation-resident through *this* path —
  **the `block` clause's own trailing receiver is the one exception,
  landed by issue #1839** (see the `block` capture bullet above): this
  narrower, ordinary-capture gap is issue #1912's own remaining scope,
  not #1839's;
  the ruled duplicate-dispatch-name error and the ruled targeted
  unmatched-remainder diagnostic (issue #2004 dispatches the plain case
  but leaves both of these as an interim first-declared-wins and a
  generic `E129` fallback respectively — see the "Sigil + name dispatch"
  bullet above; no diagnostic code is pre-assigned for either yet);
  cross-file dispatch-name resolution (v1 validates one declaration at a
  time — and #2004's `!name` dispatch is file-local for the same reason
  `claims` is); dispatching to a `flow` target rather than a top-level
  `fn` (`!name`'s own placement is legal on a `flow` too, but nothing
  scans a `flow`'s declaration into the dispatch table yet); `fn
  conventions()` registration + comptime (issue #1840 — RULED
  2026-08-01/2026-08-02, sized in `docs/conventions-comptime-sizing.md`,
  **then DISSOLVED 2026-08-03 and DELETED, issue #2165**: `docs/
  decision-log.md`'s "`fn conventions()` is DISSOLVED — handler precedence
  is a property of the `@[element]` annotation" entry found that the
  entire mechanism's information content was one ordering, and an
  ordering is expressible declaratively on the annotation itself —
  `@[convention(…, order = N)]` (issue #2164) — with no comptime evaluator,
  no registry cell, and no compiler→runtime dependency needed at all.
  `register` and `DefinitionId::CONVENTIONS_REGISTRY_CELL` are deleted;
  `register_intrinsic.rs` no longer exists. `E175` is retired in place
  (`docs/diagnostics/E175.md`), never reused, never raised); **multi-token
  style values** — issue #1719's `@[style(key = "value")]` value is a single
  presentation token today (one `StyleToken` per key), not the
  space-separated list this section's own screenplay preset describes
  (`[right, uppercase]`, `[uppercase]`); a `key = "a b"` value lowers to
  one `StyleToken::Custom("a b")` rather than two tokens.
- **Staging**: v1 = built-in screenplay preset + `!`-dispatched
  annotations (zero comptime); the §3.5 conventions-module evaluation
  arrives later for authoring full custom presets. **The preset itself
  landed, issue #1720** (`std/conventions/screenplay.brink`), and **#2080
  (ruled 2026-08-03) mounts its source into every compiled project's
  `Environment` manifest** — the same string-keyed home every project
  source lives in (no bespoke `std::`-namespace resolution mechanism; see
  `crates/internal/brink-environment`). What's still missing before a
  project can `use` an item out of it: nothing in the mounted module is
  marked `pub` and no confinement rule scopes what a project's `use` may
  reach (#1582's pub marker, #2167's closure-scoped confinement). The
  preset's handlers are proven end to end only via a project
  that inlines the same declarations directly (`tests/tier1-native/
  conventions-screenplay-preset/`) — single-file `claims`/`block`
  dispatch, exactly what #1838/#1839 shipped.

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
- **Hyphenated tag names** (RULED 2026-08-01, issue #1996, superseding
  #1740): a tag name may contain `-`, e.g. `<fade-in>` — kebab-case is
  the common convention for custom-element vocabularies a host manifest
  might expose, and the markup layer is freeform by default (§4.2), so
  there was no reason to exclude it. The hyphen is legal **only as an
  internal separator between two name segments** — never leading, never
  trailing: `<fade-in>` ✓, `<-x>` ✗, `<x->` ✗. A leading hyphen is
  largely forced by the existing grammar rather than a new rule: `<-` is
  already `THREAD` (splice) at the lexer, so an open tag's name can never
  start with one in the first place; a trailing hyphen is representable
  (a lone `-` lexes as `MINUS`) but is a **parse error**, not silently
  folded into the name. This widens the tag-name shape at **span-tag
  position only** (`brink-syntax-native::parser::markup`) — plain `IDENT`
  lexing is unchanged everywhere else in the language. A hyphen-separated
  segment may be spelled the same as a reserved keyword (`in`, `for`, …
  — `<fade-in>` itself is the motivating case: `in` is `KW_IN` in
  expression position) since a tag name is prose vocabulary, not code;
  this is narrower than "a tag may be *named* a bare keyword" — the
  tag's own opening segment is unaffected and still cannot be a bare
  keyword spelling, hyphenated or not.

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

**Required attributes + widened attribute schema (RULED 2026-08-01, issue
#1997, closing #1780).** Two gaps issue #1780 found in the schema above:
`attrs` was an *allow*-list only (a declared-but-missing attribute was
never diagnosed), and `attrs` being a bare `Vec<String>` had no room to
grow into typed attribute values without a breaking manifest change.
Issue #1997 ruled **both halves adopted**:

- **(a) Required attributes.** Each declared attribute
  (`brink_ir::ManifestSpanKind::attrs`, now `Vec<ManifestSpanAttr>`) carries
  a `required: bool`, `false` by default. A span of a declared kind that
  omits one of that kind's `required` attributes reports `E173`, gated the
  same way `E164`/`E165` are (only for a span whose name is declared, one
  report per missing attribute) and defaulting to `Warning` for the same
  `[lints]`/`@[allow(…)]`-configurability reason as `E164`/`E165`.
  Implemented in `brink_analyzer::markup_check`, alongside the existing
  checks.
- **(b) Widened attribute schema — headroom, not typing.**
  `ManifestSpanKind.attrs` moved from `Vec<String>` to
  `Vec<ManifestSpanAttr>` (`{ name, required }`, plus a reserved,
  currently-inert `ty` slot) **specifically so that adding attribute-value
  typing later needs a new check, not another manifest schema break** —
  `TypeRef` is already `#[serde(transparent)]`, so `ty`'s wire form is
  already the plain-string shape a future typed value would use.
  **⚠ This is schema headroom only. Issue #1997 does NOT implement
  attribute-value typing** — span attribute values stay static text by
  construction (`SyntaxKind::SPAN_ATTR_VALUE`), exactly as this section
  already said above; `ty` is deserialized and round-tripped but read by
  no check. A reader seeing `ManifestSpanAttr::ty` in the source should
  not conclude typing shipped.

This is a wire-format change to the `markup` section itself (a bare
attribute-name array is no longer accepted at the type level — hosts must
migrate to `[{ "name": "…" }, …]`), observable through `@brink-lang/web`.
See `docs/host-capability-manifest.md` § "Markup vocabulary" for the
updated wire shape and migration note.

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

### 4.6 Escape/markup layer coverage audit across prose scanners (#1738)

Filed from a wave retro on #1716/PR #1732: that PR landed the final inline
escape set (§8d.6) and the markup/span grammar, but only for the scanners
that reuse the shared `content::content_items_until_impl` engine. A handful
of *other* free-text scanners exist in the native parser
(`brink-syntax-native`) that never call into that engine at all — issue
#1738 asked for an explicit, durable audit of exactly which scanner honors
which piece of §4/§8d.6, since "does `\#` work here" had never been checked
scanner-by-scanner before. The table below is that audit; keep it current
when any scanner's escape/markup handling changes.

| Scanner | Where | 4-char inline escapes `\< \{ \# \\` (§8d.6) | Line-start escapes `\! \@` (§8d.6, #1744) | Markup spans `<ident>…</ident>` (§4) | Status |
|---|---|---|---|---|---|
| `content::content_items_until_impl` (the shared engine: ordinary content lines, choice-text anatomy, inline alternation bodies) | `content.rs` | **Full** — `BACKSLASH` dispatches to `markup::escape`, which recognizes exactly the four and errors on anything else | N/A (line-start-only; see the `content_line` row) | **Full** — `LT` dispatches to `markup::span` when `at_span_open`/`at_span_close` holds | Reference implementation — this is "the escape/markup layer" the other rows are compared against |
| `content::text_run_until` (the engine's own `TEXT`-run sub-scanner) | `content.rs` | N/A directly — it never sees a `BACKSLASH`/`LT`/`HASH`/`GLUE`/`DIVERT`/doc-comment token itself; it unconditionally **breaks** on each so the outer loop above can dispatch them structurally | N/A | N/A (same reason) | Structural only, by design — not a gap; folding these into `TEXT` would silently defeat the outer loop's own dispatch (its own doc comment says so) |
| `content_line` / `content_line_else_boundary` (a full `CONTENT_LINE`, including the fused remainder a `COMPACT_CUE`/`BANG_DISPATCH` reuses it for) | `content.rs` | **Full**, via the shared engine above | **Full** — the one and only call site of `markup::at_line_start_escape`/`line_start_escape`, checked once as the first item scanned | **Full**, via the shared engine above | Reference implementation |
| `markup::span` (a span's own body, recursing back into the shared engine with an `expected_close`) | `markup.rs` | **Full** — inherited "for free" by construction, not reimplemented: `span` calls `content::content_items_until_impl` for its body with the *same* `stop` its caller had | N/A (spans never open at a physical line-start; the sigil collision §1744 guards against doesn't arise mid-span) | **Full** — nested spans recurse the same way | Reference implementation, by inheritance |
| `content::tag` (a `#tag`'s own free-text body — `tag_line_tail`'s per-tag call, and `header_tag_tail`'s header-line variant) | `content.rs` | **Full, as of #2045.** The *parser's* backslash-parity tracking is unchanged from #1738/#1852 (`\{`/`\\{` depth-parity; `\#` suppresses the tag-terminating role) and the raw `TAG` CST node stays a lossless, unstripped copy of the source — there is still no `ESCAPE` sub-node here to strip at parse time. What changed: `ast::Tag::text()` (`ast/nodes.rs`, new) is now the one materialization point every consumer goes through, and it strips a *recognized* escape's backslash there, parity with `markup::escape`. `hir::lower_native::body::lower_tag` was simplified to delegate to it instead of hand-rolling the same HASH-skip + concatenation. `\<`/`\\` get the identical stripping (the full four-member set, uniformly) even though neither has a structural role here — see the Markup column | **Not applicable** — a tag body starts after a `HASH`, not at content-line start; the `@`/`!` sigil collision `\!`/`\@` exist to guard against cannot arise here | **None, ruled** (#1783, RULED 2026-08-01): `<glitch>` inside a `#` tag is intentionally literal text forever — "no spans in tags, ever." Not touched by #2045 | Fixed by #2045 — see `a_tags_text_accessor_strips_a_recognized_escapes_backslash` and its `\{` sibling |
| `element::cue_name` (an `@NAME` cue's own free-text name, before `:`/tags/newline) | `element.rs` | **Full, as of #2045** — identical shape to `tag()` above: the parser's own depth/parity tracking (#1738/#1852/#1851) is untouched, and `ast::CueName::text()` now strips a recognized escape's backslash the same way `ast::Tag::text()` does. **A real lowering consumer today (issue #1720):** `hir::lower_native::element::try_claim`'s natural-notation candidate matching reads a `CUE`'s single `CUE_NAME` child's text as a regex-match input — but via `text_node.text()` on the raw `SyntaxNode` directly, **not** through `ast::CueName::text()`, the same divergence `scene_title`'s row below documents and for the same reason (capture-offset provenance, #1838): a claimed `@VEN\#DOR` line passes `VEN\#DOR` — backslash intact — into the handler call, while `CueName::text()` would have stripped it to `VEN#DOR`. So a cue name's *pattern-matching and call-argument* text still carries an unstripped backslash; only a *display*-oriented reader going through `CueName::text()` sees it stripped. This is real, tested, and runtime-observable through the compiler as of #1720 — not gated on #1717 the way it was before that PR landed | **Not applicable** — same reasoning as `tag()` | **None** — by the same reasoning #1783 rules for `tag()` (no active span grammar runs inside a cue name); not separately ruled by number, but no code path here ever calls `markup::span` either | Fixed by #2045 — see `a_cue_names_text_accessor_strips_a_recognized_open_brace_escapes_backslash` (the `\#` case: the pre-existing `a_cue_name_with_an_escaped_hash_does_not_end_the_name_early` test, updated) |
| `element::scene_title` (a scene heading's display name, before the optional `[slug]`/tags/newline — §3.3/§8b.3) | `element.rs` | **Full, as of #2045** — the parser's own `HASH`-parity carve-out (#1738) is untouched, and `ast::SceneTitle::text()` now strips a recognized escape's backslash the same way. **Three real, if narrow, lowering consumers today (as of #1720, up from one):** `hir::lower_native::element::try_claim`/`try_dispatch`'s natural-notation candidate matching reads a `SCENE_HEADING`'s `SCENE_TITLE` child, a `CUE`'s `CUE_NAME` child (see the `cue_name` row above), and a `PARENTHETICAL`'s own text run, all as a regex-match input — but via `text_node.text()` on the raw `SyntaxNode` directly, **not** through `ast::SceneTitle::text()`, and deliberately left that way by this fix: that raw text's byte offsets are load-bearing for mapping a capture group back to a real source range (`base = text_node.text_range().start() + lead`), and running the stripped, byte-shifted string through that offset math would risk silently corrupting capture provenance for a delicate, unrelated feature (#1838). So a title's *pattern-matching* input still sees an unstripped backslash; only its *display* text (`SceneTitle::text()`) is stripped. Flagged here rather than silently decided — a real, if narrow, residual divergence between two readers of the same node, orthogonal to what #2045 was asked to resolve | **Not applicable** — a scene title starts at the heading's own line-start pattern (`INT.`/`EXT.`), not at content-line start; the `\!`/`\@` sigils this column tracks don't arise here | **None** — same reasoning as `tag()`/`cue_name()`: no code path here ever calls `markup::span`, and headings get no markup carve-out (this module's own doc comment: "a `{` on a heading line is just title text") | Fixed by #2045 — see `a_scene_titles_text_accessor_strips_a_recognized_open_brace_escapes_backslash` (the `\#` case: the pre-existing `a_scene_title_with_an_escaped_hash_does_not_end_the_title_early` test, updated) |
| `element::parenthetical` (a `(hushed)` delivery line's literal text, between the parens) | `element.rs` | **None, and not a bug** — a free-text raw scan with zero escape treatment of any kind. Confirmed no `\#`-shaped defect: `(hushed \# quiet)` stays one `PARENTHETICAL` (it stops only on `EOF`/`NEWLINE`, and paren depth for `(`/`)` — `HASH` plays no terminating role here at all, unlike `tag()`/`cue_name()`/`scene_title`, so there was never a `\#` boundary to escape) | **Not applicable** — a parenthetical starts mid-line, after a cue, never at content-line start | **None** — free text only, same reasoning as the other raw scanners above; not separately ruled by number | Included for the audit's own completeness claim ("enumerate every prose scanner"); no fix needed |

**What this PR changed:** `#` is one of the ruled four-character inline
escape set, but `tag()`/`cue_name()` gave it **zero** escape treatment
before this fix — an unescaped-*or*-escaped `#` both ended the scan
identically (splitting `#tag \#more` into two sibling `TAG`s, the second one
starting mid-word at the escaped hash). That is the one clear,
unambiguous bug this audit found: `{`/`\{` already had backslash-parity
awareness in both functions (#1852), but `#` — a member of the *same* ruled
set — had none, an internal inconsistency between two characters of one
set inside the very same two functions, not a question of whether to widen
scope. Fixed by extending the existing `backslash_count` parity mechanism
to `#`'s tag/name-terminating role, mirroring the established
non-stripping `\{` precedent exactly (the backslash stays in the emitted
text) rather than introducing new "strip the backslash" semantics these
two raw-text scanners have never had. `hir::lower_native::body::lower_tag`
needed a paired fix — it used to skip *every* `HASH` token in the node
(safe only because an interior one was structurally impossible before this
fix); now it skips only the tag's own leading delimiter.

**Superseded by the #2045 ruling below:** both the "non-stripping `\{`
precedent" description and the "`lower_tag` … now it skips only the tag's
own leading delimiter" description above are historical — accurate for
#1738/#1852's own PR, not for the current code. #2045 replaced that
`lower_tag` body entirely with a delegation to `ast::Tag::text()`, which
now strips a recognized escape's backslash; see the ruling immediately
below.

**Found and fixed during this PR's own review:** the audit above initially
missed `element::scene_title`, which turned out to share the *exact same*
pre-fix defect as `tag()`/`cue_name()` — an unconditional `HASH` stop with
no backslash awareness. Fixed the same way, with a paired parser test
(`a_scene_title_with_an_escaped_hash_does_not_end_the_title_early`); see
the table row above. `element::parenthetical` was also added to the table
for the audit's own completeness claim — confirmed to have no `\#`-shaped
defect (it never treats `HASH` as a terminator at all), so no fix was
needed there, just the missing row.

**What is still open, deliberately not decided here:** whether `tag()`/
`cue_name()` should eventually run the *rest* of §8d.6 — i.e. whether an
unrecognized backslash sequence (`\x`, a bare trailing `\`) should become a
compile error inside these two scanners the way it already is everywhere
`markup::escape` runs, instead of today's fully-permissive "any backslash
sequence is literal text, never an error." That would be a real breaking
change of the same shape PR #1732 made for ordinary content (existing
`.brink` tag bodies containing an incidental backslash would start failing
to compile) and deserves its own design ruling, not a silent decision
folded into a low-severity consistency audit. Filed as a follow-up,
**issue #2040**, rather than decided unilaterally here.

**RULED (#2045, 2026-08-02): a *recognized* escape strips its backslash in
`tag()`/`cue_name()`/`scene_title()` text too — full parity with
`markup::escape`.** Before this ruling, a *recognized* escape's backslash
was stripped in ordinary content but **retained** in these three scanners'
materialized text — the exact inconsistency #1738's own filing body used
as its motivating example:

```
Hello \# world #a \#b
```

The content-line text became `Hello # world` (backslash stripped, via
`markup::escape`), but the trailing tag's text stayed `a \#b` (backslash
retained, via `tag()`'s raw scan) — same output line, two different
escape semantics for the same `\#`. Ruled toward consistency over the
`\{`-established precedent's inertia: an author moving between ordinary
content and a tag/cue-name/scene title should not need to remember that
the same four-character escape set means "the backslash disappears" in
one and "the backslash survives, only the boundary role is suppressed" in
the other — one escape set, one reading, everywhere it is recognized. The
*parser's* structural recognition (an escaped `#`/`{` doesn't end the
scan, #1738/#1852) is completely unchanged — only the *materialized text*
these scanners' `ast::Tag::text()`/`ast::CueName::text()`/
`ast::SceneTitle::text()` accessors hand back now strips the one escaping
backslash, mirroring `markup::escape`'s own reading of the same
even/odd-backslash-run parity (`ast/nodes.rs`'s
`strip_recognized_escape_backslashes`, shared by all three). This is a
breaking change for any `.brink` file relying on the backslash surviving
into rendered tag/cue-name/scene-title text — see the changeset. Neither
#2040 (should an *unrecognized* sequence become a compile error — still
open, not decided here) nor #1883 (below) is touched by this ruling.

**Relationship to #1883** ("residual escape/depth asymmetries in
`tag()`/`cue_name()` after #1852/#1851", resolved — see §4.7b): that issue
was about a *different* axis — whether `HASH` should become depth-aware
like `COLON` (item 1) and whether `\}` should gain the same
backslash-parity treatment `\{` already has (item 2) — both about the
parser's *structural* recognition, not the *materialized-text* stripping
#2045 rules on. #2045 touched neither item, changed no parsing/depth/parity
decision, and was not a step toward resolving either one — it did not
narrow #1883, same as #1738's original `\#` fix didn't. §4.7b confirms
both items resolve to "the current asymmetry is correct, given the ruled
escape set" — no code change, no narrowing of #2045's own scope either.

### 4.7 Tag raw-text scan: brace-balancing and per-tag scope (#1728, #1787)

Filed from issue #1814: `content::tag()`'s raw-text scan and brace-depth
counter went through two rulings (#1728/PR #1777, then #1787/PR #1807)
that were documented inline on `tag()` itself but never given a spec-level
home — this subsection is that home. It sits alongside §4.6 above, which
covers the *same function's* escape-set handling; this section covers a
different, orthogonal axis: not what a tag's escaped characters mean, but
how far its raw scan runs before deciding the tag is over.

**The mechanism.** A `#tag`'s body (`tag()` in `content.rs`, called by
`tag_line_tail` for trailing tags and `header_tag_tail` for a header line's
own tags) is scanned as raw text — it never re-parses a `{…}` it meets as a
real interpolation/alternation/choice node (§4.6's table calls the sibling
engine's `TEXT`-run scanner "structural only, by design"; the same is true
here, one level up). The scan stops unconditionally at `NEWLINE`/`EOF` or at
any of `tag()`'s caller-supplied `extra_stop` kinds — checked first, before
any depth logic runs. This is where the two callers diverge: `tag_line_tail`
passes an empty `extra_stop`, so a *content* line's trailing tag balances
`{`/`}` as described below; `header_tag_tail` passes `&[L_BRACE, TILDE, GT]`,
so a *declaration header* line's own tags stop at the very first unescaped
`{`/`~`/`>` — the body opener — before the depth counter ever gets a chance
to engage (this is the entire reason the two entry points are kept
separate, per `header_tag_tail`'s own doc comment, and is pinned by
`header_tags_do_not_swallow_a_body_dialect_selector`). For a content line's
trailing tags, the scan additionally stops at an unescaped `HASH` (a new
sibling tag) or at an `R_BRACE` — but only once a `depth` counter, bumped on
every literal, unescaped `{` and brought back down on every `}`, has
returned to zero. This is pure raw-character balancing, not
interpolation awareness: `#tag {gold` (never closed) still runs to end of
line exactly as it did before #1728.

**The tradeoff (RULED, review of #1728, PR #1777, merged 2026-07-29 — "not
'no regression'").** A depth counter over raw text with no real grammar
cannot distinguish "genuinely unbalanced" from "matches something later."
Fixing the original bug means a *balanced* brace inside a tag no longer
ends it early:

```brink
flow f() {
  Hello #tag {gold} coins.
  The river bends.
}
```

now parses as one tag and two content lines, where it used to stop the tag
at the first `}` and misparse the rest. The mirror-image cost: a genuinely
*unbalanced*, unescaped `{` left open inside a tag now eats the enclosing
block's own same-line `}` closer instead of stopping there —

```brink
flow f() { Hello #tag { }
```

fails to parse, because the tag's `{` is counted and the very next `}` is
consumed as its match, leaving the flow body with no closer before
`NEWLINE`/`EOF`. This is accepted as inherent to depth-based balancing over
raw text, not a regression — pinned by
`an_unbalanced_open_brace_in_a_tag_eats_the_enclosing_blocks_own_closer`.
The depth counter uses odd/even **backslash-parity**, not a flat exclusion:
an `L_BRACE` is only skipped when it is preceded by an *odd* run of raw
`BACKSLASH`es (the last one escapes it — an escaped brace is text, not a
metacharacter, per #1716/PR #1732's literal-brace escape), while an *even*
run means the backslashes escape each other and the brace is still counted
(`\\{` counts the brace, per #1852) — `content.rs`'s
`L_BRACE if backslash_count & 1 == 0 => depth += 1`. Pinned by
`a_tag_with_an_escaped_open_brace_does_not_swallow_the_enclosing_blocks_own_closer`
(the odd case) and
`a_tag_with_an_escaped_backslash_before_a_real_brace_counts_the_brace` (the
even case).

**Scope (RULED, review of #1777, issue #1787, delivered by PR #1807,
merged 2026-07-31): `depth` is scoped per-tag, not per-line, and that is
the intended contract, not a gap.** `tag()` runs fresh for each `HASH` a
content line sees, so `depth` always restarts at zero for a new tag — an
earlier sibling tag's in-progress, unbalanced depth is simply discarded at
the `HASH` boundary, never carried into the next tag's scan. For example:

```brink
flow f() { Hello #a {x #b}
```

parses with zero errors: tag `a`'s scan is cut short by the (unescaped)
`HASH` that starts `b` — before the depth check is ever consulted, exactly
like `NEWLINE`/`EOF` — so `a`'s in-progress depth of 1 (from its own
unmatched `{`) is discarded rather than carried into `b`. `b` starts fresh
at depth zero and immediately meets the `}`, stopping there without
consuming it, leaving that brace for the flow body's own closer. The
alternative — a per-line scope that carries depth across the `HASH`
boundary — would let one tag's own unbalanced brace reach *through* a
syntactically distinct sibling tag and swallow that sibling's, or the
enclosing block's, own closer: a strictly worse and less local failure
than the already-accepted per-tag tradeoff above. A `HASH` is a real,
tokenized boundary (each one starts its own `TAG` node), unlike the raw,
grammar-blind `{`/`}` this scan balances, so treating it as anything other
than a hard reset would blur a structural boundary the CST already treats
as absolute. Pinned by
`a_tags_own_unbalanced_brace_does_not_leak_depth_into_a_sibling_tag`.

### 4.7a `element::cue_name()` shares the identical tradeoff (wave-111 retro, #1883)

`element::cue_name()`'s own raw-text scan carries the *identical,
equally unspec'd* brace-balancing tradeoff §4.7 gives a home for `tag()`
above — flagged by a wave-111 review of PR #2053 (#1814) as a gap in that
PR's own scope (§4.7 was written tag-only) rather than filed as a new
issue, since #1883 already owns the tag()/cue_name() residual-asymmetry
surface. The mechanism, the tradeoff, and the per-name scope are all the
same reasoning as `tag()`'s, applied to `element::cue_name()` in
`element.rs` instead of `content.rs`:

- **The tradeoff** is pinned by
  `an_unbalanced_open_brace_in_a_cue_name_eats_the_enclosing_blocks_own_closer`
  — the exact mirror of `tag()`'s own
  `an_unbalanced_open_brace_in_a_tag_eats_the_enclosing_blocks_own_closer`
  pinned above: `flow f() { @NAME { }` fails to parse for the same reason
  `flow f() { Hello #tag { }` does.
- **Backslash-parity** for `\{`/`\\{` is pinned by
  `a_cue_name_with_an_escaped_open_brace_does_not_swallow_the_enclosing_blocks_own_closer`
  and
  `a_cue_name_with_an_escaped_backslash_before_a_real_brace_counts_the_brace`
  — the same odd/even `backslash_count` parity as `tag()`'s.
- **Scope is per-name, not per-line**, the same way `tag()`'s is per-tag:
  `cue_line` calls `cue_name()` once per `@NAME`, so there is no sibling
  boundary within a single cue the way `tag_line_tail` has across several
  trailing tags — the per-tag-scope ruling's *reasoning* (a real,
  tokenized boundary resets local, raw-text bookkeeping) still applies,
  it just has no sibling-`HASH`-mid-scan case to exercise here the way
  `tag()`'s does.

### 4.7b Resolving issue #1883's remaining two items

Issue #1883 (filed from the same #1871 review that produced #1852/#1851)
tracked two further items beyond the escape/markup-coverage audit (§4.6)
and the brace-balancing spec home (§4.7/§4.7a) above. Both are resolved
here, per the issue's own two acceptable resolutions ("get a ruling, or
confirm the existing one still applies" / "fix the parity gap, or record
explicitly why it's intentionally asymmetric") — **neither required a
code change**; both were confirmed, by re-deriving the existing rulings'
own reasoning, to already answer the question correctly.

**Item 1 — should `HASH` become depth-aware in `cue_name()`, matching
`COLON` (#1851)? CONFIRMED: no — §4.7's own per-tag-scope ruling already
settles this.** That ruling's reasoning is not specific to *resetting*
depth at a sibling boundary — it is a claim about what kind of thing
`HASH` *is*: "a real, tokenized boundary (each one starts its own `TAG`
node), unlike the raw, grammar-blind `{`/`}` this scan balances." `COLON`
and `R_BRACE` are exactly that raw, grammar-blind kind — punctuation this
scan locally treats as "just text" while a brace is open, with no
grammar of its own to violate. `HASH` never is: an unescaped `HASH`
*always* begins its own `TAG` node, in `tag_line_tail`/`header_tag_tail`
and in the shared trailing-tag grammar `cue_line` itself reuses. Gating
`HASH`'s stop role by `depth == 0` would make that structural boundary
conditional on unrelated brace-balance elsewhere in the name — an
always-starts-a-new-`TAG` token becoming sometimes-just-name-text — the
same blurring the per-tag-scope ruling already rejects, just from the
opposite direction. `@NAME {a#b} c.` therefore still fails to parse:
pinned by `a_hash_inside_an_open_brace_still_ends_a_cue_name_early`
(`element.rs`'s test module).

**Item 2 — should `\}` gain the same backslash-parity carve-out `\{`
has, in both `tag()` and `cue_name()`? CONFIRMED: no — the asymmetry is
correct given the ruled escape set, not a gap.** `\{`'s carve-out exists
*because* `\{` is one of the ruled, final four-character inline escape
set (§8d.6, §4.6 above: `\< \{ \# \\`) — #1716/PR #1732 ruled it the
literal-brace escape, so a depth counter that counted it as a real
opener would be misreading a ruled escape. `}` is **not** a member of
that set — there is no equivalent "`\}` is a literal, non-metacharacter
close-brace" ruling anywhere to protect the depth counter against
misreading. An `R_BRACE` preceded by a `BACKSLASH` is therefore exactly
what it looks like: an ordinary backslash character followed by an
ordinary, structurally significant `}` — giving it `\{`'s carve-out would
*invent* a new escape meaning for a character the ruled grammar has never
assigned one to, not close a parity gap between two members of the same
set. `#tag \{a\}` (and `@NAME \{a\}`) therefore still end at the `\}`,
one character earlier than a "matched escape pair" reading would predict
— pinned by
`a_tags_own_unescaped_closing_brace_remains_the_terminator_even_when_preceded_by_a_backslash`
(`content.rs`) and
`a_cue_names_own_unescaped_closing_brace_remains_the_terminator_even_when_preceded_by_a_backslash`
(`element.rs`).

Neither resolution touches #2040 (should `tag()`/`cue_name()` reject an
*unrecognized* backslash sequence as a compile error) — that ruling
question remains open and is not answered by confirming these two
already-ruled-adjacent asymmetries are intentional.

**Item 3 — corpus fixture backfill.** `tests/corpus/19_tag_cue_name_brace_escapes.brink`
exercises both shapes that previously reached only in-crate unit tests: a
trailing tag's `\\{`/`}` (an escaped backslash followed by a real,
depth-counted brace) and a cue name whose own `{...}` balances internally
without swallowing the enclosing flow's closer — both parse cleanly and
are now covered by `corpus_roundtrip`/`coverage`, alongside the existing
unit-test coverage.

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
- **Succession rules are EDITOR-OWNED and externally defined** — the
  editing-time dual of chain rules (`after cue: enter → dialogue, tab →
  parenthetical`), consumed by the editor's transition machinery,
  ignored by the compiler. This is what makes the Tab/Enter behavior
  convention-driven instead of hardcoded ink.
  ⚠ **Corrected 2026-08-05.** This line previously read "succession rules
  live in the conventions file," which the 2026-08-03 "Conventions × the
  editor" ruling superseded ("the language says what a line IS; the editor
  overlay says what pressing Tab DOES") — `@[convention]` deliberately has
  no succession property. The rows are **not** declared in the conventions
  module and are **not** sourced by the compiler at all. The editor defines
  them however it likes and passes them in; the Rust side's only job is
  **validation and re-keying** against the projection's real convention
  kinds (`ConventionsProjection::with_succession`), so a rule naming a kind
  no convention declares fails loudly. Nothing about them is persisted, and
  they must never reach `.inkb` — see §5.2.

### 5.1 Implementation status — the harvest index (#2114, 2026-08-03)

The "harvest is a project-db index obligation" consequence has landed:
`brink_analyzer::harvest` (`HarvestIndex`) merges every file's HIR into a
project-wide cue/span index, and `brink-db`'s `harvest_index_query` (exposed
as `ProjectDb::harvest_index()`) is the sibling of `symbol_index_query` the
ruling names — same shape, same per-file `lowered_query` dependency, same
incrementality.

What shipped:
- **Cue payloads harvest independent of conventions.** `HirFile::cue_names`
  is a whole-tree scan for every `CUE_NAME` node (both the block `CUE` and
  the fused `COMPACT_CUE`), populated regardless of whether any `@[element]`
  handler ever claims the line. This matters because an unclaimed cue
  reports the loud `E129` and produces no `ElementMatch`/`Stmt` at all — a
  project with zero conventions would otherwise have nothing to harvest.
- **Markup spans upgrade from the host manifest today.** Unlike the cast
  roster (below), the manifest is an ordinary, already-registered project
  input — no comptime evaluation blocks reading it — so `SpanHarvest`
  already carries the manifest's `ManifestSpanKind` **verbatim** (the
  widened #1997 shape, `required` flag included) alongside harvested
  occurrences, rather than waiting on a later slice.
- **Element kinds are not indexed** — only the cue *payload* text and
  markup span *names*/*attribute names*, matching "element kinds are
  inherently declared" above.

What has **not** shipped: the **cast roster** — no type or registration
point for it exists anywhere in the compiler, so a harvested cue name has
no declaration-upgrade path yet (`CueHarvest` carries only harvest sites).
It is explicitly named as a tenant of the §3.5 module door, i.e. blocked on
the same comptime conventions machinery issue #1840 has not landed.

**Update (#2134, 2026-08-05): the cue-name completion consumer has landed.**
`brink-ide`'s `CompletionContext::CueName` (right after `@` at the start of a
line, at most a partial name typed) is offered from
`ProjectDb::harvest_completion_names()` — not the raw `harvest_index()` —
in both `brink-lsp`'s `completion` handler and `brink-web`'s
`EditorSession::completions`, proven cross-file (a cue declared in one file
completes while editing an unrelated sibling that never imports it, in both
consumers) with no conventions handler or host manifest required. The
Eq-cutoff gap the original ruling flagged — `HarvestSite` carries a
`TextRange`, so `HarvestIndex` can never `Eq`-cutoff the way
`resolution_index_query` lets `SymbolIndex` — is closed by
`harvest_completion_index_query` (`brink-db`), which projects the index down
to bare cue/span/attribute name sets (`brink_analyzer::HarvestNames`,
`HarvestIndex::names()`) before any completion path reads it.

Still **not** built: markup span/attribute completion — no `<tag …>`
completion context exists yet in `brink-ide::completion`; only the cue-name
position was in #2134's scope.

### 5.2 Implementation status — the succession-row carrier (#2115, 2026-08-04)

The 2026-08-03 "Conventions × the editor" ruling (`docs/decision-log.md`)
settled that `@[convention]` deliberately has no succession property: "The
language says what a line IS; the editor overlay says what pressing Tab
DOES." This section's "Succession rules live in the conventions file" line
above describes `DialogueDialect` (#368)'s pre-ruling shape, not where these
rows are declared today — they are **not** declared in the conventions
module. What shipped:

- **The carrier.** `brink_ir::ConventionsProjection` gained `transitions:
  Vec<TransitionRow>` / `templates: Templates` fields plus a
  `with_succession(transitions, templates) -> Result<Self, Vec<DialectError>>`
  builder that re-keys both against the projection's own `entries[].name`
  (plus `reserved_structural_kinds()`) instead of `DialogueDialect`'s
  independent `elements` list — the re-keying this ruling calls for.
- **One shared validator.** `dialect::validate_succession` is called by both
  `DialogueDialect::validate` (against its own `elements`) and
  `with_succession` (against the projection's kinds), so the two can never
  silently disagree. As a side effect it closed a pre-existing gap:
  `templates` entries were never checked against `elements` at all before
  this landed; `transitions` and `templates` are now validated identically,
  each producing its own error variant
  (`DialectError::TransitionUndeclaredKind` /
  `DialectError::TemplateUndeclaredKind`).
- ~~**The wire mirror.** `brink_format::ConventionsProjectionDef` carries
  `transitions`/`templates` through the `.inkb`-section-codec shape
  verbatim (`CONVENTIONS_PROJECTION_WIRE_VERSION` bumped `1` → `2`); nothing
  emits this section into a real `.inkb`/`StoryData` file yet (unchanged
  from #2111).~~ **REVERSED, 2026-08-05.** The ruling *"Succession is
  EDITOR-OWNED and externally defined"* (`docs/decision-log.md`) undid this
  half: `transitions`/`templates` never carry on the wire at all —
  `ConventionsProjectionDef` only ever mirrors `entries`
  (`CONVENTIONS_PROJECTION_WIRE_VERSION` bumped `2` → `3`, issue #2277).
  These fields are editor-overlay data that "never travels beyond tooling"
  (`brink-ir/src/dialect.rs`'s own doc), and `.inkb` is beyond tooling — it
  is the compiled artifact a game host loads, and a runtime has no Tab key.

What survives: the **validator**, unchanged. `ConventionsProjection::with_succession`
still re-keys externally (editor-)supplied
`transitions`/`templates` against the projection's real convention kinds,
in-process, via the same shared `dialect::validate_succession` described
above — so a rule naming a nonexistent kind still fails loudly. The editor
owns the succession data and its storage; the Rust side's only job is that
validation, never sourcing or transporting the rows itself. `with_succession`
is exercised only from `brink-ir`'s own test module today — this is a
validation service with no wiring to a real producer yet, not a feature.

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

## 7. Runtime output (RULED — naming closed by §8d.7)

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
- Naming (`Line`/`Step`/`StoryEvent`) — **closed: `Step`** (§8d.7).

### 7.1 Implementation status (#1684, 2026-08-02)

`Step`/`OutputLine`/`BlockId` shipped in `brink-runtime`, migrating
every marshal leg (`brink-web`, `bevy-brink`, the CLI TUI, `brink-ide`).
Scoped narrower than this section's full worked shape, deliberately:
`OutputLine` today is `{ text: String, tags: Vec<String>, block_id:
BlockId, element: Element }` — flat text, no `parts` decomposition yet
(see §7.2 below for `element`'s own scoping). That's the
information-identical schema-less-ink degenerate case this section's
own "superset check" calls out; the markup-`Part` structured surface
is #1683's job once the element/markup layer lands, riding the same
`Step`/`OutputLine` contract this issue created rather than reopening
it. `BlockId` counts uninterrupted runs (bumped on a
choice selection, a `Done` resume, and a host-directed jump); the
richer attachment-derived assignment described in §3.7 is deferred to
that same follow-up. The harness-side attribution safeguard
(`termination.rs::push_terminal`, reserved since PR #1513) grew its
fold logic in the same PR — oracle CASES/EPISODES measured
byte-identical to the pre-migration baseline (5607/1010/2 episodes,
365/8/397 cases).

### 7.2 Implementation status — `Element.data` populated for `attach` (#2108)

`brink_runtime::Element { kind: String, data: BTreeMap<String, String> }`
shipped as `OutputLine.element` (#1683/#1684), migrating the same marshal
legs #1684 touched (`brink-web`'s `LineJs`/`ElementJs`, the
`@brink-lang/web` `Line`/`SessionLine` TS types) — at first scoped to the
degenerate case only, every line reporting `Element::narrative()`
regardless of source markup.

**Issue #2108 populates `data` for the one case the 2026-08-03 "element
output model" ruling settled**: an `attach = StructName` convention
handler's claimed line consumes itself (no `Step::Line` at all — ruling
item 6, "AN EVENT EXISTS IFF A LINE EXISTS") and its returned struct's
fields merge into the following run's `Element.data`, copied onto every
line materialized while the run is open (item 5) — `cue` and
`parenthetical` in `std/conventions/screenplay.brink` both attach onto the
same dialogue run (item 3). Mechanism: two new bytecode opcodes
(`Opcode::AttachElement`/`Opcode::EndElementRun`, `brink-format`) push
markers into the VM output buffer's own append-only transcript
(`OutputPart::ElementAttach`/`ElementAttachEnd`, transient — never reach
the persisted `.brkt` wire format) rather than mutating a live `Flow`
field, because the buffer defers a line's commitment until later content
proves no `Glue` reaches back over it — a naively "current, mutable"
field would misattribute a later run's data to an earlier, still-buffered
line. See `crates/brink-runtime/tests/element.rs`'s
`attach_convention_data_reaches_the_following_run` for the runtime proof.

**Still scoped narrower than the full ruling, honestly**: `kind` stays
`Element::NARRATIVE` even for a claimed line — classifying `kind` itself
for a non-attach single-line handler (`heading`/`transition` reporting
their own handler name) needs either new `.inkb` line-table storage or the
same VM mechanism, and is not attempted here. `BlockId` is not re-derived
from attach runs either (§7.1's own note stands: it still just counts
terminator-bounded runs) — a run of adjacent attached lines can span more
than one `BlockId` if it also crosses a real terminator. Oracle
CASES/EPISODES measured unchanged from the current ratchet (5608
episodes) — expected, since no oracle fixture uses `@[convention]`/attach
dispatch.

**Persistence — RULED 2026-08-05, `docs/decision-log.md`** ("Two rulings:
… block metadata persists with `next_block_id`"): **block metadata
persists, and `next_block_id` persists with it** — for two independent
reasons, not one. (a) `pending_element` must persist because an open
attach run's accumulated data lives only in the VM output buffer's
`pending_element`/the transcript (`resolve_lines_annotated`'s
`current_element` accumulator, `crates/brink-runtime/src/output/mod.rs`),
neither of which survives a park — losing it drops the attributed
speaker/metadata on resume, the exact class this project refuses to ship.
(b) `next_block_id` must persist on its own account, independent of
attachment: restarting it at 0 would give the *same* uninterrupted run a
different id after resume (and could collide with ids already emitted
before the park), breaking `BlockId`'s documented "same id iff same
uninterrupted run" contract (`crates/brink-runtime/src/story/types.rs`).
Attachment and `BlockId` remain the two independent concepts they always
were (this section's own note above, and `BlockId`'s doc, both stand
unchanged) — they simply share one park boundary, so both need a
save-stable value at that boundary.

⚠ This corrects the paragraph's earlier claim that `BlockId` "was
already never persisted" — that was true, and remains true, for the
*ordinary* game-state save (`SaveState` still never captures execution
position or the output buffer — `brink-runtime::save`'s own module doc,
unchanged), because a host re-entering at a known knot never compares a
block id across that boundary. It stops being true for a flow parked
(`Step::Suspended`) *inside* an open attach run: `Step::Suspended` is
deliberately not a `BlockId` run-terminator, so an `await` mid-`cue` is
representable, and resuming from `brink_format::SuspendedFlow` (the
FlowFrame, `docs/flow-suspension-spec.md` §2) continues the *same* run
rather than re-entering a knot from the top. `SuspendedFlow` now carries
`next_block_id` and `pending_element` (the run's accumulated attachment
data at park time) for exactly that boundary — format-only, like the rest
of `SuspendedFlow` (FS-1 landed; the FS-2/FS-3 compiler synthesis and
runtime spill/restore that would produce/consume a *live* value are still
later slices, so `Story::save_state`/`load_state` still always
produce/consume `suspended: None` and this doesn't change today's
observable behavior). What was undesigned — *where* attachment's
save-stable identity lives — is now ruled; the runtime wiring that reads
and writes it is the next slice.

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
   (journaled, effect-checked via the manifest). **Scene entry, as
   shipped (#2092, PR #2144):** an AUTHORED call inside the screenplay
   preset's `heading` handler (`std/conventions/screenplay.brink`) —
   `scene_entered(title, slug)` fires when the heading LINE is claimed
   by the handler, not on "entering the stitch" (a heading is not
   promoted to a real HIR stitch — that promotion is #1717, closed
   superseded by §9.1 without delivering it) and not planted by pure
   codegen — it is an ordinary logic-line statement the handler's
   author wrote, riding the same extern/`ExternalFnHandler` call path
   any other call would. `slug` is currently always the empty string
   (no slug-bearing heading is claimable by this handler yet,
   #2077/#2078). **Written transitions** (`SMASH CUT TO:`)
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

**Three shapes lower; the rest deliberately do not.** Issue #1838 landed
natural-notation dispatch, so a **scene heading** whose text an
`@[convention(claims = "…", order = N)]` handler matches lowers to exactly one call on
that handler (`brink_ir::hir::lower_native::element`) — the first time any
of this grammar reached output. Issue #1720 (the built-in screenplay
preset) widened `element::candidate` to the two remaining literal-line
grammar shapes this section names — a real `CUE`'s name and a chain-gated
`PARENTHETICAL`'s delivery text are now claim candidates too, exactly the
same way (only a wholly literal run, no tag extension) — so `@NAME` and
`(delivery)` lines now reach output through the same mechanism once a
preset or project declares a matching handler; `std/conventions/
screenplay.brink` is the shipped built-in one. `COMPACT_CUE` (`@NAME:
text`) stays unclaimed (its fused name+text shape doesn't fit
`try_claim`'s single-text-node contract), as does any cue/heading
carrying a tag extension, and a heading carrying an explicit `[slug]`
(every worked-page heading in §8/§8c/§8d does) — `candidate`'s literalness
rule declines all three the same way it declines a `CONTENT_LINE` with
interpolation. Promoting a slug-bearing heading to a genuine HIR stitch (a
real divert target, §3.2/§3.3) is not built anywhere — issue #1717, which
would have owned that, was closed as superseded by the §9.1 ruling without
delivering it — so a heading-declared divert target, as §8c/§8d's worked
pages write one, is not reachable through any preset today; a project
still needs an ordinary `flow name() { … }` for that. Element
roles/attachment (§3.6/§8b.7–8) are the `block` capture mechanism (issue
#1839, landed) rather than a separate concept; per-flow tag *APIs* are
#474, whose iceboxed authoring surface this grammar supplies. (The
conventions `lower:` column this paragraph used to name is **dissolved**
— see `docs/decision-log.md` 2026-07-31.) `hir::lower_native` still
reports every *unclaimed* shape as not-yet-lowered (`E129`) rather than
reading it as ordinary prose or dropping it.

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

1. ✅ ~~**The `std::conventions` types** design pass~~ — **RESOLVED, not
   owed.** Dissolved by the 2026-07-31 §9.1 ruling (the extension-ergonomics
   problem "becomes `use` plus a registration call"), and the rest of §3.5's
   companion list is settled too — see §3.5's retired-2026-08-02 bullet for
   each item's disposition. This entry described it as "the last substantial
   prose-round design item"; that was already false when written and it caused
   repeated requests to re-design ruled work.
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
6. **Editor implications (NS-T)** — ✅ **The compiler-first hold below is
   LIFTED (2026-08-05, `docs/decision-log.md` "The compiler-first hold on
   the native editor track (#1131 / NS-T) is LIFTED").** Native editor work
   — including real native token classification, previously the one part
   of this section explicitly held — may proceed in parallel with
   remaining compiler work; #1131's status banner is the up-to-date charter
   for what is built vs. outstanding. Issue #2280 (native semantic-token
   classification, closed by the PR landing this update) is the first work
   shipped under the lift. The bridge features (§2b.3), the park
   decoration (§8c), and the built-in token vocabulary (§3.5b) remain
   in scope for follow-on native editor work generally.

   The paragraphs below record the **now-superseded** 2026-08-01 hold and
   its rationale — kept for history, not as current guidance:

   ⚠ **Scope of the hold, ruled 2026-08-01 (superseded 2026-08-05):** it
   covered editor *frontend* work — CM6, token rendering, the live
   renderer, `fmt`. It did **not** cover the classification/explain-match
   query family, which is emitted from `brink-db`/`brink-ide` and is
   **compiler work wearing editor's clothes** (#2006). Holding compiler-side
   queries behind the compiler track would have held them behind
   themselves.

   ⚠ **Why they were held, restated 2026-08-01 (maintainer; superseded
   2026-08-05).** The hold was **deliberate sequencing — the compiler work
   finishes before the editor work starts** — *not* an unlifted blocker
   waiting on this document. The original 2026-07-25 rationale ("don't
   classify tokens against a surface that will shift") had in fact already
   been overtaken: that surface landed 2026-07-28 (#1715 closed, #1716
   landed, #1717 closed, escape set final per §8d.6). A review on
   2026-08-01 read the bare "stay held" above as stale and concluded the
   hold should be lifted; at the time it should not have been — that
   changed on 2026-08-05, per the maintainer ruling linked above.

   **Also ruled 2026-08-01: take the free part now.** Register `.brink`
   *and* gate `semantic_tokens_full`/`_range` on `db.is_native` in the same
   change — switching on the native diagnostics, hover, go-to-definition,
   rename and cross-file scope that **already work** and that no client
   currently requests. Real native token classification stayed held at the
   time; the two halves shipped together: `parse_query` is unconditionally
   the *ink* parser with no dialect gate, so registering alone would have
   lit up a live bug (the server emitting ink-misclassified tokens over
   native source), latent only because nothing asked.

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
there (`@NAME` cue dispatch, §8b.9; the `!name` annotation-element dispatch
sigil, §3.5b, issue #2004). That is the true start of a physical line for an ordinary
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
