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
| **Statements at prose position** | ✅ | ❌ | ❌ | ❌ | **🔴 SILENT** — see below |
| `~ stmt` line escape | ✅ | ⚠️ | ❌ | ❌ | **🔴 SILENT** — #1991 |
| `> text` line escape in code body | ✅ | ✅ | ❌ | ❌ | 🔒 `E129` — #1992 |
| `return <value>` at prose position | ✅ | ❌ | — | — | #1973, 16 cases |
| Alternations `{~ {& {! {\|` | ✅ | ✅ | ✅ | ❌ | emitter gap, 17 cases |
| Inline sequences | ✅ | ✅ | ✅ | ❌ | emitter gap, 10 cases |
| Thread splice `<- flow(args)` | ✅ | ⚠️ | ❓ | ❌ | #1974; narrowed to choice-point splice |
| Scene headings / cues / parentheticals | ✅ | ✅ | ❌ | ❌ | parse (#1715) but produce **no HIR** (`E129`) |
| Block elements `@[element(…, block)]` | ✅ | ❌ | — | — | **#1839 — unblocked 2026-08-01**, unbuilt |
| Word-break spring | ❓ | ❌ | — | — | #1976, `needs-design`, 12 cases |

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

`block.rs::body_line` has **no dispatch arm for statements at content-ground
position**, so anything statement-shaped falls through to
`content::content_line` and is folded into a `TEXT` run. That single missing
arm covers assignment, bare calls, temp declarations, logic blocks, `await`,
**and** the ruled `~`-prefixed logic line.

**60 of the 210 respell failures** trace to it: assignment 27, expression
statement 19, temp declaration 14. Tracked as **#1972**, with **#1991** as the
`~`-spelling face of the same root cause.

It is the worst failure mode the project has: no compile error, no runtime
error, just a story that quietly does the wrong thing and shows the author's
code to the reader.

---

## Corpus arithmetic

`brink-respell`'s full-corpus sweep: **210 of 396 cases cannot round-trip.**

| Cause | Cases | Kind |
|---|---|---|
| Prose-body statements (#1972/#1991) | **60** | grammar — **silent** |
| Thread-start splice (#1974) | 21 | emitter |
| Inline conditional in content | 21 | emitter |
| Divert-target-as-value | 18 | grammar |
| Alternation sequence | 17 | emitter |
| Value-return at prose position (#1973) | 16 | grammar |
| `else if` conditional (#1975) | 12 | emitter |
| Word-break spring (#1976) | 12 | needs-design |
| Inline sequence in content | 10 | emitter |
| INCLUDE sites | 9 | — |
| List-literal expression | 8 | — |
| Multi-hop tunnel · onwards args · match arm | 6 | — |

**At least 48 are pure emitter gaps** needing no grammar work
(alternation + inline conditional + inline sequence).

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
