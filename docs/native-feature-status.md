# Native surface — feature status board

**As of 2026-08-06**, against `origin/main`.

A per-feature checklist for the `.brink` native surface. It exists because
"is X supported?" has four different answers on this project and collapsing
them has repeatedly produced wrong conclusions — including, on 2026-08-01,
a review that reported a feature working because it *compiled clean* while
it silently discarded the author's intent at runtime.

## The four columns, and why there are four

| Column | Question | How it is established |
|---|---|---|
| **Ruled** | Is the design settled? | `docs/decision-log.md`, `native-surface-charter.md` §8, area specs |
| **Parses** | Does the grammar accept it? | `brink-syntax-native`; a parse error or `E037` means no |
| **Runs** | Does it *do the right thing*? | `brink compile` **then `brink play`** — compiling clean is NOT sufficient |
| **Round-trips** | Can the emitter re-spell it? | `brink-respell`'s `full_corpus_sweep`; `EmitError::Unsupported` means no |

⚠ **"Runs" must be checked by running.** Three of the gaps below compile with
zero diagnostics *under `-D warnings`* and then print the author's code to the
player as story text. A `brink compile`-only check calls those features
working.

**Legend:** ✅ works · ⚠️ partial / caveat · ❌ broken or absent · 🔒 ruled but
unimplemented · ❓ unverified (nobody has checked; do not assume either way)

---

## Code dialect (`fn` bodies, `~{ }` blocks)

