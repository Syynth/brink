# brink compiler specification

`brink-compiler` turns `.ink` source text into `.inkb` bytecode through a 6-pass pipeline. It depends on `brink-syntax` (parsing), `brink-hir` (lowering), `brink-analyzer` (semantic analysis), and `brink-format` (output types). See [format-spec](format-spec.md) for the types and file formats the compiler produces.

## Compilation pipeline

The pipeline is organized as a sequence of passes:

```
Pass 1: Parse          (brink-syntax)     per-file       → AST
Pass 2: Lower          (brink-hir)        per-file       → HIR + SymbolManifest + diagnostics
Pass 3: Merge/Resolve  (brink-analyzer)   cross-file     → unified SymbolIndex + diagnostics
Pass 4: Type-check     (brink-analyzer)   cross-file     → type annotations + diagnostics
Pass 5: Validate       (brink-analyzer)   cross-file     → dead code, unused vars, etc.
Pass 6: Codegen        (brink-compiler)   per-container  → bytecode + tables
```

The LSP runs passes 1–5. The compiler runs all 6.

### Pass 1: Parse (brink-syntax)

- **Input:** `.ink` source text
- **Output:** `Parse` — lossless CST (rowan green/red tree) + `Vec<ParseError>`
- **Properties:**
  - Every byte of source appears in exactly one token (lossless roundtrip)
  - Error recovery via `ERROR` nodes — parser never panics, always produces output
  - ~230 `SyntaxKind` variants (tokens + nodes)
  - Typed AST layer with 140+ zero-cost newtype wrappers over CST nodes
  - Pratt expression parser with 10 precedence levels
  - String interpolation with nesting depth tracking

Covers all ink constructs: knots, stitches, choices, gathers, diverts, tunnels, threads, variables, lists, externals, inline logic, sequences, tags, content extensions markup.

### Pass 2: Lower (brink-hir)

- **Input:** `ast::SourceFile` from brink-syntax
- **Output:** `(HirFile, SymbolManifest, Vec<Diagnostic>)`
- **Scope:** Per-file. Does not require cross-file context. Granularity is per-knot — individual knots can be re-lowered independently.

brink-hir is a **rich semantic tree** — it preserves the full structure of the source with nesting resolved and syntactic sugar stripped, but all semantic information retained. Expressions stay as trees (not stack ops), choices/sequences/conditionals keep their branch structure, diverts/tunnels/threads are semantic nodes (not jump instructions). Both brink-analyzer and brink-compiler (codegen) consume the HIR. Codegen does the last-mile lowering from semantic nodes to bytecode.

#### Responsibilities

- **Weave folding:** flat choices/gathers (identified by bullet/dash count) → recursively nested `ChoiceSet`/`Gather` tree. Nested bullet levels (`* *`) produce nested `ChoiceSet`s inside the parent choice's body. Conditional blocks are structurally opaque — the HIR preserves them as `Stmt::Conditional` within the choice body; weave transparency for choices inside conditionals is a runtime/codegen concern (see [Ink semantics](#ink-semantics-compiler-perspective)).
- **Implicit structure:** top-level content before first knot → root content block.
- **INCLUDE recording:** records INCLUDE sites. The actual cross-file merge happens in brink-analyzer; brink-hir exports `fold_weave` which the analyzer calls on the merged content.
- **First-stitch auto-enter:** the first stitch in a knot is entered via implicit divert; other stitches require explicit diverts. Stitches with parameters are never auto-entered.
- **Strip trivia and syntactic sugar:** comments, whitespace, and surface syntax are removed; semantic content is preserved.
- **Symbol manifest:** collect declarations (knots, stitches, variables, lists, externals) and unresolved references (divert targets, variable references that may be cross-file).
- **Structural diagnostics:** malformed weave nesting, orphaned gathers, gathers inside conditional blocks, choices in conditionals without explicit diverts.

#### Source provenance

HIR nodes carry `AstPtr<N>` — a lightweight pointer (`SyntaxKind` + `TextRange`, typed via `PhantomData`) that resolves back to a live AST node given the syntax tree root. This supports LSP refactoring workflows (rename, lint fix, extract/inline) without lifetime coupling to the CST. Stale pointers from previous parses fail gracefully on resolution. `AstPtr` is implemented in brink-syntax.

