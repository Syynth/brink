# HIR admission contract — evidence file (code-level findings)

Companion to `hir-admission-contract.md`. This is the raw evidence base:
what the de facto contract actually *is* in the code today, with file:line
anchors. The draft cites these by section letter (F-A…F-K). All paths absolute.

Method: read `crates/internal/brink-ir/src/hir/**` (full node inventory +
lowering + stamp + normalize + directive channel + frame_shape), the
`brink-compiler` driver, the `brink-db` query graph (via subagent), what
`brink-analyzer` trusts (via subagent, adversarial sweep), and the decision-log
2026-07-12→19 rulings (via subagent). The oracle/runtime were not touched — this
is a compiler-frontend-boundary study.

---

## A. The de facto contract output shape

A frontend does **not** produce "an HIR tree". It produces a **triple**, per file:

    (HirFile, SymbolManifest, Vec<Diagnostic>)

- `crates/internal/brink-ir/src/hir/lower/structure/mod.rs:29` — `pub fn lower(file_id, &ast::SourceFile) -> (HirFile, SymbolManifest, Vec<Diagnostic>)`.
- `docs/compiler-spec.md:25` — "Output: `(HirFile, SymbolManifest, Vec<Diagnostic>)` per file".
- brink-db wraps this as `LoweredFile { hir, manifest, diagnostics }`, `#[derive(PartialEq)]` — the salsa early-cutoff boundary (`crates/internal/brink-db/src/queries/mod.rs:300`).

The `SymbolManifest` (`crates/internal/brink-ir/src/symbols/manifest.rs:10`) is a **separate, parallel artifact** the frontend must emit and keep consistent with the HIR body:
- declared symbols in 9 buckets: `knots, stitches, variables, constants, lists, structs, externals, labels, list_items` (each a `DeclaredSymbol { name, range, params, detail, visibility, was }`).
- `locals: Vec<LocalSymbol>` — params + temps, each scope-tagged.
- `unresolved: Vec<UnresolvedRef>` — every divert/variable/function/list/struct reference, with `{ path, range, kind, scope, arg_count }`, for cross-file resolution.
- `docs: BTreeMap<(SymbolKind, String), DocBlock>` — `///` doc metadata.

**The analyzer never re-derives the manifest from the HIR, nor vice-versa — it trusts they agree.** (Subagent, adversarial sweep, cross-cutting invariant.)

## B. The single AST→HIR admission point (no abstraction today)

- `crates/internal/brink-db/src/queries/mod.rs:293` — `parse_query` hard-calls `brink_syntax::parse(file.text)`.
- `.../queries/mod.rs:315` — `lowered_query` → private `lower_file` (`:2105`), which hard-calls `brink_ir::{lower, lower_single_knot, lower_top_level}` on the ink CST.
- **No trait, no `dyn Frontend`, no dialect match anywhere on the parse/lower path.** A second frontend plugs in by branching exactly these two call sites. That pair *is* the admission point. (Subagent DB report §4.)
- `AnalysisOptions.dialect` exists but only gates **downstream** behavior (`symbol_index_query`, analyzer diagnostic passes) — never lowering (`crates/internal/brink-analyzer/src/lib.rs:89`; `dialect_gate.rs:19-21`).

## C. Provenance is welded to the ink CST

Nearly every HIR node carries a `ptr` back into the ink syntax tree:
- `crates/internal/brink-syntax/src/ast/ptr.rs:92` — `AstPtr<N: AstNode>` stores `SyntaxKind + TextRange + PhantomData<fn() -> N>`. `SyntaxNodePtr` (`:16`) = `SyntaxKind + TextRange`.
- `SyntaxKind` is brink-syntax's ~230-variant **ink grammar** kind enum (`docs/compiler-spec.md:34`).
- Node examples that embed ink AST node *types* directly:
  - `ContainerPtr::Knot(AstPtr<ast::KnotDef>)` / `Stitch(AstPtr<ast::StitchDef>)` (`hir/types.rs:194`) — an enum literally over ink AST kinds.
  - `Knot.ptr: ContainerPtr`, `Choice.ptr: AstPtr<ast::Choice>`, `Tag.ptr: AstPtr<ast::Tag>`, `VarDecl.ptr: AstPtr<ast::VarDecl>`, `TunnelCall.ptr: AstPtr<ast::DivertNode>`, etc. — throughout `hir/types.rs`.
