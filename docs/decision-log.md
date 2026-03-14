# Decision Log

## Rename DOTTED_IDENTIFIER to PATH
- **WHEN:** 2026-02-28
- **PROJECT:** brink
- **SYSTEM:** brink-syntax
- **SCOPE:** moderate
- **WHAT:** Rename the `DOTTED_IDENTIFIER` CST node kind to `PATH` throughout the parser and syntax types.
- **WHY:** Aligns with the C# reference implementation, which uses `Path` in both the parsed hierarchy (`Parsed.Path` — a list of `Identifier` components, dot-separated) and the runtime (`Runtime.Path`). The CST node represents the same concept — one or more identifiers separated by dots — so using the same name ensures consistent terminology across the compiler pipeline.

## Remove DIVERT_CHAIN node kind
- **WHEN:** 2026-02-28
- **PROJECT:** brink
- **SYSTEM:** brink-syntax
- **SCOPE:** moderate
- **WHAT:** Remove the `DIVERT_CHAIN` CST node kind. `DIVERT_NODE` directly contains one or more `DIVERT_TARGET_WITH_ARGS` children (and optionally `TUNNEL_ONWARDS_NODE`). No grouping wrapper.
- **WHY:** The C# reference implementation has no concept of a divert chain — `MultiDivert()` returns a flat `List<Parsed.Object>` of independent `Divert` and `TunnelOnwards` objects. The AST layer determines tunnel semantics from position (not-last = tunnel), matching the C# approach on lines 78-87 of `InkParser_Divert.cs`.

## Wrap bare identifier tokens in IDENTIFIER nodes
- **WHEN:** 2026-02-28
- **PROJECT:** brink
- **SYSTEM:** brink-syntax
- **SCOPE:** moderate
- **WHAT:** The parser should wrap bare `IDENT` tokens in `IDENTIFIER` nodes for: knot/stitch names in headers, variable names in VAR/CONST/TEMP declarations, the function name and parameter names in EXTERNAL declarations, and parameter names in KNOT_PARAM_DECL. This makes name extraction uniform via `node.children()` rather than requiring token-level fishing.
- **WHY:** Without wrapping, the AST layer would need to locate the correct `IDENT` token among siblings (EQ, WHITESPACE, KW_FUNCTION, etc.) by position — fragile and inconsistent with how other constructs already use IDENTIFIER nodes. Wrapping makes `.name()` accessor trivial for all named declarations.

## Two-layer structural test macros
- **WHEN:** 2026-02-28
- **PROJECT:** brink
- **SYSTEM:** brink-syntax
- **SCOPE:** minor/local
- **WHAT:** Use a `cst!` macro for exact node-kind matching (skip tokens, assert tree shape) now, and add an `ast!` macro for property-based assertions when the typed AST layer is built. Each layer tests what it's good at — CST tests structure, AST tests semantics.
- **WHY:** Keeps concerns separated. The CST macro is immediately useful without waiting for AST types, and property checks (like `is_tunnel`, `name`) belong on typed AST wrappers rather than a hand-rolled registry of structural checks against the CST.

## Remove PIPE_PIPE compound token from lexer
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-syntax (lexer/parser boundary)
- **SCOPE:** moderate
- **WHAT:** Remove the `PIPE_PIPE` (`||`) compound token from the lexer. The lexer emits only single `PIPE` tokens; the parser handles two consecutive pipes as logical OR in expression context. This follows the same pattern already applied to `PLUS_PLUS` and `MINUS_MINUS`.
- **WHY:** The reference ink compiler is character-level and treats `||` inside `{...}` as two pipe separators with an empty branch between them. Our greedy `PIPE_PIPE` lexer token made `{a||c}` parse as an OR expression instead of a 3-branch sequence. Keeping compound operator recognition in the parser (not lexer) is context-aware and matches the existing precedent for `++`/`--`.

## Brink is the s92 ink compiler, separated for context management
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** architectural
- **WHAT:** Brink is not a standalone open-source ink implementation — it is the s92-studio ink compiler/runtime extracted into its own repo to simplify context management for agents working on it. All s92 runtime requirements (bytecode VM, hot-reload, multi-instance) carry over.
- **WHY:** Agent context windows work better with a focused repo. The separation is organizational, not functional — brink will be consumed by s92-studio.

## Bytecode VM, hot-reload, and multi-instance are core requirements
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** ink-runtime
- **SCOPE:** architectural
- **WHAT:** The bytecode VM, hot-reload safety (symbolic KnotId references, knot-relative offsets, reconciliation), and multi-instance execution (one program, many story instances) are foundational requirements for brink, carried over from s92-studio.
- **WHY:** These are driven by Codetta's game engine needs — NPC dialogue requires multiple concurrent story instances, and live editing during development requires safe hot-reload without invalidating running state.

