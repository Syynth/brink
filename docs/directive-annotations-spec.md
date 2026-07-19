# Directive annotations (`#@…`) and the flow-private storage class

Status: **v1 — implementing** (epic #473). Design rulings: `docs/decision-log.md`
2026-07-10 entries ("Annotations ride the tag channel…", "Scoped-state policy
model (final)…", "Flow-private state: runtime capability first…") and the
F6 AMENDMENT in `docs/scoped-flow-state-spec.md`. Architecture context:
the 2026-07-10 comment on issue #397.

## 1. The directive channel

Brink has two metadata channels:

- **Tags** (`# …`) — runtime-visible content metadata. Flow to the runtime,
  surface on lines/choices, and (per #474) are extracted into the format's
  static metadata table. Dynamic inline ink is allowed.
- **Directives** (`#@…`) — **compile-time** instructions to the brink
  toolchain. Any tag whose text begins with `@` (after optional leading
  whitespace) is a directive. Directives are recognized during HIR lowering,
  **consumed into structural bits, and erased** — they never appear in
  runtime tag output, never enter the #474 metadata table, and never affect
  oracle-conformant behavior of unannotated ink.

Reusing tag syntax means a brink file remains *parseable* by inklecate
(directives degrade to inert tags there), and the corpus contains zero
`@`-prefixed tags, so the reservation is behavior-neutral by construction.

### 1.1 The `@` namespace is fully reserved

Every `@`-prefixed tag, anywhere, must be a valid directive in a valid
placement. Anything else is a **compile error** — never a silent inert tag:

- Unknown directive name (`#@locale`) → error (catches typos).
- Valid directive in an unrecognized placement → error.
- Dynamic content in a directive (`#@{expr}…`) → error (directives are
  static text only).
- A directive tag sharing a line with plain tags or other directives
  (`#@local #art.png`) → error (v1: a directive line carries exactly one
  directive and nothing else).

This strictness is deliberate: the failure mode of a tag-carried directive
channel is the silent no-op, and erroring on every unconsumed `@` tag kills
that mode wholesale. (Forward compatibility across brink versions is
version-managed like everything else in the toolchain — an older compiler
rejects a newer story's directives loudly, not silently.)

### 1.2 Directive grammar

Inside the tag text (after `#`, optional whitespace, then `@`):

```text
directive      = "@" directive-name directive-args?
directive-name = ident ("-" ident)*        ; e.g. local, renamed-from
directive-args = "(" balanced-text ")"     ; interpretation is per-directive
```

The argument mini-grammar is parsed by the directive recognizer in
`brink-ir` (tag text is freeform at the CST level — no lexer changes).
`@local` takes no arguments; supplying any is an error.

## 2. Placements (v1)

A **directive line** is a `TAG_LINE` consisting of exactly one directive
tag. Two placements are recognized:

1. **Above a declaration** — the directive line immediately precedes a
   `VAR` declaration (only comments/whitespace may intervene; a doc comment
   may sit on either side). It attaches to that declaration.

   ```ink
   #@local
   VAR mood = 0
   ```

2. **Top of a knot/stitch body** — the directive line appears at the top of
   a knot or stitch body, before any content (comments and further
   directive lines may precede it; ink's conventional knot-tag position).
   It attaches to that knot/stitch.

   ```ink
   === guard ===
   #@local
   Halt! Who goes there?
   ```

Not recognized in v1 (all errors, per §1.1): inline directives on content
or choice lines, directives above a knot/stitch *header* (structurally that
tag line belongs to the previous scope's body — the error message points at
the top-of-body placement), directives at end of a block with nothing
following, file-level directives.

## 3. v1 tenant: `@local` — the flow-private storage class

`@local` marks a declaration as **flow-private**: each flow sharing a
`World` reads and writes its own copy (backed by its `FlowLocal`), instead
of the shared `World` state. Unmarked declarations keep today's behavior
(`Scope::World`) — plain ink compiles to oracle-identical output.

Valid targets:

- **`VAR`** — the variable's storage is per-flow.
- **Knot / stitch** (including `function` knots) — the definition's visit
  and turn counts, and those of **every container in its definition
  subtree** (interior weave/sequence/choice containers; a knot's marking
  covers its stitches), are per-flow. Subtree expansion happens at policy
  resolution in the runtime (the F6.1c mechanism), not in the compiler.

Invalid targets (errors): `CONST` (no runtime storage), `LIST` declarations
(deferred — the list *type* is global; revisit if per-flow list variables
are needed), `EXTERNAL`, `INCLUDE`.

There is no `@world` counterpart in v1 (unmarked already means `World`; a
stitch-level opt-out inside a `@local` knot can be expressed by a host
`WorldPolicy` override, and `@world` is trivial to add later if authoring
demand appears).

Duplicate `@local` on one target is an error.

## 4. Pipeline changes

### 4.1 brink-syntax

None. `TAG_LINE`/`TAGS`/`TAG` already parse everywhere directives are
recognized; directive recognition is downstream, over tag token text.

### 4.2 brink-ir (HIR)

- Directive recognizer module: classifies a `TAG_LINE` as directive-line /
  plain / mixed, parses name + args from tag text, with spans into the tag
  for diagnostics.
- `hir::VarDecl`, `hir::Knot`, `hir::Stitch` gain `is_local: bool`.
- File/knot/stitch structure lowering: a directive line above a `VAR_DECL`
  sibling, or leading a knot/stitch body, is consumed — it sets the flag
  and does **not** lower to a `Stmt::Content` tag statement. Every other
  `@`-tag encountered anywhere in lowering raises the appropriate
  diagnostic (new `DiagnosticCode`s; see §1.1 catalogue).
- HIR content hashing picks the flag up automatically (cache-correct).

### 4.3 brink-ir (LIR) and brink-codegen-inkb

- `lir::GlobalDef` and `lir::Container` gain `local: bool`, threaded from
  HIR in lowering.
- Codegen copies the bit onto `GlobalVarDef` / `ContainerDef`. Only
  scope-owning containers (knot/stitch) ever carry `local: true`; interior
  containers stay `false` (subtree coverage is the runtime's job).

### 4.4 brink-format — the format change

- `GlobalVarDef.local: bool` and `ContainerDef.local: bool` (wire: one `u8`
  each, appended to the existing record layouts).
- `inkb` `VERSION` **2 → 3**. The reader keeps strict equality; existing
  `.inkb` artifacts (including checked-in fixtures) are recompiled.
- `inkt` text form prints the marker **only when set** (e.g. a trailing
  `local` atom on `(global …)` / `(container …)`), so converter and
  compiler dumps of plain ink stay byte-identical.

### 4.5 brink-converter

Always emits `local: false` (inklecate has no such concept). No behavioral
change.

### 4.6 brink-runtime

- The linker carries per-global scope bits and the set of `local`-marked
  container ids onto `Program`, plus a precomputed `has_local_defaults`
  flag.
- `ResolvedPolicy::resolve` seeds from the compiled base **before**
  applying host overrides — `base ⊕ host-overrides`, exactly the F6.2
  shape: compiled `@local` bits initialize `global_scopes` /
  `knot_scopes` (via the existing subtree expansion), then `WorldPolicy`
  overrides layer on top with the existing most-specific-wins rules. A host
  override on a name always beats the compiled bit for that name's
  scope/subtree.
- The `all_world()` fast path additionally requires
  `!program.has_local_defaults()`.
- **Zero public API change.** Hosts with hand-written `WorldPolicy` lists
  keep working; the lists just become unnecessary where ink is annotated.

## 5. Conformance and testing

- Plain ink (no directives) must compile bit-identically (modulo the
  format version byte) — the oracle ratchet (5,577) and corpus report are
  the gate, unchanged.
- New-syntax stories cannot have `.ink.json`/oracle files (inklecate treats
  directives as inert tags); coverage is brink-native:
  - parser/HIR tests: recognition, consumption (no tag leaks into content),
    each diagnostic in §1.1/§3;
  - format round-trip tests for the new fields and version;
  - runtime integration: two flows over one `World` — `#@local` VAR
    isolation, `#@local` knot visit-count isolation (including an interior
    sequence under the marked knot), host override layered over a compiled
    bit;
  - `inkt` dump equality converter-vs-compiler on plain ink.

## 5b. The annotation-line channel (`@[…]`) — NS-A2 addendum

NS-A2 (#1108; stdlib-spec §9.2, ruled 2026-07-18) added a **second,
line-level spelling** for compiler annotations: the annotation line
`@[name(args)]` on a line of its own. It is the assertion final form's
carrier — v1 recognizes exactly one name, `effects`
(`@[effects(pure, silent, total, reads(gold), …)]` — paren clause
grammar since the 2026-07-19 amendment, issue #1120), in exactly one
placement (the leading run at the top of a knot/stitch body, shared
with directive tag lines in either order). Rules mirror the tag
channel's:

- superset-parsed under every dialect (`AT_L_BRACKET` token +
  `ANNOTATION_LINE` node — only the *adjacent* `@[` pair opens one; a
  lone `@` in prose stays plain text); `strict-ink` rejects the
  attached assertion via the dialect gate (`E051`);
- consumed placements erase (an annotation line never lowers to
  content); anywhere else is `E112` — never a silent drop;
- an unrecognized annotation name is `E111` (the tag-channel directive
  names do **not** alias into this channel);
- the old `#@effects(…)` tag-directive spelling remains a deprecation
  alias (shipped surface) and warns `E110`.

## 6. Future tenants (non-normative)

The channel is designed for (not implemented): `@world` (stitch-level
opt-out), type signatures (`@returns(int)`, typed params/VARs — the
`signature(def)` query carrier for the #397 pipeline), intl
(`@notranslate`, `@note(…)`, `@maxlen(n)`, stable line ids `@id(…)` — these
need the line-target placement, deliberately excluded from v1),
save-migration (`@renamed-from(old)`), `@deprecated`, lint control
(`@allow(…)`). Each lands as new recognizer cases + its own consumption
logic; the reserved-namespace rule (§1.1) means adding them is never a
breaking change for existing valid stories.
