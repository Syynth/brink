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
  format version byte) — the oracle ratchet (`RATCHET_EPISODE_COUNT`) and
  corpus report are the gate, unchanged.
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
carrier — on the **ink surface**, v1 recognizes exactly one name,
`effects` (`@[effects(pure, silent, total, reads(gold), …)]` — paren
clause grammar since the 2026-07-19 amendment, issue #1120), in exactly
one placement (the leading run at the top of a knot/stitch body, shared
with directive tag lines in either order). (§5c below extends both the
name set and the placement rule for the native `.brink` surface, which
has no tag-line spelling to share a run with.) Rules mirror the tag
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

### 5c. The channel on the native `.brink` surface (issue #1563)

The `@[…]` line is the native surface's *only* annotation channel — there
is no tag-directive spelling to alias, since `#` is the tag sigil there
too, so a native `#@name` tag is never consumed as a directive and never
erases: it always lowers as ordinary runtime tag content. That silence
used to be total (issue #1835): before `E172` (`hir::lower_native::
body::lower_tag`), a `#@name` tag compiled clean with no signal at all,
so an author porting a line from ink's tag-directive spelling got no hint
their `#@was(…)`/`#@local`/… had quietly become inert content. `E172` is
diagnostic only, never a recognizer in the ink sense — it does not
consume or erase the tag, only warns, naming the `@[name(…)]` annotation
equivalent when one exists (`was`, `allow`, `effects`) and saying plainly
when it does not (`module`, `public`, `private`, `local`). Two
differences from §5b, both following from native having real declaration
nodes:

- **Placement is Rust's**, not ink's: an annotation attaches to the
  declaration it immediately precedes (`@[effects(pure)]` on the line
  above `fn heal(…)`), with only trivia, doc comments, and further
  annotation lines allowed to intervene. Ink's top-of-body placement
  exists because a tag line above a knot header structurally belongs to
  the previous scope; native has no such problem.
- **The recognized name set is `effects`, `was`, `allow`, `element`,
  `convention`, and `style`.** `@[effects(…)]` attaches to a `flow`/`fn`
  head at either container level (top-level → `Knot.effects_assertion`,
  nested → `Stitch.effects_assertion`) and is checked by the same
  frontend-agnostic exceedance pass (`E103`/`E108`/`E109`) that judges
  ink assertions. `@[was("old::module::path")]` is the **file-level**
  module-rename record (§5 of `modules-spec`, issues #1286/#1355) and is
  recognized only as a direct child of the file. `@[allow(…)]` is
  source-level diagnostic suppression — §5d.

  `@[element(args = "…", block)]` (issue #1719, #2004) declares the
  `!name`-dispatched pattern on a `flow`/`fn` head: self-announcing,
  legal anywhere, carrying no `order` (a handler that names itself never
  competes for a line). `@[convention(claims = "…", order = N, block)]`
  (issue #1838, split out of `@[element]` into its own name by issue
  #2164's 2026-08-03 ruling) declares the natural-notation *claiming*
  pattern instead — a prose line carrying no `!name` sigil, reinterpreted
  as a call. Both must compile as a portable regex (`E159`), and both
  patterns' named captures must each bind a real parameter on the
  declaration (`E160`); `@[convention]` additionally requires the
  *converse* — every parameter must be bound by some named capture
  (`E167`), since a claimed line's rewritten call has no other source of
  arguments — except a `block`-flagged handler's trailing `content`-typed
  receiver (see below), which binds the captured run rather than a named
  group and is exempt from this check. A `@[convention]` parameter's
  declared type must be `string`, absent, or `content`, not `int`/`float`/
  `bool` or a struct — claiming always binds captures as strings, so a
  mismatched type makes the parameter unreachable (`E171`, issue #1849).
  A `block`-flagged `@[element(…, block)]` / `@[convention(…, block)]`
  annotation requires a trailing `content`-typed parameter on the
  declaration to receive the captured run (`E166`, issue #1839).

  **`order` is a required bare integer on `@[convention]`** (issue #2164,
  `docs/decision-log.md` 2026-08-03) — no default (`E178` if absent), no
  tie-break (`E179`, naming both declarations, if two handlers in one
  module share a value). It replaces the retired issue #1848 interim
  rule (top-level declaration order) as the claiming dispatch's
  precedence: the walk tries a module's claiming handlers in ascending
  `order`, first-match-wins. `@[element]` carries no `order` at all —
  no such clause exists in its grammar. (Issue #2352: the *projection*
  row `ConventionsProjection::dispatch` surfaces to editors still has an
  `order: i64` field for consistency of shape with a claim handler's own
  row, but it carries a different, synthetic meaning there — the input
  slice's array position, not an authored precedence value read off any
  clause. See `docs/prose-dialect-spec.md` §5.3 and
  `ConventionsProjection::dispatch`'s own doc.)

  **`attach = StructName` is an optional clause on `@[convention]`**
  (issue #2178, split from #2164's 2026-08-03 design-backport comment,
  `docs/decision-log.md` "The element output model") — the handler's
  declared **output schema**: a plain `struct` name, naming which keys
  the handler attaches to the run that follows and their types. Keys are
  declared (this clause); values are computed (the handler body) — no
  new declarative sub-language, since a `struct` is already declarative,
  statically known, serialized, and understood by compiler + editor +
  host. The declaration's own `: type` return annotation must name the
  same struct, or `E180` (declaration-surface-only, like every other
  check here — real name resolution of the struct name is out of scope).
  `@[element]` has no `attach` clause of its own — it is `@[convention]`
  only, the same asymmetry `order` already has.

  Two claiming handlers declaring byte-identical `claims` patterns is
  `E168` (issue #1848) — the narrow, sound slice of "these two patterns
  compete for the same line" this surface detects; see that code's own
  doc for what it does and does not catch. Two claiming handlers'
  patterns overlap, with the higher-`order` one provably subsumed by the
  lower, is `E170` (issue #1859) — a silent race where the lower-`order`
  pattern always wins, leaving the other handler dead code. `@[style(key
  = "value", …)]` (issue #1719) is a companion annotation requiring a
  paired `@[element]` or `@[convention]` on the same declaration
  (`E163`); each key must be `line`, `dispatch`, or one of the paired
  annotation's captures (`E162`), and a malformed clause list is `E161`.

  `@[effects]`, `@[style]`, and `@[element]` attach and lower at either
  container level, the same as each other — but `@[convention]` is legal
  only above a **top-level `fn`** (`E112` otherwise): the rewrite is an
  expression call, and a `flow`/nested `Stitch` is not callable as one.
  "Top-level" means a direct child of the file — a `fn` declared inside a
  `module { … }` block is **also** `E112` (issue #1847), even though it
  reads as un-nested by `flow`/`fn`-depth alone: the handler-collection
  pass that builds the claim dispatch table only ever scans the file's
  direct declarations, so admitting a module-nested claim there would
  validate and then silently register nothing to claim with. Even a
  validly-placed top-level `@[convention]` handler is further confined to
  **one file**: the conventions module `brink.toml`'s `[project]
  conventions` key (renamed from `elements` by issue #2180) names. A
  claiming handler declared anywhere else is `E169` (issue
  #1844, the 2026-07-31 §9.1 ruling's item 4) — `!name`-dispatched
  (`@[element]`) handlers have no such restriction, since they
  self-announce at the call site instead of silently reinterpreting prose.

Everything else is the reserved-namespace rule (§1.1) doing its job: an
unknown name is `E111`, a recognized name out of placement is `E112`, and
the grammar codes are shared with the ink recognizer (`E100` empty
assertion, `E101` malformed argument, `E048` duplicate). `@[style]` and
the `args`-spelled `@[element]` deliver only the declaration surface —
parse, validate, store on the `Knot`/`Stitch`. The `claims`-spelled
`@[element]` is different: issue #1838 delivers its dispatch too — a
claimed line is matched, its captures bind the handler's parameters by
name, and the line is rewritten in place to exactly one call on the
handler. The `!name` sigil dispatch rewrite the `args` spelling exists to
drive (matching a content line, binding captures, lowering to a call) is
**not** wired yet, nor is a per-*declaration* `@[was(old_name)]` rename:
both are ruled features awaiting their own slices, not annotation names
this channel may guess at.

### 5d. `@[allow(Exxx, …)]` — source-level suppression (issue #1161)

`@[allow(E151, E014)]` above a declaration or statement silences those
diagnostic codes for the whole span of whatever follows it — head and body
for a declaration, the single statement for a statement. Native surface
only: ink's `@[…]` placement is the top of a knot/stitch *body* and has no
ruled `allow` tenant, so ink authors keep the line-scoped `//brink-disable`
comment channel and the project `[lints]` table.

It attaches to *any* declaration or statement, not only the `flow`/`fn`
heads `@[effects]` requires: the scope is a `(span, codes)` fact recorded on
`HirFile::allow_scopes`, consumed by `brink_ir::suppressions::
apply_suppressions` — the same filter the comment channel already flows
through, so every consumer (CLI, LSP, wasm) applies it identically. The
attachment rule itself is generic — the next non-trivia sibling after the
annotation run, whatever node kind that is — so a content line, a divert, or
a conditional block is as valid a target as a `var`/`flow`/`fn` declaration.
The annotation line itself sits *outside* the scope it creates, so a
directive can never silence a diagnostic reported on itself.

Three rulings, each with tests:

1. **Only warnings are suppressible.** A code whose default severity is
   `Error` is rejected with **`E154`**. An error means no correct artifact
   can be produced, so silencing one would be a way to ship broken code.
   This matches the `[lints]` table's own hard-error exemption (#1160)
   rather than inventing a second, differently-shaped "which errors are
   safe to relax" policy. The B0.3 admission-validator diagnostics
   (`E121`–`E128`) are exempt as the issue requires, twice over: all are
   `Error`-severity, *and* admission output never routes through
   `apply_suppressions` at all.
2. **A source `allow` beats a project-level `deny`.** Suppression runs
   before `effective_severity` at every call site, so `@[allow(E151)]`
   removes the diagnostic even under `[lints] E151 = "deny"` or
   `deny-warnings = true`. The annotation is the more specific,
   deliberately-authored, reviewable statement, and `brink.toml` has no way
   to name a single declaration. Suppressibility is still judged on the
   code's *default* severity, which no `[lints]` entry can change — `deny`
   cannot make a warning-tier code unsuppressible, and `allow` cannot make
   an error-tier one suppressible.
3. **A suppression that does nothing is always loud.** An argument that is
   not a known diagnostic code is **`E153`** (the reserved-namespace rule,
   §1.1, applied to arguments — a typo'd suppression that silently no-ops
   is the exact failure the `@` namespace exists to prevent); a missing,
   empty, or non-identifier argument list is **`E155`**. One bad argument
   discards the whole directive: a partially-applied suppression would
   silence some codes while the author believes all of them are handled.

## 6. Future tenants (non-normative)

The channel is designed for (not implemented): `@world` (stitch-level
opt-out), type signatures (`@returns(int)`, typed params/VARs — the
`signature(def)` query carrier for the #397 pipeline), intl
(`@notranslate`, `@note(…)`, `@maxlen(n)`, stable line ids `@id(…)` — these
need the line-target placement, deliberately excluded from v1),
save-migration (`@renamed-from(old)`), `@deprecated`. (Lint control,
`@allow(…)`, has since shipped on the native surface — §5d.) Each lands as
new recognizer cases + its own consumption
logic; the reserved-namespace rule (§1.1) means adding them is never a
breaking change for existing valid stories.