## HIR lowering and semantic analysis are separate crates
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-hir, brink-analyzer
- **SCOPE:** architectural
- **WHAT:** `brink-hir` (internal crate) owns HIR types and per-file lowering from AST. It produces an HIR tree, a symbol manifest (declarations and unresolved references), and structural diagnostics. `brink-analyzer` (internal crate) takes per-file HIR + manifests, merges them cross-file, resolves references, does semantic validation (types, scopes, dead code), and produces a resolved `SymbolIndex` + semantic diagnostics. The split is per-file/structural (HIR) vs. cross-file/semantic (analyzer).
- **WHY:** Lowering is purely structural (weave folding, implicit knots, fall-through diverts) and doesn't need cross-file context. Analysis needs the whole project. Separating them gives a clean per-file → cross-file pipeline, makes HIR types a stable interface for both analyzer and codegen, and keeps lowering independently testable.

## brink-format firewall between compiler and runtime
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-format, brink-runtime
- **SCOPE:** architectural
- **WHAT:** `brink-format` is an internal crate that defines the binary interface between compiler and runtime — `Program` struct, opcode definitions, ID types (`KnotId`, `KnotRef`, `StringId`, etc.), and serialization. `brink-runtime` depends ONLY on `brink-format`, nothing else from the brink crate family. Whether `brink-format` is published or internal is deferred.
- **WHY:** The runtime must be decoupled from parsing, analysis, and compilation. This enables shipping compiled stories without the compiler, keeps the runtime suitable for `no_std`/WASM targets, and lets compiler and runtime evolve independently.

## Containers are the fundamental compilation and runtime unit
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-format, brink-runtime, brink-compiler
- **SCOPE:** architectural
- **WHAT:** Containers (not knots or stitches) are the fundamental unit in both the compiler and runtime, analogous to functions in a normal PL runtime. Knots, stitches, gathers, and labeled choice targets are all containers. Each container compiles to its own bytecode chunk. Entering a container pushes a frame, exiting pops one. Visit counting, hot-reload granularity, and addressing are all per-container.
- **WHY:** Matches the reference ink runtime's model (which has no knot/stitch distinction at runtime — everything is a Container). Unifies the execution model instead of special-casing stitches vs sub-stitch positions. Visit counting and hot-reload fall out naturally from container boundaries.

## Bytecode uses ContainerIds (path hashes), resolved at load via linker step
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-format, brink-runtime
- **SCOPE:** architectural
- **WHAT:** The `.inkb` format stores ContainerIds (hash of fully qualified path, e.g. hash of `"knot.stitch"`) in bytecode instructions. There are no compile-time indices in the file format. At load time, the runtime runs a linker step that resolves ContainerIds to fast internal indices for execution. ContainerId is the stable identity; internal indices are a runtime-only optimization.
- **WHY:** Makes `.inkb` self-describing and decouples the file format from runtime memory layout. Enables hot-reload patching: update container blobs in the unlinked layer, re-link, reconcile instances — same codepath for full recompile or granular patch.

## Runtime holds unlinked + linked layers; patching re-links
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **WHAT:** The runtime maintains two layers: an unlinked layer (`HashMap<ContainerId, ContainerBytecode>` with symbolic references) and a linked layer (resolved `Program` with fast indices). Loading, hot-reload, and patching all flow through the same linker step. A patch updates the unlinked layer, then re-links to produce a new resolved program.
- **WHY:** Unifies the startup and hot-reload codepaths. The compiler doesn't need a special patch format — it sends changed containers, and the runtime re-links. Granular patch vs full recompile is just a partial vs full update to the unlinked layer before the same link step.

## Plural resolution delegated to host with batteries-included option
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-runtime, brink-format
- **SCOPE:** moderate
- **WHAT:** The runtime defines a `PluralResolver` trait for locale-aware plural category resolution. The runtime itself ships no locale data. A separate `brink-intl` crate (or feature-gated module) provides a batteries-included resolver backed by ICU4X baked data, pruned at build time to only the locales the consumer specifies. Consumers with their own i18n system implement the trait directly. Stories that don't use localization don't need a resolver (fallback: everything maps to `Other`).
- **WHY:** Keeps the runtime lean and `no_std`-compatible while making the common case easy. Most consumers won't understand CLDR plural rules — they should just list the locales they need and get correct behavior. Consumers with existing game engine i18n don't want brink pulling in duplicate locale data.

