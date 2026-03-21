# brink-studio specification

brink-studio is a standalone, pro-grade ink story editor with a screenplay-mode editing experience, built on CodeMirror 6 with a Rust/wasm backend (`brink-ide`). It ships as both a standalone web application (like Inky, but better) and a reusable component library that can be embedded in host applications like s92-studio.

See also: [brink-ide-spec](brink-ide-spec.md) (the query layer powering IDE features), [brink-driver-spec](brink-driver-spec.md) (pipeline orchestration), [compiler-spec](compiler-spec.md) (compilation pipeline), [runtime-spec](runtime-spec.md) (story execution).

## Motivation

Ink is a powerful scripting language for interactive narrative, but existing editing tools fall into two categories: general-purpose code editors with syntax highlighting (VS Code + Inky plugin) and the legacy Inky editor. Neither provides a writing-first experience that understands ink's structural constructs — weave nesting, choice/gather flow, divert graphs — the way a screenplay editor understands scene headings, dialogue, and transitions.

brink-studio adapts Scrivener's screenplay-mode paradigm to ink: each line in the editor is a typed "element" with specific visual treatment, keyboard behavior, and succession rules. The editor maintains awareness of the author's position in ink's weave tree and provides structural editing operations that insert syntactically valid constructs. A live preview layer adds selective visual richness — styled typography, interactive choice bracket previews, and expandable divert disclosure widgets — without replacing the underlying syntax.

The target user is a professional narrative designer investing significant time in a large ink project. The editor must scale to multi-file stories with hundreds of knots and provide the structural navigation (binder view), refactoring (rename, move knot/stitch), and comprehension (divert disclosure, choice bracket preview) tools that large projects demand.

## Architecture

### Package structure

brink-studio spans two packages in the brink repository:

```
crates/brink-web/          Rust → wasm (wasm-bindgen)
                           Compiles brink-ide, brink-compiler, brink-runtime to wasm.
                           Exports: compile, semantic_tokens, completions, hover,
                                    goto_definition, structural editing, outline, etc.
                           This is the existing brink-web crate, extended with new
                           wasm-bindgen exports as brink-ide grows.

packages/brink-studio/     TypeScript (Vite library mode + Tauri app)
                           The CM6 editor, screenplay mode, live preview, player,
                           binder panel, and standalone app shell.
                           Consumes the wasm module from brink-web.
                           Exports:
                             - Tauri desktop app (primary standalone distribution)
                             - Web app (browser-based standalone, no install)
                             - Component library (editor, player, binder — embeddable)
                             - React wrappers (for host apps)
```

**Rationale:** The Rust/wasm layer already exists as `brink-web`. Rather than creating a parallel wasm crate, brink-studio extends `brink-web` with additional wasm-bindgen exports as new brink-ide features are implemented. The TypeScript package (`packages/brink-studio/`) is the new deliverable — it contains all CM6 integration, screenplay mode logic, live preview widgets, the story player, and a standalone app shell.

### Standalone app

brink-studio ships as a standalone web application — a modern replacement for Inky. The app provides:

- **Binder panel** — project tree showing file → knot → stitch hierarchy, with drag-and-drop reordering
- **Editor panel** — the CM6 screenplay-mode editor (the core of this spec)
- **Player panel** — debug-oriented story player
- **Project management** — open/save ink files from the local filesystem (via File System Access API or file input/download fallback), manage multi-file projects

The standalone app is the primary development and testing surface. All features are built and proven here first. This is the Inky replacement — an author can open brink-studio in a browser and write, compile, and play ink stories without any other tooling.

### Embeddable components

The same components that make up the standalone app are individually exportable for embedding in host applications. A host like s92-studio can mount just the editor, just the player, or all three panels, and wire them into its own layout, file management, and state systems via props and callbacks.

**brink-studio has no knowledge of any host application's internals.** It does not import from s92-studio, does not know about SpacetimeDB, and makes no assumptions about the host's framework beyond providing React wrappers as a convenience. The integration boundary is a clean props/callbacks/ref API.

