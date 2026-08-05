# brink compiler specification

`brink-compiler` turns `.ink` source text into `.inkb` bytecode through a multi-pass pipeline. It depends on `brink-driver` (pipeline orchestration) and `brink-codegen-inkb` (bytecode emission). The driver in turn composes `brink-db` (incremental file caching), `brink-ir` (HIR and LIR lowering), and `brink-analyzer` (semantic analysis). See [format-spec](format-spec.md) for the types and file formats the compiler produces, [brink-driver-spec](brink-driver-spec.md) for the orchestration layer, and [brink-ide-spec](brink-ide-spec.md) for the IDE query layer.

## Compilation pipeline

The pipeline is organized as a sequence of passes:

```
Pass 1:  Parse + Lower    (brink-syntax, brink-ir::hir)  per-file     → HIR + SymbolManifest + diagnostics
Pass 2:  Discover          (brink-driver)                 cross-file   → resolved INCLUDE graph
Pass 3:  Analyze           (brink-driver → brink-analyzer) cross-file  → SymbolIndex + ResolutionMap + diagnostics
Pass 4:  LIR Lower         (brink-ir::lir)                whole-program → Program (container tree + definitions)
Pass 5:  Codegen           (brink-codegen-inkb)           per-container → StoryData (bytecode + tables)
```

`brink-driver` orchestrates passes 2–3 and diagnostic collection. `brink-db` caches pass 1 results per file with incremental knot-level re-lowering. The LSP and compiler both use `brink-driver` for orchestration; the LSP additionally uses `brink-ide` for interactive queries (goto-def, hover, completions, etc.). The compiler runs all 5 passes.

One backend consumes the LIR:
- **Bytecode backend** (`brink-codegen-inkb`): linearizes to opcodes + line tables → `.inkb`

### Pass 1: Parse + Lower (brink-syntax, brink-ir::hir)

- **Input:** `.ink` source text
- **Output:** `(HirFile, SymbolManifest, Vec<Diagnostic>)` per file

This pass runs two stages per file:

**Stage 1a: Parse (brink-syntax)**

- Produces a `Parse` — lossless CST (rowan green/red tree) + `Vec<ParseError>`
- Every byte of source appears in exactly one token (lossless roundtrip)
- Error recovery via `ERROR` nodes — parser never panics, always produces output
- ~230 `SyntaxKind` variants (tokens + nodes)
- Typed AST layer with 140+ zero-cost newtype wrappers over CST nodes
- Pratt expression parser with 10 precedence levels
- String interpolation with nesting depth tracking

Covers all ink constructs: knots, stitches, choices, gathers, diverts, tunnels, threads, variables, lists, externals, inline logic, sequences, tags, content extensions markup.

**Stage 1b: Lower (brink-ir::hir)**

- Converts `ast::SourceFile` → `(HirFile, SymbolManifest, Vec<Diagnostic>)`
- Per-file, no cross-file context required. Granularity is per-knot — individual knots can be re-lowered independently.

brink-ir::hir produces a **rich semantic tree** — it preserves the full structure of the source with nesting resolved and syntactic sugar stripped, but all semantic information retained. Expressions stay as trees (not stack ops), choices/sequences/conditionals keep their branch structure, diverts/tunnels/threads are semantic nodes (not jump instructions).

#### HIR responsibilities

- **Weave folding:** flat choices/gathers (identified by bullet/dash count) → recursively nested `ChoiceSet`/`Gather` tree. Nested bullet levels (`* *`) produce nested `ChoiceSet`s inside the parent choice's body. Conditional blocks are structurally opaque — the HIR preserves them as `Stmt::Conditional` within the choice body; weave transparency for choices inside conditionals is a runtime/codegen concern (see [Ink semantics](#ink-semantics-compiler-perspective)).
- **Implicit structure:** top-level content before first knot → root content block.
- **INCLUDE recording:** records INCLUDE sites. The actual cross-file merge happens in brink-analyzer; brink-ir::hir exports `fold_weave` which the analyzer calls on the merged content.
- **First-stitch auto-enter:** the first stitch in a knot is entered via implicit divert; other stitches require explicit diverts. Stitches with parameters are never auto-entered.
- **Strip trivia and syntactic sugar:** comments, whitespace, and surface syntax are removed; semantic content is preserved.
- **Symbol manifest:** collect declarations (knots, stitches, variables, lists, externals) and unresolved references (divert targets, variable references that may be cross-file).
- **Structural diagnostics:** malformed weave nesting, orphaned gathers, gathers inside conditional blocks, choices in conditionals without explicit diverts.
- **Normalization pass:** `normalize_file()` runs after lowering, lifting inline sequences and inline conditionals within content to block-level `Stmt` nodes. This simplifies LIR lowering and codegen by ensuring only plain text and interpolations appear as inline content parts; block-level control flow is always represented as `Stmt::Conditional` / `Stmt::Sequence`.

#### Source provenance

HIR nodes carry `AstPtr<N>` — a lightweight pointer (`SyntaxKind` + `TextRange`, typed via `PhantomData`) that resolves back to a live AST node given the syntax tree root. This supports LSP refactoring workflows (rename, lint fix, extract/inline) without lifetime coupling to the CST. Stale pointers from previous parses fail gracefully on resolution. `AstPtr` is implemented in brink-syntax.

#### Error recovery

The HIR is always structurally valid but potentially incomplete. Fields that might be missing due to parse errors are `Option<T>`. Unparseable constructs are skipped with a diagnostic. Malformed weave gets best-effort folding with a diagnostic. No explicit error/sentinel nodes in the tree — a syntax error in one stitch does not prevent other stitches from being lowered.

#### API surface

brink-ir::hir exports composable per-knot lowering functions alongside a convenience whole-file entry point (`lower`). Per-knot functions (`lower_knot`, `lower_top_level`) enable the project database to re-lower only changed knots. `fold_weave` is public so the analyzer can call it on merged INCLUDE content after cross-file resolution.

#### Incremental strategy

The project database (`brink-db`) caches HIR per knot and uses rowan green node identity to detect unchanged knots after incremental reparse. Only changed knots are re-lowered — unchanged knots reuse cached HIR. The `SymbolManifest` is reassembled from per-knot pieces.

#### HIR type model

The HIR is organized around a small set of structural concepts:

**`HirFile`** — the root output for a single `.ink` file. Contains the root content block (top-level content before the first knot), all knot definitions, and top-level declarations (VAR, CONST, LIST, EXTERNAL, INCLUDE sites).