| Feature | Ruled | Parses | Runs | Round-trips | Notes |
|---|---|---|---|---|---|
| Expressions & operators | ✅ | ✅ | ✅ | ✅ | charter §8.1 |
| `let` / temp declaration | ✅ | ✅ | ✅ | ✅ | |
| Assignment | ✅ | ✅ | ✅ | ✅ | in **code** position only — see prose table |
| `if` / `else` | ✅ | ✅ | ✅ | ✅ | |
| `else if` chains | ✅ | ✅ | ✅ | ✅ | round-trips since **#1975** (PR #2041) |
| `for` loops | ✅ | ✅ | ✅ | ✅ | incl. `for k, v` |
| `return` with a value | ✅ | ✅ | ✅ | ✅ | in **`fn`** bodies; prose position is #1973 |
| Lambdas | ✅ | ✅ | ✅ | ✅ | lifting landed (#1709) |
| UFCS method calls | ✅ | ✅ | ✅ | ✅ | |
| `as` binding | ✅ | ✅ | ✅ | ✅ | |
| Array & map literals | ✅ | ✅ | ✅ | ⚠️ | list-literal *expressions* fail, 8 cases |
| Construction literals | ✅ | ✅ | ✅ | ✅ | |
| `or` coalescing | ✅ | ✅ | ✅ | ✅ | |
| Fn values by bare name | ✅ | ✅ | ✅ | ✅ | ruled 2026-08-01 (#1862) |
| Fn value in `var`/`const` | ✅ | ✅ | ✅ | ⚠️ | **LANDED #1774** (PR #2084) |
| `ref` params | ✅ | ✅ | ⚠️ | ✅ | variance unsound until **#1995** lands |

## Prose dialect (`flow` bodies, `>{ }` blocks)

| Feature | Ruled | Parses | Runs | Round-trips | Notes |
|---|---|---|---|---|---|
| Prose lines & interpolation | ✅ | ✅ | ✅ | ✅ | |
| Diverts `->` | ✅ | ✅ | ✅ | ⚠️ | divert-target-as-**value** fails, 18 cases |
| Choices | ✅ | ✅ | ✅ | ⚠️ | inline conditional in choice content, 21 cases |
| Conditionals in content `{if …}` | ✅ | ✅ | ✅ | 🔒 | emitter gap; **#1737 ruled 2026-08-02** — lifter recurses into spans |
| Tags `#` | ✅ | ✅ | ✅ | ✅ | markup inside a tag is **literal** (ruled #1783) |
| Markup spans `<b>…</b>` | ✅ | ✅ | ✅ | ✅ | `.inkb` **v6** `PART_SPAN` |
| Hyphenated span names `<fade-in>` | ✅ | ✅ | ✅ | ✅ | landed **#1996** |
| `@[convention(claims=…, order=N)]` handlers | ✅ | ✅ | ✅ | ❓ | `annotations-element` golden |
| Prose-bodied `fn` via `>{ }` | ✅ | ✅ | ✅ | ❓ | verified 2026-08-01: emits `[A] hi` |
| **Statements at prose position (bare, no `~`)** | ⚠️ | ❌ | ❌ | ❌ | **🔴 SILENT** — see below; by design, not open (charter §8.2) |
| `~ stmt`/`~{ }` line/block escape (assignment/bare-call/temp-decl/`until`/logic block) | ✅ | ✅ | ⚠️ | ⚠️ | #1991 + #1972 (both slices); Round-trips only leaf stmts, nested if/while/for inside `~{ }` is an emitter-only gap; Runs — the call-only-escape output-boundary fix (review finding, w111) and its whole-body-override/`fn`-default sibling (`lower_stmt_block_as_body`/`flush_code_ground_run`, **#2056**) both append a boundary, but only ONE per flushed *run*: `lir::lower::blocks::lower_block_stmt_list` still has no per-statement `EndOfLine` tracking, so two emitting calls back to back in the same run (e.g. `flow main() ~{ shout(); shout(); > x }`) still glue as `HiHi` — same for a call inside an `if`/`while`/`for` followed by another emitting call in that same run (review finding F3, #2063) |
| `> text` line escape in code body | ✅ | ✅ | ❌ | ❌ | 🔒 `E129` — #1992 |
| `return <value>` at prose position | ✅ | ❌ | — | — | #1973, 16 cases |
| Alternations `{~ {& {! {\|` | ✅ | ✅ | ✅ | ❌ | emitter gap, 17 cases |
| Inline sequences | ✅ | ✅ | ✅ | ❌ | emitter gap, 10 cases |
| Thread splice `<- flow(args)` | ✅ | ✅ | ✅ | ⚠️ | **#1974** re-nesting landed; bucket 21 → 17 |
| Scene headings / cues / parentheticals | ✅ | ✅ | ❌ | ❌ | parse (#1715) but produce **no HIR** (`E129`) |
| Word-break spring | ✅ | — | — | ⚠️ | **RULED internal 2026-08-02** — no native spelling owed; respell/emitter only |

## Conventions & elements

The machinery that makes the prose dialect *authorable as a screenplay*:
patterns claim lines, handlers receive the captures, and a project swaps the
whole vocabulary by naming a different conventions module. Sequenced as
**v1a → v1b → v1c** (#1838 → #1839 → #1840).

| Feature | Ruled | Parses | Runs | Round-trips | Notes |
|---|---|---|---|---|---|
| `@[convention(claims=…, order=N)]` pattern claiming | ✅ | ✅ | ✅ | ❓ | **v1a landed** — `annotations-element` golden |
| `@[element(args=…)]` capture binding | ✅ | ✅ | ✅ | ❓ | named captures bind params by name |
| Prose-bodied handlers `>{ }` | ✅ | ✅ | ✅ | ❓ | verified 2026-08-01 — emits `[A] hi` |
| Typed handler params (`E171`) | ✅ | ✅ | ✅ | — | #1849 closed |
| `attach = StructName` schema (`E180`) | ✅ | ✅ | ✅ | — | issue #2178 (split from #2164 item 2) — parses, validates against the handler's own return type, and runs (a claiming handler's real struct value renders via its default `Display`); `brink_db::ProjectDb::conventions_projection` (#2111) reads `ClaimHandlerDecl::attach` and resolves it to the struct's own field list + types via the conventions module's import closure — still no `.inkb`/`StoryData` emission (that consumer, #2108's host binding join, is unbuilt) |
| Confinement to one module (`E169`) | ✅ | ✅ | ✅ | — | query landed; **now reachable from live typing** — #1880 closed by #2316 (`brink-web`'s `EditorSession`, `brink-lsp`'s `analysis_loop`) and #2317 (`brink-cli`'s `Project::ide_session`), the third and last producer |
| Directive-shaped tag guard (`E172`) | ✅ | ✅ | ✅ | — | landed 2026-08-01 (#1835) |
| **`!name` sigil dispatch** | ✅ | ❌ | — | — | **reserved, unimplemented** — see below |
| Block elements `@[element(…, block)]` | ✅ | ✅ | ✅ | ❓ | **LANDED #1839** (PR #2067) — capture stops at a line carrying a divert/label |
| `fn conventions()` registration | — | — | — | — | **DISSOLVED** (2026-08-03 ruling) — the well-known registration fn, `register` intrinsic, and #1840's entire Q1–Q6 comptime chain are removed from the design; precedence is now a declared `order = N` property directly on `@[convention]`/`@[element]` (split from #2164, landed via PR #2176) — no comptime involved |
| Comptime evaluation of conventions | — | — | — | — | **DEFERRED, not blocked** — moot for conventions specifically since ordering no longer needs it; comptime as a general capability remains wanted but undesigned, to be decided when something genuinely needs it |
| `@[style]` declaration surface | ✅ | ✅ | ⚠️ | — | `StyleToken` produced (#1719) and now **read by one consumer**: `style_hover_text` (`brink-ide/src/style_hover.rs`), wired into `hover()` and reached via CLI/LSP/web — issue #2116, closed 2026-08-03 via PR #2069. No CSS class / CM6 decoration rendering yet, matching #2116's own scope fence |
| Built-in screenplay preset | ✅ | ✅ | ✅ | ❓ | **SHIPS** as `std/conventions/screenplay.brink` (#1720/PR #2081) + `scene_entered` extern (#2092), and **#2080 mounts its source into every compiled `Environment`** — **but still not importable**: nothing in it is `pub` and no confinement rule scopes a `use` into it yet (needs #1582's pub marker + #2167's confinement) |
| `[project] conventions` name validation | ✅ | ✅ | ✅ | — | **LANDED #1874**; key renamed from `elements` by **#2180** (deprecated alias still accepted, warns) |
| `std::conventions` types | ❓ | ❌ | — | — | prose-spec §9 residual — the last prose-round design item |

## Editor side — how the author interrogates a claimed line

Under conventions, a prose line silently becomes a function call. The ruled
compensation — **"no invisible expansion", a stated maintainer requirement** —
is that the editor can always show which handler claimed a line, why, and what
it bound. **#2006's six slices are all now closed (2026-08-03 through
2026-08-06), and the compiler-side half of that compensation is a property,
not just a promise, for most rows below** — classification, explain-match, the
projection, `@[style]` consumption, and the harvest index all exist and are
exercised by real callers. What is still a promise: a hover consumer that
surfaces the handler body, CM6 decoration rendering of capture spans, and the
`IdeSession`/`EditorSession` wiring gap that keeps a live editor project from
reaching its own configured conventions at all (see the two remaining ❌ rows).

Tracked as **#2006**.

| Feature | Ruled | Built | Notes |
|---|---|---|---|
| Per-line classification metadata | ✅ | ✅ | **LANDED #2112** (PR #2257, 2026-08-04) — `classify_line` records matched handler, captures, disposition, and shadowed matches |
| Explain-match query | ✅ | ✅ | **LANDED #2113** (PR #2309, 2026-08-06) — `brink_ir::explain_match`/`ExplainMatchCache`, composing #2112's walk and #2111's projection; wasm binding `EditorSession::explain_match`/`explain_match_doc` |
| Hover shows the handler body | ✅ | ❌ | the record a hover consumer would read now exists (#2112/#2113), but no hover call site reads it yet — `crates/internal/brink-ide/src/hover.rs` has no reference to `element_matches`/`ClassifiedMatch` |
| Capture spans as decoration ranges | ✅ | ❌ | the spans themselves are produced (explain-match's `captures`, raw byte ranges), but nothing renders them as editor decoration — CM6 wiring is the missing half |
| Harvest index (cues, span kinds) | ✅ | ✅ | **LANDED #2114** — project-wide, sibling of the symbol index |
| Succession rules (Tab/Enter) | ✅ | ⚠️ | slice #2115 — validator-only: `ConventionsProjection::with_succession`/`dialect::validate_succession` re-key `DialogueDialect`'s surviving `transitions`/`templates` off the projection's own convention kinds and validate them in-process; per the 2026-08-05 ruling *"Succession is EDITOR-OWNED and externally defined"* (PR #2304) there is no `.inkb`-codec wire mirror — these fields never travel beyond tooling; actually wiring Tab/Enter in CM6 stays held (editor-frontend, NS-T hold) |
| Serialized conventions projection | ✅ | ✅ | **LANDED #2111** (PR #2212, 2026-08-04) — salsa-tracked `conventions_projection_query`; the old "behind #1840" note no longer applies — #1840's `fn conventions()` comptime chain was dissolved (issue #2165, below), and `order` reads straight off the `@[convention]` annotation. **Extended #2352** (2026-08-21) — a second `dispatch` list now carries every `!name`-sigil (`@[element]`) handler too, not just `@[convention]` claiming handlers; known limitation left open pending a ruling: only the ONE configured conventions-module file is read, so a `!name` handler declared elsewhere (the common case) projects no row — see `docs/prose-dialect-spec.md` §5.3 |
| Last-good caching on comptime fault | — | — | **DISSOLVED** — the mechanism this ruling (Q2, 2026-08-01) guarded against — comptime-evaluating `fn conventions()` — was itself removed by the 2026-08-03 dissolution (issue #2165). Every `ConventionProjectionEntry` field is now a pure, total read off HIR; there is no VM execution in the path and so no fault case left to cache against (`brink_ir::hir::types`'s own "why there is no comptime-fault / last-good-value case here" doc) |
| `@[style]` consumption | ✅ | ✅ | **slice #2116, closed 2026-08-03** — delivered by PR #2069 (which predates the slice's own filing): `style_hover_text` (`brink-ide/src/style_hover.rs`) reads `StyleToken`/`StyleAnnotation` and is wired into `hover()`, reached by the CLI, LSP, and web hover call sites alike. No CSS class / CM6 decoration is produced (that half stays out of scope per the issue's own scope fence) |
| Elements reach `IdeSession` | ✅ | ✅ | #1880 is **CLOSED** (delivered across #2316 and #2317). All three known `IdeSession`/parallel-config producers now forward `[project] conventions`: `brink-web`'s `EditorSession::apply_parsed_config`, `brink-lsp`'s `analysis_loop`/`LanguageOptions`, and `brink-cli`'s `Project::ide_session`. The live regression this row tracked — `conventions_unconfigured_diagnostics` (`conventions_confinement.rs:144-158`) misfiring on every claim handler whenever the pointer was unset, since #2289 — is fixed on all three producer paths. The projection/explain-match queries (this section's own rows above) reach their real configured value rather than a permanently-empty default. **Issue #2334 (this PR)** replaced the per-producer hand-copied setter calls with one shared seam, `IdeSession::apply_analysis_options` (`crates/internal/brink-ide/src/session.rs`) — `brink-cli`'s `Project::ide_session` now calls `session.apply_analysis_options(db.analysis_options())` (`crates/brink-cli/src/ide/project.rs:685`) instead of naming `set_conventions` directly, so a future `AnalysisOptions` field can no longer be forwarded by one producer and dropped by another |
| Match ordering | ✅ | ✅ | **LANDED #2112** — the walk tries every registered pattern, uses the first match, records the rest as shadowed |
| Editor re-evaluation loop | ✅ | ✅ | **LANDED** — the projection is a salsa-cached query keyed on the conventions module's import closure (#2111); per-line classification/explain-match is memoized on `(line text, projection)` (#2113's `ExplainMatchCache`) |

⚠ **Both rulings from the §9.1 sitting are ruled and now implemented, not
owed.** Match ordering and the editor re-evaluation loop were both ruled
2026-08-02 (`docs/decision-log.md`) and both landed via #2111/#2112/#2113
(rows above). The one dependency that *was* live when this section was first
written — Q2's last-good caching leaning on "the owed re-evaluation loop" —
is now moot rather than resolved: the 2026-08-03 dissolution removed the only
mechanism (`fn conventions()` comptime evaluation) that could ever fault, so
there is nothing left for a last-good value to guard (see the row above).

⚠ **Sequencing note.** NS-T (#1131) was held behind the compiler work by
deliberate choice, 2026-08-01 through 2026-08-05. This seam is **compiler-side**
— queries emitted from `brink-db`/`brink-ide` — so per the 2026-08-01 scoping
it was never actually covered by that hold; and per the 2026-08-05 maintainer
ruling ("The compiler-first hold on the native editor track (#1131 / NS-T) is
LIFTED", `docs/decision-log.md`) the hold itself no longer applies to anything,
compiler-side or not.

## Output side — what the host actually receives

Authoring an element is only half of it. For a host to *render* a scene
heading differently from dialogue, the element has to survive to runtime
output. **Today almost none of it does.**

Verified 2026-08-01: no `element_kind`, `element_data` or `ElementKind`
anywhere in `brink-format` or `brink-runtime`; `Choice` is still
`{ text, index, tags }`; `Line` carries only text and tags.

| Feature | Ruled | On the wire | Reaches the host | Notes |
|---|---|---|---|---|
| Markup spans (`PART_SPAN`) | ✅ | ✅ | ✅ | **the one output-side thing that shipped** — `.inkb` v6, PR #1732 |
| Element kind per line | ✅ | ✅ | ⚠️ | **carrier landed** (`OutputLine.element`, PR #2109) but **always `NARRATIVE`** — nothing populates it from claim handlers yet |
| Per-line element data | ✅ | ✅ | ⚠️ | `Element.data` is an open `BTreeMap`; empty in the degenerate case, unpopulated |
| Universal block id | ✅ | ✅ | ✅ | **LANDED** — `OutputLine.block_id`, 4 tests each verified to fail when its increment site is disabled |
| `Step` / `OutputLine` contract | ✅ | ✅ | ✅ | **LANDED #1684** (PR #2102) — `Line` → `Step`; terminals carry no text |
| Choice captured environment | ✅ | ❌ | ❌ | #1508's analyzer half landed; `Choice` is still `{ text, index, tags }` |
| Scene entry / transitions as host calls | ✅ | — | ⚠️ | ruled sitting 4; the `lower:` column it used was **dissolved** by §9.1. **Scene entry answered (#2092, PR #2144):** `std/conventions/screenplay.brink`'s `heading` handler declares `extern scene_entered(title, slug)` and calls it directly — the existing Track-A extern/`ExternalFnHandler` call path, not a synthesized/codegen-only call; proven end-to-end by `issue_2092_scene_entered_extern.rs`, which reads the shipped file and drives it through a real bound handler. Still ⚠️: the preset itself is not yet reachable from a real project (#2080 mounts its source into every `Environment`, but importing an item out of it still needs #1582's pub marker + #2167's confinement), so no default project calls it yet, and written transitions (`SMASH CUT TO:`) as a departure-site style call are untouched by this row's landing |
| Display metrics / measurement | ✅ | — | ❌ | design ruled (prose-spec §6); #362's CM6 consumer unbuilt |
| Element data in XLIFF | ✅ | — | — | ruled **never exported** in v1 (decision-log 2026-07-26) — lives in the base `.inkb` |

**Consequence.** An element today is a *compile-time* concept: a claimed line
runs its handler and the handler emits ordinary prose. Nothing downstream
knows the line was a scene heading. Every renderer-side promise in the
charter — the live renderer, per-element styling, `@[style]` tokens,
Fountain/FDX export — depends on this row group, and it is gated almost
entirely on **#1683 and #1684**.

That makes #1684 more load-bearing than its "runtime refactor" title suggests:
it is the carrier for block id *and* the shape element data rides on.

### ⚠ The shape that anchors the design cannot be written yet

The 2026-07-31 §9.1 ruling turns on a distinction between two dispatch kinds:
**pattern-claiming** handlers are confined to the one conventions module, while
**`!name`-dispatched** handlers are legal anywhere *precisely because they
self-announce*. The 2026-08-01 #1866 ruling rests on the same split.

**Only the first half exists.** `brink-syntax-native`'s own comments say so —
`parser/content.rs`: *"(or, unimplemented today, the `!name` annotation
sigil…)"*; `parser/markup.rs`: *"line-start `!` is reserved for the `!name`
annotation-element dispatch"*. The `annotations-element` golden dispatches
`radio` by **regex** (`args = "^(?<chan>…): (?<text>.+)$"`), not by `!radio`.

So the confinement rule is currently enforceable only against the kind of
handler that exists. Worth filing before v1c, since #1840's registration
design assumes both kinds.

## 🔴 The one that matters most

**Statement-shaped lines at prose-body position are silently printed to the
player.**

```brink
var n = 0
flow main() {
  n = 1
  Value is {n}.
  -> END
}
```

Compiles with **zero diagnostics even under `-D warnings`**. Then:

```
$ brink play story.inkb
n = 1
Value is 0.
```

`block.rs::body_line` has **no dispatch arm for a *bare* (non-`~`) statement
at content-ground position**, so anything statement-shaped and unprefixed
falls through to `content::content_line` and is folded into a `TEXT` run.
The ruled `~`-prefixed logic line (charter §8.2) is a separate dispatch arm
and, as of #1991 + #1972 (both slices), covers assignment, compound
assignment, bare calls, temp declarations, a `~ until cond` condition-park
(native's sole `await` spelling), and a `~{ … }` multi-statement logic
block — every shape parses, runs, and round-trips correctly through `~`
(leaf statements; nested `if`/`while`/`for` inside a `~{ }` block is a
narrower, emitter-only residual — see the charter §8 status board).

**Corrected 2026-08-02 (issue #1972's second slice, per the "check whether
it is already ruled" precedent):** the **bare, unprefixed** spelling
(`n = 1`, no `~`) still hits the silent fold-into-prose failure mode above,
but this is **not an open design question** — #1972's own filing-time body
framed the sigil-vs-bare choice as undecided, but charter §8.2's
2026-07-23 ruling already settled it: `~` is the *only* mechanism a
prose-ground body has for entering code, at both line and block
granularity ("`~` = enter code ... at two granularities"). A bare `n = 1`
is therefore, by design, ordinary prose text starting with the identifier
`n` — never a statement missing an implementation — the same "distinct
syntax over overloaded spellings" principle §10 states for the surface as
a whole. Nothing needs building here; the row above stays 🔴 only because
zero-diagnostic silent prose-folding of something that *looks* like code is
still a real authoring footgun worth a future lint (a separate, much
smaller concern than a grammar decision), not because a bare-statement
grammar is still pending.

The assignment/expression-statement/temp-declaration respell buckets that
together accounted for 60 of the (then) 210 respell failures — fixed by
#1991's first slice and #1972's `~ let` temp-decl + emitter-parity
follow-up — are now all at **zero** cases (verified by `full_corpus_sweep`,
2026-08-01). See "Corpus arithmetic" below for the current bucket
breakdown.

The bare-unprefixed form is still the worst failure mode the project has:
no compile error, no runtime error, just a story that quietly does the
wrong thing and shows the author's code to the reader.

---

## Corpus arithmetic

`brink-respell`'s full-corpus sweep, re-run 2026-08-01 against this PR's
branch (397 oracle cases; `cargo test -p brink-respell --test
full_corpus_sweep -- --ignored --nocapture`): **168 of 397 cases cannot
round-trip** (229 OK), down from 210/396 before #1991+#1972. The
assignment / expression-statement / temp-declaration buckets that used to
total 60 cases are now **zero** — those cases either round-trip cleanly or,
where a second unsupported construct shared the same file, now surface that
*other* gap instead (since `respell_ink_source` fails loud at the first
unsupported node it meets, never partially). That's why several buckets
below grew relative to the pre-fix table: IfElse conditional, alternation
sequence, inline conditional in content, and return-with-value each picked
up cases that used to be miscounted under the prose-body-statements bucket.

| Cause | Cases | Kind |
|---|---|---|
| Inline conditional in content | 26 | emitter |
| Return with a value expression (#1973) | 22 | grammar |
| Thread-start splice (#1974) | 21 | emitter |
| Alternation sequence | 19 | emitter |
| Divert-target-as-value | 18 | grammar |
| `IfElse` conditional | 13 | emitter |
| Word-break spring (#1976) | 13 | needs-design |
| Inline sequence in content | 11 | emitter |
| List-literal expression | 10 | — |
| INCLUDE sites | 9 | — |
| Multi-hop tunnel chain | 3 | — |
| Tunnel-return onwards args | 2 | — |
| Match arm with no pattern | 1 | — |

**Prose-body statements (#1972/#1991) are no longer a bucket** — 0 cases,
verified above.

**At least 56 are pure emitter gaps** needing no grammar work
(alternation 19 + inline conditional 26 + inline sequence 11).

## How to read this board

- **Ruled but not implemented (🔒)** is the cheapest category — the design
  argument is over, someone just has to build it.
- **Emitter-only gaps** don't block authoring at all; they block the
  *ratification differential*, which is how we prove the native surface can
  express the ink corpus.
- **Silent failures are the only emergency.** A `❌` under **Runs** with a `✅`
  under **Parses** means the compiler accepts something and does the wrong
  thing with it.

## Keeping this honest

Re-derive rather than trust. `cargo test -p brink-respell --test full_corpus_sweep -- --ignored --nocapture`
gives the bucket counts. For a per-feature check, use **one isolated project
per probe** — native discovery is tree-is-universe, so several `.brink` files
in one directory silently merge into one project — and always run
`brink play`, not just `brink compile`.

Sources: `docs/native-grammar-holes-triage-1951.md` · `native-surface-charter.md` §8 ·
`prose-dialect-spec.md` · `docs/decision-log.md` · `tests/tier1-native/` (16 goldens).
