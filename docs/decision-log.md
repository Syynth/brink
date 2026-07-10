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
