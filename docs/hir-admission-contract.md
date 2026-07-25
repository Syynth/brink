# The HIR admission contract — draft

Status: **RULED by delegation (2026-07-19) — not fully reviewed.** The
maintainer adopted the coordinator's Q1–Q7 recommendations from summaries
without a close read of this document (see the decision-log's "Delegated
batch ruling" entry): **Q1(b)** opaque `Provenance` + resolver trait,
sequenced first · **Q2(a)** ratify the byte-range-equality join for v1
with a loud admission check (NodeIds tracked as endgame) · **Q3(b)** the
`SymbolManifest` becomes a pipeline projection of HIR · **Q4(b)** two
container levels for v1, addressing model written to generalize ·
**Q5(a)** chart #905 is a body-dialect inside the native frontend ·
**Q6(b)** native accept-list admission gate · **Q7(a)** explicit
`Return.kind: ReturnKind`. These drive B0's slice decomposition. Any
downstream surprise must be flagged loudly — the settled-deliberation
presumption does not apply at full strength.

Originally: DRAFT — design note for maintainer ruling. Companion to
`docs/native-surface-charter.md` (driver #2: "a second frontend forces a
defined HIR admission contract, after which per-domain frontends become
ordinary clients"). This document proposes the contract; nothing is ratified
until the decision-log says so. Evidence base: `hir-admission-contract-findings.md`
(cited as F-A…F-K).

This is a **pushback document**. The de facto contract that lives in the code
today is not clean: it is welded to the ink CST in several places, it trusts a
dozen invariants that hold only because one particular lowering happens to
produce them, and its failure mode is uniformly *silent wrong output*. §3 names
every ugly place plainly. The contract this document proposes is the one we
*want*; §3 and §4 are honest about the distance from here to there.

---

## 0. Framing — what "a second frontend" actually means here

The charter names three future clients: the **native parser** (prose + code),
**statecharts (#905)**, **dialogue conventions**. The decision-log complicates
the word "frontend":

- 2026-07-17 (FSM charter, boundary 2): *"a live second frontend is rejected
  (the converter/FG-6 lesson)"* — for **statecharts specifically**, where SCXML
  is a one-way import door and ink text stays the only editable canonical form.
- 2026-07-19 (typing posture): the **native surface is strict-only**; gradual
  typing does not exist on it; it is its own dialect, not a project knob.

Reconciliation this document adopts (subject to Q5): there is exactly **one new
live frontend — the native parser.** The native parser owns a **body-dialect
mechanism** (charter §3, axis 2: prose / code / chart). The chart dialect (#905)
and the future dialogue dialect are **body grammars inside the native frontend**,
not independent parsers producing HIR on their own. They are "ordinary clients"
of the contract *through* the native frontend's lowering scaffolding — which is
exactly what makes them ordinary rather than heroic. The converter's grave
(retired 2026-07-11, #544) is the standing warning against a genuinely parallel
second HIR producer.

So the contract has two jobs:

1. Define what **any** HIR producer must emit — so the native frontend is a
   peer of the ink frontend, not a bolt-on.
2. Define the **body-dialect seam** — so chart/dialogue reuse the native
   frontend's admission machinery instead of re-deriving it.

The two-client test (§2) exercises exactly these two jobs.

---

## 1. Contract statement

### 1.1 What a frontend MUST produce

Per source file, a frontend produces the triple the ink lowering produces today
(F-A):

    (HirFile, SymbolManifest, Vec<Diagnostic>)

**The `SymbolManifest` is part of the contract, not an internal detail.** This
is the single most under-appreciated obligation. The analyzer never re-derives
the manifest from the HIR body nor vice-versa — it *trusts they agree* (F-A,
F-I). A frontend that emits a correct HIR tree but an inconsistent manifest
produces silently-wrong analysis. (Q3 asks whether we should collapse this to
one artifact.)

The HIR itself (`hir::HirFile`) is a **rich semantic tree**: expressions stay as
trees, choices/conditionals/sequences keep branch structure, diverts/tunnels/
threads are semantic nodes, weave nesting is resolved, sugar is stripped. It is
*not* lowered toward the VM (no stack ops, no jump instructions, no container
IDs — those come later, F-D).

### 1.2 Node inventory + per-node invariants

The current node set (`hir/types.rs`), grouped, with the obligation each node
places on a frontend. **RULED** = load-bearing today (a consumer depends on it);
**UGLY** = holds only by construction and should be fixed (see §3).

**File root — `HirFile`**
- `root_content: Block` — UGLY: ink's pre-first-knot loose content (F-F). Native
  files have none; this is always-empty for native. Keep as `Block::default()`.
- `knots, variables, constants, lists, structs, externals, includes` — flat vecs.
  RULED: globals are *hoisted flat* regardless of source nesting (F-G). A
  frontend with real scoping still emits flat global vecs.
- `module: Option<ModuleDecl>`, `imports`, `visibility`, `was_directives` — the
  **directive channels** (F-E). RULED: these fields are frontend-agnostic; only
  their *population* is ink-tag-bound today. A native frontend fills them from
  keyword syntax (`import`, visibility keywords, `@[was]`).

**Containers — `Knot`, `Stitch`, `Param`**
- `Knot.ptr: ContainerPtr` — UGLY: an enum literally over ink AST node kinds
  (`Knot(AstPtr<ast::KnotDef>)` / `Stitch(AstPtr<ast::StitchDef>)`, F-C). See Q1.
- `Knot.name: Name`, `Stitch.name: Name` — RULED: `name.text` must be the exact
  spelling the manifest indexed the symbol under (F-I#3, F-I#5).
- `is_function: bool` — RULED, and doubly so: it must agree with the index's
  `detail == "function"` sentinel (F-I#4).
- `params: Vec<Param>` — `Param.{name, is_ref, is_divert, annotation}`. RULED:
  `is_ref` must be positionally aligned with call-site args (F-I#8).
- `is_local`, `effects_assertion`, `return_type` — directive/annotation-derived,
  frontend-agnostic channels.
- Nesting: `Knot.stitches: Vec<Stitch>`; `Stitch` has no children. RULED at
  exactly 2 levels (F-K deep-nesting, Q4).

**Blocks & statements — `Block`, `Stmt`**
- `Block { label, stmts, container_id }` — `container_id` is `None` at admission
  (stamped later, F-D). RULED: a labeled block's `label.text` must match the
  analyzer's label-name convention.
- `Stmt` variants: `Content, Divert, TunnelCall, ThreadStart, TempDecl,
  Assignment, Return, ChoiceSet, LabeledBlock, Conditional, Sequence, ExprStmt,
  EndOfLine, LogicBlock, Await`. RULED classification invariants (F-I):
  - `Divert`/`Return` are terminal; `TunnelCall`/`ThreadStart` are not (drives
    `E033`, F-I#7).
  - `Return.kind: ReturnKind { Explicit, TunnelRedirect }` — the explicit
    semantic bit (B0.2). `ptr` presence carries no meaning (F-I#6 retired).
  - A divert must be *last* in an inline conditional/sequence branch (F-I#7).
- `LogicBlock`/`BlockStmt` — the `~ { … }` closed statement set (no weave node by
  construction). `Await` at statement position only (F-H).

**Weave — `ChoiceSet`, `Choice`**
- `ChoiceSet { choices, continuation: Block, context, depth: u32, gather_id }` —
  UGLY: `context` and `depth` are pure weave-fold bookkeeping that "downstream
  passes never inspect" (F-F) yet are mandatory fields. A native `{?}` point must
  fabricate them. See §2 and Q6.
- `Choice { is_sticky, is_fallback, label, condition, start/bracket/inner_content,
  tags, body, container_id }` — RULED: `is_fallback` ⟺ the point's `else` arm.

**Content — `Content`, `ContentPart`** — plain text, glue, spring, interpolation,
inline conditional/sequence. RULED: the `normalize_file` pass lifts inline
constructs to block level before LIR; a frontend need not pre-normalize.

**Control flow — `Divert`, `TunnelCall`, `ThreadStart`, `DivertTarget`,
`DivertPath`, `Return`** — RULED: `END`/`DONE` are `DivertPath::End/Done`, never
`UnresolvedRef` paths (F-I). Path targets are unresolved `Path` nodes.

**Expressions — `Expr`** — literals, `Path`, `DivertTarget`, `ListLiteral`,
prefix/infix/postfix, `Call`, and the brink extensions (`ArrayLiteral`,
`MapLiteral`, `Index`, `StructLiteral`, `FieldAccess`, `FnLiteral`, `RefArg`).
RULED: every referencing expr's `.range` must be **byte-identical** to the
`UnresolvedRef.range` the manifest recorded for it (F-I#1 — the single most
dangerous coupling; Q2).

**Declarations — `VarDecl`, `ConstDecl`, `TempDecl`, `Assignment`, `ListDecl`,
`StructDecl`, `ExternalDecl`, `IncludeSite`** — RULED: each declaration's `name`
+ kind must match the manifest bucket and `SymbolKind` it was indexed under; the
`SymbolKind` chooses the `DefinitionId` tag byte, i.e. every `.inkb` address
(F-D, F-I#9).

**Provenance types — `Name`, `Path`, `Tag`** — `Name { text, range }`,
`Path { segments, range }`. RULED: ranges are identity keys, not just diagnostic
anchors (F-I#1, F-J).

### 1.3 Fidelity obligations (spans, names, paths)

- **Spans.** Every node's `range`/`ptr` range must be a real, non-empty source
  offset. Ranges are (a) diagnostic anchors, (b) IDE geometry, (c) **resolution
  join keys** (F-I#1), (d) local shadowing order keys (`local.range.start()`,
  F-I). A garbage/zero range does not error — it silently corrupts resolution and
  shadowing. This is why the native parser must produce real source spans even
  though the native surface renders structure elidably (charter §2).
- **Names / paths.** `Name.text` and `Path.segments` must be well-formed and
  stamped in the exact qualification convention the index is queried against:
  stitches `knot.stitch`, labels `knot[.stitch].label`, list items `List.item`
  (F-I#3). Malformed → silent non-resolution.
- **Dialect tags.** There is **no dialect tag on any HIR node** (F-B, F-I#10).
  Dialect and type-policy arrive as `AnalysisOptions`. A frontend must not try to
  embed them. (§4 proposes the native frontend declares its dialect to the
  *pipeline*, not into the *tree*.)
- **Directive/annotation channels.** The HIR fields (`is_local`,
  `effects_assertion`, `module`, `visibility`, `was_directives`) are the channel;
  a frontend populates them from its own syntax. The reserved-namespace rule
  (every `@`-mark is a valid directive in a valid placement or a hard error,
  F-E) is the model for the whole admission posture (§4).
- **Declaration completeness.** The manifest's declared-symbol buckets, locals,
  and unresolved-refs must be *complete* — every reference in the body must appear
  as an `UnresolvedRef` (except builtins/`END`/`DONE`, F-I#9), or resolution and
  the dialect gate misfire.

### 1.4 What a frontend MUST NOT do

- **Must not reach into analyzer state.** No `SymbolIndex`, `ResolutionMap`,
  inferred types, or `DefinitionId`s at lowering time. HIR is pre-resolution,
  pre-typing, pre-container-ID (F-D; `docs/compiler-spec.md` "What HIR does NOT
  do"). The frontend emits *unresolved* `Path` nodes and an `UnresolvedRef` list;
  the pipeline resolves.
- **Must not assume lowering order or a global counter.** Container IDs are
  stamped by a *later* pipeline pass from structural position (F-D); a frontend
  must not pre-assign them.
- **Must not embed dialect/type-policy in the tree** (F-I#10).
- **Must not pre-resolve builtins** (`len`, `none`, `LIST_MIN`, …) — leave them
  as unresolved refs (F-I#9).
- **Must not smuggle semantics through pointer presence** — discharged for
  `Return` by B0.2: a frontend stamps `ReturnKind` explicitly and may attach
  (or omit) provenance freely on either kind (F-I#6 retired).

### 1.5 What the PIPELINE owns downstream (not the frontend)

- **Resolution** — `Path` → `DefinitionId` (`brink-analyzer::resolve`), from the
  manifest's `unresolved`/`locals` + the merged index.
- **Typing / inference** — monomorphic HM per SCC, params-from-body-uses; strict
  vs gradual per dialect (2026-07-19).
- **Effects** — effect-row inference + `@[effects]` exceedance checking.
- **Container identity** — `normalize_file` then `stamp_container_ids` (F-D).
- **LIR + codegen** — the frontend never sees LIR, bytecode, or `StoryData`.

---

## 2. The two-client test

Walk each client through §1 and ask: does it need anything the contract lacks?

### Client #1 — the native parser (prose + code)

The native surface is a **respelling at the same semantics** (charter §2), so the
happy path is: native constructs lower to *existing* HIR nodes. Where they do:

| Native construct (charter) | Lowers to | New node? |
|---|---|---|
| `flow`/`fn` declarations | `Knot { is_function }` | reuse |
| nested `flow` (stitch) | `Stitch` (2 levels) | reuse ≤2; **gap** >2 (Q4) |
| `{?}` choice point | `ChoiceSet` + `Choice` | reuse (but fabricate `depth`/`context`, §3) |
| `*`/`+` choice lines | `Choice.is_sticky` | reuse |
| `else { … }` fallback | `Choice.is_fallback` | reuse |
| splice `<- flow(args)` | `ThreadStart` | reuse (charter §11 narrowed threads to this) |
| dissolved gather | `ChoiceSet.continuation` | reuse (frontend synthesizes the block) |
| `{if}`/`{match}` | `Conditional{IfElse/Switch}` | reuse |
| alternation `{~}{&}{!}{|}` | `Sequence` (bitmask) | reuse |
| diverts / tunnels `-> x ->` | `Divert` / `TunnelCall` | reuse |
| `return` / `return -> x` | `Return` | reuse (`ReturnKind` explicit, B0.2) |
| `var`/`const`/`flags`/`extern` | `VarDecl`/`ConstDecl`/`ListDecl`/`ExternalDecl` | reuse |
| `struct` | `StructDecl` | reuse |
| UFCS `x.foo(y)` | `FieldAccess`/`Call` (analyzer disambiguates) | reuse |
| function values `#fn` | `FnLiteral` | reuse |
| `await` (statement, flow only) | `Await` / `WhileStmt.is_await` | reuse |

**Native features with no clean ink equivalent — the additions inventory:**

1. **`for k, v` iteration** (ruled 2026-07-18). `ForStmt.var_name` is single
   (F-K). **Additive field** `val_name: Option<Name>`. *Lean.*
2. **`enum` declarations + exhaustive `match`** (§13.1, ruled 2026-07-19).
   No HIR node (F-K). New `EnumDecl { name, variants: [{name, fields}] }`; `match`
   can extend `CondKind::Switch` with variant patterns or gain a `CondKind::Match`.
   *Lean when the enum feature lands; defer the node until then, but reserve the
   `HirFile.enums` channel now so admission doesn't reject it.*
3. **`impl` blocks / companions** (S4, ruled 2026-07-19). Lower to fns in a
   virtual companion module; `Npc::greet` is the only real name,
   `DefinitionId = (companion-module, name)` (F-K). **No new container node** —
   reuse `Knot(fn)` + module qualification; `self` is an ordinary `Param`. *Lean
   (a lowering rule, not a node).*
4. **Anonymous lambdas.** `FnLiteral` is partial application over a *named*
   target, not an anonymous body (F-K). Charter §7 leans UFCS-over-lambdas
   ("no method system"). *Defer — do not add an anonymous-fn-body node until the
   code sitting rules one is wanted.*
5. **blocks-as-values** (watch list). No HIR support; `Stmt`/`Expr` are separated
   (F-K). *Defer to a semantics round (parking-lot); it is not same-semantics.*
6. **Deep container nesting >2** (watch list). No HIR support; the model,
   the stamp pass, and the name conventions all bake in exactly 2 levels (F-K).
   *Defer for v1 (native respells same semantics; depth>2 is watch-list), but the
   contract's addressing model should stop asserting exactly-2 (Q4).*

**Verdict:** the native parser needs **one lean additive field** (`for k,v`) for
v1, plus a **reserved channel** for enums, plus lowering *rules* (companions,
UFCS) that add no nodes. The rest is reuse. The friction is not missing nodes —
it is the **ugly nodes it must fabricate** (weave `depth`/`context`, the
`Return.ptr` convention, the `AstPtr` provenance) and the **silent couplings**
(§3). The contract's real work is cleaning those, not growing the inventory.

### Client #2 — the chart body-dialect (#905)

Per §0, chart is a **body grammar inside the native frontend**, not a separate
parser. It fills a container body (charter §3, axis 2) with Mermaid-lineage graph
syntax. What it needs from the contract:

- **State declarations** — the FSM value is `{def token, state NameId}`
  (2026-07-17). States map naturally onto **nested containers** (a state is a
  `flow`/`Knot`-like addressable, visit-counted, suspendable container) — which
  is exactly the **deep-nesting gap (Q4)**: a hierarchical statechart is
  containers nested >2 deep. Chart is the concrete client that *forces* the
  nesting question.
- **Transitions** — authored entry-handler diverts (2026-07-17: "the weave seam
  stays intact"). These are ordinary `Divert`/`TunnelCall` nodes. No new node.
- **Per-state frame logic / entity controller** — gated on `FlowFrame` + the
  scheduler existing (2026-07-17 boundary 4). Uses the `Await` machinery (F-H).
  No new node, but confirms `await` must be admissible inside a chart body.
- **Enum-valued states** — "#905's statechart states inherit this feature as
  their vocabulary" (§13.1). So the **enum addition (#2 above)** is shared
  between clients — another reason to reserve the channel now.

**Verdict:** chart needs **no node the native parser doesn't also need**, *if* the
deep-nesting question (Q4) is answered generously. It shares the enum addition. It
confirms `await`-in-body. The single-source-format ruling (SCXML is import-only)
means chart never needs its own `ptr`/CST provenance — it reuses whatever the
native frontend produces. **The two-client test therefore converges on Q4
(nesting) and #2 (enums) as the only genuine additions; everything else is the
existing inventory plus the §3 cleanups.** That convergence is the good news: the
contract is nearly sufficient as a node set. Its debt is in the couplings, not
the shapes.

---

## 3. The dragons — where the ink frontend violates a clean contract today

Every item here is a place the de facto contract is ugly. Named plainly, per the
pushback mandate. Ordered by how badly a second frontend gets hurt.

### D1. Provenance is welded to the ink CST (the structural dragon)

> **Status: DISCHARGED by B0.1 (issue #1148, branch `auto/b0-provenance`).**
> The seam is `brink_ir::provenance`: every HIR node now carries an opaque
> `Provenance { file: FileId, range: TextRange, kind: KindToken }` — plain,
> publicly constructible data (reconstructible from serialized parts). The
> token is `KindToken { class: NodeClass, raw: u16 }`: `class` is a
> frontend-agnostic, stable-`u16`-repr node-class vocabulary (the only half
> the pipeline may interpret; it carries the former `ContainerPtr`
> Knot/Stitch discrimination, consumed via `Knot::symbol_kind()`), `raw` is
> the producing frontend's private syntax kind. Node-resolution is behind
> the frontend-supplied `ProvenanceResolver` trait (keyed by provenance
> *value*; `None` is a normal answer); the ink frontend's implementation is
> `brink_ir::hir::InkProvenanceResolver`, which keeps `SyntaxKind + range`
> resolution behind its own seam. §4.3's "a headless compile never resolves
> ptrs" is now *tested* (`brink-ir/tests/provenance_seam.rs` garbles every
> frontend-private half and asserts byte-identical StoryData). The
> `Return.ptr`-presence bit (D5) is temporarily preserved as
> `Option<Provenance>` — retired by B0.2's `ReturnKind`.

Every node carries `ptr: AstPtr<ast::X>` / `SyntaxNodePtr` = `SyntaxKind +
TextRange` over the **ink grammar's** ~230-variant kind enum (F-C). `ContainerPtr`
is an enum *literally over ink AST node types*. The purpose is real (LSP resolves
`ptr` back to a live ink node for rename/extract), but a native frontend has no
ink tree to resolve against. Its only escape is `AstPtr::from_range` stamping
`SyntaxKind::ERROR` — which then *cannot resolve back* (kind mismatch), silently
breaking IDE node-resolution for every native node. **This is the one coupling
that cannot be papered over; it needs a decision (Q1).** A clean contract carries
*opaque provenance* (a `FileId + TextRange +` an optional frontend-defined
node-kind token) and delegates node-resolution to a frontend-supplied resolver.

### D2. Resolution joins to HIR by exact `TextRange` (the silent-failure dragon)

The entire typing/gating/effects layer finds a reference's `DefinitionId` by
looking up the *HIR expression's own byte range* in a range-keyed map built from
`UnresolvedRef.range` (F-I#1). Nothing checks the two ranges agree — the ink
lowering emits both from the *same* node, so they always do. A second frontend
that computes the manifest ref-range and the body expr-range from different nodes,
or trims trivia differently, produces: inference sees the path as `Unknown`
(silent), the dialect gate mis-flags resolved calls as unresolved builtins, void-
checks and struct-checks misfire. **No panic, no diagnostic — just wrong output.**
This is the coupling most likely to burn a native-frontend author, and it is
invisible in code review. (Q2 proposes replacing range-join with explicit node
IDs.)

### D3. `SymbolManifest` and `HirFile` are two artifacts that must agree by hand

The frontend emits the manifest (declared symbols, locals, unresolved refs) *and*
the HIR body as separate structures; the analyzer trusts they agree and never
cross-checks (F-A, F-I). Two independent hash sites must agree on local IDs
(F-I#2). Two independent encodings of "is a function" must agree (F-I#4). The
`ContainerPtr` variant must match the indexed `SymbolKind` (F-I#5, the #626
floating-stitch trap). This is a lot of redundancy for a frontend to keep
consistent, and every inconsistency is silent. (Q3: derive the manifest from HIR.)

### D4. Weave-fold artifacts masquerade as semantics

`ChoiceSet.depth: u32` and `ChoiceSet.context: {Weave, Inline}` are pure
byproducts of ink's indentation-driven weave folder, and the spec itself says
"downstream passes never inspect depth values" (F-F) — yet they are mandatory
fields. The charter *dissolves the gather as an authorial concept* (§5), so the
native frontend, which has explicit `{?}` points and braced bodies, must
**fabricate** ink weave bookkeeping it does not have. Similarly `root_content` is
ink's pre-first-knot loose content, always empty for native (F-F). These fields
should be optional/removed, not faked.

### D5. `Return.ptr` presence is a load-bearing semantic bit

> **Status: DISCHARGED by B0.2 (branch `auto/b0-return-kind`).** `hir::Return`
> now carries an explicit `kind: ReturnKind { Explicit, TunnelRedirect }`;
> `E032` and LIR `is_tunnel` key off `kind`, never `ptr` presence. `Return.ptr`
> is uniform carrying-or-not `Option<Provenance>` with no semantic load — a
> provenance-carrying tunnel return is legal, admission-clean, and still
> classifies as a tunnel (tested both directions: `brink-analyzer`
> `validate.rs` unit fixtures + the `brink-ir` `lir_lowering.rs`
> `provenance_carrying_tunnel_return_still_lowers_as_tunnel` pipeline test).
> The B0.1 presence-semantics shim is retired. A native `return -> x` lowering
> stamps `TunnelRedirect` explicitly.

Whether a `Return` is an explicit `~ return` or a tunnel return is encoded *purely
in whether a syntax pointer was attached* (`ptr.is_some()`, F-I#6). A frontend
that attaches provenance uniformly (the natural thing to do) emits spurious
`E032`. This is a semantic distinction hiding in a provenance field — it must
become an explicit enum before any frontend that stamps provenance consistently
can exist (Q7).

### D6. INCLUDE-era and ink-scoping assumptions baked into lowering

- Globals are collected by a **whole-tree descendant walk** because "in ink,
  VAR/CONST/LIST are always global regardless of where they appear" (F-G). The
  native module system has real scoping; it produces the same flat vecs by a
  different route, but the *contract* should say "the global vecs are flat and
  hoisted", not "walk descendants".
- INCLUDE survives as intra-module flat glue (2026-07-14, #719), but the charter
  rules **textual INCLUDE is dead** on the native surface (§13.2: "THE TREE IS THE
  COMPILATION UNIVERSE; IMPORTS ARE NAMING ONLY"). `HirFile.includes` and
  `IncludeSite` are ink-only baggage the native frontend leaves empty.

### D7. Declarations are outside the shared HIR walk

`hir::visit::HirVisitor` — the canonical block-tree walk every IDE query and
several analyzer passes share — deliberately skips the flat declaration vecs and
tag contents (F-I#11). So `dialect_gate`, `structs`, `map_keys`, `conversions`
each hand-walk `hir.variables`/`hir.structs` initializers separately. A frontend
that introduces a new declaration-position expression must know to update every
hand-walk site — an un-obvious, un-enforced obligation.

### D8. Builtin resolution is a hand-synced hardcoded list in two places

`resolve.rs` and LIR lowering each keep their own copy of the stdlib-name list,
kept in sync "by hand" per the code's own doc (F-I#9). A frontend must leave these
names unresolved and trust both copies agree. A duplication the contract inherits.

### D9. The `@[effects]` grammar has already drifted from its ruling

Minor but illustrative: the 2026-07-19 tower ruling says `@[effects]` uses
paren-style `reads(gold, hp)`; the implemented `parse_effects_clauses` still uses
the colon grammar `reads:` for both channels (F-E). The directive *channel* (the
HIR field) is clean; the *recognizer* lags the ruling. A frontend populating
`effects_assertion` directly sidesteps this — but it shows the channel and its ink
recognizer drift independently, which is an argument *for* keeping the channel
frontend-agnostic (as it is) and *against* letting recognizers define the
contract.

---

## 4. Admission checking

### 4.1 Posture — loud, early, tier-1

The decision-log has a standing ruling for boundary checks (2026-07-18, capability
manifest): *"Never-fail-silently at the admission boundary — a missing X should be
discovered at load, loudly, not at first call."* The reserved-`@`-namespace rule
(F-E) already applies this to the directive channel: every `@`-mark is a valid
directive in a valid placement or a hard error, never a silent inert tag.

**Proposed: apply the same tier-1 posture to HIR admission.** Today there is *no*
admission check — the analyzer trusts a dozen by-construction invariants and fails
silently when they break (§3, all of D2–D8). This is the exact opposite of the
ruled posture, and it is tolerable only because there is exactly one frontend that
happens to satisfy every invariant. A second frontend makes silent-failure a
certainty. The contract must add a **loud admission validator** that runs at the
AST→HIR seam (F-B) and rejects a malformed triple *before* analysis.

### 4.2 What is VALIDATED at the boundary (proposed)

A `validate_admission(&HirFile, &SymbolManifest) -> Vec<Diagnostic>` pass,
non-suppressible, run in `lowered_query` (F-B), checking the invariants that are
today silent (§3):

1. **Manifest ⇄ HIR agreement** (kills D3): every `UnresolvedRef.range` matches an
   actual referencing expr range in the body; every declared symbol has a
   corresponding HIR declaration node with the same name/kind; every `Knot` with
   `is_function` is indexed with the function sentinel.
2. **Range well-formedness** (kills the D2 class at admission): every node range is
   non-empty and within file bounds; no two distinct references share a range
   (the join key must be unique).
3. **Name-convention conformance** (kills F-I#3 silent failures): stitch/label/
   list-item names match the required qualification shape (`.matches('.').count()`
   expectations made explicit and checked).
4. **Control-flow classification** (kills D5/F-I#7): `Return.kind` (landed, B0.2)
   is explicit; diverts are last in inline branches; terminal-stmt rules hold.
5. **`ContainerPtr` ⇄ `SymbolKind`** consistency (kills the #626 trap, F-I#5).

Each is a hard error with a targeted `Exxx` code, in the reserved range, per the
never-reuse-codes rule (`hir/types.rs` DiagnosticCode doc).

### 4.3 What is TRUSTED (not validated)

- **Semantic correctness of resolution targets** — that a `Path` *can* resolve is
  the resolver's job, not admission's; admission only checks the *reference is
  well-formed and recorded*.
- **Type correctness** — inference/strict-mode owns this.
- **Range↔source-text fidelity** — admission checks ranges are in-bounds and
  unique, but cannot check they point at the "right" text without the frontend's
  own tree (that is the frontend's contract to keep).
- **Provenance resolvability** — whether `ptr` resolves back is an IDE concern; a
  headless compile never resolves ptrs, so admission does not require it (this is
  precisely why D1 can ship native codegen before native IDE support).

### 4.4 Versioning / extension posture for the contract itself

- **The node set is additive-open, closed to silent extension.** New nodes (enums,
  `for k,v`, a future lambda) land as new variants/fields + their admission checks
  + their downstream handling, exactly as the directive channel's reserved
  namespace makes new directives non-breaking (F-E). A frontend emitting a node
  the running compiler doesn't know is a **loud admission error**, never a silent
  drop — mirroring the directive rule and the "older compiler rejects a newer
  story loudly" posture (`docs/directive-annotations-spec.md:42`).
- **The dialect gate inverts for native.** The ink `dialect_gate` is a
  *reject-list* ("these extension nodes are illegal under strict-ink", F-B). The
  native surface is strict-only (2026-07-19) and *produces* those very nodes. The
  clean shape is a native **accept-list** admission gate (the inverse): "these HIR
  shapes are legal native input", which *also* rejects ink-only baggage
  (`root_content` outside the single synthesized `flow main()` entry divert,
  ambient `ThreadStart`, relative weave) that a native frontend should
  otherwise never emit. (Q6.)
- **Cross-dialect calls are mediated, not merged** (2026-07-19): ink symbols enter
  native code as `Unknown`; strict rejects `Unknown` escapes; annotate at the
  seam. The contract does not need to unify the two dialects' type regimes — it
  needs each frontend to declare its dialect *to the pipeline* (an
  `AnalysisOptions`-level fact, F-I#10), never into the tree.
- **DefinitionId stability is frozen** (2026-07-14): `(module, name)` name-hashed,
  `@[was]` alias table for renames. A frontend must produce names that hash
  stably; the contract inherits this and admission checks the name conventions
  that feed the hash (4.2 #3).

---

## 5. Open questions for the maintainer

Phone-rulable. Each: the tension, options, recommendation.

**Q1 — Provenance: what does a non-ink node carry where `ptr: AstPtr<ast::X>` is
today?** (D1)
- (a) Keep `AstPtr`; native uses `from_range` dummies (`ERROR` kind). Cheapest;
  IDE node-resolution silently dead for native.
- (b) Replace every `ptr` with opaque `Provenance { file, range, kind_token }`;
  node-resolution delegated to a frontend-supplied resolver trait. Touches every
  HIR node and every ptr consumer (~15 sites); decouples cleanly.
- (c) Native produces a rowan CST sharing `SyntaxKind` space. Biggest lift;
  keeps `AstPtr` working; couples the two grammars' kind enums.
- **Recommend (b).** It is the only option that makes "frontends are ordinary
  clients" true. The ink frontend keeps `AstPtr` *behind* the resolver; native
  supplies its own. Sequence it first — every other cleanup assumes decoupled
  provenance.

**Q2 — Resolution join: keep range-equality, or introduce explicit node IDs?** (D2)
- (a) Ratify byte-identical range as the contract; add the 4.2#2 admission check
  (unique, in-bounds ranges) as the guardrail. Cheap; the silent-failure mode
  becomes a loud admission error instead.
- (b) Stamp explicit `RefId`/`NodeId` at admission; resolution keys off IDs.
  Kills the dragon at the root; larger change to `ResolutionMap` + every join site.
- **Recommend (a) for v1, (b) as the tracked endgame.** (a) converts the worst
  silent failure into a loud one immediately for little cost; (b) is the real fix
  but can follow once the native frontend exists to justify it.

**Q3 — Is the `SymbolManifest` a frontend output, or a pipeline projection of
HIR?** (D3)
- (a) Frontend emits it (status quo). Two artifacts, hand-kept-consistent, silent
  when they drift.
- (b) Pipeline derives the manifest from a single well-formed HIR (one source of
  truth). Requires HIR to carry enough for `UnresolvedRef` scope/kind — it nearly
  does; the gap is per-reference scope context.
- **Recommend (b).** It deletes D3 (and half of §4.2's checks) outright: you
  cannot disagree with yourself. Cost is a real HIR-walk that builds the manifest,
  but that walk is testable in one place instead of trusted in every frontend.

**Q4 — Deep container nesting (>knot.stitch): generalize now, or defer?** (D4-ish,
F-K, forced by chart #905)
- (a) Replace `Knot { stitches: Vec<Stitch> }` with a recursive `Container` node
  now; generalize stamp + name conventions off the exactly-2 assumption.
- (b) Keep 2 levels for v1; native respells same semantics (depth>2 is charter
  watch-list); revisit when chart #905 lands.
- **Recommend (b) with a constraint:** defer the *node* change, but stop the
  *contract* from asserting exactly-2 (the addressing model, the name conventions,
  the stamp recursion should be written to generalize). Chart is the client that
  will force (a); design the contract so (a) is additive, not a rewrite.

**Q5 — Is chart #905 a client of the native frontend (body-dialect), or its own
frontend?** (§0)
- (a) Body-dialect inside the native frontend — shares admission machinery, no
  separate parser. Reconciles the charter ("ordinary client") with the FSM ruling
  ("a live second frontend is rejected").
- (b) Independent frontend producing HIR directly. Contradicts the FSM ruling and
  re-opens the converter grave.
- **Recommend (a).** This is the framing the whole document assumes; confirming it
  fixes the meaning of "the two-client test" (client #2 = a body grammar, not a
  parser) and keeps #905 cheap.

**Q6 — Does the native surface get a `Dialect::Native`, or an inverted admission
gate?** (§4.4)
- (a) Add `Dialect::Native` that (like `Brink`) gates nothing on the reject-list.
  Simple; but native should also *reject* ink-only baggage, which a no-op gate
  won't do.
- (b) A native **accept-list** admission gate (the inverse of the ink reject-list):
  enumerates legal native HIR shapes and rejects both un-lowered extensions and
  ink-only nodes (`root_content`, ambient threads). Matches strict-only.
- **Recommend (b).** The reject-list vs accept-list asymmetry is real — ink adds
  extensions to a fixed base; native *is* the base and forbids the ink-only edges.
  An accept-list is the honest shape for a strict-only surface.

**Q7 — Fix the `Return.ptr`-as-semantic-bit coupling now?** (D5) — **RULED
(a), LANDED by B0.2** (`ReturnKind` on `hir::Return`; see the D5 stamp).
- (a) Add `Return.kind: ReturnKind { Explicit, TunnelRedirect }`; stop overloading
  pointer presence. Small, local.
- (b) Leave it; document the `ptr==None`-means-tunnel convention as a contract
  obligation.
- **Recommend (a).** It is a few lines, it removes a silent-`E032` trap, and it is
  a prerequisite for Q1(b) — once provenance is uniform, pointer presence can no
  longer carry this bit anyway.