## XLIFF 2.0 as the localization authoring format
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-cli, brink-format
- **SCOPE:** architectural
- **WHAT:** Localization authoring uses one XLIFF 2.0 file per locale (`translations/<locale>.xlf`), with containers as `<file>` elements within the document. `brink-cli generate-locale` produces these from a compiled `.inkb`. `brink-cli compile-locale` compiles them into binary `.inkl` overlay files for the runtime. Brink-specific data (content hashes, audio asset references) uses XLIFF's custom namespace extension mechanism (`brink:contentHash`, `brink:audio`). Translation state tracking uses XLIFF's built-in `state` attribute (`initial`/`translated`/`reviewed`/`final`). No separate TOML or custom format — XLIFF is both the authoring format and the interchange format. CLI convenience commands handle common operations so nobody has to write XML by hand.
- **WHY:** XLIFF is the industry standard localization interchange format. Every translation management platform (Lokalise, Crowdin, Phrase, etc.) natively imports/exports it. Using XLIFF directly eliminates the need for a conversion layer between an authoring format and an interchange format. The spec requires tools to preserve unknown extensions, so brink-specific metadata (audio refs, content hashes) survives round-trips through external tooling. Supersedes the earlier TOML-per-container approach.

## Uniform DefinitionId with tagged type discriminant
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-format
- **SCOPE:** architectural
- **WHAT:** All named definitions (containers, global variables, list definitions, list items, external functions) use a single `DefinitionId(u64)` type. The high 8 bits are a type tag identifying which table the definition belongs to; the low 56 bits are a hash of the fully qualified name/path. The linker resolves all `DefinitionId` references uniformly to compact runtime indices. Temporary variables are excluded — they live on the stack frame with no format-level definition. String and message tables remain index-based (`StringId`/`MessageId`) since they are content, not named definitions.
- **WHY:** Unifies the linker into a single codepath regardless of definition type. Simplifies hot-reload reconciliation to one pass over the definition set. The type tag prevents cross-type collisions and tells the linker which table to dispatch to. u64 provides collision-free hashing for stories with thousands of containers. The runtime never sees these IDs on the hot path — they're resolved to small indices at link time.

## brink-hir exports composable transforms, not just a monolithic lowering pass
- **WHEN:** 2026-03-02
- **PROJECT:** brink
- **SYSTEM:** brink-hir
- **SCOPE:** architectural
- **WHAT:** brink-hir provides a convenience `lower(ast::SourceFile) -> (HirTree, SymbolManifest, Vec<Diagnostic>)` entry point for the common per-file case, but the underlying transforms (weave folding, knot lowering, manifest extraction, etc.) are individually public. This lets brink-analyzer call `fold_weave` on merged cross-file top-level content after INCLUDE resolution, without brink-hir needing cross-file awareness.
- **WHY:** Top-level weave folding requires the merged content stream from INCLUDE resolution, which is inherently cross-file. A purely monolithic per-file pass can't handle this. Exposing the transforms individually lets the analyzer compose them with cross-file context while keeping brink-hir's own code single-file and independently testable.

## Merge MessageId and LineId into LineId scoped to containers
- **WHEN:** 2026-03-01
- **PROJECT:** brink
- **SYSTEM:** brink-format
- **SCOPE:** architectural
- **WHAT:** MessageId and LineId are merged into a single `LineId = (DefinitionId, u16)` — the container's DefinitionId plus a local index within that container. All user-visible text output uses LineId. Each container carries its own line sub-table (content + source text content hash). The content hash enables locale tooling to detect changes and reset review status. `NameId(u16)` remains as a positional index into the name table for internal strings (definition names, debug labels) — it is not localizable and not hot-reload-sensitive. The full ID type set is: `DefinitionId(u64)` for definitions, `LineId(DefinitionId, u16)` for text output, `NameId(u16)` for internal names.
- **WHY:** Positional global indices (the old `MessageId(u16)`) are fragile across recompilation — any insertion shifts all subsequent indices, breaking `.inkl` overlays and hot-reload. Scoping to containers makes hot-reload naturally granular (container changes → its lines change, others untouched), makes `.inkl` overlays stable per-container, and eliminates the redundancy between MessageId and LineId. The content hash alongside each line enables the XLIFF regeneration tool to preserve translations while flagging changed source text for re-review.

## Pluggable PRNG for runtime
- **WHEN:** 2026-03-03
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **WHAT:** RNG is pluggable via a `StoryRng` trait on `Story<R>`. Two built-in impls: `FastRng` (default, simple/fast) and `DotNetRng` (.NET `System.Random` compat for reference ink fidelity). Game engines (bevy_prng, etc.) can provide their own.
- **WHY:** Runtime divergence from reference ink's RNG is fine to "good, actually" — determinism matters but the specific algorithm should be the engine's choice, not hardcoded to .NET's `System.Random`.

