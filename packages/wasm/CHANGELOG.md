# @brink-lang/web

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
