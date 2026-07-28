# Root-content `DefinitionId` identity — findings and recommended fix shape

**Issue:** [#1504](https://github.com/Syynth/brink/issues/1504)
**Status:** **SHIPPED.** The [primary recommendation](#primary-qualify-root-content-scope-paths-by-the-owning-file-not-the-module)
below is what landed: `hir::root_content_scope_path` qualifies a file's
anonymous root-content scope path with that file's project path, and
`IdAllocator::set_path_prefix` carries the same qualifier through LIR
lowering so the synthesized terminus is content-derived too. Every
acceptance test named below now runs unignored. Two entries in
[Suggested follow-ups](#suggested-follow-ups) also landed: the codegen
duplicate-`DefinitionId` guard (#1673) and the corpus case, added at
`tests/tier1/includes/root-weave-in-entry-and-included-file` — tier 1 per the
ruling on #1504, rather than the tier 3 this document originally suggested.
**Baseline:** every measurement below was taken at commit `999581354`
(`main`, workspace version `0.0.11`), i.e. *before* the fix. Line/column
coordinates quoted throughout are from that commit and have since moved.

## TL;DR

Both findings in #1504 reproduce. One of them is worse than the issue says.

- **(a) is not a latent landmine — it is a live, silent miscompile.** Any ink
  project where the entry file *and* an `INCLUDE`d file both carry root-level
  weave content compiles to a program with duplicate container ids. The
  linker's address map is last-write-wins, so **picking a choice from the
  included file runs the entry file's choice body instead.** Reachable through
  the ordinary `brink_compiler::compile` entry point with a plain `INCLUDE` —
  no unusual flags, no native dialect, no incremental session.
- **(b) reproduces**, but not the way the issue frames it. `discover`'s BFS
  always seeds the entry first, so an ordinary from-scratch compile mints the
  entry `FileId(0)` regardless of how many files it `INCLUDE`s — file *count*
  is not the trigger. The reachable trigger is file *registration order*: an
  editor/LSP session that admits an `INCLUDE` target before the entry file
  mints a different id space than a real compile of the same tree. Also not a
  break of the FG-4d `incremental ≡ from-scratch` gate. See
  [Correcting the framing of (b)](#correcting-the-framing-of-b).
- **#1504 does not have to wait for #1442.** The collisions in (a) are entirely
  in *anonymous* weave containers, whose identity must stay structural. #1442's
  rename-invariance question is about *named* definitions. These are separable
  layers; see [Recommended shape](#recommended-shape).

## Evidence

Four acceptance tests shipped with this analysis. They assert the behavior
brink *should* have and were `#[ignore]`d while the fix was design-gated;
un-ignoring them was the acceptance criterion for #1504, and they now run
unconditionally.

- `crates/internal/brink-ir/tests/lir_lowering/root_content_definition_id_soundness.rs`
- `crates/brink-compiler/tests/issue_1504_root_content_identity.rs`

A fifth, added in review follow-up, demonstrates the *reachable* form of (b)
(editor/LSP file-registration order, not `INCLUDE` count — see
[Correcting the framing of (b)](#correcting-the-framing-of-b)):

- `crates/internal/brink-driver/src/discover.rs`'s
  `root_content_ids_agree_between_discover_and_editor_order`

Run the first four with `--ignored` at `999581354` — before the fix — and all
four fail. Verbatim:

```
duplicate container ids in the compiled program: [
    "0x1779765f903c98e appears 2x",
    "0x1dde84850f175fb appears 2x",
    "0x1ef2ee91775101d appears 2x",
]

picking `inc one` ran the wrong choice body; got: "main one\nMAIN-ONE-BODY\nmain gathered\n"

two files' root weaves share DefinitionIds: [
    "Address(0x779765f903c98e) -> [\"c-0\", \"c-0\"]",
    "Address(0xdde84850f175fb) -> [\"c-1\", \"c-1\"]",
    "Address(0xef2ee91775101d) -> [\"g-0\", \"g-0\"]",
]

the root terminus address moved when the entry's FileId assignment shifted
  left: Some(Address(0x49590a9a660758))
 right: Some(Address(0xbd6652c9fc545d))
```

The second line is the one that matters. Given:

```ink
// main.ink
INCLUDE inc.ink
* main one
  MAIN-ONE-BODY
* main two
  MAIN-TWO-BODY
- main gathered
```

```ink
// inc.ink
* inc one
  INC-ONE-BODY
* inc two
  INC-TWO-BODY
- inc gathered
```

the player is offered `inc one` / `inc two`, picks `inc one`, and the runtime
prints `main one` / `MAIN-ONE-BODY`. `INC-ONE-BODY` is unreachable.

The fourth quoted failure above (the terminus one) is from the LIR-level unit
test, which mints each source's `FileId` from its position in a test-harness
array — an artifact of the harness, not of anything a real `INCLUDE` count
does on the ordinary `discover` path. See
[Correcting the framing of (b)](#correcting-the-framing-of-b) for the
reachable form of this same defect (editor/LSP file-registration order),
which a `brink-driver`-level test also demonstrates.

## Mechanism

### (a) Cross-file collisions

Three facts compose into the bug.

1. **Root-content scope paths are unqualified by file.**
   `lower_root_content_chunks` loops over every file and hands `make_ctx` an
   empty scope path *unconditionally*
   (`crates/internal/brink-ir/src/lir/lower/mod.rs:487` — a literal
   `String::new()` inside the per-file loop that starts at `:475`). A knot
   scopes its children under the knot name; root content has no such prefix,
   for any file.

2. **`alloc_address` is a pure hash of that path.**
   `IdAllocator::alloc_address`
   (`crates/internal/brink-ir/src/lir/lower/context.rs:392`) memoizes in a
   `used` map and otherwise returns
   `DefinitionId::new(DefinitionTag::Address, hash_path(path))`. It has **no
   collision-avoidance step** — the same path always yields the same id, by
   design (that is what makes it content-pure for FG-4d).

3. **Per-file counters restart.** The choice/gather counters (`cc`/`gc`) are
   locals re-initialized to `0` on each loop iteration, and
   `ctx.ids.reset_seq_counter()` is called per file
   (`mod.rs:498`). So file A's first choice and file B's first choice are both
   named `c-0`.

Composed: `hash_path("c-0")` is computed twice and returns the same
`DefinitionId` both times.

Note that the *shared* `IdAllocator` is not the cause. Because `alloc_address`
is a pure hash, giving each file its own allocator would produce exactly the
same colliding ids. The sole cause is the unqualified path. This matters for
the fix: "per-file allocators" alone does not fix anything.

### Blast radius

`DefinitionId` is the key for three distinct things, and a collision corrupts
all three:

| Consumer | Coordinate | Behavior on collision |
|---|---|---|
| Linker address map | `crates/brink-runtime/src/linker.rs:88` (`address_map.insert(cdef.id, …)`) | Last-write-wins. Diverts to the earlier container silently land in the later one. **This is the observed miscompile.** |
| Linker container map | `crates/brink-runtime/src/linker.rs:40` (`container_map.insert(cdef.id, idx)`) | Last-write-wins, same way. |
| Save state | `crates/brink-runtime/src/save.rs:113–115` (visit/turn counts keyed by `container.id`) | Two containers share one visit counter, so read counts and sequence progression conflate across files. |

One consumer that is *not* affected, contrary to what a first read suggests:
codegen's `scope_line_tables`
(`crates/internal/brink-codegen-inkb/src/lib.rs:261`,
`entry(scope_id).or_default()`) does merge entries — but only scope-kind
containers open a new scope (`is_scope_kind`, `lib.rs:397`, is
`Root | Knot | Stitch`), and gathers/choices inherit the enclosing scope id.
Every file's root content therefore already shares the root scope's line table
*by design*. The intl/XLIFF surface is not additionally damaged by this
collision.

### Why the oracle never caught it

The oracle corpus exercises multi-file projects, but root-level *weave* content
in an `INCLUDE`d file is unusual authoring — an included file normally contains
only knots. The collision needs two files that each open a root-level choice or
gather. Nothing in `tests/tier{1,2,3}` does that, so the ratchet is blind to it.
This is worth a corpus case regardless of which fix shape wins.

### (b) The `FileId`-keyed terminus address

`attach_root_final_gather` keys the synthesized terminus:

```rust
// crates/internal/brink-ir/src/lir/lower/mod.rs:1957
let terminus_id = ids.alloc_address(&format!("#root-terminus.{}", file_id.0));
```

This is the only `alloc_address` call in `brink-ir` whose key is a `FileId`
rather than a scope path. The existing comment above it is candid about the
motive: keeping the file in the key left single-file programs' addresses
byte-identical to what #1448 shipped.

#### Correcting the framing of (b)

The issue says "file order changes the address," and cites FG-4d
history-independence. The gate really is narrower than the issue's framing
suggests, but not in the direction an earlier draft of this doc claimed —
that draft's "adding an unrelated `INCLUDE` moves the terminus" claim is
**false on the ordinary path** and should be retracted. Two things were
conflated: the LIR-lowering unit test's own `FileId` assignment (a
harness artifact) and what a real compile does.

- `docs/fine-grained-salsa-proposal.md:488` states the constraint as
  *"id assignment must be history-independent — an incremental re-link after N
  edits produces the same bytes as a fresh build of the same source … Assignment
  must therefore be content/tree-derived, never allocation-history-derived."*
- `FileId` assignment **is** deterministic for a fixed source tree: for ink,
  `compilation_closure_files` orders the closure with
  `IncludeGraph::topological_order(entry)`
  (`crates/internal/brink-db/src/queries/mod.rs:525`). So an incremental re-link
  and a from-scratch build of the *same* tree agree. The
  `incremental ≡ from-scratch` gate is **not** violated.
- **`discover` seeds its BFS queue with the entry** (`crate::discover::discover`,
  `crates/internal/brink-driver/src/discover.rs:51`), so a from-scratch ink
  compile always mints the entry `FileId(0)`, regardless of how many files it
  `INCLUDE`s. Measured through `Driver::discover` + `db.story_data()`:
  compiling `main.ink` alone vs. `main.ink` + `INCLUDE extra.ink` gives the
  entry `FileId(0)` both times, and the solo compile's container-id set is a
  strict subset of the with-include set — no id moves. **Adding an unrelated
  `INCLUDE` does not move the terminus address on this path.** The supporting
  argument for the old claim was also a non-sequitur: `topological_order`
  fixes the *chunk order* lowering consumes (`mod.rs:502`'s `files` iteration
  order), not the `FileId` *value* baked into the terminus key —
  that value is minted once, in `ProjectDb::set_file`'s registration order
  (`crates/internal/brink-db/src/db.rs:93`), independent of topological order.
- **The case this doc previously called safe is the one that is actually
  broken.** `ProjectDb::set_file` mints `FileId`s in registration order, and
  nothing requires an editor session to register the entry file first.
  `brink-lsp`'s `load_file_from_disk` (`crates/brink-lsp/src/backend.rs:624`)
  is the shared admission sink for both an explicit `did_open` and a chased
  `INCLUDE` target — a workspace walk or an `INCLUDE` chase can register a
  sibling ahead of the entry. Registering a sibling before the entry gives the
  entry `FileId(1)` instead of `FileId(0)` and moves the terminus address for
  the *same* tree and the *same* entry file, with every other container id
  unchanged. That id is allocation-history-derived — exactly what
  `docs/fine-grained-salsa-proposal.md:488` forbids — and it breaks the same
  editor-vs-compile identity parity already asserted for native
  (`crates/internal/brink-driver/src/discover_native.rs:349`). A
  `brink-driver`-level test,
  `root_content_ids_agree_between_discover_and_editor_order`
  (`crates/internal/brink-driver/src/discover.rs`), reproduces this directly:
  it builds one `ProjectDb` via `Driver::discover` and one by `set_file`-ing a
  sibling before the entry, over the identical source pair, and the two
  programs' root-content container-id sets differ by exactly the terminus id.

So (b) is a real defect and does need fixing, but the reachable trigger is
**editor/LSP file-registration order**, not `INCLUDE` count, and it should be
filed as *save-key stability*, not as an FG-4d gate failure. Neither the
`incremental ≡ from-scratch` check nor a from-scratch-only compile will ever
catch it, which is precisely why it survived.

## The migration problem

There is no way to migrate an anonymous container's id.

`save.rs`'s rebinding path (`rebind_address`, `save.rs:301`) resolves a stale
saved id through `program.resolve_alias`. That alias table is populated from
author-written `@[was("…")]` annotations, which are **name-based**. A knot that
was renamed can carry `@[was]`; an anonymous `c-0` or `g-0` has no name to
teach, and no author-visible identity to attach an annotation to.

Consequence: **any fix that changes anonymous container ids silently
invalidates the visit counts and sequence positions in existing saves, with no
migration path and no load-time diagnostic.** That is not an argument against
fixing it — it is an argument for fixing it *now*, at `0.0.11`, rather than
after 1.0. The cost of this change is monotonically increasing.

## Coordination with #1442

The maintainer's reopening comment on #1442 asks for one answer across #1504,
#1442, and the `@[was]` facility. Having traced both, the recommendation is
that **the identity question has two layers, and the two issues live in
different ones**:

| | Named definitions (knots, stitches, labeled gathers, globals, lists) | Anonymous weave containers (`c-N`, `g-N`, `b-N`) |
|---|---|---|
| Has author-visible identity? | Yes | No |
| Can carry `@[was]` lineage? | Yes | No |
| Rename-invariant id possible? | Yes — mint at creation, persist in a ledger | No — there is no "name" to hold invariant |
| Identity must be | Minted + carried | Structural (a path) |
| Issue that lives here | **#1442** (rename churn), `@[was]` | **#1504** (collisions, purity) |

#1442 needs identity that survives a *rename*. #1504's collisions are 100% in
anonymous containers, which have no name to rename — every colliding id in the
evidence above is a `c-N` or `g-N`. Their identity is inherently structural and
will remain a path under *any* design, including one that later mints
rename-invariant ids for named definitions.

**Therefore #1504 can be settled without pre-judging #1442.** Qualifying an
anonymous container's structural path with its owning file is compatible with
every candidate answer to #1442, because it changes what the path *is*, not
whether named definitions derive their ids from paths at all. Bundling them
risks blocking a live miscompile behind a much larger design question.

This is a recommendation, not a ruling — the owner may still prefer one
combined change.

## Recommended shape

### Primary: qualify root-content scope paths by the owning file, not the module

Replace the unconditional `String::new()` at `mod.rs:487` with a qualifier
derived from the file itself — its normalized root-relative source path (or
equivalently, a stem unique within the project) — so file A's first root
choice is `a.ink::c-0` and file B's is `b.ink::c-0`.

This must be **per-file**, not "per owning module" as an earlier draft of
this recommendation said. Module path alone is insufficient and leaves (a)
unfixed for exactly the kind of project #1504 was filed against: under
`dialect = brink`, `#@module(name)` is permitted on `.ink` files
(`dialect_gate.rs` flags it only under strict-ink,
`crates/internal/brink-analyzer/src/dialect_gate.rs:103-110`), and
`docs/modules-spec.md` §1 rules — implemented and tested
(`crates/internal/brink-db/src/modules.rs:144`,
`crates/internal/brink-db/src/db.rs`'s
`included_file_inherits_module_identity`) — that an `INCLUDE`d file with no
declaration of its own inherits its includer's module. So a brink-dialect
project with `#@module(story)` on the entry, plus a plain `INCLUDE`d file
with root-level weave and no `#@module` of its own, resolves **both** files
to the module `story` and reproduces the exact (a) collision after a
module-qualified fix — the included file's `c-0` and the entry's `c-0` both
hash `story::c-0` again. Qualifying by the file itself sidesteps this
entirely, since two distinct files always have distinct paths regardless of
what module either one declares or inherits.

Fold (b) into the same change: the terminus key becomes
`{file_qualifier}#root-terminus`, dropping the `FileId` entirely and making
it content-derived like every other `alloc_address` call.

Why this shape:

- Fixes (a) and (b) with one concept, at one locus, and fixes (a) for
  declared-module projects too, unlike the module-qualified variant.
- Keeps the existing `hash(path)` model, so FG-4d purity and the
  `incremental ≡ from-scratch` gate are preserved by construction.
- Does not foreclose any answer to #1442.
- Uniform: no special-casing of the entry file (see the rejected variant
  below).

Known costs, which are the substance of the ruling being requested:

1. **Every project's root-content ids change once**, including single-file
   projects, invalidating anonymous-container visit counts in existing saves
   with no migration path (see above). At `0.0.11` this is judged cheap; it
   will not stay cheap.
2. **File renames become a new id-churn axis.** Once the path includes the
   file's own identity, renaming `inc.ink` churns its root-content ids — the
   same class of problem #1442 reports for scope renames, now extended to
   files. This is a genuine argument for solving both together, and the owner
   should weigh it. Note the churn is bounded to root-level weave content;
   knots are already qualified by name and unaffected.

### Variant considered and rejected: keep the entry file unqualified

Qualifying only *included* files would leave single-file projects
byte-identical and confine the save break to projects that are already
miscompiling. Rejected because it makes an id depend on a file's *role* in the
project rather than on its own identity, so moving a file from included to
entry silently rewrites its ids — trading a one-time break for a permanent
sharp edge.

### Rejected: content-hash discriminator

#1504 floats "per-file allocators with a content-derived discriminator." If
"content" means the file's *source text*, this should be rejected outright:
every edit to a file would change every id in it, making save-key churn
continuous rather than one-time. If "content" means the file's identity (path
or stem), it is the primary recommendation above — but note the per-file
allocator itself is irrelevant, since `alloc_address` is a pure hash (see
[Mechanism](#a-cross-file-collisions)).

## Suggested follow-ups

Independent of the ruling — both **landed**:

- ~~Add a corpus case with root-level weave content in **both** the
  entry file and an `INCLUDE`d file~~ — shipped as
  `tests/tier1/includes/root-weave-in-entry-and-included-file` (tier 1, per
  the #1504 ruling). `tests/tier3/includes/included-file-trailing-weave/` and
  `tests/tier3/includes/choice-accumulation-across-include/` already cover
  root weave in an included file alone (their entry `story.ink` has no
  root-level weave of its own), so a corpus case asking for that shape again
  would have been a duplicate. The new case exposed a *second*, unrelated
  divergence: brink does not accumulate root-weave choices across the
  `INCLUDE` splice the way C# ink does, so the case compiles but fails 0/4
  episodes — the same gap `choice-accumulation-across-include` and
  `root-content-splice-site` already record.
- ~~Consider a codegen-level assertion that no two containers share a
  `DefinitionId`~~ — shipped in #1673. This bug reached the runtime silently;
  the compiler had every opportunity to reject it. That guard is cheap, is
  independent of the identity ruling, and would have caught this at authoring
  time.

## What the fix did not disturb

`brink-intl`'s export keys a translation scope on a
`ScopeLineTable::scope_id` (`export.rs`), and codegen opens a line table only
for a scope-kind container (`Root`/`Knot`/`Stitch`). Root-level choices and
gathers inherit the **root** scope's id — the hash of the empty path, not
qualified by file — so no XLIFF unit id for a root-level line moves, and the
alias-blindness concern raised while sequencing this against #1442 does not
bite. Pinned by
`root_content_translation_scope_id_is_unaffected_by_the_qualifier`
(`crates/brink-compiler/tests/issue_1504_root_content_identity.rs`). Save
state is the surface that *does* move: anonymous visit counts and sequence
positions are re-keyed, with no migration path (see
[The migration problem](#the-migration-problem)).
