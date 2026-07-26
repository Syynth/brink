# Implementation roadmap

> Drafted 2026-07-26 from the accumulated design record (prose-dialect
> rethink #1351, Track B stdlib, UFCS sitting, module system, quick-docket
> closures). This is the **build sequencing** view: what is ready, what
> orders what, and which design rounds gate which builds. The decision log
> and per-area specs stay authoritative for *content*; this doc is
> authoritative for *order*. Update it when a track moves.
>
> Standing constraints on every item: the oracle ratchet holds
> (5,577 / 1,027 / 0) unless a line item says otherwise **in-PR**;
> one fix per commit; decline-don't-invent on anything unruled.

## Track 1 — the prose-dialect compiler spine (#1351)

The dominant chain. Strictly ordered; each step gates the next.

1. **Harness terminal fold (#1449)** — byte-identical refactor folding the
   harness's terminal handling; mandatory prerequisite of the Step
   migration so attribution stays clean. Build-ready.
2. **Transcript codec dedupe (#1443)** — collapse the duplicated part
   encode/decode loops **before** any new `OutputPart` kinds land.
   Build-ready.
3. **Step migration** — `Step::{Line, Choices, Done, End}` with
   `OutputLine { element, parts, tags, block_id }`, `.text()` derived.
   Terminal variants frozen (Done = persists/parks, End = despawn —
   #1450 resolution). *Needs its issue filed once 1–2 land.*
4. **The v6 bump** — ONE batched format break (`.inkb` v6 + `.inkl` v2 +
   `.brkt` as needed), everything enters together:
   - `LinePart::Span` (nested markup spans in the line table; hash-transparent)
   - element kind + open-map element data per line
   - universal block id (dedicated `OutputLine` field)
   - **Choice captured-environment** (guard-`as` capture-at-presentation;
     closure env-row precedent)
   - recognizer growth (#1446) · `LineFlags` whitespace fix (#1444)
   - XLIFF unit ids from `DefinitionId` (#1442) · re-import structural
     validation (#1445)
   *Needs the bump-manifest issue; steps 5+ depend on the format landing.*
5. **Native prose grammar** — block-element layer (scene headings with
   `[slug]`, cues `@NAME`, parentheticals, compact cue `@NAME: text`) +
   inline XML markup with the ruled escape set + per-flow trailing tags
   (#474 adjacency). Parser work in brink-syntax-native.
6. **Attachment + lowering** — compile-baked cue/parenthetical attachment
   (dialogue carries `speaker`/`delivery`); transitions/scene-entry lowered
   to non-blocking host calls via the conventions `lower` column;
   header-scoped scene stitches; **slug materialization** (fmt writes
   `[slug]`; `unmaterialized-slug` warn-tier lint via `[lints]`).
7. **`@[element]` annotations + `!name` dispatch** — regex-with-captures
   match, capture→param routing (content-typed captures via the fragment
   path, string-typed → element data), `@[style]` built-in presentation
   tokens, hover/explain-match metadata. Dispatch = ordinary module scope +
   brink.toml implicit usings (Track 3 dependency for the ergonomics; can
   land with explicit `use` first).
8. **Built-in screenplay preset (v1 conventions)** — zero-comptime preset +
   XLIFF v1 element rules (content translates, element data excluded —
   locale-invariant by construction).
9. **Later (post-spine):** measurement API (spec §6), conventions-as-brink-
   module comptime surface, translation round 2, per-path export.

## Track 2 — code dialect / stdlib surface

**In flight (wave 56):** #1471 `or` short-circuit fix · #1472 harness
analyzer-arm bug · #1384 config path/span threading.

**Build-ready queue (wave 57 candidates):**
- #1448 weave-terminator fix (GREENLIT; ratchet → 5,598 in-PR)
- #1449 harness fold (also Track 1 step 1 — schedule here)
- #1475 the `as` binding (statements + `{if}` templates; guard-`as`
  diagnosed not-yet-supported until the v6 bump)
- #1482 UFCS resolution pass (D1–D5 ruled) → **then** #1462 auto-ref (D5)
- #1484 `remove_at` rename
- #1476 COW no-aliasing invariant sweep (+ first-class value-model
  statement in runtime-spec)
- Native grammar gaps — scoping pass running; issues to follow from it.

**Design-gated (needs a sitting before build):**
- Protocol implementation spelling + compare/equality coherence (§9.6) —
  retires three deferrals at once: user-type construction opt-in,
  validating `construct → Option`, coherence line.
- Syntax-in-value-position (operator sections; exhibits `insert`,
  fold-over-`+`).
- Numeric tower mini-spec (§2b) · inhabited-range validator spelling (§7).
- #1483 UFCS-under-incomplete-inference (D3 improvement; additive-only).
- Then **Phase C**: the full verb × signature × row inventory tables.

## Track 3 — module system & prelude

- **Prelude design round** (charter §13.3) — now carries the brink.toml
  **implicit-usings** spelling (C# global-usings model, ruled 2026-07-25).
  Gates the `!name` dispatch ergonomics (Track 1 step 7) and the std
  prelude's final shape.
- #766 M-2 imports + visibility (existing issue; align with the ruled
  `use`-lifted-verbatim model) · #792 M-4 tooling tail.
- #1279 / #1323 — SourceTree module identity + crate-cycle placement
  (save-key landmine; feeds `DefinitionId` stability everywhere).

## Track 4 — runtime & concurrency

- **Flows-as-actors design round (#1210, absorbing closed #597)** — owes:
  re-entrancy/deadlock policy, scoped-sub-eval confirmation, emitting-
  callback semantics. Gates the FS epic's cross-flow story.
- FS epic (#889): #928 compiler slice → #980 runtime slice → #1293 engine
  clock binding for `until` time-waits.
- #1146 row-directed wake dirtying (the #1101 flake's real fix).
- Design horizon: #1211 effect-system core calculus · #1212 post-landing
  runtime restructuring · #1213 minimal-core north star.
- Environment umbrella (#1306) tail: #1356 INCLUDE-escape regression ·
  #1359 crates.io yank after v0.0.12.

## Track 5 — bevy-brink

#1062 BH-5 host ergonomics (evidence-scoped) · #937 borrow-not-copy
parallel step · #1096 call_ink_function silent-fallback bug · #1380
with_config wiring · #1076 deferred batch-call variant. The Step
migration (Track 1 step 3) touches the bevy consumer — coordinate the
Line→Step rename there in the same window.

## Track 6 — editor / NS-T (HELD)

Held for compiler-first by maintainer direction: #1131 NS-T charter,
#1350 native LSP basics, #1347 live-typing diagnostics bypass, #1358
native-awareness threading, plus the studio backlog (#57 #69 #77 #279
#280 #281 #1051). Holes' release policy re-homed here (rides `[lints]`
when opened). The prose round's editor surface (decorations, completions,
element hover) builds AFTER the Track 1 spine ships data for it.

## Track 7 — infra, diagnostics, quality

- `[lints]` tail: #1374 reserved keys · #1383 deny-warnings e2e ·
  #1417 override tier for ide/lsp/wasm.
- Config polish: #1406/#1439 double walk · #1407 prune policy · #1434
  root-relative scoping · #1435 unbounded git-boundary walk · #1436
  warning gaps · #1382 silent-drop sweep · #1387 w48 test/doc gaps.
- Diagnostics platform: #1161 `@[allow(Exxx)]` · #1162 Info/Hint tier ·
  #747 per-diagnostic reference corpus · #744 quick-fixes.
- CI/pump: #1396 excluded-dir testing · #533/#567/#574/#1467 pump disk +
  target-dir cache hygiene.
- Native parser hardening: #1191 parity/fuzzing tracking · #1200 coverage
  gate · #1252/#1253/#1256/#1285/#1309/#1335 gap items · #1282 fuzz pin.

## Icebox (unchanged, evidence-gated)

#825 first-class projections · #826 path-granular rows · #827 vectors
(numeric-tower §2b owns the door) · #829 sequence slices · #965/#966
save-migration · #1090 bounded polymorphism · #848 incremental artifacts ·
#1093 modular artifacts charter · #905 statecharts charter · #591/#592
fmt config.

## The book (NS-D, #1132)

Chapter issues (#1181–#1184, #1292) proceed opportunistically as their
subject areas stabilize; the prose dialect gets its chapter after the
Track 1 spine lands.
