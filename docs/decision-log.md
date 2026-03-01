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
