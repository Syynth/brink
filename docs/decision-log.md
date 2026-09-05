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

## XLIFF uses scope names as IDs, not definition IDs
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-intl
- **SCOPE:** moderate
- **WHAT:** XLIFF `<file id>` and `<unit id>` use human-readable scope names (knot.stitch paths) instead of hex definition IDs. Definition IDs are preserved in `brink:scope-id` extension attributes for reconciliation. Unit IDs become `scope_name:line_index` instead of `hex_id:line_index`.
- **WHY:** TMS tools (Poedit etc.) display the `id` attribute prominently. Hex definition IDs are meaningless to translators; scope names provide immediate context.

## Source filename missing from StoryData — spec gap
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-format / brink-intl
- **SCOPE:** moderate
- **STATUS:** tentative
- **WHAT:** StoryData does not carry source `.ink` filenames. XLIFF `<file original>` should ideally reference the source file, but this data isn't available. Needs to be addressed in brink-format.
- **WHY:** Translators need source file context. Multi-file ink projects would benefit from knowing which `.ink` file a scope came from.

## Branch expansion for inline sequences/conditionals in content recognizer
- **WHEN:** 2026-03-15
- **PROJECT:** brink
- **SYSTEM:** brink-ir / content recognizer
- **SCOPE:** moderate/architectural
- **WHAT:** Inline sequences and inline conditionals in content lines should be expanded at compile time into their cartesian product of complete lines. Each expanded line is independently recognized as Plain/Template and gets its own LineEntry. The runtime selects which line to emit rather than assembling text from parts. Multiple branch points in a single line produce the full cartesian product (which accurately reflects the number of distinct voicelines/translations needed). Branches containing interpolations become templates. If after expansion a branch still contains unrecognizable content, that branch falls back to EmitContent. No arbitrary complexity threshold — just recurse until recognition succeeds or falls back naturally.
- **WHY:** Each combination of branches IS a distinct translatable/voiceable line. Translators should see complete sentences, not template fragments with invisible alternatives. This also means the line table accurately represents the full set of possible outputs, which is required for translation coverage and voiceover workflows.

## Extricate line_tables from Program
- **WHEN:** 2026-03-14
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** architectural
- **WHAT:** `Program.line_tables` should not be owned by `Program`. Line tables are mutable per-locale content, not immutable linked program data. They should be extracted into a separate structure that can be swapped without touching `Program` or fighting the borrow checker.
- **WHY:** `Program` is designed as immutable post-link, but `apply_locale()` requires `&mut self` solely to swap line tables. This forces `Story` (which borrows `&Program`) to be dropped and recreated for locale changes. The line tables were placed in `Program` by agents who didn't anticipate runtime locale switching. Separating them enables hot-swap without state gymnastics.

## #voice tags are not real
- **WHEN:** 2026-03-15
- **PROJECT:** brink
- **SYSTEM:** intl
- **SCOPE:** minor/local
- **WHAT:** `#voice:` tags are not a real ink feature. Remove references to audio_ref population via tag parsing from intl docs/specs. The `audio_ref` field on `LineEntry` stays as a carrier for game-engine integration, but the compiler will never populate it from tags.
- **WHY:** `#voice:` was an assumed feature that doesn't exist in ink. Documenting it as planned work creates false expectations.

## User-configurable project scoping in LSP
- **WHEN:** 2026-03-16
- **PROJECT:** brink
- **SYSTEM:** brink-lsp
- **SCOPE:** moderate
- **WHAT:** The LSP should eventually support user-configurable control over how .ink files are grouped into projects for analysis. The current auto-discovery (root = file with no includers) is a reasonable default, but users need a way to override it.
- **WHY:** Auto-discovery produces false positives when a workspace contains multiple independent include trees that share files (e.g., test harness files that INCLUDE a subset). Users need to be able to declare project roots, exclude files, or otherwise control scoping.

## brink-studio standalone app uses Tauri
- **WHEN:** 2026-03-17
- **PROJECT:** brink-studio
- **SYSTEM:** brink-studio
- **SCOPE:** moderate
- **WHAT:** Standalone desktop app ships as a Tauri app wrapping the same CM6+wasm frontend. Wasm backend (not native Rust) for now to keep one code path.
- **WHY:** Natural fit — Rust codebase, tiny binary, proper filesystem access without File System Access API limitations. Wasm-only backend keeps things simple since s92-studio also uses wasm.

## Fountain-inspired context-dependent element transitions
- **WHEN:** 2026-03-18
- **PROJECT:** brink-studio
- **SYSTEM:** editor-ui
- **SCOPE:** architectural
- **WHAT:** brink-studio uses Fountain screenplay syntax (https://fountain.io/syntax/) as a spiritual guide for context-dependent element transitions. Element types can depend on preceding elements (e.g., in Fountain: dialogue follows character, character follows blank line). The transition table supports a `context` field for matching on preceding line types.
- **WHY:** Ink's structural elements (choices, gathers, narrative in choice bodies) have similar context-dependent behavior to Fountain's character/dialogue/parenthetical chain. Making the transition engine context-aware enables natural screenplay-style editing where pressing Enter produces the right element type based on what came before, not just what the current line is.

## EditorSession with cached parse/HIR as single source of truth
- **WHEN:** 2026-03-18
- **PROJECT:** brink-studio
- **SYSTEM:** brink-web, editor-ui
- **SCOPE:** architectural
- **WHAT:** Replace the stateless wasm functions and regex-based `classifyLine` with a stateful `EditorSession` that caches parse/HIR/analysis. A new `line_contexts()` method walks the cached HIR to produce authoritative per-line context (weave position with depth, element type, inline ranges for tags/block comments/brackets/etc.). This becomes the single source of truth for all editor decorations, the transition table, and the status bar. Regex-based line classification is removed.
- **WHY:** The regex classifier (`classifyLine`) is a fast approximation of what the parser already knows, and it gets weave depth wrong for choice body text and nested structures. Every wasm call currently reparses from scratch (no caching). An `EditorSession` fixes both problems: one parse per doc change, and all queries read authoritative data from the cached HIR. `cursor_scope` stays separate (knot/stitch visibility for completions); the new `WeavePosition` (depth + element enum) handles structural position for the editor. `ChoiceSet.depth` is added to the HIR so weave depth survives lowering.

## FileProvider: async host-owned file I/O interface
- **WHEN:** 2026-03-19
- **PROJECT:** brink
- **SYSTEM:** brink-studio
- **SCOPE:** architectural
- **WHAT:** File I/O is async and owned by a host-provided `FileProvider` interface. The driver/session layer bridges the host and wasm session. Compilation is synchronous against pre-loaded files. The `FileProvider` interface includes: `listFiles()`, `readFile(path)`, `requestFile(path)` (driver asks host to load a dependency), `onFileChanged(path, content)` (driver notifies host of editor changes), `onExternalChange(callback)` (host notifies driver of external file modifications), and `requestSave()` (best-effort persist request with no guarantees). Different hosts implement this differently: web uses in-memory + localStorage, Tauri uses the filesystem, s92-studio uses its own backend.
- **WHY:** brink-studio needs to support multiple host environments (web, Tauri, s92-studio) with a single unified API. Making file operations async covers all cases (disk, network, IndexedDB). Keeping compilation synchronous against pre-loaded files avoids async complexity in the wasm bridge — the host is responsible for having files loaded before compile is called. The driver discovers INCLUDE dependencies during parsing and requests them from the host, so the host doesn't need to know the full file graph upfront.

## View context as native wasm concept
- **WHEN:** 2026-03-19
- **PROJECT:** brink / s92-studio
- **SYSTEM:** editor-session / wasm API
- **SCOPE:** architectural
- **WHAT:** The wasm `EditorSession` gains a `set_view_context(path, start, end)` method. When a range is set: `update_source()` splices the fragment into the full file at `[start, end)`; IDE responses (line_contexts, semantic_tokens, completions, hover, etc.) filter to the range and rebase byte offsets. The TypeScript state manager stops owning splice logic — it just calls `set_view_context` before creating an EditorState. This makes "scoped editing of a sub-region" a first-class concept reusable across s92-studio, not a UI-layer hack.
- **WHY:** The current approach duplicates splice logic in TypeScript, corrupts the session when `updateSource()` is called with a fragment (causes wasm `unreachable` panic via `line_contexts`), and won't generalize to s92-studio's need to present a single knot for editing without exposing the full file.

## Screenplay conventions (character/dialogue/parenthetical)
- **WHEN:** 2026-03-21
- **PROJECT:** brink / s92-studio
- **SYSTEM:** brink-studio / brink-ide
- **SCOPE:** architectural
- **WHAT:** Screenplay formatting is implemented as editor conventions over valid ink syntax, not as language extensions. The ink source uses standard syntax with specific patterns:
  - **Character line**: `@Name:<>` — `@` prefix marks character, `:<>` is colon + glue. The runtime sees `@Name:` (a recognizable pattern for downstream game engines). The editor hides `@`, `:`, `<>` via widgets, shows just `NAME` centered/bold.
  - **Parenthetical**: `(text)<>` — parentheses visible, `<>` glue hidden. Text is styled italic/dimmed.
  - **Dialogue**: plain narrative text on the line following character/parenthetical. Glue (`<>`) makes the character line and dialogue line appear as one output line to the runtime.
  - State machine: double-blank + Tab → insert `@:<>` template. Backspace on empty character line clears entire `@:<>` structure. Shift+Tab strips sigils back to plain text. Enter mid-name splits correctly. Cursor cannot enter the `@`/`:`/`<>` regions directly.
  - Character name autocomplete via pattern-matching across project (generic capability in brink-ide, reusable for tags etc).
- **WHY:** This keeps ink syntax valid and parseable by the standard pipeline. The `@Name:` pattern is meaningful to downstream game engines (consumers of the runtime's `continue_line()` output) without requiring brink-syntax changes. The `<>` glue is standard ink. The editor conventions make it feel like native screenplay formatting while the source remains portable ink.

## Screenplay keybinding transitions
- **WHEN:** 2026-03-21
- **PROJECT:** brink / s92-studio
- **SYSTEM:** brink-studio
- **SCOPE:** moderate
- **WHAT:** Tab and Enter on screenplay elements follow Scrivener-like transitions adapted for plaintext ink:
  - **Character** → Tab: parenthetical, Enter: dialogue
  - **Parenthetical** → Tab: dialogue, Enter: dialogue (empty line converts to dialogue; non-empty inserts new dialogue line below)
  - **Dialogue (empty)** → Tab: parenthetical, Enter: element picker dropdown
  - **Dialogue (text)** → Tab: parenthetical, Enter: action/narrative (Shift+Enter: new line within dialogue)
  - **Blank line** → Enter on blank: element picker dropdown to select format
  - **Shift+Tab** on any screenplay element: strip sigils, revert to plain narrative
  - The element picker is an inline dropdown (like the existing element type dropdown in the status bar) that converts the empty line to the chosen format.
- **WHY:** Matches Scrivener's muscle memory for screenwriters. The state machine makes screenplay authoring feel native without requiring mode switches. Element picker on blank Enter avoids the need for a toolbar — the keyboard drives everything.

## Binder multi-select: same-kind siblings only
- **WHEN:** 2026-03-22
- **PROJECT:** brink
- **SYSTEM:** brink-studio
- **SCOPE:** moderate
- **WHAT:** Multi-select in the binder is constrained to same-kind siblings — multiple stitches within one knot, or multiple knots within one file. Cannot mix knots and stitches in a single selection.
- **WHY:** Avoids ambiguous drop-target semantics. If you could select a knot and a stitch from a different knot, the meaning of "drop here" becomes unclear. Same-kind siblings map cleanly to a single batch operation (reorder, move, promote/demote).

## Binder structural moves use store-level undo
- **WHEN:** 2026-03-22
- **PROJECT:** brink
- **SYSTEM:** brink-studio
- **SCOPE:** architectural
- **WHAT:** Structural move operations (reorder, move, promote, demote) in the binder use a dedicated undo stack in the Zustand store, separate from the CodeMirror editor undo stack.
- **WHY:** Structural moves can produce cross-file edits that affect files not currently open in a tab. The CodeMirror undo stack only works for the active editor buffer. A store-level undo system can track the full set of changes (primary file source replacement + cross-file edits) and reverse them regardless of which tab is open.

## Binder multi-select via Ctrl/Cmd + click
- **WHEN:** 2026-03-22
- **PROJECT:** brink
- **SYSTEM:** brink-studio
- **SCOPE:** minor/local
- **WHAT:** Multi-select binder rows using Ctrl+click (Cmd+click on macOS) to toggle individual selection without navigating. Plain click continues to open a tab as before.
- **WHY:** Standard multi-select convention across all platforms. Keeps the existing single-click-to-navigate behavior untouched. Shift+click range selection is not needed for V1 since selections are constrained to siblings (typically small lists).

## Fragment model for locale-safe slot values
- **WHEN:** 2026-03-23
- **PROJECT:** brink
- **SYSTEM:** brink-runtime, brink-format, brink-codegen-inkb
- **SCOPE:** architectural
- **WHAT:** Introduce a "fragment" model for preserving structural references across value boundaries. `OutputBuffer` gains a `fragments: Vec<Vec<OutputPart>>` store. `Value` gains a `FragmentRef(u32)` variant (index into fragments). New `BeginFragment`/`EndFragment` opcodes capture output structurally without resolving to a string. Codegen emits fragments around display-context function calls (inline expressions in output content, choice display text). Resolving a `FragmentRef` re-resolves the fragment's structural parts against the current line tables, enabling locale hot-swap without re-execution.
- **WHY:** The runtime restructuring's append-only transcript enables locale re-rendering for main output (LineRef parts resolve at read time). But slot values in templates cross a value boundary — `end_capture` resolves LineRef → String, losing structural data. Function calls that produce localized text (e.g., `{greeting(name)}`) have their output baked into slot values as resolved strings. Fragments solve this by storing the structural parts separately and letting slots reference them by index. On locale switch, fragments re-resolve against new line tables. This avoids daisy-chaining Value types (which would create recursive resolution) and keeps the transcript clean (fragments aren't narration — they're intermediate computed text).

## Replace converter pipeline with C# ink oracle as correctness source of truth
- **WHEN:** 2026-03-23
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** architectural
- **WHAT:** Remove converter-generated golden episodes (`episodes/*.episode.json`) and converter episode tests. The oracle (`oracle/*.oracle.json`) from the C# ink runtime becomes the sole golden reference for behavioral correctness. A separate converter-vs-oracle test can be added later if needed.
- **WHY:** Confidence in the brink compiler now exceeds the converter pipeline. The converter was originally built as a known-good reference (ink.json → StoryData), but it has its own bugs (e.g., trailing whitespace in choice text). The C# ink runtime is the canonical source of truth for ink semantics — testing directly against it via the oracle harness eliminates the converter as a middleman and catches bugs in both the compiler and the converter.

## HIR lowering: split context into read-only scope + write-only sink
- **WHEN:** 2026-03-28
- **PROJECT:** brink
- **SYSTEM:** brink-ir / hir lowering
- **SCOPE:** moderate
- **WHAT:** When restructuring HIR lowering, the lowering context should be split into a read-only `Scope` (file_id, current_knot, current_stitch) and a write-only `LowerSink` trait (diagnostics, symbol declarations, unresolved refs). Node-level lowering code receives `&Scope` + `&mut impl LowerSink`. Only the backbone/orchestration code mutates scope.
- **WHY:** Currently `LowerCtx` conflates read and write access — node impls take `&mut LowerCtx` which lets them accidentally mutate scope (current_knot/current_stitch). Only the structural backbone should manage scope transitions. Splitting enforces this at the type level and also enables testability: tests can swap in a recording sink without constructing a full lowering pipeline.

## Eliminate LIR planning pass via upstream structural IDs
- **WHEN:** 2026-03-29
- **PROJECT:** brink
- **SYSTEM:** brink-ir / LIR lowering
- **SCOPE:** architectural
- **WHAT:** Eliminate the LIR planning pass by stamping synthetic container `DefinitionId`s on HIR nodes in a lightweight post-HIR-lowering pass. The LIR lowerer reads pre-assigned IDs directly from HIR nodes instead of re-walking the tree with synchronized counters. This also enables a context split (immutable env / mutable allocators / scoped block state) and trait-based architecture for LIR lowering.
- **WHY:** The planner/lowerer counter-synchronization coupling has been the biggest source of compiler heartburn. Both passes must walk the HIR in exactly the same order with identical counter logic — if they diverge, container IDs silently mismatch and diverts point to wrong targets. Pushing structural identity upstream means LIR lowering becomes a simple tree walk with no planning pass, no counter coordination, and no scope-path threading for ID derivation.

## bevy-brink loading modes
- **WHEN:** 2026-04-24
- **PROJECT:** brink
- **SYSTEM:** bevy-brink (new crate)
- **SCOPE:** architectural
- **WHAT:** The Bevy asset integration has two modes. Dev mode loads `.ink` source files; the loader tracks the transitive INCLUDE graph and hot-reloads the compiled program when any file in that graph changes (typical projects go ~3 imports deep). Release mode loads precompiled `.inkb` (bytecode) plus `.inkl` (localized line tables) — no compiler in the shipped binary.
- **WHY:** Dev ergonomics require tight iteration on ink source with live reload. Release requires fast startup, smaller binaries without the compiler, and swappable localization via the runtime's existing program / line-table split.

## Expose runtime primitives for direct orchestration
- **WHEN:** 2026-04-24
- **PROJECT:** brink
- **SYSTEM:** brink-runtime
- **SCOPE:** moderate
- **WHAT:** Make the runtime primitives publicly available so `bevy-brink` (and other non-`Story` consumers) can drive execution directly: `Context`, `FlowInstance`, `Flow`, and the supporting types needed to construct/snapshot them (`CallFrame`, `CallStack`, `Thread`, `PendingChoice`, etc.) become `pub`. Extract the `continue_single` / step-loop logic from `Story` methods into `pub` free functions that take `(&Program, &LineTables, &mut FlowInstance, &mut Context, ...)` as arguments, so callers can drive the VM without a `Story<'p>` wrapper. `Story` itself remains in place unchanged and continues to serve the CLI and existing test infrastructure — it may eventually become a thin wrapper over the free functions, but that is not required now.
- **WHY:** `bevy-brink` needs to own state placement (globals in a `Resource`, flows in `Component`s, line tables swappable) and cannot do that while `Story<'p>` owns all of it behind a borrowed-program lifetime. But renaming structs or removing `Story` is a refactor beyond what the integration requires, violates the "don't refactor beyond the task" principle in CLAUDE.md, and risks destabilizing the 5574-episode ratchet. Exposing what's already there and extracting the step loop is the minimum change that unblocks bevy-brink while leaving everything else intact.

## Marker-parameterized bevy types
- **WHEN:** 2026-04-24
- **PROJECT:** brink
- **SYSTEM:** bevy-brink
- **SCOPE:** architectural
- **WHAT:** `BrinkPlugin`, `BrinkGlobals`, `BrinkFlow`, `BrinkLineTables`, and related bevy-side types are generic over a ZST marker type `M: Send + Sync + 'static = ()`. Default `()` is the single-story easy path with no boilerplate. Consumers wanting multiple concurrent story instances declare their own marker types (e.g., `struct MainStory; struct DreamSequence;`) and instantiate `BrinkPlugin::<MainStory>::default()`, `BrinkPlugin::<DreamSequence>::default()`. Each marker monomorphizes to distinct Bevy types, so queries and resources are naturally scoped.
- **WHY:** Compile-time dispatch avoids runtime overhead and keeps the type system honest about story boundaries. More ergonomic than forcing consumers to newtype manually, and is the standard bevy idiom (bevy_ecs_tilemap, leafwing-input-manager). The default `()` keeps the easy path frictionless.

## Default bevy wiring: Resources for shared state, Components for flows
- **WHEN:** 2026-04-24
- **PROJECT:** brink
- **SYSTEM:** bevy-brink
- **SCOPE:** moderate
- **WHAT:** The default bevy wiring places `Context` (story-wide globals, visit/turn counts, RNG state) in a `Resource` (`BrinkGlobals<M>`), `FlowInstance` on a `Component` (`BrinkFlow<M>`), and `LineTables` in a `Resource` (`BrinkLineTables<M>`) — one shared context and one active locale per story marker. Two opt-in escape hatches are explicitly supported: (a) per-flow locale overrides or asset-per-locale via storing `LineTables` on a Component or behind a `Handle` instead of the shared Resource, and (b) fork/branch/rollback semantics via storing a `Context` clone on a Component (so each flow runs against isolated state) instead of using the shared `Resource`. Because the runtime's step functions take `&mut Context` regardless of where it lives, no runtime primitives are added for this — `Context: Clone` is sufficient.
- **WHY:** Shared globals + single-active-locale is the common case and the simplest wiring, matching inklecate's semantics where flow writes are immediately visible to other flows. Exposing `Context` and `LineTables` as plain structs lets consumers switch to Asset-, Component-, or forked-storage when their game needs it (polyglot NPCs, per-scene locale overrides, speculative dialogue previews, rollback mechanics) without us prescribing merge semantics. We deliberately do not ship merge helpers — reconciliation policy (last-write-wins vs additive visit counts vs takes-max vs two-phase commit) is game-specific and any built-in would be wrong for most consumers.

## brink-runtime stays bevy-free
- **WHEN:** 2026-04-24
- **PROJECT:** brink
- **SYSTEM:** brink-runtime / bevy-brink
- **SCOPE:** architectural
- **WHAT:** Every public type in `brink-runtime` is a plain `Send + Sync + 'static` struct with no bevy imports. `bevy-brink` provides all `Resource`/`Component`/`Asset` wrappers and the plugin. Downstream consumers can build any indexing or sharing scheme they need (registry-keyed HashMaps, dynamic lookup, custom storage) using the runtime types directly, bypassing `bevy-brink` entirely if they wish.
- **WHY:** Keeps `brink-runtime` usable in non-bevy contexts (CLI, tests, other frameworks), preserves flexibility to support runtime-dynamic story instances, and makes bevy integration a pure layer on top rather than a coupled rewrite.

## ink↔engine binding boundary: split by World access, not sync/async return shape
- **WHEN:** 2026-05-29
- **PROJECT:** brink
- **SYSTEM:** cross-system (bevy-brink / brink-runtime)
- **SCOPE:** architectural
- **WHAT:** The external-function binding boundary is split by World access, not by sync/async return shape. (1) Externals needing no World access — pure compute + fire-and-forget event publishing — resolve synchronously, inline in the flow-driver. (2) Externals needing World access resolve asynchronously: flow enters Pending, a run-condition-gated exclusive resolver runs the binding as a real Bevy system (`run_system_with`), supplies the return value, flow resumes a later frame (schedule-based, NOT Tokio). (3) Engine→ink calls from an exclusive system are synchronous; from a non-exclusive system they use a real async API whose result handler is an observer scoped to a unique per-call entity — NOT a name/ID-keyed request/return event pair.
- **WHY:** World access is the real constraint — Bevy's borrow model can't give arbitrary typed `SystemParam` access to a binding called inline from inside another system, so World-touching work must defer. Pure/event bindings have no such constraint and shouldn't pay the 1-frame latency. The per-call-entity observer makes call/response mis-correlation structurally impossible (each call = its own entity, result event targets only it, events are crate-private so user code can't mis-fire) — eliminating the correlation-ID bookkeeping an event-pair would force on the user.

## Engine↔ink dynamic binding facility: resumable eval + exclusive driver
- **WHEN:** 2026-05-31
- **PROJECT:** brink
- **SYSTEM:** cross-system (brink-runtime / bevy-brink)
- **SCOPE:** architectural
- **WHAT:** Engine→ink calls and world-access bindings are a fully dynamic facility, not upfront-bound. The runtime exposes resumable function evaluation (`begin_function_eval` / `resume_function_eval` → `FunctionEval::{Returned(Value), AwaitingExternal}`) with a `FunctionEvalFromGame` boundary frame and isolated output. A binding that needs the Bevy World yields `AwaitingExternal`; the eval releases its borrows; an exclusive driver resolves it by synchronously running an arbitrary registered Bevy system via `run_system_with`, then resumes — looping begin→(resolve)→resume to completion within one exclusive pass (one frame). `call_ink_function(world, …)` from an exclusive system returns synchronously; non-exclusive callers defer the whole call to the resolver (per-call-entity observer, `commands.brink_call(...).on_resolved(...)`). `bind_brink_query(name, system)` registers any Bevy system. Designers can query anything (including data that doesn't exist yet) with no upfront declaration or pre-fetch.
- **WHY:** Ink functions must hold arbitrary designer logic (e.g. level-editor-set enemy spawn conditions, or per-frame per-entity world checks) without the engine pre-declaring what each function reads. Upfront/pre-fetched binding is fast but static — it can't express "query anything, including things that don't exist today." Suspend/resume (not inline resolution) is required because the live eval holds `&mut flow`/`&mut ctx`, which conflicts with the `&mut World` that `run_system_with` needs; releasing the borrows at the `AwaitingExternal` point is the only borrow-safe way to run a registered system mid-evaluation. Synchronous one-shot driving keeps the hot path (per-frame evals) immediate.

## bevy-brink locale switching: global + event-driven
- **WHEN:** 2026-05-31
- **PROJECT:** brink
- **SYSTEM:** bevy-brink
- **SCOPE:** architectural
- **WHAT:** Locale switching is global + event-driven. A `BrinkCurrentLocale<M>` resource holds the active locale (`Option<Handle<LocaleAsset>>`, `None` = base). A `set_brink_locale` `Commands` API sets the resource AND fires a `BrinkLocaleChanged<M>` event; an observer reconciles every flow's `BrinkLocale` to the current locale. New flows read the resource at spawn; an `AssetEvent<LocaleAsset>` catch-up reader handles `.inkl`s that load after a switch — all paths share one reconcile helper, so there is no per-frame polling. Per-flow exceptions use a `BrinkLocaleOverride<M>` opt-out marker (that flow's `BrinkLocale` is set manually via the `apply_locale_overlay` helper). A `BrinkBaseLocale<M>` component retains the canonical base line tables so overlays always apply to base, not an already-localized table; localized tables are cached/shared per `(base, locale)`. This supersedes a per-flow request-component approach.
- **WHY:** Games switch language globally (one setting), so a single resource is the natural source of truth and per-flow `BrinkLocale` becomes the applied result. Event-driven (vs a per-frame reconcile) keeps it explicit and cheap — work happens only on switch/spawn/asset-load. Spawn-read + asset-load catch-up cover the async cases (new flows, late-loaded `.inkl`) without polling. The opt-out marker preserves the per-flow architecture for polyglot NPCs.

## bevy-brink async bindings: event primitive + task sugar, flow-entity correlation
- **WHEN:** 2026-06-02
- **PROJECT:** brink
- **SYSTEM:** bevy-brink
- **SCOPE:** architectural
- **WHAT:** Async (defer-across-frames) ink→engine bindings ship as TWO verbs. (1) `bind_brink_async` — an event+resolve primitive: the flow parks and `BrinkExternalAwaited<M>` (an EntityEvent carrying name+args) fires once at the flow entity; the engine resolves whenever ready via `commands.resolve_brink_external::<M>(flow, value)`. (2) `bind_brink_task` — sugar owning an `AsyncComputeTaskPool` task lifecycle: bevy-brink spawns the `Send+'static` future (computed from ink args only, no World access), parks `BrinkPendingTask<M>`, and `poll_brink_tasks` resolves on completion. Correlation uses the FLOW ENTITY as the key — a flow parks on exactly one external and is frozen until resolved — so no per-call entity or correlation id, in deliberate contrast to `brink_call` (which spawns a per-call entity because many engine→ink calls can be in flight). Async bindings are a `step_one`-playback-path feature; the one-pass exclusive drivers (`advance_flow`, `call_ink_function`) reject them with `AsyncExternalUnsupported`.
- **WHY:** The existing world-access query binding resolves in one pass (`run_system_with`); externals needing multiple frames (a targeting UI awaiting a click; off-thread compute/IO) couldn't be expressed. The two flavors map to two real constraints: World-dependent multi-frame work needs the engine to drive it over frames (event primitive), while off-thread compute is `Send+'static` and can't touch the World (task sugar). The flow entity is the unambiguous correlation key precisely because a parked flow holds exactly one pending external and is frozen until resolved — nothing to mis-correlate, so a per-call entity/id would be redundant ceremony. Reusing the runtime's `AwaitingExternal`/`resolve_external` pause/resume keeps this a pure bevy-layer addition (no runtime/oracle risk).

## Book playground WASM built in CI, not committed
- **WHEN:** 2026-06-02
- **PROJECT:** brink
- **SYSTEM:** docs-book / brink-web / ci
- **SCOPE:** moderate
- **WHAT:** The mdbook playground page embeds brink-web's WASM. The bundle is generated at book-build time (wasm-pack + copy `www` → `docs/book/src/playground/`, gitignored) rather than committing the wasm to git. The book CI/deploy (`book.yml`) gains a wasm-pack build step before `mdbook build`; a `just book-assets` recipe does the same locally.
- **WHY:** Keeps generated wasm binaries out of git and guarantees the deployed playground reflects the compiler at deploy time, avoiding stale committed artifacts. Cost accepted: the book deploy now compiles brink-web to wasm.

## Brink backlog tracked in GitHub Project #6
- **WHEN:** 2026-06-02
- **PROJECT:** brink
- **SYSTEM:** process / project-management
- **SCOPE:** moderate
- **WHAT:** Brink's backlog lives in the user-level GitHub Project "Brink" (#6, owner Syynth — https://github.com/users/Syynth/projects/6), mirroring the Folklore workflow. Board columns: Todo / In Progress / Done / Icebox. Labels: area (`studio`, `editor`, `wasm`, `runtime`, `compiler`, `bevy-brink`, `intl`), severity (`severity:critical|high|medium|low`), and `needs-design`. Issues are filed one-per-finding (granular, independently closeable) and added to the board via `gh project item-add 6 --owner Syynth --url <url>`. `needs-design` and Icebox items are skipped when picking up autonomous work.
- **WHY:** A single shared source-of-truth backlog for user and agents, consistent with the Folklore setup the user already runs. Granular issues ease triage/bisecting; severity labels surface highest-impact work; `needs-design` gates items that require product/design direction before implementation.

## Book structure: separate the core toolchain from integrations/clients
- **WHEN:** 2026-06-03
- **PROJECT:** brink
- **SYSTEM:** docs/book
- **SCOPE:** moderate
- **WHAT:** Restructure the mdBook so the core compiler toolchain (compiler, runtime library, CLI, binary format, localization tooling) is documented as one domain, and integrations/clients (bevy-brink, the web/WASM surface via brink-web, brink-studio, and a future React client) as a separate top-level domain. Bevy stops being a peer of the toolchain docs and becomes one client among several. brink-web is documented as the wasm foundation that web/React clients and the studio build on; brink-studio gets its own chapter as the reference authoring app. Dividing line: "produces or runs StoryData" = toolchain; "embeds brink in a product" = integration.
- **WHY:** brink is the toolchain; Bevy is just the first of several front-ends. Treating it as a peer of the compiler docs misrepresents the architecture and won't scale as web/React/studio clients land. Separating the layers keeps the toolchain docs engine-neutral and gives each client a clear home.

## Book playground: embed the full brink-studio (standalone static build), replacing the simple brink-web playground
- **WHEN:** 2026-06-04
- **PROJECT:** brink
- **SYSTEM:** docs/book + brink-studio build
- **SCOPE:** moderate
- **WHAT:** Replace the simple brink-web www playground embedded in the book's Playground page with the full brink-studio app, built as a standalone static bundle via a new app-mode Vite config (no lib build, bundled deps, base: "./"; wasm auto-emitted by Vite, no extra plugin). Stage it through the `just book-assets` pipeline and ship it to GitHub Pages (book.yml gains Node/pnpm setup + a studio build step). The Book CI check (ci.yml) is unaffected — it only runs `mdbook build`.
- **WHY:** The studio is the far richer reference client and the better in-browser showcase of brink; one best-in-class playground beats maintaining a second minimal one. Shipping it live lets people try the real authoring experience with no install. The cost (~670 kB gzipped page, added CI build step) is acceptable for a docs playground.

## Publish the full brink Rust toolchain to crates.io as dependency closures (release-plz + cargo-dist)
- **WHEN:** 2026-06-04
- **PROJECT:** brink
- **SYSTEM:** release-infra / cross-system
- **SCOPE:** architectural
- **WHAT:** Publish the full brink Rust toolchain to crates.io by publishing dependency closures — the 5 front doors (`brink-runtime`, `bevy-brink`, `brink-compiler`, `brink-cli`, `brink-lsp`) plus all internal crates in their closures (~20 crates total). Internal crates become public with an explicit "no semver guarantees" note. `brink-test-harness`, `brink-web`, `zed-brink`, and the empty `brink` umbrella stay unpublished (`publish = false`). Automated via release-plz (unified versioning, conventional-commit changelogs) for crates.io and cargo-dist for prebuilt `brink-cli`/`brink-lsp` binaries on GitHub Releases. Internal path deps move to `[workspace.dependencies]` with versions (also aligning with the existing `dep.workspace = true` convention). JS/web → npm/JSR is a separate follow-up.
- **WHY:** A single coherent publish boundary is impossible without publishing closures — crates.io flattens deps and every front door depends transitively into `crates/internal/`, so the FS public/private split is organizational only, not a valid registry boundary. Publishing internals (as swc/salsa/rustc do) avoids a large structural refactor and respects the workspace's separation-of-concerns design principle. release-plz + cargo-dist gives hands-off, dependency-ordered releases with binary artifacts.

## Harden the release/CI supply chain (OIDC trusted publishing, SHA-pinned actions, cargo-deny)
- **WHEN:** 2026-06-04
- **PROJECT:** brink
- **SYSTEM:** release-infra / ci
- **SCOPE:** moderate
- **WHAT:** Apply maximal-security hardening to CI + release, mirroring the bevy_rmmz_assets model: crates.io Trusted Publishing (OIDC, `id-token: write`, no stored registry token in steady state); all third-party GitHub Actions pinned to commit SHAs with version comments; least-privilege `permissions: {}` + per-job grants + `persist-credentials: false`; a `crates-io` environment gate on the publish job; pinned Rust toolchain via `rust-toolchain.toml` (1.95.0, minimal profile); Dependabot covering github-actions + cargo + npm; and a `cargo-deny` CI job (`deny.toml`: advisories, permissive-only license allowlist, wildcard/source bans). The one-time first publish of the ~20 new crates uses `CARGO_REGISTRY_TOKEN` in CI (Trusted Publishing can't create new crates), after which the token is removed and per-crate trusted publishers take over. An in-place dependency audit was run: 750 crates resolved, 100% permissive licenses, 0 vulnerabilities, 3 accepted transitive informational advisories.
- **WHY:** A public crate's release pipeline is a high-value supply-chain target; OIDC removes the standing secret, SHA pins defeat tag-repointing attacks, least privilege limits blast radius, and cargo-deny continuously gates advisories/licenses/sources. Matches the security posture already established for bevy_rmmz_assets so the two projects are consistent.

## State View panel is a read-only runtime debugger
- **WHEN:** 2026-06-04
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-ui (state-view) / cross-system (runtime + wasm)
- **SCOPE:** architectural
- **WHAT:** The new "State View" panel in brink-studio is scoped as a read-only runtime debugger surfacing the transcript, global variables, current location (knot/stitch), call stack, and visit counts. It reads live state during playback and does not mutate it.
- **WHY:** The author needs the runtime's actual behavior to be legible while debugging narrative logic. A bare variables list is insufficient; seeing location + call stack + visit counts + transcript together explains why the story is where it is.

## Editable runtime state (save/restore variables) is a follow-up
- **WHEN:** 2026-06-04
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-ui (state-view) / runtime / wasm
- **SCOPE:** moderate
- **WHAT:** Making runtime state editable (poking variable values mid-playback) is deferred to a separate follow-up issue (#57) rather than shipped with the initial State View. The concrete motivating capability is save + restore of (at least) variable state.
- **WHY:** Editing live state needs a wasm setter and careful UX about when edits take effect; the immediate need is read-only visibility. Save/restore of variable state is the real capability wanted from editability and is worth tracking as its own work item.

## Runtime changes deferred to separate design issues
- **WHEN:** 2026-06-04
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** process / cross-system
- **SCOPE:** moderate
- **WHAT:** Any studio (or other) feature work that requires modifying `brink-runtime` is split out into its own separate needs-design issue rather than implemented inline. Studio-side work proceeds against existing runtime/wasm capabilities or an interim approach until the runtime API is designed. Concretely: #56's State View ships an interim view over the existing `Story::debug_state()` string (via the runtime's pre-existing, purely-additive `testing` feature — no runtime source change); the structured `DebugSnapshot` runtime API becomes its own needs-design issue (#62).
- **WHY:** The runtime is the trusted, oracle-validated core; changes carry correctness risk and deserve deliberate design before implementation. Bundling runtime changes into studio feature PRs couples UI iteration to runtime design and risks under-designed runtime APIs.

## Studio gets two views: State (runtime) + Program Explorer (compiled)
- **WHEN:** 2026-06-05
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-ui / wasm / runtime
- **SCOPE:** architectural
- **WHAT:** The studio gets two distinct, cross-linked activity-bar views: **State** (runtime/dynamic — live globals, current location, call stack, visit counts; issue #62) and **Program Explorer** (static/compiled program tables; new issue), aimed at compiler conformance + deep debugging. A shared wasm layer exposes the compiled program; the State View resolves names (knots/globals) from those tables. The two are designed together and cross-linked (e.g. a knot in the State call stack links to its container in the Program Explorer). The interim `testing`-feature hack from #56 is replaced with proper public APIs.
- **WHY:** Runtime values and compiled structure are two different lenses on the same program; separating them keeps each legible, and sharing the program-table layer makes the debugger's names nice and enables cross-linking.

## Program Explorer reuses brink-format's write_inkt for disasm/tables
- **WHEN:** 2026-06-05
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-ui / wasm / brink-format
- **SCOPE:** moderate
- **WHAT:** The Program Explorer's disassembly + tables come from brink-format's existing `write_inkt` (`StoryData` → `.inkt` text), exposed over wasm — not a new disassembler. v1 renders the `.inkt` text dump (checksum, name table, globals, lists, externals, address paths, containers with opcode mnemonics + counting flags + path_hash). Structured/filterable/cross-linked tables are a follow-up. Shows only the actively compiled program; converter side-by-side diffing is deferred.
- **WHY:** `write_inkt` already produces a full WAT-style mnemonic dump of every table; reusing it gives a complete inspector for almost no backend cost and matches the `.inkt` the corpus tooling already uses.

## External-binding work splits into a runtime/web foundation track and a deferred tooling-manifest track
- **WHEN:** 2026-06-09
- **PROJECT:** brink
- **SYSTEM:** cross-system (runtime / brink-web / brink-studio / analyzer)
- **SCOPE:** architectural
- **WHAT:** External-function binding for brink's consumers (Rust app, folklore web, RPG Maker MZ via NW.js, eventual server module) is structured as two parallel tracks. Track A (foundation): expose the runtime `ExternalFnHandler` to JS as sync bindings, with suspend/await designed-in as an additive Phase-2 step; name-based variable get/set; byte serialization + versioned save format for save/load persistence; deterministic seed control. Track B (tooling): a host-capability manifest — a serializable host-vocabulary schema (signature/base types, semantic/refined types, live value providers + widgets) feeding analyzer, runtime, and studio (see `docs/host-capability-manifest.md`). Track B is deferred and NOT a prerequisite for Track A.
- **WHY:** The manifest is additive over a bind-by-name + `Value` boundary (both already true in the runtime), so it can attach later without a rewrite; its value is author-time tooling, separable from the runtime/web binding plumbing that the web consumers are actually blocked on. Splitting lets the foundation ship first. Also corrected mid-discussion: variable get/set and in-memory state capture/restore already exist in the runtime (index-based `Context` accessors; `into_snapshot`/`from_snapshot`) — only byte-serialization persistence is genuinely new.

## Track B host-capability manifest: tooling-only, two-source (inline JSDoc + registered) enrichment
- **WHEN:** 2026-06-09
- **PROJECT:** brink
- **SYSTEM:** brink-ir / brink-analyzer / brink-ide / brink-web / studio (tooling)
- **SCOPE:** architectural
- **WHAT:** The host-capability manifest is a TOOLING/author-time artifact only — consumed by the analyzer/IDE + brink-studio, never the runtime or compiler codegen. It enriches the analyzer's existing external `SymbolInfo` (ink `EXTERNAL` carries only names/arity) with param types, semantic types, presentation/effect kind, docs, widgets. Two merged sources: (1) inline JSDoc `///` tags on `EXTERNAL` decls (`@param x {type}`, `@returns`, `@kind`, `@widget`), parsed from existing `LINE_COMMENT` trivia in brink-ir HIR lowering — no grammar change; (2) registered manifest via `EditorSession::set_host_manifest` for project-wide semantic-type *definitions*, Tier-3 providers/host-editors, and bulk/generated entries. Inline wins on conflict; author always still declares `EXTERNAL` in ink (additive, self-contained; arity disagreement = diagnostic). `HostManifest` type in brink-ir; merged in `brink_analyzer::analyze`; surfaced by existing brink-ide queries + a new insert-`EXTERNAL` code-action. MVP = Tier 1 + closed Tier 2; Tier 3 (live providers + host-rendered widgets via a `FileProvider`-style callback) is a later phase. See `docs/host-capability-manifest.md`. (Supersedes the earlier two-track entry's claim that the manifest feeds the runtime — it does not.)
- **WHY:** ink `EXTERNAL` can't express types/tags/docs/widgets, and the analyzer already arity-checks externals but has nothing to type-check against — the manifest fills exactly that hole on the existing `ProjectDb`→`analyze`→`SymbolIndex`→IDE pipeline. Tooling-only because every candidate runtime use is actually author-time (validation), host-layer (effect routing lives in binding impls), or runtime-intrinsic (fallback uses `Program` metadata). The two-source split gives self-contained inline annotation for small projects and registered/generated vocab for large/RMMZ ones, with no compiler/runtime coupling.

## Track B MVP implementation decisions (check scope, severity flag, data placement)
- **WHEN:** 2026-06-09
- **PROJECT:** brink
- **SYSTEM:** brink-ir / brink-analyzer / brink-ide / brink-web (tooling)
- **SCOPE:** moderate
- **WHAT:** Settled four implementation decisions for the host-capability-manifest MVP (Tier 1 + closed Tier 2). (1) **Static check scope = literals only:** type-mismatch and closed-domain (enum/regex/range) diagnostics evaluate only literal arguments; CONST/VAR type inference is deferred. (2) **Severity is a flag, default = error:** manifest-driven diagnostics (type mismatch, closed-domain, manifest↔ink arity disagreement, unknown semantic type) have configurable severity `{ Error (default), Warning, Off }`, surfaced as a compiler option and an IDE setter. (3) **Metadata placement = side-table + explicit arg:** merged per-external metadata lives in a new `ExternalMeta` map keyed by `DefinitionId` on `AnalysisResult` (kept off the shared `SymbolInfo`); the registered manifest reaches the analyzer as an explicit `analyze(…)` argument, not via `ProjectDb` inputs. Inline `///` docs ride a parallel `external_docs` map on the per-file `SymbolManifest` (off `DeclaredSymbol`) for the same reason. (4) **Smaller defaults:** one project-wide registered manifest per session; `@kind` is informational-only at MVP (hover, no diagnostic); the `///` parser is lenient (unknown/`@widget` tags ignored, malformed `@param` warns via E038 on that external only); `set_host_manifest` returns a clean error on bad JSON.
- **WHY:** Literal-only checking is sound with zero false positives and needs no inference pass — ink is dynamically typed, so most argument types aren't statically knowable. Error-by-default matches the author expectation that a registered manifest is binding, and is safe (no manifest ⇒ no diagnostics ⇒ existing builds unaffected); the flag leaves room to downgrade. Side-table placement follows "separate concerns by ownership" — the metadata is external-specific and optional, so it doesn't belong on the symbol type used everywhere; and an explicit arg is clearer than smuggling project-wide host data through per-file db inputs.

## Studio shell redesign direction
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-ui / studio-store
- **SCOPE:** architectural
- **WHAT:** Redesign brink-studio as a principled ink IDE. Structural reference is VS Code's region model (named regions: edge docks, editor groups, status bar; views are the movable unit; everything routes through a command registry + palette). Docking affordance follows JetBrains tool-window strips (icon strips on left/right/bottom edges, each strip with two sections; drag icons between strips to re-dock; tool windows live only in edge docks, editor always center). Visual language follows Zed (quiet, low-chrome). Inky remains the domain reference: editor⇄player two-up is the default layout. Free-form docking libraries (dockview/golden-layout) rejected. Process: written spec (docs/studio-shell-spec.md) reviewed before implementation.
- **WHY:** The current UI's problem is absence of principle, not missing features — panels are hardcoded slots with no region contract and no command system. VS Code is the only candidate with published UX guidelines, so its region model does the design thinking for us; JetBrains' strips are the best-discoverable affordance for view docking; Zed's restraint suits a writing-focused tool; free-form docking adds machinery without IDE discipline and lets layouts degrade.

## Studio shell spec: open questions resolved (Program Explorer, menus, keybindings)
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-ui / studio-shell
- **SCOPE:** moderate
- **WHAT:** (1) Program Explorer splits: the structured tables (globals/lists/externals/knot tree) remain a tool window; the raw .inkt dump becomes a read-only "Compiled Output" editor document in Phase 4 with a minimal CM6 mode. (2) No menu bar: command discoverability via a registry-generated hamburger menu (grouped commands), embed-friendly; the same registry could feed a native menu bar in a future desktop shell. (3) Keybindings: the Phase 1 key handler resolves through a keymap table built from registry defaults, with a user-override JSON (no UI) merged over defaults from day one; a full keymap-editing UI stays out of scope.
- **WHY:** (1) Tools-you-glance-at vs content-you-read: the tables are glanceable lookup (tool-window behavior), the dump is a long searchable text artifact that gains CM6 search/folding as an editor tab — the disassembly-view precedent. (2) Menu bars exist for discoverability; an in-page bar costs permanent vertical space and is wrong in embeds, while a registry-driven hamburger patches the palette's discoverability hole nearly free. (3) With the keymap layer specced anyway, the marginal cost of JSON overrides is small enough that deferring buys nothing — cheap now, annoying later; the indirection itself is the part that's expensive to retrofit.

## Studio shell: notification service replaces Toast
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / studio-ui
- **SCOPE:** moderate
- **WHAT:** The one-off Toast is replaced (Phase 3) by a shell notification service: severity-tiered model (info 5s / warning 8s / error sticky), stacked bottom-right (max 3 visible, overflow collapser, hover pauses dismissal), status-bar bell with unread badge and a capped session history (~100). Notification actions dispatch commands only — no raw callbacks (Binder's undo toast becomes a `binder.undo` action). Progress notifications and do-not-disturb are out of scope. Spec §7.5.
- **WHY:** A real IDE needs more than one transient message slot, and missed toasts must be recoverable (hence the bell/history). Command-only actions keep the everything-routes-through-commands invariant and the model serializable; the history cap follows the unbounded-growth guard principle.

## Studio shell: embedder extension API (host-provided panels), not a plugin system
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / cross-system (RMMZ embedding, Track B)
- **SCOPE:** architectural
- **WHAT:** Hosts embedding brink-studio (e.g. RPG Maker MZ — planned "RPG Maker functions panel") can register their own tool windows, commands, and status-bar items at mount time via a `StudioExtensions` config feeding the same registries as built-ins, with mandatory `host.<vendor>.` id namespacing. Host components get a curated `StudioApi` facade (insertText at cursor, dispatch, notify, select/subscribe over an explicit `StudioPublicState`) — never the raw store. Contract shapes are baked into the Phase 1 registries; public exposure lands Phase 5. Explicitly NOT a plugin system (no dynamic loading/marketplace/sandboxing — the host is trusted code that owns the page). Amends the spec's earlier non-goal ("registries are internal APIs"). Spec §8.
- **WHY:** Host-specific panels (RPG Maker functions) cannot be built into the studio, and the host already runs code in the page — so mount-time registration into existing registries gives extension for near-zero machinery. The facade follows the consumer-first API principle (store internals stay changeable); shapes-early/expose-late makes the public contract a thin door instead of a retrofit while letting docking/persistence stabilize before hosts depend on them. Dovetails with Track B: the functions panel renders the registered host-capability manifest with click-to-insert.

## Studio shell: Story Graph document (visual story explorer)
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / brink-analyzer / brink-ide (graph query)
- **SCOPE:** moderate
- **WHAT:** A Story Graph opens as a custom-rendered editor document (not a tool window): one node per knot, expanding to stitches; whole-project scope; edges are diverts (solid), choice targets (aggregated up from the weave), and tunnels/threads (dashed), with END/DONE pseudo-nodes; function-call edges excluded (possible default-off toggle later). Interaction: pan/zoom, click-to-jump to source, and a live story overlay (current-location highlight + visit-count badges while running). Read-only — authoring from the graph would be a separate spec. Requires a new deterministic story-graph query (analyzer/IDE layer, wasm-exposed) — the one cross-crate dependency. Lands as Phase 6, after Phase 4's document support, which is now explicitly component-based (text/CM6 and custom-rendered documents share one document-type API). Spec §4.1.
- **WHY:** Exploring story structure visually is content you open and read — document semantics, not glanceable-tool semantics. Knots-first granularity keeps large stories legible with drill-down; the three selected edge kinds are the story-flow backbone while function calls are usually noise; the live overlay reuses debug state the State View already has, giving the graph debugging value for free.

## Studio shell: story session as a first-class concept; session-bound views
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / studio-store
- **SCOPE:** architectural
- **WHAT:** The live story/VM instance becomes a first-class "story session" model (program identity, runner handle, transcript, debug state, choice history, status: none→running→awaiting-choice→done/ended/error), extracted from PlayerSlice in Phase 2. Player, State View, the Story Graph's live overlay, the future transcript view, and the status bar story segment are "session-bound": they select from the session, render a placeholder with a start affordance when none exists, and never own/create/mutate it — lifecycle belongs to commands (story.start/restart/stop/choose with `when` predicates). Recompile-while-running is formalized: successful compile restarts the session on the new program and replays recorded choices, truncating with a notification on divergence; failed compile leaves the old session running. Single session at MVP; views key to "the active session" so multi-session/flows extend additively. Spec §7.6.
- **WHY:** Several surfaces all tie directly into a running story, and leaving that implicit reproduces today's bundling (PlayerSlice owns the runner, State View piggybacks) — the session and the player's UI state have different lifetimes, so separate-concerns-by-ownership demands the split. Naming the concept gives the command system its `when` predicates, the inventory a session-bound/compile-bound distinction, and formalizes the already-implemented Inky-style choice replay instead of leaving it folklore.

## Studio shell: Location/navigation protocol; Settings document; shared overlay primitive
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / studio-ui
- **SCOPE:** architectural (navigation); minor (settings, overlay)
- **WHAT:** (1) Cross-surface linking is one protocol, not per-view behavior: a `Location` union over the studio's four address spaces (source span | symbol | program address | session ref) with a resolver registry translating toward source; `editor.reveal(location)` is the navigation verb dispatched by Problems rows, graph nodes, quick-open, State View frames; reverse `view.reveal(viewId, item)` ("Reveal in Binder/Graph") is specced now with receivers shipping per view; follow-selection auto-sync explicitly deferred. MVP resolvers: source + symbol; program/session resolvers land with their consumers. Spec §6.1. (2) A Settings editor document (theme, keymap-override JSON, diagnostic severity flags) in Phase 5 — VS Code settings-as-document precedent. (3) A shared anchored-overlay primitive (candidate: floating-ui) under palette/context menus/element dropdown/bell popover, landing Phase 1 with one-offs migrating as touched. Spec §7.7.
- **WHY:** (1) The spec already said "click → jump" in four places ad-hoc; naming the address spaces makes every cross-link the same operation, kills per-view translation duplication, and — because it's command-routed — gives embedder host panels navigation through `dispatch` with zero new API surface. (2) Theme/keymap/severity features already specced had no home. (3) ElementDropdown's manual rect-tracking was one of the original ad-hoc complaints; the palette needs correct overlay behavior on day one anyway, so one primitive serves all transient surfaces.

## Studio shell implementation scoped as 20 issues across 6 phase milestones
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** process / studio-shell
- **SCOPE:** moderate (process)
- **WHAT:** Shell implementation filed as 20 issues (#78–#97), each sized to one worktree (merges with the studio working), grouped by six GitHub milestones ("Shell Phase 1 — Skeleton" … "Shell Phase 6 — Story Graph") rather than an umbrella checklist issue; all added to board #6 in Todo with dependency notes in each body. Pre-existing overlaps cross-referenced: #76 (Problems panel) is superseded by #84 when it lands; #69 (click-to-source from State View / Program Explorer) becomes resolver additions on top of #81's navigation protocol.
- **WHY:** Issue granularity must match the per-issue-worktree, sequential-merge workflow — phase-sized issues are too big to merge green, and ultra-fine slices can't be verified by running the studio. Milestones chosen over an umbrella issue for native per-phase progress tracking.

## Doc-comment blocks are part of their declaration for editor-structural purposes
- **WHEN:** 2026-06-10
- **PROJECT:** brink
- **SYSTEM:** editor-ide
- **SCOPE:** moderate
- **WHAT:** A declaration's contiguous preceding `///` doc-comment block is part of that declaration for editor-structural purposes — folding, structural moves (reorder/promote/demote), and knot-view slices. Tooling must never visually or textually detach a doc block from its declaration. For folding specifically (amended 2026-06-10 after seeing both rendered): a documented knot/stitch folds as a *single region* spanning docs + header + body, with the collapsed line rendering the hidden header (e.g. `=== function damage() === ⋯`); undocumented declarations fold from their header as before, and standalone doc blocks (VAR/CONST/EXTERNAL) still fold on their own. The knot divider rule also sits at the ownership start (above the docs).
- **WHY:** Doc comments document the declaration they precede; leaving them dangling above a folded knot (or detaching them during knot reordering) breaks that attachment and silently corrupts authored documentation.

## Player may become an editor document rather than a tool window
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell
- **SCOPE:** moderate
- **STATUS:** tentative
- **WHAT:** The Player is expected to eventually move from a right-dock tool window to an editor-area document (a session tab, openable in a split for the Inky two-up), revisited when Phase 4's component-based document support lands (#90). Until then it stays a tool window. Amends the spec's §4 placement.
- **WHY:** A play session reads as content-you-open, not a glanceable tool — the same rule that split the Program Explorer. VS Code's markdown preview (live companion to the edited file, lives in an editor split) is the direct precedent. Decisive for later: multi-session/multi-flow (§7.6) maps naturally to multiple player tabs and awkwardly to one tool window with a session selector.

## Player-as-document confirmed (was tentative)
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell
- **SCOPE:** moderate
- **WHAT:** The 2026-06-10 tentative entry "Player may become an editor document rather than a tool window" is upgraded to a standing decision — the user confirmed ("I'm sure I DO want player as document") after using the Phase 1–3 shell. Implementation filed as #120, sequenced after #90 (document API + editor groups, with #91 as the simpler first document consumer). Spec §4 updated.
- **WHY:** Unchanged from the tentative entry: play sessions are content-you-open; markdown-preview precedent; multi-session maps to tabs. Confirmation came from hands-on use of the dock-based interim rather than at the originally planned Phase-4 revisit point.

## Multi-document support fixed at the wasm session layer (document handles)
- **WHEN:** 2026-06-10
- **PROJECT:** brink
- **SYSTEM:** editor-ui / wasm IDE layer
- **SCOPE:** architectural
- **WHAT:** Before editor groups (#90), replace the wasm EditorSession's single-active-file + singleton view-context API with explicit document handles (open_document / open_fragment / update_document / queries by DocId) — filed as #122, sequenced first in Phase 4. Shell work builds on the handle API; #90/#91/#120 consume it.
- **WHY:** The active-file session was a singleton shortcut; building editor groups on it would propagate focus-gated IDE features, a module-global session ref, and flush choreography into every document consumer. The underlying brink-ide layer is already file-addressed, so the assumption is wasm-boundary veneer — fixing it there is the correct fix at the right layer, and it makes fragment⇄file live mirroring fall out of the API's change specs instead of needing CM6 workarounds.

## Editor split duplicates the active editor (VS Code exact)
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell
- **SCOPE:** moderate
- **WHAT:** Mod-\ split duplicates the active editor into the new group with live same-document mirroring (CM6 sync-dispatch), matching VS Code exactly — rather than an empty-group placeholder or moving the tab. Plain open/reveal focuses an existing tab wherever it lives; duplicates arise only from explicit actions (split, future "open to the side"). The fragment⇄file overlap (symbol tab + full-file tab of one file) live-mirrors via the #122 change specs where possible, refresh-on-focus otherwise. In scope for #90.
- **WHY:** VS Code is the structural reference (standing decision) and its split semantics are the familiar ones; the cost shrank once #122's handle API made update change-specs explicit — live mirroring consumes them directly instead of needing CM6 workarounds.

## Session-bound views stay provider-agnostic (live-inspector direction)
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio / bevy-brink)
- **SYSTEM:** cross-system
- **SCOPE:** architectural
- **WHAT:** The story session (§7.6) will eventually be backed by a SessionProvider interface so that a VM running inside a game context (RPG Maker MZ via the §8 mount API or postMessage; Bevy via a dev-only websocket debug plugin) can power the studio's session-bound views as a live inspector — design tracked as #127 ("phase 7/8"). Binding now: session-bound views (player document #120, State View, graph overlay #97, status segment) must consume session data only and never reach through to the local wasm runner handle; capabilities gate the story.* commands' when predicates (observe-only providers hide drive commands); source mapping requires a program-identity (StoryData hash) match, with transcript-plus-variables as a first-class degraded mode.
- **WHY:** The session abstraction was built so views select from "the active session" without owning it — holding the provider-agnostic line now costs nothing, while letting runner details leak into views would make the inspector a rework instead of a new backend. Bevy's per-flow model maps directly onto the reserved multi-session contract, and the RMMZ embedding already plans the mount-time surface the transport needs.

## Editor-group maximize replaces player fullscreen
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell
- **SCOPE:** moderate
- **WHAT:** When the player becomes an editor document (#120), today's tool-window maximize stops applying to it; the replacement is a generic editor.maximizeGroup command — the focused editor group temporarily takes the whole editor area (other groups and open docks collapse), Escape or the command restores, same pattern as tool-window maximize (§5.4). It is a shell feature available to any document (the Story Graph is an expected second consumer), implemented in #120. The player-specific fullscreen concept is retired.
- **WHY:** §5.4's principle — maximize is a shell feature, not a player feature — carries over to the document world; a player-only solo mode would violate it, and dropping fullscreen entirely loses a mode the user actually uses. Group maximize generalizes the existing, proven interaction instead of inventing a new one.

## Story Graph renders with react-flow
- **WHEN:** 2026-06-10
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell
- **SCOPE:** moderate
- **WHAT:** The Story Graph document (#97) uses react-flow (@xyflow/react) as its graph rendering layer, confirming the spec §4.1 candidate. The auto-layout engine stays an implementation detail (layered — dagre or ELK — run off the render path). Stated by the user verbatim: "i want to use reactflow for the graph, use your judgement and be tasteful."
- **WHY:** User-stated preference; react-flow is the mature React-native graph layer with custom-node rendering that fits the token CSS, and §4.1 already named it the candidate.

## Host functions panel inserts calls, not declarations; surfaces manifest metadata
- **WHEN:** 2026-06-11
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / embedder API
- **SCOPE:** moderate
- **WHAT:** The host functions panel (§8.2 motivating example, and the eventual real RMMZ panel) browses the external functions already known via the host-capability manifest and inserts only call sites (`~ fn(args)`) at the cursor — never `EXTERNAL` declarations. The panel surfaces the metadata the manifest already carries (signatures, doc comments, semantic types). The §8.2 example text ("click-to-insert of EXTERNAL declarations and call snippets") is amended accordingly; the playground example gains a pretend manifest so the demo models the real data flow (and makes manifest-driven diagnostics live in the playground).
- **WHY:** The manifest is the catalog — the panel's job is browsing what the host already provides, not declaring it; inserting EXTERNAL lines duplicates declarations that exist (or belong in a dedicated declarations file), and the metadata is already registered host-side, so rendering it costs nothing and is the panel's whole value.

## npm publishing: @brink-lang scope, two packages (web + studio)
- **WHEN:** 2026-06-11
- **PROJECT:** brink
- **SYSTEM:** cross-system (packaging/release)
- **SCOPE:** architectural
- **WHAT:** The web packages publish to npm under the user's @brink-lang org as exactly two packages: **@brink-lang/web** (the wasm + TS wrapper — compile, EditorSession, StoryRunner — with wasm-types folded into its own declarations; already consumed by another of the user's projects) and **@brink-lang/studio** (the IDE via mountStudio; depends on @brink-lang/web as a regular versioned dependency, bundles the internal packages — studio-shell/studio-ui/studio-store/ink-editor — which stay private; react/react-dom as peers). Versioning via changesets; CI publishes from GitHub Actions (npm trusted publishing once the names exist; first publish manual). A lean runtime-only package is deferred until wasm size matters; @brink-lang/react is hypothetical.
- **WHY:** mountStudio + StudioApi is the studio's deliberate public surface, and the wrapper earned separate publication by having a real external consumer (the user's other project compiles ink through it). Publishing the wrapper as a contract beats bundling it twice; -lang scoping travels across npm/GitHub/crates.io and avoids the inkjs confusion that brinkjs invited.

## Embedder file egress: mount callback + facade pull, dirty summary in state
- **WHEN:** 2026-06-11
- **PROJECT:** brink (brink-studio)
- **SYSTEM:** studio-shell / embedder API
- **SCOPE:** architectural
- **WHAT:** Studio→host file persistence (#154, unblocking the RMMZ host) ships as: a debounced `onFilesChanged(changes)` mount option whose payload names files with a change kind (modified | created | deleted) and content; an `api.getFiles()` pull on the facade; `file.save`/`file.saveAll` commands on Mod-S flushing to the host hook; dirty state as a cheap summary on StudioPublicState with detail behind the facade. File *contents* never enter StudioPublicState. All mutation paths (CM6 edits, binder structural ops, search replace, file.new) route through one shared notify helper — closing #137 in the same change. Also: `wasmLocation` mount option and a Chromium-88 adoptedStyleSheets feature-detect shim in the studio bootstrap.
- **WHY:** The egress API is only trustworthy if every edit path feeds it — #137 proved paths already skip the internal seam; one helper makes omission impossible. Contents are big and change per keystroke, so they violate the public-state contract (cheap, reference-stable derived values); a dedicated callback/pull keeps the hot path clean. The change-kind payload designs file lifecycle in now, so renames/deletes later are additive.

## Milestone structure for host-integration / live-inspector work
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** cross-system (studio-shell + bevy-brink host integration)
- **SCOPE:** moderate
- **WHAT:** Three layered milestones — **Phase 7 Host session channel** (shared transport: #127, #173); **Phase 8 Live inspector views** (run-time surfaces stacked on the channel; placeholder, populated from #127's spec); **Phase 9 Host-aware authoring** (author-time argument picking on the same channel: #176, #174, #175, #164). #173 folds into the channel layer; #164 into authoring.
- **WHY:** The live inspector and host-aware authoring feel similar because they share one foundation — the host session channel (#127). They are distinct deliverables (run-time observation vs author-time picking) that each stack on that channel. A layered split (channel → views → authoring) makes the shared dependency explicit and forces correct sequencing: build the channel once, both consumer surfaces follow.

## Phase 7 execution autonomy — merge-on-green for impl, review gate for design
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** process / Shell Phase 7 (host session channel)
- **SCOPE:** moderate (process, time-boxed to Phase 7)
- **WHAT:** For Phase 7, the agent keeps issues up to date (board In Progress → Done), works per-issue branches, and opens one PR per issue. **Implementation PRs self-merge once tests pass.** **Design/spec PRs do NOT auto-merge** (#127 → `docs/live-inspector-spec.md`, #178 → host-entry spec) — they wait for user review even though their tests are trivially green. Sequential merges. Authority runs through the end of Phase 7.
- **WHY:** The user wants momentum within the phase without gating every code change on review, and passing tests are a sufficient bar for implementation. But specs encode architectural decisions the user owns (the `SessionProvider` interface shapes all of Phase 8 and #178's composition), so those keep a human review gate.

## Studio QoL milestone — group accumulated authoring/debugging experience features
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** studio (+ runtime enabler)
- **SCOPE:** moderate
- **WHAT:** Created milestone **"Studio QoL"** (#10) for accumulated quality-of-life features for brink-studio. Members: #57 (editable runtime state), #69 (click-to-source), #77 (instant diagnostics), #155 (editor font), #184 (runtime sandbox eval — REPL enabler), #185 (REPL), #186 (play-from-here), #14 (per-keystroke reparse), #115 (analyzer unused warnings). **#164 (binder folder tree) moved here from Phase 9** (it's studio UX, not host-vocabulary authoring). Excluded as non-QoL: #72/#119 (tech-debt/bug), #152/#163 (CI), #166 (runtime conformance). Added requirement on #115: unused-knot warnings must **respect host-usage data** — model as reachability from a root set (story start + host-declared entry knots, via the host-capability manifest / #127 connection); host-referenced knots are advisory, never hard errors.
- **WHY:** The unmilestoned backlog had accumulated a coherent cluster of studio authoring/debugging-experience features; grouping them gives a schedulable roadmap distinct from the host-integration shell phases (7/8/9) and the conformance track. #164 fits QoL better than Phase 9 now that a QoL home exists. The #115 host-usage rule prevents false "unused" warnings on knots the engine enters from outside the ink source (host-directed entry, play-from-here #186, parameterized entry #178).

## Public diagnostic contract — resolve FileId→path at the compile() boundary (ResolvedDiagnostic)
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** compiler-api (public diagnostic contract)
- **SCOPE:** architectural
- **WHAT:** The public `compile()` family returns diagnostics as `ResolvedDiagnostic { file: FileId, path: String, range: TextRange, message, code }` instead of the internal `FileId`-keyed `Diagnostic`. The `FileId`→path resolution happens once, at the `compile()` boundary, where the driver's path map is still alive. Do **not** pre-resolve line/column — keep the byte-offset `range` so each consumer maps to its own column unit (LSP UTF-16, terminal byte/char, etc.). Internal passes keep using the `FileId`-based `Diagnostic`. `path` is the public key; `FileId` is in-result correlation only (it is not stable across recompiles). The `path` is byte-identical to the string the host used as the entry / answered `read_file` with — INCLUDEs resolve relative to the including file's directory (`resolve_include_path`), so the host's namespace (implicit in the entry path it chooses) is preserved end-to-end; there is no separate "relative root" API. Confirmed compatible with the path-string-keyed host file interface (`getFiles()`, `activeFile`, `onFilesChanged`, mount `files`) and its planned richer form (more operations, same path identity).
- **WHY:** `FileId` is an internal interning index, meaningless outside the compiler instance that produced it. Leaking it across the public boundary forces every consumer to re-implement `FileId`→path resolution, and any one can get it wrong — which is the root cause of #187's secondary bug: the celeris integration had no path map, fell back to assuming the entry file, and resolved an INCLUDEd file's byte offset against `main.ink` ("offset past EOF"). Resolving at the boundary follows "internal concepts shouldn't leak into public types" and "understand the consumer"; not pre-baking line/col follows "defer resolution to the latest useful point," since column units are consumer-specific and the consumer already holds the source text.

## Web/studio replay adapter — in-place StoryRunner reload
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** brink-web / studio session (cross-consumer replay)
- **SCOPE:** architectural
- **WHAT:** The web/studio replay adapter uses an in-place `StoryRunner.reload(bytes)` that swaps Program/Story while keeping bindings + seed + the replay recorder Rust-side, rather than re-instantiating a fresh StoryRunner per recompile. `startSession` calls `reload` on the existing runner. Mirrors bevy-brink's durable-owner model (recorder lives on the durable owner; the program/flow is what gets swapped). Full-page-reload durability (localStorage persistence) is deferred; HMR-only for now.
- **WHY:** Keeps recordings entirely Rust-side (no per-compile wasm-boundary marshaling) and unifies both consumers on one "durable owner holds the recorder, swap the program" ownership model — a single mental model, validated by the bevy-brink work. The self-referential reload cost is small and localized to one `&mut self` method.

## Knot parameter arity in the compiled format (#178 host-directed knot entry)
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** format / codegen / runtime (host-directed knot entry, #178)
- **SCOPE:** architectural
- **WHAT:** Record knot/container parameter arity in the compiled format — add `param_count` to `ContainerDef`, populated by codegen from the knot's declared params (the converter reference pipeline defaults it to 0/unknown). This backs `choose_path_string_with_args(path, &[Value])` arity validation and lets `call_function` validate too. Chosen over (a) shipping without arity checks like `call_function` does today, and (b) bytecode introspection (fragile — a leading `~ temp` also emits a `DeclareTemp`, so the count over-reads).
- **WHY:** #178 requires erroring on arity mismatch, but the format had no knot param metadata (only `ExternalFnDef` has `arg_count`). Recording the real count at codegen is the only robust source — the runtime can't reliably recover it from bytecode, and the converter can't from inklecate's JSON. It's an additive metadata field, so execution behavior and the oracle are unaffected, at the cost of an `.inkb` format-version bump.

## Recompile keeps auto-reload; #181 degraded mode is mechanism-only
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** live-inspector / studio session channel
- **SCOPE:** moderate
- **WHAT:** A successful recompile continues to hot-reload the running session in place (the #179 replay-restore path). Program-identity / degraded mode (#181) is therefore built as mechanism only — the running-vs-latest-compile checksum compare, the degraded UI states (graph overlay drops current-location highlight + visit badges; status-bar "source out of sync"), and a cheap `program_checksum(bytes)` wasm util — and is NOT end-to-end exercisable locally. It goes live when a remote provider (Bevy dev-websocket / RMMZ host) runs a program older than the studio's source.
- **WHY:** The local provider always runs exactly what the studio compiled (auto-reload on every successful compile), so the running checksum == latest-compile checksum and degraded mode never triggers locally. Degraded mode is inherently a remote-provider condition (author edits while a separate game keeps running the old story). Changing recompile to leave the session on the old program would make it locally reachable but changes the "edit while playing" UX, which deserves its own consideration; we keep today's auto-reload. Bevy is the first real "remote" case where it'll be exercised.

## #182 (multi-session / flow picker) spun into its own spec, sequenced last
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** live-inspector / multi-session
- **SCOPE:** moderate
- **WHAT:** #182 (multi-session / flow picker, Phase 8.4) is spun out into its own short spec rather than riding `docs/live-inspector-spec.md`, and sequenced last in Phase 8. It carries a runtime/wasm dependency (the wasm `StoryRunner` exposes no flows today; the runtime's `FlowInstance` doesn't cross the boundary), so it needs design before build. Likely modeled as N `SessionProvider` instances + an `activeSessionId` selector in the store.
- **WHY:** It's the heaviest and least-blocking Phase 8 item, the inspector spec already defers it (§7 "designed-for, not built here"), and it requires a runtime-surface change — which by standing rule goes through a design issue, not inline.

## #182 multi-session: registry of single-session providers; isolated local runners now, shared-context seam later
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** live-inspector / multi-session
- **SCOPE:** architectural
- **WHAT:** #182 is modeled as a session **registry** above the unchanged single-session `SessionProvider` (#179): a `sessions` store slice holds an ordered `SessionEntry[]` + `activeSessionId`; the active provider's snapshot mirrors into today's reactive fields, so views are unchanged. Local multi-session uses **independent runners** — each session is its own `StoryRunnerHandle` with isolated globals, started at root or a knot via `go_to_path` — so it is studio-only with no runtime/wasm change. Shared-context `FlowInstance` sessions (true ink/bevy shared-`Context` semantics) are **deferred behind a seam**: a future shared-context source needs new wasm flow API (`spawn_flow`/`continue_flow`/`choose_flow`/`destroy_flow`/`list_flows`) but no view/registry change. A picker, hidden at ≤1 session, sets the active session; drive commands gate on the active provider's capabilities (#180). Player-tabs-per-session (#120) is a follow-on. Spec: `docs/multi-session-spec.md`.
- **WHY:** Brink doesn't compile ink's `FLOW` feature — multiple flows are host-orchestration and the wasm runner is single-flow, so the local studio is inherently single-flow. Independent runners deliver useful local multi-session (speculative / compare-two-paths / play-from-here) with isolated globals — the safer default — and zero runtime risk, while the registry stays provider-agnostic so the genuine shared-context case (the real remote/Bevy semantics) drops in later as just another source. Reusing the single-session provider as the unit avoids reworking #179.

## Pull shared-context flows forward as a local studio feature (#200)
- **WHEN:** 2026-06-14
- **PROJECT:** brink
- **SYSTEM:** live-inspector / multi-session / runtime
- **SCOPE:** architectural
- **WHAT:** The shared-context-flow seam deferred in `docs/multi-session-spec.md` §7 is pulled forward as a **local** studio feature: a **"+ New flow"** picker action spawns a real `FlowInstance` in the *same* `Story`, sharing the `Context` (globals / visit counts / rng) while keeping per-flow callstack + temps — true ink concurrent-flow semantics, runnable locally without a game. The picker keeps **both** kinds: "+ New session" (independent runner, isolated globals, #182) and "+ New flow" (shared globals). Runtime approach (user call): **add a shared path, keep isolated** — a new `Story::spawn_flow_shared` + `shared_instances` map driving named flows against `default_context`, plus `continue_flow_single` / shared `choose` / `destroy` / `debug_snapshot_flow`; the existing isolated `spawn_flow` stays for bevy-brink's per-entity model. Then wasm exposure + a `FlowSessionProvider` (shares the primary session's runner) + the picker action. Tracked as #200.
- **WHY:** The runtime already decomposes cleanly — `Context` is story-wide state (globals/visits/rng), `FlowInstance` holds per-flow callstack + temps (`CallFrame.temps`) — so sharing `Context` across flows is the correct ink semantic with no Context refactor. The user wants this locally (not just for a remote/Bevy host) to watch concurrent flows interact via shared world state in a single story. Single default-flow execution is untouched, so the oracle is unaffected (re-verified via the ratchet); a shared path rather than changing `spawn_flow` preserves bevy-brink's existing per-entity isolated-globals assumption.

## Phase 9 host-aware argument picker — Tier-3 design (static-first)
- **WHEN:** 2026-06-15
- **PROJECT:** brink
- **SYSTEM:** host-capability-manifest / studio authoring
- **SCOPE:** architectural
- **WHAT:** Host-aware argument picker (Tier 3 of the host-capability manifest), designed in `docs/host-argument-picker-spec.md` (#176 umbrella composing #174 manifest + #175 studio extension). `SemanticTypeDef` gains `values?: ValueSource` = `{source:"static", items:[{value,label,detail?}]}` OR `{source:"host"}` — a **separate** field from `constraint` (picking orthogonal to checking); `values` is **advisory** (never hardens a diagnostic; base type still compiles). #175 adds `StudioExtensions.argumentProviders?` keyed by `TypeRef`, data-only (`enumerate(ctx)→{value,label,detail}[]` + optional `resolveLabel`), rendered through the existing completion + inlay-hint UI; reuses the existing `signature_help`/`find_call_context` join point (arg position → semantic type). Transport for `host` values = **push-cache** (host pushes per-type snapshots via a dedicated `EditorSession.set_host_values`; completions/inlay serve synchronously from a cache; distinct author-time message set from Phase 8's runtime SessionProvider, may share the connection). **Static slice ships first** (schema + arg-source query + static picker + inlay labels, no transport, locally exercisable), **then** the dynamic host path (`set_host_values` push + cache; exercised end-to-end by celeris RMMZ, separate repo). Host-rendered editors (map-point picker + invocation protocol), arg-group/inter-arg widgets, and manifest generation are **out of scope** (Tier 3+, separate increments).
- **WHY:** The call-site-arg→type join already exists from Tiers 1–2, so the picker is a new *consumer*, not new infrastructure. A separate `values`/`constraint` keeps checking and picking orthogonal. Push-cache gives snappy synchronous completions (value sets change rarely; the host knows when; async-per-keystroke would jank the editor). Static-first de-risks the editor UI/plumbing before any transport and delivers immediate value for genuinely-closed labelled sets; the dynamic path is the killer feature but needs the host (celeris) as consumer. Advisory checking respects that the running game is source of truth (ids legitimately appear/disappear between sessions).

## Argument-widget authoring — Fill and Form are distinct entry points
- **WHEN:** 2026-06-15
- **PROJECT:** brink
- **SYSTEM:** brink-studio (argument widgets)
- **SCOPE:** moderate
- **WHAT:** Argument-widget authoring (designed in `docs/argument-widget-spec.md`) exposes three distinct entry points into one per-param editor: **Edit** (inline picker on an existing literal — the `hex_color` swatch), **Fill** (a clickable ghost placeholder at an empty arg slot — `set_tint(‹color›)`), and **Form** (a hover-revealed glyph on the call that composes a per-param form for the whole call, one field per param, host whole-call widget overriding the auto-composed form for multi-arg completion). **Fill and Form are kept as separate interactions**, each for its own purpose, rather than merged. The only place they appear merged is an **arg-group widget**, which presents as a single Fill target / single form field because the widget spans multiple params — that collapse is the widget model's doing, not a Fill/Form special case.
- **WHY:** Each interaction has independent value (quick single-arg fill vs. composing a complex call). Consistent, predictable per-interaction behavior beats being clever about collapsing them; the arg-group collapse falls out of the widget spanning multiple params for free. Keeps the inline picker the user explicitly still wants, while adding a click-to-fill path so authoring complex calls doesn't require typing literals like a programmer.

## Argument-widget forks resolved — inline is data-only, panel launches the Form
- **WHEN:** 2026-06-15
- **PROJECT:** brink
- **SYSTEM:** brink-studio (argument widgets)
- **SCOPE:** architectural
- **WHAT:** Resolved the open forks in `docs/argument-widget-spec.md`. (1) **Inline is always studio-rendered**: a host contributes inline only as *data* — a label string (e.g. map name + location) via `inline(ctx) → { text, className? }`, plus an optional CSS class on the chip span — never host-mounted DOM in the source text, and no thumbnails (a future `icon?` SVG-string prop is conceivable but explicitly out of scope today). Heavy host visuals live only in the **editor** (popover/modal), via a mount-callback seam (`editor.render(ctx, host, container) → teardown`), with the studio owning the chrome (positioning, focus-trap, esc/cancel). Editor `surface` (popover|modal) is declared in the manifest. (2) **The Host Functions panel becomes a Form launch point**: clicking a function opens the Form pre-targeted at that external and inserts the *completed* call at the cursor (replacing today's bare-skeleton `~ fn(name, x, y)` insert); a skeleton quick-insert stays available via modifier-click. (3) Editor typing/completion insertion never auto-opens the Form (shows Fill placeholders + the call glyph); a **Settings panel** toggle can enable editor auto-open (default off); the panel is the deliberate Form trigger. (4) The call-level Form glyph placement is **prototype-both** — ship a hover-revealed glyph and an always-visible inline glyph behind a toggle and choose by feel.
- **WHY:** Keeps the source line studio-owned and styling-bounded ("no insane shenanigans") while still letting a host label a chip meaningfully; the host's real UI surface is the editor, not the line. Panel-opens-Form turns the Host Functions panel from "paste a skeleton I then have to fix up" into "compose a real call by clicking" — that is what justifies the panel existing. Auto-modal mid-typing is intrusive, so the panel is the intentional Form trigger and Settings covers authors who want auto-open. Glyph placement is a feel decision best settled by trying both rather than guessing.

## Value picker is a label-searchable typeahead with a two-path focus rule
- **WHEN:** 2026-06-15
- **PROJECT:** brink
- **SYSTEM:** brink-studio / editor (value picker)
- **SCOPE:** moderate
- **WHAT:** The static/host value picker (`values` on a `SemanticTypeDef` + `argumentProviders`, #174/#175) must be a **searchable typeahead that filters on the label, not the inserted value** — a host's integer-id set is searched by item name (type "potion" → matches the item labelled "Potion", inserts its id `3`), usable at hundreds of entries. Two surfaces with a deliberate focus rule: (1) **typing path** stays CM autocomplete — completion items *match on the label* but *apply the value* — and **never steals cursor focus** (an author who knows the id just types it); (2) **click path** — invoking the `value-list` widget (chip / Fill placeholder) opens a popover with a **focused** search box + virtualized list, because the author deliberately opened it. Filed as #211; the `value-list` built-in widget is specced in `docs/argument-widget-spec.md` §7. Open: fuzzy vs substring, matching `detail` too.
- **WHY:** Large host value sets are unusable as a flat scroll, and authors think in names, not ids — filtering by label keeps them usable. The two-path split resolves the focus-theft concern the user raised: typing is non-intrusive (you keep typing), while an explicit click intends focus. Reuses the existing call-site-arg → semantic-type join and CM's `label`/`apply` split, so it is a rendering refinement, not new infrastructure.

## Host panel categories use nested `path`; value picker invoke is keyboard-reachable
- **WHEN:** 2026-06-15
- **PROJECT:** brink
- **SYSTEM:** brink-studio (host panel / value picker)
- **SCOPE:** moderate
- **WHAT:** Two refinements folded into `docs/host-capability-manifest.md` + `docs/argument-widget-spec.md`. (1) **Host Functions panel categories use `path: string[]`** on `ManifestExternal` — nested (category → sub-category → …), not a flat string, so a host expresses a real taxonomy; un-`path`'d externals fall into a default bucket; the panel renders collapsible sections + a search/filter box over it. Pure static presentation metadata (like `doc`/`kind`); the schema decision settles it — remaining work is implementation, so `needs-design` was cleared on #210. (2) **The `value-list` picker's invoke path is keyboard-reachable**: pressing ↑/↓ while the cursor is in the arg slot opens the searchable popover (focused search box + virtualized list), not only a mouse click on the chip/Fill placeholder. Arrow keys are a deliberate "browse" gesture, consistent with the focus rule — the typing path (CM autocomplete, match-on-label/apply-value) never opens or steals focus, but an explicit ↑/↓ or click does (#211).
- **WHY:** (1) Nested paths scale to real host vocabularies (hundreds of verbs) where a flat label would flatten distinct groups together; tying it to manifest data keeps the host owning its taxonomy. (2) Keyboard invocation lets an author who doesn't know the value engage the list without reaching for the mouse, while preserving "typing never steals focus" — the gesture, not a heuristic, decides intent.

## Form launchers: hover-card action is the default; glyph + auto-open are opt-in
- **WHEN:** 2026-06-15
- **PROJECT:** brink
- **SYSTEM:** brink-studio (argument widgets)
- **SCOPE:** moderate
- **WHAT:** The §6.5 "prototype both, choose by feel" fork resolved after seeing the options in the running studio, and the model was **decoupled**: the whole-call Form is launchable five ways, with the unobtrusive ones as defaults. (1) **The hover-card "edit arguments" action is always on** — hovering a call name shows the (restyled) hover card with an edit button; zero in-text chrome, so it needs no setting. (2) **The inline ⊞ glyph is an independent opt-in** — a live Settings → Editor control (`off` | `on line hover` | `always visible`), **default `off`** since the card already covers it without cluttering the line. (3) **A `Mod-Shift-A` keybind** opens the Form for the call the cursor is inside. (4) **The Host Functions panel** click opens the Form and inserts the completed call. (5) **Auto-open on completion-accept** — a Settings checkbox (**default off**); when on, accepting a function/method completion inserts `()` and opens the Form. All settings are live (StateField + effect, dispatched via the studio store to every open editor) and persisted under `brink-studio.editor.v1`.
- **WHY:** With multiple launchers, the defaults should keep the source line clean — the hover-card action and keybind add no in-text chrome, so they carry the default; the inline glyph is there for authors who want a visible affordance but off by default to avoid redundancy. Live (not reload-only) switching is what makes "choose by feel" actually work. Auto-open is the most invasive (it couples the completion path to the Form by inserting `()`), so it stays default-off and opt-in — matching the spec's §6.6 intent.

## Argument Form is a typed per-argument widget collection with live inter-arg context
- **WHEN:** 2026-06-16
- **PROJECT:** brink
- **SYSTEM:** editor-ui (argument widgets)
- **SCOPE:** moderate/architectural
- **WHAT:** The call Form is a collection of per-argument widgets chosen by the argument's TYPE: native scalars → text inputs; host-declared enums → studio-rendered dropdowns (the host declares the value list; the studio owns the combobox — hosts never reinvent one); host custom widgets incl. arg-groups (the map-point picker) → the host's editor embedded inline. Inter-arg context resolves from the Form's LIVE draft state, not the committed document, so dependent-argument workflows ("pick a map, then a spot on that map") work inside the Form.
- **WHY:** APIs are designed from the consumer's perspective — a host's job is to declare capability (enum values, a custom widget), not to rebuild standard UI; a value-list is simply what the studio presents for an enum type. Holding live draft state (vs. reading the document) is precisely what lets one argument parameterize another's editor before anything is written.

## The call Form is driven by the signature metadata, not the live call-site
- **WHEN:** 2026-06-16
- **PROJECT:** brink
- **SYSTEM:** editor-ui (argument widgets)
- **SCOPE:** moderate
- **WHAT:** The Form structures itself from the callee's declared signature — one control per declared parameter, and every declared arg-group widget (e.g. the map-point picker) is always rendered — independent of how many arguments the current call has. Existing arguments only seed initial values (mapped positionally); on Apply the Form writes a well-formed N-argument call (too-few args are filled in; surplus args are truncated to the signature). The query surfaces a `declared_groups` set (manifest structure, no arg-state) for this, distinct from the arg-state-driven `groups` that drive the conservative *inline* chip/ghost.
- **WHY:** The Form is a composition surface for a call per its signature; the live call-site is just the seed. A partial or malformed call is exactly when an author reaches for the Form, so its widgets must not disappear or degrade to plain text just because the arguments don't currently line up. Inline decorations stay conservative (only when arg state is unambiguous); the Form is metadata-complete.

## The studio file lifecycle (rename/move/delete) lives in brink-ide, not studio TS
- **WHEN:** 2026-06-16
- **PROJECT:** brink
- **SYSTEM:** brink-ide / editor-ui (file lifecycle, #164)
- **SCOPE:** architectural
- **WHAT:** Whole-file create/rename/delete/move (and the folder tree's path operations) are a brink-ide feature, not studio-TS glue. The non-trivial logic — rename/move with **INCLUDE-reference rewriting** (find referencing files via the include-graph reverse edges, recompute each `INCLUDE`'s new relative path, rewrite the token) — lives in brink-ide and is returned as a `MoveResult { new_source, cross_file_edits }`, mirroring the existing symbol rename/move (`rename.rs`, `structural_move.rs`). The studio is a thin UI that applies the result through the established `applyMoveResult` seam. Only the folder-tree *rendering* (Stage 1) was purely presentational.
- **WHY:** The path/reference semantics (include graph, relative-path math, multi-file edits) are compiler-database concerns that already live in brink-ide; duplicating them in TS would diverge from the analyzer's source of truth and the symbol-move infrastructure. Keeping it in brink-ide reuses the proven cross-file-edit pattern and keeps the studio thin.

## Normalize `..` in include-path resolution system-wide
- **WHEN:** 2026-06-16
- **PROJECT:** brink
- **SYSTEM:** cross-system (include resolution — compiler/brink-driver, runtime/bevy-brink, IDE/brink-db + studio)
- **SCOPE:** architectural
- **WHAT:** Normalize `.`/`..` segments in include-path resolution (`resolve_include_path`) system-wide, so relative `INCLUDE` paths that traverse upward resolve to clean file keys. Update brink-db (the canonical impl), bevy-brink's duplicate copy (`source_loader.rs`), and the brink-driver tests that asserted the old literal (`a/b/../d.ink`) behavior.
- **WHY:** Upward `../` includes previously resolved to literal non-existent keys across the compiler, runtime, and IDE — a latent system-wide bug. Stage 3 file move/rename (#164) must rewrite `INCLUDE`s to point at new locations and needs `..` for the common sibling/parent layouts; without normalization those rewrites would emit broken includes. Normalizing centrally fixes the latent bug and unblocks correct move/rename. Include resolution is compile/discovery-time, not runtime-episode-time, so the oracle corpus is expected unaffected — verified that no fixture relies on the literal behavior and the ratchet does not move.

## Structural-move same-file ref edits are folded into `new_source` via slice-local application
- **WHEN:** 2026-06-19
- **PROJECT:** brink
- **SYSTEM:** brink-ide (structural_move — move/promote/demote)
- **SCOPE:** moderate
- **WHAT:** `promote_stitch_to_knot`, `demote_knot_to_stitch`, and `move_stitch` must apply same-file reference edits to the rebuilt `new_source` (not just drop them and emit only cross-file edits). The chosen mechanism is a slice-local `apply_window(source, base, limit, edits)` helper: each verbatim slice the op already concatenates is routed through it, and inside-region refs are applied to the moved text before header rewrite. This preserves all existing structural/whitespace/doc-attachment logic. `move_stitch`'s unfinished `// TODO: adjust same-file ref offsets` is resolved by the same helper — all three ops fixed together.
- **WHY:** Both consumers (brink-web `move_result_json`, brink-cli `emit_move_result`) treat `new_source` as the complete primary-file result and discard/override any primary-file edit, so dropped same-file edits leave dangling references (E024). Slice-local application was chosen over manual offset-delta math because it reuses the existing slice partition (references are atomic, never straddling a boundary), avoids fragile arithmetic across the before/after ordering branches, and keeps the structural reconstruction — already pinned by snapshot tests — untouched.

## Studio symbol Rename is safe-by-default with an in-place breakage report (#305)
- **WHEN:** 2026-06-20
- **PROJECT:** brink
- **SYSTEM:** editor-ui (studio shared symbol context menu / rename)
- **SCOPE:** moderate
- **WHAT:** The knot/stitch **Rename** action (on the shared symbol context menu — editor / Binder / Story Graph, per #305) is **safe-by-default**: it computes the cross-file rename, re-analyzes the hypothetical result, and if that introduces diagnostics it does **not** apply — instead the rename prompt flips to a **breakage report** ("Renaming X → Y would break N places:" with a clickable severity · file:line · message list). The only override is a **Force rename** button living *inside* that report (no upfront "unsafe" checkbox on the initial prompt). Force rename applies anyway but still surfaces the introduced diagnostics (toast) so breakage is never silent. This mirrors the `brink ide rename` CLI's safe-by-default + `--unsafe`.
- **WHY:** The valuable part of an "unsafe rename" is the *report of what breaks*, not a scary checkbox you tick before you know there's a problem. Always trying safe first and surfacing breakage only when it exists keeps the common case frictionless, makes the consequence (not the mechanism) the thing the user acts on, and keeps the studio consistent with the CLI's safe-by-default refactor contract. No upfront checkbox avoids asking the user to pre-authorize a risk they can't yet see.

## F2 inline rename is a full cross-file, safe-by-default rename (#305 follow-up)
- **WHEN:** 2026-06-20
- **PROJECT:** brink
- **SYSTEM:** editor-ui (studio editor F2 rename / shared rename pipeline)
- **WHAT:** The editor's **F2 inline rename** performs a **full cross-file rename** and is **safe-by-default**, sharing the menu "Rename…" pipeline (`rename_safe` → breakage report → Force rename) seeded from the symbol under the cursor. The previous F2 (`rename_doc`/`rename_impl`) only rewrote the active file and silently dropped cross-file reference edits — that was a bug, not a feature. F2's native `prompt()` is replaced by the store-driven rename prompt; both rename entry points (context menu + F2) now flow through one `rename_safe`-backed path, so there is exactly one rename pipeline and one safety guarantee.
- **SCOPE:** moderate
- **WHY:** A rename that rewrites a declaration but not its references is broken and dangerous — it leaves dangling references. Cross-file correctness and the safe-by-default guard must be uniform across every rename entry point, not split into a fast-but-unsafe F2 alongside a careful menu path. Unifying on one pipeline removes the divergent (buggy) code path and guarantees F2 gets the same breakage report the menu does.

## Editor round 2 (celeris feedback): design-first, single 0.8.0 release
- **WHEN:** 2026-07-05
- **PROJECT:** brink
- **SYSTEM:** cross-system (editor-ui/web)
- **SCOPE:** moderate
- **WHAT:** The celeris-filed round (#363–#371 + #343/#347/#276) ships as one 0.8.0 release. Design epics #370 (Story Session) and #368 (dialect) run design fan-outs first; any build item their outcome could affect (#365, #366, #371-snapshots) is held until the relevant design is approved, while design-independent items build immediately in parallel. Snapshot/diff APIs are designed as Story Session methods. #363's class taxonomy ships as an open string-keyed scheme (documented core kinds, host-extensible). (#362 line-fit epic is parked out of this round: it depends on #366, manifest-trajectory answers from #368, and celeris-side metrics input.)
- **WHY:** These are public contracts hosts consume directly (classes, attributes, APIs) — shipping shapes in 0.8.0 that a design round would immediately rename means breaking changes for celeris; one release delivers the whole coherent surface at once.

## Story Session (#370): the journal is Rust-canonical; web and bevy are bindings
- **WHEN:** 2026-07-05
- **PROJECT:** brink
- **SYSTEM:** runtime / brink-web / bevy-brink (Story Session primitive)
- **SCOPE:** architectural
- **WHAT:** The session journal (choices, set-var, goto, external results, seed, checkpoints) lives in Rust — the existing `ReplayRecorder` generalizes into it, wrapping `FlowInstance` as a composing session layer. `@brink-lang/web`'s `StorySession` is a thin wasm binding over it (journal serializes to JSON via serde for host save slots); bevy-brink consumes the same session layer first-class. There is no JS-side journal. Supporting rulings: v1 journal format embeds a terminal `SaveState` for fast-restore; mid-turn mutations are restricted to turn boundaries by contract with a reserved position-anchor field; the typed snapshot/diff Rust export ships in v1; live-replay externals park as `AwaitingExternal` + `continueReplay()`; `callFunction` is journaled; full `runner` escape hatch with documented journal bypass; flow-tag dimension reserved in the schema; label drift at a replayed choice index is a soft warning. Constraint: the journal layer is observation-only around the VM — episode behavior untouched, no oracle regeneration.
- **WHY:** The session/replay facility is wanted in Bevy games, not just web hosts — a JS-side journal would strand bevy-brink and leave two recorders (Rust + JS) whose truncation can never stay coherent (the failure the design critiques found in all three proposals). One Rust journal is the single source of truth; every consumer binds to it.

## Dialogue dialect (#368): authoring-time/tooling artifact only — never runtime-delivered
- **WHEN:** 2026-07-05
- **PROJECT:** brink
- **SYSTEM:** editor-ui / brink-ir (dialect system)
- **SCOPE:** architectural
- **WHAT:** The dialect declaration is an authoring-time/tooling artifact: no `.inkb` embedding, no project file in v1 (mount-time config only), and the host-capability-manifest charter (tooling-only, never runtime-consumed) stands unamended. The `emitted` facet stays in the v1 schema because the *editor* needs it (studio Player cue display; future #362 line-fit) — it models what the runtime will do, it does not instruct the runtime. Brink ships the reference `DialectParser` as an ordinary opt-in library; a consumer wanting editor/game single-truth imports it and passes the same JSON in their own game code. Supporting rulings: portable-regex core (JS ∩ Rust subset, CI-enforced) with affix sugar compiling to it; classification implemented in Rust `line_contexts()` in v1 with TS as a thin interpreter; enum→string `ElementType` migration is a hard cut in 0.8.0 with a documented PascalCase→kebab mapping; `nature` is three-valued (narrative/machinery/structural); blank lines always break the dialogue chain in v1; `dialect: null` tears down the whole screenplay layer; intl×affix interaction filed as a follow-up rather than blocking v1.
- **WHY:** The game developer writes their own parser — it is the ground truth of their game; the editor's job is to model that truth for authoring, not to make game parsing config-driven (a problem no consumer has). Dropping runtime delivery removes the whole delivery-channel question (project file / .inkb metadata / manifest charter amendment) from v1 scope.

## Round-2 pump: legitimate discovered work rolls into the round autonomously
- **WHEN:** 2026-07-05
- **PROJECT:** brink
- **SYSTEM:** cross-system (editor round 2 / pump process)
- **SCOPE:** minor/local (this round's pump waves)
- **WHAT:** When a pump wave's scope-reconciliation (or a build's scopeNotes / review's scopeGaps) surfaces a **legitimate new item** — small, clearly within the round's themes (editor/web embedder surface, dialect/session follow-through) — the agent files the issue and rolls it into a subsequent pump wave without stopping for approval. Anything architectural, design-flavored, or outside the round's themes still surfaces at a checkpoint for a human ruling, per the pump's default contract.
- **WHY:** The user is delegating the round end-to-end ("just pump through all the work remaining"); pausing the train for every small discovered gap defeats the delegation, while the legitimate/architectural boundary keeps design authority with the user.

## #411 scratch-eval: proper deep-layer build (fragment codegen + program overlay), Rust-canonical, ships as 0.9.0
- **WHEN:** 2026-07-06
- **PROJECT:** brink
- **SYSTEM:** cross-system (compiler + runtime + web)
- **SCOPE:** architectural
- **WHAT:** The scratch-flow evaluation API (#411) is built "properly" in the deep layers, rejecting the pragmatic recompile+state-migration shortcut: a true fragment-compilation entrypoint in the compiler (expressions/content lowered against the project's existing symbol index) plus program-overlay linking so the fragment executes in the live program's address space and can call real ink functions. The scratch runner (cloned-Context flow: spawn/run/collect/discard, step-capped) is Rust-canonical in brink-runtime; @brink-lang/web's evaluateScratch is a thin wrapper. Released as 0.9.0 (minor — new API surface), not 0.8.1.
- **WHY:** "the more we move into the deeper layers, the more we get proper fixes we don't need to recreate later" — same rationale as the Story Session Rust-canonical ruling; bevy's live inspector is a first-class future consumer and shouldn't require re-implementing eval machinery.

## #411 scratch-eval externals policy: harness-built handler; presentation = effect; async pending supported in v1
- **WHEN:** 2026-07-06
- **PROJECT:** brink
- **SYSTEM:** runtime + web
- **SCOPE:** moderate
- **WHAT:** The @kind tiering for scratch evals is enforced by a policy handler the eval harness constructs from the analyzer's external_meta — the VM stays manifest-blind (preserves the "runtime never sees the manifest" invariant). Presentation-kind externals are treated as effects for scratch purposes (blocked in watch context; armable in eval context like other effects). Pending/async query externals are supported properly in v1: the isolated scratch flow awaits via the existing Pending/resolve_external machinery; evaluateScratch is async, with cancellation of stale in-flight evals (destroy the frozen scratch flow) and a cap on concurrent scratch evals.
- **WHY:** Presentation is by definition an effect — the manifest kind exists for client/server authority routing, not purity, so it gains nothing in scratch context. Async is cheap runtime-side (Pending already freezes/resumes flows); the real work is lifecycle (cancel + cap), worth doing once properly rather than shipping a sync-only v1.

## Scoped flow state: promote per-flow/world state model into the runtime core (0.9.0)
- **WHEN:** 2026-07-06
- **PROJECT:** brink
- **SYSTEM:** cross-system (brink-runtime core; brink-web, bevy-brink, Story all consumers)
- **SCOPE:** architectural
- **WHAT:** The per-flow/scoped-state model (today hand-rolled in bevy-brink) is promoted into brink-runtime as a first-class core; Story and bevy-brink become peer thin orchestrators over it; watch/eval (#411) demotes to a light feature on top. Sub-rulings: (1) uniform scoping — all story-state (globals/visits/turns/turn_index/RNG) is world-or-local per addressable unit via one engine-supplied WorldPolicy{default,overrides}; execution-state is intrinsically flow-local; single-flow = all-world degenerate = oracle-byte-identical. (2) ink untouched — policy is host-supplied at world creation, resolved once against Program symbols. (3) CoW layered local state (read-through chain local→parent-snapshot→world→defaults); world writes shared implicitly, local writes private; commit = fork-only, DEFERRED documented seam in 0.9.0 (keeps conflict policy out of the release). (4) Ownership: Arc for immutable (Program/tables — deletes the <'p> borrow), single-owner + step-scoped &mut for mutable, owned fork snapshots, no locks. (5) Spawn/drive API: World::new/spawn/fork + FlowStep(advance/choose/eval); core hands back owned (FlowInstance,FlowLocal) pairs and does NOT own flows. (6) Story decomposed to a behavior-preserved facade over shared Layer-2 ops (drive/Line-assembly/replay/locale), killing bevy's duplication; isolated/shared maps dissolve into policy. (7) RNG: fork snapshots parent stream; root-spawn local RNG host-supplied; draw algorithm untouched (does not reopen the parked shuffle divergence). (8) Persistent map = alloc::BTreeMap+Arc to keep the no_std goal (#434) open. Oracle-safety: behavior-preserving decomposition lands FIRST (F1 gate), byte-identical, before scoping rides on it. Spec: docs/scoped-flow-state-spec.md.
- **WHY:** Same rationale as the Story Session / deep-#411 rulings — promote capability into the runtime so every consumer benefits and nobody re-implements. The concrete driver: bevy-brink duplicates Story's drive/replay/locale orchestration (two implementations in lockstep for thousands of commits) because there's no clean primitive layer between raw FlowInstance/Context and Story. Bolting scoping onto that tangle would add a third thing to keep in sync; the decomposition removes the root cause.

## HIR span overlay in editor + source-compatible debugger direction
- **WHEN:** 2026-07-07
- **PROJECT:** brink
- **SYSTEM:** studio-editor / debugger
- **SCOPE:** architectural
- **WHAT:** Build an HIR-derived span overlay in the CodeMirror editor — nested spans projected from HIR (using preserved source spans + `DefinitionId`) carrying `data-*` attributes — as the substrate for richer styling/tooling/interactions AND as the foundation for a source-compatible debugger. Sequencing: **(A) HIR editor overlay first** — additive widening of the existing brink-ide → WASM → CodeMirror-decoration pipeline; makes the editor DOM span-addressable. **Then (B2) instruction-level (bytecode-offset → source-span) sourcemapping directly.** The line/container-granularity debugger (B1) is **explicitly skipped** — not pursued as its own deliverable. B2 is tracked as a separate epic/umbrella design issue and is **not** being addressed now; it requires threading HIR spans through LIR + codegen and either the lossy `Opcode::SourceLocation` route or a new `brink-format` debug-info section (an oracle-corpus-regen-class change).
- **WHY:** Today's highlighter is only syntax + name-resolution deep; HIR carries full structural + resolved-symbol provenance with source spans, enabling much richer editor affordances (Track A, cheap and additive). On the debugger, the user explicitly wants the *instruction-level* target over the cheap line-level one because: (1) a real stepping debugger must step through expression/divert/assignment evaluation — line granularity is too coarse to be satisfying; (2) the Track A overlay itself wants instruction-level provenance (precise per-expression spans, not just line/container), so B2 is the "real" substrate both the overlay and the debugger share; (3) instruction-level maps also unlock precise diagnostics/telemetry (exact error locations, coverage, profiling, breakpoint-on-expression); (4) a line-only debugger isn't compelling enough to build standalone — if it's built, build it properly. The format/oracle-corpus cost of B2 is accepted as the price; hence it's scoped as a deferred epic rather than skipped.

## Track A synthetic-container identity + full-branch rails
- **WHEN:** 2026-07-07
- **PROJECT:** brink
- **SYSTEM:** studio-editor
- **SCOPE:** moderate
- **WHAT:** For Track A (HIR editor overlay), accept **range-derived identity** for synthetic containers (choices/gathers/branches/threads/tunnels). Block rails cover the **full choice-branch extent** — computed as the range-union of the HIR's `fold_weave`-produced `Choice.body` (the CST `Choice` node covers only the choice line, not the folded body) — and stay stable while editing via per-parse recompute + R5 remap (last-good line decorations remapped through the change), with **no persistent handle**. Handle is the full range (or `(start, kind, depth)`), not a bare start-offset, to avoid same-block false-matches. Named containers still carry their real `DefinitionId` in addition. **Explicitly do NOT adopt or revisit** a structural/path-derived (path_hash-style) identity for synthetic containers now; defer to the #452 debugger epic, whose upgrade trigger is a *persistent* anchor on an anonymous block (edit-surviving breakpoint, pinned annotation, or runtime↔source correlation).
- **WHY:** path_hash / synthetic-container addressing was a **consistent source of pain during compiler development**, and Track A gains nothing from it — rails and structural styling recompute per render and never need a persistent anchor, so range-derived identity is sufficient and keeps Track A zero-plumbing (no HIR clone/stamp, no db change). The full-branch rail extent is free because `fold_weave` already computed the branch boundary; we inherit it rather than re-deriving. Spec: `docs/editor-hir-overlay-spec.md` §5.1, §6, §6.1.

## Shared read-only HIR visitor + walker unification
- **WHEN:** 2026-07-07
- **PROJECT:** brink
- **SYSTEM:** compiler (brink-ir / brink-ide / brink-analyzer)
- **SCOPE:** architectural
- **WHAT:** Introduce a single shared read-only HIR visitor (`brink_ir::hir::visit` + `HirVisitor` trait; **dumb-walker + stateful-visitor** model, **enter/exit** hooks, **opt-in `Expr` descent**) and migrate the read-only HIR block-tree walkers onto it — `story_graph`, `line_context`, `folding`, analyzer `validate` (**4 walks → 1**), `external_check`. Do this **now/first** as a behavior-preserving precursor (#457) before the HIR editor overlay's `project_hir` (#454 phase 1), so the overlay rides the shared visitor rather than a 6th hand-rolled walk. Transform passes (`stamp`, `normalize`, `fold_weave`, LIR lowering) **stay separate** — their mutation/order-dependence is essential, not sloppy. Relocating the analyzer's reference/declaration collection (today emitted during AST→HIR lowering into `SymbolManifest`, *not* a HIR walk) onto the visitor is **deferred backlog (#458)**, bundled with an under-traversal correctness audit. Visitor design forced by the catalog: **enter+exit** (the overlay's container extent fold is post-order) and canonical deterministic child order.
- **WHY:** The block descent is re-implemented ~identically in 5+ places with clear duplication (`validate` alone runs 4 full traversals) and a latent correctness hazard: inconsistent `Expr` descent means a hand-rolled walker can silently miss references nested in interpolations / call-args. A shared visitor is a **correctness guardrail**, not just DRY — `external_check`'s `walk_block/walk_stmt/walk_expr` is already a working prototype. Doing it first keeps the overlay on a clean base and avoids adding yet another bespoke walk. Family split (read-only query descents unify; mutable/order-sensitive transform passes stay separate) confirmed by a full walker catalog (2026-07-07).

## Layered HIR structural architecture: projection as the canonical model
- **WHEN:** 2026-07-07
- **PROJECT:** brink
- **SYSTEM:** cross-system (brink-ir visitor, brink-ide line_context/folding/projection, brink-analyzer)
- **SCOPE:** architectural
- **WHAT:** Adopt a **layered structural architecture** with one canonical model. The shared HIR visitor (#457) is the **traversal primitive**, used at multiple layers. The **projection** (#454) — HIR → spans + per-line container/weave stack, built on the visitor — is the **single canonical structural model**. Per-line/per-span structural features are **views** over it: `line_context`'s `element`/`weave`, `folding`'s scaffold/nature, the rails/editor overlay. **Trivia** (comments/block-comments, from the CST) and **dialect** (regex over source) are **separately-layered facets**, never fused into the structural walk. Consequences: (1) `line_context` today conflates HIR-structure + CST-trivia + dialect into one 1361-line walk; its `WeaveElement` (`ChoiceLine`/`ChoiceBody`/`GatherContinuation`/`ConditionalBranch`/`SequenceBranch`) *is* the projection's per-line container stack (R7 rails generalized). It is **NOT migrated onto the visitor as-is** (that preserves the conflation); it is **subsumed** — re-expressed later as `compose(projection-line-view, trivia-facet, dialect-facet)`. `folding` likewise becomes a projection consumer. (2) **Neither is rewritten now** — they work (folding is behavior-preserving post-fix; line_context untouched). The projection is built and proven FIRST, as a new pass; the consumers are re-expressed as follow-up. (3) **#457 closes at 4/5** — `external_check`/`folding`/`story_graph`/`validate` migrated; `line_context` is **subsumed, not a pending migration**. (4) **#454's projection is elevated** from "editor overlay" to "canonical HIR structural model": its data contract must make the per-line weave/element view a first-class output that `line_context` and `folding` derive from, not a rails-only detail. (5) **Analysis is the upstream half of the same model:** reference/declaration **collection** (today interleaved in lowering, #458) is a **mis-placed visitor pass** — correct layering is `visitor → collection → resolve → projection → views`, with collection upstream (feeding analysis) and the projection downstream (consuming it). Doing collection as a visitor pass is exhaustive-by-construction, folding in #458's under-traversal audit; #458 stays deferred (risk unchanged) but is reframed as "correct the altitude," not "DRY."
- **WHY:** `line_context` and `folding` each hand-roll partial, overlapping per-line structural classifications, conflated with trivia/dialect, and are both known to be not-fully-correct (folding's non-transitive-context bug found in review; line_context's conflation). That `WeaveElement` is exactly the projection's container stack proves they're independently approximating the same model. One structural model (the projection) with layered facets and thin views removes the duplication and the conflation-driven bugs, at the right altitude (generalize the mechanism, don't special-case). Building it as a new pass first — not rewriting the working consumers — de-risks: the model is proven before anything depends on it, and no behavior changes until a consumer is deliberately re-expressed.

## F5 speculative-eval: mechanism-agnostic API on the simplest correct impl (B); overlay dropped, incremental codegen is a separate track
- **WHEN:** 2026-07-07
- **PROJECT:** brink
- **SYSTEM:** cross-system (speculative-eval / F5 / compiler-perf)
- **SCOPE:** architectural
- **WHAT:** F5 (Tier-1 speculative eval of arbitrary fragments) ships the `evaluate`-accepts-fragments capability on the **simplest correct mechanism (B)**: wrap the fragment in a synthetic symbol (`function $eval_<h>()` for an expression, `=== $eval_<h> ===` for content), recompile the project (cached by (live checksum, fragment source, kind)), and run it via the F4 `Speculation` over the recompiled `Program`, seeded from the live state via the name-keyed save/load path (robust — `DefinitionId`s are content-hashed). The bespoke fragment-compiler + `OverlayProgram`/`ProgramLike` overlay (the earlier 2026-07-06 "deep-layer build" ruling for #411) is **DROPPED**. Compilation performance — general **incremental codegen** (codegen/LIR/analysis are whole-project non-incremental today; only per-knot HIR is cached) — is a **separate, behind-the-API track**, invested in only if measurement shows recompile latency is a real problem.
- **WHY:** The consumer API is mechanism-agnostic — celeris only ever calls `evaluate(source)`; A/B/C differ only in latency, which is a perf characteristic behind the API, not a shape it should bend around ("i don't want to design/assume we can't make this particular API fast enough… it shouldn't require that it's done in a different way"). So ship the simplest correct implementation and make it fast *at the right layer* (general incremental codegen benefits the whole toolchain) only when measured to need it — rather than a speculation-only bespoke overlay. Caching makes B viable regardless of raw compile speed: a compile is paid once per distinct fragment per project version, not per re-eval; the per-eval run is cheap. The earlier deep-build ruling was correct for the *state model* (F1–F4, genuinely reused) but over-applied to the fragment mechanism, and was made before we knew ids were content-hashed (which de-risks B's state migration).

## Book code examples must be compile-checked
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** docs (docs/book, CI)
- **SCOPE:** moderate
- **WHAT:** The book's Rust examples stop being ```rust,ignore``` and become real doctests (hidden setup lines where needed), with `mdbook test` added to CI alongside the existing `mdbook build`. Where an example genuinely cannot compile in isolation, `ignore` stays but must be a deliberate, justified exception rather than the default.
- **WHY:** All 48 Rust blocks were `ignore`, so CI compiled none of them and the book silently rotted through an API change — `Story::new(&program, …)` survived the `Program → Arc` refactor (ac3619d4) in two chapters and in brink-runtime's own crate doc comment. Prose can only be kept honest by review; code can be kept honest by the compiler. Making the examples executable converts a recurring manual audit into a build failure.

## bevy-brink re-exports all public-signature brink_runtime types; World aliased as BrinkWorld; whole crate re-exported as `runtime`
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** bevy-brink
- **SCOPE:** moderate
- **WHAT:** bevy-brink re-exports every brink_runtime type that appears in its public API so consumers never need a direct brink-runtime Cargo dependency. Types get their own names (FlowInstance, Program, Choice, Line, RuntimeError, FallbackHandler) EXCEPT World, aliased as `BrinkWorld` to avoid the E0659 glob collision with `bevy::prelude::World`. Additionally the whole crate is re-exported as `pub use brink_runtime as runtime;` as a stable escape hatch for any type not individually re-exported.
- **WHY:** The existing Value/LocaleMode/Transcript re-exports already establish 'no direct brink-runtime dep' as intent; coverage just lagged. Plain `pub use World` was rejected because it breaks the idiomatic `use bevy::prelude::*; use bevy_brink::*;` preamble (verified: E0659). The `runtime::` module escape hatch future-proofs against the same class of leak recurring.

## Bevy book examples become compile-checked doctests (not `rust,ignore`), backed by a `book-test` recipe
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** docs/book
- **SCOPE:** moderate
- **WHAT:** The Bevy book examples should be compile-checked doctests rather than ```rust,ignore``` fences, backed by a new `book-test` Justfile recipe. Motivating case: the `commit_from` example wouldn't compile against `bevy_brink::World`, which is what surfaced the missing re-export.
- **WHY:** Ignored examples silently rot — the re-export gap existed precisely because nothing compiled the book's Bevy code. Compile-checking makes the docs a conformance test on the public API surface.

## Flow-private state: runtime capability first, language feature later
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** cross-system (runtime + compiler)
- **SCOPE:** architectural
- **WHAT:** F6 lands the runtime capability with host-authored (imperative) policies. A language storage class for flow-private vs shared vars is a separate later epic: the compiler will emit a default policy that World creation merges with host overrides — so F6's policy API is shaped as compiled-defaults ⊕ host-overrides even while the compiled side is empty. (The language epic is the one place `brink-format` eventually changes — scope bits carried in the compiled `Program`; F6 itself touches no format.)
- **WHY:** ink has only global `VAR` and `temp` — no flow-private persistent storage class — and the compiled `Program` keeps no file provenance, so natural authoring ultimately needs the language change. But the runtime substrate is unblocked now, and the language feature layers on cleanly by generating the same policy the runtime already consumes.

## Entity flows persist state-only via per-flow SaveState
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** scoped-flow-state (brink-runtime / bevy-brink)
- **SCOPE:** moderate
- **WHAT:** Entity flows must survive save/load. Each flow's durable state round-trips as an ordinary name-keyed `SaveState` (`save_state`/`load_state` lifted off `Story` to work over any flow's context); a save = one `SaveState` for the shared World + one per entity, composed host-side (bevy). No `brink-format` change; `SAVE_FORMAT_VERSION` stays 1. State-only: a paused flow resumes from its knot entry with state intact, not mid-line.
- **WHY:** `FlowLocal`'s persistable content matches `SaveState`'s shape one-to-one, and name-keying gives recompile safety. Narrative delivery is essentially serial, so mid-line resume of background flows isn't needed — resume-from-entry is acceptable, even ideal.

## Scoped-state policy model (final): per-World, default World, subtree-inclusive knot scope
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** scoped-flow-state (brink-runtime)
- **SCOPE:** architectural
- **WHAT:** `WorldPolicy` stays per-World, installed once at World creation — host-authored at plugin setup now, compiler-emitted later and merged as base ⊕ host-overrides. Default is **World**; private (Local) units are the enumerated, marked case. Spawning a flow is just (entry) → `FlowInstance` + fresh `FlowLocal` — no policy parameter. A knot override applies to its **whole definition subtree** (interior sequence/weave containers included). Per-flow policy and absorb/merge machinery are explicitly rejected; promotion of private state to world state is written in ink.
- **WHY:** Scope is a property of the variable/knot, declared once and true for every flow. Plain `VAR` must keep meaning shared (ink compatibility + oracle anchoring), so private is the marked case — and default-World is the only default that survives the language transition without flipping. Per-entity privacy comes from each flow's own `FlowLocal`. Subtree ruling: sequence/cycle counters key interior container ids and idiomatic per-entity memory is carried by visit counts (stopping/cycle greetings, `{not knot}` choice gates), so non-subtree knot scoping silently half-breaks the motivating use case.

## F6 slicing — value now, language later, no rework
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** scoped-flow-state / bevy-brink
- **SCOPE:** moderate
- **WHAT:** **F6.1** (brink-runtime, oracle-gated): extract one shared drive-to-terminal op replacing `Story`'s loop + bevy's four duplicates; reconcile bevy's misnamed `STEP_LIMIT` onto core `LINE_LIMIT` semantics; lift `save_state`/`load_state` off `Story` to any flow's context; subtree-inclusive knot-scope resolution in `ResolvedPolicy`. **F6.2** (bevy-only): `BrinkContext` `World` → `FlowLocal`; advance system builds `ContextView`; policy installed at plugin setup in base ⊕ overrides shape; delete drive loops + `commit_*` helpers. **F6.3** (bevy-only): per-entity `SaveState` durability. The language storage class is a separate later epic — issue filed, not started. Stale #441 items corrected: `apply_locale` calls STAY (locale overlay is unrelated to scoped state); `BrinkGlobals` already wraps `World`.
- **WHY:** Every F6 piece is invariant under the language feature landing — it changes only where the private-name list comes from. That's what delivers the value now without a rework or a default-flip later.

## #479 machinery/narrative folding: container-bounded runs, opt-in + gated
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** editor-ui / brink-ide
- **SCOPE:** moderate
- **WHAT:** (1) The fold unit stays run-based — interleaved narrative breaks machinery runs (single-region-per-container aggregation rejected: containers mix natures) — but runs are additionally bounded by projection container extents so they never cross weave boundaries. *Refined during implementation:* the bound covers **Choice/Gather containers only** — bounding by conditional/sequence branch containers would regress the #365 pure-routing case (scaffold + arms must fold as one region), and bounding by inline-construct containers would fragment the narrative run hosting them; both are pinned by regression tests. (2) machinery/narrative leave DEFAULT_ACTIVE_KINDS — hosts opt in via setActiveFoldKinds on mode entry. (3) Run computation is gated by a wasm session-level setting (`set_fold_runs_enabled`, off by default), so folding queries skip it unless the host activates it. (4) The embedding app wants both prose mode (collapse machinery) and logic mode (collapse narrative), built on foldAllOfKind.
- **WHY:** In practice the 2+-line runs produce gutter noise everywhere while the intended view modes were never host-wired; bounding by weave containers aligns folds with the rails, and opt-in + gating removes both the noise and the per-keystroke cost for hosts that don't use them.
## LineContext.has_tags is true for tagged choice lines
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** editor-ui (brink-ide line_context)
- **SCOPE:** minor/local
- **WHAT:** `LineContext.has_tags` must be true for any line carrying an author-written tag, including choice lines (`* Choice # tag`), regardless of tag region (start/bracket/inner). The historical suppression in the line_context walk is a bug to remove, not behavior to preserve.
- **WHY:** Verified in the C# reference implementation, not just docs: runtime `Choice.cs` carries `List<string> tags`, populated in `Story.ProcessChoice` by popping tags from the evaluated choice content streams — i.e., choice-line tags always surface at runtime, routed by region (both/choice-only/content-only). The oracle records per-choice tags and brink passes all choice-tag cases (tagsInChoice 4/4 episodes, tagsInChoiceDynamic, I100), so brink's compiler+runtime are already correct; the gap is editor-metadata-only, and HIR `Choice.tags` being empty is a modeling artifact of the (correct) slot distribution.

## Metadata-tag APIs: static tags in the format, dynamic tags via speculation
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** cross-system (compiler → format → runtime API / speculative eval)
- **SCOPE:** moderate
- **WHAT:** Brink will expose equivalents of the C# `globalTags` and `TagsForContentAtPath` — metadata tags declared at the top of the file and at the top of any flow container's body (knot, stitch, and function). **Statically visible tags are extracted at compile time into a metadata table in the format** (`brink-format`/StoryData), making the common case a simple table lookup. Because tag content can be dynamic inline ink (`# {character}_greeting.jpg`), full fidelity requires evaluation: **dynamic tags are computed by leveraging the speculative-execution mechanism**, not by scanning or mutating live story state. The APIs must be consistent: function-body tags retrievable regardless of parameter count (C#'s silent-empty for parameterized functions is a bug to fix, not replicate; C# also hard-errors on dynamic tags in these APIs — brink supports them via speculation).
- **WHY:** Reading metadata without visiting the flow is a real consumer need (studio, bevy-brink). The C# quirks are accidents of its naive runtime container scan; the format table sidesteps that class of bug for the static case, and speculation covers the dynamic case the C# API simply can't serve. Oracle-safe: queries are read-only and don't affect episode behavior; the one episode-observable behavior (a function's top-of-body tag leaking into the calling line's tags on inline calls) stays as-is for conformance.

## Annotations ride the tag channel; declarations marked via above-line directive tags
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** cross-system (syntax → compiler → format; language epic #473)
- **SCOPE:** architectural
- **WHAT:** The annotation mechanism for #473 (and future compiler directives) reuses ink tag syntax instead of a new sigil: any tag whose text begins with `@` (e.g. `#@local`) is a brink compiler directive — static text only, consumed at compile time and erased from runtime tag output. Directives use the standard ink tag placements (above-line, top-of-knot-body, top-of-file). For declarations (VAR/CONST/LIST/EXTERNAL), which cannot carry tags in ink, brink relaxes above-line attachment: a directive tag line above a declaration attaches to that declaration (option 2 — chosen over trailing tags on the declaration line).
- **WHY:** Reusing tag syntax gets the line/knot/file placement grammar for free and aligns with the #474 static-tag extraction pass; files remain parseable by inklecate (directives degrade to inert tags rather than parse errors); the corpus has zero `@`-prefixed tags, so existing behavior and the oracle ratchet are untouched by construction. Option 2 keeps files fully inklecate-parseable, and the trailing-tag form's extra targets aren't needed right now.

## Scripting-epic direction: #473 first, then query-substrate phase 0; conformance tail deprioritized
- **WHEN:** 2026-07-10
- **PROJECT:** brink
- **SYSTEM:** cross-system (compiler architecture; epics #473/#397/#460)
- **SCOPE:** architectural
- **WHAT:** Sequencing for growing toward the scripting layer: (1) #473 (flow-private storage class via directive annotations) ships first, as scoped, ahead of any restructuring. (2) The scripting epic's phase 0 is the shared substrate — symbol-service extraction, LIR logic/narrative split hygiene, query-shaped (salsa-style) pipeline, with `signature(def)` stubbed — serving scripting growth, incremental compilation (#460), and brink-ide from one restructuring. (3) Language growth is additive on top; the ink subset stays oracle-anchored, and the remaining conformance tail (14 failing cases) is deprioritized — 84% is accepted as the anchor, not a number to keep pushing.
- **WHY:** The prerequisite work for scripting growth is the same work as the parked incremental-compilation story and the IDE's query needs — doing it once while the layers are open beats retrofitting (this is the measurement change the F5 ruling anticipated). #473 is thin, independent, and ships the annotation mechanism that later carries type syntax.

## Phase-0 query engine: salsa, adopted coarse-grained
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** compiler architecture (#499 / #397 phase 0)
- **SCOPE:** architectural
- **WHAT:** The query-shaped pipeline restructuring builds on **salsa**, adopted at coarse granularity: inputs are file texts; queries are per-file/per-def functions returning the existing plain IR types behind `Arc` (`parse`, `hir`, `signature`, `resolve`, `lir`, `codegen`); the IRs themselves are NOT rewritten as tracked structs — the blast radius stays in the driver/analyzer orchestration. Early cutoff on `Eq` results provides the signature firewall automatically. A bespoke content-addressed memoization layer was considered and rejected.
- **WHY:** (1) The failure-mode asymmetry: bespoke invalidation bugs are silent stale artifacts (wrong compiles in the editor); salsa bugs are over-invalidation (perf only) — for a correctness-identity project the framework-owned invalidation wins. (2) Salsa's result-backdating subsumes the hand-built signature-firewall design for free. (3) The product direction includes live per-keystroke semantic tooling (type checker, inference hovers) at studio scale — exactly what fine-grained tracking is proven for (rust-analyzer, Astral's ty, both also shipping salsa-in-wasm). (4) The "engine-agnostic interface, swap later" hedge is illusory — salsa owns function signatures, so bespoke-then-salsa would be a rewrite. Residual risks accepted: framework API churn (pinned; now stably stewarded), wasm size/compile-time cost (to be measured via #498).

## Fine-grained salsa + IR rework is the eventual destination; coarse-grained is a stepping stone
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** compiler architecture (#499 / #397)
- **SCOPE:** architectural
- **STATUS:** tentative
- **WHAT:** The 2026-07-11 coarse-grained salsa ruling is phase-0 scope, not the end state. The expected long-term direction is fine-grained salsa integration — tracked structs, per-definition/field-level dependency granularity — including reworking the IRs (HIR/LIR) where that pays, as live semantic tooling grows. Phase-0 implication: query boundaries must not foreclose finer granularity — prefer def-keyed queries where cheap, keep per-def hashes in `hir`, and treat "IRs stay as-is" as a phase-0 constraint rather than a permanent principle.
- **WHY:** The live-tooling product direction (type checker, per-keystroke semantics at studio scale) will eventually justify field-level tracking; acknowledging the destination now keeps phase-0 choices from ossifying the coarse shape and makes the later migration incremental instead of a second restructuring.

## Tier-1 closures: env rows of val/ref captures
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** language design (#397 value-model round)
- **SCOPE:** architectural
- **WHAT:** Closures are values `{fn: DefinitionId token, env: row}`. Env entries are by-value snapshots (default) or explicit `ref` captures via capture-list syntax; `ref` captures are restricted to durable cells (compile error on temps). The env is analyzer-transparent (a typed row) but author-opaque (no reflection, indexing, or comparison). Effect-row variables bind at creation site, so every closure value carries a concrete effect row. Still open: cross-flow resolution of a ref-captured `#@local` cell (late binding through the executing flow's scope view proposed).
- **WHY:** Live capture where intended without the upvalue problem — no heap-promoted cells or identity objects; env rows serialize symbolically so closures live in saves and the journal; creation-site binding keeps closures fully transparent to the effects system (host callbacks carry knowable ECS access sets); explicit `ref` matches ink's existing aliasing spelling.

## Error handling v1: ink-side infallible; errors are host events
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** language design / runtime (#397 value-model round)
- **SCOPE:** architectural
- **WHAT:** v1 treats the script side as infallible: no exceptions, no unwinding, no in-language error values. Runtime faults (bad index, dead handle, invalid projection) are defined, deterministic outcomes — total operations with specified failure values where defined, otherwise turn-terminating diagnostic events surfaced to the host and recorded in the journal (replays fail identically). Result-shaped recoverable errors are a later, demand-driven addition that can join the effects system without grammar changes.
- **WHY:** Matches ink lineage; keeps journal/replay simple (errors are events, not control flow); hosts already own the player-facing error surface; "may fail" slots into effect rows later if recoverable errors ever arrive.

## Value-model round: closing ratifications
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** language design (#397 value-model round)
- **SCOPE:** architectural
- **WHAT:** (1) The model (data-is-values / identity-is-names) and the sharing-unobservable invariant are standing law. (2) Maps: insertion-order iteration; v1 key domain {int, string, bool}. (3) Projections: indices snapshot at ref creation; invalidation is a turn-terminating fault; overlaps resolve by immediate write-through order. (4) Host boundary: snapshot-only contract + Handle tokens + load-time rehydration hook (bevy EntityMapper-based) — ratified WITH the snapshot-economics analysis: crossings are O(1) Arc bumps; retained-snapshot memory is bounded at (retained generations + 1), never accumulating history; Arc::ptr_eq is available to hosts as a change-detection hint (host-side only — never script-visible semantics); the wasm boundary serializes and is exempt from snapshot economics. (5) The compiler-guarantees contract (spec §9) is standing law. (6) Effects: direction ratified — inferred rows internally, declared/frozen at entry points via the #@ channel; detailed design gets its own round before implementation. (7) Cross-flow ref-captured #@local cells: late binding through the executing flow's scope view. (8) v1 ships ptr_eq only; collapsing/sweeps stay specified-optional. Tier-1 build ordering: format schema on paper → runtime value core (hand-assembled-bytecode testable, oracle-inert) → single format bump riding the runtime PR → compiler surface last.
- **WHY:** Each follows from the ratified model and prior rulings; alternatives costed in docs/value-model-spec.md. The snapshot-economics discussion resolved the bevy memory/time concern (bounded divergence, lock-free parallel reads, free change detection); runtime-led ordering keeps every intermediate merge oracle-neutral by construction and validates the wire schema before it freezes.

## Wasm-observable behavior changes require a @brink-lang/web changeset
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** release process (crates ↔ npm trains)
- **SCOPE:** moderate
- **WHAT:** Any PR — including crates-only PRs — that changes behavior observable through the `@brink-lang/web` wasm surface (compileProject output, session APIs, diagnostics) must carry a `@brink-lang/web` patch changeset. Supersedes the implicit "crates-only PRs carry no changeset" convention (under which #527 shipped an observable change that would only reach npm on the next unrelated TS bump).
- **WHY:** The 0.9.1 saga established that consumer latency on behavior fixes hurts; the changelog should tell consumers the truth about behavior, not only about TS diffs. Enforced via pump house rules + CLAUDE.md.

## Structs: VERSION 4 reserves the surface; implementation rides the typed dialect
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** language design / format (#397 Tier-1)
- **SCOPE:** architectural
- **WHAT:** Brink will have structs — closed-shape records (`Value::Record`: flat field array + interned shape id), distinct from open-keyed maps — but not in Tier-1. Maps ship first (they subsume structs functionally); structs arrive with the typed dialect, where they get field checking and compile-time field offsets. VERSION 4 reserves their format surface now (value tag, shapes section, field-op opcode space) per the one-bump rule.
- **WHY:** All ratified value-model machinery (COW, serialization, collapsing, projections, sharing invariant) applies to records unchanged, so the only cost of "room for structs" is reserved identifiers — while retrofitting a value tag later would burn a format version. Positioning structs as the typed dialect's data-modeling story pairs them with the checker and the bevy derive boundary where they earn their keep.

## T1b surface rulings: strict-ink default, pure-logic blocks, sigil literals, lowercase stdlib
- **WHEN:** 2026-07-11
- **SYSTEM:** language design (#397 Tier-1, T1b round)
- **WHAT:** (1) Extension syntax is gated by a **superset grammar, analyzer-gated** dialect mechanism: brink-syntax always parses the full brink grammar; the analyzer rejects extension constructs under `strict-ink` with targeted diagnostics; the **default with no declaration is `strict-ink`**, and the oracle corpus/conformance CI pin strict explicitly. Dialect stays an authoring-time/tooling artifact (never in `.inkb`, never runtime-delivered), per the #368-round precedent. (2) Multi-line `~ { }` blocks are **pure logic only** in T1b: assignment, block-scoped `temp` (shadowing warns), if/else, while, for-in (values for arrays, insertion-order keys for maps), break/continue, return, calls — no text output, choices, diverts, tunnels, or threads inside a block; no weave concept may appear in `Expr`/`Stmt`. (3) Collection literals use **sigils**: `#[…]` arrays, `#{…}` maps, expression position only — prose position stays out of T1b because `#` opens a tag there (tags legally contain `{}` interpolation, so `#{…}` mid-prose is ambiguous); expression position is genuinely collision-free. (4) Stdlib slice 1 ships as **lowercase free functions** (len/keys/values/contains pure; push/insert/remove mutating) gated to the brink dialect; author definitions shadow builtins with a warning; **mutators require an lvalue** first argument and lower through the ratified RMW discipline; method-call syntax is ruled out for T1b (collides with dotted paths). Full surface + build sequencing in docs/t1b-surface-spec.md (also the roadmap's "strict-ink mode design note").
- **WHY:** One grammar keeps the IDE/formatter/diagnostics whole-language and makes strict-mode checking a cheap analysis-input variation under the salsa pipeline; defaulting strict keeps divergence from the C# oracle anchor a visible per-project choice, preserving the ratchet's meaning mechanically. The pure-logic fence is the crisp seam #397's sizing warned about — loosening later is additive, tightening is a break. Sigils make extension syntax self-identifying and collision-proof where it is legal, at the cost of familiarity (maintainer's call over the JSON-ish alternative). Lowercase free functions carry the Lua-class positioning; the lvalue-mutator rule keeps value semantics honest at the surface (no illusion of reference mutation on rvalues).

## `contains(map, needle)` is total; mutator arity mismatch is a compile error
- **WHEN:** 2026-07-12
- **PROJECT:** brink
- **SYSTEM:** brink-runtime, brink-ir (T1b stdlib slice 1, docs/t1b-surface-spec.md §5)
- **SCOPE:** moderate
- **WHAT:** (1) `contains(m, needle)` on a **map** with a `needle` outside the ratified key domain (int/string/bool — a float, array, map, …) returns `false`, not a runtime fault. This matches the **array** branch's existing totality (`contains(arr, needle)` is a structural-equality scan that already accepts any needle type) and value-model-spec §11c ("total operations with specified failure values where defined") — a needle that can never be a key simply isn't contained; there's no "the key isn't there" failure mode to escalate the way indexing/`MapGet` (§6) legitimately does for a present-but-wrong-typed key. Only the `MapContains` map-branch call site changed (`brink-runtime`'s `collection_ops.rs`); indexing/mutation faults are unchanged. (2) A collection mutator (`push`/`insert`/`remove`) called with the wrong argument count is now a **targeted, Error-severity compile error** (`E058`) naming the expected signature (e.g. `push(container, value)`), replacing the generic `E031` warning the arity check used to share with ordinary function-call arity checking. E031 never blocked compilation, so a malformed mutator call used to silently vanish from the lowered bytecode (the RMW lowering was dropped with no compile failure). Pure-function (knot/external) arity checking still uses E031, unchanged.
- **WHY:** (1) keeps `contains` behaviorally uniform across both container kinds — an author checking "is this in my map" shouldn't have to know the key domain to avoid a crash, exactly as they don't for arrays. (2) closes a silent-data-drop hole (CLAUDE.md "flag silent data drops" / "no debug_assert-only guards on reachable arms" — the equivalent principle for warning-only diagnostics guarding unreachable-in-bytecode statements): a warning that doesn't stop compilation but also doesn't lower anything is strictly worse than either a real error or real (if degraded) behavior.

## Stdlib totality rulings: contains() returns false on non-key needles; mutator arity is a compile error
- **WHEN:** 2026-07-12
- **SYSTEM:** language design (#397 Tier-1, T1b stdlib)
- **WHAT:** (1) `contains(m, needle)` with a needle outside the map key domain (float, collection, …) returns **false** instead of raising an `InvalidMapKeyType` fault — a value that can never be a key is simply not a member, making `contains` total on both branches like the array branch. Indexing/mutation key faults (spec §6) are unchanged; a static analyzer warning for statically-visible non-key needles is deferred (#582). (2) Wrong-arity calls to the stdlib mutators (`push`/`insert`/`remove`) are a **targeted compile error** naming the expected signature, replacing warning E031 + silently-dropped RMW lowering; pure-function arity behavior (warning + Null) is unchanged. Both shipped in PR #584 (#580, #581); this entry backfills the log requirement those issues carried.
- **WHY:** (1) value-model-spec §11c's total-operations principle, symmetry with the array branch, and removing a story-crashing footgun from a read-only membership test. (2) A mutator call that does nothing is never what the author meant, and unlike pure functions there is no fallback value to produce — silent no-op mutation violates the silent-drop rule.

## brink-fmt block formatting: blank-line-run collapsing ratified
- **WHEN:** 2026-07-12
- **SYSTEM:** brink-fmt (T1b formatter)
- **WHAT:** Inside `~ { }` blocks the formatter collapses runs of 2+ blank lines to one (positions preserved, counts not). Ratified as the intended reading of the #573 ruling's "preserved verbatim in place" — position is preserved verbatim; counts normalize to the whole-file formatter's existing one-blank-line norm. Exact-count preservation, if ever wanted, becomes a #592 config knob, not the default.
- **WHY:** Consistency with the rest of the formatter and with common fmt-tool behavior; resolves the literal-wording ambiguity PR #602's review flagged before future fmt/IDE work builds on the block renderer.

## Language-facility doctrine: standard functions vs intrinsics vs syntax
- **WHEN:** 2026-07-12
- **SYSTEM:** language design (#397 Tier-1)
- **WHAT:** (1) **Ladder rule** — prefer a standard facility when a signature can describe it; an intrinsic when semantics need VM/lowering knowledge but the call shape stays honest (arguments are evaluated values, no hidden binding); syntax only for binding-time access (names-as-values, lvalues/cells, static signatures), new binding structure, evaluation-order control, or lexical recognizability (dialect gate, formatter). (2) **Rules attach to resolved definitions, never names** — author shadowing stays uniform (the T1b stdlib ruling stands; no reserved names); an intrinsic's special typing/lowering rule applies only when resolution lands on the builtin. (3) **Types are gradual-capable, effects are conservative-total** — an intrinsic's typing rule is deferrable DX (`Unknown` until it lands); effect rows are never omittable, only pessimized. (4) **Every new intrinsic states, at introduction, its typing rule (or "Unknown until slice N") and its effect row.**
- **WHY:** T1b already created intrinsics (stdlib with the lvalue rule, VM-native lowering) and syntax (sigil literals, blocks); this makes the placement criteria explicit before T1c mints more of both, and keeps the "shadow grammar" of special rules visible and taxed.

## T1c rulings: function values are partial application, not closures
- **WHEN:** 2026-07-12
- **SYSTEM:** language design (#397 T1c round)
- **WHAT:** (1) The T1c feature is **partial application over named functions** — a function value is `{fn token, bound-arg row, effect row}`; the author-facing name is "function values / partial application," not "closures" (no lexical environment exists to close over). (2) Creation is syntax: `#fn(name, args…)` binds a **prefix** of the declared params; capture mode comes from the target's own signature (`val` snapshots now, `ref` captures a durable cell) — no new capture markers. (3) **All `ref` params must be bound at creation**, where the target is statically named — thereafter every function value's remaining params are `val`-only, so dynamic surfaces are values-only. (4) `bind(f, args…)` is a **stdlib intrinsic** for val-only currying over existing function values (typing rule: consumes the head of the param row; `Unknown` until the checker lands; effect-transparent — copies the value's row; ordinarily shadowable per the doctrine). (5) Both call forms ship: direct `f(args)` (dynamic dispatch with runtime type/arity fault) and `call(f, args…)`. (6) Cross-flow invocation of a closure that `ref`-binds a `#@local` is a **turn-terminating fault** in T1c; the ruled destination is creating-flow identity (#597); late binding is explicitly not the direction. (7) Host boundary: function values cross as opaque tokens `{DefinitionId, env}`; invocation always re-enters the VM and is journaled — the host never dereferences the env. (8) Effect-row field exists on the value from day one (creation-site binding rule), populated trivially until T2. (9) No anonymous lambdas in Tier-1. **The T1c spec draft is held** pending the strict-typed-mode epic's (#605) first ruling round, since `FnValue` typing shapes `#fn`'s surface.
- **WHY:** Ink has no lexical free variables — partial application (JS `Function.bind` with signature-driven ref modes) covers the callback/state-machine/parameterized-behavior use cases without inventing a scoping construct, preserves the signature firewall, and makes every static obligation (name→value, ref binding, effect binding) happen at one marked site. Refs-bound-at-creation dissolves the dynamic-call lowering ambiguity and the host-boundary ref question simultaneously.

## Typed-mode rulings (#605 round): boundaries-only annotations, structs ride along, inline syntax, types-first sequencing
- **WHEN:** 2026-07-12
- **SYSTEM:** language design (#605 strict typed mode epic)
- **WHAT:** (1) Strict mode requires type annotations at **boundaries only** (host-callable functions, entry points, `#fn` targets crossing to the engine — where effect rows freeze); everything else is inferred via monomorphic HM per call-graph SCC, params-from-body-uses, call-site inference forbidden (signature firewall preserved). Accepted costs on the record: inferred-signature ripple on body edits, caller-site surfacing of body bugs. (2) **Structs land with strict mode** (TM-4) — the heterogeneity errors strict introduces get their intended answer immediately. (3) **Inline type syntax** (`heal(ref hp: int, amount: int): int`), brink-dialect-gated — revises #473: the `#@` channel keeps its other tenants but never carries types; one way to write a type. (4) **Sequencing: types → T1c → effects** — milestone TM (typed mode, slices TM-1 substrate / TM-2 syntax / TM-3 strict policy / TM-4 structs / TM-5 tail) inserts between T1b and T1c; T1c unfreezes with FnValue types from day one; the T2 effects round follows, reusing the SCC fixpoint skeleton. Full surface in docs/typed-mode-spec.md (PROPOSED sections ratify at the spec PR review).
- **WHY:** Boundaries-only mirrors the effects freeze rule — one mental model, annotations exactly where interfaces should be documented anyway; mono-HM over brink's small nominal universe with no overloading/traits stays near-linear (the Rust-signature rule was traits-driven and the Swift blowup ingredients are absent); inline syntax is justified under the facility doctrine once strict mode makes signatures routine; types-first proves the shared inference machinery on the self-contained problem before effects and function-value types build on it.

## Struct declaration syntax amended: the decl body mirrors the construction literal
- **WHEN:** 2026-07-13
- **SYSTEM:** language design (#605 typed mode, TM-4 surface)
- **WHAT:** `STRUCT Point = #{ x: float, y: float }` — the declaration keeps ink's `NAME = …` convention and its body takes the same braced `#{…}` shape as the construction literal (`Point#{x: 1.0, y: 2.0}`), with types in value position. Amends the flat comma-list form (`STRUCT Point = x: float, y: float`) ratified hours earlier in PR #607, before any TM-4 implementation existed. Multiline + trailing comma; brink-fmt formats bodies like blocks.
- **WHY:** Declaration and usage should rhyme — the flat form read like nothing else in the language and degraded past a few fields. Zero cost: caught pre-implementation.

## Salsa fine-grained migration promoted: tentative destination → committed before the epic closes
- **WHEN:** 2026-07-13
- **SYSTEM:** compiler architecture (#397 / #623)
- **WHAT:** The fine-grained salsa trajectory (tracked structs, per-def query granularity, symbolic-ref codegen linking — the phase-0 #499 ruling's "tentative destination") is **committed: required before the #397 epic is done**. Measurement now governs *sequencing only*: the trigger is TM-1's (#617) compile-bench report vs the #498 baseline — immediate scheduling if inference already moves warm ide-reanalyze materially; otherwise no later than before the T2 effects implementation, which must not ship per-def effect rows on the coarse substrate. Epic filed as #623 (design round required); sweeps up #517 and the incremental==from-scratch harness extension to new per-def query families.
- **WHY:** The typed-mode query family (and effects after it) is inherently per-def; running it indefinitely on coarse memoization wastes the architecture the phase-0 substrate was built to enable. Committing now lets TM slices make locally-correct choices (lazy inference, per-def shapes) knowing the granularity destination instead of hedging.

## Fine-grained salsa rulings (#623 round): SCC batching, Arc<plain>, inference-first, codegen link deferred
- **WHEN:** 2026-07-13
- **SYSTEM:** compiler architecture (#623, docs/fine-grained-salsa-proposal.md)
- **WHAT:** (1) **SCC solving = lifted batching**: scc_membership()/solve_scc(SccId) derived queries lifting TM-1's landed solver (reachability SCCs + Kahn ordering + bounded fixpoint); salsa never sees a cycle; the native cycle_fn/cycle_initial path is explicitly declined for v1 (order-sensitive Unknown-absorption per #627 makes in-salsa fixpoints a determinism risk; zero prior art in-repo/wasm) — recorded as a contained, consumer-invisible migration if ever measured worthwhile, since inferred_signature(def)'s shape is identical under both. (2) **Storage = Arc<plain> per-def-keyed queries**; salsa tracked structs declined for v1 (identity-layer rework vs DefinitionId for marginal per-field benefit) and stay available later behind the same APIs. (3) **Migration order = family-by-family, inference-first** while inference is consumer-free (window closes at TM-2); FG-1 bundles the signature_query per-file dependency fix AND re-sourcing inference's (index, resolutions) off analysis_query's almost-never-backdating memo. (4) **Symbolic-ref codegen linking defers to FG-4** — the #460 win, orthogonal to the type-system shape TM-2 needs; per-def LIR/HIR decoupling arrives there. Slices: FG-1 input fixes → FG-2 per-def/per-SCC decomposition + the missing inferred_signature(def) API → FG-3 analysis_query split → FG-4 symbolic-ref link. FG-1/FG-2 run before TM-2 per the #625 ratification.
- **WHY:** Batching is the honest expression of the math (an SCC's signatures are one joint computation) and preserves 37 landed tests; Arc<plain> buys designed-cutoff granularity with zero identity risk; inference-first exploits the consumer-free window and unblocks TM-2 against the final shape; both contested forks are the reversible options.

## TM-2 rulings (#618 / PR #640): E063 stays opt-in until TM-3; strict-ink suppresses annotation content checks
- **WHEN:** 2026-07-13
- **SYSTEM:** language design / compiler diagnostics (#605 typed mode, TM-2)
- **WHAT:** (1) **E063 (annotation-vs-body-inference mismatch) ships opt-in**: `annotation_mismatches` is a public analyzer function for consumers that already hold an `InferenceResult` (future hover, TM-3), NOT auto-invoked in `finish_analysis` — auto-wiring would force whole-project inference on every analysis, retiring TM-1's documented laziness invariant ahead of schedule. TM-3's strict-policy wiring (which must run inference anyway) is where E063 starts firing in production; #619 inherits that obligation explicitly. #618 still closes with TM-2: the diagnostic, its semantics, and its tests exist — invocation policy is what's deferred. (2) **Under `strict-ink`, annotation content checks (E061 unknown type name / E062 fn-reserved) are suppressed**: the dialect gate already rejects the annotation whole (E051), and stacking content diagnostics on rejected syntax is noise. Content checks run only under `dialect = brink` (`finish_analysis` gates the call).
- **WHY:** (1) Preserving the laziness invariant until the slice that genuinely needs inference keeps TM-1/FG guarantees intact and avoids paying the #638 whole-project re-execution gap on every IDE keystroke for a diagnostic gradual mode treats as advisory anyway. (2) One actionable diagnostic per span: the strict-ink author's fix is to remove the annotation or switch dialects — critiquing the inside of syntax the project rejects helps no one.

## TM-3 design note resolved: `Ty::Conflicted` lattice point added to `unify()`
- **WHEN:** 2026-07-13
- **SYSTEM:** language design #605/TM-3, compiler type inference (`brink-analyzer`, #627)
- **WHAT:** `unify()` gains a distinct absorbing lattice point, `Ty::Conflicted`: unifying any two genuinely disjoint concrete types (e.g. `int`/`string`) now yields `Ty::Conflicted` instead of degrading to `Ty::Unknown`, and `Conflicted` joined with anything — including `Unknown` — stays `Conflicted` (`Conflicted ⊔ x = Conflicted`). The join stays monotone and the SCC fixpoint (`MAX_SCC_ITERATIONS`) is unaffected — proven end to end with a mutually-recursive fixture, not just at the `unify` unit level. Gradual/advisory consumers (`signature()`'s `Sig::value_type` downcast, E063's `annotation_mismatches`) read `Conflicted` exactly like `Unknown` via the new `Ty::is_unresolved()` helper — zero behavior change for every TM-1/TM-2 consumer that exists today. The TM-1 determinism/property tests are extended with permuted-order fixtures (`conflict_detection_is_order_independent_across_real_source` and the `unify_all` property tests in `ty.rs`) proving conflict detection no longer depends on declaration/observation order — the exact bug this issue exists to close: pre-#627, `observe()`'s `is_unknown()` short-circuit meant whichever concrete type was observed *last* silently won, masking the conflict entirely depending on source order. Strict-mode reporting of conflicted slots at their definition (TM-3 proper) is explicitly out of scope here — that's #619's job; this PR lands the lattice point and its plumbing only.
- **WHY:** A strict mode that provably misses int/string conflicts depending on declaration order is a weaker promise than the spec makes, and retrofitting the lattice after TM-3 consumers exist would re-touch every unify call site and every gradual/advisory reader of `Ty`. Landing the absorbing point now, while it's a no-op for every existing consumer, means TM-3 (#619) only has to add reporting, not redesign the lattice underneath it. Correctness above all.

## FG-2.1 rulings (#638 round): pre-scan read-set, full narrowing, before TM-3, one PR
- **WHEN:** 2026-07-13
- **SYSTEM:** compiler architecture (#623 FG spine, #638, docs/fg2-1-lazy-globals-spec.md)
- **WHAT:** (1) **Lazy-globals seam = pre-scan + narrow map**, specified as a reusable per-def body-facts projection (`referenced_globals(def)`, the `call_edges(def)` family): brink-analyzer stays pure; the scan doubles as the per-def global **read-set** that T2 effect rows will carry (global reads feed purity checking, replay invalidation, host-boundary freezing — the maintainer's framing at the round). The salsa-reentrant resolver-callback option is declined. (2) **Full narrowing**: FG-2.1 also shrinks `solve_scc`'s HIR input to SCC-members' declaring files via per-def projection queries (`def_body_query`, `inferable_defs_query`) and drops pass-1's useless globals — globals-only would leave the all-files edge firing one step upstream. (3) **Sequencing: FG-2.1 lands before TM-3** (the FG-1/FG-2-before-TM-2 shape-locking principle; the consumer-free window is still open because E063 shipped opt-in), after #626. (4) **One behavior-neutral PR** (the FG-2 precedent), gated by composed-equals-monolithic, incremental-fuzz equals-fresh with a new VAR/CONST-initializer edit mutation, and a pointer-identity NON-re-execution test (the FG-1 analogue). Ratified: FG-2 as merged is structure+value-cutoff only; callable `known_sigs` stays eager in the fixpoint working set (callable laziness would need the declined native-cycle path).
- **WHY:** The pre-scan turns a perf refactor into effects groundwork — one walk shape serves both eras; full narrowing is the only version that moves warm reanalyze once TM-3 gives inference a per-edit consumer; before-TM-3 lets strict mode wire against the final dependency graph and makes its bench report meaningful.

## TM-3 completion rulings (#659 round): int()/float()/string() conversion intrinsics
- **WHEN:** 2026-07-13
- **SYSTEM:** language design (#605 typed mode, TM-3 completion, issue #659)
- **WHAT:** (1) **Parse failure is a turn-terminating fault** (value-model §11c pattern): `int("potato")` / `float("x")` fault like a missing map key — no zero-defaulting, no silent garbage. This is a new, distinct fault-carrying path; the pre-existing uppercase `INT()`/`FLOAT()` builtins keep their legacy silent-0-on-failure behavior untouched (oracle-anchored, byte-identical). (2) **Domains — permissive numerics + bool**: `int()` accepts float (truncate), string (parse), bool (`true`→1), int (identity); `float()` accepts int (widen), string (parse), bool (`true`→1.0), float (identity); `string()` accepts everything (display form, same as interpolation — per spec §4, total, never faults). Divert/list/collection/struct inputs to `int`/`float` are a compile error (`E078`) under `types = strict` when statically classifiable, and a runtime fault (`InvalidConversionDomain`) otherwise (including under `types = gradual`). (3) **float→int truncates toward zero**, matching vanilla ink's `INT()` exactly — `int(2.9) = 2`, `int(-2.9) = -2` — so the two functions never disagree and the C# oracle precedent carries over. Implementation: VM-native ops (`ConvertInt`/`ConvertFloat`/`ConvertString`, opcodes `0xD3`-`0xD5`), lowercase names (`int`/`float`/`string`), author-shadowable with a warning (`E035`) per the T1b stdlib-slice-1 ruling (`is_t1b_stdlib_name`), intrinsic typing rules declared at introduction (`infer_intrinsic`: fixed return types `Int`/`Float`/`String`, per the facility doctrine).
- **WHY:** Strict mode's §4 "everything else explicit" rule left strict-mode authors with no way to convert between `int`/`float`/`string` — a usability hole once anyone actually authors under `types = strict`. Reusing the stdlib-slice-1 machinery (VM-native, shadowable, dialect-gated) avoids inventing a second intrinsic-dispatch mechanism; matching vanilla ink's `INT()` truncation exactly keeps the oracle comparison meaningful for any story that happens to exercise both; a turn-terminating fault (rather than silent 0) is consistent with the value model's existing "no silent garbage" posture for out-of-domain runtime failures.

## T1c spec rulings (draft round): strict static call checking, display form, save rehydration, effect placeholder
- **WHEN:** 2026-07-13
- **SYSTEM:** language design (#397 T1c spec round)
- **WHAT:** (1) Under `types = strict`, calls through function values are statically checked: known `fn(T…): R` mismatches are compile errors, `Unknown`/`Conflicted` callees are escape errors — the ruled runtime type/arity fault becomes gradual-mode-only; boundary annotations gain the `fn(T…): R` form so fn-typed params can cross boundaries under strict. (2) `string(f)` stays total; display form is signature-like with bound args rendered as defaults, e.g. `fn heal(hp = 1)`. (3) Fn values save like every other value (no serialization carve-out — pause/resume must hold); the V4 named-env encoding is the best-effort cross-version validation, and a param name/mode mismatch on load or invoke is a defined fault, never silent misbinding. A policy lint against long-lived fn values is possible later, not a value-model rule. (4) The effect-row field ships from day one populated with the pessimal touches-everything row — space reserved, semantics open until T2.
- **WHY:** (1) A type checker that doesn't check calls is noise — strict authors should never see the dynamic-dispatch fault. (2) Total `string()` was already ruled; a boring stable signature form is the least-regrettable observable surface. (3) Cross-version loads have no real guarantee; best-effort validation + clean fault beats both silent garbage and breaking the serializability invariant. (4) Pre-allocating effect space keeps old saves conservatively readable by future effect-aware code.

## Parallel agent dispatch runs through the autonomous-pump process
- **WHEN:** 2026-07-13
- **SYSTEM:** process
- **WHAT:** Multi-agent parallel work uses the vendored autonomous-pump skill (.claude/skills/autonomous-pump + BRINK-CONFIG.md) rather than ad-hoc Agent-tool dispatch — waves get the full process: manifest, watcher, merge train, wave-boundary reconciliation. Single one-off agents may still be dispatched directly.
- **WHY:** The pump process is where the operational lessons live (foreground gates, stall detection, sequential merge train, reconciliation of findings into issues); ad-hoc dispatch reproduces the briefs but silently drops the process safety net.

## Struct construction literals: source-order evaluation, duplicate field is a compile error
- **WHEN:** 2026-07-14
- **SYSTEM:** language design (#605 typed mode / TM-4 surface; issues #675/#676)
- **WHAT:** (1) Construction-literal initializers (`Point#{y: g(), x: f()}`) evaluate in **source order** (left-to-right as written), not shape-declaration order; codegen reorders values into shape offsets AFTER evaluation. (2) A duplicate field in a construction literal is a **compile error** (next free E-code), replacing the current silent drop of the first initializer's side effect.
- **WHY:** Left-to-right matches every other expression's evaluation order in the language — least author surprise, and side-effect order stays what the author wrote; shape order is a memory-layout concern that belongs to codegen, not semantics. Duplicate-field-errors is the no-silent-garbage posture applied at compile time.

## LSP diagnostics race fixed with a single serialized publisher; tokio confined to the lsp binary
- **WHEN:** 2026-07-14
- **PROJECT:** brink
- **SYSTEM:** brink-lsp
- **SCOPE:** moderate
- **WHAT:** The did_open/change/save per-file diagnostics publish and the background analysis_loop publish are unified behind one serialized publisher that owns decide→send→record atomically, tagging each publish with (generation, tier: PerFile|Analysis) and applying a monotone anti-downgrade rule (newer generation wins; Analysis breaks ties; a PerFile never overwrites a same-or-newer Analysis). The dead `generation` atomic is revived to carry the ordering. Plumbing is an async (tokio::Mutex) publish fn rather than a dedicated task/mpsc — acceptable specifically because it lives entirely in the brink-lsp binary, which is a leaf crate already built on tokio/tower-lsp and never compiled to wasm.
- **WHY:** The bug is a wire send-ordering race across two tasks, so cache-record-only fixes can still strand the client on the parse-only subset — only a single owner of both send and record closes it. tokio primitives are fine here because brink-lsp is not in the wasm path; the same pattern would NOT be acceptable in the lower plumbing (brink-db/brink-ir/brink-analyzer) that brink-web compiles to wasm, which is the standing constraint worth recording.

## Modules & visibility rulings (#719 round)
- **WHEN:** 2026-07-14
- **SYSTEM:** language design (#719 modules round)
- **WHAT:** (1) **Module unit**: file-as-module by default (stem = name); optional `#@module(name)` header declares/names it; several files may share one module (same declaration, or INCLUDE-glue under a declaring head — included files inherit module AND default). (2) **Imports**: names cross module boundaries ONLY via import — `IMPORT { a, b AS c } FROM mod` (bare use) and `IMPORT mod` (qualified `mod.knot.stitch`); importable = all top-level public defs (knots, functions, VAR/CONST/LIST/STRUCT); stitches only via qualified form; NO globs; module-name-vs-knot ambiguity in qualified position = compile error (fix with alias). IDE auto-import rides the existing INCLUDE-rewrite machinery (quick-fix insert, completion-with-edit, rename coherence). (3) **INCLUDE splits roles**: intra-module file glue with today's flat semantics (100% compat, both dialects, ungated); IMPORT/#@module/#@private/#@public are brink-gated. (4) **Visibility × residence are orthogonal axes** sharing the directive channel: visibility (public/`#@private`) = who may reference the name; residence (world/`#@local`) = which instance owns state; all four combinations legal; knots get both (per-flow visit counts). Boundary rules: `#@private` hides the NAME not the CELL (private world-cells still ship in effect rows — real scheduler conflicts); host SEMANTIC access respects visibility, host PERSISTENCE machinery sees everything (saves/journal/replay — pause/resume must hold). (5) **Declaration flips the default**: explicitly declared modules default PRIVATE (opt-in to encapsulation; `#@public` overrides); undeclared stem-modules default PUBLIC (ink compat; `#@private` overrides). Undeclared-file stem colliding with a declared module name = compile error. Dev tooling gets a documented visibility override (play-from-here); production hosts respect visibility. (6) **Renames via `#@was(old_name)`**: compiler ships an old→new DefinitionId alias table in .inkb (compiled-declarations pattern, section-locally versioned); rehydration consults it on the miss path; the fault message teaches the fix; IDE rename writes the directive automatically; directive is deletable after a migration window. Applies to knots too — fixes the PRE-EXISTING silent save-break on knot renames. Identity stays name-hashed: (module, name); imports/aliases are consumer-side and never affect identity. Rejected: content-hashed identity, permanent GUIDs, fuzzy load-time rematching.
- **WHY:** Imports make cross-module coupling explicit and give the compiler a declared dependency graph (incremental edges, capability audits, #717's import table) while auto-import removes the ceremony; the declaration-flips-default rule makes encapsulation a single deliberate gesture with zero legacy breakage; name-not-cell and semantic-vs-persistence keep the two axes from leaking; #@was turns renames from silent identity breaks into recorded, deletable migration events using machinery (#@ channel, compiled declarations, rename tooling) that already exists.
## Handle rehydration mechanics + handle-parameterized capability atoms
- **WHEN:** 2026-07-14
- **SYSTEM:** language design / bevy-brink (T1d round follow-up, T2 amendment)
- **WHAT:** (1) **Rehydration is two-halved**: save-side keying (live resource → durable SaveKey) + load-side resolution (SaveKey → new resource) via one per-kind trait (KIND name, Resource, SaveKey: Serialize, save_key → Option, resolve → Option). SaveKey is a **reconstruction recipe, not just a foreign key** — a timer saves its remaining duration and resolves by spawning anew; ephemerality (save_key → None) is a per-kind CHOICE, never a spec-assigned category. bevy-brink owns opaque token ids + registries, persists token→SaveKey beside SaveState, rebinds registries at load keeping token ids stable; EntityMapper integrates for scene-based games. (2) **Never-fail-load stays the invariant** (player saves are sacred), refined: load produces a rehydration REPORT (rebound / dead-by-resolve / dead-ephemeral / dead-by-UNREGISTERED-KIND) + host policy knob Lenient (prod) vs StrictKinds (dev/CI fails loud on unregistered kinds — integration drift caught at the desk); optional per-binding dead-deref host event for telemetry. Replay unaffected (journal replays results). (3) **Registry GC at quiescent points**: script state is enumerable, so the host computes the live token set at -> DONE sweeps and drops unreachable registry entries — no script-side destructors needed. (4) **Capability atoms may be handle-parameterized** (→ effects-spec amendment): rows can carry Transform(@argN)-style atoms; the factored wire encoding RESERVES the parameter slot now; T2 v1 populates everything as (any) — component-granular, today's design; instance resolution ships later as a narrowing rung (token comparison at schedule-commit — the existing selection machinery). Reactive sleep gains per-entity subscriptions; parallel scheduling gains token-disjointness; **possession-bounded capabilities recorded as the tier-2 security model** (handles are true ocap tokens — no literal syntax, only bindings mint them). Reverses the earlier "binding's declared access, nothing more" position (a v1 simplification, not a principle).
- **WHY:** The naive rehydrate(id) hook assumed hosts can interpret opaque ids — splitting keying from resolution matches where knowledge actually lives and makes ephemeral-vs-resumable the implementor's one-line choice; the report/policy split protects players from failure and developers from silence; parameterized atoms are what make the flagship effects consumers (per-entity ambient flows) actually cheap, cost nothing until populated, and were nearly foreclosed by an unexamined "nothing more" ruling — the flat-rows lesson applied again.
## Drop the converter pipeline before VERSION 4
- **WHEN:** 2026-07-11
- **PROJECT:** brink
- **SYSTEM:** cross-system (converter / format / test-harness)
- **SCOPE:** architectural
- **WHAT:** brink-converter, brink-json, and CLI `.ink.json` ingestion are retired before the V4 format work begins. Runtime/harness tests migrate to compiler-built fixtures. The oracle (C# runtime episodes) remains the sole correctness anchor; the converter leaves the trust hierarchy. Resurrectable from git history pinned at VERSION 3 if conformance debugging ever demands it.
- **WHY:** Correctness was always anchored by oracle episodes, not converter output; the structural-diff debugging role is deprioritized with the conformance tail; and post-Tier-1 the converter can only ever cover the legacy subset while charging double implementation cost on every format change — starting with V4's literal-pool fold. Dropping it before V4 means the value core never pays that tax.

## Cross-module name collisions: hard-error stopgap now, import-scoped resolution committed
- **WHEN:** 2026-07-14
- **SYSTEM:** language design (#719 modules, issue #784)
- **WHAT:** (1) M-2c's diagnostic-over-flat-resolution is NOT acceptable as shipped semantics: duplicate knot names are only an inklecate-compat warning (E022), so a compiling program can `IMPORT { ambush } FROM quest_a` and silently bind quest_b's `ambush` via the flat duplicate-winner — reachable silent wrong-binding contradicting spec §2. (2) **Stopgap (immediate)**: cross-DECLARED-module same-name duplicates escalate from warning to hard error under `dialect = brink`; within-module duplicates keep the compat warning. Unique cross-module names make the flat winner the only candidate, so imports bind correctly by construction. (3) **Endgame (committed, before the modules milestone closes)**: true import-scoped resolution through resolve.rs's lookup sites (byte-identical strict-ink/tier1 gate), which then RELAXES the stopgap — same-name coexistence arrives as a feature unlocked by correct machinery.
- **WHY:** Silent misbinding is the no-silent-garbage class; the stopgap is one check extension and makes the unsound world unreachable at the mild, temporary cost of cross-module name uniqueness; rushing the delicate resolution threading under wave pressure is how byte-identity guarantees get broken.

## FG-5 memory bounding: runaway ceilings only — steady-state LRU rejected by data
- **WHEN:** 2026-07-15
- **SYSTEM:** compiler architecture (#623 FG-5, issues #537/#647)
- **WHAT:** No steady-state LRU capacities are set on the salsa query families. The #537 measurement (issue comment, 2026-07-15) showed memo counts are pure functions of project size with ZERO session-length growth (RSS drift ≤1.4 MB over 1,500 edits; the #542 churn fix holds at scale), so tight capacities would only cause recompute churn on a working set that is legitimately the whole project. Instead: (1) per-family absolute count ceilings as runaway guards (~4,096 per-file, ~16,384 per-def), never evicting in steady state; eviction unit = salsa's memo row; (2) #538 heap_size estimators target def_body/solve_scc/signature/infer_body/lowered (Arc payloads ~5× salsa-visible bytes); (3) #647 re-scoped to exactly that; #537 closed answered-by-data; the bench probe gap filed as #819.
- **WHY:** Bounding what doesn't grow is a policy in search of a problem; ceilings guard the pathological case without taxing the normal one, and the measurement is honest about its own instrument (per-def families needed a probe the stock bench lacks).

## T1e ratified as current posture; narrowings recorded as exploration intents
- **WHEN:** 2026-07-15
- **SYSTEM:** language design (#397 T1e, PR #818)
- **WHAT:** docs/t1e-spec.md ratified. The two judgment calls are **current posture, not permanent**: (1) projections exist only in ref-argument position (`heal(ref npc.hp, 5)`, `#fn` binding) — no standalone `temp r = ref a[0]`; first-class projection values recorded as icebox #825 (maintainer wants it explored; opening it is a design round on reference-value aliasing/lifetime). (2) Effect rows carry the projection's root cell only; path granularity uses the already-reserved capability-parameter slot when explored — icebox #826, triggered by measurable struct-field contention (#821 can detect). Also from the same session's corpus research: the missing vector type recorded as icebox #827 (float-math ergonomics; evidence via the #822 boids/steering/SAT ports). Display/equality/diagnostics/sequencing PROPOSED items ratified as written.
- **WHY:** Narrow-now-widen-later is additive in both cases (a new syntax position; a reserved encoding slot), while the reverse is a break; recording the exploration intent as icebox issues keeps "posture" from silently hardening into "doctrine" — the maintainer explicitly wants both revisited.

## Vector math types: structs-first; sequence slices/ranges endorsed as range-projections
- **WHEN:** 2026-07-15
- **SYSTEM:** language design (post-Tier-1 directions; iceboxes #827/#829)
- **WHAT:** (1) **vec2/vec3-class math types are structs-first**: the struct feature (+ the future methods round — shape-attached `v.dot(w)`) should account for them; stdlib-over-structs is the middle rung; a native vector type only if the #822 float-trio ports prove structs+methods can't reach acceptable ergonomics (facility-doctrine ladder). (2) **Sequence slices/ranges are an endorsed future addition** (upgraded from t1b's mere exclusion): a slice-as-view is a range-segment projection, so VAL_PROJECTION reserves segment kind `2=range (start,end)` now — never emitted in T1e; design round at #829 decides value-vs-view semantics, invalidation, and iteration.
- **WHY:** Vectors are the canonical test of whether structs+methods carry their weight — reaching for a native type before that evidence exists would skip the ladder; reserving the range segment kind costs one enum value and prevents the wire from foreclosing slices, the same one-byte foresight that saved narrowing, records, and effect parameters.

## Single compile pipeline: the legacy one-shot path is removed
- **WHEN:** 2026-07-15
- **SYSTEM:** compiler architecture (#623 FG epic; FG-6)
- **WHAT:** brink-compiler's direct one-shot route (driver → lower_to_program → codegen, bypassing salsa) is REMOVED: every consumer — CLI, oracle harness, compile_bench, brink-web — compiles through ProjectDb (create db → set files → pull story_data). The monolithic lowering entry becomes internal to the query implementation or is deleted where the chunk composition supersedes it. The composed-equals-monolithic equivalence family retires in favor of cross-version byte-identity (inkb_hashes). compile_bench measures the real pipeline by construction; its cold numbers re-baseline (the phase-0 fixed db-setup cost, ~+24% on the pathological batch shape, becomes universal — accepted; re-measure at real-project shape and report honestly). #838 narrows to its incremental mode. Filed as FG-6, the epic's true closing slice.
- **WHY:** Two pipelines = double maintenance, drift risk, and the thrice-recurred instrument-mismatch class (benches measuring the path nobody optimizes); a query engine you built and don't route production through is decoration. The ratchet gate itself compiles through the switched path, so the migration is proven by the strictest instrument in the repo.

## FG epic closes on the honest reanalyze win; incremental assembly deferred post-T2
- **WHEN:** 2026-07-15
- **SYSTEM:** compiler architecture (#623 FG epic)
- **WHAT:** The fine-grained-salsa epic (#623) is judged DELIVERED on its interactive-latency goal — IDE reanalyze −30/−41% (phase 0, partial-pull path) — and closes accepting the honestly-measured #838 finding that warm full-`story_data` recompile is ~35% SLOWER (assembling a complete linked artifact is intrinsically O(total) per re-pull; per-knot memoization saves re-lowering but not assembly, and adds tracking overhead). Rationale for acceptance: full-artifact recompile is a build/on-save operation, not an interactive path; the interactive path (reanalyze/diagnostics) is the one that won. **Incremental artifact assembly / per-container hot-reload** (splice a recompiled chunk into a live Program rather than re-link all N) is WANTED but scheduled AFTER T2 — filed as its own post-T2 design issue; it is the correct fix for when full-recompile latency matters (hot-reload into a running game), and converges with the runtime linker + #717 dynamic linking per the three-moments appendix. #623 closes when FG-6 (#841) cleanup lands; incremental assembly is explicitly OUT of the epic.
- **WHY:** Measuring honestly beat the number going up: the epic's real prize (typed per-def inference on a fine-grained substrate, and interactive reanalyze latency) was delivered, while the one regressed workload is non-interactive and has a known, differently-shaped fix that belongs with hot-reload, not bolted onto this epic. Correctness/honesty above making the bench green.

## FG-6 cleanup tail (#841): both deferred deletions declined after audit; docs corrected instead
- **WHEN:** 2026-07-15
- **SYSTEM:** compiler architecture (#623 FG epic; FG-6 closing slice)
- **WHAT:** #841's two PR #844 follow-ons were re-audited, not blindly executed. (1) `lower_to_program`/`lower_to_program_with_type_mode` (`brink-ir`) stay `pub`, not narrowed to `pub(crate)` or deleted: a fresh external-caller grep confirms `brink-db`'s production link phase (`lir_lowering_query`) truly no longer calls them (FG-4c/d/e's chunk composition supersedes that use), but two other external crates still call them deliberately — `compile_bench`'s legacy-path benchmark rows (the explicit baseline the `ProjectDb`-driven row is measured against) and `golden_i078.rs` (a golden pipeline test pinning this function's exact output in isolation). Narrowing would break both real consumers for no correctness gain, so the stale doc comments claiming `brink-db` still calls this entry were corrected in place instead (`crates/internal/brink-ir/src/lir/lower/mod.rs`) — the accurate "not on the compile path, still needed by these two crates" story is now recorded where the next reader will find it. (2) The "composed-equals-monolithic test family" the decision log's original FG-6 ruling named for retirement never existed as a committed *pipeline* comparison — no test ever compiled via both the old `brink-compiler` one-shot driver and `ProjectDb` and diffed `StoryData`; that equivalence was checked ad hoc (`inkb_hashes`) during the PR #844/#846 switch, not in committed test code. `query_equivalence.rs` and `fg2_scc_dependency_edges.rs`'s "composed vs. monolithic" tests are a different, still-real seam — salsa query decomposition (FG-1/FG-2/FG-3) vs. a direct non-decomposed `brink_analyzer` call — unrelated to which pipeline `brink-compiler` routes through. Nothing there was deleted; `query_equivalence.rs`'s header now says so explicitly. Gate: `cargo test --workspace` + `oracle_snapshots` (5,577 episodes, unchanged) both green; clippy clean, confirming no dead code was left behind by declining the deletions.
- **WHY:** An issue asking to delete something is not itself proof the deletion is safe — house rule is "grep for external callers first," and the grep found real ones. Deleting `compile_bench`'s comparison baseline or `golden_i078`'s isolation test to satisfy a cleanup checklist item would have been the "hack that inflates the count" this project's correctness-above-all principle rejects. Documenting *why* the code is shaped this way, in the code, is the honest version of "cleanup" here.
## T2 effects rulings (foundations sitting): rows on types, flow mechanisms, factored encoding, trust tiers
- **WHEN:** 2026-07-13
- **SYSTEM:** language design (#397 T2 effects round, sitting 1)
- **WHAT:** (1) **Effect rows are components of function types** — `Ty::Fn` carries `{reads, writes, calls}`; the shared unifier joins rows exactly as it joins element types, so heap/collection/copy flow is answered by existing type inference (no separate points-to machinery). Unknown-typed slots → pessimal row (conservative-total holds; strict mode therefore buys scheduler precision). (1a) **The layer model:** atomic effects (cell read, cell write, external call) are emitted by expressions; every atom is absorbed into the enclosing definition's row; a def's row coalesces via the same per-SCC fixpoint as TM-1 type inference (FG-2.1's `referenced_globals` pre-scan is the collection pass) and lives in the signature object beside param/return types (firewall/Eq-cutoff applies unchanged). **Rows attach ONLY to function types** — effects are properties of computation, never data; `Ty::Fn` is the sole row-carrying type constructor because a function value is the only data that encapsulates pending computation; collections containing fn values carry rows via element types (containment, not effectfulness). (2) **Shipped entry rows contain what the host can act on**: World cells per-cell + external call kinds; temps and `#@local` cells excluded from shipped rows (no host consequence by construction); internal inference keeps full per-cell precision. Rows are **unordered sets** — ordering is the journal's contract, not the row's. (3) **Four flow mechanisms, v1 scope**: effect polymorphism via row variables on fn-typed params (instantiated at call sites — shallow, since rows fix at creation) SHIPS; host-callback rows manifest-declared SHIPS; heap fallback = the type-level join SHIPS; runtime narrowing = optional host optimization, **selection not inference** (precomputed `DefinitionId → row` table lookup, gated by the self-write check, at schedule-commit) — specified, host implements whenever. (4) **`EffectRows` ships factored rows** (direct part + per-dispatch entries with narrowability bit + static fallback) plus the `DefinitionId → row` table; flat rows would structurally foreclose narrowing. Table extends **only at load boundaries**. (5) **Refinement ladder under the optimizer-not-gatekeeper doctrine** (borrow-analysis precedent): reachability slicing → flow-sensitive per-def → runtime narrowing; refinements only ever narrow, so they're adoptable anytime without wire or contract changes. (6) **Three trust tiers**: first-party `.inkb` = inferred, trusted, zero checks; foreign ink (mods/DLC) = declared caps **verified against bytecode at load** (JVM-verifier style; reject on exceedance; VM cap-mask enforcement named as fallback only); host natives = manifest-declared honesty contract for the scheduler (host is the TCB — not a security boundary). (7) **The load-bearing invariant is "no runtime code synthesis," not "no lambdas"** — lambdas-later pass through (synthesized DefinitionIds, same creation-freeze, capture inherits the durable-cell rule); anonymous-def save-stable identity is the lambda round's problem. Non-goals on record: tier-2 implementation, entity-granular capabilities (manifest syntax space reserved), dynamic linking (#717). OPEN for sitting 2: freeze syntax, drift policy, entry-point definition, manifest grammar, reactive-sleep API.
- **WHY:** Rows-on-types makes the heap case dissolve into machinery TM already runs and keeps one inference engine instead of two; factored encoding and load-extensible tables were both discovered as hard requirements by working the narrowing and capability mechanisms concretely — shipping flat rows first would have foreclosed them permanently; the trust tiers turn the capability framing from metaphor into an actual mod-sandboxing mechanism at zero cost to first-party code; recording the no-runtime-codegen invariant prevents the lambda absence from being mistaken for a foundation.

## T2 effects rulings (sitting 2): no lockfile, assertions-only contract, default-public entries with #@private
- **WHEN:** 2026-07-14
- **SYSTEM:** language design (#397 T2 effects round, sitting 2)
- **WHAT:** (1) **No lockfile.** Effect inference is deterministic from source (no external moving part — the Cargo.lock analogy fails), so a checked-in generated row file is rejected as compiler output cosplaying as input. The shipped `.inkb` rows ARE the frozen record. (2) **The only author-facing contract is the optional inline `#@effects(…)` upper-bound assertion** (`#@effects(pure)` sugar for the empty row); inference exceeding an assertion is a compile error; nothing else errors or warns — there is no drift policy because there is nothing to drift against. Drift *visibility* is tooling: a `brink ide` effects-diff subcommand (CI-surfaceable as a PR comment) + IDE hover. (3) **Default-public entry set**: every knot/stitch ships its row — no `#@entry` marker; `#@private` opts out (not an entry point, row stays internal, host lookup = load-time error), with full visibility semantics deferred to the modules round. The host is treated as "outside every module" — one visibility concept covers both axes. #657's effects half closes; its types half stands (manifest-listed host-callable functions need annotations). (4) **Visibility & modules design epic** is the next major design round, carrying five forks: module unit (file vs declaration), INCLUDE evolution (flat under strict-ink forever; import semantics brink-side), per-module visibility defaults, namespacing vs DefinitionId save-stability (moving a knot between modules breaks saved tokens — fault vs migration directives vs re-export aliases), and #717 dynamic linking as the late-loaded half of the same design.
- **WHY:** (1) A pin artifact with no reproducibility problem to solve adds merge conflicts, staleness, and ceremony for zero mechanical value (maintainer's catch). (2) Errors belong exactly where a human made a promise; everything else is visibility, and visibility is tooling's job. (3) Play-from-here already means any knot is a host entry; default-public matches ink's openness while `#@private` gives real encapsulation and removes the unmarked-entry footgun (pessimal-row fallback never needed). (4) `#@local`/`#@private` are a visibility system growing fingers before it has a body — naming the body prevents piecemeal semantics.

## T2 host mechanics settled: borrow-not-copy, frame-start consistency, two-level bevy integration
- **WHEN:** 2026-07-15
- **SYSTEM:** language design / bevy-brink (T2 sitting 3; effects-spec §12)
- **WHAT:** (1) **Implementation split**: compiler half (row inference + #@effects + EffectRows emission on the delivered FG substrate) builds FIRST; host half builds later against real shipped rows. (2) **Borrow, don't copy**: host parallelism = rows as FilteredAccessSet currency + UnsafeWorldCell-scoped task-pool stepping with buffered writes (bevy's own executor pattern); the snapshot-only contract applies at the ink boundary only (values crossing into script), never as host-side world copying. (3) **Prefetch = resolve QueryStates ahead (QueryBuilder/ComponentId) + prevalidate handle liveness + ELIMINATE park/resume round-trips** (world-read bindings become synchronous against the held borrow) — zero data copying. (4) **Frame-start consistency is the batch default**: reads pin to the frame-start tick, writes apply in deterministic flow-id order; everything parallelizes incl. write-write; peer writes visible next frame; the serial host API keeps immediate visibility as the documented serial mode. The write-write wire bit is NOT needed v1 (reserved-slot question only). (5) **Two-level bevy integration**: v1 = exclusive-system driver with internal per-flow parallelism; v2 = SystemParamBuilder/QueryParamBuilder aggregate-access system rebuilt at story load boundaries (which aligns exactly with the ruled table-extends-at-load-boundaries invariant), letting bevy's scheduler run narrative concurrently with unrelated game systems. (6) Mechanism notes banked: per-container rows are the RESUME-scheduling estimate; reactive-sleep wake sets = live condition reads ∪ transitive fn rows (no new shipped granularity). Remaining for the final host sitting: manifest grammar (+ change-detectable-read bit), sleep author-facing API, serial-mode docs.
- **WHY:** Copying the world to parallelize was the wrong model (maintainer's catch) — bevy already ships the access-proof machinery (QueryBuilder, Access, UnsafeWorldCell, SystemParamBuilder) largely motivated by scripting use cases, and reusing it one storey down means the scheduler design is mostly load-bearing bevy precedent rather than invention; frame-start consistency buys maximal parallelism with full determinism at the cost of one-frame write visibility, which matches simulation-tick intuition; the load-boundary alignment between dynamic system access and the rows table is banked before it could be foreclosed.

## T2 sitting 4 (final): host-driven sleep with the wake contract, opaque-strings manifest — the design round closes
- **WHEN:** 2026-07-16
- **SYSTEM:** language design / bevy-brink (T2 sitting 4; effects-spec §13)
- **WHAT:** (1) **Reactive sleep is host-driven** — a bevy-brink API, no language change; ink-level `await {cond}` recorded as a future direction (own round). The precise wake contract: wake_when sets a standing WAKE policy (parking happens at natural yield points); parked flows cost zero (skipped by Collect, dependency set in the wake map); dependency change triggers RE-EVALUATION not waking (condition = pure fn, purity provable via rows) and only condition-true wakes; woken flows run normal turns; policies persist by default (wake_once for one-shots; host-clearable); turn-boundary parks only (choice/external parks keep their own resume paths; END is inert); spawn+wake_when = dormant spawn. Bevy shape: FlowSleep component on the flow entity (registry alternative rejected). (2) **Manifest capability grammar**: per-external `"effects": {reads/writes, detect: {name: bool}}` with engine-vocabulary compiler-opaque string names (bevy maps name→ComponentId at registration — the HandleKind pattern); the detect bit = change-detection-backed vs must-poll; entity-granular syntax reserved not designed. (3) Serial-mode semantics are a documentation deliverable with the host implementation. **This closes the T2 design round and with it the entire #397 design program** — everything remaining is implementation.
- **WHY:** Host-driven sleep needs zero language surface and composes from parts that exist (begin_function_eval + rows proving condition purity); the wake contract's re-evaluate-don't-wake rule is what keeps hundreds of parked ambient flows at true zero cost; opaque strings are the only grammar keeping the format host-agnostic; and closing the round on a precisely-worded contract beats closing it on vibes.

## FlowFrame suspension + await: designed and parked (the yielding-flows round)
- **WHEN:** 2026-07-16
- **SYSTEM:** language design (#889 round; docs/flow-suspension-spec.md)
- **WHAT:** (1) **The FlowFrame model** is the durable representation for ALL parked flows: current container id + a return stack of tunnel-return container ids (Vec<DefinitionId>) + a compiler-synthesized name-keyed frame record (a plain Value holding every local that crosses a yield) + the wake policy (await-site id + condition fn token). No instruction offsets ever serialize — recompile-stability rides container/DefinitionId identity like everything else. This is ALSO what makes durable save-at-any-park (choices included) a format capability for the first time: SaveState today carries world-state only (verified — no position/callstack fields exist). (2) **`await` is statement syntax in logic blocks** (the facility doctrine demands syntax: turn-splitting cannot hide in an honest function call); direct-expression conditions (`await gold > 100`) capture as compiler-synthesized pure fns whose identity = the await site's synthesized resume-container path (site-stable; the general-lambda identity problem does not apply); `while await cond { }` desugars to yield-with-policy → body → loop (the bool never materializes; false = host cancelled the policy = clean shutdown); standalone await = wake_once. (3) **Stackless at the format, composable via tunnels**: awaiting helpers are TUNNELS (statement-level, name-stable returns — ink's existing two-level structure IS the color boundary); functions are permanently synchronous (aligns with purity/effects/expression semantics); no colored-function virality by construction. (4) **Auto-promotion**: locals crossing a yield compile into the frame record (spill-on-park: ordinary temp slots while running — ZERO hot-path overhead; O(k) Arc-bump value moves at park/wake, k = crossing locals per site) — for-loop iterators crossing awaits just work; the model is one sentence: locals in awaiting scopes live in the flow's frame. (5) **Costs ruled acceptable**: parked flow ≈ low hundreds of bytes (thousand flows = sub-MB, saves +tens of KB); frame-shape drift across recompiles rides the name-keyed rehydration machinery (missing→default-with-report, extra→dropped-with-report); park-depth cap sibling to the step limit; mid-expression await and awaiting-inside-functions stay out permanently. Implementation is a post-T2 milestone (format section + compiler liveness/synthesis + runtime spill/restore + host wake integration); design-complete-and-parked.
- **WHY:** The maintainer's line — "we have to solve the limitations properly or this isn't doable" — was correct: the cheap stackless sketch banned composition, which is a dealbreaker for reusable gameplay logic; tunnels dissolve it using a boundary ink authors already have. Spill-on-park keeps awaits free until used; name-keyed frames make suspended state as patch-tolerant as globals; and the co-benefit (durable save-at-choice) is table stakes for shipped games that the format never had. Serializable recompile-stable coroutines are the moat versus 'just embed Lua' — no mainstream scripting runtime offers them.

## Bevy host track: sanctioned-unsafe exemption, pump-driven with drive-it gates, harness baseline-first
- **WHEN:** 2026-07-16
- **SYSTEM:** bevy-brink (the host epic; effects-spec §12–§13 implementation)
- **WHAT:** (1) **The project's first `unsafe` exemption**: BH-3's parallel step phase may use `UnsafeWorldCell` inside ONE small, audited module in bevy-brink — the workspace `unsafe_code` deny stands everywhere else; the module requires a written safety argument beside every block (the proof obligation: per-flow Access sets are disjoint, computed by BH-1 from effect rows), and the **determinism law (parallel ≡ serial-in-flow-id-order, byte-identical) is its standing behavioral witness** — a failing law run quarantines the module (serial driver is the ruled fallback; parallelism is a perf feature, never a correctness dependency). (2) **Driving arrangement**: full pump treatment with the human drive-it gate at each boundary — the maintainer exercises the scheduler in a real bevy app between waves (the pump doctrine's human verification, finally load-bearing: schedulers have UX). (3) **BH-B, the scenario harness, lands baseline-first**: serial-driver baselines exist BEFORE the parallel phase (the #498/#838 denominator lesson applied prospectively); axes matrix per the 2026-07-16 discussion (flow count, active:parked, world size, disjointness, wake fan-out, change pressure, row width, turn weight, condition complexity, binding mix incl. sync-read-vs-round-trip, parallelism curve, save/load at scale, memory), per-phase instrumentation so regressions name their phase.
- **WHY:** UnsafeWorldCell is bevy's own executor primitive and the rows exist precisely to prove the disjointness it requires — refusing it would mean reimplementing bevy's parallelism worse; fencing it to one witnessed module keeps the workspace guarantee meaningful. Baseline-first is the thrice-learned instrument lesson made prospective. The drive-it gate matters here as nowhere else in the program: frame-loop feel is not unit-testable.
## First-class FSMs & entity statecharts: chartered for T3/4 with four standing boundaries
- **WHEN:** 2026-07-17
- **SYSTEM:** language design (#521 revisit; the FSM round — chartered, not ratified)
- **WHAT:** The FSM concept (declaration syntax, SCXML-informed semantics, entity statecharts with per-state frame logic, a visual graph editor) is chartered for a T3/4 design season with four boundaries RULED now so exploration can't drift into known graveyards: (1) **Lens, not peer** — the visual graph is a PROJECTION of the ink FSM declaration; graph edits lower to structured text edits via the existing CST/code-action machinery; ink text is the only editable canonical form (bidirectional graph↔text conversion is rejected — the UML-round-trip graveyard). (2) **One source format** — SCXML is a one-way import door (generates an ink declaration once, human-owned after); a live second frontend is rejected (the converter/FG-6 lesson). (3) **Host vocabulary stays host-side** — sprites/assets per state bind via host-side mappings keyed on state names or command bindings in entry handlers; asset references never enter brink declarations (format host-agnosticism is a ruled value). (4) **Sequencing gate** — the FSM *language core* (declaration, value = {def token, state NameId}, transitions, static checking, stdlib ops per the facility doctrine: declaration = syntax, operations = intrinsics) may design/build independently; the *entity-controller runtime* (per-state frame logic) waits for BH (scheduler) + FlowFrame (ticking) to exist; the *graph editor* is editor-package surface (studio-shell remains dropped). Design direction recorded: hierarchical-capable core with SCXML transition semantics as the reference; data-FSM + authored entry-handler diverts (the weave seam stays intact); celeris' entity-statechart exploration is the launch consumer and its SCXML subset usage will pick the v1 feature set.
- **WHY:** The maintainer asked for pushback before rope ("we might need to be conservative, even if it's a very cool idea") and the vision's value survives entirely inside the safe forms — the graph-as-lens preserves diffs/reviews/merges that graph-as-peer destroys; every rejected alternative has a named graveyard or an in-repo lesson; and gating the runtime on BH/FlowFrame avoids building the third floor before the second.

## Map/record equality is insertion-order-insensitive
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** runtime/value-model
- **SCOPE:** moderate (observable == semantics, wasm-observable)
- **WHAT:** `#{a:1, b:2} == #{b:2, a:1}` is TRUE — map and record equality compares content (key→value pairs), never insertion order. OrderedMap's derived order-sensitive PartialEq is a bug (#909); ==, contains()-style membership, and any equality-derived operation must use content comparison. Iteration and serialization order remain insertion-order (unchanged) — only equality ignores it.
- **WHY:** Equality reflects value semantics, not construction history; two maps holding the same data are the same value regardless of the order keys were added.

## Pump communication is durable-by-default: findings and context go to GitHub, not just the workflow
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** pump-process
- **SCOPE:** moderate (process)
- **WHAT:** Pump agents post their substantive outputs to GitHub as they work: reviewers always comment their verdict on the PR (approvals included, with scope gaps); build agents comment scope-overflow notes on the issue; fix agents comment applied/skipped dispositions on the PR; merge agents comment only when noteworthy (conflicts, semantic fixes); lessons + scope reconciliation append one comment per wave to the standing "Pump: wave ledger" issue (#967). Labels for visibility/search: pump:ledger, pump:scope (reconciliation-filed issues), pump:lesson (graduated lessons). Workflow-internal messages remain the orchestration mechanism, never the only record.
- **WHY:** Findings and inter-agent context were evaporating with sessions — durable, natively-readable history on the PRs/issues matches the repo's issue-driven workflow and lets humans audit what reviewers actually said.
## FS-3 design round: flow-addressed wake API, parked-flow save/load, caps
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** runtime + web surface (flow suspension)
- **SCOPE:** architectural
- **WHAT:** (1) Flow-addressed consumption: continue lives on flows, story methods = primary-flow sugar; `Line::Suspended {text,tags}` per-flow terminal variant, pre-park text flushes with it; waking never auto-continues. (2) `wakeCheck()` re-evaluates dirty parked conditions (read-set/effect-row dirty-tracking; `#@local` writes dirty only their flow; conditions always evaluated in the owning flow's context; pure manifest externals are legal conditions and always-dirty). (3) Save/load: all flows captured; never-fail-load with per-flow rebound/unresumable + rehydration report + Lenient/Strict knob; missing frame name without #@was = unresumable (never resume-with-default); per-flow SuspendedFlow independently serializable (row-per-flow persistence opt-in). (4) Park-depth cap 8, at-cap = turn-terminating fault; oracle bar = vanilla-unreachable opcodes, ratchet byte-identical, no regen. Futures RECORDED not designed: journal-replay resume rung; story-version + migration-hook facility. Spec §10.
- **WHY:** Ruled against two real manual hosts (SpacetimeDB reducers, RPG Maker MZ): reducers are short-lived and transactional (no push wakes, host controls when output is produced, durable park is a hard requirement); flush-at-park because pre-await text describes the pre-wait state; flow-addressed because multi-flow ambient casts make a single story stream the wrong shape; unresumable-over-default because silent state substitution is the banned laundering pattern.

## Multi-marker capability scoping: global manifest + per-marker registries, validated at load
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** bevy-brink (capability system)
- **SCOPE:** moderate
- **WHAT:** #912 fork resolved as (b): the capability manifest stays app-global; registries stay per-marker; loading a story under a marker whose registry lacks a manifest-required capability is a HARD, immediate load error (the tier-1 admission posture applied per-marker). No per-marker manifest duplication; no silent UnknownCapability err-tables at call time.
- **WHY:** Never-fail-silently at the admission boundary — a missing capability should be discovered at load, loudly, not at first call via logged error tables; per-marker manifests would force duplication for no added safety.

## Detect-bit merge across calls in one container: AND (conservative)
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** bevy-brink (capability join / wake system)
- **SCOPE:** moderate
- **WHAT:** #913 fork resolved as (b): when one container's externals touch the same capability with conflicting detect bits, the merged bit is the AND — the capability is change-detectable for the container only if ALL its reads are detect-capable; otherwise that container's wake conditions poll. Replaces the accidental last-write-wins BTreeMap::insert fold. BH-4's Detect phase consumes the merged bit.
- **WHY:** Conservative-total is the house posture: a missed wake is the engine-race bug class (gameplay-visible, hard to diagnose); an unnecessary re-evaluation of a pure condition is a wasted microsecond. Per-call granularity (option c) adds scheduler complexity before any measurement says it matters.

## FS-3 implementation: continuation-splitting, invisible containers, web-surface-first slicing
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** compiler + runtime + web surface (flow suspension)
- **SCOPE:** architectural
- **WHAT:** (1) Await resume = continuation-splitting: await sites split containers; FlowFrame stores the synthesized continuation container id (stable identity from the await site); resume = ordinary divert into it. No instruction offsets, per spec §3. (2) Continuation containers are INVISIBLE: no visit counts, not divert targets, hidden from IDE nav (debug excepted). (3) Slicing: FS-3w (flow-addressed web surface, ships first against today's runtime), FS-3c (compiler: liveness/frame shapes/splitting, E052 stays), FS-3r (VM park/resume/wakeCheck/save, fence drops) — maintainer approval between slices. Spec §11.
- **WHY:** Invisible containers because visit counts on plumbing would corrupt shuffle/once semantics in behavior loops; web-surface-first so real consumers (SpacetimeDB, RPG Maker MZ) migrate interface shape early and the VM slice changes behavior, not interface; slices keep each review humanly readable while the fence guarantees nothing half-exists on main.

## Compound gameplay v2: BSP room-recipes, MGS-lenient suspicion, LOS-mandatory escape, clock-as-consequence
- **WHEN:** 2026-07-18
- **PROJECT:** brink (drive-app demo)
- **SYSTEM:** demos/compound
- **SCOPE:** moderate (demo design; feeds #905/#901/#827 evidence)
- **WHAT:** Layout = BSP + room recipes, solvable-by-construction, seeded pure fn (rejects the 3-column spine — "looks like jezzball"). Guards = MGS-lenient suspicion accumulation with visible tells; escape requires breaking LOS (guards track last-known-position, search, decay — running never wins); no telepathy (shout recruitment; global alarm only via a reachable Alarm-panel room). Dynamics: gold-in-danger + exit-banking push-your-luck; run-noise vs walk-silent + thrown coins; clock pressure ONLY at alarm ≥ 2 or the opt-in timed Vault; sidegrades not upgrades. Drive-app plan §10.
- **WHY:** Drive-session verdict: no opposing dynamics → no decisions ("it literally isn't a game"); difficulty was monotonic in upgrades. Every mechanic doubles as migration evidence: Investigate = the wake-on-stimulus archetype, recipes/generation = the systems-logic pure-fn specimen, per-guard memory = #@local.

## Compile error-gate is entry-closure-scoped; whole-project has_errors stays for IDE surfaces
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** brink-db / wasm session (context collapse, #1032/PR #1048)
- **SCOPE:** architectural
- **WHAT:** compileProject(entry) gates artifact production on errors within the entry's INCLUDE-closure only (new closure-scoped query on the #815 reachability machinery). The whole-project has_errors query is unchanged — it serves IDE/project surfaces and FG-4a's tested aggregation invariant. Errors in unrelated session files still surface as diagnostics but never block an unrelated entry's build. The same semantics applies to every future context-assembly consolidation (compile_fragment, #1052).
- **WHY:** The db collapse made "whole project" mean "everything the editor session ever loaded," so a broken scratch file would brick Play for an unrelated story — build-tool semantics (cargo builds -p foo despite broken bar) and consumer reality (multi-story editor sessions) both demand entry scoping; keeping both queries preserves FG-4a's invariant instead of trading it away.

## Option[T] pulled forward as a compiler-known builtin; the absence doctrine
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting / native surface)
- **SCOPE:** architectural
- **WHAT:** (1) `Option[T]` becomes the third compiler-known parameterized builtin alongside `[T]`/`[K: V]` — a compiler-owned enum, checker-known, NO user generics unlocked (#1090 candidate (b) grows by one member; the ledger stays open for generics proper). (2) The absence doctrine: **a fault says "your program is wrong"; Option says "the world didn't have one"** — faults reserved for bugs (OOB indexing, malformed questions), Option for expected absence (empty extremum, missing key, exhausted iterator). (3) Ships as one package with its ergonomics: the `x or default` coalescing spelling, and display-boundary forgiveness — an interpolation whose FINAL value is None renders as nothing; everywhere else `Option[T]` ≠ `T`, strict; cut by position, not dialect; nested compositions never forgiven; forgiveness traceable in transcript/debug tooling. (4) The flips: seq `min`/`max`/`first`/`last`/`pop` → Option on empty (was empty⇒fault), text `find` → `Option[int]`, seq `index_of` → `Option[int]`, map `get` → `Option[V]` (the -1 sentinels die unshipped; `get_or` subsumed by `or`). The martyr strategy is superseded — its pressure arriving mid-draft was accepted as the #1090 evidence.
- **WHY:** The flags verb design hit the wall immediately: `first(mood)` on empty has no honest answer without Option (ink's silent-empty hides bugs; a fault is the wrong tool for expected absence), and the same wall stood behind map `get` and the seq extremums. The machinery objection to early Option evaporated when arrays/maps established the parameterized-builtin door. One doctrine with no day-one exceptions beats a per-verb patchwork; the display-boundary cut delivers "forgiving for the author in narrative context, strict in the middle of an A* implementation" without dialect-dependent typing rules.

## The protocol registry: closed, compiler-declared, three v1 entries with effect contracts
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting / native surface)
- **SCOPE:** architectural
- **WHAT:** A CLOSED set of compiler-declared protocols that user types (structs, enums) may IMPLEMENT but never DECLARE — no bounds, no user-defined protocols, no generics unlocked. Two-tier discipline: closed overload families (math kit, numeric tower, len/contains) remain mechanism-free checker-known intrinsics; a registry entry exists only where user types participate in a compiler behavior, and promotion from tier 1 is evidence-gated via #1090's ledger. V1 entries, each carrying an effect contract the checker enforces on impls: **display** (`fn(T): string`, pure·silent·total; feeds the display-boundary rule; structural defaults, user override; machine states inherit it), **compare** (`fn(T, T): int`, pure·silent·total; owns the still-open total-ordering doctrine incl. the NaN decision; the compare-vs-structural-equality coherence line still owed), **iterate** (pull-shaped `next(ref Self): Option[T]`, writes-receiver·silent·total, laws attached and property-harness enforced). Iterate is pull because push-desugared `for` bodies are fn-value callbacks and functions never await — push would ban `await` inside `for` bodies in flows; pull desugars inline and iterators park across suspensions. `for` is the only v1 consumer (concrete-site resolution under mono-HM); user iterables joining map/filter/fold stays #1090-gated; each/for_each remain derived verbs. Implementation spelling ⏳ (code-dialect sitting).
- **WHY:** The registry names a pattern the design had been accreting unnamed (iteration's closed set, equality's ruled semantics, the owed ordering doctrine, display's new need) while refusing the Haskell slide — every tier-1 family individually promotable "for consistency" until protocol soup; the user-participation test plus the evidence gate keeps it three entries. Per-protocol effect contracts are the brink-native prize: a comparator or display impl provably cannot emit dialogue, advance state, or crash the turn — behavior constraints no type-system-only trait mechanism can express.

## Mutation posture: UFCS auto-ref on lvalue receivers + imperative/past-participle naming
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting / native surface)
- **SCOPE:** moderate (call-site semantics for every mutating stdlib verb)
- **WHAT:** Mutating verbs declare `ref` first params. In UFCS method position, an LVALUE receiver auto-refs — `inventory.push(sword)` with no call-site sigil; field-path receivers write through the ruled RMW machinery. An RVALUE receiver is a compile error (`[1,2].push(3)`, `a.sorted().push(x)` — mutating a temporary loses the mutation). The free-call form stays explicit (`push(ref inventory, sword)`): the sugar is earned in method position only, and one fully-spelled form teaches what the sugar means. Naming convention ruled with it: imperative = in-place (`sort push insert remove reverse shuffle`), past-participle = functional (`sorted reversed shuffled`) — the verb name carries the mutation signal, and the confusion lattice closes from both sides (`let b = a.sort()` is a unit type error; `a.sorted().push(x)` is the rvalue error).
- **WHY:** The call-site sigil's warning function is vestigial in a COW value-semantics language — no aliasing, escape, or spooky action for it to warn about; the only information it would carry ("this line mutates") is supplied by the naming convention in English (kinder to the cold-reader writer than a sigil) and by the effect rows regardless of spelling (which also catch indirect mutation a direct-call sigil never could). Swift pays the `&` ceremony because `inout` is rare; `push` is the most-typed line shape in scripted content.

## The trio is pure-required; eager/lazy dissolved; the weird thing gets the ugly method
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting / native surface)
- **SCOPE:** architectural (trio semantics + a standing naming principle)
- **WHAT:** (1) `map`/`filter`/`fold` REQUIRE pure·silent callback rows (reads legal; totality not required). Stage interleaving becomes unobservable by construction, so the spec commits to "one logical pass, order unobservable, implementation may fuse freely" — the eager-vs-lazy question is DISSOLVED, permanently an implementation detail, never a language question again. Only fusion-visible artifact: which element's fault fires first when several would (unspecified). The trio is algebra; the #672-B property laws hold unconditionally. `filter_map(f: fn(T): Option[U])` is the Option-mapper. (2) Effectful iteration is a separate concept with separate spellings: `each` (no result) and `map_each` (effectful transform — produces the array, callback may write/emit, sequential in iteration order, element-by-element, never fused). (3) Standing naming principle ruled with it: **the weird thing gets the ugly method** — convenience is spent on the pure/common spelling, friction on the effectful/rare one; the name itself is the speed bump. Further `_each` variants only on evidence. The trio's rejection error names both exits ("make it pure, or say map_each").
- **WHY:** With effectful callbacks, lazy-vs-eager is OBSERVABLE (interleaving changes write/emit/draw order) — any permissive trio must pick and forever defend a phase-order semantics. Requiring pure·silent callbacks deletes the whole question, makes fusion unconditionally legal instead of row-gated-per-chain, and joins the established position-demands-row pattern (wake conditions, display/compare impls). The named casualty — the dice-drawing map, rng being a write — is exactly the case that pins order, and it keeps a defined-order home in map_each. Maintainer's framing, verbatim: "the weird thing gets the ugly method."

## The ordering doctrine: NaN faults at ordering contexts in dev, pinned placement in prod
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting / native surface)
- **SCOPE:** architectural (ordering semantics for sort/sort_by/min/max/heap + the compare protocol; introduces the dev/prod behavior split)
- **WHAT:** (1) NaN flows through arithmetic (the math domain's NaN-totality ruling untouched); ORDERING CONTEXTS are where it stops. DEV mode: a NaN operand in sort/sort_by/min/max/heap_push is a turn-terminating fault. PROD mode: a pinned total order applies — ordinary IEEE order with -0 == +0 as a tie, NaN greater than everything, NaN-vs-NaN ties (deliberately NOT IEEE totalOrder, whose -0 < +0 would split compare from == on clean data). On NaN-free data the modes agree exactly and cohere with </== — zero divergence. (2) THE FENCE: the dev-fault/prod-continue split is available ONLY where prod behavior is defined, total, and fabricates no data — placement qualifies (elements preserved, deterministic, save/replay-safe); fabrication never does (int("potato"), OOB indexing stay always-fault, all modes). Checked int overflow noted as a sibling candidate for the same knob, not ruled. Knob home (project config + host override) deferred to the tooling sitting. (3) Effect rows are mode-independent: ordering verbs over [float] carry faults unconditionally (conservative union); int/string/bool orderings total; totality-gated positions flag float orderings in both modes. (4) Orderable: int, float, bool (false < true), strings lexicographic by USV (locale collation = intl pipeline), arrays lexicographic element-wise recursively; structs/enums ONLY via explicit registry compare impl (no structural auto-order; derive-by-fields evidence-gated); not orderable: maps, flags subsets, divert targets. (5) Comparison OPERATORS stay frozen IEEE (NaN < x false, NaN == NaN false — ink-inherited, oracle-guarded); only stdlib verbs carry the doctrine (the two-surface pattern's third application). (6) sort_by comparators: pure·silent per the trio ruling + the consistent-total-order law; implementation may fault on detected inconsistency; guarantee floor "some permutation, never worse"; heap_push checks at entry.
- **WHY:** NaN's poison activates at comparison, not arithmetic — feeding IEEE's partial order to a sort yields implementation-defined garbage in most languages, the bug-travel pattern this program exists to kill. The dev/prod split is the game-industry assert discipline (Rust's overflow precedent): surface the bug loudly on the dev box, never crash in front of a player — affordable here precisely because a principled non-fabricating prod fallback exists, and fenced to exactly that criterion so the knob can never become a general fault-suppression lever. Maintainer-proposed; the fence and mode-independent rows were the design conversation's shaping contributions.

## Maps ruled: contains_key/contains_value, get-Option, insert reserved, lambda debt recorded
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting, domain 4)
- **SCOPE:** moderate
- **WHAT:** Maps ruled as spec'd (stdlib-spec §5): `Map { k: v }` literal, `[K: V]` homogeneous, keys = scalars + unit enum variants; #856 indexing stands (read faults, write inserts); verbs `len contains_key contains_value get keys values remove clear` — `get → Option[V]` per the Option package (`or` subsumes get_or), `contains_value` an honest O(n) content-equality scan, `remove`/`clear` in-place idempotent-total (the seq-remove-faults / map-remove-total divergence is CHOSEN: an index is a claim, a deletion is a wish); `keys`/`values` eager insertion-order snapshots; `entries` gated on the anonymous-record closer; prelude `len` only. **`insert` is RESERVED, not shipped**: syntax isn't passable, so verb-form demand arrives with lambdas/pipelines — it is exhibit #1 of the code-dialect sitting's syntax-in-value-position item (with fold-over-`+` as exhibit #2). Recorded with it: **the lambda debt** — the entire fn-value verb layer presumes undesigned fn-value literals; lambda design is the code-dialect sitting's opening item, with by-value capture pre-registered (no ref captures v1). 🔶 filed: `for ref x in xs` mutating iteration (index-desugar over RMW, no projections).
- **WHY:** contains_key/contains_value kills bare-contains ambiguity on the one collection where it exists; reserving insert avoids two day-one spellings of one concept while framing the door for the general syntax-in-value-position mechanism to decide; the lambda confession converts an implicit dependency into a scheduled design item before Phase C sequencing could trip over it.

## Flags ruled: the LIST-op audit lands; the numeric coupling is frozen
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting, domain 5)
- **SCOPE:** moderate
- **WHAT:** The flags verb surface ruled as spec'd (stdlib-spec §6): plain renames count/all/none/contains/add/remove/intersect/range/invert (same runtime ops, oracle-held); `first`/`last` rename LIST_MIN/LIST_MAX into domain-order vocabulary returning Option on empty; `next`/`prev` step a single-flag subset (edge → none; multi-flag/empty input faults — malformed question is a bug); `index_of(flag)` → int, total on a single flag, faulting on multi/empty (the ordinal query). **The numeric coupling is FROZEN**: ink's explicit numeric flag values and subsets↔ints conversions stay on the ink-frozen surface, never respelled; native flags are pure ordered symbols; "symbol with data" is enums-with-payloads' job. Prelude: contains, count. Operator forms (?, ^) stay parked for the code-dialect sitting.
- **WHY:** The rename to first/last commits flags to reading as an ordered symbolic domain rather than numbers wearing names; the freeze cuts the last tie to the LIST-as-arithmetic abuse pattern that machines and enums now serve properly — migrating stories keep that code on the frozen surface or own the int explicitly via index_of.

## Random ruled: rng-as-cell, and rand::int made total by the language's first value refinement
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting, domain 6) + type system (refinements)
- **SCOPE:** architectural (introduces value refinements)
- **WHAT:** (1) RNG is a named runtime state cell owned by std::rand; every draw is an ordinary WRITE in the row — no new effect dimension. Free consequences: pure-gated wake conditions statically exclude draws (re-roll instability), the pure-required trio rejects draw-bearing callbacks (dice-maps go to map_each), @[effects(pure)] asserts rng-freedom. (2) Determinism: algorithm pinned as a stability contract; draws are a pure function of state → (value, state'); state saves/loads with the story; seeded replay = identical transcript cross-platform; unseeded stories host-seeded at start (manifest-visible). (3) **rand::int is total BY TYPE**: its parameter is the **inhabited range — the language's first value refinement**. Statically-provable literals coerce free (const-folded bounds); statically-empty literals are a compile error; computed bounds must pass the validator `(a..b).nonempty() → Option[<inhabited range>]` — parse-don't-validate, the Option tax paid once at the dynamic-data boundary, draws amortized. Plain `range` stays possibly-empty (iteration load-bearing: `for i in 0..n` runs zero times; `pick(0..n)` → none). (4) `pick(iterable) → Option` covers all dynamic-content draws; UFCS `enemies.pick()` is the direct-collection spelling. Verbs: int, float, chance, pick, shuffle/shuffled, seed; NO prelude entries. Ink's RANDOM/SEED_RANDOM stay frozen over the same cell. (5) **Refinement doctrine recorded**: effect rows are refinements on functions; this is the species applied to a value; CLOSED compiler-known refinements only, checker-minted evidence, no user predicates (liquid-types = its own future ledger; population today: one). Type/validator spelling ⏳ code-dialect sitting.
- **WHY:** rng-as-cell reuses the entire row machinery where a new dimension would duplicate it; the refinement reshape (maintainer-driven across three rounds: fault → verb-split → preclude-by-API) makes the invalid draw UNREPRESENTABLE rather than detected — no fault path, no per-draw Option tax on dice literals, and the Option lands exactly where dynamic data enters. The maintainer anticipated refinements as the effect system's sibling; the closed-set fence keeps them from becoming a predicate language by accretion.

## Collections+ ruled: Weighted evidence-by-construction, the humble heap
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting, domain 7)
- **SCOPE:** moderate
- **WHAT:** (1) `Weighted[T]` parameterized builtin; `Weighted { weight: value }` literal (chartered grammar; positive-int weights v1); one draw verb `rand::roll(w) → T` in rand's namespace (row writes the rng cell). Evidence-by-construction: empty/zero/negative-weight tables are refused (compile error where classifiable, construction fault for computed weights — E078-style split, NEW diagnostic code owed by Phase C), so roll over any existing table is total. Designated evolution recorded not built: a validating Option-returning constructor if dynamic table-building shows dossier demand. v1 is construct-and-roll; len/iteration/mutation deferred. (2) Heap: verbs over ordinary arrays in std::collections — heap_push (NaN entry-check per §4b), heap_pop/heap_peek → Option; min-heap; zero new value kinds, zero wire work; sealed Heap[T] builtin recorded as the upgrade path if shape-confusion incidents accumulate. (3) Further collections (deque, set-as-type) evidence-gated; std::collections is the landing zone, the dossier the door.
- **WHY:** Weighted independently arrives at the same parse-don't-validate shape as the inhabited range — construction is the validator, totality follows; the humble heap buys the dossier-evidenced need without value-kind or wire cost, with the escalation path named instead of built.

## Closers ruled: anonymous records retired, for-k-v iteration, the assembled prelude, the std:: tree
- **WHEN:** 2026-07-18
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib sitting, closers)
- **SCOPE:** architectural (retires a value-model surface concept from the native surface)
- **WHAT:** (1) **Anonymous records RETIRED from the native surface**: homogeneous bags = maps; typed shapes = declared structs; **multi-return = declare the struct** (no tuples, no third structural-record concept; records-as-maps rejected on typing — heterogeneous fields die in [K: V] homogeneity; lightweight inline record types = ledger-gated code-dialect question). (2) The entries() question dissolves: **`for k, v in m`** two-binding iteration desugars to key-iteration + `let v = m[k]` (total by construction; no pair shape materializes); reified entries() evidence-gated. (3) Construction syntax: one initializer grammar `TypeName { … }`, per-type meaning — protocol-vs-grammar (the maintainer's recalled C#-lineage initializer-protocol direction from an earlier UNRECORDED thread) filed as #1103 for the code-dialect sitting; this sitting commits only to the grammar shape. (4) Assertion spellings final: `@[effects(…)]`, subsets of {pure, silent, total}, exceedance-only; doc-sync owed to effects-spec/#1087 (#@effects superseded). Holes' release policy PARKED past the sitting by the maintainer. (5) Prelude final list: full math kit incl. trig · len/contains/char_at (text) · len/contains/push (seq) · len (maps) · contains/count (flags) · nothing from rand/collections; prelude names shadowable with the E035-lineage warning. (6) Docs display notation: pseudo-generic letters with the "not writable in source" banner. (7) std:: tree final: math/text/seq/map/flags/rand/collections; host:: per manifest; no std::prelude module (the prelude is pre-granted naming, not a place).
- **WHY:** Retirement beat keep-narrowed once the two load-bearing uses fell: multi-return is better served by a named two-line struct (documentation for free), and the pair-shape need vanished entirely under two-binding for (a desugar, not a datatype). The lost-thread episode (initializer protocol discussed, never recorded) is itself the argument for this log entry's existence.

## Findings batch 1 ruled: all seven recommendations adopted
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (NS program, tracker #1106 findings queue)
- **SCOPE:** moderate (seven composition-audit rulings; unblocks NS waves A3/A4/A5)
- **WHAT:** The maintainer ruled Phase C's seven blocking findings, adopting every recommendation ("all recs", post-flight): **F0** — `sort_by` is in-place (`ref a`, the naming law), `sorted_by` added as the functional twin, the §9.4 display exemplar reselected to `map`. **F1** — both interpolation AND the `string()` conversion intrinsic dispatch through the `display` protocol: one display path; totality survives via the contract. **F3** — `chance(p)` clamps p to [0,1], NaN → false: total (interpretation, not fabrication; not an ordering context so §4b's fence doesn't apply). **F6** — the registry method names (`display`/`compare`/`next`) are RESERVED: author shadowing is a hard compile error, not E035. **F7** — ranges become a real Value kind (wire/equality/display/save) — FlowFrame iterator spill demands a durable form; the inhabited-range refinement is a view over it. **F8** — refinements are inert in gradual mode with a runtime-fault residual (the general rule for all future refinements; the int()/E078 precedent). **F10** — `for k, v in m` snapshots the key set at loop entry (maps' for = deliberate exception to live pull; removed-key reads fault honestly). F27/F28 (Option truthiness, pre-B4 display) remain open pending the maintainer's read.
- **WHY:** Each recommendation was the composition-audit's reasoned resolution of a seam where the 2026-07-18 rulings met; adopting them wholesale keeps the doctrine lattice consistent (naming law, one-display-path, placement-not-fabrication, contract trustworthiness, durable-wire, gradual posture, honest-fault) without re-litigating any prior ruling.

## F27/F28 ruled: Option has no truthiness; display renders totally until B4
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (NS program, findings queue batch 2)
- **SCOPE:** moderate (supersedes one A1-shipped behavior)
- **WHAT:** **F27 (b)**: Option has NO truthiness — condition-position `Option[T]` is a compile error under strict and a runtime fault under gradual; explicit `== none`/`== some(x)`/(post-B1) `as`-binding required. Supersedes A1's shipped falsy-none; implementation fix filed. **F28 (a)**: `none`/`some(…)` render totally in stringify/display until B4's display-boundary forgiveness lands with the native surface; `string()`'s ruled totality preserved.
- **WHY:** Truthiness is a quiet coercion of exactly the kind `Option[T] ≠ T` exists to ban — the falsy-none convenience reads pleasant and reintroduces the silent-absence bug class one position at a time; the explicit forms cost one comparison and keep absence visible. F28's total render is the only choice that doesn't either break `string()` totality or pre-empt a Track B wave in the brink dialect.

## Tower mini-spec ruled (T1-T5); A2 judgment calls confirmed; knob home; compare coherence
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (NS program — airport sitting, pre-boarding)
- **SCOPE:** architectural (tower representation; closes the last Track A design gates)
- **WHAT:** (1) **Tower mini-spec** (docs/tower-mini-spec.md): glam-backed Value kinds (maintainer's call — "literally back stuff with glam types"; unaligned variants; workspace-pinned version shared with bevy for identity marshal; blessed-structs alternative rejected on f64-field fidelity drift); ALL matrix sizes v1 (mat2/3/4 — the economy argument died with glam backing); conventions per glam wholesale; componentwise-IEEE equality, tower not orderable; wire = hand-serialized little-endian f32 lanes, never glam memory/serde (SIMD/version repr independence for saves+replay); libm-transcendental determinism footnote recorded not solved. Unblocks A8 (#1114). (2) **A2's six judgment calls confirmed** with item 1 AMENDED: @[effects] clause grammar goes Rust-meta-item paren-style — reads(gold, hp) not reads: gold — so flags can never be swallowed into open clauses; the deprecated #@effects colon grammar is frozen (E110 surface does not evolve). Items 2-6 as shipped (DivisionByZero in faults; type-error species excluded; E110 warning; @[ prose parsing with \@ escape; pure-silent-total empty-row display). (3) **Dev/prod knob home**: project config (brink.toml profile) + host-API override; tooling implements when A4 needs it. (4) **compare/equality coherence**: compare is ordering only; equality stays structural always; divergence legal and documented — sort never implies dedup.
- **WHY:** Glam-backing converts the tower's correctness burden from hand-rolled-and-hand-tested to battle-tested-by-construction in the one domain the oracle cannot cover, at the cost of one pure-math dependency and a wire-independence discipline; the paren-clause amendment removes a demonstrated author footgun by the same structural move Rust's meta grammar uses (delimited clauses beat positional rules); the remaining rulings close every non-sitting design gate in Track A — the substrate is now fully specified.

## Value-position ruled: nothing v1 — lambdas are the spelling; dedicated verbs are the valve
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (code dialect — airport sitting part 2)
- **SCOPE:** moderate
- **WHAT:** The syntax-in-value-position mechanism is NOTHING v1 — no operator sections, no `(+)`-values, no verb twins. Lambdas (ruled this sitting) are the value-position spelling of any operation: `fold(0, |acc, x| acc + x)`. The reserved `insert` stays reserved-unshipped (the door stays framed). The ergonomic valve is DEDICATED VERBS — `sum()` (and kin like `product()`) land in `std::seq` when evidence demands, exactly Rust's own posture (no sections; `Iterator::sum` exists). Ledger stays open for closure-noise evidence.
- **WHY:** Maintainer: "do the Rust thing and have a .sum() method anyway, if we ever need it." One-spelling-per-concept at its purest; Rust culture writes the closure and moves on.

## The spelling cluster ruled: companion-module impls, NonEmptyRange, methods-not-operators
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (code dialect — airport sitting part 2)
- **SCOPE:** architectural (S4 touches the module system)
- **WHAT:** (S1, absorbed into S4) Protocol impls use Rust-shaped impl blocks: `impl display for Npc { fn display(n: Npc): string { … } }` — explicit receiver param, NO `self` receiver magic beyond the S4 sugar below; the method name matches the protocol's ruled method (the F6 reservation and the spelling are one fact); the block is the contract-enforcement site. (S2) The inhabited-range refinement is named on Rust's NonZero precedent: type **`NonEmptyRange`**, validator **`.non_empty()`** → `Option[NonEmptyRange]`. (S3) No operator forms for `contains`/`intersect` — methods only; ink's `?`/`^` stay frozen-surface; flags `+=`/`-=` keep their already-ruled dual life. (S4, maintainer-designed) **impl blocks are COMPANION-MODULE sugar**: `impl Npc { fn greet(self, …) }` declares fns in a virtual module co-named with the type, mounted at the type's declaring module — **`Npc::greet` is the only actual name** (DefinitionId = (companion-module, name), riding the ruled wire identity). `self` is a reserved word = sugar for the first param typed by the block. Multiple impl blocks merge. UFCS lookup: companion-first (receiver type known), then in-scope free fns; at most one candidate each — namespace lookup, NOT dispatch; no method system. Riders: **companions are virtual** (nothing on disk creates or blocks them) and a **casing partition** makes lexical collision unspellable — filesystem/declared module segments MUST be snake_case, type names MUST be UpperCamel, both compile-checked; folders and types can never block each other in either direction. Cross-module impl coherence restriction PARKED for the full sitting. Legacy filename-mapping rules from the modules round untouched.
- **WHY:** Maintainer's design across three moves ("abusing modules — Npc::greet is the only actual name"): companions dissolve the name-collision dilemma without receiver dispatch, reuse the module system's resolution/imports/identity machinery wholesale, and the casing partition converts a collision class into an unspellable one. Separator stratification survives literally: `.` walks data, `::` names declarations.

## Typing posture ruled: the native surface is strict-only; strict becomes the brink dialect's default
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (typing policy across surfaces)
- **SCOPE:** architectural
- **WHAT:** (1) **The native surface is strict-only** — `types = strict` is a property of the dialect, not a project knob; gradual typing does not exist on the native surface. Deletes every "under gradual" edge from Track B: companion UFCS lookup always resolves (no qualification tax), F27's gradual fault arm never fires on native, F8's refinement residual confines to compat surfaces. (2) **The brink dialect's DEFAULT flips to strict** (gradual remains an opt-out knob). (3) Reconciliation of the TM-era "gradual is the permanent default" ruling: per-surface — strict-ink/compat keeps gradual default forever (oracle corpus untouched, byte-identity preserved); brink dialect defaults strict; native is strict-only. The gradient matches each surface's era. Cross-dialect calls are mediated by the boundaries-only annotation doctrine (ink symbols enter native code as Unknown; strict rejects Unknown escapes; annotate at the seam). (4) Implementation of the default flip = **NS-A9** (filed): config-resolution change + corpus/fixture fallout audit + changeset — a real behavior change for every brink-dialect compile. (5) PARKED: the transitional brink dialect's long-term fate once the native surface lands.
- **WHY:** Maintainer: "under the new syntax, gradual typing shouldn't happen … gradual doesn't even need to be the default under brink dialect." The new surfaces were designed under the strict doctrine (boundaries-only annotations + inference); gradual is the compat posture, not the future's.

## .brink is the native-surface extension; editor support and the book are first-class NS workstreams
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design + program structure (native surface)
- **SCOPE:** moderate (charter §8.5 partially resolved; two workstreams elevated)
- **WHAT:** (1) **Native-surface files are `.brink`** — the charter §8.5 extension question is ruled; naming/migration/coexistence details (ink↔native converters, mixed trees) remain for their round, but the extension is fixed and tooling may build against it. (2) **Editor support is a first-class NS workstream, not a B0 afterthought** (maintainer: "strong editor support for this new native syntax frontend"): LSP for the native frontend (semantic tokens across prose/code dialects, completion incl. UFCS/companion-aware, hover with rows+signatures, diagnostics), fmt, and the live renderer the charter's rendering principle promises (renderer-elidable marks, Obsidian/Scrivener-style) — chartered as NS-T, decomposed into waves at the code-dialect sitting alongside B0 (the friction-journal instrument DEPENDS on this: the writer validates through the editor, so editor quality gates ratification itself). (3) **The book gets serious investment** (maintainer's words): chartered as NS-D — native-surface-first rewrite, example-led per the charter §6 docs note ("first-class, example-led explanation, not a footnote"), teaching by concept not glyph list (§10c), the stdlib/types/effects chapters against the ruled spec; begins against the brink dialect now where content is surface-independent, completes against .brink syntax as B-track lands.
- **WHY:** The writer-validation exit criterion makes editor quality load-bearing for the season (a cold reader cannot journal syntax friction through a broken editor), and the book is how one-spelling-per-concept actually reaches its audience; both were implicit in charter text and are now explicit workstreams with issues.

## Lambdas ruled: Rust pipes under the RustScript north star; charter §7 amended
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design (code dialect — the sitting's opening item, closed early)
- **SCOPE:** architectural (fn-value literal surface + a charter-level north-star update)
- **WHAT:** (1) **Charter §7 amended: the code dialect's north star is RustScript**, superseding "Lua-adjacent feel" — the dialect was already Rust-shaped in every ruled bone (fn/let/match/enums/structs/use/::/@[…]/ranges), so family coherence is itself the cold-reader story; a one-off anonymous-fn form would have been the inconsistency. (2) **Lambda surface = Rust pipes with colon returns**: `|g| g.awake` · `|g: Guest|: bool { … }` · `||` zero-arg. Colon (not `->`) matches the ruled declaration syntax and keeps `->` purely a divert (one arrow, one meaning). No `move` keyword ever — brink captures BY-VALUE always (Rust's move semantics as the only mode; ratifies the pre-registered bet), no ref captures v1. (3) Riders: single-expression or block bodies; `return` leaves the lambda; last expression is the value; **assignment to a captured binding is a compile error** (a snapshot write is always a lost write — kills closure-mutation confusion structurally); lambdas are fn-colored always (never await — the axiom, restated so the parser never relitigates); params optionally annotated, mono-HM infers at call sites, context-free bare lambda = the E107 posture; bare-name fn references are the same value species; rows compose per #872 unchanged. (4) Ripple recorded, not pre-decided: the RustScript north star bears on the remaining parked spellings (protocol impl blocks, inhabited-range naming) at the full sitting.
- **WHY:** The maintainer named the north star ("RustScript... L1 I lean a"); pipes are what that family's lambda looks like, and the cross-dialect pipe-glyph reuse concern dissolves at the body-dialect boundary (SQL-in-host, not overloading within one grammar). The two adaptations (colon returns, no move) make them brink's pipes rather than borrowed ones.

## Delegated batch ruling — coordinator recommendations adopted, NOT FULLY REVIEWED
- **WHEN:** 2026-07-19
- **PROJECT:** brink
- **SYSTEM:** language design + program process (five pending batches resolved at once)
- **SCOPE:** architectural (multiple), with an explicit review caveat
- **WHAT:** The maintainer, back from travel and ruling from the coordinator's summaries WITHOUT reading the underlying documents ("i'll take your rec, note them as not fully reviewed in case i'm confused later"), adopted every standing coordinator recommendation:
  - **F29** (protocol faults granularity): (a) — symmetric carve-out; a provably-total `display`/`compare` impl is not forced to carry the conservative faults bit. Unblocks A4→A7.
  - **Batch 4 / HIR admission contract (Q1–Q7)**: Q1(b) opaque `Provenance { file, range, kind_token }` + frontend-supplied resolver trait, sequenced first · Q2(a) ratify byte-range-equality join for v1 with a loud admission check; explicit NodeIds tracked as the endgame · Q3(b) the `SymbolManifest` becomes a pipeline projection derived from HIR · Q4(b) keep 2 container levels v1 but write the contract's addressing model to generalize · Q5(a) chart #905 is a body-dialect inside the native frontend · Q6(b) native accept-list admission gate · Q7(a) explicit `Return.kind: ReturnKind`. PR #1134 merges with these folded.
  - **Batch 5 (F30–F33)**: F30(a) range `==` compares the denoted sequence (`1..=6 == 1..7`) · F31 partial-(b): adopt `mat*mat`, `mat*scalar`, `vec/scalar` rows now, everything else keeps faulting (follow-up issue) · F32(a) tower values are always-truthy · F33(a) `dot`/`cross` stay ambient intrinsics, revisit at B5.
  - **Batch 6 / NS-D book outline (D1–D10)**: all (a) — native-only spine with compat chapter + ink-authors appendix; bevy tutorial in the book, spec stays in docs/; stdlib inline-with-concepts + signature tables; class-split transitional-dialect handling (compiling brink-dialect examples with callouts vs `.brink`-first proposed fences); wave-0 truth-sync (already landed, #1143); one fence-walker CI test; intro reframe waits for `.brink`; glyph table as reference appendix; iteration chapter ships with fn-value spellings; concepts/contributing kept. PR #1142 merges with these folded.
  - **Process**: `Test (all features)` stays non-required for now; the local gate template gains the feature-gated suites (finding 6 remedy (a)). The #1101 spurious-wake fix direction is (a) **row-directed wake dirtying** (re-evaluate only policies whose condition's inferred read row intersects the changed cells) — design/implementation as its own wave, issue to follow.
- **WHY / CAVEAT (load-bearing for future readers):** These are REAL rulings for build purposes — waves may proceed against them — but they are **delegated and provisionally held**: the maintainer accepted the coordinator's recommendations sight-unseen and may reopen any of them on first close read. Agents encountering a surprise, contradiction, or awkwardness downstream of any decision in this entry must flag it LOUDLY to the maintainer rather than assume it was deliberately examined — the usual "the ruling was considered and is settled" presumption does NOT apply at full strength here. Each affected document carries a "ruled by delegation 2026-07-19, not fully reviewed" marker pointing back to this entry.

## 2026-07-19 — evening walkthrough: batch 7 (NF-1..6 incl. B0.8b converter slice), F34/F35/F17, Q-R1..4 (sourcemap epic scheduled), BW-1..5, #1146 quarantine
- **WHEN:** 2026-07-19 (evening)
- **PROJECT:** brink
- **SYSTEM:** language design + program process (interactive walkthrough of the complete pending queue)
- **SCOPE:** architectural (multiple; two rulings explicitly OVERRIDE standing doc recommendations — NF-4, Q-R3)
- **WHAT:** The maintainer ruled the complete pending queue in an interactive walkthrough; every ruling is stamped (dated, 2026-07-19) into its governing doc. One line each:
  - **NF-1 (a)**: the native lexer/CST is a new peer crate `crates/internal/brink-syntax-native`.
  - **NF-2 (b)**: writer-sufficient subset — prose dialect complete, code dialect minimal (existing-HIR constructs only); deferred constructs parse-but-reject loudly ("ruled but not yet lowered"); gap list published at writer onboarding.
  - **NF-3 (b)**: declared source root in brink.toml; module path = relative path; INCLUDE never consumed for `.brink`; RIDER: cross-dialect INCLUDE (`.ink`↔`.brink`, either direction) is a hard error, never a silent skip.
  - **NF-4 (a) — OVERRIDES the doc's recommendation (b)**: writer onboarding gates on ALL of B0 (incl. B0.8/B0.9/B0.10), not first light; NS-T timing pressure correspondingly relaxed but not void.
  - **NF-5 (c+)**: snapshots + hand-curated respelled differentials as B0.7/B0.8 exit gates, PLUS new slice **B0.8b** — HIR→brink emitter + mechanical corpus converter (parallel lane, entry B0.8 merged); full-corpus episode-identity differential = the B0 ratification exit gate; emitter deliberately shared with the future `.brink` formatter and printer-based IDE rewrites (the ruling's rationale); crate suggestion `brink-respell` — NEVER `brink-converter` (#544's retired crate).
  - **NF-6 (a)**: admission validator always-on; measured perf budget in B0.3's exit criteria; per-check dev-demotion only by individual maintainer ruling.
  - **F34**: comparator write-guard = runtime check keyed on ExecMode — Dev faults (new tracked fault `ComparatorWroteState`) on any world-write mid-comparator; Prod skips the check (the write executes; defined + deterministic since the comparison sequence is fixed) — §4b's placement-never-fabrication pattern applied to effects.
  - **F35 (b)**: bevy-brink's default ExecMode keys off `debug_assertions` (dev builds → Dev, release → Prod); hosts can still set explicitly; the core runtime default stays Dev.
  - **F17 — CONFIRMED as landed**: Weighted[T] multiset equality (the A7 extension via content-over-form) ratified; explicit RULED stamp added to §8 (closes the thin-ruling flag); internal sorted-canonicalization considered and REJECTED (requires a total order on T; Weighted of non-orderable T is legal).
  - **Q-R1 (a)**: new optional strippable `SectionKind::DebugInfo` (tag 0x11, omit-when-empty); dormant `Opcode::SourceLocation` retired.
  - **Q-R2**: adopt accommodations A1–A3 (+ free riders A4/A5) — already landed in B0.1.
  - **Q-R3 — OVERRIDES the memo's recommendation**: the source-map epic IS scheduled now — it enters the active queue rather than waiting for B0.4.
  - **Q-R4**: the debug section is section-locally versioned; a NodeId column is reserved as the v2 extension; v1 entry encoding stays with the epic's design round.
  - **BW-1**: book file paths stay.
  - **BW-2**: error fences carry ` ```ink,error(Exxx) ` info-strings; the fence walker asserts the named code.
  - **BW-3**: the Option annotation asymmetry is intended-until-B1.
  - **BW-4**: `insert` = compat-with-expiry.
  - **BW-5**: the fence walker is commissioned now.
  - **#1146 quarantine authorized** (recorded in this log only, per the ruling).
- **WHY:** Clearing the whole pending queue in one interactive sitting (in contrast to the delegated batch of the same date, these were walked through with the maintainer) unblocks every waiting lane at once: B0 loses its last ruling gates, the stdlib's ExecMode seam is closed for A4/bevy-brink, the source-map epic gets its perishable format decisions plus a schedule, and the book's writing waves get their conventions. Stamps live in: b0-findings.md + b0-sequencing.md (batch 7), stdlib-spec.md §4b/§8 (F34/F35/F17), sourcemap-epic-evaluation.md addendum (Q-R1..4), ns-d-book-outline.md §5 (BW-1..5). Where a ruling overrides a doc's recommendation (NF-4, Q-R3), the stamp says so explicitly.

## Doc comments ruled first-class on the native surface: CST-node attachment + inner `//!` form
- **WHEN:** 2026-07-20
- **PROJECT:** brink
- **SYSTEM:** language design + native parser (front-end doc-comment channel)
- **SCOPE:** moderate (adds a doc-comment surface to the native charter; defines slice B0.6b)
- **WHAT:** The native surface's doc comments become **first-class by structural attachment**, not trivia re-derivation. Today `DocBlock` (`brink-ir`, `host_manifest.rs`) is already a rich structured type (`@param name {type}`, `@returns {type}`, `@kind`, with diagnostics E038/E043) and the OLD ink parser's channel is fully wired — but *attachment* is a trivia-walk: HIR lowering (`doc_comment.rs::collect_doc_lines`) re-derives which declaration a `///` block belongs to by walking `prev_token()` backward over whitespace. The native channel was hardcoded `doc: None` (B0.6 judgment call 5, deliberately deferred). Ruling: (1) **Attachment level B** — the native lexer lexes `///` as a distinct `DOC_COMMENT_OUTER` token (vs plain `LINE_COMMENT`); the parser collects a contiguous run of them immediately preceding a declaration into a **`DOC_COMMENT` CST node emitted as the leading child of that declaration node** (no longer floating trivia); the AST gains a `.doc()` accessor; native HIR lowering reads the node and feeds its text to the *existing* `DocBlock` `@`-tag parser (which is format-agnostic string parsing — factor `parse_lines`/DocBlock construction apart from the old parser's `collect_doc_lines` trivia walk so both front-ends share it). Attachment is thus decided **once, structurally, by the layer with the most context**, with a stable range the #452 source-map epic can key on. (2) **Inner form added**: `//!` lexed as `DOC_COMMENT_INNER`, collected at the start of a knot/flow/file body into a `DOC_COMMENT` (inner variant) attached to the enclosing container node — documents the *enclosing* knot/flow/file (Rust `//!` precedent; ink only ever had the leading form). (3) **Native-only** — the old ink parser's trivia-walk is left as-is for its remaining life; not retrofitted. (4) The `@param/@returns/@kind` text-parse stays in HIR (not pushed into the grammar) — rejected the heavier "full grammar production" option (level C) as coupling the grammar to the manifest tag vocabulary + TypeRef. Filed as **B0.6b**; off the critical path to "author a scene" (that's B0.7→B0.10) but a ruled slice that builds alongside.
- **WHY:** Maintainer: "the old parser attached them as trivia, but if there's any way to make that process more 'first class' i'd like that." The content model was never the problem — the *attachment* was a heuristic re-derivation of something the parser already knows structurally; promoting it to a CST node makes every consumer (HIR, fmt, LSP hover, source maps) read the same authoritative association instead of each re-walking trivia. The inner form was chosen (over leading-only) to give file/flow-level headers a home.

## Unified block/effect/coroutine model ratified (native surface)
- **WHEN:** 2026-07-20
- **PROJECT:** brink
- **SYSTEM:** language design + HIR/checker + runtime model (native surface)
- **SCOPE:** architectural (a substrate unification + one new capability + one new surface affordance); full spec in `docs/block-effect-model.md`
- **WHAT:** Ratified the unified block/effect/coroutine model (`docs/block-effect-model.md`, RULED 2026-07-20). Core: (1) **a block is an expression** whose value is its tail and whose behavior is its inferred **effect signature** — what a braced construct "is" (interpolation, choice body, conditional arm, fn/flow/lambda body) is a block + effects decided by the type checker, not a syntactic category the parser guesses; this dissolves G-2 (interpolation = value-tail, body = diverge/unit-tail) and the "constructs embedded in content" grammar class. Prose and code dialects are skins over one substrate. (2) **A flow is a coroutine; fn = the degenerate flow that never suspends.** The **suspension ladder is the color** — await ⊂ choice ⊂ turn, ordered by who resumes (engine/player/driver); a block's color is the outermost rung it reaches; call *down* the ladder is safe, call *up* is up-coloring (rejected). Generalizes `flow-suspension-spec.md` §4's fn/tunnel boundary. (3) **Value-returning flows — RULED:** a flow may declare a return type; the declaration is the toggle between **coroutine** (must yield a value, may not laterally divert away) and **state** (an FSM node, may divert away). FSMs fall out: flows are states/transitions; fns compute and *return* divert-*targets* (pure data), flows *follow* them (record vs follow — following is flow-colored, a fn may not up-color to follow). (4) **Sequences are an effect — RULED:** the cycle/shuffle/once family is an impurity effect on an ordinary block, not a separate universe. (5) **Syntax liberalization — RULED, uniform:** any operand-position suspension (await, choice, coroutine call — no carve-out) is legal at the surface and **ANF-lowered** to a statement boundary, converting "operands live on the eval stack across a yield" into "named locals across a yield" — so **no stackful trace is ever stored** (honors `flow-suspension-spec.md` §2 "no instruction offsets, ever"); visibility of invisible awaits is an editor concern (NS-T), not a grammar carve-out. (6) Two **type rules (new):** no lateral divert from a value-position flow (stricter than Rust `!`-coercion, because brink's divert is lateral, not return/unwind); no calling up the suspension ladder. (7) Validated against the runtime: the coroutine machine is overwhelmingly **substrate** (FlowFrame, continuation-splitting §11.1, spill-on-park, name-keyed save, value_stack handoff) — the REFACTOR/NEW work concentrates in HIR/checker; §10 of the doc tags every component SUBSTRATE/REFACTOR/NEW as a build-scope map. **Deferred to stub issues:** flow concurrency/structured spawning (#1210), a real effect-system core calculus (#1211), post-landing runtime restructuring (#1212). Entry-mode dual is v2. **Folds into B0.8** (code-body lowering).
- **WHY:** The design surfaced from three "this construct has no clean HIR home" frictions in B0.7 (return→DONE as Divert, match-arm-as-block, single-line-colon `else`) plus the G-2 brace ambiguity — all the same "constructs embedded in content" root. Unifying blocks as effect-signed expressions dissolves that class; the coroutine framing was already what `flow-suspension-spec.md` built ("serializable, recompile-stable coroutines"), so the model is mostly *naming what the runtime already does* plus value-returning flows as the one genuinely new capability. Maintainer ruled value-returning flows, sequences-as-effect, and uniform syntax liberalization directly; ratified the whole doc after review.

## Block/effect model — scoping correction + minimal-orthogonal-core north star
- **WHEN:** 2026-07-20
- **PROJECT:** brink
- **SYSTEM:** language design + HIR/analyzer/codegen (native surface); refines the 2026-07-20 block/effect ratification
- **SCOPE:** moderate (right-sizes the block-model claims to what the code supports; sets a directional north star)
- **WHAT:** An architectural scoping pass on the block/effect migration corrected two overstatements in the ratified `docs/block-effect-model.md` and set a north star. (1) **One shared body IR already exists** — `Block`/`Stmt` in `brink-ir`, both frontends already target it; the per-construct types are `Stmt` variants *inside* `Block`, not separate bodies. So the migration is **evolve `Block` in place** (add `tail` + effect signature + structural-kind side-data), NOT introduce a shared type. (2) **The two-step "relocate ink-HIR into `brink-syntax`" plan is rejected** — it inverts a dependency edge into a cycle the codebase already ruled against (`lower_native/mod.rs` judgment call #1) and manufactures a second body structure (the very thing to avoid). Instead the ink-shaped `Stmt` variants become a `brink-ir`-private module that stops being the cross-frontend interface. (3) **Faithful-superset verdict = SUPERSET-WITH-SIDE-DATA, not clean erasure:** the structural control-flow kinds (`SequenceType`, `CondKind`, choice sticky/fallback/guard/trisection, `ChoiceSet.continuation`) survive to LIR `ContainerKind` and drive codegen — they persist as **side-data on `Block`**, they are NOT dissolved into effects. "Sequence is just an effect" was aspirational; the effect row is *additive annotation*, the structural kind is load-bearing. So the model genuinely unifies the **surface / tail / effect / coroutine** axes (the real wins) but NOT the structural control-flow zoo — forcing those together would be a false unification (lying about codegen). (4) **Coupling:** analyzer MODERATE→SURGERY (~250 construct-keyed sites across ~12 passes; the effect subsystem already exists so effect rows are additive), HIR→LIR lowering SURGERY (the oracle-critical dense consumer), codegen MECHANICAL/firewalled (0 `hir::` refs — the oracle safety valve: freeze LIR `ContainerKind`, prove byte-identity at 5,577 after every slice). (5) **Size: arc, not a separate track** — buildable-now slices S1–S5 (tail; structural side-data; re-point analyzer; effect rows; B0.8 native bodies emit `Block`) plus the FS-3r-gated coroutine tail (suspend/ladder, ANF, value-returning flows) that rides the already-parked FS-3r. (6) **North star — RULED direction (#1213):** the language should ultimately reduce to a **minimal set of completely orthogonal concepts** with constructs derived from that basis; first candidates are folding `Conditional` + `Sequence` into one `Select { branches, discipline }` (`Choice` stays distinct — interactive + convergent) and collapsing LIR `ContainerKind` (overlaps #1212). **Explicitly sequenced after the block/effect wins** — orthogonalizing a basis still being assembled is premature.
- **WHY:** The maintainer pushed back that the side-data compromise "feels like we're not getting the benefit we wanted." Correct: at the structural level the compromise moves the distinction onto a `Block` field rather than erasing it. But the structural kinds are genuinely different machines (visit-counted cursor / predicate branch / interactive-convergent choice) and are load-bearing to codegen — the design hitting that wall is it finding a real joint, not failing. The real wins (surface, tail/G-2, effect axis, coroutines) are orthogonal to the structural zoo and land regardless. The deeper reduction is a legitimate goal but belongs after the concepts exist, hence the north star + deferral.

## `flow main()` ratified as the native story-entry convention; first light run (3/9 respell fixtures episode-identical)
- **WHEN:** 2026-07-21
- **PROJECT:** brink
- **SYSTEM:** language design (native surface, story entry) + HIR lowering + test infrastructure
- **SCOPE:** moderate (closes #1106's entry-convention question; builds the first native HIR→episode harness; two systematic divergences found and precisely root-caused, deliberately not force-fixed)
- **WHAT:** (1) **Ratified**: a top-level, non-function, zero-parameter `flow main()` is a native story's default standalone entry point — RustScript-idiomatic, no new syntax, mirrors `fn main()`. `lower_native::lower` (`crates/internal/brink-ir/src/hir/lower_native/mod.rs`, `entry_root_content`) synthesizes `root_content` as a single `Divert` into `main`, reusing the existing `Divert`/`Block` HIR — the same mechanism ink's own root-weave-is-the-entry lowering already uses. No `main` ⇒ `root_content` stays empty (not an error — host-entry-only, effects-spec §10 "play from here" unaffected). This supersedes the interim top-level bare `-> flow` entry-divert spelling the `exhibit-fogg-passage` respell fixture used (PR #1202) — all 9 `tests/tier1-brink-respell/*` fixtures were re-respelled to the `main` convention. (2) **Root-caused and fixed, one commit**: bare `return` inside a non-function container (e.g. a `flow` reached by tunnel call) was unconditionally stamped `ReturnKind::Explicit` by `lower_native`'s `RETURN_STMT` arm, tripping E032 ("return outside function") for valid tunnel-return code — native's single `return` keyword unifies ink's `~ return` (function) and bare `->->` (tunnel return), and the lowering had no access to the enclosing container's `is_function` flag at the point a single item is dispatched. Fixed with a small post-lowering pass (`fixup_return_kind`) called once per `Knot`/`Stitch` with the known `is_function`, walking every structural nesting point and recomputing `tail`. (3) **Built**: `brink-test-harness::corpus::{compile_and_explore_from_brink_native, explore_from_brink_native}` — the honest minimal native pipeline (parse → `lower_native` → `brink_analyzer` (dialect `Brink`, single file, no `brink-db`/salsa, no INCLUDE graph) → `lir::lower_to_program` → `brink_codegen_inkb::emit` → `brink_runtime::link` + explore) — and `crates/internal/brink-test-harness/tests/first_light.rs`, which diffs each respell fixture's native episodes against its paired ink oracle. (4) **First-light result, reported honestly, not forced green: 3 of 9 fixtures episode-identical** (`manual-stitch-v1`, `weave-options`, `sticky-choice`). The other 6 fail, but every failure reduces to exactly **two** precisely-diagnosed, deliberately-unfixed systematic issues (both flagged rather than patched, per the "honesty is the whole point" mandate — see the full build report for exact diffs): **(a) a `brink-syntax-native` parser bug** (not a `lower_native` bug — out of this build's scope) — `content_items_until`'s shared prose-scanning loop (`crates/internal/brink-syntax-native/src/parser/content.rs`) calls `p.skip_ws()` unconditionally at the top of every loop iteration, discarding significant inter-token whitespace (e.g. the space after a choice's `[]` bracket close, or after a `<>` glue marker) that ink-equivalent text requires — affects `basic-tunnel`, `complex-flow-v1`, `exhibit-fogg-passage`, `gather-basic`; **(b) an open semantics question this ruling doesn't answer** — ink grants literal ROOT content a free pass to end implicitly (no `-> END` needed), but does not grant that grace to a knot/flow's content reached by divert (that's ink's own "ran out of content" runtime error). Wrapping former root content in `flow main()` moves it from the first bucket to the second, so a `main` relying on ink's root-content grace now errors instead of ending cleanly — affects `basic-tunnel` (also hit by (a)), `const-vars`, `simple-glue`. Recorded as an open question in `docs/native-surface-charter.md` §14, not decided here.
- **WHY:** The entry-convention question was the last blocker on `docs/b0-sequencing.md`'s critical path to first light (B0.6 → B0.7 → B0.10-checkpoint) and had already surfaced once as a fidelity gap (PR #1202's interim top-level-divert workaround, explicitly flagged "confirm as the ruled entry convention in the G-batch"). Running the respell corpus end-to-end for the first time is this project's explicit standing value ("correctness above all... a correct fix to the wrong layer is worse than no fix") in action: it found one real, narrowly-scoped `lower_native` bug (fixed) and two systematic issues one layer down (parser) and one layer up (runtime/ruling-semantics) that a narrower "just make the fixtures parse" milestone would never have surfaced. Both are reported precisely rather than papered over so the next slice (or a follow-up ruling) can address them deliberately.

## Effect system for the native surface: one unified row, checking-not-handlers, bounds+row-poly
- **WHEN:** 2026-07-21
- **PROJECT:** brink
- **SYSTEM:** language design + analyzer (effects); amends `effects-spec.md` (§14) and corrects `block-effect-model.md` §3
- **SCOPE:** architectural (reopens the closed T2 "row dimensions" question to add two dimensions + reframe the row as THE effect signature)
- **WHAT:** Settled the native-surface effect-system design (full write-up: `effects-spec.md` §14; `block-effect-model.md` §3 reconciled). (1) **Checking discipline, not algebraic handlers** — handlers live at the host boundary (the `FlowInstance` API is already the suspension-handler interface; engine code is the handler) + the named structured-concurrency primitives (#1210); there are **no in-language handlers** and (maintainer-confirmed) no writer-authored handler scenario. Consequence: the checker needs **no effect-row subtraction**. Bounded further by the runtime's **one-shot, serializable** continuation (the FlowFrame — name-keyed, no stackful trace), which structurally excludes multi-shot handler use-cases (backtracking/nondeterminism) and leaves exactly the linear coroutine/scheduling family. (2) **The effect row is THE one canonical, extensible effect signature** — every consumer queries it (host scheduler, wake-map, coloring checker, fusion, and consumers not yet identified; maintainer: "I don't think we're done finding consumers"). Explicit reversal of the transient "row vs color vs tail as sibling systems" framing: **color folds into the row**. (3) **Two new dimensions:** `suspend(rung)` (folds the suspension color in; the no-call-up-the-ladder rule is an inferred check over it) and provisional `terminates` (`-> END/DONE`; sibling to `faults`; consumer = structured-concurrency lifetime). **Out of the row (on the merits):** general control-transfer (a plain divert) stays structural — the block `tail`; and sequence-impurity (visit cursor) stays out per the §10/NS-A6 ruled posture. (4) **`reads` is a dependency axis (coeffect), not an effect** — it does not make a def impure; the two ruled purity predicates stand (strong `@[effects(pure)]` reads-free; weak E105 wake-gate reads-OK), and **fusion uses the weak one** (a fold may read a stable global). (5) **Bounds + shallow row-polymorphism are complementary, both already in the ruled spec** — `Ty::Fn` row variables (§6.1, ruled) *propagate* callback effects (fuse-when-pure), `@[effects(…)]` upper-bound assertions (§10) *constrain* them; the maintainer's "bounds, default ⊤ maximal, `pure`=∅" instinct is the constraint layer. (6) **§6.1 row-polymorphism is IN-SCOPE, not deferred** — without it every call through a fn-value is opaque/pessimal, gutting effect precision across the native higher-order core (lambdas, fn-value iteration); it's tractable (substitution over creation-fixed rows). (7) **Reconciliation flags for a later pass:** §11's "no function coloring" non-goal (held consistent: the suspend dimension is *inferred*, never author-annotated — no coloring syntax, no virality); and the pre-existing `await` posture gap between `effects-spec.md` §13.1 (await = future/host-driven) and `flow-suspension-spec.md` §3 (await = ruled statement syntax). (8) **Build reality:** the shipped `EffectRow` (set-based row, per-SCC fixpoint, `EffectRows` wire format, `@[effects]` assertions) is reused unchanged; the work ADDS the two dimensions, wires row inference to native-lowered HIR, and builds §6.1. Off the "author a scene" critical path.
- **WHY:** A grounding pass (against the code) + a reconciliation pass (against the ruled `effects-spec.md`, at the maintainer's prompting) showed the effect-system design was mostly **already ruled and partly built** — the design conversation was re-deriving it, and an early framing (a fresh `Emit/Transfer/Suspend/World/Impure` lattice in `block-effect-model.md` §3) conflicted with the shipped row. The maintainer corrected two coordinator over-reaches: treating the spec's *prior omission* of suspend/terminate as a *ruling against* them (it wasn't — they weren't on the table), and proposing to defer §6.1 (which would make effects pessimal exactly where the native surface lives). Net: the row becomes the single extensible effect signature; most "new" work is wiring + two dimensions + building a ruled-but-unbuilt piece, not a new system.

## The block-effect insight distilled: meaning from analysis, not the parser; S2 dropped, #1213 downgraded
- **WHEN:** 2026-07-21
- **PROJECT:** brink
- **SYSTEM:** language design (native front-end); refines the block/effect model
- **SCOPE:** moderate (reframes the thesis; drops a migration slice; downgrades a north-star issue)
- **WHAT:** Distilled the durable insight of the block/effect design and corrected the framing accordingly. (1) **The thesis is "meaning comes from analysis, not the parser's guesses."** The parser stays syntax-directed and never disambiguates by heuristic; wherever the *same* surface can mean different things, the **type/analysis layer** resolves it (the tail's type decides interpolation-vs-control; the inferred effect row decides what a block does). `Block`+`tail`+effect-row are the *machinery* that enables this; they were never valuable as a structural unification. The precise rule: the parser still commits where syntax is *genuinely distinct* (`{~a|b}` vs `{cond:…}` vs `{? *…}`); only the **ambiguous overlaps** move to the type layer. `block-effect-model.md` §1 rewritten to lead with this. (2) **The structural unification is low-value — dropped as a plan.** The scoping already showed the structural kinds (Sequence/Conditional/Choice) are different machines that survive to LIR `ContainerKind`; collapsing them onto `Block` *relocates* the discipline dispatch (variant → `.discipline` field) rather than erasing it, and `Choice` resists folding. The real payoff is narrow (dedup the triplicated branch-walk + aesthetic) — a tidiness win against oracle-guarded surgery. So **#1213 (minimal orthogonal core) is downgraded from a sequenced north-star to a *guiding value, not a committed slice*** (bias new design toward the small basis; collapse the variants only if the pain is felt or a new construct benefits). (3) **The old "S2" (structural-kind side-data on `Block`) is DROPPED from the migration** — the kinds already live on the `Stmt` variants (readable, correct, LIR-bound); moving them onto `Block` *is* #1213. The chain is now **S1 (tail, done) → S3 (consumers read off `Block`) → S4 (wire the ruled effect row + build §6.1) → S5 (native bodies emit `Block`)**. (4) Both `effects-spec.md` §14.5 reconciliation flags **resolved**: §11 "no function coloring" (the suspend dimension is inferred, never annotated; `flow-suspension-spec.md` §4 already ruled the compatible "color = the existing fn/knot distinction, no virality" position) and the `await` posture gap (§13.1's "future direction" wording was stale drift — `await` is planned per flow-suspension-spec §3).
- **WHY:** Maintainer distilled it: "the insight was having some of the lowering come from types, not from parsing." That is the thing that held up across the sitting; the structural-unification packaging did not (the maintainer questioned it, and the honest cost/benefit is thin). Framing the doc around the durable insight — and stripping the migration of the slice that chased structure — keeps the model honest about what it actually buys.

## Flows end implicitly (native): no `-> DONE` ceremony; fall-through = DONE, `-> END` stays explicit
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design + native HIR lowering (native-surface flow termination)
- **SCOPE:** moderate (a native-dialect termination rule; unblocks first-light `main`-entry fixtures; adds one follow-up lint)
- **WHAT:** A native `flow` (and any braced body — choice branch, conditional arm) that **runs out of content ends implicitly**; no trailing `-> DONE` is required. (1) **Implicit fall-through lowers to the DONE terminal** (turn/flow complete), NOT END. `-> END` stays the *explicit-only* authorial act — "the story is permanently over, no resumption." This is the "permanent things are loud, benign things quiet" gradient: the common, benign end (falling off the bottom) is silent; the rare, consequential one (`-> END`) is spelled. `-> DONE`/`-> END` remain available as explicit spellings (charter §11 keeps them verbatim) — they just stop being *mandatory*. (2) **This kills ink's "ran out of content. Need a `-> DONE` or `-> END`?" runtime error on the native surface.** Rationale walked and steelmanned: ink's error exists because ink's `===`-delimited knots have *implicit* extent (a knot runs until the next header), so "ran out" is genuinely ambiguous — did the author stop on purpose or forget a divert? **Native retires that ambiguity twice over**: flow bodies are **brace-delimited** (`flow f() { … }` — the `}` is an explicit boundary), and choice branches are **braced bodies too** (not ink's flat `*`-weave where a branch's extent is invisible until the next `*`/gather). With every body explicitly closed, "a body that runs out just ends" is one uniform rule across flows and branches; the maintainer's read — "with our flow-body-block syntax it feels extra unnecessary, doubly so given our choice syntax" — is correct. (3) **Value-returning flows are the one exception, unchanged:** a flow that declares a return type must produce a value (the coroutine/state toggle, ratified 2026-07-20); falling through *without* a value is a **checker** error ("declares a return type but may fall through without returning"), not a runtime "ran out of content." The return-type declaration is the toggle: no return type ⇒ ends implicitly as DONE; has one ⇒ must return. (4) **The one thing ink's error was genuinely good for — an asymmetric choice branch that dead-ends while its siblings divert (a fingerprint of a forgotten `->`) — is relocated, not lost:** captured as a **low-priority, opt-in analyzer lint** (#1219), NOT a hard error and NOT a first-light blocker. Braced branch bodies weaken even this signal (the leaf has a visible boundary), so it is a heuristic warning, not a safety net. (5) **Implementation:** rides the BE-S1 `tail` substrate — a non-value native flow whose `Block` tail is `Tail::Unit` gets an implicit DONE terminator during native lowering. **Native-dialect only** (`lower_native`): the ink pipeline's termination semantics are unchanged and oracle-guarded; ink knots that run out still behave exactly as before. (6) **Empirical check (done):** the oracle records `outcome: "Ended"` for pure root-runout (`tests/tier1/glue/simple-glue`), and the harness comparator (`brink-test-harness/src/oracle.rs:281`) **normalizes `Done`/`Ended`** ("both terminal, C# doesn't reliably distinguish") — so first-light byte-identity passes whether implicit-end emits `Done` or `End`. The choice of DONE is therefore made on **semantic** grounds (the Line-enum distinction is real for `bevy-brink` hosts), not because a test forced it.
- **WHY:** Maintainer: it "always felt a little unnecessary," and the brace-delimited flow/choice syntax makes it "doubly unnecessary." The steelman (does ink's error survive braces?) found the flow-boundary argument dies to the visible `}`, and the residual real value — catching asymmetric branch dead-ends — is a *narrow lint's* job, not a blunt global runtime error's. Native is a fresh language and need not inherit one of ink's best-known papercuts. Unblocks the first-light `main`-entry fixtures (`const-vars`/`simple-glue`/`basic-tunnel`) that regressed only because wrapping former root content in `flow main()` moved it from ink-root's implicit-end grace into a divert-reached knot that errored.

## Native project root: `brink.toml`'s directory defines `story::`; a rootless `.brink` is a named single-file project
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** native surface project layer (discovery + module identity); closes the open half of NF-3
- **SCOPE:** architectural (fixes the physical definition of `story::`, which is welded into `DefinitionId` save keys)
- **WHAT:** The **directory containing `brink.toml`** — found by walking up from the entry, reusing `brink-project-config`'s existing config discovery — is a native project's source root, and therefore the physical definition of charter §13.2's `story::`. Every `.brink` under it is in the compilation universe; each file's module path is its root-relative path (`market/barter.brink` → `story::market::barter`). A `.brink` file with **no `brink.toml` anywhere above it** is an explicit **single-file project** whose root is its own directory — a named, documented mode, not a silent fallback. Adding a `brink.toml` to a previously-rootless file therefore changes its module path and hence its `DefinitionId`s; that is a **recognized identity migration** riding the existing `#@was` machinery (modules-spec M-3), not a bug to be prevented. **No new `brink.toml` schema** — the config's *location* is the declaration; an explicit `source = "src"` key (NF-3's literal recommendation) is not built, deferred until a `src/` layout is actually wanted. Rejected: requiring `brink.toml` for all native compiles (breaks fixtures and scratch files to buy a guarantee the migration path already covers); anchoring on the entry file's directory (silently truncates the tree, contradicting "`story::` = the whole project").
- **WHY:** The source root is not a compiler convenience — it *is* the definition of `story::`, and `DefinitionId = (module, name)` welds the choice into save keys permanently (#719). The hazard in the interrupted B0.10b WIP was never that a fallback existed, but that it was **silent and identity-changing**: the same file resolved to `story::market::barter` or `story::barter` depending on whether a config happened to sit above it. Naming the rootless case as its own mode makes that difference a declared property of the project rather than an accident of the filesystem, and the codebase already owns the tool for the single lifecycle event it costs. Config-location-as-declaration also matches the convention writers know from `Cargo.toml`/`package.json` and needs no new schema. Corrected in passing: NF-3 is a *finding with a recommendation*, not a charter ruling — the WIP's comments cite "charter §13.2 / NF-3" for a decision that had not actually been made until now.

## Native source-loading seam: a `SourceTree` trait with a map-backed impl; the root is caller-supplied
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** native surface project layer (source discovery); resolves #1224
- **SCOPE:** architectural (defines the seam every host loads a native project through)
- **WHAT:** Native source loading goes through a **`SourceTree` seam** — `list(root) -> root-relative keys` plus `read(key) -> source` — with three implementations: **real-filesystem** (in `brink-driver`, the CLI path), **git-revision** (fixes #1224's baseline diff), and **in-memory map** (web, LSP, and tests). The **save-key-critical rules stay central in `brink-db`** and are not per-host: keys are root-relative paths, the set is **sorted by key before `FileId`s are minted**, and `native_module_path(key)` derives the module. No host implementation can drift them. The **root is a caller-supplied parameter**, never discovered inside the seam: the CLI resolves it via `brink-project-config`'s `brink.toml` walk-up (per the same-day project-root ruling), while web and LSP pass their own project root. Rejected: a `read_file(path)`-closure-only seam (structurally cannot express enumeration — this is the #1224 bug); an eager `Vec<(key, source)>` producer (simpler, but forfeits laziness for the CLI/LSP single-file cases and makes the LSP overlay a special case rather than a composition).
- **WHY:** `read_file(path) -> String` can express "fetch this named file" but not "enumerate what exists", which is why the B0.10b WIP's `discover_native` silently ignored the closure and read the working tree — breaking `load_git_baseline`'s working-tree-vs-HEAD diff into a diff of nothing (#1224). But enumeration must also not be *demanded* of hosts that already own their file set: `brink-web` pushes files via `update_file` and deliberately links no filesystem at all (its `Cargo.toml` pins `brink-project-config` to `parse_str` only, and it never links `brink-driver`), so a wasm host cannot enumerate anything. A trait with a map-backed impl serves both — enumerate-capable hosts implement it honestly, push hosts hand over a map. The map impl's larger payoff is as the **test seam**: multi-file and determinism tests, precisely the gates the interrupted WIP never got, become hermetic and can feed hostile key orders that a real filesystem won't reproduce. The root stays caller-supplied because discovering it requires a filesystem walk-up — exactly the capability non-fs hosts lack.

## Merge policy: merge queue + branch protection; no direct merges to `main`
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** repo infrastructure (CI/CD, merge process)
- **SCOPE:** moderate (changes how every change lands, and how autonomous build waves finish)
- **WHAT:** `main` gets **branch protection** plus a **GitHub merge queue**. Nothing merges directly: PRs enter the queue, GitHub batches them, runs CI once per batch against the projected merge result, and merges only if green. Protection is **checks-only — no required review approval**: only accounts with write access can merge anyway, and fork PRs from non-collaborators cannot merge regardless, so a review requirement would gate only the maintainer and their own agents rather than adding any protection from outside contributors. Consequence for the build pump: its merge train must **enqueue** (`gh pr merge --auto`) instead of calling `gh pr merge`, and a wave now ends when its PRs are queued rather than when they are landed.
- **WHY:** On 2026-07-22, eight sequential merges to `main` each fired a full `main` pipeline (CI + release-plz + npm-release + Book) while six PR branches were simultaneously building. Measured: **19–20 jobs running against 72 queued**, oldest run waiting ~40 minutes, on an account whose concurrent-job ceiling is 20. Per-merge `main` pipelines are pure duplication when merges arrive in bursts, and a queue collapses them into one verified batch. Branch protection additionally closes the gap that let a merge land on `main` while another session's stack was mid-flight — which happened the same day and put a green PR into conflict purely through timing.

## CI consolidation: 29 jobs → ~12 by collapsing fuzz-smoke and sub-minute checks
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** repo infrastructure (`.github/workflows/ci.yml`)
- **SCOPE:** moderate (changes CI shape and failure granularity, not what is verified)
- **WHAT:** Collapse the **9 fuzz-smoke jobs into 1** (loop the targets within a single job) and the **7 sub-minute checks into one lint job** (Format, Clippy, Clippy all-features, no_std ×2, cargo-deny, publishable). E2E stays sharded ×4 and the Test variants stay separate — those are genuinely slow and genuinely distinct failures. Coverage is unchanged; only job packaging changes. Pairs with the `concurrency` group tracked in #1237.
- **WHY:** Measured on a real successful run: 29 jobs, ~60 job-minutes total. The fuzz-smoke suite alone was **9 jobs — 45% of the account's 20-slot concurrency ceiling — for 17 minutes of work**, and seven sub-minute checks (Format 0.3, no_std ×2 at 0.6, publishable 0.7, cargo-deny 0.8, Clippy 1.0) consumed seven slots plus seven checkout+cache setups for roughly 4 minutes of real work. At 29 jobs per run a **single push nearly saturates the entire account's CI capacity**, making multi-PR wave throughput bounded by job *count* rather than by work done. Accepted costs: coarser failure granularity (a red "Lint & static checks" is less legible than a red "Format"), and slightly longer wall-clock on the merged jobs.

## Native grammar supports flat `else if` chains
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface grammar) + `brink-syntax-native` parser
- **SCOPE:** minor/local (one grammar affordance; ruled in response to a parser-coverage-wave finding)
- **WHAT:** The native parser recognizes **`else if <cond> { … }`** (and the colon form) as a chained conditional arm — a flat `else if` is a supported spelling, handled in `at_else_arm` (`crates/internal/brink-syntax-native/src/parser/family.rs`), not only the nesting-based spellings that work today (`else { {if …} }` / `else: {if …}`). The chain lowers the same as explicit nesting.
- **WHY:** The #1191 native-parser test-coverage wave (issue #1197, PR #1247) found that a flat `else if` currently doesn't chain — `at_else_arm` only opens an `ELSE_BRANCH` when `else` is immediately followed by `{`/`:`, so the `if` tail falls through to prose/interpolation debris. Chaining works only via explicit nesting. Maintainer ruled the flat sugar should be supported: it is the conventional, expected spelling (every C-family and Rust-family language has it), and requiring authors to hand-nest `else { {if …} }` is a papercut with no upside. Distinct from the *silent-misparse* colon-`else` bug found in the same PR (same-line `else:` swallowed with no diagnostic), which is a bug to fix, not a feature to add.
## Studio E2E is de-required (advisory), not removed
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** repo infrastructure (CI/CD, branch protection) + brink-studio
- **SCOPE:** moderate (changes what gates `main`; keeps the coverage as signal)
- **WHAT:** The `E2E` check (Playwright against `@brink-lang/studio`, 4 shards) was **removed from `main`'s required status checks** but the job **still runs on every PR** — advisory, not blocking. A flake or a studio-shell breakage no longer blocks an unrelated merge; the green/red signal is preserved for anyone who looks. Not deleted, and not moved to nightly (yet — that stays open as a later throughput lever).
- **WHY:** brink-studio is a reference shell that is **no longer directly maintained** — the real editor-package consumer is an external embedding app. The instinct was to turn the studio E2E off. But the suite is doing double duty: ~5 specs are studio-shell chrome (`drag-redock`, `tab-drag`, `groups`, `theme`, `settings`), while ~15 are the **only end-to-end coverage of the wasm → editor-package → real-DOM pipeline the external embedder depends on** (`host-extension` — literally the embedder extension API — plus `extract-code-action`, `symbol-rename`, `file-rename`, `compiled-output`, `conversion`, `decorations`, `play-from-here`, `story-graph`, …). Deleting it wholesale would drop coverage of a **live external contract** whose exported-but-uncalled surfaces silently rot (the standing brink-studio caveat). The tests are not currently broken — every E2E run this session was green (the one failure was a runner bus-error, not a test). So the real cost was **merge-blocking maintenance burden on a required check**, not a correctness problem — and de-requiring removes exactly that while keeping the signal. It is also the CI throughput long pole (4 of ~16 checks, ~20 job-minutes); de-requiring is a step toward, but not the same as, cutting that cost (which would mean nightly-only). If/when the editor package grows its own integration tests, revisit splitting the contract specs out of the studio harness (candidate captured, not committed).
## Native content: alternation markers win over interpolation (`!`/`~`/`&`/`|`-led braces)
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + `brink-syntax-native` parser (`family::at_alternation`)
- **SCOPE:** minor/local (resolves a surface ambiguity found by the #1191 coverage wave)
- **WHAT:** A `{`-led content brace whose first token is an alternation marker — `!` (once), `~` (shuffle), `&` (cycle), or `|` — is **always** the alternation family, never bare-`{expr}` interpolation. To interpolate an expression that begins with one of those characters, **parenthesize**: `{(!x)}`. Concretely: `{!x}` is a once-alternation (preserving single-branch once like `{!greeting}`, "show once"); `{|x| x}` is a malformed alternation and errors (it is not lambda interpolation). `{-x}` still interpolates because `-` is not a marker.
- **WHY:** The wave (#1194) flagged that `{!x}` and `{|p| body}` can't reach interpolation even though `{-x}` can, an apparent inconsistency against charter §6 ("bare `{expr}` = interpolation, and nothing else, ever"). The initial fix idea was a lookahead (top-level `|` ⇒ alternation, else interpolation) to make both interpolate. Rejected: (a) it would silently break the single-branch `{!a}` once — flipping "show `a` once" into "render the boolean not-of-`a`", a bad trap; (b) **there is no real use case for interpolating a lambda** — a lambda in content position renders a function value, which is meaningless, so the `|`-carve-out was solving a non-problem (maintainer: "why would we ever want to interpolate a lambda?"). With the lambda case gone, the simplest rule wins: markers are markers, parens escape. `~`/`&` have no prefix-operator meaning so nothing is lost there; only `!` carries a `-`/`!` asymmetry, justified because `!` is genuinely overloaded (once-marker) and `-` is not.

## Native content: a `<-` outside a choice point is a warning, not silent prose
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + `brink-syntax-native` parser + diagnostics
- **SCOPE:** minor/local (a diagnostic on a deliberately-narrowed construct)
- **WHAT:** `<-` appearing outside a choice point emits a **warning-severity** diagnostic — not a hard error, and not the current silent-swallow-into-prose. The warning should be **higher-confidence when the token(s) after `<-` resolve to a real knot/flow reference** (near-certainly a misremembered ink thread), and softer when it is bare punctuation that may be intentional prose. Threads were narrowed on the native surface (charter §11, line 216: "no native spelling for general `<-`; only the scoped splice inside choice points survives") — the warning fires when an author reaches for the removed general-thread form.
- **WHY:** In ink, threads (`<- knot`) live in general weave content (harvesting choices into a hub), so a writer coming from ink will type `<- other_knot` out of habit; silently rendering it as literal text (current behavior) is the worst outcome. But a hard error is too strong — `<-` can legitimately appear as characters in dialogue (an arrow), so it must not block. A warning that escalates confidence on a resolvable knot target threads the needle: near-certain mistakes are flagged loudly, genuine prose is not rejected. The runtime thread machinery is unchanged; this is purely a surface diagnostic.

## Native decl: empty `flags` is spelled `flags F = ()`; a bare `flags F =` is an error
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + `brink-syntax-native` parser (`decl::flags_member_list`)
- **SCOPE:** minor/local (fixes the one asymmetric recovery path in `decl.rs`)
- **WHAT:** `flags F =` with **nothing** after the `=` is a **parse error** (emit a diagnostic, like the sibling zero-progress recovery paths in `decl.rs` already do — param list, struct body, use-tree list). The **explicit empty set** is spelled `flags F = ()` and is **legal**. Mirrors the ruled LIST behavior (a bare `LIST f =` is an error; the empty list has an explicit spelling).
- **WHY:** The wave (#1192) found `flags_member_list` silently accepts an empty member list on a bare `=` — it `break`s with zero progress and, uniquely among `decl.rs`'s recovery paths, records no diagnostic. That silent accept is a house-rule violation (silent drops are bugs). Maintainer ruled it should behave like LIST: bare-`=` is a mistake and errors, while an intentional empty set gets an unambiguous explicit spelling (`()`), so "empty on purpose" and "forgot the members" are distinguishable at the surface.

## Native content: empty alternation `{~}` / `{&}` is an error (brink-syntax parity)
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + `brink-syntax-native` parser
- **SCOPE:** minor/local (restores parity with the reference grammar)
- **WHAT:** An alternation with **zero branches** — `{~}`, `{&}`, `{&\n}`, etc. — is a **parse error**, matching brink-syntax's behavior (`sequence_stopping_empty_emits_error` / `sequence_symbol_empty_emits_error`), not the current silent-accept-with-empty-body.
- **WHY:** The wave (#1194, and the #1247 review) found empty alternations accepted with no diagnostic, a silent divergence from the ink/brink-syntax reference the coverage test was written against. No use case for a zero-branch alternation was identified; maintainer ruled to match parity rather than carry an undocumented native divergence.

## Native content: `{|…}` is always a stopping-sequence; no malformed-lambda concept (corrects the earlier ruling)
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + `brink-syntax-native` parser (`family::inline_alternatives`)
- **SCOPE:** minor/local (corrects the pipe clause of the same-day "alternation markers win" ruling)
- **WHAT:** A `{`-led brace whose first token is `|` is **always** a stopping-sequence alternation (charter §116-117: `{| }` is the stopping-sequence marker, a peer of `{~}` shuffle / `{&}` cycle / `{!}` once). `{|x| x}`, `{|heads|tails}`, and `{|heads| tails}` are **all valid two-branch stopping-sequences** — whitespace after the separator is ordinary branch content. There is **no "malformed alternation" concept** for the pipe form. A lambda in content position must be parenthesized (`{(|x| x)}`), exactly as `!` uses `{(!x)}`. The `inline_alternatives` space-after-first-separator heuristic (added in #1261) is **removed**.
- **WHY:** This **supersedes the pipe clause** of the earlier same-day entry ("alternation markers win over interpolation"), which said `{|x| x}` "becomes a malformed alternation and errors." That was made without fully reckoning with charter §116, which already rules `{|}` a real stopping-sequence marker — so `{|x| x}` is not a lambda to reject, it is a valid (if degenerate) two-branch stopping-sequence, no more surprising than `{~x}` being a shuffle rather than a "malformed not-expression." The heuristic that distinguished "malformed lambda" from "valid alternation" by whether a space followed the single separator was fragile and wrong at the boundary: it rejected `{|heads| tails}`, a perfectly ordinary spaced stopping-sequence, because it is byte-for-byte identical to a lambda `|params| body` — a collision the parser genuinely cannot resolve syntactically. Since lambdas-in-bare-interpolation don't exist (ruled) and always parenthesize, the consistent rule is simply "marker wins, uniformly": every `{|…}` is a stopping-sequence, and the paren escape is the one and only way to write a lambda there. Surfaced by adversarial review of PR #1271 (the recovered family.rs fix), which caught that the heuristic over-fired on valid spaced alternations.

## Native decl: `use { a, b }` (bare group, no leading path) is an error
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + `brink-syntax-native` parser (`decl::at_use_decl` / `use_tree`)
- **SCOPE:** minor/local (closes the last rulings-needed item from the #1191 coverage wave, #1256)
- **WHAT:** A `use` with **no leading path** — a bare group like `use { a, b };` — is a **parse error** ("a `use` needs a module path"), not a valid import. The unreachable bare-group branch in `use_tree` (which `at_use_decl`'s lookahead never routes to) is **pruned**; `at_use_decl` stays as-is (only `IDENT`/`::` after `use`). Every real import names a module: `use story::market::{barter, haggle};`.
- **WHY:** The #1192 coverage wave found `use { a, b }` is unreachable — `use_tree` has a bare-`{` group branch but `at_use_decl` never dispatches to it, so the form misparses as `{expr}` interpolation. A bare group has no meaning: there is no module to draw `a`/`b` from (a `use` group is the *tail* of a path — `use story::m::{a, b}` — never the whole thing). Rather than invent an implied scope (prelude? current module?) for a form no one needs, reject it cleanly and prune the dead grammar branch. Consistent with "imports are naming only": a `use` names a module and selects from it; with no module named there is nothing to select.

## Native module identity: pure function of the root-relative path; `FileId` never in `DefinitionId`
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** native surface project layer (b0-10b discovery wiring) + module identity / save-key stability
- **SCOPE:** architectural (fixes how `DefinitionId`'s module component is derived — save-key-critical, the #719 landmine)
- **WHAT:** A native file's module is computed **directly** as `native_module_path(root-relative path)` (`story::market::barter` from `market/barter.brink`), a **pure function of that file's own path** — it is **not** routed through the shared `resolve_modules` machinery that handles ink `#@module` declarations and INCLUDE inheritance. Native has no INCLUDE, so no inheritance; a file's module depends on **nothing but its own path** — no cross-file dependency, no include-graph input, no declaration-order input. Nested `module { }` blocks within a file extend the path deterministically (`…::barter::foo`). **Hard invariant (not a choice):** `DefinitionId = (module, name)` where `module` is this path-derived string; **`FileId` must never leak into `DefinitionId`** — `FileId`s are minted in discovery-sorted order, so a leak would renumber definitions when any file is added and break every existing save.
- **WHY:** The module string is folded into every `DefinitionId`, and `DefinitionId`s key players' serialized save state — so the derivation *is* the save-key contract. A pure-path function is the most stable possible derivation: the same file always yields the same module regardless of what else is in the tree, what order discovery ran, or whether the ink resolver's behavior later shifts. Routing native through `resolve_modules` (the interrupted WIP's `filesystem_derived`-flag approach) would couple native's save keys to machinery it doesn't need and widen the surface for the #719 class of instability. Rejected on that basis. (Moving a file, or adding a `brink.toml` that relocates the root, still changes its module — that is a declared `#@was` migration, ruled separately.)

## Native discovery ignores non-`.brink` files in the source tree
- **WHEN:** 2026-07-22
- **PROJECT:** brink
- **SYSTEM:** native surface project layer (b0-10b discovery)
- **SCOPE:** minor/local (discovery-walk scope)
- **WHAT:** The native discovery walk enumerates **`.brink` files only**; any other file in the tree (a stray `.ink`, reference material, a converter input, non-source files) is **silently skipped**, not an error. A native project's compilation universe is its `.brink` files.
- **WHY:** Charter §8.5 defers intra-story `.ink`/`.brink` mixing to a later round, so a native project is single-dialect (all `.brink`) by construction. A non-`.brink` file under the root is simply not part of the native universe — treating it as an error would be a papercut for anyone keeping `.ink` files around for reference or conversion, with no upside (it can't be a native source anyway). Least-surprising, and keeps the walk a pure "collect the `.brink` files" operation.

## Native visibility: top-level flows default to Private (module-scoped); cross-file refs use `use`
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) + module identity / visibility
- **SCOPE:** moderate (a native-surface visibility default; shapes multi-file authoring)
- **WHAT:** A native top-level declaration (flow, fn, var, …) defaults to **Private** — module-scoped. Because each native file is a **declared** module (path-derived identity, PR #1287), a symbol is visible within its own file/module by default; referencing it from another file requires an explicit `use story::…::name`. Single-file stories are unaffected (one module — everything mutually visible). This fell out of `declared: true` on native modules (declared modules default private in `effective_visibility`) and is **ratified as intended**, not a side-effect to undo.
- **WHY:** Charter §13.2 already rules "IMPORTS ARE NAMING ONLY … `use` grants source-visible names and nothing else" — the native model is private-by-default encapsulation with explicit `use` to bring cross-module names into scope. Private-default is the natural, charter-consistent behavior; a public-by-default would erase module encapsulation as the default and diverge from the ruled import model. **Not a save-key change** — visibility is not hashed into `DefinitionId`. Surfaced by the #1287 adversarial review as an emergent behavior worth ratifying explicitly rather than leaving implicit.

## Code-ground sitting — RustScript statements, blocks-as-values, UFCS, `until` condition-park (B0.8 surface)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** language design (native code/scripting dialect, charter §7); scopes B0.8 body lowering
- **SCOPE:** architectural (closes charter §7's "own sitting pending"; defines the scripting surface)
- **WHAT:** The code-ground ("scripting") dialect's surface, ruled in an interactive sitting. North star **RustScript**, expression-oriented, no tildes.
  1. **Rust-shaped statements:** `let x = e`; assignment `x = e` / `x.field = e` (RMW paths); `if c { } else { }`; `while c { }`; `for x in iter { }`; `return e`; bare expression statements. All lower to **existing HIR** (NF-2 fence — `Conditional`, `ForStmt`, `FnLiteral`, …); semantics inherited from the brink dialect (Track A), verified by differential-vs-brink-dialect HIR-equality tests.
  2. **Blocks-as-values:** `{ stmts; tail }` evaluates to its tail; `let x = { … };` is valid. Rides the BE-S1 `Block.tail` substrate.
  3. **UFCS:** `x.foo(y)` — field access wins; if `x` has no member `foo`, it desugars to the free function `foo(x, y)`. No method system; UFCS is pure sugar over free functions; reuses the analyzer's existing FieldAccess/Call ambiguity ownership.
  4. **`until <pure-bool-expr>;` is the flow-suspension construct — NOT `await`.** It parks the flow until the boolean condition becomes true (reactive), then resumes. `await` is **retired on the native surface**: it plants the wrong future-resolution mental model, whereas brink's construct is a **condition-park** — which the runtime's `FlowSleep` reactive-wake already implements. `until` reads as a prose stage-direction (`until the door opens;`) and matches the game-scripting `WaitUntil` idiom.
  5. **The `until` operand is pure and boolean, checker-enforced** — the wake machinery re-evaluates it, so it must be side-effect-free and read-only; its reads are what the wake-map keys on (reuses the existing `check_named_condition_purity` / `DetectSummary` machinery).
  6. **`until` is written; suspend is inferred.** No contradiction with the 2026-07-21 effect ruling ("suspend dimension inferred, no annotation"): `until` is an explicit control construct (there is no call-form for "park until true"), while the *suspend effect* on a flow containing an `until` (or calling a suspending binding) is inferred into its effect row. The writer never annotates suspension.
  7. **No value-await construct.** Getting a value from the engine / another flow is a plain call / return / choice with *inferred* suspension: a value-returning sub-flow returns its value; a choice yields its selection; a world-query binding call returns its result (pausing via `AwaitingExternal`/`resolve_external`, resolved out-of-band). None needs a keyword.
  8. **Time waits unify into `until` — no separate sleep/`WaitForSeconds` construct.** A duration is a condition over a clock: with an engine-exposed readable clock (`elapsed()`/`now()`), a time-wait is `until elapsed(seconds(5));` — an ordinary pure-bool condition that reads the clock and re-evaluates each tick (its `DetectSummary` is always-dirty) until the deadline passes. This unifies state waits, time waits, and combinations (`until door.open || elapsed(seconds(30));` — a timeout-or-event, which a two-construct design could not express) under **one construct on the existing condition-based runtime, with no new timer primitive**. Requires an engine clock binding as a surface dependency.
- **WHY:** The runtime already implements the condition-park (`FlowSleep`) and inferred suspension; the sitting gives them a native authoring spelling over built, tested primitives. The RustScript north star (charter §7) drives the Rust-shaped statements + UFCS, and existing-HIR-only (NF-2) keeps B0.8 a pure frontend-lowering slice with a ready differential oracle. The `await`→`until` change is a **clarity ruling**: `await`'s async-future baggage actively mis-teaches (every async-experienced reader gets the wrong model), while a condition-park is a distinct, game-scripting-idiomatic concept deserving its own word. Time-as-condition falls out of the wake machinery for free, so unifying beats a separate timer construct that would need new runtime — and it uniquely enables timeout-or-event as one expression.
- **Follow-ups:** NS-D book must lead with "`until` parks on a *condition*, not a *future*"; an engine-exposed clock binding (`elapsed()`/`now()`) is an NS-T/bevy-brink surface requirement; charter §7 updated from sketch to ruled.

## Native multi-file linking: codegen closure = the discovered module set (#1296)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** native surface — codegen/linking closure (`story_data_query`)
- **SCOPE:** architectural (defines native's compilation unit; unblocks multi-file native compilation)
- **WHAT:** A multi-file native project's codegen closure is **every discovered `.brink` module** — the native discovery set **is** the compilation unit. `story_data_query`'s current INCLUDE-transitive-closure scoping is ink-specific (native files have no INCLUDE edges); native gets a discovery-set-driven closure so all sibling modules link into the one `StoryData`. A `.brink` file that fails to compile is an error **even if no other module references it** (mirrors Rust: the whole module tree compiles; the tree is the unit). **Compilation universe ≠ execution entry:** every discovered module contributes its definitions to the linked `StoryData`, but the designated entry file still provides the *start flow*. Reachability-based dead-module elimination is explicitly a **future optimization** — it can only *subtract* from the closure, so it can never renumber/restabilize `DefinitionId`s and is save-key-safe to add later.
- **WHY:** Native discovery already ruled "a native project's compilation universe is its `.brink` files" (2026-07-22) — codegen ranging over that same set is the consistent completion of that ruling. It's the least machinery (the discovery walk already produces the exact set; no resolver-complete cross-module reference graph needed) and it matches the charter's Rust north star, where the module tree is the compilation unit and an unreferenced broken file is still a compile error. Reachability-from-entry was rejected as premature: it needs the cross-module graph now for a dead-code-elimination benefit a story project doesn't need (you ship what you wrote), and it stays available later as a pure, save-key-safe subtraction. Explicit manifest link-roots rejected as unnecessary ceremony when discovery is already authoritative.

## bevy-brink config discovery: brink.toml read-as-asset (hybrid w/ plugin override), bounded walk-up (#1029)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** bevy-brink — dev-mode InkLoader / project config discovery through the async AssetReader
- **SCOPE:** moderate (closes the gap where bevy stories compile with default AnalysisOptions — no way to set `dialect = brink` / `types = strict`)
- **WHAT:** A bevy-brink story's `ProjectConfig` is sourced **hybrid**: by default `brink.toml` is **read as an asset** through the async `AssetReader` — drained alongside the INCLUDE deps the InkLoader already BFS-fetches, registered as a load dependency (so it **hot-reloads**), parsed to `ProjectConfig`, applied to `AnalysisOptions` before the sync `brink_compiler::compile`. A **`BrinkPlugin::with_config(ProjectConfig)` out-of-band override wins** when set (packed/programmatic/embedded escape hatch). Location: a **bounded walk-up** from the entry asset — sibling-first, climbing ancestors **up to the AssetReader source root and stopping** (never above the mount). Precedence mirrors the existing `AnalysisOptions` doctrine ("explicit editor/API override always wins"); the read-as-asset default mirrors the CLI's `brink.toml` walk-up, bounded here by the virtual/packed mount.
- **WHY:** The InkLoader already implements the "drain the async AssetReader into a map, run the sync compile over it" pattern — `brink.toml` simply joins the drain, so the mechanism is nearly free and stays uniform with the CLI (config is a file, not a bespoke channel). Read-as-asset gives dev-mode hot-reload for free (dependency registration) and bundles into packed asset packs with no special handling. The plugin override is the escape hatch for embedded/programmatic builds and for anyone who wants config in game code — and having the override *win* matches the override doctrine already ruled for `AnalysisOptions`. An **unbounded** ancestor walk-up was rejected: a virtual/packed AssetReader mount has no meaningful "above the source root," so the walk-up is bounded by the mount — which also naturally terminates it (guard-against-unbounded-growth). Sibling-only and fixed-path were rejected as diverging from the CLI's walk-up (sibling-only forces a config per story dir; fixed-path forbids per-story override).

## Modular-artifacts round convened: load-time mounting, layered-precedence resolver, override via patch markers (#1093)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** format / runtime linking — per-module artifacts, DLC/UGC (V5 round; consolidates #717 + #848 + translation stability)
- **SCOPE:** architectural (a format-V5-scale round; makes the module the artifact unit and commits brink to an override-capable mod ecosystem)
- **WHAT:** The modular-artifacts round (#1093) convened; three **anchor rulings** settled, the rest drafted in `docs/modular-artifacts-spec.md`. **(1) Load-time mounting is mandatory** — the runtime mounts module artifacts at load, not only at build; UGC/mod content is discovered on the *player's* machine and can never be statically re-linked against a shipped base, so "static-link-always" is disqualified once UGC is a goal (the base may still assemble statically as its own build step). **(2) The linked-program invariant becomes a layered-precedence resolver** — base at the bottom, modules layered above by precedence; each `DefinitionId` resolves to the highest-precedence layer that defines it. Tractable because `link()` is already a `DefinitionId`→index map build and `DefinitionId = (module, name)` is globally unique + save-stable (same id = same definition regardless of carrying artifact). **(3) Override is in scope (total-conversion) but explicit** — a module redefining an existing `DefinitionId` **must declare it** (`#@override`, spelling TBD); an **undeclared** redefinition is a **conflict error** (one-definition rule, enforced at admission). **Shared state cells (`VAR`) are one-definition always** — the base owns the cell + its save slot; mods reference, never redefine (this closes #717's "missing 10%"). Precedence among legitimate overrides is a separate ordering concern (priority/load-order manifest, later).
- **WHY:** §13.2 made modules the compilation + addressing unit; making them the artifact unit aligns format with language, and the FG-4b/c symbolic-ref + relocation machinery is the oracle-proven linker foundation. The **UGC question drove the anchor**: asked "what would UGC/mods require," the answer disqualifies static-link-always (can't re-link against unknown-at-ship modules on the player's machine) and forces load-time mounting + admission/capability bounds as the security perimeter. The maintainer chose **override + total-conversion** (a genuine mod ecosystem, not just first-party DLC), which forces the one-definition-rule crux to the front. **Explicit patch markers over silent last-wins** because in a verified-admission system override must be intentional and auditable (admission verifies exactly what a mod replaces), accidental collisions should fault not silently resolve, and cells must stay base-owned or saves break under module-set churn. The remaining ~5 surfaces (chunk format, runtime resolver detail, admission granularity, version/compat stamps, per-module translation units, mount API + save churn) are drafted in the spec for a written-review round rather than resolved by chat poll — a round this size wants a doc.

## Direction (tentative): flows-as-actors — cross-flow function-value invocation is a message-send to the home flow (#597/#1210)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** language/runtime design — flow concurrency (#1210) + T1c function values (#597)
- **SCOPE:** architectural (a concurrency model; likely supersedes the 2026-07-12 creating-flow-identity ruling)
- **STATUS:** tentative
- **WHAT:** The maintainer's direction for the cross-flow function-value problem: **flows are actors, function values are messages to their home flow.** A function value that `ref`-captures a `#@local` cell is **homed to its creating flow**; invoking it from another flow B is a **synchronous message-send** — B **parks**, the home flow A **evaluates the callback body in its own context** (its `FlowLocal`/`World` as the context view; output isolated, transcript untouched — the `begin_function_eval` shape), then B **resumes** with the return. Execution relocates to the data; the data never leaves home. This is a **third option** beyond the 2026-07-12 menu (carry the cell ref cross-flow = creating-flow-identity, chosen; resolve against the caller's scope = late-binding, rejected) — both of those assumed the body runs in the *caller*; this one runs it in the *owner*, dissolving the dichotomy. **Likely supersedes** the creating-flow-identity destination for #597. The hard part it owns: **re-entrancy/deadlock** when a callback (running while its caller is parked) sends back into a parked flow — bounded/known (forbid callback suspension, forbid re-entering a parked flow, or detect-cycle-and-fault), unlike shared-mutable-memory's unbounded aliasing. Smaller opens: scoped-sub-eval vs. narrative-advance in the home flow (lean: scoped sub-eval, no advance), and what an emitting callback means.
- **WHY:** Creating-flow-identity relaxes the T1c fault by letting flow B mutate flow A's *private* `#@local` cell — which **breaks the exact privacy invariant `#@local` exists to provide**, and turns every cross-flow write into a determinism/aliasing hazard that must be journaled as raw memory writes. The actor reframe removes the violation at the root: only A-context code ever touches A's locals, so privacy holds by construction. It rides machinery that already exists and is oracle-exercised (`begin_function_eval`/`resume_function_eval` + `AwaitingExternal`/resume = "evaluate a function value in an isolated context, pausing/resuming on suspension"), retargeted to another flow's context. Determinism gets *easier*: a journaled synchronous message-send with a defined return, not a stream of cross-flow writes. And "flows are actors" **is** a flow-concurrency model, so #597 is not a standalone patch — it's a sub-question of #1210, which becomes the round that settles it.

## Compilation environment as a deterministic, serializable input; Project/SourceTree is the producer (#1306)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** compiler architecture — the compilation-environment seam / determinism boundary
- **SCOPE:** architectural (a determinism-boundary refactor; umbrella that dissolves the config-plumbing cluster and reserves the library/external-module seam)
- **WHAT:** Separate the **effectful producer** from the **pure input**. `Environment` = a reified, content-addressed, **serializable** input value (source set + content hashes, project config, a reserved `resolved_deps` slot, entry) — the determinism boundary; `compile(Environment)` is a pure function (already standing architecture: query-shaped compilation over salsa `ProjectDb` inputs, which the oracle ratchet's stability depends on). `Project` / `SourceTree` are explicitly the **producer** side — mount-specific, effectful, where all ambient reads and (future) dependency resolution live: `Project::load(tree, overrides) -> Environment`. **Ruled: `Environment` is serialized/reified NOW**, not deferred to when libraries arrive — the boundary is explicit, cache/repro-ready, diffable from day one. The config cluster folds in as "the producer resolves config into the `Environment`": #1029 (bevy, already ruled — becomes an instance), #1051 (studio seam), #1160 (lint control plane), #591/#592 (fmt), the LSP mount (#1131); the bounded walk-up moves from RealFs `find_config` onto `SourceTree.list/read` (mount-agnostic), and the ink `read_file`-closure vs native `SourceTree` duplication is retired. External libraries (**explicitly NOT on the roadmap** — designed-for, not built) = external **modules** with stable `(module, name)` identity → the reserved `resolved_deps` slot; **the-tree-is-the-universe generalizes to the-environment-is-the-universe** (`{ local module tree } + { resolved external module set }`), and compile-time libraries mount with the **same** identity/linking machinery load-time DLC/UGC modules use (#1093) — one mechanism, two phases.
- **WHY:** brink already compiles as a pure query over salsa inputs, so the determinism boundary *exists* — but the inputs are pushed imperatively (`set_file`/`set_entry`/`set_analysis_options`) with no nameable value to hold, hash, serialize, cache on, or diff. Reifying `Environment` as a serializable value turns "deterministic if you set it up right" into "deterministic by construction, with a reproducible, hashable input artifact" — enabling build caching, reproducible builds, and input diffing. **Serialize-now over defer** because the boundary's whole value is its explicitness; deferring keeps the implicit imperative-input coupling and pays the reification cost later against more call sites. Separating producer from input quarantines every ambient/non-deterministic behavior (fs walks, AssetReader drains, dep resolution) on the producer side, so compilation can never accidentally read ambient state — the environment is genuinely an input, not a process. `Environment ≠ Project`: `Project` is the effectful resolver, `Environment` is the pure product. Prior art the maintainer cited: Cargo.lock (pinned resolved set → deterministic build), Zig's explicit build graph, Bazel's hermetic sandbox — resolution is effectful and pinned; the build over the pinned set is deterministic.

## Native interleaving & body-dialect spelling: `~`=enter-code, `>`=emit-prose, at body + line granularity
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** language design (native surface) — charter §3 body-dialect axis + §8.2 interleaving escapes
- **SCOPE:** architectural (records the native interleaving system; closes a durable-record miss; defines the B0.8 parser roadmap)
- **WHAT:** The native surface's body-dialect selection and prose↔code interleaving use **two sigils with one mnemonic each — `~` = enter code/logic, `>` = emit prose output — at two orthogonal granularities:**
  - **Whole-body selector** (the brace on a declaration's body): **plain `{ … }` = the per-keyword default** (`fn` → code-ground, `flow` → prose-ground); **`~{ … }` = code-ground body** (statements written directly, no per-line `~`; a code-bodied `flow` is the "Compound guard" of §3); **`>{ … }` = prose-ground body** (emits bare text; a prose-bodied `fn`). This is §3's promised "every combination honestly spellable, defaults per keyword."
  - **Line/block escapes within a body** (§8.2's "grains" — the term was loose, it just means these fine-grained line escapes): inside a prose-ground body, **`~ stmt`** (or `~{ … }`) runs code — ink's logic line, kept; inside a code-ground body, **`> a line of text`** emits one prose line (and `>{ … }` a prose block).
  - The sigil is **orthogonal to granularity**: `~` always enters code, `>` always emits prose, whether switching a whole body or escaping a single line/block. Distinct from the *marker-inside* annotated-brace family (`{?`/`{if`/`{~`…), which annotates a content-embedded brace's kind — not a body's dialect.
- **WHY:** One mnemonic sigil per direction (`~` = logic, reusing ink's logic sigil; `>` = emit/output) keeps the system learnable and orthogonal across both granularities. Plain braces stay the ceremony-free common case — you reach for a sigil only for the non-default, so the 90% case pays nothing. Inside a `~{ }` code body, per-line `~` is dropped (matches the RustScript no-tildes code-ground ruling); the body-level selector subsumes it. Recorded now because it was decided but never written — the **third native-surface durable-record miss** in one session (after construction initializers #1103, and the body selectors surfaced alongside this); the charter §8.2 "grains" reference had no definition even the maintainer retained, which is what exposed the miss.
- **Follow-ups:** charter §4 gains the body-selector spelling and §8.2 moves from open-sitting to RULED; **#1309's scope expands** to the selectors + line escapes — a real parser gap, since none of `~{` / `>{` / `> line` / `~ line`-in-prose-body are parsed today. Motivates a charter-vs-parser conformance sweep before planning the B0.8 waves.

## SourceTree misfiled in brink-db → extract leaf `brink-source-tree`; `brink-environment` is the L3 producer (#1323/#1312/#1306)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** crate architecture / dependency layering
- **SCOPE:** architectural (fixes a foundational misplacement; unblocks config-over-SourceTree and the #1306 environment refactor)
- **WHAT:** The `SourceTree` trait (+ `RealFs`/`GitRev`/`InMemory` + the config-discovery walk) moves out of `brink-db` into a new **L0 leaf crate `brink-source-tree`** (depends on ~nothing). Rationale, from the actual dep edges: `SourceTree` is a **read-only virtual-source seam** ("provide the sources being compiled, by key") with zero conceptual dependency on the salsa DB/analyzer/IR — it only lived in `brink-db` because `discover_native` was written there. That was the misplacement; extracting it is a **correction, not a band-aid**. The naming is scoped deliberately: `source-tree` over `vfs` because it is read-only compiler input, not a general read/write filesystem — the narrow name draws the boundary and stops future conflation with artifact linking. The rational stack (all arrows point down): **L0** `brink-source-tree` + `brink-project-config` (already a clean leaf; gains a dep on source-tree for config *discovery*) + ir/format/syntax · **L1** `brink-analyzer` (consumes policy + IR) · **L2** `brink-db` (salsa queries over sources via source-tree + analysis) · **L3** `brink-driver` / future **`brink-environment`** (composes source-tree + config + resolved-deps + entry → the #1306 serializable `Environment`). The `Environment` value is L3 because it *composes* `ProjectConfig` + resolved sources — it cannot be the L0 leaf without recreating the cycle. Future-proof: libraries (#1093) arrive as compiled **artifacts** into `resolved_deps` (linked at L2/L3), NOT as re-parsed source, so they never flow through the source seam; DLC/UGC mounting is a runtime concern; source-tree stays honestly "the sources being compiled" at every stage.
- **WHY:** #1312 (config discovery over SourceTree) hit a package cycle (`brink-project-config → brink-db → brink-analyzer → brink-project-config`) because it needed `SourceTree` from `brink-db`. The build agent correctly refused to add the cyclic edge (house rule 7). The fix is not a shim but recognizing that `SourceTree` was in the wrong layer: a foundation parked in the top orchestration crate. Moving it to an L0 leaf both crates can depend on downward breaks the cycle by construction and is the honest home. The rest of the layering is sound — `project-config` as a dependency-free leaf is good, and the #1234 inversion (`analyzer → project-config` for `Dialect`/`TypePolicy`) is fine (policy flows downward from config into analysis), so neither is touched. `brink-environment` remains worth standing up, but as the L3 producer when #1306 starts, not as the cycle-breaker.

## Collection/construction initializer: `TypeName { }` is a std-registered construction protocol (#1103)
- **WHEN:** 2026-07-23
- **PROJECT:** brink
- **SYSTEM:** language design (native surface / stdlib) — construction & the protocol registry
- **SCOPE:** moderate (fixes the meaning under the already-settled brace syntax; shapes how every collection/struct is built in the code dialect)
- **WHAT:** `TypeName { … }` construction is **the protocol registry's 4th entry** (alongside `display`/`compare`/…), *not* closed compiler grammar dispatch — construction desugars to a `construct` protocol method (the C# `Add`-dispatch lineage). **This round, only std types register**; user-type opt-in (the `impl` spelling) is **reserved/deferred**. The brace *tokens* (element / pair / field forms) remain fixed surface grammar the parser produces — the protocol governs dispatch/meaning only. Three cascade rulings: **(A)** duplicate keys in a map literal (`Map { k:1, k:2 }`) are a **compile error** (new E076-lineage code), consistent with struct dup-field; **(B)** the **total** literal (`Weighted { … }`, faults on an invalid table — the 90% value-position case) ships this round, and the **validating constructor (→ `Option`)** for data-driven/runtime tables is **ratified as a protocol member but its user-facing spelling is deferred** (parked with the impl-spelling); **(C)** a spread / from-existing form (`Map { ..other, k:v }`) is **deferred** — no demonstrated demand, extensible later at zero grammar cost.
- **WHY:** Making construction a protocol keeps it symmetric with the registry doctrine already ruled (display/compare are protocols — construction as grammar-only would be the lone exception) and future-proofs new collection types (`Heap[T]`, host types) joining the literal grammar with no grammar churn. It also gives evidence-by-construction (Weighted's refusal of invalid tables, the §7 refinement pattern) a principled home: the fallible `construct → Option` is just a protocol variant. Std-only opt-in this round captures that symmetry and the fallible form without prematurely committing the user-facing `impl` spelling. Dup-key-as-error follows the "a typo shouldn't silently win" discipline (last-wins would swallow the mistake). Defer B's spelling and C entirely to keep the round tight — the code-dialect writers (B0.8, building now) need only the total literal.

## Native prose dialect to be rethought from the ground up (#1351 supersedes the "second pass")
- **WHEN:** 2026-07-24
- **PROJECT:** brink
- **SYSTEM:** language design (native surface, charter §5 prose-ground) + NS-T editor/renderer
- **SCOPE:** architectural (reopens the prose surface in full; gates the entire editor workstream)
- **WHAT:** The native **prose dialect is to be redesigned from the ground up** — superseding the standing "ruled = first-pass sufficient, revisit later" framing (charter §8 refresh, 2026-07-23). **Charter §5 is reopened in full**, including items ruled in the first pass: choice points as `{? … }` blocks, the **dissolved gather**, the scoped splice `<- flow(args)`, the kept choice-line anatomy (`[]` display-split, `<>` glue, divert arrows), and the `~`/`>` line escapes + `~{`/`>{` body-dialect selectors (ruled 2026-07-23). The round is a redesign, not a deepening; it must decide explicitly **what it keeps**, since keeping is free and changing has a landed-code cost (B0.7 prose lowering, first-light fixtures, `emit_native` prose arms + the `brink-respell` corpus, and all of `brink-syntax-native`'s prose parsing already ship). The **editor track (NS-T #1131/#1350) stays held** behind it.
- **WHY:** Prompted by a grounding survey of the shipped studio editor (recorded on #1351), which surfaced two facts that make redesigning *now* — before NS-T builds on the surface — much cheaper than redesigning after. **(1) Elision is a promise, not a behavior:** charter §2 requires every structural mark to be "renderer-elidable," but the shipped editor deliberately elides almost nothing (only dialect hidden-groups `@`/`:<>`/`<>` and repeated sigils at depth > 1), and the studio spec states "without replacing the underlying syntax" as its explicit thesis; the reveal model is also *inverted* from Obsidian (hide unconditionally + hold the cursor out via `atomicRanges` + a `transactionFilter`, with no cursor-proximity reveal anywhere). So what elision *means* on the native surface is still genuinely open, and the marks can be designed with the renderer in mind rather than retrofitted. **(2) Line-local vs nesting:** the editor's whole classification pipeline is line-local (`classifyLine(text)` sees one line), while `~{`/`>{` selectors and `{? … }` blocks make a line's dialect a **nesting** property and the dissolved gather makes depth a **tree** property rather than a prefix count — so NS-T was never a retokenize, it is an architectural revisit. A surface designed against that constraint (or deliberately overriding it) is far cheaper to render. The repo already flags NS-T, not the parser, as the real schedule risk ("the writer validates through the editor" — `docs/b0-findings.md`), which raises the cost of getting the prose surface wrong.

## Prose-dialect rethink, sitting 1 — element/markup two-layer model, XML spans in the line table (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (native prose dialect, #1351 ground-up rethink) + line-table/wire format + intl
- **SCOPE:** architectural (defines the prose surface's data model; supersedes charter §5's first-pass direction; full draft in docs/prose-dialect-spec.md)
- **WHAT:** Sitting 1 of the prose rethink, ruled interactively. **(1) Goals/north stars:** element-typed prose for a Scrivener/CELTX-lineage writer (each paragraph *is* a typed element); Fountain as format posture (reads naturally raw, decorates in CM6); Yarn Spinner as markup-data posture (spans → character-range attributes to the host); mechanism-over-bespoke (screenplay conventions are the flagship *preset*, the facility is the product); **explicit format, editor-supplied ergonomics** (marks are real, decorations soften — no parser inference); **superset doctrine** — the new output model is a superset of current runtime behavior, today's ink output being the degenerate case (untyped narrative, no spans) sitting unchanged within it. **(2) Two layers, both wire-deep:** block layer = elements (declared line patterns + chain rules, the shipped at_cue dialect promoted to first-class; classification static, never runtime-dependent), inline layer = markup. **(3) Elements are annotative; structure only gets costumes** — the exceptions are spellings of existing structure: a scene heading may declare a stitch (slug explicit-optional/inferred-default; heading text = display name rendered on diverts; retitle-with-inferred-slug = a rename caught by the `@[was]` net), and a transition dresses the divert that follows. **(4) Markup = XML tags** (v1 XML-only, markdown sugar deferred); blunt lexing (`<`+letter opens, `<>`/`<-` lexically distinct) with `\<` `\{` `\#` `\\` escapes and unknown-escape = error; **freeform by default, validated against a host-manifest markup vocabulary when declared** (the externals pattern; vocab is host-authored/generatable, elements are project-authored in a dedicated file referenced from brink.toml — brink.toml holds pointers, not content; co-locate declarations only when authored/generated by the same source). **(5) Nesting doctrine:** a tag closes in the fragment scope it opened in (markup×logic nest, never overlap; straddling a branch = compile error); spans are line-scoped (mechanically forced by the locale line-count invariant); spans are presentational, interpolation stays the only dynamic channel. **(6) Wire (grounded by the line-table survey on #1351):** spans live in the line table as a **nested `LinePart::Span{name, attrs, children}`** — NOT the runtime fragment model (flat value-slot vectors; right spirit, wrong mechanism — recorded so it is not re-derived); nesting-by-type makes unbalanced translations a decode error rather than rendering corruption; **span hash-transparency ruled now, before any markup ships** (markup normalized out of `source_hash` like interpolations — deciding later is a mass TM invalidation); the `.inkb` v6 + `.inkl` bump is acknowledged and to be **batched** with the intl-spec's future recognizers; the line recognizer must admit spans (else marked lines shred into per-run entries and stop being single translation units); `Line` grows a structured span surface, structural parts preferred over byte offsets (post-trim hazard). **(7) Conventions-as-brink-module is a deferred door held open by construction:** the conventions schema is defined *as a value*; the producer can later compile-and-evaluate a code-ground-only conventions module (proc-macro staging rule) into the same Environment slot — v1 reads a data file. **Open threads** (⏳, in the spec): runtime representation (the general typed-line shape), translation round 2, runtime output surface, choices×elements, dynamic element payloads (`@{challenger}`), NS-T implications.
- **WHY:** The rethink's grounding surveys reframed the design twice: the editor survey showed elision is a promise the shipped renderer never cashed and that classification is line-local — so the format must be explicit and the editor supplies the feel (decorations), rather than the parser inferring; and the line-table survey overturned the working assumption that markup could ride the fragment model, relocating spans into the line table where translators and locale overlays actually live. XML tags satisfy XLIFF's paired-inline-code model nearly by construction (xliff2 already models `<pc>`; only brink-intl's mapping needs writing), and nesting-by-type turns the well-formedness doctrine into a structural guarantee at every layer. Hash-transparency and the batched format bump are the two decisions that are cheap now and ruinous later. The superset doctrine keeps ink compatibility by construction: the general model contains today's behavior as its zero case, so nothing forks. The authorship test (who writes/generates a declaration) cleanly split markup vocabulary (host manifest, generatable from engine code) from element conventions (project file, Rust+TS-interpretable per the dialect-JSON precedent).

## Prose-dialect sitting 2 — break-compat ruled, element roles replace glue ceremony, completions doctrine, measurement tiers (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) + runtime output contract + editor tooling + display metrics (#362)
- **SCOPE:** architectural (continues sitting 1; docs/prose-dialect-spec.md §§3.6–3.7, 5–7 updated)
- **WHAT:** **(1) Breaking the runtime output contract is RULED acceptable** — no external consumers exist; in-repo consumers migrate in-PR, `@brink-lang/web` takes a major; not gratuitous, but compat holds no veto over structured output. **(2) Element structure replaces glue ceremony (RULED):** elements declare a role (attached-forward / content / structural); the runtime proceeds past a cue into its dialogue because the schema says so, not because the author wrote glue — glue demotes to literal text joining. Mechanism (leaned, final call open): attachment resolves at **compile time over the source chain** — attached lines emit nothing, content lines carry baked data; control flow cannot tear an attachment; a dangling cue is a compile diagnostic. Consecutive-lines/blocks direction 🔶 under discussion: per-line emission + baked block id, block delivery as API sugar. **(3) Completions doctrine (RULED): harvest-by-default, declaration-upgrades** — cue names complete project-wide from usage (a new project-db index obligation), the optional cast roster and the manifest markup vocabulary upgrade completion with validation/types/docs; **schemas are tooling-grade** (editor-consumed fields are legitimate schema); **succession rules** (Scrivener Enter/Tab) live in the conventions file as the editing-time dual of chain rules. **(4) Measurement (#362 becomes a consumer of this round):** host declares per-element budgets + per-span width behavior in the manifest; overflow is a **lint** through the `[lints]` plane (deny-able → a CI guarantee); **measurement tiers RULED with variable tracing explicitly OUT** (maintainer skeptical, conceded): static text always measurable — including translated line tables, the intl overflow check being the killer application; slots via opt-in declared allowances (`slot_info` width hint; roster-bound cue names are a lookup, not tracing); otherwise honestly unmeasurable, never guessed. Host measurer callback as escape hatch; recognizer growth (#1446) promoted to measurement prerequisite (variants must be enumerable). **(5) Runtime output shape 🔶 PROPOSED (not yet ruled, spec §7):** split content from terminality (`Step::{Line, Choices, Done, End}`, terminals carry no text), `OutputLine { element, parts, tags }` with `.text()` derived not stored, `Choice.parts` making choice text first-class prose.
- **WHY:** With no consumers, compat-driven design (the stored `text` twin field) loses its justification — one source of truth beats a compatibility shim for nobody. Element roles capture the real insight that a cue was never *content* — encoding attachment at compile time over the source chain is what makes the feature robust (emission-order grouping breaks under control flow) and moves failure to compile time. The completions doctrine extends the already-ruled freeform/declared pattern so the whole prose layer follows one rule: usage is free, declaration adds power — which incentivizes rosters/manifests instead of mandating them. The measurement retreat (no variable tracing) trades an overpromise for the tractable tier that catches the bug class that actually ships: translated static text overflowing localized UI. The maintainer's dialogue-box use case ("this line is too long, make some edits") lands as a lint precisely because the lints control plane was just built — severity-configurable, host-parameterized, CI-enforceable.

## Prose-dialect sitting 3 — product vision: the interactive screenplay (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) — product vision, choices, runtime output ratification
- **SCOPE:** architectural (the product frame the syntax round designs against; docs/prose-dialect-spec.md §2b)
- **WHAT:** **(1) The vision (RULED): the document is linear dramatic prose on an interactive skeleton — and the two never wear each other's clothes.** Branches read as screenplay (full conventions, no compromise, within any linear run); structure reads as structure (choice points, transitions, scene boundaries are their own visual register, made beautiful by the editor, never disguised as pseudo-screenplay elements). No-costume ruled *in principle*, with one reserved aesthetic pass: choice-point syntax must *naturally complement* the conventions — the syntax round's check. **(2) Choices are typed prose (RULED):** an option's text is element-typed by the same conventions as any line (dialogue-choice with `speaker`, action-choice as imperative narrative; a cue above a choice block types the options via chain rules — the Telltale pattern). `Choice.element` is the same machinery, not a special case. **The ink `[]` choice anatomy is re-ratified unchanged** — it already answers spoken-vs-summary per choice; the maintainer does not intend to change it. **(3) Scene-grained is the golden path; the editor dissolves the granularity tension (RULED):** linear leaves on a graph spine is the industry's own interactive-screenplay model and what the conventions serve best; conversation-grained weaves stay fully powered; the bridge is editorial — **scrivenings-style inline-destination view** (select a choice, see its target rendered in place), **extract-to-stitch refactoring** (inline choice body → knot/stitch + auto-divert), and **story-graph visualization** — chartered to NS-T, gated on this round. Composition is the editor's job; the format stays honestly graph-shaped. **(4) Per-path export is in the vision (RULED):** any single playthrough renders as a genuine linear screenplay (Fountain/FDX table reads, per-character VO recording sheets); the machinery is adjacent to the intl pipeline (the same extractor-over-line-tables family as the xliff exporter, inheriting line-identity work); consequence — the element set must map cleanly onto what Fountain/FDX can express. **(5) Riders ruled this sitting:** `.text()` derived-not-stored ✓; **dynamic element payloads ruled necessary as a capability** (even though the maintainer's in-flight project doesn't need them); with the Choice-element slot resolved by (2) and terminals worked via the parked cluster (#1448–#1450), **the §7 Step/OutputLine output shape is RULED in substance** — naming remains ⏳.
- **WHY:** The screenplay tradition has no vocabulary for "the reader decides," so pretending choices are screenplay constructs would lie in exactly the way this project has refused to lie everywhere else (explicit format, editor-supplied grace) — the no-costume principle is the honesty posture at product level. Choices-as-typed-prose reuses the element machinery instead of inventing a parallel scheme, and the re-ratified `[]` anatomy already carries the option-vs-delivery distinction with zero new surface. The scrivenings insight makes granularity a *view* question rather than a format question — the writer authors scene-grained leaves and the editor composes any path into continuous prose, which is the strongest version of no-costume. Per-path export turns the element layer from editor sugar into a production pipeline (table reads, VO sheets are real industry pain), and riding the intl extractor family means the feature inherits the identity/stability work already done for translation rather than growing a rival mechanism.

## Prose-dialect sitting 4 — concrete syntax: header-scoped scenes, [slug], line-neutral diverts, lowered transitions (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351, the syntax round) + charter §4 amendment + conventions schema
- **SCOPE:** architectural (the prose surface's concrete spellings; docs/prose-dialect-spec.md §8b–8c; docs/prose-element-inventory.md was the straw)
- **WHAT:** **(1) Lyrics dropped** (its `~` conflict dies with it). **(2) Header-scoped stitch bodies (RULED, amends charter §4):** a scene-heading stitch runs to the next heading or the enclosing close — the one exception to "braces are the universal body delimiter," scoped to preset heading-elements in prose-ground; heading-stitches are flat siblings (scenes don't nest, as on a real page); the braced `flow x { }` spelling stays first-class for nesting; this restores ink's own header-scoped stitch. **(3) Slug = trailing `[slug]`** (`INT. MARKET SQUARE - NIGHT [market] #tense`) — `#x#` rejected (tag-lexer clash), `{x}` rejected (lexes as interpolation; headings get no carve-out); line order pattern → slug → tags. **(4) Tags on declarations:** trailing `#tag`s on header lines (both spellings) become **container-level per-flow tags** — the authoring surface #474 was iceboxed waiting for. The conventions schema gains an **address-capture role** (slug feeds `DefinitionId`). **(5) Divert line-neutrality (native dialect):** diverts are invisible to line assembly — never end, join, or contribute to an output line; glue is the only joiner; placement is therefore pure formatting and fmt normalizes to own-line (choice-line trailing diverts exempt as anatomy); ink dialect keeps its oracle-bound joining. **(6) Transitions and scene entry are lowered HOST CALLS, not content lines** (maintainer direction: "essentially host function calls with an event type, non-blocking") — their runtime consumer is the engine, riding the existing command/extern machinery (non-blocking, journaled, effect-checked via the manifest); scene entry = the heading element's default lowering (`scene_entered(title, slug)` planted at the top of the body — pure codegen); written transitions (`SMASH CUT TO:`) = departure-site style calls; the bare scene divert needs no authored transition (the slugline implies the cut, as in real screenwriting); **diverts remain absolutely invisible — no annotations, no target-inference**. **(7) The lowering column (RULED, staged):** the conventions schema gains per-element `lower: content | call(name, args ← payload) | nothing`; v1 declarative; the §3.5 comptime conventions-module later computes the same value (maintainer's rewrite-module idea) through the already-open door; **power boundary: per-element call/content/nothing only** — arbitrary rewriting is a macro system, its own future round. **(8) Compact cue `@NAME: text`** accepted (the Yarn cross — cue+single-dialogue fusion as a second declared pattern). **(9) Choice typing recorded as 🔶 lean (c)**: cue supplies speaker, quoted options are spoken, unquoted/bracketed are action; mixed blocks legal; no cue → roster-PC attribution. **(10) The gated-questline case validated** (spec §8c): `~ until cond` + `-> next` at scene end parks the flow (FlowSleep), wake diverts, arrival fires scene-entry — zero new machinery; **parking surfaces at the FlowInstance/advance layer, never as a Step variant**; fused `until cond -> target` is optional sugar ⏳. **(11)** Point markers (`<pause/>`, `<sfx/>`) named as an explicit span use case; Yarn's persisted-line-ID model added to the translation-round-2 agenda as a fork vs `source_hash`.
- **WHY:** Header-scoping is what a screenplay *is* (scenes are next-slugline-delimited — braces would be the costume lying), and the flat-sibling constraint it imposes matches the form; ink's stitches already worked this way, so the amendment restores rather than invents. `[slug]` wins on lexical honesty — `{}` means logic everywhere in prose and a slug is not logic, while a heading is never a choice line so `[]` is unambiguous there. Line-neutral diverts make fmt's normalization *safe by construction* (in ink, divert placement changes output via line-joining; native's per-content-line model plus glue-as-only-joiner removes the trap). Lowering transitions to host calls fixes a channel confusion the maintainer caught: a scene transition is an instruction to the engine, not content the player reads — script-visibility is served by the source and the editor, runtime delivery by the command channel that already exists (non-blocking, journaled, effect-checked), costing zero runtime changes during the deferred runtime window. The lowering column completes the mechanism arc — classification, editing, export, and now runtime meaning are all declared per element — with the comptime module as its staged future source, and the power boundary keeps it from becoming an accidental macro system. The questline case is the composition proof: five prior rulings and two built subsystems interlock with nothing new needed.

## Prose sitting 4 addendum — conventions are authored as a brink module, not a data file (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) + conventions schema + producer/Environment
- **SCOPE:** architectural (flips §3.4/§3.5's v1-data-file posture; docs/prose-dialect-spec.md §3.5 rewritten)
- **WHAT:** **The authored form of project conventions is a `.brink` module** — code-ground only, exporting a pure `fn conventions() -> Conventions` (`@[effects(pure)]` is the determinism gate; proc-macro staging rule: the module cannot use the conventions it defines). The producer compiles + evaluates it (existing machinery: the compiler and `begin_function_eval`) and freezes the value into the `Environment`. **Presets ship as brink modules** (`std::conventions::screenplay`) so projects import-and-extend with ordinary value code rather than forking JSON. **The data/JSON form demotes to generated interchange** — the serialized value the TS editor consumes (evaluated through wasm; `brink conventions export` for non-wasm embedders) — and is never authored. The host capability manifest stays data (generated from engine code). Cast roster + PC identity live in the module. Cost assessed honestly before ruling: ~1.5–2× the conventions feature itself, zero runtime changes (all machinery exists), no new architecture — the Environment ruling pre-paid the staging. Design pass owed: the `std::conventions` types (extension ergonomics), entry convention (`fn conventions()` mirroring `flow main()`), `brink.toml` pointer key, portable-regex validation, editor re-eval loop, native-construction-literal sequencing (#1103 build; brink-dialect module acceptable meanwhile).
- **WHY:** The maintainer's judgment ("the manifest thing kinda sucks by comparison") is correct on the merits: a data file is a second, deader language for describing brink concepts — patterns, captures, lowerings as stringly JSON with no checking, no composition, and presets that can only be forked, never extended. Config-as-code (the build.zig / vite.config lineage) with a serialized lockstep artifact gives one source of truth: the module is authored, the JSON is compiled. The dichotomy was false — the value was always the interface (ruled at sitting 1); this just moves authorship to the live side of it. The purity assertion turns the effect system into the comptime determinism police at zero new cost, and shipping presets as modules is mechanism-over-bespoke in its strongest form.

## Prose sitting 4, addendum 2 — the @[element] annotation surface: call-lowered elements, capture routing, no invisible expansion (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) — element authoring surfaces + compiler tooling metadata
- **SCOPE:** architectural (adds the second authoring surface; flips v1 staging; docs/prose-dialect-spec.md §3.5b)
- **WHAT:** **(1) `@[element(pattern)]` on a fn is the second element-authoring surface (RULED):** a matching prose line lowers to a call to that fn — the maintainer's macro-rules-family sketch, adopted with its discipline named: an expansion that can only be one call has no hygiene problem; the body is arbitrary *runtime* code bounded by its effect row (existing machinery). **Zero comptime** — annotation metadata is static, the rewrite is ordinary lowering, so this surface needs none of the conventions-module staging. **(2) Two surfaces, one interface:** the producer folds annotations into the same resolved conventions value as the declarative side; the sitting-1 value-is-the-interface ruling survives any authoring form. **Role boundary:** annotations declare content/call-shaped elements only — attachment, chains, and structure stay declarative. **(3) Capture contract (RULED):** named captures bind to parameters by name (compile-checked at the annotation); `string` params get literals (static) or call-boundary stringification (dynamic); **`content` params receive a first-class content value via the existing fragment-capture path** (`BeginFragment…EndFragment → FragmentRef` — the same machinery behind display-position calls), with the captured prose compiling **through the normal line path — translation-resident and measurable**. Prose-bodied handlers (`>{ }`) give once-translated parameterized templates. Deferred: numeric coercion, context injection, Option-for-optional-captures. **(4) Staging flipped: v1 = built-in screenplay preset + annotations** (no comptime at all); the §3.5 conventions-module evaluation arrives later for authoring full custom presets — the module ruling stands, re-sequenced. **(5) No invisible expansion (RULED, maintainer requirement):** the compiler emits per-line classification metadata — matched kind, matching handler (fn + source location), capture bindings as spans, disposition — through the LineContext/ide query family; hover shows the handler body; an explain-match query answers is-this-matched/by-what/what-bound (and lists attempted patterns on a miss); capture spans double as decoration ranges. **Match ordering ⏳:** declaration-order + overlap diagnostics is the lean; rule owed.
- **WHY:** The maintainer's instinct located the real weakness of the pure-declarative framing — custom behavioral elements deserve locality (match and meaning in one declaration) and arbitrary bodies — and the annotation form provides both while *reducing* cost: it needs no comptime, so the favorite surface ships first and the expensive machinery defers until someone authors a full preset. Restricting expansion to a single call converts the macro family's dangers (hygiene, opacity, expansion blowup) into properties the language already polices — types and effect rows on a function body. The capture contract rides entirely on existing parts (fragments, slots, the line table), which is what keeps captured prose translatable and measurable instead of vanishing into arguments. No-invisible-expansion is the macro discipline's other half: every rewrite is inspectable (hover = body, query = match explanation), preempting the classic tooling failure of annotation-driven systems.

## Prose sitting 4, addendum 3 — sigil + name dispatch for annotation elements; @[style] editor hooks (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) — the @[element] annotation surface + editor presentation
- **SCOPE:** moderate (hardens the macro surface; dissolves the owed match-ordering rule; docs/prose-dialect-spec.md §3.5b rewritten)
- **WHAT:** **(1) Annotation elements are restricted to sigil lines with name dispatch (RULED):** `!name args…` — the maintainer proposed the sigil restriction; the adopted form goes one step further to **name dispatch**: `!` + first identifier selects the annotated fn directly (fn name or annotation alias), and the annotation's `args` pattern parses only the remainder into captures. Consequences: prose can never be silently claimed by a user pattern (every rewritten line is self-announcing — the explicit-format posture applied to macros); **the owed match-ordering rule dissolves** (name dispatch is unambiguous; duplicate names are ordinary duplicate-definition errors; an unmatched remainder is a targeted diagnostic naming line + handler pattern). Doctrine recorded: **pattern power proportional to auditability** — natural-notation pattern claiming stays on the declarative side (one centralized, auditable place); the scattered annotation surface gets name dispatch. Glyph `!` accepted with the Fountain force-action inversion noted (maintainer: "good point of reference, not married to it"). **(2) `@[style]` (RULED):** a companion annotation mapping captures and the whole line to **editor style hooks** — each emits a stable semantic class/data-attribute (the editor's existing class-contract pattern) plus optional basic inline defaults (color/weight/italic) that themes override; the conventions value gains the same style column so declarative elements use one field; **editor-presentation only**, firmly distinct from the runtime markup layer (output styling = the handler emits spans). Rides the already-ruled no-invisible-expansion metadata channel (capture spans as decoration ranges).
- **WHY:** Without a sigil, a user-declared pattern can claim natural prose in a way that is declared but invisible at the use site — the undeclared-inference sin re-entering through config; the sigil restores self-announcement at zero visual cost (decorations restyle sigil lines per project). Name dispatch over bare-sigil-plus-patterns buys the stronger properties: no ordering rule to write, no pattern competition to debug, and diagnostics that name their handler. The auditability doctrine explains why the declarative side keeps pattern power while annotations do not: a single conventions file is one place to audit; scattered annotations are not. @[style] extends tooling-grade schema to presentation with the same one-value discipline, and the presentation/markup boundary keeps the editor's decoration layer from being confused with runtime output styling.

## Prose sitting 4, addendum 4 — built-in presentation tokens for @[style] (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) — editor presentation vocabulary
- **SCOPE:** minor/moderate (extends the @[style] ruling; docs/prose-dialect-spec.md §3.5b)
- **WHAT:** `@[style]` values draw from a **built-in presentation vocabulary** — a closed, LSP-semantic-token-style set every conforming editor implements natively with no plugin or CSS: alignment (`left`/`center`/`right`), emphasis (`bold`/`italic`/`dim`/`mono`), case (`uppercase`), and `conceal` (implemented by the shipped hidden-span/atomic-range machinery; also the declared spelling for hiding the `!name` dispatch prefix). Raw color stays a basic theme-overridable default; unknown names remain custom hooks emitting stable `brink-*` classes. The conventions value carries the same style column — making **the screenplay preset self-describing** (transitions declare `[right, uppercase]`, cues `[uppercase]`, replacing hardcoded editor CSS; a bare token-conforming editor renders screenplay with zero configuration). Degradation stated up front: full set in the CM6 package; plain LSP carries emphasis/color via standard semantic-token modifiers, no alignment/conceal. Indent-level tokens considered and deferred.
- **WHY:** The maintainer's requirement — "this is right-aligned" must not need an editor plugin — is the LSP semantic-token lesson applied to presentation: a small standard vocabulary beats per-project CSS for the common cases while custom hooks keep the escape hatch. Making the preset self-describing through the same tokens converts hardcoded editor CSS into declared conventions, so custom elements get presentation identical to shipped ones — mechanism-over-bespoke kept at the presentation layer.

## Prose sitting 5 — rapid closure rulings: cue-only choice typing, tags-as-extensions, Step, open-map element data (#1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) — closure batch over the round's remaining opens
- **SCOPE:** moderate (closes nearly all remaining syntax-round items; docs/prose-dialect-spec.md §8d + §8b.10 + §9)
- **WHAT:** Rapid-fire maintainer rulings. **(1) Choice typing is cue-only** (flipping the recorded lean): the cue above a choice block types all its options as that speaker's dialogue options; quotes carry no typing semantics; no cue → plain action-choices (PC dialogue options require the PC's cue — explicit); non-spoken options need no rule — an all-bracketed option delivers nothing via the existing `[]` anatomy. **(2) Block id universal** — every run of same-element adjacent content lines carries one. **(3) Centered = `<center>` span**, not an element. **(4) Cue extensions ride the ordinary tag channel** (`@VENDOR #(v.o.)`) — no parsed capture, no new payload machinery; the cue line's tags attach with the cue's data; export mapping may translate known tags to Fountain extensions. **(5) Fused `until cond -> target` deferred** — two-line canonical; sugar can land later breaklessly. **(6) Escape set final**: `\<` `\{` `\#` `\\` inline + `\!` `\@` line-start; unknown escape = compile error. **(7) The output enum is `Step`** (naming closed). **(8) The output format bakes no scene-specific fields**: element data is an open map produced by conventions and handlers — time-of-day or anything else is preset-configurable data, never a privileged field. **(9)** Context-injection and numeric-coercion deferrals stand. The complement-pass page (everything applied) is recorded in the spec at §8d.
- **WHY:** Cue-only won because the quote mechanism was redundant machinery — the `[]` anatomy already makes non-spoken options deliver nothing, so the cue can type the block wholesale with zero per-option rules, and quotes return to being prose (less inference, the standing posture). Tags-as-extensions and open-map element data are the same judgment applied twice: reuse the existing channel over inventing a parsed field, and keep the wire format free of preset-specific privilege — what a scene heading captures is the preset's business, not the format's. `Step` marks the contract break honestly. The deferral of the until-sugar reflects its cost/benefit (identical semantics, purely additive later).

## Compile-baked attachment ratified (prose #1351)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) / compiler
- **SCOPE:** moderate (fixes the attachment layer for the prose build)
- **WHAT:** Attachment is resolved at **compile time**: cue and parenthetical lines do not survive as runtime output; the following dialogue lines carry `speaker`/`delivery` (and the cue's tags) in their element data. The runtime performs no chaining. (This had fallen out of the open-threads discussion informally; ratified explicitly on maintainer "yes".)
- **WHY:** The runtime stays dumb and each transcript line is self-attributed — translation, export, and hosts never reconstruct adjacency. Matches the standing "defer nothing the compiler already knows" posture.

## Block id is a dedicated OutputLine field, not element data
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) / runtime output format
- **SCOPE:** minor/local (format shape)
- **WHAT:** The universal block id (sitting-5 ruling: every run of same-element adjacent content lines carries one) lives as a **first-class field on `OutputLine`**, not an entry in the open element-data map.
- **WHY:** The open-map doctrine exists to keep *preset-specific* payloads out of the format; block identity is the opposite case — universal, present on every line — so it earns a typed field. Hosts group lines without knowing any preset.

## `!name` dispatch = ordinary module scope + brink.toml implicit usings
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) / module system
- **SCOPE:** moderate (dispatch namespace + a project-config surface)
- **WHAT:** `!name` element-annotation dispatch resolves through **ordinary module scope** — file `use` block plus prelude; there is no separate macro registry. To avoid per-file ceremony, brink.toml gains **C#-style implicit usings**: a project-config list of `use` paths injected into every file's scope (an explicit duplicate `use` is harmless; a file-local name beats an implicit one; ambiguity between two implicit usings errors at the use site demanding qualification). Exact table spelling and precedence text are straw pending the prelude-design docket item (charter §13.3).
- **WHY:** Maintainer proposed the C# global-usings model directly. It satisfies "technically imported" (the no-invisible-expansion doctrine holds: brink.toml is visible, checked in, deterministic-env input per #1306, and hover/explain-match names the supplying using) while a screenplay preset can still be available project-wide for free.

## Done vs End resolved by host lifecycle, not conformance (#1450)
- **WHEN:** 2026-07-25
- **PROJECT:** brink
- **SYSTEM:** runtime output contract / bevy-brink
- **SCOPE:** moderate (freezes the `Step` terminal variants)
- **WHAT:** Keep both `Step::Done` and `Step::End`. **Done** = turn complete; the flow (and its bevy entity) persists and may park/resume. **End** = story permanently over; the host despawns the flow entity. The axis is host-observable lifecycle, exactly as the #1450 dig charter demanded ("decide from bevy-host need, never conformance").
- **WHY:** Maintainer: "end can despawn the flow entity in the bevy side so i think i do see a bit of value in done vs end." That is concrete host-need evidence — the distinction drives a real lifecycle action, so merging the variants would push a per-flow liveness question onto every host.

## Slugify = tooling materializes the slug into the source (prose #1351)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) / editor tooling / save stability
- **SCOPE:** architectural (slug = stitch name = save key via DefinitionId)
- **WHAT:** Inferred scene-heading slugs are **materialized into the source by tooling**: on format/save, the inferred slug is written as explicit `[slug]` text after the heading; from then on it is ordinary explicit source. Renaming the heading title never moves the slug; re-deriving the slug from a new title is an **explicit editor refactor** (framed with its save-impact), never automatic. A "recalculatable" sigil on machine-written slugs was considered and **rejected** — it would reintroduce the silent-rename hazard materialization exists to kill. The slugify algorithm operates on the preset's isolated title capture (prefix/time-of-day never reach it): lowercase, non-alphanumeric runs → `_`, trim; same-scope collision = compile error demanding a distinct explicit slug. Rider (lean, unvetoed): the compiler accepts never-materialized inferred slugs (agent/no-formatter workflows) with a configurable `unmaterialized-slug` warn-by-default lint via the `[lints]` control plane; `fmt` is the auto-fix.
- **WHY:** The slug keys visit counts, parked flows, and diverts — an inferred slug that tracks the display title breaks saves on any rename, and the compiler can never warn because it never sees the old text. Materialization keeps zero-ceremony authoring (type the heading, hit save, the slug appears) while making stability structural rather than tooling-vigilance. Same explicit-format posture as everything else in the round: the mark is real, the editor softens it.

## XLIFF v1 excludes element data — element data is locale-invariant by construction
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** intl / prose rethink #1351
- **SCOPE:** moderate (fixes intl scope for the prose build)
- **WHAT:** v1 XLIFF export carries content and markup spans only; element data payloads are never exported. The boundary is the `@[element]` capture type: **content-typed captures ride the line table and translate; string-typed captures land in element data and do not**. Consequences accepted: element kind + data live in the base `.inkb` shared across every locale (locale line tables carry parts only); translators cannot corrupt machine-facing data on re-import; speaker/channel display names get no per-line translation (correct — consistency; localized display names later via a roster-level table or XLIFF v2 metadata groups, not a v1 blocker). Known trap, mitigated by discipline + tooling: reader-visible prose routed through a `string` capture silently becomes untranslatable — the rule is "reader-visible ⇒ `content`"; hover shows the handler, and a lint may later flag prose-looking string captures.
- **WHY:** Maintainer confirmed after ramification review. Keeps the intl pipeline's scope fixed (no open-map serialization in the XLIFF codec) and yields the clean invariant that a line's machine data is identical in every locale.

## `x or y` short-circuits — the landed eager evaluation is a defect (#1471)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** language design / codegen / runtime
- **SCOPE:** minor/local (observable evaluation order of one operator)
- **WHAT:** `or` coalescing **short-circuits**: the RHS is not evaluated when the LHS is `some`. PR #1469's eager both-operands behavior (honestly pinned + flagged unruled by the build itself) is a defect; #1471 files the fix (branch-based lowering, flip the eager pin to a short-circuit proof).
- **WHY:** `x or expensive_call()` must not run the call on the happy path — side effects make the order observable, and every peer language's coalescing operator short-circuits; eager evaluation would be a standing surprise.

## The `as` binding: one construct, both condition positions, `{if}` spelling
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** language design (Option surface, F27 lineage) / native syntax
- **SCOPE:** moderate (fixes the binding-form pattern for the language)
- **WHAT:** The two documented `as`-binding spellings (draft F16's statement form; the Option book chapter's template form) are **not competitors — they are the same `if` construct in brink's two condition positions**: `if EXPR as NAME { … }` in statements, and `{if EXPR as NAME: … else: …}` in templates, per the already-ruled template-conditional spelling (charter §, `{if cond: …}` — maintainer reconfirmed; the book chapter's bare `{EXPR as r: …}` is stale on the native surface and gets a docs fix). Semantics ruled once: the binding is **immutable**, typed `T` from `Option[T]`, scoped strictly to the success arm; in `while`, it rebinds each iteration; **v1 restriction: the `as` binding must be the entire condition** — no `&&`/`||` composition (let-chains lineage can land later breaklessly). **Explicitly deferred:** `as` in choice guards (`* {if cond} [text]`) — a guard evaluates at presentation time while the body runs at pick time, so a binding's freshness across that gap needs its own ruling before admission.
- **WHY:** Whichever binding grammar shipped first would set the pattern for all future binding forms, so the reconciliation had to precede the build. Unifying on the single `if` keyword adds zero new syntax surface (the template position already exists), and the whole-condition restriction keeps v1 scoping trivial to specify while leaving the chain extension purely additive.

## Choice-guard `as` un-deferred: capture-at-presentation, by-value (COW), rides v6
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** language design (Option surface) / runtime choice machinery / save format
- **SCOPE:** moderate (supersedes the choice-guard deferral in the same-day `as`-binding entry)
- **WHAT:** `as` in choice guards (`* {if EXPR as name} [text] body`) is admitted with **capture-at-presentation** semantics: the binding is captured **by value at choice presentation** — ordinary COW value semantics, the same by-value rule closure capture ruled — and the pending choice serializes the captured value across saves (the closure env-row precedent). The pick-time body sees the value the player saw; the world moves on independently; nothing can spook the capture because no aliasing exists (`as` binds the unwrapped payload, never a `ref`). Syntactic re-evaluation at the use site was **rejected**: a pick-time re-run can produce `none`, breaking the narrowing guarantee, and any fallback would be invisible control flow. Implementation is **sequenced with the `.inkb` v6 bump** (the Choice record grows a captured environment — transcript/wire codec territory, #1443 adjacency); until then #1475 diagnoses guard-`as` as not-yet-supported.
- **WHY:** Choices are already presentation-time snapshots — the display text bakes into the transcript, and ink's own model splits presentation-text from pick-time-body evaluation — so "what you saw is what you get" extends an existing seam to the binding rather than creating one. COW value semantics makes the snapshot free and sound: no special deep-copy, no second value semantics, no effect-system interaction.

## The COW no-aliasing invariant is load-bearing — sweep it, state it first-class (#1476)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** cross-system (runtime value model / type & effect system / docs)
- **SCOPE:** architectural (confirms the foundation; directs validation)
- **WHAT:** Confirmed on maintainer challenge: the **immutable, copy-on-write, no-aliasing value model is the core invariant** the type and effect system's soundness rests on — sigil-free mutators, by-value closure capture, T1e root-cell effect rows, and flows-as-actors all assume it; `ref` mutation is lvalue-rebinding, never shared-structure aliasing. Directive: (1) **sweep the implementation** to validate no shared-mutable aliasing leaks (#1476 — Value representation, every mutation path, captures, serialization, equality); (2) **state the invariant explicitly** in runtime-spec.md with cross-references, instead of leaving it scattered in unrelated rulings' rationale text.
- **WHY:** An agent misread the model as reference semantics ("binding stable, contents live") and the misread nearly informed the v6 Choice-record design — caught only because the maintainer challenged soundness. An invariant this load-bearing must be mechanically validated and impossible to misread from the specs.

## UFCS resolution pass designed: type-directed, in the analyzer, five rulings
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** language design / brink-analyzer (the pass #1322's scope-overflow note owed)
- **SCOPE:** moderate (unblocks B3 #1462 and every future UFCS-dependent verb)
- **WHAT:** UFCS resolution is **type-directed** name resolution and lives in **brink-analyzer, interleaved with inference** (field-access-wins is unanswerable without the receiver's type). Algorithm for `recv.name(args)` at a call position: infer `recv`'s type → if it has field `name`, field access (must be fn-typed; call the value, rows per #872) → else resolve `name` as a free fn in **ordinary lexical scope** (file `use` + implicit usings + prelude) and desugar to `name(recv, args)` → neither = one diagnostic naming both attempts. Only the final pre-`(` segment gets this; bare `a.b` stays field/container walking; each call in `a.b().c()` resolves independently; ink dialect untouched; explicit free-call always available. Rulings: **(D1)** a matching but non-callable field is a **hard error**, never a fallback to the free fn — field-access-wins means wins outright. **(D2)** The verdict is recorded in a **side-table** (node → resolved target), consumed by LIR lowering and the IDE hover/go-to-def queries; HIR stays immutable. **(D3)** Receiver type unknown at the resolution point = **error demanding an annotation** (E107-family posture) — accepted *for now*, with an explicit maintainer rider that this is **planned to be improved later** (smarter inference ordering / deferred resolution — tracked as its own needs-design issue, additive when it lands). **(D4)** The candidate set is ordinary lexical scope **only** — no method sets, no inherent impls; any in-scope free fn is method-callable. **(D5)** Auto-ref fires **only when the free fn's first param is declared `ref`**; the desugar spells the projection explicitly (`party.members[0].heal(5)` → `heal(ref party.members[0], 5)`, riding T1e ref-argument machinery); rvalue receiver + `ref` param = compile error ("cannot mutate a temporary"); non-`ref` first param = plain by-value desugar with no lvalue requirement.
- **WHY:** The semantics were ruled across three earlier sittings but the pass had no design and no owner, which is exactly how B3 stalled (the wave-55 build agent rightly declined to invent analyzer architecture autonomously). D1 keeps resolution predictable — outcomes must not depend on a field's type. D2 matches the salsa/incremental architecture and pays for IDE queries once. D3 trades inference cleverness for a clear diagnostic today, on the standing "tightening later is additive, guessing wrong is not" principle — with the improvement explicitly planned so the annotation tax is understood as temporary. D4 is what makes UFCS "just sugar" over the ruled import model. D5 composes mutation with the existing projection machinery instead of inventing a parallel one.

## Quick-docket closures (five rapid-fire rulings)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** cross-system (compiler conformance / stdlib / editor workflow)
- **SCOPE:** moderate in aggregate; each item local
- **WHAT:** (1) **#1448 greenlit** — the weave-terminator fix enters the wave queue (root-scope-only synthetic final gather; ratchet 5,577→5,598 in-PR; must not extend to non-root terminal containers). (2) **The `remove` divergence is accidental — made consistent by renaming, not posture-flattening**: seq remove-by-index → `remove_at` (the `_at` faulting-index family with `char_at`); `remove` uniformly names identity-based idempotent-total removal (#1484). (3) **View-materialization ratio: principle over numbers** — the mechanism is spec'd; the constants are implementation-tunable performance facts pinned by bench-counters, never semantics; tuned only with evidence. (4) **Weighted-table surface (`len`/iteration/mutation) ruled OUT for v1** — maintainer: agent invention, least valuable; construct-and-roll stands, anything further evidence-gated. (5) **Holes' release policy stays parked, re-homed to the NS-T editor round** — with the maintainer-noted alignment recorded: it is the same shape as the slugify workflow, so the lean when it opens is holes ride the `[lints]` control plane like the unmaterialized-slug lint.
- **WHY:** (1) The stated deferral condition ("ready to touch runtime/compiler in general") is met by the active Track B waves. (2) Each posture is right for its domain — an index is a structural claim, a key is a membership question — so the accident was one name spanning both; renaming preserves both contracts. (3) Unobservable-by-law sharing means thresholds carry no semantic weight; ruling numbers without evidence would be theater. (4) No dossier demand exists; the validating-constructor evolution remains the recorded door if it appears. (5) Ship-with-holes is an authorial-workflow judgment, which is editor territory — and expressing it as a lint tier reuses machinery instead of inventing release-gate ceremony.

## NG-C ruled: `: type` returns everywhere — the colon is the one annotation spelling (#1489)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** language design / native syntax
- **SCOPE:** moderate (fixes the return-annotation spelling language-wide)
- **WHAT:** Return-type annotations on native `fn`/`flow` headers are spelled **`: type` after the param list** — `fn probability(g: Guest): float { … }` — matching the ratified lambda convention (`|g: Guest|: bool`), so the language has **one return-annotation spelling in every position**. The Rust-arrow alternative (`-> type`, parser-accepting the DIVERT token in the arrow-safe header position) was seriously weighed for the RustScript header silhouette and **rejected**: the deciding case is composition into cramped, prose-adjacent annotation positions — concretely the scene-heading slug bracket, where a future parameterized/returning stitch spells `[market(supplies: int): QuestResult]` cleanly while an arrow would put a live divert token inside a scene heading. **Rider (heading gap recorded, not a defect):** heading-declared scene stitches are **parameterless and untyped in v1** — the heading is prose-ground costume for the plain case; parameterized or return-typed stitches use the explicit code-ground declaration form; growing the slug bracket (`[slug(params)]`, `[slug(...): T]`) is recorded as a purely additive later extension if writer demand appears.
- **WHY:** Maintainer, after weighing the torn case: "the arrow would be crazy in the slug position." One spelling that survives every position beats a prettier spelling that only works in headers — the north-star silhouette loses one battle so the annotation grammar never forks. The rider turns the noticed heading gap ("we came up with a syntax for stitches that doesn't account for parameters") into a chosen boundary instead of an accident.

## Lowering consumes analyzer types, never re-derives — the coalesce ruling generalized (#1492)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** compiler architecture (analyzer ↔ LIR boundary)
- **SCOPE:** architectural (a standing layer principle, applied first to coalesce)
- **WHAT:** On the PR #1479 blocking finding (Option-returning call as a coalesce chain operand faults under short-circuit lowering because `rhs_is_option_shaped` cannot see function return types), the maintainer ruled the strong form: **an ill-typed expression is rejected at analysis, before lowering exists; a well-typed one lowers mechanically off the analyzer's recorded types. LIR never re-derives typing with syntactic heuristics — it reads verdicts.** Applied to coalesce as #1492: an analyzer→LIR type side-channel (built on #1482's side-table pattern — one plumbing shape, two payloads), delete the heuristic, chain shapes from inference. Gradual-mode residue (lean): an unknown-typed LHS under brink-dialect gradual keeps the runtime check as its semantics (Option coalesces, plain faults) — strict/native never reaches it. PR #1479 re-drives on top; #1471's short-circuit ruling stands.
- **WHY:** Maintainer: "shouldn't we reject at lowering in the first place if the total expression doesn't type? why does LIR care at that point?" — the heuristic existed only because no channel carries the analyzer's verdicts down, and re-deriving types in the wrong layer is exactly where a blind spot becomes a runtime fault. Strict mode already computes everything the lowering needs (E066 reads it); the fix is plumbing, not policy. Recorded as a standing principle because the same wrong-layer temptation will recur with every typed lowering decision.

## XLIFF `<unit id>` reverts to DefinitionId-based — supersedes the 2026-03-14 "scope names as IDs" decision, in part (#1442)
- **WHEN:** 2026-07-26
- **PROJECT:** brink
- **SYSTEM:** brink-intl / intl-spec
- **SCOPE:** moderate
- **STATUS:** tentative — logged from the issue text by an autonomous build agent (no live chat approval was available for this run); flag for confirmation or edit at the next interactive session.
- **WHAT:** `<unit id>` in exported XLIFF is rebuilt as `{scope_id}:{line_index}` (the scope's `DefinitionId`, e.g. `0x0100000000000001:0`), not `{display_name}:{line_index}`. This partially reverses the 2026-03-14 "XLIFF uses scope names as IDs, not definition IDs" entry: that decision's `<file id>` half is untouched (still the human-readable scope name), only the `<unit id>` half changes. The display name rides along as the `name` attribute on `<unit>`, set to `{scope_name}:{line_index}` — the pre-#1442 id verbatim — so translator-facing tooling (Poedit etc.) that surfaces `name` gets exactly the old readable label. `brink-intl` gained `migrate_unit_ids`/`brink migrate-xliff` to re-key already-exported `.xlf` files in place (rewrites only the `id` attribute; content, state, and `brink:*` extensions are untouched, so no translation is lost), and `regenerate-xliff` re-keys for free on next recompile since it already matches by content hash, never by id.
- **CORRECTION (2026-07-26, applied during review):** the original text of this entry claimed the new scheme makes unit ids "stable across renames" / "survives a pure rename." **That is false.** A `DefinitionId` is itself a hash of the scope's (qualified) name/path (`manifest.rs::hash_name`, `hir/stamp.rs::alloc_address`, `lir/lower/context.rs`), so renaming or moving a knot/stitch still assigns a new `DefinitionId`, and `docs/intl-spec.md:415` says outright: "A knot renamed or moved gets a new `DefinitionId` and its translations are orphaned." `{scope_id}:{index}` churns on rename exactly as `{display_name}:{index}` did — and is *worse* for stitches, whose ids hash the qualified path (`knot.stitch`), so renaming the enclosing knot now churns stitch unit ids that the old scheme left stable. The real, still-valid benefits of this change are: a canonical NMTOKEN-safe id (display names can contain characters invalid in an XML `id`), consistency with the already-documented `brink:scope-id` extension and `IntlError::InvalidUnitId` format, and decoupling from a mutable, non-unique-across-scopes display field (collisions between same-named scopes are no longer possible). Real rename stability is a `DefinitionId`-level change that needs maintainer sign-off on #1442 and is not delivered by this PR.
- **WHY:** Issue #1442 (filed by the maintainer): renaming a knot/stitch changed every unit id under it, and — worse than mere churn — display names aren't guaranteed unique across scopes, so a rename could collide two units onto the same id (that collision risk *is* fixed by this change). The project already treats id-stability-across-rename as an architectural class of bug (#719 lineage: "save-key-landmine") — but this PR does not close that gap for renames, only removes the collision risk and the non-canonical id shape; see the correction above.

## Type-name surface ruled: angle brackets, uppercase non-primitives, Option/Weighted annotatable
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** language design (type annotation surface, cross-dialect)
- **SCOPE:** architectural (one atomic rename across grammar, specs, book, fixtures)
- **WHAT:** Four parts, resolving a split that already existed unnoticed. **(1) Angle brackets are the canonical type-argument delimiter** — `Option<int>`, not `Option[int]`. The specs wrote square brackets (`Option[T]` ×7, `Heap[T]`, `Weighted[T]`) while the implemented annotation grammar used angle brackets (`array<T>`, `map<K,V>`); they never collided only because the square-bracket types had no annotation grammar at all. Angle brackets win: they are what ships and is tested, they match the RustScript north star, and reserving `[…]` for values keeps it free for the just-ruled array literal `[1, 2, 3]` (#1490). **(2) Every non-primitive type name is Uppercase** — `Array<T>`, `Map<K,V>`, `Option<T>`, `Weighted<T>`, `Flags`, `Heap<T>`, **`List<T>`, `Handle<T>`**, and user structs (already uppercase). **Primitives stay lowercase**: `int`, `float`, `bool`, `string`, plus the keyword-ish `void`/`divert`. **(3) `Option<T>` and `Weighted<T>` become annotatable** (#1552) — `Option<T>` has real demand (#1168: a user fn returning an Option escapes strict inference with no annotation escape hatch). **(4) `range` stays construction-only**, deferred pending demonstrated demand for declaring one.
- **WHY:** Maintainer: "it's just foreign to have types which can be generic be lowercased like that." The decisive evidence is that brink already spelled one type two ways depending on position — the B5 construct literals are `Map { }` / `Flags { }` / `Weighted { }` (uppercase) while their annotations were `map<K,V>` / `array<T>` (lowercase). Capitalization now carries information — **if you can construct it, it is capitalized** — instead of being decoration, and `Map { … }` finally agrees with `Map<K,V>`. Primitives stay lowercase on the Rust/TypeScript precedent and because `int` reads better in a writer-facing tool. Cost accepted: a breaking, mechanical rename of the ink dialect's annotation surface, to land as ONE atomic PR with `brink fmt` able to rewrite.
## Three surface rulings: bracket array literals, dual-reading `use`, annotation-first global typing
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** language design (native surface / module system / typed mode)
- **SCOPE:** moderate (each unblocks filed build work)
- **WHAT:** (1) **NG-D — array/sequence literals are spelled `[1, 2, 3]`** on the native surface (#1490). The B5-symmetric alternative (`Array { … }` through the construct-literal grammar, zero new grammar) was weighed and rejected. (2) **`use` is dual-reading** (#1592): a trailing segment that resolves to a **module** licenses that module, exactly as Rust's `use` does; a trailing segment resolving to an item keeps today's behavior. The current silent no-op — nothing licensed, no diagnostic possible because a directory prefix is never in `declared_exports` — is retired; a trailing segment that resolves to neither must diagnose. (3) **Global typing is annotation-first** (#1549): a global's type comes from its declared annotation, not from inference over its assignments. The `VAR arr = 0; … arr = #[…]` reassignment idiom gets a type by **annotating the declaration**; assignment-derived inference is **explicitly left open for later exploration**, not rejected.
- **WHY:** (1) The brackets are already lexed and idle in expression position, and the everyday collection literal deserves the lightest spelling — construct-literal symmetry is worth less than not making the most common literal ceremonious. (2) Charter §13.2 already committed to "Rust's `use` syntax lifted verbatim", and Rust's is dual-reading; more decisively, a silent no-op is the one outcome nobody wants, and the item-only reading cannot even produce a diagnostic. (3) The two inference options both derive a **save-key-adjacent** type from control flow the compiler cannot generally order (cross-knot reachability is undecidable; "first assignment" is undefined across multiple entry points), and the standing posture is less inference with explicit marks. Annotation-first is additive: global inference can layer on later without breaking anything annotated.

## Frame-local projection receivers are legal; effect rows are a durable-cell concern (#1531)
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** language design (UFCS auto-ref × T1e projections) / effects
- **SCOPE:** moderate (resolves a live conflict between two standing rulings)
- **WHAT:** `let g = Guest { … }; g.hp.heal(5)` is **legal**. A **frame-local cell is a valid projection root**; **effect rows are required only for projections rooted in DURABLE cells**. This resolves the #1531 conflict in favour of the 2026-07-18 mutation-posture ruling (field-path receivers write through the ruled RMW machinery) and **narrows D5's "projection receivers ride T1e" deferral to the durable case only** — which #1530 unblocks separately. E143's current advice ("bind the receiver to a durable cell") must stop firing for frame-locals; it is presently a dead end, since struct-typed durable globals are not spellable (#1530), leaving projection receivers non-functional in *both* positions.
- **WHY:** Three reasons, and the maintainer confirmed no effects-side reason to block. (1) **Consistency with assignment**: plain field assignment already special-cases multi-segment paths (`try_lower_field_assignment`), so refusing `g.hp.heal(5)` while allowing `g.hp = 5` refuses the same mutation on spelling grounds — and when the mutator path failed to handle multi-segment paths, the COW sweep (#1476) classified that as a bug, not as design. (2) **The T1e objection dissolves rather than being overridden**: effect rows exist so the engine knows what a call touches; a frame-local root has no durable identity worth naming *and needs no row*, because the mutation is unobservable outside the frame — simpler than the durable case, not harder. (3) **COW value semantics make it safe by construction**: a local is a value, `ref` projection is lvalue-rebinding, closures capture by value, and `ref` params do not outlive their call — so escape is structurally prevented, not merely unlikely.

## Branch expansion stands; `Select` is target-side only; expansion count becomes an authoring metric (#1446)
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** brink-ir content recognizer / brink-intl / editor tooling
- **SCOPE:** architectural (settles the v6-blocking contradiction; adds an authoring surface)
- **WHAT:** The apparent conflict was **a full ruling versus a speculation that got scheduled as though settled**. The 2026-03-15 ruling ("Branch expansion for inline sequences/conditionals in content recognizer", moderate/architectural, no `STATUS: tentative`) **STANDS**: inline conditionals and sequences expand at compile time into the cartesian product of complete lines, each its own `LineEntry`; the runtime selects a line rather than assembling text from parts. intl-spec's "Future recognizers" section — which proposed routing the same construct into `LinePart::Select` — is **RETRACTED** (it was written speculatively, "*would allow*", and the same section records the shipped reality: "Select entries only come from hand-authored translations compiled via compile-locale"). **`Select` stays TARGET-side only** — the mechanism by which a *translator* introduces variation the source never had (gender, plurals). Consequences: **(a)** #1446's actual complaint (lines shredding into multiple translation units) is fixed by *implementing an existing ruling*, with **no format bump**; **(b)** v6 **loses** that payload — prose-dialect-spec §4.4's batching note amended; the bump still carries `LinePart::Span`, element data, block id, and the Choice captured environment. **(c) NEW — the growth guard is an authoring metric, not a compiler warning** (maintainer): expansion factor is exposed through the db/ide query layer so the editor can show *inline, while writing*, how many voice lines a line becomes, with policy in the `[lints]` tier. This is deliberately the **same surface as the ruled §6 measurement design** (host-manifest budgets, overflow-as-lint, tiered measurement) — a second metric on one authoring-metrics surface, not a parallel mechanism.
- **WHY:** Each branch combination IS a distinct translatable and **separately recordable** line, which is exactly what the ruled per-character VO recording sheets need; `Select` would hand a translator fragments with invisible alternatives and give a VO director nothing to record. The cartesian blowup the ruling accepts is honest — five binary conditionals genuinely are 32 recordings — so the right guard is **visibility while authoring** rather than a compile-time cap: the writer sees the cost as they create it, and project policy (not the compiler) decides what is too much. Filing the metric alongside text-length measurement keeps one surface for "budgets an author must respect."

## Flags verbs live in the type's companion module (#1541) — riding the 2026-07-19 airport ruling
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** language design (stdlib flags domain × the companion-module system)
- **SCOPE:** moderate (closes the #1541 dispatch-shape question by inventing nothing)
- **WHAT:** **Flags verbs are synthesized into each declared flags type's COMPANION MODULE** (the virtual module co-named with the type, ruled 2026-07-19, "The spelling cluster ruled: companion-module impls"). Consequences, all falling out of that ruling rather than being new: the three **type-level verbs** (`all`, `none`, `range`) are called as **`Mood::all()`** — no type name in value position is ever admitted, which the earlier `all(Mood)` table spelling would have required; the **instance verbs** (`count`, `contains`, `add`, `remove`, `intersect`, `first`, `last`, `index_of`) live in the same companion and are reachable as `s.count()` through the **already-ruled companion-first UFCS lookup** (receiver type known → companion → in-scope free fns), as well as `Mood::count(s)`. `Ty::List` narrowing at the call site remains the instance-verb dispatch mechanism. Authors extend their own flags types with ordinary `impl Mood { … }` blocks, which **merge** with the synthesized companion per that ruling; **redeclaring a synthesized name is a compile error, not a shadow.** Because bounded polymorphism is icebox (#1090), these are **synthesized per flags type**, not one generic verb. The verbs' already-ruled postures are untouched: `first`/`last` return `Option`, `index_of` keeps its ⚠F21 fault, `add`/`remove` stay idempotent.
- **WHY:** The maintainer recalled a prior ruling in this territory and was right — the airport sitting had already designed companion modules ("abusing modules — `Npc::greet` is the only actual name"), which answers the dispatch-shape question wholesale: `.` walks data, `::` names declarations, so `Mood.Red` (member) and `Mood::all()` (companion fn) cannot collide, and the compile-checked casing partition makes the collision class unspellable rather than merely avoided. Nothing new is introduced; the only decision was where the synthesized verbs live, and any other home would have re-invented the companion.

## Process: search the decision log before opening a design discussion
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** cross-system (agent process)
- **SCOPE:** minor/local but standing
- **WHAT:** Before opening any design discussion or presenting options on a language/architecture question, **grep `docs/decision-log.md` and the relevant spec for the territory first**, and cite what is already ruled. Two failures on 2026-07-27 prompted this: (1) a from-scratch derivation of associated-function dispatch that re-invented — worse — the already-ruled companion-module design (2026-07-19); (2) autonomous waves re-queuing issues already labelled `needs-design`, burning slots on declines. Related standing rule for build agents: never cite a value, `file:line`, or PR/issue attribution without re-reading it at the ref being cited (the w76 fabricated-verification rejection).
- **WHY:** The decision log is long enough that recall is unreliable, and a re-derivation that lands *near* an existing ruling is worse than no proposal — it silently forks the design, and the fork is only caught if a human happens to remember. Two live spec/grammar drifts (lowercase annotation names violating the ruled casing partition; `Option[T]` square brackets) had already survived unnoticed for exactly this reason.

## Supersession notes: angle brackets over `Option[T]`; casing partition already required UpperCamel
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** docs hygiene / language surface
- **SCOPE:** minor/local (records two corrections against earlier entries)
- **WHAT:** (1) The 2026-07-27 angle-bracket ruling **supersedes every `Option[T]`-style square-bracket type spelling in earlier entries and specs**, including this log's own 2026-07-19 spelling-cluster entry (`Option[NonEmptyRange]` → `Option<NonEmptyRange>`). (2) The same day's "non-primitive type names are Uppercase" ruling is **not a new decision but ENFORCEMENT of the 2026-07-19 casing partition**, which already required type names to be UpperCamel and compile-checked — the lowercase annotation vocabulary (`array<T>`, `map<K,V>`, `list<T>`, `handle<T>`) has been violating it since. #1552 is therefore a conformance sweep, not a fresh surface change.
- **WHY:** Both drifts survived because the annotation grammar and the type-name rulings never met in one place; recording the supersession explicitly keeps the next reader from treating the older spelling as still-live.

## Format durability doctrine: strictness follows durability; sections skippable; `.brkt` is a log (#1519)
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** brink-format (`.inkb`/`.inkl`/`.brkt`) / save format
- **SCOPE:** architectural (settles the last v6 prerequisite; sets the forward-compat doctrine)
- **WHAT:** The repo had grown **three unnamed versioning philosophies** — `.inkb`/`.inkl` (whole-file version, hard reject, regenerate), the save format (whole-file version *plus* section-local version bytes, tolerant, `LoadReport` reports what it could not apply), and `.brkt` (no policy: positional `if off < bytes.len()` probes whose control flow *is* the schema). The organizing axis is **durability, not artifact kind**, and `.brkt` is the mistake: durable but built as though regenerable. Ruled: **(1) `.inkb` KEEPS its section offset table.** An investigation confirmed random access is real *today* — the reader exposes an explicit tiered API ("Tier 2: Index-only parse"), and live consumers depend on it: `brink-intl::compile` reads the index then **only the line-tables section** of a base `.inkb`; bevy-brink (`brkt.rs`, `locale.rs`, the locale-switch example) reads **only the checksum** to validate a locale against its base without parsing the program; brink-cli does index-only reads. Inline TLV would force a scan and break these. **(2) Each section entry gains an explicit LENGTH**, so a reader can **skip an unknown `SectionKind`**. This is the whole ruling in one change: it converts the format from "strict about every change" to "strict about breaking changes, tolerant of additive ones". **(3) `.brkt` is reshaped as a stream of tagged, length-framed RECORDS, not a document with sections** — it is append-only by design (the runtime restructuring's premise), so growth becomes "a new record tag" and the positional-order hazard cannot exist. **(4) Pre-reservation ceremony (value-model-spec §9's one-bump rule) quietly stops being load-bearing** — but **nothing is removed**: already-reserved tags stay reserved (deleting them would itself require a bump, for no gain). Skippable sections mean additive growth needs neither pre-reservation nor a bump. **(5) Consequence for v6:** #1519 is **not** on the v6 critical path — spans ride the existing `u8` part-tag dispatch, and a Choice captured environment can ride as a nested value exactly as `VAL_CLOSURE`'s captured environment already does. A genuinely new top-level section requires explicit justification; the bump manifest states, per payload, which path it takes.
- **WHY:** Maintainer accepted a clean break on the grounds that there are **no consumers yet** — no shipped saves, no transcripts in the wild — so the break costs only the work and never gets cheaper. The forward pressure is #1093 (per-module bytecode, linking, stable translation units, **DLC**) and #848 (incremental assembly, "splice not re-link"): the moment an artifact *ships*, hard-reject-on-any-mismatch is the wrong policy, and skippability is precisely the durability property those ambitions need. Strictness following durability keeps the simple policy where it is correct (regenerable build artifacts) without paying for tolerance nobody consumes.
- **RIDERS — mods/UGC (maintainer asked; the doctrine holds, with two constraints recorded):** **(a) Untrusted input is a separate axis.** DLC is first-party; UGC is not. Skippability makes the *length* attacker-controlled — it MUST be validated against remaining bytes, decoding MUST be panic-free on malformed input (the fuzzing track is the relevant harness), and the content checksum is **integrity, not authenticity** (a mod recomputes it trivially; it is not a security boundary). **(b) UGC promotes the identity question from hygiene to load-bearing.** A mod references base-game definitions by `DefinitionId`, and those are hashes of name/path — the same weakness that orphans translations on rename (#1442) and lets two files collide (#1504). Under mods it means **a base-game rename silently breaks every mod that referenced it.** The identity cluster is therefore not merely a translation concern but "does UGC survive a patch", and should be ruled with that weight.

## R1 ruled: modules-spec §5 STANDS; anonymous-container state is bounded, reported, and opt-in-nameable
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** identity (DefinitionId) / saves / intl / UGC
- **SCOPE:** architectural (closes the identity cluster #1442/#1504 without reopening a standing ruling)
- **WHAT:** **R1 — the modules-spec §5 ruling STANDS.** Identity remains **name-derived, with `#@was` as the sole migration edge**; the rejected alternatives (**stamped/permanent GUIDs** — "hostile to text-first merging"; **fuzzy load-time rematching** — "silent-garbage risk") stay rejected. §5's three gaps are ordinary work, not design: **(G1) transitive aliases** — renaming a knot re-keys every stitch/label beneath it (their qualified names contain the knot's name) but `#@was` mints exactly ONE entry, so a declared rename still loses the descendants' saved counts; the compiler knows every descendant path and must emit an alias per descendant (deriving at load is impossible — ids are hashes, a path cannot be recovered from one). **(G2) the IDE never writes `#@was`** despite §5 ruling it does — unimplemented, not undecided. **(G3) undeclared renames** are detectable **at authoring time** by diffing against the previous manifest and *asking the author* — which does NOT reopen the fuzzy-matching rejection, because that rejection was of **silent** guessing **at load**; asking at authoring time is its opposite.
  **(G4) anonymous containers are bounded, not solved.** Scoping fact established by inspection: the save keys **globals by NAME**, and only **visit/turn counts** by scope `DefinitionId` (`VisitEntry`'s own doc: "Absent for anonymous counted containers (gathers, choice points)"); sequences derive position straight from the visit count (`vm.rs`: `Cycle => visit_count % count`, `Stopping => visit_count.min(count-1)`). **And a visit count for an anonymous container is unreadable by construction** — an author must name a scope to reference it, so those counts are consumed only implicitly by the runtime. Therefore the entire exposure is: **a once-only choice may reappear, and a sequence may restart**, for a player who saved mid-story and then received a patched build. No variable data is at risk; no author-written logic can break. Ruled response, proportionate to that: **(a)** fix the #1504 miscompile (qualify anonymous scope paths by owning module) as **correctness, unbundled from R1**; **(b)** surface dropped anonymous state through the existing tolerant `LoadReport` rather than losing it silently; **(c)** **naming is the opt-in** — an author who needs a choice's state to survive patches gives it a label and gets ordinary name-derived identity; **(d) NEW (maintainer): a configurable lint for unnamed *stateful* choices/sequences**, so the opt-in is discoverable rather than folklore — tier-able through `[lints]` like every other diagnostic, so a team doing live-ops can raise it while a single-shot project leaves it off. **Explicitly NOT built:** content-derived (hashed) container ids and a sidecar/lockfile id map — both were designed out loud and judged disproportionate to the bounded prize; revisit only if the graceful degradation is shown to actually hurt.
- **WHY:** The maintainer was open to reopening §5 but disinclined, and asked what it would take to close the gaps without doing so — the right question, because three of the four close as ordinary work and the fourth turned out far smaller than it appeared once the exposure was measured rather than assumed. In-source stamping was rejected on authoring grounds ("garbage text appearing in their text file"), and hiding it in the editor was rejected as making the file lie about itself. Content-hashing was attractive (it kills structural renumbering and is symmetric with translation identity — the same edit invalidates both) but buys only re-shown choices and restarted sequences, which degrade gracefully; a sidecar adds a second source of truth and still needs matching heuristics inside it. The lint is what converts "anonymous state is best-effort" from a trap into an informed choice.
- **UNBUNDLING (recommended by the analysis, accepted):** named definitions can carry `#@was` lineage (#1442's domain) while anonymous weave containers have no name to hold invariant and must stay structural (#1504's domain). Every colliding id in the live miscompile is a `c-N`/`g-N`, and qualifying an anonymous path by owning module is compatible with **every** answer to R1 — so a shipping correctness bug must not sit behind an identity ruling.

## CORRECTION to the R1 entry: intl never consults the alias table — translation churn is a standing workflow defect
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** identity × intl
- **SCOPE:** moderate (amends the same-day R1 entry's blast-radius statement and re-sequences the work)
- **WHAT:** Two corrections to the R1 ruling entry, both verified rather than reasoned. **(1) The gap-4 blast radius was understated.** R1 stated the anonymous-container exposure was bounded to visit/turn counts (a re-shown once-only choice, a restarted sequence) on the argument that an anonymous count is unreadable by author expressions. That holds for choice/gather containers — but **anonymous scopes also carry TRANSLATION UNITS**: `brink-intl/src/export.rs` builds each scope's name as `name_index.get(&lt.scope_id)` — an **`Option`** — so unnamed scopes reach XLIFF, and **root content is exactly such a scope**. So anonymous identity is not purely a visit-count concern. The ruling's disposition is unchanged (still bounded, still no stamping, naming still the opt-in); only the statement of what moves was incomplete. **(2) The alias table is SAVE-ONLY, and this makes translation churn a standing workflow defect rather than migration debt.** Verified: **zero** alias usage anywhere in `crates/internal/brink-intl` — saves consult it (`rebind_address` → `Program::resolve_alias`), intl never does. `compile-locale` matches scope ids by **string equality** and fails `IntlError::ScopeNotInBase`; `regenerate_lines` sees only two `LinesJson` and has no alias edges to consult. Consequence, and this is the maintainer's framing: **every future rename orphans the translations beneath it, forever, regardless of how well `#@was` is declared or expanded.** Landing #1504 does not *cause* this — it exercises it once more. **Re-sequencing:** intl alias-awareness (#1442, rescoped) is therefore not "migration for #1504" but a prerequisite for a sane translation workflow at all, and should land with or before #1504 so a rebind path exists when ids move.
- **WHY:** The maintainer drew the distinction that matters: *"this translation churn will happen even after we land this work if we don't account for it — it's not a pre-existing data thing, it's a 'this is the workflow for translations' thing."* Framing it as one-time migration debt would have shipped #1504, closed the ticket, and left every subsequent rename silently destroying translator work. Also ruled: **mods managing their own translation churn is acceptable** — the XLIFF tooling is a first-party workflow, so a UGC author who localises is responsible for their own re-sync; this does not raise the priority of alias-awareness for third parties, only for the first-party pipeline.
- **CORRECTION (2026-07-28, applied during PR #1693 review):** this entry's re-sequencing ("should land with or before #1504 so a rebind path exists when ids move") did not hold for the shape #1504 was actually filed against, and PR #1693 shipped #1504 without waiting for #1442. Verified, not reasoned: `brink-intl`'s export keys a translation scope on `ScopeLineTable::scope_id`, and codegen opens a line table only for a scope-kind container (`Root`/`Knot`/`Stitch`) — every root-level choice and gather inherits the **root** scope's id, which is the hash of the empty path and is **not** file-qualified by #1693's fix. So the qualifier #1693 introduced never moves a translation scope id, and the alias-blindness this entry's re-sequencing was protecting against does not bite for this specific change. Pinned by `root_content_translation_scope_id_is_unaffected_by_the_qualifier` in `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`. This does **not** reopen or weaken this entry's standing finding — the alias table is still save-only, `compile-locale`/`regenerate_lines` still have no rebind path, and a future identity change that *does* qualify a scope id that carries translation units (a knot/stitch rename, or a differently-shaped root-content fix) still needs #1442 landed first or alongside it, exactly as ruled here. The departure from the ruled sequencing is scoped narrowly to: #1504's shipped fix shape happens not to touch a translation-bearing id.

## intl alias rebinding is automatic at compile-locale/regenerate time, not a `migrate-xliff` step (#1442)
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** intl × identity
- **SCOPE:** moderate (settles the open sub-question in #1442's rescope; no identity-model change)
- **STATUS:** tentative
- **WHAT:** `compile_locale` and `regenerate_lines` now consult the compiled `#@was` alias table and rebind a scope whose `DefinitionId` moved under a declared rename. The rebind happens **automatically, inside those two functions**, rather than being absorbed by `migrate_unit_ids` / `brink migrate-xliff`. `compile_locale` reads the edges from the base `.inkb`'s own `AliasTable` section — the artifact it already parses, so no new parameter; `regenerate_lines` takes `StoryData::alias_table` as a third argument, because it only ever sees two `LinesJson`. A direct id match always beats a rebind; two translated scopes landing on one base scope via a rebind is `IntlError::AmbiguousScopeRebind` rather than a silent last-write-wins drop; rebinding is keyed on the id alone, so anonymous scopes (root content is one) rebind like named knots. `migrate_unit_ids` keeps its original, narrower job: the one-off `<unit id>` *spelling* migration from PR #1594.
- **WHY:** The two are different kinds of change. #1594's migration is a **format** change — the id scheme itself moved, once, for files exported by an older brink; a one-shot CLI rewrite is exactly right for that. An id **move** under a rename is not a format change: it can happen on any recompile, forever, and a manual step fails open — the author who forgets to run it loses translator work silently, which is the failure mode #1442 was reopened to end. Automatic rebinding fails safe instead, and costs nothing to carry, because both surfaces already hold the artifact the alias edges live in. Keeping the two paths separate also keeps `migrate-xliff` idempotent and honest about what it does.
- **LIMIT (verified, not assumed):** rebinding can only carry what the alias table records, and `#@was` on a knot mints exactly one entry. A stitch re-keyed only because its parent was renamed still orphans — and re-declaring `#@was` on that stitch is **not** an author-side workaround, because the old name is qualified with the parent's *current* name and the edge collapses to a self-edge (pinned by `rename_identity.rs::stitch_was_cannot_bridge_an_ancestor_rename`). Transitive aliasing remains #1671's.
## Single-line braced choice bodies are legal (#1206) — no interim diagnostic
- **WHEN:** 2026-07-27
- **PROJECT:** brink
- **SYSTEM:** language design (native grammar) / block-effect model
- **SCOPE:** minor/local as a ruling; the delivery is architectural (rides block-as-expression)
- **WHAT:** **Single-line braced choice bodies are legal.** This is not a new decision — the **block/effect model (RULED 2026-07-20)** already dissolves it: "what a braced construct *is* (interpolation, choice body, conditional arm, fn/flow/lambda body) is a block + effects decided by the type checker, **not a syntactic category the parser guesses**", and that entry names "the G-2 brace ambiguity" as one of its motivating frictions. Under the model there is no parse-time body-vs-interpolation decision at all: **value-tail = interpolation, diverge/unit-tail = body.** Today's `is_body_open_brace` (newline lookahead) is exactly the guess the model deletes; when it guesses wrong it silently reparses a body as an interpolation. **No interim diagnostic will be built.** The silent misparse stands until the model's checker lands.
- **WHY:** The only argument for an interim diagnostic was protecting authors from silence — and the maintainer's point is decisive: **nobody is authoring on brink yet.** Spending a diagnostic code, a PR, docs, and a later removal to be loud for an audience of zero is work created and then destroyed. The model delivers the correct error for free (a block whose tail does not match its position) once built.
- **RIDER (the gap this exposed):** the block-as-expression / effect-signature checker — the half of the 2026-07-20 ruling that dissolves G-2 — **was never built and had no tracking issue.** The ruling said it "folds into B0.8"; B0.8 closed (#1177, #1294, #1322, #1309) having shipped statement grammar and code-body lowering *without* it. Filed as its own tracking issue so the ruling has an owner instead of an assumed home.

## Effect-row wire semantics under row-polymorphism: rows with holes, and runtime narrowing is committed (#1680)
- **WHEN:** 2026-07-28
- **PROJECT:** brink
- **SYSTEM:** effects + format (`docs/effects-spec.md` §6/§7/§12, `EffectRows` section `0x0D`)
- **SCOPE:** architectural
- **WHAT:** **(1) Runtime narrowing is COMMITTED, not an optional host optimization** — the maintainer's call: "i do want runtime narrowing, and a harness to figure out how effective it is." §6.4's "SPECIFIED; optional host optimization" framing is superseded; narrowing is a build target with a measurement obligation attached. **(2) The `EffectRows` table stays one row per def, and higher-order defs serialize a row WITH A HOLE** — intrinsic effects plus a row-variable slot — rather than a single ground row. First-order defs are unchanged (a ground row is already correct, because §6.1 fixes every value's row at its creation site). §7's existing mechanism fills the hole: a live fn value is a token, the host asks for the tokens reachable from a dispatch cell, and each token's own ground row substitutes into the slot. **Rejected: joining over every call site's instantiation**, and **rejected: pessimal-at-the-def**. **(3) Consequently T1c draft-round item (4)** ("the effect-row field ships from day one" on function values) **is superseded by §7's token-lookup model** — it becomes a decision-log correction, not a field on `ClosureValue`, which keeps that `Serialize`/`Deserialize` type out of the save-compat conversation entirely. **(4) Build order inverts**: §6.1 row variables are a PREREQUISITE of rows-on-`Ty::Fn`, not a follow-on (Fork B) — the structural `&unify(param_ty, arg_ty) != param_ty` test at `infer/body.rs:1372` and `:1449` is promoted to E063 Error under `types=strict` (`strict.rs:191`), so any non-variable row landing on `Ty::Fn` first fires spurious E063 on correct code.
- **WHY:** The decisive argument is **separate compilation**. A join over call-site instantiations requires whole-program knowledge; if `map` ships in an independently-compiled module its callers live in artifacts that do not exist yet. That is flatly incompatible with #1093 (modular artifacts — per-module bytecode, linking, stable translation units) and with the standing dynamic/runtime/partial-linking ambition. It would work today and become unimplementable the moment modules land — the worst failure shape, since nothing would signal it in the meantime. Rows-with-holes needs no whole-program join anywhere and stays valid when a caller arrives from another module later. The cost of keeping the information is near zero right now and was pre-paid: the `EffectRows` section is **section-locally versioned specifically so the row encoding can grow without a format-wide bump**, and rows are still **inert metadata the runtime does not read** — so there is no consumer to break and no v6 bump. The alternative bakes a permanent precision loss into the artifact; keeping the information lets consumers ignore it. The one standing objection to rows-with-holes — that its only consumer was an optional, unbuilt optimization — is removed by (1). The harness is part of the ruling rather than a follow-up because the value model already shipped on argued-but-unmeasured performance claims once (`docs/runtime-bench.md`); narrowing's benefit is an empirical question about real stories, and committing to it without a way to falsify it would repeat that.

## Fork A — fn-value call-graph edges are harvested STRUCTURALLY; salsa's cycle API stays declined (#1680)
- **WHEN:** 2026-07-28
- **PROJECT:** brink
- **SYSTEM:** compiler architecture (#623 FG spine) + effects (`docs/effects-spec.md` §6)
- **SCOPE:** architectural
- **WHAT:** The last of #1680's five blocking forks. **(1) Fn-value callees become call-graph edges via a NEW STRUCTURAL ATOM, not a row-derived edge** — a per-def set of the targets whose fn values the body *creates*, harvested by the same body walk that already produces `direct_calls`/`referenced_globals`, with empty sigs and empty globals. Fed into the existing call graph, the existing SCC batching and `solve_scc_effects` fixpoint handle the rest. **No cycle is introduced, because no row is ever consulted to build the graph.** **(2) Salsa's native `cycle_fn`/`cycle_initial` stays DECLINED** — the #623 ruling is upheld, on a strengthened rationale rather than deference. **(3) `EffectAtoms.opaque`** (today a flat "calls through a function value → pessimal") **collapses to a real row** whenever the fn values reaching a site were created in-project; it stays pessimal only for genuinely unknown sources, which §6.2 (manifest-declared host callbacks) and §6.3 (the heap's type-row join) already own. **(4) Known gap, scoped separately:** `#fn(g)` and `bind` name existing index symbols and work immediately; a **lambda literal does not** — its `DefinitionId` is minted during LIR lowering (`alloc_lambda_address`), downstream of HIR inference. Lifted lambdas need an index symbol minted at HIR time. This is not a discovery: PR #1713's characterization test already pinned it, and identified the obstacle as **pure keyspace (no index symbol → no `DefKey`/SCC membership), not timing**.
- **WHY:** The cycle Fork A worried about (`call_graph_query → scc_membership_query → solve_scc_query → call_graph_query`) only exists if the call graph needs *inferred rows* to know its fn-value edges. It does not, because §6.1 fixes every fn value's row **at its creation site**, and creation sites are **syntactic** — `#fn` names a target literally, `bind` copies from a known value. So the edge is a structural fact, and the atoms family already has exactly this discipline, stated in its own doc comments: *"discard the computed types, keep the structural fact."* Extending a pattern that is already load-bearing and tested beats introducing a new fixpoint mechanism. On salsa's cycle API specifically: **Fork A's cycle is strictly worse than the one #623 declined.** #623 concerned ordinary recursion *within* inference — values cycling. Fork A would place the **call graph itself** inside the fixpoint, so SCC *membership* becomes iteration-order-dependent; #627's order-sensitive Unknown-absorption determinism risk therefore applies with more force, not less, against a standing hard project rule on determinism. The general principle recorded: **keeping the dependency graph acyclic is a better property than making a cycle tractable** — an acyclic graph is debuggable, deterministic, and incrementally sound by construction, while a tractable cycle is only as trustworthy as its termination argument.

## Fn-value aliasing-channel enumeration ratified — complete over today's grammar, reopened by any new binding position
- **WHEN:** 2026-07-31
- **PROJECT:** brink
- **SYSTEM:** effects (`docs/effects-spec.md` §6.1a; issues #1735, #1755, #1817)
- **SCOPE:** moderate (promotes a drafted enumeration to a settled ruling; adds a standing reopening condition)
- **WHAT:** §6.1a's **fn-value aliasing-channel enumeration is RATIFIED**. It lists the six channels through which a fn value can reach or escape a local, each with a verdict and a pinning test: (1) a bare `Temp` write — the one channel `local_fn_origins` actually narrows; (2) a `Param` write/read — never narrowed, params get row-variable treatment via §6.1b instead; (3) a `ref`-param **call-site rebind** (`poke(f, cb)`) — folded in as untraced; (4) a `ref` **projection** at a call site (`ref npc.hp`) — unwrapped to its root, so a Temp root is channel 3 and a global root is channel 6; (5) a **`#fn`-creation-site `ref`-binding** (`#fn(heal, player_hp)`) — formerly a genuine under-report, fixed by #1755/PR #1808 by charging the bound cell as a write at the *creation* site; (6) **the heap** (a `VAR`/`CONST` cell or collection element) — outside the mechanism's keyspace entirely and pessimal by design. **⚠ The ratification carries a standing condition: completeness is claimed over the channels TODAY'S GRAMMAR has, and any new binding or aliasing position added to the grammar reopens this enumeration.** Explicitly NOT settled by this: giving `ref` params their own row-variable/hole treatment (#1755's option (b), a §8 refinement rung tracked at **#1809**) — that is precision, not soundness, and its absence does not block ratification.
- **WHY:** The enumeration drafted on 2026-07-29 under #1735's `needs-design` posture and **named its own blocker in its own text**: it could not support a claim that nothing was left unmodelled while a channel remained open. Channel 5 was that channel, and it was a real conservative-total (§3) violation — `#fn(heal, player_hp)` binds `heal`'s `ref hp` param to a caller cell at *creation*, but the callee resolves `hp` as a `Param` and cannot see which cell, and the eventual call site narrows to the target while carrying no record of the cell, so the write was recorded **nowhere**. PR #1808 cleared it, so the condition the draft set for itself is met: the enumeration is traced against existing code rather than proposing architecture, and every channel now has both a verdict and a test pinning it. The standing condition exists because "no *known* unmodelled channel" is not "no unmodelled channel" — channel 5 was missed for exactly the reason a future channel would be, namely that one keyword bound at two syntactically distinct positions and only one was enumerated. Writing the reopening trigger into the ruling costs nothing now and is the difference between the next grammar addition being audited and being rediscovered as an under-report.

## Conventions are annotated handlers: the declarative element surface is subsumed by the annotation surface (§9.1 settled)
- **WHEN:** 2026-07-31
- **PROJECT:** brink
- **SYSTEM:** language design (prose rethink #1351) — the element-authoring surface; supersedes parts of sitting 4 and addendum 2
- **SCOPE:** architectural
- **WHAT:** **There is ONE element mechanism, not two.** A preset element (scene heading, cue, parenthetical) is *literally what `!radio` is*: a matched line, captures bound to params by name, and **exactly one call** to an annotated handler. Specifically — **(1) The `lower:` column is DISSOLVED.** Sitting 4 item 7's `lower: content | call(name, args ← payload) | nothing` does not exist; there is a handler or there isn't. "content" is a line with no handler; "nothing" is a handler that emits nothing. **(2) Block elements (NEW):** `@[element(args = …, block)]` captures the *following run* as a `content` param — the same first-class fragment-capture path `!radio` uses for the rest of its line, widened in scope, not a new mechanism. **Terminator: a blank line, or any element-level line.** The handler **WRAPS** the block (receives it and decides emission); it does NOT tag it — an ambient "current speaker" would be implicit state, would show as a write in the effect row, and breaks under control flow. Interior lines are already classified and lowered by their own handlers, so a parenthetical inside a cue block still gets its own handler. **(3) Attachment and chains as declared concepts are DISSOLVED** — attachment is now an ordinary block capture. Structure was already moot: #1715 put header-scoped stitches and slug→`DefinitionId` in the *parser*. **(4) Pattern-claiming is confined to ONE module** — the conventions module named in `brink.toml`. `!name`-dispatched handlers stay legal anywhere precisely because they self-announce. §3.4's pointer mechanism (`elements = "screenplay"` or a path; built-in presets nameable) survives **verbatim**; only the format of the pointed-at thing changes, from a JSON table to a `.brink` module. **(5) The conventions module exports a well-known `fn conventions()` that REGISTERS handlers in order** — imperative registration, statement order is resolution order. It replaces §3.5's `fn conventions() -> Conventions`; the well-known name survives, the returned `Conventions` **type does not**. **(6) The contract for a swapped-in module IS the registration list** — no required roles, no protocol. A comics project simply has no scene heading. The only real coupling is the host: an engine listening for `scene_entered` against a module that never calls it is a *host-manifest* mismatch, and the manifest already owns that boundary. **(7) The capability fence is the EFFECT ROW, not a syntactic whitelist.** `fn conventions()` is real code, comptime-evaluated, bounded by `@[effects(pure)]` — §3.5's already-ruled determinism gate. Pure buys conditionals, loops, helper calls, computed registration; it denies host calls, world reads, clock, filesystem, and every other source of build nondeterminism. A violation is an ordinary effect diagnostic, not a bespoke rule. **(8) Adapter erasure is NOT adopted** (considered and dropped): with registration there is no collection, so no heterogeneous-signature problem, so no adapters — and none of their provenance, FG-4d identity-derivation, or carry-the-handler's-row machinery is needed. **(9) Addendum 2's "zero comptime" is REVERSED as a restriction:** conventions need comptime evaluation from day one. This deliberately spends the staging win that ruling claimed ("it needs no comptime, so the favorite surface ships first"). ⚠ Comptime evaluation is a genuine capability step, not merely a fence — §3.5 names `begin_function_eval`, which today is a **runtime** API on `FlowInstance`; using it during compilation means the compiler runs a program to compile a program, and owes answers on the available language subset, comptime-fault behavior, and error→source mapping.
- **WHY:** The declarative table and the annotation surface were two ways to express one thing, and the table was the worse one — a second, deader language for describing brink concepts, with a `lower:` column that only ever encoded "what happens to this line," which a handler answers directly by *being* the answer. Collapsing them removes the column, the chain-rule engine, the `Conventions` type, and §9.1's "types shaped for extension ergonomics" — that last item stops being a data-structure design problem and becomes `use` plus a registration call. The auditability rationale that originally justified the split ("natural-notation pattern claiming is reserved for the declarative side: one centralized, auditable place") is **preserved and strengthened**: claiming is still confined to one file named in `brink.toml`, and that file is now readable source rather than an interpreted table. No-invisible-expansion gets strictly better — under the old design a declaratively-lowered line had no handler body to hover; now every matched line points at a real function. Registration-as-code beats both a manifest-only fence and an arbitrary classifier: a syntactic whitelist ("only `register` calls") would need its own diagnostic and would be fought by anyone wanting a computed list, while an imperative *classifier* (dispatching per line rather than registering once) would force comptime execution into every keystroke, break the Rust/TS interchange §3.4 requires, and make the explain-match query answerable only by tracing execution. Registration runs once, produces a flat ordered set, and leaves classification a mechanical walk — so the editor still reads a serialized projection and "why didn't this match" stays answerable. Using the effect row as the fence costs nothing new: it is the machinery #1680 and its dependencies just made sound, and §3.5 had already named purity as the determinism gate before this sitting.

## Conventions comptime: the four blocking rulings (#1840) — and a correction to the §9.1 fence
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** language design (#1351 prose rethink) + effects — settles what `docs/conventions-comptime-sizing.md` (PR #1858) declined on
- **SCOPE:** architectural
- **WHAT:** **Q4 — `register`'s effect row (this CORRECTS the 2026-07-31 §9.1 ruling).** That ruling said the capability fence is the effect row and named `@[effects(pure)]`. That was **internally inconsistent**: `pure` asserts the *empty* row (`effects-spec` §10), but `register` must write something, so the ruled example failed its own assertion with E103. **Ruling: `register` writes a NAMED REGISTRY CELL, and `fn conventions()` DECLARES that write rather than claiming purity.** This follows the house precedent exactly — §10 already rules every RNG draw is *"an ordinary write"* to a named cell with `pure` denying it. The fence is still the effect row and still not a bespoke checker; the *assertion* was misnamed, not the mechanism. The determinism property actually wanted — *writes only the registry, reads nothing ambient, calls nothing external, no rng* — is expressible in today's row with **zero new machinery**: no new dimension, no redefining `pure` inside a comptime frame. Rejected: `register` as an `EXTERNAL` (lands in `EffectRow.calls`, fails the assertion) and as a row-exempt intrinsic (a bespoke exemption, against the RNG precedent). **Q1 — handler identity across the comptime boundary.** The sizing doc's binary (`DefinitionId` vs module-qualified name) is **false, because neither carries the payload**: the consumer needs a `ClaimHandler` — compiled `claims` pattern, param-name list, annotation `TextRange` — all of which come from the conventions module's **CST**. **Ruling: TWO INDEPENDENT READS.** The compiler reads the conventions module's CST for `ClaimHandler` records (what #1838 already does within a file); it separately comptime-evaluates `fn conventions()` for an **ordered list of identities**; it joins them. `DefinitionId` is the join key — non-invertibility is a non-issue because the CST side already holds the display name and puts it in the record, and the join is on a hash both sides compute independently, so no cross-file *name* resolution is reintroduced. Two mismatch directions get diagnostics: **registered-but-not-declared** is an error; **declared-but-not-registered** is a WARNING suppressible with `@[allow(…)]` (matching E168's shape), since conditional registration is legal under Q4's row. **Q2 — comptime fault behavior.** First, a scoping distinction: an **effect-row violation is NOT a comptime fault** — it is E103 at compile time, already handled. Q2 governs *runtime* faults during evaluation (step limit, arithmetic). **Ruling: the BUILD hard-fails** (and `brink check`/CLI with it) — a faulted evaluation silently reclassifies every claimed line to plain content, which is total, not graceful, and is the silent drop CLAUDE.md forbids. **The EDITOR keeps the LAST-GOOD value of that same module**, keeps classifying, and shows the error — because §3.5's owed re-evaluation loop re-runs on every keystroke, so degrade-to-empty would flicker the whole script between classified and plain while the author types. ⚠ **It must NEVER substitute a different module's conventions** — falling back to the stdlib preset when a *custom* module breaks would claim lines with rules the author never chose, producing plausible wrong output; obviously-broken beats silently-wrong. A default (non-overriding) project cannot reach this state at all: the stdlib module is ours and tested. A custom module broken on first open with no last-good yields an empty set plus a **loud** error — warranted because the failure is total rather than local. **Q3 — comptime error → source. ACCEPTED AS A v1 COMPROMISE, not a satisfying answer.** There is no bytecode→source mapping at instruction granularity (`Opcode::SourceLocation` is dormant, discarded with `Nop`; `SourceLocation` is per *line-table entry*, built per content node, so a `register(…)` **call** never gets one; `ContainerDef` carries no range field). **Ruling: v1 reports a NAME-LEVEL STACK TRACE, no ranges** — walk `thread.call_stack`, read each frame's `container_idx`, look up `ContainerDef::name`. Bare-container-name was rejected as too weak, because Q4's row permits loops and helper calls, so the faulting function is often not `conventions()` itself. Known costs, recorded rather than discovered: `ContainerDef::name` is `Option<NameId>`, so a lifted lambda prints as anonymous (its synthesized `{scope}.#lambda-{offset}` identity means the range is recoverable later without a format change); and `RuntimeError::StepLimitExceeded(u64)` carries only the limit, so this is an **error-signature change**, not merely a read. Ranges remain **#452**'s job (`needs-design`, its pivotal Q-R1 blocked on a format ruling since 2026-07-19) — #1840 is explicitly NOT downstream of that epic. **Side ruling: the stdlib preset's resolved projection is PRE-FROZEN and shipped.** `std::conventions::screenplay` is fixed, so a default project pays **zero** comptime; only an overriding project evaluates anything. This is an early instance of module-level build caching, whose general facility is **#1093** (modular artifacts — per-module bytecode).
- **WHY:** Q4 is the load-bearing correction: a fence that its own canonical example fails is not a fence, and the RNG cell had already settled this exact shape — side effects are named cells and `pure` means none of them. Naming the write instead of asserting purity keeps every property the §9.1 ruling wanted (the row *is* the fence; violations are ordinary diagnostics; no bespoke checker) at no cost, because the row already expresses it. Q1's binary dissolved once the payload question was asked: identity is the cheap part and the CST holds everything expensive, so the comptime boundary should carry the one thing it uniquely knows — order — and nothing else; that also stops the projection schema from being a decision that cannot be revisited. Q2 splits on consumer rather than on severity because a build and a live editor have genuinely opposite obligations: a build may fail and must not lie, while an editor must stay responsive and must not pretend. The never-substitute-a-different-module rule is the same silently-wrong-beats-nothing judgment applied to fallback. Q3 is accepted as a compromise with its upgrade path named, because the alternative is making the prose dialect wait on an epic that has been blocked on a maintainer ruling for twelve days; the name-level stack is what makes the compromise livable — an author always learns *which function* faulted, and only loses *which line*.

## Native fn values are bare names; `#fn`'s binding form stays ink-only (#1862)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** language design — native expression grammar (T1c fn values; unblocks #1840)
- **SCOPE:** moderate
- **WHAT:** **(1) In the native dialect, a statically-named function in expression position IS a fn value** — no sigil: `register(screenplay::scene)`. A call stays `screenplay::scene()`, so reference-vs-call is unambiguous (Rust's function-item model). **(2) `#fn(…)`'s BINDING form (a positional prefix bound at creation) gets NO native spelling for now** and remains ink-dialect-only. **(3) `#fn` is NOT retired from ink** — the ink spelling stands unchanged; this adds a native surface, it does not migrate one.
- **WHY:** The `#` prefix is not merely disliked, it is grammatically wrong in native: `#` is already the tag sigil in native content position (`brink-syntax-native/src/parser/block.rs`), so `#fn` collides with the one meaning `#` has there. A bare name needs no sigil, no new token, and no collision. The typo hazard (writing `foo` for `foo()`) is caught by the type checker as an ordinary mismatch, so it is a diagnostic-quality concern rather than a correctness one. **The binding form splits on a real asymmetry:** for *value* params, `#fn(f, a)` is now redundant with lambdas — since lifting landed (#1709/#1710), `|x| f(a, x)` says the same thing more plainly. For **`ref`** params it is NOT redundant: lambda capture is by-value always (2026-07-19 ruling — no ref captures in v1), while `#fn(heal, player_hp)` binds a `ref` param to a *cell*. A by-value lambda structurally cannot express that. That asymmetry is precisely what created **channel 5** of the §6.1a aliasing enumeration — the trickiest case in the effect analysis and a genuine under-report until #1755 fixed it. So a native binding form would need both a spelling *and* a fresh look at whether creation-site `ref` binding should exist at all given that cost. Nothing currently blocked requires it: #1840's conventions module needs only zero-bound-arg references. Adding the part that is needed and leaving the part that is contested open is the smaller, more reversible move.

## Content-as-value: an internal `Expr::Fragment` lowering form, turn-scoped — no new primitive, no surface syntax
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** prose dialect (#1351) + runtime output model — unblocks the `content` half of #1912 and the block-capture mechanism of #1839
- **SCOPE:** moderate (smaller than the "new primitive" framing it replaces)
- **WHAT:** **(1) There is NO new runtime primitive to build — the mechanism already exists.** `Opcode::BeginFragment` / `EndFragment` are shipped, and `EndFragment`'s own contract is *"End fragment capture — store parts and push `Value::FragmentRef`"*. The VM already brackets emitted output and pushes a content value onto the stack. **(2) What is missing is an EXPRESSION FORM that lowers to that bracket.** Today the only emitter is codegen's `emit_slot_expr`, for a call in display position, so the capability is reachable only through a codegen-internal path. **Ruling: add an INTERNAL `hir::Expr::Fragment` / `lir::Expr::Fragment` node** that evaluates a span of content through the normal line path inside the fragment bracket and yields the ref. **(3) NO SURFACE SYNTAX.** The node is constructed only by the capture machinery — `try_claim` binding a capture to a `content` param, and #1839's block capture. Both consumers need the identical lowering, so one internal form serves both. A user-facing spelling is deliberately NOT added: nothing needs it, and adding it invites design questions (scoping, nesting, escape) that the internal form does not raise. **(4) `content` is TURN-SCOPED.** Legal as a parameter and in expression position within the turn; storing one in durable state (a `VAR`/`CONST`) is a compile error. **(5) Block capture is representationally sound** — verified, because this was the live risk: a `Fragment` is `Vec<OutputPart>` and **`OutputPart::Newline` exists**, so a multi-line block is `LineRef, Newline, LineRef, …`. `fragment.rs`'s "captured sub-region of output" describes capture SCOPE, not a line-spanning restriction. Interior lines stay `LineRef`s — deferred and resolved against current line tables at read time — so **each line inside a captured block keeps its own line-table entry and stays independently translatable**, which is exactly the property the capture contract promised. ⚠ **UNVERIFIED AND OWED**: the *representation* was traced, the *emit path* was not. Whether emitting a multi-line fragment expands into successive output lines (rather than being treated as one line's worth of parts) is plausible from the structure but unconfirmed — #1839's build MUST prove it with a test rather than assume it.
- **WHY:** The framing this replaces — "a new content-as-value primitive crossing HIR/LIR/codegen" — was accurate about the gap but wrong about its size, and the difference matters: one is an architectural addition, the other is exposing a shipped capability one layer up. Keeping the node internal is what keeps it small; every hard question about content values (can they nest, can they escape, what is their scope) is a question about a *surface* type, and the capture machinery raises none of them because it creates and consumes the value within one call. Turn-scoping follows the same logic: a `FragmentRef` is an index into the output buffer's fragment store, so escape is the only way it can dangle, and forbidding escape is both statically checkable and sufficient for every use case that exists. It is also the relaxable direction — permitting escape later is additive, whereas retracting it would not be. The multi-line verification is recorded because it was the design's live risk: had fragments been strictly sub-line, `> {body}` would have flattened a cue block onto one line and the whole block-capture ruling would have needed rework.
## Delete `crates/zed-brink` — dead ink-only scaffold with a live CI gate
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** repo structure / CI
- **SCOPE:** minor/local
- **WHAT:** **Remove `crates/zed-brink/` entirely**, along with `.github/workflows/zed-brink.yml` and its references in the root `Cargo.toml` exclude list, `release-plz.toml` comments, `docs/releasing.md`, and `docs/book/src/contributing/crate-layout.md`. The historical decision-log entry at :688 that mentions it is a record and stays untouched.
- **WHY:** It is non-functional and cannot be built by anyone. `extension.toml`'s grammar points at `file:///Users/syynth/code/rs/brink/tree-sitter-ink` — an **absolute local path that does not exist in the repo** — so the extension is unbuildable outside one machine's incidental filesystem state. It is 9 files / 484K across 7 commits, `version = "0.0.1"`, unpublished (`publish = false` per the 2026-07 crates.io ruling), workspace-**excluded**, and **ink-only**: `languages = ["Ink"]` with an `ink`-only language dir, so it contributes nothing to the native surface, which is where editor support is actually owed (#1131/#1350, and the `.brink` client registration that NS-T.1 needs). Meanwhile it carries **its own satellite CI workflow** — the exact class of `[workspace]`-excluded satellite with its own lockfile that made main go red unnoticed for several merges on 2026-08-01 (#1890/PR #1896, and #1905's open question about promoting those gates to required). Deleting it removes a maintenance surface and a red-main hazard in exchange for a capability nobody has. Recovery is a `git revert` if a real Zed extension is ever wanted, and it would want a native grammar anyway rather than this ink tree-sitter snapshot.

## Issues and docs must state current state upfront; stale bodies are a defect
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** cross-system — process (issue hygiene, spec maintenance, agent briefing)
- **SCOPE:** architectural (changes how every future issue and spec is maintained)
- **WHAT:** **An issue body and a spec section must accurately state current state, and must be updated when that state changes** — not left as a filing-time snapshot with the truth accumulating in comments below. Going forward: (1) when work lands that invalidates an issue's premise, **edit the body**, don't only comment; (2) a spec section that has been superseded says so **at the point of the claim**, not only in a later section; (3) tracking/umbrella issues carry checkbox state that matches reality — a closed child means a checked box; (4) issues whose title no longer describes their residual scope get **retitled** rather than left to mislead. Existing stale artifacts get a one-time correction pass (#1351, #1180, #1683, #1684, #1449, #1131, #1106, `prose-dialect-spec.md` §9.6 and §9.2, `CLAUDE.md`'s `Line` enum).
- **WHY:** Measured cost, not a hypothetical. In a single review session on 2026-08-01, **three of five track assessments were wrong**, every one of them from trusting an issue body: #1351's body says "Sitting to be held fresh — enumerate the full list at its opening" while **four comments beneath it record five completed sittings** (2026-07-25) and a 2026-07-28 decomposition into build-ready issues; #1131/#1350's hold was inherited from a 2026-07-25 state of the world that the 2026-07-28 grammar landing dissolved, and `prose-dialect-spec.md:955` keeps propagating it with no dated re-decision behind it; #1180's body has **never been edited since filing** (`createdAt == updatedAt`, zero comments) and carries no status at all, though ~80% of its scope has landed. #1683's title still says "perform the v6 bump" when v6 shipped 2026-07-29 and the issue's real residual is "fill the open v6 line". This is a **compounding** cost under the pump: agents are briefed from issues, so a stale body doesn't mislead one reader, it misleads every agent that reads it until someone notices — and #1902 was already lost to exactly this failure mode (a false premise propagated from a doc claim into a build brief, traced by the agent to unreachable code and reverted). The remedy is cheap and local: the person who lands the work knows the premise moved, and is the one keystroke away from saying so.

## NS-T is held by deliberate sequencing (compiler before editor), and takes the free part now
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** NS-T editor track (#1131, #1350) — corrects an inference about #1351's hold
- **SCOPE:** moderate
- **WHAT:** **(1) The NS-T hold is deliberate sequencing — the compiler work finishes before the editor work starts — NOT an unlifted blocker.** A review on 2026-08-01 traced #1350's 2026-07-25 "blocked-by #1351" comment, observed that the prose surface it waited on had landed 2026-07-28 (#1715 closed, #1716 landed, #1717 closed, escape set final), found no dated artifact re-affirming the hold, and concluded it was stale and should be lifted. That inference was **wrong about the reason** — the original rationale was indeed overtaken, but the hold stands on independent sequencing grounds. `docs/prose-dialect-spec.md` §9.6 now records this so the inference is not repeated. **(2) Take the free part now:** register `.brink` **and** gate `semantic_tokens_full`/`_range` on `db.is_native` **in the same change**. Real native token classification stays held. **(3) The record is corrected on scope: "NS-T is ~0%" is false.** The analysis half is built — `.brink` workspace scan and watcher, compile-identical module identity (#1572/#1576), native project grouping and cross-file scope (#1562), native diagnostics, native db frontend, `IdeSession` native-awareness (#1358) — and the entire HIR-based IDE feature set (hover, navigation, rename, signature, effects, story_graph, hir_projection) is already dialect-generic, proven by a passing `brink-web` test asserting cross-file **native** hover. What is absent is the CST-presentation half.
- **WHY:** The sequencing is the maintainer's: finishing the compiler before opening a second front keeps the surface under one set of hands, and every editor feature built against an unfinished compiler is rework. But the *stated* reason had rotted into a bare "stay held" line with no date behind it, which is why a careful review reached the opposite conclusion — the fix is to write the real reason down, not to lift the hold. The "free part" carve-out exists because the cost/benefit is lopsided: the analysis work is already built and shipping nothing, purely because `.brink` is registered in no client, so a registration plus a one-line dialect gate converts existing sunk work into a working feature at near-zero risk. ⚠ The two halves must ship **together**: `parse_query` is unconditionally the ink parser with no dialect gate, so registering `.brink` alone would light up a live bug — the server confidently emitting ink-misclassified tokens over native source — which is latent today only because no client asks. This is a direct instance of the pattern the maintainer flagged this session (capability lands, consumption is deferred to a held issue, and the capability sits unused); the carve-out is the cheapest available answer to it.

## Terminal-classifier R1: #1520 folds into the Step migration (#1684); R2 keeps the deferred fault
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** runtime — the terminal cluster (#1520, #1574, #1684, #1522)
- **SCOPE:** architectural
- **WHAT:** Three rulings closing the cluster `docs/design/yield-time-terminal-classifier.md` had been holding open. **(R1, #1520) The classifier FOLDS INTO #1684** rather than landing as a standalone refactor — the classifier's output *is* `Step`'s own variants, so there is no interim shape to design, and every marshal leg (`brink-web`, `bevy-brink`, `brink-ide`, the TUI, the benches) takes ONE breaking edit instead of two (§8d.7 already ruled the enum becomes `Step`; §7 ruled its terminals carry no text). Rejected: a `Line`-shaped change now (two breaking edits per consumer), and a side-channel `terminal()` accessor (a *second* way to ask "why did we stop" at the exact moment §7 exists to fuse it into one — note a weak form of this already shipped unruled via #1573/PR #1577 promoting `did_safe_exit` to production API). **#1684 now carries ZERO outstanding rulings and is implementation-blocked only.** **(R2, #1574) `RanOutOfContent` KEEPS the deferred fault** — brink delivers the `Done` line and faults on the *next* call; it does NOT adopt `Story.cs`'s raise-on-discovery + suppress-trailing-text behavior. `Story::did_safe_exit()` remains how a caller distinguishes a real `-> DONE` from running out. ⚠ Consequences that must not stay implicit: the `oracle.rs:227` extra-step allowance and #1522's are now **PERMANENT** and must stop being described as pending retirement; the divergence from `Story.cs`'s `!canContinue` branch is **intentional** and needs a spec home saying so; `terminal_classification.rs`'s characterization tests now pin ruled rather than provisional behavior. **(R2 rider) The ran-out-of-content MESSAGE splits into four**, matching C#'s call-stack-keyed variants (tunnel `->->`, function `~ return`, plain, unknown-reason) — filed as #1993. `RuntimeError` is already `thiserror`; the string comparison lives in the *harness* because oracle episodes store C#'s rendered text, so this is four `#[error(...)]` arms, not a move away from strings. ⚠ Error-signature change ⇒ wasm-observable ⇒ changeset; **may move the ratchet UP** and the PR must report the delta.
- **WHY:** R1 is chosen for consumer cost, not elegance: the enum's destination was already ruled, so any interim shape is a second migration everyone pays for and nobody keeps. Folding also removes the sequencing question (#1520-before-#1684 vs after) that nobody had answered. R2 goes the other way deliberately — the oracle is ground truth on *behavior*, but matching C#'s fault *timing* would retire two allowances and rewrite pinned tests for a benefit that is empirical and unmeasured, while the deferred fault plus `did_safe_exit` already gives callers the distinction they need; making the divergence permanent and *documented* is worth more than making it match. The rider splits the other way because message granularity is pure upside: it costs four `#[error]` arms and a call-stack read, and it converts episodes that can never match today into episodes that can. Timing and message granularity were conflated in one issue for a week; they are independent axes and are now ruled independently.

## Claiming handlers, prose bodies, and the interleaving escapes were ALREADY RULED (#1850 closed)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** language design — native surface, conventions (#1850, #1839, #1991, #1992)
- **SCOPE:** moderate (corrects the record; unblocks #1839)
- **WHAT:** **#1850 ("can a claiming handler emit prose?") is CLOSED as already-ruled and already-implemented.** `docs/native-surface-charter.md` §4 (RULED 2026-07-23): the brace prefix selects a body's dialect — plain `{ … }` = per-keyword default (`fn` → code, `flow` → prose), `~{ … }` = code-ground, **`>{ … }` = prose-ground, explicitly "a prose-bodied `fn`"**. `docs/prose-dialect-spec.md` §3.5b already builds on it by name ("prose-bodied handlers (`>{ }`) are once-translated parameterized templates"). Verified end to end on main: `fn radio(chan: string, text: string) >{ [{chan}] {text} }` compiles and `brink play` emits `[A] hi` — the ruled `!radio` shape. The issue's premise conflated **callability** with **body dialect**; they are orthogonal. ⇒ **#1839 (block capture) is unblocked.** Separately confirmed the same 2026-07-23 ruling's **line-granularity** half (charter §8 status board: `~ stmt` runs code in a prose body, `> text` emits prose in a code body) and found BOTH unimplemented: `> text` in a code body raises E129 (#1992), and **`~ stmt` in a prose body SILENTLY EMITS AS PROSE and drops the statement** — compiles clean, prints `~ n = 5` as story text, leaves the variable unassigned, no diagnostic (#1991, severity:high).
- **WHY:** Recorded as a decision entry, rather than only as issue comments, because the *failure mode* is the reusable part. Three separate design questions were put to the maintainer this session that the specs had already answered, each because a search came back empty and the absence was trusted. The `{ }`-defaults-per-keyword half of the body-dialect ruling is what makes this particular one easy to get wrong: a probe written with plain `{ }` on a `fn` gets `E037 unexpected token in statement block` and reads as proof that prose bodies do not exist, when it is only proof that the default is code. The `~ stmt` finding is the concrete cost of the gap between a ruling and its implementation — a ruled construct that neither works nor errors, but silently prints compiler syntax into the reader's story.

## Lambda annotations firewall, with an eager incompatibility error (#1932)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** typed mode — lambda inference (#1932, #1910, implementation #1994)
- **SCOPE:** moderate
- **WHAT:** **A lambda's written annotation takes priority over its body-derived type — AND the checker must immediately error if the two are incompatible.** Not silent trust: the annotation governs the lambda's type for everything downstream, but an incompatible body is an **eager error at the lambda**, not a deferred surprise at the call site. Implementation (#1994): narrow #1910's body-derived read-back to the **unannotated** case, add the incompatibility diagnostic, and record the rule in `docs/typed-mode-spec.md` beside the top-level-`fn` precedence rule, since the two now differ deliberately. ⚠ Do not revert #1910 — its fix for the unannotated case is correct and closed real `BASELINE` rows.
- **WHY:** `typed-mode-spec.md` §TM-2 already states the principle as "annotation = firewall", and #1910/PR #1928 flipped lambdas away from it **as a side effect** — `infer_lambda` began reading body-derived narrowing back "instead of rebuilding the lambda's `Ty::Fn` row from written annotations alone" — without anyone choosing that. The maintainer's reasoning for restoring it: a lambda annotation is written to *constrain* a body you are about to write, so it is intent rather than a hint; and the error belongs where the author can act on it. The eager-error half is what keeps this from being a footgun — a firewall that silently masks a wrong body would trade a confusing call-site error for an invisible one, which is worse.

## Ref-parameter arguments are checked invariantly (#1920)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** typed mode — call-site argument checking (#1920, implementation #1995)
- **SCOPE:** moderate (rejects code that compiles today)
- **WHAT:** **`ref` parameter arguments are checked INVARIANTLY** — the argument type must match the parameter type exactly. By-value arguments keep covariant widening. Applies UNIFORMLY to the direct-call check (#1864/PR #1875), the UFCS-desugared check (#1881/PR #1914), and every other by-ref call-checking site.
- **WHY:** This is a soundness fix wearing a design question's clothes. `assignable(Float, Int)` is true, so `fn scale(ref x: float)` accepts an `int` cell and then writes a `float` back through storage that is statically an `int`. A `ref` slot both reads *and* writes through the caller's storage, so widening in one direction only is not sound for the write-back — the covariance that is correct for a by-value argument is exactly wrong for a by-ref one. Applying it to only one of the two call-checking sites would reopen the same hole in a different spelling, which is why the ruling names both.

## Markup surface: tags are metadata, hyphens allowed, attrs gain required + a widened schema
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** prose dialect §4 — the markup layer (#1783, #1740, #1780; implementations #1996, #1997)
- **SCOPE:** moderate
- **WHAT:** Three §4 rulings. **(1) #1783 — markup inside a `#` metadata tag is LITERAL TEXT, intentionally.** `Hello. # <glitch>loud</glitch>` keeps `<glitch>` as characters; no spans in tags, ever. Current behavior is correct and #1778's pinned shape stops being provisional — one sentence in §4, no code. **(2) #1740 — span tag names MAY contain `-`.** `<fade-in>` becomes legal; widen the lexer in span-tag position specifically, without affecting `IDENT` elsewhere, covering both `LT IDENT` and `LT SLASH IDENT`, and pin whether a leading/trailing hyphen is legal rather than leaving it to the lexer's accident (#1996). **(3) #1780 — BOTH halves adopted:** `markup` gains a **required-attribute flag** (today only *unknown* attrs fire, via E165 — a declared-but-missing one is never caught), and `ManifestSpanKind.attrs` is **widened NOW** from `Vec<String>` to a per-attribute shape even though value typing is not being implemented, so adding types later is not a breaking manifest change (#1997). Span attribute *values* stay static text for now, and the spec must say so.
- **WHY:** (1) follows from §4.3's own framing — spans are *presentational* and line-scoped while a `#` tag is metadata the host reads, so presentational markup in a channel that never renders has no meaning; ruling it explicitly stops the next agent relearning it the way #1778 did. (2) the markup layer is freeform-by-default and host vocabularies borrowed from XML/HTML are kebab-case by convention, so the `LT IDENT` shape was an accident of the grammar rather than a decision. (3) takes the schema widening now precisely because it is cheap now and breaking later: the manifest is a published surface, and the difference between `Vec<String>` and a record is a migration the moment anyone depends on it.

## Trailing comments stay literal in prose (#1638)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** native parser — prose text contract (#1638)
- **SCOPE:** minor/local
- **WHAT:** **A trailing same-line `//` or `/* … */` on a prose line remains literal story text.** `text_run_until`'s existing contract is correct as documented ("including any interior whitespace/plain-comments — those are literal prose here, not trivia to discard"). No code change; state it in the spec so it stops reading as an open question. The lint variant (keep literal, warn on comment-looking line ends) was offered and explicitly not adopted.
- **WHY:** One rule beats one rule plus an escape hatch. Stripping trailing comments would make `//` unwritable as literal prose without a new escape — which immediately matters for any story containing a URL or a quoted code sample — and that escape would then need its own spec section, its own diagnostics, and its own round-trip handling in the emitter and XLIFF. The authoring hazard (shipping a comment to players) is real but narrow, and it is visible in the output the moment it happens.

## A recognized escape strips its backslash in tag/cue-name/scene-title text too (#2045)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** prose dialect §4.6 — the markup/escape layer audit (#1738, #2042, #2045)
- **SCOPE:** minor/local, breaking for authored content
- **WHAT:** `content::tag()`/`element::cue_name()`/`element::scene_title()` — the three raw free-text scanners that give the ruled §8d.6 inline escape set (`\< \{ \# \\`) *structural* recognition only (#1738/#1852: an escaped `#`/`{` doesn't end the scan early) — now also **strip** the recognized escape's backslash from their *materialized* text, matching what `markup::escape` already does for ordinary content, choice text, and span bodies. The parser's own backslash-parity/depth tracking is completely unchanged (still decides where each node ends); the strip happens one layer later, in three new/updated `ast` accessors in `brink-syntax-native/src/ast/nodes.rs` (`Tag::text()` — new; `CueName::text()`/`SceneTitle::text()` — updated) that share one helper, `strip_recognized_escape_backslashes`. `hir::lower_native::body::lower_tag` was simplified to delegate to `Tag::text()` instead of hand-rolling the same HASH-skip logic. Applies to the full four-member escape set uniformly (`\<`/`\\` included, not just the two — `\#`/`\{` — with an existing structural role). `docs/prose-dialect-spec.md` §4.6's audit table and its "still open" prose are updated in the same PR, not a second table.
- **WHY:** #1738's own filing body used the exact cross-scanner inconsistency this closes as its motivating example (`Hello \# world #a \#b` showing two different treatments of the same `\#` on one line), and it was never decided — only tracked. Ruled toward one reading of the escape set everywhere it's recognized, rather than preserving the `\{` "structural-only" precedent's inertia: an author should not have to remember that the same backslash behaves differently in ordinary content versus a tag/cue-name/scene-title. `element::scene_title`'s one real lowering consumer (`try_claim`/`try_dispatch`'s natural-notation pattern matching) deliberately keeps reading the *raw*, unstripped `SyntaxNode.text()` rather than `SceneTitle::text()`, because that raw text's byte offsets are load-bearing for mapping a regex capture group back to a real source range — running stripped, byte-shifted text through that offset math risked silently corrupting capture provenance for an unrelated, delicate feature (#1838). This does **not** narrow #1883 (a different axis — `HASH`/`COLON` depth-awareness and `\}`/`\{` parity, both about parsing structure, not materialized text) and does not touch #2040 (whether an *unrecognized* backslash sequence should become a compile error inside these scanners — still open, not decided here). `cue_name()`'s fix has **no current runtime-observable effect**: `hir::lower_native` still meets a bare `CUE` node at its loud-`E129` default arm (no HIR lowering yet, #1717), so `CueName::text()`'s stripping is proven at the parser/AST level only, pending that slice landing.

## A native var/const MAY hold a fn value; file scope has no flow-local state to capture (#1774)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** language design — T1c fn values × flow state (#1774, #1764, #597, #1210)
- **SCOPE:** moderate
- **WHAT:** **A native `var`/`const` MAY hold a fn value — both a bare-name function reference and a lambda literal.** The `E083` gate in `is_const_foldable_kind`'s `Lambda` arm is lifted for this case. Implementation owes: re-read that arm's creation-site-capture rejection against file scope rather than special-casing around it, and **pin with a test** that a file-scope lambda cannot capture flow-local state, so a future change introducing file-scope flow context fails loudly instead of opening a privacy hole silently. Unblocks #1764's seven hand-recursions.
- **WHY:** The 2026-07-23 tentative direction "flows-as-actors" homes a *capturing* fn value to its creating flow **in order to protect `#@local` privacy** — "only A-context code ever touches A's locals". That reason does not reach file scope: globals live in `Context` and are **story-wide** (`docs/scoped-flow-state-spec.md:51`), while flow-local is a **sparse override private to the flow** (`:108`), and a file-scope lambda has no flow context to capture one from. So the invariant the homing rule exists to protect is never at stake and no home flow is required. A root/primary flow does exist (`scoped-flow-state-spec.md:114`; `flow-suspension-spec.md:163`) and could serve as a home, but the requirement does not arise — which is why this ruling does **not** depend on the actor direction being ratified, even though that direction is `STATUS: tentative` and makes #597 a sub-question of the #1210 stub. ⚠ Recorded because the first pass got this wrong in the other direction: it ruled "disallow" on a report that no actor-style ruling existed, when the entry was in this very file — a grep piped through `head -8` filled with `refactor` matches and truncated before reaching it. Absence of evidence from a truncated search is not evidence of absence.

## The editor↔conventions seam is compiler work, not editor work — the NS-T hold does not cover it (#2006)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** conventions/elements × IDE query layer (#2006, #1131, #1840)
- **SCOPE:** moderate (changes what is pumpable today)
- **WHAT:** **The classification/explain-match query family is compiler work wearing editor's clothes, and is therefore NOT held by the NS-T sequencing hold.** Maintainer, 2026-08-01. The NS-T hold (recorded the same day: "the editor track is held because the compiler work comes first") covers editor *frontend* work — CM6, semantic-token rendering, the live renderer, `fmt`. It does **not** cover queries emitted from `brink-db`/`brink-ide`, which are compiler-side artifacts that happen to have an editor as their consumer. Concretely unheld: per-line classification metadata (matched kind, handler + source location, capture bindings as spans, disposition), the **explain-match** query, hover-shows-handler-body, capture spans as decoration ranges, the harvest index obligation for cue payloads and span kinds, and the serialized conventions projection. Still held: anything that renders. ⚠ **Two rulings remain owed and now gate a pumpable decomposition rather than a parked one** — **match ordering** (decision-log marks it `⏳`, "declaration-order + overlap diagnostics is the lean; rule owed") and the **editor re-evaluation loop** (`prose-dialect-spec.md` §3.5's owed list). Match ordering should be ruled **before** explain-match is built, since "attempted patterns on a miss" has no defined order without it.
- **WHY:** The distinction is real and load-bearing, not a technicality. "No invisible expansion" is a stated maintainer requirement: under conventions a prose line silently becomes a function call, and the ruled compensation is that the author can always ask which handler claimed it, why, and what it bound. That compensation is produced by the *compiler* — it is the same walk that does the claiming, reporting what it did — and consumed by whatever frontend exists. Filing it under "editor" and holding it behind the compiler track inverted the dependency: the compiler emits it, so holding it behind compiler work holds it behind itself. The cost of the misfiling is measurable — v1a claiming ships today with no way to interrogate a claimed line, and the gap widens with every convention that lands (v1b/v1c were both unblocked 2026-08-01). Also worth recording: the two owed rulings were tolerable while this looked parked; now that the work is pumpable they are the actual blocker, and Q2's last-good caching has *already* shipped a dependency on the re-evaluation loop it assumes.

## Match overlap is recorded, not stopped at; the conventions projection is cached on its closure (#2006)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** conventions classification × IDE query layer (#2006, #1840, prose-spec §5/§9)
- **SCOPE:** moderate (clears the last two ⏳ items on the conventions/editor seam)
- **WHAT:** The two rulings owed since the §9.1 sitting, both now closed. **(1) Match ordering — the walk KEEPS GOING.** §9.1 had already ruled that registration order is resolution order, so *which* handler wins was never open; the residual was whether classification stops at the first hit. It does not: the walk tries **every** registered pattern against every line, uses the **first** match, and **records the rest as shadowed**. Consequences: overlap detection is **exact rather than heuristic** (no static regex-intersection approximation is needed); a shadowed-pattern diagnostic falls out of the recorded matches instead of requiring its own analysis; and **explain-match becomes exact on a hit as well as a miss** — it can name what else would have matched, not merely what was attempted. **The cost is paid with caching, not with a cheaper walk** (maintainer's instruction): per-line classification is memoized on `(line text, projection revision)`, so editing one line reclassifies one line, and only a projection change invalidates the whole file. **(2) The editor re-evaluation loop — the two halves run at different rates.** The **projection** (comptime-evaluating `fn conventions()` into the flat ordered set) is a cached query keyed on the conventions module **and its import closure**, recomputed only when that closure changes. **Classification** runs every keystroke against the cached projection. Q2's "re-runs on every keystroke" refers to classification; comptime evaluation is **never** per-keystroke. Editing a story file reclassifies; editing `conventions.brink` re-evaluates and then reclassifies everything. Q2's last-good value is the previously cached projection.
- **WHY:** (1) picks exactness over per-line cheapness because the cheap option quietly costs the thing the whole design is for. "No invisible expansion" is a maintainer requirement, and a walk that stops at the first hit can answer "why didn't this match" but **not** "why did *this* one win" — the more useful question once an author has more than one pattern. Stopping early would also have pushed overlap detection into static regex comparison, which is either expensive or a heuristic that misses real collisions; recording actual matches makes it exact for free. The performance objection is real but is a **caching** problem, not an ordering one, and salsa already memoizes at exactly this granularity. (2) resolves an ambiguity that Q2 shipped on top of: "the loop re-runs on every keystroke" never distinguished the comptime half from the classification half, and reading it literally would have forced VM execution into every keystroke — the precise cost §9.1 rejected an imperative classifier to avoid. Splitting the rates keeps both properties: the author sees classification track their typing, and nobody pays comptime for editing prose.

## brink-compiler takes brink-runtime as a real dependency for conventions comptime (#1867)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** build architecture — conventions comptime (#1867, #1840)
- **SCOPE:** moderate (an architectural direction, taken deliberately rather than as a side effect)
- **WHAT:** **`brink-compiler` takes `brink-runtime` as a REAL dependency, not a dev-dependency**, so `fn conventions()` can be comptime-evaluated through `begin_function_eval`. The compiler links and executes the VM it emits bytecode for. **The narrower interpreter facade is the RECORDED FALLBACK, not a rejected option** — it is reachable on either of two named triggers: (a) a measurable compile-time regression attributable to linking the VM, or (b) a portability constraint from `docs/no-std-portability.md` that makes linking the VM into the compiler untenable.
- **WHY:** `begin_function_eval` already exists and is exactly what the 2026-08-01 conventions-comptime ruling named, so this costs zero new machinery. More importantly, ONE evaluator means comptime and runtime semantics cannot drift — the failure mode a second, narrower interpreter invites is a conventions module that behaves differently at build time than at run time, silently. That risk is worse than a fatter dependency graph. The facade is kept on the record with concrete triggers rather than discarded, because the objections to it (graph weight, the compiler carrying the VM's step limits and fault modes) are real but currently theoretical. ⚠ **This entry was written 2026-08-02, a day late.** The ruling was made on 2026-08-01 but recorded only in a session scratchpad — it never reached this log or issue #1867, which stayed open and `needs-design`. A wave-110 build agent was briefed that the dependency shape was ruled, correctly checked the issue, found no such ruling, and **declined to build #1840 on that basis**. The decline was right and the briefing was wrong: a ruling that lives only in a scratch file does not exist. This is the same failure the 2026-08-01 "issues and docs must state current state upfront" entry rules against, committed by its own author.

## Pattern-claiming confinement splits by dispatch kind, not by nesting (#1866)
- **WHEN:** 2026-08-01
- **PROJECT:** brink
- **SYSTEM:** conventions — claiming confinement (#1866, #1844, §9.1)
- **SCOPE:** minor/local (restates §9.1 rather than adding a rule)
- **WHAT:** Whether a claiming handler inside a `module { … }` block may claim is **the wrong axis**. The split is by **dispatch kind**, exactly as §9.1 item 4 already ruled: **pattern-claiming** handlers (`claims = "…"` — scene headings and the like) are confined to the **one conventions module** named in `brink.toml`; **`!name`-dispatched** handlers are **legal anywhere**, precisely because they self-announce at the call site. `E112`'s provisional "nested in a module block" placement fence should therefore be replaced by the dispatch-kind rule composed with `E169`'s file confinement, not kept as a nesting rule of its own. ⚠ Residual, flagged and assumed YES pending correction: a `module { … }` block **inside** the conventions file still counts as being in the conventions module (same file, same module).
- **WHY:** Recorded because the issue was filed as an open design question and closed as already-ruled, and that distinction is worth preserving: the answer was in §9.1 the whole time, and re-deriving it cost a round trip. Note also that the ruling was only half-enforceable when made — `!name` dispatch was reserved but unimplemented until #2004 landed 2026-08-02, so the confinement rule was in practice stricter than ruled for a day.

## `register` is a comptime-only intrinsic; calling it elsewhere is a diagnostic (#1840 Q5)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** conventions comptime (#1840)
- **SCOPE:** moderate (unblocks conventions v1c)
- **WHAT:** **`register` is a T1b intrinsic, legal ONLY inside the conventions module's `fn conventions()`**, where the comptime evaluator intercepts it during `begin_function_eval`. **No opcode, no runtime registry cell, no bytecode** — `fn conventions()` is comptime-consumed and never emitted, so the registry has no runtime life. Calling `register` anywhere else is a **compile error** (one new diagnostic code). Q4 had already ruled its *meaning* (a write to a named registry cell, the RNG-cell shape); this settles its lowering and legality. Rejected: a real opcode plus a runtime registry (ships machinery nothing reads), and an intrinsic that lowers to nothing outside comptime (a call that silently does nothing is the silent-drop class CLAUDE.md treats as a bug by default).
- **WHY:** Three consecutive waves declined #1840, the last of them on a genuine finding: `register` did not resolve at all (`E025`), and Q4 had explicitly rejected modelling it as an `EXTERNAL`, so it could not ride the existing external-call dispatch. The agent concluded there was no coherent runtime story and therefore no deliverable slice. The missing step was recognising that **`register` does not need a runtime story** — the registry exists only while the compiler is evaluating, so "comptime-only intrinsic" is not a limitation but the correct shape. The e2e objection dissolves the same way: a direct test of `register` is impossible, but a project with a conventions module producing classified output exercises it end to end, which is the behaviour anyone actually cares about.

## Span-nested inline conditionals lift; the lifter recurses into spans (#1737)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** prose dialect — template lifting × markup (#1737)
- **SCOPE:** moderate
- **WHAT:** **`hir::normalize::try_lift_inline` recurses into `Span::children`**, so `<b>{cond: a|b}</b>` lifts to template form like any other inline conditional and the span survives. **Accepted cost:** markup-wrapped conditionals multiply translation-table entries per branch, so a line's translation footprint grows with the number of branches inside spans. ⚠ The implementation must preserve §4.3's **line-scoped span invariant** — the locale system swaps whole line vectors by index and hard-rejects count mismatches, so a span may never split or cross line-table entries. Rejected: ruling the shape out of scope in §4.4, and diagnosing it instead of flattening.
- **WHY:** §4.3 already rules that markup and logic nest freely inside each other; the flattening fallback was an implementation gap, not an expression of policy, and it lost the author's span **silently**. Ruling the shape out of scope would have carved an exception into a nesting promise the spec makes unconditionally, to save a translation-table cost that is real but bounded and paid only by lines that actually use the shape. The diagnose-instead variant was the right answer only if the shape stayed unsupported; once it works, there is nothing to diagnose.

## Ink's `?` / `!?` list-match operators route through UFCS, not a native operator (#2043)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** native expression grammar × respell (#2043)
- **SCOPE:** minor/local
- **WHAT:** **No new native operator.** The respeller lowers ink's `list ? item` to `list.contains(item)` and `list !? item` to `!list.contains(item)`, using the prelude verb that already exists. Zero native grammar change. ⚠ **Owed by the implementation: verify precedence and chaining equivalence** — an operator and a call parse differently, so any ink idiom relying on `?`'s precedence, or on chaining, must produce the same tree through the UFCS lowering. Pin it with a differential, not an assumption. Rejected: importing ink's `?`/`!?` spelling verbatim, and minting a native `has`/`!has`.
- **WHY:** The native surface has consistently preferred readable spellings over punctuation — the 2026-08-01 `#fn` → bare-name ruling turned on exactly that reasoning — and `?` as containment is not self-evident to a reader who does not already know ink. But designing a *new* native operator was equally unattractive when `contains` already exists in the prelude and says what it means. Routing through the existing verb costs no grammar, no spelling decision, and no teaching surface; the only thing it owes is proof that the parse trees agree.

## Unrecognized inline escapes are rejected (#2040)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** prose dialect — the escape set (#2040, §8d.6)
- **SCOPE:** moderate (breaking for some existing content)
- **WHAT:** **An unrecognized inline escape is a compile error.** `\q` is diagnosed, naming the four valid escapes (`\<` `\{` `\#` `\\`). One new diagnostic code. ⚠ **Breaking:** prose containing a literal backslash before a letter (Windows paths, LaTeX-ish text) now errors until doubled as `\\` — the diagnostic message MUST name the four valid escapes **and** point at `\\` for a literal backslash, or the fix is not discoverable. ⚠ Apply **consistently across every scanner in §4.6's audit table** (`tag()`, `cue_name()`, `scene_title`, `parenthetical`, content); a rejection in one and a pass-through in another is the exact asymmetry class #1738/#1883 exist to close.
- **WHY:** §8d.6 ruled the escape set final, and a closed set that silently accepts members outside it is not closed in any way an author can rely on. The realistic failure is a typo — an author reaches for an escape that does not exist, gets no feedback, and ships a stray backslash to the reader. That is the same silent-degradation shape the `~ stmt` bug had. The breaking-change cost is real but narrow and mechanically fixable, and it is discoverable at build time rather than in the finished story.

## Word-break springs are internal; native gets no spelling (#1976)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** prose dialect — `ContentPart::Spring` (#1976)
- **SCOPE:** minor/local (retires a `needs-design` flag)
- **WHAT:** **Nobody writes springs — it is an internal concept.** No native authoring spelling exists and none is owed. The respeller emits whatever internal form the 12 ink corpus cases need and the emitter round-trips it. ⇒ #1976 stops being `needs-design`; its remaining work is purely the respell/emitter round-trip for those 12 cases — an emitter task, not a grammar one — and no book or spec teaching surface is owed.
- **WHY:** A spring is a layout artifact of how ink handles overflow, not something an author reasons about while writing a scene. Giving it a native spelling would add grammar, spec text and book coverage for a concept whose entire corpus presence is incidental. Keeping it internal also keeps the native surface's stated smallness honest: the respeller's job is to preserve ink's *behaviour*, not to surface every mechanism ink used to achieve it.

## Lifted-lambda identity: HIR mints, LIR consumes (#1727)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** lambda lifting × effect fixpoint (#1727, #1770)
- **SCOPE:** architectural
- **WHAT:** **The direction inverts: HIR stamps a lifted lambda's identity and `alloc_lambda_address` READS it**, rather than deriving its own from `ctx.scope_path`. ⚠ Owed by the implementation: LIR's allocator stops being self-contained, so every path that lowers a lambda must carry the HIR id through — name those paths in the PR. Rejected: a path-independent per-body ordinal (non-self-describing, and inserting a lambda earlier renumbers later ones — a save-key hazard if these ever persist), and a v1 scope covering only non-nested lambdas.
- **WHY:** The id-parity problem exists only because the id is derived **twice** from a structure that differs between the two derivations: `ctx.scope_path` is mutated while descending into conditional/sequence/choice bodies, so a lambda nested in a branch gets a LIR path that HIR-time minting cannot reproduce. Every option that keeps two derivations has to keep them agreeing forever; removing the second derivation removes the class of bug rather than one instance. The v1-scope alternative was tempting because it unblocks #1770's common case cheaply, but it leaves the effect fixpoint incomplete for precisely the nested shape #1770 exists to make sound — an unsound analysis that reports success is worse than one that is honestly incomplete. ⇒ Unblocks #1770.

## `HASH` stays depth-blind and `\}` stays unconditional in tag()/cue_name() (#1883)
- **WHEN:** 2026-08-02
- **PROJECT:** brink
- **SYSTEM:** prose dialect §4.7/§4.7b — tag()/cue_name() raw-text scan (#1883, #1787, #1852, #1738)
- **SCOPE:** minor/local (confirms two existing rulings extend to answer both items; no code change)
- **WHAT:** Issue #1883's two remaining open items are both resolved **without a code change**. **(1) `HASH` does NOT become depth-aware in `cue_name()` the way `COLON` did (#1851).** §4.7's own per-tag-scope ruling ("a `HASH` is a real, tokenized boundary … unlike the raw, grammar-blind `{`/`}` this scan balances") already answers this: `COLON`/`R_BRACE` are exactly that raw, grammar-blind kind of punctuation, but an unescaped `HASH` always begins its own `TAG` node — gating it by `depth == 0` would make that structural boundary conditional on unrelated brace-balance, the same blurring the existing ruling already rejects. `@NAME {a#b} c.` still fails to parse. **(2) `\}` does NOT gain `\{`'s backslash-parity carve-out, in either `tag()` or `cue_name()`.** `\{`'s carve-out exists because `\{` is one of the ruled, final four-character inline escape set (§8d.6: `\< \{ \# \\`); `}` is not a member of that set, so there is no equivalent "`\}` is a literal, non-metacharacter close-brace" ruling to protect against — an `R_BRACE` preceded by a `BACKSLASH` is exactly what it looks like, an ordinary backslash followed by an ordinary, structurally significant `}`. `#tag \{a\}`/`@NAME \{a\}` still end at the `\}`. Both confirmations are pinned by new tests (`a_hash_inside_an_open_brace_still_ends_a_cue_name_early`; `a_tags_own_unescaped_closing_brace_remains_the_terminator_even_when_preceded_by_a_backslash` and its `cue_name()` sibling) and written up in `docs/prose-dialect-spec.md` §4.7b. The issue's third item (corpus fixtures for `\\{` and a braced cue name) and a wave-111 retro's fourth item (a §4.7 cue-name cross-reference, now §4.7a) are also delivered in the same PR.
- **WHY:** The issue itself offered exactly these two resolution paths ("get a ruling, or confirm the existing one still applies" / "fix the parity gap, or record explicitly why it's intentionally asymmetric") as valid outcomes, not just "fix it" — and re-deriving the existing rulings' own stated reasoning shows both already-established principles extend cleanly to answer these specific questions, rather than leaving them open pending a fresh design sitting. Treating "is this character a member of the ruled escape set" as the deciding question (item 2) keeps the depth counter's backslash-awareness scoped to what it exists to protect — a ruled escape's meaning — rather than generalizing "any backslash-adjacent brace" into a new, unruled escape. Treating "does this token always start its own CST node" as the deciding question (item 1) keeps `HASH`'s role as an absolute grammar boundary, not a raw character this scan happens to balance.

## An empty content/Fragment capture renders nothing, at read time, line-table entry untouched (#2091)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** runtime output resolution — `brink-runtime::output` (#2091, #1839, #1720/#2081)
- **SCOPE:** minor/local (a read-time rendering rule; no compiler/line-table change)
- **WHAT:** **An empty `content`/Fragment interpolation renders as nothing, not a blank line.** A resolved line is suppressed from reader-visible output only when (a) its fully-resolved text is empty, (b) it carries no tags, and (c) at least one of its parts interpolated a `content`-typed value (`Value::FragmentRef`) that itself rendered empty. Two distinct call sites produce that `FragmentRef`, and the check cannot (and does not try to) distinguish them — both are suppressed identically: issue #1839's `block`-capture receiver (e.g. a cue immediately followed by a parenthetical, where `capture_block`'s terminator ends the run at zero interior lines), AND the ordinary **display-position call-composition** pattern `brink-codegen-inkb::content::emit_slot_expr` emits for *every* template slot whose expr is a function call (`lir::Expr::is_function_call()`, both dialects, e.g. a line whose only content is `{ f() }`) — not only the block-capture case this issue was filed against. **The line-table entry is present-but-empty, never omitted or renumbered** — this is purely a read-time decision in `resolve_lines`/`take_first_line` (`brink-runtime::output`), so locale hot-swap (index-matched line-vector swap) is unaffected. **Deliberately excluded:** a line that resolves empty for any other reason — a literal blank line, or a self-closing inline markup span (`<pause/>`) with no children — keeps its existing blank-beat behavior (`inline-markup-point-marker` fixture, issue #1716), which is a separate, already-settled question, not this issue's scope. `tests/tier1-native/conventions-screenplay-preset/`'s `expected.txt` (pinned as-is by issue #1720/PR #2081) is updated to drop the now-suppressed blank line.
- **WHY:** A `block`-capturing handler whose captured region is legitimately empty (per #2072's undecided-but-permitted default) still binds a real, present `Value::FragmentRef` to its `content` param — interpolating it alone on a template line used to still consume a full line's worth of output, a visible blank the reader never asked for and the author had no way to suppress. The discriminator (`part_involves_fragment_ref` — was a `FragmentRef` involved at all) is structural and does not, and cannot, tell a block-capture `FragmentRef` apart from an ordinary call-composition one, so the rule's actual scope is broader than the filing handler shape: any `content`-typed emptiness, not `block`-capture specifically. That structural scope is still narrower than "any line that resolves empty" — it avoids re-litigating the inline-markup point-marker's already-pinned, deliberate blank-beat behavior, a different mechanism (a literal, slot-free `Span`) with no dynamic value driving the emptiness, so the same discriminator cleanly tells the two apart without needing to touch #1716's fixture.

## #2091's suppression extends to the string-capture and fragment-interior paths (#2147)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** runtime output resolution — `brink-runtime::output::resolve_parts` (#2147, #2091, #2140)
- **SCOPE:** minor/local (extends an existing read-time rule to sibling call paths; no new discriminator)
- **WHAT:** **The #2091 empty-`content`/Fragment suppression applies uniformly across every `resolve_parts`/`resolve_lines_annotated` call path, not only the streaming/batch path #2140 fixed.** `resolve_parts` (`OutputBuffer::end_capture`'s `Opcode::EndStringEval` string-capture path, and `OutputBuffer::resolve_fragment` — the resolver `ChoiceDisplay::Fragment` reads through) now carries the identical per-line invariant `resolve_lines_annotated` already had: a line is dropped, not left as a blank, when it resolves fully empty and at least one part interpolated a rendered-empty `Value::FragmentRef`. This also reaches the **interior** of a nested, multi-line fragment (a fragment whose own captured region spans more than one line, where an interior line is contributed purely by a further-nested, rendered-empty fragment) — not only a transcript line's own top level. Parity also covers the trailing, unterminated-line case: `resolve_lines_annotated` drops its own final entry (and the newline that would have introduced the next one) when it is itself suppressible; `resolve_parts` now does the same rather than leaving a stray trailing newline `resolve_lines` would not have produced.
- **WHY:** #2091's own review flagged this as a gap: PR #2140 fixed the streaming/batch line-splitting path but left `resolve_parts` — reached from `end_capture`, `resolve_fragment`, and (recursively) from inside `resolve_lines_annotated` itself whenever a rendered line references a fragment — with no suppression at all, so the same empty-capture shape rendered a visible blank line in a captured string (an unrecognized choice display, a `~ temp x = "..."` string-eval) or inside a fragment's own multi-line interior, while the top-level transcript path had already stopped doing that. Leaving the two paths disagreeing would have meant the same authored content renders differently depending on which internal path happens to resolve it — an implementation detail the author has no way to reason about or route around. A single discriminator applied everywhere is simpler to hold correct than one copy per call path, and matches #2091's original framing: this is about `content`/Fragment-driven emptiness generally, not the one code path its first fix happened to land in.

## `fn conventions()` is DISSOLVED — handler precedence is a property of the `@[element]` annotation
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** language design (#1351 prose rethink) — conventions/elements; reverses §9.1 item 5 and the whole #1840 Q1–Q6 line
- **SCOPE:** architectural
- **WHAT:** **The well-known `fn conventions()` registration function is removed from the design, and with it the `register` intrinsic and the comptime-evaluation requirement it existed to justify.** Handler precedence becomes a declared property of the annotation that already declares the handler: `@[element(claims = "…", order = …)]`. This **reverses** the 2026-07-31 §9.1 sitting's item 5 ("the conventions module exports a well-known `fn conventions()` that REGISTERS handlers in order — imperative registration, statement order is resolution order") and item 9 ("Addendum 2's 'zero comptime' is REVERSED as a restriction: conventions need comptime evaluation from day one"), and it moots the entire #1840 Q1–Q6 chain: **Q1** (two independent reads joined on `DefinitionId`) collapses to ONE read, because order now arrives on the same annotation as the pattern; **Q2** (comptime fault behavior), **Q3** (the name-level stack-trace compromise), **Q4** (`register`'s effect row / the named registry cell), and **Q5** (`register` as a comptime-only intrinsic) all describe a mechanism that no longer exists; **Q6** (how the evaluator intercepts `register` — the contradiction six agents in a row correctly declined to resolve) is dissolved rather than answered. The 2026-08-02 **#1867 ruling** (`brink-compiler` takes `brink-runtime` as a REAL dependency so `fn conventions()` can be evaluated through `begin_function_eval`) **loses its stated justification** and is withdrawn *for this purpose*; `brink-compiler` keeps `brink-runtime` as a dev-dependency. **Comptime evaluation is DEFERRED, not rejected** — it is a capability the maintainer wants, but it will be decided in a context that genuinely needs it, rather than adopted as a side effect of expressing one ordering. **UNAFFECTED and still ruled:** the 2026-08-02 match-ordering ruling (the walk tries every registered pattern, uses the first match, records the rest as shadowed, memoized per `(line text, projection revision)`) is untouched — only the *source* of the order changes, not the semantics of using it. Pattern-claiming stays confined to the one `brink.toml`-named conventions module (§9.1 item 4); `!name`-dispatched handlers stay legal anywhere and need no `order` at all, since they self-announce rather than compete for a line.
- **WHY:** `fn conventions()` was never wanted for itself — it was a workaround for a type-system gap. The original design was `CONST conventions = [...]`, which is not representable because the handlers are heterogeneous (`heading(kind, title)`, `transition(text)`, `cue(name, body: content)`) and the array has no element type; §9.1 item 8 records that causation exactly ("with registration there is no collection, so no heterogeneous-signature problem, so no adapters"). Registration was the way to build an untypeable list without naming its type. But the *information content* of that entire mechanism is one ordering — and an ordering is expressible declaratively on the annotation that already carries the pattern, the captures, and the handler identity. Q6's contradiction was the symptom, not the disease: the design had reached a point where the compiler had to link, emit, and execute a VM in order to learn something the source text already states. Six consecutive declines were the mechanism telling us the task was ill-posed. Putting `order` on the annotation deletes the comptime dependency, the `register` intrinsic and its E175 confinement pass, the `CONVENTIONS_REGISTRY_CELL` named cell and its `conventions_write` effect row, the compiler→runtime dependency, and the comptime-fault/error-mapping questions — roughly 880 lines of landed code plus four open rulings — while preserving every property the design actually wanted: precedence is explicit and authored, claiming stays confined and auditable, no-invisible-expansion holds (every matched line still points at a real function), and the editor still reads a serialized projection. It also makes the projection **statically computable**, which unblocks the NS-T editor seams that were queued behind #1840. Cost accepted knowingly: removing registration removes *subtraction* — with a registration list, a project importing a preset could decline to register one of its handlers; with declaration-site ordering it cannot, since the handler is declared in the imported file. Whole-module swap still works (a comics project writes its own module and imports no preset). Per-handler opt-out is now an OPEN question, recorded below rather than invented here.
- **SUBTRACTION — RULED 2026-08-03 (owed item 2, closed): there is no subtraction, because there is no implicit inclusion.** The `brink.toml`-named conventions module is the SOLE source of active claiming handlers: what it declares is what the project gets, and nothing is installed by importing. A default project points `elements` at the built-in preset, so the preset file *is* that project's conventions module and all of its handlers are active. A project that wants a subset, or wants to reorder, points `elements` at its **own** module and **respells each handler it wants explicitly** — an ordinary declaration carrying its own `claims` and `order`, whose body may be a one-line delegation to the `std::` implementation. Verbosity is accepted deliberately: a conventions module is small, authored once, and edited rarely, so an explicit list that reads top-to-bottom as the precedence order is worth more than a terse import-and-remove facility. This also keeps §9.1 item 6's property intact under a different mechanism — the contract for a swapped-in module is still exactly the list of handlers it declares, with no required roles and no protocol.
- **CONSEQUENCE TO SETTLE (new, created by the subtraction ruling):** the delegation pattern collides with #1844's landed confinement check (E169), which makes an `@[element(claims = …)]` declaration *illegal* outside the named conventions module. A custom module that delegates must be able to reference the preset's handler bodies without the preset's own annotations firing a confinement diagnostic. Either the preset exposes its handler bodies as plain, unannotated functions with the annotations living only in the preset-as-conventions-module, or confinement weakens from "illegal outside" to "inert outside". **Not ruled here** — but nothing can import the preset until #2080 lands `std::` resolution anyway, so this is owed before delegation is reachable, not before the annotation work starts.
- **ORDERING KEY — RULED 2026-08-03 (owed item 1, closed): a BARE INTEGER.** Not a sparse-integer convention, not named anchors (`order = before(cue)`). Rejected for the same reason the rest of this ruling exists: an anchor form reintroduces cross-declaration references — a small dependency graph to resolve, with cycles to detect and diagnose — to express something a plain number already expresses. Sparse-by-convention is a documentation practice authors may adopt freely, not a language feature to build.
- **STILL OWED (not ruled here, do not invent):** (1) ~~ordering key shape~~ — RULED above. — a bare integer, a sparse integer convention, or named anchors (`order = before(cue)`); each trades collision-handling against readability across module boundaries. (3) **Cross-file tie-breaking** — two handlers with equal `order` must resolve deterministically (module path then declaration order is the obvious candidate, given the house determinism rule), and `order`'s absence needs a defined default. ⚠ Note that under the subtraction ruling, (3) is now a MUCH narrower question than when it was filed: all active handlers are declared in ONE module, so cross-*file* ties arise only if a future ruling reintroduces multi-file installation.

## Claiming and `!name` dispatch split into two annotations: `@[convention]` and `@[element]`
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** language design (#1351 prose rethink) — conventions/elements authoring surface
- **SCOPE:** moderate
- **WHAT:** **The single `@[element(…)]` annotation splits in two, by mechanism.** **`@[convention(claims = "…", order = N)]`** declares a *pattern-claiming* handler: it competes for lines it did not announce, so it needs a precedence (`order`, a bare integer per the ruling above) and it stays **confined** to the `brink.toml`-named conventions module (§9.1 item 4). Scene heading, cue, parenthetical are conventions. **`@[element(args = "…", block)]`** declares a **`!name`-dispatched** handler — `!radio` and friends: self-announcing, invoked explicitly by the author at the site, **legal anywhere**, and carrying **no `order` at all**, because a handler that names itself never competes for a line. **This narrows §9.1 item 1's "there is ONE element mechanism, not two" to the LOWERING layer, where it remains true and unchanged:** both still lower to a matched line, captures bound to params by name, and exactly one call to an annotated handler. What splits is the **authoring surface**, not the mechanism underneath it.
- **WHY:** One annotation was carrying two disjoint property sets under two different legality rules — a union type pretending to be a single concept. `order` is meaningless on a `!name` handler; confinement applies to claiming and explicitly does not apply to dispatch (§9.1 item 4 rules `!name` handlers legal anywhere *precisely because* they self-announce). An author reading `@[element(claims = …)]` next to `@[element(args = …)]` has no cue that one is confined-and-ordered and the other is neither. The split also matches the structure the implementation already has: `element.rs` keeps `ClaimHandler` and `DispatchHandler` as separate types in separate collections (`handlers` vs `dispatch: BTreeMap<String, DispatchHandler>`), resolved by separate entry points (`try_claim` vs `try_dispatch`) — the code had already discovered these were two things while the surface still called them one. The naming follows the concepts rather than the implementation: a **convention** is an implicit rule about how prose is read ("an all-caps line is a cue"), which is exactly what a claiming pattern encodes; an **element** is something the author explicitly places. This also makes "the conventions module" literally the module of `@[convention]` declarations, which it previously was not.
- **CONSEQUENCE TO SETTLE — SETTLED 2026-08-03, see "The `brink.toml` key ... renamed from `elements` to `conventions`" entry below (issue #2180, PR #2214):** the `brink.toml` key **`[project] elements = "screenplay"` is now misnamed** — it points at the module of `@[convention]` declarations, and under this ruling `element` means the other thing. Renaming it to `conventions` would make the vocabulary consistent; it is a config-surface break, so it is recorded here rather than ruled. #1844's confinement check reads this key.

## `order` is REQUIRED on `@[convention]`, and duplicates within a module are a compile error
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** language design (#1351 prose rethink) — conventions/elements authoring surface
- **SCOPE:** moderate
- **WHAT:** **`order` is a REQUIRED property of `@[convention]`, not an optional one, and two `@[convention]` declarations carrying the same `order` within the same module are a COMPILE ERROR.** This closes both sub-questions the bare-integer ruling left owed: there is no "default when `order` is absent", because it can never be absent; and there is no tie-breaking rule, because ties are rejected rather than resolved. Precedence over a project's conventions is therefore **total, explicit, and authored** — the compiler never infers, defaults, or falls back to declaration order to decide which pattern is tried first. Two diagnostics are owed: a **missing `order`** on a `@[convention]`, and a **duplicate `order`** within one module (which should name both declarations, the way a duplicate-definition error does). `@[element]` is unaffected — it takes no `order` at all, since a `!name`-dispatched handler announces itself and never competes for a line.
- **WHY:** The maintainer's framing is the argument: *"you just have to tell us what order you want them in."* Every alternative to requiring it reintroduces exactly the implicitness this whole line of rulings was removing. An optional `order` with a default means some conventions are ordered by the author and others by a rule the author has to know, in one list, resolved silently. Allowing duplicates means a tie-break rule, and any tie-break rule is a second, invisible ordering mechanism sitting underneath the visible one — precisely the shape of the accidental declaration-order precedence the preset shipped with (#2166). Rejecting duplicates also makes the failure **local and legible**: two numbers that collide is a one-line fix an author can see, whereas a silent tie-break produces a working build whose behavior nobody chose. The cost is real and accepted: adding a convention to a dense list may mean renumbering neighbors, since bare integers were ruled over sparse-by-convention and named anchors. That cost is paid once, in a file that is small, authored rarely, and read top-to-bottom as the precedence order it encodes.

## The element output model: `attach = StructName` is the declared handler schema; a no-world-reads fence is owed on convention handlers
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** language design (#1351 prose rethink) — conventions/elements authoring surface
- **SCOPE:** moderate
- **WHAT (item 2, DELIVERED by issue #2178):** `@[convention]` gains an optional `attach = StructName` clause — the handler's declared **output schema**. The governing split: **declared** (this clause: which keys, what types — static, editor-readable, cacheable) vs. **computed** (the handler body: the actual values). The schema is a plain `struct` name, not a new declarative sub-language — a `struct` is already declarative, statically known, serialized, and understood by compiler + editor + host. A handler may never attach a *computed key name*: dynamic key names would destroy the static projection both the editor and the load-time host-binding check depend on. Checked at the declaration (`E180`): the handler's own `: Type` return annotation must name the same struct `attach` does.
- **WHAT (item 3, NOT BUILT — tracked as #2179):** a convention handler may call pure fns and commands, but may never read world state, enforceable now (no comptime needed) because: if classification depended on game state, the editor could never display it, the projection could never be cached, and explain-match would depend on a save file. The original comment's instruction was to wire the *existing* capability-manifest/effect-row join (`compute_container_access` in `crates/bevy-brink/src/capability.rs`) rather than write a new analysis. Investigating for #2178/#2179 found that mechanism does not fit: it lives in `bevy-brink`, which `brink-ir`'s own `host_manifest.rs` module doc says must never be depended on by the compiler (and the reverse edge is equally unwanted); it operates on already-codegen'd `Program`/`EffectRowEntry` data with no source spans at all (needed for "diagnostic must point at the offending call"); and it requires a live, host-registered `CapabilityRegistry<M>` that does not exist at `brink compile` time. The other same-repo "capability manifest" (`brink_ir::host_manifest::ExternalKind`/`HostManifest`, `Query`/`Effect`/`Presentation`/`Plain`) is compile-time and IDE-facing but is explicitly documented as advisory-only ("never consumed by the runtime or by codegen... only enriches") and defaults to `Plain` (unclassified) at nearly every call site in the analyzer — neither a "trust it" nor a "distrust it" default is sound as a hard compile-time gate built from that data. Building the fence therefore needs either a new default-classification policy for unclassified externals (exactly the kind of undetermined default the parent ruling's own "do not invent it" caution warns against) or a new analysis outright — both out of scope for a "wire the existing join" instruction. Left for #2179 to design properly rather than force a wrong wiring.
- **WHY:** Both items are independently justified in the original comment: the schema-as-struct design keeps the projection static and cacheable without inventing a sub-language; the no-world-reads fence keeps a convention handler's classification independent of game state so the editor/projection/explain-match tooling can all treat it as a pure function of source text. Item 3's investigation is recorded here so the next agent does not re-discover the same architectural mismatch from scratch. Recorded late: this entry transcribes items 2–3 of issue #2164's 2026-08-03 "design backport" comment, which stated both rulings but never itself landed in this log (PR #2176 only logged the `order` ruling above via #2160; these two items were left as comment-only rulings until this entry). Item 1 of that same comment (the attach-vs-wrap attachment MODE ruling) is **not** transcribed here — it is tracked separately at icebox issue #2169. Per the "a ruling lands in a spec, not only in the log" discipline, both items above also have a spec home: item 2 (`attach`) is documented in `docs/prose-dialect-spec.md` §3.5b and `docs/directive-annotations-spec.md`; item 3 (no-world-reads) has none yet, since it is not built.

## Stdlib mounts into `Environment`'s manifest at the producer, as plain source (#2080)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** compiler (`crates/internal/brink-environment`) — the `std::` mount
- **SCOPE:** minor/local (implements an already-ruled mechanism; no new design)
- **WHAT:** `Project::load` mounts the `std/` stdlib source tree (currently just
  `std/conventions/screenplay.brink`) into every `Environment`'s manifest,
  embedded at compile time via `include_str!` (so it mounts identically on
  hosts with no filesystem, e.g. `@brink-lang/web`'s wasm build). No new
  resolution mechanism was built: #2080's 2026-08-03 issue ruling found
  `Environment` (#1306) already generalizes `the-tree-is-the-universe` to
  `the-environment-is-the-universe` (`{ local module tree } + { resolved
  external module set }`), and native module identity is a pure function of
  a source's string key — so the stdlib needs no virtual module tree, no
  second `FileId` space, and no bespoke preset registry. It joins the
  manifest exactly like any project file, under the same root-relative,
  forward-slash key convention; `brink_db::modules::native_module_path`
  mints its identity the same way it would for a project file at that path.
  A project source already present at the same key wins over the embedded
  copy rather than being silently clobbered. The stdlib ships as **source**
  in the manifest, not as a `resolved_deps` entry — that slot stays reserved
  for #1093's compiled per-module artifacts.
- **WHY:** The issue's own original framing (a virtual/embedded module tree
  distinct from tree-is-universe discovery) assumed a problem that the
  `Environment` seam had already solved by design; building a second
  mechanism would have reopened exactly the parallel-universe surface
  `external_conventions` is being deleted for (#2165). Embedding via
  `include_str!` (rather than a real on-disk path) is required because the
  compiler must run in wasm with no filesystem — the same constraint that
  already keeps `brink-conventions export`/host I/O injected rather than
  baked in.
- **SCOPE FENCE (explicit, not an oversight):** this is the *mount* only —
  the stdlib source is now present in every compiled environment's db, but
  nothing in it is marked `pub` and no confinement rule scopes what a
  project's own `use` may reach into it. A real `use std::…` importing an
  item out of the mounted module still needs #1582's pub marker and #2167's
  closure-scoped confinement — both ruled, neither built here. Also
  untouched: the `[project] elements` → `conventions` rename the same
  2026-08-03 ruling records as owed (a breaking config change needing its
  own deprecation path and reader sweep) — a separate, larger piece of work
  from the mount itself.

## The LSP holds one `ProjectDb` per governing `brink.toml`, plus a per-file orphan carve-out (#1580)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** `brink-lsp` (native project-extent partitioning) — refines the
  2026-07-22 native-project-root ruling (NF-3, lines ~1836-1842 above) for the
  editor specifically
- **SCOPE:** architectural (changes the LSP from one shared native project to
  N independent ones, each with its own `ProjectDb`, `FileId` space, and
  symbol index)
- **WHAT:** `brink-lsp` no longer applies `brink_driver::native_source_root`
  once to `roots.first()`. `NativeProjects` (`crates/brink-lsp/src/backend/projects.rs`)
  discovers **every** governing `brink.toml` in the opened workspace (walking
  every workspace folder *downward*, not just up from one starting point) and
  gives each its own fully independent `ProjectDb` — own native root, own
  symbol index, own `DefinitionId` space, disjoint `FileId` ranges via a new
  `ID_STRIDE`-wide stride per project (`ProjectDb::with_id_base`). A `.brink`
  file under no governing `brink.toml` at all falls back to NF-3's rootless
  single-file-project mode (`NativeProjectKey::Orphan`, rooted at the file's
  own directory) **only when some other part of the workspace has a real
  `brink.toml`**; when the *whole* workspace is unconfigured, every native
  file still shares the single legacy default project, preserving the
  pre-#1580 "open a folder of `.brink` files with no config yet" workflow
  byte-for-byte. Only the default project's root is also declared as the ink
  root (`set_ink_root`) — ink's project extent is INCLUDE-reachability from
  that one root, unaffected by this issue, so `native_root`/`ink_root`
  diverge by design on any workspace with more than one governing
  `brink.toml`.
- **TENSION WITH NF-3, NAMED RATHER THAN HIDDEN:** the wholly-unconfigured
  fallback above is a **knowing, non-compile-identical carve-out**, not an
  instance of "editor extent equals compile extent." A real standalone
  compile of a rootless file roots it at *that file's own directory*
  (`native_source_root_inner`'s entry-relative fallback); the legacy default
  project this branch falls back to is rooted at the *first workspace
  folder* instead. It is also, by construction, the one case where an
  unrelated `brink.toml` appearing **elsewhere** in the workspace changes a
  file's module identity without anything changing above that file's own
  directory — exactly the class of hazard NF-3's ruling names ("the same
  file resolved to `story::market::barter` or `story::barter` depending on
  whether a config happened to sit above it"). It is accepted here, and
  documented as a carve-out rather than silently shipped, because the
  alternative (`Orphan` unconditionally) would fragment the "no config yet"
  workflow the existing `crates/brink-lsp/tests/integration.rs` "Native
  `.brink` workspaces" tests pin. Whether the wholly-unconfigured fallback
  should instead be `Orphan` unconditionally is **left open**, not resolved
  by this issue.
- **KNOWN LIMITATION:** a `brink.toml` that appears, moves, or disappears
  mid-session re-syncs every already-discovered project's own root, but does
  not retroactively move an already-admitted file to a newly discovered
  sibling project — that file keeps its original project until closed and
  reopened, or the session restarts. Only the steady-state extent (what a
  fresh workspace load computes) is guaranteed correct.
- **WHY:** Native module identity is root-relative (#1576) and computed off
  a single Salsa `native_root` input per `ProjectDb`. A workspace with more
  than one governing `brink.toml` — story sources plus a fixture or example
  project elsewhere in the tree — previously had only one recognized root;
  every file outside it fell back to `root_relative_key`'s
  absolute-path-embedding identity instead of the clean, root-relative name a
  real standalone compile of its own `brink.toml` would mint. Namespacing
  `DefinitionId` keys instead (one shared db, tagged identities) was
  considered and rejected: it would diverge from what a real compile of
  either project alone mints, the exact divergence class the NF-3 ruling
  calls out as unacceptable. Recorded here per the "a ruling lands in the
  decision log, not only a module doc / PR body" discipline (rules 20d/21b) —
  this PR's own module doc (`projects.rs`) and PR #2202's body stated both
  the per-`brink.toml` partition and the orphan carve-out, but neither had
  landed in this log until now.

## `[project] elements` renamed to `[project] conventions`, with a deprecated warning-emitting alias (#2180)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** brink-project-config / brink-analyzer / brink-db — conventions/elements config surface
- **SCOPE:** moderate
- **WHAT:** The `brink.toml` key that names the project's conventions module is renamed from `elements` to `conventions`, settling the "CONSEQUENCE TO SETTLE" left open by this same date's "Claiming and `!name` dispatch split into two annotations" entry above (the key predates that split and, post-split, named a module of `@[convention]` declarations rather than `@[element]` ones — a misnomer). **The old key is not hard-broken**: `brink-project-config::parse_str_at` still accepts `[project] elements` as a deprecated alias — it is parsed into the same `ProjectConfig::conventions` field and behaves identically downstream, but emits a `ConfigWarning` naming the rename. If both `elements` and `conventions` are set in the same file, `conventions` wins and a second warning names the conflict. Every reader of the old key/field/function names is renamed alongside it: `brink-project-config`'s parser and `ProjectConfig::conventions` field, `brink-analyzer`'s `AnalysisOptions::conventions` field, `is_path_shaped_conventions_pointer` (was `is_path_shaped_elements_pointer`), `validate_conventions_preset` (was `validate_elements_preset`), `BUILTIN_CONVENTION_PRESETS`/`INJECTABLE_CONVENTION_PRESETS` (were `BUILTIN_ELEMENT_PRESETS`/`INJECTABLE_ELEMENT_PRESETS`), `brink-db`'s `conventions_confinement_diagnostics_query` (#1844/`E169`) and `register_intrinsic_diagnostics_query` (#1840/`E175`), `brink-ide`'s two `AnalysisOptions` construction sites, the wasm-exported `EditorSession::apply_project_config` (`@brink-lang/web`), `E169`'s and `E175`'s diagnostic titles/docs, `docs/prose-dialect-spec.md` §3.4/§3.5, `docs/directive-annotations-spec.md`, `docs/conventions-comptime-sizing.md`, `docs/native-feature-status.md`, and `docs/compiler-spec.md`'s diagnostic-code table.
- **WHY:** A rename alone, with no deprecation path, would silently un-configure every existing project's conventions module the moment it upgrades brink — the exact "a configured project becomes silently unconfigured" failure mode the issue itself calls out, and worse than a compile error because nothing would tell the author anything changed: `E169`'s confinement enforcement would just stop firing. A hard error was considered and rejected for the same reason a warn-and-alias is strictly better here: the deprecated spelling is unambiguous (there is only one old name, no ambiguity about migration), the fix is a one-line rename an author can make at their own pace, and the warning path costs nothing to keep around for a release or two. `conventions` winning over `elements` when both are set (rather than an error) follows the same posture the file's other "unset means untouched" fields use — the more-specific/newer key is the one advancing, and a second warning still tells the author about the redundant old key rather than silently dropping it.

## Native visibility marker: a `pub` keyword (#1582)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** language design — modules/visibility (M-2, `docs/modules-spec.md` §4) on the native surface
- **SCOPE:** moderate
- **WHAT:** **A native declaration opts into cross-module visibility with a `pub` keyword preceding it** — `pub flow`, `pub fn`, `pub var`, `pub const`, `pub struct`, `pub extern`, `pub flags`. Absent `pub`, a declaration stays **Private**, which the 2026-07-23 ruling already ratified as intended. `pub` is the native spelling of the brink dialect's `#@public`; it produces the same `VisibilityMark::Public` the analyzer already consumes, so `effective_visibility` and every downstream gate are unchanged. **Rejected: an `@[pub]` annotation** — consistent with the annotation channel native already uses heavily, and cheaper to parse, but visibility is the most frequent modifier in the language and belongs in the signature rather than as metadata attached to it; every language with this exact model (Rust, Swift, Kotlin) spells it as a keyword. **Rejected: inverting the default** (public unless marked `priv`) — it would have been the cheapest migration, but it directly reverses the 2026-07-23 ruling, which was recorded as *"ratified as intended, not a side-effect to undo."*
- **WHY:** The issue framed this as an open choice between "add a marker" and "let a `use`-qualified path suffice." **The second was never available.** `check_cross_module_refs` (`crates/internal/brink-analyzer/src/modules.rs`) implements two INDEPENDENT gates: a private target is **`E087` unconditionally**, and only a *public* target in another declared module reaches the second gate (`E025` unless imported). Native already has `USE_DECL` in its grammar, so the import half exists — it simply never gets a chance to matter, because visibility rejects the reference first. Without a marker there is literally no way to write a native definition another file may legally reference, which is why every cross-file native reference raises E087 today. The private-by-default half is also right for a reason worth preserving: `modules-spec` §4 rules that *"declaring a module is the single deliberate gesture that opts into encapsulation; casual ink stays open."* Native modules derive identity from their file path and are therefore always declared, so private-by-default is that ruling applied consistently rather than an accident — native is the dialect where the author has already opted into structure. `pub` is confirmed free: zero occurrences as an identifier across all in-tree `.brink` sources.
- **FORCING CASE:** the stdlib delegation path needs this. For a project's own conventions module to call `std::conventions::screenplay::heading`, that `fn` must be public — so the built-in preset cannot ship its delegation story without this marker. #2080 (stdlib into the `Environment` manifest) and #2167 (E169 confinement vs delegation) both wait on it.
- **OWED (small, but do not guess):** (1) **ordering against annotations** — `@[element(…)]` then `pub fn`, mirroring Rust's `#[derive(…)] pub struct`, is the obvious shape but should be pinned in the grammar and the book. (2) **which forms accept `pub`** — the seven declaration kinds above; knots/stitches are ink-dialect constructs and do not take it. (3) `pub` becomes a **reserved word** in native; harmless today (zero in-tree uses) but it is a grammar-level break worth naming in a changeset.

## LSP native project extent: one project per `brink.toml` (#1580)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** editor — brink-lsp project extent × native module identity (#1576, #1572)
- **SCOPE:** moderate
- **WHAT:** **The LSP discovers every governing `brink.toml` in a workspace and maintains one `ProjectDb` per project**, applying the existing `brink_driver::native_source_root` rule *per project* rather than once to `roots.first()`. **Editor extent then equals compile extent**, so navigation, rename, and diagnostics mean exactly what the build means. **Rejected: re-rooting a single `ProjectDb` to the active file's project** — smaller, and correct for the common single-project workspace, but cross-project navigation rebuilds the DB on every focus change and a workspace-wide symbol search can only ever see one project. **Rejected: namespacing module keys by root while keeping one DB** — the cheapest fix for the collision specifically, but editor module identity would then differ from compile identity, so navigation and rename would reason about identities the build never produces. That is the same divergence class as `brink_ir::DialogueDialect`, where the editor classified lines by one rule set and the compiler by another; we are deleting that, not adding another instance.
- **WHY:** This is a **correctness** bug, not tidiness. `DefinitionId` is `hash_qualified_name` over the module path plus name, and native module identity is **root-relative** (#1576). So two files at the same root-relative path in different workspace roots — `game/main.brink` and `demo/main.brink` — produce the same module path, the same qualified name, and therefore **the same `DefinitionId`**. In an editor that means goto-definition and rename can target the wrong file. The framing on the issue slightly undersells what is wrong: the LSP is not missing the compiler's rule — `backend.rs` says outright that it *"mirrors the compiler's own `brink_driver::native_source_root` rule"* and calls it at `:531`. It applies that rule **once**, to `roots.first()`, and stops. The fix is to apply a rule it already implements per project. ⚠ **This becomes reachable rather than latent once `pub` lands (#1582):** cross-file native references are illegal today (every one raises E087), so a merged editor project mostly cannot resolve across the files it wrongly merged. With a visibility marker, a fixture under `tests/` and a story file under `story/` sharing one editor project genuinely resolve into each other — a project the real build never forms.
- **OWED (name it, do not guess silently):** **where a `.brink` file under no `brink.toml` lives.** Lean: **each orphan file is its own single-file project**, which gives a scratch file hover and diagnostics without merging it into a real project, and matches the `modules-spec` §4 posture that casual, undeclared source stays open rather than being forced into someone else's encapsulation. Not ruled — the implementer must surface the choice rather than pick it in passing.

## E169 confines `@[convention]` to the conventions module AND its import closure (#2167); the classification walk is Rust-only (#2112)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** prose dialect — conventions confinement (#1844/#2167) × editor classification (#2112)
- **SCOPE:** moderate
- **WHAT:** **(1) `@[convention]` is legal in the `brink.toml`-named conventions module and in anything that module imports; illegal elsewhere (E169).** This replaces #1844's "legal only in the named module" rule, which made a conventions module imported as a *library* into a violation of itself. Rejected: **splitting the preset** into plain `pub fn` bodies plus a thin annotated wrapper (no language change, but the preset becomes two files where one is nearly empty, and every third-party conventions library must repeat the split to be reusable); and **"inert outside"**, ignoring stray annotations rather than diagnosing them (simplest rule, but it discards E169's entire reason for existing). **(2) The classification walk runs in RUST ONLY**, exposed to the TS editor through wasm; the editor never runs the patterns itself.
- **WHY:** (1) is nearly free because **the import closure already has to be computed** — #2111's projection query is specified to invalidate on "the conventions module *and its import closure*", so E169 reuses a set that exists regardless. It also keeps the diagnostic aimed at the mistake it was built for: a `@[convention]` in a story file, where the author expects lines to be claimed and they silently will not be, is exactly the silent-no-op class CLAUDE.md forbids — and "inert outside" would have *re-created* that bug in the name of simplifying the rule. The case E169 was wrongly catching (a conventions library being reused) stops firing. The residual oddity — an imported library's conventions are legal but **not installed** — is already the ruled model: there is no implicit inclusion, so an author importing a conventions library already knows its annotations advertise what is on offer rather than installing it. (2) follows the maintainer's standing objection to implementing one thing in multiple places: a TS-side walk means two regex engines that must agree exactly, forever, on every pattern an author writes. One implementation keeps the serialized projection **dumb data** rather than a specification two engines must interpret identically. The per-keystroke wasm call is affordable because the already-ruled memoization is keyed on `(line text, projection revision)` — editing one line reclassifies one line.
- **CONSEQUENCE:** §9.1's owed **"portable-regex validation"** pass is **demoted, not deleted.** With only Rust running the patterns, nothing stops an author using Rust-specific regex features, and any future non-Rust consumer of the projection would then break. It becomes a forward-compatibility guard rather than a correctness requirement — worth keeping on the books with that reasoning attached, so it is not later dropped as "solved."

## The stdlib reaches the compiler through `Environment`'s manifest; `[project] elements` renames to `conventions`
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** compiler architecture — the `Environment` seam (#1306) × the `std::` namespace (#2080)
- **SCOPE:** moderate
- **WHAT:** **(1) Built-in stdlib source enters compilation as ordinary `Environment` manifest entries, not through a second file space or a bespoke preset registry.** `Environment` (#1306, `crates/internal/brink-environment/`) is already the designed and built seam for exactly this: its own doc rules that *"`the-tree-is-the-universe` generalizes to `the-environment-is-the-universe`: `{ local module tree } + { resolved external module set }`"*. Sources are addressed by **string key** (root-relative, forward-slash) into a hash-addressed manifest, and **native module identity derives from those keys** — so `std/conventions/screenplay.brink` yields a `std::conventions::screenplay` identity by the same rule every project file already follows, with no special-casing downstream. `brink_environment::compile` materializes the set with `db.set_file(key, text)` per key; adding stdlib entries at the **producer** (`Environment::load`) is therefore the whole mechanism. ⚠ **This supersedes the FileId framing the maintainer was asked to rule on** — whether the preset "joins discovery" and shifts FileIds is not a question at this layer, because the Environment addresses by key and FileIds are downstream of the manifest. That framing was the agent's error, corrected before it was ruled. **Stdlib does NOT go in `resolved_deps`** — that field is explicitly reserved for #1093's *compiled per-module artifacts*, and its `ResolvedDep` is a placeholder whose shape is defined when #1093 is built; the stdlib ships as **source**, so the manifest is its home. **(2) `[project] elements` is RENAMED to `[project] conventions`.** The key locates the module of `@[convention]` declarations, and under the same-day annotation-split ruling `element` now means the other thing (`!name` dispatch). The old name is a leftover from before the split.
- **WHY:** (1) is mechanism-over-bespoke in its strongest form: the seam was designed, ruled, built, and is already the sole production road into compilation (`brink-cli`'s `main.rs` and `brink-web`'s `compile.rs` both call `brink_environment::compile`; every remaining closure-based `brink_compiler::compile*` caller is a test). Inventing a preset registry or a second FileId space beside it would add exactly the parallel-universe surface the `external_conventions` injection seam is being deleted for — a join with nowhere for bugs to be caught. Keying stdlib the same way project files are keyed means confinement, claiming, analysis, and identity all work unmodified. (2) is a plain naming correction with a real cost of leaving it alone: a config key named `elements` that must point at a file full of `@[convention]` declarations is the kind of drift that teaches every future reader the wrong model, and it is cheapest to fix now, before anything ships depending on the name.
- **OWED:** the rename is a **breaking config-surface change** — it needs a deprecation path (accept `elements` with a warning, or hard-break) and a sweep of every reader, including #1844's confinement check, `brink-project-config`'s parser, the book, and `docs/prose-dialect-spec.md` §3.4.

## Conventions × the editor: the projection is the only interchange, and the effect system fences handlers
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** prose dialect × editor seam (#2111–#2115) × effects
- **SCOPE:** architectural
- **WHAT:**
  **(1) The attachment schema is a STRUCT — there is no new declarative sub-language.** A convention declares the keys it attaches by naming an ordinary `struct` (sketch: `attach = Cue`), and the handler body computes the values imperatively. This resolves the tension between "the editor must interact with this" and "we must not invent a declarative DSL implemented multiple times in multiple places": a `struct` is already declarative, already statically known, already serialized, and already something the compiler, the editor, and any host binding must understand.
  **(2) The governing split is SCHEMA vs VALUE, not declarative vs imperative.** DECLARED, in the projection, editor-readable: the `claims` pattern, `order`, mode (attach/wrap), the resulting `kind`, and **which keys are attached and their types**. COMPUTED, in the handler body, compiler-only: emitted text, normalized values, side effects. ⚠ **A handler may never attach a computed KEY NAME** — the moment key names are dynamic the projection stops being statically knowable and both the editor and the load-time host-binding check lose their ground.
  **(3) The serialized conventions projection is the SOLE editor interchange.** The editor reads data and never executes brink code, preserving §9.1's requirement that classification stay "a mechanical walk" and that explain-match not be answerable only by tracing execution. Two consumers, one artifact: the **editor** reads it as a salsa-cached query (#2111, recomputed only when the conventions module and its import closure change), the **host** reads the same projection from `.inkb` at load for the binding join.
  **(4) `brink_ir::DialogueDialect` (#368) IS the dissolved declarative table and comes out.** Its `elements: Vec<DialectElement>` (in precedence order) plus `chain: Vec<ChainRule>` is precisely the JSON table §9.1 replaced with `@[convention]` modules — that ruling names chains as dissolved explicitly. **Surviving:** its `transitions` and `templates` fields, already marked "editor-overlay — never travels beyond tooling", are genuine Tab/Enter succession affordances the language should NOT express (#2115); they re-key off convention kinds from the projection instead of carrying their own element list. **The language says what a line IS; the editor overlay says what pressing Tab DOES.**
  **(5) The editor sees raw CAPTURES but not computed VALUES.** It ran the regex, so `name = "VENDOR"` is available; it did not run the handler, so `voiceover = true` is not. Sufficient for classification, explain-match, styling and gutters — which is why the design works without comptime.
  **(6) A convention handler MAY call pure fns and commands; it may NEVER read world state.** `bevy-brink` already distinguishes the three binding kinds, and `bind_brink_query` (world access) is already the special one that cannot run inline while the VM steps. At comptime a command stubs to a no-op — nothing is listening during a build — so `heading` calling `scene_entered` stays evaluable. **Enforced by existing machinery, not a bespoke checker:** the capability manifest already declares `{reads, writes, detect}` per binding and `compute_container_access` already joins effect rows against it; the rule is that a convention handler's joined access must contain **no reads**. ⚠ **This restriction stands on its own merits, independent of comptime**: if classification depended on game state, the editor could never display it (no world), the projection could never be cached (invalidated by arbitrary gameplay), and explain-match would depend on a save file. It makes a class of incoherence unrepresentable rather than merely discouraged, and is enforceable **now**.
  **(7) Comptime's real motivating case is RECORDED, and the mechanism is deliberately NOT decided here.** Wanting the editor to display *computed* attachment values (and to check a handler's output against its declared schema) is a genuine reason to evaluate handler calls at build time: for a literal claimed line every argument is constant, so the call is evaluable, memoizable on `(handler, args)`, and degrades to raw captures on failure. Rule (6) is what makes it safe. This is a far better motivation than the one comptime was originally attached to — computing an ordering, which a bare integer now does. **Nothing depends on it**: the projection, classification, and the host binding all work with raw captures, so the mechanism question is deferred to whenever the editor side is actually built.
- **WHY:** The maintainer named the real tension directly — wanting the editor to interact with conventions while refusing to invent a declarative DSL that must be implemented in Rust and TS and kept in agreement. Splitting on schema-vs-value dissolves it: the only thing crossing the interchange is a flat table of patterns, orders, and struct schemas, which is *data*, not a language, so handlers stay fully expressive at zero cost to the editor. Reusing `struct` as the schema carrier is the same mechanism-over-bespoke move that killed the JSON table in the first place. (6) is the effect system finally being used for what it was built for: the world-read restriction is expressible as a predicate over a join that already exists, so the "capability fence is the effect row" principle — which failed its own canonical example for `register` and had to be corrected in Q4 — turns out to be exactly right one layer over, for handlers rather than for registration.
- **BACKPORTED, NOT RE-FILED:** this design goes into the issues that will build it (#2164, #2108, #2111, #2112, #2113, #2115, #2165) rather than into new tracking issues — maintainer's instruction, to avoid half-building two versions of the same thing.

## The element output model: attachment is block-level metadata, delivery is per-line
- **⚠ RELATIONSHIP TO THE EXISTING `attach = StructName` ENTRY:** a narrower entry recording ONLY item 2 (the declared-schema clause) was transcribed by a build agent while this one was stranded — it is titled "The element output model: `attach = StructName` is the declared handler schema…" and is NOT superseded. This entry is the propagation MECHANISM that entry's own body says is tracked elsewhere. Read both.
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** prose dialect — the element/output seam (#2108, #1683) × `bevy-brink` consumer API
- **SCOPE:** architectural
- **WHAT:** Settled by designing the consumer API first (house principle: *understand the consumer*), against a real screenplay and checked against Fountain.
  **(1) Emitted text and attached metadata are INDEPENDENT axes.** A convention handler separately decides what text it emits (`OutputLine.text` — a line, glued, or nothing) and what key/values it attaches. `speaker` is metadata; whether `VENDOR` appears as visible text is a *format* choice a screenplay preset makes and a speech-bubble game's conventions would not. A consumer binding reads metadata, so it is unaffected by any rendering choice.
  **(2) An attaching convention CONSUMES its own line** and attaches its captures to the run that follows; the cue and parenthetical lines cease to exist as content and become metadata on the dialogue.
  **(3) Attachment ACCUMULATES onto the following run — it does not inherit down a nesting tree.** `cue` and `parenthetical` both attach to the same run; neither wraps the other. There is therefore no scope-reasoning burden on the author and no ancestry stack on the line.
  **(4) Attachment is BLOCK-LEVEL, and the run IS the block.** `BlockId` already means *"every run of same-element adjacent content lines carries one"*, and its own doc names compile-time-baked ids as *"a superset of this"* — this is that superset. Data is stored ONCE per block; "same run, same data" is structural rather than an enforced rule.
  **(5) Delivery is N EVENTS, ONE PER LINE**, each carrying a COPY of its block's metadata. Decisive reason: a block is not just lines — executable statements interleave with them (`~ change_music('dramatic')` mid-speech), so an atomic block delivery would either delay those side effects or run them out of order with the text. The per-event copy is duplication in *delivery*, not in the data model, and it is what lets an observer read `trigger.speaker` directly instead of resolving a `block_id` against a side table.
  **(6) TWO MODES, both justified.** **attach** — consume my line, annotate the following run, never receive it (cue, parenthetical, character extension). **wrap** — receive the following run as `content` and decide its emission (Fountain's title page: a region of key-values consumed as a unit and turned into document metadata). Fountain supplies a concrete case for each, so `block`/wrap is retained rather than dissolved into attach.
  **(7) Captures and attachments share ONE key/value space on the line, carrying `Value` (not `String`).** A consumer declares what it needs by name and never has to know whether a key came from a capture on this line or an attachment from its block. `Value` follows the `BrinkCommand` precedent (commands already take `Value`) and avoids round-tripping a captured `int` through a string.
  **(8) The consumer API mirrors `BrinkCommand` exactly:** `#[derive(BrinkElement)]` on a struct whose fields map to the key/value space by name, registered with `bind_brink_element::<M, T>("cue")`. A game that binds nothing still works — lines flow through with their `kind` set and no typed event.
  **(9) Tags and element data are ORTHOGONAL axes, and both survive on a claimed line.** Element data is *systematic* — schema'd by the handler signature, every cue has a `name`. Tags are *ad-hoc* — per-line, author-written at the call site. `INT. MARKET SQUARE # establishing` classifies as a heading AND keeps `establishing`. ⚠ Handlers must NOT emit tags as a metadata side channel; that would give two ways to say one thing, and element data is the typed one.
- **WHY:** The design was reached by asking what a Bevy author writes, not what the runtime finds convenient. The earlier proposal — a compiler-imposed ancestry stack on every line — failed that test twice: it baked a screenplay-shaped concept into the general mechanism (a comics project inherits a "speaker" model it never wanted), and it ran alongside the handler's existing control of output rather than through it, quietly taking away the attachment the maintainer had originally expressed with glue. Making attachment an *author-chosen* set of keys keeps the runtime ignorant of what a speaker is, which is the property that lets one mechanism serve screenplay, comics, and chat-log formats. Separating emitted text from attached metadata dissolves an entire class of question — the maintainer's correction, that speaker is metadata and never printed content, invalidated a three-option ruling that had been drafted around a coupling that did not exist. Block-level storage was chosen because the maintainer's own framing (*"runs of multiple lines of dialogue being a single block… we want those runs to have the same data"*) describes `BlockId`'s existing semantics exactly; adopting it means the invariant is structural rather than maintained. Fountain was used as the falsification test rather than the screenplay preset alone, because the preset is ours and agrees with us by construction.
- **ICEBOXED (not a gap to close now):** **position/region-dependent claiming** — Fountain's title page is `Title:`/`Credit:` key-values *only before the first blank line*, and elsewhere those are action. Not expressible today: claims are pure per-line regex with no notion of document position. Ruled valuable eventually, deferred deliberately: **line-level claiming is by far the most useful surface and is what the design leans on for now.**
- **NOTED, UNSETTLED:** (a) **one concept, two spellings** — Fountain has *forced* forms (`.INT`, `@NAME`, `!action`, `>transition`) beside natural ones, and a convention has exactly one `claims` today; two conventions delegating to one `fn` works but burns two `order` slots per pair. (b) **backward reference** — Fountain's dual dialogue (`CHARACTER ^`) renders alongside the *previous* speech, and attachment only goes forward.

## `std::` and libraries are PEER ROOTS of `story::`, not children of it — and bare names resolve in module scope
- **WHEN:** 2026-08-04
- **PROJECT:** brink
- **SYSTEM:** compiler architecture — module namespace topology × name resolution (#2080 mount, #2197/#2216/#2238/#2241/#2242 fallout)
- **SCOPE:** architectural
- **WHAT:** **(1) `story::*` is the universe of what the AUTHOR provided. `std::*` — and every future library — is a TOP-LEVEL PEER of `story`, never inside it.** The module forest has several roots; a project's tree is exactly one of them. This CORRECTS the shipped implementation: `brink_db::modules::native_module_path` unconditionally prefixes every derived module path with the literal `"story"`, so the #2080 stdlib mount produced **`story::std::conventions::screenplay`** — the standard library as a subdirectory of the user's project. Consequently `is_std_module` is a **string-prefix test** (`module == "story::std" || module.starts_with("story::std::")`), duplicated in `brink-analyzer::resolve` and `brink-ir::lir::lower::decls` because the two crates cannot share a helper without a dependency cycle — a fact those functions' own doc comments state plainly. Under peer roots, "is this std?" stops being a string convention and becomes a structural property: a different root.
  **(2) A bare name resolves in the REFERRER'S MODULE SCOPE, not against a project-global name map.** `brink-analyzer` already models this correctly — `resolve::Candidacy` is `InScope` (the referrer's own declared module, or the legacy `module == None` world) / `Imported` (a public definition in a declared module this file imports) / `Other` (no line of sight). **LIR lowering never received it.** `decls::lookup_global` walks a flat `index.by_name` and resolves same-**file**-then-anything-not-std; it has no module concept at all, and `ShapeTable` had the same shape until #2238. The std exclusions are compensating for a missing scope step, not implementing a policy.
  **(3) Activating a module's CONVENTIONS is not importing its NAMES.** A conventions module claims lines everywhere prose appears — that is what the `brink.toml` `conventions` pointer means. It never implied the module's `struct`s and `fn`s enter the project's namespace. Those are two facilities and the flat mount merged them.
- **WHY:** The maintainer's framing is the whole argument: *"we're basically just using Rust's module system and it doesn't have this problem, so why should we?"* In Rust, writing a module containing `Cue` cannot conflict with anything — you must deliberately `use` a conflicting name to create a conflict. brink gets a conflict from mere existence, and the reason is that these two pieces were skipped while everything downstream assumed they were present. `tree-is-universe` was safe for exactly as long as the tree was homogeneous: every file in it was the author's, every bare name had one meaning project-wide, and a flat map returned the same answer scoped resolution would have. The mount introduced files the author did not write, cannot edit, and did not ask for — as **peers in a flat namespace** — and the two resolution implementations immediately disagreed. **The five std gates so far (`lookup_global`, `lookup_unique_by_name`, `ShapeTable`, `ShapeTable::resolve`'s fast path, and `brink-analyzer`'s `declared_shapes`) are five symptoms of one absent mechanism**, and #2241/#2242 would be six and seven. Under peer roots plus module-scoped resolution the entire class **cannot arise**: the preset's `Cue` is `std::conventions::screenplay::Cue`, invisible to a project file that never imports it; the project's own `Cue` resolves in scope; and the preset's conventions still claim prose, because activation travels through the config pointer rather than through name visibility. That is strictly fewer moving parts than the status quo — it deletes gates rather than adding them.
- **CONSEQUENCE — WORK IS RESEQUENCED:** #2241 and #2242 are **symptoms and are held behind this**; fixing them individually adds two more gates that must then be unwound. The two halves are tracked as their own issues: the topology change (`native_module_path` must stop forcing every path under `story`, and `is_std_module`'s string convention must be replaced by root identity) and the lowering change (module-scoped bare-name resolution — either port `Candidacy` into lowering, or answer why lowering re-resolves at all rather than consuming the analyzer's already-scoped `ResolutionMap`).
- **OWED:** whether lowering re-resolving names is load-bearing. If there is a real phase-ordering reason, the honest fix is porting `Candidacy` into lowering; if there is not, the fix is **deleting a duplicate implementation** rather than gating it, and the bug class goes with it. Do not choose by assumption.
- **ADDENDUM 2026-08-04 — `DefinitionId` MAY BE BROKEN to do this.** The topology change alters module paths, and `DefinitionId` is `hash_qualified_name` over module path + name, so every derived id moves. **That is acceptable right now and is not a reason to defer or soften the change.** Maintainer's call, taken deliberately while the format is pre-release and there are no saves in the wild worth preserving. ⚠ **This permission is TIME-BOUNDED and expires at the first release that ships saves** — after that, an id change needs the migration facility (#966) and this addendum must not be cited as precedent. Practical consequences to expect and report rather than be surprised by: existing save files stop loading; compiled `.inkb` artifacts must be regenerated; and any insta snapshot that embeds an id will diff. ⚠ **The oracle ratchet must still hold**: it compares episode content (text, tags, choices, state), not ids, so a pure id renumbering must NOT move `RATCHET_EPISODE_COUNT` — if it does, that is a real behavioral change hiding inside the rename, not an accounting artifact, and it must be reported rather than accepted with `INSTA_UPDATE`.

## CORRECTION: the no-world-reads fence is RULED, but the mechanism named for it does not exist at that layer (#2179)
- **WHEN:** 2026-08-03
- **PROJECT:** brink
- **SYSTEM:** prose dialect — conventions legality (#2164 item 3 / #2179) × effects
- **SCOPE:** moderate (corrects an earlier same-day entry)
- **WHAT:** The 2026-08-03 entry "Conventions × the editor" item (6) ruled that **a convention handler may call pure fns and commands but may never read world state**, and asserted this was *"enforced by existing machinery, not a bespoke checker: the capability manifest already declares `{reads, writes, detect}` per binding and `compute_container_access` already joins effect rows against it."* **That assertion is WRONG and is withdrawn.** The RULE STANDS — it was justified independently of any mechanism, and that justification is untouched. What is retracted is the claim that it is enforceable today by wiring an existing join. Three independent blockers, surfaced by the #2179 investigation and verified directly: **(1) wrong crate, wrong dependency direction** — `compute_container_access` lives in `bevy-brink`, while a convention handler is checked in `brink-ir` at HIR-lowering time; `brink-ir/src/host_manifest.rs`'s own module doc states that `brink-ir` *"is compiler/IDE-only and must never depend on `bevy-brink`'s ECS types, and the reverse edge is equally unwanted"*, and `brink-ir`'s Cargo.toml confirms zero such dependency. **(2) no source spans** — the join consumes already-codegen'd `Program`/`EffectRowEntry`, and `EffectRowEntry` carries no range field, so the diagnostic-points-at-the-offending-call requirement is structurally unmeetable from that data. **(3) needs a runtime registry** — `compute_container_access` takes a `CapabilityRegistry<M>`, a host-registered per-marker ECS registry populated by a *running Bevy app*; `brink compile`/`brink check` have neither an app nor a marker type. The other candidate, `brink_ir::host_manifest::ExternalKind`/`HostManifest`, is the right *layer* (compile-time, IDE-facing) but its own doc says it is *"never consumed by the runtime or by codegen — the manifest only enriches"* tooling, so it cannot carry a legality gate either. **#2179 therefore needs a real design pass on mechanism**, and is not the wire-up-an-existing-join task it was briefed as.
- **WHY:** Recorded as a correction rather than a silent edit because the wrong claim was load-bearing in two places — the decision-log entry and #2164's design-backport comment — and it was briefed to a build agent as settled. The agent **correctly declined** rather than inventing an analysis to fit the instruction, and traced the dead ends with file:line evidence so the eventual design pass does not re-derive them; that decline is the process working, and the briefing was the defect. The general lesson is the one this project keeps relearning: *"the effect system already does this"* is a hypothesis about a specific function at a specific layer, not a property of the effect system in the abstract — it has to be traced before it is asserted, and this entry's author asserted it without tracing. Note the same over-claim shape as the #1867/Q5 contradiction: a mechanism named from what it *ought* to be able to do rather than from reading what it takes as input and where it lives.

## Two rulings: annotations become real references (`RefKind::Type`), and block metadata persists with `next_block_id`
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** name resolution (#2249) + runtime save/resume (#2108)
- **SCOPE:** architectural
- **WHAT:** **(1) #2249 — TM-2 / field / temp TYPE ANNOTATIONS ARE REGISTERED AS REAL REFERENCES (`RefKind::Type`) in `brink-ir::symbols::project`'s walk**, so the four remaining unscoped lowering lookups (`collect_externals`' fallback-fn lookup, `structs.rs`'s nested-field-type lookup, `lookup_address_id`, and the annotations themselves) **consume the analyzer's `ResolutionMap`** exactly as PR #2248's site now does. **Rejected: porting `Candidacy` into lowering** for those sites — self-contained and smaller-blast-radius, but it creates a SECOND scoped-resolution implementation that must agree with the analyzer's forever, which is the precise duplication that produced this entire bug cluster (five separate std gates, because five lookups each re-resolved). ⚠ This deliberately changes `project.rs`'s own recorded choice to treat field `TypeExpr`s as *"a nominal-only grammar, resolved later by a different mechanism"*. **(2) #2108 — BLOCK METADATA PERSISTS, AND `next_block_id` PERSISTS WITH IT.** Element attachment is keyed by `BlockId`; if attachment must survive a save, `BlockId` must too. **Rejected: a separate stable key** (a second identity for one runtime concept plus a mapping between them that can drift), and **rejected: accepting the loss** (a player who saves mid-dialogue and reloads gets the speaker silently dropped — player-visible, with no signal, which is the class this project refuses to ship). ⚠ `Flow::next_block_id`'s doc currently states outright that it is *"Never persisted — a resumed save simply continues numbering from a fresh `0` … never across a save/load boundary."* That contract **must be rewritten, not silently invalidated** — it was load-bearing in several other places' reasoning.
- **WHY:** (1) is the only option that *reduces* the number of resolution implementations rather than adding one, which is the whole point of the cluster #2238/#2246/#2241/#2233/#2245 closed. Two consequences were **inferred and are NOT yet verified**: that a struct name in an annotation becomes navigable (go-to-definition / rename / find-references) once it is a real ref, and that an unresolvable annotation gets a real diagnostic instead of #2240's silent-drop class. Both are plausible and neither is established — an implementer must check and report, and a refutation is a finding rather than a failure. (2) follows from taking the existing guarantee seriously: "ids are only ever compared within one flow's own lifetime" was true **because nothing needed them across a boundary**, and now something does. Working around that with a second key preserves the letter of a promise whose only justification has expired, at the cost of more machinery.
- **⚠ PROCESS NOTE — THIS ENTRY EXISTS BECAUSE ITS ABSENCE BROKE A WAVE.** Both rulings were made interactively and then briefed to build agents **without ever being written down**. A wave-134 agent working #2108 verified the claim, found no such entry in this log and #2108's own thread still reading *"still open … none chosen"*, and **correctly declined under rule 12r** rather than building architecture on an unlanded ruling. That is the fifth instance in one session of a ruling living only outside the repo (see also the 2026-08-04 recovery of three stranded entries). The rule the project already has is right and was not followed: **a ruling that lives only in a conversation does not exist.** Record it before briefing anyone against it.

## #2249 IMPLEMENTED: the `RefKind::Type` ruling above, as built — the fork resolved and both flagged consequences checked
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** compiler architecture — name resolution (issue #2249, the fork the 2026-08-04 "peer roots" entry's OWED clause and #2246's own review left open)
- **SCOPE:** moderate (a fourth/fifth `RefKind` migration in the same family as #2246, not a new architectural direction)
- **WHAT:** Of #2249's four audited "no analyzer-recorded resolution" sites, **two are real instances of the #2246 pattern and two are not, and they are resolved separately:**
  **(1) A struct field's declared type and a `VAR`/`CONST`/`temp` TM-2 type annotation get a new `RefKind::Type`.** `symbols::project`'s walk now registers one for a `TypeExpr::Named` leaf; `resolve::resolve_type_ref` (new) resolves it via the same `lookup_by_name`/`ImportScope` machinery `resolve_struct_ref` uses, `SymbolKind::Struct` only. Lowering (`build_shape_table`'s field loop, `build_struct_shape_data`'s identical loop, `structs::record_global_annotation`, `context::LowerCtx::record_temp_annotation`) consumes it directly; `ShapeTable::resolve` — the `brink-ir`-side primitive this replaces, no production caller left — is deleted, not merely gated off. **Deliberately no diagnostic on a miss** (unlike `RefKind::Struct`'s `E068`): a `TypeExpr::Named` leaf is not always a struct reference — `int`/`float`/`List`/… are equally legal and were never meant to resolve here, so "no declared struct named this" is the common, legal case. This was verified empirically, not assumed: `brink-analyzer/tests/proptest_resolve.rs`'s existing `completeness` property (500 generated cases, "every unresolved ref resolves or is diagnosed, no ref silently dropped") broke immediately when `RefKind::Type` was added to its generator with a diagnosing arm, because a generated scalar-keyword path has no declared struct to resolve to and, under a diagnosing design, would have flagged as an error — `RefKind::Type` is the first `RefKind` this genuinely-tested completeness invariant does not hold for, and the property test itself now says so in its own doc rather than silently excluding it.
  **(2) `collect_externals`'s fallback-`fn` lookup and `context::LowerCtx::lookup_address_id` are a genuinely different shape of gap and are NOT ported.** Both are self-declaration lookups — an `extern foo`'s same-named `fn` fallback and a locally-declared label's own address — inferred by the compiler from two declarations' matching identity, never resolved from a path the *user* wrote at that exact call site the way a divert target, a variable read, or a type annotation is. There is no HIR reference to hang a `RefKind` on at either site; inventing a synthetic one with no real source span would itself be the "second scoped-resolution implementation" this fork exists to avoid. Both remain on `decls::lookup_global`, already correctly std-excluded (a property that predates and is independent of this issue), unchanged.
  **(3) The two "inferred, not verified" consequences #2249's own body flagged were checked, not assumed:** (a) **navigability** — YES, confirmed by tracing `brink-ide::navigation::find_def_at_offset`, which iterates `AnalysisResult.resolutions` generically by `(file, range)` with no `RefKind` filter, so any `ResolvedRef` — including a new `RefKind::Type` one — is goto-def-reachable for free, no `brink-ide` changes needed. (b) **a real diagnostic on an unresolvable annotation** — NO, deliberately not added, per (1) above; it does **not** subsume any part of issue #2240 (a different failure — #2240 is `build_shape_table`'s own self-declaration lookup silently dropping a struct when every surviving same-name candidate is std-declared, unrelated to whether a *reference* to that struct resolves).
- **WHY:** The point of this whole cluster, stated in #2246's own PR and repeated in #2249's filing, is that five independent std gates existed because five lookups each re-derived the answer instead of consuming the analyzer's one already-scoped `ResolutionMap` — "reduce implementations, don't grow a sixth." Porting `RefKind::Type` for the two sites that structurally match (a nominal name the analyzer can resolve with full `Candidacy` semantics) does that; forcing the same treatment onto `collect_externals`/`lookup_address_id`, which resolve something the user never wrote a reference to at all, would not reduce anything — it would fabricate a reference to justify the pattern. Declining to diagnose a `RefKind::Type` miss is not a loophole left for convenience: it is the only choice consistent with the grammar, proven by the fact that the alternative (diagnose every miss) breaks an existing, unrelated, already-passing property test the moment it is tried — the clearest evidence available that "unresolved is not synonymous with invalid" for this one reference kind.
- **STILL OWED (named, not fixed here):** `annotations::check`'s `E061` unrecognized-type-name diagnostic (`annotations::declared_struct_names`) remains project-flat — a std-only struct name is "recognized" for `E061` regardless of whether the referrer imports it, so a `~ temp c: Cue` naming an unresolvable, unimported std-only `Cue` still raises no diagnostic anywhere, exactly the gap #2249's "compounding diagnostic gap" section named and left open. `brink-analyzer`'s `declared_shapes`/`declared_struct_names` becoming referrer-scoped is the natural fix; not attempted here, and not blocking this landing.

## The conventions projection is NOT wired into `.inkb` until a consumer needs it (#2237)
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** prose dialect — the conventions projection's host half (#2111/#2212, #2237) × `brink-format`
- **SCOPE:** moderate
- **WHAT:** **`ConventionsProjection` gets no `.inkb` `SectionKind` tag and no `StoryData` field yet.** #2237 is closed as PREMATURE, to be reopened when a real consumer needs it — concretely, when #2108's host-binding half (`bind_brink_element` / the load-time join) is built. **What already exists and is NOT in question:** `brink-format`'s `conventions.rs` carries a complete, round-trip-tested wire codec, and its `ConventionAttachDef::Resolved { name, fields }` carries **fully resolved fields with types**, not a bare struct name — so a host reading `.inkb` would be self-sufficient without any struct declarations to resolve against. **What is missing is a PIPELINE, not a format:** a one-shot `brink compile` never goes through `brink-db`'s salsa layer, so it has no way to obtain a projection value to hand codegen, and #2259 deleted `brink_analyzer::conventions_registry` — the whole-project join #2237's own body assumed would produce one. **Rejected: building a second producer in `brink-compiler`** (a compile-time walk of the conventions module's HIR) — it would unblock a section tag immediately but creates two producers of one value that must agree forever, structurally the same duplication that produced the five-lookup std-collision cluster waves 129–134 spent six waves closing. **Rejected: routing one-shot compile through `brink-db`** — exactly one producer, but it changes a deliberate architectural boundary (`compile(&Environment)` is documented as non-incremental by design) for one feature's benefit.
- **WHY:** There are **zero production readers**: the only `write_conventions_projection` call sites in the tree are round-trip tests. Allocating a wire section tag and a `StoryData` field to serve no reader locks in a pipeline shape before the consumer that would constrain it exists — and the consumer is what defines the requirement. The deletion of `conventions_registry` (#2259) widened the design space rather than narrowing it, so deciding now would be deciding with *less* information than a later attempt will have. This is the same judgment the project applied to `Element` in reverse: that field shipped as a carrier before anything populated it, and the resulting "landed but inert" state persisted for weeks and misled several briefings, including the author's. Better to leave the codec sitting complete and unwired, with its absence explicit, than to ship a half-pipeline that reads as done.
- **⚠ CORRECTION TO A BRIEFING THIS ENTRY SUPERSEDES:** wave 135's brief for #2237 asserted the projection "landed the attachment schema as a NAME, not resolved fields" and asked whether resolved fields were a prerequisite. That was **stale** — PR #2212 had already changed it to a resolved `ConventionAttachSchema`. The build agent verified against the code and corrected it rather than inheriting the claim. Verified again here directly: `ConventionAttachDef::Resolved { name, fields: Vec<ConventionAttachFieldDef> }` with per-field `ty`.

## Succession is EDITOR-OWNED and externally defined; the Rust side validates only, and it must never reach `.inkb`
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** prose dialect — editor succession (#2115, #2270) × `brink-format`
- **SCOPE:** architectural
- **WHAT:** **(1) Succession rules (`transitions`, `templates` — the Tab/Enter/Shift-Tab behaviour) are an EDITOR feature, defined EXTERNALLY, and the Rust tooling never sources them.** The editor owns the data and decides its own storage; it hands the rows in when it wants them checked. **(2) The Rust side's ONLY job is validation and re-keying** — `ConventionsProjection::with_succession` re-keys the rows against the projection's *real* convention kinds so a rule naming a kind no convention declares fails loudly. That is a genuine service the editor cannot perform for itself, because it requires the compiler's view of which kinds exist; it is also the one thing #2115 built that survives this ruling. **(3) `transitions`/`templates` MUST NOT be carried in the `.inkb` wire format.** `ConventionsProjectionDef`'s fields for them come out. **(4) #2270 is DISSOLVED, not deferred** — it asked how Rust should *source* a `DialogueDialect`, and the answer is that it never does. The existing externally-supplied path (`set_dialect`-shaped) is already the right architecture and is already built.
- **WHY:** The maintainer's framing settled it: *"this is an editor feature, not a compiler feature… the rust tooling should almost just assume it's defined externally in all cases."* Reading the code against that exposed a direct contradiction already in the tree: `brink-ir/src/dialect.rs` documents these fields as *"Editor-overlay … **never travels beyond tooling**"*, while `brink-format/src/conventions.rs` carries them in `ConventionsProjectionDef` — and `.inkb` **is** beyond tooling; it is the compiled artifact a game host loads. A runtime has no Tab key. Two docs on the same fields disagreed, and the wire mirror was the one that was wrong. #2237's same-day deferral is what makes this cheap: no `SectionKind` tag and no `StoryData` field were ever allocated, so removing the fields now costs a type edit, whereas removing them after a section tag ships would be a format change. The deeper lesson is the one #2270 embodies — the issue asked a well-formed question about the wrong layer, and answering it as asked would have built a config surface for data the compiler should never have held.
- **OWED (code, not docs):** strip `transitions`/`templates` from `brink_format::ConventionsProjectionDef` and its codec, keeping `ConventionsProjection::with_succession`'s validation and re-keying intact on the `brink-ir` side. Partial undo of #2115's transport half; its validator half stands.

## The compiler-first hold on the native editor track (#1131 / NS-T) is LIFTED
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** editor — native surface (#1131 NS-T charter)
- **SCOPE:** architectural (sequencing)
- **WHAT:** **The 2026-08-01 "compiler work comes first" hold on the native editor track is lifted.** #1131's status banner recorded that hold as *deliberate sequencing, not staleness* — that framing was correct and is now superseded rather than corrected. Native editor work (semantic tokens, folding, completion, inlay hints, code actions for `.brink`, plus registering `.brink` in a client) may proceed in parallel with remaining compiler work. ⚠ **The hold is lifted for a REASON, not because the compiler side is finished** — it explicitly is not: #2108's persistence half, #2113, #2277, #2134 and the remaining NS-T seams are all open. Do not read this entry as "the compiler track is done."
- **WHY:** The maintainer's reason is the ruling: *"this is necessary so i can just see some of it."* The analysis half of native editor support has been built for weeks and **reaches no user** — #1131's own banner says so plainly: hover, navigation, rename, signature, effects, story_graph and hir_projection are all already dialect-generic and work on `.brink`, and `brink-web` has a passing test proving cross-file native hover renders — but `.brink` is registered in no client, so none of it is visible. What is genuinely absent is the **CST-presentation half**, because `brink-ide::semantic_tokens` takes ink's `SyntaxNode` and `classify_token` walks ink's `SyntaxKind`, while native parses to `brink_syntax_native` — a different CST type entirely. Work that cannot be seen cannot be validated, and this project has now repeatedly shipped things that were "landed but inert" — `Element` reported `NARRATIVE` for weeks, the harvest index (#2114) still has no TS consumer at all, and the conventions projection's only reader is its own test. Each was found by reading code rather than by using the product. A visible editor is the feedback loop that would have caught those in an afternoon. The original sequencing rationale (don't classify tokens against a prose surface that will shift) was already overtaken when that surface landed 2026-07-28; what remained was ordering preference, and the cost of that ordering has now exceeded its benefit.
- **CONSEQUENCE:** #1131's status banner must be rewritten — it currently instructs readers that the hold stands and warns that an earlier review misread it as stale. Left unedited it will cause exactly the inverted mistake in the other direction.

## Conventions are PROJECT-WIDE by definition; file-local claiming is a defect, and declaring one outside the configured module is an error
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** prose dialect — conventions/elements (§9.1 item 4) × `brink-db` confinement (#1844/E169)
- **SCOPE:** architectural
- **WHAT:** **(1) A conventions module's claiming applies to the WHOLE PROJECT. Today it applies only within the declaring file, and that is a DEFECT — not a staged limitation to be lifted later.** Maintainer's framing: *"it's never file local. you configure conventions for a project, that's why they're conventions and not 'local patterns' or something."* Anything in the same project as the `brink.toml`-named module is subject to that module's conventions. **(2) Declaring `@[convention]` outside the configured module is an ERROR, and it is an error even when NO conventions module is configured** — in that case there is no module for the declaration to belong to, so the declaration is incoherent on its face. This CORRECTS the current silence: `conventions_confinement_diagnostics_query` treats an unset `conventions` key as one of two cases to pass over silently, on the reasoning (recorded in `brink_analyzer::conventions_module_diagnostics`' own doc) that *"nothing is being confined to yet, so a project that hasn't opted in stays exactly as permissive as it always was."* That reasoning does not survive (1): conventions are project configuration, so a `@[convention]` with no configured module is a misconfiguration, not an opt-out.
- **WHY:** §9.1 item 4 ruled that *"pattern-claiming is confined to ONE module — the conventions module named in `brink.toml`."* That was implemented as a **restriction on declaration sites** but never as **the source of project-wide claiming**, so the two halves of one rule drifted apart: the compiler enforces where you may *write* a convention while ignoring where it *applies*. The result inverts the design — a conventions module can be declared correctly and still claim nothing outside its own file, which makes it unusable for its entire purpose, since any real project keeps conventions in one module and prose across many files. Discovered by looking rather than reading: with the native fixture open in the studio, `VENDOR` in `story.brink` renders as a claimed cue while `KID` in `market/barter.brink` renders as plain prose — the same construct, claimed in one file and not the other. `tests/tier1-native/conventions-screenplay-preset` cannot show this because it is a single file by construction, which is also why the gap survived a passing corpus. ⚠ The same session demonstrated the second half accidentally: a hand-written fixture declared `@[convention]` handlers in an ordinary story file with **no `conventions` key set at all**, and the compiler accepted it silently.
- **CONSEQUENCE:** this reframes #2167. It is currently filed as a narrow question about E169 confinement colliding with the delegation pattern; the actual statement is that conventions must reach the whole project, and confinement is what makes that coherent rather than a restriction for its own sake. The "cross-file claiming reach" language in #2165's deletion note and in `lower_native/element.rs`'s module doc describes the defect, not the intent.

## Module-qualified divert resolution corrected; the #1592 dual-reading ruling narrowed to qualified-only access
- **WHEN:** 2026-08-05
- **PROJECT:** brink
- **SYSTEM:** brink-analyzer (module resolution) / brink-ir (native `Path` lowering)
- **SCOPE:** moderate (a bug fix that also narrows a standing ruling's implementation, not a new design)
- **WHAT:** **(1)** A module-qualified divert (`-> barter::haggle`, after `use story::market::barter;`) now resolves — the native `Path` lowering was normalizing `::` to `.` (`hir::lower_native::expr::lower_path`), making a module-qualified path indistinguishable from ink's own dotted `knot.stitch` addressing, so it could never match (`unresolved divert target: barter.haggle`, note the dot). Fixed by threading a `crosses_module_wall: bool` bit through `hir::Path`/`UnresolvedRef` from the native AST (`ast::Path::crosses_module_wall`), and a dedicated `resolve::lookup_qualified_divert` that resolves the qualifier against `scope.qualified_modules`. **(2)** The more dangerous flip side: a bare `-> haggle` after only a module-qualified import (no symbol-level or glob import of `haggle` itself) used to *also* resolve, silently — `resolve::classify`'s `Candidacy::Imported` grants access for **either** a qualified-module import or a bare item import, correct for reference kinds whose own syntax can be qualified either way, but wrong for a divert with no qualifier segment in source at all. Fixed with a divert-specific `lookup_knot_bare`/`is_qualified_import_only` gate that excludes a candidate reachable *only* via `qualified_modules` before it can win a bare lookup — mirroring the existing std-reserved-root exclusion's own shape, not a new mechanism. **(3) This narrows the #1592 dual-reading ruling's own downstream documentation and tests, which had drifted into over-reading it.** The 2026-07-27 ruling's actual words ("a trailing segment that resolves to a module licenses **that module**, exactly as Rust's `use` does") were correct and are unchanged; `resolve::import_coverage_for_file`'s doc comment, `docs/modules-spec.md` §2, and `crates/internal/brink-ir/tests/native_use_dual_reading.rs` had all restated it as "the submodule's exports become **bare**-referenceable", which is a different and wrong claim — Rust's `use a::b;` makes `b` nameable as `b::item`, never brings `item` into bare scope. The two dual-reading tests that asserted the wrong reading (`use_naming_a_module_licenses_its_exports_bare`, `parent_importing_its_own_declared_child_submodule_licenses_with_no_e090`) now assert `-> barter::haggle` resolves and bare `-> haggle` does not. **The over-read prose was more widespread than that first pass caught** — a follow-up review found the same "bare"/"bare-visible" over-read repeated at five more sites: four doc comments in `crates/internal/brink-analyzer/src/modules.rs` (the `parent_importing_its_own_declared_child_submodule_licenses_with_no_e090` block's own inline comments, the `dual_reading_both_item_and_submodule_neither_is_suppressed` test doc, and the `parent_module_importing_its_own_declared_submodule_is_not_e090` / `aliased_trailing_segment_resolving_to_a_submodule_is_e129` test docs) and `.changeset/issue-1592-dual-reading-use.md`'s first and third bullets (the changeset ships as user-facing release notes on merge, so it carried the wrong claim furthest). All are now corrected to the qualified-access reading in this same PR.
- **WHY:** The maintainer's own repro and correction (issue #2287) is the direct trigger; tracing the fix's blast radius surfaced the deeper cause — `classify`'s single `Candidacy::Imported` tier conflates "this file may write `module::name`" with "this file may write bare `name`", and the #1592 dual-reading feature happened to populate `qualified_modules` in a way that exercised exactly that conflation for a divert target, which is the one reference kind with no legitimate qualified-vs-bare ambiguity in its own grammar (a divert's source spelling always tells you which form was written). Two existing end-to-end tests (`native_use_dual_reading.rs`) had to change as a direct, provable consequence of the correct fix, not as an incidental rewrite — reverting the divert-specific gate makes both fail again, confirming they were pinning the pre-#2287 (wrong) behavior.
- **CONSEQUENCE:** Any future reference kind that reuses `classify`'s `Imported` tier for a divert-shaped "no qualifier at all in source" lookup must use the same `lookup_knot_bare`-style exclusion, not the flat `lookup_by_name` — the conflation is systemic to `classify`, only closed here for `RefKind::Divert`. `resolve_function`'s parallel single-segment `Knot` lookup (`-> haggle` as a *call*, e.g. `haggle()`) was traced and found to have the identical latent gap but was left unfixed (out of this issue's scope) — flagged as a follow-up on issue #2287 rather than folded into this PR. *(Closed 2026-08-22 by issue #2298's fix — `resolve_function`'s "try knots" step now shares the same exclusion via the kind-generic `lookup_bare_excluding_qualified_only` helper.)*

## Move toward a GitHub merge queue for main
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** process / repo-infra
- **SCOPE:** moderate
- **STATUS:** tentative
- **WHAT:** Adopt a GitHub merge queue for `main`: PRs merge through a queue that validates each against main plus its queued predecessors, replacing bare `--auto` arming as the landing mechanism.
- **WHY:** Three same-day incidents (2026-08-05/06) of two individually-green PRs breaking on contact: #2251/#2217 (rename vs. new caller), #2289 inverting the meaning of `IdeSession`'s `conventions: None` sentinel, and #2231's editor stdlib mount colliding with #1880's fix. CI validates a PR against its branch point, not against concurrent siblings; at wave cadence the interleave rate exceeds what per-PR review can absorb.

## The editor path gets its own CI-enforced acceptance gate
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** editor / test-harness
- **SCOPE:** moderate
- **WHAT:** Build a Rust-level, CI-enforced acceptance test for the editor path: a real multi-file project (the NATIVE_FIXTURE shape — brink.toml, conventions module, cross-file divert, stdlib-colliding names) driven through `EditorSession` with project config applied, asserting zero diagnostics plus positive queries (explain-match entries, completions, cross-file claiming). Wave health monitoring reads this gate alongside the oracle ratchet.
- **WHY:** The oracle ratchet held at 5608 all day (2026-08-05) while the editor visibly regressed (native fixture: 1 → 3 → 7 diagnostics), because the editor track had no measured invariant — autonomous work optimizes what is measured. Browser-based verification proved untrustworthy as a substitute (#2324: the playground never applied brink.toml at all).

## Slug-bearing headings: strip structure, then match
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** prose-dialect / conventions
- **SCOPE:** moderate
- **WHAT:** A scene heading's grammar-parsed `[slug]` and trailing `#tags` are stripped before `@[convention]` pattern matching — the pattern sees only the title text, so preset patterns work unchanged on slugged headings. The slug is delivered as a reserved capture alongside the pattern's own captures, mirroring §8b.5's reserved address-capture role. (Closes the #2077 design question.)
- **WHY:** Every worked heading in the prose-dialect spec carries a slug, so the previous decline-on-slug rule meant no preset could claim any of the spec's own examples. Stripping keeps the pattern about the prose and the structure structural — and keeps #2078 orthogonal: slug = addressability, claim = presentation/metadata.

## Compact cue desugars to cue + content line
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** prose-dialect / conventions
- **SCOPE:** moderate
- **WHAT:** A compact cue (`@NAME: dialogue`) matches its `@[convention]` pattern against the **name segment only**, exactly as if it were a block cue's line; the fused dialogue lowers as an ordinary content line that the attachment applies to (the first line of the attached run). Literalness applies only to the name segment; dialogue keeps full markup/interpolation rights. (Closes the #2079 design question.)
- **WHY:** The rejected alternative (flattening name + separator + dialogue into one run for a two-capture pattern) would decline the whole cue whenever the dialogue carried markup or interpolation, making compact cues second-class next to block cues. The desugar makes compact and block cues the same thing spelled two ways. Feasibility was checked before ruling: `candidate` (element.rs) is already a per-node-kind dispatch that selects which sub-node's text is offered to matching (`CUE` → `CUE_NAME`, `SCENE_HEADING` → `SCENE_TITLE`), so a `COMPACT_CUE` → `CUE_NAME` arm is structurally identical to existing arms — no new matching machinery, no `try_claim` contract change. Implementer must pin with a test that the fused dialogue lands **inside** the attached run rather than being treated as a run boundary.

## LIR optimization stage; reachability prune is its first pass
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** compiler / lir
- **SCOPE:** architectural
- **WHAT:** The pipeline gains a general optimization stage between LIR lowering and codegen (`LIR → passes → LIR`). Its first resident pass is **reachability pruning**: the stdlib mount stays universal and unconditional (analysis uniformity, per the environment-is-the-universe model), but codegen emits only definitions reachable from the artifact's roots — unreferenced stdlib content must not reach `StoryData` (closes the #2228 design question). Until the pass lands, the current pollution is an **accepted, documented interim**, so agents stop bouncing off the issue. The stage's contract — pass ordering, determinism rules, and especially the prune's **root set** (story entry plus host-callable surface; engine→ink `begin_function_eval` means roots are not just the entry point; mounted stdlib symbols are roots only when referenced by project code) — gets a design issue and is specced before implementation.
- **WHY:** Mount-on-demand was already rejected (usage-dependent analysis universes break determinism of the environment). Emission-side pruning preserves both principles at once: the environment stays uniform, the shipped artifact carries only what the project mentions — the same species of ruling as #2277's "editor overlay data must not reach .inkb," applied to library content. A general stage rather than a one-off hook because future whole-program work (constant folding, dead-branch elimination) otherwise has nowhere to live; each pass must be a pure function of the program with no iteration-order dependence.

## Mounted stdlib presents as a read-only library node
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** editor-ui / studio
- **SCOPE:** moderate
- **WHAT:** The studio shows mounted `std/` files as a visually distinct, collapsed, read-only "Library" section in the Binder — browsable, openable read-only, excluded from save-all and search/replace. Goto-def/hover into stdlib lands in the same read-only view. Independently of presentation, the session enforces read-only on mounted file ids at the session level — the current gap where a by-id route (e.g. search/replace) can edit a mounted file and silently fork the stdlib into the user's project is a bug under any policy. (Closes the #2306 product question; #2232's LSP/CLI policy should follow the same read-only + navigable model.)
- **WHY:** Goto-def on an inherited symbol (`Cue`, `heading`) must land somewhere, which forces a presentable read-only form regardless — the only real question was browsability, and the conventions an author inherits (patterns, `order` values, attach structs) are exactly what they need to consult. The environment-is-the-universe model reads naturally as "the universe should be visible." Hiding was an implementation default from #2231's review (phantom-row fix), never a product decision.

## E169 governs story::* only; peer roots are library carriers by position
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** conventions / modules
- **SCOPE:** moderate
- **WHAT:** E169 confinement stays fully strict **within the project's own universe** (`story::*`): an `@[convention]` declaration in any project file outside the `brink.toml`-named conventions module is loud, unchanged. Files under **peer roots** (`std::`, and any future mounted library root) are **library carriers by position**: their annotations activate only when that module is itself the project's named conventions module (e.g. `conventions = "screenplay"`); otherwise they are documentation of what the library offers, not active claims and not violations. Delegation therefore works with no preset restructuring — a project's own module respells handlers whose bodies delegate via plain calls to peer-root functions, annotations on the callee being irrelevant to the call. (Closes the #2167 design question.)
- **WHY:** The user's `brink.toml` governs the user's universe; peer roots are not governed by it (the #2245 peer-root doctrine, carried to its conclusion). The rejected alternatives: splitting every preset into an annotated-module half and a plain-library half (a structure tax on every library, invisible to authors), and "inert outside the module" for project files too (a silent-drop pattern — a misplaced annotation in your own project doing nothing with no signal is exactly what E169 exists to prevent). The positional rule also generalizes cleanly: depending on a third-party conventions library works identically to depending on std.

## No-world-reads fence: analyzer effect-row check; unclassified externals are diagnosed
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** conventions / analyzer
- **SCOPE:** moderate
- **WHAT:** The #2179 fence (a `@[convention]` handler may call pure functions and commands but never read world state) is enforced in **brink-analyzer**: compute the handler's transitive call closure from the existing HIR effect rows, classify externals via `brink_ir::host_manifest::ExternalKind`, and diagnose any reachable `Query`-kind external at the offending call site with a real span. This promotes `ExternalKind` from advisory to **load-bearing**. An external whose kind is unclassified (`Plain`) called from a convention handler is **diagnosed too** — unclassified is unprovable, and the fix is to classify the external (`@kind` doc tag or manifest). The bevy-brink mechanism named in the original backport comment (`compute_container_access`) is confirmed unusable for this (wrong crate/dependency direction, no spans, requires a live registry) — the #2179 decline comment's trace is adopted as the evidentiary record.
- **WHY:** The rule exists because classification must be a pure function of the text — otherwise the projection can't cache, explain-match depends on a save file, and the editor can't display results. A syntactic "no externals at all" fence would ban the commands the ruling explicitly permits; deferral leaves the invariant unprotected. Conservative-on-`Plain` matches the silent-drop stance and only bites inside handler bodies — a new, narrow surface where loudness is cheap.

## Merge-queue probe result: unavailable on user-owned repo; strict up-to-date adopted instead
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** process / repo-infra
- **SCOPE:** moderate
- **WHAT:** The merge-queue direction (previous entry, was tentative) resolved by probe: GitHub's `merge_queue` ruleset rule is rejected (422 `Invalid rule 'merge_queue'`) on this user-owned repo while an identical request with a control rule type succeeds — merge queue remains an organization-repo feature. Adopted instead: **`strict: true` on main's required status checks** (applied 2026-08-06) — every PR must be up-to-date with main before merging, so required checks always run against the true merge base. `ci.yml` keeps the `merge_group` trigger (PR #2337, a no-op without a queue) so an eventual org transfer is turnkey. An org transfer is the only path to a true queue and stays available as a future, separately-planned decision.
- **WHY:** Strict mode closes the exact gap behind the three 2026-08-05/06 green+green→broken incidents: a PR predating a colliding merge goes `BEHIND` and must re-validate against current main instead of merging stale. The failure mode moves from "main silently broken" to "stale PR visibly stalled," which is the right direction. Wave merge trains already merge `origin/main` before their fix pass, so train-landed PRs are naturally current; the cost lands on stragglers, loudly.

## Desktop app v1: Tauri shell, local build first; mobile deferred
- **WHEN:** 2026-08-06
- **PROJECT:** brink
- **SYSTEM:** brink-desktop / studio
- **SCOPE:** architectural
- **WHAT:** Revive ruling-ledger #28 (2026-03-17, "brink-studio standalone app uses Tauri" — never built, no owning issue). **v1 is a local build**: no signing, no notarization, no updater, no release matrix — a macOS dev build; promotion to a distributable is a later, separate stage with its own workflow (never touching cargo-dist's `release.yml`). Wasm backend reaffirmed (one integration path; native-core-over-IPC stays a perf escape hatch requiring evidence). New `docs/desktop-shell-spec.md` owns the design; the `src-tauri` crate is **excluded from the root cargo workspace** (Tauri's dependency graph would join Bevy's in every workspace build for zero coverage benefit). A **mobile client is recorded as an interest and explicitly deferred** — Tauri 2's mobile targets keep it on this same stack and frontend, which is part of why the shell choice stands; nothing may be added *for* mobile before it is scheduled.
- **WHY:** Local-first ships in spike-sized effort with zero distribution overhead, and the embedder architecture has matured to where the shell is genuinely thin: `mountStudio` is a proven embedding seam, `FileProvider` was designed with a Tauri/FS implementation named in its doc, `pushExternalChange` + the #320 conflict surface were built for a real fs watcher, and #2324's `brink.toml` discovery re-runs automatically on config changes. The March ruling's design is now mostly built — the shell is the missing 10%.

## Desktop persistence adopts the celeris overlay model; the machinery is shared in @brink-lang/editor
- **WHEN:** 2026-08-07
- **PROJECT:** brink (+ celeris)
- **SYSTEM:** brink-desktop / @brink-lang/editor
- **SCOPE:** architectural
- **WHAT:** D2 replaces D1's write-through-with-debounce with the model celeris ruled 2026-06-29 (celeris `docs/decision-log.md`, "Narrative lens file model"): **overlay, not write-through** — keystroke-live buffer, dirty = buffer ≠ last canonical save; **autosave is a real save** (same canonical write + dirty-clear; triggers = ⌘S or every-X-min); a **bounded backup ring** in host app-data for crash-restore/rollback, orthogonal to dirty. Wiring insight adopted: **the #154 egress feeds the RING, not canonical files** — crash protection at ~500 ms granularity while canonical writes stay explicit. The generic half (ring policy + sink interface, autosave scheduler, restore) is built ONCE in `@brink-lang/editor` per celeris's own layering ruling ("editor lifts are a single editor epic; the lens consumes"); brink-desktop and celeris both consume it, and celeris's planned greenfield autosave/ring services (designed, never built) are superseded. Working defaults: autosave on at 2 min; ring ≤ 25 entries or 10 MB per project. Note: celeris §10's "brink today" snapshot is partially stale in brink's favor — its editor-epic item B (never-clobber conflict hook + merge view) landed upstream as #320 Track V, so celeris's interim dirty-guard is obsolete.
- **WHY:** Celeris's ruling names the exact defect of D1's expedient model: dirty affordances, autosave, and crash protection are ORTHOGONAL axes that write-through conflates — under it dirty is meaningless, rollback impossible, and the #320 conflict machinery untestable (the egress re-baselines every 500 ms, so a real conflict window barely exists). Two Tauri hosts disagreeing about save semantics would be a permanent maintenance tax; one shared implementation with host-provided sinks serves both.

## Desktop close: no dirty prompt; quit awaits the final save
- **WHEN:** 2026-08-07
- **PROJECT:** brink
- **SYSTEM:** brink-desktop
- **SCOPE:** minor/local
- **WHAT:** No dirty-close confirmation prompt — autosave (2 min) plus save-on-close make it dead UI. Instead, the one real safety piece: a Tauri on-close-requested hook that AWAITS the final `saveAll` before the process exits, closing the quit-mid-save data-loss window (today's close-save is fire-and-forget).
- **WHY:** A prompt that almost never fires with anything at stake is noise; the narrow real hazard is process exit racing an in-flight write.

## External deletion of an open file: keep the view, mark orphaned
- **WHEN:** 2026-08-07
- **PROJECT:** brink
- **SYSTEM:** editor / studio
- **SCOPE:** moderate
- **WHAT:** When a file open in the editor is deleted externally, the session drops the file but the open view KEEPS its buffer, marked "deleted on disk" and dirty; ⌘S recreates the file. Never auto-close the tab.
- **WHY:** The never-clobber principle applied to deletion — an external `rm` must not destroy an open buffer's content; auto-close is the same clobber class #320 exists to prevent.

## [project] entry beats mountStudio's entryFile
- **WHEN:** 2026-08-07
- **PROJECT:** brink
- **SYSTEM:** editor / project-config
- **SCOPE:** moderate
- **WHAT:** When both exist, `brink.toml`'s `[project] entry` wins; the host's `entryFile` argument is the fallback for configless projects. `ProjectSession` owns initial-tab selection; the desktop's regex peek at the TOML dies. (Settles #2331's precedence question; the schema slot follows.)
- **WHY:** Config is authored project truth — hosts should stop duplicating the choice, and every host parsing the config itself to honor `entry` was the alternative.

## Cue/parenthetical tag extensions: strip-then-match, uniformly
- **WHEN:** 2026-08-07
- **PROJECT:** brink
- **SYSTEM:** prose-dialect / conventions
- **SCOPE:** moderate
- **WHAT:** Trailing `#tags` on a cue or parenthetical (`@VENDOR #(v.o.)`) strip before pattern matching, exactly as #2077 ruled for headings — one literalness doctrine across all claimed shapes; tags flow through the existing channel. (Settles #2350; the heading-tags delivery caveat from #2344 — Content.tags as an explicitly interim carrier pending #474 — applies identically here.)
- **WHY:** The asymmetry was an accident of implementation order, not a design choice; uniform stripping keeps preset patterns clean everywhere.

## Oracle conformance is no longer the core metric
- **WHEN:** 2026-08-07
- **PROJECT:** brink
- **SYSTEM:** cross-system / process
- **SCOPE:** architectural
- **WHAT:** Oracle conformance is NO LONGER the project's core metric. The ratchet (5,608) remains enforced as a REGRESSION FLOOR — ink compatibility must not silently degrade, and unexpected movement in either direction is still stop-and-report — but the remaining ~1,000 mismatched episodes are not the priority backlog, and "the gap" is not the measure of progress. The project's center is the NATIVE surface and the authoring experience: the `.brink` language (conventions, prose dialect), the editor packages, and the desktop studio. CLAUDE.md's "Current state" framing rewritten accordingly; pump-wave rules keep the ratchet's enforcement language but drop "core metric" framing.
- **WHY:** The C# oracle only measures the ink-compat subset, which is now a compatibility layer rather than the product; the last month's real progress (conventions, native syntax, editor, desktop) is invisible to that metric, and chasing the residual mismatches has poor return next to native-surface work.

## Quit-time saveAll retries on a 750ms interval, capped by the existing 3s wait
- **WHEN:** 2026-08-14
- **PROJECT:** brink
- **SYSTEM:** brink-desktop
- **SCOPE:** minor/local
- **STATUS:** ⚠ **PROVISIONAL — chosen by the implementation, NOT ruled by the maintainer.** #2434's body explicitly left this open ("Needs a design call on exact shape: dispatch-then-poll-then-redispatch-once-more, or poll-and-redispatch-until-cap"), and the maintainer was unreachable when this landed. The shape below was picked by the PR that implements it. It shipped rather than blocking because it is *constrained by* — not in tension with — the existing #2370 capped-wait ruling. It is recorded here so the choice is visible rather than buried in `quit.ts`, **not** to imply ratification. Surfaced on #2434 for the maintainer's call; if they prefer redispatch-once, or a different interval, this entry and `redispatchIntervalMs` change together. Every other entry in this log is a maintainer ruling — this one is not, and should not be cited as precedent until ratified.
- **WHAT:** `awaitSaveAllBeforeQuit` (`packages/brink-desktop/src/quit.ts`) re-dispatches `file.saveAll` every 750ms (`redispatchIntervalMs`) for as long as `getDirtyFiles()` stays non-empty, still bounded by the existing ~3s `timeoutMs` cap from the #2370 ruling — poll-and-redispatch-until-cap, not a single fixed extra retry. A redispatch is skipped once the remaining budget is smaller than the redispatch interval, so the loop never fires a write it cannot let settle before the cap expires. 750ms was chosen as roughly the shortest interval that comfortably outlasts a normal host write (well under the 3s cap, giving room for several attempts) without redispatching so often that a slow-but-healthy write gets needlessly resubmitted mid-flight.
- **WHY:** #2434: once #2426 made `file.save`/`file.saveAll` correctly leave a mid-write edit dirty after its write settles, a single dispatch-and-poll burns the whole cap on that one dirty path and quits with the edit unwritten (backup ring only). Poll-and-redispatch-until-cap keeps the #2370 safety property (a hung write still cannot make quit hang — the cap is unchanged) while giving a mid-write edit repeated chances to actually land on disk. `TauriFileProvider.requestSave`'s serialization (#2403) is what makes redispatching safe to do freely rather than only as a last resort: an overlapping redispatch queues behind whatever write is already in flight instead of racing it.

## MPL-2.0 admitted for the five transitive Tauri dependencies
- **WHEN:** 2026-08-15
- **PROJECT:** brink
- **SYSTEM:** brink-desktop / CI supply-chain policy
- **SCOPE:** moderate
- **WHAT:** MPL-2.0 is admitted for exactly five crates, via `deny.toml`'s `[licenses] exceptions` (per-crate, not a blanket `allow` entry): `selectors`, `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`. This clears the 5 `error[rejected]` licence findings the `cargo-deny (src-tauri)` step in `.github/workflows/desktop-smoke.yml` reports against `packages/brink-desktop/src-tauri`'s excluded `Cargo.lock` (#2470/#2488). `deny.toml`'s `[licenses]` comment, which claimed the allowlist was "100% permissive, no copyleft obligations", is corrected in the same change — that sentence had been false of the src-tauri graph since the repo took a Tauri dependency, and the config now states the real policy: permissive-only by default, plus MPL-2.0 for these five crates. **The ruling's scope is the licence question and nothing else.** The 16 unmaintained RUSTSEC advisories and the `error[unlicensed]: brink-desktop = 0.1.0` finding are NOT ruled on: they are neither silenced nor allowlisted, `[licenses] private.ignore` stays unset, and the step keeps `continue-on-error: true`. Measured against cargo-deny 0.19.8 — the version the pinned action image actually ships — the audit goes from 22 errors to 17.
- **WHY:** All five arrive transitively through Tauri itself (`selectors`/`cssparser`/`cssparser-macros`/`dtoa-short` via `dom_query` under `tauri-utils` and `wry`; `option-ext` via `dirs-sys` → `dirs` under `tauri`, `tauri-build` and `wry`), and there is no version bump or feature flag that removes them — the alternative to admitting them is dropping Tauri. MPL-2.0 is file-level copyleft: its obligations attach to modifying and distributing the covered files, and this project links those crates rather than modifying them, so no source-disclosure obligation is triggered. Per-crate `exceptions` rather than adding "MPL-2.0" to the blanket `allow` list keeps the admission as narrow as the mechanism permits — a sixth MPL-2.0 crate appearing in either graph still fails the audit and comes back for a ruling.

## @brink-lang/editor gets its own test suite
- **WHEN:** 2026-08-17
- **PROJECT:** brink
- **SYSTEM:** ink-editor / test-harness
- **SCOPE:** minor-to-moderate
- **WHAT:** Maintainer ruling, quoted verbatim: *"editor should have its own test suite yeah."* `packages/ink-editor` gains its own `vitest.config.ts`, `test` script, and `src/__tests__/*` suite (#2559), closing the gap where `@brink-lang/editor`'s only coverage was `packages/brink-studio/src/__tests__/*` reaching in through the studio's alias map — a published package's regressions gated by a *different* published package's suite. Two sub-decisions made by the implementing PR, not separately asked of the maintainer: **(1) ADD, not MIGRATE** — the four-plus existing `brink-studio` tests that already reach `ink-editor` internals stay exactly where they are; a duplicate-covered class is not a defect, only a silently-dropped one is. **(2) No wasm alias in `packages/ink-editor/vitest.config.ts`** — unlike the studio's config, this suite's tests import their subject directly from source (never through `index.ts` or the `@brink-lang/editor` specifier), so it needs no built `crates/brink-web/www/pkg` at test-run time; the workspace install in the same CI job still does (#2479).
- **WHY:** A package's own regressions should fail from that package's own CI step, not silently pass until (or unless) a different package's suite happens to exercise the same code path — the same reasoning behind the editor acceptance gate's standing (2026-08-06 entry above). ADD-not-MIGRATE and no-wasm-alias were both load-bearing choices for #2559 (they shape what the new suite can and cannot import) but narrow enough, and downstream enough of the actual ruling, that the implementing PR made the call rather than blocking on a second round-trip.

## Unified `[` type-annotation diagnostic (#2792): one call site stays exempt
- **WHEN:** 2026-08-17
- **PROJECT:** brink
- **SYSTEM:** brink-syntax-native (native parser, type-annotation grammar)
- **SCOPE:** minor/local
- **STATUS:** ⚠ Implementation-decided during PR review, NOT a fresh maintainer ruling — recorded so the diagnostic surface this PR settled on is visible, per the review finding that flagged its absence here. Surface for confirmation at the next interactive session; the alternative the review also offered (drop the lambda-return half of #2792 entirely and ask for a ruling) was not taken.
- **WHAT:** #2792 ("`[` isn't the type-argument delimiter") unifies the diagnostic across every position that reads a type name — params, `let`/`var`/`const`, struct fields, and lambda params — via a single check in `types::type_name_or_generic` (`reject_bracket_after_type_name`). **One position is deliberately exempt: a lambda's own return annotation** (`|x: int|: T […]`, `expr.rs::lambda_expr`, via `types::lambda_return_type_annotation`), because unlike every other annotation position it is immediately followed by an expression — the lambda body — and `[` legally starts one (the array-literal atom, #1490 — `|x: int|: List<int> [1, 2]` is a fully legal program). The exemption is the outermost type only; a type argument nested inside `<…>` at that same call site (`|x: int|: List<Option[int]> …`) still gets the check, since nothing legally follows a closing `>` there but the check itself. Consequence: `|y: int|: Option[int] { none }` — a bare (non-generic) return type immediately followed by `{` — goes back to the pre-#2792 silent-data-drop behavior (the leftover `[int]` reads as the lambda's `ARRAY_LITERAL` body, dropping the real ` { none }` body, zero diagnostics), pinned as a known, accepted gap by `lambda_return_annotation_square_bracket_mistake_is_a_known_silent_drop_not_a_diagnostic` (`expression.rs`).
- **WHY:** A parser-review pass on #2792 found the unconditional check regressed `|x: int|: List<int> [1, 2]` from zero errors at `main` to a spurious diagnostic with an otherwise-correct tree — the shared check cannot tell "trailing bracket is the retracted `Option[T]` mistake" from "trailing bracket legally starts the next construct" by looking at the type alone; only the calling position knows what follows, and only this one position has an expression, not punctuation, on its other side. Between (a) exempting this one call site, reopening the narrower silent-drop #2792 also fixed there, and (b) rejecting legal array-literal-bodied lambda returns outright, (a) was judged the smaller cost — the false positive would fire on legal code with no workaround, while the silent drop is a real but narrower gap (a non-generic return type is the only shape it touches, and it existed unfixed at this exact position before #2792 shipped).

## Knot-interior anonymous containers get the same unconditional #file: qualifier root content already had (#2229)
- **WHEN:** 2026-08-20
- **PROJECT:** brink
- **SYSTEM:** brink-ir (hir::stamp — container-id stamping; lir::lower — the knot chunk's `IdAllocator` prefix and choice-scope spelling)
- **SCOPE:** moderate — a save-compatibility break, accepted
- **WHAT:** `hir::stamp_container_ids`'s per-knot loop now qualifies a knot's *interior anonymous* (unlabeled choice/gather/conditional-branch/sequence-branch) container-hashing scope with the same `#file:{path}` prefix `root_content_scope_path` already applies to root content (#1504) — closing the fourth M-2d collision site (#2197/#2213/#2215/#2226 fixed the other three: `lookup_container_id`, `lookup_global`, `lookup_label_id`); review of this PR showed it was NOT the last, see the two companions below. Two files legitimately declaring a same-named knot (their `native_module_path`s always differ, so `insert_symbol` lets them coexist per #790) previously stamped every unlabeled descendant container at the same structural position to the identical `DefinitionId`, tripping the #1673 duplicate-id `E060` codegen guard. The qualifier is **unconditional** — applied to every knot-interior anonymous container on every real (path-registered) compile, not gated on whether an actual collision exists this time. The knot's own *label* scope (`knot_path`, used for named-label lookup) stays bare and unqualified, matching `SymbolIndex.by_name`'s unqualified `{knot}.{label}` keying — only the anonymous-container hashing scope changes. **Two review-found companion fixes ship in the same PR** (adversarial review of #2907; both fall inside the same ruled break class, so they ship with the one id shift rather than as a second break later): **(1)** the stamping pass only covers containers HIR stamps — *inline*-sequence wrapper containers (e.g. an alternation inside choice text) are minted at LIR time by the knot chunk's own `IdAllocator` (`LowerCtx::alloc_sequence_id`), which started at the bare knot name, so two same-named knots still collided (`E060`) on that shape after the stamping fix alone; `lower_knot_chunk` now sets the same per-file `set_path_prefix` `lower_root_content_chunks` always did, making the qualifier actually reach every knot-interior anonymous container, stamped or LIR-minted. **(2)** `hir::stamp::stamp_stmt` spelled choice segments bare — `c{n}` — contra `root_content_scope_path`'s documented `c-N`/`g-N`/`b-N`/`s-N`; once knot interiors joined the shared `#file:` namespace, an authored knot legally named `c0` hashed the same scope as a root anonymous choice's subtree, a **single-file regression** (compiles on pre-#2229 `main`, `E060` with the stamping fix alone). Choice segments now spell `c-{n}` at both the stamping sites and LIR's `lower_choice` scope narrowing (kept in lock-step per #1727's parity ruling), so a synthesized segment can never equal a legal authored identifier.
- **WHY:** Gating the qualifier on whether a real collision exists (i.e. only qualify when a sibling file actually declares the same knot name) was considered and rejected: it would make a container's `DefinitionId` — and therefore whether an old save's visit count for it still resolves — depend on the unrelated existence of *other files in the project*, a strictly worse stability property than depending on the file's own path. Unconditional qualification, applied consistently, is simpler and matches #1504's own precedent for root content.
- **CONSEQUENCE — accepted, not a defect:** this unconditionally changes the `DefinitionId` of every knot-interior unlabeled choice/gather/conditional-branch/sequence-branch container — plus, via the two companions, every knot-interior LIR-minted inline-sequence wrapper and (the `c-{n}` respelling) every anonymous choice container *anywhere*, root content and single-file projects included — for every real compile, not only the specific M-2d-colliding pair. `brink-format::save.rs` keys visit counts by `DefinitionId`, so any already-compiled/saved story with an unlabeled knot-interior container (the ordinary case — almost every real story) gets new addresses after this ships, silently detaching that save file's visit-count keys from the recompiled story. **Ruled accepted (Option A)**, consistent with #1504's own precedent (which made the same class of unconditional-qualifier change for root-level anonymous containers): the guarantee being broken — visit counts for *unlabeled, knot-interior* containers surviving a compiler upgrade on unchanged source — is already the weakest stability slice in the system, since these ids are path/position-derived and any nearby source edit re-addresses them anyway; there is no released player base with durable saves today, so the break is at its cheapest now. **What still survives:** named containers (labels, knots, stitches) and all other name-keyed state are unaffected — only *unlabeled* knot-interior containers' addresses move. `LoadReport` (`docs/format-spec.md`) degrades tolerantly on an unresolvable visit-count key rather than failing the load. The save format's durability promise (`format-spec.md`) holds for name-keyed state; it was never a promise about anonymous-container position-derived ids, which this ruling makes explicit.
- **Also landed alongside this fix:** a `tests/tier1-native/` M-2d golden fixture (`m2d-knot-interior-file-qualifier/`) — two files, same-named knot, unlabeled descendant containers, an inline alternation inside choice text (pinning companion 1's LIR-minted wrapper), and a *labeled* choice reusing the same label in both knots (pinning that the bare label scope and the file-qualified anonymous scope coexist) — so a further same-class call site cannot go unfixed silently the way this one did (found twice, in review of #2213 and #2223, before finally getting its own issue). A single-file regression test for companion 2 (`brink-test-harness/tests/issue_2229_synthesized_choice_segment.rs`: root nested choices + an authored knot named `c0`, must compile clean through the real `compile_path` road). A companion fix in `crates/internal/brink-respell/tests/ink_corpus_convert.rs` (harness-only, `brink-test-harness::corpus::explore_from_brink_native_at`/`compile_and_explore_from_brink_native_at`) — the differential's two compare legs were registering file-path qualifiers asymmetrically (a real, root-relative key for the `compile_path`-based ink leg; no path at all for the in-memory `.brink` leg), a pre-existing harness gap this fix's unconditional qualifier was the first change to expose. The oracle ratchet is unaffected: `oracle/*.oracle.json` and `StepRecord` key `visit_changes`/`variable_changes` by name, never by address hash.

## `block` and `attach = StructName` are mutually exclusive on one `@[convention]` handler; combined "wrap AND attach" semantics stay unruled (#2264, `E186`)
- **WHEN:** 2026-08-21
- **PROJECT:** brink
- **SYSTEM:** brink-ir (hir::lower_native::annotation — `parse_convention`; hir::lower_native::element — `try_claim`'s dispatch)
- **SCOPE:** small — closes a silent-drop gap; no existing project that never combined the two clauses is affected
- **WHAT:** A single `@[convention(claims = "…", order = N, block, attach = StructName)]` declaration — both clauses on the same handler, in either order — now diagnoses `E186` and is never registered as a claiming handler (`parse_convention` returns `None`, the same "never a partial `ConventionAnnotation`" posture `E159`/`E166`/`E167`/`E178`/`E180` already take). Before this, `try_claim`'s dispatch (`if is_block { .. } else if is_attach { .. }`) had no exclusivity check upstream: `block` always won the `if`, and `attach` — parsed and stored on `ConventionAnnotation` — was never consulted, with zero author signal that half of what they wrote did nothing.
- **WHY:** Issue #2264 offered two paths — diagnose the co-occurrence as a hard error, or design what "wrap AND attach" would mean together and implement that. This ruling takes the first path only. Defining combined semantics (does the wrapped call's own return value also attach to the run it wraps? to itself?) is a design question with no ruling and no test pinning any answer — house rule 7 (design before implementation) says that question is not decided by default, and house rule 9 (flag silent drops) says the silent-acceptance status quo cannot stand while it's undecided. **Explicitly NOT decided by this entry:** whether `block` and `attach` can ever cooperate on one handler, and if so what it would mean. That question stays open; a future ruling would need to supersede this exclusivity, not just add to it.
- **CONSEQUENCE:** a `.brink` project that previously declared both clauses on one handler (silently getting wrap-mode behavior with `attach` inert) now gets `E186` at analysis time instead, on both the db-direct and off-db analysis roads (shared HIR lowering). No known in-repo project relied on the previously-silent behavior; the ink-compat surface (`.ink`) has no equivalent annotation and is unaffected.
- **Also landed:** `docs/diagnostics/E186.md`; `docs/compiler-spec.md`'s diagnostic table range bump (`E001`–`E185` → `E001`–`E186`); `docs/prose-dialect-spec.md` §3.5b cross-referenced from both the `attach` bullet and the `block` bullet.

## Desktop D4: public distribution, signed + notarized on macOS, unsigned elsewhere, independent versioning
- **WHEN:** 2026-08-22
- **PROJECT:** brink
- **SYSTEM:** brink-desktop / repo-infra
- **SCOPE:** architectural
- **WHAT:** D4 is un-deferred. The desktop app ships **publicly**, which makes Apple codesigning + notarization non-negotiable on macOS (an unsigned public build is effectively broken there). Platforms: **macOS Apple Silicon, Windows, Linux**. **Windows ships UNSIGNED for now** — there is no cheap notarization equivalent; SmartScreen warns until a download earns reputation, and adding a cert later is a secrets change because the workflow uses the same conditional-signing shape as macOS. **Versioning is INDEPENDENT**: `tauri.conf.json`'s `version` is the source of truth, released on `desktop-v*` tags, decoupled from the crate/npm pipelines. New `.github/workflows/desktop-release.yml` — deliberately NOT `release.yml`, which is cargo-dist-generated and forbidden to edit. **iOS is not being built, but must not be foreclosed**: the two couplings that would block it — the `brink-cli` sidecar (iOS cannot ship subprocess binaries) and the native folder picker (iOS has no arbitrary-directory access) — are today confined to one feature behind `cli.ts` and to the `FileProvider` seam respectively, and must stay that way; sidecar- or picker-dependent behavior must never become load-bearing in the core editing loop.
- **WHY:** Public distribution is the stated goal, and the staging (pipeline first, credentials second) means nothing waits on Apple paperwork: signing steps are conditional on secrets, so the workflow is real and testable while unsigned. Independent versioning matters concretely — the crate/npm release pipeline has not shipped in five weeks (299 pending changesets), and coupling the desktop to it would inherit that stall.

## Update UX: check on launch, prompt to install; save before relaunch
- **WHEN:** 2026-08-22
- **PROJECT:** brink
- **SYSTEM:** brink-desktop
- **SCOPE:** moderate
- **WHAT:** The desktop app checks for updates **silently, a few seconds after launch**, and prompts only when one is found (Install and Restart / Later); a manual **Check for Updates…** item in the app menu reports every outcome including "up to date". Nothing installs without consent, and nothing installs silently. Installing **awaits the canonical save before relaunching**, reusing `awaitSaveAllBeforeQuit` (quit.ts) rather than a third save discipline.
- **WHY:** An editor that swaps itself out under an author mid-session erodes trust, and a "Check for Updates…" button that can produce no visible outcome is a broken button — hence silent-on-launch but talkative-on-request. Relaunching is the same hazard quitting is: an in-flight canonical write must not be raced, and #2370/#2434/#2444 already taught that seam the redispatch-and-cap discipline, so a third copy would only re-learn it.

## The updater lands only once a real signing keypair exists
- **WHEN:** 2026-08-22
- **PROJECT:** brink
- **SYSTEM:** brink-desktop
- **SCOPE:** minor/local
- **WHAT:** `tauri-plugin-updater` is deliberately NOT wired in the first D4 change. It requires a real public key in `tauri.conf.json`; a placeholder would produce an app that advertises an update channel it can never verify. It lands as its own change once `tauri signer generate` has produced a keypair (private key + password in repo secrets, public key in config).
- **WHY:** Landed-but-inert config is the exact pattern this project's reviews keep catching (#2305, #2113). An update channel that cannot verify anything is worse than no channel — it looks like a feature.

## The update manifest lives on a `desktop-latest` alias release, not GitHub's repo-wide "latest"
- **WHEN:** 2026-08-22
- **PROJECT:** brink
- **SYSTEM:** brink-desktop / repo-infra
- **SCOPE:** architectural
- **WHAT:** The shipped app polls `https://github.com/Syynth/brink/releases/download/desktop-latest/latest.json`. `desktop-latest` is a permanent alias release holding only the manifest, re-uploaded by `desktop-release.yml` on every desktop release with `--latest=false`; the binaries stay on the versioned `desktop-v*` release the manifest points into. The rejected alternative was hosting the manifest on GitHub Pages beside the book, which decouples it from releases entirely at the cost of a second publish path.
- **WHY:** `releases/latest/download/...` — the obvious form, and the one originally shipped in #2996 — is wrong for this repo specifically: GitHub's "latest" is the newest release across the WHOLE repo, and brink runs three independent release trains. It resolved to `@brink-lang/studio@0.14.0` at the time of the ruling, a release containing no `latest.json`. Tauri reads the resulting 404 as "no update available", so the first `@brink-lang/*` publish after a desktop release would have killed the update channel silently, with no error surfaced anywhere. This is precisely the failure mode independent desktop versioning was chosen for. The choice is load-bearing beyond ordinary config because the endpoint is **baked into every shipped binary**: an installed app keeps polling the address it was built with, so a wrong value is unfixable in the field by any later release. Pinned by `updater_endpoint_does_not_depend_on_the_repo_wide_latest_release` (src-tauri) rather than left to review, since the broken form is indistinguishable from the working one until months after shipping.

## Crate versions jump to 0.0.15 to line up with the npm packages
- **WHEN:** 2026-08-22
- **PROJECT:** brink
- **SYSTEM:** repo-infra / release
- **SCOPE:** moderate
- **WHAT:** The crates.io release train skips 0.0.12 through 0.0.14 and publishes **0.0.15**, matching the `@brink-lang/*` npm packages' 0.15.0 published the same day. release-plz had computed 0.0.12 from conventional commits; that release PR was retargeted rather than accepted. The two trains stay **independently versioned** — nothing enforces the correspondence, and neither train blocks on the other.
- **WHY:** Two published surfaces of one project drifting to unrelated numbers (0.0.11 vs 0.14.0) makes "which crate version goes with which npm version" a question nobody can answer from the numbers alone, and both are published from this one repo on the same day. Aligning the trailing component makes the pairing legible at a glance. The alignment is a **convention, not a mechanism**: the trains will drift again the first time one releases without the other, and that is accepted — the alternative (coupling the release trains) would make every npm patch drag a crates publish behind it, which is worse than occasional drift. The skipped numbers are burned permanently on crates.io; that is the deliberate cost of the alignment.

## The Player advances ONE line at a time by default; "auto" opts into run-to-pause
- **WHEN:** 2026-08-23
- **PROJECT:** brink
- **SYSTEM:** brink-studio / player
- **SCOPE:** moderate
- **WHAT:** Every reveal in the Player — initial load, after choosing a choice, and the Continue button — advances a **single line**. An **"auto" checkbox, unchecked by default**, switches all three to run-to-next-pause (the current `continueToPause` behaviour). Today `LocalSessionProvider.reveal()` calls `continueToPause()` unconditionally, in a method whose own doc comment reads "Reveal the next line from the runtime" — the code and its documentation have disagreed since it was written.
- **WHY:** The Player is an authoring tool before it is a preview. Dumping every line to the next choice makes it impossible to see where a line lands, which convention fired on it, or where output came from — the author is handed a wall of text and has to reconstruct the ordering mentally. Stepping is what makes the Player usable for debugging a scene. Run-to-pause stays available because reading a long stretch to reach a particular choice is the other real workflow, and toggling beats forcing dozens of clicks; it is off by default because the debugging case is the one that needs the default, and an author who wants to skim can opt in.

## A project is anchored on a FILE: open a `.ink` (it is the entry) or a `brink.toml` (it declares one)
- **WHEN:** 2026-08-23
- **PROJECT:** brink
- **SYSTEM:** brink-studio / brink-desktop — project open model
- **SCOPE:** architectural
- **WHAT:** There are exactly **two doors into a project, and both are files, not folders**. (1) Opening a `.ink`/`.brink` file makes **that file the entry point**, and the filesystem around it is shown. (2) Opening a `brink.toml` uses its `[project] entry`. **An explicit file open BEATS a governing `brink.toml`** — when a `brink.toml` governs the opened file (found by the compiler's own **walk-up** discovery, not merely a same-directory check), the app **warns**, and offers a **one-click switch to the toml project when the opened file is that toml's declared `entry`**. Display extent follows Inky: the entry file (highlighted in the binder), sibling `.ink` files, everything reachable through the include tree, and `.ink` files in subfolders of the entry's folder; include-tree files falling **outside** that root are shown **marked as outside the project**, the way the Library section is. Files shown but **not in the compile closure carry an editor banner** saying they are not analyzed. **"New Project" picks a folder and creates `main.ink` + `brink.toml`.** Upgrading a file-anchored project to a toml project **rewrites the recents entry in place** — one entry, never two. Native (`.brink`) is explicitly **deferred**; the ink flow is what must work.
- **WHY:** The failure this replaces is the worst class this codebase recognises: a project with no `brink.toml` produced a Player where Run/Restart/Continue all did **nothing, silently** (#3010) — no diagnostic anywhere, discoverable only by someone who owns the compiler. Anchoring on a file removes it **by construction**: you cannot open a project without choosing an entry, because opening *is* choosing one. This also settles an **owed** question from #1580, which recorded the lean "each orphan file is its own single-file project" and explicitly demanded it be ruled rather than picked in passing. ⚠ **This REVISES #2331** ("`[project] entry` beats `mountStudio`'s `entryFile`"), which is retained for its own case: a *host-supplied default* still loses to authored config. What changes is that a **human's explicit open is not a default** — it is the strongest available statement of intent, and losing to a config the author may not know exists reproduces "why is it compiling the wrong thing". The warning plus one-click switch is what keeps that from being silent in the other direction. Walk-up discovery is required because the build uses it: a same-directory check would miss the nested case, which is exactly where a wrong guess is least visible. Scaffolding `main.ink` alongside the config means **a new project is never born in the broken state that prompted all of this** — the first run plays.

## Editor layout: literal whitespace, Inky parity
- **WHEN:** 2026-08-23
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** The editor imposes NO layout of its own. Removed: standalone-divert right-align, the screenplay indents (character/parenthetical/dialogue + dialogue column width), CHARACTER uppercase, the 8.5in page cap and script-page margins, the weave-depth artificial indent, and the superscript depth-sigil collapse (nested sigil runs render as typed). Colors/highlighting stay. The classification taxonomy (element classes, `data-depth`, `brink-divert-standalone`) remains the host contract; an embedder wanting a styled layout adds its own CSS — no toggle is kept in the package. Whitespace/tab indent guides are added as the one presentation aid (default on; `indentGuides: false` opts out).
- **WHY:** Real-project authoring (working alongside Inky) showed the screenplay layout doesn't hold up — forced geometry fights the file's actual whitespace. The target is Inky parity from a layout perspective, minus Inky's wrapped-line padding, which is explicitly not wanted.

## RMMZ / Chromium 88 support dropped
- **WHEN:** 2026-08-23
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** The editor packages no longer maintain compatibility with the Chromium 88 floor (NW.js / RPG Maker MZ embeds — the #276-era constraint). Modern CSS/JS features (first user: the `1lh` unit for the indent-guide wrap breaks) may be used without fallbacks for that floor. Existing #276-era workarounds in the codebase are not being proactively removed — they are simply no longer load-bearing and can go opportunistically.
- **WHY:** Maintainer: "i'm just dropping the rmmz support" — no active consumer targets that embed floor, and preserving it taxed every editor presentation change with fallback engineering.

## TODO feature: panel, editor highlight, and Problems integration
- **WHEN:** 2026-08-23
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** The studio gets a TODO feature over ink-native `TODO:` lines (AUTHOR_WARNING nodes): (1) a dedicated TODOs tool window grouped by file → containing knot/stitch, with filter/search, count badge, and click-to-navigate (open file + scroll to line; group headers navigate too); (2) prominent editor highlighting of TODO lines — Inky-grade visibility (amber full-line band + left bar + bold keyword); (3) TODO lifetime is purely existence-in-source — no persistence or done-state; a removed TODO animates out of the panel with a strikethrough; (4) TODOs also surface in the Problems panel as Info-severity diagnostics with lint code `todo`, emitted at HIR lowering (the pass that already owns per-file diagnostics), making them `[lints] todo = "allow"`-suppressible and never compile-blocking. Design canvas: `.design/todo-panel/`.
- **WHY:** Maintainer: TODO lines are the natural authoring workflow marker in ink ("it's extremely noticeable in the inky editor" — parity there matters); grouping by file + knot/content mirrors how authors think about where work remains; existence-based lifetime avoids a second source of truth; the Problems integration was approved once the plumbing check showed it rides the existing lowering-diagnostics pipe rather than requiring new compiler plumbing.

## External renames: allowed behind the always-unsafe Force gate
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui / compiler-ide
- **SCOPE:** moderate
- **WHAT:** EXTERNAL functions become renameable (declaration + all call sites) instead of refused outright — but the safe-rename verdict for an external is ALWAYS unsafe, with a synthesized report entry naming the host-binding consequence ("the engine must re-register under the new name"), so the rename only applies through the breakage report's Force path. Builtins remain non-renameable.
- **WHY:** Maintainer ("let's do the force gate version"): the old refusal protected the story↔engine name contract, but "this breaks something → refuse" is exactly the case the breakage-report surface exists for — inform-and-allow is the project's established rename posture, and every current brink user controls both sides of the binding.

## Desktop performance: measure first, dev-only instrumentation, dep sweep in scope
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** cross-system (editor-ui / brink-web / desktop — performance)
- **SCOPE:** architectural
- **WHAT:** Desktop/studio performance work proceeds **measurement-first**: no performance fixes land until extensive profiling + monitoring infrastructure exists and a recorded baseline characterizes the badness in particular, numeric ways (keystroke latency, scroll frame time / viewport-fill lag, compile-cycle cost, startup). Confirmed hot paths are filed as GitHub issues with numbers attached; fixes are a separate, later wave judged by re-running recorded scenarios through an offline compare tool. The instrumentation + Perf HUD are **dev-only** (stripped/disabled in production builds). A **web dev-dependency sweep (vite 6→8 at minimum)** is in scope, sequenced after baseline capture so the toolchain delta is itself a measured, discrete commit. Profiling runs are **recorded artifacts** (probe JSON + CDP trace + meta, per run) supporting offline analysis and cross-attempt/cross-change comparison.
- **WHY:** The maintainer's judgment: the performance is so uniformly bad that no single fix is plausible, so fixing before characterizing risks unfalsifiable guesswork — "we need extensive profiling and performance monitoring so we know for sure that performance is bad in particular ways" before any fix. This also honors the standing evidence gates (desktop-shell-spec: native-core-over-IPC is a perf escape hatch requiring measured evidence; F5 ruling: incremental-compile investment only if measurement shows). Symptoms driving this: visible delay on a single line break and scrolling ahead of CM6's rendered viewport in large files, reproduced in a packaged build on high-end hardware. Dev-only instrumentation was chosen over ship-hidden; the dep sweep rides along because profiling on a two-majors-old toolchain risks chasing upstream-fixed artifacts.

## Desktop perf: optimization wave first, async-architecture rework second
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** cross-system (editor-ui / brink-web / brink-ide — performance)
- **SCOPE:** architectural
- **WHAT:** The desktop-perf fix work is sequenced **optimization-first**: the measured hot paths (#3063 snapshot clone, #3064 per-keystroke query stack, #3065 byte_to_utf16→LineIndex, #3066 compile fan-out laziness, #3067 rails gutter, #3068 startup, #3069 first-post-edit compile) are made fast on the CURRENT synchronous architecture before the editor moves to the async/off-main-thread intelligence model. The async rework remains the acknowledged destination (industry shape: language intelligence off the UI thread, last-known-results rendering, an explicitly enumerated sync core — to be specced when its phase opens), but is deliberately second. For #3063 this means the optimization-shaped fix (share/COW the HIR instead of deep-cloning, keeping both analysis roads) rather than retiring the off-db road — road retirement is an architecture-phase question. Every fix is judged by the recorded scenarios + `scripts/perf-compare.mjs` per docs/desktop-perf-baseline.md.
- **WHY:** Maintainer: optimization-first "will allow us to FEEL the improvements and get a sense," and moving slow code behind a worker first would "wall off the stuff that's slow and then we have no pressure to improve it." This also aligns with the physics of the rework itself: off-thread analysis speed becomes result *staleness*, so the worker architecture inherits whatever analysis speed exists — fast-first means the rework ships with fresh results instead of laundering slowness through a thread boundary.

## Desktop perf charter: effect-inference opt-out, compile triggers, frame budgets
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** cross-system (compiler / editor-ui / studio — performance)
- **SCOPE:** architectural
- **WHAT:** (1) **Effect inference becomes opt-out-able/deferrable, and for the ink dialect specifically it should be possible to skip it entirely** — mechanism to be designed (the existing dev/prod-knob ruling points at `brink.toml` profile + host-API override as the knob home; dialect-sensitive defaults are on the table). (2) **The studio's compile trigger moves from the 500 ms while-typing debounce to compile-on-save and compile-on-player-interaction** (Run/restart/continue, explicit compile) — typing runs analysis only. (3) **Performance budgets are adopted and written into the baseline doc, with the frame budget at 8 ms** (120 fps — ProMotion macOS), the marquee scenario being high-speed scrolling in a large file with rendered text staying ahead of the scroll.
- **WHY:** (1) Real consumer: a live project uses brink-studio as the editor but ships on the reference inkjs compiler/runtime, so effect tracking is "literally useless there" — paying 88% of cold compile (and the ~1.1 s first-post-edit re-inference, #3069) for data with zero consumers is indefensible; ink-compat content has no effects surface anyway. (2) Resolves the player-follows-typing question deliberately: the live-reload-while-typing property is given up in exchange for a typing loop that never compiles; save/interaction are the natural freshness points. (3) Budgets make "done" falsifiable and are the standing pressure mechanism from the optimization-first ruling: whatever budget the optimization wave cannot meet becomes the quantified case for the async-architecture phase. 8 ms rather than 16 ms because the target hardware is 120 Hz.

## Live editor diagnostics route through the db road (option A — #1347's preserved call, ruled)
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** brink-ide / brink-db / brink-web — editor analysis architecture
- **SCOPE:** architectural
- **WHAT:** The maintainer call #1385 deliberately preserved is now made: the editor's live-typing analysis adopts **option A** from `docs/live-typing-diagnostics-divergence.md` — `IdeSession`'s per-edit path routes through the db (`db.analysis()`-shaped queries) instead of `IdeSnapshot::analyze`, retiring the off-db pure path as the editor's diagnostics producer. Scope of the package: (1) the routing change itself, with `update_and_analyze` syncing db options (closing #2885's gap as a side effect); (2) `live_typing_db_divergence.rs` extended to pin the new arrangement; (3) an enumeration pass over compile-road-only diagnostics (codegen/lowering-time) so compile-on-save cannot hide an error class until save — anything found is either added to the live surface or explicitly ruled save-time; (4) `IdeSnapshot` becomes vestigial for this consumer (brink-lsp's own pure-path call site is not in this package's scope and follows separately). ⚠ **This REVISES the same-day optimization-first entry's #3063 clause** ("share/COW the HIR instead of deep-cloning, keeping both analysis roads"): road retirement for the editor IS the #3063 fix — Arc/COW is moot on this path.
- **WHY:** The divergence class, not instance: #1526/#1553/#1562/#1347–#1358 were four hand-repairs of the same seam, whose root cause is structural — the pure path's `(FileId, HirFile, SymbolManifest)` inputs carry no path- or config-derived facts, so every db-keyed check diverges until someone copies its key into the snapshot. Option B (#1358) closed the measured instance but institutionalized the parallel path; under compile-on-save (ruled same day) the pure path's output becomes what authors stare at for whole sessions, making the open class intolerable. The divergence doc recommended A on correctness but recorded performance as unmeasured — the desktop-perf baseline supplied the missing measurement, and it points the SAME direction: the pure path's snapshot clone is 28–33 ms per keystroke (91% of large-file keystroke cost, #3063) while the db road's warm reanalysis is ~1–3 ms. One change closes the correctness class, deletes the dominant keystroke cost, resolves the #2885 options gap, and makes the editor's hot path actually use the fine-grained-salsa substrate built for it.

## Search results: stable snapshot, per-match editable cards with context
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** The search-results surface (shared by text search and Find References) is rebuilt as a list of per-match cards: a header row (file:line, containing knot/stitch, and for references a kind-of-use badge with the declaration pinned/badged) above an individual small editable buffer showing the match with context lines — default 1 before / 2 after, user-tunable. Inline editing stays. Once a search has been performed the result set is a FROZEN SNAPSHOT: edits — including edits that invalidate the match — never remove or re-filter rows; only running a new search replaces the set. Performance permitting ("if it's not too slow"): virtualize the card list rather than compromising the per-card buffers.
- **WHY:** Maintainer: inline editing is the point of the surface, and stability under edits is what makes editing through results trustworthy — a row vanishing mid-edit because it no longer matches destroys the workflow. Context lines (1/2) give enough surrounding prose to edit confidently; per-card headers carry the identity information references need. Addendum (same day): an explicit ↻ refresh re-runs against current sources and replaces the snapshot — the user-initiated counterpart to the freeze (references refresh re-resolves from the declaration's edit-mapped current position); cards are fully syntax-highlighted via a per-file semantic-token cache (one wasm call per file with results).

## Cmd-click on a definition runs Find References
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** minor/local
- **WHAT:** Cmd/Ctrl-clicking a symbol token that IS the definition (the clicked position sits inside the definition's own span, same file) runs Find References instead of Go to Definition. Use sites keep navigating to the definition; empty/unavailable references fall back to selecting the declaration.
- **WHY:** Maintainer: "cmd clicking a definition should do find references by default, not go to definition, because you're already... there" — self-navigation is a dead action at the definition, and the references surface is the useful counterpart from that position (matches the VS Code/JetBrains convention).

## Search panel reuses the binder's expand/collapse-all buttons
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** minor/local
- **WHAT:** The search summary row's collapse-all/expand-all controls are the binder header's icon buttons (same `ExpandAllIcon`/`CollapseAllIcon` + `.brink-binder-tool` treatment), not bespoke glyphs.
- **WHY:** Maintainer ("re-use the expand/collapse all buttons from the binder, please"): one icon vocabulary for the same operation across panels — new panels should reach for the existing control set before inventing controls.

## Search panel top: references as a scope chip in the query box
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** minor/local
- **WHAT:** The Search panel's top piece is Direction C of the search-panel-header canvas: the bordered references header is gone; references identity renders as a dismissible scope chip (`refs <symbol> ✕`) INSIDE the query box (typing replaces it — the existing exit semantic made visible), and below the form sits one flat summary strip shared by both modes: count left ("N results/references · M files"), tool cluster right (binder expand/collapse-all icons · context value `1↑2↓` · ↻). The strip never wraps; the chip symbol ellipsizes.
- **WHY:** Maintainer ("C is great, let's do that") after rejecting the stacked bordered pill ("this is dookie"): the ✕ belongs on the thing it clears, the chip makes typing-to-exit physically obvious (JetBrains scope-chip convention), and a strip that carries no identity always fits one row at panel widths.

## Cmd-click on an INCLUDE path opens the file
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** minor/local
- **WHAT:** Cmd/Ctrl-clicking the file-path text of an `INCLUDE` line opens that file (same `onNavigateToFile` route as the context menu's "Open <file>" item). The clickable span is the path text only — the INCLUDE keyword stays an ordinary click.
- **WHY:** Maintainer ("⌘click on the file in an include statement should open that file, i think"): the path is a navigable reference like any other symbol; cmd-click is the editor's universal follow-the-reference gesture, so it should not dead-end on INCLUDEs.
## Demo gate off the PR path
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** ci
- **SCOPE:** minor/local
- **WHAT:** `demo.yml` loses its `pull_request` trigger — the compound-demo build no longer runs (even advisorily) on PRs. The weekly scheduled run and `workflow_dispatch` remain the demo's health check.
- **WHY:** Maintainer, while merging the perf stack with only DEMO_GATE outstanding: "the demo is silly" as a per-PR gate — it delays merges for signal that the weekly run provides just as well; it was never in the required-checks list, so a red demo could only ever slow a human down, not protect main.

## Option A goes total: the off-db analyzer composition is DELETED, LSP included
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** brink-ide / brink-lsp / brink-analyzer / brink-db — analysis architecture
- **SCOPE:** architectural
- **WHAT:** (1) The same-day option-A ruling's "brink-lsp out of scope" clause is SUPERSEDED: brink-lsp's `analysis_loop` migrates to per-root `db.analysis()` pulls in the same package (off-lock via a second salsa read handle if the pinned salsa supports one; otherwise in-lock, accepted — warm pulls are ~1–3 ms). (2) `brink_analyzer::analyze_with_modules` is **deleted**, not left as a parallel engine — its remaining callers (the overlay/dir-move breakage gates' throwaway analyses, the brink-ir/brink-ide test harnesses) migrate to throwaway `ProjectDb` pulls; the analyzer's piece functions survive (they are what the salsa queries call). (3) `brink-db/tests/query_equivalence.rs` is **deleted, not re-aimed**. (4) The compile-road-only diagnostic classes stay compile-time **by design**: LIR-lowering codes (E030 E031 E052 E054–E059 E073–E077 E082 E083 E099 E120 E143 E144 E148 E158 E181 E183 E184 E187) and codegen codes (E052 E057 E081, synthesized E060) are artifact-construction errors, structurally gated behind an error-free closure; the ruled compile-on-save trigger is their natural surface. Per-file parse/lowering diagnostics were verified to already reach the editor via `db.diagnostics(file)` — no gap.
- **WHY:** Maintainer, at plan review: "i really want to just REMOVE the dead/slow version, not simply stop using it in the editor" — leaving a parallel engine alive anywhere preserves the divergence class the whole change exists to kill, and preserves the maintenance duty of keeping it correct. On the equivalence test, maintainer: "if we don't have two paths, do we need equivalence testing?" — no: equivalence testing was symptom management for having two implementations of one truth; with one producer the divergence class is impossible by construction, and correctness is held by the behavioral suites downstream of the single road (oracle ratchet, tier goldens, acceptance gate). The save-time ruling for LIR/codegen classes follows from their structure (they cannot fire until the closure is error-free) rather than from convenience.

## Per-knot incremental lowering: keys, frontend order, sequencing
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** brink-db / brink-syntax / brink-syntax-native — incremental lowering (#3084)
- **SCOPE:** architectural
- **WHAT:** The three questions `docs/per-knot-incremental-lowering-spec.md` §5 left open are ruled: (1) segment memos key on **salsa tracked structs** minted by the segmentation query with content-hash-seeded identity — not positional `(file, index)` keys, not interned content hashes; (2) **ink's frontend goes first**, native follows once the assembly/range-rebasing machinery is proven; (3) the work lands **before the async-architecture phase**, as optimization-wave-shaped (byte-identical output, no API/threading change). The spec's measurement gate (split the ~30 ms single-file pipeline per pass) remains implementation step one.
- **WHY:** (1) Tracked structs survive both offset shifts and knot insertion/reorder, are garbage-collected by salsa (the immortal-interned-memo caveat flagged on `MemberSet` is deliberately avoided), and align with the ruled salsa-native-workspace direction (partition as tracked data) so the two effortlessly converge. (2) The measured symptom is an ink project (the studio+inkjs consumer), and ink's line-anchored headers make its segmenter nearly trivial — the native brace-aware segmenter's risk must not hold the ink win hostage. (3) The same logic as the wave-first ruling: land the felt improvement now, and let the async phase inherit cheap analysis as result-freshness rather than absorbing slow analysis behind a thread boundary.

## #3088 is fixed properly: one lowering composition, silent E095/E049 drop fixed
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** compiler (ink lowering / brink-db)
- **SCOPE:** moderate
- **WHAT:** The `lower_file` double-lowering defect (#3088) is fixed by restructuring, not by a byte-identical patch: declarations are lowered once (values + diagnostics from one walk), `lower()` and the db road share the same composition pieces so they cannot drift again, and the previously-dropped file-level `#@module`/`#@was` arbitration diagnostics (E095 self-alias, E049 was-without-module) now reach the editor, pinned by a db-road test.
- **WHY:** The silent drop is a live bug by the house rule (silent drops are bugs until proven otherwise) — the analyzer explicitly skips re-diagnosing E095 on the assumption lowering surfaced it, while the db road discarded it. No test pins the broken behavior; the affected population is malformed module directives, where failing loudly is correct. Preserving the drop for byte-identity would embalm a bug and hand #3084's assembler a documented wart. (The maintainer chose the proper fix over the byte-identical patch and a two-PR split when presented with all three.)

## Segment tracked structs carry their text (2x source residency) — provisional
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** brink-db — per-knot incremental lowering (#3084)
- **SCOPE:** moderate
- **STATUS:** tentative
- **WHAT:** `FileSegment` tracked structs store each segment's source text as an untracked (identity) field — salsa's own untracked-field hash IS the ruled content-hash identity, and its recreate-time equality check makes hash collisions harmless. Consequence: source text is resident ~2x (the input plus the segment copies). Accepted provisionally; the memory accounting must cover the new ingredient from day one, and the posture is revisited once real numbers (memory bench / studio-scale project) are in hand.
- **WHY:** The alternative — segments carrying only ranges, lowering slicing the original file text — silently reintroduces a whole-file input dependency and nothing ever backdates (the exact trap the FG-3 range-free projections avoid). A hand-rolled content-hash field hashes the same bytes anyway and makes collisions alias distinct knots. Source text is the smallest payload class in the db, but the maintainer flags the duplication as a likely real cost — hence tentative, with measurement before any doubling-down.

## Editor freshness model: per-segment IDE queries, debounced project analysis
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor-ui / brink-ide / brink-web (#3064)
- **SCOPE:** architectural
- **WHAT:** The per-keystroke editor stack is restructured in two ruled steps. (1) APIs first: `line_contexts`, `semantic_tokens`, and `folding_ranges` become per-segment memoized queries over the #3084 segment substrate (content-keyed fragments + rebase assembly) — so every knot's classification/coloring stays fresh on every keystroke at roughly the edited knot's cost, meeting the ruled freshness requirement (fresh within the knot being typed) with no staleness window; `updateDocument` splits into a cheap delta-splice source push (sync, per keystroke) and a whole-project analysis pull that the editor debounces on pause. (2) Then the async scheduling model on the JS side (lazy folding, mapped-then-refreshed decorations, debounced diagnostics).
- **WHY:** Measured (segment-road delta, 2026-08-24): ~56 of the 72 ms browser keystroke is wasm whole-doc IDE query walks — not analysis (4.3 ms native) and not tree construction (parse 3.8 ms), so rowan incremental reparse alone would recover little. The rowan-idiomatic incrementality at our scale is salsa memoization over stable fragment identities — already built for lowering; pointing the IDE queries at the same substrate beats both viewport-scoping and staleness debouncing for the surfaces the maintainer ruled must stay knot-fresh. Debounce-on-pause is reserved for the whole-project pull, where every mainstream editor already does it.

## Outbound query deltas: segment-keyed outputs with identity versions (option A)
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** brink-web / editor packages (#3064)
- **SCOPE:** architectural
- **WHAT:** The per-keystroke wasm→JS payloads (whole-file line contexts ~836KB, spans ~640KB JSON) are replaced by a segment-keyed delta protocol, sequenced AFTER C1 (inbound edit deltas) and C2 (debounce/mapping for the non-sync surfaces): a tiny per-keystroke segment manifest (`[{id, lineStart, version}]`, where the salsa tracked-struct identity IS the version — stable across shifts, changed exactly when content changes) plus per-segment pulls returning segment-relative data; TS caches results per segment id, re-fetches only changed segments, and re-offsets shifted ones with local arithmetic. Scope after C2 = the surfaces that stay keystroke-sync (line contexts, tokens, folds). Gate: the TS-assembled result must equal the wasm-assembled one, same corpus-parity discipline.
- **WHY:** "No good reason to send unbounded data for a bounded update" (maintainer). Viewport-scoping was considered and rejected for the sync surfaces: it converts SCROLLING into a wasm-query-per-frame workload, and the ruled scroll budget (text ahead of high-speed scroll) is served free from a JS-side segment cache. Structured transfer alone (no JSON) bounds nothing.

## Editor background architecture: split worker (option B), one architecture for web + desktop, spec first
- **WHEN:** 2026-08-24
- **PROJECT:** brink
- **SYSTEM:** editor packages / brink-web (worker architecture)
- **SCOPE:** architectural
- **WHAT:** The editor's analysis session moves off the UI thread as a **split** architecture (option B of the four surveyed): a Web Worker owns the full project `EditorSessionHandle` (analysis, diagnostics/compile, refactors, panels — everything with project state), while the main thread keeps a **minimal classifier instance** scoped to the open document's per-segment substrate (lex/parse/lower, classifier tokens, line contexts — no symbol index, no resolution, no project files) so newly typed text is styled same-frame by the real lexer. The transport is the already-shipped outbound delta protocol (version-keyed segment manifest + owned slices + config epoch) plus edit-span ingress. The same worker architecture serves the browser playground, the embeddable studio, and the Tauri desktop webview — no desktop-native (Tauri IPC) fork; desktop may layer process isolation later if ever needed. A spec (`docs/editor-worker-spec.md`) is reviewed before implementation. Rejected: pure worker (option A — frame-behind styling flicker on fresh tokens with no synchronous layer under it), wasm threads/SharedArrayBuffer (COOP/COEP burden on every embedder, salsa-on-wasm-threads unproven), desktop-native primary (forks web/desktop).
- **WHY:** The perf program made the main thread *usually* fast (~6–7 ms keystrokes) but not *structurally* non-blocking — deferred refreshes, diagnostics/compile, and cold pulls still run on the UI thread when they fire. A worker makes blocking impossible by construction. B over A because the main-thread classifier is measured cheap (~0.13 ms warm token walk) and is the moral equivalent of VS Code's synchronous grammar layer — except it is the real lexer, so same-frame styling is never wrong about token boundaries. The main-thread instance must never grow project features; that boundary is structural (a capability-stripped session surface), not a convention.

## W5 moves the doc-scoped road to the worker (completeness over minimal diff)
- **WHEN:** 2026-08-25
- **PROJECT:** brink
- **SYSTEM:** editor packages / worker architecture (W5)
- **SCOPE:** architectural
- **WHAT:** W5 migrates the doc-scoped query road to the session worker as `docs/editor-worker-spec.md` §12 specifies, rather than stopping at the W4 project-road split. The alternative — leaving doc-scoped queries in-process permanently, since they measure single-digit milliseconds behind the classifier — was surfaced explicitly and declined.
- **WHY:** The program's stated goal is a main thread *structurally unable* to block on project analysis, not one that usually doesn't; leaving a sync analysis road on the main thread leaves the class of regression open forever. Moving it also retires the transitional dual-session residency (the full wasm session leaves the main thread; only the classifier and player remain).

## W5c: the main-thread session demotes to content-store + fallback; a guard replaces the delete
- **WHEN:** 2026-08-25
- **PROJECT:** brink
- **SYSTEM:** editor packages / worker architecture (W5c)
- **SCOPE:** architectural
- **WHAT:** The spec's W5 "delete the synchronous main-thread session" lands as a demotion instead: the session survives as the content store feeding the worker replica and as the complete in-process fallback road, while the structural goal — recurring paths that cannot pull analysis on the main thread — is enforced by worker-fed stashes (deferred rebuilds read them; a dirty bit keeps a stash from being served across an edit it predates) plus a lexical boundary guard pinning every surviving analysis call to a documented allowlist. One-shot command paths (goto/rename/symbols/search-cards) stay main-side at incremental-analysis cost, tracked by #3110.
- **WHY:** The fallback road the architecture deliberately keeps (no-Worker environments, non-vite embedders, worker crashes) requires a fully functional main-side session — deleting it would delete the fallback. The guard delivers the same regression-impossibility the delete was for, at a fraction of the diff and without forking behavior the mock/test surface depends on. (Implemented under the maintainer's wrap-up directive; flagged for veto.)

## Performance instrumentation ships in production builds
- **WHEN:** 2026-08-25
- **PROJECT:** brink
- **SYSTEM:** editor packages / studio / desktop
- **SCOPE:** moderate
- **WHAT:** The perf surface (probe, browser observers, wasm counters, `__brinkPerf`, the Performance tool window) is no longer dev-only: `mountStudio` enables it by default in all builds, with `MountStudioOptions.perf: false` (playground `?perf=0`) as the embedder opt-out. Supersedes the dev-only edge of the 2026-08-24 measure-first ruling. Corollaries: the worker realm reports its own probe + wasm counters through host-level protocol queries (`hostPerfReport` family), and the probe's User Timing mirror self-clears its own entries so an always-on session stays bounded.
- **WHY:** Real projects are opened in production desktop builds — a dev-only panel structurally cannot measure the case that matters (the maintainer's own project remains slow where the small test fixtures are fast). The panel's payload is structurally content-free (static span/counter names, numeric values), so shipping it leaks nothing from an author's project.

## Content-logic delimiters render as code, not prose
- **WHEN:** 2026-08-25
- **PROJECT:** brink
- **SYSTEM:** editor packages / semantic tokens
- **SCOPE:** minor/local
- **WHAT:** The `{` / `}` around inline alternatives, conditionals, and interpolations — and the `|` between alternative branches — classify as operator semantic tokens (both ink and native classifiers), so they take the code color instead of blending into dialogue/action prose. Prose-absorbed and escaped braces/pipes stay uncolored.
- **WHY:** Author feedback relayed by the maintainer: uncolored delimiters visually merge with the surrounding dialogue, tricking the reader into parsing them as part of the prose. The delimiters are structural code; they should read like the conditions they delimit.

## Manuscript colorway + Inky themes ship as selectable themes
- **WHEN:** 2026-08-25
- **PROJECT:** brink
- **SYSTEM:** editor packages / studio themes / semantic tokens
- **SCOPE:** moderate
- **WHAT:** The writing-first colorway designed live against author feedback ships as a new selectable theme, "Manuscript" (option B of docs/editor-color-design.md — existing themes untouched): prose brighter than everything (#f2f4fc), narrative structure markers (`* + [ ] -`) and the halt words (`END`/`DONE`) hot red (#ff5d62), all other machinery in one tight cool band ordered by conceptual distance (definitions #b9a9e6 → diverts #a4abdf → keywords #8ba6cb → ops #90afcc → bindings #93b8c8 → strings #98bab4), tags yellow, cues rendered as plain prose. Faithful ports of Inky's two looks ship alongside as "Inky" (flow blue / logic green on white) and "Inky Dark" (red bullets, sage flow, leaf-green logic, cream prose on #282828), colors read from the app's own stylesheets. Supporting classifier split: choice bullets/gather dashes/weave brackets get a `marker` token type, diverts/tunnels/threads/glue get `divert`, `END`/`DONE` get `halt`, and header equals-runs classify with their definition; expression-position lexemes keep their operator classification. Existing themes preserve their exact look via CSS fallbacks.
- **WHY:** The author's feedback ruled the shape (machinery colored by what it does; hue distance = conceptual distance; markers rare-and-bright per the Inky convention; prose as the page's one true foreground) through four mockup iterations; a new theme rather than a re-map keeps the Catppuccin themes for anyone who prefers the IDE feel, and the Inky ports give ink authors a familiar landing.

## Detached gutters over reducing gutter content (#3119)
- **WHEN:** 2026-08-26
- **PROJECT:** brink
- **SYSTEM:** editor packages (CodeMirror layout)
- **SCOPE:** moderate
- **WHAT:** The WebKit editor-layout cost is fixed by removing the gutters from CodeMirror's scroller flex/sticky flow (absolute, auto height) and compensating with content padding, rather than by reducing what the gutters contain (merging the four columns, painting rails without DOM, or a canvas gutter). Engaged only when line wrapping is on.
- **WHY:** Measurement, not architecture, chose it: the cost is independent of gutter content — hiding 48 of ~100 gutter elements in place changes nothing, and one column costs the same as four — so every content-reduction option was worth ~0. It is also not paint (layer promotion and paint containment each moved the number by zero) and not the markup alone (a synthetic replica lays out in ~0ms in both engines). Detachment is sound rather than a hack because gutters are sticky to survive HORIZONTAL scrolling, which a permanently wrapping view never does.

## Restored editor tabs are scoped per project, with an LRU cap
- **WHEN:** 2026-08-26
- **PROJECT:** brink
- **SYSTEM:** studio-shell / editor-state persistence
- **SCOPE:** architectural
- **WHAT:** Editor state that survives a reload — open tabs, tab order, pin/preview state, the active tab per group, the group split structure and sizes, and each open document's cursor + scroll — is persisted under a storage key namespaced by a project scope the host supplies, not globally. The store keeps one entry per project, most-recently-used first, evicting past a fixed cap so the payload cannot grow without bound. This is the studio's first project-scoped storage key; layout, theme, keymap and editor settings stay global, because they are preferences about the app rather than state about a project.
- **WHY:** Tabs are the first state whose meaning depends on which project is open — a global slot would either restore tabs naming files that do not exist in the current project, or force dropping them on every project switch, so moving between two projects would permanently cost whichever one you left. Per-project scoping keeps both. The cap is what makes it safe to key by project at all: without eviction the entry count grows with every project ever opened, which is small per entry but unbounded, and unbounded accumulation in a fixed-size store is the failure this repo already guards against elsewhere ("Guard against unbounded growth").

## Continuous view orders files by binder order
- **WHEN:** 2026-08-26
- **PROJECT:** brink
- **SYSTEM:** studio editor surfaces (continuous / "Scrivenings" mode)
- **SCOPE:** moderate
- **WHAT:** The continuous editing surface — the one that scrolls through several files as a single view with headings between them — orders those files by BINDER order, not by INCLUDE/compile order.
- **WHY:** The two orders diverge, and the choice decides what the mode means. Binder order is the authoring order the writer arranged themselves; it is already first-class (the `.binder.json` order sidecar, drag-to-reorder), so the continuous view reads the manuscript the way its author laid it out. Compile order is reachability from the entry file — a property of the story's execution, not of the manuscript, and one the author does not directly control.

## Each editor surface keeps its own persisted state; the active file is shared
- **WHEN:** 2026-08-26
- **PROJECT:** brink
- **SYSTEM:** studio editor surfaces / editor-state persistence
- **SCOPE:** architectural
- **WHAT:** The editor region is a swappable SURFACE — the tabbed/code editor, a focused single-document editor, and a continuous multi-file view are alternative components filling the same region, reusing the existing document components (editor, player, graph) unchanged. Each KIND of surface owns its own persistable state, stored separately, rather than all three projecting from one shared model. The likely exception is "the active file", which is shared across surfaces so switching surfaces keeps you on the document you were working on.
- **WHY:** The surfaces have genuinely different state — a tab list and split geometry mean nothing to a continuous scroll, and a scroll offset through a concatenated manuscript means nothing to a tab strip — so forcing them through one model would either lose state on every switch or accumulate fields most surfaces ignore. Keeping them separate lets each surface restore exactly what it had. The active file is the exception because it is the one piece of state that means the same thing everywhere, and it is what makes switching surfaces feel like changing the view rather than losing your place.

## The three editor views are named Code, Single File, and Continuous
- **WHEN:** 2026-08-26
- **PROJECT:** brink
- **SYSTEM:** studio editor surfaces
- **SCOPE:** moderate
- **WHAT:** The swappable editor surfaces are named, and the names are the user-facing vocabulary: **Code view** is what exists today (tabs, groups, splits); **Single File view** shows one file at a time with a native player split; **Continuous view** scrolls through several files as one manuscript. Each is a distinct view the author chooses, not a mode flag mutating the others' behaviour.
- **WHY:** Naming them settles what each is FOR before any of them is built, which is what the earlier framing lacked — "main-editor mode" described a tab-semantics tweak rather than a view, and pushed the design toward mutating pin rules inside the existing surface. Calling today's surface "Code view" also stops it being the unnamed default that other views are defined against: it is one of three, with its own audience (a writer working across files, wanting structure and splits) rather than the baseline everything else deviates from. "Single File" carries the player split in its definition because a writer on one file still needs to run it — that is what makes it a usable view rather than a stripped-down Code view.

## The editor root area has one occupant; Graph and Settings are peers of the views
- **WHEN:** 2026-08-26
- **PROJECT:** brink
- **SYSTEM:** studio editor surfaces
- **SCOPE:** architectural
- **WHAT:** The editor root area holds exactly one occupant at a time. The three views — Single File, Code, Continuous — inhabit it, and the **Story Graph** TAKES OVER the same area rather than opening as a document inside whichever view is active: it is a peer of the views, not a content of them. Single File view is built first.
- **STATUS:** tentative for Settings — the Graph half is not tentative. Settings placement is OPEN with three candidates raised in quick succession: (a) take over the editor root area like the Graph, (b) a modal, (c) pop out into its own window, "like zed — i like zed's settings view". All three beat the status quo of a tab, because none of them costs you the writing surface permanently.
  The deciding factor is likely the desktop/web asymmetry rather than taste: brink-desktop is a Tauri app where a second OS window is first-class and matches the Zed reference exactly, but the embeddable studio and the playground cannot open a real window (a popup is blocked as often as not), so (c) means building (b) as the fallback anyway. (a) is the only one that needs no second implementation.
- **WHY:** It answers "where does the Graph go in a view with no tab strip" without inventing a second mechanism for it. Both are whole-window activities — you consult the graph or change a setting, then go back to writing — so occupying the area you were writing in matches what you are actually doing, and it means every view gets them for free instead of each view needing its own answer. It also keeps the views honest: a view is defined by how it presents FILES, and neither the graph nor settings is a file.

## The Story Graph becomes a tool window, not a takeover
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** studio editor surfaces / story graph
- **SCOPE:** moderate
- **WHAT:** The Story Graph moves out of the editor root area and becomes a TOOL WINDOW, docked beside the editor like the Program Explorer or State View. It stops being a takeover. Settings stays a takeover for now.
- **WHY:** Shipping the one-occupant rule cost something that was not visible when it was ruled: the graph could previously sit in a split beside the editor, so you could watch it update as you typed, and a takeover makes the graph and an editor mutually exclusive. Authoring against a structure view is a watch-while-you-write activity — you add a divert and want to see where it lands — so mutual exclusion is the wrong shape for it specifically. A tool window restores co-visibility WITHOUT reintroducing "the graph is a tab", and it describes the graph more honestly than either: it is a view onto the story, like the other tool windows, not a document you edit. Settings is genuinely consult-and-dismiss, so the takeover still fits it.

## Spellcheck: prose only, squiggles always, dictionary in brink.toml
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor / spellcheck
- **SCOPE:** architectural
- **WHAT:** Spelling is checked on PROSE ONLY — the HIR overlay's `content` spans — never on machinery (`divert`, `logic`, `var_ref`, calls) and not on knot/stitch names. Results merge into the editor's diagnostic set so they render as squiggles and are listable, but the Problems panel FILTERS THEM OUT BY DEFAULT; the author opts in to seeing them in the list. The per-project custom dictionary (story proper nouns) lives in `brink.toml`. Grammar checking is out of scope for the first version.
- **WHY:** Prose-only is the whole reason this is worth building rather than switching on the browser's native spellcheck: a generic checker flags every knot name, variable and divert, which is why spellcheck is normally turned off in code editors. Brink already knows which spans are prose, so it can do the thing others cannot. Squiggles-always-but-filtered-by-default resolves the tension between "I want to find my typos" and "fifty proper nouns must not bury a real compile error" — the Problems panel's filters, grouping and persistence already exist, so the list costs almost nothing while defaulting to quiet. `brink.toml` keeps the dictionary with the project so collaborators share it and it survives a fresh clone: a character's name is a fact about the manuscript, not about one machine.
- **NOTE:** `brink-project-config` is parse-only (`toml`, not `toml_edit`), so "Add to dictionary" needs a config WRITER that preserves comments and formatting. That is the same machinery the "suppress this lint project-wide" beta note needs, so it should be built once and shared rather than twice.

## `drafts` is a real document status, and nothing disappears because of it
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** brink.toml schema / studio (binder, continuous view, banner)
- **SCOPE:** architectural
- **WHAT:** `drafts` in `brink.toml` is a list of path globs naming files with DRAFT STATUS — not merely a banner suppressor. A draft file: (a) never shows the "not included from main" banner, (b) is visibly marked in the Binder, and (c) still appears in Continuous view's manuscript, visibly marked in its heading rather than omitted.
- **WHY:** The name has to earn itself: a setting called `drafts` that only silenced one banner would be a lie about what it models, and the honest small version would have been `[lints] unreachable` instead. Making it a status means one declaration explains several behaviours rather than each surface inventing its own rule for "work in progress".
  The Continuous half is the interesting call, and it goes the other way from the obvious one. A read-through arguably wants only the story as it stands — but a view whose whole appeal is "scroll through everything" must not silently omit files, because the author cannot tell the difference between a file they marked draft and a file that failed to load. Marking is legible; disappearing is not. So drafts stay in the scroll and say what they are.
- **NOT DECIDED:** whether draft status affects search results or compile behaviour. Both are deliberately left open — this ruling covers the banner, the Binder, and Continuous only.

## Draft files are not compiled
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** brink.toml / compile pipeline / studio diagnostics
- **SCOPE:** moderate
- **WHAT:** Files with draft status (see "`drafts` is a real document status") are NOT compiled. This closes the compile half of the question that ruling deliberately left open. Search behaviour remains undecided.
- **WHY:** A draft is work in progress by definition, so compiling it produces diagnostics about text the author already knows is unfinished — noise in the Problems panel that competes with errors in the actual story. Not compiling drafts is what makes the status worth declaring: scratch scenes and cut material stop reporting on themselves.
- **OPEN, and it needs an answer before implementation:** what happens when a draft file IS reachable from the entry — someone marks a file `drafts` that `main.ink` still INCLUDEs. Skipping it would break diverts into it while the story looks fine, which violates the "nothing disappears silently" principle the drafts ruling rests on. Candidates: compile it anyway and warn that draft status is being overridden by reachability; refuse and diagnose the contradiction; or treat reachability as the stronger signal and ignore the glob. Not decided here.

## Reachability wins: a file is a draft only if marked AND not included
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** brink.toml / compile pipeline / studio
- **SCOPE:** moderate
- **WHAT:** Draft status is DERIVED, not merely declared: a file is a draft when it matches a `drafts` glob **and** is not reachable from the entry. A marked file that `main.ink` still reaches is simply not a draft — it compiles normally, with no special treatment. This resolves the open question left by "Draft files are not compiled".
- **WHY:** It removes the contradictory state instead of handling it. The earlier framing allowed "marked draft but included", which forced a choice between breaking diverts silently, diagnosing a conflict the author has to resolve, or letting a glob quietly disable part of a working story — all three bad, and the first violates the principle that nothing disappears silently. Making reachability the stronger signal means **draft status can never break the story by construction**: the only files it can exclude from compilation are files the compilation never reached anyway.
  It also makes the rest coherent rather than coincidental. The "not included from main" banner is precisely the not-reachable signal, so suppressing it for drafts is exact — an included file has no banner to suppress. And "drafts are not compiled" stops being a compile-pipeline exclusion at all; it is just the existing behaviour for unreachable files, minus the diagnostics noise.
- **ALSO:** a draft file's EDITOR should be badged or styled so the state is visible while writing in it, not only in the Binder. Exact treatment open.

## Draft status travels with the filename
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** studio (binder, editor views, tabs)
- **SCOPE:** minor/local
- **WHAT:** A draft file is marked wherever the studio NAMES it: the Binder row, the Continuous view section heading, the Single File view header, and the Code view tab.
- **WHY:** Stated as a rule rather than a list of four places, because the list is not the point — the point is that a file's name and its draft status should never be shown apart. Any surface added later that names files inherits the requirement without needing another ruling. It also answers the question the previous ruling left open ("badge or style the editor somehow") in a way that does not depend on which view is active: whichever surface you are looking at, the file that is a draft says so.

## Indent guides are drawn by us, not by the indentation-markers package
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** ink-editor (indent guides)
- **SCOPE:** moderate
- **WHAT:** `@replit/codemirror-indentation-markers` is dropped and the editor draws its own indent guides, giving exact control over horizontal position (#3141) and per-row height (#3143).
- **WHY:** Both defects are unreachable from outside the package. Its background string hardcodes a half-character offset (`${startOffset * indentWidth}` string-concatenated with `".5ch"`), which centres the marker in the character cell while the caret sits at the cell's left edge — so guides read as 3.6px misaligned and no configuration changes it. Its background-size carries only a width, so the guides fill the line box and cannot show the per-row gap the maintainer wants. Overriding either from CSS means restating the geometry, which needs `indentWidth` — and that is not one of the package's options (`highlightActiveBlock`, `hideFirstIndent`, `markerType`, `thickness`, `activeThickness`, `colors`); it derives from the editor's `indentUnit`, so any override silently decouples if that changes. Owning the guides makes the indent unit ours, both defects direct consequences of our own code, and future changes to guide appearance a normal edit rather than a fight with a shorthand.
- **CONSTRAINTS the replacement must respect (all learned the hard way, see the code's comments):** guides must be computed for the VIEWPORT only — the package's active-block highlight was disabled because its block scan walked lazily-computed indentation toward both ends of the document on every cursor move, O(doc) per keystroke on a real file; wrapped lines must not paint a guide beside every continuation row (the existing `1lh` cap exists for exactly that); and the geometry must hold at every editor font size, since that is user-settable.

## Indent size becomes configurable, and everything that indents reads the same setting
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** brink-fmt / brink.toml / ink-editor (indent guides, indentUnit)
- **SCOPE:** moderate
- **WHAT:** The formatter gets a configurable indent size, declared in `brink.toml`. That setting is the SINGLE SOURCE for indentation: the formatter emits it, the editor's `indentUnit` adopts it, and the indent guides position against it. No component may hardcode a width.
- **WHY:** It also settles the indent-guide decision made minutes earlier and makes the alternative retroactively wrong. Overriding the indentation-markers package from CSS required hardcoding `4ch`, which was already a silent-drift hazard against `indentUnit` — with a user-configurable indent it stops being a hazard and becomes a guaranteed break the first time anyone sets a different width. Owning the guides is what allows them to read the configured value at all.
  Stated as "everything that indents reads the same setting" rather than as two separate features, because the failure mode is disagreement: a formatter that writes four spaces while guides are drawn every two is worse than either choice alone, and the bug would look like a rendering glitch rather than a config mismatch.

## Draft status is an icon variant, not a text badge
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Tabs get file icons, and a file's draft status is carried by a
  VARIANT of the ink-file icon — orange, dashed — rather than by a separate
  "DRAFT" text badge. The variant replaces the badge everywhere the studio
  names a file (Binder row, Code tab, Single File header, Continuous section
  heading).
- **WHY:** Icons in tabs were wanted independently, and once every naming
  surface carries a file icon, the icon is already the thing sitting beside
  the name — so encoding status in it costs no additional space and cannot
  drift away from the name it describes. A text badge is a second element
  competing with the filename for the same row, and it reads as an
  annotation ON the file rather than a property OF it. Dashed-and-orange
  says "provisional" without words, which is what a draft is.

## The default indent is 4, and drafts are silent in the status bar too
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** moderate
- **WHAT:** When `[project] indent` is unset the width is **4**, and every
  component reads that one value. The formatter's own `Spaces(2)` default
  goes; the editor's `indentUnit` stops hardcoding four spaces and reads the
  config too. Separately, the status bar's "— file not analyzed" text is
  suppressed for a draft, as the out-of-scope banner already is.
- **WHY:** Of the two candidates, 4 was already what an author SEES — the
  editor indented by four while the formatter wrote two, so picking 2 would
  have changed the more visible half to match the less visible one. It also
  matches the `DEFAULT_INDENT` already declared in `brink-project-config`.
  The cost is real and accepted: `brink fmt` reformats existing projects
  that never set the key. For the status bar: a draft is deliberately
  outside the story, so "not analyzed" reports the intended state as though
  it were a finding — the same noise the `drafts` key exists to remove, in a
  quieter voice. Suppressing the banner but not this left the feature half
  done.

## brink.toml opens in the Settings takeover, in every view
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Clicking `brink.toml` in the Binder opens the **Settings**
  takeover rather than an editor tab, in every editor view. Settings gains a
  "Project" section carrying the whole config document — the structured form
  AND the raw text below it, unchanged from what an editor tab showed.
- **WHY:** Continuous view renders the project's MANUSCRIPT, and `brink.toml`
  is deliberately not part of it — so it had nowhere to go and was simply
  unreachable there (#3166). Routing to Settings answers that once for every
  view instead of per-view, and puts project settings where app settings
  already live. Carrying the raw text along is not optional: the form models
  only `entry`/`conventions`/`dialect`/`types`, and #3015 ruled the text
  below it to be the escape hatch for everything it does not model — which
  now includes `drafts` and `indent`. A form-only Settings section would
  have made those two uneditable from the studio entirely.

## Settings becomes a modal or window before ship
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Before shipping, Settings moves out of the editor takeover and
  into a modal or its own window. Sequenced AFTER the `brink.toml` interface
  is finished.
- **WHY:** Settings is consult-and-adjust, not a place you work — taking over
  the editor area costs the file you were looking at for something you leave
  in seconds. The takeover was the right call while Settings was small
  (2026-08-26, "the editor root area has one occupant"); the `brink.toml`
  interface makes it a substantial surface with its own navigation, which is
  what a modal or window is for. Sequenced second so the surface is built
  once, in its final shape, rather than moved twice.

## TODO notes must be configurable, like any advisory diagnostic
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** compiler
- **SCOPE:** minor/local
- **WHAT:** `[lints]` can override any diagnostic whose default severity is
  not `Error` — including the advisory `Info` tier, of which `E189` (the ink
  `TODO:` author note) is one.
- **WHY:** A TODO note is the clearest case of a diagnostic an author might
  not want reported: it marks work they already know about. `allow` is the
  only lever that turns a diagnostic off, so excluding advisory codes from
  overridability left them with none. The analyzer's own gate always
  accepted them (`validate_lint_code` refuses only `Error`); what excluded
  them was a too-narrow predicate added alongside the settings surface,
  which hid them from the UI while the compiler would have honoured them.

## Settings is a modal with a section rail, Zed-shaped
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Settings opens as a **modal** over the studio, laid out as a
  searchable section rail on the left and ONE section at a time on the right.
  It is no longer a document type and no longer takes over the editor area.
  Sections are registered entries (id, title, keywords, icon, body), not a
  hand-laid-out page.
- **WHY:** Implements the 2026-08-27 modal ruling now that the `brink.toml`
  interface is done. The single scrolling page put the project's lint table
  and the theme picker in one column, so finding anything meant scrolling
  past everything else — the rail is what makes it scale. Search matches a
  section's KEYWORDS as well as its title, so "todo" reaches Diagnostics and
  "theme" reaches Appearance, neither of which is in the section's name.
  Registered sections keep the page from drifting behind what is actually
  configurable, the standing failure of a hand-built settings screen.

## Settings splits App and Project scope, and the pane header owns the title
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** The Settings modal carries an **App / Project** scope switch at
  the top of the rail. Project sections write `brink.toml`; App sections
  write this machine's storage. The `brink.toml` section is renamed
  **General** (the scope is already called Project), the modal's pane header
  is the only `<h2>` — section bodies use subordinate group headings — and
  the inner boxes, dividers and scrollers inside sections are removed.
- **WHY:** Where a setting is written changes what changing it means:
  project settings are versioned and shared with everyone who opens the
  project, app settings are yours and follow you between projects. That was
  previously only a hint inside a mixed Diagnostics section holding both.
  The heading cleanup is the same point — "Project" appeared three times in
  one pane (scope, section, form legend), and one section at a time inside a
  scrolling pane makes every inner box and scroller a nesting level the pane
  already provides.

## Prose checking uses Harper, in its own wasm module, not LanguageTool
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** architectural
- **WHAT:** The editor gets spelling and light grammar checking backed by
  **Harper** (`harper-core`, Apache-2.0), shipped as a **separate
  `brink-prose` cdylib** loaded on demand behind a `ProseChecker` seam —
  never compiled into `brink-web`. LanguageTool is ruled out entirely. A
  sentence-rewrite feature, if ever wanted, is a *separate* seam invoked on
  a selection, not a bigger checker.
- **WHY:** LanguageTool has no offline Rust engine — `languagetool-rust` is
  an HTTP client for the Java server, offline means bundling a JRE, its best
  rules need a ~16GB n-gram set, and its sentence rewriting is a cloud AI
  tier absent from the self-hosted build. Harper is offline and fast, and
  the separate module is forced by measurement rather than taste: a probe
  containing nothing but `harper-core` is **6.15 MB gzipped against
  `brink-web`'s entire 2.61 MB**, and `wasm-opt -Os` moves it 2% because the
  payload is data (an FST dictionary plus POS-tagger weights), not code.
  "Ship it and shrink it later" is therefore not available — shrinking would
  mean forking Harper. `harper.js` costs the same bytes, so the choice was
  never size; it was when the bytes load and whether we can supply our own
  parser. A real `impl Parser` over content spans wins over feeding Harper
  blanked text, because blanking loses true positives on every line holding
  an interpolation.

## Prose checking is scoped by measurement: no per-element rules, dialect day one
- **WHEN:** 2026-08-27
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Prose lints check **content spans only**, with **no
  per-element-kind rule scoping** in v1 (the seam stays able to add it).
  The dictionary is **seeded from the project symbol table, cue names
  included**; ~~an author word list lives in **its own file**, not
  `brink.toml`~~ — **REVISED, see "Prose dictionary lives in `brink.toml`"
  below**. `[prose] dialect` ships day one. Prose lints ride the
  diagnostics channel but are **editor-only** — never emitted by `brink
  compile`, never reaching the oracle ratchet or the editor acceptance gate.
- **WHY:** Each clause was measured against real brink-shaped prose rather
  than assumed. The predicted failure — stylized dialogue drowning in
  squiggles — does not occur: `"Not tonight."` and `"You shouldn't be here.
  Not after dark. Not you."` both produce zero lints, as do cue lines and
  scene headings, so per-element scoping would be budget spent on a problem
  that is not there. The failure that *does* occur is `"Kaelen"` flagged as
  a misspelling suggesting "Karen" — fatal for fiction, and uniquely
  solvable here because a cue line is structural, so the manuscript naming
  its own characters teaches the dictionary for free. `"colour"`/`"harbour"`
  flagged under the American dialect makes dialect unusable-without, not a
  refinement. ~~The word list is separate because it is machine-appended and
  unbounded, and a config file that is mostly word list stops reading as
  configuration.~~ **That clause was never ruled — it was an agent
  recommendation recorded as a maintainer decision, and it contradicted the
  standing ruling above it ("Spellcheck: prose only, squiggles always,
  dictionary in brink.toml"). See the correction entry below.** Editor-only
  is a semantic claim: a misspelling is not a
  compiler claim about the program.

## Debugger epic (#452): v1 DebugInfo contract + D1 design round
- **WHEN:** 2026-08-28
- **PROJECT:** brink
- **SYSTEM:** brink-format + brink-runtime (debugger epic #452, D1/#3179)
- **SCOPE:** architectural
- **WHAT:** (1) **Scope**: full GDB-style debugging (breakpoints, step
  in/over/out, call stack, variable inspection), brink-desktop as first
  consumer; **both source surfaces** (`.ink` and `.brink`) must be
  debuggable. (2) **Carrier**: an in-file, strippable
  `SectionKind::DebugInfo` (tag `0x11`) inside `StoryData`, not a sidecar
  file — confirms Q-R1. (3) **Ship policy**: dev/studio compiles and an
  explicit `brink compile` debug flag emit the section; release export
  omits it entirely (byte-identical release artifacts, no VERSION bump, no
  oracle exposure). (4) **Breakpoint anchors**: range-keyed in v1; the
  `NodeId` column stays reserved for v2 per Q-R4. (5) **VM seam**:
  feature-gated debug hooks on the `effect-trace` paired-cfg-stub pattern,
  not a promotion of `step_once` to public API. (6) **Granularity**: a
  DWARF-`is_stmt`-style statement-boundary flag in v1 (every v1 entry
  flagged, since only `lir::Stmt`/`Container` provenance exists yet, #3183)
  plus a real prologue-end marker (a flag bit on the entry whose own
  offset is the landing point, not a separate field) — expression-level
  rows arrive later as unflagged entries sharing the same shape, no version
  bump, no reader change; `lir::Expr` provenance is now critical-path for
  #3183, not optional. (7) **v1 entry encoding** (`docs/debugger-spec.md`
  §2): `(bytecode_offset_delta, file_idx, range_start, range_len,
  kind_token, flags)`, unsigned-LEB128 varint for the high-cardinality
  per-container entry table (new `codec::write_varint`/`read_varint`,
  scoped to this section only — every other section keeps the format's
  fixed-width house style), sorted ascending by offset per container for
  floor/binary-search lookup, indexed by `container_idx` in lockstep with
  the `Containers` section so a running VM position resolves with a direct
  array index. (8) **File table**: section-local (fresh numbering per
  artifact, not the compiler's project-wide `FileId` space),
  project-root-relative paths, and — RULED — **records which surface
  (ink/native) parsed each file**, because `KindToken.raw` is
  frontend-private and the two `ProvenanceResolver` impls have independent
  `u16` numbering; v1 carries the full `KindToken` (class + raw)
  unconditionally now that surface-per-file disambiguates `raw` on read.
  (9) **Synthetic sentinel**: `Provenance::synthetic`'s `FileId(u32::MAX)`
  (reaching LIR on the root container and `#root-terminus` gather, #3189)
  maps to a reserved `file_idx = 0` in the section-local file table, never
  a real file — chosen over omitting the entry so the "every container has
  a covering entry at offset 0" invariant holds uniformly. (10) **Locals**:
  a per-container `LocalsTable` (`slot: u16 -> name`, matching the real
  `DeclareTemp`/`GetTemp`/`SetTemp` operand width) **includes an optional
  declaring source range in v1** — cheap (one row per declared temp, not
  per instruction) and doubles as the disambiguation key if slot reuse
  across sibling scopes turns out to be real. (11) **Frame semantics**:
  step in/over/out defined per `CallFrameType` across both vocabularies,
  with two explicit non-analogues named rather than faked — a `Thread`
  frame is not returnable-from (ink's own `->->` strips Thread frames
  rather than returning through them), and a condition-park (`until` on the
  native code ground, `~ await`/`~ while await` on the ink surface — both
  lower to the same `AwaitStmt` HIR node) ends the VM turn
  (`Step::Suspended`) with no synchronous "next instruction" to step to
  until `wakeCheck()` next resolves the parked condition true.
- **WHY:** Recorded on issue #3179 to remove `needs-design` from the rest
  of the debugger epic (#452) — every other debugger ticket (D2–D9) should
  be buildable against this contract without a further ruling. The carrier
  and the VM seam, ruled independently by the maintainer, avoid two known
  failure modes: an opcode-interleave carrier would perturb the VM's
  step-limit accounting and could flip oracle episode outcomes (the
  2026-07-19 evaluation memo's risk table, corrected — a new section is the
  *safe* option, not the risky one), and a public `step_once` would put
  debugger instrumentation on the production hot path (`CLAUDE.md`
  "Instrumentation doesn't belong in the production path"). The
  statement-flag-now / expression-rows-later design pays a one-bit cost
  today specifically so the eventual fine-grained table the maintainer
  expects never needs a breaking version bump — the same "no version bump"
  property release-export omission and the `NodeId`-reservation already
  rely on elsewhere in this same design. Naming the two "no honest
  analogue" cases explicitly (Thread step-out, await-park stepping) rather
  than picking a plausible-looking but false analogy keeps the debugger
  from lying to authors about what just happened, which is worse than an
  operation simply being unavailable.

## Debugger epic (#452): D8 debug budget + live-inspector-spec §9 supersession
- **WHEN:** 2026-08-28
- **PROJECT:** brink
- **SYSTEM:** brink-runtime (debugger epic #452, D8/#3186)
- **SCOPE:** moderate — **orchestrator call recorded during PR #3218's fix
  round, pending maintainer confirmation** (spec-drift review findings on
  #3186: the budget semantics were previously recorded nowhere but an
  issue comment, and `docs/live-inspector-spec.md` §9 still read as if it
  contradicted this ruling).
- **WHAT:** (1) **Debug budget**: `Story::debug_run`/`debug_step`/
  `debug_run_watching` (D8, `crates/brink-runtime/src/debug_control.rs`)
  get their own step budget, `DEFAULT_DEBUG_BUDGET = 200_000`, tracked in
  a loop-local variable — entirely separate accounting from the
  production step limit (`FlowInstance::STEP_LIMIT`/`Stats::steps`).
  Debug-hook code never reads or writes `Stats::steps`. Exceeding the
  debug budget is the new public `RuntimeError::DebugBudgetExceeded {
  breakpoint, ceiling }` — never `RuntimeError::StepLimitExceeded`, which
  would misreport which budget fired. Documented in
  `docs/debugger-spec.md` §1.4. (2) **`docs/live-inspector-spec.md` §9
  supersession**: that section's "Pause/step execution control,
  breakpoints... are not designed here" is now stale — D8 shipped exactly
  that, owned by `docs/debugger-spec.md` (§1.4, §4) instead. §9 now
  records the supersession rather than silently disagreeing with the
  debugger epic.
- **WHY:** D8's own PR (#3218) implemented the budget per a 2026-08-28
  decision-comment ruling on issue #3186, but that ruling was never
  transcribed into either owning spec (`docs/debugger-spec.md` §1.4, which
  covers the VM seam this budget belongs to) or this log — an adversarial
  review of the PR flagged both as spec drift: an orphaned ruling
  (comment-only, no spec/log record) and a contradicted spec
  (`live-inspector-spec.md` §9 reading as if it still excluded this
  territory). Recording both here during the fix round keeps the two
  debugger-related specs from disagreeing and gives the budget semantics
  a durable home instead of an issue comment, while leaving the ruling
  itself flagged as **orchestrator-recorded, not yet maintainer-confirmed**
  — a human should still sign off on `DEFAULT_DEBUG_BUDGET`'s specific
  value and the never-touch-`Stats::steps` accounting rule the next time
  this area gets a design pass.

## MPL-2.0 admitted for `colored`, reached through Harper
- **WHEN:** 2026-08-28
- **PROJECT:** brink
- **SYSTEM:** cross-system
- **SCOPE:** moderate
- **WHAT:** `deny.toml`'s `[licenses] exceptions` table admits **MPL-2.0 for
  `colored`**, the sixth per-crate exception. The blanket `allow` list stays
  permissive-only — the licence is still not accepted graph-wide.
- **WHY:** `colored` arrives through `harper-core`, the prose checker's
  engine (#3207): `colored <- burn-tensor <- burn <- harper-pos-utils <-
  harper-brill <- harper-core <- brink-prose`. It is a terminal-colouring
  crate, `optional` in `burn-tensor` and enabled only by that crate's `std`
  feature, so it is almost certainly dead code in the wasm artifact
  `brink-prose` actually ships — and there is no feature flag on this side
  that removes it without patching Harper's own dependency tree. The licence
  reasoning is the 2026-08-15 Tauri ruling's, unchanged: MPL-2.0 is
  file-level copyleft whose obligations attach to modifying and distributing
  those files, which this project links rather than modifies.

  Recorded as its OWN ruling rather than folded into the 2026-08-15 one
  because the two differ in the part that matters: those five are
  unavoidable through a framework the project was already committed to,
  whereas this one came in with a dependency the project CHOSE and could
  have declined by dropping Harper. Reading them as one precedent would
  make "it was transitive" sound like the standard, when the standard is
  the per-crate ruling itself.

## `\}` joins the inline escape set, revising the ruling that declined it
- **WHEN:** 2026-08-28
- **PROJECT:** brink
- **SYSTEM:** prose dialect — the escape set (#3156, §8d.6)
- **SCOPE:** moderate
- **WHAT:** `R_BRACE` joins `is_escapable`, making the inline set
  `\< \{ \# \\ \}` — five, not four. A **bare** `}` in content still
  terminates the enclosing block; the escape is what lets an author opt out
  of that role. The raw-text scanners (`tag()`/`cue_name()`) are
  deliberately NOT changed in the same breath, so `#tag \{a\}` still
  truncates; closing that is its own ruling.
- **WHY:** This REVISES the earlier confirmation that `\}` should not be an
  escape (§4.7b, issue #1883 item 2). That ruling reasoned "`}` is not a
  member of the set, so there is no `\}` is a literal close-brace ruling to
  protect" — which is self-referential, and did not weigh what the set's
  closure actually cost. Measured while fixing #3156: a `}` ANYWHERE in a
  content line terminates the enclosing block, so with `\}` rejected there
  was no spelling — escaped or bare — that put a literal `}` into native
  prose. The character was unwritable.

  The failure mode was the expensive kind. `\{` works, so an author has
  every reason to believe `\}` does; instead the flow was silently
  truncated, the divert after it fell outside the flow, and the only
  diagnostic pointed at the file's REAL closing brace ("unexpected token at
  top level") rather than at the typo. A wrong program, not a wrong colour.

  The scanners are excluded from this ruling rather than swept along
  because their exclusion, though reasoned from the same now-false premise,
  was confirmed deliberately and twice — reversing that deserves its own
  decision rather than arriving as a side effect of this one.

## Prose dictionary lives in `brink.toml`; casing stays literal for now
- **WHEN:** 2026-08-28
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** The author's custom prose dictionary lives in `brink.toml`, not
  a dotfile — **reaffirming** the original ruling in "Spellcheck: prose
  only, squiggles always, dictionary in brink.toml" and **revoking** the
  contradicting clause in "Prose checking is scoped by measurement". The
  implemented `.brink-dictionary` file is to be replaced by a `[prose]
  dictionary` key. Separately: dictionary matching **stays literal**
  (exact-case) for now, to be revisited after real use rather than designed
  up front.
- **WHY:** On the storage question the rationale was already recorded and
  was never displaced: the dictionary is a fact about the manuscript, so it
  belongs with the project, shared by collaborators and surviving a fresh
  clone. The "machine-appended and unbounded" counter-argument was an agent
  invention, and the concrete cost of the dotfile was immediate — "Add to
  dictionary" appeared to do nothing, because it wrote somewhere with no UI
  surface and no presence in the config the author reads.
  On casing, the maintainer declined to decide from the measured matrix
  (title case would give correct proper-noun behaviour via Harper's
  `is_proper` metadata; lowercase silently disables capitalisation checks)
  on the grounds that the right expansion is easier to see after living with
  the literal behaviour than before. Literal is the honest default: it does
  exactly what the author typed and claims nothing further.
- **NOTE (process):** The contradicting clause is the second time this log
  has carried an agent recommendation styled as a maintainer ruling. A
  clause in an entry is only a decision if the maintainer said it.

## Prose checking: button up the ink surface first, native later
- **WHEN:** 2026-08-28
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** For prose checking, the **ink** surface is finished first and to
  a "100% buttoned up" standard; the native surface follows later. Concretely
  this parked #3252 (cue names are never harvested on the native surface,
  because `@[convention]` claims populate no `LineContext.dialect`) and
  admitted the two-part ink cue fix instead: seed cue names in title case,
  and exclude character-cue lines from prose ranges.
- **WHY:** The two surfaces fail for unrelated reasons and only one of them
  was blocking use. The claiming pass does not run on ink at all —
  `@[convention]` handlers live in a `.brink` conventions module and
  `brink-syntax` has no notion of them — so the native defect could be
  separated cleanly rather than held as a prerequisite. On ink the
  classification already worked and the whole remaining gap was casing: the
  cue teaches `GRISWOLD`, the prose writes `Griswold`, matching is literal.
  Fixing that finished a surface rather than half-fixing two.
- **NOTE:** This is a sequencing decision about prose checking, not a
  reversal of "the project's center is the NATIVE surface". It stands
  because the ink gap was small, measured, and in the way.

## Line-variant groups stage 3: combo kinds keep the lift; mixed lines share alternative state
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** compiler (hir normalize / lir lower / codegen)
- **SCOPE:** moderate
- **WHAT:** Two rulings settling #3275's design questions. (1) Combo-kind
  lines (`shuffle|once`, `shuffle|stopping`) and structural-branch lines
  **keep the cartesian lift** rather than moving to the shared-inline
  fragment path: each rendering stays a whole line in the line table — a
  translation unit and a VO slot. (2) Mixed lines (a stateful alternative
  beside an inline conditional or other unclaimed construct) **pin ink's
  shared-state semantics**: the cloned alternative keeps one container id
  across every lifted branch, so it advances once per line view whichever
  branch renders. The enabling move is stamping container ids on pristine
  HIR before normalization (both compile roads), with clones deriving ids
  deterministically and sharing revoked per lift level when a branch fails
  to reassemble into a claimable variant line.
- **WHY:** On (1) the maintainer's question was "what's the benefit of not
  lifting them?" — and the only benefit was deleting normalize code, while
  the cost was per-line VO association on those lines. Whole-line entries
  won. On (2) the per-branch private copies produced `p` twice in
  `{n > 1: late|early} {&p|q}` where ink documents one shared advance;
  the ruling pins the documented semantics (permanent `variant_flip`
  case). One-time golden churn was accepted for the stamping move; the
  parity design made it moot — zero churn, ratchet unmoved at 5608.
- **NOTE:** Stage 3c's residue analysis concluded the lift machinery
  retires **nothing**: with (1) ruled, every arm of `try_lift_inline`
  (once→stopping synthesis, synthesized else, cartesian recursion,
  sharing revocation) is reachable through combo-kind, conditional-
  bearing, or structural-branch lines. #3275 closes with the lift as the
  permanent fallback model, documented in `normalize.rs`'s module doc.

## Anonymous-id counters go weave-block-local; labels anchor their subtrees
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** compiler (hir stamp)
- **SCOPE:** moderate
- **WHAT:** Two rulings on anonymous-container identity, taken together as
  one one-time renumbering. (1) The conditional/sequence counter (`b-N`/
  `s-N`) stops being knot-global and adopts the scoping the choice/gather
  counters already had: fresh wherever the walk enters a body whose scope
  path narrows uniquely (choice bodies, sequence branches), threaded
  through whatever continues the enclosing weave (continuations,
  conditional branches). An insertion now shifts anonymous ids only for
  later siblings in the same weave block, never across the whole knot.
  (2) A `(label)` anchors its subtree: a labeled choice's or labeled
  block's descendants scope under `#lbl:{label_id}` instead of the
  positional path, so naming a container insulates everything inside it.
- **WHY:** The global counter existed to keep two independent walks (HIR
  stamping and the LIR planning pass) synchronized; the pristine-HIR
  stamping move (#3283) reduced identity to a single walk and made the
  constraint obsolete. Block-local counters shrink E157's exposure and
  the save-invalidation blast radius, and label anchoring makes E157's
  own suggested fix maximally effective. The cost — existing saves'
  anonymous visit states detach once (`LoadReport::
  anonymous_states_dropped`) — was accepted as a one-time break; named
  state is unaffected. The re-scoping also fixed a real miscompile guard
  trip: a block-level `{stopping:}` with a choice in two branches
  stamped both `{wrapper}.c-0` (branch bodies recursed under the
  wrapper's scope with fresh counters), tripping E060 on legal ink;
  branch bodies now recurse under the branch's own indexed path.
- **NOTE:** The LIR lowerer's twin display-name counter keeps its global
  numbering — post-#3283 it feeds container display names only, so
  stamped id paths and display names can drift apart in dumps. Accepted;
  deriving names from stamped ids is a possible future cleanup.

## Config list settings follow the dictionary's shape, and globs show what they matched
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio-settings
- **SCOPE:** moderate
- **WHAT:** A `brink.toml` list-valued key gets its Settings surface in the
  shape the prose dictionary established — an add field, removable rows,
  and pure `source -> source` transforms in `studio-store` rather than
  edit logic living in the panel. `[project] drafts` is the first to
  follow it. Where the list holds globs rather than literals, each row
  additionally reports what it currently matches, distinguishing three
  states: drafts produced, files matched that the story still reaches
  (so not drafts), and nothing matched.
- **WHY:** The user asked for drafts "as a list like dictionary, but with
  nicer globs". A glob is not a literal: an author cannot tell by reading
  it back whether it worked. Two ordinary mistakes are invisible in a
  bare list — a typo matching nothing looks exactly like a working
  pattern, and a pattern naming a file the entry still reaches produces
  no draft at all under "reachability wins" (2026-08-27), so it appears
  to do nothing for no stated reason. Showing the match set turns both
  into something the author can see and fix. The dictionary's shape is
  reused because these panels are where an author goes to confirm an
  action elsewhere in the app worked, and two list surfaces that behave
  differently would undercut that.

## Debugger UI: debug info on by default for studio compiles; pause is a first-class verb
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio + editor pipeline (debugger epic #452 / D9, #3249, #3230)
- **SCOPE:** architectural
- **WHAT:** (1) **All studio compiles emit the `DebugInfo` section by
  default.** The per-session mechanism #3229 built (`setDebugInfoEnabled`,
  the salsa cutoff, recompile-on-toggle) stays exactly as built, but its
  default flips to **on**; an App-settings toggle is the opt-out for
  projects where the emission cost is noticed. Release export still omits
  the section — the ship-policy ruling does not move. This SUPERSEDES the
  #3229 consequence "not on by default" (the mechanism ruling stands; the
  default does not). (2) Because the running story's bytes therefore
  always carry debug info, **there is no "debug mode"**: #3249's
  enter/leave lifecycle (recompile + consented restart) is moot. A gutter
  click mid-play binds a breakpoint immediately; stepping can begin from
  wherever the story is, with no artifact switch and no restart. (3)
  **Pause/resume is a first-class Player verb**: a pause control and
  breakpoints hit during normal play suspend the story into the debugger;
  Continue resumes normal play. The interleaved play↔debug path
  (transcript/choice coherence when mixing the production advance loop
  with `debug_run`/`debug_step`) is REQUIRED proof for the drive-loop
  work, not an optional scenario.
- **WHY:** The maintainer's framing: the Player should compile with debug
  info by default "so we can quickly/easily start debugging without
  interrupting the play experience." The #3229 default-off ruling was
  motivated by an asserted (never measured) per-keystroke cost; emission
  is one linear LIR walk writing varints, compiles are debounced and on
  the worker, and the first PR flipping the default must measure it in
  the perf HUD. What default-on buys is structural: #3249's hard problem
  ("enabling debug mid-playthrough produces a different artifact, so it
  must restart") disappears, and "debug from here" — explicitly deferred
  as too large — becomes free, because there is no artifact switch and
  therefore no container-identity remapping. One-time cost: the editor
  acceptance gate re-baselines in lockstep (#3230 names this).

## Debugger UI: session-only debug state; breakpoints persist per project
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio (debugger epic #452 / D9, #3249)
- **SCOPE:** moderate
- **WHAT:** No debugger state beyond breakpoints survives a studio
  reload: paused-ness, frame selection, and the run/step transcript are
  session-ephemeral. **Breakpoints persist per project** (they are cheap,
  inert markers), alongside the other per-project layout state. The
  App-settings debug-emission opt-out persists as a setting, like any
  other setting.
- **WHY:** Answers #3249's second maintainer question. With debug info on
  by default there is no mode to persist; breakpoints are the only state
  an author would miss the next morning, and persisting them matches
  every IDE convention while costing nothing when no session is running.

## Debugger UI round = the Player half of #3199; StateView is replaced, not extended
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio (debugger epic #452 / D9, #3199)
- **SCOPE:** architectural
- **WHAT:** (1) This design round **folds the Player rebuild in**: the
  Player is redesigned with debugging as its organizing feature, rather
  than designing the debugger against today's Player and rebuilding it
  again later. The Story Graph half of #3199 stays a separate, later
  round — answering #3199's own "one round or two" question as two. (2)
  The **State View is a redesign/replacement**, not a minor modification:
  it is rebuilt as the debugger's inspection surface (interactive call
  stack with frame selection, locals-first variables, a breakpoints
  section), and the Player toolbar is rebuilt to carry the transport
  (pause/resume/step) controls.
- **WHY:** #3199 itself warns that a Player rebuild designed without
  reference to the debugger "would likely be redesigned again
  afterwards"; the maintainer chose to take that head-on rather than
  design twice. The StateView call reflects that the existing component,
  while already rendering call stack + locals (#3140), was built as a
  passive inspector — frame selection, breakpoint management, and
  stop-reason presentation are structural additions, not decorations.

## Player rebuild: paced auto-reveal as a transport button; tag toggle; visible line rows
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio (Player rebuild — debugger UI round, #452 D9 / #3199)
- **SCOPE:** moderate
- **WHAT:** (1) **Auto is a toggle button in the transport** (double-arrow
  fast-forward icon, pressed state = on), replacing the checkbox. (2) An
  **App setting governs auto-reveal pacing**: even when the runtime
  delivers a turn's lines as a chunk, auto playback can reveal them **one
  line at a time in rapid succession** rather than as a block — the pacing
  is a playback concern in the Player, not a runtime delivery change.
  (3) A **toggle shows the tags** delivered with each line
  (`OutputLine.tags`), rendered as muted per-line chips. (4) The
  transcript gets a **subtle row-based highlight** so the author can see
  what constitutes a delivered line and where its boundaries are —
  subtle but informative.
- **WHY:** Maintainer direction on reviewing the first canvas. The pacing
  point is explicitly about the *playback* of auto mode ("maybe the
  runtime delivers in a big chunk, but the playback of auto mode should
  still do one at a time") — the reading experience should keep line
  rhythm even when the engine batches. Tags and line boundaries are
  authoring visibility: a line is the runtime's delivery unit (and the
  debugger's stepping unit), so the author needs to see its edges and
  its metadata without leaving the Player.

## Breakpoints share the play gutter's column
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** editor-ui (debugger UI round, #452 D9 / #3233)
- **SCOPE:** moderate
- **WHAT:** The breakpoint glyph renders **in the same gutter column as
  the "play from here" ▶**, not in a separate host-gutter column. Their
  placements rarely conflict — ▶ appears only on hovered *header* lines
  (knot/stitch), breakpoints live on statement lines. Where both apply
  (a breakpoint on a header line), the hover glyph stays ▶ and the
  gutter's context menu carries "Set breakpoint here". The paused-here
  execution arrow overlays the same column.
- **WHY:** Maintainer reasoning: "a place where a breakpoint can go and
  a 'play from here' don't overlap much." Avoids growing the gutter
  column count, consistent with the detached-gutters ruling (#3119)
  which preferred detaching gutters over thinning their content.
  Implementation consequence: breakpoint markers merge into the play
  gutter's column (extend `play-from-here.ts`'s gutter or point the
  host-marker rendering at its slot) rather than mounting
  `hostGutterExtension`'s parallel column; the host-gutter marker model
  can still carry the data.

## Play is stepping: no auto-start, and the editor tracks the live line while playing
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio + editor-ui (debugger UI round, #452 D9 / #3199)
- **SCOPE:** architectural
- **WHAT:** (1) **The story does not play by default.** Opening the
  studio/Player never starts a session; the Player opens idle ("ready")
  and Run compiles and begins one. (2) **While a session is running, the
  editor reflects it with the per-line treatment continuously** — the
  execution highlight follows each delivered line as it is revealed
  (paced or manual), not only when paused. Since playback advances one
  line at a time anyway, "debugging" and "stepping through the story"
  are ONE experience: auto mode is automatic stepping, Continue is a
  step, pause/breakpoints just stop the advance — the visualization is
  already live either way.
- **WHY:** Maintainer direction on the canvas: "once the player is in
  'play' mode and we have a live session running, the editor should
  reflect that with the per line treatment... we can kind of unify the
  'debugging' and 'stepping through the story' experience." This
  collapses what would otherwise be two visual systems (a play mode with
  no source feedback + a debug mode with highlights) into one, and makes
  the always-on debug-info ruling pay off during ordinary playtesting,
  not just at breakpoints. Color language: live line = success tint,
  paused = warning tint + arrow, selected frame = accent tint + hollow
  arrow, parked = info dashed. Degraded suppression applies to all of
  them identically. The editor never auto-scrolls to follow playback by
  default (typing while a story runs must stay hostile-free); clicking
  the status chip reveals the current line — whether a follow-execution
  toggle is wanted stays an open knob for the build.

  The granularity ladder behind the unification (maintainer, same
  session): "the author wants to debug their story and logic line by
  line anyway, and the instruction and code level 'step line' is just
  for even more granular and detailed investigation or when the
  programmer needs to step in to help." Three tiers: (1) the **story
  line** (delivered `OutputLine`) is the author's primary stepping unit
  — the existing reveal-next IS the first-class step, and auto mode is
  it self-advancing; (2) **source-statement** step into/over/out is the
  deeper logic-investigation tier; (3) **instruction-level** `stepi` is
  the programmer-assist tier — which is why it lives in the Program
  Explorer, not the Player toolbar.

## The Debugger panel owns the flow list, not the status bar
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio (debugger UI round, #452 D9 / #3223)
- **SCOPE:** moderate
- **WHAT:** The list of open flows/sessions lives in the **Debugger
  panel** as its own section (above Frames), not in the bottom
  rail/status bar — the status bar's `SessionPicker` retires. Selecting
  a flow scopes Frames/Variables and the transport to it; a parked flow
  shows its "parked — resumes here" state in this list (and its Frames
  view shows the resume frame), never as a pseudo-frame in another
  flow's call stack. This is F12's selection surface answered: when
  #3223 lands multi-flow runtime support, the panel's Flows section is
  where the author picks the flow being debugged.
- **WHY:** Maintainer direction. Flow selection is debugger context —
  it belongs beside the call stack it scopes, not in global chrome; the
  status bar keeps only the one-line story status. Call stacks are
  per-flow, so the flow list is also the only honest home for a parked
  flow's resume state.

## Choice-point visualization, runtime-value hover, Player appearance settings
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio + editor-ui + runtime seam (debugger UI round, #452 D9)
- **SCOPE:** moderate
- **WHAT:** (1) **The Player gets its own appearance settings**, starting
  with font size — its own knob on the `--bs-editor-font-size` precedent
  ("make the text bigger about the thing you read is a different request
  from make the UI bigger"), separate from the app type scale. (2) **When
  the story waits on a choice, the editor lights the choice point**:
  every presented choice's line is highlighted, and authored choices
  that were NOT added to the block render dimmed **with the reason**
  (condition evaluated false, once-only exhausted) — driven by runtime
  state, not editor-side guessing. This requires a new runtime/bridge
  seam: a **choice presentation report** (per choice point: the
  candidates evaluated, presented/rejected, and the rejection reason) —
  filed as its own runtime ticket alongside #3223. (3) **Editor hover
  shows runtime variable values during a live, in-sync session** —
  globals always, frame locals while paused — layered onto the existing
  hover, suppressed under `sessionDegraded`.
- **WHY:** Maintainer direction on the canvas. Choice dimming turns the
  commonest story-logic question — "why didn't my choice appear?" —
  into something read off the source instead of debugged by
  experiment; runtime hover makes play mode an inspection surface
  without opening the panel. Both are the always-on-debug-info ruling
  paying off during ordinary playtesting.
- **NOTE (revised same session):** the "choice presentation report"
  runtime seam in (2) is NOT needed — maintainer: "we can look at visit
  counts." Rejection reasons derive by elimination from surfaces that
  already exist: the presented set (`DebugState.choices`) + visit counts
  (`DebugState.visits` — once-only exhaustion IS the body container's
  visit count) + #3234's anonymous-container ids in the overlay
  projection for the identity join. The only possible bridge change is
  widening the visits snapshot if it filters out anonymous choice-body
  containers — W11 verifies that first. The condition-failed label is
  by-elimination (a catch-all); edge cases live in W11's proof list.

## Runtime save/load for testing: idle-Player surface, location by App setting
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio + runtime consumer (debugger UI round, #452 D9 / #57)
- **SCOPE:** architectural
- **WHAT:** (1) The author can **save and load runtime state the way a
  game would** — visit counts, globals, position: the story's durable
  state, NOT internal/ephemeral runtime state — to get to a particular
  place in the story and keep testing from there. The save payload is
  the runtime's existing `SaveState` boundary, not a new format. (2)
  The controls are a **separate surface in the Player, present when it
  is idle**: the idle body shows Run-from-start plus the saves list
  (load one to start there); saving the current session is available
  while one runs. (3) **Where saves live is an App setting**: "keep
  saves on my machine" (private app-data folder, per project) versus
  "project saves" (inside the project tree, shareable/committable),
  extensible to more locations later. (4) Loading against a newer
  compile surfaces the runtime's `LoadReport` in the UI (e.g.
  "3 anonymous visit states dropped") rather than loading silently —
  the #3283/#3234 identity work is what makes these saves survive
  ordinary editing at all.
- **WHY:** Maintainer direction on the canvas: authors need
  checkpoint-style iteration ("get into a particular place and keep
  testing from there"), and the idle Player body is otherwise empty —
  it becomes the natural launcher. This is the "newer, larger ask" #57
  was already flagged to be re-scoped against; value *editing* stays
  out of scope here. Machine-local is the proposed default location
  (does not dirty the repo unasked) — flagged as a knob, not ruled.

## Hot reload: edits during play reach the running Player
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio + runtime consumer (debugger UI round, #452 D9)
- **SCOPE:** architectural
- **WHAT:** Content edits made during play mode must be reflected in
  the running Player as immediately as possible — "hot-reloading as
  much as we possibly can by any means necessary." On every successful
  studio compile, the live session **migrates to the new program
  automatically**: snapshot the durable state (the same `SaveState`
  boundary as F17), swap programs, reload the state, re-anchor
  breakpoints, and surface the `LoadReport` inline when anything
  dropped. Migration lands at turn boundaries (waiting on a choice,
  paused, parked, between paced reveals); a failed compile keeps the
  old program running with the error surfaced. The degraded
  "out of sync" state becomes the **fallback** — compile failing, or a
  migration that cannot preserve the current position — not the steady
  state after every edit. This SUPERSEDES the live-inspector-spec §5
  posture in which every edit degrades the session until restart.
- **WHY:** Maintainer: "it's just a fact of how the player/editor need
  to interact." The reason it doesn't happen today is design, not
  accident — the studio was built to degrade honestly rather than
  guess — but the pieces that make honest migration possible now
  exist: the `SaveState` boundary, `LoadReport`'s explicit
  dropped-state accounting, and the #3283/#3234 block-local identity
  work whose whole point was shrinking the save-invalidation blast
  radius of an edit. Hot reload composes them; where they can't
  preserve state, the old degraded path remains the honest fallback.

## Idle launcher: save-location doors + play-from-anywhere typeahead
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio (Player rebuild — debugger UI round, #452 D9)
- **SCOPE:** moderate
- **WHAT:** (1) The idle Player's saves present **exactly like the
  landing screen's Recent list** (maintainer screenshot, this session):
  two stacked sections, each an uppercase cap label over the bordered
  recents-style row list — where the landing says "RECENT", the Player
  says **"PROJECT"** (project saves) and **"THIS COMPUTER"** (machine
  saves). Rows follow the recents anatomy: small mono chip, name,
  right-aligned muted context. Both locations first-class and always
  visible; the App setting picks the default target for new saves
  rather than hiding a location.
  (2) Next to "Run from the start" sits a **combobox/typeahead search
  over knots and stitches, with file locations as context**, to "play
  from there" — the launcher form of the existing play-from-here start
  path (#186), reusing the symbol/outline query and the quickpick
  idiom. (3) Each save offers **two actions: Load and Fork**. Load
  attaches the session to the slot — "Save state" writes back to it,
  like continuing a save file. Fork starts from a copy — the session is
  not attached, and the next save picks a new location/slot, leaving
  the checkpoint untouched.
- **WHY:** Maintainer direction. Visual continuity: the launcher is the
  Player's landing screen, so it borrows the landing's established door
  vocabulary. The typeahead makes "get me to this scene" a typed
  action instead of a hunt through gutters or the Binder — together
  with saves it makes the idle Player the place testing starts from.
  Load/fork separates the two testing motions: continuing a checkpoint
  versus branching experiments off one without clobbering it.

## Debugger panel: live value editing, Watch mini-REPL, break-on-write
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** studio + runtime bridge (debugger UI round, #452 D9 / #57 / #411)
- **SCOPE:** moderate
- **WHAT:** (1) **Locals and globals are editable from the Debugger
  panel** — click the value, inline mono input, Enter commits, Esc
  cancels, type-checked against the value's current type; **v1 edits
  scalars only** (int/float/bool/string; lists and structs stay
  read-only until a value-editor design of their own); **editing is
  allowed only while paused**. Globals commit through the existing
  `Story::set_variable` (needs wasm exposure); locals need a new
  debug-seam set-temp-in-frame call. Edits must go through the
  dirty-marking write path so watchpoints and parked-condition wake
  checks observe them. No undo — F17's fork-a-save is the safety net.
  (2) **A Watch section ships as the full mini-REPL**, not
  expressions-only: arbitrary typed expressions AND divert/content
  fragments with side-effect-proof transcript previews, re-evaluated at
  each stop. Verified wired end-to-end, not a #411 build: F4.1–F4.3
  (`Speculation`/`KindTieredHandler`, web `speculate()`) plus F5.1
  tier-1 fragment evaluation (`compile_fragment` mechanism B, cached
  per (checksum, fragment, kind)) are all landed with wasm tests
  through the real `evaluate()` export — the maintainer's "check
  again, i think you can do a full mini-repl easily" was correct, and
  the scratch-eval-spec's "never landed" provenance note is stale for
  everything this needs. (3) **Break-on-write** via the variable-row
  context menu ("Break on write"), listed in the Breakpoints section
  with a distinct glyph — D8's `WatchpointObserver`/
  `debug_run_watching` already provide the runtime half.
- **WHY:** Maintainer direction with explicit alignment round. This
  re-scopes the editing half of #57 INTO the round (the save/restore
  half landed as F17). Paused-only editing was chosen over
  globals-anytime for the simpler mental model, accepting the loss of
  live wake-condition poking while parked.

## Parked/awaiting debug positions report the resume point and call site, tagged
- **WHEN:** 2026-08-29
- **PROJECT:** brink
- **SYSTEM:** runtime (debugger epic #452 / #3225 — the ruling D9's W5 was blocked on)
- **SCOPE:** moderate
- **WHAT:** While a flow is **condition-parked** (`Step::Suspended`),
  `Story::debug_position()` and the frame reads report the **resume
  point** — `(continuation container_idx, offset 0)`, resolving just
  past the park statement per `docs/debugger-spec.md` §2.6 — carried
  with an **explicit parked tag**, never as a live position. While
  **awaiting a deferred external** (`StepOutcome::AwaitingExternal`),
  they report the **calling frame's call site** with a *distinct*
  awaiting-external tag (resumption is host-driven, not
  condition-driven; the External frame stays opaque). Rejected: the
  pre-park position (lies about the future — that statement never runs
  again) and "no position" (starves every consumer of the ruled
  "parked — resumes here" treatment). The tag is API-level so no
  consumer can honestly render "currently at". Implementation rides
  with the #3215 `#[non_exhaustive]` hygiene fix (the shape change is
  otherwise breaking), and sequences consciously against FS-3r (#980).
  Recorded in `docs/debugger-spec.md` §4.1.
- **WHY:** The split-at-park compilation model means "where the flow
  stopped" and "where it resumes" are different containers — there is
  no single honest current position, so the API reports the one
  forward-looking consumers need and marks what it is. This unblocks
  W5 (#3298)'s park presentation and closes #3225's design ask.

## Continue runs to the next content line and resumes play; step verbs are the statement tier
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** debugger-ui / runtime debug seam
- **SCOPE:** moderate
- **WHAT:** The Player's Continue verb (and the reveal-while-paused
  click, which collapses into it) advances the VM **until the next
  content line is delivered** — a committed output line — or a
  breakpoint/choices/park/terminal stops it early. On an ordinary
  content-line stop, Continue **resumes normal play**: the paused state
  clears (band back to live green, chip gone, step buttons disable);
  every subsequent reveal stays breakpoint-bounded via the W5 drive
  loop, so nothing is lost in safety. Continue must NOT batch free-run
  to the next choice (today's `debug_run` mapping) and must NOT halt at
  every source statement (today's `debug_step_line("over")` mapping —
  which made an author click through each `~` statement to reach
  content). Step Over/Into/Out remain statement-granular toolbar verbs
  for the programmer tier — the granularity ladder's middle tier, as
  ruled. This REVISES the W5 pin "a paused reveal line-steps and stays
  paused" and the spec's transport-table Continue row. Implementation:
  one runtime verb whose stop predicate is "a content line committed",
  built by extending #3321 (the commit-lag follow-up) since the
  glue-boundary/commit predicate is the same design; ships as the next
  PR in the debugger-ui stack (before W7), and W7 re-points the
  transport at it. Statement-tier steps keep the commit lag for now
  (the W6 highlight makes it legible); that residue stays open on
  #3321.
- **WHY:** Play-is-stepping makes "resume normal play" mean the reveal
  cadence, not a classic F5 free-run — and the author-tier advance must
  be measured in story lines, not source statements: grinding through
  logic lines one click at a time is the programmer tier leaking into
  the author experience. Because the verb runs *through* the commit
  boundary, the delivered line lands at the stop — solving the felt
  half of the #3321 lag in the same design.

## The Player toolbar carries a Stop button
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** debugger-ui / Player
- **SCOPE:** minor/local
- **WHAT:** The Player toolbar gains a Stop button (filled square, after
  Restart, disabled while idle) dispatching `story.stop` — added to the
  ruled toolbar set (Run · Restart · Stop · Auto · transport · tags ·
  chip · Maximize).
- **WHY:** With the ruled "no auto-start", Stop is the route back to the
  idle launcher (and W14's saves screen) — without it the only exits
  from a session were Restart or the palette.

## Saves carry the transcript; loads and reloads stop dropping it
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** debugger-ui / Player saves
- **SCOPE:** moderate
- **WHAT:** The checkpoint payload includes the TRANSCRIPT (the studio's
  `TranscriptLine[]`, stored beside the runtime `SaveState` — the
  runtime boundary is unchanged), and Load/Fork restore it. The
  hot-reload migration likewise carries the live transcript through
  instead of clearing it.
- **WHY:** Too many operations currently drop the transcript (loads,
  reloads, restores) — the author loses the story-so-far exactly when
  they're trying to keep testing from a point.

## Player toolbar sections collapse to menus one at a time under pressure
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** debugger-ui / Player toolbar
- **SCOPE:** moderate
- **WHAT:** When the Player is too narrow for the full toolbar, its
  sub-sections collapse INTO overflow menus one group at a time (rather
  than wrapping, truncating, or shrinking) — transport cluster first,
  then the secondary controls — until the toolbar fits.
- **WHY:** The Player is often a narrow split; the earlier width
  complaint showed the toolbar crowding out the chip. Collapsing whole
  groups keeps every verb reachable at any width.

## Fast-forward is a one-shot continue-maximally, not a mode toggle
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** debugger-ui / Player transport
- **SCOPE:** moderate
- **WHAT:** The FF button no longer toggles a persistent auto mode: one
  click runs the story to the next stop (choices/breakpoint/terminal) —
  ink's `ContinueMaximally` shape — honoring the paced/all-at-once App
  setting for delivery, then reverts to single-line reveals. Equivalent
  to enable-auto → continue → re-disable-auto, as one gesture.
- **WHY:** A sticky auto mode changes what every later click means; a
  one-shot verb is predictable and matches the C# API authors know.

## Studio saves carry the structural transcript and re-render it
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** editor-ui / runtime seam
- **SCOPE:** moderate
- **WHAT:** The transcript a save carries (and the one a hot reload preserves) is the *structural* transcript — the runtime's `OutputPart` stream (`LineRef`s + slots, the `.brkt` content model) — not resolved text. Loads, forks, and hot reloads re-render it against the CURRENT program's line tables, so editing the script and reloading re-renders the story-so-far with the updated text.
- **WHY:** The runtime was built for exactly this ("defer resolution to the latest useful point"; `.brkt` + `.inkb` re-renders without re-executing). Storing resolved text freezes the prose at save time and goes stale the moment the author edits a line.

## Studio-side saves and transcripts serialize as JSON, not binary
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Everything the studio persists for the author (save slots, their transcripts) serializes as human-readable JSON. The binary `.brkt`/`.inkb` codecs remain the shipping-game formats; the wasm boundary exposes the structural transcript as JSON for the studio.
- **WHY:** Maintainer: "binary formats are for shipping games" — authoring-side artifacts should be as human-readable (inspectable, diffable, hand-fixable) as possible.

## Dead-end menu items gate on capability, not notify-on-click
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** minor/local
- **WHAT:** "Reveal in Program Explorer" (and by extension any context-menu item whose action can only work under a live session) is *omitted* from the menu when no session can answer, rather than shown and failing with a notification on click. Implemented as a per-open presence predicate (`canRevealInstructions`) the host wires to session state.
- **WHY:** The source→address resolver runs through the live session's program; with no session the item is a guaranteed dead end. An item that can never work is worse than no item.

## A rebound key displaces the old owner, and says so
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** studio-settings
- **SCOPE:** moderate
- **WHAT:** When an author binds a chord already held by another command,
  the studio saves the new binding, takes that chord off the previous
  owner, and names it in a warning shown before the save. It does not
  block the save, and it does not allow two commands to hold one chord.
  A displaced command loses only the colliding chord, keeping its others.
  Commands keep MULTIPLE bindings throughout — the keymap UI edits the
  whole set as chips rather than a single primary. Keymaps stay app-scope
  (`localStorage`), not `brink.toml`: per-machine, not carried with a
  project.
- **WHY:** `Keymap.byChord` is a `Map<chordId, commandId>`, so two
  commands holding one chord means the last registered silently wins and
  the other is dead with nothing reporting it. Of the three options,
  displacing is the only one matching what the engine actually does —
  blocking is safe but obstructive, and allowing duplicates would let an
  author configure something that quietly does not work, which is the
  existing defect rather than a fix for it. The multi-binding rule exists
  because several commands ship two or three defaults specifically to
  dodge browser-reserved chords (#107); flattening them in the editor
  would re-break what those alternates were added to fix.

## Probe-found edge cases become permanent C#-oracle corpus cases
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** test-corpus
- **SCOPE:** moderate
- **WHAT:** Every hand-minimized edge case found by the gen-expressions
  generator, or by reference-differential probing against the C# ink
  runtime, gets turned into a permanent case under `tests/` with a real
  C#-oracle golden (`oracle/*.oracle.json`, generated via `tools/ink-oracle`
  — never hand-written), not just fixed and forgotten. This applies both to
  cases brink already handles correctly (which raise
  `RATCHET_EPISODE_COUNT`) and to cases brink is known to mismatch (which
  are added anyway, documented as expected mismatches, and excluded from the
  ratchet delta until fixed).
- **WHY:** Maintainer directive: "for these weird edge cases, we absolutely
  super need to create new oracle tests that fully cover this forever." A
  fix without a locking regression test is not durable — the next
  refactor of the lowering/codegen layers these edge cases exercise (nested
  gathers and their fallback choices, #3383; multi-conditional lifting,
  #3386; sequence sharing across lifted branches, #3275; evaluation-order
  of lifted function calls, #3395) can silently reintroduce the bug with
  nothing in CI to catch it.

## Uninitialized `~ temp` reads play, and warn twice
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** compiler+runtime
- **SCOPE:** moderate
- **WHAT:** A `~ temp` used on a path its declaration does not dominate is
  handled in both halves of the pipeline, not one. (1) The compiler emits
  `E193`, a warning-level, `[lints]`-overridable diagnostic naming the use
  site and the declaration, for each of three shapes: a sibling choice
  branch, a gather reached before the declaring branch, and a read written
  textually ahead of the declaration. (A fourth shape this entry originally
  listed here — a stitch referencing a temp declared at its knot's root —
  turned out, on PR #3369's review, not to be a dominance question at all:
  see "Compat-deny diagnostic tier" below, which supersedes this entry for
  that one shape.) (2) The runtime reads an uninitialized temp slot as the
  typed default (`0`, which is also `false`) and reports a runtime warning
  through the diagnostics/output channel, instead of pushing a `Null` that
  faults on the next operator. (3) Alongside those, every such reference —
  textually-preceding reads and stitch references included — resolves to
  the temp's own slot, never to a phantom global that fails at link with
  `unresolved global` (issue #3362; this resolution behavior is unaffected
  by the compat-deny split — a stitch's reference to its knot's temp still
  resolves to the real slot, it is just reported through a different code
  now). The ruling is recorded in `docs/compiler-spec.md` "Temp scope and
  definite assignment" and `docs/runtime-spec.md` "Uninitialized temp
  reads".
- **WHY:** What plays in Inky must play in brink — the C# reference prints
  the line and warns (`Variable not found: 'n'. Using default value of 0
  (false)…`), so an author who tests in Inky and then opens brink was
  meeting a hard fault where the reference had a warning, which breaches
  the ink-compat floor. The compile-time diagnostic is the primary fix
  rather than the runtime fallback alone, because the author should learn
  about the mistake before playing, and a warning that a `[lints]` entry
  can turn down leaves a project that leans on the pattern deliberately
  somewhere to go.

## Knot/stitch navigation click reveals in place when its file is already open
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** studio-shell / editor-ui
- **SCOPE:** moderate
- **WHAT:** A single-click (navigation, `pinned === false`) open of a knot or stitch whose file is already open as a whole-file tab — anywhere, not only the active group — reveals in place inside that tab instead of minting a `path::name` fragment tab. A pinned open (double-click) is excluded from this: it always mints or focuses the fragment tab, unchanged from before. Implemented as `openSymbolTarget` (`packages/brink-studio/src/mount.tsx`), gated ahead of the normal `openDocument` fallback in `setDocumentOpener`.
- **WHY:** Every knot/stitch click previously minted a fresh fragment tab regardless of whether its file was already open, because `EditorGroupsState.openDocument`'s existing-tab reveal matches by exact `documentKey` and a symbol's fragment key (`"path::name"`) never equals its file's whole-file key (`"path"`) — the common case of browsing structure while a file is already open just kept stacking tabs (#3356). Restricting the reveal to navigation opens (not pinned) preserves docs/studio-shell-spec.md §7.8's Fragment⇄file overlap as first-class: a pinned open is a deliberate "give me a dedicated, focused view of this knot" action, and silently retiring that into the whole-file tab would remove a feature four e2e specs encode, not fix a bug.

## Compat-deny diagnostic tier
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** compiler
- **SCOPE:** architectural
- **WHAT:** A new diagnostic class, **compat-deny**: "inklecate rejects
  this; brink can run it; you must opt in." Default severity `Error`;
  overridable per project through the existing `[lints]` table — to `warn`,
  or all the way to `allow` ("we should allow it to be turned off if the
  user wants, it's annoying"). The CLI's `--warn`/`-D` flags gain the same
  reach. A project's `[lints]` entry travels with the project, so a host
  consumer running `brink compile` (bevy, a game build) gets exactly the
  permissiveness the studio does — there is no studio-only switch.
  Mechanically: `DiagnosticCode::severity()` stays `Error` and
  `DiagnosticCode::is_overridable()` returns `true` for tier members (widened
  specifically for this tier — every other `Error`-default code stays
  non-overridable, issue #1160's original rule); `effective_severity` and
  `validate_lint_code` both defer to `is_overridable`/`is_compat_deny` rather
  than re-deriving "not Error" so the two definitions cannot drift apart.
  **Admission invariant (must be tested):** a code may sit in this tier only
  if brink produces a *working* program when the code is downgraded — every
  compat-deny code needs a fixture that compiles under `allow` and plays
  correctly; anything that would fail at link or fault at runtime is NOT
  admissible and stays a hard, non-overridable error. **First member:**
  `E194` — "a knot's `~ temp` is not visible from its stitches in ink",
  split out of `E193` shape 4 during PR #3369's review: brink plays the
  program (`Stitch sees 7.`), inklecate rejects it (`Unresolved variable:
  n`). Recorded in `docs/compiler-spec.md` "Compat-deny diagnostics".
- **WHY:** Brink accepts a superset of ink at several points, and each such
  point where the official compiler rejects the program but brink produces
  a working one needs the same treatment — a named tier keeps them
  consistent and discoverable rather than each one inventing its own
  severity story. Defaulting to `Error` keeps an ink-compat project honest
  by default (the same wall Inky would show it); making it overridable all
  the way to `allow`, not just `warn`, was a deliberate maintainer call
  against #1160's usual "hard errors are never downgradable" posture,
  because the admission invariant already guarantees the downgraded program
  genuinely works — there is no real defect left to protect the author
  from once they opt in.

## Observable runtime semantics: the host-facing trace
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** cross-system — runtime / compiler / test-harness (`docs/observable-semantics-spec.md`)
- **SCOPE:** architectural
- **WHAT:** Two programs are observably equivalent iff every run (start point, RNG seed, choice sequence, fixed external results) yields the same trace: output steps in order (lines with text/tags/element data; choice sets compared **by order**; terminal kind), external calls in order with arguments, **host-readable global state at every turn boundary** (in, not out), and host-invoked function results. Bytecode layout, step counts, timing, compile diagnostics, runtime warnings, temps, stacks, visit counts as internals, and RNG state as such are unobservable. A second, separate invariant binds source-level tools: translation identity (line-table scope ids / text hashes) must be unchanged for every line the tool did not edit. A future `#@internal`-style marker to take a global out of the host-visible set is noted, not built.
- **WHY:** Maintainer: the bar for a "safe" transformation "is not byte-identical in the compiled output, it's more like identical in observable runtime semantics, which is a notion we probably need when we work on the optimizer anyway." One definition shared by auto-fix, fmt, respell, incremental lowering and the optimizer, instead of five private ones. Globals are in because hosts read them regardless of whether the story does; choices are ordered because hosts pick by index; translation identity is separate because it is not runtime-observable but an author with a shipped locale would rightly call breaking it unsafe.

## Safe auto-fix means observably equivalent — and the oracle harness is not enough on its own
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** cross-system — auto-fix / optimizer testing (`docs/observable-semantics-spec.md` §4–§5)
- **SCOPE:** architectural
- **WHAT:** A *Safe* fix (batchable: fix-all, CLI, fix-on-save) is one that satisfies the observable-equivalence definition plus translation identity — nothing weaker. Fixes that change meaning or lose text require positive author intent and never batch. Correctness for the optimizer is the same relation, and "optimized and non-optimized programs must be observably identical in all cases"; the guarantee ladder is corpus differential (tier 0), property testing over generated programs (1), pass-level metamorphic properties (2), mutation sensitivity of the oracle itself plus `cargo-mutants` on the optimizer (3), and runtime fuzzing of optimized output (4). Tier 0 and the oracle-sensitivity study ship first.
- **WHY:** Maintainer: "we'll need a stronger guarantee than the oracle harness, it's got plenty of stuff, but not nearly enough, we'll need something more like mutation testing, or property testing or similar to test the optimizer properly." The corpus only covers shapes someone already wrote down; an optimizer's bugs live in the shapes nobody did. Mutation-testing the oracle first is what proves the definition is complete before anything rests on it.

## Story-level program generator is its own epic; `.ink` first; native via both direct generation and respell
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** test-harness (`docs/observable-semantics-spec.md` §4.1, #3370)
- **SCOPE:** moderate
- **WHAT:** The proptest story-level generator is a standalone epic (#3370), prerequisite to the optimizer and immediately useful to auto-fix, fmt, respell and incremental lowering. Its first deliverable is the **`.ink` grammar**; the native grammar follows. The native half is built **both** ways — direct `.brink` generation and routing generated `.ink` through `brink-respell` — and the respell route is itself one more equivalence property, `trace(P) = trace(respell(P))`.
- **WHY:** Maintainer: "i need regular .ink support more urgently than native syntax right now"; and on the native half, "we'll need to do both, but we should include the respell as another form of test, because we should define the equivalence properties" first. Defining the properties before the generator keeps every consumer stating the same claim against the same oracle.

## Auto-fix: lazy per-code fixers; diagnostics stay data; three tiers each backed by a test
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** brink-ide / brink-analyzer / studio / brink-cli / brink-lsp (`docs/autofix-spec.md` §2–§3)
- **SCOPE:** architectural
- **WHAT:** No per-diagnostic trait; `Diagnostic { file, range, message, code }` is unchanged. Fixes are computed lazily by per-code `Fixer` impls in `brink-ide` (a `static FIXERS` registry, one per code, with a registry test), returning `Fix { code, title, applicability, edits: Vec<FileEdit>, caret }` — minimal text edits are the only fix currency, and the three existing quick-fixes migrate to it. `max_applicability` is declared statically per fixer (so surfaces can count without computing edits); the per-instance value may only be lower. Tiers: **Safe** = observably equivalent + translation identity, proven per fixer by `assert_safe_fix` (compile → apply → recompile → empty `trace_diff` → line-table identity); **Suggested** = discharges the diagnostic with no new error (the existing `StructuralResult.safe` property), applied one instance per click unless the project promotes the code; **Placeholder** = leaves a hole, never batched. An optional typed `data` payload is added to a diagnostic only when a specific fixer provably needs it.
- **WHY:** Maintainer, after weighing a diagnostic trait: diagnostics are data that travel across wasm/LSP/CLI and are built at ~200 sites — a trait buys nothing the `DiagnosticCode` enum's metadata methods don't already give; fixes are behaviour that must be lazy because eager edit construction on every keystroke is exactly the cost the live-typing perf work fights. Tiers name the test that backs them so "safe" is never a label somebody typed: "implementing fixes could potentially be complex and some are safe for auto-fix versus some requiring positive intent from the user."

## Fix scope is the compilation, not the file
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** brink-ide (`docs/autofix-spec.md` §4–§5)
- **SCOPE:** moderate
- **WHAT:** `FixCx` is the compilation (`ProjectDb`); a fixer emits whatever edits the fix needs in whichever files. Surfaces differ only in which *diagnostics* they select (all / in this file / at the cursor / one row / by code), never in which files may be written. A per-file selection — fix-on-save, "Fix all in this file" — may therefore edit other files: in the studio those edits land on the other buffers and mark them dirty (rename's existing road); the CLI and LSP `fixAll` write every touched file. Batching drops (never merges) overlapping edits within a round and re-analyzes the compilation to a fixpoint, capped at 5 rounds.
- **WHY:** Maintainer: "i don't see why they have to be explicitly single-file? if they need to be cross-file to work, they need to. i don't think we should [be] intrinsically tied to files in the first place. for ink it should be tied to the compilation overall." The trace-equivalence definition is compilation-wide already, so tying scope to the compilation makes the safety notion and the mechanism the same thing.

## Auto-fix policy layering: `[fix]` in brink.toml is what; the app setting is when, as a ceiling
- **WHEN:** 2026-09-01
- **PROJECT:** brink
- **SYSTEM:** brink-project-config / studio settings / brink-cli (`docs/autofix-spec.md` §6–§8)
- **SCOPE:** moderate
- **STATUS:** tentative
- **WHAT:** `brink.toml` gains a `[fix]` table shaped like `[lints]`: per code `auto` (promote a Suggested fix to batch), `ask` (default), `off` (never offer). It travels with the project and applies identically to `brink fix`, LSP `fixAll`, the Problems "Fix all", and on-save; it is edited from the existing lints table in Settings as a Fix column. The app-scope "Fix on save" setting is a personal ceiling — Off / Safe only / Everything the project allows — and the effective on-save policy is the intersection with the project policy: the editor can only be more conservative than the project. Explicit actions (`brink fix --suggested E033`, a row click) may widen per run; the implicit save only narrows. Entry points RULED: Problems panel (row + header), editor context menu, code actions, command palette, `brink fix` as its own subcommand with `--dry-run`/`--diff`/`--suggested`/`--code`.
- **WHY:** Maintainer ruled the `[fix]` knob ("yes, it can even go in the existing diagnostics UI"), the save setting plus the three entry points, and the subcommand ("it can generate patches, dry-run, etc."), and named the app-setting ↔ `brink.toml` relationship as the one point of uncertainty — hence tentative. The ceiling-∩-policy shape keeps a team decision (promote E033) from being silently re-decided per editor while never letting an editor exceed what the project admitted.

## The equivalence oracle is its own trace type, not an extended `Episode`
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** test-harness (`docs/observable-semantics-spec.md` §3/§3.1, #3376)
- **SCOPE:** moderate
- **WHAT:** Tier 0 of the guarantee ladder ships as `brink_test_harness::trace` — a `Trace`/`RunSpec`/`trace_diff` triple that records exactly `docs/observable-semantics-spec.md` §2's list and nothing else — rather than as new fields on `Episode`. `Episode` stays the C# oracle's record. Programs are `.inkb` bytes at the `trace_diff` boundary; `differential(pre, post, config)` is the pre-vs-post entry point later consumers (auto-fix's `assert_safe_fix`, the optimizer) point at. Translation identity (§2.2) is a separate result computed from the real `brink_intl::export_lines`. Mutation sensitivity (§4 tier 3a) only counts a mutant as a survivor when it is **grounded** — the baseline trace demonstrably exercised the site the mutation edits.
- **WHY:** `Episode` records visit counts and RNG writes, which §2 explicitly calls unobservable; folding the definition into it would smuggle internals into the definition, and adding fields would change the on-disk golden-episode schema the ink ratchet reads. Grounding is what makes the tier 3a number mean anything: without it a mutant in an unexplored knot "survives" because the exploration never looked, which says nothing about the definition and would make a 0% survivor rate unreachable for reasons unrelated to the oracle's quality.

## A module-private native global is not host-readable, so it is outside item 3
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** test-harness / native surface (`docs/observable-semantics-spec.md` §2.3, #3376)
- **SCOPE:** moderate
- **WHAT:** The oracle reads host-readable state through the host's own road (`Story::variable`, i.e. `getVar`), which honours `#@private`. Because the native surface is always a declared module and therefore defaults private (`docs/modules-spec.md`), a `.brink` `var` without `pub` is absent from §2 item 3's capture, and two programs differing only in such a global are reported observably equivalent. Recorded in §2.3 next to the `#@internal` escape hatch, and pinned by a test in both directions (`pub` covered, private not).
- **WHY:** Item 3 says *host-readable*, and a private global genuinely is not: the host is outside every module. Left undocumented this would look like an oracle bug the first time someone hit it; documented, it is the escape hatch §2.3 contemplates, already present in fact on the native surface — which an optimizer's designer needs to know before assuming every global write is protected.

## Project-declared dialogue dialect lives in brink.toml
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** editor-ui / project-config (dialogue dialect, #368)
- **SCOPE:** moderate — REVISES the 2026-07-05 #368 ruling's "no project file in v1 (mount-time config only)"; that spec filed the project-file home as the expected follow-up, and this is it.
- **WHAT:** A project declares its dialogue dialect in `brink.toml`: a `[dialect]` table (`preset = "…"` plus `[[dialect.elements]]` overlays using the spec's affix sugar, and the run rule below) is the PRIMARY authoring form; `dialect = "path.json"` remains as the escape hatch for a full hand-written artifact. Both resolve to one `DialogueDialect`; `mountStudio({ dialect })` stays as the embedder override. Tracked as #3387.
- **NOTE (2026-08-30, implementation):** the table is spelled `[dialogue]`, not `[dialect]` — `[project] dialect` already names the SOURCE surface (`strict-ink`/`brink`) and `[prose] dialect` the spell-check English; `[dialogue]` is the DialogueDialect's own noun. The file form is `[dialogue] file = "path.json"` (a bare top-level `dialogue = "…"` string only parses before any table header in TOML).
- **WHY:** The dialect is "how this project's text works," which is exactly `brink.toml`'s charter (it already hosts `prose_dialect` and `conventions`); the common case is tiny (a preset plus a kind or two) and reads as TOML with the affix sugar, and a second file you must know to reference is friction for ten lines. Brink cannot bake every author's format into the app — the artifact is the capability, the project owns the format.

## No dialect by default
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** editor-ui / player
- **SCOPE:** moderate
- **WHAT:** A project that declares no `[dialect]` gets none: the Player prints plain lines (the Inky posture) and the editor applies no cue form. The shipped `at-cue` preset becomes opt-in. Tracked under #3387; the Player half is #3389.
- **WHY:** Nothing is assumed about a project's conventions; presets are offered, never imposed. This is what makes "not baking one user's format into the app" true by construction, and it keeps the plain-ink experience identical to Inky for anyone who wants exactly that.

## Dialects declare what ends a dialogue run in the emitted stream
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** editor-ui / dialect artifact
- **SCOPE:** minor/local
- **WHAT:** The chain rule gains an emitted-side facet, `run_ends_at` — the list of declared kinds (and the reserved `"choices"` boundary) whose appearance ends the active speaker's run in RUNTIME-EMITTED text. Consumers of emitted text (the Player, an engine importing the resolved dialect) apply it through one shared run-state helper. Tracked as #3388.
- **WHY:** The source-side chain rule has a hard break the emitted stream lacks — "blank ALWAYS breaks" — and ink swallows blank lines on output, so a cue-less dialogue line after a cue is unattributable downstream unless the dialect says when a run ends. Declaring it (rather than hardcoding a guess) keeps the Player and the author's own engine reading the same answer from the same artifact.

## Engines consume the RESOLVED dialect as a compile output
- **WHEN:** 2026-08-30
- **PROJECT:** brink
- **SYSTEM:** cli / packaging
- **SCOPE:** moderate
- **WHAT:** `brink compile` (and the studio's export) emits `dialect.json` beside the compiled story — the project's dialect with the preset merged and affix sugar expanded. A game engine reads that derived product plus the parser, never the `brink.toml` source declaration. The parser/validator/types move to a tiny pure-TS `@brink-lang/dialect` package (re-exported by `@brink-lang/editor`). Tracked as #3393.
- **WHY:** Single truth without drift: the source is authored once in TOML and the JSON is generated, so there is no hand-edited copy to diverge, and the engine needs no preset-resolution logic. A game codebase should not have to depend on an editor package (CodeMirror and all) to read a JSON schema.

## Program generator: typed model + corpus mutation, inkjs as the reference harness, a capture tier
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** test-harness / brink-gen (`docs/program-generator-spec.md`)
- **SCOPE:** architectural
- **WHAT:** (1) Architecture A + C: the generator builds a typed semantic model (declare-before-use, terminating by construction) and prints it to `.ink`; proptest shrinks on the model; corpus mutation (the #3376 mutator) is the second source; string-grammar generation is not used. (2) The reference differential for generated ink-valid programs runs on **inkjs** (runtime + JS compiler), sanctioned as a proxy by replaying every checked-in C# oracle episode; the C# runtime stays the tie-breaker. (3) A **capture tier**, `tests/tier4-generated/`: shrunk counterexamples and coverage-novel generated stories are promoted into the corpus with provenance (`oracle-source` inkjs/csharp), outside `RATCHET_EPISODE_COUNT`, with its own must-pass target. (4) `crates/internal/brink-gen` is its own crate. (5) Feature order is the corpus ladder as written; biasing is a data `Profile` with bait flags.
- **WHY:** Maintainer, 2026-09-02: "1-yes" (A+C); "maybe we use inkjs as the harness here so it's easier to run not on my laptop? we already have web tooling?"; "i'd also like to consider capture for interesting cases so they join the corpus, maybe as a new tier or something"; "4- yes"; "ordering looks fine as-is." A typed model is what makes shrinking produce readable counterexamples and validity hold by construction; inkjs removes dotnet from the loop so the strongest ink-compat check available runs in CI; the capture tier turns every found bug into a permanent regression case rather than a transient seed.

## Conventions editor: choice text hidden by default, branch headers never lines, sections stacked
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-settings (Conventions section; #3411, #3408)
- **SCOPE:** minor/local
- **WHAT:** (1) Choice-text lines are hidden from the marking list by default, behind an "Include choice text" toggle; hidden lines are not taught from. (2) Conditional branch header lines (`- cond: text`, `- else: text` inside a multi-line `{ … }`) are never passage lines — only the content after the colon is. (3) The section is stacked in working order: the passage you pick, the lines you mark, what the studio learned, then the Player preview — not two columns. (4) Clicking into "Your lines" lists every knot and stitch before anything is typed; typing narrows.
- **WHY:** Maintainer, on seeing it run: choice lines are the player's options far more often than dialogue and crowd the list ("cut out choice lines, or add that as an option"), but the ink docs' own sub-format puts a cue inside choice text, so they stay reachable rather than gone. Branch headers carry a condition and a colon — never dialogue, and a `Name:` false positive waiting to happen. Two columns squeezed the line text beside a five-way control; stacking gives each step the width.

## Player: a speaker who keeps talking is one run, however the script cued it
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-player (dialogue runs, #3389; Conventions preview #3411)
- **SCOPE:** minor/local
- **WHAT:** Adjacent runs by the same speaker (same kind, nothing between them) fold into one group: the speaker header prints once and the lines flow under it. A run with something in between — an action, a choice echo, narration — keeps its own header.
- **WHY:** Maintainer, seeing per-line cues render as a header per line: "if we have the CUE be sticky, we should render the speaker cue differently … in this script it's per-line, but if it weren't, we'd still want that." The cue is sticky by the run rule already; the render should read the way a reader experiences it.

## A cloned stateful alternative shares one counter, not one body
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** compiler (HIR normalize/stamp, LIR lower, codegen)
- **SCOPE:** moderate — amends the #3275 ruling's "sharing revoked" corner;
  ruling (1) of #3275 (whole-line renderings for cloned lines) is
  reaffirmed
- **WHAT:** When lifting an inline construct clones a stateful alternative
  into two or more branches, every clone still lifts into whole-line
  renderings (one line-table entry, translation unit, and VO slot each),
  and every clone keeps its own wrapper container — but the clones share
  ONE visit-count state: clone 0 keeps the stamped id (its wrapper, or the
  variant path's empty stub, carries the count), every other clone gets a
  derived `container_id` and records the original as its `counter_id`,
  and codegen selects that clone's branch by touching the original
  container (`TouchVisit`, seeded for shuffles from the original's
  `path_hash` via `ShuffleIndexOf`) instead of reading its own wrapper's
  count. A claimed variant line whose alternative is a clone touches the
  original too. The per-lift-level revocation — a clone in an unclaimable
  branch getting its own counter — is removed.
- **WHY:** #3401: revocation made `{c|d|e}` drift a view behind the C#
  reference whenever a conditional or glue made its branch unclaimable
  (`apc bpc bpd` vs `apc bpd bpe`; 93 of 512 probed shapes, one trigger).
  Two shapes were on the table. Sharing the clone's BODY (keep the clone
  inline, enter one container from every site) is ink's own container
  model and also matches, but it moves those lines from whole-line units
  to per-token fragments in the line table — `brink export-xliff` for
  `{a|b}{true:p}{c|d|e}` went from twelve whole-line units to seven
  fragments, orphaning translations and VO slots, which is exactly what
  #3275's ruling (1) chose to protect. Sharing only the COUNTER keeps
  both: the maintainer ruled for it (2026-09-02, "C: lift + shared
  counter"). It is codegen-only — the variant path already touches shared
  stubs this way — with no runtime or format change, and saves stay keyed
  to the stamped id. Pinned by three C#-oracle cases
  (`sequence-leads-multi-construct-line`,
  `sequence-cloned-into-glued-line`,
  `sequence-shared-across-mixed-claim-branches`); ratchet 5619 → 5622.

## Conventions editor: teach-by-example is the design direction
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-settings (Conventions section; #3392)
- **SCOPE:** moderate
- **WHAT:** The non-technical Conventions editor in Settings is built as "teach by example": the author pastes a few lines as they actually write them, marks each line (cue / dialogue / action / narration), the studio proposes the `[dialogue]` rules and shows them back as plain sentences with the lines that support each, and nothing is written to `brink.toml` until the author confirms. Chosen over three alternatives on the design canvas (recipe tiles, rule sentences, a guided wizard).
- **WHY:** Maintainer: "clearly the best, by a long shot" — authors who already have pages should not have to describe their format, they should show it. The stated risk is implementation complexity (rule inference); the direction stands on the condition that inference is explainable and verified, not clever: propose from a small set of shapes and confirm by re-parsing the marked lines, surfacing anything the shapes cannot explain as a decision for the author rather than a guess. The inference tests cover the ink documentation's own suggested line formats (`Name: line`, cues with line tags, quoted prose with attribution) alongside the studio's presets — corpus recorded on #3392.

## Conventions editor: sample lines come from a knot/stitch selector
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-settings (Conventions section; #3392)
- **SCOPE:** minor/local
- **STATUS:** tentative
- **WHAT:** The teach-by-example editor pulls its sample lines from the project through a content selector — the same knot/stitch typeahead the Player's "play from" launcher uses — rather than (only) a paste box or "the open file".
- **WHY:** Maintainer: the author should point at a passage they know is representative ("pull the content in from a given knot/stitch"), and the studio already has the affordance for choosing one; reusing it keeps the two pickers identical and avoids a paste step for lines that are already in the project. The pulled passage is shown whole: the marked-lines list and the Player preview scroll for long runs rather than trimming to the first few lines.

## A Safe auto-fix must have a pre-image: `assert_safe_fix` cannot certify a fixer whose diagnostic blocks compilation
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** test-harness / auto-fix (`docs/autofix-spec.md` §3.1, `docs/observable-semantics-spec.md` §5, #3417)
- **SCOPE:** moderate
- **WHAT:** `brink_test_harness::fix::assert_safe_fix` ships as the executable form of the `Safe` tier: it compiles a `tests/fix/<code>/{before,expected}` pair through the production road, explores the pre-fix program's run set, replays exactly those runs on the post-fix program (`trace_diff`), and diffs the exported line tables, tolerating only the units the fixture's `rewrites.txt` declares. Three consequences are recorded rather than designed around. (1) A fixer whose diagnostic **prevents compilation** has no pre-image, so §2's definition is inapplicable, not merely unsatisfied — the verdict is `NoPreImage` and such a code can never be `Safe`. Measured on all four migrated fixers (E025, E063, E080, E081), all four of which already declare `Suggested`. (2) An empty trace diff over a baseline that produced no content is not evidence — the helper counts the pre-fix program's line/choice/external/probe events and refuses to certify a run set with none. (3) The obligation is split across two crates, because `brink-test-harness` depends on `brink-ide` and the dependency only runs one way: `brink_ide::fix`'s registry test demands the fixture exists, the harness's `tests/fix_safe_obligations.rs` enumerates the same registry and runs it.
- **WHY:** `Safe` is what licenses an unattended batch edit (§5's fix-all, `brink fix`, on-save), so the tier has to name a test that would actually fail. Two of the three findings are ways the test could have passed while proving nothing — a comparison with one program in it, and a comparison of two stories that both do nothing — and both were reached by accident while writing the first fixtures, not hypothesised. Splitting the obligation rather than duplicating the oracle keeps one implementation of the definition; enforcing only the harness half would let a `Safe` fixer ship with no fixture at all, and enforcing only the `brink-ide` half would let it ship with a fixture nobody ran.

## Player look: provenance chip off the text, no row stripes, dialogue indented, choices link back, styling in Settings
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-player
- **SCOPE:** moderate
- **WHAT:** (1) The provenance chip (`file:line` on hover) must never cover the line's text. (2) Transcript rows are not striped (no alternating row background). (3) Dialogue lines are indented a little under their speaker's header, so the speech reads as coming from the speaker. (4) A choice echo (`> Enough shopping`) links back to the choice's source the way a line does. (5) Player styling lives in Settings → Player (app scope): font family first — the desktop app enumerates the machine's fonts through the Tauri side; the web build offers a curated list plus a free-text family — with further knobs to follow.
- **WHY:** Maintainer, reviewing the Player after the dialogue-run work: the chip "is covering the text"; "there's no reason it shouldn't link back to where it was"; indentation "to indicate it came from the speaker"; stripes read as a table, not prose; the font is a matter of taste, and taste is a setting — browsers cannot list installed fonts (fingerprinting), so enumeration is a desktop capability and the web gets a curated list.

## Player look: design pass first; choice echoes carry their source marker; the editor follows the Player closely
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-player, editor-ui
- **SCOPE:** moderate
- **WHAT:** (1) The Player gets a small design pass before more knobs are added — a better baseline (even padding on text rows, consistent rhythm) rather than fixes one at a time. (2) A choice echo in the transcript shows `*` or `+` to its left according to the choice's kind in the source (once-only vs sticky), marking it as the reader's pick. The two button-styling directions proposed (quiet buttons; echo as a "You" run) were both declined. (3) The editor's "follow the Player" behaviour is strengthened: the editor should track the Player's position much more closely as the story advances, not only on hover or ⌘-click. (4) The row highlight in the transcript runs the full width of the pane, not just the text block. (5) The pass aims at "a really strong and clear visual design for the player, something that really sells it" — directions are drawn on the canvas and one is chosen before implementation.
- **WHY:** Maintainer: "we should absolutely have a better baseline to work from"; the marker "indicate[s] it was a choice from the user, based on what type it was in the source"; the existing follow "is not nearly strong enough, it should follow the player much more closely."

## Player look: direction C ("Stage") chosen; provenance as a hanging icon button; hover links to the editor; tags shown
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-player, editor-ui
- **SCOPE:** moderate
- **WHAT:** (1) The Player's reading surface follows direction C of the design pass — modern, colour-led: each speaker's block hangs off a rule in the speaker's palette colour with the name as a small label, asides inside the block, choices as cards carrying their `*`/`+` marker. Directions A (Manuscript) and B (Screenplay) are dropped. (2) The provenance affordance is a small icon button, absolutely positioned so it hangs below the row's edge over the next row, revealing `file:line` as a tooltip on hover — not a text chip in the row. (3) Hovering a transcript line highlights its source line in the editor (distinct from the follow band). (4) The transcript shows a line's tags. (5) Still open: a visual element that ties the choice cards to the transcript rows — "we're closer to good choices here, but I want something more." (6) Narration reads at full strength; action lines are the dimmed ones — "action is dimmed, narration isn't" (this reverses the Player's current italic-muted narration). (7) The provenance button is present only while its row is hovered. (8) The spine (the rail the speaker segments and choice nodes share — accepted: "that's neat") reacts to the line kind: solid coloured for a speaker, plain for narration, dotted along action text. The echo ring sits on the centre of its text line.
- **WHY:** Maintainer, on the canvas: "C is pretty good. it's not quite there, but we can drop the other two from consideration"; the link "should be an icon button that when hovered reveals the filename:line"; "hovering the line should highlight in the editor, as well"; "i'd like to see tags in the example".

## E031/E176 Safe trim removes the leading excess argument, not the trailing one
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** brink-ide auto-fix (`docs/autofix-spec.md` §9, #3428, milestone 8 of #3374)
- **SCOPE:** moderate
- **WHAT:** The `Safe` fixer for `E031` (ordinary call over-arity) and `E176` (divert-with-args over-arity) deletes the call/divert site's **leading** `got - expected` supplied arguments and keeps the **trailing** `expected` ones — the opposite end of the list from `creation_site_fix`'s `TrimFnLiteralArgsFixer` (the `#fn(...)`/`call(...)`/`bind(...)` function-value path), which keeps the leading prefix. The safety guard (withhold the fix) therefore checks the **leading**, dropped arguments for a nested call (or, on ink, an `++`/`--` increment — native has no expression-position mutation to guard against), not the trailing ones.
- **WHY:** Proven empirically, not inferred from reading the bytecode: compiling and playing `-> accuse("Hastings", "Poirot")` against `flow accuse(who) { I accuse {who}! }` prints "I accuse Poirot!". The classic calling convention these two diagnostics cover (`Opcode::Call`/`Opcode::CallExternal`) pushes every supplied argument in source order, and the callee's own parameter-binding prologue pops exactly its declared count off the shared value stack LIFO — so the trailing supplied argument binds to the declared parameter, and the leading excess is evaluated (for any side effect) and then silently discarded. A fixer that trimmed the trailing arguments instead would be observably wrong for any over-supplied call, which is exactly the shape `Safe` exists to rule out.

## E031/E176 Safe trim also withholds on a non-isolated call site and on any `ref` param
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** brink-ide auto-fix (`docs/autofix-spec.md` §9, #3428, milestone 8 of #3374)
- **SCOPE:** moderate
- **WHAT:** Two more conditions withhold the `E031`/`E176` `Safe` trim, on top of the leading-arguments-must-be-pure guard above: (1) the call's own return value must be popped in isolation — the call must be the entire right-hand side of a `~ temp`/`~` assignment (or, for `E176`, the entire divert; a divert is never itself nested inside a larger expression on either surface, so this only narrows `E031`'s ordinary-call shape in practice) — never a sub-expression of something larger like `~ temp r = 1 + greet(...)`; (2) the resolved target must declare no `ref` parameter.
- **WHY:** (1) A call embedded in a larger expression leaves its leaked leading argument sitting *beneath* the call's own return value on the shared value stack; the enclosing operator's pop then reads that leaked value as its other operand instead of discarding it, so trimming the source-level leading argument changes the program's computed result rather than reproducing it — proven by `-> accuse` and this fixer's own repro, `~ temp r = 1 + greet("Al", "Bob")` against `greet(name)`, which pops `"Al"` (not `1`) as `+`'s other operand before the fix. (2) `lower_call_args` decides `ref`-ness positionally against the *declared* params, while the runtime binds the actual argument value by *trailing* position — trimming the leading arguments re-indexes which supplied argument lands on the `ref` param, silently flipping write-back (`VAR hp = 10` / `heal(ref h, amt)` / `heal(hp, hp, 5)`: before the fix `{hp}` stays `10`; after the offered trim it would become `15`). Both found in review before merge; the `Safe` tier admits no exceptions to "observably equivalent," so both are withheld outright rather than special-cased.

## Fix on save is an app-scope ceiling, default off, resolved through `effective_fix_policy`
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** studio-ui, brink-web
- **SCOPE:** moderate
- **STATUS:** tentative
- **WHAT:** The studio's "Fix on save" setting is `off | safe | project`, defaults to **off**, and lives with the other app-scope editor settings (`brink-studio.editor.v1`) — never in `brink.toml`. It resolves as a CEILING over the project's `[fix]` table rather than as a tier filter: `safe` maps to the ceiling `"ask"` and `project` to `"auto"`, and both go through `ProjectConfig::effective_fix_policy(code, ceiling)` rather than any intersection re-derived at the call site. An unrecognized persisted value lands on `off`. The on-save run pushes no undo entry and raises no toast of its own.
- **WHY:** `docs/autofix-spec.md` §6.2 marks the ceiling relationship TENTATIVE and asks for it to stay resolved in exactly one function, so the relationship can change in one place. The default-off half is not tentative: an editor that silently rewrites a manuscript on every Ctrl-S is not a default anyone opted into. `safe` is expressed as a ceiling rather than `Select{tiers:["safe"]}` because a tier filter would ALSO withdraw a Safe fix the project turned `"off"` — the project's own opinion has to keep applying underneath the personal one.

## `fix_all` over wasm restores the session; the report carries the sources to write
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** brink-web, studio-ui
- **SCOPE:** moderate
- **WHAT:** `EditorSession::fix_all` rolls the batch loop's intermediate rewrites back before returning, and reports `files: [{ path, new_source }]` instead. The host applies them through its own seam, exactly as it applies `apply_fix`'s `StructuralResult`.
- **WHY:** The studio's apply seam (`applyMoveResult`) snapshots each file for undo *as it writes*. A session left holding the fixed text would make that snapshot capture the fixed text, and Undo after "Fix all safe" would restore nothing. Keeping the wasm query side-effect-free also makes it the same shape as every sibling on that boundary, so a host cannot be surprised by which of them mutate.

## E095 Safe fix needs exactly one narrowing guard — the physical-line overlap with a following declaration's own `#@was` lookback
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** brink-ide auto-fix (`docs/autofix-spec.md` §9, #3425, milestone 8 of #3374)
- **SCOPE:** minor/local
- **WHAT:** The `Safe` fixer for `E095` (`#@was(name)` naming a definition's own current name) deletes the stale tag line, **except** when the diagnostic's own physical line is also read by a different owner as a live (non-self) rename: (i) a file-level module self-alias whose line also attaches to a following `VAR`/`CONST`/`LIST`/`EXTERNAL` whose name differs from the `#@was` argument, or (ii) a declaration-level self-alias whose file also carries a `#@module` (in the same leading run) whose name differs from the argument. Both withhold the fix outright rather than deleting.
- **WHY:** Reproduced through the production road (`brink_test_harness::corpus::compile_via_environment`): `#@module(town)` / `#@was(town)` / `VAR gold = 0` self-aliases the module (`E095` there), but `file_module_was`'s file-level scan and `VAR gold`'s own `directives_before` lookback both read the identical physical tag line — `assemble_hir_file`'s module arm documents this coincidence as "an entirely ordinary authoring style". Compiling both sides shows `alias_table` going from one live `AliasEntry` (the `VAR`'s own `town -> gold` rename) to empty once the line is deleted — the self-alias reasoning is correct for *the module*, but the same line is a live rename for the declaration. The mirror shape (`#@was(gold)` self-aliasing the `VAR`, `#@module(town)` differing) loses the module's alias instead. Neither original PR text nor `assert_safe_fix` caught this: the harness diffs traces and exported line tables only, never the alias table, so its `ObservablyEquivalent` verdict on the *unrelated* `tests/fix/E095/` fixture (where the line attaches to no declaration at all) is not evidence for this shape. Outside this one physical-line overlap the original no-narrowing reasoning holds.

## E014 Safe fixer covers only the catch-all "bare `~`" shape, ink-only, and is deleted as a whole physical line
- **WHEN:** 2026-09-02
- **PROJECT:** brink
- **SYSTEM:** brink-ide auto-fix (`docs/autofix-spec.md` §9, #3423, milestone 8 of #3374)
- **SCOPE:** moderate
- **WHAT:** `empty_logic_line_fix::EmptyLogicLineFixer` fires only when the located `LogicLine` carries none of `stmt_block()`/`await_stmt()`/`return_stmt()`/`temp_decl()`/`assignment()` and no `Expr` child either — the one `E014` raise site (`logic_line.rs`'s trailing catch-all) that is genuinely effect-free, out of **fifteen** raise sites total across three files: `logic_line.rs`'s own five (the catch-all plus four malformed-partial `~ temp`/`~ x =` shapes), `logic_block.rs`'s six `~ { … }` block-statement mirrors (`TempDecl`/`Assignment`/`ForStmt` missing a name/target/value), and `control_flow.rs`'s four native mirrors. The four `logic_line.rs` malformed-partial sites are refused by the same five-accessor structural check. The six `logic_block.rs` sites are refused by a *different* mechanism, not that check: they diagnose at the inner `TempDecl`/`Assignment`/`ForStmt` node's own range rather than the enclosing `LogicLine`'s, so the fixer's exact-range `LogicLine` lookup never matches there and returns early before the five accessors are even consulted. The fixer never fires on a native file at all: native's own "nothing after `~`" shape parses as an `EXPR_STMT` with a missing operand and raises `E015`, not `E014` — there is no native CST shape this fixer's own code is ever raised for; `control_flow.rs`'s four native `E014` sites are excluded by the same ink-only dialect gate. The deletion range extends to the whole physical line (back through any leading indentation, forward through trailing whitespace and the line break) **only when the located node's own range is itself confined to one physical line and carries no `LINE_COMMENT`/`BLOCK_COMMENT` trivia token** — a comment (single-line trailing, or a multi-line `/* … */` that pulls several physical lines into the node's own range) withholds the fix outright rather than being deleted along with the line.
- **WHY:** The five accessors being `None` is only reachable one way in the grammar (`atom()`'s catch-all returning `false`, consuming nothing and building no node) — proven from the CST, not read off the diagnostic message, per the issue's "re-establish effect-freedom itself" requirement. Extending to the whole physical line matters because the `LogicLine` node's own range starts at the `~` token, not at the line's start — a fix that deleted only the node's own range on an indented line (e.g. inside a choice body) would leave the leading indentation glued onto the next line's content instead of removing a clean line, which is not what "delete the line" means and is not proven safe by the flush-left fixture alone. The comment guard was added on review: `Parser::skip_ws` treats `LINE_COMMENT`/`BLOCK_COMMENT` as trivia and attaches it into the `LogicLine`'s own token run, so `~ // TODO: …` passed the five-accessor check exactly like a genuinely bare `~` and the fix silently deleted the author's comment — measured before the fix (`~ // TODO: bump the score here` produced one `Safe` fix whose applied result dropped the `TODO` text entirely), which is not `Safe` by `docs/autofix-spec.md` §3's own definition (Safe requires no lost text). A multi-line `/* … */` compounded this: because it is one trivia token, the `LogicLine` node's own range already spanned every line the comment covered, so the pre-review "whole physical line" extension deleted all of them, not one.

## Auto-fix: a tested end-to-end usability pathway before more fixer implementations
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** auto-fix (studio surfaces, brink fix, LSP) / process
- **SCOPE:** moderate — sequencing of the #3374 epic
- **WHAT:** With a handful of Safe fixers working (E025 add-import, E014,
  E092, E095, E110, E031/E176 after waves D–F), the next work is wiring
  and proving the *usability* of auto-fix end to end — the Problems-panel
  Fix row and "Fix all safe (N)", the editor and Problems context menus,
  the command palette, fix-on-save under the app setting, `brink fix`
  over a real project, and LSP quickfixes — each covered by an e2e or
  integration test on a fixture project that actually carries fixable
  diagnostics, plus the usability bugs the reviews surfaced (#3447,
  #3459, #3462, #3463, #3464). The remaining fixer implementations
  (#3429 and the Suggested/Placeholder tiers) are backlogged until that
  pathway exists and is tested.
- **WHY:** Maintainer, 2026-09-03: "if we have a handful of fixes
  working, i'd like to start working on wiring through the actual
  usability of these, and then we can backlog the remaining fix
  implementations, once they have a tested end-to-end pathway to be
  usable." Every fixer PR so far was verified through Rust
  `EditorSession` tests and vitest, never by an author clicking Fix on a
  real project; more fixers add nothing an author can use until the
  pathway is proven, and the pathway's own bugs (fixes offered for
  `[lints]`-allowed codes, fix-on-save persisting only the focused file)
  are already known.

## A fix surface must intersect its own diagnostic road's severity resolution
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** auto-fix (`brink-web` fix road, `brink-ide::fix::select`)
- **SCOPE:** moderate
- **WHAT:** Every auto-fix surface subtracts the diagnostic codes its own
  road suppresses before any other narrowing. `ProjectDb::diagnostics` is
  the RAW per-file list and carries two kinds of suppressed diagnostic:
  the `@[allow(…)]` / `#@allow` source channel (already handled by
  `brink_ir::suppressions::apply_suppressions`) and a `[lints] X = "allow"`
  project setting, which `brink_analyzer::effective_severity` answers
  `None` for and which the Problems panel therefore never renders. The
  subtraction lives on `Select::excluded_codes`, so `fix_offers`,
  `fix_count` (the `N` in "Fix all safe (N)"), `fix_all` and the cursor
  menu inherit it from one place; `brink-web` fills it by asking
  `effective_severity` about every `DiagnosticCode::ALL` member rather than
  re-deriving which levels suppress. It is a subtraction, not a `codes`
  whitelist, so it also applies to an unrestricted selection and a caller
  naming the code explicitly cannot reverse it. Recorded in
  `docs/autofix-spec.md` §5 and §7.
- **WHY:** Issue #3459. PR #3454's own docs claimed `fix_offers` shows
  "exactly what the Problems panel sees", and it did not: a `Warning`-base
  fixable code turned `"allow"` was fix-counted into "Fix all safe (N)" and
  batch-rewritten with no visible row to explain the edit — an editor
  silently changing a manuscript over a problem the author had explicitly
  turned off. The severity policy is per-caller (the CLI resolves its own
  `AnalysisOptions`), which is why the suppressed set is an *input* to
  `Select` rather than something `collect` derives; the LSP surface had
  already reached the same conclusion independently in #3422, and this
  makes the rule the spec's rather than one surface's.

## `file.save` routes a cross-file fix-on-save write through `file.saveAll`'s own confirm→retire algorithm, narrowed to the touched set
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** brink-studio (`file-commands.ts`), brink-desktop
- **SCOPE:** minor/local
- **WHAT:** `file.save`'s fix-on-save step now inspects `runFixOnSave`'s own return value — every path the batch actually rewrote. When that names files besides the one being saved, `file.save` no longer calls `project.save([path])`; it calls a shared `hostSaveBatch` helper (the exact per-path confirm→retire dance `file.saveAll` already used, factored out rather than duplicated) with the write narrowed to `[path, ...otherWritten]` — not the whole dirty set, which stays Save All's job. A toast names the OTHER file(s) written; the focused file's own `Saved <path>` notice and fix-on-save's no-toast-of-its-own rule are unchanged.
- **WHY:** Issue #3462: `file.save` always narrowed its host-save write to the focused path, so a cross-file fix batch (currently latent — no registered fixer produces one yet, but `runFixOnSave` already supports it) would leave the other file staged and silently dirty while the save reported success. Reusing `file.saveAll`'s existing, already-swept confirm→retire call site (rather than adding a second one) means the race-safety property `save-retire-invariant.test.ts` pins for that call site covers this new caller too, with no new `SAVE-PATH` id or driver to maintain.

## Peek: hovering a Player transport action forecasts what it will hit
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** player / editor-ui / runtime
- **SCOPE:** moderate
- **WHAT:** Hovering Continue in the Player forks the live story (the F4 `Speculation`, sandboxed, clone-run-drop), runs exactly ONE continue call — never the auto run — and highlights the source of what it produced (a line, or the presented choices) in the editor, then discards the fork. Externals stay sandboxed in a peek; a forecast that diverges from the real press because an external was not called is acceptable. Hovering a choice card does both: its own text lights as delivered-content hover, and the one-line result of picking it lights as a peek. The speculation exposes the same current-path query as the live story, read before its advance, so a peeked line carries its knot exactly like a transcript row. No knot-change chip on the peeked line for now.
- **WHY:** The permanent live band already sat on the next line, but only as a side effect of the runtime position — at a choice point it landed on the first choice and said nothing about what a specific action would do. A forecast tied to the hovered action answers "what happens if I press this" directly, and the speculation facility built for the watch REPL already gives a side-effect-proof fork at the exact position, so this is wiring, not new machinery.

## Editor execution bands: tint means state, the bar means attention
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** editor-ui
- **SCOPE:** moderate
- **WHAT:** Execution highlights split into two channels that stack on one line. The whole-line TINT is owned by the runtime position — `live`, `paused` (with the gutter arrow), `frame`, and the choice-point set — exactly one per line. The inset left BAR is transient attention: `follow` (solid accent, the line just revealed), `hover` (solid muted, delivered content under the pointer), and `peek` (dashed accent, a forecast, styled as "not yet real", no gutter glyph). `follow`/`hover`/`peek` are bar-only — they carry no tint of their own — and the policy no longer dedupes them against a tinted line. The editor's own active-line (cursor) highlight gets its own colour when it lands on a tinted line, so the two coexist instead of one hiding the other.
- **WHY:** The bands competed for one channel (the background), so the policy had to pick a winner per line and the debugger's stop marker, the follow band, and any forecast could never show together on the line they all cared about — the stepping case makes this concrete: the paused tint marks the stop, and a hover over Continue/Step must draw its forecast over it. Giving each meaning its own channel makes the rule teachable in one sentence and lets a hovered choice show both its text and its consequence at once.

## Structural rails: rightmost gutter, one compact hover for the whole stack
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** editor-ui (HIR rails gutter, `packages/ink-editor/src/hir-overlay.ts`, studio `editor.css`)
- **SCOPE:** moderate — presentation of the rails column; the HIR projection it draws from is unchanged
- **WHAT:** (1) The structural rails column moves to the RIGHT of every
  other gutter — after line numbers and the play/breakpoint gutter,
  adjacent to the text. (2) The stacked rails become ONE hover target: a
  single tooltip for the whole stack at that line, listing each colour
  with its constituent (knot / stitch / choice / gather / branch name and
  line range), instead of a tooltip per bar. (3) The lanes pack with no
  gap between bars, so the column is more compact.
- **WHY:** Maintainer, 2026-09-03, during the auto-fix drive-it on a
  real project: "maybe first we can move the structural rails to the
  right of the current gutter. i'd also like to make them a single hover,
  which shows each color and its constituents, so it can be a little more
  compact, and we don't need gaps between them." The per-bar hover made
  the 3px bars a fiddly target and the gaps spent width on nothing; a
  single legend-style hover reads the whole nesting at once, and sitting
  next to the text the rails line up with the structure they describe.

## Prose checking runs in its own worker, not the session worker
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** editor-perf (`packages/brink-studio/src/prose-worker.ts`, `prose-checker.ts`, `crates/brink-prose`)
- **SCOPE:** moderate — where prose checking runs; its results and the `ProseChecker` interface are unchanged
- **WHAT:** The `brink-prose` wasm module moves off the main thread into a
  **second, independent** Web Worker rather than becoming a capability of the
  session worker (`docs/editor-worker-spec.md` §15). The worker imports the
  module lazily, on the first check; a check superseded by a newer edit is
  dropped before it is posted; no-`Worker` environments and worker crashes
  fall back to the in-process road. Separately, `brink-prose` caches the
  merged dictionary and the curated `LintGroup` across calls, keyed on the
  project's words and the dialect.
- **WHY:** Measured on main (#3491): one check took 651 ms on a real
  1,125-line file and 4.8 s p95 on the 8k-line perf fixture, all on the main
  thread, 700 ms after the author stopped typing. The work is genuinely
  O(document) and cannot be made free — it can only be moved. It stays a
  SEPARATE worker for the same reason the crate is separate: 6.5 MB gzipped
  against `brink-web`'s 2.6 MB, downloaded only if someone checks prose, so
  folding it into the session worker would either tie that download to boot
  or make boot conditional on a feature most consumers never use.

## A gutter's per-line callback never reads a host hook
- **WHEN:** 2026-09-03
- **PROJECT:** brink
- **SYSTEM:** editor-ui (`packages/ink-editor/src/play-from-here.ts`,
  `packages/brink-studio/src/execution-highlights.ts`)
- **SCOPE:** moderate — a standing contract for every host seam a
  CodeMirror gutter reads, not just this one
- **WHAT:** `gutter({ lineMarker })` runs once per visible line, so a host
  hook must be read once per render and shared across the lines, never
  called from inside the callback. Host truth reaches a gutter only
  through an explicit refresh effect, and a refresh dispatches a
  transaction — so caching a host answer per `EditorState` is exactly
  "once per render". That cache is only safe while every path that can
  show a view re-dispatches the refreshes: a host answer that changes
  while the view is unmounted leaves no transaction behind, and a reused
  `EditorState` would serve the pre-unmount answer, so mounting must
  self-serve them (found on this PR's review; the same hole #518 fixed
  for the overlay). Corollary for the
  other side of the seam: a host callback must not eagerly evaluate an
  expensive argument the policy consults on only one branch — pass a
  thunk (`executionHighlightsFor`'s HIR projection).
- **WHY:** Measured on #3490: the play gutter's arrow-vs-dot decision
  called the studio's `getExecutionHighlights` once per visible line, and
  that hook eagerly pulled the file's HIR projection — 10,045 synchronous
  whole-document `getHirSpansDoc` wasm calls across a 228-keystroke burst
  on a 1,125-line file (~38 per keystroke, p50 input latency 48 ms), all
  of them computing an answer that was `[]` because no session was
  running. The cost is invisible at review time because the callback
  itself looks like a cheap lookup; the multiplier lives in CodeMirror.

## Lift-order hoist: prefix interpolations evaluate into hidden temps before a lifted construct; a direct call keeps display-position capture
- **WHEN:** 2026-09-04 (implements the 2026-09-02 #3395 ruling, option B)
- **PROJECT:** brink
- **SYSTEM:** compiler (`hir::normalize`, LIR `DeclareTemp`, codegen), format (`DebugInfo` locals row), runtime/wasm/studio (locals views) — `docs/compiler-spec.md` "Normalization pass", `docs/debugger-spec.md` §3
- **SCOPE:** moderate
- **WHAT:** (1) `normalize_file`'s lift now evaluates every interpolation LEFT of the lifted construct — calls and pure reads alike, spans included — in source order into synthetic `~ temp`s (`TempDecl::synthetic`, `$lift{n}`) declared before the lifted statement; every branch clone reads the temp. Each expression evaluates once; stateful alternatives in the prefix stay shared clones (#3275); the suffix is the next lift level's prefix; a synthetic read is never re-hoisted. (2) The temp name uses `$`, not an identifier character, so an authored temp can never alias it through `alloc_temps`'s name-keyed dedup — the ruling's `_l{n}` spelling would have. (3) **Codegen composes a synthetic temp's direct-call value the way `emit_slot_expr` composes a call in a slot** (side-effect output captured into the `FragmentRef` with the return value), so `{$lift0}` replays a printing function's text where `{f()}` stood. Measured with inkjs 2.4.0 as the reference: a literal `~ temp t = shout()` hoist prints `X` on its own line then `ayes` in ink and brink alike, while the original `a{shout()}{n == 1:yes|no}` prints `aXyes` — the composition is what makes option B observably equivalent for the common `{describe()}{cond:…}` idiom rather than trading one divergence for another. An authored `~ temp x = f()` is NOT composed (ink emits that call's text immediately). (4) The `DebugInfo` locals row gains a `synthetic` bit (section version 2: the `has_range` byte is now a flags byte, reserved bits rejected; `.inkt` spells it `(local 2 "$lift0" synthetic (range …))`), carried through `DebugLocal` → `wasm-types` → the studio's Debugger and State View, which hide those rows. (5) The two #3395 `expected_mismatch` cases flip and their flags are removed; `RATCHET_EPISODE_COUNT` 5622 → 5624; compile-and-play regression tests pin seven shapes against the reference (fn-then-cond `1yes`, read-then-effectful-cond `0yes`, seq-fn-cond ×3, cond-then-fn control, text-emitting call `aXyes`, void call, two constructs with calls between, a `once` sequence's exhausted branch). Still owed, maintainer-local: the reverse-shape C# oracle case the ruling asked for (`{n}{f() == 1:yes|no}`), which needs dotnet.
- **WHY:** The ruling chose hoisting for order and for evaluating each expression exactly once; building it exposed that a hoisted call's *output* is ordered too, and that the compiler already had the right primitive for display-position calls (the fragment composition, `docs/decision-log.md` 2026-08-01 "Content-as-value"). Reusing it keeps the fix inside the ruled shape — no lazy-condition variant, no effect-row gate — and `a{shout()}{…}` is the only shape where the two differ. Hiding the temps (rather than not minting them) keeps the debugger honest about slots that exist while showing the author only what they wrote.

## The inkjs oracle: a sanctioned stand-in for the C# reference, with .NET's generator installed
- **WHEN:** 2026-09-04 (implements `docs/program-generator-spec.md` §6, RULED 2026-09-02; issue #3379)
- **PROJECT:** brink
- **SYSTEM:** test tooling (`tools/inkjs-oracle`, `brink-test-harness::inkjs`, `brink-gen` differential), CI (`ci.yml` `inkjs-sanction`, `inkjs-differential.yml`)
- **SCOPE:** moderate
- **WHAT:** (1) `tools/inkjs-oracle` ports `tools/ink-oracle`'s crawler (DFS over choices, one step per `Continue()`, `state.ToJson()`/`LoadJson()` snapshots, variable observers, visit-count diffs, `storySeed = 0`, external fallbacks on) onto inkjs 2.4.0's unbundled compiler, emitting `OracleEpisode.cs`'s JSON field for field. It only ever writes to a caller-named output directory — never next to a golden; the C# oracle alone blesses goldens. (2) **The "RNG seed mapping" §6 asked to prove does not exist; the generator is replaced instead.** inkjs draws every shuffle, `RANDOM` and `LIST_RANDOM` from a Park–Miller PRNG; the C# runtime from `new System.Random(seed)`. `dotnet-random.mjs` ports .NET's subtractive generator (the port `brink-runtime/src/rng.rs` already carries, same test vectors) and installs it over `inkjs/engine/PRNG`'s export — possible only because the compiler module path shares the engine's CommonJS modules; the rollup bundle `inkjs/full` inlines its own copy and is deliberately not used. (3) The sanction (`tests/inkjs_sanction.rs`, per PR): every oracle golden — 414 cases, tier1–3 plus four GitHub-corpus cases — replayed through inkjs matches: 400 byte for byte, 14 after exactly two normalisations, both artefacts of the C# tool rather than of ink semantics: its no-`onError` error wrapper (`Ink had 1 error. It is strongly suggested…`) with the maintainer's absolute source paths, and double- vs single-precision float printing (`0.6666666666666666` vs `0.6666667`; the normaliser only rewrites tokens with more significant digits than an `f32` can print, so `0.0` stays `0.0`). `KNOWN_DIVERGENCES` there is empty and checked both ways like `expected_mismatch`. Warn-and-continue is the tool's default (§6), `--strict-warnings` the C# tool's mode; no golden needed strict to match. (4) The differential (`brink-gen/tests/inkjs_differential.rs`, `Profile::PLAIN_INK`, `diff_oracle` on both sides after the same normalisation) runs nightly with 512 cases, advisory, and on PRs touching the lane. Its first 300 stories found two brink divergences from ink, filed with shrunk reproductions — #3507 (`{0} <>` then a line: ink `0 world`, brink `0world`) and #3508 (`* [a  0]`: ink presents `a  0`, brink `a 0`) — and carries them as issue-keyed predicates so the lane is green until something new appears. (5) `CLAUDE.md`'s trust section names the tool as the cloud session's way to ask what ink prints for a shape the corpus does not settle — evidence for a ruling, not the ruling.
- **WHY:** The C# oracle needs `dotnet`, so a cloud session could neither answer "what does ink do here?" nor run a reference differential over generated stories. §6 ruled inkjs as the Node-only reference but made its use conditional on reproducing the goldens; measuring that turned up the PRNG mismatch (which would have made every shuffle golden a false divergence) and, once fixed at the source, a corpus-wide match that needed no per-case allowances. Splitting the lanes as §7 ruled keeps the deterministic sanction on the merge gate and the randomised differential where a new finding is a bug report, not a blocked unrelated PR.

## Whitespace before glue: a spring after an inline construct, and glue trims whitespace-only text with the newline it consumes
- **WHEN:** 2026-09-04 (issue #3507; maintainer: "fixed with springs")
- **PROJECT:** brink
- **SYSTEM:** compiler (`hir::lower::content`, `block/branchless.rs`), runtime (`output::resolve_parts` / `resolve_lines_annotated`) — `docs/compiler-spec.md` "Content and ContentPart", `docs/runtime-spec.md` glue resolution
- **SCOPE:** small
- **WHAT:** (1) The space between an inline construct's `}` and `<>` lowers to `ContentPart::Spring`. The lexer folds a space after TEXT into the TEXT token (`hello <>` always kept its space), but after `}` it is a WHITESPACE trivia token the parser skips before `GLUE_NODE`, and content lowering only looked at node children — so `{0} <>` / `world` printed `0world` where ink prints `0 world`. Three loops mint the spring (`lower_content_node_children`, the post-promoted-block trailing parts, the branchless conditional body); the else-bearing branch body already carried the whitespace as deferred text, which codegen turns into the same spring. Only an inline construct earns it — after TEXT the space is in the text, after another glue there is nothing to separate. A `Spring`, not `Text(" ")`: whitespace-only text would leak into line recognition and the line tables. (2) The first cut made the differential find the other half: when the construct renders EMPTY, ink's glue (`TrimNewlinesFromOutputStream`) removes the trailing newline AND every whitespace-only string after it, so `a` / `{false:x} <>` / `b` prints `ab`. Both runtime resolvers now track where the current line began (a glue-removed newline included) and, on glue, drop whitespace-only text since that point; with content on the line the newline is not trailing and the space survives. `glue_skips_whitespace_only_text_to_find_newline` had pinned `a b` — the divergence itself — and now pins `ab`. (3) The 512-case CI run found the third piece: inside an else-bearing multiline conditional the deferred whitespace lowered as `Text(" ")`, which the lift folded into a whitespace-only LINE (`emit_line " "; glue`) that the runtime's glue scan took for content — so `a` / `{false:a} <>` / `-> END` in an else arm delivered `a\n` where ink delivers `a`. The else-arm path now mints the same spring as the other three, and `mark_glue_removals` lets a whitespace-only or empty `LineRef` pass the way `is_content` already classifies it. Sixteen compile-and-play shapes pinned against inkjs, trailing newlines included; ratchet and native goldens unchanged; the #3507 entry leaves the differential's `KNOWN_DIVERGENCES`. Widened by (1): the native emitter still refuses `Spring` (2026-08-02 ruling, #1976), so respelling an ink line of the shape `{x} <>` is refused until #1976's emitter round-trip lands — noted on that issue. Still owed, maintainer-local: the C#-golden corpus case (dotnet); the shapes go into `tests/tier4-generated` when #3380 lands.
- **WHY:** The spring is the marker the pipeline already reserves for "a space that may or may not render", and the runtime's resolution of it is the one place ink's conditional-whitespace rule can be stated once for both the empty and non-empty cases; a parser change (making the trivia a TEXT node) would have changed the CST shape for every tool and put whitespace-only text where the recogniser deliberately treats it as structural.

## The capture tier: tests/tier4-generated, promoted by script, blessed by inkjs until dotnet re-blesses
- **WHEN:** 2026-09-04 (implements `docs/program-generator-spec.md` §5, RULED 2026-09-02; issue #3380 route 1)
- **PROJECT:** brink
- **SYSTEM:** test corpus (`tests/tier4-generated/`), `brink-test-harness` (`corpus::collect_generated_cases`, `tier4_generated.rs`, `corpus_report`), `scripts/promote-generated.mjs`
- **SCOPE:** moderate
- **WHAT:** (1) A case is `story.ink` + `oracle/*.oracle.json` (the C# oracle's episode shape, so `diff_oracle` compares it unchanged) + `case.toml` with `[provenance] source/property/seed/oracle-source/issue` and, when it pins an open bug, `[source] expected_mismatch` with the curated corpus's two-way rule. (2) The shared corpus walk prunes `tier4-generated` by name, so the tier can never leak into `RATCHET_EPISODE_COUNT`, the inkjs sanction, the parallel gate or the respell sweep; `tier4_generated.rs` is its own must-pass target and `GENERATED_CASE_COUNT` there moves only through a promotion (the script bumps it; a hand-added or removed case fails by count). (3) `pnpm promote:generated` takes a `.ink` or a saved failing run's `--- source ---` block, refuses a story brink rejects or the inkjs oracle cannot golden, and writes the case; `--rebless-csharp` is the maintainer-local dotnet step that flips `oracle-source`. Every spawn carries a timeout. (4) First four cases, all promoted through the script: the two #3507 shapes now passing, and the #3508 and #3510 shapes flagged until fixed — the shapes this session found before the tier existed.
- **WHY:** The differential and the probes were producing findings faster than dotnet-blessed corpus cases could absorb them, and a finding without a checked-in reproduction is a note in an issue. inkjs is sanctioned against every C# golden, so a golden it produces is evidence at that strength — enough to pin a regression, honestly labelled as not yet C#-blessed, with one command to graduate it when a maintainer has dotnet.

## Choice display text keeps whitespace runs; output lines still collapse at compile time
- **WHEN:** 2026-09-04 (issue #3508)
- **PROJECT:** brink
- **SYSTEM:** codegen (`brink-codegen-inkb`, `in_choice_display`) — `docs/intl-spec.md` "Hash computation" note
- **SCOPE:** small
- **WHAT:** inklecate keeps text verbatim (`"^a  0"` in the compiled JSON); the C# runtime collapses whitespace runs only when it renders an output line (`CleanOutputWhitespace`) and presents a choice's text as the evaluated string trimmed at the ends (`Trim(' ', '\t')`), interior runs and tabs untouched. brink collapses at compile time, in `add_line_with_hash`/`add_template_line`, into the line table — observably identical for output lines, wrong for choice text (`* [a  0]` presented `a 0`). Codegen now emits a choice's display entries verbatim (`emit_choice` sets `in_choice_display` around the display emission; the start text's OUTPUT copy is a separate content statement and collapses as before). `source_hash` is computed before either treatment, so line identity is unchanged; the display entries' text — what XLIFF exports for such a choice — is now the text the player sees. Six compile-and-play shapes pinned against inkjs (bracket, start+bracket+suffix, tabs, end-trimming, a templated display, conditioned and tagged); the tier4 case `choice-text-whitespace-run` flips to passing; the differential's `KNOWN_DIVERGENCES` is empty.
- **WHY:** The narrow fix, at the layer that made the wrong call: moving the collapse to the runtime (ink's model) would have changed the line-table text of every output line with a run — a translation-visible change for no observable gain — so the compile-time collapse stays for output lines and choice text simply stops being subjected to it.

## Toolchain pinned to Rust 1.98.1
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** `rust-toolchain.toml` (the single source every CI lane and `scripts/setup-dev.sh` derive from)
- **SCOPE:** small
- **WHAT:** 1.97.1 → 1.98.1 (released 2026-09-01), maintainer-requested. Three new pedantic findings fixed at their sites: `clippy::chunks_exact_to_as_chunks` (the weighted-pair walks in `brink-ir`'s LIR lowering and `brink-runtime`'s collection ops now use `as_chunks::<2>()`), `clippy::map_or_identity` (`brink-ide`'s import/include block scans use `unwrap_or`), and `.ok().is_some_and(..)` → `is_ok_and` in the harness's corpus predicates. rustfmt 1.98 reformats nothing. Workspace clippy (default and `--all-features`), the wasm32 check of `brink-web`, and the workspace test suite are green under the new toolchain in a cloud session; `bevy-brink` and the `src-tauri` workspace cannot be linted there (no GTK/wayland) and are covered by their own CI lanes.
- **WHY:** The repo pins deliberately (no Dependabot ecosystem for the file); a point release that has been out for days with no regressions reported is the routine cadence, and every later PR in the autonomous plan runs clippy, so the bump goes first.

## Source-transformation equivalence over generated stories: the formatter is Safe, the respeller has four defects on record
- **WHEN:** 2026-09-04 (`docs/observable-semantics-spec.md` §4.1)
- **PROJECT:** brink
- **SYSTEM:** `brink-gen` (`tests/equivalence.rs`, `Profile::inline_conditionals`, `Profile::RESPELLABLE`), `brink-fmt`, `brink-respell`
- **SCOPE:** moderate
- **WHAT:** (1) `trace(P) = trace(fmt(P))` holds, with translation identity (§2.2), on every generated `plain_ink` story measured — the formatter is *Safe* in §5's sense over the generator's reach. (2) `trace(P) = trace(respell(P))` does not hold yet: the property's first 300 stories shrank to four emitter defects, each filed with a one-line reproduction and carried as an issue-keyed known divergence (the differential's discipline) — #3515 (fallback choice as an `else` arm the native parser rejects), #3516 (`not` as a bare word, operand parentheses dropped), #3517 (ink `VAR` → module-private `var`, host-readable globals lost; needs a ruling on emitting `pub var`), #3518 (nested binary expression loses its parentheses, values change). (3) The emitter refuses most of `plain_ink` (inline conditionals in content ×225 of 300, springs ×35), so the generator gains its first data knob, `Profile::inline_conditionals`, and a `RESPELLABLE` profile the respell property runs on with a one-in-ten non-vacuity floor (72 of 300 traced equal); the plain-ink run prints its tally by construct and asserts only that nothing accepted diverges. (4) A refusal (`EmitError::Unsupported`) is a skip, never a failure — the emitter's honest "no faithful spelling" is not a divergence; output that does not compile is.
- **WHY:** The generator exists to serve exactly these properties (#3370); running them on day one measures where each transformation stands instead of assuming. A per-PR property that fails on a known, filed defect blocks unrelated PRs, and a property that passes only because the emitter refused every input proves nothing — the tally and the floor keep both honest until the issues close and the floor rises.

## Generator functions tier: typed calls, `ref` parameters, a DAG call graph; five printing-function divergences on record
- **WHEN:** 2026-09-04 (`docs/program-generator-spec.md` §7)
- **PROJECT:** brink
- **SYSTEM:** `brink-gen` (`model::{Function, Param, FnSig}`, `Expr::Call`, `Item::Call`, `strategy::{RawFunction, decode_functions}`, `Profile::{max_functions, max_params}`), `tests/inkjs_differential.rs`
- **SCOPE:** moderate
- **WHAT:** (1) The model gains functions (rules 8–9): a fixed position in `Story::functions`, typed parameters with `ref`, a body of items only (ink forbids diverts and choices in a function; an empty function is rejected by inklecate, so the decoder pads with a line), an optional `~ return expr` whose type is inferred against the body's scope. (2) Calls are typed like every other expression: arity and argument types against the signature, a `ref` argument must be a visible variable of the parameter's type, a value function is legal only in expression position and a void one only as a `~ f(x)` statement. (3) The call graph is a DAG by construction — function `i` may call only functions `< i`; flow code may call any — so termination stays a property of the model, not of the explorer's step limit. (4) The raw skeleton carries `RawExpr::Call`/`RawItem::Call` as byte indexes into the callable functions of the wanted return type, decoded with the same fall-back discipline as variables (no suitable function, or no variable for a `ref` parameter → the byte reads as a literal / the statement drops), so every skeleton still decodes to a valid story and shrinking stays monotone. (5) `Profile::max_functions` (2 in `DEFAULT`/`PLAIN_INK`, 0 in `STRUCTURE`/`RESPELLABLE`) and `max_params` (2). (6) The tier's first 300-story differential run found five divergences, all in the one shape the hand-written corpus covers thinly — a function that *prints*: #3519, #3521, #3522, #3523, #3524, each filed with a one-line reproduction and carried as a `KNOWN_DIVERGENCES` predicate until fixed (#3521 needs a ruling: it is the #3395 lift's "reverse shape").
- **WHY:** The ruled ladder puts functions before tunnels and threads; the DAG rule and the void/value split are what keep "valid by construction" true without a filter. The five findings are the tier's justification in one run: none of them is reachable by a story without a printing function, and every one is a real compiler or runtime defect, not a generator artifact.

## Function-end trim treats glue as transparent (#3522)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`OutputBuffer::trim_function_end`) — `docs/runtime-spec.md` "Glue"
- **SCOPE:** small
- **WHAT:** On function return the trailing-whitespace trim now skips over `Glue` parts instead of stopping at them, removing the `Spring`/`Newline`/whitespace-only text beneath: `{0} <>` at the end of a function followed by `a` prints `0a` (ink), not `0 a`. Found by the functions tier of the program generator; four compile-and-play regression tests pin the shape and its neighbours (text before the glue keeps its own space, a newline under the glue goes, the same in a display-position call) against inkjs; the oracle ratchet is unchanged at 5624.
- **WHY:** The C# loop `continue`s past every non-text object and only `break`s on non-whitespace text; brink's `break` on the first non-whitespace *part* conflated "a glue object" with "text that ends the trim". Matching the walk, not the outcome of one case, is the fix that stays correct for the next shape.

## Every multi-line conditional arm starts with a newline (#3523)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** compiler (`hir::lower::conditional::{multiline, conditional_with_expr}`) — `docs/compiler-spec.md` "HIR responsibilities"
- **SCOPE:** small
- **WHAT:** The explicit arms of a multi-line conditional (`- cond:`, `- else:`, switch arms) now lower with a leading `Stmt::EndOfLine`, as the implicit first arm of `{ cond:` (its first-newline rule) and every block-sequence branch (`lower_block_sequence`) already did. inklecate compiles every multi-line branch body as `["\n", "^text", "\n", …]`, verified across the switch, cond-list, branchless-with-else and content-on-the-marker-line forms with inkjs. The runtime's newline dedupe hides the newline whenever output already ends in one; a printing function called in the condition is the shape that exposes it (`{ (1 < f()): x - else: b }` with `f` printing `a` is `a` / `b` in ink and was `ab` in brink). Six compile-and-play regression tests pin the forms; the oracle ratchet is unchanged at 5624.
- **WHY:** Found by the functions tier of the program generator. The three lowering sites disagreed with each other, not just with inklecate: the fix makes the rule uniform ("a multi-line arm starts with a newline") rather than special-casing the one shape that showed.

## A function's newline is dropped while the function has printed nothing (#3519)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`OutputBuffer::{mark, push_newline_in_function}`, `CallFrame::function_output_start: Option<OutputMark>`, `vm.rs` `EmitNewline`) — `docs/runtime-spec.md` "Glue"
- **SCOPE:** small
- **WHAT:** The function frame's call-time output mark now carries which target it names (capture/fragment depths) as well as its length, and `EmitNewline` reads the top frame: while that frame is a function and nothing content-bearing has been pushed past its mark on the same target, the newline is dropped, matching ink's `functionStartInOutputStream` rule. A capture or fragment begun inside the function (different depths) or a tunnel/thread frame on top disables the rule, as in C#. `x{f(f(true))}y`, with `f` starting on a conditional block, prints `xaatruey` (ink) instead of `xa` / `atruey`; five compile-and-play regression tests pin the nested-call, temp-initialiser, lifted-prefix, single-call and printed-then-newline shapes against inkjs; the oracle ratchet is unchanged at 5624.
- **WHY:** Found by the functions tier of the program generator. brink's `push_newline` suppressed a leading newline only through the enclosing scope's "no content yet" check, which is what C# does for *lines*; the function-level rule is a second, frame-scoped mark, and the corpus never had two printing calls on one line to tell the two apart. The mark records the target's identity because the length alone is meaningless once a capture begins inside the function — the same reason C# resets the mark on `BeginString`.

## A slot containing a call composes like a slot that is a call (#3525)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** codegen (`ContainerEmitter::emit_slot_expr`), LIR (`Expr::contains_function_call`) — `docs/compiler-spec.md` "Normalization pass"
- **SCOPE:** small
- **WHAT:** `emit_slot_expr` now applies the 2026-08-01 "Content-as-value" fragment composition to any slot expression that contains a function call anywhere inside it — `{f() == "x"}`, `{n + f()}`, `{f(true) and f(true)}` — not only to a bare `{f()}`; the same predicate gates the #3395 synthetic-temp composition. A compound slot was evaluated bare before `emit_line`, so a printing call's text reached the transcript ahead of the line's earlier content: `a{(f() == "alpha")}` printed `a!atrue` where ink prints `aa!true`. The predicate walks the operator, coalesce, builtin-argument, function-value, collection-constructor and indexing shapes; a call through a function value counts; the native-only lambda-taking sequence operations are not walked (their callees are values whose bodies are invisible here either way). A call-free compound slot is still evaluated bare — the composition's empty-fragment rules (#1839) are not spread to every `{n + 2}`. Six compile-and-play regression tests pin the comparison, arithmetic, two-calls-under-`and`, bare-call, no-prefix and call-free shapes against inkjs; the oracle ratchet is unchanged at 5624.
- **WHY:** Found by the functions tier of the program generator. The composition exists because ink evaluates a slot at its display position, output included; whether the call sits at the slot's root or under an operator does not change where ink puts its text, so the predicate, not the mechanism, was the gap. Keeping call-free slots bare keeps the bytecode of every ordinary interpolation byte-identical.

## `Choice.index` numbers the visible choices (#3527) — reverses the raw-position contract
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`FlowInstance::choose`, `collect_choices`, `Story::resolved_choices_for`, `Choice::index`) — `docs/runtime-spec.md` "Choice selection"
- **SCOPE:** small, but a public-contract reversal
- **WHAT:** `Choice.index` is now the position among the *visible* choices, numbered contiguously, and `choose(index)` maps it to the `pending_choices` position (the runtime's own fallback auto-selection keeps addressing the pending list). Before, the index was the raw pending position — an invisible fallback ahead of a visible choice made the visible indices skip (`0, 2`) — and a unit test pinned that on purpose (`choice_index_is_raw_pending_choices_position_with_invisible_default_mixed_in`: "a caller must never re-derive the index from array position"). That contract existed for internal consistency with `choose`, not for conformance: ink's `Choice.index` numbers `currentChoices`, which holds visible choices only, and `ChooseChoiceIndex` takes that number, so the harness's episode diff (which compares `index`) diverged on any shape with a fallback before a visible choice — a thread's fallback merged ahead of the main flow's choices, found by the tunnels-and-threads tier of the program generator, or a `+ ->` written first. `InvalidChoiceIndex.available` now reports the visible count. Every consumer that echoes `choice.index` back into `choose` (studio, wasm, bevy) stays consistent; the debug snapshot's indices derive from the same visible enumeration. Four compile-and-play regression tests pin the thread-fallback, fallback-first and out-of-range shapes against inkjs; the oracle ratchet is unchanged at 5624. **Flagged for the maintainer:** a journaled transcript recorded before this change replays a different choice only for a fallback-before-visible shape (the session journal's label-drift check is the soft guard).
- **WHY:** The runtime's public choice API mirrors C#'s, and a value the reference numbers `0, 1` that brink numbers `0, 2` is an observable divergence, not an implementation detail; re-deriving an index from a visible list is exactly what every ink host does. Mapping inside `choose` keeps the internal invariant (pending positions) without exposing it.

## Generator tunnels-and-threads tier: flow kinds, a DAG of tunnel calls, threads from plain knots only
- **WHEN:** 2026-09-04 (`docs/program-generator-spec.md` §7)
- **PROJECT:** brink
- **SYSTEM:** `brink-gen` (`model::{FlowKind, Exit::TunnelReturn, Item::{TunnelCall, Thread}}`, `strategy::Site` kind ranges, `Profile::{max_tunnels, max_threads}`)
- **SCOPE:** moderate
- **WHAT:** (1) Every knot has a `FlowKind` (rules 10–11): the entry is a plain knot; a tunnel knot is entered by `-> t ->` and leaves by `->->`, `-> END`, or a divert that stays among tunnel flows — never `-> DONE`; a thread knot is entered by `<- t` from a plain knot's weave and leaves by `-> DONE`, `-> END`, or a divert among thread flows — never `->->`. (2) Termination stays by construction: tunnel calls from flow code and from threads may name any tunnel, from inside tunnel `i` only a tunnel `> i` (a DAG, as for functions); diverts never cross kinds, so the once-only back-edge rule applies within each kind's contiguous range of the one flow table; a thread never starts another thread. (3) The raw skeleton adds tunnel and thread knot lists and two byte-indexed items; the decoder resolves a raw exit within its kind's range, reads a tunnel's `Done` as `->->`, and drops an item with nothing legal to name. `Profile` gains `max_tunnels` (2) and `max_threads` (1), 0 in `STRUCTURE`/`RESPELLABLE`. (4) The shrink-quality test now allows a tunnel or thread knot to survive beside the entry knot (shrinking deletes, never relocates). (5) The first run found #3527 — a thread's invisible fallback merged ahead of the main flow's choices made `Choice.index` skip a value — fixed alongside.
- **WHY:** Tunnels and threads are the next rung of the ruled ladder and the first constructs whose termination argument is about the call stack rather than the flow graph; keeping them as knot kinds (rather than separate lists) keeps one flow table and one exit resolver, with the kind range doing the work the DAG rule does for functions.

## Call-stack snapshot cache invalidated by every mutable frame access (#3528)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`CallStack::{last_mut, get_mut}`) — `docs/runtime-spec.md` "Fork snapshots"
- **SCOPE:** small
- **WHAT:** `CallStack::last_mut` and `get_mut` now clear `cached_snapshot` like `push`/`pop`/`materialize` already did. Sequence that exposed it: `<- t` forks the main thread (caching its frames), `~ temp t0 = 1` writes into the top frame through `last_mut` (cache untouched, now stale), the next choice's `fork_thread` hits the cache, and `choose` restores frames without `t0` — `{t0}` after the choice printed `0` where ink prints `1`. Every other path was safe by accident: a thread restored by `choose` has an empty `own`, so its first write materializes and clears the cache — which is why no corpus case saw it. Four compile-and-play regression tests pin the temp-after-spawn, printing-thread, temp-before-spawn and reassign-after-spawn shapes against inkjs; the oracle ratchet is unchanged at 5624.
- **WHY:** Found by the tunnels-and-threads tier of the program generator. A cache keyed on "no push/pop since" is a cache keyed on the wrong invariant: the frames are mutable in place, so the only safe invalidation is on every `&mut CallFrame` handed out.

## Generator lists tier: `LIST` globals as a typed value domain
- **WHEN:** 2026-09-04 (`docs/program-generator-spec.md` §7)
- **PROJECT:** brink
- **SYSTEM:** `brink-gen` (`model::{ListDecl, Ty::List, Literal::List, Expr::{Item, ListFn}, BinOp::{Has, Hasnt, Intersect}}`, `strategy::Env::{ty_of, literal, item}`, `Profile::{max_lists, max_list_items}`)
- **SCOPE:** moderate
- **WHAT:** (1) Rule 12: a `LIST` declaration is a global of type `Ty::List(i)`; its values are subsets of its items, item names are unique across the story. (2) The expression language gains list literals, single items, union/difference/intersection, `?`/`!?`, `LIST_COUNT`, and the unary built-ins `LIST_MIN`/`LIST_MAX`/`LIST_ALL`/`LIST_INVERT`, all typed by rule 12 (`+`/`-` keep their int meaning on ints; `==`/`!=` still demand one type on both sides); `+=`/`-=` on a list target are legal like on an int. (3) With lists in the story the decoder's type byte picks one of four types instead of three, and the raw expression shapes read into list operations under a list type (`Neg` → `LIST_INVERT`, `Not` → `LIST_MAX`, one int shape in five → `LIST_COUNT`, two bool shapes in ten → `?`/`!?`); without lists every existing story decodes exactly as before. (4) A list literal is never empty (a bit-free byte picks the first item): `()` as a `VAR`/temp initializer is a shape ink accepts but whose typed-empty semantics are not what this tier is for; an empty list is reached through `-`/`^`, which is where the empty-list printing rule (an empty line, in ink) gets exercised. `VAR`s stay int/bool/str. (5) `Profile::max_lists` (1) and `max_list_items` (4), 0 lists in `STRUCTURE`/`RESPELLABLE`; the respell property's #3517 predicate now also recognises `LIST` (the emitter makes it a private native global too).
- **WHY:** Lists are the next rung of the ruled ladder and the first value domain with its own algebra; keeping them as a `Ty` variant rather than a separate expression language lets every existing typing rule, scope rule and decode fall-back apply unchanged, and keeps the no-lists stories byte-identical.

## Generator sequences tier: inline alternatives, `RANDOM`, and the two bounds they need (#3538)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** `brink-gen` (`model::{SeqKind, Part::Seq, Expr::Random}` rules 13–14, `strategy::{confine_random, cap_line_variants, decode_seq}`, `Profile::{max_seq_alts, allow_random, EXHAUSTIBLE}`) — `docs/program-generator-spec.md` §7
- **SCOPE:** medium
- **WHAT:** The generator emits inline sequences in all four ink flavours (`{a|b}`, `{&a|b}`, `{!a|b}`, `{~a|b}`) and `RANDOM(min, max)` with ordered literal bounds. Two placement rules keep the model sound, and they are rules rather than bugs found: alternatives are letters, digits and spaces with no two empty ones adjacent (ink parses a `{…}` as an expression first, so `{a?|a}` and `{alt||}` do not compile under inklecate though brink accepts both), and `RANDOM` stands only in a printed interpolation. A third bound mirrors the compiler: a line's sequences enumerate into whole-line variants and the product stays within `VARIANT_CAP` (32), which `lir::lower::recognize` enforces as a hard error (E191).
- **WHY `RANDOM` is confined:** anywhere but an interpolation, a drawn value can reach a choice guard — directly, or through a variable an assignment wrote — and the set of choices offered at a point stops being a function of state alone. The harness's explorer is an exhaustive DFS with no state dedup, so a guard that flickers between visits multiplies the episode count without bound, and "which choices exist" would depend on the draw order, which is not what this tier tests.
- **ALSO:** `Profile::EXHAUSTIBLE` is new, and `tests/smoke.rs`'s exhaustive-exploration property uses it instead of the default. `DEFAULT` bounds a story's size but not its choice tree: one default story measured **39,844 episodes** — the same with and without its sequences, since the DFS branches on choices alone — against the property's 4,096 cap. The profile flattens choice nesting to one level, gives each knot one stitch and drops to three knots; the property's runtime fell from ~60s to ~3s. This is a pre-existing hazard the tier surfaced, not one it caused.
- **FOUND, NOT FIXED:** #3538 — a shuffle's seed is brink's container path hash, which cannot equal inklecate's path string, so the two pick different permutations. Already visible in the corpus (`tier2/conditional/shuffle` 0/1, `I107-shuffle-stack-muddying` 0/2) and carried as a `KNOWN_DIVERGENCES` predicate. Fixing it means adopting ink's container path scheme for seeding — a maintainer's ruling, not an implementation detail.

## The optimizer is a post-compile `.inkb` transform; pruning is a compiler step (#2336, supersedes 2026-08-06)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** compiler / optimizer — `docs/optimizer-spec.md`, `docs/reachability-prune-spec.md`, `docs/optimizer-catalogue.md`
- **SCOPE:** architectural
- **SUPERSEDES (in part):** "LIR optimization stage; reachability prune is its first pass" (2026-08-06). That ruling's SUBSTANCE stands unchanged — the mount stays universal and unconditional, the shipped artifact carries only what the project reaches, mount-on-demand stays rejected, every transform is a pure function with no iteration-order dependence. Its MECHANISM is NOT discarded either: `LIR → passes → LIR` remains the right home for the compiler's own whole-program work, and the reason that ruling gave for wanting it — constant folding and dead-branch elimination otherwise have nowhere to live — still holds for exactly those transforms, which need types, provenance and lowering's invariants and can never move post-compile. What is superseded is only the identification of that stage WITH "the optimizer".
- **WHAT:** Two things that were one. (1) **Reachability pruning is a compiler emission step**, not an optimizer pass: unconditional, no config, computed over `lir::Program` before codegen. The 2026-08-06 wording — "codegen emits only definitions reachable from the artifact's roots" — already described this. (2) **The optimizer is a post-compile `.inkb` → `.inkb` transform** in a new `brink-opt` crate depending only on `brink-format`. It is a different thing from the compiler's LIR transform layer, not a replacement for it: the two exist side by side, and whether the compiler's side generalises into a pass list now or on its second inhabitant is left open. The boundary between them: the compiler decides what to ship; the optimizer makes what ships cheaper without changing what it does.
- **WHY:** The LIR placement was chosen when the prune was the only transform in view, and the prune is the one candidate that genuinely wants LIR (it must tell project definitions from mounted ones, and only the compiler always knows — the artifact's `debug_info` is `Option` and absent from release builds). Of the five candidates now catalogued, four operate on things that do not exist at LIR: bytecode (peephole), line tables, the literal and name pools, and — the fact that settled it — effect rows, which `StoryData` ships, so the one pass whose evidence was assumed to be compiler-only works fine on the artifact. The fence is also strictly stronger post-compile: the control is the untouched artifact byte-for-byte, rather than a second compilation that must be proven equivalent through both compile roads.
- **ALSO:** the move dissolves an asymmetry rather than papering over it. As a LIR stage whose only pass was the mount prune, the optimizer did nothing for an ink program — permanently, by construction — which sat badly with ink as a peer surface. Operating on artifacts, it serves both surfaces equally, and the one native-only transform is now filed where being native-only is unremarkable.
- **STILL OPEN:** each document carries its own questions — the prune's root set (`use` as a root, whether project definitions are ever pruned, the pruned-name list, id stability) and the optimizer's exposure (whether `brink compile` runs it, whether an optimized artifact is marked, per-pass toggles). The framework document (observable surface, per-pass contracts) stays deliberately deferred until two real passes have pushed on it.
## A function's whitespace-rendering value is trimmed at its end (#3536)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`output::trim_function_end`) — `docs/runtime-spec.md` §"Resolve glue"
- **SCOPE:** small
- **WHAT:** The function-end trim now removes a trailing `ValueRef` that renders as whitespace — an empty list, `""`, a `none` — along with the newline behind it, instead of stopping at it. `~ f()` where `f` prints `{l}` for an emptied list `l` now contributes nothing, matching ink; before, it left a blank line (or, with nothing before it, made a blank first line).
- **WHY:** ink stringifies a value into the output stream as it is pushed, so `TrimWhitespaceFromFunctionEnd` sees an inline-whitespace `StringValue` and trims it. brink defers value resolution by design (`docs/` design principles: resolve at read time, so a locale swap can re-render), so the transcript holds an unresolved `ValueRef` and the trim has to make the same judgement from the value itself — `OutputPart::is_visible`, the predicate #3533 introduced for the identical question about blank lines. A visible value still stops the trim.
- **FOUND BY:** the program generator's lists tier, which is the first thing in the repo to write a function whose whole output is an empty list. Pre-existing: it reproduces identically before #3533/#3534. Ratchet unchanged at 5624.

## Blank lines at a turn boundary, and tag-only lines, follow ink's output stream (#3533, #3534)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`output::{is_visible, has_completed_line, take_first_line, flush_lines_at_yield}`, `Flow::line_delivered_this_turn`) and HIR lowering (`content::accumulator`'s `TagLineOutput`) — `docs/runtime-spec.md` "Blank lines at a turn boundary", `docs/compiler-spec.md` "Tag-only lines"
- **SCOPE:** medium
- **WHAT:** A blank line (a whitespace-only interpolation on a line of its own) between a delivered line and a turn boundary is no longer delivered — ink's lookahead evaluates it in the same `Continue` and drops it, keeping it only when non-whitespace content follows; a turn's leading blank lines collapse to one. Separately, a tag-only line no longer lowers to a blank line: it contributes only its tags, which attach to the next line, as ink's `lineIsPureTag` dictates. Both were found by the lists tier of the program generator (`{LIST_INVERT((li0_0))}` on its own line before `-> END`).
- **WHY (landed as ONE commit, against the usual one-fix-per-commit rule):** the two are the same question — what a whitespace-only line does to the output stream — and each alone is worse than either the before or the after state. With only the runtime half, a tag-only line's blank line survives as a delivered blank step (its tags make it non-blank), and `tests_github/bobon4uto__dream_on`'s snapshot churns by 72k lines before the next commit reverts it. With only the lowering half, the tags land correctly but the blank-line divergence stays. Ratchet unchanged at 5624 either way and at the end; the `dream_on` snapshot loses 1,220 mismatch entries (620 + 600 tag mismatches gone, 420 text mismatches gone) and gains none.
- **NOT fixed here (#3524):** a tag-only line that is the last thing before a boundary. ink gives the tags their own step with empty text; brink merges everything left at a yield into one `Step::Line`, so they hang on the preceding line. That merge is exactly #3524's root cause and is fixed there, not by widening this change.
- **NATIVE-VISIBLE SIDE EFFECT — flagged for the maintainer:** the same split change makes a blank line BETWEEN two content lines a delivered line, where brink used to swallow it. That is ink's behavior (`a` / `{e}` / `b` prints three lines, inkjs-verified) and the old brink swallowed it only as an accident of `take_first_line` splitting at the LAST newline before the next content instead of the first. One native golden moves with it: `tests/tier1-brink/option-verbs`, whose `{f2}` line (an `Option` holding `none`) now prints a blank line before "after bare f2 interpolation.". Its `expected.txt` is updated here. `docs/stdlib-spec.md` §1.6b (B4) rules that a final-`None` at the display boundary "must not count as content for leading-newline/glue suppression" — that rule is untouched (it governs `push_newline`), but a maintainer who reads it as also meaning "a line whose only content is `none` prints nothing" should reverse this golden and say so: the ink half of the change does not depend on it, and `none` is a native-only value the oracle cannot arbitrate.
- **ALSO FOUND, filed as #3535:** glue after a blank line (`a` / `{e}` / `<> b`) — ink prints `a b` (the glue walks back past the blank line to the earlier newline), brink prints two lines.

## List origins follow ink's constructor and retention rules (#3532)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`list_ops::{effective_origins, build, retain_origins_on_assign}`, `value_ops::list_binary_op`, `vm.rs` `SetGlobal`/`SetTemp`) — `docs/runtime-spec.md` "List origins"
- **SCOPE:** medium
- **WHAT:** A non-empty list's origins are its items' (recomputed on read); an empty list's are whatever it was built with — none for `^`, `LIST_MIN`/`MAX`/`ALL`/`INVERT` and `list + int` (fresh lists in ink), the left operand's for `+`/`-`, the input's for a non-empty `LIST_RANGE` — and an empty list assigned to a global or an existing temp (directly or through a `ref`) takes the old value's origins unconditionally. brink had propagated the `origins` field by hand per operation (`^` merged both sides, `LIST_INVERT` cloned its input's, retention fired only when the new value had none), so `LIST_INVERT(LIST_INVERT((a,b)))` printed `a, b` where ink prints nothing and `LIST_COUNT(LIST_INVERT(l ^ m))` was 4 where ink says 0. Found by the lists tier of the program generator (`{(f0_a() ^ LIST_MAX((li0_0)))}` with `f0_a` returning `LIST_INVERT(LIST_INVERT((li0_0)))`). A compile-and-play regression test pins a 25-line inkjs table; the oracle ratchet is unchanged at 5624.
- **WHY:** The corpus rarely empties a list and then asks about its universe; the generator does it constantly. The reference's rule is a property of how each operation constructs its result, so brink models the construction (`build`) and the getter (`effective_origins`) once rather than patching each opcode's hand-copied origins.
- **NOT retained on `DeclareTemp`:** ink retains there when a same-named temp already exists in the call-stack element; a brink temp slot is reused across the knots of one frame, so the slot's previous value is not evidence of a same-named temp — retaining would invent origins from an unrelated temp more often than it would match ink. Recorded, not fixed.

## List containment is false when either operand is empty (#3531)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`list_ops::{list_contains, list_not_contains}`) — `docs/runtime-spec.md` "List containment with empty operands"
- **SCOPE:** small
- **WHAT:** `?` and `!?` now return `false` / `true` whenever either list operand is empty, matching ink's `InkList.Contains` (which short-circuits on an empty list on either side) instead of the vacuous subset test that made `l ? ()` `true`. Found by the lists tier of the program generator on its first run (`~ l -= (l ^ (l ^ l))` then `{(l !? l)}`). A compile-and-play regression test pins all six empty/non-empty combinations for both operators against inkjs; the oracle ratchet is unchanged at 5624.
- **WHY:** The corpus never asks whether a list contains nothing; the generator, which empties lists by arithmetic, does. The reference's answer is a deliberate special case, not a mathematical accident, so brink follows it exactly.

## In-text chips are a sufficient widget bar for a native editor
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** cross-system — GPUI native desktop evaluation (`crates/brink-gpui/`, `EDITOR-SWEEP.md`; the spike lived at `spikes/gpui-desktop/` when this was written)
- **SCOPE:** architectural (scopes a possible future port; nothing is committed to yet)
- **WHAT:** For the purpose of judging whether a GPUI-native editor could host brink's authoring surfaces, the widget capability proven in the spike — an **in-text chip**: text spliced into the shaped line, styled with its own run, able to draw its own content (a painted quad at the chip's own bounds), clickable with the click consumed rather than moving the caret — is **good enough**. A *nested element* inside a line (one with its own layout and children, e.g. an editable input mid-line) is explicitly NOT required.
- **WHY:** The surfaces that motivated the question are argument widgets and the colour picker, and both decompose into a chip plus a popover: the chip carries the affordance and its drawing, the popover carries the editing. GPUI does popovers natively. Holding out for a full nested-view widget would price in the hardest part of a CodeMirror `WidgetType` for a capability the authoring surfaces do not actually need. This bounds the editor question for any future port: the fork must carry inlays and chips, not a general in-line view system.

## The GPUI-native app is the destination; the web studio is transitional
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** cross-system — studio/desktop architecture (`crates/brink-gpui/` — `spikes/gpui-desktop/` when this was written — `docs/desktop-shell-spec.md`, `docs/studio-shell-spec.md`)
- **SCOPE:** architectural
- **WHAT:** The GPUI-native desktop app is intended to **replace** the studio, not to join it as a third consumer. Until it covers what the Tauri/webview studio covers, the two coexist with the native app deliberately narrower — but that coexistence is a **transition, not an end state**, and every choice is made for the replacement. Concretely: the native app is the destination authoring surface; the web studio is maintained, not grown; and nothing may be built that forecloses full replacement.
- **WHY:** The maintainer's judgement after driving the spike — "the app feels SO much better in GPUI and almost looks nicer already" — is the deciding evidence, and the five spike rounds found no blocking capability behind it: the engine drives a native editor at ~1.14 ms per keystroke, the Binder reaches near-parity in a third of the code, the manuscript view works, in-text chips are proven, and the studio's CSS maps onto GPUI with two absent features across four uses. What remains is volume, which is a schedule cost rather than a technical risk. Choosing the destination now (rather than drifting into a permanent two-implementation split) is what keeps the transition bounded: it settles that authoring features go native-first once the native app can host them, that the web studio freezes rather than accumulates, and that the editor acceptance gate must move down from the wasm `EditorSession` onto the engine both surfaces share.
- **CONSEQUENCES ACCEPTED:** maintaining a fork of `gpui-base`/`gpui-component` for the product's life (round 4); revisiting the 2026-08-06 ruling that kept Tauri partly to preserve a future mobile client, since GPUI forecloses it; and deciding separately what becomes of the published `@brink-lang/web` / `@brink-lang/editor` packages and their external embedder.

## Both studio consumers sit on the same layer; no ruled behaviour above it
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** cross-system — `brink-web` / `brink-ide` layering
- **SCOPE:** architectural
- **WHAT:** During the coexistence of the web studio and the GPUI-native app, the two must consume the **same layer**. Today they do not: the web studio sits on `brink-web`'s `EditorSession`, the native app on `brink-ide`'s `IdeSession` — one wrapping the other — so anything implemented in the wasm wrapper is invisible below it. The rule going forward: **`brink-web` may contain marshalling, and nothing else.** Any behaviour with a ruling behind it lives at or below `IdeSession`. Where a surface needs a session-level concept the shared layer lacks, the concept moves down rather than the consumer moving up.
- **WHY:** A consumer at a lower layer than another does not simply miss features — it silently re-implements them, and the second implementation is free to disagree with the first. The spike produced exactly that: it had to reimplement `brink.toml` discovery and application because that lives in `EditorSession`, and it could not offer drafts at all because `draft_path_list` — the ruled "reachability wins" semantic (2026-08-27) — lives in `brink-web/src/editor/outline.rs` with no home in `brink-ide`. Every such case is a rule with two definitions and one test suite. Making the layer shared also gives the editor acceptance gate a place to stand for both surfaces: moving it down onto the shared session turns the layering rule into something mechanically enforced rather than remembered.
- **KNOWN STRANDED SET (2026-09-04, to be moved down):** `program_model.rs` (the Program Explorer's structured model, including the size report the treemap consumes), `speculation.rs`, `draft_path_list`, and project-config discovery/application. **Correctly wasm-only** (marshalling, stays put): `value_marshal.rs`, `editor_dto.rs`, `story_runner.rs`, `external_binding.rs`. Much of the surface is already right — auto-fix (`brink-ide/src/fix/`), explain-match, hover, completion and refactor are all bindings over lower-layer logic.

## The native studio's analysis runs off the main thread
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** cross-system — GPUI native studio (`crates/brink-gpui/`, `docs/gpui-studio-spec.md` §3)
- **SCOPE:** architectural
- **WHAT:** Project analysis does not run on the UI thread. The single `IdeSession` **moves** to a worker thread (it is already `Send` — `brink-lsp` runs it as `Arc<Mutex<NativeProjects>>` under a multi-threaded server) and returns plain data: diagnostics, the refined `kinds` map, resolved symbols. The main thread keeps only the open document's syntax. Salsa snapshot handles (`ProjectDb: Clone`, rust-analyzer's model) are **rejected as unnecessary**: `brink-db` requires no changes.
- **WHY:** The spike's headline "1.14 ms per keystroke" was measured on a 44-file, 13.9k-line project, and the maintainer's objection — those are numbers on small projects — is borne out by measurement. A synthetic project at the scale of a large commercial script (807k words, 113k lines) analyzes in **7.8 ms median / 11.6 ms max** per keystroke, and 2.4M words in 23 ms. Scaling is linear in words out to 4.8M, so there is no cliff — but 7.8 ms is **8x Zed's own synchronous budget** (`sync_parse_timeout`, 1 ms in release, `crates/language/src/buffer.rs:1152`), which settles it. Snapshots are rejected on cost of a different kind: `salsa::Storage::clone` is genuinely cheap (two `Arc` bumps; memo tables are shared), but writing an input needs `zalsa_mut`, whose `cancel_others` "blocks until all other workers with access to this storage have completed". Cancellation is cooperative, so a keystroke's latency becomes *how long until the background query reaches a checkpoint* — an unbounded quantity we would then have to bound. Moving the session wholesale avoids the question entirely.

## No debounce in the native studio
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** cross-system — GPUI native studio (`docs/gpui-studio-spec.md` §3.1)
- **SCOPE:** architectural
- **WHAT:** Keystrokes are never coalesced behind an idle timer. Every edit starts its work immediately. This is a standing constraint on the whole authoring surface, not a tuning choice for one code path.
- **WHY:** The maintainer's position, stated flatly. The structural consequence is what makes it a good constraint rather than a costly one: debounce buys latency headroom by making the author wait a fixed interval to see their own edit reflected, and it permits per-keystroke work to stay O(file) or O(project) indefinitely, because the cost is merely paid less often. Forbidding it forces each keystroke's main-thread work to be **O(edit)** — which is the correct shape, and which the engine already supports. Measured (`incr` bench): reparsing one segment via `brink_syntax::segment_file` (#3084) costs **17–51 microseconds, flat regardless of file size**, against 0.97–77 ms for a whole-file reparse — a 7–12x win whose residual O(file) term is a lex-only pass that stays under 1 ms through ~6k-line files.

## The native studio's region model drops the bottom rail
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** GPUI native studio shell (`docs/gpui-studio-spec.md` §4.1); revises `docs/studio-shell-spec.md` for the native surface only
- **SCOPE:** architectural
- **WHAT:** There is no bottom rail. The left and right rails are each split into an upper and a lower group, and **the lower groups address the bottom dock** — left-lower opens on the bottom-left, right-lower on the bottom-right. With the editor center and status bar that is five surfaces and one placement rule: a tool window's rail slot is the only declaration of its home, and layout persistence keys on `(edge, group)`. The former spec's two-sections-per-dock model is dropped; `Tiles` (free-form docking) remains available in the toolkit and deliberately unused.
- **WHY:** The maintainer's own correction of the first build — the earlier region model was written from a misapprehension of what JetBrains does. Removing the bottom stripe and re-homing its buttons to the bottoms of the side stripes is IntelliJ's actual new-UI arrangement. It also collapses a redundancy: with a bottom rail, a panel's home was declared twice (which rail, and which dock), and those could disagree. The implementation is free — `gpui-kit`'s dock layout is already a pane tree, and `normalize.rs` rule 2 replaces a one-child `Split` with that child (keeping its `NodeId`), so a single-sided bottom dock takes the full width with no special case and no panel-entity teardown.

## The native studio layers as model / shell / features
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** GPUI native studio (`docs/gpui-studio-spec.md` §2)
- **SCOPE:** architectural
- **WHAT:** Three crates from the start: `brink-gpui-model` (entities over `IdeSession`, the analysis worker, events — gpui but no `gpui-component`), `brink-gpui-shell` (regions, docks, commands, persistence; defines the `Item`/`Panel` traits), `brink-gpui` (Binder, Editor, Problems, Player, and `main`). **The shell must not depend on the feature crate.** Within a tier, one crate until it hurts.
- **WHY:** Taken from surveying Zed (2026-09-04 at `c91e24a`), which shows both halves. First, the seam is not where it looks: `gpui` is the *application* framework (entities, executors, tasks) and 157 of 244 crates depend on it, `text` and `project` included; the real boundary is `ui`/`theme`, and `project` is 82k lines of gpui-dependent, UI-free model. Second, `workspace` defines `Item` and `Panel` as traits and depends on `project` but **not on `editor`** — features plug into the shell, the shell never learns what they are, and concrete wiring happens once at the top. That inversion is the load-bearing part, and it is the one boundary that is expensive to retrofit; everything else can be split later on demand. The studio's five-package TypeScript split is not precedent here, since it exists partly for npm publishing.

## A shuffle's unpicked list is removed from order-preservingly (#3538, part 1 of 2)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** runtime (`vm::handle_shuffle_with_hash`) — `docs/runtime-spec.md` §"Shuffle algorithm"
- **SCOPE:** small
- **WHAT:** The partial Fisher-Yates now removes the picked index with `Vec::remove` rather than `swap_remove`, matching the reference's `unpickedIndices.RemoveAt(chosen)`. `swap_remove` moves the last unpicked element into the hole, so from the second draw of a loop onward brink indexed a differently-ordered list than ink and picked a different alternative. A compile-and-play regression test pins four loops of a three-way shuffle against inkjs; the ratchet rises 5624 → 5627 (`tier2/conditional/shuffle` 0/1 → 1/1, `tier2/sequences/I107-shuffle-stack-muddying` 0/2 → 2/2).
- **WHY the corpus took so long to convict it:** the failure signature is easy to misread. The *first* draw of every loop still agreed, because nothing has been removed yet — so a case that draws once per loop passes, and a case that draws three times shows iteration 0 matching with 1 and 2 swapped. That reads like a seeding problem, and it was filed as one.
- **CORRECTS AN EARLIER ATTRIBUTION:** #3538 was opened against the container path hash, on the reasoning that a shuffle seeds from `path_hash` and brink's container paths are not inklecate's. That reasoning was right about the *mechanism* and wrong about *this* symptom. Measured on the case that raised it (`tier2/conditional/shuffle`): brink's path hash for the sequence container is 636, and instrumenting inkjs shows it seeding from `"test.0.0"` — the same 636. The paths already agreed; the removal did not.
- **THE PATH-HASH BUG IS REAL AND STILL OPEN** (part 2, the ruling of the same date): a choice-free knot's sequence gets `k.0.0` from brink against inklecate's `k.0` (295 vs 201), because brink applies its implicit-stitch rule where inklecate emits no such level. The two bugs are independent and both had to be measured separately — `tests/tier4-generated/shuffle-path-hash` stays `expected_mismatch` after this fix, which is how they were told apart.
- **SCALE, corrected:** this was described mid-session as "most of the ratchet gap". It is not. Fixing it moves the ratchet by **3 episodes**, because `tests_github/dream_on` — 1,000 of the 1,012 failing episodes — still fails every episode on unrelated divergences. What it does move is the *depth* of those failures: dream_on's snapshot loses 973 mismatch entries (10,637 → 9,664) while gaining no passing episode. A large diff and a small ratchet delta are consistent here, and the ratchet is the number that counts.

## A lifted else-less conditional owes its line's newline on the all-false path (#3530)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** `brink-ir` (`hir::normalize`, the `synthesized_else_branch` gate) — `docs/compiler-spec.md` §"Source-order rule" sibling bullet
- **SCOPE:** small
- **WHAT:** The normalization lift synthesizes an else arm for a no-else conditional whenever the line carries prefix text, suffix text, **or its own end-of-line**. Previously only the first two, so a construct alone on its line (`{f():a}`) emitted no `EndOfLine` on the untaken side and a printing condition's output ran into the next line — `ab` where ink gives `a` / `b`. Four compile-and-play tests pin the printing-false, printing-true, silent-false and silent-true cases against inkjs.
- **WHY the newline is owed:** ink suppresses a line's `\n` only when the line produced no content, and a condition that prints IS content — the newline is a property of the line, not of the arm that ran. Synthesizing an arm holding only an `EndOfLine` is safe for the silent case because the runtime drops a newline with no content before it, so `{false:a}` still emits nothing; the two guard tests exist to keep that true.
- **CORPUS IMPACT: none, and that is the point.** Ratchet unchanged at 5627 (its value after the #3538 removal-order fix, which this change sits on top of), no snapshot moved, 384 pass / 6 fail / 414 unchanged. The shape simply does not occur in the curated corpus — which is why the generator found it and the corpus never had. `tests/tier4-generated/else-less-conditional-call` flips from `expected_mismatch` to passing, and the tier's two-way check is what forced the flag's removal into this same change rather than letting it go stale.

## Ink is a full peer surface, not a compatibility floor
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** project direction — `CLAUDE.md` ("What we're building", "Current state", "Workflows")
- **SCOPE:** architectural
- **SUPERSEDES (in part):** "Oracle conformance is no longer the core metric" (2026-08-07). That ruling's core stands: **the ratchet percentage is not the measure of progress**, and a number going up is not a plan. What is superseded is its *surface* half — the framing of `.ink` as "a maintained floor, not the goal", and of the residual mismatches as a compat subset the native surface had outgrown.
- **WHAT:** `.ink` and `.brink` are peers. Both are first-class ways to author for brink, held to the same standard, and the maintainer's own initial use case is as much strict ink as native. The oracle ratchet keeps its standing as a CI-enforced regression floor — never down, unexpected movement in either direction is stop-and-report — but the open divergences behind it are now **defects in a peer surface**, each one a story an author cannot write, rather than an acceptable residue.
- **WHY:** The 2026-08-07 framing was written when native was the only surface being actively grown, and it correctly demoted the ratchet *number*. It also, incidentally, demoted the *surface* — which no longer matches what the project is for. The two demotions are separable, and only the first was intended to last.
- **CONSEQUENCE:** ink-surface conformance is a driving track, not opportunistic maintenance. It is driven by root cause — the `brink-gen` generator and its inkjs differential find the gaps, the corpus pins them — and not by the ratchet number, which remains a floor rather than a target.

## A discovered conformance gap gets a corpus repro first
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** `tests/tier4-generated/`, `scripts/promote-generated.mjs` — `CLAUDE.md` "Conformance gaps"
- **SCOPE:** process
- **WHAT:** When a conformance gap is discovered, **the first action is a minimal repro in the corpus** — before the root-cause analysis, before the fix, before the issue text is finished. Minimise the case, promote it with `pnpm promote:generated`, let the tier carry it. A gap not yet fixed is promoted with `--expected-mismatch` and its issue number. This applies however the gap was found: the generator's differential, a corpus sweep, a bug report, or a shape tripped over by hand.
- **WHY:** The 8-item generator plan worked — it found real divergences at a good rate — but the fixes and the issues were the artifacts, and the *cases* lived only in the differential's transient output. A gap found and then fixed left nothing behind that would notice it coming back; a gap found and not yet fixed left nothing behind at all. The generator is a search, not a suite: what it finds only becomes durable when a case is checked in.
- **BOTH WAYS:** the tier checks a case against its declared expectation in both directions, so an `expected_mismatch` case that starts matching is a **failure**. That is deliberate — it is how a gap closed as a side effect of unrelated work gets noticed instead of silently persisting as a stale flag. Backfilled for every gap found this session in #3543 (16 cases, `GENERATED_CASE_COUNT` 4 → 20).

## inkjs is trusted as the reference; `dotnet` is not vendored
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** `tools/inkjs-oracle/` — `CLAUDE.md` "Trust hierarchy"
- **SCOPE:** moderate
- **WHAT:** Where brink and inkjs disagree, treat **inkjs as right and brink as the defect**, and open the issue. The alternative — making the C# runtime runnable in a cloud session by vendoring `dotnet` and the reference implementation — was considered and **declined**: not worth the toolchain weight and the licensing care, given that `tools/inkjs-oracle/` already reproduces every checked-in golden with `KNOWN_DIVERGENCES` empty, which CI's `inkjs-sanction` job keeps true.
- **WHY:** The trust hierarchy's two highest ranks are exactly the two a cloud session cannot open, which left every cloud-session semantics question either blocked on the maintainer or answered by guessing from a lower rank. The sanction measured the stand-in's fidelity rather than assuming it — 414/414 cases, no divergences — which is enough to promote "evidence for a ruling" to "the answer, absent a specific reason to doubt it".
- **NOT an infallibility claim:** a *specific* case where inkjs looks wrong is still worth surfacing for a maintainer ruling, and the C# runtime remains the tie-breaker in principle. It is simply not something to go install.

## Container path hashes stay ink-compatible on both surfaces (#3538)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** compiler — container path construction, shuffle seeding
- **SCOPE:** moderate
- **WHAT:** brink computes **ink-compatible container path hashes**, for native as well as ink. A shuffle's seed in ink is `sequenceHash + loopIndex + storySeed`, where `sequenceHash` is the sum of the character codes of the container's *path string*. brink already sums char codes of a path string and already carries three hand-written inklecate-compatibility rules in codegen — but they are heuristics over brink's own container tree rather than a reconstruction of inklecate's, and they diverge. Measured: for `=== k ===` holding a lone sequence and no choice, brink emits `k.0.0` (295) where instrumented inkjs seeds from `k.0` (201) — brink applies its implicit-stitch rule where inklecate emits no such container level. The fix is to compute a path string compatible with inklecate's scheme and hash it ink's way.
- **WHAT IT IS NOT:** the compiler's *shape* is not dictated by this. brink keeps its own container model, ids and layout; what it gains is the ability to *compute* a compatible path on demand for the purposes that need one. The constraint is on a derived value, not on the structure it is derived from.
- **MEASURED AFTER THE RULING — there are TWO causes, and the second is the expensive one.** A twelve-shape sweep (brink's emitted `path_hash` against an instrumented inkjs's real `seqPathStr`) found 8 of 12 diverging, the 4 agreements being coincidences of the character sum rather than construction. Cause 1 is the implicit-stitch rule above. Cause 2: **inklecate's path component is the index in the parent's flat runtime content array.** Holding everything else constant, 1/2/3 lines of plain text before a sequence give `k.2` / `k.4` / `k.6` — two runtime objects per source line, the string and its newline — and `two seqs in one knot` gives inklecate `k.0`, `k.2` against brink's `k.0.0`, `k.0.1`. brink numbers a sequence by its ordinal among its parent's children; inklecate numbers it by position among the parent's emitted objects. brink's containers hold bytecode, not a list of ink runtime objects, and the count inklecate *would* have emitted before a point is a property of inklecate's codegen. So "compute a compatible path on demand" means carrying a shadow model of inklecate's emission, not writing a naming function — which is a materially larger commitment than this ruling was made against. Options and the full table are on #3538; the choice between them is not yet ruled.
- **NATIVE TOO, deliberately:** the initial instinct was to let native skip the compatibility computation, since nothing in native needs to agree with inklecate. Rejected on **respelling**: `brink-respell` re-emits ink as `.brink`, so a respelled story is expected to behave like its original — and if the two surfaces seeded shuffles differently, respelling would silently change which alternative a shuffle picks. A surface-dependent seed is a surface-dependent story.
- **NOT the whole shuffle gap — one of two independent bugs.** This ruling was made on the reading that shuffle divergences are seeding divergences. Working it turned up a second, unrelated cause in the same feature, fixed separately the same day: the partial Fisher-Yates removed the picked index with `swap_remove` instead of order-preservingly, permuting the survivors from the second draw of each loop onward. On the very case that raised this issue (`tier2/conditional/shuffle`), the path hashes **already agreed** — brink's 636 against instrumented inkjs's `"test.0.0"`, also 636 — and the removal was the whole story there.
- **SCALE, measured rather than assumed:** the mid-session claim that correcting the path hashing "should close most of the gap" was wrong twice over, and the correction is worth keeping. The removal fix, which is what the corpus was actually convicting, moved the ratchet by **3 episodes** (5624 → 5627). It could not have moved much more: `dream_on` is 1,000 of the 1,012 failing episodes and gains no passing episode from it, because each of those episodes also fails on divergences that have nothing to do with sequences. A per-story mismatch-entry count (dream_on's fell 10,637 → 9,664) measures the *depth* of a failure, not whether it passes — reading one as the other is what produced the wrong estimate. What remains open here is real, and its size is unmeasured until it is fixed.

## Optimizer plumbing: `brink-opt` v1, and the three §9 rulings (#2336)
- **WHEN:** 2026-09-04
- **PROJECT:** brink
- **SYSTEM:** `crates/brink-opt`, `brink-test-harness::opt_fence` — `docs/optimizer-spec.md` §9/§10, `docs/optimizer-framework-spec.md` §1
- **SCOPE:** moderate
- **WHAT:** Step 1 of `optimizer-spec.md` §8: the crate, `ArtifactStats`, and the fence, with **no optimization pass**. The three open questions are ruled: (1) `brink compile` does NOT run the optimizer — `brink opt` is a separate explicit step, so an artifact's provenance is never ambiguous; (2) the "was optimized" artifact marker is **deferred**, at no cost, because `.inkb`'s house rule exempts a new optional-omitted-when-empty section from a `VERSION` bump, so it is exactly as cheap later and can then record real pass names; (3) `--passes=` toggles are **deferred** — this departs from the spec's own proposal — because a toggle grammar with no inhabitants cannot be tested and risks being designed wrong, the same trap the framework spec is deferred to avoid. `OptConfig { passes: PassSet }` exists in the API regardless.
- **WHY NO `brink opt` SUBCOMMAND YET:** `brink-cli` is publishable and today depends only on publishable crates — an invariant that holds across the whole workspace. `brink-opt` is `publish = false` while it does nothing, and a published crate cannot normal-depend on an unpublished one, so wiring it in would break `cargo publish -p brink-cli` **at release time** — CI's "Publishable crates exist on crates.io" step would not catch it, since it verifies that each publishable crate exists, not that its dependencies do. A CLI for an optimizer with an empty pass list is useless to an author anyway; it lands with the first real pass, when hand-publishing the crate is worth doing.
- **THE FENCE IS THE DELIVERABLE, AND ITS DESIGN PROBLEM IS VACUITY.** With an empty pass list, trace-equality, line-identity, idempotence and stability are all green — and so would be a fence that compared nothing at all. Three things make greenness evidence: (a) every check, positive and negative, goes through ONE seam (`opt_fence::judge`), so a control going red is a statement about the same code path the fence uses; (b) four negative-control passes (`brink_opt::control`, behind `test-control`, never in `default`) each trip exactly one obligation, and the matrix has a red cell in every column; (c) three tiers of non-vacuity floor — case counts, summed-`ArtifactStats` content floors, and per-control kill floors with `mutate.rs`-style grounding.
- **THE CONTROLS ARE SEPARABLE BECAUSE OF A FACT ABOUT THE FORMAT:** `line_identity_diff` compares only `(scope_id, index, source_hash)` and never reads `LineEntry::content`, while the runtime reads `content` and never reads `source_hash`. So `control:retext` trips the trace oracle alone and `control:rehash` the identity oracle alone. A single control tripping both would prove neither, since either oracle could be doing all the work. `control:drift` (a per-run `audio_ref`) trips neither semantic oracle and only run-to-run stability — which is what justifies keeping byte-level checks at all.
- **A SPEC CLAIM CORRECTED:** §8.1's "an empty optimizer is provably byte-identical" reads as though the whole v1 fence is a tautology. Four obligations are; the fifth is not. The road is `read_inkb → optimize → write_inkb`, so byte-identity with no passes asserts **`write_inkb ∘ read_inkb == id`** — which nothing in the tree checked (`brink-format`'s own round-trip tests use synthetic and hand-built values). Measured: it holds over 419 real corpus artifacts. The fence reports it as its own failure line naming `brink-format`, so a format bug is never misattributed to the optimizer.
- **THE FENCE CAUGHT A FLAW IN ITS OWN GROUNDING PREDICATE ON THE FIRST RUN,** which is the best evidence it works. `tier1/diverts/I132-comparing-diverts` survived `control:retext`: it emits `1`/`0`/`0`/`1` from pure value interpolation, producing NO line-table entries, and its only line entries belong to two knots never entered (they exist solely as divert targets to compare). "Do the runs emit any text?" grounded it; "does a line-table entry supply text the runs emit?" correctly does not. `is_line_text_grounded` is the corrected predicate, and it deliberately under-grounds (skipping `Template` entries) since that direction can only lower a kill count the floors already guard.
- **TWO OBSERVABLES ADDED TO THE FRAMEWORK SPEC'S LIST**, both found while building this and absent from every earlier draft: `.inkl` overlays carry a `base_checksum` matching the `.inkb` header CRC and are index-aligned with `ScopeLineTable`, so **any byte change invalidates every existing overlay** and optimization must precede localization; and `source_hash` is a translation key the runtime never reads, which is load-bearing for the controls above.

## Superinstructions are the optimizer's first pass; the format grows opcodes as passes need them
- **WHEN:** 2026-09-05
- **PROJECT:** brink
- **SYSTEM:** `crates/brink-opt` (`peephole.rs`, `passes.rs`), `brink-format` (`Opcode::EmitLineNl`, `.inkb` v7), `brink-runtime` (`vm.rs`) — `docs/optimizer-peephole.md`, `docs/optimizer-spec.md` §8/§11, `docs/optimizer-catalogue.md`
- **SCOPE:** moderate
- **WHAT:** The first real optimizer pass is a **peephole superinstruction**, not the catalogue's proposed line-table dedup: `EmitLine → EmitNewline` fuses into a new `EmitLineNl` opcode (`0x6C`) whose runtime arm runs the two original bodies in sequence. It rides on a shared rewriting engine that owns every legality concern once — labels (a window may begin at a jump target or `AddressDef` but never swallow one), relocation (relative jumps re-encoded, `AddressDef.byte_offset` and `DebugEntry.bytecode_offset` moved per container, an entry on a swallowed instruction landing on the replacement), and refusal to touch a container that stops decoding. Later fusions are one `Rewrite` impl each.
- **WHY THIS PASS FIRST:** the runtime campaign measured ~450 host instructions per bytecode instruction, dominated by fetch/dispatch rather than opcode bodies, and #3575's bigram histogram named the pair: 8.9% of every instruction TheIntercept executes, 3.6% of hanoi's. Instructions executed is the metric with the most headroom, and dedup's collisions (VO slots, translator context) are still unruled. Measured: opcodes executed −8.9% on TheIntercept, −3.6% on hanoi, −0.1% on crucible (arithmetic-bound; its pairs are the next rewrites).
- **RULED — new opcodes are acceptable.** Each fused form spends an unreserved opcode byte and a `VERSION` bump; the maintainer accepted that cost ("we can add the op codes to the format if needed"). v7 is the first bump spent this way. Codegen never emits a fused form: the compiler stays one-instruction-per-construct and the fused vocabulary belongs to the optimizer alone, which keeps the artifact's provenance legible (§9 ruling 1) and the unoptimized road the one the oracle ratchet measures.
- **THE FENCE CHANGED SHAPE WITH THE FIRST INHABITANT.** `bytes_identical` was a standalone claim while no pass existed; it is now asserted *against* `changed` in both directions (untouched case ⇒ identical bytes, the `brink-format` round-trip claim still live; changed case ⇒ different bytes, or the pass lies about its outcome). Every sweep and the generator property gained a **change floor** (150/390 tier1–3, 10/29 native, half of generated cases), so a pass set that silently stops matching is red rather than quietly green. Measured on landing: 297 and 24 rewritten.
- **HOT PATCHING is preserved by placement:** the optimizer rewrites `StoryData` before linking, so the two-layer linker (2026-03-01) sees a fused instruction exactly as it sees any other, and a re-link over a patched artifact needs nothing new.
- **NOT RULED HERE, surfaced:** three shapes the histogram exposes are compiler emission rather than instruction pairs — `DeclareTemp` per parameter on every call, the six-instruction fragment wrapping of call slots, a leading `EmitNewline` on every conditional-arm container. Fusing them would paper over codegen the compiler should not be doing; which the compiler fixes first is open (`docs/optimizer-peephole.md` §6). The `brink opt` subcommand stays deferred until the crate is hand-published (§10 reasoning unchanged).

## Pass 2: binary fusions land on the peephole engine (`.inkb` v8)
- **WHEN:** 2026-09-05
- **PROJECT:** brink
- **SYSTEM:** `crates/brink-opt` (`passes::BinaryFusion`, `peephole::Emit`), `brink-format` (`BinaryKind`, `BinaryImm`/`BinaryJumpIfFalse`/`BinaryImmJumpIfFalse`), `brink-runtime` (`vm.rs` fused arms, `jump_unless`) — `docs/optimizer-peephole.md` §1/§4/§5
- **SCOPE:** moderate
- **WHAT:** The second superinstruction family, in the same sitting as the first and under its ruling: a binary operator fused with the `PushInt` immediate feeding it and/or the `JumpIfFalse` consuming it, longest window first. Three opcodes carrying one `BinaryKind` operator byte (`0x6D`–`0x6F`, `.inkb` v8) rather than one opcode per operator — the table grows by families. The engine gained branches *inside* replacements (`Emit::Branch` naming its absolute old target, relocated like any kept jump) and engine-enforced label refusal; a label on the `JumpIfFalse` shortens the window to the immediate form instead of blocking it.
- **WHY THE SPLIT OF WORK:** the runtime arm is the constituent bodies through the same helpers (`value_ops::binary_op` with `Value::Int(imm)`; `jump_unless`, factored out of `JumpIfFalse`), so int/float promotion, string comparison and every error path are the unfused ones by construction — the fusion is over instruction boundaries only. That is what let it land the same day: nothing about operator semantics was re-implemented.
- **MEASURED (both passes vs unoptimized):** opcodes executed −22.8% crucible / −15.0% hanoi / −11.7% TheIntercept; Ir per iteration −16.0% / −7.1% / −2.6%. `fib`'s `n <= 1`, `n - 1`, `n - 2` are the three windows, three dispatches saved per call. Corpus fence: 335 of 390 ink cases and 26 of 29 native cases now rewritten.
- **`.inkt` SPELLING, a hazard avoided:** the operator operand is `kind=le`, a `kv_operand`, not a bare `le` — a bare word operand would let `add` on the next line parse as a trailing operand of the previous instruction and silently swallow that instruction (the #3273 shape, caught here by the `.inkt` round-trip proptest before it reached a file).
- **A FENCE FLAKE FOUND AND FIXED ON THE WAY:** `the_generator_produces_stories_the_oracle_can_distinguish` failed one run in six, on a generated story whose only rendered text was `beta` and which carried a one-letter choice label `[a]` on a stitch its runs never reached. `is_line_text_grounded` used plain substring containment, so `"beta".contains("a")` grounded the story, the retext control could not be observed, and the test asserted a kill that could not happen. Whole-run containment (`contains_bounded`: the literal is not a fragment of a longer alphanumeric word) is the corrected predicate; the story is pinned as a fixture in `opt_negative_control.rs`. Same lesson as v1's `I132` finding: the grounding predicate is where a negative control quietly lies, and it must be decided before the verdict, never from it.
- **NEXT, from the post-pass histogram:** `GetTemp → BinaryImm{,JumpIfFalse}` is 9.7% + 9.7% of crucible — folding the local read is the next rewrite (3→1 and 4→1 on the original shapes); `Duplicate → BinaryImmJumpIfFalse` 4.9% of hanoi. The compiler-side shapes (`Call → DeclareTemp`, `EnterContainer → EmitNewline`, fragment wrapping) remain surfaced, not fused.

## Pass 3: the left-operand folds close out the peephole family (`.inkb` v9)
- **WHEN:** 2026-09-05
- **PROJECT:** brink
- **SYSTEM:** `crates/brink-opt` (`passes::LeftOperandFold`), `brink-format` (`GetTempBinaryImm`, `GetTempBinaryImmJumpIfFalse`, `DuplicateBinaryImmJumpIfFalse`), `brink-runtime` (`vm.rs` `read_temp`) — `docs/optimizer-peephole.md` §1/§4/§5/§6
- **SCOPE:** moderate
- **WHAT:** The third superinstruction family, same sitting, same ruling: fold the `GetTemp` or `Duplicate` that produced a fused operator's *left* operand into the operator. Three opcodes (`0x70`, `0x71`, `0x74`, `.inkb` v9). The pass runs **after** `binary-fusion`, on its output — the order in `OptConfig::defaults` is load-bearing — so it is idempotent and label-safe for free and any future rewrite producing `BinaryImm*` inherits the fold. `GetTemp`'s body was factored into `read_temp` and the fused arms call it, so pointer/projection auto-dereference and the #3354 unwritten-slot warning stay one body.
- **MEASURED (all three passes vs unoptimized, paired runs):** opcodes executed −37.9% crucible / −19.2% hanoi / −12.4% TheIntercept; Ir per iteration −26.2% / −9.4% / −2.3%. `fib`'s `n <= 1`, `n - 1`, `n - 2` are each one instruction now — five dispatches saved per call.
- **FLAGS ON EXISTING OPCODES — considered, declined:** a "then newline" bit on `EmitLine` or a "left from temp" bit on `BinaryImm` costs the same at dispatch as a kind byte, but changing an existing opcode's encoding changes every *unfused* instruction too, so codegen output would no longer be byte-identical across a bump and the optimizer-only provenance of the fused forms would blur. Working rule: a new discriminant per window *shape*, a kind/flag byte for variants *within* a shape (`BinaryKind`, `ChoiceFlags`), never a flag that changes an instruction's length. 92 unreserved opcode bytes remain.
- **THE PEEPHOLE FAMILY IS COMPLETE FOR NOW.** After pass 3 every instruction pair above 6% in the reference histograms is a codegen decision (`Call → DeclareTemp`, `EnterContainer → EmitNewline`, the fragment wrapping of call slots), not a fusable shape. Those are the compiler track. The optimizer's next entry is `dedup line table` — which the maintainer's stated preference places **in emission** (`brink-codegen-inkb`, where `intern_string` already dedups its neighbours) rather than in a pass: it is a translator quality-of-life fix as much as an optimization, and being born deduplicated is smaller and earlier than deduplicating after the fact. Its three blocked fields (`audio_ref`, `source_hash`, `source_location`) and the translator-context question are the design questions to rule before it is built.

## The native studio opens `brink.toml` in the editor; Settings holds only the form
- **WHEN:** 2026-09-05
- **PROJECT:** brink
- **SYSTEM:** GPUI native studio (`crates/brink-gpui/`, `docs/gpui-studio-spec.md` §4.8)
- **SCOPE:** moderate
- **WHAT:** In the native studio `brink.toml` is a **document like any other**: listed in the Binder beside the sources, opened in Code view (and so shown by Single File view) as a TOML-painted tab in the same shared buffer, saved by the same `cmd-s`, with a Problems row on it opening the tab at the span. Settings ▸ General carries **only the form** — the `entry` / `conventions` / `dialect` / `types` selects and the drafts list — plus an "Open brink.toml" door to the tab for every key it does not model. This is the native studio's own shape, **not** the web studio's 2026-08-27 ruling ("brink.toml opens in the Settings takeover, in every view", form and raw text together), which stands for the web studio.
- **WHY:** The maintainer's call, stated as "unlike the web version". The web ruling's reason — Continuous view renders the manuscript and had nowhere to put the file, so a takeover carrying the raw text was the only way to keep `drafts` and `indent` editable at all — does not bind the native studio, whose Code view can open the file directly. With a real tab available, a second editor over the same text inside a modal is one editor too many: the tab is where text is edited, the modal is where settings are set, and the shared buffer keeps the two agreeing without either owning the other.

## The dialect inference and the `[dialogue]` section writer live in Rust; the TypeScript is a mirror held to one corpus
- **WHEN:** 2026-09-05
- **PROJECT:** brink
- **SYSTEM:** brink-ide (`dialect_infer`, `dialogue_section`, `diagnostic_registry`); studio-settings; GPUI native studio
- **SCOPE:** moderate
- **WHAT:** The teach-by-example Conventions editor's engine — the shape inference over marked lines, the source and emitted parsers it verifies against, the emitted-side run rule, the `[dialogue]` table projection — and the marker-stamped `[dialogue]` section writer are **Rust**, in `brink-ide`, the crate both studios sit on. `@brink-lang/dialect`'s `infer.ts`/`config.ts`/`DialectParser`/`runsOf` and studio-store's `dialogue-section.ts` stay as the web studio's pure-TypeScript mirror (an editor or a game engine shares them without wasm), and the two are held together by **one golden corpus** duplicated in both test suites — a case added to one is added to the other — and by the section hash, which both compute identically so a section either studio writes reads as its own in the other. The same move puts the Diagnostics section's code list and its author-facing category table in `brink_ide::diagnostic_registry`, with `brink-web` reading it rather than owning it.
- **WHY:** The maintainer asked for the conventions/dialect logic in Rust so the native studio could carry the Conventions UI. A second implementation in the app crate would have been a third copy free to drift from the other two; putting the one the native studio reads in `brink-ide` makes it the reference, with the TypeScript kept for the reason it was written pure in the first place. Duplicating the corpus rather than sharing a fixture file is deliberate: each suite stays self-contained in its own toolchain, and the rule that a case is added to both is cheaper than a cross-language fixture loader.
