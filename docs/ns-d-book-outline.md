# NS-D: the book — proposed outline (issue #1132)

Status: **PROPOSAL — nothing here is ratified.** First deliverable of
NS-D per #1132 ("outline = first deliverable"). Everything below —
structure, titles, classifications, sequencing, conventions — awaits
the maintainer's nod. Authorities cited throughout: the
native-surface charter (§§5, 6, 10c, 11, 13), decision-log
2026-07-19 entries, stdlib-spec §§1–9, typed-mode-spec, effects-spec,
tower-mini-spec.

## 0. Framing

The rewritten book is **native-surface-first**: brink is the
language; ink is the permanently supported compatibility frontend
(charter §1). The current book's spine — "brink is a toolchain for
inkle's ink, plus an opt-in dialect" — inverts. The new spine is the
language itself, taught **by concept, not glyph list** (charter
§10c), **example-led** (charter §6 docs note: the annotated-brace
family gets "a first-class, example-led explanation, not a
footnote").

**Surface-dependence classes** used below:

- **(A) surface-independent** — semantics are Track-A shipped in the
  brink dialect; the chapter can be written NOW with CI-compiled
  examples (NS-A9: brink dialect defaults strict, so examples
  compile under strict).
- **(B) ruled-but-unspelled-or-unimplemented** — semantics are RULED
  (and often the spelling too), but the `.brink` frontend doesn't
  exist yet; draftable now with placeholder spellings, compilable
  only when B-track lands.
- **(C) blocked on unruled design** — names what it waits on.

A proposed **running setting** ties the scenes together: a
cliff-edge inn (working name: *the Last Light*) with an innkeeper,
a guard NPC (the `Phase` enum's home), a `Mood` flags domain
(`calm, wary, hostile` — the charter's own exhibit), a market
downhill (`story::market::{barter, haggle}` — the module ruling's
exhibit), and a dice corner (the rand chapter's home). One world,
reused; each chapter's opening scene is a real scrap of it.

---

## 1. Proposed table of contents

### Part I — Starting out

**1. Introduction** — *class B*
What brink is: a narrative scripting language where prose and code
are both first-class, compiling to one bytecode VM; ink as the
lineage and the permanent compat frontend. The three-axes thesis in
one page (coloring / body dialect / addressing, charter §3) — as
orientation, not theory. First `.brink` sample on page one.
Teaches: charter §§1–3 (framing only). *(B: the page-one sample
wants `.brink`; the identity reframe is a maintainer call — see
findings F9.)*

**2. Installation & the toolchain** — *class A*
Install the CLI, compile, play, the project layout, `brink.toml` in
brief. Teaches: current CLI docs; NS-A9 strict default.

**3. Your first story** — *class B*
A complete small story at the inn: one file, two flows, a choice
point, a variable, a divert — the whole loop from source to
playthrough. Teaches: charter §§4–5 end-to-end at tour depth.
*(B: the walkthrough should be `.brink`; a brink-dialect interim
version is possible but would be rewritten wholesale.)*

### Part II — Telling stories (the prose dialect)

**4. Flows and the shape of a story** — *class B*
`flow` declarations, braced bodies, nesting (`garden.gate` —
stitches are nested flows), many declarations per file, addressing
falls out of structure, `fn` vs `flow` coloring taught as
story-time vs expression-time. Teaches: charter §§3–4.

**5. Choices and choice points** — *class B*
The `{?` block: `*`/`+` lines, braced choice bodies, `[]`
display-split, labels `* (name) [text]`, conditional guards
`* {if cond}`, `else` as the fallback, the splice `<- flow(args)`.
And the chapter's centerpiece: **the gather is dissolved** — "after
the choices" is simply the next line after the block. Written for
two audiences at once: the cold reader (who never learns gathers
existed) and, in one clearly-marked sidebar, the ink author (who
needs the funeral). Teaches: charter §5, sitting-2 fallback ruling.
*(C sub-item: the flat-choice-run compact spelling, charter §10
caveat (a) — the chapter notes it as open, teaches the braced
form.)*

**6. Interpolation, branching, and alternation: the annotated-brace
family** — *class B*
The maintainer-flagged first-class chapter (charter §6 DOCS NOTE).
One brace grammar, annotation declares the kind: bare `{expr}` is
interpolation and nothing else ever; `{if …: … else: …}` and
`{match …:}`; the alternation characters (`{~ } {& } {! } {| }` —
spelling tentative); entry markers (`-` opens an arm; multiline
anatomy taught with a full worked scene, since this construct "was
under-understood even by the implementer" in ink's spelling).
Teaches: charter §6. *(C sub-item: alternation chars final call,
charter §8.4.)*

**7. Movement: diverts, tunnels, and return** — *class B*
`->` kept verbatim (args, `-> END`/`-> DONE`, targets-as-values);
tunnels `-> place ->`; **`return`** as the one leave-this-container
word, and `return -> x` as the honest spelling of ink's `->->x`.
Teaches: charter §11 (diverts/tunnels/return rulings).

**8. Lines, tags, speakers, and glue** — *class B*
Tags, the speaker idiom, `<>` glue and when to reach for it,
`TODO:` and comments. Teaches: charter §11 (kept items). *(C
sub-item: the Fountain-lineage dialogue dialect and `@SPEAKER:<>`
are a future sitting — the chapter names the idiom, defers the
dialect.)*

**9. Mixing prose and code** — *class C — blocked on the
interleaving sitting (charter §8.2)*
Prose→code beyond interpolation; code→prose emission; grains. The
chapter slot is reserved; only interpolation (ch. 6) is teachable
today.

### Part III — Working with values (the code dialect)

**10. Values and types** — *class A*
int/float/bool/string, inference (declared at boundaries, inferred
internally), strict typing as the language's posture (native is
strict-only; the brink dialect defaults strict per NS-A9), the
coercion story, `Unknown` at the ink seam. Substrate: the TM-era
Types chapter (#621) — this is its rewrite, not a new start.
Teaches: typed-mode-spec §§1–5, 2026-07-19 typing-posture ruling.

**11. Functions, `ref`, and lambdas** — *class B (large A core)*
`fn` declarations, params/returns, `ref` parameters and path
projections (`ref npc.hp` — A, shipped), UFCS (`x.foo(y)` ≡
`foo(x, y)`, companion-first lookup — "namespace lookup, not
dispatch"), lambdas (`|g| g.awake`, colon returns, by-value
capture always, captured-binding assignment is a compile error).
Teaches: charter §7 (RustScript north star), lambda ruling
2026-07-19, path-projections material (kept). *(B because the
lambda/UFCS spellings are native; ref/projections sections are A.)*

**12. Structs, enums, and flags** — *class B (struct section A)*
Declared structs (`TypeName { … }` construction, defaults);
`enum` — one-of, named-field payloads, exhaustive `match`, unit
variants as map keys, `Phase.Patrol` dot access; `flags` — many-of,
ordered symbol domain (`flags Mood = (calm), wary, hostile`), the
verb surface (contains/count/first/last/next/prev/index_of), the
numeric coupling frozen on the ink side. Teaches: charter §13.1,
typed-mode-spec §6, stdlib-spec §6. *(C sub-item: the initializer
protocol-vs-grammar question, #1103.)*

**13. Collections: arrays, maps, and ranges** — *class A*
Array and map literals, `[T]`/`[K: V]` typing, indexing contracts
(`a[i]` faults OOB — an index is a claim; `m[k]` read-faults /
write-inserts; `remove` is a wish, total), the verb surfaces,
ranges `0..n`/`0..=n`, views as a performance contract (one
sidebar). Teaches: stdlib-spec §§4–5, 3b. *(Current spelling:
`#[…]`/`#{…}` sigils via the callout convention, §4 below.)*

**14. Option and absence** — *class A*
The doctrine chapter: **a fault says "your program is wrong";
Option says "the world didn't have one."** `some`/`none`,
`x or default`, no truthiness (F27 — explicit `== none` /
`== some(x)`), `find`/`get`/`min`/`max` returning Option,
`filter_map`. The display-boundary forgiveness taught as *coming
with the native surface* (F28: total render until B4). Teaches:
stdlib-spec §1.1/§1.4/§1.6, F27/F28 rulings.

**15. Iteration** — *class A*
`for` over the closed iterable set; `for k, v in m` (snapshot
semantics, F10); `for ref x` mutating iteration; the pure trio
`map`/`filter`/`fold` (pure·silent callbacks required, "one
logical pass, order unobservable"); `each`/`map_each` and the
naming law — **the weird thing gets the ugly method**. Teaches:
stdlib-spec §4, trio ruling. *(Tension: trio examples want lambdas
(B); until then, brink-dialect function values spell the callbacks
— findings F10.)*

**16. Ordering, sorting, and comparing** — *class A*
What's orderable and what isn't; `sort_by`/`sorted_by` (in-place vs
functional twins, F0); min/max/heap; the NaN doctrine — dev faults,
prod pins a total order, the fence ("placement qualifies;
fabrication never does"); compare-vs-equality coherence (sort never
implies dedup); operators stay frozen IEEE. Teaches: stdlib-spec
§4b, ordering-doctrine ruling, tower T4 (not orderable).

**17. Effects: rows, purity, and `@[effects]`** — *class A*
Every definition has a row (reads/writes/emits/calls/faults); you
never write one, the compiler infers; `@[effects(…)]` as the only
contract — paren-clause grammar (`reads(gold, hp)`), exceedance-only
errors, `pure`/`silent`/`total`; why positions demand rows (wake
conditions, trio callbacks, display impls). Rewrite of the existing
Effects chapter (already example-led) onto the final spelling.
Teaches: effects-spec §10, stdlib-spec §9.2, A2 amendment. *(C
sub-item: the `emits` extension #1087 — own mini-sitting.)*

**18. Randomness and determinism** — *class A*
The rng cell (every draw is a write in the row), seeded replay as a
stability contract, `rand::int` and **NonEmptyRange** — the
language's first value refinement, taught as parse-don't-validate
("pay the Option once at the boundary"); `pick`, `chance` (clamps,
NaN→false), `Weighted { … }` + `rand::roll` (evidence-by-
construction), shuffle. Teaches: stdlib-spec §§7–8, rand ruling,
S2 naming ruling.

**19. The numeric tower** — *class A (gated on A8 landing)*
vec2/3/4, quat, mat2/3/4; componentwise ops, `mat * vec`,
`quat * quat`, `dot`/`cross`; the scalar kit as the tower's width-1
floor; equality componentwise-IEEE, not orderable; saves carry
lanes, not memory layouts (one honest paragraph on why). Teaches:
tower-mini-spec T1–T5, stdlib-spec §2b.

**20. Modules, imports, and companions** — *class B*
The filesystem-derived tree (`story::` root, path on disk = path in
language), `::` crosses module walls / `.` walks everything inside,
**the tree is the compilation universe; imports are naming only**,
`use` lifted from Rust (tooling types it, not you), engine-only
roots, `host::`, impl blocks as companion-module sugar
(`Npc::greet` is the only actual name; `self` as first-param
sugar), the casing partition. Teaches: charter §13.2, S4 ruling.
*(C sub-items: cross-module impl coherence (parked); `host::`
mounting details (stdlib-sitting docket).)*

**21. Protocols: display, compare, iterate** — *class B*
The closed registry; `impl display for Npc { … }`; per-protocol
effect contracts; one display path (interpolation and `string()`
both dispatch through it, F1); reserved method names (F6);
pull-shaped `iterate` and why (`for` desugars inline; iterators
park across suspensions). Teaches: stdlib-spec §9.6, S1/S4
rulings. *(B: semantics are A-grade, but every example needs the
native impl-block spelling.)*

**22. Text and math** — *class A*
The two "workhorse" domains in one chapter: the text verb surface
(USV characters, casing posture, `find → Option`), the math kit
(NaN-totality, `div_floor`/`mod_floor` as the blessed grid verbs),
the prelude (what's ambient and why, shadowing warning). Teaches:
stdlib-spec §§2–3, closers §9.3.

### Part IV — Running stories (the host view)

**23. The execution model** — *class A (keep, refreshed)*
The step function, `Line` variants, the consumer loop. Substrate:
existing concepts/execution-model.md.

**24. State, saves, and sessions** — *class A (keep, refreshed)*
Execution-local vs durable state, named flows, sessions/replay,
speculation. Substrate: existing state-model + embedding chapters.

**25. Suspension and waiting** — *class C — blocked on the `await`
native spelling (charter §8.3)*
Flow suspension, wake conditions (pure·silent·total gates), the
engine's role. Semantics have a draft home (flow-suspension-spec);
the authorial spelling is unruled. Slot reserved.

**26. Localization** — *class A (keep)*
The `.ink`→`.inkb`→XLIFF pipeline, plurals, locale switching.
Substrate: existing localization chapters.

### Part V — Integrations

**27. Embedding in Rust** — *class A (keep)*
Loading/linking, external functions, the runtime API. Substrate:
existing embedding chapters.

**28. Bevy** — *class A (keep + grow)*
Plugin, assets, flows-as-entities, bindings, handles, saves. Grows
a host-effects section (trust tiers, ECS scheduling from rows) as
the T2 implementation lands. Substrate: existing bevy chapters;
effects-spec §§12–13. Spec home stays docs/bevy-brink.md (findings
F2).

**29. Web, Studio, and the playground** — *class A (keep)*
Substrate: existing web/studio chapters.

### Part VI — Compatibility & reference

**30. The ink frontend** — *class B (converter story C)*
Ink as the permanently supported compat frontend: what it is, when
you're on it, the two kinds of "correct" (oracle conformance vs
dialect semantics), gradual typing as the compat posture, the
boundary doctrine (ink symbols enter native code as Unknown;
annotate at the seam). Substrate: dialect/conformance.md +
dialect/enabling.md. *(C sub-item: ink↔native converters and mixed
trees — charter §8.5 remainder.)*

**31. Appendix: brink for ink authors** — *class B*
The concept map, one page per move: gathers → structure; `LIST` →
`flags` (+ enums for "symbol with data"); threads → the scoped
splice; `->->` → `return ->`; `* ->` → `else`; tildes → the code
dialect. Written for the migrating reader; the main text never
depends on it.

**32. Appendix: syntax at a glance** — *class B*
The one glyph table — deliberately an appendix, so the *chapters*
teach by concept (§10c) and the *lookup need* is still served.
Every row links back to the owning chapter.

**33. Reference** — *class A (keep as a cluster)*
CLI (compile/convert/play/ide), `brink.toml`, runtime API, bytecode
& opcodes, binary format, containers & DefinitionId, line
templates, errors. Substrate: existing cli/ + reference/ chapters,
kept with flag updates (NS-A9 default).

**34. Contributing** — *class A (keep)*
Crate layout, workflow, test corpus. Substrate: existing
contributing chapters.

**Tally (chapter-level):** 34 entries — **A: 17** (2, 10, 13–19,
22–24, 26–29, 33, 34 — counting the two clusters once each),
**B: 13** (1, 3–8, 11, 12, 20, 21, 30–32), **C: 2 full chapters**
(9, 25) plus named C sub-items inside 5, 6, 8, 12, 17, 20, 30.

---

## 2. Migration map (every existing chapter)

Legend: **keep** = survives with at-most flag/link updates ·
**rewrite** = same slot, new content · **fold** = content absorbed
into a listed chapter, file retired · **retire** = no successor
beyond compat-appendix mention.

| Existing file (docs/book/src/…) | Disposition | Reason |
|---|---|---|
| introduction.md | **rewrite** → ch. 1 | "toolchain for ink" framing inverts to native-language-first |
| toolchain/installation.md | **keep** → ch. 2 | tooling is surface-independent |
| toolchain/first-story.md | **rewrite** → ch. 3 | walkthrough respells in `.brink` |
| toolchain/project-config.md | **keep** (update) → ch. 33 | NS-A9 default flip; dev/prod knob home when shipped |
| toolchain/cli/index.md | **keep** → ch. 33 | accurate |
| toolchain/cli/compile.md | **keep** (update) → ch. 33 | `--types` default text stale post NS-A9 |
| toolchain/cli/convert.md | **keep** → ch. 33 | accurate |
| toolchain/cli/play.md | **keep** → ch. 33 | accurate |
| toolchain/cli/ide.md | **keep** → ch. 33 | accurate; grows native-frontend material with NS-T |
| toolchain/embedding/index.md | **keep** → ch. 27 | host-facing, surface-independent |
| toolchain/embedding/loading.md | **keep** → ch. 27 | ditto |
| toolchain/embedding/external-functions.md | **keep** → ch. 27 | ditto |
| toolchain/embedding/named-flows.md | **keep** → ch. 24 | ditto |
| toolchain/embedding/sessions.md | **keep** → ch. 24 | ditto |
| toolchain/embedding/speculation.md | **keep** → ch. 24 | ditto |
| toolchain/localization/overview.md | **keep** → ch. 26 | pipeline unchanged |
| toolchain/localization/xliff.md | **keep** → ch. 26 | ditto |
| toolchain/localization/plurals.md | **keep** → ch. 26 | ditto |
| toolchain/dialect/index.md | **fold** → ch. 1 + ch. 30 | the "opt-in dialect" framing dissolves; ink becomes the compat chapter |
| toolchain/dialect/enabling.md | **fold** → ch. 30 + ch. 33 | dialect selection becomes compat + config material |
| toolchain/dialect/blocks.md | **retire** | `~ { … }` is transitional scaffolding; braces are ground on native — compat appendix mention only |
| toolchain/dialect/literals.md | **fold** → ch. 13 | concepts keep; `#[…]`/`#{…}` sigils become current-spelling callouts, then compat notes |
| toolchain/dialect/indexing.md | **fold** → ch. 13 | same contracts, one collections chapter |
| toolchain/dialect/stdlib.md | **retire** | slice-1 snapshot; superseded by chs. 13–19, 22; its "no method syntax" contradicts the UFCS ruling |
| toolchain/dialect/types.md | **rewrite** → ch. 10 | the #621 substrate; "gradual (the default)" superseded by NS-A9; native strict-only posture |
| toolchain/dialect/function-values.md | **rewrite** → ch. 11 | "never 'closure' or 'lambda'" framing superseded by the lambda ruling on native; partial-application story becomes a compat note |
| toolchain/dialect/effects.md | **rewrite** → ch. 17 | structure keeps (already example-led); `#@effects` colon spelling → `@[effects(…)]` paren clauses (E110 deprecation) |
| toolchain/dialect/path-projections.md | **fold** → ch. 11 | shipped and correct; belongs with `ref` in the functions chapter |
| toolchain/dialect/modules.md | **rewrite** → ch. 20 | native module system is a different design: filesystem-derived tree, imports naming-only, INCLUDE dead, companions |
| toolchain/dialect/conformance.md | **rewrite** → ch. 30 | becomes the ink-frontend chapter's conformance spine |
| toolchain/concepts/index.md | **keep** → part IV front | accurate |
| toolchain/concepts/execution-model.md | **keep** → ch. 23 | accurate |
| toolchain/concepts/state-model.md | **keep** → ch. 24 | accurate |
| toolchain/concepts/architecture.md | **keep** → ch. 34 area | accurate |
| toolchain/concepts/pipeline.md | **keep** (update) → ch. 34 area | gains the second frontend + HIR admission contract when B0 lands |
| toolchain/reference/* (7 files) | **keep** → ch. 33 | reference material, surface-independent |
| integrations/bevy/* (5 files) | **keep** → ch. 28 | accurate; grows host-effects section with T2 impl |
| integrations/web/* (3 files) | **keep** → ch. 29 | accurate |
| integrations/studio/index.md | **keep** → ch. 29 | accurate |
| contributing/* (3 files) | **keep** → ch. 34 | accurate |

**Counts: keep 30 · rewrite 7 · fold 5 · retire 2** (44 content
files).

---

## 3. Sequencing proposal

**Wave 0 — truth-sync patches to the live book (immediate, before
any new chapter).** The book must never contradict a RULED spec
section, and today it does in three places: `types = gradual "(the
default)"` (NS-A9 flipped it), the `#@effects` colon spelling
(superseded, E110-deprecated), and any falsy-`none` example (F27
superseded the shipped behavior). Small surgical PRs, not rewrites.
This is also findings F5.

**Wave 1 — the class-A code-dialect core, in dependency order:**
ch. 10 (Values & types — the #621 rewrite) → ch. 13 (Collections)
→ ch. 14 (Option) → ch. 15 (Iteration) → ch. 16 (Ordering) →
ch. 17 (Effects rewrite) → ch. 18 (Randomness) → ch. 22
(Text & math) → ch. 19 (Tower, as A8 lands). Rationale for
dependency order over reader order here: Option's doctrine is
load-bearing for Iteration and Ordering; Effects needs the trio's
purity vocabulary; Randomness needs Effects (draws are writes) and
Option (pick). Every chapter ships with CI-compiled brink-dialect
examples under strict.

**Wave 2 — class-A keeps and refreshes (cheap, parallelizable):**
Part IV/V refreshes, reference updates, ch. 2. Any-time work;
wave-able chapter-per-issue.

**Wave 3 — class-B drafts, paced with B-track and NS-T:** Part II
(chs. 4–8) plus 11, 12, 20, 21, 30–32, drafted with the
`proposed`-fence convention (§4). The reader-journey order (prose
before code) and the writing order (code before prose) disagree —
deliberately: Part II's syntax isn't real yet, and drafting it
*ahead* of the parser is exactly how the docs and the syntax ratify
together (#1132).

**Where the friction journal plugs in (charter §10):** two loops.
(1) *Writing-time*: every chapter's examples are authored fresh
against the spec, never pasted from it; every point where the
author of the chapter has to re-read a ruling twice is journaled —
that's spec-clarity signal even before syntax exists. (2)
*Reading-time (the designated cold reader)*: once B0 + the NS-T
editor exist, the writer cold-reads Part II drafts in the rendered
editor and authors scenes from them; each confusion is a journal
entry that feeds *both* chapter revision and syntax ratification.
Class-B chapters are not ratifiable before loop 2 has run on them —
same exit criterion as the syntax itself.

---

## 4. Voice & format conventions (proposed)

**Register.** The book teaches; the spec rules. Prose is direct and
concrete, examples first, doctrine named in the author's own terms
("an index is a claim, a deletion is a wish"; "the weird thing gets
the ugly method") — the rulings' language is often already the best
teaching line, and reusing it verbatim keeps book and spec in one
voice.

**Chapter shape (every teaching chapter):**
1. **A scene** — a small, real situation from the running setting,
   with a complete compiling example, before any terminology.
2. **The concept** — what it is, why it's shaped that way, the
   doctrine in one line. Concept per section; never organized by
   glyph (§10c). Glyphs are introduced as spellings *of* concepts.
3. **The details** — reference tables: verb surfaces with full
   effect rows, signatures in the ruled pseudo-generic display
   notation with the standing banner (*display notation; `T` is
   not writable in source* — closers §9.4).
4. **Where this is ruled** — a closing cite box: spec file +
   section + decision-log date. The book **cites, never
   re-derives**; if book and RULED spec ever disagree, the book is
   wrong by definition — fix or escalate, never paper over.
   DRAFT/🔶 spec content is never taught, at most trailed ("under
   design").

**Example testing (extending the existing Book CI job):**
- ` ```ink ` fences: compiled AND run under `Dialect::Brink`,
  strict (NS-A9), via the harness pattern the Types/Function
  Values/Effects chapters already use
  (`book_*_examples.rs`); a following ` ```text ` block is
  byte-matched output.
- ` ```ink,error ` fences: must fail with the named diagnostic
  code.
- ` ```rust ` via `mdbook test` doctests; ` ```ts ` via
  `ts-check` — both unchanged.
- **New fence class for class-B placeholder spellings:**
  ` ```brink,proposed ` — rendered with a standing "proposed
  spelling — not yet compiled" callout, excluded from compile
  checks. When the native frontend lands, the flip from
  `brink,proposed` to CI-checked ` ```brink ` is done per-chapter
  and *is itself the B-track acceptance test for that chapter's
  examples*. No unmarked non-compiling example may exist in the
  book, ever.
- Proposed infra task: generalize the three per-chapter
  `book_*_examples.rs` tests into one walker keyed on fence info
  strings (findings F6), so new chapters get CI coverage by
  convention instead of by new test file.

**The transitional-dialect convention ("current spelling" box):**
class-A chapters teach the *concept* in dialect-neutral prose and
show *compiling brink-dialect examples*; where the ruled native
spelling differs from the brink-dialect spelling (e.g. `#[…]` vs
`[…]`, function values vs lambdas), a standard callout box —
**Current spelling** — shows the compiling form and names the ruled
future form in one line. Class-B chapters invert: `.brink`-first
with `brink,proposed` fences. Full options analysis in findings F4.

**Locale of truth (summary of the rules above):** semantics —
spec + decision log rule, book cites; spellings — ruled charter
spellings in class-B prose, compiling dialect spellings in class-A
examples; behavior — every claim about behavior is backed by a
CI-run example or a spec cite, preferably both.
