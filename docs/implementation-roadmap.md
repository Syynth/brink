# Implementation roadmap

> **Rewritten 2026-07-27** after a day of rulings and ~120 merged PRs. This is
> the **build-sequencing** view: what is done, what is ready, what orders what.
> The decision log and per-area specs stay authoritative for *content*; this doc
> is authoritative for *order*. Update it when a track moves.
>
> **Priority order is unchanged: language and runtime first.** The editor track
> (Track 6) is documented here for situational awareness — *how far away is it,
> and what gates what* — not because it is being worked. It is deliberately
> placed after the compiler tracks it depends on.
>
> **Refreshed 2026-07-30** after pump waves 84–92 (28 of 32 items landed).
> Standing constraints: the oracle ratchet holds (**5,607 episodes / 365 of 396
> cases**) unless a line item says otherwise **in-PR**; one fix per commit;
> decline-don't-invent on anything unruled; and **search the decision log before
> opening a design discussion** — five of seven "open questions" this week
> turned out to be already ruled.

## Where things actually stand

**Conformance moved for the first time in months**: 5,577 → **5,607** episodes,
failing cases 14 → **7**, via the root-final-gather fix (#1448), the
label-scope step fix (#1503), and the multi-file INCLUDE terminus (#1502).

**Track B (stdlib surface) is complete.** `or` coalescing (short-circuit), the
`as` binding, `for k, v`, display-boundary None-render, `TypeName { }`
construction, `remove_at`.

**UFCS is complete end to end** — design sitting → resolution pass (#1482) →
LIR consumption (#1506) → auto-ref (#1462) → IDE hover/goto-def/references/
rename. Companion-module dispatch (`Mood::all()`) is ruled but unbuilt.

**The native surface went from unusable to usable.** It is strict-only, and
this morning it had *no type-annotation syntax at all* — every binding was
structurally condemned to E065/E066. Now: annotations on params, lets, returns,
lambdas, struct fields (NG-A/B/C/E); `use` statements that actually resolve
(#1581 — they never did; the test that "proved" it passed via bare-name
fallback); import aliases honored (#1590).

**Both v6 blockers dissolved rather than resolved** — see Track 1 step 3.

## Track 1 — the prose-dialect compiler spine (#1351)

The dominant chain. Strictly ordered; each step gates the next.

1. **Harness terminal fold (#1449)** — **PARTIAL, still open.** PR #1513 landed
   the harness-side half and *explicitly declined* the runtime-side classifier
   collapse, which is now **#1520** (`needs-design`). An earlier revision of this
   doc recorded this as DONE; that was wrong — it read the PR title, not the
   outcome.
2. ~~**Transcript codec dedupe (#1443)**~~ — **DONE** (PR #1512).
3. **The v6 bump — now writable.** Both prerequisites cleared 2026-07-27, and
   the payload list shrank:
   - **#1446 ruled**: inline conditionals/sequences use **branch expansion**
     (the 2026-03-15 ruling, never implemented — #1667), *not* `LinePart::Select`.
     `Select` stays target-side only. **Recognizer growth leaves the bump.**
   - **#1519 rescoped**: `.inkb` keeps its offset table (random access is real —
     brink-intl reads only the line-tables section; bevy reads only the
     checksum). It gains a **length per section entry** so unknown sections are
     skippable, and `.brkt` becomes **framed records** (it is a log). **Not on
     the critical path** — spans ride the existing `u8` part-tag dispatch.
   - ~~**Choice captured environment (guard-`as`)**~~ — **DONE** (#1508). Turned
     out to need **no nested value and no wire-format change at all** — simpler
     than even this doc's own "rides as a nested value like `VAL_CLOSURE`"
     guess: the guard's binding reuses the *existing* `OptionBind` opcode
     (issue #1475) in the condition-evaluation frame, and the pending choice's
     existing thread-fork snapshot (which already restores tunnel/function
     temps across a pick) carries the captured slot value across selection for
     free. Verified end to end including a `StorySnapshot` round trip.
   - **Remaining v6 payloads**: `LinePart::Span` · element kind + open-map
     element data · universal block id (a dedicated `OutputLine` field).
   - **Moved out**: #1442 (intl alias-awareness — its own defect, Track 3) ·
     #1445, #1444 (both landed).
   - ⚠ *Owed: the bump-manifest issue, stating per payload whether it rides
     part-tag dispatch, a nested value, or a genuinely new section.*
4. **Step migration** — `Step::{Line, Choices, Done, End}` with
   `OutputLine { element, parts, tags, block_id }`, `.text()` derived. Terminal
   variants frozen (Done = flow persists/parks; End = host despawns the entity).
   Touches the bevy consumer — coordinate the `Line`→`Step` rename there.
   ⚠ *Owed: its issue.*
5. ~~**Native prose grammar**~~ — **LANDED 2026-07-28** (#1715 / PR #1725):
   scene headings with trailing `[slug]` + tags, block cues, parentheticals,
   the compact `@NAME: text` form, and header-scoped stitches (flat siblings).
   The inline **markup layer** landed alongside it (#1716 / PR #1732) and forced
   a real **`.inkb` v6 / `.inkl` v2 bump** for `PART_SPAN` — spans did *not*
   ride the existing part-tag dispatch for free, as this doc previously assumed.
   Manifest validation followed in #1733 / PR #1778. Original scope was: block elements (scene headings with `[slug]`,
   cues `@NAME`, parentheticals, compact `@NAME: text`) + inline XML markup with
   the ruled escape set + per-flow trailing tags (#474).
6. **Attachment + lowering** — compile-baked cue/parenthetical attachment;
   transitions/scene-entry lowered to non-blocking host calls via the
   conventions `lower` column; header-scoped scene stitches; **slug
   materialization** (fmt writes `[slug]`; warn-tier lint for unmaterialized).
7. ~~**`@[element]` annotations + `!name` dispatch**~~ — **LANDED 2026-07-28**
   (#1719 / PR #1724), including the `@[style]` declaration surface. Original note: ⚠ **newly unblocked**:
   per-declaration annotations only started lowering on 2026-07-27 (#1563); the
   whole dispatch design assumed a mechanism that did not exist.
8. **Built-in screenplay preset** + XLIFF v1 element rules (content-typed
   captures translate; string-typed element data does not).
9. **Later**: measurement API (§6, with #1668 riding the same surface),
   conventions-as-brink-module, translation round 2, per-path export.

## Track 2 — code dialect / stdlib

**Ruled 2026-07-27, build-ready:**
- ~~**#1490** array literals `[1, 2, 3]`~~ **CLOSED** · ~~**#1592** dual-reading
  `use`~~ **CLOSED** (delivered by PR #1686 on 2026-07-27 — this list carried it
  as build-ready after it had already shipped)
- **#1552** type-name conformance sweep — angle brackets + Uppercase
  non-primitives (`Array<T>`, `Option<T>`, `List<T>`, `Handle<T>`); primitives
  stay lowercase. **Not a new decision** — enforcement of the 2026-07-19 casing
  partition that the lowercase annotation vocabulary has been violating.
- **#1541** flags verbs → each flags type's **companion module** (`Mood::all()`,
  `s.count()` via companion-first UFCS lookup), riding the ruled 2026-07-19
  companion-module design; check whether that machinery exists before building.
- ~~**#1531** frame-local projection receivers legal~~ **CLOSED**
- **#1549** annotation-first global typing
- ~~**#1471** `or` short-circuit~~ **CLOSED** · ~~**#1667** branch expansion~~
  **CLOSED** — delivered by PR #1694 *before* this list was written; the issue's
  "never built" premise was already false when filed

**Design-gated:** protocol impl spelling + compare/equality coherence (§9.6 —
retires three deferrals) · syntax-in-value-position · numeric tower (§2b) ·
inhabited-range validator · #1483 UFCS-under-incomplete-inference · then
Phase C's verb × signature × row inventory.

**Newly discovered, unowned until now:** **#1665 — the block-as-expression /
effect-signature checker**, the half of the 2026-07-20 block/effect ruling that
dissolves G-2. It "folds into B0.8"; B0.8 closed without it. This is what makes
a braced construct's identity a *checker* decision instead of a parser guess,
and it is why single-line braced choice bodies silently misparse today (#1206 —
ruled legal, delivery deferred here).

## Track 3 — identity, modules, intl

**R1 ruled (2026-07-27): modules-spec §5 stands.** Name-derived identity with
`#@was` as the sole migration edge; stamped GUIDs and fuzzy load-time
rematching stay rejected. Its three gaps are ordinary work:

- **#1671** — transitive `#@was`. Renaming a knot re-keys every stitch beneath
  it but mints **one** alias, so a *declared* rename still loses descendants'
  saved visit counts. Silent save-data loss.
- **#1672** — the IDE writing `#@was` (ruled in §5, **never implemented
  anywhere**) + authoring-time detection of undeclared renames.
- **#1674** — anonymous-container state: `LoadReport` surfaces what was
  dropped; **naming is the opt-in**; configurable lint for unnamed stateful
  choices.

**#1504 — LIVE MISCOMPILE, unbundled and build-ready.** Unqualified anonymous
scope paths collide across files: the player picks a choice from an INCLUDEd
file and the *entry* file's body runs. Last-write-wins in the linker. The
oracle cannot see it (no corpus case puts root weave content in an INCLUDE).
**#1673** adds the codegen duplicate-id guard that would have caught it.

**#1442 — intl is alias-blind.** Zero alias usage in brink-intl: saves consult
the alias table, translations never do. So a perfectly declared rename **still
orphans every translation beneath it, on every future rename** — a standing
workflow defect, not migration debt. **Ruled to land with or before #1504**,
which moves anonymous root-content ids that XLIFF units key on.

**Module system:** prelude round (charter §13.3, carries brink.toml implicit
usings) · #766 M-2 imports/visibility · #792 M-4 tooling tail · #1279/#1323
SourceTree module identity.

## Track 4 — runtime & concurrency

**Flows-as-actors design round (#1210)** — owes re-entrancy/deadlock policy,
scoped-sub-eval confirmation, emitting-callback semantics. Gates the FS epic.
FS epic (#889): #928 compiler slice → #980 runtime slice → #1293 engine clock.
**#1520** yield-time terminal classifier (design landed) → **#1574** R2
(same-call `RanOutOfContent`, retire both extra-step allowances).
**#1573** `did_safe_exit` has no production-reachable equivalent.
Design horizon: #1211 effect-system core calculus · #1212 post-landing runtime
restructuring · #1213 minimal-core north star.

## Track 5 — bevy-brink

The wake-signal cluster is closed: #1146 row-directed dirtying, #1632 the
handle-host ledger bypass, #1633 the parallel-driver `FlowSleep` filter, #1609
command-binding purity. #1101's flake is **verified dead over 120 runs**.
Remaining: #1062 BH-5 host ergonomics. The Step migration (Track 1 step 4)
touches this crate — coordinate the rename in that window.

## Track 6 — the editor (NS-T) — *awareness only, not scheduled*

Priority is language + runtime; this section exists to answer "how far away is
it, and what gates what." The path splits into two very different timelines.

### E1 — native *code* editing (near; a handful of issues)

Landed incidentally this week: real module identity for IDE/LSP on native
(#1526) · cross-file LSP scope by module graph (#1562 — before it, **every
native file was its own project** and cross-file navigation silently returned
nothing) · native-awareness through the pure analysis path (#1358, which also
resolved #1347's live-typing divergence) · UFCS hover/goto-def (#1507) ·
`def --at`, find-references, rename (#1539) · three rename-range corruption
fixes (#1550/#1560/#1571) · options propagation across IdeSession/LSP/
EditorSession (#1553) · `@[allow(Exxx)]` (#1161) · Info/Hint tier (#1162) ·
`Unnecessary` tags (#1618) · quick-fixes (#744) · params/temps in hover (#530).

**Remaining for E1:**
1. **#1350 — native LSP basics**: register `.brink` as a served language,
   semantic tokens from the native CST, native diagnostics. *The keystone.*
2. **#1572 — LSP identity keyed on absolute paths** while identity is
   contractually root-relative. Self-consistent today, so nothing *looks*
   broken, but every LSP-minted `DefinitionId` diverges from a real compile.
3. **#1580 — project extent**: the LSP treats every `.brink` in the workspace
   as one project; the compiler scopes to the SourceTree root.
4. **#1582 — native definitions carry no `VisibilityMark`**, so everything
   defaults Private and cross-file native references raise E087. `needs-design`
   (a defaults ruling) and arguably the hardest blocker here.
5. **#1672 — rename writes `#@was`** (also Track 3).

### E2 — authoring quality (after E1)

`fmt` for `.brink` (cosmetic indentation; whitespace is never load-bearing) ·
completions (UFCS + companion-module-aware per the S4 ruling, protocol methods,
registry-informed) · #362 measurement consumer (CM6 overflow against the ruled
§6 surface) · #1668 expansion-factor metric on that same surface ("this line
becomes N voice lines").

### E3 — the prose surface (gated on Track 1)

Decorations for renderer-elidable structural marks · element completions
(character names, markup tags) · element hover / explain-match · the **live
renderer** the charter promises. **None of it can start before Track 1 steps
5–8 ship the data it renders.**

So the honest answer to "how far from the editor working": *native code
editing* is a handful of issues away and mostly already built; the **writer's**
editor is behind the prose spine, by construction.

### Studio backlog (independent)
#57 #69 #77 #279 #280 #281 #1051 · holes' release policy re-homed here.

## Track 7 — infra, diagnostics, quality

CI: **bevy-brink was excluded from the workspace test *and* clippy runs in four
places** — roughly a dozen bevy PRs landed this week trusting a gate that never
ran their tests. Fixed by #1641 with a dedicated required job.
Diagnostics platform: #1161/#1162/#747/#744 landed; #1617 (which codes move to
Info/Hint) parked by maintainer.
Config/lints tail: #1374 reserved keys · #1407 prune policy.
Native parser: #1191 tracking **closed** (all 9 children discharged) · #1200
coverage gate · #1282 fuzz harness.

## Icebox (evidence-gated, unchanged)

#825 first-class projections · #826 path-granular rows · #827 vectors · #829
sequence slices · #965/#966 save-migration · #1090 bounded polymorphism · #848
incremental artifacts · #1093 modular artifacts charter (**note:** it is what
turns `.inkb` durable and makes #1519's skippable sections load-bearing) · #905
statecharts · #591/#592 fmt config.

## The book (NS-D, #1132)

Chapter issues (#1181–#1184, #1292) proceed as their subject areas stabilize;
the prose dialect gets its chapter after the Track 1 spine lands.

## Addendum — what landed in waves 84–92 (2026-07-28 → 2026-07-30)

Recorded here rather than woven in, so the sequencing above stays readable and
this stays honestly dated.

**The effect system closed out.** `Ty::Fn` now carries a `FnRow` (#1680,
PR #1754) — an orphaned ruling from 2026-07-13, re-ruled 2026-07-21, declined
twice for five unruled decisions, all five of which were ruled on 2026-07-28
(decision log: *"Effect-row wire semantics under row-polymorphism"* and
*"Fork A — fn-value call-graph edges are harvested STRUCTURALLY"*). With it:
lambda lifting (#1709), the full stdlib verb layer (#1679), the Fork A
structural atom (#1726), a real soundness bug in effect inference (#1749) and
its seven-sibling audit (#1764).

**Track 1 step 5 is done**; step 6 is *not*, and the reason is specific:

- **§9.1 — the `std::conventions` types design pass — is the live blocker.**
  It gates **#1717** (step 6a/6b) and **#1720** (step 8's preset). Two build
  agents independently declined on it without seeing each other's analysis.
  The two decisions it owes: the concrete `Conventions` schema type + v1 file
  format, and the `brink.toml` pointer/load path. (A third question that used
  to sit here — what a synthesized `scene_entered`/transition call does when
  no matching `EXTERNAL` was authored — is answered by #2092/PR #2144: the
  screenplay preset declares `extern scene_entered(title, slug)` itself and
  ships a no-op ink-side fallback `fn`, so a matching `EXTERNAL` always
  exists; there is no synthesized-call-with-no-fallback case to design for.)
- **#1737** (span nesting an inline conditional) is blocked by
  `prose-dialect-spec` §4.4/§4.5, which label that interaction still-open.
- **#1727** — RULED 2026-08-02 and delivered: HIR mints a lifted lambda's
  identity (`hir::stamp_container_ids`, `hir::LambdaExpr::container_id`) and
  LIR lowering only consumes it, closing the id-parity problem the original
  "mint an index symbol" framing ran into (`stamp_container_ids` running
  *after* `symbol_index_query`, while an index entry needs to exist *before*
  `inferable_defs_query`). Giving the lambda an actual `SymbolIndex` entry /
  `DefKey` so it can join the effect fixpoint's SCC solve is **#1770**'s job,
  not this one's — that sequencing question is still open, just no longer
  blocking the identity half.
- **#1774** — may a native `var`/`const` hold a fn value? — decides whether
  #1764's seven analyzer walks are live at all.
- **#1730** (native `fmt` has no `.brink` pipeline) blocks **#1718**;
  `brink-fmt` has no `brink-syntax-native` dependency at all.

**Standing caution this doc earned:** two entries above were wrong for the same
reason — they recorded a PR's *title* rather than its *outcome*, and an issue's
*existence* rather than whether its premise was still true. Six issues in July
had tracker state that contradicted the code. The retro phase now trues that up
per wave (PR #1729).