## Choice text: dual-path bytecode model
- **WHEN:** 2026-03-02
- **PROJECT:** brink
- **SYSTEM:** brink-format / brink-runtime
- **SCOPE:** architectural
- **WHAT:** Choice text supports two bytecode patterns: (1) High-level: `ChoiceDisplay(idx)` / `ChoiceOutput(idx)` reference pre-computed line table entries — static, localizable, optimized. (2) Low-level: `begin_string_eval` / `enter_container` / `end_string_eval` evaluates arbitrary code to produce display text; target container handles output text. `BeginChoice(flags, target_id)` always pops display text from the value stack regardless of which path produced it. `ChoiceOutput` is optional metadata. ink.json conversion is a long-term supported feature (with caveats), not just temporary spike tooling, so the low-level path is first-class.
- **WHY:** Ink is extremely flexible — choice text can contain arbitrary logic that can't always be statically decomposed into line table entries. The high-level path enables localization (`.inkl` overlays swap line tables) and optimization. The low-level path ensures correctness for complex dynamic cases and for the ink.json converter, which doesn't have access to the original bracket syntax.

## Layered execution model: dumb VM, smart orchestration
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **WHAT:** The runtime execution model is split into three state types (Flow, Context, Program) and four execution layers. **Flow** is an isolated execution context (threads/call stack, value stack, output buffer, pending choices, external function pending state). **Context** is saveable game state (globals, visit counts, turn counts, turn index, RNG seed). **Program** is immutable linked bytecode. The VM (`vm::step(flow, context, program) -> Stepped`) processes a single instruction and returns — it is maximally dumb. Higher layers build on this: line-level continuation (loops until newline boundary, handles glue lookahead), passage-level continuation (loops until choices/done/ended), and the Story orchestrator (manages flows, contexts, external function binding, choice selection). External functions yield from the VM; the caller provides a return value or invokes the ink fallback. Thread completion is visible to the caller via a `ThreadCompleted` variant.
- **WHY:** The previous model had a single `step()` that ran to completion (equivalent to `ContinueMaximally`), making it impossible to associate tags with specific lines, handle external functions, or give callers fine-grained control. The new model makes the VM as dumb as possible — higher layers orchestrate. This enables per-line tag association (fixes i18n test failures), fire-and-forget external calls (common in game integration), and future multi-flow support without changing the VM.

## Instanced flows with per-entity contexts
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **WHAT:** Flows can be instanced — the Story can spawn multiple (Flow, Context) pairs for the same scene template in the Program, each with fully independent state (visit counts, globals, conversation progress). The Story provides flow-specific context support for mapping entities to their instances. For example, a shopkeeper conversation scene defined once in ink can be instantiated per-NPC. When the Story executes an instanced flow, it uses the instance-specific context rather than the default.
- **WHY:** Enables multi-NPC/multi-entity support from a single ink scene definition without duplicating ink source. Each entity maintains independent conversation state (which dialogue branches they've seen, what variables they've set, etc.). Falls out naturally from the Flow/Context separation — no special VM support needed, purely an orchestration concern at the Story layer.

## External function handler is a trait, not a binding map
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** moderate
- **WHAT:** External function resolution uses a trait (`ExternalFnHandler`) passed to the orchestration layer, not a `HashMap<String, Handler>` stored on `Story`. The handler returns an enum: `Resolved(Value)` (done), `Fallback` (use ink fallback body), or `Pending` (async, caller resolves later via `story.resolve_external()`). The `Program` stores external fn metadata (name, fallback container) from `StoryData.externals` via the linker.
- **WHY:** Different consumers need different strategies for when to fallback vs error vs async-resolve. Baking a binding map into Story couples the resolution policy to the runtime. A trait lets the orchestration layer be agnostic to individual function mapping.

## Explicit registration for instanced flow variable scoping
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **WHAT:** Instanced flows use explicit registration to determine which globals are shared vs per-instance. The game developer registers shared globals when setting up an instance template. Everything else in the Context (visit counts, turn counts, turn index, RNG, and all unregistered globals) is per-instance by default. The VM sees a flat key-value store; the backing store handles the shared/instance split transparently.
- **WHY:** Explicit registration is easier for the writer to understand than convention-based (file-of-origin) or static-analysis (reference graph) approaches. No magic — the game developer knows their data model and declares it. The ink author doesn't need to understand instancing at all; the runtime handles it.

