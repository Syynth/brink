# The native surface — charter & sitting-1 convergence

Status: **DRAFT — design sittings in progress** (first sitting
2026-07-18, this document transcribes its convergence). Nothing here
is implemented; nothing is ratified until the decision-log says so.
Companions: `docs/flow-suspension-spec.md` (the await machinery this
surface will spell), charter #905 (the chart dialect's future client),
#368 (absorbed: dialogue conventions become the prose dialect's
Fountain lineage).

## 1. What this is

A **new primary surface syntax** for brink — prose and code both —
compiling to the same HIR as the ink frontend. Ink syntax remains a
permanently supported compatibility frontend (the oracle corpus
guards it forever); the native surface becomes what brink documents,
teaches, and prefers.

**Three drivers, all standing:**
1. **Evidenced friction**: code-only entities are walls of tildes;
   ink's block syntax crams unrelated constructs into one spelling;
   the AST is hard to disambiguate (maintainer gripes, 2026-07-18,
   backed by the Compound friction journal and corpus lanes).
2. **The architectural enabler**: a second frontend forces a defined
   **HIR admission contract**, after which per-domain frontends
   (statecharts #905, dialogue conventions) become ordinary clients
   instead of heroic projects.
3. **Design sensibility**: brink is the maintainer's language; the
   surface should think like its author. This is a first-class
   reason, not an apology.

## 2. Ground rules

- **Same semantics.** This round respells; it does not change
  behavior. Everything lowers to today's HIR.
- **The parking lot** (semantic changes deliberately deferred, each
  gets its own future round): inline markup / rich text in output.
- **The watch list** (same-semantics at the core, edges may touch
  HIR): blocks-as-values in the code dialect's
  everything-is-an-expression posture; declared container nesting
  deeper than knot.stitch; `for`-generated choices (sugar over the
  proven recursive-thread pattern — see §7).
- **The rendering principle**: the prose dialect targets
  Obsidian/Scrivener-style live rendering. Therefore every
  structural mark must be **renderer-elidable** — an explicit token
  the presentation layer can consume/restyle — and **whitespace is
  never semantically load-bearing** (fmt maintains cosmetic
  indentation; the renderer draws the real fold).
- **Strictness**: structure is exact, never inferred loosely. (Ink's
  weave nesting is relative/forgiving — "the count is a suggestion" —
  which taxes authors AND withholds the guarantee. We invert both.)

## 3. The three axes (the thesis)

Every container has three independent properties ink welds together:
1. **Coloring** — story-time (`flow`: visit-counted, addressable,
   suspendable) vs expression-time (`fn`: call/return). The
   FlowFrame boundary, surfaced. Keywords RULED: **`fn` / `flow`**
   (flow matches the runtime's own vocabulary end-to-end).
2. **Body dialect** — which sub-language fills it: **prose**
   (Fountain-lineage), **code** (Lua-ish, expression-oriented),
   future **chart** (Mermaid-lineage, #905). Interleaving between
   dialects is designed per-pair (§8).
3. **Addressing** — position in the nested namespace; falls out of
   structure (file-as-module per the modules round, containers nest).

Ruled: **equally first-class spellings** for the combinations — a
code-bodied `flow` (the Compound guard) is exactly as natural as a
prose-bodied one. Defaults may exist per keyword but every
combination is honestly spellable.

## 4. Containers

- Declarations: `flow garden(mood) { … }`, `fn heal(hp) { … }` —
  keyword + name/params + braced body. No tags, no name-as-attribute
  (the SFC lesson: tags suit anonymous facets; named things deserve
  declarations). No one-flow-per-file constraint — files hold many
  declarations (RULED: hard requirement).
- **Stitches are nested `flow`s** — `garden.gate` because addressing
  is nesting. (Depth >2 = watch list.)
- Braces are the universal body delimiter — "solid" — for
  containers, fn bodies, choice bodies, annotated blocks alike.

## 5. Prose-ground structure: the weave, respelled

- **Choice points are explicit blocks** — the annotated brace `{?`
  … `}` (tentative spelling; the concept is RULED). All points are
  explicit; the runtime's emergent accumulation gets an honest
  syntax:
  - `*` / `+` choice lines inside the point (once / sticky — kept).
  - **Choice bodies take braces** when they have nested content:
    `* 'A wager!'[] I returned. { … }`.
  - **Splice**: `<- some_flow(args)` inside a point harvests that
    flow's choices into it (thread assembly, scoped and explicit —
    strictly clearer than ink's ambient accumulation; same HIR
    machinery).
  - `for`-generated choices: watch list (§2) — sugar over the
    community's recursive-thread generator pattern
    (FUNC_populate_options_thread is the exhibit).
- **THE GATHER IS DISSOLVED.** With explicit points and braced
  bodies, "after the choices rejoin" is simply the next line after
  the block. The `-` gather sigil, depth-matched rejoining, and the
  sequential-gather squint all cease to exist as authorial concepts
  — same semantics, structure now visible. (The single most
  identity-altering respelling in this charter.)
- Choice-line anatomy is **kept as-is**: the `[]` display-split,
  `<>` glue, divert arrows. (RULED: "the choice syntax is fine.")

## 6. The annotated-brace family (the block system)

One brace grammar; the annotation position declares the kind:
- **Bare `{expr}` = interpolation** — and nothing else, ever.
- **Words where logic branches**: `{if cond: … else: …}` (inline or
  multiline), `{match x: …}` for switch forms. Conditional choice
  guards reuse it: `* {if cond} [text]`.
- **Characters where content alternates**: `{~ }` shuffle, `{& }`
  cycle, `{! }` once, `{| }` stopping-sequence (chars tentative).
- **`{?` choice point** (§5) — one member of the family.
- **Entry markers**: inside a multiline annotated block, a leading
  `-` opens an entry/arm (multi-line entries, nested structure
  allowed; entry = until next `-` or `}`). `-` has NO other meaning
  anywhere (gathers are gone; in plain prose a leading dash is just
  text). Escape for a literal leading hyphen inside an entry.
  - **DOCS NOTE (maintainer-flagged)**: entry markers and multiline
    block anatomy were under-understood even by the implementer in
    ink's spelling — the new docs owe this construct a first-class,
    example-led explanation, not a footnote.

## 7. Code-ground (sketch — own sitting pending)

**North star: RustScript** (AMENDED 2026-07-19, superseding the
original "Lua-adjacent feel" — ruled in the lambda sitting: the
dialect was already Rust-shaped in every ruled bone — fn/let/match/
enums/structs/use/::/@[…]/ranges — so family coherence IS the
cold-reader story). Expression-oriented (Rust's
everything-is-an-expression as the pole star, blocks-as-values on
the watch list); no tildes — code is the ground. UFCS
(`x.foo(y)` ≡ `foo(x, y)`) is the leading answer to the
methods/lambdas question per the friction dossier (#901 comment,
2026-07-18): sugar over free functions, no method system. Field
access beats UFCS on resolution. Details = sitting 2.

## 8. Open items (the remaining sittings)

1. Code dialect details: expression grammar, UFCS resolution rules,
   stdlib round (heap/tuples/floor-div/char_at/weighted tables), the
   #827 vec decision (structs+UFCS may suffice — decide with syntax
   in hand).
2. Interleaving escapes, full inventory (prose→code beyond
   interpolation; code→prose emission; grains).
3. Divert/tunnel/thread spelling in the new surface (`->`, `->->`,
   `<-` are load-bearing ink idioms — keep? respell?). `await`'s
   native spelling (currently contextual-keyword in ink dialect).
4. Alternation annotation chars finalization; `{?` final call.
5. File extension, naming, migration/coexistence story (ink ↔ native
   converters?), tooling plan (parser, fmt, LSP, renderer), and the
   HIR admission contract document.
6. The chart dialect (#905's season, as this contract's client).

## 9. Exhibits

The Fogg passage (WritingWithInk) and FUNC_populate_options_thread
served as sitting-1's concrete anchors; respelled versions live in
the sitting transcript and should graduate into the spec's examples
when drafting begins.

## 10. Audience & evaluation (added after the critical review, 2026-07-18)

- **Beginner-friendliness is an elevated, named driver.** The
  maintainer is onboarding a writer who is willing to learn but not
  ink-fluent; the surface is designed for the person *joining* the
  project, not for preserving fluent-reader reflexes. RULED framing:
  the semantics fix the concept inventory either way, so **more
  distinct syntax = fewer overloaded spellings per concept** — the
  pedagogically cheaper direction even with a longer glyph list.
  Overloading (one brace, four meanings) is where beginner confusion
  lives; one-spelling-per-concept is the target.
- **The validation instrument**: the writer is the designated cold
  reader. When a prototype parser exists, their first authored
  scenes — and every confusion they hit — form the syntax friction
  journal (the Compound-journal method applied to notation).
  Ratification should follow real authored content, not exhibits.
- **Honest caveats carried from the critical review** (open, not
  blockers): (a) the flat choice-run — ink's most scannable form —
  pays a ceremony tax in the redesign; a compact spelling for the
  bodiless flat case stays on the open list (§8). (b) The
  annotated-brace family's at-a-glance legibility leans on syntax
  highlighting and the rendered editor; the bare-diff/grep
  experience should be checked deliberately during prototyping.
  (c) Total mark vocabulary grows even as overloading falls —
  the docs must teach by concept, not by glyph list.

## 11. Sitting 2 — the narrative layer, completed (2026-07-18)

Walked the full remaining concept inventory; every item disposed:

- **Diverts — KEPT verbatim** (`->`, `-> knot(args)`, `-> END`/`-> DONE`,
  divert-targets-as-values). Ink's best syntax; ratified untouched.
  **But `-> DONE` is no longer *required*** (RULED 2026-07-22): a flow
  or any braced body that runs out of content **ends implicitly** —
  lowering to the DONE terminal, no ceremony. `-> END` stays
  explicit-only (the permanent "story over" act); `-> DONE`/`-> END`
  remain available but optional. Ink's "ran out of content. Need a
  `-> DONE`?" error is retired on the native surface — brace-delimited
  flow *and* choice bodies make body extent explicit, so "runs out ⇒
  ends" is unambiguous. Value-returning flows are the exception (must
  return; checker-enforced). See decision-log 2026-07-22.
- **Tunnel calls — KEPT as `-> place ->`** (RULED after weighing a
  `<->` challenger: in content position the arrows are the prose
  ground's motion vocabulary). **Tunnel return — RESPELLED as
  `return`**, unified with the code dialect's "leave this container";
  ink's return-redirect `->-> x` becomes the self-explanatory
  composition `return -> x` (pop the obligation, then go), which also
  documents the stack-effect difference from a plain divert that
  ink's spelling hides.
- **Threads — narrowed**: no native spelling for general `<-`; only
  the scoped splice inside choice points survives. Runtime unchanged
  (ink compat + oracle hold it); parking-lot note records the
  maintainer's appetite for eventual removal.
- **Glue — KEPT** (`<>`), with a parking-lot entry for its real cost:
  boundary-retraction semantics force speculative lookahead in the
  runtime. The sketched escape (semantics round, someday): eager
  fragment delivery with a joins-previous flag — line assembly moves
  to the presentation layer; the classic Line API keeps speculation
  as a compatibility shim.
- **Tags & speakers — KEPT**; the `@SPEAKER:<>` idiom is explicitly
  assigned to the future Fountain-lineage dialogue-dialect sitting.
- **Fallback choices — RESPELLED as `else`**: a choice point's
  fallback is its else-branch — `else { … }` last in a `{?` block,
  one per point. Ink's `* ->` cipher dies. (`* []` displayable-empty
  choices are unrelated and pass through.)
- **Choice labels — KEPT** in the one label syntax: `* (name) [text]`.
- **Declarations — lowercased**: `var`, `const`, `struct`, `extern`
  (short form ruled), `import`. **LIST is renamed `flags`** (RULED):
  the concept is an ordered domain of named symbols with
  subset-valued variables; `list` is reserved for real sequences
  (stdlib season), `states` is radioactive (#905), `set` reads as an
  imperative. `flags Mood = (calm), wary, hostile`.
- **Trivia — KEPT**: `//`, `/* */`, `TODO:`.

New sibling work discovered by the sitting: **#1087 — an `emits`
effect** (statically know whether an invocation produces content;
purity ≠ silence, so wake conditions want pure AND silent). T2's
first extension; designed in its own mini-sitting.

Remaining open in the narrative layer: the flat-choice-run compact
spelling (§10 caveat a) — everything else is converged. Next sitting:
the code/scripting dialect (§7).

## 13. Enums & the module system (sitting 3 continued, 2026-07-19)

### 13.1 Enums (new feature — additive, brink-native)
**`flags` is many-of; `enum` is one-of.** Rust-form declarations
with **named-field payloads only** (no tuple variants — one
construction syntax, §12.8's capability applied to variants) and
**concrete payload types only** (no generics — `Option`/`Result`
are the #1090 ledger's predicted first entries, by design):

    enum Phase { Patrol, Suspicious { level: float }, Chase { target: Npc } }

Exhaustive `match` under the static regime (missing variant =
compile error — the agent-author's refactoring supervisor). Unit
variants join the map-key domain. Variant access is **dot**
(`Phase.Patrol`): the separator stratification below reserves `::`
for module walls. Runtime tail (Value variant kind, wire form,
marshal legs — #950's exhaustive matches enforce completeness),
name-keyed rehydration + `@[was()]` on renames per standing
posture. #905's statechart states inherit this feature as their
vocabulary.

### 13.2 The module system (respelled + resolved)
- **Separator stratification**: `::` crosses module walls; `.`
  walks everything inside (containers, fields, variants, UFCS).
  `story::npcs::guard.patrol`.
- **The tree is filesystem-derived** (directories = segments, files
  = leaf modules; no mount ceremony — deviation from Rust, earned
  by the auto-include heritage and the agent driver: path on disk
  = path in language). Uniqueness comes from the filesystem;
  declared `module name` blocks nest within files as before.
- **`story::` is the absolute root and represents the whole
  project.** One project = one story-universe = **one compiled
  artifact**; runtime multiplicity is flows + `local` state (the
  scoped-flow-state model IS the many-narratives machinery — no
  multi-binary story needed).
- **THE TREE IS THE COMPILATION UNIVERSE; IMPORTS ARE NAMING
  ONLY.** Every file in the tree compiles and ships. `use` grants
  source-visible names and nothing else. Consequences: textual
  INCLUDE is dead; engine-only-reachable modules (an NPC dialogue
  no module imports, spawned by the host by absolute path) are
  first-class — the module graph has many roots; no re-export
  ceremony exists or is needed (absolute paths + prelude +
  visibility cover Rust's pub-use use cases).
- **Rust's `use` syntax lifted verbatim** (`use
  story::market::{barter, haggle};`), with the ceremony owned by
  tooling (auto-import inserts, fmt organizes) — neither the
  writer nor their agent types it by hand.
- **Saves/wire always record absolute paths** regardless of
  imports (DefinitionId = (module, name) as ruled; the #719
  save-stability landmine stays defused).
- **`host::` root (stdlib-sitting design)**: engine-reachable
  functions mount into the graph from the capability manifest
  (`host::audio::play_sound`) — the manifest gains namespace
  discipline; unification with in-tree `extern fn` designed there.
- Runtime-state sharing across simultaneously running flows/hosts
  is a host wiring concern; module-level sharing is of
  DECLARATIONS. (One line, so nobody conflates the two.)

### 13.3 Stdlib-sitting docket (updated)
Prelude design · `host::` mounting · the verb inventory · real
sequences + the `list` reclaim · map surface · iteration protocol
· #827 vec decision · anonymous-record fate · **faults-vs-Result
posture for fallible functions** (now spicier: enums exist,
`Result` is ledger-gated) · assertion spellings (`@[effects]`,
holes' release policy).

## 14. Doc comments (B0.6b, RULED 2026-07-20)

§11 ruled trivia to be **KEPT**: `//`, `/* */`, `TODO:`. Doc comments
carve two spellings back out of that bucket and promote them to
**first-class by structural attachment** — a CST node the parser
builds, not a fact HIR re-derives by walking trivia backward from a
declaration. See `docs/decision-log.md` → "Doc comments ruled
first-class on the native surface" for the full ruling; this section
records the surface + attachment model where the spec lives.

**Two spellings, one content model:**
- **`///` (outer)** — immediately precedes a declaration
  (`flow`/`fn`/`var`/`const`/`flags`/`struct`/`extern`/`use`/
  `import`/`module`) and documents *that* declaration. Ink's own
  spelling, unchanged.
- **`//!` (inner)** — sits at the very start of a knot/flow/file
  body and documents the *enclosing* container instead of a
  following declaration (Rust precedent; ink had no equivalent —
  ink's weave has no "start of body" position clean enough to own
  one). Gives a flow/file a header without needing a fake leading
  declaration to hang `///` off of.
- **Exactly three slashes** is the outer marker (`///`); a fourth
  (`////`) falls back to a plain `//` comment (Rust precedent —
  a separator rule of `////////` banners stays available). `//` and
  `//!` are otherwise lexed the same way (run to end-of-line).

**Attachment is CST-node, not trivia-walk** — decided once,
structurally, by the parser (the layer with the most context), not
re-derived per-consumer:
- A contiguous run of `///` lines (a blank line, or a plain `//`
  line, breaks the run — same contiguity rule the old trivia-walk
  used) immediately preceding a declaration becomes a `DOC_COMMENT`
  CST node emitted as **the leading child of that declaration's own
  node** (`FLOW_DECL { DOC_COMMENT, KW_FLOW, IDENT, … }`, not a
  floating sibling). The AST's `.doc()` accessor reads it directly —
  no backward token walk, ever, on the native surface.
- A contiguous run of `//!` lines at the very start of a `flow`/`fn`
  body (leading blank lines tolerated; real content first
  disqualifies it) becomes a `DOC_COMMENT` node as the leading child
  of the enclosing `BLOCK` — `ast::Block::doc()`. Same shape at the
  very start of a file (`ast::SourceFile::doc()`), CST-only for now:
  no native HIR type represents whole-file identity yet (§13.2:
  identity is filesystem-derived, a project-layer fact), so nothing
  consumes it below the AST today — reserved for the LSP/fmt/
  source-map consumers this attachment model exists to serve.
- One `DOC_COMMENT` node shape covers both spellings; a
  `.is_inner()` accessor tells them apart by which token kind (
  `DOC_COMMENT_OUTER` vs `DOC_COMMENT_INNER`) its children carry.
  Neither token is trivia (`SyntaxKind::is_trivia`) — the parser
  dispatches on them directly, so a formatter or LSP hover walking
  the tree finds the doc exactly where the grammar says it lives.
- **Judgment call, flagged for a later ruling, not resolved here**:
  an *unattached* `///` run — nothing declaration-shaped follows —
  falls back to sitting bare in the tree with no diagnostic, same
  posture as ordinary trivia. Whether that should instead earn an
  `unused_doc_comment`-style warning is open.
- When both spellings are present on the same container (a leading
  `///` before `flow`, AND a `//!` at the top of its own body), the
  outer form wins and the inner form is simply not consulted — they
  are not merged. Not a hard ruling, a pragmatic default: the outer
  form is the one visible without opening the container.

**Content model unchanged, and deliberately NOT pushed into the
grammar** — the `@param name {type}` / `@returns {type}` / `@kind`
tag vocabulary stays a plain string-parse over the attached node's
lines (`DocBlock`, `@`-tag handling, E038 malformed / E043
inapplicable-to-this-declaration-kind), now factored so both
frontends' attachment steps (native's CST-node read; ink's trivia
walk) feed the identical parser. Rejected the heavier alternative
(a real grammar production for `@param`/`@returns`/`@kind`, coupling
the grammar to the host-manifest tag vocabulary and `TypeRef`) as
disproportionate to what the tags need to express.

## 15. Story entry: `flow main()` (RULED, 2026-07-21, #1106 G-batch)

**A top-level `flow main()` is a native story's default standalone
entry point.** No new syntax — the RustScript-idiomatic answer to
"where does a `.brink` story start", mirroring `fn main()`.
Mechanically, `lower_native::lower` synthesizes `root_content` as a
single `Divert` into `main` (the same `Divert`/`Block` HIR ink's
own root-content-is-the-entry model already uses — see
`crates/internal/brink-ir/src/hir/lower_native/mod.rs`'s
`entry_root_content`). A file/project with no top-level `main`
compiles with an empty `root_content` — not an error, just "no
standalone entry point"; any other top-level `flow`/`fn` remains a
**host entry point** only (effects-spec §10 "play from here" —
engine-driven scene entry by absolute path, unaffected by this
ruling). A `main` that takes parameters is not matched (a bare
entry divert can supply none) — it stays an ordinary, host-enterable
flow.

This resolves the long-flagged native story-entry question (#1106 /
the G-batch) that the `exhibit-fogg-passage` respell fixture first
surfaced (PR #1202): a top-level bare `-> flow` entry-divert line is
now **superseded** by the `main` naming convention, not a parallel
spelling — the respell corpus (`tests/tier1-brink-respell/`) was
updated to match (docs/decision-log.md, "B0.10: flow main() native
story-entry convention").

**Open question surfaced by first light, not yet ruled**: ink grants
literal ROOT content a free pass to end implicitly (no `-> END`
needed — running off the end of the story's outermost content is
`Ended`, not an error) but does **not** grant that same grace to a
knot/flow's content reached by an ordinary divert (running off the
end there is ink's own "ran out of content, do you need a '-> DONE'
or '-> END'?" error). Wrapping former root content in `flow main()`
per this convention moves it from the first bucket to the second —
so a `main` that would have relied on ink's root-content grace now
needs an explicit terminator it didn't need before. Whether `main`
should inherit root's implicit-end grace (since it now plays root's
role) or whether native authors are simply expected to always
terminate `main` explicitly (arguably in keeping with native's more
explicit posture elsewhere) is not decided by this ruling and needs
one — see the first-light build report (issue #1106) for the
concrete fixtures (`const-vars`, `simple-glue`, `basic-tunnel`) this
affects.
