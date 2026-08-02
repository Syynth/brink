# Native surface — feature status board

**As of 2026-08-01**, against `origin/main`.

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
| `else if` chains | ✅ | ✅ | ✅ | ❌ | emitter gap **#1975**, 12 cases |
| `for` loops | ✅ | ✅ | ✅ | ✅ | incl. `for k, v` |
| `return` with a value | ✅ | ✅ | ✅ | ✅ | in **`fn`** bodies; prose position is #1973 |
| Lambdas | ✅ | ✅ | ✅ | ✅ | lifting landed (#1709) |
| UFCS method calls | ✅ | ✅ | ✅ | ✅ | |
| `as` binding | ✅ | ✅ | ✅ | ✅ | |
| Array & map literals | ✅ | ✅ | ✅ | ⚠️ | list-literal *expressions* fail, 8 cases |
| Construction literals | ✅ | ✅ | ✅ | ✅ | |
| `or` coalescing | ✅ | ✅ | ✅ | ✅ | |
| Fn values by bare name | ✅ | ✅ | ✅ | ✅ | ruled 2026-08-01 (#1862) |
| Fn value in `var`/`const` | ✅ | ❌ | — | — | ruled **2026-08-01** (#1774); `E083` still gates it |
| `ref` params | ✅ | ✅ | ⚠️ | ✅ | variance unsound until **#1995** lands |

## Prose dialect (`flow` bodies, `>{ }` blocks)

| Feature | Ruled | Parses | Runs | Round-trips | Notes |
|---|---|---|---|---|---|
| Prose lines & interpolation | ✅ | ✅ | ✅ | ✅ | |
| Diverts `->` | ✅ | ✅ | ✅ | ⚠️ | divert-target-as-**value** fails, 18 cases |
| Choices | ✅ | ✅ | ✅ | ⚠️ | inline conditional in choice content, 21 cases |
| Conditionals in content `{if …}` | ✅ | ✅ | ✅ | ❌ | verified 2026-08-01; emitter gap |
| Tags `#` | ✅ | ✅ | ✅ | ✅ | markup inside a tag is **literal** (ruled #1783) |
| Markup spans `<b>…</b>` | ✅ | ✅ | ✅ | ✅ | `.inkb` **v6** `PART_SPAN` |
| Hyphenated span names `<fade-in>` | ✅ | ❌ | — | — | 🔒 ruled 2026-08-01 → **#1996** |
| `@[element(claims=…)]` handlers | ✅ | ✅ | ✅ | ❓ | `annotations-element` golden |
| Prose-bodied `fn` via `>{ }` | ✅ | ✅ | ✅ | ❓ | verified 2026-08-01: emits `[A] hi` |
| **Statements at prose position (bare, no `~`)** | ⚠️ | ❌ | ❌ | ❌ | **🔴 SILENT** — see below; by design, not open (charter §8.2) |
| `~ stmt`/`~{ }` line/block escape (assignment/bare-call/temp-decl/`until`/logic block) | ✅ | ✅ | ⚠️ | ⚠️ | #1991 + #1972 (both slices); Round-trips only leaf stmts, nested if/while/for inside `~{ }` is an emitter-only gap; Runs — the call-only-escape output-boundary fix (review finding, w111) and its whole-body-override/`fn`-default sibling (`lower_stmt_block_as_body`/`flush_code_ground_run`, **#2056**) both append a boundary, but only ONE per flushed *run*: `lir::lower::blocks::lower_block_stmt_list` still has no per-statement `EndOfLine` tracking, so two emitting calls back to back in the same run (e.g. `flow main() ~{ shout(); shout(); > x }`) still glue as `HiHi` — same for a call inside an `if`/`while`/`for` followed by another emitting call in that same run (review finding F3, #2063) |
| `> text` line escape in code body | ✅ | ✅ | ❌ | ❌ | 🔒 `E129` — #1992 |
| `return <value>` at prose position | ✅ | ❌ | — | — | #1973, 16 cases |
| Alternations `{~ {& {! {\|` | ✅ | ✅ | ✅ | ❌ | emitter gap, 17 cases |
| Inline sequences | ✅ | ✅ | ✅ | ❌ | emitter gap, 10 cases |
| Thread splice `<- flow(args)` | ✅ | ⚠️ | ❓ | ❌ | #1974; narrowed to choice-point splice |
| Scene headings / cues / parentheticals | ✅ | ✅ | ❌ | ❌ | parse (#1715) but produce **no HIR** (`E129`) |
| Word-break spring | ❓ | ❌ | — | — | #1976, `needs-design`, 12 cases |

## Conventions & elements

The machinery that makes the prose dialect *authorable as a screenplay*:
patterns claim lines, handlers receive the captures, and a project swaps the
whole vocabulary by naming a different conventions module. Sequenced as
**v1a → v1b → v1c** (#1838 → #1839 → #1840).

| Feature | Ruled | Parses | Runs | Round-trips | Notes |
|---|---|---|---|---|---|
| `@[element(claims=…)]` pattern claiming | ✅ | ✅ | ✅ | ❓ | **v1a landed** — `annotations-element` golden |
| `@[element(args=…)]` capture binding | ✅ | ✅ | ✅ | ❓ | named captures bind params by name |
| Prose-bodied handlers `>{ }` | ✅ | ✅ | ✅ | ❓ | verified 2026-08-01 — emits `[A] hi` |
| Typed handler params (`E171`) | ✅ | ✅ | ✅ | — | #1849 closed |
| Confinement to one module (`E169`) | ✅ | ✅ | ⚠️ | — | query landed; **unreachable from live typing** (#1880) |
| Directive-shaped tag guard (`E172`) | ✅ | ✅ | ✅ | — | landed 2026-08-01 (#1835) |
| **`!name` sigil dispatch** | ✅ | ❌ | — | — | **reserved, unimplemented** — see below |
| Block elements `@[element(…, block)]` | ✅ | ✅ | ✅ | ❓ | **v1b landed** — `annotations-element-block` golden; cross-file injection (#1863) doesn't carry `block` yet, tracked as #2068 |
| `fn conventions()` registration | ✅ | ❌ | — | — | **v1c · #1840** — 4 blocking questions ruled 2026-08-01 |
| Comptime evaluation of conventions | ✅ | ❌ | — | — | #1840; dependency shape ruled 2026-08-01 (#1867) |
| `@[style]` declaration surface | ✅ | ✅ | ❌ | — | `StyleToken` produced, **zero consumers** (#1719) |
| Built-in screenplay preset | ✅ | ❌ | — | — | #1720; `dialect.rs`'s `Default` is legacy hardcoding, not this |
| `[project] elements` name validation | ✅ | ✅ | ✅ | — | #1874 landed: a bare preset-shaped name is checked against `brink-analyzer`'s `BUILTIN_ELEMENT_PRESETS` (empty until #1720 ships a real preset) via `apply_project_config`, warning through the existing `ConfigWarning` channel; a path-shaped pointer is never rejected (still #1844's job) |
| `std::conventions` types | ❓ | ❌ | — | — | prose-spec §9 residual — the last prose-round design item |

## Editor side — how the author interrogates a claimed line

Under conventions, a prose line silently becomes a function call. The ruled
compensation — **"no invisible expansion", a stated maintainer requirement** —
is that the editor can always show which handler claimed a line, why, and what
it bound. **That compensation is currently a promise, not a property.**

Tracked as **#2006**.

| Feature | Ruled | Built | Notes |
|---|---|---|---|
| Per-line classification metadata | ✅ | ❌ | matched kind · handler + source location · capture bindings as spans · disposition |
| Explain-match query | ✅ | ❌ | is-this-matched / by-what / what-bound; lists attempted patterns on a miss |
| Hover shows the handler body | ✅ | ❌ | every matched line points at a real function (§9.1's improvement over the dissolved table) |
| Capture spans as decoration ranges | ✅ | ❌ | the same spans drive editor decoration |
| Harvest index (cues, span kinds) | ✅ | ❌ | ruled a **project-db index obligation**, sibling of the symbol index |
| Succession rules (Tab/Enter) | ✅ | ❌ | live in the conventions file; what makes transitions convention-driven, not hardcoded ink |
| Serialized conventions projection | ✅ | ❌ | what the editor reads instead of tracing execution |
| Last-good caching on comptime fault | ✅ | ❌ | ruled Q2 2026-08-01; never substitute another module's conventions |
| `@[style]` consumption | ✅ | ❌ | `StyleToken` produced in `brink-ir`, read by nothing (#1719) |
| Elements reach `IdeSession` | ✅ | ❌ | #1880 — `E169` unreachable from live typing |
| **Match ordering** | ⏳ | ❌ | **RULE OWED** — "declaration-order + overlap diagnostics is the lean" |
| **Editor re-evaluation loop** | ⏳ | ❌ | **OWED** (§3.5) — yet Q2 already depends on it existing |

⚠ **Two rulings are still owed here**, and one of them has already been
depended on: Q2's last-good caching is justified *because* "§3.5's owed
re-evaluation loop re-runs on every keystroke" — a ruling shipped ahead of the
thing it assumes.

⚠ **Sequencing note.** NS-T (#1131) is held behind the compiler work by
deliberate choice. But this seam is **compiler-side** — queries emitted from
`brink-db`/`brink-ide` — so it is not obviously covered by that hold.

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
| Element kind per line | ✅ | ❌ | ❌ | #1683 — v6 residual payload, unimplemented |
| Per-line element data | ✅ | ❌ | ❌ | #1683 — open-map payload |
| Universal block id | ✅ | ❌ | ❌ | #1684 — a dedicated `OutputLine` field; zero lines exist |
| `Step` / `OutputLine` contract | ✅ | ❌ | ❌ | #1684 — R1 folded #1520 into it 2026-08-01 |
| Choice captured environment | ✅ | ❌ | ❌ | #1508 — rides the open v6 line |
| Scene entry / transitions as host calls | ✅ | ❓ | ❓ | ruled sitting 4; the `lower:` column it used was **dissolved** by §9.1 — unverified what replaced it |
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
