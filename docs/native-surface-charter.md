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

Lua-adjacent feel; expression-oriented (Rust's
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