#### Error recovery

The HIR is always structurally valid but potentially incomplete. Fields that might be missing due to parse errors are `Option<T>`. Unparseable constructs are skipped with a diagnostic. Malformed weave gets best-effort folding with a diagnostic. No explicit error/sentinel nodes in the tree — a syntax error in one stitch does not prevent other stitches from being lowered.

#### API surface

brink-hir exports composable per-knot lowering functions alongside a convenience whole-file entry point (`lower`). Per-knot functions (`lower_knot`, `lower_top_level`) enable the analyzer to re-lower only changed knots. `fold_weave` is public so the analyzer can call it on merged INCLUDE content after cross-file resolution.

#### Incremental strategy

The analyzer caches HIR per knot and uses rowan green node identity to detect unchanged knots after incremental reparse. Only changed knots are re-lowered — unchanged knots reuse cached HIR. The `SymbolManifest` is reassembled from per-knot pieces.

#### HIR type model

The HIR is organized around a small set of structural concepts:

**`HirFile`** — the root output for a single `.ink` file. Contains the root content block (top-level content before the first knot), all knot definitions, and top-level declarations (VAR, CONST, LIST, EXTERNAL, INCLUDE sites).

**`Knot` and `Stitch`** — named containers with optional parameters, a function flag (for `== function knot_name ==`), and a body. Each knot may contain stitches. Stitches have the same shape as knots minus the function flag and child stitches.

**`Block`** — the universal body type. A flat sequence of statements. Used for knot bodies, stitch bodies, choice branches, conditional branches, and sequence branches. This uniformity keeps the tree regular — any structural position that can hold content uses `Block`. No statement in a block "owns" the rest of the block — content after a `ChoiceSet` or any other statement is simply the next item in the list.

**`Stmt`** — the things inside a block: content output, diverts (`->`), tunnel calls (`->->`), thread starts (`<-`), temp declarations, assignments, returns, choice sets, block-level conditionals, and block-level sequences.

