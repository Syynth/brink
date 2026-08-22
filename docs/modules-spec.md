# Modules & visibility — namespaces, imports, and the rename door

Status: **draft for ratification** (#719 round, rulings 2026-07-14 —
see the decision-log entry of that date). Sections marked **RULED**
transcribe the round; **PROPOSED** details ratify at this PR's
review. Companions: `docs/directive-annotations-spec.md` (the `#@`
channel), `docs/effects-spec.md` §10 (`#@private`'s effects
consequence, ruled there; full semantics live here),
`docs/scoped-flow-state-spec.md` (`#@local` residence), #717
(dynamic linking — the late-loaded half of this design).

## 1. The module unit — RULED

- **File-as-module by default**: every `.ink` file is a module named
  by its stem. Zero ceremony, zero breakage.
- **`#@module(name)`** at the top of a file *declares* the module —
  naming it explicitly and opting into the declared-module default
  (§4).
- **Multi-file modules are always deliberate**: either several files
  carry the same `#@module(name)`, or files are INCLUDE-glued under a
  declaring head file — included files inherit the includer's module
  AND its visibility default.
- An *undeclared* file whose stem collides with a *declared* module's
  name is a **compile error** (accidental membership with mixed
  defaults is the one footgun; one diagnostic kills it).

## 2. Imports — RULED (grammar spelling PROPOSED)

Names cross module boundaries **only via import**. Within a module,
everything is bare.

```ink
IMPORT { ambush, guard_talk AS gt } FROM quest_3   // bare use: -> ambush
IMPORT quest_3                                      // qualified: -> quest_3.ambush.start
```

- **Importable set**: all top-level public definitions — knots,
  functions, VARs, CONSTs, LISTs, STRUCTs. Stitches are reachable
  only through the qualified form.
- **No globs** (`IMPORT *` does not exist).
- **Ambiguity is an error**: if `x` is both an imported module name
  and a visible definition, qualified `x.y` is a compile error —
  fixed with an alias. No silent precedence.
