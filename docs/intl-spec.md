# brink internationalization specification

`brink-intl` provides localization tooling for brink stories — line table export/import, locale overlay compilation, XLIFF round-trip, plural resolution, and translation regeneration. It depends on `brink-format` (types and file formats) and consumes compiled `.inkb` files. See [format-spec](format-spec.md) for line table types and `.inkl` layout, [runtime-spec](runtime-spec.md) for locale loading and line resolution, [compiler-spec](compiler-spec.md) for text decomposition and template production.

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
                    │  export:   .inkb → .xliff        │
                    │  import:   .xliff → lines.json   │
                    │  regen:    .inkb + .xliff → .xliff│
                    │                                  │
                    │  plural:   IcuPluralResolver     │
                    └───────────────┬─────────────────┘
                                    │
                    ┌───────────────▼─────────────────┐
                    │          brink-runtime           │
                    │  .inkb + .inkl → localized story │
                    └─────────────────────────────────┘
```

`brink-intl` sits between the compiler and runtime. The compiler produces `.inkb` files with base-locale line tables. `brink-intl` exports those line tables for translation and compiles translated content back into `.inkl` overlays that the runtime loads.

A separate, general-purpose XLIFF 2.0 crate (`xliff2`) handles the XLIFF format (data model, read, write). `brink-intl` depends on it for XLIFF import/export but does not require it for the JSON-based workflow.

## Line table scoping

Line tables are scoped to **lexical scopes** — knots and stitches. A knot and all its internal containers (gathers, choice targets, inline sequence wrappers) share one line table. `EmitLine(5)` means "line 5 of the current scope's line table."

### Implementation

`ScopeLineTable` in `brink-format` groups line entries by scope `DefinitionId`. Each `ContainerDef` stores a `scope_id` field pointing to its enclosing lexical scope. At link time, the linker builds a `scope_id → table_index` mapping, and each `LinkedContainer` stores a `scope_table_idx` so the VM can resolve `program.line_table(container_idx)` in O(1).

Scope-defining containers (root, knots, stitches) introduce their own scope. Non-scope containers (gathers, choice targets, inline sequences) inherit their parent's scope. Line indices are assigned during codegen per-scope — `add_line()` appends to the scope's table, and the returned index is relative within that scope.

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
```

This is better for localization because:
- Line tables group at the level translators think about (knots/stitches, not gathers)
- Fewer, larger tables with more stable indices
- The export shows meaningful units of narrative, not compiler fragments
- The line table boundary IS the public container boundary — no separate "boundary rule" needed

And better for the runtime because:
- Lookup is `program.line_table(container_idx)` → scope table — O(1) with no search
- The mapping is implicit via `LinkedContainer.scope_table_idx`, set once at link time

## Codegen contract

Localization tooling operates post-compilation — it reads `.inkb` files and extracts everything it needs from `StoryData`. The compiler must put enough information into `LineEntry` during codegen for the tooling to function.

### LineEntry

```rust
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
    range_start: u32,                  // byte offset start in the source file
    range_end: u32,                    // byte offset end in the source file
}
```

**`content`** — `Plain` for simple text, `Template` for interpolated text. The compiler's LIR recognizer (`recognize.rs`) identifies interpolation patterns and produces templates with `Literal` and `Slot` parts.

**`source_hash`** — computed via `brink_format::content_hash()` (64-bit, Rust `DefaultHasher`). For plain text, hashes the text directly. For templates, hashes a normalized form with `"{…}"` placeholders for interpolations. Computed during LIR recognition where source text is still available via `AstPtr`.

**`slot_info`** — for each `Slot(n)` in a template, the compiler records the source expression that produced that slot value. This lets tooling display `{player_name}` instead of `{slot 0}`. Populated during LIR lowering. The runtime ignores this field.

**`audio_ref`** — an audio asset identifier associated with this line, populated by external tooling (not by the compiler). Stored alongside content so that localized versions can provide locale-specific audio. Both `.inkb` and `.inkl` carry audio refs — the localized version replaces the base.

**`source_location`** — the file and byte range of the ink source text that produced this line. Populated during LIR lowering from `AstPtr` → `TextRange`. The runtime ignores this field.

### Template recognition

Template recognition runs during **LIR lowering** in `recognize.rs`. The recognizer inspects HIR content nodes and either:

- **Matches:** produces a `RecognizedLine::Template` with the `LineTemplate`, slot expressions, and full metadata (source hash, slot info, source location)
- **Declines:** falls through to plain text recognition or per-part lowering