- Purpose: LSP refactoring resolves `ptr` back to a live ink node (`ptr.rs:50` `resolve`, `docs/compiler-spec.md:61`). Escape hatch `AstPtr::from_range` stamps `SyntaxKind::ERROR` (`ptr.rs:125`), but such a ptr **cannot resolve back** (kind mismatch), so IDE node-resolution silently dies for a non-ink frontend.
- IDE consumers of the ptr/visit surface: `crates/internal/brink-ide/src/{folding,story_graph,hir_projection,fn_value_hover}.rs`, `brink-fmt/src/lib.rs`, plus ~10 analyzer passes (grep for `HirVisitor`).

## D. Container identity is name-string + positional-scope-string keyed

- **Container IDs are NOT in admitted HIR.** They are stamped *after analysis*, before LIR, by `stamp_container_ids` (`hir/stamp.rs:24`), inside `normalized_stamped_query` (`crates/internal/brink-db/src/queries/mod.rs:1341`).
- Stamp walks **only** `root_content` → knot bodies → stitch bodies (`stamp.rs:24-62`). No deeper nesting exists to walk.
- IDs are `DefaultHasher` hashes of synthetic **scope-path strings**: `"{scope}.c{n}"` (choice), `"{scope}.g-{n}"` (gather), `"b-{n}.{i}"` (cond branch), `"s-{n}"` (sequence) (`stamp.rs:107-256`, `alloc_address` `:286`).
- Labeled containers instead take the **analyzer-assigned** id via `lookup_label_id(index, "knot.stitch.label")` (`stamp.rs:296`) — so stamp and the analyzer's label-id allocation must agree on the *string* scheme.
- LIR reads the stamped ids, falling back to `ctx.root_id` when `None`: `cs.gather_id`, `choice.container_id`, `branch.container_id`, `seq.container_id` all `.unwrap_or(ctx.root_id)` (`crates/internal/brink-ir/src/lir/lower/mod.rs:834,983,1018,1249`). LIR also does `lookup_container_id(index, knot_name)` — **name-string keyed** (`:670,772,1498`).
- `DefinitionId` itself is a content hash of `(declared-module, name, tag-byte)` (`manifest.rs:391`), the tag from `SymbolKind::definition_tag()` (`symbols/index.rs:123`). **Frozen into `.inkb`** — tripwire test `known_good_bare_definition_ids` (`manifest.rs:732`). Decision-log 2026-07-14 (#719): "Identity stays name-hashed: (module, name)".

## E. The directive/annotation channel is entirely ink-CST-bound (in population)

The `#@…` tag-channel and `@[…]` annotation-line channel are recognized by walking the ink CST:
- `hir/lower/directive.rs` — the whole module. Recognition keys off `SyntaxKind::TAG_LINE`, `ANNOTATION_LINE`, `KNOT_BODY`, `STITCH_BODY`, `SOURCE_FILE`, sibling-walk (`prev_sibling_or_token`), etc.
- Produces clean HIR fields: `Knot/Stitch/VarDecl.is_local: bool`, `.effects_assertion: Option<EffectsAssertion>`, `HirFile.module: Option<ModuleDecl>`, `.visibility: Vec<VisibilityDirective>`, `.was_directives: Vec<TextRange>`, `.imports: Vec<Import>` (`hir/types.rs:41-83`, `211-246`).
- So the **channel (HIR fields) is clean and frontend-agnostic**; only the **population** (parsing `#@` tags out of the ink CST) is ink-bound. A native frontend populates the same fields from its own keyword syntax (`extern`, `var`, visibility keywords).
- Decision-log: `@[effects(…)]` is the ruled final form (2026-07-18 closers); `#@effects` is a deprecation alias that warns E110 (`directive.rs:766`). **Drift note:** the code's `parse_effects_clauses` (`directive.rs:810`) still uses the colon grammar `reads:` for *both* channels, but the 2026-07-19 tower ruling says `@[effects]` uses paren-style `reads(gold, hp)` "so flags can never be swallowed into open clauses". The implemented annotation-channel grammar has not caught up to the ruling.
- Reserved-namespace rule (`docs/directive-annotations-spec.md:27`): every `@`-tag must be a valid directive in a valid placement or it's a hard error (E044–E050) — never a silent inert tag. This is the *tier-1 admission posture* already applied to one channel.

## F. Weave-folding artifacts leak into the "semantic" HIR

The charter dissolves the gather as an authorial concept (§5, "THE GATHER IS DISSOLVED"), but the HIR still models choices as weave-fold output:
- `ChoiceSet { choices, continuation: Block, context: ChoiceSetContext, depth: u32, gather_id }` (`hir/types.rs:469`).
- `ChoiceSetContext::{Weave, Inline}` (`:455`) and `depth: u32` (`:480`) are pure weave-fold bookkeeping. `docs/compiler-spec.md:117` even says "Downstream passes never inspect depth values" — yet the field is mandatory on the node.
- The continuation is "loose ends for codegen to wire up" when unlabeled (`:474`). A native frontend with explicit `{?}` points + braced bodies + `else` must *synthesize* this ink-shaped structure (fake `depth`, `context`, gather continuation).
- `root_content: Block` (`:43`) — ink's file-level loose content before the first knot. Native files hold only declarations; `root_content` would always be empty.

## G. Globals are hoisted by whole-tree walk (ink semantics baked into lowering)

- `hir/lower/structure/mod.rs:117-134` — "In ink, VAR/CONST/LIST are always global regardless of where they appear. Walk the entire tree to collect them all." (`syntax().descendants().filter_map(ast::VarDecl::cast)`).
- STRUCT is top-level-only (`:137`, `file.struct_decls()` direct-children). So the flat `HirFile.variables/constants/lists` vecs encode an ink scoping rule; native's real module scoping produces the same flat vecs by a different route.

## H. `await` / continuation model (FS rulings — constrains the contract)

- `AwaitStmt` (`hir/types.rs:435`) at `Stmt::Await` (`:358`) and `BlockStmt::Await` (`:391`); `WhileStmt.is_await` (`:424`). **Statement position only** (`docs/flow-suspension-spec.md:48,72` — "Mid-expression await is permanently out").
- Functions never await; only tunnels (`flow-suspension-spec.md:74`). Decision-log 2026-07-16.
- Continuation identity = `module + enclosing def + site index`, **source pre-order**, never an instruction offset (`hir/frame_shape.rs:20-63`, `ContinuationSite { def_path, site_index }`). "Site indices are assigned in source pre-order within a def, so adding an `await` after existing ones does not renumber the earlier sites."
- Frame shapes are **name-keyed** (`frame_shape.rs:1-43`), rehydrated across recompiles via the same machinery as `#@was`/saves.
- E052 fence: `await` parses + passes the E105 purity gate but is fenced at LIR lowering until FS-3r (`hir/types.rs:1285` E052 doc; `stamp.rs:273` stamps no container yet).
- **Contract obligation on a frontend:** place `await` only at statement position in *flow* (not fn) bodies, and preserve source pre-order of await sites (continuation identity depends on it).

## I. The analyzer's silent-failure couplings (the dragons — adversarial sweep)

No analyzer pass hard-panics on a violated HIR invariant in production; **the uniform failure mode is silent wrong output** (dropped signature, misfired/missing diagnostic, `Ty::Unknown`). The load-bearing couplings a second frontend can violate silently:

1. **Range-equality join.** The entire typing/gating layer joins resolutions to HIR by *exact* `TextRange`. `ResolvedRef.range` is copied from `UnresolvedRef.range` (`resolve.rs:228`); consumers look up by the *HIR expr's own* `.range`: `dialect_gate.rs:73,284,300`; `infer/body.rs:379`; `strict.rs:900`; `structs.rs:604`; `await_purity.rs:51`. Nothing checks the manifest ref-range equals the body expr-range — the ink lowering emits both from the *same* node. A frontend computing them from different nodes (or different trivia trimming) makes inference see the path as `Unknown` (silent) and makes the dialect gate mis-flag resolved calls.
2. **DefinitionId content-hash agreement at two independent sites.** Locals hash `(scope-qualified-name, tag)` via the shared `local_definition_id` (`manifest.rs:416`), called both by `insert_local` (`:348`) and `resolve.rs::lookup_local_in_scope` (`:756`). Manifest `LocalSymbol.{scope,name,kind}` and `UnresolvedRef.scope` must be constructed so both hash sites agree. Frozen into `.inkb`.
3. **Name-spelling conventions, unvalidated.** Stitches `"knot.stitch"`, labels `"knot.label"`/`"knot.stitch.label"` (`lookup_label_in_knot` requires `name.matches('.').count()==2`, `resolve.rs:991`), list items `"List.item"` (suffix-match `resolve.rs:948`). Wrong qualification → silent `E024`/`E025`, never a "malformed name" error.
4. **Two "is a function" encodings.** HIR `Knot.is_function` (`strict.rs:220`, `infer/mod.rs:455`, `validate.rs:179`) AND index `SymbolInfo.detail == Some("function")` — a stringly-typed sentinel (`signature.rs:186`). Both frontend-stamped, never cross-checked.
5. **`ContainerPtr` variant ⇄ indexed `SymbolKind`.** `collect_defs`/`strict`/`structs` switch on `knot.ptr` to pick `SymbolKind::Knot`/`Stitch` for the id lookup (`infer/mod.rs:444`, `strict.rs:210`, `structs.rs:279`). The #626 "floating stitch" trap: a `= stitch` before any knot lands in `hir.knots` as `ContainerPtr::Stitch` with a bare name; wrong ptr → def silently drops from inference, no diagnostic.
6. **`Return.ptr` as a semantic bit.** `E032` (return-outside-function) fires only when `ret.ptr.is_some() && !in_function` (`validate.rs:223`); a tunnel return has `ptr == None`. A frontend that always attaches a ptr emits spurious `E032`. The tunnel-vs-explicit distinction is encoded *purely in whether a syntax pointer was attached*.
7. **Divert-last-in-inline-branch.** `validate.rs:288` trusts HIR puts a divert last in an inline conditional/sequence branch; a non-last divert yields spurious `E033`/`E029`.
8. **`is_ref` positional alignment.** `record_ref_param_writes` (`body.rs:716`) matches call args to params *positionally*; a mis-stamped `Param.is_ref` silently drops or fabricates an effect-write.
9. **Builtins not pre-resolved.** `resolve`/`dialect_gate` assume the frontend leaves `len`/`keys`/`none`/`LIST_MIN`… as *unresolved* refs. Issue #2863 unified the previously hand-synced name lists: `resolve.rs`'s `is_builtin_function`/`is_t1b_stdlib_name` are now thin delegates to the single canonical list in `brink_ir::lir` (`brink-analyzer` no longer hand-keeps its own copy), and `dialect_gate.rs:228`'s `is_t1b_stdlib_call_name` was already a delegate to `resolve::is_t1b_stdlib_name`, not a second hand-kept copy — its doc just read that way. The "not pre-resolved" coupling itself is unchanged (a frontend still must leave these names unresolved), but the specific hand-sync duplication this item named is gone.
10. **`Dialect`/`TypePolicy` are NOT in HIR** — they arrive as `AnalysisOptions` (`lib.rs:75`); a frontend cannot and must not embed them.
11. **Declarations aren't in the shared block-tree walk.** `hir::visit::HirVisitor` deliberately skips the flat file-level decl vecs and tag contents (`hir/visit.rs:19-33`). Issue #2098 migrated `structs`/`map_keys`/`conversions` (plus `coalesce`/`contains_domain`/`range_refinement`) off their own hand-rolled initializer recursion onto `visit::visit_with_decl_initializers`, so those six no longer hand-walk `hir.variables`/`hir.constants`/`hir.structs` initializers separately. The real remaining initializer hand-walks are `comparator_contract.rs:384-389` and `ufcs::resolve` (`ufcs.rs`) — see #2141 for the tracked follow-up. (`dialect_gate.rs:87-95` and `annotations.rs:256-265` are a different family: they hand-walk decl *annotations* — `var.annotation`/`c.annotation` — not initializer expressions, so they were never part of this hand-walk group.) A frontend adding a new declaration-position expression must remember every hand-walk site.

## J. Salsa/incrementality invariants the contract inherits

- A knot body lives in exactly one file → per-def queries take only the *declaring-file* HIR dependency (`crates/internal/brink-db/src/queries/mod.rs:33-56` module doc; `def_body_query` `:641`).
- `LoweredFile`/`AnalysisResult` derive `PartialEq` for early-cutoff (`queries/mod.rs:300`, `lib.rs:104`). Every HIR type derives `PartialEq/Eq` (incl. `AstPtr` via manual impls, `ptr.rs:107`). A frontend's provenance token must be `Eq`.
- Ranges are deliberately *zeroed* in `resolution_index_query`/`inference_index_query` to keep them out of downstream `Eq` (subagent DB report §2) — but resolution still joins on the *body* ranges (dragon #1). Ranges are both cache-poison and identity-key.

## K. Charter watch-list items — HIR support status

- **blocks-as-values** (code dialect everything-is-expression): NO HIR support. `Stmt`/`Expr` are separated; `LogicBlock` is a `Stmt` that yields nothing. Semantics-touching → parking-lot.
- **deep nesting > knot.stitch**: NO HIR support. `Knot.stitches: Vec<Stitch>`; `Stitch` has no sub-stitches; stamp recurses exactly 2 levels; name conventions bake in 2 (`.matches('.').count()==2`). Real gap.
- **`for`-generated choices**: sugar over the recursive-thread pattern; lowers to existing `ThreadStart` + `ChoiceSet` (charter §5/§7). No new node.
- **`for k, v`** (ruled 2026-07-18): `ForStmt.var_name: Name` is single (`hir/types.rs:444`). Additive `val_name: Option<Name>` **landed** (#1461, Track B2).
- **enums** (§13.1, ruled 2026-07-19): NO HIR node. New `EnumDecl` + exhaustive `match` (could extend `CondKind` or reuse `Switch`). Additive.
- **impl blocks / companions** (S4, ruled 2026-07-19): lower to fns in a virtual companion module — `Npc::greet` is the only real name, `DefinitionId = (companion-module, name)`. NO new HIR container; reuse `Knot(fn)` + module qualification.
- **UFCS** (`x.foo(y)` ≡ `foo(x, y)`): resolves via existing `Expr::FieldAccess` vs `Call` disambiguation, already an analyzer job (`hir/types.rs:712` doc). No new node.
- **lambdas**: `FnLiteral` (`:732`) is partial application over a *named* target — NOT an anonymous body. True lambdas would be a new node; charter §7 leans UFCS-over-lambdas, so defer.
- **`flags`** (renamed LIST): reuse `ListDecl`.
- **splice** (`<- flow(args)` in a point): reuse `ThreadStart` (charter §11 narrowed threads to exactly this).
- **`{?` / annotated-brace family**: `{if}`→`Conditional(IfElse)`, `{match}`→`Conditional(Switch)`, alternation→`Sequence` (bitmask), `{?}`→`ChoiceSet`, `else`→`Choice.is_fallback`. All reuse existing nodes.