## HIR is a rich semantic tree, not a thin bytecode-adjacent IR
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-hir
- **SCOPE:** architectural
- **WHAT:** The HIR preserves full source structure with semantic nesting resolved. Weave folding, implicit structure, and sugar stripping are applied, but all semantic information is retained: expressions stay as trees (not stack ops), choices/sequences/conditionals keep their branch structure, diverts/tunnels/threads are semantic nodes (not jump instructions), tags stay associated with their content. Both brink-analyzer and brink-compiler (codegen) consume the HIR. Codegen does the last-mile lowering from semantic nodes to bytecode.
- **WHY:** Enables structural diagnostics during HIR folding (malformed weave, orphaned gathers) and richer semantic errors during analysis (the analyzer sees full structure, not half-lowered IR). Keeps codegen cleanly separated as a walk over semantic nodes. A thin bytecode-adjacent IR would force diagnostic information to be reconstructed or lost.

## Per-knot incremental HIR lowering boundary
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-hir / brink-analyzer
- **SCOPE:** architectural
- **WHAT:** HIR lowering is per-knot granular. brink-hir exports per-knot lowering functions (`lower_knot`, `lower_top_level`) alongside the convenience whole-file `lower()`. The analyzer caches HIR per knot and uses rowan green node identity to skip re-lowering unchanged knots after incremental reparse. Knots are structurally independent so this is safe. `fold_weave` operates at the `Block` level, reusable for both per-knot and cross-file INCLUDE merging.
- **WHY:** Rowan's incremental reparse already tells us which knots changed via green node identity. Per-knot caching exploits this for O(changed knots) instead of O(all knots) on each keystroke. The composable transform API makes this natural without adding complexity to brink-hir itself.

## HIR source provenance via AstPtr
- **WHEN:** 2026-03-04
- **PROJECT:** brink
- **SYSTEM:** brink-hir / brink-syntax
- **SCOPE:** architectural
- **WHAT:** HIR nodes carry `AstPtr<N>` (SyntaxKind + TextRange, typed via PhantomData) for source provenance. Resolves back to live AST nodes given the syntax tree root. Lightweight (no Arc, no lifetime coupling), typed, and supports LSP refactoring workflows (rename, lint fix, extract/inline). Stale pointers from previous parses fail gracefully on resolution. `AstPtr` is implemented in brink-syntax.
- **WHY:** TextRange alone is sufficient for pointing at errors but not for structural refactoring (rename, lint fix, extract). `AstPtr` bridges HIR back to CST without lifetime coupling, following the pattern proven by rust-analyzer.

## Rename brink-hir to brink-ir, add LIR for codegen
- **WHEN:** 2026-03-05
- **PROJECT:** brink
- **SYSTEM:** brink-ir (formerly brink-hir), brink-compiler
- **SCOPE:** architectural
- **WHAT:** Rename `brink-hir` to `brink-ir`. The existing HIR types and lowering become a submodule (`brink_ir::hir`). A new LIR submodule (`brink_ir::lir`) defines the post-analysis, codegen-ready representation. The analyzer transforms HIR → LIR (resolving names, assigning container boundaries, connecting loose ends, assigning temp slots). Codegen backends walk the LIR to emit output. Two backends: `.inkb` (brink native bytecode) and `.ink.json` (inklecate-compatible JSON, so brink can be a drop-in replacement compiler for the reference ink ecosystem). The LIR is backend-agnostic — it captures the resolved, linearized program structure without committing to a specific instruction encoding.
- **WHY:** The HIR is a rich semantic tree optimized for diagnostics and analysis — expressions as trees, nested choice structures, unresolved paths. Codegen needs a different shape: resolved references, decided container boundaries, linearized control flow, loose ends connected. A separate LIR avoids retrofitting codegen concerns into the HIR and keeps the analyzer's output cleanly separated from its input. The dual-backend requirement (`.inkb` + `.ink.json`) further motivates a backend-agnostic intermediate form — the two targets have fundamentally different instruction models, so both need to walk a common resolved representation rather than being bolted onto HIR directly.

## Compiler is I/O-agnostic, file reading injected by caller
- **WHEN:** 2026-03-05
- **PROJECT:** brink
- **SYSTEM:** brink-compiler
- **SCOPE:** architectural
- **WHAT:** `brink-compiler` does not perform filesystem I/O directly. File reading is injected by the caller (closure, trait, or similar mechanism). The compiler discovers INCLUDEs by parsing, then calls back to the caller to resolve and read each included file. A thin convenience wrapper (`compile_path` or similar) handles the common case of reading from disk. The core `compile` entry point works in WASM, tests, and editor contexts without a real filesystem.
- **WHY:** The compiler needs to run in WASM (no filesystem), in tests (HashMap of fake files), and potentially in editor contexts (buffers, not disk). Baking `std::fs` into the compiler would make all of these require workarounds. Injecting I/O keeps the compiler pure and the platform shim minimal.