- **`AS` is additive, not a rename**: `guard_talk AS gt` binds `gt`
  as a *second* local spelling of `guard_talk` — the source name
  stays resolvable too. This departs from Rust's `use … as`, which
  drops the original binding (issue #1590).
- A bare `a.b` can only mean module-qualified access if `a` was
  imported as a module *in this file* — the reader checks the file
  header, never guesses.
- **Auto-import is an IDE guarantee**: referencing an out-of-scope
  name offers a quick-fix inserting the IMPORT; completion offers
  out-of-module names with the import edit attached; rename keeps
  import lines coherent (the INCLUDE-rewrite machinery precedent).
- **A bare import's trailing segment dual-reads (RULED, issue #1592,
  2026-07-27; corrected 2026-08-05, issue #2287)**: `IMPORT { barter }
  FROM story::market` / `use story::market::barter;` checks `barter`
  against **both** readings independently — an item `story::market`
  publicly exports, and a declared submodule `story::market::barter`
  in its own right — and licenses whichever holds. **Both may hold at
  once, with no precedence between them**: the item is bare-importable
  under its own name, and (if `barter` is also a declared submodule)
  that submodule's public exports become **qualified-accessible**
  (`barter.ambush`/`barter::haggle`, per dialect) — exactly as an
  explicit qualified import of it would grant, and matching Rust's own
  `use a::b;` (`b` becomes nameable as `b::item`, never brings `item`
  into bare scope). **Never bare**: importing the module does not, by
  itself, bring any of its members' bare names into scope — only a
  symbol-level or glob import of a specific member does that (§2's
  "importable set" above). Issue #2287 corrected this bullet and its
  implementation, which had drifted into granting bare access as well
  — the exact defect the issue reported (`-> haggle` resolving after
  only `use story::market::barter;`, with no `barter::` qualifier
  anywhere in source). The well-formedness check (`E088`) only fires
  when the trailing segment resolves as **neither**. A parent module
  importing its own declared child submodule this way (`story::market`
  writing `use story::market::barter;`) is not a self-import (`E090`)
  — it is the mandatory spelling the import-required gate demands to
  reference the child's exports; self-import (`E090`) still fires for
  the qualified form (`IMPORT story::market;` from within
  `story::market`) and for the leaf-item form naming the importing
  file's own module outright (`use story::market::barter;` from inside
  `story::market::barter` itself). **Aliasing a trailing segment that
  resolves as a module has no representation** (`AS`/`as` renames one
  local binding, not a whole export set) and is rejected the same way
  the single-segment module-alias form is (native's `use a as m;`) —
  loud, never silently dropped.

**PROPOSED spellings**: `IMPORT`/`FROM`/`AS` as shown (declaration
position, top of file, after any `#@module`); duplicate import of
the same name = error; self-import = error.

## 3. INCLUDE's role — RULED

`INCLUDE` becomes **intra-module file glue**: multiple files, one
module, flat visibility among them — its existing behavior,
reframed. It stays **ungated** (identical semantics in both
dialects); a legacy project is one big default-public module where
everything works untouched. `IMPORT`, `#@module`, `#@private`,
`#@public` are brink-dialect-gated (E051-class rejection under
strict-ink, superset-parse as always).

## 4. Visibility × residence — RULED

Two orthogonal axes share the directive channel:

- **Visibility** (compile-time): who may *reference the name*.
  public / `#@private` (module-internal; **the host is outside every
  module**).
- **Residence** (runtime): who *owns the state instance*.
  world / `#@local` (per-flow instance; for knots, per-flow visit
  counts — shipped since #473).

All four combinations are legal and meaningful; directives stack on
one declaration. Knots take both axes.

**Defaults — declaration flips them**: a *declared* module
(`#@module` present anywhere in it) defaults **private**
(`#@public` overrides per definition); an *undeclared* stem-module
defaults **public** (`#@private` overrides). Declaring a module is
the single deliberate gesture that opts into encapsulation; casual
ink stays open.

**This section describes the ink dialect's directive spelling.** The
native surface (`docs/native-surface-charter.md` §13.2) is always a
*declared* module (identity is filesystem-derived) and therefore
always defaults private, with no undeclared-stem-module case; it
opts a declaration into public with a `pub` keyword rather than an
`#@public` tag directive (issue #1582, RULED 2026-08-03). Both
spellings produce the identical `VisibilityMark::Public` this
section's `effective_visibility` logic consumes.

**Stdlib mount carve-out (issue #2197, per #2080's scope fence):** a
`std…`-mounted module's items are invisible to bare-name
resolution regardless of any `pub`/`#@public` marking, until an
explicit `use std::…` import exists (#1582/#2167, not yet built) —
this is a resolution-layer gate (`brink-analyzer::resolve::
lookup_by_name_direct`), not a visibility default, and does not
change once those marks are added. Issue #2216 unified the
scope-free UFCS-receiver lookup (`resolve::lookup_unique_by_name`,
which has no `ImportScope` to classify against) onto the same gate,
so a std-mounted candidate is invisible there too, including as the
sole match — for any referrer *outside* the std tree, the two
lookups can no longer disagree about std visibility. A referrer
*inside* the std tree is the one remaining asymmetry:
`lookup_by_name_direct`'s `InScope` tier still resolves a
std-mounted candidate for a file that is itself part of
`std…` (so std's own internal references keep working), but
`lookup_unique_by_name` has no scope to classify `InScope` under and
excludes that same candidate unconditionally — stricter than
`lookup_by_name` for exactly that referrer.

**A referrer's `ImportScope` is derivable only from the resolved
`ModuleMap`, never from `hir.module` alone (issue #2272's gate finding).**
`hir.module` only ever carries an explicit `#@module(...)` directive; a
**native** `.brink` file's real declared module is *path*-derived, computed
once via `ModuleMap`/`brink-db`'s `module_map_query` —
`analyze_with_modules`'s own doc states `hir.module` "carries a
deliberately empty `name`" for native source, never `None`. Building an
`ImportScope` from `hir.module` in isolation therefore silently excludes a
native file's own siblings, including the mounted stdlib's own internal
same-module references (`std/conventions/screenplay.brink` referencing its
own `Cue`/`Parenthetical` structs) — a per-file-local re-derivation that
looked `brink-analyzer`-only in isolation misfired exactly there. Every
consumer of `ImportScope` in this codebase (`resolve_query`'s own
scope-build, `annotations::check`'s referrer-scoped `E061`,
`effects_assertion_diagnostics_query`'s clause resolution) must build it
from the project's real `ModuleMap` — `brink-db`'s `module_map_query` on
the db-direct road, the per-file scope `analyze_with_modules`'s own
resolution loop builds and threads onward on the off-db road — never from
a file's own HIR alone.

**`std` is a peer root of `story`, not a child of it (issue #2245, RULED
2026-08-04; generalized to a set of reserved roots by issue #2251):**
`story::*` is the universe of what the project *author* provided; `std::*`
— and every future mounted library — is a top-level peer of `story`,
never nested under it. `brink_db::modules::native_module_path` mints
`std::conventions::screenplay` for the mounted preset, never
`story::std::conventions::screenplay`; a file's leading root-relative path
segment decides which root it qualifies under, a structural fact rather
than a project-config lookup — checked against `brink_ir::RESERVED_ROOTS`,
not a single hardcoded literal. `is_std_module` followed suit: it used to
be a `story::std…`-prefix string test duplicated independently in
`brink-analyzer::resolve` and `brink-ir::lir::lower::decls` (those crates
cannot share a helper without a dependency cycle in the direction
`brink-ir` → `brink-analyzer` — but the *reverse* edge already exists,
since `brink-analyzer` depends on `brink-ir`). It now lives once, in
`brink-ir::symbols` (the substrate `SymbolIndex`/`ResolutionMap` already
live in, precisely so `brink-ir::lir` can consume analyzer-shaped data
without depending on `brink-analyzer`), and both former copies were
deleted in favor of the shared helper.

Issue #2245's fix (PR #2250) shipped that helper as a single `&str`
constant (`STD_ROOT`) and a check that answered only "is this the std
root?" — correct for the one library mounted at the time, but unable to
answer the ruling's general form ("and every future mounted library")
without either re-deriving the check by hand at a second call site or
silently falling through to the `story` branch for an unrecognized root.
Issue #2251 generalized it: `brink_ir::RESERVED_ROOTS` is now the *set*
of reserved peer-root names (`&["std"]` today), and `brink_ir::
is_reserved_root_module` (renamed from `is_std_module`, since none of its
three call sites — the `Candidacy::Other` exclusion in
`brink-analyzer::resolve::lookup_by_name_direct` and
`::lookup_unique_by_name`, and `brink-ir::lir::lower::decls::
lookup_global`'s unscoped fallback — were ever std-*specific*; each
excludes "any reserved-root candidate this file doesn't itself belong
to") checks membership against the whole set. A second mounted library
becomes a one-line addition to `RESERVED_ROOTS`, not a new branch at any
consumer. This intentionally stops short of a per-root visibility
*policy* type: with one root mounted, there is no second data point to
generalize a differing policy from, so no policy hook was added. That
does not mean no policy decision was made — generalizing a single
`std`-specific check into a set-membership test bakes in the decision
that every future [`RESERVED_ROOTS`] member inherits `std`'s
bare-name-fallback exclusion identically, with no per-root opt-out. A
root that needs *different* visibility behavior than `std` still needs
the policy type this section declines to add.

Issue #2217 (open) — a project's own file legitimately placed at a
`std/…` path is misclassified as the mounted preset, because the gate is
purely path-shape-based (leading segment, not embedded-vs-project origin)
— is **not** resolved by this generalization: the gate is still exactly
as path-shape-based as before, now against a set of shapes instead of
one. #2251 did not touch that axis.

**A third participant (issue #2238), narrowed by issue #2246:** LIR
struct-shape resolution joined the std-invisibility gate so that a
project's own struct and a mounted std preset's same-named struct can
coexist without one silently claiming the other's shape id. Issue #2246
found that **a struct construction literal's shape name is a HIR
reference the analyzer already resolves** (`resolve::resolve_struct_ref`,
full `Candidacy`/module-scope semantics, into the same `ResolutionMap`
every other reference kind uses) — lowering (`expr::lower_struct_literal`,
`decls::eval_const_struct_literal`) was re-deriving the answer instead of
consuming it. For a construction literal, then, the coexistence guarantee
above is now delivered by `brink-analyzer::resolve::lookup_by_name_direct`
itself, the same lookup the rest of this section describes — **not** by
`ShapeTable::resolve`/`decls::lookup_global`, which construction literals
no longer call at all.

**Closed by issue #2249:** the paragraph above used to end here with
`ShapeTable::resolve` (via `decls::lookup_global`) as the remaining
resolution path for the cases with no HIR reference to consume — a struct
field's declared type, and a `VAR`/`CONST`/`temp` TM-2 type annotation
(`project.rs`'s own doc used to read: "Field `TypeExpr`s never register
unresolved refs… a nominal-only grammar, resolved later by a different
mechanism"). Issue #2249 closed that gap the same way #2246 closed the
construction-literal one: `symbols::project`'s walk now registers a
`RefKind::Type` reference for a field's/annotation's `Named` leaf, resolved
by a new `resolve::resolve_type_ref` (identical `lookup_by_name`,
`ImportScope`-aware machinery — `SymbolKind::Struct` only). Lowering
(`build_shape_table`'s field loop, `build_struct_shape_data`'s identical
loop, `structs::record_global_annotation`,
`context::LowerCtx::record_temp_annotation`) consumes that recorded
resolution directly; `ShapeTable::resolve` had no production caller left
and was deleted. The `lookup_global`/`lookup_by_name_direct` asymmetry this
section describes is therefore now **closed** for this reference kind too
— a std file referencing a *sibling* std struct in a type annotation, with
no import, resolves via the `InScope` tier exactly like every other
reference kind, where it previously could not. `resolve::
lookup_unique_by_name`'s own analogous asymmetry (issue #2233, a
`referrer_module` hint) remains the one unreconciled case in this family.

**Closed by issue #2272.** The paragraph above used to end here, describing
`annotations::check`'s `E061` "unrecognized type name" diagnostic
(`annotations::declared_struct_names`) as project-flat with no
referrer-scoping or std-exclusion of its own — an unresolvable-but-silent
`~ temp c: Cue` (std-only `Cue`, no import) raised no diagnostic anywhere,
compounding the gap `resolve_type_ref` (issue #2249) left open by design
(a `TypeExpr::Named` leaf isn't always a struct reference, so
`resolve_type_ref` itself stays silent on a miss). Issue #2272 closed it:
`check_one`'s bare-`Named` arm now routes through the identical
`ImportScope`/`Candidacy`-aware `lookup_by_name` `resolve_type_ref` already
uses (`SymbolKind::Struct` only) instead of consulting `names.structs`
(still project-flat — unchanged consumer: `resolve`'s own `Ty::Struct`
resolution and `structs::declared_shapes`'s field-type resolution both
still want "declared anywhere"). "Declared" for `E061`'s bare-`Named` check
now means declared **and reachable from this referrer**, not merely
declared somewhere in the project — when a name is declared in the project
but out of scope, the diagnostic now names the module it lives in rather
than staying silent or falling back to the generic "unrecognized type"
message. `names.lists`/`names.handles` (the `List<L>`/`Handle<K>` arms)
stay project-flat — `resolve_type_ref` never scopes those vocabularies
either, so there is no referrer-scoping precedent for #2272 to mirror
there.

**Two sibling lookups audited and found not to fit the `RefKind` pattern
(issue #2249):** `lir::lower::decls::collect_externals`'s extern-to-
fallback-`fn` pairing and `context::LowerCtx::lookup_address_id`'s
locally-declared-label addressing are both genuine self-declaration
lookups — the pairing/existence check is inferred by the compiler from two
declarations' matching names (or a scope-qualified label's own
declaration), never resolved from a path the *user* wrote at that call
site the way a divert target, a variable read, or a type annotation is.
There is no HIR reference to hang a `RefKind` on at either site; both
remain on `decls::lookup_global`, unchanged.

**Correction to the record:** `docs/decision-log.md`'s 2026-08-04 "peer
roots" entry lists `ShapeTable::resolve`'s fast path among five sites
"each independently taught to skip std" — issue #2246 found this false for
that one site. The fast path (`if ids.len() <= 1 { return ids.first()...
}`) returned a bucket's sole candidate unconditionally and **never called
`lookup_global` at all**, so a struct name only a mounted std module
declared, with no project-side homonym, resolved straight through with no
import — the fast path had not, in fact, been taught to skip std. #2246
fixed it to always route through `lookup_global`, whether the bucket holds
one candidate or many.

**The gate is root identity, not a provenance channel (issue #2217):** the
std-invisibility gate excludes a candidate whenever its module's root is
`std` ([`is_std_module`](../crates/internal/brink-ir/src/symbols/roots.rs)),
identically whether that module is the *embedded* stdlib mount or a
project's own file that legitimately lives at a `std/…` path —
`mount_stdlib`'s "project source at the same key wins" carve-out means both
mint the exact same `std::…` module identity via `native_module_path`, by
design, so there is no separate "which one is this really" fact to consult.
Reconsidering `is_std_module` to distinguish the two would undo the "peer
roots" ruling above, not fix a bug in it. What issue #2217 actually found
missing was diagnosability: an author who places their own code under
`std/` (with no intent to override any embedded module) got a bare
"unresolved name" `E024`/`E025`/`E068` with nothing pointing at the rule
responsible. `brink-analyzer::resolve::unresolved_diag` now checks whether
the unresolved name has any declaration at all under the `std::` root
(`is_std_shadowed_name`) and, if so, appends a hint naming the peer-root
rule and the `use std::…` path forward — same diagnostic code, no new one
allocated, resolution behavior unchanged.

**Boundary rules** (keeping the axes from leaking):

1. **`#@private` hides the name, not the cell.** A private
   world-cell still ships in effect rows — two flows in the same
   module's knots genuinely conflict on it and the scheduler must
   see it. Rows are scheduling metadata, not an access surface.
2. **Host semantic access respects visibility; host persistence
   machinery sees everything.** Variable get/set, entry lookup,
   `begin_function_eval` on a private definition → refused (the
   load-error class ruled in effects-spec §10). Save/load/journal/
   replay serialize the whole state including private cells —
   persistence is not access, and pause/resume must hold.
3. **Dev tooling gets a documented visibility override** (the
   play-from-here affordance): editors/debug hosts may start flows
   at private knots; production hosts respect visibility. This is a
   host capability, not a language switch.

## 5. Identity and renames — RULED (alias-table encoding PROPOSED)

- **Identity stays name-hashed**: `DefinitionId` = hash of
  `(module, name)`. Imports and aliases are consumer-side and never
  affect identity. Moving a file does NOT break identity when the
  `#@module` name (or stem) rides along — the door to guard is
  *renaming the module* (or a definition).
- **`#@was(old_name)`** on a module or definition records the
  rename. The compiler ships an **old→new DefinitionId alias table**
  in `.inkb` (compiled-declarations pattern; its own section,
  section-locally versioned like `EffectRows`). Rehydration consults
  the table **on the miss path only**: saved fn tokens, divert
  values, and visit-count keys rebind deterministically.
- The rehydration fault message teaches the fix: *"saved token
  `quest_3.ambush` resolves to nothing; if `quest_3` was renamed,
  add `#@was(quest_3)`."*
- **IDE rename writes the directive automatically** (module and knot
  renames both go through the #305/#306 rename machinery).
- The directive is **deletable** after a shipped migration window.
- This retrofits to knots immediately: knot renames silently break
  saves *today*; `#@was` on a knot fixes the pre-existing hole with
  the same machinery.
- **Rejected**: content-hashed identity (breaks on every edit),
  permanent GUIDs (hostile to text-first merging), fuzzy load-time
  rematching (silent-garbage risk).

## 6. Interactions — RULED direction, details ride their own tracks

- **Effects/capabilities**: a module's aggregate row + its import
  list is a legible coupling audit; module boundaries are natural
  capability boundaries (T2 trust tiers).
- **Strict-mode boundaries** gain the third leg ruled in the
  typed-mode round's terms: module *exports* join host-callable
  functions and `#fn` targets as annotation-required surfaces
  (#657's types half resolves here — a public def in a declared
  module is a boundary).
- **Incremental compilation**: imports are declared dependency
  edges; the FG substrate narrows per-module resolution scopes to
  them (replacing all-files edges where imports exist).
- **Dynamic linking** (#717): `IMPORT` lists are the import table;
  declared modules with explicit exports are the one-definition-rule
  unit. This spec is the static half; nothing here forecloses the
  late-loaded half.

## 7. Diagnostics — PROPOSED

Allocating from the next free code (verify by grepping the registry
at implementation): private-reference-across-modules (with
did-you-mean-IMPORT / is-#@private note), unresolved import,
duplicate/self import, module-vs-definition qualified ambiguity,
stem-collision-with-declared-module, `#@was` target unknown
(warning: nothing to migrate), `#@public`/`#@private` in the wrong
default context (warning: redundant).

## 8. Testing — PROPOSED

- Oracle ratchet byte-identical (vanilla ink sees none of this;
  INCLUDE semantics unchanged by construction — prove with the
  strict-ink corpus gate).
- tier1-brink corpus wing: cross-module diverts/calls/imports of
  every importable kind, private rejection, both defaults, INCLUDE
  inheritance, qualified stitch access, ambiguity error, `#@was`
  save → rename → load rebinding end-to-end (the flagship case:
  saved fn token + divert value + visit counts across a module
  rename).
- Property: identity stability under file moves; alias-table
  round-trip through inkb/inkt.
- IDE: auto-import quick-fix, rename-writes-#@was, breakage report
  on cross-module rename.

## 9. Sequencing — PROPOSED (spine — single reviewed agents, oracle-gated)

1. **M-1 name model**: module identity in the symbol tables,
   `(module, name)` DefinitionId hashing, `#@module` directive,
   stem/declared rules + collision diagnostic. (Identity change =
   the one genuinely delicate slice; checked-in `.inkb` artifacts
   regenerate.)
2. **M-2 imports + visibility**: IMPORT grammar/HIR/resolution,
   `#@private`/`#@public`, defaults, all §7 diagnostics, host
   semantic-access enforcement + dev override.
3. **M-3 renames**: `#@was`, alias-table section, rehydration
   miss-path lookup, fault message.
4. **M-4 tooling tail** (pump wave): auto-import quick-fix +
   completion edits, rename integration, book chapter ("Modules"),
   fmt/folding polish.
