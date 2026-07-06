# Issue #368 — Dialogue-Dialect System: Design-Round Comparison

**Scope:** Compare three competing API proposals for the registrable dialogue-dialect system (#368), fold in the adversarial critiques of each, and recommend a basis plus the rulings still needed from the project owner.

---

## 1. The design problem

Today the `@Name:<>` character cue and `(text)<>` parenthetical are hardwired across at least seven editor sites: the screenplay classification post-pass (`element-type.ts:221-242`), sigil geometry (`screenplay.ts` `CHAR_SUFFIX_LEN`/`GLUE_LEN`/`characterName()`, plus the hidden decorations, atomic ranges, and edit-guard that all re-derive the same offsets), the screenplay rows of the Tab/Enter `TRANSITIONS` table (`transitions.ts:174-192`), the `@:<>` template and name-surgery handlers in `keybindings.ts:37-125`, and `extractLineContent`/`CONVERTIBLE_TYPES` in `ink-operations`. Issue #368 asks for a **dialogue dialect**: a host/project-registered declaration of the line conventions, consumed by every one of those subsystems — and, critically, by **game runtimes**, because engines parse the same convention out of `continue_line()` output. The `@Name:<>` dialect ships as the default preset.

Standing rulings from the issue: (1) element kinds are an **open string taxonomy** (per #363); (2) **V1 is mount-time config**, with the host-capability manifest as the eventual home — V1 shapes must validate against that trajectory; (3) the `@Name:<>` preset is the default; (4) proposals must flag hazards for in-flight work (#363 headless taxonomy, #364 data-attrs, #365 fold kinds, #366 lines table, #367 inline markup).

The decisive tensions the three proposals resolve differently:

- **Data vs. code:** can the dialect be pure JSON, or do some conventions need host hooks?
- **Pattern language:** portable regexes, literal affixes, or both?
- **Where classification lives:** TS post-pass, Rust `line_contexts()`, or both (and who owns sync)?
- **What the runtime consumes:** the same artifact, a projection of it, or a generated one — and by what delivery channel, given the manifest is chartered as tooling-only and games load compiled `.inkb` assets?
- **Transition-table ownership:** does a dialect carry the whole key-transition table or only rows for its own kinds?

All three critiques returned **"adopt with changes"** — no proposal is shippable as written, and the winning design is a merge.

---

## 2. The proposals

### Proposal 1 — "Dialect as pure data: one JSON artifact, three interpreters"

**Thesis.** A `DialogueDialect` is versioned pure JSON — no functions, no `RegExp` objects. Patterns are strings in a **portable regex subset** (JS `RegExp` ∩ Rust `regex` crate: named groups yes; lookaround/backreferences no). Everything else is *derived from match indices*: named groups marked `hidden` yield the replace decorations, atomic ranges, edit guard, and cursor clamps in one derivation site; other named groups become `data-*` line attributes; a `template` string (validated to round-trip against the pattern) drives insertion, conversion, and format-document. A parallel `emitted` facet per element describes the post-glue shape the runtime parses out of `continue_line()` output. Classification moves **into Rust `line_contexts()`**; TS keeps a thin fallback. The whole transition table (structural + dialect rows) becomes data interpreted over a closed set of primitive verbs.

**API sketch (condensed).**

```ts
interface DialogueDialect {
  version: 1; name: string;
  elements: ElementDecl[];          // ORDERED array — classification precedence
  chain: ChainRule[];               // "narrative after cue → dialogue", blank breaks
  transitions: TransitionRule[];    // the WHOLE Tab/Enter table as data
}
interface ElementDecl {
  kind: string;                     // open taxonomy; CSS class = `brink-<kind>`
  nature: "narrative" | "machinery";
  source?: {                        // absent for chain-only kinds (dialogue)
    pattern: string;                // "^(?<lead>@)(?<speaker>[^:]*)(?<tail>:<>)$"
    contentGroup?: string;          // which group is editable content
    hidden?: string[];              // group names → ALL editor geometry derived
    template: string;               // "@${speaker}:<>" — round-trip validated
  };
  emitted?: { pattern: string; strip: boolean };  // runtime-output shape
  malformed?: { pattern: string; message: string; severity: string }[];
}
interface ChainRule { after: string[]; is: string[]; becomes: string; carry?: string[] } // carry → data-speaker on whole run
// TransitionRule: { on, key, hasContent?, context?, do: TransitionAction, hint }
// TransitionAction: closed verb set — insertLine | newSibling | convert | weave | clearLine | trap
```

Mount: `brinkStudio({ dialect })` (default `AT_CUE_DIALECT`), `setDialect(view, d)` via the existing screenplay compartment; wasm `EditorSession.setDialect(json)` mirroring `set_host_manifest`, plus `extract(kind, text)` for #366 cast detection; Rust `classify_dialect()` extends `LineContext` with `dialect_kind` + captured `attrs`.

**Keyed strengths (critique-verified).**
- Every cited hardcoded site checks out; match-index-derived geometry is "the right kill" for the real duplication (~10 hand-synced offset sites collapse to one derivation).
- `ChainRule` exactly reproduces today's post-pass (immediate predecessor, blank breaks); `carry` delivers whole-run `data-speaker`, which celeris actually asked for.
- Ordered arrays honor the repo determinism rule; template↔pattern round-trip validation is a strong anti-drift invariant.
- `brink-<kind>` as a *derivation rule* plus a reserved structural list is the correct answer to #363's open taxonomy — existing classes byte-identical, new kinds non-breaking by construction.
- Rust-side classification is the right layer for the named consumers (#365 fold runs live in `folding.rs`; #366 wants a wasm-side extractor).
- Rejection of host classifier callbacks is well-argued: callbacks can't run in the S92 plugin or brink-ide, so pure data is the only shape with one truth.
- The ruling-4 flags for #363/#364/#365 are concrete and correct.

**Keyed holes (from the critique).**
- **Runtime delivery channel contradicts the manifest charter.** `docs/host-capability-manifest.md` says the manifest is tooling/author-time only, "never consumed by the runtime." Making the dialect "the same manifest entry" either amends that charter (an unacknowledged decision) or needs a different home — and games load compiled `StoryData`/`.inkb`, so there is no existing channel from studio config to a shipped game. Unresolved.
- **The `emitted` facet is strictly more permissive than `source`** — `^@(?<speaker>[^:]*):` matches narrative like `@channel: hello`, producing false speaker attribution in the shipped game. The proposed round-trip validation can only prove true positives; no negative-fixture mechanism exists.
- **Replace-wholesale transitions are the wrong default:** a host customizing only its cue shape must copy ~14 structural weave rows into its JSON, freezing them at copy time. Structural rows must stay interpreter-owned.
- **Internal inconsistencies:** the stated validation rule ("chain/transition kinds all declared") rejects its own preset (chain references `narrative`, transitions reference structural kinds — none declared); `hasContent`/`convert` are undefined for pattern-less kinds; the chain vocabulary uses TS-derived kinds (`choice-body`) that Rust `LineElement` doesn't have, putting two competing taxonomies in the payload with no precedence contract.
- **`setDialect` is broken as designed:** `elementTypeField` only recomputes on `docChanged`; compartment reconfigure swaps decorations but not classification. Needs a StateEffect recompute path plus Rust re-run.
- **Migration understated:** studio-store duplicates the `ElementType` enum, and studio-ui's published `StudioApi` leaks PascalCase enum-name strings — the enum→string move touches four packages and an already-public string contract, not one semi-major bump.
- **Hot-path cost:** `elementSpans`-on-demand re-runs regexes where today there is O(1) arithmetic (doc-wide atomic ranges, per-keystroke guard). Spans should be computed once at classification time and cached.
- Kind-level `nature` cannot express #365's machinery set (standalone-vs-tunnel divert is per-line, not per-kind).

---

### Proposal 2 — "Dialect as code: five hooks over a mandatory serializable spec"

**Thesis.** The dialect is a TypeScript **interface the host implements** — five hooks (`classify` / `chain` / `shape` / `compose` / `transitions`, plus `lint` and an `onKey` escape hatch) that partition exactly today's hardcoded surface. Every dialect, including hand-coded ones, must carry a required `spec: DialectSpec` — a JSON-serializable projection that is the single portable artifact: it is what the manifest stores and what runtime plugins parse with (`DialectParser.fromSpec`). The data-declarative form is not a second mechanism but a convenience constructor, `dialectFromSpec(spec)`; the shipped preset is literally `dialectFromSpec(AT_CUE_SPEC)` plus one `onKey` layer for the imperative character-line behaviors. Geometry is derived from the `template`'s affixes (`"@${content}:<>"` ⇒ hidden prefix `@`, hidden suffix `:<>`). Rust stays out of V1.

**API sketch (condensed).**

```ts
interface DialogueDialect {
  readonly id: string;
  readonly spec: DialectSpec;       // REQUIRED portable projection, even for code dialects
  classify(text, base: LineInfo, ctx: ClassifyContext): { kind, fields? } | null;  // bounded look-around
  chain(prev: LineInfo): { next; speaker?: "inherit"|"none"; throughBlank? } | null;
  shape(kind, text): { content: Span; hidden: Span[]; protect? } | null;  // ONE fn feeds all 5 geometry consumers
  compose(kind, content): { text; cursor } | null;   // inverse of shape; drives convert/picker/format
  transitions(): TransitionRow[];   // merged ABOVE the built-in weave table
  lint?(text, base): DialectDiagnostic[];
  onKey?(ev, doc: DialectDocApi): boolean;           // escape hatch for imperative behaviors
}
interface DialectSpec {
  id: string; version: 1;
  elements: Record<string, ElementDecl>;   // match (named groups → fields), content, template,
                                           // nature, speaker role, chainNext, emitted, nearMiss, picker
  transitions: TransitionRow[];
  triggers?: [...];                        // e.g. double-blank Tab inserts cue template
}
function dialectFromSpec(spec: DialectSpec): DialogueDialect;   // THE convenience constructor
function extendDialect(base, overrides): DialogueDialect;       // wrap-and-override composition
class DialectParser {                       // pure, lives in ink-operations
  static fromSpec(spec): DialectParser;
  parseEmitted(text): { kind; fields; rest } | null;   // engines call this
  parseSource(text): { kind; fields; content } | null; // #366 cast detection calls this
}
```

**Keyed strengths (critique-verified).**
- The hook-to-hardcoded-site mapping table is accurate; the five hooks genuinely partition today's surface rather than inventing abstractions.
- **Required `spec` with `dialectFromSpec` as the only privileged path is the cleanest answer to #368's hardest question** ("same manifest entry, or a generated artifact?"): exactly one portable artifact, both sides hydrate from it, hook expressiveness honestly scoped as editor-local. Matches #367's "manifest as producer" precedent.
- Template-derived geometry is faithful to how the code actually works — the three geometry consumers already compute identical spans from the same affix lengths.
- Correct ownership boundary: `classify` runs only over base-narrative lines, so a dialect can never reclassify ink structure.
- Merge-above-built-in-table transitions (vs. P1's replace-wholesale) keeps structural rows editor-owned.
- Good in-flight flags: don't export `characterName()` publicly (#366); keep #365's "machinery/narrative" lexically identical to `ElementNature`.
- Mount wiring mirrors existing architecture (compartment + facet, like `documentHandleFacet`).

**Keyed holes (from the critique).**
- **Three of its own contracts break its own acceptance gate (byte-identical behavior):** (a) the "narrative-natured lines only" classify gate mishandles cues **inside choice bodies**, which today classify with preserved depth but never chain to dialogue — classify eligibility and chain eligibility must differ; (b) `onKey` scoped to "dialect-kind lines" cannot reach the Backspace-fold that fires on narrative/blank lines *adjacent to* a cue, and needs two-line edit support; (c) `compose` has no indentation channel, so the format-document normal form strips the leading whitespace choice-body weave math depends on.
- **Migration claim is false:** `ElementType` is not confined to ink-editor — studio-store duplicates the enum, and `StudioApi` leaks PascalCase enum-name strings into the published embedder API.
- **Determinism violation:** `elements: Record<string, ElementDecl>` — the same JSON through serde or manifest tooling has no guaranteed order; needs an ordered array (repo hard rule).
- **Regex portability unaddressed:** spec patterns are JS RegExp source, but the Rust-twin trajectory requires the `regex` crate subset; only anchoring/backtracking checks are mentioned.
- Chain-only kinds (dialogue: no `match`/`template`) leave `shape`/`compose`/`lineHasContent`/`{convert:"dialogue"}` undefined — as written, `dialectFromSpec(AT_CUE_SPEC)` cannot express the existing Parenthetical→Dialogue rows.
- "Rust stays out of V1" collides with `nature: "machinery"`: Rust fold runs are dialect-blind, so run boundaries diverge from what `LineInfo.nature` reports in the editor.
- **`parseEmitted` is underspecified for composite lines** — the *normal* case (`@Alice:<>` + `(warmly)<>` + text emit as one line); no iteration protocol, parenthetical's non-anchored grammar never shown. This is exactly what celeris and the S92 plugin build against.
- Packaging gaps: `ink-operations` is private; `@brink-lang/web` has zero TS workspace runtime deps today; no deprecation story for `extractLineContent`'s dialect-free signature.

---

### Proposal 3 — "Dialect as a manifest entry: regex-free affix shapes, derivation rules, Rust schema first"

**Thesis.** Design the durable artifact first: a versioned, JSON-serializable, **regex-free** description — element kinds declared as **literal affixes** around a content slot (`{ prefix: "@", suffix: ":", glued: true, contentRole: "speaker" }`), a chain, and a transition **overlay** (structural weave rows stay editor-core — deliberately, unlike P1). Seven spec'd derivation rules turn each shape into classification regex, hidden geometry (reproducing `CHAR_SUFFIX_LEN`/`GLUE_LEN` exactly as derived values), content region, templates, conversion, emitted-line parsing, and malformed diagnostics. The schema's source of truth is **Rust `brink-ir::dialect`** (same module family as `host_manifest.rs`), with a TS mirror; `EditorSession.set_dialect(json)` mirrors the shipped `set_host_manifest` seam. The V1 shape *is* the future manifest `"dialect"` section — nothing renames in the manifest era. No generated artifact: engines reimplement the small spec'd derivation (`parseEmittedLine` ≈ 30 lines).

**API sketch (condensed).**

```ts
interface DialogueDialect {
  version: 1; name?: string;
  elements: DialectElement[];       // ordered, classification precedence
  chain: ChainRule[];               // { after: string[], from: "narrative", to: "dialogue" }
  transitions: DialectTransition[]; // OVERLAY on the built-in weave table
}
interface DialectElement {
  kind: string; nature: "narrative" | "machinery";
  shape: { prefix?: string; suffix?: string; glued?: boolean; contentRole?: "speaker"|"text" };
  hidden?: { prefix?: boolean; suffix?: boolean };  // glue always hidden when glued
  reservedPrefix?: boolean;         // near-miss diagnostics + format-document fixes ("@" yes, "(" no)
  template?: { label: string; pickerKey?: string; blankTab?: boolean };
}
// DialectAction: { convert: kind } | { newline: true } | "strip" | "clear" | "trap"
// Editor: brinkStudio({ dialect }), setDialect(view, d), ResolvedDialect (compiled, cached)
// Wasm:   EditorSession.set_dialect(json) / clear_dialect()   [mirrors set_host_manifest, lib.rs:1189]
// Rust:   brink-ir::dialect with Default = at-cue preset → #365 folding codes against it TODAY
// Shared: parseEmittedLine(d, text) → { segments: [{kind, content, contentRole}], rest }
```

**Keyed strengths (critique-verified).**
- Code citations precise; the derived geometry (character 1+3, parenthetical 0+2) reproduces today's constants exactly, and the 13-row preset table is a faithful data rendering of the current screenplay rows including the trap/clear subtleties.
- **The regex-free keystone is correctly argued:** the entry must round-trip serde/TS/future C#, `RegExp` neither serializes nor ports, and affixes derive strictly more than a bare regex could (geometry, templates, wrapping, emitted parsing).
- **Structural transitions stay editor-core; dialects overlay only their own kinds** — the right default that P1 got wrong.
- `nature` + `brink-${kind}` integrate the in-flight issues by rule, not list; the seam mirrors a shipped pattern (`set_host_manifest`, existing screenplay compartment); "V1 shape IS the manifest entry" genuinely de-risks ruling 2.
- Best-in-round flags for parallel work: #365 re-hardcoding `@[^:]*:<>` in Rust would create the second dialect copy this epic exists to prevent; #366 must not export `characterName()`.

**Keyed holes (from the critique).**
- **Emitted-line parsing is unsound for non-reserved-prefix elements** — the glue that disambiguates `(text)<>` from prose in source is *consumed* by the runtime, so `parseEmittedLine` cannot distinguish a parenthetical from narrative starting with `(`. Needs positional constraints (e.g. parenthetical segments peel only after a reserved-prefix cue segment). This is the runtime half of the pitch, and it currently doesn't hold.
- **Two live classifiers with no sync owner:** TS post-pass (facet) and Rust (set_dialect, needed *now* for #365 folding/diagnostics/format-document) must byte-agree; the two mount calls are separate and nothing keeps them consistent.
- **The manifest entry mixes runtime-shared semantics with editor UX:** `transitions`, `template`/pickerKey/blankTab, `hidden`, and English `hint` strings are pure editor behavior traveling in a "shared with the game runtime" section — violates the repo's separate-concerns-by-ownership principle and the issue's own manifest rationale.
- **Derivation rules underspecified** (no-suffix glued shapes, multi-char suffixes, metacharacter escaping, empty-shape semantics) — and since engines reimplement from prose, every underspecified corner is a cross-engine drift vector, the epic's named failure mode.
- Behavior parity is overstated: `keybindings.ts` Enter name-split runs *before* `findTransition` and shadows the preset's own Enter row; `isFoldableIntoName` gates on Blank, which has no `nature` — not derivable from the schema as claimed.
- **No intl story:** cue affixes live inside XLIFF-exported translatable text; `parseEmittedLine` on locale-resolved output silently assumes translators preserved affix bytes. Unaddressed.
- The flagship consumer walkthrough applies the wrong-shape parser (emitted parser over the *source-side* #366 lines table).
- Migration accounting inverted (claims to delete non-exported internals; misses the actually-published `ElementType`/`CONVERTIBLE_TYPES`/`extractLineContent` breakage); `DialectAction`'s mixed string/object union forces an untagged serde enum with unusable errors.

---

## 3. Comparison table

| Dimension | P1 (pure data, portable regex) | P2 (hooks + mandatory spec) | P3 (regex-free affixes, Rust-first) |
|---|---|---|---|
| Artifact form | Pure JSON, single artifact | Code interface, required JSON `spec` projection | Pure JSON, single artifact |
| Pattern language | Portable regex subset (JS ∩ Rust), named groups | JS regex in spec (portability unaddressed) | Literal affixes; regex derived internally per consumer |
| Expressiveness ceiling | Anything the regex subset + closed verbs express | Unbounded (hooks) — but hooks are editor-local & can drift from spec | Affix-shaped only (no ALL-CAPS cues without an escape hatch) |
| Geometry derivation | Named-group match indices (`hidden: [...]`) — most general | Template affix decomposition | Affix lengths (reproduces today's constants exactly) |
| Attribution / data-attrs | Named groups → `data-*`; `chain.carry` → whole-run `data-speaker` | `fields` + `speaker` role | `contentRole: "speaker"` (one slot only) |
| Transition ownership | Whole table in dialect (**critique: wrong default**) | Dialect rows merged above built-in table | Overlay only; structural rows editor-core (**right default**) |
| Rust involvement in V1 | Classification moves into `line_contexts()` | None (**critique: breaks #365 nature coherence**) | `brink-ir` schema + `set_dialect` now (**critique: dual-classifier sync unowned**) |
| Runtime/emitted story | Explicit `emitted` facet (**false-positive risk, no negative validation**) | `DialectParser.parseEmitted` (**composite-line protocol unspecified**) | Derived from `glued` (**unsound for non-reserved prefixes**) |
| Manifest trajectory | "Drop verbatim" — **contradicts manifest charter**; delivery channel unresolved | Spec is the entry; hooks don't travel — honest scoping | V1 shape = manifest entry; **but mixes editor UX into shared semantics** |
| Determinism | Ordered arrays ✓ | `Record<string,…>` ✗ (repo rule violation) | Ordered arrays ✓ (serde action enum fragile) |
| Behavior-parity risk | Moderate (setDialect recompute broken; hot-path re-matching) | High (3 contracts provably break byte-parity) | Moderate (keybinding precedence shadows own rows) |
| Migration honesty | Understated (4 packages, StudioApi string leak) | False ("confined to ink-editor") | Inverted (deletes non-exports, misses real breakage) |
| Critique verdict | Adopt with changes (5 mandated) | Adopt with changes (repairable, gate unachievable as written) | Adopt with changes (4 mandated + intl ruling) |

---

## 4. RECOMMENDATION

**Winning basis: Proposal 1** — the pure-data artifact with portable named-group regexes, ordered arrays, match-index-derived geometry, chain rules with `carry`, string kinds under the `brink-<kind>` derivation rule, and classification implemented once in Rust `line_contexts()`. It is the richest verified core: one artifact, three interpreters, no host code in the classification path (all three critiques converged on pure-data as the only shape that keeps editor and runtime truth unified), and its named-group model uniquely unifies geometry, attribution, and extraction in a single derivation site. Its defects are all repairable amendments; P2's defects are contract-level (its own acceptance gate is unachievable under three of its stated contracts, and its hook expressiveness is precisely the editor-local divergence channel the issue's "semantics shared with the game runtime" rationale forbids); P3's affix DSL is elegant but its runtime-side pitch is unsound as specified and its prose-spec'd derivation rules are a cross-engine drift vector.

**Mandatory amendments to P1 (from its critique — treat as requirements, not options):**

1. **Structural transition rows stay interpreter-owned.** Dialects contribute rows only for kinds they declare; dialect rows resolve before built-in weave rows. (Graft P3's overlay model; settles P1's open question 3 against its own lean.)
2. **Resolve the runtime delivery channel** before the schema hardens — the manifest is chartered never-runtime-consumed and games load `.inkb`. See Decision 2.
3. **Harden the `emitted` facet:** anchor/positionally constrain emitted grammars (graft P3's critique fix: non-reserved glued shapes peel only after a reserved-prefix segment), add a **negative-fixture** validation suite alongside the round-trip check, and pin the composite-line iteration protocol (P2's critique showed this is the artifact celeris and S92 actually build against).
4. **Fix the internal inconsistencies:** validation rule becomes "declared OR reserved-structural," with defined `nature`/`hasContent` semantics for reserved kinds; explicit contract for pattern-less kinds ("no source shape ⇒ content = whole trimmed line, convert-to resolves to strip"); a single kind vocabulary shared with Rust including the TS-derived kinds (`choice-body`, blank-after-choice promotion) with a stated precedence contract.
5. **Specify the `setDialect` recompute path** (StateEffect-triggered reclassification + Rust re-run) and **cache derived spans on LineInfo at classification time** — never re-match in the atomic-ranges/guard/keybinding hot paths.
6. **Preserve indentation in all compose/convert/normal-form operations** (P2's critique found this: choice-body screenplay lines depend on the ws prefix for weave math; normal form is defined over the trimmed tail).
7. **Classify-vs-chain eligibility split** (also from P2's critique): dialect classification runs on narrative *and* choice-body base lines (preserving depth, as today); chaining runs on narrative only. Cues inside choice bodies must keep today's behavior.

**Grafts from Proposal 3:**

- **Split the artifact into a semantics core and an editor overlay.** Core (elements/shapes/patterns/chain/emitted/nature) is the runtime-shared truth and the future manifest section; overlay (transitions, hidden flags, templates/pickerKey/blankTab, hint strings) is editor behavior. One JSON document, two sections with different owners — per the repo's separate-concerns-by-ownership principle. Only the core travels to runtimes.
- **The Rust schema home and seam:** `brink-ir::dialect` next to `host_manifest.rs`, `EditorSession.set_dialect(json)` mirroring `set_host_manifest`, `Default` = the at-cue preset so #365 folding codes against it *today*, with a JSON conformance test pinning the Rust and TS presets to identical bytes.
- **Affix sugar:** offer P3's `{ prefix, suffix, glued, contentRole }` shape as a declaration form that *mechanically compiles* to P1's pattern + template + hidden groups. The common case (every known convention is affix-shaped) gets P3's simplicity and zero pattern/template/geometry drift; the portable-regex form remains the general representation and the only thing consumers interpret. This also answers P3's underspecified-derivation critique — the derivation is code in one place, not prose reimplemented per engine.

**Grafts from Proposal 2:**

- **`DialectParser` as a named V1 deliverable** — a pure TS class (home: `ink-operations`, re-exported publicly; packaging cost from P2's critique must be paid explicitly) with `parseEmitted` (defined composite-segment iteration protocol) and `parseSource`, plus a Rust twin hydrated from the same JSON. `detectCast(lines, dialect)` ships as the #366 answer instead of exporting `characterName()`.
- **`extendDialect(base, overrides)`-style composition**, so hosts add a kind to the preset without forking it.
- **The dev-mode conformance-check discipline**, repurposed: shared fixture corpora (positive *and* negative) run against both the TS and Rust interpreters in CI — the concrete answer to the two-regex-engines and dual-classifier drift risks.
- **P2's `onKey` analysis as scoping input** (not the hook itself): the Enter-split/Backspace-fold surgeries stay interpreter code, generic over content regions, with the corrected scope "on a hidden-affix line *or adjacent to one*" and two-line edit support.

**Explicitly rejected:** host code hooks in the classification path (P2's `classify`/`onKey` as public API). Every critique's strengths section independently endorsed the reason: code can't run in the S92 plugin or brink-ide, so it forks the one truth this epic exists to unify. A genuinely novel convention gets a new interpreter primitive plus a `version` bump, not a callback.

**Migration plan (all three critiques found this understated):** the enum→string move is a coordinated change across `ink-editor`, `studio-store` (duplicate enum), `studio-ui` (`StatusBar`, `StudioApi`'s published PascalCase strings), and `brink-studio` tests — plan it as its own tracked workstream with a mapping/deprecation story, not a footnote (see Decision 5).

---

## 5. Decisions for the project owner

Only genuine either-way calls remain below; everything the critiques settled (structural rows interpreter-owned, ordered arrays, negative-fixture validation, no code hooks, indentation preservation, spans cached at classification time) is folded into the recommendation above.

1. **Pattern language surface.** Recommended: portable-regex core with affix sugar compiling to it. Alternative with real support (P3 + its critique): affix-only v1, reserving an optional portable-`pattern` escape hatch for a later additive version. Affix-only is smaller and impossible to write non-portably; regex-core is more expressive now (ALL-CAPS cues, multi-group shapes) at the cost of documenting and CI-enforcing the JS∩Rust subset. Which ships in v1?

2. **Runtime delivery channel.** How does a shipped game get the dialect bytes? Options: (a) embed the semantics core in `StoryData`/`.inkb` metadata at compile time; (b) a project file (`brink.toml` or similar) consumed by both studio and the asset pipeline; (c) amend the host-capability-manifest charter so the dialect becomes its first runtime-consumed entry. (a) and (b) preserve the charter; (c) is one home for everything but is a real charter change. This blocks the manifest-era story and should be ruled before the schema freezes.

3. **Where the project dialect lives pre-manifest** (related to but separable from 2): mount-time host code only, a studio project setting, or a project file that `brink ide` CLI and headless analysis can also read (enabling malformed-cue diagnostics in CI). Mount-only is the minimal ruling-2 shape; a project file front-loads part of decision 2.

4. **Classification home and sequencing.** Recommended end-state is one implementation in Rust `line_contexts()` with TS as a thin interpreter over the same JSON — but #364/#365 are building against the TS pass *now*. Ship Rust-side classification in V1 (more churn now, no dual-classifier window), or TS-first with Rust following (less churn, but the sync-owner problem P3's critique flagged exists for the interim)? If TS-first: who owns byte-agreement until the cutover?

5. **Enum→string migration strategy.** The move breaks `ElementType` consumers in four packages and the published `StudioApi` `element.type` PascalCase strings. Options: hard cut in one coordinated semi-major across `@brink-lang/*`; or a one-release compatibility window (deprecated enum alias + a PascalCase↔kebab mapping shim in `StudioApi`). Also: confirm kebab-case as the canonical kind spelling (vs. Rust snake_case and StudioApi PascalCase — a mapping must be blessed either way).

6. **`emitted` facet timing.** Ship the emitted/runtime facet in dialect v1 (locks the shape early, good for the manifest, but no in-repo consumer until the S92/bevy-brink plugin work starts), or ship source-only v1 with `emitted` as an additive `version` 1.x/2 change once a real runtime consumer exists to validate against? The hardening work in amendment 3 is required whenever it ships.

7. **Intl interaction.** Cue and parenthetical affixes live inside XLIFF-exported translatable text (`.ink` → `.inkb` → export-xliff), and runtime parsing happens on locale-resolved output. Ruling needed on: are speaker names translatable content or excluded from translation units? Does export-xliff strip/protect dialect affixes? Is the dialect declared per-locale-invariant (translators must preserve affix bytes, tooling validates)? No proposal addressed this; it constrains the emitted grammar.

8. **`nature` vocabulary.** Two-way (`narrative`/`machinery`) with per-line properties (e.g. `standalone`) handled by interpreter logic, or add a third `structural` value for dialect kinds that must join neither #365 run type (scene headings)? Both P1 and P3 asked; the critiques note kind-level nature can't fully express #365's machinery set regardless, so this is about the declared vocabulary, not the fold implementation.

9. **Chain blank-line semantics.** Today blank lines always break the dialogue chain. Lock that as the only v1 behavior, or include `skipBlank?: boolean` (P2's `throughBlank`) in the v1 schema? Adding it later is additive schema-wise but changes classification results for existing dialects, so P1's critique-endorsed position is that it's better decided now.

10. **`dialect: null` semantics.** For pure-ink projects: does `null` disable only the dialect classification pass, or also tear down the entire screenplay compartment (decorations, transitions, keybinding layer) for true headless composition with #363's theme opt-out? Affects what "no dialect" hosts pay for.