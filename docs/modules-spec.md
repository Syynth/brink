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
  2026-07-27)**: `IMPORT { barter } FROM story::market` /
  `use story::market::barter;` checks `barter` against **both**
  readings independently — an item `story::market` publicly exports,
  and a declared submodule `story::market::barter` in its own right —
  and licenses whichever holds. **Both may hold at once, with no
  precedence between them**: the item is bare-importable under its
  own name, and (if `barter` is also a declared submodule) that
  submodule's public exports become bare-referenceable too, exactly
  as an explicit qualified import of it would grant. The well-formedness
  check (`E088`) only fires when the trailing segment resolves as
  **neither**. A parent module importing its own declared child
  submodule this way (`story::market` writing
  `use story::market::barter;`) is not a self-import (`E090`) — it is
  the mandatory spelling the import-required gate demands to reference
  the child's exports; self-import (`E090`) still fires for the
  qualified form (`IMPORT story::market;` from within `story::market`)
  and for the leaf-item form naming the importing file's own module
  outright (`use story::market::barter;` from inside
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
`story::std…`-mounted module's items are invisible to bare-name
resolution regardless of any `pub`/`#@public` marking, until an
explicit `use std::…` import exists (#1582/#2167, not yet built) —
this is a resolution-layer gate (`brink-analyzer::resolve::
lookup_by_name_direct`), not a visibility default, and does not
change once those marks are added. Issue #2216 unified the
scope-free UFCS-receiver lookup (`resolve::lookup_unique_by_name`,
which has no `ImportScope` to classify against) onto the same gate,
so a std-mounted candidate is invisible there too, including as the
sole match — the two lookups can no longer disagree about std
visibility.

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