The LIR `RecognizedLine` enum carries the recognition result:

```rust
enum RecognizedLine {
    Plain { text: String, metadata: LineMetadata },
    Template { template_parts: Vec<LinePart>, slot_exprs: Vec<Expr>, metadata: LineMetadata },
}

struct LineMetadata {
    source_hash: u64,
    slot_info: Vec<SlotInfo>,
    source_location: Option<SourceLocation>,
}
```

Codegen handles both:
- `RecognizedLine::Template` → evaluate slot expressions (push to stack) + `EmitLine(idx, slot_count)`, add enriched `LineEntry` with `LineContent::Template` to the scope's line table
- `RecognizedLine::Plain` → `EmitLine(idx, 0)`, add `LineEntry` with `LineContent::Plain`

### Implemented recognizers

**Plain text:** `[Text(s)]` → `LineContent::Plain(s)`. Single text part with no interpolations.

**Interpolation templates:** `[Text, Interpolation, Text, ...]` with at least one `Interpolation` → `LineContent::Template([Literal, Slot, Literal, ...])`. Handles single and multiple interpolations. Each interpolation becomes a `Slot(n)` with its expression pushed to the stack before `EmitLine`.

### Branch expansion (inline conditionals/sequences)

**RETRACTED (2026-07-28, issue #1667):** this section previously proposed two "future recognizers" — inline conditionals lowering to `LinePart::Select`, inline sequences lowering to a `Slot` carrying the branch index. Neither is the mechanism; the 2026-03-15 decision-log ruling was implemented differently, and `LinePart::Select` is **not** part of it — it stays target-side only, a translator-introduced variation compiled from a hand-authored `.inkl` overlay via `compile-locale`. Routing source-side branching through it would mean the runtime assembles a line's text from parts at read time, which is exactly what the ruling replaces.

The actual mechanism turned out to already be half-built: `hir::normalize_file` (added 2026-03-15, the same day as the ruling) lifts an inline conditional/sequence out of its content line into a block-level `Sequence`/`Conditional` statement, splicing the surrounding prefix/suffix text into every branch and giving each branch its own child container — so each branch already reaches the ordinary content recognizer (`try_recognize`) independently, gets its own `LineEntry`, and the runtime already *selects* the branch (the same condition-evaluation / `CurrentVisitCount`-driven bytecode used for block-level conditionals/sequences) rather than assembling text inline. What was missing (issue #1667's actual fix) was much smaller: the spliced prefix/branch/suffix content parts were never *merged* — they stayed three separate adjacent `Text` parts, so the recognizer's `Plain` pattern (`parts.len() == 1`) could never match, and every branch silently fell back to `EmitContent`, which still emits one line-table entry **per fragment**. `normalize.rs::extend_merging_text` merges adjacent `Text` parts at each splice seam, trimming the right-hand side's leading whitespace at the join — that one fix is what actually lets a spliced branch recognize as `Plain`/`Template` and get a real, single `LineEntry`. The merge only trims one side, so a seam can still carry doubled whitespace or a literal tab through to codegen; rendered output stays identical anyway because `add_line_with_hash` (`brink-codegen-inkb/src/lib.rs`) runs every `Plain`/`Template` line through `collapse_whitespace` before it lands in the line table, the same as it always has.

A branch whose merged content still doesn't reduce to a single `Text` part (`try_recognize`'s `Plain` pattern) or a run of only `Text`/`Interpolation` parts with at least one interpolation (`Template`) still falls back to the pre-ruling per-fragment `EmitContent` path for that branch — for example, an interior `Glue` or `Spring` part left adjacent to a `Text` part after splicing. No arbitrary complexity threshold was added; recognition either succeeds on the merged flat line or falls back exactly as it always has.

**Known gap, not part of this fix:** `normalize_file` only walks `Block.stmts` (root/knot/stitch bodies, gather continuations, choice *bodies*) — it does not touch a `Choice`'s own display/bracket/inner text. An inline conditional/sequence embedded directly in choice display text (`* Pick {x: A|B}`) still keeps its `ContentPart::InlineConditional`/`InlineSequence` shape all the way to codegen and is still assembled from parts at runtime. Filed as a follow-up on issue #1667 rather than folded into this change.

**`source_hash`/TM impact:** this fix does not change which lines get a `source_hash` or what canonicalizes into one — a branch that was already being spliced by `normalize_file` (present since 2026-03-15) keeps the same composed text and the same `source_hash`; it just now reaches `RecognizedLine::Plain`/`Template` (one clean `LineEntry`) instead of falling back to `EmitContent`'s per-fragment `LineEntry`s. Any `.xlf` unit that was already exported from the fragment path is affected: `EmitContent`'s fragments are keyed by their own (fragment-level) `source_hash`, distinct from the merged line's single `source_hash`, so an existing translation compiled against the old, per-fragment line table would be orphaned. See the PR body for issue #1667 for the exact oracle before/after — this is a genuine migration question for any story with inline conditionals/sequences that already has translated `.xlf` units, not something absorbed silently by this change.

### Container boundary rule

Template merging does not cross **public container boundaries** — diverts, tunnels, and function calls. Content within compiler-internal containers (inline sequence wrappers, choice content containers) is still one logical block and may be merged. Under the lexical scope model, this is natural: all content within a scope shares one line table, and the scope boundary IS the public container boundary.

### Binary format

The `.inkb` line table section encodes per entry:

```
LineEntry encoding:
  [content + source_hash]
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

The `.inkl` overlay format carries `content` and `audio_ref` but NOT slot info or source location — those are source-language concerns, not translation concerns.

## Line table export (lines.json)

The primary export format is a JSON file representing the full line table contents of a compiled `.inkb`. The simplest way to inspect, edit, and round-trip line tables without XLIFF tooling.

Implemented in `brink_intl::export_lines()`.

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

Implemented in `brink_intl::compile_locale()`.

### Process

1. Read the translated `lines.json`
2. Read the base `.inkb` (needed for the content checksum and scope list)
3. For each scope in the translated JSON:
   - Resolve its `DefinitionId` against the base — directly, else through the base `.inkb`'s `#@was` alias table (see [Matching strategy](#matching-strategy))
   - Parse each line's `content` back into `LineContent` (plain or template)
   - Collect `audio_ref` from each line's `audio` field
   - Build a line table with the translated entries
4. Write the `.inkl` file:
   - Header: magic `b"INKL"`, format version, BCP 47 locale tag (from CLI arg), base `.inkb` checksum
   - Per-scope line tables keyed by scope `DefinitionId`

### Validation

The compile step validates:

- Every scope `DefinitionId` in the JSON exists in the base `.inkb`, either directly or after `#@was` alias rebinding (see [Matching strategy](#matching-strategy)); one that resolves to neither is `IntlError::ScopeNotInBase`, naming the id the file carried
- Two scopes in the JSON do not land on the same base scope via a rebind (`IntlError::AmbiguousScopeRebind`) — letting the last one win would silently drop a translation
- Line indices are contiguous and match the base `.inkb` line counts per scope
- Template slot indices are within bounds (the base `.inkb` knows how many slots each `EmitLine` pushes)
- Select variants use valid `SelectKey` syntax

Missing scopes are allowed — the runtime's `Overlay` mode fills them from the base locale.

## Content hashing

The `source_hash` field on each `LineEntry` enables the regeneration workflow. It is computed during compilation from the source text that produced the line.

### Hash computation

The hash is a 64-bit value computed via `brink_format::content_hash()` using Rust's `DefaultHasher`. The input depends on the line type:

- **Plain text:** hashes the text string directly.
- **Templates:** hashes a normalized form where literal text is preserved and each interpolation is replaced with `"{…}"`. This means refactoring the compiler's decomposition strategy (e.g., changing what becomes a slot vs literal) does NOT change the hash.

This means:
- Changing the text changes the hash
- Adding/removing whitespace or tags DOES change the hash
- The hash is stable across compiler internals changes

The hash is computed during LIR recognition where source text is still available via `AstPtr`. Each HIR content node's `AstPtr` provides a `TextRange` back into the CST, which is used to derive the hash input. This is the same source data that populates `SourceLocation`, so both are computed in the same pass.

## Template production (compiler-side)

The compiler produces `LineContent::Template` entries via the LIR recognition pass in `brink-ir/src/lir/lower/recognize.rs`.

### How it works

The recognizer inspects sequences of HIR content parts for each line. When a line contains at least one `Interpolation` part mixed with `Text` parts, the recognizer:

1. Walks the parts, building `LinePart::Literal` for text and `LinePart::Slot(n)` for interpolations
2. Collects the slot expressions (for stack evaluation before `EmitLine`)
3. Computes `LineMetadata` (source hash, slot info with expression names, source location)
4. Returns `RecognizedLine::Template`

Codegen (`brink-codegen-inkb/src/content.rs`) then:
1. Evaluates each slot expression (pushing values to the stack)
2. Calls `add_template_line()` to register the `LineContent::Template` in the scope's line table
3. Emits `Opcode::EmitLine(idx, slot_count)` where `slot_count = slot_exprs.len()`

The runtime's `resolve_line()` pops `slot_count` values from the stack, then walks the template parts — substituting `Slot(n)` with the stringified stack value and resolving `Select` parts via the plural resolver cascade.

### Template boundaries and glue

The template boundary is the **end of line** — each source line of ink content produces at most one line table entry. Glue (`<>`) suppresses line breaks and can cause content from adjacent source lines to merge at runtime, but recognizers do not merge across glue boundaries.

### Fallback

When no recognizer matches a run of parts (e.g., content with only text and no interpolations, or patterns not yet recognized), the compiler falls back to `LineContent::Plain` or per-part emission. This is always correct — it just doesn't produce localizable templates for that content.

## Regeneration workflow

When source `.ink` files change and are recompiled, existing translations must be preserved. The regeneration workflow diffs a new `.inkb` against an existing translated `lines.json` (or XLIFF) and produces an updated file.

Implemented in `brink_intl::regenerate_lines()` (JSON) and `brink_intl::regenerate_locale()` (XLIFF).

### Matching strategy

Matching operates in two tiers: **scope matching** then **line matching within each scope**.

**Scope matching** uses `DefinitionId` — the lexical scope's identity, stable across recompiles (hash of fully qualified path). A knot renamed or moved gets a new `DefinitionId`, so an *undeclared* rename orphans its translations. Scopes present in the old translation but absent from the new `.inkb` are orphaned. Scopes in the new `.inkb` with no old translation are new.

**Alias rebinding (`#@was`).** A scope whose id moved because the rename was *declared* is not orphaned. Both `regenerate_lines` and `compile_locale` consult the compiled `#@was` alias table (`docs/modules-spec.md` §5) — the same old→new edges the save path uses — and rebind the moved scope instead of treating it as gone:

| Surface | Direction | Where the edges come from |
|---------|-----------|---------------------------|
| `regenerate_lines` / `regenerate_locale` | new id → pre-rename ids | `StoryData::alias_table`, passed by the caller |
| `compile_locale` / `compile_locale_xliff` | stale id → current id | the base `.inkb`'s `AliasTable` section, read in-place |

Rules that hold for both directions:

- **A direct id match always wins** over a rebind, so an already-regenerated file is unaffected.
- **Rebinding is keyed on the id alone** and never reads the scope's display name, so anonymous scopes — root content is one — rebind exactly like named knots.
- **Alias chains are not followed**, mirroring `Program::resolve_alias`: the compiler emits `old -> new` against the definition's *current* id, never `old -> old2`.
- **The rebind is a one-time cost.** Regeneration writes the new ids into its output, so the `#@was` directive stays deletable after its migration window.
- **The `#@was` net is transitive** (issue #1671): a rename mints one entry per definition the directive covers *and* one entry per descendant re-keyed only because its parent was renamed (every stitch/label whose qualified name is prefixed by the renamed container). Re-declaring `#@was` on an untouched descendant is never necessary and does nothing useful — the old name would be qualified with the *current* parent name, so that edge collapses to a self-edge (pinned by `rename_identity.rs::stitch_was_cannot_bridge_an_ancestor_rename`); the transitive bridge is minted automatically by the ancestor's own `#@was` instead.

Rebinding is automatic rather than a separate CLI migration step (`migrate_unit_ids` / `brink migrate-xliff` remains scoped to the one-off `<unit id>` *spelling* migration, which is a format change, not an id move): the alias edges already ride in the artifacts both surfaces read, an id move can happen on any recompile, and a manual step would fail open every time an author forgot to run it.

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

## XLIFF workflow

For professional translation workflows, `brink-intl` supports XLIFF 2.0 import/export via the `xliff2` crate.

### xliff2 crate

A general-purpose Rust XLIFF 2.0 crate (`crates/internal/xliff2/`). Provides:

- **Data model:** `Document`, `File`, `Unit`, `Segment`, `Source`, `Target`, inline elements (`Ph`, `Pc`, `Sc`, `Ec`, `Mrk`, `Cp`, `Sm`, `Em`)
- **Write:** serialize data model to XLIFF 2.0 XML
- **Read:** parse XLIFF 2.0 XML to data model
- **Round-trip:** preserve unknown extensions and attributes through read/write cycles

Built on `quick-xml`. No brink-specific types or logic.

### brink-intl XLIFF integration

`brink-intl` maps between its line table model and XLIFF via `xliff_convert.rs`:

| brink concept | XLIFF element |
|---------------|---------------|
| Lexical scope | `<file>` |
| Scope display name, else the scope id (`knot.stitch`, or empty string for root) | `<file id="...">` |
| Scope `DefinitionId` | `brink:scope-id` extension attribute on `<file>` |
| Line | `<unit>` |
| `LineId` | `<unit id="scope_id:line_idx">` |
| `LineContent::Plain` | `<source>` with text content |
| `LineContent::Template` | `<source>` with inline `<ph subType="brink:slot">` elements for slots (the `subType` marker is the brink-ownership signal, #1823; pre-#1823 exports carried only `id="s{n}"` + `equiv="{slot n}"`, which the importer still accepts — see the import table) |
| `PartJson::Span` (non-empty `children`, #1734) | paired `<pc>` inline code, `dataRefStart` -> `<data>` carrying `{name, attrs}` |
| `PartJson::Span` (empty `children`, point marker, #1734) | standalone `<ph subType="brink:x">`, `dataRef` -> `<data>` carrying `{name, attrs}` |
| `source_hash` | `brink:hash` extension attribute on `<unit>` |
| `audio_ref` | `brink:audio` extension attribute on `<unit>` |
| Translation status | XLIFF `state` attribute (`initial`/`translated`/`reviewed`/`final`) |

Brink-specific metadata uses XLIFF's custom namespace extension mechanism (`xmlns:brink="urn:brink:xliff:extensions:1.0"`). The XLIFF spec requires conformant tools to preserve unknown extensions, so brink metadata survives round-trips through translation management systems.

#### Import direction: disposition of every inline element

The table above is export-direction (brink → XLIFF). The import direction (`xliff_to_lines_json` → `elements_to_parts`) must also handle everything a *translation management system* may put in a `<source>`/`<target>` that brink's own exporter never emits. Translator work that reaches import and is not decoded is unrecoverable — there is no second copy — so `elements_to_parts` matches every `xliff2::InlineElement` variant explicitly (no `_` catch-all: a new variant is a compile error, not a silent drop). Each element is **decoded**, **explicitly rejected**, or **ignored**:

| XLIFF element | disposition | rationale |
|---------------|-------------|-----------|
| text, `<![CDATA[...]]>` | decoded as literal text | CDATA is plain character data spelled with a different XML quoting mechanism — no structural ambiguity, nothing to lose (#1799) |
| `<cp hex="…"/>` | decoded to the character it names | XLIFF's escape hatch for a character its producer could not or would not write literally; the code point *is* the content, and there is no structure to reconstruct (#1811). An out-of-range or unparseable `hex` is `IntlError::InvalidCodePoint` |
| `<ph>` (brink span marker / select / slot) | decoded to `Span`/`Select`/`Slot` | brink's own standalone-code shapes |
| `<ph>` (foreign) | ignored (#1823) | brink's own shapes are recognized by brink-owned markers only: `Select` requires the `dsel{n}` `dataRef` prefix, `Slot` requires `subType="brink:slot"` (or the legacy pre-#1823 pair `id="s{n}"` **and** `equiv="{slot n}"`, kept so old brink exports re-import losslessly). Anything else — whatever its `id` spelling, `dataRef`, or namespace-stripped local name — is a foreign standalone code: `<ph>` is an empty element holding attributes only, so ignoring it cannot lose translator work |
| `<pc>` (brink) | decoded as a span, recursing into its children | brink's own paired-span shape |
| `<pc>` (foreign — no brink `subType`/`dataRefStart` marker, per `looks_like_span_marker`) | children spliced in place (#1823) | a TMS paired code (bold/italic markup, an extension-namespace element whose local name collides with `pc`) has a real content model that can hold translator text; the wrapper's own identity is not brink content, but what it wraps must survive — the same treatment as foreign `<mrk>` |
| `<sc>`/`<ec>`/`<mrk>` carrying a brink `subType`/`dataRef` | explicitly rejected — `IntlError::UnsupportedSpanSplit` | a brink `<pc>` a tool re-expressed as a split pair or a wrapping mark: structure is lost and cannot be rebuilt (#1734) |
| `<mrk>` (foreign) | children spliced in place | a TMS annotation — terminology, reviewer comment, QA flag — wraps a *span of translated text*. Its `id`/`type`/`ref`/`value` are not brink content and are dropped, but the marked text is translator work and must survive (#1812) |
| `<sc>`/`<ec>` (foreign), `<sm>`, `<em>` | ignored | empty elements carrying attributes only. The text such a pair *spans* is a sibling of the markers, not a child, so it is already recovered as ordinary text; ignoring the markers drops annotation metadata, never translator work |

The table above assumes every element in a `<source>`/`<target>` reached `elements_to_parts` as an `xliff2::InlineElement` in the first place. That assumption depended on a second catch-all one layer down, at the XML-reading boundary itself (#1824).

#### Reading direction: the XML-boundary catch-all (#1824)

`elements_to_parts` (above) can only be as exhaustive as the `Vec<InlineElement>` `xliff2::read::read_inline_content` (`crates/internal/xliff2/src/read/inline.rs`) hands it. Before #1824, that function had its own, separate catch-alls: an unrecognized `Event::Start` routed to `skip_element`, which discarded every byte of text nested inside it, and an unrecognized `Event::Empty` hit a bare no-op. Text carried inside a TMS-authored extension element wrapping a `<target>` — the same shape a QA flag or reviewer comment uses — never became an `InlineElement` at all, so no amount of exhaustive matching in `elements_to_parts` could recover it: the drop happened before brink-intl's model existed in memory. This was "the one remaining defeat of #1821."

`read_inline_content`'s `Event::Start`/`Event::Empty` matches are exhaustive over XLIFF 2.0's *closed* inline-markup vocabulary (Core §5.4-§5.6: `<ph>`, `<pc>`, `<sc>`/`<ec>`, `<mrk>`, `<sm>`/`<em>`, and the standalone `<cp>` escape) — XLIFF 2.0 has no extensibility point that adds a *new* inline element name to `<source>`/`<target>` content, and brink's own exporter never emits one outside this set. So an element name reaching either catch-all is, by construction, TMS extension markup (§10) — the same category #1821 already had to reason about for a foreign, non-brink-marked `<mrk>`.

The two catch-alls resolved differently, because they face different XML shapes:

- **`Event::Start`** (a name with a body) now recurses into the wrapper with `read_inline_content` itself and splices the resulting children directly into the parent's element list — the same "the wrapper is not brink content, but what it wraps is" reasoning #1821 applied to foreign `<mrk>`, just applied to an element name the reader has never heard of. This handles arbitrary nesting depth, CDATA, and known elements nested inside an unknown wrapper (a `<ph>` inside a memoQ `<mq:comment>` still decodes as `Ph`), because the recursive call still dispatches on every recognized name.
- **`Event::Empty`** (a self-closing name) stays a no-op — deliberately, not by oversight. An empty element is, by XML's own grammar, incapable of carrying character data, so there is no text this arm could ever discard; it is the direct analog of the foreign `<sc>`/`<ec>`/`<sm>`/`<em>` "ignore" arms above, which rest on the identical premise.

This is a knowing tradeoff, not a free lunch: a nested extension element can carry TMS *metadata* prose rather than translation — a memoQ `<mq:reason>` holding a QA note (`"Bonjour <mq:comment><mq:reason>QA flag text</mq:reason>le monde</mq:comment>"`) is indistinguishable, by name alone, from a nested element that genuinely wraps more translated text, and the reader cannot tell the two apart. #1824 accepts that over-recovery — occasionally splicing in TMS commentary as if it were content — in preference to the alternative, which is unrecoverable silent loss of real translator work whenever the guess goes the other way.

### XLIFF generation

```
brink export-xliff --input story.inkb --output en.xliff --source-lang en
```

Implemented in `brink_intl::generate_locale()`. Reads `.inkb`, exports `LinesJson` via `export_lines()`, converts to XLIFF via `lines_json_to_xliff()`. Scopes become `<file>` elements, lines become `<unit>` elements.

### XLIFF regeneration

```
brink regenerate-xliff --input story.inkb --existing en-to-ja.xliff --output en-to-ja.xliff
```

Implemented in `brink_intl::regenerate_locale()`. Same diff rules as the JSON regeneration workflow, but operating on XLIFF:
- Unchanged lines: preserve `<target>` and `state`
- Changed lines: update `<source>`, reset `state` to `initial`, preserve `<target>` as reference
- New lines: add `<unit>` with `state="initial"`
- Orphaned lines: removed from output

### XLIFF to .inkl compilation

```
brink compile-locale --input ja.xliff --base story.inkb --output ja.inkl --locale ja
```

Implemented in `brink_intl::compile_locale_xliff()`. Converts XLIFF back to `LinesJson` via `xliff_to_lines_json()`, then delegates to `compile_locale()` for `.inkl` generation.

## Plural resolution

The `PluralResolver` trait is defined in `brink-format`:

```rust
pub trait PluralResolver {
    fn cardinal(&self, n: i64, locale_override: Option<&str>) -> PluralCategory;
    fn ordinal(&self, n: i64) -> PluralCategory;
}
```

### IcuPluralResolver

`brink-intl` provides `IcuPluralResolver`, backed by ICU4X's `PluralRules` with baked CLDR data (`icu_plurals` + `icu_locale_core`). All locales are shipped — the compiled data is ~50KB total, so per-locale feature gating is unnecessary.

```rust
use brink_intl::IcuPluralResolver;

let resolver = IcuPluralResolver::new("en")?;
assert_eq!(resolver.cardinal(1, None), PluralCategory::One);
assert_eq!(resolver.cardinal(2, None), PluralCategory::Other);

// Locale override for individual calls
assert_eq!(resolver.cardinal(0, Some("ar")), PluralCategory::Zero);

// Ordinal
assert_eq!(resolver.ordinal(1), PluralCategory::One);   // 1st
assert_eq!(resolver.ordinal(2), PluralCategory::Two);   // 2nd
assert_eq!(resolver.ordinal(3), PluralCategory::Few);   // 3rd
assert_eq!(resolver.ordinal(4), PluralCategory::Other); // 4th
```

### DefaultPluralResolver

`brink-intl` also exports `DefaultPluralResolver`, a no-op that always returns `PluralCategory::Other`. Used as the fallback when no locale-aware resolver is configured.

### Runtime wiring

The resolver is set on `Story` and accessed through the `StoryState` trait:

```rust
use brink_intl::IcuPluralResolver;

let mut story = Story::new(&program);
story.set_plural_resolver(Box::new(IcuPluralResolver::new("en")?));
```

The VM's `resolve_line()` receives the resolver via `state.plural_resolver()` and passes it to `resolve_select()`. The select cascade is: **Exact → Keyword → Cardinal/Ordinal → default**. Without a resolver, Select parts fall back to their default text.

## CLI commands

All localization commands are subcommands of the `brink` CLI:

| Command | Description |
|---------|-------------|
| `brink export-xliff` | Export line tables from `.inkb` to XLIFF 2.0 |
| `brink compile-locale` | Compile translated XLIFF to `.inkl` |
| `brink regenerate-xliff` | Diff new `.inkb` against existing XLIFF, produce updated file |

The JSON workflow (`export-lines`, `compile-locale` from JSON, `regenerate-lines`) is available programmatically via `brink-intl` functions but not exposed as separate CLI commands — the XLIFF workflow is the primary user-facing interface.

## Open questions

### Choice text in the export

Choices produce two lines (display + output) via the dual-path model. Under the lexical scope model, both lines live in the same scope table, and source provenance makes the pairing obvious in context. The `choice` field (`"display"` / `"output"`) provides an explicit hint but is not strictly necessary — a translator can see the context from the surrounding lines and source ranges. If more explicit metadata is needed in the future, line entries can carry additional tags.

### Scope export filtering

`export-lines` emits all scopes that have at least one line entry. Scopes with zero line entries (pure control flow, no user-visible text) are omitted. No other filtering is applied — trivial scopes with one or two lines are included.

### Inline conditional recognition

The compiler does not yet recognize ink inline conditionals as `LinePart::Select` entries. Currently Select entries only come from hand-authored translations compiled via `compile-locale`. A future recognizer could pattern-match `{count > 1: apples | apple}` into `Select { slot, variants, default }`, allowing the compiler to produce localizable plural forms directly from ink source.

### Inline sequence recognition

Similarly, inline sequences could be recognized as `Slot` entries where the sequence index becomes the slot value. This is a natural extension of the recognizer infrastructure.