**`ChoiceSet` and `Gather`** — the core weave folding output. A `ChoiceSet` groups consecutive choices at the same weave depth with an optional `Gather` as their convergence point. Each choice has the three-part content split (start/bracket/inner from ink's `[...]` syntax), an optional condition, optional label, sticky/fallback flags, tags, an optional explicit divert, and a `Block` body. The gather has its own content and tags but no body — it is the convergence point, not a container for continuation. Content after a gather is simply the next statement in the parent `Block`, not owned by the `ChoiceSet`. Choice bodies may themselves contain nested `ChoiceSet`s — weave nesting is recursive via the tree structure, not depth counters.

**`Content` and `ContentPart`** — a line of text output with inline elements. Parts include plain text, glue, expression interpolation (`{expr}`), inline conditionals (`{cond: a | b}`), and inline sequences (`{&a|b|c}`). Block-level conditionals and sequences are separate `Stmt` variants, not content parts — this reflects the genuine semantic distinction in ink between inline elements (which produce text fragments) and block elements (which contain statements).

**`Conditional` and `BlockSequence`** — block-level control flow. Conditionals have branches (each with an optional condition and a `Block` body). Block sequences have a `SequenceType` and branches (each a `Block`).

**`Expr`** — expression trees preserved as-is. Literals (int, float, bool, string with interpolation parts, null), unresolved path references, divert targets as values, list literals, prefix/infix/postfix operations, and function calls. No lowering to stack operations — codegen handles that.

**Control flow nodes** — diverts, tunnel calls, and thread starts are separate statement types (not a single divert variant) reflecting their distinct ink semantics. Each carries a target path and optional arguments.

**Declarations** — VAR, CONST, temp, assignment (with `=`/`+=`/`-=`), LIST (with members carrying name, optional explicit ordinal, and active/inactive flag), EXTERNAL (name + param count), and INCLUDE sites.

**`Name` and `Path`** — a `Name` is a single identifier with its text and an `AstPtr` back to the source. A `Path` is a dotted sequence of names (e.g., `knot.stitch.label`). Paths are unresolved at the HIR level — the analyzer resolves them to `DefinitionId`s.

#### Sequence types

Sequence type is a **bitmask**, not an enum. The reference ink compiler supports combining flags (e.g., `shuffle stopping`). Symbols: `$` = stopping (also the default when no annotation), `&` = cycle, `!` = once, `~` = shuffle. Valid combinations: each standalone, `shuffle | stopping`, and `shuffle | once`. All other combinations are structural errors.

#### Weave folding algorithm

The weave folder (`fold_weave`) converts a flat stream of `WeaveItem`s (choices, gathers, and statements with depth markers) into a recursively nested tree. Based on the reference ink compiler's `Weave.cs` `ConstructWeaveHierarchyFromIndentation`:

1. **Group by depth:** scan the flat item list. When a choice or gather at depth > base appears, collect it and all subsequent items at that depth or deeper. Recursively fold the collected items into a nested `Block` and insert it as a statement in the parent.
2. **Build choice sets:** within a single depth level, consecutive choices form a `ChoiceSet`. If a gather follows choices at the same depth, it becomes the `ChoiceSet`'s convergence point.
3. **Gathers don't own continuations:** content after a gather is the next sibling statement in the parent `Block`, NOT nested inside the gather or the `ChoiceSet`. A `Block` is always a flat list of statements — no statement swallows the tail of its parent block.
4. **Standalone gathers:** a gather that appears without preceding choices (e.g., a labeled gather used as a divert target) is emitted as its own statement, not wrapped in a `ChoiceSet`.
5. **Conditionals are opaque:** conditional blocks are preserved as `Stmt::Conditional` within choice/gather bodies. The weave folder does NOT recurse into conditionals to extract choices. Weave transparency for choices inside conditionals is handled at runtime/codegen via loose end propagation (see reference `Weave.cs` `PassLooseEndsToAncestors`).
6. **Loose end tracking:** choices and gathers without explicit diverts are "loose ends" that codegen must connect to the next gather. The HIR records the structure; codegen handles divert insertion.
7. **Auto-enter gathers:** a gather that follows only non-choice content (no choices in the current section) is auto-entered in the main flow. A gather that follows choices is only reachable via divert from those choices.

**Invariant:** after folding, no `WeaveItem` depth markers remain in the tree. Nesting is encoded entirely by the recursive `Block` → `ChoiceSet` → `Choice.body: Block` → `ChoiceSet` → ... structure. Downstream passes never inspect depth values.

#### What HIR does NOT do

- **No cross-file context** — that is brink-analyzer's job
- **No bytecode emission** — that is brink-compiler's job (codegen)
- **No name resolution** — paths stay as unresolved `Path` nodes; the analyzer resolves them to `DefinitionId`s
- **No type checking** — the analyzer handles this after name resolution
- **No container boundary decisions** — the HIR has knots, stitches, choices, gathers as semantic nodes; codegen decides which become bytecode containers

### Pass 3–5: Analyze (brink-analyzer)

- **Input:** `Vec<(FileId, HirFile, SymbolManifest)>` from all files
- **Output:** `(SymbolIndex, Vec<Diagnostic>)`
- **Responsibilities:**
  - Merge per-file symbol manifests into a unified symbol table
  - Resolve INCLUDE file graph
  - Name resolution: paths → concrete symbols (DefinitionIds)
  - Scope analysis: temp is function-scoped, VAR/CONST are global
  - Type checking: expression types, assignment compatibility
  - Validation: undefined targets, duplicate declarations, dead code, unused variables
  - Circular include detection

The analyzer also owns the **project database** — the stateful, long-lived cache of parsed trees and analysis results. Both the compiler and LSP interact with this:

- **Compiler:** creates a project database, loads all files, runs passes 1–5, feeds results to codegen
- **LSP:** holds a long-lived project database, updates incrementally on file edits, serves queries against cached results

### Pass 6: Codegen (brink-compiler)

- **Input:** HIR trees + resolved `SymbolIndex`
- **Output:** bag of `ContainerBytecode` blobs (each with its line sub-table) + metadata (written to `.inkb`)
- **Responsibilities:**
  - Per-container bytecode emission
  - Expression lowering → stack ops + jumps
  - Choice lowering → choice point opcodes (see [Choice text decomposition](#choice-text-decomposition))
  - Sequence lowering → sequence opcodes
  - Divert/tunnel/thread lowering → control flow opcodes
  - Implicit diverts: end-of-root-story gets implicit gather + `-> DONE`
  - Text decomposition: static text blocks → line templates with slot placeholders (see [Text decomposition](#text-decomposition))
  - Per-container line table building (each line entry has content + source text content hash)
  - All cross-definition references use `DefinitionId` — no resolved indices in the output

## Text decomposition

Brink separates executable logic from localizable text. The bytecode is locale-independent — all user-visible text is referenced via `LineId = (DefinitionId, u16)`, a container-scoped local index into the container's line sub-table. Locale-specific content lives in `.inkl` overlay files that replace line content per container.

During codegen, the compiler decomposes text into line entries in the container's line sub-table:

- **Plain text** (no interpolation, no inline logic) → a line with a single `Literal`, emitted via `EmitLine(u16)`.
- **Interpolated or structured text** (contains `{variables}`, inline conditionals, or inline sequences) → a line with a `LineTemplate`, emitted via `EmitLine(u16)`. The compiler pushes slot values onto the stack before the emit.

The `u16` is the local line index within the current container. The runtime resolves this to the container's line sub-table entry.

Example: `I found {num_gems} {num_gems > 1: gems | gem} in the {cave_name}.` compiles to:

```
GetLocal(num_gems)          // push slot 0
GetLocal(cave_name)         // push slot 1
EmitLine(2)                 // format line 2's template with 2 slots from stack
```

Line sub-table entry 2:

```
I found {0} {0 -> one: gem | other: gems} in the {1}.
```

The plural logic lives in the line template, not the bytecode. Translators can restructure sentences, reorder slots, and alter plural/gender forms per locale without touching the compiled program.

### Scope of text decomposition

The compiler can only build message templates for **static text blocks** — contiguous text where the full structure is visible at compile time within a single expression or line.

**Can be one line:**

- A single line with interpolation: `Hello, {name}!`
- A single line with inline conditionals: `{flag: yes|no}`
- A single line with inline sequences: `{a|b|c}` (sequence index becomes a slot)
- Statically glued lines (both sides are literals or simple interpolations)
- Choice display / choice output text

**Each fragment is its own line (cannot be merged):**

- Text across container boundaries (diverts, tunnels, function calls, threads)
- Text in dynamically bounded loops
- Text produced by external function calls

The boundary rule: if it crosses a container call, each side is independent.

## Choice text decomposition

Ink's bracket syntax splits choice text into three roles:

```
* Pick up the sword[.] You grab the sword.
```

- Before `[` → appears in both the choice list and the output
- Inside `[...]` → appears only in the choice list
- After `]` → appears only after selection

This three-part split is a source-language authoring convenience. For localization, the compiler decomposes each choice into **two independent lines**:

- **Choice display** — the complete text shown in the choice list (before + inside bracket)
- **Choice output** — the complete text emitted after selection (before + after bracket)

`BeginChoice(flags, target)` always pops the display text from the value stack. The choice target (`DefinitionId`) is encoded directly in the opcode — no separate divert instruction. Two bytecode patterns are supported:

**High-level (static/templated text):** The compiler resolves bracket syntax at compile time and stores both texts as line table entries. `EvalLine` reads a line and pushes it as a String to the value stack (same as `EmitLine` but targeting the value stack instead of the output buffer). `ChoiceOutput` stores a line table reference on the pending choice for emission when the player selects it.

```
EvalLine(5)                   // push display text from line table
BeginChoice(flags, target)    // pop display text, register choice
  ChoiceOutput(6)             // output text line reference
EndChoice
```

Translators localize each line independently with no structural coupling. This allows the target language to use completely different grammatical constructions for the choice prompt vs. the narrative output.

**Low-level (dynamic text):** When choice text contains arbitrary logic that cannot be statically decomposed into a line table entry, `BeginStringEval`/`EndStringEval` captures evaluated text as a String and pushes it to the value stack. The choice target container handles output text directly. This path is also used by the ink.json converter, which does not have access to the original bracket syntax.

```
BeginStringEval
  EnterContainer(choice_text)   // arbitrary code that emits text
EndStringEval                   // capture text, push String to value stack
BeginChoice(flags, target)      // pop display text, register choice
EndChoice                       // no ChoiceOutput — target handles output
```

Both patterns are first-class. `BeginChoice` is agnostic to how the display text was produced — it always pops one String from the value stack.

## Localization authoring (XLIFF)

Localization source files use **XLIFF 2.0** — one file per locale (e.g., `translations/ja-JP.xlf`). Containers are represented as `<file>` elements within the XLIFF document. Brink-specific metadata (content hashes, audio asset references) uses XLIFF's custom namespace extension mechanism.

Workflow:

1. **Generate:** `brink-cli generate-locale` reads a compiled `.inkb` and produces an XLIFF file with all translatable lines (organized by container), including context annotations for translators.
2. **Translate:** Translators work in the XLIFF file directly or import it into a translation management platform (Lokalise, Crowdin, etc.). Audio asset references are added to the XLIFF via the `brink:audio` extension attribute. Translation state tracking uses XLIFF's built-in `state` attribute (`initial`/`translated`/`reviewed`/`final`).
3. **Compile:** `brink-cli compile-locale` reads the translated XLIFF and produces a binary `.inkl` overlay.
4. **Regenerate (on source changes):** `brink-cli generate-locale` diffs the new `.inkb` against the existing XLIFF by `LineId`, preserving human-edited fields (translations, audio refs), updating machine-managed fields (original text, context), and using the source text content hash to detect changed lines and reset their review status.

XLIFF was chosen because every major translation management platform natively imports/exports it, and the spec requires tools to preserve unknown extensions — brink-specific metadata survives round-trips through external tooling.

## LSP (brink-lsp)

Thin protocol adapter over `brink-analyzer`. Depends on analyzer, NOT on compiler.

Planned features:

- Diagnostics (streamed on every change)
- Go to definition (via SymbolIndex position lookup)
- Find references
- Rename (find references → workspace edit)
- Hover (symbol type, doc comment, usage count)
- Autocomplete (knot/stitch names at diverts, globals, local vars)
- Semantic tokens
- Document/workspace symbols
- Signature help (external function parameters)

## Ink semantics (compiler perspective)

Key semantics from the reference C# ink implementation relevant to compilation:

- **INCLUDE with top-level content:** top-level content from included files is merged inline at the INCLUDE location. Knots/stitches are separated and appended to the end of the story.
- **Stitch fall-through:** stitches do NOT fall through. The first stitch in a knot is auto-entered via an implicit divert emitted by the compiler. Other stitches require explicit `-> stitch_name`. Stitches with parameters are never auto-entered.
- **Root entry point:** all top-level content becomes an implicit root container. The compiler appends an implicit gather + `-> DONE` so the story terminates gracefully.
- **Gathers:** convergence points in the HIR (with optional labels, content, and tags). Gathers do not own a body — content after a gather is the next sibling statement in the parent block. At the bytecode level, gathers become named containers that choice branches divert to — codegen handles the lowering.
- **Choices inside conditional blocks:** choices (`*`) can appear inside `{ - condition: ... }` multiline conditional blocks. Gathers (`-`) are explicitly forbidden inside conditional blocks — the reference compiler errors with "You can't use a gather (the dashes) within the { curly braces } context." In the HIR, conditional blocks are structurally opaque — the weave folder does NOT extract choices from inside conditionals to merge them into the outer weave. Instead, choices inside conditionals stay nested within the `Stmt::Conditional` node. Weave transparency is deferred to codegen/runtime via loose end propagation. brink-syntax's `multiline_branch_body` handles this: `STAR`/`PLUS` dispatches to `choice()`, while `MINUS` breaks out of the body loop (gathers end the branch, matching the reference's gather-forbidden semantics).
