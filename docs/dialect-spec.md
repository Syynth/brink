# Dialogue Dialect — implementation spec (#368, gating #365/#366)

Status: **approved design** (2026-07-05 rulings; see `docs/design/dialogue-dialect-proposals.md`
for the design round and `docs/decision-log.md` "Dialogue dialect (#368): authoring-time/tooling
artifact only"). This spec is the build contract for Wave B of editor round 2.

## Scope ruling (the big one)

**The dialect is an authoring-time/tooling artifact — never runtime-delivered.** No `.inkb`
embedding, no project file in v1 (mount-time config only), the host-capability-manifest charter
(tooling-only) stands unamended. The `emitted` facet exists so the **editor** can model what the
runtime will see (studio Player cue display; the future #362 line-fit epic) — it does not
instruct any runtime. Brink ships the reference `DialectParser` as an ordinary opt-in library:
a consumer wanting editor/game single-truth imports it and passes the same JSON in their own
game code — their wiring, their choice.

## The artifact (winning basis: P1 pure data + P3 grafts)

A `DialogueDialect` is **versioned pure JSON** — no functions, no `RegExp` objects. Host code
hooks in the classification path are **rejected** (they can't run in `brink-ide` or engine
plugins; they fork the single truth). Two sections with different owners
(separate-concerns-by-ownership):

```jsonc
{
  "version": 1,
  "name": "at-cue",
  // ── semantics core (the durable truth; future manifest section) ──
  "elements": [            // ORDERED — classification precedence (determinism rule)
    {
      "kind": "character",             // open string taxonomy; CSS class derives as brink-<kind>
      "nature": "narrative",           // "narrative" | "machinery" | "structural" (ruling: 3-way)
      "source": {
        "pattern": "^(?<lead>@)(?<speaker>[^:]*)(?<tail>:<>)$",   // portable-regex subset
        "contentGroup": "speaker",
        "hidden": ["lead", "tail"],    // ALL editor geometry derives from these match indices
        "template": "@${speaker}:<>"   // round-trip validated against pattern
      },
      "emitted": { /* post-glue shape, positionally constrained — see hardening */ },
      "malformed": [ /* near-miss diagnostics: pattern + message + severity */ ]
    }
    // parenthetical, dialogue (chain-only: no source shape), …
  ],
  "chain": [               // "narrative after cue → dialogue"; blank ALWAYS breaks (ruling)
    { "after": ["character", "parenthetical", "dialogue"], "is": ["narrative"],
      "becomes": "dialogue", "carry": ["speaker"] }    // carry → whole-run data-speaker
  ],
  // ── editor overlay (never travels beyond tooling) ──
  "transitions": [ /* rows for DECLARED kinds only — OVERLAY on the built-in weave table */ ],
  "templates":   { /* pickerKey, blankTab, labels */ }
}
```

- **Pattern language** (ruling): portable-regex core — the JS `RegExp` ∩ Rust `regex` subset
  (named groups yes; lookaround/backreferences no), enforced by a CI conformance check — plus
  **affix sugar**: `{ prefix, suffix, glued, contentRole }` compiles mechanically to
  pattern + template + hidden groups in ONE derivation site (never prose-spec'd per consumer).
- **Structural transition rows stay interpreter-owned.** Dialects contribute rows only for
  kinds they declare; dialect rows resolve before built-in weave rows.
- Named groups beyond `contentGroup`/`hidden` emit as `data-*` line attributes.
- Validation: "declared OR reserved-structural" kinds in chain/transitions; explicit contract
  for pattern-less kinds (content = whole trimmed line; convert-to resolves to strip);
  template↔pattern round-trip check; **negative fixtures** alongside positive ones.

## Classification home (ruling: Rust in v1)

Classification is implemented **once, in Rust `line_contexts()`** (`brink-ir::dialect` next to
`host_manifest.rs`; `Default` = the at-cue preset). TS becomes a thin interpreter over the same
JSON for the regex-fallback path only. Seam: `EditorSession.set_dialect(json)` /
`clear_dialect()` mirroring the shipped `set_host_manifest` pattern.

- A JSON conformance corpus (positive + negative fixtures) runs against BOTH the Rust and TS
  interpreters in CI — the anti-drift gate.
- `LineContext` gains the dialect kind + captured attrs; `#365`'s fold-run computation consumes
  `nature` **in Rust** (this is why #365 is gated on this spec — it must not re-hardcode
  `@[^:]*:<>` in `folding.rs`).
- Hot paths: derived spans (hidden geometry, content region) are computed at classification
  time and cached on the line info — the atomic-ranges/edit-guard/keybinding paths never
  re-match regexes.

## What the dialect replaces (parity contract)

The hardcoded sites, all rerouted through the resolved dialect (byte-identical behavior for the
default preset is the acceptance gate):

- `element-type.ts` screenplay post-pass (cue/parenthetical/dialogue chain; the classify pass
  runs on narrative AND choice-body base lines preserving depth; **chaining runs on narrative
  only** — cues inside choice bodies keep today's behavior),
- `screenplay.ts` sigil geometry (`CHAR_SUFFIX_LEN`/`GLUE_LEN`/`characterName()` → all derived
  from hidden-group match indices),
- Tab/Enter/Shift-Tab screenplay transition rows (`transitions.ts`) + the `keybindings.ts`
  name-surgery handlers (stay interpreter code, generic over content regions, scoped "on a
  hidden-affix line or adjacent to one"),
- `convert.ts` `extractLineContent`/`CONVERTIBLE_TYPES` (conversion follows `template`;
  **indentation is preserved** in all compose/convert operations — choice-body weave math
  depends on the ws prefix),
- format-document + malformed-cue diagnostics (dialect-aware via `malformed`).

## Enum→string migration (ruling: hard cut in 0.8.0)

`ElementType` enum → string kinds (`brink-<kind>` scheme, kebab-case canonical; the
PascalCase↔kebab mapping documented). Touches `ink-editor`, `studio-store` (duplicate enum),
`studio-ui` (`StatusBar`, published `StudioApi` strings), `brink-studio` tests. One coordinated
breaking change in the 0.8.0 release notes — no compat shim.

## Public surface

- `brinkStudio({ dialect })` — default `AT_CUE_DIALECT` preset; `dialect: null` tears down the
  **entire screenplay layer** (classification, decorations, transitions, keybindings — true
  headless with #363, per ruling).
- `setDialect(view, d)` — live reconfigure via the existing screenplay compartment PLUS a
  StateEffect-triggered reclassification and a Rust `set_dialect` re-run (the P1 critique's
  recompute fix).
- `extendDialect(base, overrides)` — add a kind to the preset without forking it.
- `DialectParser` (pure TS, public) — `parseSource` / `parseEmitted` with a **defined
  composite-segment iteration protocol** (a cue + parenthetical + text emitting as one line is
  the normal case, pinned by tests). `detectCast(lines, dialect)` ships as the #366 answer —
  `characterName()` is NOT exported publicly.

### `emitted` hardening (from the design round — mandatory)

Emitted grammars are positionally constrained: non-reserved-prefix shapes (parenthetical) peel
only after a reserved-prefix segment (cue) — never from arbitrary prose. Negative fixtures prove
`@channel: hello` prose and `(aside)` prose do NOT parse as cue/parenthetical.

## Unblocks

- **#365** (fold kinds/pills): builds against `nature` from Rust classification; pill cast-names
  via the dialect extractor.
- **#366** (lines table): exposes the compiler lines table + `detectCast` instead of exporting
  the hardcoded `characterName()`.

## Follow-ups filed out of scope

- Intl × affixes (cue affixes inside XLIFF-translatable text) — file as an issue; constrains
  emitted grammar later, does not block v1.
- Project-file home (CLI/CI malformed-cue diagnostics) — possible later; v1 is mount-time only.

## Deliverables (Wave B build order)

1. **`brink-ir::dialect` + Rust classification** — schema (serde, ordered), at-cue preset as
   `Default`, `line_contexts()` integration, conformance fixtures (pos+neg), `set_dialect` seam
   in brink-web. Byte-parity tests against today's TS classification for the preset.
2. **Editor integration** — resolved-dialect consumption (geometry/decorations/transitions/
   convert/keybindings), enum→string migration across the four packages, `dialect` option +
   `setDialect`, taxonomy doc update.
3. **`DialectParser` + `detectCast` + #366 lines-table exposure** — public parser, cast
   detection, wasm lines-table query + TS types.
4. **#365 fold kinds** — per its issue spec, now against Rust `nature` classification.

Each deliverable is one PR on the merge train; (1) blocks (2)–(4); (3) and (4) can run parallel
after (2) lands.