## brink-ir owns all symbol types
- **WHEN:** 2026-03-05
- **PROJECT:** brink
- **SYSTEM:** brink-ir, brink-analyzer
- **SCOPE:** architectural
- **WHAT:** `brink-ir` owns both `SymbolManifest` (produced by HIR lowering) and `SymbolIndex`/`SymbolInfo`/`SymbolKind`/`Scope` (populated by the analyzer). The analyzer depends on `brink-ir` for these types and provides the logic to fill them — no type definitions of its own except its result wrapper. This lets `brink-ir::lir` consume `SymbolIndex` directly for LIR construction without depending on the analyzer.
- **WHY:** The IR crate owns data structures; the analyzer owns logic. Keeping symbol types in `brink-ir` avoids a circular dependency (LIR needs resolved symbols, analyzer produces them) and makes the LIR lowering independently testable. The analyzer becomes a pure transform: manifests in, populated index out.

## Flat Exxx diagnostic codes
- **WHEN:** 2026-03-05
- **PROJECT:** brink
- **SYSTEM:** cross-system (diagnostics infrastructure)
- **SCOPE:** architectural
- **WHAT:** Flat `E001`–`E999` error codes, single global namespace shared across all crates (parser, HIR, analyzer). Codes are never reused once assigned. Each code gets a `.md` explanation file in `docs/diagnostics/`. A central enum maps code → short title + default severity. Warnings may later get a separate `W` prefix or lint system but for now share the `E` space. Modeled after rustc's `E0xxx` system.
- **WHY:** Gives us a stable, documentable, user-facing error catalogue. Flat namespace is simpler than per-layer prefixes. Explanation files enable `--explain` CLI support and generated docs. Never-reuse policy keeps old references valid.

## `FLOW VAR` keyword for per-instance variable scoping
- **WHEN:** 2026-03-05
- **PROJECT:** brink
- **SYSTEM:** brink-syntax, brink-format, brink-runtime
- **SCOPE:** architectural
- **WHAT:** `FLOW VAR x = false` declares a variable scoped to the flow instance rather than shared globally. `VAR` (no modifier) remains the default and is shared across all instances. The instance flag propagates through the compiler into `GlobalVarDef` as a single bit. The linker partitions globals into shared/instance ranges. The Context provides a split backing store transparent to the VM — `GetGlobal`/`SetGlobal` don't branch on scoping. Files that don't use `FLOW VAR` are standard ink.
- **WHY:** Multi-instance flows need per-entity state (conversation progress, local flags) without polluting the shared global namespace. Explicit author opt-in via a keyword is self-documenting and avoids runtime registration APIs. `FLOW` ties directly to the runtime's flow concept. Minimal compatibility impact — zero cost for authors who don't use instancing.

## Positions use resolved indices at runtime
- **WHEN:** 2026-03-05
- **PROJECT:** brink
- **SYSTEM:** runtime-spec
- **SCOPE:** moderate
- **WHAT:** Call frame positions use resolved runtime indices (`u32` container index + `usize` offset), not symbolic `(DefinitionId, offset)`. Translation to/from `DefinitionId` happens at reconciliation (`story.reload`) and save/load boundaries, not during execution.
- **WHY:** Resolved indices are faster on the hot path (no hash lookups per opcode). The program is immutable after linking, so indices are stable until reload. Reconciliation is a one-time batch pass that can map old→new via the old and new programs. Paying per-opcode cost for a rare operation (reload) is the wrong tradeoff.

## Built-in function recognition belongs in the analyzer
- **WHEN:** 2026-03-06
- **PROJECT:** brink
- **SYSTEM:** brink-analyzer
- **SCOPE:** moderate
- **WHAT:** The analyzer recognizes ink built-in function names (TURNS_SINCE, CHOICE_COUNT, RANDOM, SEED_RANDOM, INT, FLOAT, FLOOR, CEILING, POW, MIN, MAX, LIST_COUNT, LIST_MIN, LIST_MAX, LIST_ALL, LIST_INVERT, LIST_RANGE, LIST_RANDOM, LIST_VALUE, LIST_FROM_INT) and does not emit E025 (unresolved reference) diagnostics for them. The set of built-ins is defined in brink-ir (shared between analyzer and LIR) so both layers agree on what's built-in. LIR lowering maps these to `Expr::CallBuiltin` variants.
- **WHY:** Without this, calls to built-in functions produce false "unresolved reference" errors because the analyzer can't find a declaration for them in any manifest. Built-in recognition is a semantic concern — the analyzer already resolves all other references, so it should also know which names are compiler-provided. Defining the built-in set in brink-ir avoids duplication between the analyzer and codegen.

