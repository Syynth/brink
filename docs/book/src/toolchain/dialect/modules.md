# Modules

By default a brink project is one flat namespace: every `.ink` file glued in
with `INCLUDE` contributes its knots, functions, and globals to a single shared
pool, and any name can reach any other. That is exactly strict ink, and it stays
byte-for-byte unchanged. Modules are the **opt-in** layer on top: a way to draw
boundaries so a large story's parts can name things freely inside themselves
without colliding with — or accidentally depending on — everything else.

Nothing on this page turns on until you write a `#@module` directive. A project
that never mentions one behaves precisely as it always has.

## The module unit: `#@module`

Every file is already a module, named by its file stem: `quest_3.ink` is the
module `quest_3`. That is the zero-ceremony default and it is *undeclared* — a
permeable member of the legacy pool.

Writing `#@module(name)` at the top of a file **declares** the module:

```ink
#@module(quest)

=== ambush ===
The bandits spring from the treeline.
-> DONE
```

Declaring a module does two things. It names the module explicitly (so several
files can share one name — see below), and it opts the file into the
**declared-module defaults**, the most important of which is that definitions
become private unless you say otherwise (covered under
[Visibility](#visibility-public-and-private)).

A multi-file module is always deliberate. Either every file carries the same
`#@module(name)`:

```ink,ignore
// quest_intro.ink
#@module(quest)

// quest_boss.ink
#@module(quest)
```

…or an included file inherits its includer's module. A file with no `#@module`
of its own that is `INCLUDE`d under a declaring head file joins that head's
module **and** its visibility default.

One footgun is closed by a hard error: an *undeclared* file whose stem happens
to collide with a *declared* module's name (`shared.ink` next to a
`#@module(shared)`) is a compile error. Accidental membership with mismatched
defaults is the one dangerous case, and a single diagnostic kills it.

## Imports

Once a module is declared, its names stop leaking across the boundary. A name
crosses into another module **only** through an `IMPORT`. Inside a module,
everything stays bare — you never qualify a same-module reference.

There are two spellings.

**Bare import** brings specific names into local scope, optionally binding an
extra local name to it with `AS`:

```ink,ignore
IMPORT { ambush, guard_talk AS gt } FROM quest_3

=== square ===
-> ambush          // used bare
{ gt() }
```

`AS` is **additive**, not a rename: `guard_talk` stays resolvable under its
own name alongside the alias `gt` — unlike Rust's `use … as`, which drops the
original binding.

**Qualified import** brings the module in under its own name; its exports are
then reached through a dotted path:

```ink,ignore
IMPORT quest_3

=== square ===
-> quest_3.ambush.start
```

The importable set is every top-level public definition: knots, functions,
`VAR`s, `CONST`s, `LIST`s, and `STRUCT`s. Stitches are not directly importable —
they are reachable only through the qualified form (`quest_3.ambush.start`).

A few rules keep imports unambiguous:

- **No globs.** `IMPORT *` does not exist.
- **Ambiguity is an error.** If `x` names both an imported module and a visible
  definition, a qualified `x.y` is a compile error. Resolve it with an alias —
  brink never silently picks a winner.
- **A dotted `a.b` is module-qualified only if `a` was imported as a module in
  *this* file.** The reader checks the file's own header; it never guesses from
  elsewhere in the project.

## Visibility: public and private

Who may *reference a name* across a module boundary is its visibility. The
default flips on whether the module is declared:

| Module kind | Default visibility |
| --- | --- |
| Undeclared stem-module (legacy pool) | **public** |
| Declared `#@module` | **private** |

That flip is what keeps the pre-modules world unchanged — every legacy
definition is public — while making a freshly declared module encapsulated by
default. Override the default per definition with `#@public` / `#@private`,
written just under the header:

```ink,ignore
#@module(quest)

=== ambush ===
#@public
The bandits spring from the treeline.
-> DONE

=== plan_ambush ===
#@private          // internal helper, never imported
~ return roll_initiative()
```

Restating the default (a `#@public` on an already-public definition, or
`#@private` in a declared module on something already private) is a redundant
override and draws a warning — the directive is there to *change* the default,
not to decorate it.

The host — the engine embedding the runtime — sits outside every module, so it
sees only public names (with a development-time override for debugging).

## Renames and `#@was`

Module and definition names are identity: a compiled story, a save file, or a
late-loaded chunk refers to a knot by a name-derived id. Renaming a public name
would ordinarily break every one of those references. `#@was` is the migration
door:

```ink,ignore
#@module(quest)
#@was(quest_three)          // this module used to be `quest_three`

=== ambush ===
#@public
#@was(the_ambush)           // this knot used to be `the_ambush`
-> DONE
```

A `#@was(old_name)` records a former name. The alias travels into the compiled
artifact, so a save or a dynamic link that still names `the_ambush` rehydrates
onto `ambush` instead of faulting. `#@was` takes exactly one non-empty
old-name argument, and it must differ from the definition's *current* name
(naming yourself migrates nothing — that's a diagnostic).

The editor's **Rename** refactor writes `#@was` for you: renaming a knot,
stitch, `VAR`, `CONST`, or `LIST` — from the CLI (`brink ide rename`), the LSP
(F2), or the studio's rename-safe path — stamps `#@was(old_name)` onto the
declaration automatically, in the same edit set as the rename itself. It only
fires under `dialect = brink` (`#@was` is itself a brink extension — under
strict ink it would be rejecting its own migration door), and it never
overwrites an existing `#@was`, so re-renaming an already-migrated
declaration keeps its original record rather than losing the chain back to
the name a save might still carry.

A rename that never goes through that machinery — a hand edit, a `sed`, a
merge — still needs `#@was` added by hand, same as before. The editor helps
here too: it diffs each file's declared names against the previous compile,
and when a name disappears while exactly one same-kind name appears in its
place, it surfaces a hint — *"`hub` disappeared and `plaza` appeared — did
you rename it?"* — pointing at the exact `#@was` to add. This is not the
fuzzy load-time rematching this page's alias table deliberately avoids: it
never resolves anything on its own, it only asks, at authoring time, while
you still remember what you meant. A rename that never passes through brink
tooling at all (so nothing is there to diff) stays undetected — that residual
gap isn't solved, only narrowed.

## Editor support

Modules come with IDE guarantees so the boundaries help rather than nag:

- **Auto-import quick-fix.** Reference a public name that lives in another
  module without importing it, and the out-of-scope diagnostic offers a
  one-click *"Import `name` from `module`"* fix that inserts the `IMPORT` line in
  the right place — below any existing import block, else below the `INCLUDE`
  block, else at the top under the `#@module` header.
- **Rename writes `#@was`.** See above — every rename surface stamps the
  migration directive automatically.
- **Undeclared-rename hint.** See above — a same-kind name that vanishes and
  reappears prompts a quiet, non-blocking question rather than staying silent.
- **Folding.** A run of two or more leading `IMPORT` statements folds into a
  single `IMPORT … (N modules)` region, mirroring the `INCLUDE` block fold.
- **Formatting.** `brink fmt` canonicalizes `IMPORT` spacing —
  `IMPORT {  a , b  AS c } FROM  m` becomes `IMPORT { a, b AS c } FROM m`.

## Compatibility

Every trigger on this page — `#@module`, `#@public`, `#@private`, `IMPORT`,
`#@was` — is a construct no strict-ink or existing brink story contains. Import
enforcement only ever *adds* diagnostics, and it keys off a *declared* target
module, which the entire pre-modules corpus lacks. A plain multi-file `INCLUDE`
project with no `#@module` anywhere remains one big public pool that resolves
exactly as it did before modules existed.