**`Knot` and `Stitch`** — named containers with optional parameters, a function flag (for `== function knot_name ==`), and a body. Each knot may contain stitches. Stitches have the same shape as knots minus the function flag and child stitches.

**`Block`** — the universal body type. A flat sequence of statements. Used for knot bodies, stitch bodies, choice branches, conditional branches, and sequence branches. This uniformity keeps the tree regular — any structural position that can hold content uses `Block`. No statement in a block "owns" the rest of the block — content after a `ChoiceSet` or any other statement is simply the next item in the list.

**`Stmt`** — the things inside a block: content output, diverts (`->`), tunnel calls (`->->`), thread starts (`<-`), temp declarations, assignments, returns, choice sets, block-level conditionals, and block-level sequences.

**`ChoiceSet` and `Gather`** — the core weave folding output. A `ChoiceSet` groups consecutive choices at the same weave depth with an optional `Gather` as their convergence point. Each choice has the three-part content split (start/bracket/inner from ink's `[...]` syntax), an optional condition, optional label, sticky/fallback flags, tags, an optional explicit divert, and a `Block` body. The gather has its own content and tags but no body — it is the convergence point, not a container for continuation. Content after a gather is simply the next statement in the parent `Block`, not owned by the `ChoiceSet`. Choice bodies may themselves contain nested `ChoiceSet`s — weave nesting is recursive via the tree structure, not depth counters.

**`Content` and `ContentPart`** — a line of text output with inline elements. Parts include plain text, glue, expression interpolation (`{expr}`), inline conditionals (`{cond: a | b}`), and inline sequences (`{&a|b|c}`). Block-level conditionals and sequences are separate `Stmt` variants, not content parts — this reflects the genuine semantic distinction in ink between inline elements (which produce text fragments) and block elements (which contain statements).

**`Conditional` and `BlockSequence`** — block-level control flow. Conditionals have branches (each with an optional condition and a `Block` body). Block sequences have a `SequenceType` and branches (each a `Block`).

**`Expr`** — expression trees preserved as-is. Literals (int, float, bool, string with interpolation parts, null), unresolved path references, divert targets as values, list literals, prefix/infix/postfix operations, and function calls. No lowering to stack operations — LIR lowering handles that.

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
6. **Loose end tracking:** choices and gathers without explicit diverts are "loose ends" that codegen must connect to the next gather. The HIR records the structure; LIR lowering handles divert insertion.
7. **Auto-enter gathers:** a gather that follows only non-choice content (no choices in the current section) is auto-entered in the main flow. A gather that follows choices is only reachable via divert from those choices.

**Invariant:** after folding, no `WeaveItem` depth markers remain in the tree. Nesting is encoded entirely by the recursive `Block` → `ChoiceSet` → `Choice.body: Block` → `ChoiceSet` → ... structure. Downstream passes never inspect depth values.

#### What HIR does NOT do

- **No cross-file context** — that is brink-analyzer's job
- **No bytecode emission** — that is brink-codegen-inkb's job
- **No name resolution** — paths stay as unresolved `Path` nodes; the analyzer resolves them to `DefinitionId`s
- **No type checking** — the analyzer handles this after name resolution
- **No container boundary decisions** — the HIR has knots, stitches, choices, gathers as semantic nodes; LIR lowering decides which become bytecode containers
- **No temp slot allocation** — handled by LIR lowering

### Pass 2: Discover (brink-driver)

- **Input:** entry file path + `read_file` callback
- **Output:** fully populated `ProjectDb` with all reachable files parsed and lowered

`brink-driver::Driver::discover()` performs BFS INCLUDE resolution starting from the entry file — each discovered file is parsed and lowered (pass 1) via `ProjectDb::set_file()`, and its INCLUDE declarations are followed transitively. Include path resolution is string-based (splits on `/`), not `std::path`-based, since ink uses `/` as path separator universally.

`ProjectDb` is the stateful, incremental cache used by both the compiler (one-shot) and LSP (long-lived). It caches:
- Parsed CST (rowan green tree) per file
- Lowered HIR + SymbolManifest per knot within each file
- Per-file diagnostics (parse errors + HIR lowering diagnostics)
- Suppression directives per file

For the compiler, `driver.discover()` is a single call that loads the entire project. For the LSP, `db.set_file()` / `db.update_file()` updates a single file incrementally — only changed knots are re-lowered (detected via rowan green node identity), and the INCLUDE graph is updated. See [brink-driver-spec](brink-driver-spec.md) for the full orchestration API.

### Pass 3: Analyze (brink-driver → brink-analyzer)

- **Input:** `Vec<(FileId, &HirFile, &SymbolManifest)>` from all files
- **Output:** `AnalysisResult { index: SymbolIndex, resolutions: ResolutionMap, diagnostics: Vec<Diagnostic> }`

`brink-driver` orchestrates analysis via `Driver::analyze()`, which calls `brink_analyzer::analyze()` on the cached HIR/manifest data. Analysis performs three responsibilities in sequence:

1. **Merge manifests** (`manifest::merge_manifests`) — merge per-file symbol manifests into a unified `SymbolIndex`. Duplicate declarations emit warnings (E022/E023/E026) matching inklecate's permissive behavior (first-wins semantics). Names that shadow built-in functions emit E035.
2. **Resolve references** (`resolve::resolve_refs`) — name resolution: unresolved `Path` nodes → concrete `DefinitionId`s. Handles scope analysis (temp is function-scoped, VAR/CONST are global). Produces a `ResolutionMap` mapping source ranges to their resolved definitions. Resolution follows ink's hierarchical scoping: local stitches/labels first, then knots, then top-level, then labels by suffix match. Function call arity is checked (E031).
3. **Validate** (`validate::validate`) — structural validation passes over the HIR:
   - E029: choices in inline context (conditional/sequence) without explicit divert
   - E032: `~ return` statement outside a function knot
   - E033: unreachable code after divert/return/tunnel (warning)
   - E034: choice set consisting entirely of fallback choices (warning)

After analysis, `brink-driver::Driver::collect_diagnostics()` gathers both per-file lowering diagnostics and cross-file analysis diagnostics, applies suppression directives (`brink-disable`, `brink-expect`, and the native-dialect declaration-scoped `@[allow(…)]` annotation), and partitions into errors and warnings. See [brink-driver-spec](brink-driver-spec.md).

### Pass 4: LIR Lower (brink-ir::lir)

- **Input:** HIR files + `SymbolIndex` + `ResolutionMap` from analysis
- **Output:** `Program` — a resolved, container-centric representation of the entire program

LIR is the critical bridge between the high-level semantic HIR and backend codegen. It transforms the per-file, name-relative HIR into a single merged program where all references are resolved, container boundaries are decided, and temp slots are allocated.

`lower_to_program()` consumes files in topological (INCLUDE) order and produces a `Program` containing:

- **Root container** — the top of a container tree. Every knot, stitch, gather, choice target, sequence wrapper, and conditional branch is a `Container` with a `DefinitionId`, a body of structured `Stmt`s, and child containers.
- **Global definitions** — `GlobalDef`, `ListDef`, `ListItemDef`, `ExternalDef` — all with assigned `DefinitionId`s and `NameId`s.
- **Name table** — interned strings indexed by `NameId`.

#### LIR design properties

- **Flat container list via tree.** Containers form a tree: root → knots → stitches → gathers/choice targets. Each container has a `children` vec holding its nested containers. The `ContainerKind` enum distinguishes the source construct (`Root`, `Knot`, `Stitch`, `Gather`, `ChoiceTarget`, `Sequence`, `SequenceBranch`, `ConditionalBranch`).

- **Structured statements.** Conditionals, sequences, and choice sets keep their branch structure within each container. Each backend serializes this structure into its output format (jump offsets for bytecode, nested arrays for JSON). This avoids committing to a bytecode-specific linearization that the JSON backend can't use.

- **Fully resolved.** No unresolved `Path` nodes. Every reference is a `DefinitionId` (globals, containers, list items, externals) or a temp slot index (`u16`). The LIR never needs the `SymbolIndex` or `ResolutionMap` — all lookups are done during lowering. Unresolved paths (expected to be already reported by the analyzer) fall back to `Expr::Null` for expressions and `DivertTarget::Done` for diverts.

#### LIR lowering responsibilities

- **Container planning:** decides which source constructs become containers (knots, stitches, gathers, choice targets, sequence wrappers) and assigns `DefinitionId`s.
- **Name resolution application:** replaces all HIR `Path` references with resolved `DefinitionId`s or temp slot indices using the `ResolutionMap`.
- **Temp slot allocation:** assigns `u16` slot indices to temp variables and parameters across the entire knot/function scope (including child containers that share the parent's call frame).
- **Counting flags:** assigns `CountingFlags` to containers based on their kind and whether they're referenced by visit-count expressions. Labeled containers with visit references get `COUNT_START_ONLY`.
- **Loose end resolution:** choices and gathers without explicit diverts get implicit diverts to the next gather target (`gather_target` on `ChoiceSet`).
- **Built-in function recognition:** intercepts function calls whose names match ink built-in functions (`TURNS_SINCE`, `LIST_COUNT`, `INT`, `FLOOR`, etc.) and converts them to `Expr::CallBuiltin` nodes instead of container calls.
- **Divert target resolution:** classifies divert targets as `Address`, `Variable` (global holding a divert target), `VariableTemp` (temp/param holding a divert target), `Done`, or `End`.
- **Call argument resolution:** resolves `ref` arguments to `RefGlobal(DefinitionId)` or `RefTemp(slot, name)` — always a *cell* reference, never a pointer into a collection's storage (the value model's "`ref` is cell-level" invariant; `docs/runtime-spec.md` §"Value model", `docs/value-model-spec.md` §7).
- **Template recognition:** the recognizer pass (`recognize.rs`) inspects content lines and produces `RecognizedLine::Plain` or `RecognizedLine::Template` with full metadata (source hash, slot info, source location). Currently recognizes plain text and interpolation patterns (`Text + Interpolation` mixtures). Content with inline conditionals, sequences, or glue mixed with expressions falls back to per-part emission (`ContentEmission::EmitContent`).

### Pass 5: Codegen (brink-codegen-inkb)

- **Input:** LIR `Program`
- **Output:** `StoryData` (written to `.inkb` via `brink-format`)
- **Entry point:** `brink_codegen_inkb::emit(&program) -> StoryData`

Codegen walks the LIR container tree and emits bytecode for each container:

- **Expression lowering** → stack ops + jumps (including short-circuit `and`/`or` via `JumpIfFalse`)
- **Choice lowering** → `BeginChoice`/`EndChoice` opcodes (see [Choice text decomposition](#choice-text-decomposition))
- **Sequence lowering** → `Sequence`/`SequenceBranch` opcodes
- **Conditional lowering** → condition evaluation + `JumpIfFalse` + branch bodies
- **Divert/tunnel/thread lowering** → `Goto`/`TunnelCall`/`ThreadCall` and variable variants
- **Implicit diverts:** end-of-root-story gets implicit gather + `-> DONE`
- **Text decomposition:** recognized lines → scope line table entries; unrecognized content → inline emit opcodes
- **Per-scope line table building** — all containers within a lexical scope (knot/stitch/root) share one line table keyed by scope `DefinitionId`. Each line entry carries content, source hash, slot info, source location, and optional audio ref.
- **Address table building** for intra-container labels
- **All cross-definition references use `DefinitionId`** — no resolved indices in the output

**Note:** `StoryData.source_checksum` is currently hardcoded to `0`. This field is intended to identify a specific compilation but is not yet computed.

## Text decomposition

Brink separates executable logic from localizable text. The bytecode is locale-independent — all user-visible text is referenced via `EmitLine(idx, slot_count)`, a scope-relative index into the lexical scope's line table. Locale-specific content lives in `.inkl` overlay files that replace line content per scope.

During codegen, the compiler decomposes text into line entries in the scope's line table:

- **Plain text** (no interpolation, no inline logic) → `LineContent::Plain(s)`, emitted via `EmitLine(idx, 0)`.
- **Interpolated text** (contains `{variables}`) → `LineContent::Template([Literal, Slot, ...])`, emitted via `EmitLine(idx, slot_count)`. The compiler pushes slot values onto the stack before the emit.
- **Unrecognized content** (inline conditionals, sequences, glue mixed with expressions) → emitted as individual opcodes (`EmitLine` for text fragments, `EmitValue` for expressions, inline conditional/sequence logic). Falls back to per-part emission, not a single template.

The `idx` is the local line index within the current lexical scope. The runtime resolves this via `LinkedContainer.scope_table_idx` to the scope's line table.

Example: `I found {num_gems} gems in the {cave_name}.` compiles to:

```
GetLocal(num_gems)          // push slot 0
GetLocal(cave_name)         // push slot 1
EmitLine(2, 2)              // format line 2's template with 2 slots from stack
```

Scope line table entry 2:

```
LineContent::Template([Literal("I found "), Slot(0), Literal(" gems in the "), Slot(1), Literal(".")])
```

Translators can restructure sentences, reorder slots, and alter plural/gender forms per locale without touching the compiled program.

### Template recognition

Template recognition runs during **LIR lowering** in `recognize.rs`, not during codegen. This is the last layer with access to:

- The **HIR** with `AstPtr` → source provenance (text ranges, original source text)
- The **SymbolIndex** with resolved variable names
- The content structure **before** artificial container boundaries are inserted

The recognizer produces `RecognizedLine` variants with full `LineMetadata` (source hash, slot info, source location). Codegen consumes these directly — `emit_recognized_line()` for templates, `emit_plain_line()` for plain text.

**Currently recognized patterns:**
- Plain text: `[Text(s)]` → `RecognizedLine::Plain`
- Interpolation: `[Text, Interpolation, Text, ...]` with at least one `Interpolation` → `RecognizedLine::Template` with `Literal`/`Slot` parts

**Not yet recognized (falls back to per-part emission):**
- Inline conditionals as `LinePart::Select`
- Inline sequences as `LinePart::Slot`
- Glue-joined cross-line content

### Scope of text decomposition

The compiler can only build message templates for **static text blocks** — contiguous text where the full structure is visible at compile time within a single expression or line.

**Can be one line:**

- A single line with interpolation: `Hello, {name}!`
- A single line with multiple interpolations: `{a} and {b}`
- Choice display / choice output text

**Each fragment is its own line (cannot be merged):**

- Text across container boundaries (diverts, tunnels, function calls, threads)
- Text in dynamically bounded loops
- Text produced by external function calls
- Content with inline conditionals or sequences (currently emitted as per-part opcodes)

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
EvalLine(5, 0)                // push display text from line table
BeginChoice(flags, target)    // pop display text, register choice
  ChoiceOutput(6, 0)          // output text line reference
EndChoice
```

Translators localize each line independently with no structural coupling. This allows the target language to use completely different grammatical constructions for the choice prompt vs. the narrative output.

**Low-level (dynamic text):** When choice text contains arbitrary logic that cannot be statically decomposed into a line table entry, `BeginStringEval`/`EndStringEval` captures evaluated text as a String and pushes it to the value stack. The choice target container handles output text directly. (The retired .ink.json converter pipeline also used this path — it had no access to the original bracket syntax.)

```
BeginStringEval
  EnterContainer(choice_text)   // arbitrary code that emits text
EndStringEval                   // capture text, push String to value stack
BeginChoice(flags, target)      // pop display text, register choice
EndChoice                       // no ChoiceOutput — target handles output
```

Both patterns are first-class. `BeginChoice` is agnostic to how the display text was produced — it always pops one String from the value stack.

## Localization authoring (XLIFF)

Localization source files use **XLIFF 2.0** — one file per locale (e.g., `translations/ja-JP.xlf`). Scopes are represented as `<file>` elements within the XLIFF document. Brink-specific metadata (content hashes, audio asset references) uses XLIFF's custom namespace extension mechanism (`xmlns:brink="urn:brink:xliff:extensions:1.0"`).

Workflow:

1. **Export:** `brink export-xliff` reads a compiled `.inkb` and produces an XLIFF file with all translatable lines organized by lexical scope, including context annotations for translators.
2. **Translate:** Translators work in the XLIFF file directly or import it into a translation management platform (Lokalise, Crowdin, etc.). Audio asset references are added via the `brink:audio` extension attribute. Translation state tracking uses XLIFF's built-in `state` attribute (`initial`/`translated`/`reviewed`/`final`).
3. **Compile:** `brink compile-locale` reads the translated XLIFF and produces a binary `.inkl` overlay.
4. **Regenerate (on source changes):** `brink regenerate-xliff` diffs the new `.inkb` against the existing XLIFF by scope + content hash (LCS alignment), preserving translations, updating source text, and resetting state for changed lines.

See [intl-spec](intl-spec.md) for full details on the localization pipeline, line table export, regeneration algorithm, and plural resolution.

## LSP (brink-lsp)

Thin protocol adapter over `brink-ide`. The LSP owns concurrency (tokio, Arc/Mutex, debounced background analysis) and protocol handling (tower-lsp). All IDE intelligence lives in `brink-ide`, which the LSP calls and converts results to LSP types. See [brink-ide-spec](brink-ide-spec.md).

The LSP holds a long-lived `ProjectDb`, updates incrementally on file edits (per-knot re-lowering via green node identity), and serves queries against cached analysis results. The compiler creates a one-shot `Driver`, discovers all files, and runs the full pipeline.

Implemented features:
- Diagnostics (streamed on every change, with suppression directives)
- Go to definition
- Find references
- Rename (cross-file)
- Hover (symbol info, built-in function docs)
- Autocomplete (context-aware: diverts, expressions, dotted paths)
- Semantic tokens (full syntax highlighting with resolution-based classification)
- Signature help (function call parameter info)
- Document symbols / workspace symbols
- Folding ranges
- Inlay hints (parameter names at call sites)
- Code actions (sort knots/stitches, format region)
- Document formatting (via brink-fmt)
- Workspace file discovery and file watcher registration

## Ink semantics (compiler perspective)

Key semantics from the reference C# ink implementation relevant to compilation:

- **INCLUDE with top-level content:** top-level content from included files is merged inline at the INCLUDE location. Knots/stitches are separated and appended to the end of the story.
- **Stitch fall-through:** stitches do NOT fall through. The first stitch in a knot is auto-entered via an implicit divert emitted by the compiler. Other stitches require explicit `-> stitch_name`. Stitches with parameters are never auto-entered.
- **Root entry point:** all top-level content becomes an implicit root container. The compiler appends an implicit gather + `-> DONE` so the story terminates gracefully.
- **Gathers:** convergence points in the HIR (with optional labels, content, and tags). Gathers do not own a body — content after a gather is the next sibling statement in the parent block. At the bytecode level, gathers become named containers that choice branches divert to — LIR lowering handles the container creation.
- **Choices inside conditional blocks:** choices (`*`) can appear inside `{ - condition: ... }` multiline conditional blocks. Gathers (`-`) are explicitly forbidden inside conditional blocks — the reference compiler errors with "You can't use a gather (the dashes) within the { curly braces } context." In the HIR, conditional blocks are structurally opaque — the weave folder does NOT extract choices from inside conditionals to merge them into the outer weave. Instead, choices inside conditionals stay nested within the `Stmt::Conditional` node. Weave transparency is deferred to LIR lowering/codegen via loose end propagation. brink-syntax's `multiline_branch_body` handles this: `STAR`/`PLUS` dispatches to `choice()`, while `MINUS` breaks out of the body loop (gathers end the branch, matching the reference's gather-forbidden semantics).

## Diagnostic Codes

Every diagnostic the compiler can emit has a stable code (`E001`–`E176`) and a
per-code reference file under [`docs/diagnostics/`](diagnostics/) with a summary,
explanation, minimal repro, and fix guidance. `DiagnosticCode::as_str` /
`DiagnosticCode::from_str_code` (`crates/internal/brink-ir/src/hir/diagnostics.rs`) are the
source of truth for the code set; `crates/internal/brink-test-harness/tests/diagnostic_docs_validation.rs`
asserts every variant has a corresponding doc file and that no orphaned doc files exist.

| Code | Summary |
| --- | --- |
| [`E001`](diagnostics/E001.md) | Knot definition is missing a name. |
| [`E002`](diagnostics/E002.md) | Stitch definition is missing a name. |
| [`E003`](diagnostics/E003.md) | Knot or stitch parameter is missing a name. |
| [`E004`](diagnostics/E004.md) | `VAR` declaration is missing a name. |
| [`E005`](diagnostics/E005.md) | `VAR` declaration is missing an initializer. |
| [`E006`](diagnostics/E006.md) | `CONST` declaration is missing a name. |
| [`E007`](diagnostics/E007.md) | `CONST` declaration is missing an initializer. |
| [`E008`](diagnostics/E008.md) | `LIST` declaration is missing a name. |
| [`E009`](diagnostics/E009.md) | `LIST` member is missing a name. |
| [`E010`](diagnostics/E010.md) | `EXTERNAL` declaration is missing a name. |
| [`E011`](diagnostics/E011.md) | RETIRED — the parser always materializes a `FILE_PATH` node inside `INCLUDE_STMT` (possibly empty) and... |
| [`E012`](diagnostics/E012.md) | Divert is missing a target. |
| [`E013`](diagnostics/E013.md) | RETIRED — `parser/divert.rs::path` always creates a `PATH` node (empty on error + E037), so... |
| [`E014`](diagnostics/E014.md) | Logic line has no effect (bare `~`). |
| [`E015`](diagnostics/E015.md) | Expression is missing an operand. |
| [`E016`](diagnostics/E016.md) | Unknown or unsupported operator. |
| [`E017`](diagnostics/E017.md) | Function call is missing a name. |
| [`E018`](diagnostics/E018.md) | RETIRED — `parser/divert.rs::path` always creates a `PATH` node (empty on error + E037), so... |
| [`E019`](diagnostics/E019.md) | RETIRED — the parser only builds a `CHOICE` node after seeing a bullet token, so a bullet-less choice CST... |
| [`E020`](diagnostics/E020.md) | Inline conditional is missing a condition. |
| [`E021`](diagnostics/E021.md) | Inline sequence has no branches. |
| [`E022`](diagnostics/E022.md) | Duplicate knot definition. |
| [`E023`](diagnostics/E023.md) | Duplicate variable/constant definition. |
| [`E024`](diagnostics/E024.md) | Unresolved divert target. |
| [`E025`](diagnostics/E025.md) | Unresolved variable reference. |
| [`E026`](diagnostics/E026.md) | Duplicate list item. |
| [`E027`](diagnostics/E027.md) | Ambiguous bare list item reference. |
| [`E028`](diagnostics/E028.md) | RETIRED — circular INCLUDE is detected at the discovery phase and surfaces as... |
| [`E029`](diagnostics/E029.md) | Choice nested in conditional without explicit divert. |
| [`E030`](diagnostics/E030.md) | String interpolation in constant initializer is ignored. |
| [`E031`](diagnostics/E031.md) | Function call argument count mismatch. |
| [`E032`](diagnostics/E032.md) | Return statement outside function. |
| [`E033`](diagnostics/E033.md) | Unreachable code after divert. |
| [`E034`](diagnostics/E034.md) | Choice set has only fallback choices. |
| [`E035`](diagnostics/E035.md) | Name shadows a built-in function. |
| [`E036`](diagnostics/E036.md) | Expected diagnostic not produced (`// brink-expect`). |
| [`E037`](diagnostics/E037.md) | Syntax error reported by the parser (malformed source). |
| [`E038`](diagnostics/E038.md) | Malformed `///` doc-comment tag on a declaration. |
| [`E039`](diagnostics/E039.md) | Registered host manifest disagrees with the ink `EXTERNAL` arity. |
| [`E040`](diagnostics/E040.md) | Doc-comment / manifest references an unknown semantic type. |
| [`E041`](diagnostics/E041.md) | External call argument type mismatches the manifest signature. |
| [`E042`](diagnostics/E042.md) | External call argument violates a closed-domain constraint. |
| [`E043`](diagnostics/E043.md) | Well-formed `///` doc-comment tag that doesn't apply to this declaration kind (e.g. |
| [`E044`](diagnostics/E044.md) | Unknown directive name (e.g. |
| [`E045`](diagnostics/E045.md) | Directive has no valid target in this position. |
| [`E046`](diagnostics/E046.md) | Directive contains dynamic inline logic — directives are static text. |
| [`E047`](diagnostics/E047.md) | Directive must be the only tag on its line. |
| [`E048`](diagnostics/E048.md) | Duplicate directive on one target. |
| [`E049`](diagnostics/E049.md) | Directive not supported on this target (e.g. |
| [`E050`](diagnostics/E050.md) | Directive does not take arguments or trailing text. |
| [`E051`](diagnostics/E051.md) | A brink-extension construct (block, sigil literal, indexing) was used under the `strict-ink` dialect. |
| [`E052`](diagnostics/E052.md) | A brink-extension construct parses and analyzes cleanly under the `brink` dialect, but its LIR lowering... |
| [`E053`](diagnostics/E053.md) | RETIRED — previously a non-suppressible backstop rejecting T1b brink-extension HIR nodes (`LogicBlock`,... |
| [`E054`](diagnostics/E054.md) | A block-scoped `temp` (`~ { … }`, docs/t1b-surface-spec.md §2) or `for` loop variable shadows an... |
| [`E055`](diagnostics/E055.md) | `push`/`insert`/`remove`'s first argument is not an lvalue (a variable, temp, or indexed path) — mutators... |
| [`E056`](diagnostics/E056.md) | `push`/`insert`/`remove` was used in expression position — they return nothing and are only valid as a... |
| [`E057`](diagnostics/E057.md) | `break`/`continue` used outside any enclosing `while`/`for` loop. |
| [`E058`](diagnostics/E058.md) | Collection mutator (`push`/`insert`/`remove`) called with the wrong number of arguments — a targeted... |
| [`E059`](diagnostics/E059.md) | A choice set, labeled gather block, multi-line conditional, or sequence was found nested inside inline... |
| [`E060`](diagnostics/E060.md) | `brink-codegen-inkb` refused to emit bytecode for a `Program` that violates an invariant an earlier,... |
| [`E061`](diagnostics/E061.md) | A type annotation names something that isn't a recognized nominal type... |
| [`E062`](diagnostics/E062.md) | RETIRED — : |
| [`E063`](diagnostics/E063.md) | A param/return/`VAR`/`CONST`/`~ temp` type annotation disagrees with the type TM-1's body inference would otherwise derive, or (issue #1877) a `VAR`/`CONST`/`~ temp` declaration initializer or a plain assignment disagrees with its target's already-known declared type, or (issue #1881) a UFCS-desugared call's receiver or written argument disagrees with the desugared free function's already-known declared param type. |
| [`E064`](diagnostics/E064.md) | `types = strict` was requested but the project's dialect isn't `brink` — strict typing is a brink-dialect... |
| [`E065`](diagnostics/E065.md) | Under `types = strict`, a def's inferred signature or body slot (param, return, or temp) resolved to... |
| [`E066`](diagnostics/E066.md) | Under `types = strict`, a def's inferred signature or body slot resolved to `Ty::Conflicted` — the body's... |
| [`E067`](diagnostics/E067.md) | Under `types = strict`, a `~ x = f()` / `~ temp x = f()` assigns the result of a call whose resolved def... |
| [`E068`](diagnostics/E068.md) | A struct construction literal's leading shape name (`Name#{…}`) doesn't name any declared `STRUCT`. |
| [`E069`](diagnostics/E069.md) | Under `types = strict`, a struct construction literal omits a declared field — names the missing field. |
| [`E070`](diagnostics/E070.md) | A struct construction literal supplies a field the shape doesn't declare — names the extra field. |
| [`E071`](diagnostics/E071.md) | Under `types = strict`, a struct construction literal's field initializer disagrees with the field's... |
| [`E072`](diagnostics/E072.md) | RETIRED — : |
| [`E073`](diagnostics/E073.md) | Non-suppressible defense-in-depth backstop, mirroring `E053`/`E060`/ (former) `E072`: |
| [`E074`](diagnostics/E074.md) | A field-write target (`p.field = expr`) is a *chained* projection — `p.a.b = v` or a mixed `p.a[i].b = v` — or (issue #2121) an indexed write whose index chain's own root is a struct-field projection (`p.field[i] = v`, `push(p.field[i], v)`)... |
| [`E075`](diagnostics/E075.md) | A struct construction literal used as a `VAR`/`CONST` declaration default doesn't match its declared shape: |
| [`E076`](diagnostics/E076.md) | A map literal used as a `VAR`/`CONST` declaration default has a key that isn't a compile-time-constant... |
| [`E077`](diagnostics/E077.md) | An array element, map value, struct field, or `#fn` bound `val` arg nested inside a `VAR`/`CONST`... |
| [`E078`](diagnostics/E078.md) | Under `types = strict`, an unresolved (builtin, not author-shadowed) call to `int(x)`/`float(x)` where `x`... |
| [`E079`](diagnostics/E079.md) | `#fn(name, …)`'s target does not resolve to a statically-named function definition (`=== function name... |
| [`E080`](diagnostics/E080.md) | A `ref` param of a `#fn` target is not bound in the creation-site prefix, or is bound to a non-durable lvalue. |
| [`E081`](diagnostics/E081.md) | `#fn(name, args…)` binds more arguments than the target declares — the bound-arg row is a *prefix* of the... |
| [`E082`](diagnostics/E082.md) | A T1b block-scoped `temp` (`~ { … }`) — or a `for`-loop variable, which desugars the same way — was... |
| [`E083`](diagnostics/E083.md) | A scalar `VAR`/`CONST` declaration default whose *source expression kind* can never be a compile-time... |
| [`E084`](diagnostics/E084.md) | A struct construction literal (`Name#{…}`) supplies the same field name more than once. |
| [`E085`](diagnostics/E085.md) | An *undeclared* file whose module (its file stem) collides with a *declared* module's name... |
| [`E086`](diagnostics/E086.md) | A malformed `#@module(…)` directive: |
| [`E087`](diagnostics/E087.md) | A reference resolves to a `#@private` definition in another module. |
| [`E088`](diagnostics/E088.md) | A bare-form `IMPORT { name } FROM mod` names a definition that the *declared* module `mod` does not... |
| [`E089`](diagnostics/E089.md) | An `IMPORT` brings the same local name into scope twice (a repeated bare import, or two imports whose... |
| [`E090`](diagnostics/E090.md) | An `IMPORT` names the importing file's own module — a module cannot import itself; its own names are... |
| [`E091`](diagnostics/E091.md) | A qualified access `a.b` is ambiguous: |
| [`E092`](diagnostics/E092.md) | A `#@public`/`#@private` override that restates the module's default (e.g. |
| [`E093`](diagnostics/E093.md) | Conflicting or repeated visibility directives on one declaration (both `#@private` and `#@public`, or the... |
| [`E094`](diagnostics/E094.md) | A malformed `#@was(…)` directive: |
| [`E095`](diagnostics/E095.md) | `#@was(name)` names the thing's own *current* name — a self-alias that would be a no-op entry in the... |
| [`E096`](diagnostics/E096.md) | Two *declared* modules (`#@module(name)`, different names) each define a same-name, same-kind symbol. |
| [`E097`](diagnostics/E097.md) | A `ref lvalue-path` projection expression (`ref npc.hp`, `ref inventory[idx]`) appears somewhere other... |
| [`E098`](diagnostics/E098.md) | A `ref lvalue-path` projection's segment (a dotted field, or a `[…]` index) disagrees with the root's... |
| [`E099`](diagnostics/E099.md) | A `ref lvalue-path` projection with at least one path segment (dotted field or `[…]` index — a *real*... |
| [`E100`](diagnostics/E100.md) | `#@effects` with no argument at all (`#@effects`, `#@effects()`, or an argument that parses to nothing) —... |
| [`E101`](diagnostics/E101.md) | A malformed `#@effects(…)` argument: |
| [`E102`](diagnostics/E102.md) | A `#@effects(…)` clause names an identifier that isn't a declared global `VAR`/`CONST` (for... |
| [`E103`](diagnostics/E103.md) | **The exceedance error** (docs/effects-spec.md §10, sitting 2, 2026-07-14 ruling): |
| [`E104`](diagnostics/E104.md) | A call `expr(args…)` whose callee isn't a bare variable/temp/param name (an `INDEX_EXPR`,... |
| [`E105`](diagnostics/E105.md) | An `await <cond>` / `while await <cond>` condition is not effect-free. |
| [`E106`](diagnostics/E106.md) | A `#{key: |
| [`E107`](diagnostics/E107.md) | A fresh, un-annotated declaration (`VAR x = none`, `CONST x = none`, `~ temp x = none`) whose initializer... |
| [`E108`](diagnostics/E108.md) | `@[effects(silent)]` exceedance: |
| [`E109`](diagnostics/E109.md) | `@[effects(total)]` exceedance: |
| [`E110`](diagnostics/E110.md) | The deprecated `#@effects(…)` tag-channel spelling — superseded by the `@[effects(…)]` annotation final form. |
| [`E111`](diagnostics/E111.md) | An `@[…]` annotation line naming something outside the channel's closed name set: |
| [`E112`](diagnostics/E112.md) | An `@[…]` annotation line outside a recognized placement — ink's leading run at the top of a knot/stitch... |
| [`E113`](diagnostics/E113.md) | A declaration named after a registry protocol method — `display`, `compare`, or `next`: |
| [`E114`](diagnostics/E114.md) | A registered protocol impl's inferred effect row exceeds its protocol's effect contract (`display`/`compare`: |
| [`E115`](diagnostics/E115.md) | An ill-formed protocol impl registration: |
| [`E116`](diagnostics/E116.md) | A condition-position expression (an `if`/`while` condition, a `{cond: |
| [`E117`](diagnostics/E117.md) | A range-refinement violation under `types = strict` (the E078 precedent — strict-only; gradual mode is... |
| [`E118`](diagnostics/E118.md) | A protocol impl registration named a numeric-tower kind (`vec2`/`vec3`/`vec4`/`quat`/`mat2`/`mat3`/`mat4`)... |
| [`E119`](diagnostics/E119.md) | A pure-callback verb's callback provably breaks the pure·silent contract. Two verb families carry this code, because the 2026-07-18 sitting ruled both: |
| [`E120`](diagnostics/E120.md) | NS-A7 `Weighted<T>` construction refusal: |
| [`E121`](diagnostics/E121.md) | Contract §4.2 check 1a (manifest ⇄ HIR agreement): |
| [`E122`](diagnostics/E122.md) | Contract §4.2 check 1b (manifest ⇄ HIR agreement): |
| [`E123`](diagnostics/E123.md) | Contract §4.2 check 1c: |
| [`E124`](diagnostics/E124.md) | Contract §4.2 check 2a (range well-formedness): |
| [`E125`](diagnostics/E125.md) | Contract §4.2 check 2b (join-key uniqueness, Q2(a)): |
| [`E126`](diagnostics/E126.md) | Contract §4.2 check 3: |
| [`E127`](diagnostics/E127.md) | Contract §4.2 check 4: |
| [`E128`](diagnostics/E128.md) | Contract §4.2 check 5: |
| [`E129`](diagnostics/E129.md) | A native construct parses cleanly but has no HIR lowering yet in this slice (a nested `module { … }`... |
| [`E130`](diagnostics/E130.md) | A native `flow` is declared more than two levels deep (a `flow` nested inside another nested `flow`'s... |
| [`E131`](diagnostics/E131.md) | `<-` (splice) used outside a choice point: |
| [`E132`](diagnostics/E132.md) | A native file-level `@[was(…)]` rename record carries no quoted old module path — a missing argument, or... |
| [`E133`](diagnostics/E133.md) | A native file's `root_content` carries something other than the one documented shape a native lowering may... |
| [`E134`](diagnostics/E134.md) | A native file's HIR carries an `IncludeSite` — native has no textual `INCLUDE` graph (charter §13.2, "the... |
| [`E135`](diagnostics/E135.md) | A `ThreadStart` (`<- target`) appears somewhere other than the two legal native splice positions B0.7's... |
| [`E136`](diagnostics/E136.md) | A native `ChoiceSet` carries a `depth`/`context` other than the B0.7-documented neutral values (`depth =... |
| [`E137`](diagnostics/E137.md) | The B0.9 native strict-only enforcement point: |
| [`E138`](diagnostics/E138.md) | A map literal supplies the same key twice (`Map { k: |
| [`E139`](diagnostics/E139.md) | A construction literal's entries are not in the form its target type constructs from — `Map { a }`... |
| [`E140`](diagnostics/E140.md) | **D1**: |
| [`E141`](diagnostics/E141.md) | `recv.name(args)` resolved as neither: |
| [`E142`](diagnostics/E142.md) | **D3**: |
| [`E143`](diagnostics/E143.md) | **D5**: |
| [`E144`](diagnostics/E144.md) | A UFCS call site that `brink-analyzer::ufcs` **resolved** cleanly has reached LIR lowering, which does not... |
| [`E145`](diagnostics/E145.md) | The v1 whole-condition restriction: |
| [`E146`](diagnostics/E146.md) | RETIRED (issue #1508) — choice-guard `as` bindings now lower for real. |
| [`E147`](diagnostics/E147.md) | An `as` binding whose condition is a statically-known **non-Option** type (`if 5 as n { … }`). |
| [`E148`](diagnostics/E148.md) | A write to an `as` binding — `if find(s) as i { i = 0; }`, `pop(i)`, `i[0] = x`, `bump(ref i)`, … The... |
| [`E149`](diagnostics/E149.md) | A `remove(a, i)` call whose first argument is statically known to be an array: |
| [`E150`](diagnostics/E150.md) | A def (function or value-returning flow/stitch) declares a non-`void` return type but its body may fall... |
| [`E151`](diagnostics/E151.md) | A native `{? … }` choice's own body falls through (no divert/return) while a sibling choice in the same... |
| [`E152`](diagnostics/E152.md) | A `contains(m, needle)` call whose `needle` argument is statically visible as outside the map key domain... |
| [`E153`](diagnostics/E153.md) | An `@[allow(…)]` argument is not a diagnostic code this compiler knows (`DiagnosticCode::from_str_code`... |
| [`E154`](diagnostics/E154.md) | An `@[allow(…)]` names a real diagnostic code that is **not suppressible**: |
| [`E155`](diagnostics/E155.md) | An `@[allow(…)]` whose argument list is missing, empty, or not a flat list of bare code identifiers... |
| [`E156`](diagnostics/E156.md) | A lambda body assigns to a captured binding — a `let` binding or parameter declared outside the... |
| [`E157`](diagnostics/E157.md) | An unnamed once-only choice, or an unnamed sequence (`{cycle: …}` / `{stopping: …}` / `{once: …}`... |
| [`E158`](diagnostics/E158.md) | A lambda body reads a name that is a local of the enclosing frame but is not yet bound when lambda... |
| [`E159`](diagnostics/E159.md) | An `@[element(…)]` annotation whose `args` clause is missing, whose value is not a quoted string, or... |
| [`E160`](diagnostics/E160.md) | An `@[element(args = "…")]` pattern's named capture group does not match the name of any parameter... |
| [`E161`](diagnostics/E161.md) | An `@[style(…)]` clause that is not a `key = "value"` pair, or an `@[style(…)]` argument list that... |
| [`E162`](diagnostics/E162.md) | An `@[style(…)]` clause's key is neither `line`, `dispatch`, nor the name of a named capture group... |
| [`E163`](diagnostics/E163.md) | An `@[style(…)]` annotation with no paired `@[element(…)]` or `@[convention(…)]` on the same declaration. |
| [`E164`](diagnostics/E164.md) | An inline markup span (`<name>…</name>`) whose tag name is not declared in the host manifest's markup vocabulary. |
| [`E165`](diagnostics/E165.md) | An inline markup span carries an attribute the host manifest does not declare for that span kind. |
| [`E166`](diagnostics/E166.md) | A `block`-flagged `@[element(…, block)]` / `@[convention(…, block)]` annotation whose declaration has no trailing `content`-typed parameter to receive the captured run, or whose would-be receiver is also one of the pattern's own named captures. |
| [`E167`](diagnostics/E167.md) | A `@[convention(claims = "…", order = N)]` handler declares a parameter that its pattern never captures, so a claimed line has nothing to bind it to. |
| [`E168`](diagnostics/E168.md) | Two `@[convention(claims = "…", order = N)]` handlers declare byte-identical patterns, so the later-declared one can never claim anything the earlier one didn't already claim first. |
| [`E169`](diagnostics/E169.md) | A top-level `fn` carries a pattern-claiming `@[convention(claims = "…", order = N)]` annotation, but this file is not the project's configured conventions module (`brink.toml`'s `[project] conventions`, renamed from `elements` by issue #2180). |
| [`E170`](diagnostics/E170.md) | Two `@[convention(claims = "…", order = N)]` handlers declare textually different patterns that provably overlap, so the later-declared (higher-`order`) one can never claim anything in this file. |
| [`E171`](diagnostics/E171.md) | A `@[convention(claims = "…", order = N)]` handler's captured parameter declares a type other than `string`/untyped/`content`, but every capture binds as a plain string literal — numeric capture coercion is deferred. |
| [`E172`](diagnostics/E172.md) | A native `#…` tag begins with `@` — the ink-dialect directive-tag shape (`#@private`/`#@was`/`#@local`/…) — but native has no directive channel, so it lowers as an ordinary runtime tag. |
| [`E173`](diagnostics/E173.md) | An inline markup span of a declared kind is missing an attribute the host manifest marks `required` for that kind. |
| [`E174`](diagnostics/E174.md) | A lambda's own written annotation (a parameter's `: T` or the lambda's `: R` return annotation) disagrees with the type its body actually infers. |
| [`E175`](diagnostics/E175.md) | RETIRED (issue #2165) — was `register`'s comptime-only-intrinsic confinement check; `fn conventions()`/`register` were dissolved by the 2026-08-03 ruling (`docs/decision-log.md`), so precedence is now a static `order` property with no confinement diagnostic to raise. |
| [`E176`](diagnostics/E176.md) | A divert-with-args site (`-> knot(args)`, tunnel call, or thread-start) supplies the wrong number of arguments for its resolved target's declared parameters — `E031`'s sibling for the divert call shape. |
| [`E178`](diagnostics/E178.md) | A `@[convention(claims = "…")]` annotation has no `order` clause — required, with no default. |
| [`E179`](diagnostics/E179.md) | Two `@[convention(…)]` declarations in the same module carry the same `order` — reported against every declaration in the group. |
| [`E180`](diagnostics/E180.md) | A `@[convention(…, attach = StructName)]` clause names a struct the handler's own declared return type does not agree with. |

## Known limitations

Issues that are documented here so they are not silently rediscovered. Each should be addressed or explicitly accepted.

### Silent data drops

- **`AUTHOR_WARNING` / `TODO:` nodes** — silently dropped during HIR lowering. The `lower_body_children` match does not handle `AUTHOR_WARNING` syntax kind; it falls through to a `debug_assert!` that is a no-op in release builds. These should either be preserved as HIR nodes (for LSP display) or explicitly skipped with a comment.
- **Const evaluation of binary expressions** — `eval_const_expr` in `decls.rs` returns `ConstValue::Null` for any expression that is not a literal, path, divert target, list literal, or prefix negation/not. This means `VAR x = 2 + 3` silently initializes `x` to `Null` instead of `5`. The catch-all `_ => Null` should at minimum emit a diagnostic.
- **String interpolation in const context** — `hir::StringPart::Interpolation(_) => None` silently discards interpolation parts when evaluating const string values, producing a partial string. E030 is emitted as a warning.

### Analyzer gaps

- **No type checking.** Type mismatches (e.g., using a string where a divert target is expected) are not detected.
- **Limited structural validation.** Dead code detection (beyond E033 unreachable-after-divert), unused variable detection, and circular reference checking are not implemented.

### Codegen gaps

- **`StoryData.source_checksum` hardcoded to `0`.** This field exists in the output format but is never computed. It is intended to identify a specific compilation for cache invalidation or locale overlay validation.
