# brink internationalization specification

`brink-intl` provides localization tooling for brink stories — line table export/import, locale overlay compilation, XLIFF generation, and plural resolution. It depends on `brink-format` (types and file formats) and consumes compiled `.inkb` files. See [format-spec](format-spec.md) for line table types and `.inkl` layout, [runtime-spec](runtime-spec.md) for locale loading and line resolution, [compiler-spec](compiler-spec.md) for text decomposition and template production.

## Architecture

```
                    ┌─────────────────────────────────┐
                    │         brink-compiler           │
                    │  .ink → .inkb (with line tables) │
                    └───────────────┬─────────────────┘
                                    │
                    ┌───────────────▼─────────────────┐
                    │           brink-intl             │
                    │                                  │
                    │  export:   .inkb → lines.json    │
                    │  compile:  lines.json → .inkl    │
                    │                                  │
                    │  (future)                        │
                    │  export:   .inkb → .xliff        │
                    │  import:   .xliff → lines.json   │
                    │  regen:    .inkb + .xliff → .xliff│
                    └───────────────┬─────────────────┘
                                    │
                    ┌───────────────▼─────────────────┐
                    │          brink-runtime           │
                    │  .inkb + .inkl → localized story │
                    └─────────────────────────────────┘
```

`brink-intl` sits between the compiler and runtime. The compiler produces `.inkb` files with base-locale line tables. `brink-intl` exports those line tables for translation and compiles translated content back into `.inkl` overlays that the runtime loads.

A separate, general-purpose XLIFF 2.0 crate handles the XLIFF format (data model, read, write). `brink-intl` depends on it for XLIFF import/export but does not require it for the JSON-based workflow.

## Line table scoping

### Current model (per-container)

Line tables are currently per-container. Every container (knot, stitch, gather, choice target, inline sequence wrapper) has its own line table. `EmitLine(5)` means "line 5 of this container's table." This produces dozens of tiny tables, many for compiler-internal containers with no meaningful identity from the author's or translator's perspective.

### Proposed model (per-lexical-scope)

Line tables are scoped to **lexical scopes** — knots and stitches. A knot and all its internal containers (gathers, choice targets, inline sequence wrappers) share one line table. `EmitLine(5)` means "line 5 of the current frame's line table."

The active line table is determined by the **call stack frame**, not by which container the VM is currently inside. When the VM enters a knot, the frame carries that knot's line table. Internal containers within that knot — gathers, choice targets, inline sequences — all emit against the same table via the frame. When control diverts to another knot or calls a tunnel, that pushes a new frame with a different line table.

```
=== knot "shop" (lexical scope) ===
  Line table:
    0: "Welcome to my shop!"               ← emitted from knot root
    1: "What would you like to buy?"        ← emitted from knot root
    2: "Buy a sword"                        ← choice display (gather container)
    3: "Buy a shield"                       ← choice display (gather container)
    4: "You bought the sword."              ← choice output (choice target container)
    5: "You bought the shield."             ← choice output (choice target container)
    6: "Thanks for your purchase!"          ← emitted from a gather

  All containers within "shop" reference this same table.
  Entering "shop" makes this table active on the frame.
```

This is better for localization because:
- Line tables group at the level translators think about (knots/stitches, not gathers)
- Fewer, larger tables with more stable indices
- The export shows meaningful units of narrative, not compiler fragments
- The line table boundary IS the public container boundary — no separate "boundary rule" needed

And better for the runtime because:
- Lookup is `frame.line_table[idx]` — O(1) with no container-to-table indirection
- The mapping is 1:1 with the call stack, which is already managed

### Impact on other specs

This model changes assumptions in format-spec and runtime-spec:

- **`LineId` meaning changes** — `(DefinitionId, u16)` where the `DefinitionId` is the lexical scope (knot/stitch), not the individual container. This affects `.inkl` keying and the regeneration workflow's stability guarantees.
- **Line index assignment moves to LIR lowering** — it's the layer that knows both the lexical scope and the content structure. It assigns indices across all content within a scope, not per-container.
- **`StoryData.line_tables` changes** — from one-per-container (parallel to `containers`) to one-per-scope. Line tables need their own scope-to-table mapping, independent of the container list.
- **Call frame gains a line table reference** — currently the call frame has container index + offset. It also needs to know which line table is active. Could be implicit (derived from the container's scope at link time, stored in `ContainerDef`) or explicit (stored on the frame at entry time).
- **Codegen changes** — `add_line` currently adds to "this container's table." It needs to add to "this scope's table," which means codegen needs to know which scope it's emitting into.

These changes need to be reconciled into format-spec and runtime-spec before implementation.

## Codegen contract

Localization tooling operates post-compilation — it reads `.inkb` files and extracts everything it needs from `StoryData`. This only works if the compiler puts enough information into `LineEntry` during codegen. The codegen contract defines what the compiler must produce for the tooling to function.

### LineEntry enrichment

The current `LineEntry` carries `content: LineContent` and `source_hash: u64`. This is insufficient for localization tooling. The enriched entry adds slot metadata, audio references, and source provenance:

```
struct LineEntry {
    content: LineContent,              // Plain(String) or Template(LineTemplate)
    source_hash: u64,                  // hash of original ink source text
    audio_ref: Option<String>,         // audio asset identifier, if any
    slot_info: Vec<SlotInfo>,          // metadata per slot index (empty for Plain)
    source_location: Option<SourceLocation>,  // where in the .ink source this line came from
}

struct SlotInfo {
    index: u8,                         // matches the Slot(u8) index in the template
    name: String,                      // source expression text, e.g. "player_name", "num_gems"
}

struct SourceLocation {
    file: String,                      // source file path (relative to project root)
    range: (u32, u32),                 // byte offset start, byte offset end in the source file
}
```

**`slot_info`** — for each `Slot(n)` in a template, the compiler records the source expression that produced that slot value. This lets tooling display `{player_name}` instead of `{slot 0}`. For simple variable references, the name is the variable name. For complex expressions, it is the full source expression text. The runtime ignores this field — it only needs the stack index.

**`audio_ref`** — an audio asset identifier associated with this line, populated by external tooling (not by the compiler). Stored alongside content so that localized versions can provide locale-specific audio. Both `.inkb` and `.inkl` carry audio refs — the localized version replaces the base.

**`source_location`** — the file and byte range of the ink source text that produced this line. Enables tooling to show surrounding context, link back to the source for review, and correlate lines across recompiles even when indices shift. The runtime ignores this field.

### Template recognition layer

Template recognition should happen during **LIR lowering**, not during codegen. LIR lowering is the last layer with access to:

- The **HIR** with `AstPtr` → source provenance (text ranges, original source text)
- The **SymbolIndex** with resolved variable names
- The content structure **before** artificial container boundaries are inserted

This avoids the problem of reconstructing source information at the codegen layer where it's already been discarded.

The LIR `Content` type gains a variant for recognized templates:

```
enum ContentEmission {
    /// Fallback: emit parts individually (current behavior)
    Parts(Vec<ContentPart>),

    /// Recognized template: emit as single EmitLine with slot pushes
    Template {
        template: LineTemplate,
        slot_exprs: Vec<Expr>,
        metadata: LineMetadata,
    },
}

struct LineMetadata {
    source_hash: u64,
    slot_info: Vec<SlotInfo>,
    source_location: Option<SourceLocation>,
}
```

LIR lowering runs the recognizers on HIR content nodes. If a recognizer matches, it produces a `Template` with full metadata computed right there while the HIR is still in hand. If no recognizer matches, the existing per-part lowering fires as-is and codegen handles them individually.

Codegen then handles both cases:
- `ContentEmission::Template` → emit slot pushes + `EmitLine(template_idx)`, add enriched `LineEntry` to the scope's line table
- `ContentEmission::Parts` → emit per-part as today (individual `EmitLine`/`EmitValue`/etc.)

### What codegen must produce

For every line added to a scope's line table, the pipeline must:

1. **Produce the correct `LineContent`** — `Plain` for simple text, `Template` for interpolated/structured text (via the recognizer pipeline)
2. **Compute a real `source_hash`** — from the original ink text, before decomposition (currently hardcoded to `0`)
3. **Populate `slot_info`** — one entry per slot in the template, with the source expression name
4. **Populate `source_location`** — file path and byte range of the source text that produced this line
5. **Populate `audio_ref`** — via external tooling or game-engine integration (the compiler does not populate this field)

Items 3 and 4 are metadata for tooling. They are serialized into the `.inkb` line tables section but are not loaded by the runtime's fast path. The `.inkl` overlay format carries `content` and `audio_ref` but NOT slot info or source location — those are source-language concerns, not translation concerns.

### Binary format impact

The `.inkb` line table section gains new fields per entry. These are appended after the existing fields to maintain forward compatibility:

```
LineEntry encoding:
  [existing: content + source_hash]
  has_audio_ref: u8 (0 or 1)
  if has_audio_ref:
    audio_ref: length-prefixed string
  slot_count: u8
  for each slot:
    index: u8
    name: length-prefixed string
  has_source_location: u8 (0 or 1)
  if has_source_location:
    file: length-prefixed string
    range_start: u32 LE
    range_end: u32 LE
```

## Line table export (lines.json)

The primary export format is a JSON file representing the full line table contents of a compiled `.inkb`. This is the simplest way to inspect, edit, and round-trip line tables without XLIFF tooling.

### Format

```json
{
  "version": 1,
  "source_checksum": "0xabcd1234",
  "scopes": [
    {
      "name": "shop",
      "id": "0x0100000000abcdef",
      "lines": [
        {
          "index": 0,
          "content": "Welcome to my shop!",
          "hash": "a1b2c3d4e5f6a7b8",
          "audio": "audio/en/shop_welcome.ogg",
          "source": { "file": "story.ink", "range": [42, 62] }
        },
        {
          "index": 1,
          "content": {
            "template": [
              "I have ",
              { "slot": 0 },
              " items for sale."
            ]
          },
          "hash": "b2c3d4e5f6a7b8c9",
          "slots": [
            { "index": 0, "name": "item_count" }
          ],
          "source": { "file": "story.ink", "range": [63, 93] }
        },
        {
          "index": 2,
          "content": "Buy a sword",
          "hash": "c3d4e5f6a7b8c9d0",
          "choice": "display",
          "source": { "file": "story.ink", "range": [94, 118] }
        },
        {
          "index": 3,
          "content": "You bought the sword.",
          "hash": "d4e5f6a7b8c9d0e1",
          "choice": "output",
          "source": { "file": "story.ink", "range": [94, 118] }
        }
      ]
    }
  ]
}
```

### Fields

- **`version`** — format version (integer, currently `1`). Allows future schema evolution.
- **`source_checksum`** — the `.inkb` content checksum (from the `.inkb` header). Used by the regeneration workflow to detect source changes.
- **`scopes`** — array of line tables, one per lexical scope in the `.inkb`.

Each scope:

- **`name`** — human-readable scope path (knot or stitch name), resolved from the `.inkb` name table. For display and translator context only — not used for matching.
- **`id`** — `DefinitionId` of the lexical scope, as hex string. Stable identity.
- **`lines`** — array of line entries, ordered by index.

Each line:

- **`index`** — `u16` line index within the scope. Matches the `LineId` local index.
- **`content`** — either a plain string or a template object (see [Content representation](#content-representation)).
- **`hash`** — `u64` content hash as hex string. Computed from the source text during compilation. Used by the regeneration workflow to detect changed lines.
- **`audio`** — (optional) audio asset identifier for this line. Translators can replace this with a locale-specific audio path.
- **`slots`** — (optional, only for templates) array of slot metadata objects, each with `index` (u8) and `name` (source expression text). Absent for plain lines.
- **`choice`** — (optional) `"display"` or `"output"`. Present when this line is one half of a choice text decomposition. Lines with the same `source` range and different `choice` values are paired.
- **`source`** — (optional) source provenance: `file` (path relative to project root) and `range` (byte offset pair `[start, end]`). Enables tooling to show surrounding ink context and link choice display/output pairs.

### Content representation

Line content has two forms, mirroring `LineContent` from [format-spec](format-spec.md):

**Plain text:** a JSON string.

```json
"content": "Hello, world."
```

**Template:** an object with a `template` key containing an array of parts.

```json
"content": {
  "template": [
    "literal text",
    { "slot": 0 },
    {
      "select": {
        "slot": 0,
        "variants": [
          { "cardinal:one": "apple" },
          { "cardinal:other": "apples" },
          { "exact:0": "no apples" }
        ],
        "default": "apples"
      }
    }
  ]
}
```

Template parts:

| JSON form | `LinePart` variant | Description |
|-----------|-------------------|-------------|
| `"string"` | `Literal(String)` | Static text fragment |
| `{ "slot": n }` | `Slot(u8)` | Runtime value interpolation by stack index |
| `{ "select": { ... } }` | `Select { slot, variants, default }` | Plural/keyword branching |

Select variant keys use the format `type:category`:

| Key format | `SelectKey` variant | Example |
|------------|-------------------|---------|
| `cardinal:zero` through `cardinal:other` | `Cardinal(PluralCategory)` | `{ "cardinal:one": "apple" }` |
| `ordinal:zero` through `ordinal:other` | `Ordinal(PluralCategory)` | `{ "ordinal:one": "1st" }` |
| `exact:N` | `Exact(i32)` | `{ "exact:0": "no apples" }` |
| `keyword:K` | `Keyword(String)` | `{ "keyword:feminine": "une" }` |

### Translation workflow (JSON)

Translators (or tooling) create a translated `lines.json` with the same structure. Each line's `content` is replaced with the localized version. Slot indices and select structure may be rearranged to match target-language grammar. Audio refs are replaced with locale-specific paths:

```json
{
  "index": 0,
  "content": "いらっしゃいませ！",
  "hash": "a1b2c3d4e5f6a7b8",
  "audio": "audio/ja/shop_welcome.ogg"
}
```

```json
{
  "index": 1,
  "content": {
    "template": [
      { "slot": 0 },
      "個の商品がございます。"
    ]
  },
  "hash": "b2c3d4e5f6a7b8c9"
}
```

The `hash` field is preserved from the source — it records which version of the source text the translation was made against. The compile step does not validate it; the regeneration workflow uses it to detect stale translations.

## Locale compilation (lines.json → .inkl)

`brink-intl` compiles a translated `lines.json` into a binary `.inkl` overlay file that the runtime loads via `load_locale()`.

### Process

1. Read the translated `lines.json`
2. Read the base `.inkb` (needed for the content checksum and scope list)
3. For each scope in the translated JSON:
   - Parse each line's `content` back into `LineContent` (plain or template)
   - Collect `audio_ref` from each line's `audio` field
   - Build a line table with the translated entries
4. Write the `.inkl` file:
   - Header: magic `b"INKL"`, format version, BCP 47 locale tag (from CLI arg), base `.inkb` checksum
   - Per-scope line tables keyed by scope `DefinitionId`

### Validation

The compile step validates:

- Every scope `DefinitionId` in the JSON exists in the base `.inkb`
- Line indices are contiguous and match the base `.inkb` line counts per scope
- Template slot indices are within bounds (the base `.inkb` knows how many slots each `EmitLine` pushes)
- Select variants use valid `SelectKey` syntax

Missing scopes are allowed — the runtime's `Overlay` mode fills them from the base locale.

## Content hashing

The `source_hash` field on each `LineEntry` enables the regeneration workflow. It is computed during compilation from the source text that produced the line.

### Hash computation

The hash is a 64-bit value derived from the **source text content** of the line — the ink text as written by the author, before any decomposition into template parts. For a plain line `Hello, {name}!`, the hash covers the entire string `Hello, {name}!`, not the decomposed template.

This means:
- Changing the text changes the hash
- Refactoring the compiler's decomposition strategy (e.g., changing what becomes a slot vs literal) does NOT change the hash
- Adding/removing whitespace or tags DOES change the hash

The hash function is not specified — any deterministic 64-bit hash is acceptable. The only requirement is consistency within a single `.inkb` build.

### Current state

`source_hash` is currently hardcoded to `0` in the compiler. Computing real hashes is the first implementation step.

### Hash input for templates

Template recognition runs during LIR lowering, where the HIR with `AstPtr` is still available. Each HIR content node's `AstPtr` provides a `TextRange` back into the CST, which can be used to slice the original source text directly. The hash is computed from this raw source text — no reconstruction, no synthetic representation.

For a content line spanning `Hello, {name}!`, the `AstPtr` gives the byte range of that entire line in the source file. The recognizer slices the source text at that range and hashes it. This is the same source text that populates `SourceLocation`, so both are computed from the same data in the same pass — no circular dependency.

## Template production (compiler-side)

The compiler currently produces only `LineContent::Plain` for all text. Template production requires a pattern recognition pass that identifies sequences of content parts that can be merged into a single `LineTemplate`.

### Pattern recognizer infrastructure

Template recognizers run during **LIR lowering**, where the HIR (with source provenance via `AstPtr`) and resolved symbols (via `SymbolIndex`) are still available. Each recognizer inspects HIR content nodes and either:

- **Matches:** consumes a run of content nodes, returns a `ContentEmission::Template` with the `LineTemplate`, slot expressions, and full metadata (source hash, slot info, source location)
- **Declines:** returns `None`, falls through to the next recognizer or to per-part lowering

This design allows incremental addition of recognizers without modifying existing ones. The recognizer that fires first wins.

### Container boundary rule

Template merging must not cross **public container boundaries** — diverts, tunnels, and function calls. Content that flows through compiler-internal containers (inline sequence wrappers, choice content containers) is still one logical block and may be merged. Under the lexical scope model, this is natural: all content within a scope shares one line table, and the scope boundary IS the public container boundary.

### Recognizer progression

Recognizers are added incrementally, starting from the simplest patterns:

**Phase 1: Plain text**
```
[Text(s)] → LineContent::Plain(s)
```
This is what the compiler already does, but routed through the recognizer infrastructure to prove the plumbing works.

**Phase 2: Single interpolation**
```
[Text(a), Interpolation(expr), Text(b)]
  → Template([Literal(a), Slot(0), Literal(b)])
  + push expr before EmitLine
```

**Phase 3: Multiple interpolations**
```
[Text(a), Interpolation(e1), Text(b), Interpolation(e2), Text(c)]
  → Template([Literal(a), Slot(0), Literal(b), Slot(1), Literal(c)])
  + push e1, push e2 before EmitLine
```

**Phase 4: Inline conditionals as Select**
```
[Text(a), InlineConditional(cond), Text(b)]
  → Template([Literal(a), Select { slot: 0, ... }, Literal(b)])
```
Requires the conditional to have statically-known branches that can be expressed as select variants.

**Phase 5: Inline sequences as slots**
```
[Text(a), InlineSequence(seq), Text(b)]
  → Template([Literal(a), Slot(0), Literal(b)])
```
The sequence index becomes a slot value.

Each phase is independently testable and shippable.

### Fallback

When no recognizer matches a run of parts, the compiler falls back to per-part emission (the current behavior). This is always correct — it just doesn't produce localizable templates for that content.

## Regeneration workflow

When source `.ink` files change and are recompiled, existing translations must be preserved. The regeneration workflow diffs a new `.inkb` against an existing translated `lines.json` (or XLIFF) and produces an updated file.

### Matching strategy

Matching operates in two tiers: **scope matching** then **line matching within each scope**.

**Scope matching** uses `DefinitionId` — the lexical scope's identity, stable across recompiles (hash of fully qualified path). A knot renamed or moved gets a new `DefinitionId` and its translations are orphaned. Scopes present in the old translation but absent from the new `.inkb` are orphaned. Scopes in the new `.inkb` with no old translation are new.

**Line matching** within a scope uses `source_hash` alignment, not line index. Line indices are unstable — inserting or deleting a line shifts all subsequent indices within the scope. Matching by index would incorrectly mark every shifted line as changed.

Instead, the regeneration tool aligns the old and new hash sequences within each scope using longest common subsequence (LCS). The hash sequence is the ordered list of `source_hash` values from the line table. LCS finds the largest set of lines that appear in the same relative order in both old and new, identifying which lines are unchanged, which are inserted, and which are deleted.

### Alignment algorithm

Given old hashes `[h0, h1, h2, h3, h4]` and new hashes `[h0, h1, hX, h2, h3, h4]` (line inserted at index 2):

1. Compute LCS of the two hash sequences → `[h0, h1, h2, h3, h4]`
2. LCS-aligned pairs are **unchanged** — preserve translation, update index
3. New-side elements not in the LCS (`hX` at new index 2) are **insertions** → `untranslated`
4. Old-side elements not in the LCS are **deletions** → `orphaned`

Deletion works symmetrically: old `[h0, h1, h2, h3, h4]` → new `[h0, h1, h3, h4]` produces LCS `[h0, h1, h3, h4]`, and `h2` is orphaned.

### Duplicate hashes

A scope may contain duplicate lines (same text, same hash). LCS handles this naturally — it aligns by position within the sequence, so two identical lines at positions 3 and 7 are matched to their positional counterparts, not confused with each other.

### Changed lines

When a line's text is edited (not inserted or deleted), its hash changes. LCS will not align the old and new versions — the old hash appears as a deletion and the new hash as an insertion. To detect edits vs insert+delete pairs, the tool applies a secondary heuristic after LCS:

For each unmatched old line and unmatched new line at the **same original position** (or adjacent positions), if they are the only unmatched lines in that region, treat them as an **edit** → `needs_review`, preserve old translation as reference.

This heuristic is best-effort. Ambiguous cases (multiple adjacent changes) fall back to orphan + untranslated, which is safe — no translation is silently applied to wrong content.

### Diff summary

| Alignment result | Status | Action |
|-----------------|--------|--------|
| LCS-aligned (same hash, possibly different index) | `translated` | Preserve translation, update to new index |
| New-side only, no edit match | `untranslated` | New line, no translation |
| Old-side only, no edit match | `orphaned` | Removed line, mark or discard (configurable) |
| Positional edit match (old hash ≠ new hash) | `needs_review` | Preserve old translation as reference, flag for review |

### Status tracking

Each line in the translation file carries an implicit status:

| Status | Meaning |
|--------|---------|
| `untranslated` | No translation provided (content field absent or null) |
| `translated` | Translation provided, hash matches current source |
| `needs_review` | Translation exists but source hash changed since translation was made |
| `orphaned` | Line existed in previous source but not in current `.inkb` |

In `lines.json`, status is tracked implicitly:
- Untranslated: `content` is `null`
- Translated: `content` is present
- Needs review: `content` is present but `hash` doesn't match the current source hash
- Orphaned: present in old file, absent in new `.inkb`

## XLIFF workflow (future)

The JSON workflow is sufficient for programmatic use and basic manual translation. For professional translation workflows, `brink-intl` will support XLIFF 2.0 import/export via a separate general-purpose XLIFF crate.

### XLIFF crate

A general-purpose Rust XLIFF 2.0 crate (not brink-specific, publishable to crates.io). Provides:

- **Data model:** `Document`, `File`, `Unit`, `Segment`, `Source`, `Target`, inline elements (`Ph`, `Sc`, `Ec`, `Mrk`)
- **Write:** serialize data model to XLIFF 2.0 XML
- **Read:** parse XLIFF 2.0 XML to data model
- **Round-trip:** preserve unknown extensions and attributes through read/write cycles

Built on `quick-xml`. No brink-specific types or logic.

### brink-intl XLIFF integration

`brink-intl` maps between its line table model and XLIFF:

| brink concept | XLIFF element |
|---------------|---------------|
| Lexical scope | `<file>` |
| Scope `DefinitionId` | `<file id="...">` |
| Line | `<unit>` |
| `LineId` | `<unit id="scope_id:line_idx">` |
| `LineContent::Plain` | `<source>` with text content |
| `LineContent::Template` | `<source>` with inline `<ph>` elements for slots |
| `source_hash` | `brink:hash` extension attribute on `<unit>` |
| `audio_ref` | `brink:audio` extension attribute on `<unit>` |
| Translation status | XLIFF `state` attribute (`initial`/`translated`/`reviewed`/`final`) |

Brink-specific metadata uses XLIFF's custom namespace extension mechanism (`xmlns:brink="urn:brink:xliff:extensions:1.0"`). The XLIFF spec requires conformant tools to preserve unknown extensions, so brink metadata survives round-trips through translation management systems.

### XLIFF generation

```
brink generate-locale --input story.inkb --output en.xliff --source-lang en
```

Reads `.inkb`, walks line tables, emits XLIFF 2.0 with scopes as `<file>` elements and lines as `<unit>` elements. Equivalent to exporting `lines.json` but in XLIFF format.

### XLIFF regeneration

```
brink regenerate-locale --input story.inkb --existing en-to-ja.xliff --output en-to-ja.xliff
```

Same diff rules as the JSON regeneration workflow, but operating on XLIFF:
- Unchanged lines: preserve `<target>` and `state`
- Changed lines: update `<source>`, reset `state` to `initial`, preserve `<target>` as reference
- New lines: add `<unit>` with `state="initial"`
- Orphaned lines: remove or mark (configurable)

### XLIFF to .inkl compilation

```
brink compile-locale --input ja.xliff --base story.inkb --output ja.inkl --locale ja
```

Reads translated XLIFF, extracts `<target>` content, and compiles to `.inkl`. Equivalent to the JSON compile path but reading from XLIFF.

## Plural resolution

The runtime defines a `PluralResolver` trait (see [format-spec](format-spec.md)):

```
trait PluralResolver {
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory;
    fn ordinal(&self, n: i64) -> PluralCategory;
}
```

`brink-intl` provides a batteries-included implementation backed by ICU4X baked data.

### ICU4X resolver

The `brink-intl` resolver uses ICU4X's `PluralRules` with baked data, pruned at build time to only the locales the consumer specifies via Cargo features.

```rust
// Cargo.toml
[dependencies]
brink-intl = { version = "...", features = ["locale-en", "locale-ja", "locale-ar"] }
```

Each locale feature gates the baked CLDR data for that locale. The resolver binary contains only the plural rules for enabled locales. No runtime data loading.

### Fallback resolver

Stories without localization don't need a resolver. When no resolver is provided, all plural lookups fall back to `PluralCategory::Other`. This is the default behavior — a story compiled without templates works identically with or without `brink-intl`.

## CLI commands

All localization commands are subcommands of the `brink` CLI:

| Command | Description |
|---------|-------------|
| `brink export-lines` | Export line tables from `.inkb` to `lines.json` |
| `brink compile-locale` | Compile translated `lines.json` to `.inkl` |
| `brink regenerate-lines` | Diff new `.inkb` against existing `lines.json`, produce updated file |
| `brink generate-locale` | (future) Export line tables from `.inkb` to XLIFF |
| `brink regenerate-locale` | (future) Diff new `.inkb` against existing XLIFF |

## Open questions

### Template boundaries and glue

The default template boundary is the **end of line** — each source line of ink content produces at most one line table entry. Glue (`<>`) suppresses line breaks and can cause content from adjacent source lines to merge at runtime, but recognizers are not required to merge across glue boundaries initially.

Recognizers MAY absorb glue and merge across source line boundaries in the future. This is a natural extension of the recognizer infrastructure — a more aggressive recognizer can consume `[Text, Glue, Text, Interpolation, Text]` as a single template. But the initial implementation treats end-of-line as the boundary.

### Choice text in the export

Choices produce two lines (display + output) via the dual-path model. Under the lexical scope model, both lines live in the same scope table, and source provenance makes the pairing obvious in context. The `choice` field (`"display"` / `"output"`) provides an explicit hint but is not strictly necessary — a translator can see the context from the surrounding lines and source ranges. If more explicit metadata is needed in the future, line entries can carry additional tags.

### Scope export filtering

`export-lines` emits all scopes that have at least one line entry. Scopes with zero line entries (pure control flow, no user-visible text) are omitted. No other filtering is applied — trivial scopes with one or two lines are included.

### EmitLine slot count

`EmitLine` gains a slot count parameter: `EmitLine(line_idx: u16, slot_count: u8)`. Codegen pushes `slot_count` values onto the stack before emitting the opcode. The runtime pops that many values and passes them to the template resolver.

For `LineContent::Plain` lines, `slot_count` is `0` and no values are popped. For templates, `slot_count` equals the number of distinct `Slot` and `Select` parts in the template. The runtime can validate at execution time that the slot count matches the template's actual slot count — a mismatch is a codegen bug and should produce a runtime error, not silent corruption.

`EvalLine` (push line content as string value) gets the same treatment: `EvalLine(line_idx: u16, slot_count: u8)`.

This change needs to be reflected in format-spec (opcode encoding) and runtime-spec (execution semantics).

### .inkl binary layout

The format-spec gives a one-line description of `.inkl` layout. An implementer writing `.inkl` serialization needs the byte-level format: section headers, scope-to-table indexing, entry counts, per-entry encoding. The layout follows the same principles as `.inkb` (length-prefixed sections, DefinitionId keys) but the specific encoding needs to be specified in format-spec before Phase 2 implementation. The `.inkl` carries `content` and `audio_ref` per line entry — no slot info or source location.

## Implementation phasing

### Phase 1: Foundation

1. Reconcile lexical scope line table model into format-spec and runtime-spec
2. Compute real `source_hash` in codegen (currently hardcoded to `0`)
3. Pattern recognizer infrastructure in LIR lowering — plain text recognizer
4. Create `brink-intl` crate with `export-lines` (`.inkb` → `lines.json`)

### Phase 2: Round-trip

5. Specify `.inkl` binary layout in format-spec
6. `.inkl` binary write support in `brink-format`
7. `compile-locale` in `brink-intl` (`lines.json` → `.inkl`)
8. Runtime `.inkl` loading (wiring up the existing `load_locale` spec)

### Phase 3: Templates

9. Single-interpolation pattern recognizer (with slot info + source location metadata)
10. Multi-interpolation pattern recognizer
11. Template-aware `lines.json` export
12. Template-aware `resolve_line` in the runtime (replacing the `"[template]"` stub)

### Phase 4: Regeneration

13. `regenerate-lines` workflow (`lines.json` diffing)
14. Status tracking in `lines.json`

### Phase 5: XLIFF

15. General-purpose XLIFF 2.0 crate
16. `generate-locale` (XLIFF export)
17. `regenerate-locale` (XLIFF diffing)
18. XLIFF → `.inkl` compilation

### Phase 6: Plural resolution

19. ICU4X-backed `PluralResolver` in `brink-intl`
20. Locale feature gates for baked data pruning