### Layer diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  Host application (e.g., s92-studio)                    OPTIONAL │
│  Thin wrapper that mounts brink-studio components               │
│    - Provides file content via props / callbacks                │
│    - Owns layout, persistence, and host-specific concerns       │
│    - brink-studio has no knowledge of the host                  │
├─────────────────────────────────────────────────────────────────┤
│  brink-studio                                                    │
│  TypeScript, Vite (library mode + standalone app)               │
│    - Standalone app shell (binder + editor + player)            │
│    - CM6 editor with screenplay mode                            │
│    - Live preview (decorations, widgets)                        │
│    - Binder panel (project tree, drag-drop, navigation)         │
│    - Story player (debug-oriented)                              │
│    - React wrappers (ref-based, uncontrolled)                   │
│    - Wasm API surface (typed TS bindings)                       │
├─────────────────────────────────────────────────────────────────┤
│  brink-web wasm module                                           │
│    - brink-ide (semantic tokens, completions, hover, goto-def,  │
│      rename, code actions, structural editing, outline)         │
│    - brink-compiler (ink source → bytecode)                     │
│    - brink-runtime (bytecode execution)                         │
├─────────────────────────────────────────────────────────────────┤
│  brink-syntax / brink-ir / brink-analyzer / brink-driver         │
│  (consumed transitively through brink-ide and brink-compiler)   │
└─────────────────────────────────────────────────────────────────┘
```

### Build tooling

| Concern | Choice | Rationale |
|---------|--------|-----------|
| Language | TypeScript | Type safety for CM6's precise API surface; s92-studio is TS and consumes typed exports |
| Bundler | Vite (library mode) | Familiar from s92-studio; Rollup under the hood for production; built-in dev server |
| Output | ES modules | Consumed by s92-studio's Vite build via package import |
| Wasm build | wasm-pack | Same as brink-web today; produces `pkg/` with `.wasm` + JS glue + `.d.ts` |
| Desktop shell | Tauri | Lightweight native wrapper; Rust codebase aligns; proper filesystem access |
| Package manager | pnpm | Consistent with s92-studio monorepo |

### Dependency on brink-web

brink-studio imports the wasm module built from `crates/brink-web/`. The build process:

1. `wasm-pack build crates/brink-web/ --target web` produces `crates/brink-web/www/pkg/`
2. `packages/brink-studio/` references the pkg output (via local path or workspace link)
3. Vite handles wasm loading and initialization at runtime

The wasm module is the single integration point between the TypeScript editor and the Rust backend. All IDE intelligence flows through wasm-bindgen function calls.

## Visual hierarchy

ink's structural elements map to a three-level hierarchy inspired by Scrivener's organizational model:

| ink construct | Scrivener analog | Binder role | Visual weight |
|---------------|------------------|-------------|---------------|
| File (`.ink`) | Binder folder | Top-level container | Not visible in editor; shown in binder |
| Knot (`=== name ===`) | Folder / Part | Chapter-level grouping | Large heading, strong visual break |
| Stitch (`= name`) | Document / Scene | **Primary editing unit** | Scene heading, prominent but smaller than knot |
| Labeled gather/choice | Bookmark | Inline sub-heading | Subtle heading within stitch body |

**Stitches are scenes.** This is the central design insight. The stitch is the primary unit of work — the thing an author opens to write, the thing that appears in the binder as a navigable item, the thing that can be dragged and reordered. Knots are organizational chapters that group stitches. Files are acts or volumes that group knots.

The binder tree structure:

```
act1.ink                      (file)
  chapter1                    (knot — chapter level)
    scene1                    (stitch — scene level, primary editing unit)
    scene2                    (stitch)
  chapter2                    (knot)
    scene1                    (stitch)
    scene2                    (stitch)
act2.ink                      (file)
  ...
```

Within a stitch body, labeled gathers (`- (label_name)`) and labeled choices (`* (label_name) [Choice text]`) appear as inline sub-headings. They are navigable (shown in an outline panel, linkable) but are not binder-level items — they don't participate in drag-and-drop reordering at the binder level.

## Element type catalog

Every line in the editor is classified as one of the following element types. Each type has three properties: **visual treatment** (how it looks), **entry trigger** (how you create it), and **succession** (what happens when you press Enter).

### Structure elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Knot header** | Large font, bold, full-width rule above. Strong visual break. Distinct background band. | Type `===` at start of line, or use binder "new chapter" action. | New stitch header (if knot has stitches) or narrative text |
| **Stitch header** | Medium font, bold, subtle rule above. Scene heading style. | Type `=` at start of line (single equals), or use binder "new scene" action. | Narrative text |

### Flow elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Narrative text** | Body font, normal weight. Standard prose appearance. Full line width. | Default element type in any body context. Typing any non-sigil text. | Narrative text (same depth) |
| **Choice** (`*` non-sticky, `+` sticky) | Single sigil (`*` or `+`) regardless of weave depth. Indentation reflects depth. Bracket content `[...]` gets distinct styling. Sticky choices (`+`) get a subtle visual indicator (e.g., pin icon or different bullet). | Type `*` or `+` at start of content line. Tab on a gather line converts it to a choice. | New sibling choice (same depth and type) |
| **Gather** (`-`) | Single dash, indentation reflects depth. Subtle horizontal rule styling — acts as a convergence marker. | Type `-` at start of content line. Shift+Tab on a choice line converts it to a gather (exits choice block). | Narrative text (at gather's depth) |

### Control flow elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Divert** (`->`) | Right-aligned when standalone at end of line (screenplay transition style). Arrow symbol `->` preserved, target name styled as a link. Disclosure widget (expand to preview target content). | Type `>` at start of content line (inserts `-> ` and triggers target completion). | Narrative text |
| **Divert (inline)** | Stays inline, not right-aligned. Arrow and target styled as a link. Disclosure widget available. | Type `->` within a line. | N/A (inline, not a line element) |
| **Tunnel** (`-> target ->`) | Inline styling, not right-aligned. Visually distinct from plain divert (e.g., bidirectional arrow indicator). | Type full tunnel syntax. | Narrative text |
| **Thread** (`<- target`) | Inline styling with thread indicator. | Type `<-` at start of line. | Narrative text |

### Logic elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Logic line** (`~`) | Monospace font, dimmed/muted color. Visually recessed — this is "backstage" content that doesn't produce player-visible output. | Type `~` at start of content line. | Narrative text |
| **Variable declaration** (`VAR`, `CONST`) | Monospace font, keyword highlighted. Typically appears in file preamble. | Type `VAR` or `CONST` at start of line. | Variable declaration (when in preamble) or narrative text |
| **List declaration** (`LIST`) | Monospace font, keyword highlighted, list items with enum-member styling. | Type `LIST` at start of line. | Narrative text |
| **Temp declaration** (`~ temp`) | Same as logic line. | Type `~ temp` or Tab from a logic line context. | Narrative text |

### Meta elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Comment** (`//`, `/* */`) | Italic, dimmed. Distinctly "not content." | Type `//` at start of line. | Comment (for block comments) or narrative text |
| **Tag** (`#`) | Pill/badge styling after content, or on its own line with decorator color. | Type `#` after content or at line start. | Narrative text |
| **Include** (`INCLUDE`) | Monospace, file path styled as a link (clickable to open file). Typically in file preamble. | Type `INCLUDE` at start of line. | Narrative text |
| **External** (`EXTERNAL`) | Monospace, function signature styling. | Type `EXTERNAL` at start of line. | Narrative text |

### Screenplay elements

