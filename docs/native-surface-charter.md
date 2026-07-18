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

## 12. Sitting 3 — the scripting dialect (2026-07-18)

### 12.1 Ground-rule amendment: the same-semantics wall is scoped
Ink-heritage semantics stay frozen (oracle-guarded, compat forever).
**Brink-native additions (structs, fn values, handles, projections,
typed syntax, sigil literals, await's shape) are revisable during
this design** — zero real authored code depends on their v1
spellings or edges. Corpus updates + changesets are the only tax.

### 12.2 A third design driver: the author's agent
Expected authoring model: **the human writes dialogue; their LLM
agent writes the script code.** The surface must be agent-friendly:
explicit over emergent, one spelling per concept, strict ASTs,
canonical fmt, files that carry their own truth (no inference
ripple across files). This driver retroactively explains most prior
rulings and decides several below.

### 12.3 RustScript
The code dialect's pole star is **Rust, not Lua** (the engine's
language; the maintainer's muscle memory). Adopted: `let` bindings,
`match`, expression-orientation (block value = final expression),
**semicolons** as statement terminators, `name: type` (the TM
syntax verbatim), `fn f(a: int) -> int` (the return arrow cannot
collide with diverts — fn bodies have no story-time). Deliberately
NOT adopted: ownership/lifetimes/borrows (COW value model; `ref`
params stay), traits, generics (§12.9).

### 12.4 Declarations & annotations
- The scope ladder, one keyword each: `var` (world) · `local`
  (per-flow, durable) · `let` (call-transient) · `const`.
  `local x = 5;` — no modifier stacking.
- **The deletable-metadata razor**: grammar for behavior
  (`private`, `local`, `module name`); **annotations `@[name(args)]`**
  for checked metadata — `@[was(courtyard)]`, `@[effects(pure,
  silent)]` — Rust attribute grammar wearing `@[` (the `#` channel
  belongs to tags).

### 12.5 UFCS (ruled, four rules)
`x.foo(a)` ≡ `foo(x, a)` (receiver = first arg, always). Field
beats function (shadow warning). Candidates = lexical scope via
module imports — no type-directed lookup. Typing inherited from
ordinary calls; dot-completion filters by receiver type.

### 12.6 Control flow
`while` / `for x in e` / `loop` / `break` / `continue` in code
contexts. In flow bodies: pure respelling (divert-loop lowering —
the hand-written label+divert idiom). In fn bodies: **flagged
additive** (the expression-time evaluator gains loop support; no
existing program changes; recursion-lowering rejected for its
hidden stack texture). `loop` guarded by the standing step limits.
Iteration protocol over flags/sequences designed in the stdlib
sitting.

### 12.7 Dialect grounds & escapes (completes §8.2)
Defaults: `flow` bodies open in prose ground, `fn` bodies in code
ground. **Two escape glyphs, each active only in the other ground,
at three grains** (line / block / whole-body): `~` = code follows
(`~ stmt;`, `~ { … }`, `flow g() ~{ … }`); `>` = content follows
(`> line`, `> { … }`, `fn f() >{ … }`). Content lines in code carry
interpolation/glue and anchor the #1087 emits effect. Inline grain:
`{expr}` interpolation only (no code→prose inline — a `>` line is
the minimum emission). **Rare dialects get named brace annotations,
not glyphs**: `flow guard(post) chart{ … }` — sigils are earned by
frequency; every future per-domain frontend arrives as a name.

### 12.8 Structs & construction
Rust form: `struct Point { x: float = 0.0, y: float }` (field
defaults RULED in), construction `Point { x: 1.0 }` under Rust's
ambiguity conventions (RULED). **Brace-construction is a language
capability**: `Type { …elements… }` with the element grammar
determined by the type (struct→fields, map→`k: v` pairs, flags→
members; compiler-known for builtins v1; a user-facing protocol
may be earned via the #1090 ledger). Arrays: `[1, 2, 3]`. Maps: a
type we already have that's owed surface (literal + verbs + typed
params), not a new type. Anonymous-record literals (`#{…}`):
**deferred into the stdlib/native-types sitting** — judged against
declared-shapes + map literals once those exist.

### 12.9 Typing regime
**The native surface is statically typed with no gradual tier** —
`Unknown` is a compile error everywhere; gradual remains the ink
dialect's world. Inference-first (the mono-HM substrate carries
it); **committed source always shows explicit signatures**, made
free by **typed holes**: `fn dist(a: _, b: _) -> _` checks against
the inferred types and **fmt materializes them into the source**
(fmt gains an analyzer dependency — first such feature; holes
never survive `fmt --check`). Locals infer (`let`, annotation
optional). Host boundary typed by the manifest. **Expression holes
(`?name`) chartered as the sibling feature** — the author↔agent
handoff primitive (author leaves `?give_reward`; the file checks;
the agent's work queue is greppable); dev-warning vs
release-error decided at its design.
Generics: middle-ground posture, evidence ledger #1090 (candidates
a–e recorded there); mandatory static typing will feed it fast, by
design.

### 12.10 Flows and values
Flows have **no return type in v1** (functions return values;
tunnels return control — semantically honest asymmetry). **Typed
tunnel results are chartered as a named post-FS-3r addition**:
`let x = -> negotiate(stakes) ->;` restricted to statement
position (binds at the continuation-split's resume point — no
mid-expression suspension), with `return expr` filling a typed
slot and flow signatures gaining `-> T` then.

### 12.11 Remaining for the stdlib sitting (the last box)
The verb inventory (heap, floor-div, `char_at` family, weighted
tables), real sequences + the `list` name reclaim, map surface,
the iteration protocol, the #827 vec decision (structs + UFCS +
initializers may suffice), anonymous-record fate, the effects/
holes assertion spellings' final forms.
