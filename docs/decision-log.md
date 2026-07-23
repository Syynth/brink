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