Screenplay elements are editor conventions layered on top of valid ink syntax. They are not recognized by `line_contexts()` (which reports them as narrative) — instead, a client-side post-pass in the TS layer pattern-matches their syntax and assigns screenplay element types. This keeps the brink-syntax and brink-ide layers unaware of screenplay conventions.

The underlying ink syntax uses `@Name:<>` for character lines and `(text)<>` for parentheticals. The `:<>` is colon + glue — the runtime sees `@Name:` as a recognizable pattern for downstream game engines, and `<>` (standard ink glue) merges the character/parenthetical line with the following dialogue line into a single output line.

| Element | Ink syntax | Visible in editor | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------|-------------------|-----------------|---------------|---------------------|
| **Character** | `@Name:<>` | `NAME` (centered, bold, accent color) | `@`, `:`, `<>` hidden by replace widgets. Name text uppercased in display. Centered on the line. | Tab on a blank line preceded by a blank line inserts `@:<>` template, cursor between `@` and `:` | Dialogue (new line below) |
| **Parenthetical** | `(text)<>` | `(text)` (italic, dimmed, indented) | `<>` hidden by replace widget. Parentheses visible, styled. Indented and italic. | Tab from character line or empty dialogue line | Dialogue (new line below; if empty, converts to dialogue) |
| **Dialogue** | Plain narrative text following character or parenthetical | Normal text (indented from both margins) | Wider indent than narrative, narrower than full width. Screenplay dialogue layout. | Enter from character or parenthetical line. Tab from narrative after double-blank. | See transition table below |

**Character line structure:**
```
@Name:<>
│ │   ││
│ │   │└─ glue (hidden) — merges with next line in runtime output
│ │   └── colon (hidden) — separator for runtime pattern matching
│ └────── name text (visible, centered, bold, uppercased)
└──────── character sigil (hidden)
```

**Parenthetical structure:**
```
(text)<>
│    │││
│    ││└─ glue (hidden) — merges with next line
│    │└── close paren (visible, styled)
│    └─── parenthetical text (visible, italic)
└──────── open paren (visible, styled)
```

**Cursor restrictions:** The cursor cannot enter the `@`, `:`, or `<>` regions. These are atomic replace decorations. If the user backspaces from the line below into a character line, the cursor lands between `@` and `:` (in the name text region). If the user presses Enter in the middle of the name (e.g., `@Hello|friend:<>` where `|` is cursor), the result is:
```
@Hello:<>
friend
```
The second line becomes plain narrative text (the name is split, the sigils stay with the first part).

**Smart backspace:** On a character line with no name text (`@:<>`), Backspace clears the entire line including all sigils, returning it to a blank line. Shift+Tab on any screenplay element strips the sigils and converts to plain narrative text.

**Screenplay element transitions (Tab / Enter):**

| Current element | Tab | Enter | Shift+Tab |
|----------------|-----|-------|-----------|
| **Character** | Parenthetical | Dialogue (new line) | Strip to narrative |
| **Parenthetical** | Dialogue | Dialogue (empty → convert; non-empty → new line) | Strip to narrative |
| **Dialogue (empty)** | Parenthetical | Element picker dropdown | Strip to narrative |
| **Dialogue (with text)** | Parenthetical | Action/narrative | Strip to narrative |
| **Blank line** (after blank) | Character (insert `@:<>`) | Element picker dropdown | — |

Shift+Enter within dialogue inserts a new line that stays within dialogue format.

The **element picker** is an inline dropdown (similar to the existing element type dropdown in the status bar) that appears on Enter from a blank or empty dialogue line, allowing the user to choose the next element type (character, parenthetical, dialogue, narrative, choice, gather, divert, etc.).

**Classification:** Screenplay elements are identified by a TS post-pass in `element-type.ts`, in the same layer that already promotes blank lines after choices to choice bodies. The post-pass runs after `line_contexts()` returns from wasm and pattern-matches:
- Line matching `^@[^:]*:<>$` → Character
- Line matching `^\(.*\)<>$` → Parenthetical
- Narrative text immediately following a Character or Parenthetical line → Dialogue

**Autocomplete for character names** is deferred to a generic pattern-matching autocomplete capability in brink-ide (see deferred items). When implemented, it will collect all `@Name:` occurrences across the project and suggest them when typing in a character line. This capability will also be reusable for tag autocomplete and other pattern-based suggestions.

### Inline elements

Inline elements live within content lines and do not have their own element type in the state machine. They receive rich styling within the line:

| Element | Visual treatment |
|---------|-----------------|
| **Inline conditional** (`{cond: a \| b}`) | Braces styled as delimiters, condition expression highlighted, branches visually separated. Stays as syntax in v1. |
| **Inline sequence** (`{&a\|b\|c}`) | Sequence type sigil (`&`, `~`, `!`, etc.) highlighted, branches visually separated. Stays as syntax in v1. |
| **String interpolation** (`{expression}`) | Expression highlighted within braces. |
| **Glue** (`<>`) | Subtle symbol, dimmed. |

**Multi-line blocks:** When an inline conditional or sequence opens a multi-line block (the branches contain statements on their own lines), standard element-type behavior applies within those blocks. The opening `{condition:` line is treated as a conditional opener, and lines within the branches are classified by their own element types.

## State machine

### Weave cursor

The editor maintains a **weave cursor** — the author's current position in the choice/gather nesting tree. The weave cursor has a depth (0 for top-level content, 1 for content inside a first-level choice, etc.) and a context (whether you're in a choice body, at the choice level, or at a gather).

The weave cursor is not a separate data structure from the CM6 editor state — it is derived from the current cursor position by analyzing the surrounding syntax tree. However, the state machine uses it to determine the behavior of Enter, Tab, and Shift+Tab.

### Key transitions

