# @brink-lang/web

## 0.11.1

### Patch Changes

- c246a4a: Analyzer: new `E106` warning for statically-visible non-key-domain
  map-literal keys (docs/t1b-surface-spec.md §3, issue #598).

  `#{key: expr, …}` map-literal keys are ratified to the int/string/bool
  domain at runtime (`RuntimeError::InvalidMapKeyType`). §3 already claimed
  "the analyzer warns on statically-visible non-key types", but nothing
  implemented it — `MapLiteral` lowering did zero key-domain checking, so a
  float, array (`#[...]`), nested map (`#{...}`), struct (`Name#{...}`),
  function-value (`#fn(...)`), or ink `LIST` literal used directly as a key
  compiled silently and only failed at runtime.

  `brink-analyzer::map_keys::check` now flags every such entry with `E106`
  (warning severity), wired into `per_file_diagnostics` unconditionally under
  `dialect = brink` (map literals don't exist under `strict-ink` at all —
  already rejected whole by the dialect gate's `E051`). Policy-independent
  like the construction-literal duplicate-field check (`E084`): fires
  identically under both `types = gradual` and `types = strict`, no shape
  resolution needed. A dynamic key (a variable, call, index, or any other
  non-literal expression) is not statically visible and is never flagged —
  the runtime fault remains the sole backstop for those.

  Observable through `@brink-lang/web`: any brink-dialect project compiled
  through the wasm runtime with a non-key-domain literal map key now surfaces
  this new diagnostic in the returned diagnostics array.

- ae66340: Issue #628: `InferredType::List` (the phase-0 `signature()`/hover stub) now
  carries the declaring LIST's name instead of dropping it. A VAR initialized
  directly to a list literal (`VAR w = (sunny)`) previously fed
  `infer::collect_globals` an `Unknown` type via a lossy `InferredType -> Ty`
  conversion — weakening typed-mode inference for list VARs and, under
  `types = strict`, spuriously tripping the Unknown-escape check (`E065`) for
  anything assigned from such a VAR, unlike sibling nominal types
  (`Ty::Struct`, `Ty::Handle`) which were already treated as clean.

  Observable through `@brink-lang/web`: hovering a list-literal-initialized
  VAR/CONST now shows its nominal type, e.g. `w: list<Weathers>`, instead of
  the bare `w: list` it showed before.

- 7baa01f: brink-fmt: canonicalize whitespace around type-annotation colons (#642).

  Type annotations in knot parameters, return types, VAR/CONST/LIST declarations,
  TEMP declarations, and struct fields now render with canonical spacing:
  `name: type` (no space before colon, one space after), regardless of source
  spacing. This normalizes `name:type` (no space), `name: type` (space), and
  `name:  type` (multiple spaces) to a consistent canonical form, matching the
  ink language reference's documented style.

  Changes apply to:

  - Knot headers: `=== function f(x:int, y: int): int ===` → `=== function f(x: int, y: int): int ===`
  - Declarations: `VAR gold:int = 100` → `VAR gold: int = 100`
  - Logic lines: `~ temp name:string = who` → `~ temp name: string = who`
  - Struct fields: `STRUCT P = #{x:int, y: float}` → `STRUCT P = #{x: int, y: float}`

  Formatting remains idempotent: re-formatting an already-canonical annotation
  produces identical output.

  Observable through `@brink-lang/web`: the editor's "Format knot" code action
  now produces canonicalized annotation spacing in formatted output.

- aa43bb6: Analyzer: `E071` (mistyped struct construction field, strict mode) now
  classifies variable-, call-, and index-valued initializers, not only
  literal-shaped ones (issue #670).

  `STRUCT` construction-literal type checking previously only classified
  literal-shaped field initializers (scalars, arrays, maps, nested struct
  literals) — a variable, function call, or indexing expression stayed
  silently unchecked, deferring entirely to the runtime fault. `E071` now also
  consults the whole-project inference substrate (`BodyTypes::locals` for a
  param/temp, the declaration-derived type for a global `VAR`/`CONST`, the
  resolved callee's `InferredSig::return_ty` for a call, and the base's
  classified element/value type for an index) when the initializer's own
  shape isn't literal. Whenever that resolution lands on `Unknown` or
  `Conflicted` — unresolved, unannotated, or genuinely contradictory — the
  field stays silently unchecked, same "Unknown never disagrees" posture as
  every other gradual-mode-aware check in this analyzer.

- edf92bc: Added the M-2 module imports + visibility surface (docs/modules-spec.md
  §2/§4/§7), building on M-1's module name model.

  - **`IMPORT` grammar** — both forms: bare `IMPORT { a, b AS c } FROM mod`
    and qualified `IMPORT mod`. `FROM`/`AS` stay contextual soft keywords;
    only `IMPORT` is reserved. Superset-parsed always; the brink-dialect gate
    rejects `IMPORT` under strict-ink (E051-class), like `#@module`.
  - **`#@private` / `#@public` visibility** on every importable definition
    (knot, function, VAR, CONST, LIST, STRUCT). Effective visibility follows
    declaration-flips-default: a declared module defaults private, an
    undeclared stem-module defaults public, and the per-definition directive
    overrides that.
  - **Diagnostics** (§7): private-cross-module reference (E087), unresolved
    import (E088), duplicate import (E089), self-import (E090), qualified
    ambiguity code reserved (E091), redundant-override warning (E092), and
    conflicting visibility directives (E093). `#@private`/`#@public` are
    brink-dialect-gated under strict-ink (E051).

  Compat: purely additive and brink-gated. The entire pre-modules world keeps
  visibility public and stays in the permeable flat namespace, so no existing
  story's resolution changes.

- d350551: T1d-2 (#767): manifest handle-kind vocabulary + the `handle<K>` typed-mode
  annotation form (`docs/t1d-spec.md` §3). A registered `HostManifest` can now
  declare a handle kind — `{ "name": "AudioInstance", "base": "handle" }` — and
  the brink-dialect typed annotation grammar gains `handle<K>` (`docs/typed-mode-spec.md`
  §3's first amendment), resolving to a new `Ty::Handle(K)` lattice point:
  pointwise kind match, cross-kind = `Ty::Conflicted` (the #627 lattice). Under
  `types = strict`, a mismatched/unregistered handle kind reuses the existing
  `E065`/`E066`/`E061` machinery — no new diagnostic codes. `Ty::Fn` composes
  with handle-typed params/returns for free (the existing pointwise row
  unification needed no special-casing).

  Observable through `@brink-lang/web`:

  - `HostManifest`'s `BaseType` (`packages/wasm-types`, re-exported by
    `@brink-lang/web`) gains a `"handle"` variant — a host can register
    `{ "base": "handle" }` semantic types.
  - `setHostManifest`'s diagnostics now recognize `handle<K>` annotations: a
    `handle<K>` naming an undeclared/unregistered kind reports `E061` (same
    code, extended message); a declared kind resolves cleanly.

  Scope: this slice wires the manifest vocabulary, the grammar/lattice, and
  the annotation-firewall/diagnostic-content seams (`per_file_diagnostics`,
  `strict::check`'s escape-exemption path). It does not thread the manifest
  through the salsa fine-grained-incremental type-inference substrate
  (`brink-db`'s FG-2 `solve_scc_query`/`call_edges_query` pipeline, or the
  non-salsa `infer_project`/`signature()` seams) — so a genuine cross-kind
  handle mismatch detected purely from body-usage inference (as opposed to an
  explicit annotation) isn't caught yet. Flagged as a follow-up, not silently
  dropped.

- 3c1e1e1: Host semantic-access enforcement for `#@private` definitions (M-2b,
  docs/modules-spec.md §4 boundary rules 2/3), building on M-2's compile-time
  visibility surface.

  - **Per-definition visibility compiled into `StoryData`** — a new optional
    `.inkb`/`.inkt` `Visibility` section (tag `0x0E`) enumerates every
    `#@private` definition's `DefinitionId`. Omitted entirely for all-public
    stories, so the entire pre-modules corpus stays byte-identical and no
    format version bump is needed. Writer + reader + round-trip land together
    for both codecs.
  - **Runtime refuses host semantic access to private defs.** With visibility
    enforcement on (the default), `getVar`/`setVar` on a `#@private` variable
    no-op (`undefined`/`false`), and `goToPath`/`goToPathWithArgs`/`runKnot`/
    `callFunction` into a `#@private` knot or function error. The host is
    outside every module.
  - **Persistence is unaffected.** Save/load/journal/replay serialize the whole
    state, including private cells — persistence routes through `DefinitionId`,
    never the enforced name-based host surface, so pause/resume still holds.
  - **Documented dev-tooling override (play-from-here).** A new
    `setDevVisibilityOverride(allow)` on the story runner and session runs the
    story with enforcement off so editors and debug hosts can start flows at
    private knots and inspect private state; the studio's "play from here"
    sessions enable it automatically. Production hosts leave it off. A host
    capability, not a language switch — the compiled program is identical
    either way.

- c03a73a: M-2c: public cross-module resolution now requires an `IMPORT`
  (docs/modules-spec.md §2), completing the M-2 module surface.

  - **Import-required resolution (`E025`)** — a reference resolving to a
    _public_ definition in another **declared** module which the referring
    file did not `IMPORT` is now `E025` with a did-you-mean-`IMPORT` message.
    Bringing the name in (bare `IMPORT { name } FROM mod`) or importing the
    module qualified (`IMPORT mod`) clears it. The restriction keys off the
    _target's_ module being declared, so the permeable legacy world is
    untouched: a plain multi-file `INCLUDE` project with no `#@module` is one
    big default-public module and every cross-file bare reference keeps
    resolving byte-identically (§3). Only genuinely multi-_declared_-module
    projects are constrained; strict-ink and the existing single-module brink
    corpus resolve exactly as before.
  - **`E091` qualified ambiguity** — a `IMPORT mod` (qualified) whose module
    name also names a definition visible bare in the same file makes `mod.y`
    ambiguous; flagged at the import (fixed with an alias).
  - **`E092` redundant-override warning** — a `#@public`/`#@private` that
    merely restates its module's visibility default is now covered by
    end-to-end reachability tests.

- 83717d3: T1d-2b (#774): threads the registered `HostManifest`'s handle-kind
  vocabulary through `infer_project`/`solve_scc` (and `brink-db`'s FG-2
  `signature_query`/`solve_scc_query` salsa substrate) into inference —
  `docs/t1d-spec.md` §3's remaining gap, disclosed as deferred in T1d-2
  (#767, PR #769). `handle<K>` param/return/temp annotations now resolve to
  `Ty::Handle(K)` during body-usage inference, not just at the
  `signature()`/annotation-firewall seam.

  Observable through `@brink-lang/web`: under `types = strict` with a
  registered `HostManifest` declaring two or more handle kinds, a genuine
  cross-kind handle mismatch detected purely from body-usage inference (e.g.
  two locals of different declared handle kinds compared or reassigned
  together, with neither side's slot independently exempted by its own
  annotation) now reports `E066` (Conflicted-escape) — reusing the existing
  TM-3 machinery, no new diagnostic code. This is the #767 acceptance
  criterion ("binding declared `handle<AudioInstance>` rejects
  `handle<Timer>` at compile time") becoming reachable end-to-end. `types =
gradual` is unaffected — TM-1 inference stays advisory-only there,
  byte-identical.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — vanilla ink has
  no handles by construction, so this is oracle-inert.

- 302c6a2: Added the M-3 renames surface (docs/modules-spec.md §5), completing the
  modules spine: `#@was(old_name)` on modules and definitions, a compiled
  old→new `DefinitionId` alias table in `.inkb`/`.inkt`, and a rehydration
  miss-path lookup that rebinds saved state deterministically instead of
  silently orphaning it under a stale id.

  - **`#@was(old_name)` directive** — on a file-level `#@module` declaration
    (records the module's rename) and on any definition (VAR, CONST, LIST,
    EXTERNAL, knot, stitch). Brink-dialect-gated, like `#@module`/`#@private`.
    A self-alias (`old_name` equals the current name) warns "nothing to
    migrate" (E095); a missing/empty argument is E094.
  - **Compiled `AliasTable` section** (`.inkb` format v5, section tag
    `0x0F`, since `0x0E` was independently claimed by the M-2b `Visibility`
    section) — one-byte section-locally-versioned old→new `DefinitionId`
    rows, sorted for the runtime's binary-search lookup. Matching `.inkt`
    text atoms (`(alias_table (alias $old -> $new))`). Empty for every story
    that uses no `#@was`, including the entire pre-M-3 corpus.
  - **Rehydration miss-path lookup** — `Story::load_state`/the free
    `load_state` function now consult the alias table when a saved visit/
    turn-count id, or a divert-target/fn-token/closure-target id embedded in
    a saved global's value, doesn't match the current program. Still
    unresolved after that surfaces a teaching message in the new
    `LoadReport::unresolved_renames` field (only for a program that actually
    carries alias-table entries — an ordinary content edit with no `#@was`
    stays exactly as silent as before).
  - Retrofits the pre-existing silent save-break on a plain knot rename (no
    module involved) with the same machinery.

  Compat: the `.inkb` format version bumped 4 → 5 (a brand-new mandatory
  section, not part of the v4 RFC's pre-reserved inventory) — checked-in
  `.inkb` artifacts regenerate. `LoadReport` gained a field
  (`unresolved_renames`), changing the JSON shape `StoryRunner::load`/
  `load_bytes` return. The alias table itself is additive and brink-gated;
  the entire pre-M-3 corpus emits an empty table and sees no behavior
  change.

- 4a08940: Close the `FlowInstance`-level host visibility gap left by M-2b (#772/#781):
  `begin_function_eval`/`begin_function_value_eval`/`choose_path_string(_with_args)`
  now refuse `#@private` definitions on any `FlowInstance` driven directly,
  not just through `Story`.

  - **`WebSpeculation.goToPath`/`.evalFunction`/`.resumeFunctionEval`** (the
    wasm bindings over `brink_runtime::Speculation`, which drives a
    `FlowInstance` clone directly rather than a `Story`) now correctly refuse
    a `#@private` knot or function with the same `PrivateAccess` error the
    `StoryRunner`-level `go_to_path`/`call_function` surface already enforced
    — previously a speculative fork could read past a private boundary that a
    live `Story`-mediated session already blocked.
  - Same documented dev-tooling override: a `FlowInstance`'s own visibility
    enforcement flag mirrors `Story`'s, and `Story` keeps every flow it owns
    (default, named, shared) synced to its own setting, so a `Story`-level
    `setDevVisibilityOverride`/play-from-here session behaves identically
    whether or not it composes a `Speculation`.

- b86fee8: M-2c stopgap: cross-**declared**-module same-name duplicate definitions
  are now a hard error under `dialect = brink` (issue #784).

  - **`E096`** — two _declared_ modules (`#@module(name)`, different names)
    each defining a same-name, same-kind symbol (a knot, stitch, VAR/CONST,
    LIST, STRUCT, EXTERNAL, or label) is now a compile error, reported at
    _both_ definitions' spans. Flat resolution (unchanged by this stopgap —
    true import-scoped resolution is tracked separately, #790) binds a bare
    name to whichever declared-module definition merge happens to see first,
    so two declared modules sharing a name silently made that binding
    order-dependent. Escalating to a hard error makes flat resolution correct
    by construction until scoping lands.
  - A duplicate _within_ one declared module (same module name on both
    files), or involving any undeclared/legacy file, keeps the existing
    `E022`/`E023`/`E026` warning — unchanged.
  - Gated to `dialect = brink` only: under `strict-ink` (the default), this
    code never fires — the compat/oracle corpus is untouched.

- 1e1be68: Closed two M-3 rehydration miss-path gaps disclosed by the renames PR
  (#782 / docs/modules-spec.md §5): a saved VAR/CONST/LIST global whose own
  name was renamed (`#@was`) — declared-module or bare — now rebinds through
  the compiled alias table instead of being dropped as unknown, and a saved
  `Value::List`'s active items/origins now deep-rebind on a rename exactly
  like `Array`/`Map`/`Record` already did.

  - **`SaveState` gains a `global_ids` field** — each saved global's
    compiled `DefinitionId` at save time, keyed by the same name as
    `globals`. Additive and `#[serde(default)]`, so an older save missing
    the field just falls back to the pre-existing unknown-global report — no
    behavior change for saves that don't use `#@was`. This is what lets the
    miss path recover a renamed global's identity: a VAR/CONST/LIST living
    in a **declared** module hashes as `(module, name)`, so the bare name
    string alone can't reconstruct it once the name itself changed.
  - **`Value::List` is now deep-rebound** — `load_state`'s recursive
    id-rebind walk previously covered `DivertTarget`/`FnRef`/
    `VariablePointer`/`Closure` and their `Array`/`Map`/`Record` containers,
    but fell through to a no-op for `Value::List` itself; its `items`/
    `origins` `DefinitionId`s are now walked and rebound the same way.
  - A global-name miss that resolves via the alias table rebinds silently
    (same discipline as address/global-pointer misses); still unresolved
    (only checked for a program that carries alias-table entries at all)
    reports through `LoadReport::unresolved_renames` alongside the existing
    `unknown_globals` entry.

  Compat: `SaveState`'s JSON shape gains one field (`global_ids`) — decoders
  that deserialize leniently (ignore unknown/extra fields) are unaffected;
  `StoryRunner`/`StorySession`'s `save_state`/`load_state` round-trip it
  transparently.

- c36b8c4: Issue #786 (T1d follow-up): extends the strict call-checking machinery to
  `EXTERNAL` binding call sites — a manifest-registered binding declared to
  take `handle<AudioInstance>` now rejects a `handle<Timer>` argument at
  compile time, closing the last disclosed gap from T1d-2 (#767, PR #769)
  and T1d-2b (#774, PR #779): those two slices covered a _local-vs-local_
  handle-kind mismatch found by body-usage inference, but not a _binding's
  own declared param_ vs. a call-site argument.

  Mechanism: `infer::collect_external_sigs` resolves each manifest-registered
  `EXTERNAL`'s declared parameter/return types to `Ty` (handle kinds via the
  same `declared_handle_kinds` vocabulary `handle<K>` annotations already
  resolve against) and seeds them into `known_sigs` before body inference
  runs — a call to the binding now types its arguments through the exact
  same `known_sigs`/`observe`/`unify` path an ordinary knot/stitch call
  already uses. A cross-kind argument folds to the pre-existing `Ty::Conflicted`
  lattice point and reports through the existing `E066` (Conflicted-escape)
  diagnostic — no new diagnostic code.

  Observable through `@brink-lang/web`: under `types = strict` (`IdeSession
.set_type_policy("strict")`) with a registered `HostManifest` (`setHostManifest`)
  declaring two or more handle kinds and at least one `EXTERNAL` binding whose
  manifest entry declares a handle-kinded param, a call site passing an
  argument of a _different_ declared handle kind now reports `E066` where it
  previously reported nothing. `types = gradual` is unaffected — the existing
  runtime fault at the binding boundary stays the only enforcement there,
  byte-identical. An `EXTERNAL` with no matching registered manifest entry
  (inline-doc-only) stays unchecked, same as before this issue.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — analyzer/
  diagnostic surface only, no compiler/codegen change reachable by vanilla
  ink (no handles by construction), so this is oracle-inert by construction.

- 71dd2fc: M-2d: true import-scoped resolution (docs/modules-spec.md §2), relaxing the
  #784/#793 `E096` stopgap.

  Resolution now consults each file's own `IMPORT` list and declared module: a
  bare reference with same-name candidates across different declared modules
  binds to the module _this file_ imported, rather than to the flat
  duplicate-winner. Because same-name public definitions across declared
  modules can now be disambiguated per-importer, they are **legal** — the
  `E096` "duplicate definition declared in two different modules" hard error is
  retired; two modules may each export `ambush` and two files may import
  different ones, each binding its own.

  - **Import-scoped `lookup_by_name`** (`brink-analyzer::resolve`) — a new
    per-file `ImportScope` (own declared module + imported modules) threads
    through every resolution lookup site. With zero or one candidate (all of
    strict-ink and every single-module project) the fast path is byte-identical
    to the previous flat resolver, so no existing corpus resolution moves.
  - **Coexistence in the index** (`brink-analyzer::manifest`) — a cross-declared
    -module same-name/same-kind pair is no longer dropped as a duplicate; both
    are indexed. Within-module and legacy/undeclared duplicates keep the
    ordinary `E022`/`E023`/`E026` warning; strict-ink is untouched.

  Byte-identical strict-ink and single-module resolution; oracle ratchet
  (5,577) unchanged.

- 213a7f5: Added the M-4 modules tooling tail (docs/modules-spec.md §9): editor
  affordances riding the existing code-action, folding, and formatting seams.

  - **Auto-import quick-fix** — a cursor on an out-of-scope module reference
    (`E025`, import-required) now offers an _"Import `name` from `module`"_
    quick-fix that inserts the `IMPORT` line in the right place: below any
    existing `IMPORT` block, else below the `INCLUDE` block, else at the top
    under the `#@module` header. The offer is session-aware (it reads the
    module-qualified db that produces the live `E025` squiggle) and resolves
    as a pure source rewrite through the same `resolve_code_action` seam. It
    surfaces in both the wasm editor's code-action menu and the LSP.
  - **Import-block folding** — a run of two or more leading `IMPORT`
    statements folds into a single `IMPORT … (N modules)` region, mirroring
    the `INCLUDE` block fold.
  - **`IMPORT` formatting** — `brink fmt` canonicalizes import spacing:
    `IMPORT {  a , b  AS c } FROM  m` becomes `IMPORT { a, b AS c } FROM m`,
    and `IMPORT   mod` becomes `IMPORT mod`. Malformed (mid-edit) imports are
    left verbatim.

  Compat: purely additive and brink-gated. Every trigger requires a
  `#@module`/`IMPORT` construct absent from the entire pre-modules corpus, so
  no existing story's diagnostics, folds, or formatting change.

- 730c947: Circular-`INCLUDE` error messages are now deterministic.

  `IncludeGraph::find_cycle` (`crates/internal/brink-db/src/include_graph.rs`)
  previously picked its DFS start node from a `HashMap`'s key iteration order,
  so which rotation of a multi-file `INCLUDE` cycle got reported in
  `DiscoverError::CircularInclude` depended on that map's per-process
  `RandomState` seed. `brink-web`'s wasm-exported `compile` / `compile_fragment`
  / `compile_project` (`crates/brink-web/src/compile.rs`, `session.rs`) reach
  this path through `brink_compiler::compile` -> `brink_driver::discover` ->
  `ProjectDb::find_cycle`, and surface the message verbatim into the JSON
  `error` field. A multi-file project with a circular `INCLUDE` chain compiled
  through `@brink-lang/web` now gets a stable, reproducible cycle-rotation
  string across runs instead of one that could vary process to process.

- a0d9ee2: Close the `spawn_flow` by-id visibility gap left by M-2b (#772/#781/#783/#796):
  `Story::spawn_flow`'s `DefinitionId` entry point and `Story::spawn_flow_shared`'s
  resolved `container_idx` entry point now refuse a `#@private` target with the
  same `PrivateAccess` error the named-lookup paths already enforce.

  - **`StorySessionHandle.spawnFlow`/`StoryRunnerHandle.spawnFlow`** (the wasm
    bindings over `brink_runtime::Story::spawn_flow_shared`, which resolve the
    target path to a `container_idx` themselves via `find_address` before
    calling in) now correctly refuse a `#@private` knot with `PrivateAccess`
    instead of silently starting a flow at it — previously a host holding (or
    resolving) a private target's address could bypass the name-based refusal
    entirely.
  - Same documented dev-tooling override: `Story::set_visibility_enforcement`
    still governs both entry points, matching every other refusal surface.

- 7ac0a5d: Issue #805 (PR #794 / issue #786 lineage): widens `EXTERNAL` call-site
  checking under `types = strict` to three cases the wave-10 reconciliation
  flagged as missing:

  1. **Scalar semantic types from the manifest vocabulary** — a binding
     declared to take a manifest-registered scalar semantic type (e.g.
     `switch_id`, `base: int`) now rejects a mismatched literal type
     (`string`) at compile time, not just `handle<K>` kinds.
  2. **Inline-doc-only externals** — an `EXTERNAL` documented purely via an
     inline `///` `@param`/`@returns` doc comment, with no matching
     `ManifestExternal` entry in the registered manifest, now gets a checked
     signature too (previously disclosed as out of scope by PR #794).
  3. **Return-position kind checking** — a binding's own declared _return_
     type (handle or scalar) now flows into and is checked at its call site's
     usage, not just its declared param types.

  Mechanism: `infer::collect_external_sigs` now merges a binding's declared
  signature from both sources (inline doc wins by param name, else the
  registered manifest entry wins by position — the same merge order
  `external_check::analyze_externals` already uses for its own enrichment),
  and resolves every param/return `TypeRef` against the full registered
  `SemanticTypeDef` table (scalar bases in addition to `handle<K>` kinds).
  Mismatches fold to the pre-existing `Ty::Conflicted` lattice point and
  report through the existing `E066` diagnostic — no new diagnostic code.

  Observable through `@brink-lang/web`: under `types = strict`
  (`IdeSession.set_type_policy("strict")`) with a registered `HostManifest`
  (`setHostManifest`), a call site now reports `E066` for (1) a literal
  mismatching a binding's declared scalar semantic type, (2) a cross-kind
  argument to an inline-doc-only binding, or (3) a caller local receiving a
  binding's declared return kind and later used against a conflicting kind —
  none of which previously reported anything. `types = gradual` is
  unaffected — byte-identical.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — analyzer/
  diagnostic surface only, no compiler/codegen change reachable by vanilla
  ink, so this is oracle-inert by construction, same as #786.

- 1198586: Issue #815 (FG-4 train): `IncludeGraph::topological_order` no longer appends
  project files unreachable from the compile entry point as a "shouldn't
  happen in practice, but be safe" fallback. Only `entry` and files it
  transitively `INCLUDE`s now feed `lir_lowering_query`'s inputs.

  Mechanism: `topological_order` used to run a post-order DFS from `entry`,
  then append every remaining live file (sorted by `FileId`) that DFS didn't
  reach. `brink-driver::discover` (the one-shot CLI/oracle-corpus compile
  path) never produces unreached files — it only ever loads `entry` plus its
  transitive `INCLUDE` closure — so the fallback was always a no-op there,
  which is why the oracle corpus (5,577 episodes) is byte-identical before and
  after this change. The fallback mattered only for `ProjectDb`'s other role
  as the long-lived editor-session model, where files are added independently
  of any single entry point and an unrelated file (or an orphaned one, e.g.
  after removing an `INCLUDE`) can coexist with `entry` in the same session
  with no `INCLUDE` edge between them at all.

  Observable through `@brink-lang/web`: in a multi-file editor session (an
  `IdeSession`/equivalent with more than one file loaded and an entry set via
  `setEntry`), a file with no `INCLUDE` relationship to the current entry no
  longer contributes its globals/lists/externals/knots to the compiled
  `StoryData`, and editing that unrelated file no longer invalidates the
  entry's compiled LIR. That file's own diagnostics are unaffected — they run
  as independent per-file passes (`analysis_diagnostics_query`/
  `diagnostics_query`) that were never routed through `topological_order`, so
  they still surface exactly as before.

  Oracle ratchet unchanged (5,577 episodes, byte-identical) — every corpus
  case has a single entry-reachable file set, so this is oracle-inert by
  construction.

- 058f410: T1e-1: `ref lvalue-path` path-projection grammar, HIR, and creation-site
  checks (docs/t1e-spec.md §2/§6, issue #831, tracking #828). No LIR/VM
  support lands in this slice — every path projection still hits a
  deliberate "not yet lowerable" fence (see `E099` below).

  - New expression-position grammar: `ref` followed by an lvalue-shaped
    operand — a plain path, a dotted field chain, `[…]` indexing, or a mix
    (`ref npc.hp`, `ref party[leader].hp`) — legal only as a direct argument
    of a call, `#fn(…)`, or `bind(…)`. Superset grammar (always parses);
    under `dialect = strict-ink` it's a hard `E051` at analysis, same as
    every other brink extension — the oracle/strict-ink corpus is untouched.
  - **`E080`** (reused, not a new code) now also covers the `ref
lvalue-path` form: a projection's root must be a durable global `VAR`
    (`#@local` flow-locals included) — a `temp`/param root or a `CONST` root
    is a compile error, same rule T1c's unmarked ref-argument form already
    enforces.
  - **`E097`** — a `ref` projection outside ref-argument position (a
    standalone value, or nested inside another expression) — a deliberate
    v1 narrowing, tracked as icebox #825.
  - **`E098`** — under `types = strict` only, a projection segment (dotted
    field or `[…]` index) that disagrees with the root's statically-known
    declared shape (`VAR name: Shape = …`).
  - **`E099`** — a path projection with at least one real segment (dotted
    field or `[…]` index — not a bare single-name `ref`) reaches lowering:
    no `MakeProjection`/`ProjRead` support exists yet (lands in T1e-2,
    tracking #828), so this is a clean, targeted stop rather than a silent
    drop or a miscompile. A bare single-name `ref x` (zero segments) is not
    a real projection and lowers exactly like today's unmarked
    ref-argument form — never hits this fence.

- 7500e27: T1e-2: real `MakeProjection`/`ProjRead`/`ProjWrite` lowering, root-cell RMW,
  persistence, and `.inkt` support for path projections (docs/t1e-spec.md
  §3/§4, issue #842, tracking #828). Replaces the T1e-1 `E099` lowering fence
  for every real path-projection ref-argument (`heal(ref npc.hp, 5)`,
  `#fn(heal, ref party[leader].hp)`) with genuine execution.

  - **`Value::Projection`** (wire tag `VAL_PROJECTION`, first emission of that
    reserved tag): `(root cell, ordered segments)`, each segment `Index(i32)`
    or `Key(Value)` — the range-segment kind (`2`) stays RESERVED, never
    emitted (icebox #829, sequence slices). Structural equality (same root +
    equal segments), `Arc`-wrapped for O(1) clone.
  - **`MakeProjection` opcode**: emitted at every real path-projection
    `ref`-argument creation site — index/field-name segment expressions
    evaluate once, in source order (snapshot-at-creation, spec §1(1)).
  - **Root-cell RMW** (`ProjRead`/`ProjWrite`, spec §3: take → walk →
    `make_mut` spine → write → store back): a projection-bound `ref`
    parameter's reads/writes dereference through the identical walk, reused
    by `GetTemp`/`SetTemp`/`TakeTemp`'s dispatch — purely additive, no
    behavior change for any pre-T1e program.
  - **`ProjectionInvalidated`** turn-terminating runtime fault (spec §1(2)):
    a shrunk array, a removed map key, or a struct field the current shape no
    longer declares, checked at read/write time against the root's _current_
    value — never a clamp, never silent.
  - **Persistence**: a projection serializes as an ordinary value; rehydration
    validates the root cell exactly like `VariablePointer` today, including
    the `#@was` alias-table miss path.
  - **`.inkt` atoms** land with a reader in this same PR (the `docs/t1e-spec.md`-
    adjacent #742 discipline): `(projection <cell> (segments (index N) |
(key V) …))`, plus per-codec round-trips (`.inkb`, `.inkt`, transcript).
  - Fixes a pre-existing gap in `#fn(target, ref …)` ref-argument validation
    that rejected every `ref`-marked argument (even the T1c-era bare-path
    form) as "not an lvalue" once T1e's `ref` grammar wrapper was in play —
    `#fn(heal, ref npc.hp)` now validates and lowers correctly.

  Not observable through `@brink-lang/web` directly (no wasm-facing API
  change), but the wire format (`VAL_PROJECTION`) and VM fault surface
  (`ProjectionInvalidated`) are new behavior any consumer executing compiled
  `.inkb` through the wasm runtime can now encounter, so this ships as a
  patch per the wasm-observable-behavior convention.

- bcb5cd3: T1e-3: path-projections tooling tail (docs/t1e-spec.md §8 item 3, issue
  #850). Closes the T1e milestone (#828).

  - **Fixed a display bug**: a `#fn`/closure-bound `ref` parameter captured
    via a path projection (`#fn(heal, ref npc.hp)`) rendered its display form
    as `fn heal(ref hp = ref npc.hp, amount)` — the projection's own `ref `
    prefix nested inside the outer `ref hp = ` the fn-value display already
    supplies. Now renders `fn heal(ref hp = npc.hp, amount)`, matching the
    spec's `ref npc.inventory[3]` path-display convention. Fixed at both the
    runtime (`string(f)`/interpolation, `brink_runtime::value_ops`) and the
    static IDE hover renderer (`brink-ide`'s `fn_value_hover`) that mirrors
    it, so `@brink-lang/web`'s hover surface picks up the same correction.
  - **Completion**: right after typing `ref ` in a call's argument position,
    completion now offers only durable `VAR`s (the only legal `ref
lvalue-path` root, E080) instead of the full argument-position set
    (which also includes `CONST`/param/temp — none of them legal ref roots).
    Path _continuations_ after a `.`/`[` aren't attempted (needs the root's
    resolved shape, out of scope for "where cheap").
  - **`brink-fmt`**: `ref lvalue-path` arguments inside a `~ { … }` block now
    format with the canonical zero-space convention around `.`/`[`/`]`
    (`ref npc.hp`, `ref inventory[idx]`), matching the display form's own
    spacing rather than preserving whatever spacing the author typed.
  - **bevy-brink pass-through audit**: added end-to-end tests locking in that
    a path-projection ref-argument can never reach an `EXTERNAL`/host binding
    as a raw `Value::Projection` — structurally impossible today (an
    `EXTERNAL` declaration has no `ref`-parameter grammar), and any value
    _derived_ from reading a projection-bound parameter inside ink always
    arrives at a binding pre-resolved to a plain snapshot.
  - New book chapter, "Path Projections"
    (`docs/book/src/toolchain/dialect/path-projections.md`), with
    compile-checked `ink`/`text` examples following the Function Values
    chapter's precedent.

- c62687c: Indexed assignment to an absent map key now inserts (JS/Python semantics)
  instead of faulting `MapKeyNotFound` — issue #856, ruled 2026-07-15.
  `memo[k] = v` on a fresh key works, matching the existing `insert()`/
  `push()` stdlib mutators' insert-on-absent behavior; a repeat assignment to
  the same key still overwrites in place rather than growing the map.

  - **`IndexSet`'s map branch** (`brink-runtime`'s `write_index_upsert`, used
    by the `IndexSet` opcode) is now insert-on-absent for a valid-domain key
    (int/string/bool) — array bounds and the map key-domain check are
    unaffected (still turn-terminating faults, no silent growth).
  - **Reads are unaffected** (value-model-spec §11c): `m[k]` (`IndexGet`) and
    `MapGet` still fault `MapKeyNotFound` on a missing key. Path-projection
    writes (`ref`-bound `ProjWrite`, `docs/t1e-spec.md` §4) also keep the
    strict fault-on-missing-key behavior — only the direct `IndexSet` opcode
    changed.
  - **Compiler lowering**: plain `a[idx] = v` (`lower_flat_indexed_assignment`)
    no longer runs a non-mutating pre-check read before taking the root — that
    precheck existed to catch the very fault this issue retires, and it can't
    distinguish "absent map key" from "array out of bounds" before deciding
    whether to fault. Compound assignment (`+=`/`-=`) is unaffected (the
    precheck's value is still needed as the operand). Net effect: a fault
    during plain `a[idx] = v` (array out-of-bounds, an invalid-domain map key,
    or a non-collection root) can now leave the root `Value::Null`, matching
    the documented, already-shipped trade-off `insert`/`remove`'s
    author-supplied keys make (`fault_during_insert_leaves_root_null`) —
    compound assignment still leaves the root untouched on a fault.

  Observable through `@brink-lang/web`: any consumer executing compiled
  `.inkb` through the wasm runtime can now see `memo[k] = v` on a fresh key
  succeed instead of raising `MapKeyNotFound`, so this ships as a patch per
  the wasm-observable-behavior convention. Oracle ratchet unaffected (brink-
  dialect collections only — vanilla ink has none): 5,577 episodes still pass.

- 8870113: Stdlib slice 1 completion: `char_at(s, i)` string-indexing primitive
  (docs/t1b-surface-spec.md §5, issue #857) — a corpus finding that blocked
  string-algorithm ports (levenshtein/tokenizers/edit-distance) with no way
  to read a character out of a string.

  - **Chars, not bytes**: `i` indexes Unicode scalar values (`str::chars`),
    not UTF-8 bytes — a byte-indexed read would panic or split a multi-byte
    sequence for any non-ASCII text. Returns the char at `i` as a
    single-character `String` (ink has no separate char type).
  - **Turn-terminating fault** (value-model-spec §11c: no silent garbage) on
    `i` outside `[0, char_count)`, a non-`Int` `i`, or a non-`String` `s` —
    never a clamp, never a silently-empty result. New `RuntimeError`
    variants `CharAtOutOfBounds`/`CharAtIndexNotInt`.
  - **VM-native** (`CharAt` opcode, `0xDD`), lowercase name, author-
    shadowable with a warning (`E035`) per the existing stdlib-slice-1
    ruling (`is_t1b_stdlib_name`).
  - **Typing rule** declared at introduction: fixed `Ty::String` return
    (a char-as-1-string result), independent of argument types — the domain
    check is a runtime/gradual-mode concern at the `CharAt` op, matching the
    `int`/`float`/`string` conversion intrinsics' posture.
  - `.inkt` text support lands with a reader in this same PR (writer +
    reader + round-trip test, matching the `#742`-adjacent discipline).

  Observable through `@brink-lang/web`: new VM opcode/fault surface any
  consumer executing compiled `.inkb` through the wasm runtime can now
  encounter, so this ships as a patch per the wasm-observable-behavior
  convention.

- e16e8f8: Issue #858: `brink-fmt` now retokenizes single-line `~ expr` logic lines
  through the CST instead of passing the statement's own text through
  unchanged (only the outer `~ ` prefix was previously normalized). A
  single-line logic line now gets the same canonical single-space-around-
  tokens rendering, and the `ref lvalue-path` zero-space convention around
  `.`/`[`/`]`, that a `~ { … }` multi-line block statement already received —
  e.g. `~ temp x   =   0` now formats to `~ temp x = 0`, and
  `~ heal(ref  party[ leader ] . hp,   5)` now formats to
  `~ heal(ref party[leader].hp, 5)`. Reachable through @brink-lang/web via the
  editor's "Format knot" code action (`code_actions`/`resolve_code_action`),
  which runs the whole document through `brink_fmt::format`.
- 820f6c5: T2-2: `#@effects(…)` author-facing assertion surface + the exceedance
  compile error (docs/effects-spec.md §10, sitting 2 — 2026-07-14; issue
  #861, tracked from #859). Builds on T2-1's advisory `effects(def)`
  substrate (issue #860).

  - **Grammar** (the `#@` directive channel, brink-dialect-gated → `E051`
    under strict-ink): `#@effects(reads: gold, writes: alarm, calls: audio)`
    declares an upper-bound effect row on a knot/stitch; `#@effects(pure)` is
    sugar for the empty row. Placement mirrors `#@local` — top of a
    knot/stitch body.
  - **The only diagnostic is exceedance** (`E103`): the definition's inferred
    effect row is not covered by (⊄) its declared bound. Per the sitting-2
    ruling there is no drift policy — an inferred row _narrower_ than its
    bound stays silent; nothing else warns.
  - A clause naming an identifier that isn't a declared global `VAR`/`CONST`
    (`reads`/`writes`) or a declared `EXTERNAL` (`calls`) anywhere in the
    project is `E102`; malformed directive grammar (missing argument, unknown
    clause keyword, non-identifier value) is `E100`/`E101`.
  - Wired lazily: an unannotated project never triggers effect-row inference
    — only defs that actually carry `#@effects(…)` cause `effects(def)` to be
    computed.

  Oracle byte-identical (5,577 episodes unmoved) and the strict-ink corpus
  untouched — this is a brink-dialect-only analysis surface with no format,
  codegen, or runtime change. Ships as a `@brink-lang/web` patch because the
  new diagnostic codes (`E100`–`E103`) are editor-observable (LSP/IDE
  diagnostics) through the wasm analysis pipeline.

- 45eb96b: T2-3: first real emission into the reserved `EffectRows` `.inkb` section
  (docs/effects-spec.md §11, format-v4-rfc §2). The wire surface grows —
  compiled `.inkb` artifacts now carry a factored effect-row table — even
  though the runtime does not consume rows yet (additive metadata; the
  linker never reads them, so episodes stay byte-identical).

  - **Section graduated** — `EffectRows` (tag `0x0D`) moves from reserved
    (count-0) to a real, section-locally-versioned section (version byte
    bumped, no format `VERSION` bump — the reservation existed for exactly
    this). Writer and reader land together, with `.inkt` text atoms and
    per-codec round-trips (inkb + inkt).
  - **Factored rows** — each entry ships a direct part (reads / writes /
    call atoms / opaque) plus a per-dispatch list (`{cell, narrowable-bit,
static fallback}`, empty in v1 — a flat row would foreclose §7
    narrowing). Every knot/stitch ships its container row (the host's
    resume-scheduling estimate, §12.1), keyed in a `DefinitionId → row`
    table.
  - **Reserved parameter slots** — each call atom carries a
    capability-parameter slot populated `(any)` in v1 (component-granular;
    path-granular #826 is the later consumer) and a reserved
    handle-parameter slot (t1d-spec §7), left `None` in v1.

- e8cb050: T2-4 effects tail (docs/effects-spec.md §10, issue #863): IDE hover now shows
  a knot/stitch's inferred **effect row** on a stable line — `reads: …; writes:
…; calls: …`, or `pure`, or `opaque` for a definition that dispatches through
  a function value. Purely advisory display; the only contract remains the
  optional `#@effects` assertion (`E103` exceedance, unchanged).

  Editor-observable through the shared `brink_ide::hover` path (LSP/wasm hover),
  hence a `@brink-lang/web` patch. No behavior change to compiled output — effect
  rows are additive metadata the runtime never reads.

- Fixed a silent-no-op compiler bug (#869): a direct call through a computed
  fn-value callee — `handlers[state]()`, `obj.field()`, `get_handler()()` —
  used to compile clean and silently drop the call entirely (the parser left
  the trailing `(args…)` unconsumed, so it resurfaced as prose text on the
  content line instead of being parsed as part of the call). Direct-call
  syntax is scoped to a bare variable/temp/param callee (t1c-spec §3); any
  other callee shape now parses as a real (if always-rejected) `CALL_EXPR`
  node and produces a loud, unconditional compile error (`E104`) naming the
  ratified `call(f, args…)` form as the fix, in every dialect and mode.

  Compat: previously-compiling sources using one of these computed-callee
  shapes as a direct-call target now fail to compile with `E104` instead of
  silently dropping the call — the only prior alternative was a wrong,
  silently-corrupted output, so this is a strict improvement, not a
  regression. `call(f, args…)` (the explicit form) is untouched and already
  dispatches through exactly these callee shapes correctly.

- fe0c16d: Fix: T2-2's `#@effects(…)` `reads`/`writes`/`calls` clause resolution
  (`resolve_cell`/`external_declared`) bypassed M-2d's import-scoped
  resolution (issue #790), independently picking a flat, smallest-id
  same-named candidate instead of routing through the shared
  `ImportScope`/`lookup_by_name` machinery every other reference uses
  (issue #881, tracked from #859; the #811 lesson: twin semantic checks
  share one helper, never re-derive).

  Under multi-module projects where two declared modules each publicly
  export a same-name `VAR`/`CONST`/`EXTERNAL`, this could attribute a
  `#@effects` assertion's clause to the _wrong_ module's cell relative to
  the one the asserting definition's body actually reads/writes/calls
  (via the real import-scoped resolver) — producing a spurious `E103`
  exceedance diagnostic, or, by luck of id ordering, silently masking a
  real one.

  `resolve_cell` and `external_declared` now resolve through
  `brink-analyzer::resolve::lookup_by_name` with the asserting file's own
  `ImportScope`, exactly like every other reference resolves — same-name
  cross-module cells are now attributed per-importer, consistently with
  what the body's own resolution binds.

  Oracle byte-identical (5,577 episodes unmoved); single-module and
  strict-ink projects are unaffected (the fast path is byte-identical
  whenever there is at most one same-named candidate).

- 6266cbf: T2-3 follow-up (#882): wire the ruled freeze semantics into `EffectRows`
  emission. The section-local encoding version bumps 1 → 2 (still no format
  `VERSION` bump) — every row gains a leading `is_entry` byte, so a compiled
  `.inkb`/`.inkt` artifact's `EffectRows` bytes change even though runtime
  behavior does not (the section remains additive metadata the linker never
  reads; episodes stay byte-identical — oracle ratchet unchanged at 5,577).

  - **Entry set respects visibility** — a `#@private` definition's row now
    ships with `is_entry: false`: it is not a legitimate host-lookup entry
    point (`docs/effects-spec.md` §10; host semantic lookup on it is refused
    per `docs/modules-spec.md` §4 rule 2). Every other definition defaults
    `is_entry: true`, unchanged from T2-3.
  - **The row itself is never dropped.** `#@private` hides the _name_, not the
    _cell_ (`docs/modules-spec.md` §4 rule 1) — a private knot/stitch/function
    can still be captured as a first-class fn-value token a _public_ path
    holds, and the dispatch-narrowing machinery (§7) resolves such tokens by
    `DefinitionId`, not by name. So the `DefinitionId → row` table always
    carries every def's row regardless of `is_entry`; only host-facing lookup
    is gated by it. This is unconditional (not a reachability computation over
    whether a public path actually captured such a token today).
  - **Writer and reader land together for both codecs** (`.inkb` + `.inkt`),
    each with its own round-trip test for both `is_entry: true` and
    `is_entry: false`, plus an end-to-end `ProjectDb`-level test proving a
    `#@private` def's row is excluded from the entry set but still resolvable
    in the table, alongside an unaffected public row.

- 9e9f07a: Fixed a `.inkt` dump-parity bug (#883, the #742/#871 class): the
  `struct_shapes` section (TM-4 struct/record shape declarations) was fully
  round-tripped through the binary `.inkb` format but silently dropped
  entirely by the `.inkt` textual dump — neither written nor read, despite
  the module doc's claim that every `StoryData` field is represented. A
  compiled story containing `STRUCT` declarations now shows its
  `struct_shapes` section in the `.inkt` debug view (`program_inkt()`,
  surfaced in brink-studio's compiled-output panel) instead of it vanishing.

  Also added a structural exhaustiveness guard to `brink-format`'s
  `proptest_inkt` suite: a match over every `Opcode`/`Value` variant with no
  wildcard arm, so a future variant added to either enum without matching
  generator coverage fails to compile instead of silently escaping fuzz
  coverage — the mechanical fix for this recurring bug class (tracked from
  #397).

- 878be79: Fixed a duplicate `E046` diagnostic on directives with dynamic content
  (`#@effects({expr})`, `#@was({expr})`, `#@private`/`#@public` with dynamic
  content). `apply_scope_directives` had its own generic `d.dynamic` check
  that fired for every directive, including ones with a dedicated handler
  (`effects_assertion_from_directives`, `was_from_directives`,
  `visibility_from_directives`) that independently re-checks `d.dynamic` and
  emits its own `E046`. The generic check is removed in favor of the
  dedicated handlers' own checks — unknown dynamic directives (no dedicated
  handler) still get exactly one `E046` via the fallback arm.

  Compat: strictly fewer diagnostics for an already-invalid construct
  (dynamic content is never valid in a directive); no change for any
  directive that isn't dynamic.

- c66409b: Fixed `Map`/`Map` and `Record`/`Record` equality (`==`/`!=`) faulting with a
  `TypeError` at runtime instead of comparing. `value_ops::binary_op` had no
  match arm at all for these two variant pairs, even though `Value`'s own
  `PartialEq` already implements the ratified structural-equality-with-an-
  `Arc::ptr_eq`-fast-path rule (value-model-spec §4) — the same comparison
  `contains()`'s Array branch already exercises for element containment.
  Both arms now delegate to `Value`'s `PartialEq`; ordering operators
  (`<`, `>`, `<=`, `>=`) on maps/records still fault, as before — no ordering
  is defined for either.

  Note: map equality currently follows `OrderedMap`'s existing (insertion-
  order-sensitive) derived `PartialEq` unchanged. Whether two maps with the
  same entries in a different insertion order should compare equal is a
  separate, still-open question tracked in #909 (parked for a maintainer
  ruling) — this fix does not decide it either way, and map-equality
  semantics may change once that ruling lands.

- 86c4bee: Map/record `==`: map equality is now content-based, not
  insertion-order-sensitive (issue #909, ruled 2026-07-18 —
  `docs/decision-log.md` "Map/record equality is insertion-order-insensitive").

  `#{a:1, b:2} == #{b:2, a:1}` now evaluates `true`. Previously,
  `OrderedMap`'s derived `PartialEq` compared its backing `Vec<(MapKey,
Value)>` positionally, so two maps holding identical key/value pairs
  inserted in different orders compared unequal — a silent correctness bug,
  since ink authors have no way to observe or control the internal `Vec`
  layout an equality check was leaking.

  `OrderedMap` now hand-implements `PartialEq` as a content comparison: same
  entry count (fast-path reject on size mismatch), then every key in one map
  looked up and value-compared in the other — order-independent by
  construction. Every equality-derived operation (`==`, `!=`, and any future
  membership/contains-style check built on `Value::eq`) picks this up
  automatically through `Value`'s existing `PartialEq` delegation to
  `Value::Map`'s `Arc::ptr_eq` fast path and structural fallback — no call
  site changes needed.

  **Unchanged**: iteration order (`iter`/`keys`/`values`) and
  serialization/wire order both stay insertion-order — only equality ignores
  it. Record equality (shape-ordered fields, not insertion-ordered) is
  unaffected by this ruling.

  Observable through `@brink-lang/web`: any ink script comparing two map or
  record values containing maps via `==`/`!=` now gets content-based results
  regardless of the order the maps' keys were built in.

- fdf94f6: FS-1 (#915, tracking #889): the FlowFrame suspended-flow section in
  `SaveState` — format only (`docs/flow-suspension-spec.md` §2/§9). No
  compiler `await` support and no runtime spill/restore land in this slice;
  `Story::save_state`/`load_state` always produce/consume `None`.

  - `SaveState` grows an optional `suspended: Option<SuspendedFlow>` field
    behind `#[serde(default)]`/`skip_serializing_if` — an older save missing
    the key still deserializes, and an unsuspended save's wire form is
    byte-identical to before (no `"suspended": null` noise).
  - `SuspendedFlow` (section-locally versioned via
    `SUSPENDED_FLOW_SECTION_VERSION`, independent of `SAVE_FORMAT_VERSION`):
    the parked flow's current container `DefinitionId`, its tunnel-return
    stack (`Vec<DefinitionId>`), a name-keyed frame record (an ordinary
    `Value`, so no new wire representation), and a `WakePolicy` (await-site
    id + optional condition fn token + a `WakeSource` host-source
    discriminant). All identity rides name-stable `DefinitionId`s, never
    instruction offsets — the same recompile-stability contract as the rest
    of `SaveState`.
  - Round-trip tests per `docs/flow-suspension-spec.md` §7: both
    `WakeSource` variants, the absent/backward-compat case, and a
    frame-shape-drift case proving the name-keyed encoding survives a
    missing/extra/renamed crossing-local between save and load (the
    tolerant _decode_ itself is FS-3 scope).

  Inert wire growth: this is purely additive surface with no producer yet,
  so no existing save or story's observable behavior changes.

- 9d559a3: Fixed `Array`/`Array` equality (`==`/`!=`) faulting with a `TypeError` at
  runtime instead of comparing. `value_ops::binary_op` had no match arm at all
  for this variant pair, even though `Value`'s own `PartialEq` already
  implements the ratified structural-equality-with-an-`Arc::ptr_eq`-fast-path
  rule (value-model-spec §4). The arm now delegates to `Value`'s `PartialEq`;
  ordering operators (`<`, `>`, `<=`, `>=`) on arrays still fault, as before —
  no ordering is defined.

  Unlike the parked map-ordering question in #909, array equality is
  unambiguously order-sensitive by construction — element order is observable
  array structure, not an incidental insertion artifact — so there is no
  analogous ruling to park here: `[1, 2] == [2, 1]` is `false`.

- cc1d11e: FS-2 (#928, tracking #889): the FlowFrame compiler slice — `await`
  grammar/HIR/lowering, the effect-free condition purity gate, and the LIR
  lowering fence (`docs/flow-suspension-spec.md` §3/§5). Compiler + analyzer
  only; the runtime spill/restore is FS-3.

  New syntax reaches the wasm parser surface, so the whole grammar is
  observable through `@brink-lang/web`:

  - `await <cond>` parses at statement/logic position — the top-level
    `~ await …` logic line and inside a `~ { … }` block — plus the
    persistent-await `while await <cond> { … }` loop. `await` is a contextual
    (soft) keyword: it stays an ordinary assignable identifier everywhere
    else (`await = 5`, `while await { … }`), so no existing ink is affected.
  - Under the default strict-ink dialect, `await` is a brink extension and is
    rejected with `E051`, like every other superset construct.
  - Under `dialect = brink`, an `await` condition must be **effect-free**
    (read-only): reads are the wake dependency set, but a transitive write to
    a global cell or an effectful call is a compile error — a new diagnostic,
    `E105`, built on the effects machinery. A bare fn-value reference used as
    a dynamic condition (`await ready`) is read-only by construction and is
    never flagged.
  - Every `await` construct is then fenced at LIR lowering with `E052` (the
    reserved "parses/analyzes before its lowering lands" code): its runtime
    spill/restore semantics are FS-3, so a program using `await` refuses to
    lower to bytecode rather than silently dropping the suspension point.

  Vanilla ink has no `await`, so no existing story's compiled output or
  runtime behavior changes.

- 62cb759: FS-2 follow-up (#928, tracking #889): harden the `await`-condition purity
  gate (E105) flagged in PR #935's review.

  - The purity walk (`brink-analyzer::await_purity`) now recurses into
    `Expr::StructLiteral` field initializers in both the effectful-condition
    check and the salsa callee-collection path. An effectful call nested in a
    struct-construction condition (`await Flag#{on: raise_alarm()}`) previously
    slipped past E105 because `StructLiteral` was treated as a non-recursing
    leaf; it is now correctly rejected. (`FnLiteral` stays a leaf — a lambda
    body is not invoked during condition re-evaluation.)
  - Added end-to-end coverage: a two-hop transitive write
    (`condition → outer() → inner() → writes a global`) trips E105, and an
    effectful call inside a struct-construction condition trips E105.

  Wasm-observable: a program with such a condition, which previously produced
  no E105, now surfaces the purity error through the diagnostics surface.

- a350dcf: Runtime `==`/`!=` completeness sweep (issue #939, tracked from #397):
  `VariablePointer`, `TempPointer`, and `Projection` values no longer
  fault with a type error when compared with `==`/`!=` — they now compare
  correctly (token equality for the pointers, structural same-root-cell +
  equal-segments equality for projections), delegating to `Value`'s own
  `PartialEq` exactly like the prior fixes for `FnRef`/`Closure`/`Handle`/
  `Array`/`Map`/`Record` (#918, #931).

  Also fixes a float-equality inconsistency: direct float `==`/`!=` used
  to tolerate an `f32::EPSILON` fudge factor while a float nested inside
  an array/map/record/projection always compared by exact IEEE equality.
  Both routes now use exact equality (matching the C# reference ink
  runtime's plain `x == y` and the already-shipped collection-equality
  behavior) — a small behavior change: two floats that previously
  compared equal only because they happened to land within
  `f32::EPSILON` of each other (e.g. accumulated rounding error from
  independent arithmetic paths) now compare unequal, same as arrays/maps
  already did with the same inputs.

- 3ad1bc5: brink-format: `read_inkb`'s container decoder now rejects a `.inkb` whose
  declared `param_count` disagrees with the number of per-param name/mode
  metadata entries that actually follow it (#954, sibling of the `.inkt`
  reader's same guard, #745).

  `ContainerDef::params`'s documented invariant is that `params.len()` always
  equals `param_count` whenever per-param metadata is present at all. Before
  this fix, `decode_container` built a `ContainerDef` from the two
  independently-read counts with no consistency check, so a mutated/corrupt
  `.inkb` could construct exactly the inconsistent state the `.inkt` reader
  now rejects. Fixed by validating the invariant at decode time and returning
  a new `DecodeError::ParamCountMismatch` on mismatch — a defined decode
  error, never a panic (the format fuzz lanes wired up in #948 exercise this
  exact path).

  Observable through `@brink-lang/web`: `read_inkb` is called unconditionally
  (not feature-gated) from `brink-web`'s session/story-runner/compile paths,
  so a corrupted `.inkb` payload with this specific inconsistency now surfaces
  as a clean decode error instead of constructing invariant-violating data.

- 2b7dd5a: Runtime: `brink-runtime`'s uppercase `INT()`/`FLOAT()` builtins no longer
  silently fold an unconvertible value to `0`/`0.0` (issue #955, the
  cast-operator leg of the wildcard-fan-out class #950 explicitly scoped
  out).

  `value_ops::cast_to_int`/`cast_to_float` (backing `Opcode::CastToInt`/
  `CastToFloat`) used to end in a `_ => Value::Int(0)` / `_ =>
Value::Float(0.0)` wildcard arm — so a future `Value` variant would
  silently cast to zero instead of getting a considered answer, the same
  hazard class #950 fixed for the marshal/serialize legs. The reachable
  domain (`Int`/`Float`/`Bool`/`String`, including the legacy
  silent-0-on-string-parse-failure fallback) is **unchanged** — verified
  byte-identical against the oracle (5,577 episodes, unmoved). Every other
  `Value` variant (`List`, `DivertTarget`, `VariablePointer`, `TempPointer`,
  `Null`, `FragmentRef`, `Array`, `Map`, `Record`, `FnRef`, `Closure`,
  `Handle`, `Projection`) now raises `RuntimeError::InvalidConversionDomain`
  instead — none of `value-model-spec.md`, `t1c-spec.md`, `t1d-spec.md`, or
  `t1e-spec.md` rules a conversion for these, so faulting is the conservative
  default (the same value-model-spec §11c "no silent garbage" precedent the
  T1b lowercase `int()`/`float()` intrinsics already follow), reusing the
  same fault variant with an uppercase `target` label (`"INT"`/`"FLOAT"`) to
  distinguish it from the lowercase intrinsics' own faults.

  Observable through `@brink-lang/web`: any JS host driving a story through
  `continue_single`/`continue_flow`/`advance` where the ink script calls
  `INT()`/`FLOAT()` on one of the previously-wildcarded variants now sees the
  call reject with a runtime-error `JsError` instead of silently continuing
  with a zero. None of these variants are reachable from vanilla ink source
  today (they're brink-only value kinds), so this cannot fire from a
  plain-ink story — only from brink-specific constructs (records, function
  values, handles, path projections) an author explicitly casts.

## 0.11.0

### Minor Changes

- c9475df: Added `EditorSessionHandle.setLanguageDialect(value)` and
  `EditorSessionHandle.setTypePolicy(value)` (#693), mirroring
  `setSemanticTypeCheck`/`setExternalCheck`. The raw
  `WasmEditorSession.set_language_dialect` (#611) and
  `WasmEditorSession.set_type_policy` (#660) wasm levers existed, but
  `EditorSessionHandle` — the surface `@brink-lang/web` consumers actually
  use — exposed neither, so no JS caller could opt into the brink dialect or
  the typed-mode policy at all (every new construct raised `E051` with no
  opt-in path). `setLanguageDialect("brink" | "strict-ink")` and
  `setTypePolicy("strict" | "gradual")` now delegate to the wasm session and
  bump the generation counter, same as every other mutating call on the
  handle.

### Patch Changes

- 8a3635d: Fixed formatter line classification for constructs containing inline
  `/* … */` block comments (observable via `format_document`). The line
  classifier used to mark any physical line containing a block-comment
  token anywhere in its subtree as a pure comment line, which skipped the
  line's real construct entirely:

  - a single-line `STRUCT Point = #{x: float, /* mid */ y: float}` was
    passed through verbatim instead of being normalized by the struct
    renderer;
  - a block comment on a multiline struct's `#{ /* c */` opening line (or
    a `~ { /* c */` logic-block opening line) caused the entire body to
    lose its indentation;
  - a one-liner `~ x = 5 /* foo */` logic line skipped `~`-spacing
    normalization.

  A single-line block comment nested inside a construct whose renderer
  handles comments itself (struct bodies, `~ { … }` block bodies, plain
  `~` logic lines) is now left to that construct's formatting.
  Free-floating comments — banners, multi-line comments, and comments
  outside those regions (e.g. `STRUCT Point /* c */ = #{…}` or trailing
  after a block's closing `}`) — keep the verbatim treatment.

- 34951ec: Fixed the formatter silently dropping a comment attached to a
  `~ { … }` logic block outside its body (observable via
  `format_document`). A comment that is a direct child of the logic line —
  trailing after the closing brace (`} /* note */`, `} // note`) or
  leading between `~` and `{` (`~ /* c */ {`) — was deleted, because the
  block body was rebuilt from the inner statement block alone. The block
  renderer now emits leading comments on the header line and trailing
  comments on the closing line. A leading comment on the opening line no
  longer de-indents the body to column 0, and a single-line block that
  carries a trailing comment now expands to the canonical multiline form
  (matching the comment-free case) instead of being frozen verbatim.
- 81ddfa7: Fixed a fuzzer-discovered parser bug (PR #672 workstream C's new
  `parse_lossless` fuzz target, which builds with debug-assertions on):
  a `bump_assert` invariant inside the parser could fire on legitimately
  reachable token sequences — e.g. an un-flushed `WHITESPACE` token
  still sitting at the parse position when `conditional_with_expr_standalone`
  dispatches into `expression()` on a `#fn(...)`/sigil-literal expression
  inside a `MULTILINE_BLOCK` — crashing the parser with a
  `debug_assert_eq!` panic in debug builds. In release builds (including
  the shipped `@brink-lang/web` wasm), the same mismatch compiled away
  silently: the parser consumed the unexpected token with no diagnostic
  at all, corrupting the tree instead of erroring.

  `bump_assert` now always emits a proper parse error on a mismatch, in
  every build profile. Observable through `@brink-lang/web`: compiling
  ink source that hits this token-position edge case no longer panics in
  debug tooling, and — this is the real production-facing change — no
  longer silently mis-parses in the shipped wasm build; it now returns a
  normal `ok: false` result with a recovery-error diagnostic, like any
  other malformed input.

- 9c58d6e: Fixed a fuzzer-discovered linker panic (PR #672 workstream C's new
  `vm_no_panic` fuzz target for malformed `.inkb`, previously masked by
  a CI job structure that never actually ran it — see the accompanying
  CI fix — now caught on its first real run). `link()` indexed
  `StoryData::name_table` with a container/address-path `NameId` taken
  straight from the input bytecode with no bounds check; an out-of-range
  `NameId` panicked with `index out of bounds`.

  `link()` now returns `RuntimeError::InvalidNameId` on an out-of-range
  `NameId` instead of panicking. Observable through `@brink-lang/web`:
  `new StoryRunner(story_bytes)` (and every other entry point that links
  caller-supplied `.inkb` bytes) no longer panics/traps the wasm module
  on malformed/corrupted input — it returns a normal error result, like
  any other malformed input.

- f68c094: `brink-fmt`'s `STRUCT` declaration formatting (TM-4b) no longer silently
  drops comments living inside the struct body. Observable through
  `@brink-lang/web` via the `FormatKnot` code action
  (`brink_ide::code_actions::format_region` → `brink_fmt::format`):

  - Multiline `STRUCT` bodies now preserve leading, interleaved, and
    same-line trailing comments between/around fields instead of dropping
    them.
  - Single-line `STRUCT` bodies preserve interleaved block comments instead
    of dropping them.
  - Removed an unreachable dead branch in the multiline struct renderer.

- b9ad39f: Fixed (#674): the brink-dialect assignment-target grammar now recognizes
  an `Index` base for a `.field` write — `arr[i].field = v` parses as a real
  assignment target instead of failing with a generic "expression is missing
  an operand" (E015) parse error. The compiler still rejects this shape (a
  chained/mixed field write, T1e) but now reports the intended `E074`
  diagnostic — "chained field-write projection (p.a.b = v) is not
  supported" — pointing at the target expression, matching the diagnostic
  `o.inner.v = 2.0` already got. Observable through editor/compile
  diagnostics for `.ink` source under `dialect = brink`; no change to
  `p.field = v` (single-level, still lowers via RMW take/make_mut/write-back)
  or to plain `arr[i] = v` indexed assignment.
- b7b7eb0: Struct construction literals (`Name#{field: expr, …}`, TM-4c): fixes #675
  and #676 per the ruling in decision-log "Struct construction literals:
  source-order evaluation, duplicate field is a compile error" (2026-07-14).

  - (#676) Initializers now evaluate in **source** order (left-to-right as
    written), not the shape's declaration order — codegen reorders only the
    already-evaluated _values_ into shape offsets afterward. Previously, when
    the author's field order differed from the shape's declaration order and
    two or more initializers had observable side effects, those effects fired
    in shape order instead of source order.
  - (#675) A duplicate field in a construction literal is now a real compile
    error (`E084`), naming the repeated field, under both `types = gradual`
    and `types = strict`. Previously a duplicate silently kept the last
    initializer's value while the earlier, shadowed initializer's expression
    — including any side effect — was dropped without lowering it at all.

  Observable through `@brink-lang/web`: `compile_project`/`compile_fragment`
  now return a diagnostic (`E084`) for a construction literal with a
  duplicate field, and the compiled bytecode for a well-formed literal whose
  source field order differs from its shape's declared order now evaluates
  initializers in the order the author wrote them.

- d29671d: RCA'd #680 ("`ref`-argument call co-occurring with a `temp` decl in the
  same `~ { }` block resolves to the wrong global slot"): the `ref`-argument
  call was a red herring — the actual defect is reading a T1b block-scoped
  `temp` (`~ { … }`) from _outside_ its own block. LIR lowering's fallback
  for "temp not currently visible" (kept for inklecate-compat forward-
  reference emulation of classic, non-block temps) previously caught this
  case too, silently compiling to a phantom global id that was never
  registered — a runtime-only `UnresolvedGlobal` fault with no compile
  diagnostic.

  Observable through editor diagnostics: referencing a block-scoped `temp`
  (by value or by `ref` argument) after its `~ { … }`/`while`/`for`/`if`
  block has already closed is now a real, non-suppressible compile error
  (`E082`) instead of a silent runtime fault. A `ref`-argument call
  co-occurring with a `temp` decl in the _same_ block — the issue's literal
  repro shape — was already correct and is unaffected.

- ca45425: Fixed #692: a scalar `VAR`/`CONST` declaration default whose _whole_
  value is a non-constant reference or call (`VAR x = someOtherVar`,
  `VAR x = f()` — including either wrapped in a prefix/infix operation,
  e.g. `VAR x = -f()`) previously folded silently to `Null` through
  `eval_const_expr`'s `Path` (`SymbolKind::Variable`) arm and its
  catch-all, with zero diagnostic. This is the same silent-fold bug
  #673/#679 fixed one level down inside array/map/struct declaration-
  default literals (`E075`/`E076`/`E077`), left unfixed at this bare
  top-level scalar position.

  Observable through `@brink-lang/web`: compiling such a declaration
  default now surfaces a real, non-suppressible compile error (`E083`)
  instead of silently producing a `Null` global. A `VAR`/`CONST`
  referencing another `CONST`, or a `Path` reference nested _inside_ a
  collection/struct/fn literal, is unaffected (the latter remains the
  pre-existing, separately-tracked gap #679's scope notes named).

- abc369a: T1c-2 (#700): function values (`#fn(…)`) now lower, execute, and persist —
  the first live use of the V4-reserved `PushFnRef`/`MakeClosure`/`CallValue`
  opcodes and `VAL_FN_REF`/`VAL_CLOSURE` value tags. Observable through
  `@brink-lang/web`:

  - **Program model + disassembly**: a `#fn(…)` baked into a declaration
    default renders as a function-value (`fn <path>(…)`) rather than
    erroring or showing `null`, and the new opcodes disassemble
    (`push_fn_ref` / `make_closure` / `call_value`).
  - **Speculation / eval-function results**: a function value crosses the
    typed-value JSON boundary as an opaque token (`{ "type": "fn", target,
bound }`) — the host never dereferences the env (spec §6); the
    callback-invocation surface lands in T1c-3.
  - **Runtime dispatch**: calling a function value (direct `f(args…)` or
    explicit `call(f, args…)`) works; a non-function callee, a wrong-arity
    explicit call, a rehydration mismatch (a saved closure whose target
    param was renamed/re-moded after a recompile), or invoking a closure
    that `ref`-binds a flow-private `#@local` cell are turn-terminating
    faults — never silent garbage.
  - **Persistence**: function values save/load as ordinary values (save
    state, journal, speculation snapshots); `ref`-bound cells round-trip
    losslessly through the transcript codec.

  The `#inkb` wire format gains per-container parameter name/mode metadata
  (an additive trailing field) so a rehydrated closure can be validated
  against the current signature.

- 30e09f9: T1c-3 (#701): the `bind`/`call` function-value stdlib, the authoritative
  display form, and structural equality land — all observable through
  `@brink-lang/web`:

  - **`bind(f, args…)` stdlib intrinsic**: val-only currying over an existing
    function value — consumes the head of the remaining param row and returns
    a new function value (lowercase, brink-dialect-gated, author-shadowable
    with the E035-class warning, effect-transparent). Lowers to the new
    `bind_value` opcode (`0xD9`), which disassembles alongside `call_value`.
    Over-binding more args than the target has remaining params, or binding a
    non-function value, is a turn-terminating fault (spec §3).
  - **Display form**: `string(f)` and `{f}` interpolation now render the stable
    signature-like form — `fn heal(ref hp = player_hp, amount)` (bound `val`
    args print their value, bound `ref` args print the captured cell name,
    unbound params print bare). This is a permanently observable surface (spec
    §5), property-tested for stability.
  - **Structural equality**: `==`/`!=` on two function values compare
    structurally (same fn token + equal bound rows); any ordering operator
    (`<`, `>=`, …) is a runtime fault in gradual mode / a type error in strict
    (spec §5). Function values remain rejected as map keys.

  Crates-only work (bevy-brink also gains the host callback-invocation surface,
  `call_ink_function_value`), but the runtime-observable behavior above flows
  through `@brink-lang/web`, so it carries a patch per the wasm-observable rule.

- 2541c08: T1c-4 (#702) mechanical tail — corpus growth, a new "Function Values" book
  chapter, and IDE polish. Only the IDE polish is observable through
  `@brink-lang/web`:

  - **Hover on a fn-value slot** (a `VAR`/`CONST`/`temp` bound directly to a
    `#fn(target, args…)` literal, at its declaration or a later plain
    assignment) now shows the bound signature display form — the same
    `fn heal(ref hp = player_hp, amount)` shape `string(f)` renders at
    runtime (spec §5), built statically from the HIR. Every other hover case
    is unchanged; a slot never bound to a direct `#fn(...)` literal (a
    `bind()` result, a copy of another variable, an ordinary value) shows
    nothing extra, same as before.
  - **Completion after `#fn(`** now offers only statically-named function
    definitions (the same shape `#fn`'s E079 creation-site check requires),
    not the generic value-symbol list every other call-argument position
    offers. Completion everywhere else (including `#fn(name, ` — past the
    first argument) is unchanged.

  Crates-only otherwise: the tier1-brink corpus wing grows (a triple-level
  `bind`-of-`bind` chain, a wrong-typed-argument fault, the cross-flow
  `#@local` `ref`-bind fault, and save/load with a live function value inside
  an array/map), and grammar fuzzing extends to `#fn` in both dialects
  (parser is dialect-agnostic, so this is parser-layer coverage) — none of
  this changes any wasm-observable behavior.

- 5b07740: Fix #708: a bare `INCLUDE` (no path) no longer aborts compilation with a
  raw I/O error. Discovery now skips the empty include path the parser
  already flagged, so the parser's `E037` ("expected file path")
  diagnostic reaches the caller. Observable through `@brink-lang/web`:
  `compile_project`/`compile_fragment`/editor compiles on a project
  containing a bare `INCLUDE` now return `ok: false` with an `E037`
  warning entry (placed on the offending line) instead of a generic
  `error: "I/O error: file not found: …"` string with no source location.
- d02c4e2: T1c follow-up (#712): a global `VAR`/`CONST` initialized with `#fn(...)`
  (or annotated `fn(T…): R`) now carries its declaration-derived `Ty::Fn`
  through to call-position checking under `types = strict`, instead of
  escaping as `Unknown`. Observable through editor diagnostics:

  - Calling directly through such a global (`heal_player(5)`, no local temp
    in between) type-checks against the target's known signature: arity/
    argument-type mismatches report `E063` exactly as they already did for a
    `#fn(...)`-initialized local temp.
  - An explicit `VAR f: fn(int): int = …` annotation on the global now wins
    over inference, matching the existing annotation-wins firewall rule.
  - Reassigning a fn-typed local from two globals with genuinely
    incompatible signatures still reports the pre-existing `E066`
    (Conflicted-escape) — previously masked because both globals silently
    escaped as `Unknown`, which unified without a conflict.
  - Gradual mode is unaffected — these checks only ever run under
    `types = strict`.

- 20d2bfa: T1c-2 completion gap fix (#721): the direct-call form `f(args…)` — where
  `f` is a variable/temp holding a function value — dispatches through the
  same `call_variable` opcode as the classic divert-target-variable call.
  That opcode carried no argument count, so the popped-arg count for the
  function-value arm was derived from the resolved target's arity instead
  of the count actually supplied at the call site; a gradual-mode arity
  mismatch on the direct form could leave a stray value on the stack
  instead of faulting.

  `call_variable` now carries an explicit `argc` operand (codegen emits the
  exact count pushed at that call site; the divert-target-variable-call arm
  ignores it, unchanged). Observable through `@brink-lang/web`:

  - **Disassembly**: `call_variable` now renders as `call_variable
argc=<n>` in program-model output.
  - **Runtime dispatch**: a wrong-arity direct call `f(args…)` now faults
    with the same `FunctionValueArity` turn-terminating fault as the
    explicit `call(f, args…)` form, instead of risking a corrupted value
    stack.

- d38fa08: Fixed #743: a bare `VAR` reference _nested one level inside_ a
  `VAR`/`CONST` declaration-default collection/struct/`#fn` literal — an
  array element, a map value, a struct field, or an `#fn(name, args…)`
  bound `val` arg — previously folded silently to `Null` through
  `eval_const_expr`'s `Path` (`SymbolKind::Variable`) arm, with zero
  diagnostic. This is the residue #679's scope notes flagged and #692/
  `E083` deliberately left alone (`E083` governs only the _whole_
  top-level default, not a construct nested one level in).

  Observable through `@brink-lang/web`: compiling such a nested `VAR`
  reference (array element / map value / `#fn` bound `val` arg) now
  surfaces the existing, non-suppressible `E077` — the same code
  `#673`/`#679` already use for any other never-constant nested element
  kind — instead of silently producing a `Null` entry. A struct field
  was already covered (any struct literal used as a declaration default
  is unconditionally `E075`, regardless of field content). A `Path`
  reference resolving to a `CONST`/list item/knot/stitch/function is
  unaffected — it still folds for real.

- 9bef954: T1d-1 (#757): `Value::Handle` — the runtime + format spine for opaque
  host-resource tokens (`docs/t1d-spec.md` §2/§6), the first emission of the
  V4-reserved `VAL_HANDLE` wire tag. No literal syntax and no new opcode —
  handles enter the script world only via bindings. Observable through
  `@brink-lang/web`:

  - **Native binding-argument marshal** (`value_to_js`): a handle passed as
    an argument to a JS-implemented external now crosses as a plain object
    `{ kind, id }` (`kind` the raw manifest `NameId`, `id` a decimal string
    so a full-range `u64` never loses precision as an `f64`) instead of
    silently folding to `null` (the #667 wildcard-arm hazard class).
    Deliberately **not** reconstructed by `js_to_value` — letting any JS
    object shaped `{kind, id}` become a real `Handle` would let a binding
    forge a capability token out of thin air.
  - **Speculation / eval-function results** (`value_to_typed_js`): a handle
    crosses the typed-value JSON boundary as `{ type: "handle", kind, id }`
    — `kind` resolved to its manifest name where possible (`"?"` for a stale
    `NameId`), `id` as a decimal string for the same precision reason.
  - **Program model / disassembly**: a handle default value (reachable once
    T1d-2 wires manifest-aware bindings into declaration defaults) renders
    as `handle <Kind>#<id>`, not `null`.

  Runtime-side (not directly `@brink-lang/web`-observable, but load-bearing
  for the above): `Value::Handle { kind: NameId, id: u64 }` with token
  equality (`kind == kind && id == id`), no ordering (any `<`/`>`/`<=`/`>=`
  is a runtime `TypeError` fault), and never a legal map key. `string(h)`
  displays as `handle <Kind>#<id>`. Handles save/load and journal-replay as
  ordinary values. The `.inkt` textual format gains a matching `(handle
<kind> <id>)` atom and `:handle` declared-type keyword, both with a real
  reader landing in this same PR (the #742 write/read-asymmetry class this
  PR does not repeat for its own new atom).

- 1e71455: Added the M-1 module name model (docs/modules-spec.md §1/§5): every `.ink`
  file is a module named by its stem, and a file-level `#@module(name)`
  directive declares the module explicitly. `DefinitionId`s are now hashed
  as `(module, name)` for **declared** modules; INCLUDE-glued files inherit
  their includer's module. An undeclared file whose stem collides with a
  declared module's name is a compile error (`E085`), and a malformed
  `#@module` (missing/empty name, or a second declaration) is `E086`.
  `#@module` is brink-dialect-only — under strict-ink it is rejected with
  the standard `E051`-class diagnostic.

  Identity is unchanged for the entire pre-modules world: an undeclared
  stem-module contributes nothing to the hash, so every existing story's
  `DefinitionId`s — and every saved game — stay byte-identical.

## 0.10.1

### Patch Changes

- e2acdbb: CONST declarations now accept a TM-2 inline type annotation
  (#641, docs/typed-mode-spec.md §3: "optional anywhere"): `CONST name: type
= expr`, mirroring the `VAR` annotation surface end to end.

  Superset grammar (`brink-syntax`): `const_declaration` now peeks for
  `at_type_annotation` after the identifier, same discipline as
  `var_declaration` — an unannotated `CONST` produces the exact same CST as
  before this change. HIR (`brink-ir`) gains an `annotation: Option<TypeExpr>`
  field on `ConstDecl`, lowered structurally with no validity checking.

  Analysis (`brink-analyzer`): `dialect_gate` flags a `CONST` annotation as
  `E051` under `strict-ink`, same as every other TM-2 annotation site.
  Annotation _content_ checks (`E061` unknown type name / `E062` reserved
  `fn(...)` type) run through the same `finish_analysis`-gated call as `VAR`
  — brink dialect only (maintainer ruling 2026-07-13), verified rather than
  re-gated. `signature()`'s firewall now resolves a `CONST`'s annotation and
  has it override the literal-inferred `value_type`, same annotation-wins
  rule as `VAR`.

  `brink-fmt` renders the annotation for free through the existing
  single-line declaration renderer — idempotence tests added, no renderer
  change. `brink-ide`'s parse → HIR → analyze → project pipeline doesn't
  crash on annotated or reserved/unknown-type `CONST` sources. Grammar fuzz
  coverage (`proptest_syntax.rs`) extended with a `CONST`-typed strategy
  mirroring the existing `VAR`-typed one.

  Oracle corpus is byte-identical (5,577 passing episodes) — none of it uses
  brink-dialect annotation syntax, and the grammar addition is fully
  optional/additive.

- 6ed8a8d: #585: a nested choice (or labeled gather block/conditional/sequence)
  embedded inside an un-lifted inline conditional in a choice's own
  display/bracket/inner text (e.g. `_ Pick {x > 0: - true: _ nested -> END

  - else: text}`) is now a targeted, Error-severity compile error (`E059`),
replacing a `debug_assert!(false, …)` guard on the arm that handles it.

  This is a real behavior change for real ink, not just defense-in-depth:
  in a release build (including the shipped `@brink-lang/web` wasm), the
  `debug_assert!` was compiled out, so `lower_stmt` silently returned `None`
  and dropped the nested statement — `lower_to_program` still produced
  `Some(program)` with no diagnostic, and `lir_query` treated that as a
  successful compile. The web playground would silently accept this input
  and produce a wrong story with the nested construct missing, with no
  indication anything was lost. With `E059` now Error-severity, `lir_query`
  gates on it (`program: None` once `lir_errors` is non-empty), so this same
  input now fails to compile in the web build instead of silently
  miscompiling. Sibling of #578's analogous `E057`/`E058` hardening
  (`t1b-4-diagnostics-hardening.md`), which shipped the same kind of
  changeset for the same reason.

  #586's codegen backstop (out-of-loop `LogicBreak`/`LogicContinue` in
  `brink-codegen-inkb`) is unaffected: that input is already rejected
  non-suppressibly upstream by LIR lowering's `E057` before a `Program`
  ever reaches codegen, so no valid compile path's observable behavior
  changes — no changeset needed for that half.

  Oracle corpus: unchanged, 5,577 passing episodes.

- eb06ccc: #603: formatting a T1b `~ { … }` multi-line logic block whose CST subtree
  contains a parse error (mid-edit or otherwise malformed input) now bails to
  byte-for-byte verbatim pass-through for that block only, instead of running
  it through `render_logic_block`'s indentation-aware reindenting. #602's
  reindenting assumed a well-formed subtree; on malformed input it could
  corrupt the block (a trailing `//` comment between an `if` condition and its
  `{` swallowed the brace onto the comment line; a multi-line call under a
  parse error injected spurious blank lines and broke idempotence; a lone
  `else`/brace line got mangled into mismatched braces). Well-formed `~ { … }`
  blocks are unaffected and continue to reindent as before; everything outside
  `~ { … }` blocks is untouched.

  This is reachable from the web playground: `brink-web` depends on
  `brink-ide`, which calls `brink_fmt::format` in `code_actions.rs` and
  `formatting.rs`; `brink-web` exposes those as the `code_actions` /
  `code_actions_doc` / `resolve_code_action` wasm-bindgen methods (the
  "Format knot" / "Format stitch" code actions). Running a code action on a
  knot/stitch containing a malformed `~ { … }` block (the normal state of a
  block mid-edit) now leaves it untouched instead of corrupting it.

- 1154eb4: Added a recursion-depth cap (128 levels, `MAX_DECODE_DEPTH`) to the
  `VAL_ARRAY`/`VAL_MAP` decoder in both the `.inkb` reader
  (`brink_format::read_inkb`, reachable from `@brink-lang/web`) and the
  runtime transcript reader (`.brkt`). Previously a crafted file of deeply
  nested single-element arrays (~5 bytes/level) could recurse unboundedly and
  stack-overflow the reader (#553). Nesting beyond the cap now returns a
  proper decode error (`DecodeError::MaxDepthExceeded` /
  `TranscriptError::MaxDepthExceeded`) instead of crashing the wasm module.
  Valid data — including hand-built collections nested exactly at the cap —
  decodes byte-identical to before; the oracle corpus is unaffected (still
  5,577 passing episodes).
- 0f6ae50: Wired `completions()`/`completions_doc()`, `signature_help()`/
  `signature_help_doc()`, and `folding_ranges()`/`folding_ranges_doc()` up to
  the #589 IDE entry points (#600), which had landed in `brink-ide` and
  `brink-lsp` but were never called from the wasm bridge:

  - Completion now offers the T1b stdlib slice 1 functions (`len`/`keys`/
    `values`/`contains`/`push`/`insert`/`remove`, docs/t1b-surface-spec.md §5)
    as `kind: "stdlib"` items, once the new `set_language_dialect("brink")`
    session method is called (defaults to `"strict-ink"`, matching
    `AnalysisOptions::default()` — stdlib names are never offered until a host
    opts in, mirroring `brink-lsp`'s `initializationOptions.dialect`).
  - Signature help now calls `signature_help_with_dialect`, so a call to one of
    those same names shows its signature (mutators render their first
    parameter as `name: lvalue`, e.g. `push(a: lvalue, v)`) once brink dialect
    is set — falling back to `None` under the default, exactly like completion.
  - Folding now includes `~ { … }` logic-block folds (and their nested
    `if`/`while`/`for` sub-folds) as `kind: "structural"` ranges, unconditionally
    — no dialect gate, since the construct parses and lowers identically in
    both dialects (only strict-ink flags it as a diagnostic, `E051`).

  New host-facing API: `EditorSession.set_language_dialect(value: "brink" |
"strict-ink")`. No other wasm-observable behavior changed.

- e96d2a1: Fixed permanent spurious `E051` ("brink extension") diagnostics in the
  playground for `brink`-dialect projects (#611, the wasm-side twin of #599).

  `IdeSession::analyze`/`reanalyze`/`analyze_overlay`/`analyze_projection`
  (`brink-ide`) all built `AnalysisOptions` with `..AnalysisOptions::default()`
  for the `dialect` field, ignoring the session's declared T1b compiler
  dialect entirely — `EditorSession.set_language_dialect` (#589/#600) only
  gated stdlib completion and signature help, never the background analysis
  pass that produces diagnostics. A project opened with brink-dialect syntax
  (`~ { … }` blocks, `#[…]`/`#{…}` sigil literals, postfix indexing) kept
  showing `E051` on every valid construct no matter what dialect was set.

  `set_language_dialect` now forwards into `IdeSession`, which threads the
  declared dialect through all four analysis entry points and re-analyzes
  immediately (like `set_external_check`/`set_semantic_type_check`). No other
  wasm-observable behavior changed.

- bd69ac6: New pure conversion intrinsics `int(x)`, `float(x)`, `string(x)` under the
  brink dialect (maintainer-ruled domains: permissive numerics + bool;
  parse failure is a turn-terminating fault; float→int truncates toward
  zero matching `INT()`). New compileable surface reachable through the
  wasm compile entry points; out-of-domain arguments are `E078` compile
  errors under `types = strict`.
- f40c345: Wired `types = strict` (TM-3, #619) reachability through `brink-ide`,
  `brink-lsp`, and `brink-web` (#660) — PR #656 landed the strict-mode checks
  themselves but left `IdeSession`'s two `AnalysisOptions` literals hardcoded
  to `TypePolicy::Gradual` (no setter), so strict mode was reachable only via
  the compiler CLI's `--types strict`; the IDE/LSP/web surface could not turn
  it on at all.

  - `IdeSession` (`brink-ide`) gains `set_type_policy`/`type_policy`,
    mirroring `set_language_dialect`/`language_dialect` exactly. `snapshot()`
    and `analysis_options()` now thread the registered policy through instead
    of a hardcoded `TypePolicy::default()`.
  - `brink-lsp` reads `initializationOptions.types` (`"strict"` or
    `"gradual"`; defaults to `"gradual"`), mirroring the existing `.dialect`
    handling, and feeds it to both the foreground session and the background
    `analysis_loop`.
  - `EditorSession.set_type_policy(value: "strict" | "gradual")` (`brink-web`)
    mirrors `set_language_dialect` and re-analyzes immediately. `strict`
    requires `set_language_dialect("brink")`, or analysis (and
    `compile_project`) reports a single project-level `E064` config-error
    diagnostic instead of running the normal passes — the caller's
    responsibility, same as the CLI.

  **wasm-observable**: `EditorSession.set_type_policy` is a new host-facing
  entry point, and `compile_project`/background analysis now surface
  `E065`/`E066`/`E067`/error-severity-`E063` diagnostics (or `E064`) for a
  project that opts in — behavior no wasm consumer could previously reach at
  all. No other wasm-observable behavior changed; the default (`Gradual`,
  never calling `set_type_policy`) is byte-identical to before.

- f25362a: Fixed #673: a collection or struct literal used as a `VAR`/`CONST`
  declaration default (`VAR arr = #[1, 2, 3]`, `VAR m = #{"a": 1}`, `VAR p =
Point#{x: 1.0, y: 2.0}`) used to compile silently to `Value::Null` with no
  diagnostic — `brink-ir::lir::lower::decls::eval_const_expr` (the
  compile-time constant-folding path `VAR`/`CONST` defaults go through) had
  no arm for `ArrayLiteral`/`MapLiteral`/`StructLiteral` and fell through to
  its catch-all `_ => ConstValue::Null`.

  - Array/map literal defaults (including nested ones, and constant
    references inside them, e.g. `#[SOME_CONST, 2]`) now constant-fold into
    the real `ConstValue::Array`/`Map` — the same representation
    `brink-codegen-inkb` already materializes into a real `Value::array`/
    `Value::map` global default (this wiring already existed for
    expression-position array/map literals; declaration defaults now share
    it). A map key that isn't a compile-time-constant scalar (int/string/
    bool) in a declaration default is a new compile error (`E076`) — a
    declaration default has no runtime `MapNew` construction step left to
    fault at the way a mid-story map literal does. Likewise, an array
    element or map value whose expression can never constant-fold (a
    function call, indexing, field access, or `++`/`--` — e.g. `VAR arr =
#[f(), 2]`) is a new compile error (`E077`), never a silently-`Null`
    element. The check is keyed off the source expression kind, so
    constant-foldable shapes (`#[1 + 2, -SOME_CONST]`, nested literals)
    are unaffected.
  - A struct construction literal used directly as a declaration default is
    a new compile error (`E075`) — `ConstValue` has no record-carrying
    variant (adding one is a format question outside this fix), and unlike
    arrays/maps there's no existing runtime-construction step for a
    declaration default to defer to. Construct the struct via an ordinary
    assignment after declaration instead (`VAR p = 0` then `~ p =
Point#{...}`).

  `E075`, `E076`, and `E077` are LIR-lowering diagnostics, so — like `E053`/
  `E073`/`E074` — they're never suppressible via `// brink-disable`/
  `// brink-disable-all`.

  Oracle corpus: unchanged, 5,577 passing episodes — vanilla ink has no
  collection/struct sigil literals for this to affect.

- ebce613: T1c-1: `#fn(name, args…)` function-value creation — grammar, HIR, typing,
  and strict call checking (#699, docs/t1c-spec.md §2/§4/§8). Observable
  through editor diagnostics:

  - `#fn(…)` parses in expression position (superset grammar, the
    `#[…]`/`#{…}`/`Name#{…}` sigil family); under `strict-ink` it rejects at
    analysis with the standard E051 "brink extension" diagnostic. Prose
    position is unchanged — `#` still opens a tag.
  - New creation-site diagnostics under `dialect = brink`: E079 (target is
    not a statically-named function definition), E080 (a `ref` param unbound
    at creation, or bound to a non-durable lvalue — temps/params, CONSTs,
    rvalues, and field projections all reject; only VAR cells are durable),
    E081 (more args bound than the target declares).
  - `fn(T…): R` type annotations are now legal (E062 retired — it no longer
    fires) and resolve to a real checker type; unknown names inside a fn type
    still flag E061.
  - Under `types = strict`, calls through function values are statically
    checked via the existing TM-3 codes: Unknown callee → E065, Conflicted
    callee → E066, non-callable/arity/argument-type mismatches → E063 (the
    `int → float` coercion applies to call arguments).
  - Compiling a program that actually uses `#fn` under `dialect = brink`
    still rejects at lowering with a targeted E052 ("not yet implemented" —
    LIR/codegen/VM land in T1c-2). No behavior change for strict-ink
    projects or gradual-mode diagnostics.

- 3c5808f: Added `EditorSessionHandle.setSemanticTypeCheck(level)` (#532), mirroring
  `setExternalCheck`. Previously `WasmEditorSession.set_semantic_type_check`
  was only reachable on the raw wasm session, which `EditorSessionHandle`
  holds in a private field — the `@brink-lang/web` public wrapper had no
  method to reach it, so the severity lever was dead code for consumers of
  the package. `setSemanticTypeCheck("tolerant" | "error")` now delegates to
  the wasm session and bumps the generation counter, same as every other
  mutating call on the handle.
- eaff136: Added the T1b superset grammar (#569, docs/t1b-surface-spec.md §§1-4):
  multi-line `~ { … }` logic blocks (assignment, `temp`, `if`/`else if`/`else`,
  `while`, `for … in …`, `break`/`continue`, `return`, expression statements),
  `#[…]`/`#{…}` sigil collection literals in expression position, and postfix
  indexing (`a[0]`, chained `grid[y][x]`) plus indexed assignment. Parsed
  through CST/AST/HIR; nothing lowers to LIR or codegen yet (lands in T1b-2).

  Introduced a compiler dialect gate (`AnalysisOptions::dialect`,
  `Dialect::{StrictInk, Brink}`, default `StrictInk`) — a new analysis input,
  not embedded in `.inkb`. Under `StrictInk` (the default every existing
  caller gets), every extension construct now produces a targeted diagnostic
  (`E051`) at its exact span instead of whatever parse/analysis error it
  previously produced for that byte sequence. Under `Brink`, the same
  constructs produce a "not yet implemented — lands in T1b-2" diagnostic
  (`E052`). Both dialects still fail to compile source using this syntax —
  this is a diagnostic-quality change for a previously-unsupported syntax
  shape, not new compileable output.

  Plain ink is unaffected: the oracle corpus remains byte-identical (5,577
  passing episodes) since none of it uses the new syntax, and the new grammar
  is purely additive (`if`/`while`/`for`/`break`/`continue`/`in` are
  contextual keywords, recognized only at block-statement-start position
  inside a new `~ { … }` block — they remain ordinary identifiers everywhere
  else, so no existing knot/variable/function name is reserved).

  The dialect gate's diagnostics are analysis diagnostics, so they can be
  suppressed like any other (`// brink-disable-all` or a line directive). LIR
  lowering now defends against that case directly: if a suppressed gate lets
  extension syntax reach LIR lowering, compilation still fails with a new
  internal-error diagnostic (`E053`) instead of silently dropping the
  construct or corrupting the compiled output.

- ba69a35: T1b-2 (#570): the brink dialect now compiles and runs the full T1b surface
  (docs/t1b-surface-spec.md §§2-4) — `~ { … }` logic blocks (`if`/`else if`/
  `else`, `while`, `for x in arr`/`for k in map`, `break`/`continue`, `return`,
  block-scoped `temp`), `#[…]`/`#{…}` sigil collection literals (constant
  literals go through a new V4 literal pool, `PushLiteral(idx)`; dynamic
  literals through new `ArrayNew`/`MapNew` opcodes), and postfix indexing
  (`a[0]`, chained `grid[y][x]`) including indexed assignment via the ratified
  RMW discipline (take → `make_mut` → write-back on the root cell; chains
  lower to nested RMW through synthetic temps, never interior references).
  Out-of-bounds array indices and missing map keys are turn-terminating
  runtime faults on both read and write — no silent growth on write-past-end.

  The `Brink` dialect no longer rejects any of this ("not yet implemented —
  lands in T1b-2", `E052`) — it just compiles. `StrictInk` is unaffected
  (`E051` still rejects every extension construct at its exact span).

  Block-scoped `temp` declarations (including `for` loop variables) thread
  into the same symbol manifest the IDE's cross-ref/rename/unused-variable
  tooling reads, and get a new warning (`E054`) when they shadow an
  already-visible temp (an outer classic `~ temp` or an enclosing block).

  Format: a new `LiteralPool` `.inkb`/`.inkt` section (additive alongside the
  existing `ListLiterals` section — `PushList` is unaffected) and twelve new
  opcodes in the previously-reserved `0xBE`-`0xC9` block (`ArrayNew`, `MapNew`,
  `IndexGet`, `IndexSet`, `CollectionLen`, `MapGet`, `MapInsert`, `MapRemove`,
  `MapContains`, `CollectionKeys`, `CollectionValues`, `PushLiteral`) — inert
  until this compiler surface emits them, so no existing `.inkb` output
  changes shape unless it uses T1b syntax. Also fixes a pre-existing gap found
  while adding this: the `.inkt` text format's `value`/`type_name` grammar
  could not parse an `Array`/`Map` value back (only write one) — global
  variable defaults with a collection default (possible since #525) now
  round-trip correctly too.

  Oracle corpus: unchanged, 5,577 passing episodes — the strict-ink corpus
  never reaches any of this new surface by construction.

- 124bb9e: T1b-3 (#571): the brink dialect ships stdlib slice 1
  (docs/t1b-surface-spec.md §5) — lowercase free functions, brink-dialect-gated
  (`strict-ink` never sees them). Pure: `len(x)`, `keys(m)`, `values(m)`,
  `contains(x, v)` (arrays: element containment; maps: key containment).
  Mutating: `push(a, v)`, `insert(x, k_or_i, v)`, `remove(x, k_or_i)` — all
  three require an lvalue first argument (a variable, temp, or indexed path)
  and lower through the same take → `make_mut` → write-back RMW discipline
  indexed assignment uses (§4); passing an rvalue (`push(#[1, 2], 3)`) is now
  a targeted compile error (`E055`), and using a mutator's result — they
  return nothing — is a compile error too (`E056`).

  An author-defined function of the same name shadows the builtin, with a
  warning (`E035`, reusing the existing "name shadows a built-in function"
  code); imported vanilla ink that defines e.g. `len` keeps working under the
  brink dialect. Under `strict-ink`, an unresolved call to any of the seven
  names is now rejected the same way other brink-extension syntax is (`E051`).

  VM-native: the array-generalized `MapInsert`/`MapRemove`/`MapContains`
  opcodes (reserved+live since #575, now compiler-emitted) are the mutators'
  primitives — despite the `Map*` names, they now also handle `Array`
  containers (index-based insert-with-shift/remove-with-shift/element-scan),
  since the frozen v4 collection-opcode block has no dedicated array-append
  opcode and the RFC's one-bump rule reserved exactly this set. `push(a, v)`
  desugars to `insert(a, len(a), v)`. No wire-format change — same opcode
  bytes, generalized VM-side semantics.

  Also fixes a latent gap this surface exposed: diagnostics produced during
  LIR lowering (as opposed to the earlier analysis phase) were always treated
  as warnings regardless of their own severity, so an Error-severity one could
  never actually block compilation. `E055`/`E056` are the first Error-severity
  diagnostics LIR lowering ever produces; the pipeline now partitions
  lowering-phase diagnostics by severity like every other diagnostic source,
  so they correctly fail the compile instead of silently compiling anyway.

  Oracle corpus: unchanged, 5,577 passing episodes — the strict-ink corpus
  never reaches any of this new surface by construction.

- 75b8a3b: T1b-4 diagnostics/semantics hardening (#577, #578, #580, #581, #568):

  - **#577**: `break`/`continue` used outside any enclosing `while`/`for`
    loop is now a targeted, Error-severity compile error (`E057`). Previously
    it lowered unconditionally to an unguarded jump, and codegen silently
    degraded that to a no-op (`Opcode::Nop`) instead of ever surfacing an
    error — the compiler would accept clearly-wrong ink and produce dead
    bytecode for it. The check runs at LIR-lowering time (the same layer as
    the T1b-3 mutator checks, `E055`/`E056`), so it is a real, non-suppressible
    compile error, not a suppressible analysis diagnostic.
  - **#578**: an inline multiline conditional/sequence that keeps its
    `InlineConditional`/`InlineSequence` shape all the way to LIR lowering
    (rather than being lifted to a top-level statement by HIR normalization —
    reachable via a choice's own display/bracket/inner text, which
    normalization never touches, or via a second inline construct on one
    content line) could contain a `~ { … }` T1b logic block. Lowering that
    case hit an internal `debug_assert!`-guarded "unreachable" arm — a panic
    in debug builds, a silent statement drop in release. It now routes
    through the same real lowering path top-level blocks use.
  - **#580** (RULED): `contains(map, needle)` with a `needle` outside the
    map-key domain (a float, array, map, …) now returns `false` instead of
    faulting — total on both the array and map branches, matching the array
    branch's existing behavior. Indexing/mutation faults on a bad key are
    unchanged (value-model-spec §6); `contains` never had a "the key isn't
    there" failure mode to escalate to a fault the way those do.
  - **#581** (RULED): a collection mutator (`push`/`insert`/`remove`) called
    with the wrong argument count is now a targeted, Error-severity compile
    error (`E058`) naming the expected signature (e.g.
    `push(container, value)`), replacing the generic `E031` warning the arity
    check used to share with ordinary function-call arity checking. E031
    never blocked compilation, so a malformed mutator call used to silently
    vanish from the lowered bytecode with no compile failure. Pure-function
    arity checking is unchanged.
  - **#568**: a debug-build `console.warn` diagnostic for the third lossy-leg
    failure mode at the `value_to_js` wasm boundary (alongside the existing
    key-coercion-collision (#555) and key-reordering (#564) diagnostics): a
    `Value::Float` map value whose lossless `f32` → `f64` widening would print
    with more digits in a real JS engine than the value's own shortest
    decimal (e.g. `0.1f32` widens to the `f64` whose shortest round-trip
    decimal is `0.10000000149011612`). No value precision is actually lost —
    the widening is exact — but the extra digits are a genuine
    "where-did-these-come-from" surprise. Diagnostic-only; `value_to_js`'s
    marshaling is unchanged.

  Oracle corpus: unchanged, 5,577 passing episodes.

- 350b663: Added hover text for the T1b stdlib slice 1 functions (`len`/`keys`/
  `values`/`contains`/`push`/`insert`/`remove`, docs/t1b-surface-spec.md §5,
  #589): hovering one of these names now shows its signature (mutators render
  their first parameter as `name: lvalue`, e.g. `push(a: lvalue, v)`) and a
  one-line semantics summary, unconditionally — like the existing built-in
  (`INT`/`FLOOR`/…) hover text — so the info is available even in a strict-ink
  project, where a use of the name is otherwise flagged as a brink extension.
  No other wasm-exposed behavior changed: the new dialect-gated stdlib
  completion, signature help, and `~ { … }` block-folding queries land in
  `brink-ide` and are wired into `brink-lsp` only in this PR — the
  `@brink-lang/web` bridge (`brink-web`) does not yet call them.
- 9e1257d: T1b-4 (#576): closes the indexed-write COW cliff value-model-spec §5
  promises but PR #575 hadn't yet delivered — `blocks.rs`'s RMW lowering read
  the root/intermediate cells it mutates via `GetGlobal`/`GetTemp`, which
  `Arc`-clone the slot instead of consuming it, so `array_make_mut`/
  `map_make_mut` always saw a shared `Arc` and COW-copied on every write —
  O(n) per write, O(n²) for a loop of indexed writes or `push`es.

  Two new opcodes in the previously-reserved sharing-discipline block
  (`docs/format-v4-rfc.md` §3): `TakeGlobal(DefinitionId)` at `0xCA` and
  `TakeTemp(u16)` at `0xCD` (freshly claimed, adjacent to the reservation —
  `0xCB`/`0xCC` stay reserved for `StoreVarIfNew`/`EqVars`). Both move a
  slot's current value out, leaving `Value::Null` behind, instead of cloning;
  `TakeTemp` auto-dereferences like `GetTemp` (a `ref` parameter's pointed-to
  location is taken, the pointer itself untouched).

  The compiler now emits them for the **flat** RMW shape — `a[i] = v`/
  `a[i] op= v` and `push`/`insert`/`remove` on a bare variable, the exact
  loop-append case the spec's "one cliff" targets — with every other
  sub-expression (index, value, and for indexed assignment the pre-mutation
  `current` read) evaluated _before_ the take, so an expression referencing
  the same variable by name still sees its pre-mutation value. **Chained**
  indexed assignment/mutators (`grid[y][x] = v`, `push(grid[y], v)`) are
  unchanged: a nested element is still referenced from inside its parent
  until the write-back cascade completes, so a take at any level but the
  root buys nothing there — the sanctioned §7 clone-based fallback stays in
  place for that shape.

  **Fault-during-RMW slot state** (a new, deliberately-defined behavior): for
  indexed assignment and `push`, a fault (out-of-bounds index, missing map
  key, non-collection root) is now caught by a non-mutating pre-check
  _before_ anything is taken, so the root is **never** lost to a fault on
  these paths — identical to the pre-#576 behavior. `insert`/`remove` at an
  arbitrary author-supplied key don't get an equivalent free pre-check; a
  fault there leaves the taken root holding `Value::Null` — a documented,
  tested trade-off consistent with this VM's pre-existing no-rollback-on-
  fault model (a fault anywhere mid-turn already leaves earlier same-turn
  mutations applied).

  Benchmark (`crates/brink-runtime/benches/runtime.rs`, `loop_append_bench`):
  10k sequential `push`es on a freshly-created array — measured 464.8ms
  median before this change, 13.91ms median after (~33x), consistent with
  closing an O(n²) cliff.

  Oracle corpus: unchanged, 5,577 passing episodes — T1b syntax never reaches
  the strict-ink corpus by construction.

- 9213d77: Formatting a T1b `~ { … }` multi-line logic block (docs/t1b-surface-spec.md
  §2) now reindents its internals instead of passing them through verbatim
  (#573): 4-space indent per nesting level inside the block, opening brace on
  its statement's line, closing brace on its own line at the parent depth,
  one statement per line, comments and blank lines preserved in place
  (blank-line runs collapse to one, trailing comments stay attached). This
  supersedes T1b-1's verbatim pass-through contract. Everything outside `~ {
… }` blocks is untouched.

  This is reachable from the web playground: `brink-web` depends on
  `brink-ide`, which calls `brink_fmt::format` in `code_actions.rs` and
  `formatting.rs`; `brink-web` exposes those as the `code_actions` /
  `code_actions_doc` / `resolve_code_action` wasm-bindgen methods (the
  "Format knot" / "Format stitch" code actions). A knot or stitch containing a
  `~ { … }` block now formats differently through the playground.

- f835cfd: Format VERSION 4 (T1a-4, #526): the single planned Tier-1 format bump. The
  `.inkb` binary and the runtime transcript (`.brkt`) now serialize
  `Value::Array`/`Value::Map` as tree encodings (a length prefix then
  recursively-encoded children, insertion order preserved, map keys restricted to
  the scalar `int`/`string`/`bool` domain) instead of folding a collection to
  `null`. A binding/external that returns a collection and is inline-emitted
  (`{ext()}`) now round-trips through the persisted transcript byte-for-value
  identical. The strict reader hard-rejects any version but 4. The reserved
  Tier-1 value-tag/section/opcode surface (function values, closures, handles,
  projections, records) is frozen at v4 per the §9 one-bump rule so later
  milestones add data without another bump. Compiled `.inkb`/transcript bytes read
  through `@brink-lang/web` change accordingly; no opcode emits collections yet,
  so scalar behavior and the oracle are byte-identical.
- b8392a2: Tier-1 value model, state plumbing (T1a-3, #525): collection values
  (`Array`/`Map`) now cross the wasm JSON boundary as structured trees instead of
  folding to `null`. `eval_function`/`resume_function_eval` results serialize an
  array as `{ "type": "array", "items": [...] }` and a map as
  `{ "type": "map", "entries": [{ "key": ..., "value": ... }] }`, preserving
  insertion order and each key's scalar type (int/string/bool). A JS binding that
  returns a native array or plain object is now read back as an ink `Array`/`Map`
  (recursively; object keys become string map keys in JS property order), and an
  ink collection passed to a binding marshals to a native JS array/object.
  Snapshot-only per value-model spec §8 — the boundary copies trees; the host
  never retains a handle into script state. No opcode emits collections yet, so
  scalar behavior and the oracle are byte-identical.
- 6089ed6: Added TM-2 inline type annotation syntax (#618, docs/typed-mode-spec.md
  §3): `name: type` after knot/stitch params and `VAR`/`temp` declarations,
  `): type ===` in the function-header return position, and the `~ temp
name: type = expr` ascription form. Type names are lowercase nominals —
  `int`, `float`, `bool`, `string`, `divert`, `void`, `list<L>` (nominal per
  a declared `LIST`), `array<T>`, `map<K, V>`; `fn(T…): R` function types
  parse but are reserved until T1c (a targeted diagnostic, `E062`, fires on
  any use). An unrecognized type name gets a targeted diagnostic (`E061`).

  Superset grammar, same dialect-gate pattern as T1b: `brink-syntax` always
  parses annotations regardless of dialect; under `strict-ink` every
  annotation is a brink-extension diagnostic (`E051`) at its span, same as
  every other T1b extension construct. `E061`/`E062` are unconditional in
  both dialects (they check annotation _content_, independent of whether the
  syntax itself is allowed).

  Annotations feed `signature()`'s firewall: an annotated knot/stitch param
  or knot return type is exposed on `Sig` (`param_annotations`,
  `return_annotation`); an annotated `VAR` overrides its literal-inferred
  `value_type` (annotation wins over inference) — the existing
  `infer::collect_globals` seam picks this up with no further change. A new
  `annotation_mismatches` function compares an annotation against TM-1 body
  inference and reports a disagreement (`E063`, advisory/warning severity —
  strict-mode policy is TM-3's call). `~ temp` ascriptions parse and lower to
  HIR but aren't yet wired into body inference (that would touch
  `infer::body::BodyCtx`, out of scope per #638).

  `brink-fmt` renders annotations for free through its existing single-line
  token-collapsing passes (knot headers, declarations, logic lines) — no
  renderer changes were needed, only idempotence tests. `brink-ide`'s
  parse → HIR → analyze → project pipeline doesn't crash on annotated or
  reserved/unknown-type sources. Grammar fuzz coverage (`proptest_syntax.rs`)
  extended with a depth-bounded type-expression strategy so the superset
  parser never panics on type-annotated input.

  Oracle corpus is byte-identical (5,577 passing episodes) — none of it uses
  brink-dialect annotation syntax, and the grammar addition is fully
  optional/additive at every position it touches.

- 1c389ec: TM-4 (#620) foundation: `Value::Record` lands in the shared value core —
  closed-shape records with an interned `ShapeId` and a flat, shape-ordered
  field vector, following the exact COW/equality/serialization machinery
  already ratified for `Array`/`Map` (Arc-shared field vector, `make_mut`
  copy-on-write, structural equality with an `Arc::ptr_eq` fast path, plus a
  shape-identity check — two records are never equal unless their shapes
  match). Round-trips through `.inkb`, the `.inkt` text format, and the
  runtime transcript (`.brkt`), all via the new `VAL_RECORD` (`0x0F`) wire tag
  (`docs/format-v4-rfc.md` §1).

  Format: the reserved `StructShapes` `.inkb` section (`0x0C`) goes live —
  shape id, name, and ordered field names, wired into `write_inkb`/`read_inkb`
  alongside the existing sections (`SECTION_COUNT` 11 → 12; every checked-in
  `.inkb` fixture regenerates once with the extra section, per the
  single-version regenerate-on-mismatch policy). Three new opcodes go live in
  the previously-reserved field-op block (`0xCE`-`0xD0`): `RecordNew(shape_id)`,
  `RecordGetDyn(name_id)`, `RecordSetDyn(name_id)` — the by-name field
  construct/get/set ops correct in both dialects (turn-terminating fault on a
  missing field, matching the existing `MapGet`/`IndexGet` fault pattern).
  Static-offset field ops (`RecordGet(offset)`/`RecordSet(offset)`, the
  strict-mode performance payoff `docs/typed-mode-spec.md` §6 anticipates)
  stay named and numbered (`0xD1`-`0xD2`) but reserved — no `Opcode` variant
  yet, the same "reserved, decode rejects" discipline `StoreVarIfNew`/`EqVars`
  already established.

  No compiler surface (grammar/HIR/analyzer/LIR/codegen for `STRUCT`
  declarations or `Name#{…}` construction) is included in this PR — every new
  opcode/section is inert until a follow-up compiler milestone emits it,
  mirroring how T1a's collection-value reservation preceded T1b's live
  grammar/codegen wiring. See the PR description's scope note for what
  remains open against issue #620.

  Oracle corpus: unchanged, 5,577 passing episodes — nothing in the compiler
  pipeline changes; the new surface is reachable only through direct
  hand-assembled bytecode (this PR's own VM tests) and the `.inkb`/`.inkt`/
  transcript round-trip tests.

- 81f0055: TM-4b (#665): the struct compiler surface lands — grammar, HIR, and
  analyzer, diagnostics-only (codegen lands with TM-4c, #666), per
  `docs/typed-mode-spec.md` §6.

  - **Grammar** (brink-syntax): `STRUCT Name = #{ field: type, … }`
    declarations (single-line or multi-line — the body mirrors the
    construction literal's shape); `Name#{field: expr, …}` construction
    literals in expression position; postfix `.field` access wherever the
    existing dotted-`PATH` grammar doesn't already cover it (a bare
    `ident.ident` chain still parses as one `PATH`, unchanged). All brink
    extension syntax — superset grammar, byte-identical CST for every
    non-struct program.
  - **HIR** (brink-ir): `StructDecl`/`StructFieldDecl` items, `Expr::StructLiteral`,
    `Expr::FieldAccess`, `SymbolKind::Struct` manifest registration.
  - **Analyzer** (brink-analyzer): resolution fallback for field access
    (static dotted paths like `knot.stitch`/`List.Item` resolve first and
    win; only a head resolving to a variable/temp/param makes `.field` a
    field access); dialect gate flags every new construct under strict-ink
    (`E051`); `Ty::Struct` nominal joins the TM-2 annotation grammar
    (declared struct names no longer trip `E061`); strict-mode-only
    construction checks naming the offending field — missing (`E069`), extra
    (`E070`), mistyped (`E071`); unresolved shape names (`E068`).
  - **LIR**: struct constructs (construction literals, field access — both
    the new grammar and the ambiguous-path resolution-fallback case) reject
    with a real, non-suppressible `E072` diagnostic — the T1b-1 discipline
    (grammar/HIR/analyzer land before codegen) plus the E053-backstop lesson
    (a real diagnostic, not a `debug_assert!`-guarded silent drop).

  Wasm-observable surface: the parser accepts the new grammar (new
  `SyntaxKind`s reach `brink-ide`/`brink-web`'s CST-derived tooling); five new
  diagnostic codes (`E068`-`E072`) can now be produced and surfaced through
  `brink-web`'s diagnostics API; `editor_dto::symbol_kind_str` gains a
  `"struct"` arm (was previously unreachable — a new `SymbolKind` variant);
  the semantic-tokens legend gains a 13th token type, `"struct"` (existing
  indices unchanged, purely additive).

  Oracle corpus: unchanged, 5,577 passing episodes — no existing program uses
  `STRUCT`/`Name#{…}`/the new field-access grammar, and LIR lowering rejects
  every struct construct rather than emitting bytecode.

- 6e007d3: TM-4c (#666): structs become executable — LIR lowering + codegen for
  construction, field reads, and single-level field writes, per
  `docs/typed-mode-spec.md` §6.

  - **LIR** (brink-ir): `Expr::StructLiteral` lowers to `RecordNew(shape_id)`
    with initializers reordered into shape declaration order (each evaluated
    exactly once; see `lower_struct_literal`'s doc for the evaluation-order
    caveat when the author's field order differs from the shape's own).
    `Expr::FieldAccess` (and the ambiguous multi-segment-`Path` shape a bare
    `p.x` parses as) lowers to a `RecordGet` read, chaining through nested
    struct-typed fields. `p.field = expr`/`p.field op= expr` lowers through
    the ratified take → `make_mut` → write-back RMW discipline, mirroring
    `lower_indexed_assignment`'s single-level (`n == 1`) fast path — a
    **chained** write (`p.a.b = v`) or a **mixed** chain (`arr[i].field = v`)
    is a real, non-suppressible `E074` diagnostic (T1e boundary), never a
    silent miscompile. `E072` (the old "reject every struct construct"
    backstop) is retired; `E073` is its narrower replacement (a construction
    literal naming an unresolved shape reaching LIR).
  - **Codegen** (brink-codegen-inkb): emits the `StructShapes` table (shape
    ids interned deterministically, declaration order — never `HashMap`
    iteration); field ops default to the by-name `RecordGetDyn`/`RecordSetDyn`
    forms. Under `types = strict`, when a field access's record shape is
    provably known at compile time (a `VAR`/`temp` carrying a TM-2
    struct-typed annotation, or a direct construction-literal chain — never
    general type inference, which `brink-ir` cannot depend on), it emits the
    static-offset `RecordGet`/`RecordSet` forms instead. `types = gradual`
    never emits the offset forms, even with an annotation present (the
    annotation is unenforced there, so trusting it would be unsound).
  - **Runtime** (brink-runtime/brink-format): materializes the reserved
    `RecordGet`/`RecordSet` (`0xD1`/`0xD2`) opcodes — flat bounds-checked
    offset into the record's own field vector, no shape re-check (that's the
    performance payoff over the by-name forms); out-of-range is a
    turn-terminating `RuntimeError::RecordFieldOffsetOutOfRange`, never
    UB/panic. Same COW (take → `make_mut` → write-back) discipline as
    `RecordSetDyn`.
  - **Gradual construction faults** (value-model-spec §11c): a construction
    literal missing a declared field or supplying an undeclared one compiles
    under `types = gradual` (strict already rejects it at `E069`/`E070`) to a
    deterministic runtime fault via a reserved sentinel `ShapeId`
    (`RuntimeError::InvalidShapeId`) — no new opcode needed.

  Wasm-observable surface: `Opcode::RecordGet`/`RecordSet` are real variants
  now (disassembler text in `brink-web::program_model` and the `.inkt`
  writer both cover them); struct declarations, construction literals, field
  reads, and single-level field writes all compile to real bytecode reachable
  through `@brink-lang/web`'s compile/run surface for the first time.

  Oracle corpus: unchanged, 5,577 passing episodes — no existing program uses
  `STRUCT`/`Name#{…}`/field-access grammar.

- 0308aec: TM-5 (#621, docs/typed-mode-spec.md §9 step 5): hover and inlay hints now
  surface _inferred_ types, not just declared/annotated ones, through the
  FG-narrowed per-def seam (`ProjectDb::inferred_signature`/`infer_body` —
  never the whole-project `type_inference()`).

  Hover: a `temp` or parameter with no annotation now shows its inferred
  type (`` `x: int` ``) instead of nothing; an unannotated knot/stitch
  header falls back to its inferred signature for any param/return position
  a TM-2 inline annotation or doc-tag doesn't already cover. A declared
  annotation (TM-2 `name: type`, or a `///` doc-tag/host-manifest type for
  externals) always wins over inference — the firewall rule — and an
  `Unknown`/unresolvable inferred type shows nothing rather than noise.

  Inlay hints: a new `InferredType` kind renders an inferred-type ghost
  label (`: int`) right after an unannotated `~ temp name = …` declaration;
  an explicit `: type` ascription suppresses it (already visible in the
  source). Exposed through `@brink-lang/web`'s `inlay_hints`/`hover` JSON as
  `"inferred_type"` and the existing hover content string respectively; the
  LSP maps it to the standard `TYPE` inlay-hint kind (previously every hint
  defaulted to `PARAMETER`).

  `brink_ide::hover::hover` and `brink_ide::inlay_hints::inlay_hints` both
  gained a `&ProjectDb` (plus `FileId` for `inlay_hints`) parameter to reach
  the per-def queries — an internal API change to `brink-ide`, `brink-lsp`,
  and `brink-web`'s wasm bridge, not a `.inkb`/runtime change. Boundary-
  annotation quick-fix is explicitly out of scope (#657, parked).

## 0.10.0

### Minor Changes

- 73e2746: Line-classification fixes (#478) — deliberate behavior changes to the
  `line_contexts` contract and `LineInfo`:

  - A choice line with an inline divert (`* [Go] -> hub`) now classifies as
    `choice` (was `divert`), so Tab/Enter smart-editing transitions work on
    it again.
  - Every gather-label line — continuation labels, `LabeledBlock` labels,
    top-level labeled gathers — uniformly classifies as `gather` with
    `gather_continuation` weave at its sigil depth. Previously a labeled
    block with an inline divert showed `divert` while the visually identical
    continuation form showed `gather`.
  - Choices inside conditional/sequence branches report their sigil depth
    (was 0), so depth-dependent transitions and gutter depth markers work
    inside arms.
  - Blank lines inside a choice body inherit the body weave (element stays
    `blank`); the editor maps them to `ChoiceBody` so Tab works anywhere in
    the body — replacing the old single-shape TS post-pass, and covering
    deeper blank runs it missed.

- 36bf266: Machinery/narrative fold runs are now opt-in (#479). `foldingRanges` /
  `folding_ranges_doc` return structural folds only unless the host enables
  run computation via the new session-level `setFoldRunsEnabled(true)`
  (mirrors `setDialect`; also on `DocumentHandle`), and the editor's default
  active fold kinds are `structural` only — hosts implementing prose/logic
  view modes activate `machinery`/`narrative` with `setActiveFoldKinds` and
  collapse with `foldAllOfKind`. Runs are additionally bounded by weave
  containers (choice branches / gather continuations), so a run fold never
  crosses weave structure; conditional scaffold + arms still fold as one
  pure-routing region, and inline `{a|b}` alternatives don't fragment
  narrative runs.
- 973858f: Add the HIR structural projection to the editor session (#454 phase 2):
  `getHirSpansDoc(doc)` returns nested semantic spans (kind, depth, resolved
  `def_id`/`target_id` identity) plus a per-line container stack for rails, via
  the new wasm `hir_spans_doc` export. New `HirSpan` / `HirLineContainer` /
  `HirProjection` types.
- 54c37df: Extend the HIR projection's coverage (#463): new span kinds
  `divert_stmt` (whole divert/tunnel/thread statements, distinct from the
  `divert` target reference inside them; suppressed for statements inside
  inline logic in choice text), `divert_terminal` (`-> END` / `-> DONE` — no
  longer unprojected, and never flagged unresolved), `logic` (assignments
  and returns), and `conditional` / `sequence` (whole-construct extents,
  non-container). Container extents now include gather/labeled-block labels,
  so labeled gather lines (`- (g)`, `- (g) text`, nested labeled blocks) are
  covered by their containers and render their rails. Multi-line
  non-container spans that straddle a fragment view's start are dropped from
  `getHirSpansDoc` instead of being clamped to the view's top-left.
- 1bca37c: LineInfo on one shared projection (#480). The HIR projection is now
  computed once per edit and cached on the session — `getLineContextsDoc`,
  `getFoldingRangesDoc`, and `getHirSpansDoc` all share it instead of each
  re-projecting. `LineContext` gains two additive fields the editor now
  consumes instead of deriving: `option_path` (option identity from real HIR
  nesting — the TS weave re-walk only serves the pre-wasm regex fallback)
  and `standalone` (structural divert-vs-tunnel/thread fact — no more text
  sniffing in the editor or fold-run natures). Span kinds `tunnel_stmt` and
  `thread_stmt` split out of `divert_stmt`, which now means a simple
  `-> target` statement only.
  Also fixes `has_tags`: it is now true for **any** line carrying an
  author-written tag — tagged choice lines (`* Choice # tag`), tags inside
  inline conditional/sequence branches, and standalone `#` lines — where the
  legacy walk under-reported (decision 2026-07-10; verified against the C#
  reference, whose runtime surfaces choice-line tags).
- 6289b0e: Weave structure is now foldable (#476): choice branches fold from their
  choice line (full-branch extent) and gather continuations fold from their
  gather line, derived from the HIR projection's container extents. Choice
  folds were previously dead code (single-line CST ranges), so story weave
  never folded at all. Conditional/sequence folds are unchanged. Known
  limitation: an unlabeled gather whose own line is prose gets no fold yet
  (ptr-less line content; upstream lowering gap).

## 0.9.0

### Minor Changes

- 5075db7: Add the speculative-evaluation web binding (F4.3, part of #439): a sandboxed,
  side-effect-proof fork of a running story that never mutates it, driven by
  its own composable verbs.

  `StoryRunnerHandle.speculate(options?)` forks a `SpeculationHandle` exposing
  `goToPath`/`advance`/`advanceAsync`/`choose`/`evalFunction`/
  `evalFunctionAsync`/`resumeFunctionEval`/`resumeFunctionEvalAsync`/
  `resolveExternal`/`takePendingPromise`/`pendingExternalName`/`transcript`/
  `externalsReport` — the composable primary surface. Externals are gated by a
  caller-supplied `name -> "query" | "effect"` policy map plus a `"watch" |
"eval"` context (mirrors `brink_runtime::KindTieredHandler`): query externals
  always run live; effect externals only run live under `context: "eval"` with
  `liveEffects: true` armed, and otherwise fall back to the ink fallback body.
  An async (`Promise`-returning) bound external is awaited transparently by the
  `*Async` verbs, exactly like `StoryRunnerHandle.continueStoryAsync`.

  `StoryRunnerHandle.evaluate(source, opts)` is a thin convenience over those
  verbs for the common cases: a knot/stitch path (`"cellar.intro"`) is driven to
  its next natural stop (a `done`/`end` line, or a `choices` line reported via
  `reachedChoices` rather than picked); a function call with literal arguments
  (`"check(1, 2)"`) is evaluated via `evalFunction`. Anything else (an arbitrary
  expression, a non-literal argument) reports a diagnostic rather than running —
  that's the Tier-1/F5 boundary (`docs/speculative-eval-spec.md`). `opts.signal`
  (an `AbortSignal`) cancels an in-flight evaluation, dropping the speculation
  and rejecting with an `AbortError`.

  Function-evaluation results marshal through a new richer `TypedValue`
  (`int`/`float`/`bool`/`string`/`null`/`list`/`divert`) instead of the
  scalar-only `ExternalValue` the external-binding boundary uses — a `list`
  carries its resolved member names/ordinals and a `divert` its resolved
  knot/stitch destination, rather than collapsing to `null`.

  Also renamed `docs/scratch-eval-spec.md` to `docs/speculative-eval-spec.md`
  and threaded the speculative/`Speculation`/`speculate` naming through it and
  its cross-reference in `docs/scoped-flow-state-spec.md` — it is now framed as
  that plan's Tier-1 (arbitrary-expression) follow-on to the Tier-0 fork-based
  `Speculation` this release ships.

  The oracle corpus is unaffected — this is purely additive to the runtime and
  web binding.

- cbc27aa: Add Tier-1 fragment support to `StoryRunnerHandle.evaluate()` (F5.1, part of
  #440): an arbitrary author-typed expression (`"has(sword) && gold > 2"`,
  `"gold"`), content (`"You have {gold}"`), or lone divert (`"-> cellar"`) — not
  just a bare knot path or a literal-arg call (Tier 0) — now evaluates instead
  of coming back as a dead-end diagnostic.

  Mechanism: the fragment is wrapped as a synthetic knot/function
  (`=== function __eval_<hash>() ===\n~ return (...)` for an expression,
  `=== __eval_<hash> ===\n...` for content — classified by trying the
  expression wrap first and falling back to content), recompiled against the
  project's full sources via a new `brink-web` entrypoint,
  `compile_fragment(entry, sources, syntheticSource)` (multi-file/`INCLUDE`-
  aware, unlike the single-file `compile()`), then run through the already-
  shipped F4 `Speculation` machinery: a fresh `StoryRunnerHandle` over the
  recompiled program, seeded from the live runner's current state
  (`load(liveRunner.save())`, name-keyed — globals by name, visit/turn counts
  by content-hashed id, both stable across the recompile), `speculate()`, then
  `evalFunction`/`goToPath` exactly as the Tier-0 path already does. The
  speculation and its scratch runner are discarded when done; nothing touches
  the live runner. `evaluate()`'s return shape (`SpeculationResult`) is
  unchanged — Tier-1 is invisible to the caller beyond accepting more `source`.

  Since a `StoryRunner` holds no reference to the file set it was compiled
  from, `evaluate()` gains an `opts.projectSource: { entry, files }` option —
  required only for a Tier-1 `source`, supplied by the consumer (the editor,
  which has the project's live sources). Without it, or when a fragment fails
  to compile as either an expression or content, `diagnostics` comes back
  non-empty and nothing runs (no crash).

  The scratch runner starts with no external bindings of its own, so
  `evaluate()` copies the live runner's registered bindings and
  lenient-unbound policy onto it first (`StoryRunner.binding_names`/
  `get_binding`/`lenient_unbound`, new) — a query/effect external the fragment
  touches resolves the same way it would on the live runner, matching Tier-0's
  guarantee (Tier-0 gets this for free by forking the same runner).

  Compiled fragments are cached per `StoryRunnerHandle`, keyed by
  `(program checksum, fragment source)`: a fragment compiles once per program
  version, then every re-evaluation (e.g. a watch panel re-running on every
  step) is a cache hit. The cache is bounded (200 entries, FIFO eviction) so a
  long session of one-off watches can't grow it without bound. A new
  `StoryRunnerHandle.checksum()` (mirroring `StoryRunner::checksum` /
  `programChecksum`, but read off the already-linked program so it survives
  `reload`) keys the cache to the running program's identity.

  The oracle corpus is unaffected — this is purely additive to the compiler's
  web binding and the web/TS speculative-eval wrapper; the runtime's own
  drive/episode path is untouched.

## 0.8.0

### Minor Changes

- 3cf1062: Fold kinds (#365): `FoldRange` now carries a `kind: "structural" | "machinery" | "narrative"`.

  - **`structural`** — everything the folding pass emitted before #365 (knot/stitch declarations, doc comments, conditionals, sequences, choice sets, the INCLUDE-block fold). User-invoked in every mode; never auto-collapsed.
  - **`machinery`** — a maximal run of `>= 2` consecutive machinery-natured lines (logic `~`, VAR/CONST/LIST decls, standalone diverts, conditional/sequence scaffold lines). Run-based over the per-line classification (base, or a registered dialect's declared `nature`, #368) — never HIR-block-based, so a narrative-bearing conditional's scaffold lines don't drag its prose branches into a machinery fold.
  - **`narrative`** — the symmetric run of `>= 2` consecutive narrative-natured lines (plain prose, or dialect kinds like `character`/`parenthetical`/`dialogue`).

  Editor-side (`@brink-lang/editor`):

  - `foldingExtension` takes a live-reconfigurable **active-kinds set** (default: all three); `setActiveFoldKinds(view, kinds)` reconfigures a mounted view.
  - New exported commands `foldAllOfKind(kind)` / `unfoldAllOfKind(kind)` — bulk fold/unfold every current range of one kind. Mode auto-collapse is always **host-invoked** (call these on your own mode-entry hook); the extension itself never forces a collapse.
  - Machinery/narrative folds render a JetBrains-style summary pill instead of the generic `…` placeholder: `brink-fold-pill` + `brink-fold-pill-machinery`/`brink-fold-pill-narrative` + `brink-fold-pill-icon`/`brink-fold-pill-summary`/`brink-fold-pill-count` child spans — class-addressable, zero inline styles. The machinery pill summarizes salient calls/assignments/divert targets (capped at 2, "+N more"); the narrative pill shows a first-line snippet, cast (via the registered dialect's carried `speaker` attribute — not a re-hardcoded `characterName()`), and line count.
  - The existing declaration fold placeholder (`.brink-fold-decl`) now carries `data-decl-kind="knot" | "stitch" | "function"` plus a `.brink-fold-decl-icon` slot span.

  `brink_ir::ElementNature` (narrative/machinery/structural) and `ResolvedDialect::nature_of` are new in the Rust dialect schema, consumed by `brink-ide::folding::machinery_and_narrative_folds` — never re-hardcoding a kind list in Rust or TS.

- 58d93ee: Compiler lines table + public `DialectParser` (#366): a host can now work out the cast (and similar per-line analyses) from the compiler's own line table instead of duplicating the `@Name:<>` convention.

  - **`@brink-lang/web`**: `StoryRunnerHandle.linesTable()` returns the compiled program's line table — one entry per scope (root/knot/stitch), project-wide (`INCLUDE`s already resolved by the compile), each line carrying its text (plain or a slot/select template) and, when known, its source span (`file` + byte range). Reuses the exact shape the `export-xliff` CLI path already produces (`brink_intl::export_lines`) rather than inventing a second representation. Static for the loaded program — no running `Story` required.
  - **`@brink-lang/editor`**: `DialectParser` (pure TS, no CM6/wasm dependency) — `parseSource(text)` classifies plain `.ink`-style source line-by-line against a `DialogueDialect` (mirrors `element-type.ts`'s classify + chain passes); `parseEmitted(text)` walks _runtime-emitted_ text (the post-glue output of `continue_line()`) into composite segments per the pinned iteration protocol: a cue + parenthetical + trailing text emitting as ONE line is the normal case, and a non-reserved-prefix shape (e.g. a parenthetical) never opens a composite line — it only peels as a continuation after a reserved-prefix (cue) segment.
  - **`detectCast(lines, dialect)`** ships as the #366 answer to cast detection: it walks `parseSource` output and collects the distinct values of whichever attr a dialect's chain rules `carry` forward (dialect-agnostic — not hardcoded to `speaker`). `characterName()` is NOT exported publicly (stays `screenplay.ts`-internal, per the dialect-spec ruling).

  First consumer: celeris cast detection feeding its speaker-color settings surface. The same lines-table exposure serves future analyses (per-speaker word counts, the #362 line-fit metrics epic).

- 6785663: Dialogue dialect editor integration (#368): the screenplay behavior (`@Name:<>` character cues, `(beat)<>` parentheticals, the dialogue chain) is now driven by a `DialogueDialect` — a versioned, pure-JSON schema — instead of hardcoded regexes.

  - **`brinkStudio({ dialect })`** (default `AT_CUE_DIALECT`, byte-identical to the old hardcoded behavior). `dialect: null` tears down the screenplay layer — classification, decorations, dialect transition rows, dialect keybinding behaviors — for true headless composition (pair with `theme: false`, #363); the structural weave keymap (Choice/Gather/Narrative Tab/Enter transitions) stays active, per the spec's structural-rows-stay-interpreter-owned rule. A custom `DialogueDialect` object drives classification/decorations/transitions/conversions with zero editor code changes.
  - **`@brink-lang/web`**: `EditorSessionHandle` gains `setDialect(dialect)` / `clearDialect()` (wrapping the wasm `set_dialect`/`clear_dialect` seam from #386), and the `DialogueDialect` schema types + the `LineContext.dialect` facet are published from the type surface.
  - **`setDialect(view, dialect)`** live-reconfigures an already-mounted editor: swaps the screenplay compartment, forces reclassification, and re-runs the wasm `set_dialect`/`clear_dialect` when a document handle is present.
  - **`extendDialect(base, overrides)`** adds a kind (or overrides chain/transitions/templates) without forking a preset.
  - Classification is authoritative in Rust (`brink_ir::dialect` + `line_contexts_with_dialect`, landed in #386) when a wasm document handle is present. Without one, the editor falls back to a thin TS interpreter over the identical JSON (`ResolvedDialect`), pinned against the same conformance corpus (`tests/dialect_fixtures/at_cue.json`) as the Rust side so both paths agree on every case.
  - Screenplay geometry (`screenplay.ts`'s hidden decorations, atomic ranges, edit guard, cursor clamps) is now derived from the resolved dialect's hidden-group match indices — computed once at classification time and cached, never re-matched in per-keystroke hot paths. The `CHAR_SUFFIX_LEN`/`GLUE_LEN` constants and the public `characterName()` export are gone; the geometry is dialect-derived and internal.
  - The Tab/Enter/Shift-Tab transition table and name-surgery keybindings now consult a dialect's declared overlay rows before the built-in structural weave table (inert for the default preset, which ships no overlay rows).

  ### BREAKING CHANGE: `ElementType` enum → open string union (0.x hard cut, ruled 2026-07-05)

  `ElementType` used to be a numeric TS `enum`. It is now a `const` object of kebab-case kind strings mirroring the existing `brink-<kind>` CSS class scheme — `ElementType.Character`-style call sites migrate mechanically (the values still compare correctly), but the type is now `string`, and two PascalCase leaks are now kebab-case:

  - `@brink-lang/studio`'s published `StudioApi`: `StudioPublicState.element.type` was `"KnotHeader"`, `"NarrativeText"`, `"Choice"`, … — now `"knot-header"`, `"narrative"`, `"choice"`, ….
  - `@brink/studio-store`'s duplicate `ElementType` enum is deleted; it now re-exports the real one from `@brink-lang/editor` (still available as `ElementTypeEnum`).

  Full PascalCase→kebab mapping table in `docs/editor-consumer-guide.md`. No compat shim — both packages are pre-1.0.

- f72f181: Expose the Rust `StorySession` journal/replay layer (#370, PR #385) on `@brink-lang/web` as `StorySessionHandle` (#387): `advance`/`continueSingle`/`continueToPause`, `choose`/`resolveExternal`, turn-boundary `setVar`/`goToPath`/`saveState`/`loadState`, journaled `callFunction`, `snapshot`/`diff` (+ standalone `diffSnapshots`), `exportJournal`/`StorySessionHandle.restore`, `reload`/`continueReplay`, and `restart`. Fixes the wire-format lie where `awaiting_external` was smuggled into the `Line` union: `advance()` now returns a distinct `StepOutcome` (`{ type: "line", line } | { type: "awaiting_external", deferred, name? }`), keeping the two park states (promise-in-flight vs. deferred out-of-band) explicit. New TS types (`StepOutcome`, `SessionJournal`, `StateSnapshot`, `StateDiff`, `ReplayOutcome`, etc.) ship from `@brink/wasm-types` and are re-exported here.
- 9d1dd69: Add the spec-mandated deferred+debounced journal-append persistence hook to `StorySessionHandle` (#390, `docs/story-session-spec.md`) that #387/#389 dropped: `onJournalDirty(listener)` registers a callback that fires **after** the call stack that grew the journal has fully unwound (never synchronously inside `advance`/`choose`/`resolveExternal`/`setVar`/`goToPath`/`loadState`/`callFunction`/`reload`/`continueReplay`, and never re-entrantly while another `StorySessionHandle` method is on the stack), coalescing bursts of calls into a single notification. The signal is intentionally minimal — `{ eventCount: number }` (new `JournalDirtySignal` type from `@brink/wasm-types`) — hosts pull the actual journal via the existing `exportJournal()`. `onJournalDirty` returns an unsubscribe function; `restart()` resets the dirty baseline so a fresh journal isn't reported dirty. `crates/brink-web` gains one additive `WebSession.journal_event_count()` accessor as the cheap dirty-signal source.
- 1f91422: Story-graph edges now carry source spans (#371): each `StoryGraphEdge` lists
  its `occurrences` — the divert sites that produced it, as UTF-16 spans
  (`{file, start, end}`), one entry per site on aggregated edges. Path targets
  anchor on the target path's span; `-> DONE`/`-> END` on the divert statement.
  New `StoryGraphEdgeOccurrence` type exported; the field is optional and
  omitted only for synthesized diverts with no source anchor.
- a11b115: Studio migration onto the public `StorySession` (#388, deliverable 3 of docs/story-session-spec.md):

  - **`StorySessionHandle` gains the Program Explorer / State View / shared-flow surface** that was only on `StoryRunnerHandle` before: `debugSnapshot()` (live position — globals, call stack, visit counts, pending choices, RNG), `programModel()` / `programInkt()` (static, compile-bound), and the shared-flow quartet `spawnFlow` / `continueFlow` / `chooseFlow` / `destroyFlow` / `flowNames` / `flowDebugSnapshot`. A flow spawned this way shares the _session's own_ globals/visits/rng (true ink concurrent-flow semantics) — the same VM instance the session drives, not a second one. This was a real gap in the shipped #389/#393 bindings (flagged in the design round's critique of the studio-migration proposals): without it, `@brink/studio-store`'s `LocalSessionProvider` couldn't migrate onto the session without regressing shared flows (#200) or the State View.
  - `crates/brink-web`'s `WebSession` now retains the decoded `StoryData` (mirroring `StoryRunner`) so `program_model`/`program_inkt` can be derived without a second decode, and delegates `debug_snapshot`/the flow quartet through the documented `StorySession::story()`/`story_mut()` escape hatch (journal-bypass, by design — flow stepping was never meant to journal).
  - `debugSnapshot().pending_choices[].index` now carries the choice's raw, pre-filter `pending_choices` position (the same index `choose()` expects), instead of leaving consumers to infer it from array position — which is wrong whenever an invisible-default choice sits at the same pause point, since invisible-default choices are filtered out of `pending_choices` but still occupy a slot in the runtime's underlying list.

  `@brink/studio-store`'s `LocalSessionProvider` (private, not published) now drives `StorySessionHandle` instead of `StoryRunnerHandle`: choice/continue/restart flow through the session, replay-on-recompile flows through `reload()`'s typed `ReplayOutcome` (`replayed` / `diverged` / `failed`), and persistence is push-based via `onJournalDirty` (no polling, no bespoke save timing). The pre-migration `{choiceLog}` localStorage blob gets a one-time migration to the journal format the first time a fresh session starts (replayed against the new session exactly like the old silent re-walk, but building a real journal along the way) rather than a hard reset — divergence still truncates + parks + notifies exactly as before.

## 0.7.0

### Minor Changes

- 8be15da: Unified all structural-op results into a single **breaking** `StructuralResult` (replaces `MoveResult`/`SymbolRenameResult`) with an op-wide safe-by-default breakage gate. Added `deleteSymbol`, atomic `rename_dir`, `extract_to_knot`/`extract_to_function`, document-agnostic `findReferencesAt`/`referencesToSymbol`, `resolve_code_action`, and auto-import ops. BREAKING: consumers of `MoveResult`/`SymbolRenameResult` migrate to `StructuralResult` (the `safe`/`introduced_diagnostics`/`cross_file_edits` fields are preserved).

## 0.6.0

### Minor Changes

- b0746e7: Knot/stitch **Rename** — a full, cross-file, safe-by-default rename on the shared symbol context menu (editor / Binder / Story Graph) and the editor's **F2**. A clean rename applies immediately; one that would introduce diagnostics flips to an in-place breakage report whose only override is an explicit **Force rename** (mirroring the `brink ide rename` CLI's `--unsafe` gate). An open symbol-view tab survives its own rename (re-keyed in place).

  F2 is now a full cross-file rename — the previous single-file F2 was a bug. `@brink-lang/web` gains `rename_symbol` / `rename_symbol_at` and drops the superseded `rename_doc` / `rename` exports (and the corresponding `doRename` handle methods).

## 0.5.1

### Patch Changes

- 080a715: Fix: ordinary words that happen to match ink keywords (e.g. "and", "or", "not") are no longer highlighted as code when they appear in prose. Keyword highlighting is now limited to expression/logic contexts, so narrative text renders as plain text. (#275)

## 0.5.0

### Minor Changes

- a6bceef: Binder file lifecycle — manage whole files and folders directly in the binder.

  - **Delete** files and folders from the context menu, with undo.
  - **Rename** files and folders inline (F2 or the context menu). Every `INCLUDE` that points at a renamed or moved file is rewritten automatically, and `..`-relative include paths now resolve correctly across the toolchain.
  - **Move** files by dragging onto a folder, drag a file back out to the project root, and multi-select to move several files at once — all undoable, with one "Moved N files" step.
  - Renaming a file keeps its open editor tab in place (pin, split, and selection are preserved) instead of reopening it.

  `@brink-lang/web` gains the `rename_file` session op, which computes the edit set for a file move: the re-keyed file content plus the referrer `INCLUDE` rewrites.

## 0.4.2

### Patch Changes

- 05325c0: Argument-widget + editor polish.

  - **Bundle the editor font** — the studio now self-hosts JetBrains Mono
    (Latin, regular/bold/italic), so embedders without it installed (e.g. RPG
    Maker MZ / NW.js) no longer fall back to the system monospace.
  - **Typed widgets in the Host Functions panel** — composing a fresh call from
    the panel now uses the same value-list dropdowns, host widgets, and
    arg-group controls as the in-editor call Form, not plain text fields.
  - **Host-sourced value-lists in the Form** — a slot whose semantic type
    declares `values: host` now surfaces its dropdown items from the pushed host
    cache, not just static manifest items.

## 0.4.1

### Patch Changes

- facc579: Argument-widget fixes.

  - **Embedded host content theming/positioning** — widget popovers (the color
    picker, host pickers, the call Form) now mount inside the `.brink-studio` root
    and use `position: fixed`, so embedded host content inherits the theme tokens
    and positions correctly when the studio is embedded in a host page (rather than
    rendering unstyled or mis-placed against `document.body`).
  - **Auto-open on completion-accept** — the completion kind map was keyed by the
    wrong casing, so every completion was typed `"text"`. This both mis-iconed
    completions and disabled "open the Form when accepting a function completion".
  - **The call Form is driven by the signature metadata**, not the live call-site,
    so a partial or over-full call still renders its declared widgets (e.g. an
    arg-group picker) instead of degrading to plain text fields; Apply writes a
    well-formed call.

## 0.4.0

### Minor Changes

- 755868c: Argument widgets — rich, type-driven call-site authoring.

  - A whole-call **Form** that renders one control per argument, chosen by the
    argument's type: a text input, the built-in color picker, a host-declared
    **value-list dropdown**, or a host **custom widget** — including **arg-groups**
    (one widget over several parameters, e.g. a 2D point picker) whose editor
    embeds inline. The Form holds live draft state, so an arg-group's inter-arg
    context resolves from the current form (pick a map, then a spot on that map)
    before anything is written.
  - **Inline editing** of typed arguments in the editor: color swatches,
    value-list name labels, host-rendered chips, and arg-group chips — Edit a
    filled literal, Fill an empty slot, or open the Form (an opt-in inline glyph,
    the always-on hover-card action, the `Mod-Shift-A` keybind, or the Host
    Functions panel).
  - A host **argument-widget API** (`StudioExtensions.argumentWidgets`): built-in
    and host-provided widgets, popover/modal editor surfaces, and arg-group
    widgets that receive resolved inter-arg context.
  - The `argument_widgets` IDE query now reports per-slot value-list items and
    per-group inter-arg context indices across the wasm boundary, so the studio
    can render dropdowns and resolve context from live form state.

## 0.3.0

### Minor Changes

- bcd23b7: Add program-identity, flow-control, and host-value APIs.

  - `programChecksum(bytes)` — the source-identity checksum of compiled `.inkb`
    bytes (matches `ProgramModel.checksum`) without constructing a runner.
  - Shared-context flows on `StoryRunnerHandle`: `spawnFlow`, `continueFlow`,
    `chooseFlow`, `destroyFlow`, `flowNames`, `flowDebugSnapshot` — concurrent
    flows of one story that share globals / visit counts / rng.
  - `EditorSessionHandle.setHostValues` / `clearHostValues` — push host-provided
    values for `host`-source semantic types into the editor's value cache (the
    author-time argument picker).

## 0.2.0

### Minor Changes

- 20764ef: Add `StoryRunnerHandle.goToPath(path)` — ink's `ChoosePathString` equivalent. Moves the play head to a named knot or stitch (`"knot"` / `"knot.stitch"`); subsequent `continue*` calls run from there. The session keeps its state: variables and visit/turn counts survive, and the jump itself counts as a visit to the target, exactly like a `-> path` divert. Pending choices are abandoned (callstack reset); the transcript so far is kept. Throws on an unknown path (naming it), and refuses to jump while the story is parked on an unresolved async external — resolve it (or `reset()`) first.
