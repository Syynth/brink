# Candidate element inventory — STRAW for the syntax round (#1351)

Status: **prepared material, not rulings.** Everything here is 🔶 straw
unless marked RULED (carried from sittings 1–3, docs/prose-dialect-spec.md).
Purpose: walk into the syntax round reacting to a concrete list instead of
building it live. Organized as: the screenplay preset's elements, the
interactive-native pieces, deliberate exclusions, the glyph-conflict
ledger, and the open questions the sitting must answer.

Framing observation: with the explicit-format posture (marks are real,
the editor softens), **the preset is approximately "Fountain with the
inference removed"** — Fountain already defines a force-marker for most
elements; we make the force-marker the spelling and drop the guesswork
(ALL-CAPS-line-means-cue is exactly the cleverness we refused).

## 1. The screenplay preset — core elements

| Element | Role (§3.6) | Recognition (declared) | Data payload | Spelling candidate | Fountain/FDX export | Status |
|---|---|---|---|---|---|---|
| **Scene heading** | structural (declares a stitch — RULED) | `INT.`/`EXT.`/`INT./EXT.` prefix pattern | title (= display name, RULED), slug (explicit or inferred, RULED), time-of-day parse optional | `INT. MARKET SQUARE - NIGHT` + explicit slug syntax ⏳ (see §5.3) | Scene Heading | pattern RULED-adjacent; slug spelling ⏳ |
| **Action** (= the general mechanism's *narrative* default, preset-skinned) | content | default — any unmarked content line | — | plain text | Action | RULED (it's the degenerate element) |
| **Cue** (character) | attached-forward (RULED) | `@NAME` line | `speaker`; optional extension (`V.O.`, `O.S.`) | `@VENDOR` · `@VENDOR (V.O.)` — **note: the shipped at_cue dialect's `:<>` tail dies here**; it existed only because ink lacked attachment (§3.6 supplies it) | Character (+ extension) | spelling 🔶 (bare `@NAME` straw) |
| **Parenthetical** | attached-forward (RULED) | `(…)` line, chain: after cue or dialogue | `delivery` | `(hushed)` | Parenthetical | RULED-adjacent |
| **Dialogue** | content | chain: line after cue/parenthetical (RULED mechanism) | inherits `speaker`/`delivery`/block id | plain text under a cue | Dialogue | RULED |
| **Transition** | dresses a divert (RULED) | pattern: trailing `TO:` line (or declared list) | transition kind | ⏳ round question §5.2: separate lines (`CUT TO:` then `-> market`) vs fused — **no-costume leans separate or `CUT TO: -> market`**; bare fused `CUT TO: market` disguises a divert | Transition | concept RULED; spelling ⏳ |
| **Lyrics** | content | ⚠ Fountain's `~` force-marker **collides with the `~` logic-line escape** (RULED, 2026-07-23) | — | ⏳ respell or defer; candidates: defer from v1 preset, or `<lyrics>` as markup instead of element | Lyrics | ⏳ (conflict, §4) |
| **Centered** | ⏳ element vs *markup span*? | Fountain: `>text<` | — | lean: a **span** (`<center>` or preset alias) — it's per-line presentation, not a kind of content | Centered | ⏳ |

## 2. Interactive-native (no Fountain analog — and none needed)

| Piece | Treatment | Notes |
|---|---|---|
| **Choice options** | **typed prose (RULED, sitting 3)** — dialogue-choice (`speaker: <PC>`) or action-choice (imperative) | ⏳ **typing mechanism** (§5.1): chain rule (cue above the block types the options — Telltale pattern) vs per-option marker vs preset default. `[]` anatomy re-ratified unchanged. |
| **Choice point `{? }`** | structure (skeleton register — never costumed) | the §2b complement pass: one aesthetic pass so skeleton + conventions sit well together |
| **Gather/rejoin** | dissolved (RULED, charter §5) | next line after the block; no element |
| **Splice `<- flow(args)`** | structure | unchanged |
| **PC identity** | project convention (roster tie-in) | which cue is "the player" — a conventions-file declaration; powers dialogue-choice typing + VO export grouping |
| **Per-path export of choices** | *choices dissolve in path export* | a path has its choices resolved: the chosen option's delivered text renders as dialogue/action; unchosen options simply don't exist in that path. No Fountain mapping needed — a pleasing consequence of §2b.4 |

## 3. Deliberate exclusions (v1)

| Fountain construct | Fate | Why |
|---|---|---|
| Emphasis `*i*`/`**b**`/`_u_` | **markup layer** (RULED: XML-only v1, sugar deferred) | `<b>`/`<i>` come free as tags |
| Boneyard `/* */` | already exists | brink comments |
| Notes `[[…]]` | defer | brink has comments + doc-comments; an *attached authorial note* element can join a later preset rev if the writer wants it |
| Sections `#` / Synopses `=` | **defer — the flow/stitch tree IS the outline** | scene headings + the binder/story-graph cover the organizational need; `#` collides with tags anyway (§4). Revisit only on demonstrated writer need |
| Page break `===` | skip | renderer concern; meaningless to the runtime |
| Dual dialogue `^` | defer | rare in interactive scripts; if ever needed it's a data flag on the cue, not an element |

## 4. The glyph-conflict ledger (the collisions the round must adjudicate)

| Glyph | Fountain meaning | brink meaning today | Verdict needed |
|---|---|---|---|
| `~` line-start | lyrics force | **logic-line escape (RULED)** | hard conflict — lyrics must respell or defer |
| `#` | section header | **tags (`#tag`)** | hard conflict — sections deferred; also affects `#slug#` candidate (§5.3) |
| `>` line-start | transition force / centered open | free in prose-ground (`>` emit-escape is code-ground only; `->` divert is lexically distinct) | available, but two Fountain meanings compete for it |
| `@` line-start | character force | at_cue precedent (cue) | **aligned** — happy accident, keep |
| `.` line-start | scene-heading force | free | probably unneeded (INT./EXT. pattern is primary; explicit posture doesn't need a second force spelling) |
| `!` line-start | action force | free | unneeded — action is the default |
| `=` line-start | synopsis | free in native prose | moot if synopses defer |
| `[[` | note open | `[` is choice display-split (choice lines only) | distinguishable; moot if notes defer |

## 5. Open questions for the sitting (the agenda)

1. **Choice-option typing mechanism** — chain-rule-from-cue (Telltale),
   per-option marker, or preset default with override? Interacts with
   the PC-identity convention (a bare choice block with no cue above:
   whose dialogue is it, or is it action by default?).
2. **Transition spelling** — separate lines vs `CUT TO: -> market`.
   The no-costume principle rules out a fused form that hides the
   arrow; does the two-line form read well enough, or is arrow-in-line
   the complement?
3. **Explicit slug spelling** — candidates: Fountain scene-number style
   `INT. MARKET - NIGHT #market#` (native to the tradition; ⚠ brushes
   the `#tag` lexer), an annotation (`@[slug(market)]` — consistent
   with directive syntax, heavier), or a heading-trailing token TBD.
4. **Lyrics + centered** — respell, demote to markup, or defer.
5. **Cue extensions** — `(V.O.)`/`(O.S.)` as parsed payload vs opaque
   text; CONT'D stays derived (RULED, never authored).
6. **The complement pass itself** — with the inventory fixed, one
   aesthetic read of a full marked-up scene: do `{? }`, `->`, `@`,
   `INT.` sit together as one language on the page?
7. **Conventions-file expression** — each row above must be sayable in
   the conventions format (pattern, role, chain, payload captures,
   succession, export mapping) — the inventory doubles as the schema's
   requirements list.