## Flag silent data drops
- **WHEN:** 2026-03-06
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** architectural
- **WHAT:** Any time a lowering pass, transform, or conversion silently drops data (AST children, HIR nodes, content parts, etc.) without emitting a diagnostic or error, it must be flagged immediately. Silent drops are always bugs until proven otherwise. Agents must report silent drops to the user before attempting to fix them.
- **WHY:** The `{ expr: ... - else: ... }` pattern in HIR lowering silently dropped block-level constructs (temp declarations, nested conditionals, return statements) inside `InlineBranch.content` because `lower_content_node_children` only handles a subset of node kinds. This went unnoticed until LSP folding exposed the missing data. Silent drops are insidious — they produce no errors, pass all existing tests, and only surface when downstream consumers notice missing information.

## LIR container tree with ID-based lookup
- **WHEN:** 2026-03-06
- **PROJECT:** brink
- **SYSTEM:** brink-ir (LIR)
- **SCOPE:** architectural
- **WHAT:** Restructure LIR from flat `Vec<Container>` to a tree (`Program.root: Container` with `Container.children: Vec<Container>`). All containers live in the tree — ChoiceSet/Choice reference their target containers by `DefinitionId`, backends build a `HashMap<DefinitionId, &Container>` for O(1) lookup. No embedding containers inside statements.
- **WHY:** The flat list forced the JSON backend to reverse-engineer parent-child relationships from path strings, which was fragile and lossy. The tree preserves structure that's known during HIR→LIR lowering. Both backends benefit from uniform tree traversal. ID-based references keep the indirection consistent with how diverts already work, and a one-time lookup map avoids scan overhead.

## Conformance work loop for JSON corpus
- **WHEN:** 2026-03-06
- **PROJECT:** brink
- **SYSTEM:** cross-system (brink-compiler, brink-ir, brink-analyzer)
- **SCOPE:** process
- **WHAT:** When working on JSON corpus conformance, follow a loop: run corpus test → root-cause first failure → present analysis to user for decisioning on fix location → implement → commit → repeat. Work first-failure-first in sorted order (tier1 basics first).
- **WHY:** Greenfield with 0/384 passing. Fixes often cascade — one root cause can unblock many tests. Working first-failure-first ensures a solid foundation. Presenting the fix location before implementing prevents wasted work when the fix belongs in a different layer than expected.

## RCA-first work loop with failing tests
- **WHEN:** 2026-03-10
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** moderate
- **WHAT:** The episode corpus work loop should be: (1) find the first failure, (2) root-cause it, (3) write failing tests that would pass if the RCA was addressed, (4) enter plan mode and present the RCA, failing tests, and proposed fix for approval — before implementing anything.
- **WHY:** The user wants to review the RCA and fix approach before implementation. Writing failing tests first proves the diagnosis is correct and provides a regression gate. This replaces the previous loop where the agent would explain the RCA in prose and then implement immediately after approval.

## Program ownership: borrowed references, Arc deferred
- **WHEN:** 2026-03-13
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **STATUS:** tentative
- **WHAT:** The `Program` type (and the `LinkedBinary`/`LinkedLocale` split) uses borrowed references (`&'p`) for now, not `Arc`. `Story<'p>` continues to borrow the program. `Arc` upgrade is deferred until Bevy `Handle<T>` integration requires it.
- **WHY:** Borrowed references are simpler, provide compile-time lifetime guarantees, and have zero overhead. The current use case (single-threaded game loop, caller owns everything) doesn't need shared ownership. `Arc` only helps when `Story` needs to be handed off without the caller also holding the `Program`, or for cross-thread sharing. Bevy integration will likely need `Arc` for its asset pipeline, but that's a future concern.

## compile-locale requires .inkb as base input
- **WHEN:** 2026-03-13
- **PROJECT:** brink
- **SYSTEM:** brink-intl
- **SCOPE:** moderate
- **WHAT:** The `compile-locale` command requires `.inkb` as the `--base` input, not `.ink.json` or `.inkt`. This ensures the base checksum is always valid for `.inkl` header validation.
- **WHY:** When loading from `.ink.json` (converter path), the checksum is 0, which would make stale-translation detection impossible. Requiring `.inkb` keeps the validation chain intact. Users must compile to `.inkb` first, which is the intended production workflow anyway.

## General-purpose XLIFF 2.0 crate + brink-intl separation
- **WHEN:** 2026-03-13
- **PROJECT:** brink
- **SYSTEM:** brink-intl / xliff crate
- **SCOPE:** architectural
- **WHAT:** XLIFF 2.0 support is split into two crates: a general-purpose XLIFF 2.0 crate (format-only, publishable to crates.io) and `brink-intl` (brink-specific reconciliation, locale tooling). The XLIFF crate is a dependency of brink-intl. The XLIFF crate handles read/write/data model for XLIFF 2.0 documents. All brink-specific concerns (regeneration/merge workflow, `.inkl` compilation, content hash comparison, audio ref mapping) live in brink-intl.
- **WHY:** The Rust ecosystem has no usable XLIFF 2.0 library (the existing `xliff` crate is abandoned, alpha-only, and only supports XLIFF 1.2). Keeping the format crate general-purpose benefits the community and enforces clean separation from brink-specific concerns. The XLIFF spec is complex enough to warrant its own crate boundary.