| Current element | Key | Result | Weave cursor change |
|----------------|-----|--------|---------------------|
| Narrative text (any depth) | Enter | New narrative text line at same depth | None |
| Choice line (`*`/`+`) | Enter | New sibling choice at same depth and type | None |
| Choice line | Shift+Enter | New narrative text line inside choice body | Depth +1 (enters choice body) |
| Choice body content | Enter | New narrative text line at same depth (inside body) | None |
| Any line at depth > 0 | Shift+Tab (at line start) | Depends on context — see below | Depth -1 |
| Any line at depth N | Tab (at line start) | Depends on context — see below | Depth +1 |
| Gather line | Enter | New narrative text line at gather's depth | None |
| Stitch header | Enter | New narrative text line | Depth resets to 0 |
| Knot header | Enter | New stitch header or narrative text | Depth resets to 0 |

### Tab / Shift+Tab behavior

Tab and Shift+Tab at line start navigate the weave depth by converting the current line's element type:

**Tab (increase depth):**
- Narrative text at depth N becomes narrative text at depth N+1 (indented into the previous choice's body)
- Gather at depth N becomes a choice at depth N+1

**Shift+Tab (decrease depth):**
- Choice body content at depth N becomes a new sibling choice at depth N-1 (exits the choice body, becomes a peer choice)
- Choice at depth N becomes a gather at depth N-1 (exits the choice set)
- Gather at depth N becomes narrative text at depth N-1

The visual indentation updates immediately to reflect the new weave position. The underlying ink syntax (number of sigils) is rewritten to match.

### Sigil-based element conversion

At the start of a content line (before any non-whitespace content), typing a sigil converts the line's element type:

| Typed | Conversion |
|-------|-----------|
| `-` | Line becomes a gather. Depth determined by current weave cursor. |
| `*` | Line becomes a non-sticky choice. Depth determined by current weave cursor. |
| `+` | Line becomes a sticky choice. Depth determined by current weave cursor. |
| `~` | Line becomes a logic line. |
| `>` | Inserts `-> ` (divert arrow + space) and triggers completion of valid divert targets. |

Sigil conversion happens only at line start, before any content. Typing `*` in the middle of a narrative line does not convert it to a choice.

## Visual treatment details

### Choice bracket hover

Choice brackets (`[...]`) receive interactive hover behavior that teaches ink's text suppression mechanics:

- **Default state:** Bracket content has distinct styling (e.g., different background, subtle border) to visually separate it from the "before" and "after" parts of the choice text.
- **Hover over bracket content:** The "before" text and bracket text are shown in their "choice presented to player" rendering. The "after" text is dimmed, showing that it won't appear in the choice label.
- **Hover over before/after content:** The bracket text is dimmed, showing the "output after choice is selected" rendering (before text + after text, bracket text suppressed).

This provides an interactive preview of how ink's three-part choice text model works without leaving the editing context.

### Divert disclosure widget

Standalone diverts display a disclosure widget (expand/collapse toggle) that, when expanded, shows the first few lines of the divert target's content inline below the divert line. This provides a "peek" at where the divert goes without navigating away.

Implementation: CM6 line widget decoration. The widget:
1. Resolves the divert target via brink-ide's goto-definition
2. Reads the target's source content from the wasm module
3. Renders a read-only preview block below the divert line
4. Collapses on click or when the cursor moves away

Cross-file diverts show the target file name as a header in the disclosure.

### Divert right-alignment

Standalone end-of-line diverts (`-> target` as the sole content of a line, or appearing after content with nothing following) are right-aligned like screenplay transitions. The `->` symbol and target name are pushed to the right edge of the editor.

Inline diverts (appearing mid-line with content after them), tunnels (`-> target ->`), and threads (`<- target`) are NOT right-aligned. They stay in place to avoid visual confusion when flow control is embedded in content.

### Weave depth indentation

Choices and gathers at weave depth N are indented by N levels (using a configurable indent width, default 2em). The raw ink syntax uses repeated sigils for depth (`* *` for depth 2), but the editor displays a single sigil with indentation:

| Raw ink | Editor display |
|---------|---------------|
| `* Choice at depth 1` | `* Choice at depth 1` (no indent) |
| `* * Choice at depth 2` | `  * Choice at depth 2` (1 level indent) |
| `* * * Choice at depth 3` | `    * Choice at depth 3` (2 levels indent) |
| `- Gather at depth 1` | `- Gather at depth 1` (no indent) |
| `- - Gather at depth 2` | `  - Gather at depth 2` (1 level indent) |

The underlying document still contains the full ink syntax with repeated sigils. The editor's decoration layer hides the extra sigils and applies indentation. Editing operations (Tab, Shift+Tab, typing) update the actual syntax.

### Typography

The editor uses a proportional body font for narrative content and a monospace font for logic/code elements. This visual split reinforces the distinction between "content the player sees" and "logic that runs behind the scenes."

| Element category | Font | Weight | Size |
|-----------------|------|--------|------|
| Knot header | Proportional | Bold | Large (e.g., 1.5em) |
| Stitch header | Proportional | Bold | Medium (e.g., 1.25em) |
| Narrative text | Proportional | Normal | Body (1em) |
| Choice text | Proportional | Normal | Body (1em) |
| Gather text | Proportional | Normal | Body (1em) |
| Divert | Proportional | Normal | Body (1em), right-aligned when standalone |
| Character name | Proportional | Bold | Body (1em), centered, accent color |
| Parenthetical | Proportional | Normal, italic | Body (1em), indented, dimmed |
| Dialogue | Proportional | Normal | Body (1em), indented from both margins |
| Logic / Variable / Temp | Monospace | Normal | Slightly smaller (0.9em) |
| Comment | Proportional | Normal, italic | Body (1em), dimmed |
| Tag | Proportional | Normal | Small (0.85em), pill/badge |
| Include / External | Monospace | Normal | Body (1em) |

## Binder and structure

### Outline data

brink-studio provides an outline API that returns the structural hierarchy of an ink file. This data powers the standalone app's binder panel and is available to host applications for building their own binder UI.

The outline includes:
- Knots with their names, ranges, and function flag
- Stitches within each knot, with names and ranges
- Labeled gathers and labeled choices within each stitch, as sub-heading items

This maps to the existing `document_symbols` function in brink-ide, extended to include labeled gathers and choices as leaf-level children of stitches.

### Binder panel

brink-studio ships its own binder panel component. The binder:

- Displays the file → knot (chapter) → stitch (scene) hierarchy
- Shows labeled gathers and choices as inline sub-headings within stitches
- Supports drag-and-drop reordering of stitches within a knot, and moving stitches between knots
- Supports drag-and-drop reordering of knots within a file
- Clicking a stitch navigates the editor to focused editing mode (that stitch only)
- Clicking a knot or file navigates to scrivenings mode (all content in that scope)
- Context menu: rename, delete, create new knot/stitch

The binder uses brink-studio's own structural editing wasm API for all reorder/move operations and the outline API for building the tree. When embedded, a host application may use this binder or replace it with its own UI consuming the same outline data.

## IDE features

All IDE features are powered by brink-ide compiled to wasm. The CM6 editor calls into the wasm module for intelligence and renders the results using CM6's extension system.

### Features available today (in brink-ide)

| Feature | brink-ide module | CM6 integration |
|---------|-----------------|-----------------|
| Semantic tokens | `semantic_tokens` | `EditorView.decorations` — CSS classes per token type |
| Completions | `completion` (context detection, visibility filtering) | `@codemirror/autocomplete` source |
| Hover | `hover` | `@codemirror/view` tooltip |
| Go-to-definition | `navigation::goto_definition` | Ctrl+Click handler or command |
| Find references | `navigation::find_references` | Command → highlights or panel |
| Rename | `rename::prepare_rename`, `rename::rename` | Command → inline rename widget |
| Code actions | `code_actions` | Lightbulb menu or command palette |
| Inlay hints | `inlay_hints` | `EditorView.decorations` — inline widgets for parameter names |
| Signature help | `signature` | Tooltip on `(` while typing function arguments |
| Folding | `folding` | `@codemirror/language` fold service |
| Document symbols / outline | `document` | Outline panel data source |
| Formatting | `formatting` (format region, sort knots/stitches) | Format command / on-save |

### Features requiring brink-ide extensions

These features require new functionality in brink-ide (new Rust code in `crates/internal/brink-ide/`):

#### Structural editing

Structural editing operations insert, move, or transform syntactically valid ink constructs. Unlike text editing (which operates on characters), structural editing operates on the AST.

```rust
// Proposed brink-ide API additions

/// Insert a new choice after the choice at `offset`. Returns the text edit
/// and the cursor position for the new choice's content.
pub fn insert_sibling_choice(
    source: &str,
    offset: TextSize,
) -> Option<(TextEdit, TextSize)>;

/// Insert a gather line after the current choice set containing `offset`.
/// Returns the text edit and cursor position.
pub fn insert_gather_after_choices(
    source: &str,
    offset: TextSize,
) -> Option<(TextEdit, TextSize)>;

/// Change the weave depth of the element at `offset` by `delta` levels.
/// Positive delta increases depth (Tab), negative decreases (Shift+Tab).
/// Returns the text edit that rewrites the sigils and adjusts indentation.
pub fn change_weave_depth(
    source: &str,
    offset: TextSize,
    delta: i32,
) -> Option<TextEdit>;

/// Extract a knot's source text for moving between files.
/// Returns the full text of the knot (header through end of body).
pub fn extract_knot(source: &str, knot_name: &str) -> Option<String>;

/// Extract a stitch's source text for moving between knots.
pub fn extract_stitch(
    source: &str,
    knot_name: &str,
    stitch_name: &str,
) -> Option<String>;

/// Remove a knot from the source, returning the modified source.
pub fn remove_knot(source: &str, knot_name: &str) -> Option<String>;

/// Remove a stitch from a knot, returning the modified source.
pub fn remove_stitch(
    source: &str,
    knot_name: &str,
    stitch_name: &str,
) -> Option<String>;

/// Insert a knot at a specific position (after another knot, or at the end).
pub fn insert_knot(
    source: &str,
    knot_text: &str,
    after_knot: Option<&str>,
) -> String;

/// Insert a stitch into a knot at a specific position.
pub fn insert_stitch(
    source: &str,
    knot_name: &str,
    stitch_text: &str,
    after_stitch: Option<&str>,
) -> String;

/// Reorder stitches within a knot to match the given name order.
pub fn reorder_stitches(
    source: &str,
    knot_name: &str,
    stitch_order: &[&str],
) -> String;

/// Reorder knots within the source to match the given name order.
pub fn reorder_knots(
    source: &str,
    knot_order: &[&str],
) -> String;
```

**Rationale:** Structural editing must live in brink-ide (Rust/wasm) rather than in TypeScript because it requires AST awareness — knowing where knots, stitches, choices, and gathers begin and end. brink-syntax provides the parse tree; brink-ide provides the editing operations on top of it. The TypeScript layer translates user gestures (Enter, Tab, drag-drop) into calls to these APIs.

#### Enhanced outline

The current `document_symbols` function returns knots and stitches. It needs to be extended to include labeled gathers and labeled choices as children of their containing stitch:

```rust
/// Extended document symbol with sub-heading support.
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    pub range: TextRange,
    /// Full range of the element's body (for scrivenings mode — determines
    /// the extent of the stitch when showing it in isolation).
    pub body_range: Option<TextRange>,
    pub children: Vec<DocumentSymbol>,
}
```

#### Divert target resolution for preview

The divert disclosure widget needs to look up the content at a divert target. This is already possible by combining `goto_definition` (to find the target's location) with reading the source at that location. No new brink-ide API is needed — the TypeScript layer composes these existing operations.

## Story player

brink-studio ships a debug-oriented story player component. Unlike a production game player that shows only what the player would see, this player surfaces runtime internals for authoring and debugging.

### Player features

| Feature | Description |
|---------|-------------|
| Story text | Rendered output of `continue_maximally()` |
| Choices | Clickable choice buttons with full choice text |
| Tags | Display tags for each content line and choice |
| Visit counts | Show current visit count for the active knot/stitch |
| Variable inspector | Expandable panel showing all variable names, types, and current values |
| Turn counter | Display the current turn number |
| Flow indicator | Show current position in the story (knot.stitch path) |
| Step history | Scrollable log of all content produced, with timestamps |
| Restart / Reset | Reset story state to initial |
| Navigate to source | Click on rendered content to jump to the corresponding source line in the editor |

### Player architecture

The player uses the same `StoryRunner` wasm interface as brink-web today, extended with additional query methods:

```rust
// Proposed additions to brink-web's StoryRunner

/// Get the current knot path (e.g., "chapter1.scene2").
pub fn current_path(&self) -> String;

/// Get all variable names and their current values as JSON.
pub fn variables_json(&self) -> String;

/// Get the visit count for a specific knot/stitch path.
pub fn visit_count(&self, path: &str) -> u32;

/// Get the current turn count.
pub fn turn_count(&self) -> u32;
```

### Editor-player interaction

The editor and player are separate components. The host application (s92-studio) wires them together:

1. Editor content changes trigger recompilation (debounced)
2. Successful compilation produces story bytes
3. Story bytes are passed to the player component
4. The player optionally preserves story state across recompilations (continue from current point) or resets

The player emits events that the host can use to coordinate with the editor (e.g., "user is viewing content from knot X" could scroll the editor to that knot).

## Component API surface

### Editor component

The core editor component is framework-agnostic (vanilla CM6). A thin React wrapper provides the integration surface for s92-studio.

#### Vanilla API

```typescript
interface BrinkEditorOptions {
  /** Initial document content. */
  initialContent: string;

  /** Wasm module instance (initialized brink-web). */
  wasm: BrinkWasm;

  /** Called when the document content changes. */
  onChange?: (content: string) => void;

  /** Called when compilation produces a result. */
  onCompile?: (result: CompileResult) => void;

  /** Called when the user navigates to a definition in another file. */
  onNavigateToFile?: (path: string, offset: number) => void;

  /** Called when the outline (document structure) changes. */
  onOutlineChange?: (symbols: DocumentSymbol[]) => void;

  /** Auto-compile on change, with debounce in ms. 0 to disable. */
  compileDebounceMs?: number;

  /** Whether to show the screenplay-mode visual treatment. */
  screenplayMode?: boolean;

  /** Whether to show live preview (divert disclosure, etc.). */
  livePreview?: boolean;
}

interface BrinkEditor {
  /** Replace the editor content. */
  setContent(content: string): void;

  /** Get the current editor content. */
  getContent(): string;

  /** Scroll to and highlight a specific byte offset. */
  revealOffset(offset: number): void;

  /** Scroll to a specific knot/stitch by name. */
  revealSymbol(knot: string, stitch?: string): void;

  /** Focus the editor. */
  focus(): void;

  /** Destroy the editor and clean up. */
  destroy(): void;

  /** Get the current document outline. */
  getOutline(): DocumentSymbol[];

  /** The CM6 EditorView, for advanced integration. */
  readonly view: EditorView;
}

/** Create and mount a brink editor. */
function createBrinkEditor(
  container: HTMLElement,
  options: BrinkEditorOptions,
): BrinkEditor;
```

#### React wrapper

```typescript
interface BrinkEditorProps {
  /** Document content. Changes are reported via onContentChange,
   *  but the component does NOT re-render on every keystroke.
   *  This is the "initial" content — set it to load a file,
   *  not to control every character. */
  content: string;

  /** Wasm module instance. */
  wasm: BrinkWasm;

  /** Called when content changes (debounced). */
  onContentChange?: (content: string) => void;

  /** Called when compilation produces a result. */
  onCompile?: (result: CompileResult) => void;

  /** Called when the user navigates to another file. */
  onNavigateToFile?: (path: string, offset: number) => void;

  /** Called when the outline changes. */
  onOutlineChange?: (symbols: DocumentSymbol[]) => void;

  compileDebounceMs?: number;
  screenplayMode?: boolean;
  livePreview?: boolean;
}

interface BrinkEditorRef {
  setContent(content: string): void;
  getContent(): string;
  revealOffset(offset: number): void;
  revealSymbol(knot: string, stitch?: string): void;
  focus(): void;
  getOutline(): DocumentSymbol[];
  readonly view: EditorView;
}

const BrinkEditor: React.ForwardRefExoticComponent<
  BrinkEditorProps & React.RefAttributes<BrinkEditorRef>
>;
```

**Rationale:** The React wrapper is "uncontrolled" in the sense that CM6 owns the document state internally. The `content` prop is treated as "load this content" rather than "the content must always be this value." This avoids the performance disaster of re-rendering CM6 on every keystroke. The host uses `onContentChange` to learn about edits and `ref.setContent()` to load new files.

### Player component

```typescript
interface BrinkPlayerOptions {
  /** Compiled story bytes. */
  storyBytes: Uint8Array;

  /** Wasm module instance. */
  wasm: BrinkWasm;

  /** Called when the user clicks rendered content (for source navigation). */
  onNavigateToSource?: (offset: number) => void;

  /** Whether to show the debug inspector (variables, visit counts, etc.). */
  showDebugInspector?: boolean;
}

interface BrinkPlayer {
  /** Load a new story. */
  loadStory(bytes: Uint8Array): void;

  /** Reset the current story to its initial state. */
  reset(): void;

  /** Destroy the player and clean up. */
  destroy(): void;
}

function createBrinkPlayer(
  container: HTMLElement,
  options: BrinkPlayerOptions,
): BrinkPlayer;
```

A React wrapper follows the same pattern as the editor (forward ref, uncontrolled).

### Wasm API

The wasm API is the typed TypeScript interface over the wasm-bindgen exports from brink-web. It wraps the raw wasm calls with TypeScript types and handles JSON serialization/deserialization.

```typescript
interface BrinkWasm {
  // Compilation
  compile(source: string): CompileResult;

  // IDE features
  semanticTokens(source: string): SemanticToken[];
  completions(source: string, offset: number): CompletionItem[];
  hover(source: string, offset: number): HoverInfo | null;
  gotoDefinition(source: string, offset: number): LocationResult | null;
  findReferences(source: string, offset: number): LocationResult[];
  rename(source: string, offset: number, newName: string): FileEdit[];
  signatureHelp(source: string, offset: number): SignatureInfo | null;
  inlayHints(source: string, startOffset: number, endOffset: number): InlayHint[];
  codeActions(source: string, offset: number): CodeAction[];
  documentSymbols(source: string): DocumentSymbol[];
  foldingRanges(source: string): FoldRange[];

  // Structural editing
  insertSiblingChoice(source: string, offset: number): EditResult | null;
  insertGather(source: string, offset: number): EditResult | null;
  changeWeaveDepth(source: string, offset: number, delta: number): EditResult | null;
  extractKnot(source: string, knotName: string): string | null;
  extractStitch(source: string, knotName: string, stitchName: string): string | null;
  removeKnot(source: string, knotName: string): string | null;
  removeStitch(source: string, knotName: string, stitchName: string): string | null;
  insertKnot(source: string, knotText: string, afterKnot: string | null): string;
  insertStitch(source: string, knotName: string, stitchText: string, afterStitch: string | null): string;
  reorderStitches(source: string, knotName: string, stitchOrder: string[]): string;
  reorderKnots(source: string, knotOrder: string[]): string;

  // Formatting
  formatDocument(source: string): string;
  formatRegion(source: string, knotName: string, stitchName: string | null): string;

  // Runtime
  createRunner(storyBytes: Uint8Array): StoryRunner;
}
```

## CM6 extension architecture

The screenplay mode and live preview are implemented as CM6 extensions. This section describes the key extensions and how they compose.

### Screenplay mode extensions

| Extension | CM6 mechanism | Purpose |
|-----------|--------------|---------|
| Element type classification | `StateField` | Tracks the element type and weave depth of each line. Updated on document changes by parsing line prefixes against the syntax tree. |
| Element styling | `EditorView.decorations` (line decorations) | Applies CSS classes per element type. Line-level decorations for font, weight, size, indentation. |
| Weave indentation | `EditorView.decorations` (replace decorations) | Hides repeated sigils (`* *` → `*`) and applies indentation via line padding. |
| Screenplay sigil hiding | `EditorView.decorations` (replace decorations) | Hides `@`, `:`, `<>` in character lines and `<>` in parentheticals via atomic replace widgets. Cursor cannot enter these regions. |
| Screenplay post-pass | Part of `elementTypeField` | After `line_contexts()` returns from wasm, pattern-matches `@Name:<>` → Character, `(text)<>` → Parenthetical, and following narrative → Dialogue. Same mechanism as the existing choice body promotion. |
| Sigil conversion | `EditorView.inputHandler` | Intercepts single-character input at line start. If the character is a recognized sigil, converts the line's element type instead of inserting the character literally. |
| State machine keybindings | `keymap` | Enter, Shift+Enter, Tab, Shift+Tab with context-sensitive behavior based on element type and weave depth. Includes screenplay element transitions (character → parenthetical → dialogue cycle). |
| Element picker | `EditorView` tooltip/widget | Inline dropdown on Enter from blank/empty-dialogue lines. Lets user choose next element type without a toolbar. |
| Divert right-alignment | `EditorView.decorations` (line decorations) | Applies right-alignment CSS to standalone divert lines. |

### Live preview extensions

| Extension | CM6 mechanism | Purpose |
|-----------|--------------|---------|
| Choice bracket styling | `EditorView.decorations` (mark decorations) | Applies distinct CSS class to bracket content within choices. |
| Choice bracket hover | `EditorView.domEventHandlers` + `hoverTooltip` | Detects hover over choice text regions and shows before/after preview in tooltip or by toggling CSS classes on the surrounding content. |
| Divert disclosure | `EditorView.decorations` (widget decorations) | Line widget below standalone diverts. Expands on click to show target content. |
| Semantic highlighting | `EditorView.decorations` (mark decorations) | CSS classes from semantic tokens (existing pattern from brink-web). |

### IDE feature extensions

| Extension | CM6 mechanism | Purpose |
|-----------|--------------|---------|
| Autocompletion | `autocompletion()` with custom source | Calls wasm completions API, returns CM6 completion results. |
| Hover tooltips | `hoverTooltip()` | Calls wasm hover API, renders markdown tooltip. |
| Lint/diagnostics | `lintGutter()` + `setDiagnostics` | Compiler warnings and errors from compilation. |
| Inlay hints | `EditorView.decorations` (widget decorations) | Parameter name hints from wasm inlay hints API. |
| Go-to-definition | `EditorView.domEventHandlers` (Ctrl+Click) | Calls wasm goto-definition, navigates within file or emits cross-file navigation event. |

### Extension composition

All extensions are bundled into a single `brinkStudio()` extension that the editor component installs:

```typescript
function brinkStudio(options: {
  wasm: BrinkWasm;
  screenplayMode: boolean;
  livePreview: boolean;
}): Extension;
```

Individual features can be enabled/disabled via CM6 compartments, allowing runtime toggling of screenplay mode and live preview without rebuilding the editor state.

## Standalone app

The standalone app is brink's answer to Inky — a self-contained application for editing ink projects. It composes the editor, binder, and player components into a fixed layout and provides its own project management.

### Desktop app (Tauri)

The primary standalone distribution is a **Tauri desktop app**. Tauri wraps the same CM6+wasm frontend in a lightweight native shell (~5-10MB), providing:

- Native filesystem access — open/save/watch files without browser API limitations
- Native menu bar and keyboard shortcuts
- Window management (resize, minimize, fullscreen)
- OS integration (file associations for `.ink`, recent files)

The Tauri app uses the **wasm backend** (same code path as the browser and embedded versions). This keeps one integration path rather than maintaining a separate native Rust backend. If performance becomes a bottleneck on large projects, a native backend via Tauri commands is a future option.

The Tauri shell itself is minimal — its job is filesystem access and window chrome. All editor logic lives in the shared TypeScript/CM6 layer.

### Web app

The same frontend also runs as a standalone web application (no Tauri required). The web version uses the File System Access API where available, with `<input type="file">` / download fallback. This is useful for quick editing, sharing, and environments where installing a desktop app isn't practical.

### Layout

Three-panel layout (resizable):

```
┌──────────┬──────────────────────────┬──────────────┐
│  Binder  │        Editor            │    Player    │
│          │                          │              │
│  file →  │  screenplay-mode CM6     │  story text  │
│  knot →  │  editor with live        │  choices     │
│  stitch  │  preview                 │  debug info  │
│          │                          │              │
└──────────┴──────────────────────────┴──────────────┘
```

### Project management

| Feature | Tauri (desktop) | Web (browser) |
|---------|----------------|---------------|
| Open file(s) | Native file dialog | File System Access API; `<input type="file">` fallback |
| Save | Direct filesystem write | File System Access API; download fallback |
| Multi-file project | Open a directory; watch for changes | Open a directory (FSAA); manual refresh fallback |
| New file | Create on disk | Create in-memory; prompt to save |
| Recent projects | OS recent files list | localStorage |

The binder panel in standalone mode owns the full file → knot → stitch tree, including drag-and-drop reordering of stitches and knots (using the structural editing wasm API to rewrite the ink source).

### Scrivenings mode

When the user clicks a file in the binder (rather than a specific stitch), the editor shows all knots and stitches in that file concatenated — Scrivener's "scrivenings" mode. Each stitch boundary has a visual separator. The user can edit any part of the file in this view.

When the user clicks a specific stitch, the editor shows only that stitch's content (from its header to the next stitch header or knot end). This is the focused editing mode.

## Embedding in host applications

brink-studio's components are designed for embedding, but **brink-studio itself has no knowledge of any host application.** The integration boundary is a clean props/callbacks/ref API. Host-specific concerns (persistence, layout, file management) are the host's responsibility.

This section describes the integration patterns a host would use. It is guidance for host developers, not a specification for brink-studio.

### Integration pattern

A host application (e.g., s92-studio) would:

1. **Mount components** — use the React wrappers (`BrinkEditor`, `BrinkPlayer`) or the vanilla `createBrinkEditor()` / `createBrinkPlayer()` functions
2. **Provide content** — pass file content to the editor via `ref.setContent()` when the user opens a file
3. **Receive changes** — listen to `onContentChange` callbacks and persist edits to the host's storage
4. **Wire navigation** — listen to `onNavigateToFile` for cross-file go-to-definition and open the target file
5. **Use outline data** — listen to `onOutlineChange` to populate a binder/tree UI with the host's own tree component
6. **Structural editing** — call the wasm API's structural editing functions (`reorderStitches`, `extractKnot`, etc.) in response to drag-and-drop in the host's binder UI

### Binder responsibility split

The standalone app ships its own binder panel. A host application may choose to use it, replace it with its own binder UI (consuming outline data from the editor), or combine both.

| Concern | Standalone app | Host application |
|---------|---------------|-----------------|
| File management | File System Access API / download | Host's file system (e.g., SpacetimeDB, local FS) |
| File tree UI | brink-studio's binder panel | Host's project panel (consuming outline data) |
| Knot/stitch navigation | brink-studio's binder panel | Host's tree UI calling `ref.revealSymbol()` |
| Drag-drop reorder | brink-studio's binder panel | Host's drag-drop calling wasm structural editing API |

### Theming

brink-studio defines `--brink-*` CSS custom properties with sensible defaults (dark theme). A host application can override these properties to match its own theme:

```css
/* Host override example */
.host-container {
  --brink-bg: #1a1a2e;
  --brink-fg: #e0e0e0;
  --brink-accent: #64b5f6;
  /* ... */
}
```

This approach requires no JS coordination and works regardless of the host's UI framework.

## Deferred / out of scope

| Item | Status | Notes |
|------|--------|-------|
| Full inline rendering of conditionals/sequences | Future | V1 shows styled syntax. Future versions could render branch previews. |
| Custom keybind configuration | Future | V1 uses fixed keybindings. Future versions could allow user customization. |
| Spell checking | Future | Could integrate with browser spell check or a dedicated service. |
| Collaborative editing | Out of scope | Would require OT/CRDT integration at the host level. Not on the roadmap. |
| Export to PDF / print | Out of scope | Not part of the editor's responsibility. |
| Localization of editor UI | Future | V1 is English-only. |
| Undo/redo integration with host | Deferred | CM6 has built-in undo/redo. Integration with a host's undo system is a future concern for the embedding layer, not brink-studio itself. |
| Pattern-based autocomplete (brink-ide) | Deferred | Generic capability to collect pattern matches across the project (e.g., all `@Name:` occurrences for character name autocomplete, all `#tag` occurrences for tag autocomplete). Not screenplay-specific — a general brink-ide feature. |