## Break opcode format for slot count
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-format / intl-spec
- **SCOPE:** architectural
- **WHAT:** Change `EmitLine(u16)` → `EmitLine(u16, u8)` and `EvalLine(u16)` → `EvalLine(u16, u8)` to carry slot count. Breaking format change.
- **WHY:** We're still greenfield — better to do it properly now than work around it later. Explicit slot count catches codegen bugs at runtime.

## Combine interpolation recognizers
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-ir / intl-spec
- **SCOPE:** moderate
- **WHAT:** Single-interpolation and multi-interpolation pattern recognizers are implemented as one general recognizer, not phased separately.
- **WHY:** The implementation is naturally general — no algorithmic reason to limit to one slot. The spec's phasing was a suggestion for incremental delivery, not a hard requirement.

## Dedicated template test corpus
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-test-harness / intl-spec
- **SCOPE:** moderate
- **WHAT:** Build a dedicated test corpus for template features since the episode corpus (based on inklecate) can't validate them.
- **WHY:** Templates are a brink-specific feature that other ink runtimes don't have. Requires investment in purpose-built test cases.

## Metadata fields as stubs
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-format / intl-spec
- **SCOPE:** minor/local
- **WHAT:** Add `slot_info` and `source_location` to `LineEntry` as stub fields (types defined, serialized, but not populated with real data yet).
- **WHY:** Get the types and binary format in place now so we don't need another format break later.

## Select defaults to fallback
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-runtime / intl-spec
- **SCOPE:** moderate
- **WHAT:** Template resolution handles `LinePart::Select` by always using the `default` value. Full plural resolution deferred to Phase 6.
- **WHY:** Unblocks template support without requiring ICU4X/PluralResolver infrastructure. The default fallback is correct behavior when no resolver is configured.

## Regeneration uses hash-based alignment, not index-based matching
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-intl
- **SCOPE:** moderate
- **WHAT:** `regenerate-lines` matches old→new lines by aligning the hash sequences within each scope (LCS or similar), not by matching on `(scope_id, line_index)`. Index-matched lines with mismatched hashes are not assumed to be "changed" — they may be shifted. Hash-equal lines are presumed identical regardless of index. After alignment: unmatched new lines are `untranslated`, unmatched old lines are `orphaned`, hash-matched lines at different indices preserve their translation.
- **WHY:** Inserting or deleting a line in the middle of a scope shifts all subsequent indices. Naive index matching would mark every shifted line as `needs_review` and lose the association between the old translation and its (unchanged) source line. Hash-based alignment correctly detects that the content didn't change — only its position did.

## xliff2 crate naming
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** intl-spec
- **SCOPE:** moderate
- **WHAT:** The publishable XLIFF crate is named `xliff2`, covering XLIFF 2.0 only.
- **WHY:** `xliff` is taken on crates.io. XLIFF 1.2 and 2.0 are fundamentally different schemas — bundling both doubles surface area for no immediate benefit. The `2` suffix clearly communicates scope. 1.2 can be a separate crate later if needed.

## Use thiserror for error types
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** moderate
- **WHAT:** All error types should use `thiserror` derives, not hand-rolled `Display` + `Error` impls. New crates must use thiserror. Existing crates should be migrated when touched.
- **WHY:** The hand-rolled pattern is boilerplate-heavy and error-prone. `thiserror` is already in workspace deps and produces identical output with less code.

## xliff2 module architecture
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** xliff2 crate / intl-spec
- **SCOPE:** architectural
- **WHAT:** Core XLIFF 2.0 types carry generic extension storage (raw namespace-qualified elements/attributes). Known modules (Metadata, etc.) are feature-gated and provide typed accessors over the extensions bag. Unknown extensions are preserved through read/write round-trips. Initial release includes core + metadata module.
- **WHY:** XLIFF 2.0 modules are separate namespaces by design. A generic extensions mechanism means adding module support later is purely additive — no core type changes needed. Feature gates keep the dependency/surface area minimal for consumers who don't need every module.

## CLI intl commands speak XLIFF only
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-cli / brink-intl
- **SCOPE:** moderate
- **WHAT:** The CLI's localization commands use XLIFF 2.0 as the sole external format. `LinesJson` is an internal implementation detail, not a user-facing format.
- **WHY:** The JSON lines format was a placeholder internal representation. XLIFF is the industry-standard TMS interchange format — there's no reason to expose two formats to users.
