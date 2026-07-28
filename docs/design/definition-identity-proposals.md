# Definition identity under rename — design writeup

Status: **ruled.** R1 was answered on 2026-07-27 (PR #1670): `modules-spec`
§5 stands — identity stays name-derived, `#@was` is the sole migration
edge, and stamped GUIDs and fuzzy load-time rematching remain rejected.
The gaps this writeup inventoried are now separately owned: #1442 (intl
alias-awareness, delivered), #1671 (transitive `#@was`), #1672 (the IDE
writing the directive), #1674 (anonymous-container state), #1504
(root-content collisions). The analysis below is kept as the record of
how that answer was reached.

Originally filed as: design only for issue #1442 (`needs-design`,
reopened after PR #1594), which asked for *one* answer covering itself,
#1504 (multi-file `DefinitionId` collisions) and the `#@was` migration
facility.

This document is the design-first artifact #1442 asks for. It states the identity question once, inventories what
`DefinitionId` actually is today and which durable surfaces key on it,
shows measured evidence of the failure, reports what the project has
**already ruled** about identity (which shrinks the option space more
than the issue text assumes), lays out the surviving options with
tradeoffs, and isolates the single question that is a maintainer ruling
rather than an implementation choice.

It deliberately stops short of prescribing an implementation. The
`needs-design` label plus #1442's own "architectural, save-key-adjacent,
and requiring maintainer sign-off" is read here as: do not pick the
shape unilaterally.

Companion artifact: `crates/internal/brink-intl/tests/rename_identity.rs`
(added alongside this document) pinned the failure as executable
evidence. Its translation-orphaning case flipped when #1442 landed; the
churn and shallow-net cases still hold, by design.

---

## 1. The question, stated once

> When the author renames a definition, what — if anything — carries the
> definition's *durable* attachments (saved visit counts, translations,
> audio references) across the rename?

Three issues are three faces of that one question:

| Issue | Face of the question |
|-------|----------------------|
| #1442 | Translations orphan on rename (XLIFF unit ids churn) |
| #1504 | Two *different* definitions can be handed the *same* id |
| `#@was` | The declared-rename migration edge — the answer already ruled for saves |

The unifying observation is that a name-derived identity conflates **what
a definition is** with **what it is called**, so it fails in both
directions at once:

- **same name, different definitions → collision** (#1504)
- **same definition, different name → churn** (#1442)

Any answer that fixes only one direction leaves the other open, which is
why #1442 asks for one answer rather than three.

## 2. What identity is today

`DefinitionId` is a tagged 64-bit value: an 8-bit `DefinitionTag`
discriminant over a 56-bit hash (`crates/internal/brink-format/src/id.rs:48-52`,
minted through `DefinitionId::new` at `id.rs:81`). The hash is always
derived from a **name or a path string** — there is no allocator, no
counter, and nothing persisted between compiles.

There are three distinct minting schemes in production, all landing in
the same tag space:

| Scheme | Where | Hashed input | Used for |
|--------|-------|--------------|----------|
| Qualified-name hash | `brink-analyzer/src/manifest.rs:284-286`, fn at `:416-424` | `tag`, then optional declared module, then the symbol name | every **declared** symbol — knots, stitches, labels, VAR/CONST, lists, externals, structs (`SymbolKind::definition_tag`, `brink-ir/src/symbols/index.rs:123-133`) |
| Path hash | `brink-ir/src/hir/stamp.rs:286-290` and `brink-ir/src/lir/lower/context.rs:392-400` (hash fn at `:424-428`) | the scope path string alone (`""`, `"knot.c0"`, `"knot.g-0"`) | **synthetic** containers — choice targets, gathers, conditional/sequence wrappers, the root |
| Scope-prefixed local hash | `brink-analyzer/src/manifest.rs:441`, minting at `:450` | `tag`, then a `knot.stitch.`-style scope prefix, then the local's bare name | scoped locals — params/temps (`SymbolKind::LocalVar`); non-durable, not file-qualified, so it does not disturb R1, but is a third scheme a maintainer must know about |

Two consequences worth naming, because they surface again in §5 (Options):

- A stitch's *declared* name is already the qualified `knot.stitch`
  (`brink-ir/src/symbols/project.rs:332-343`), and a synthetic
  container's path is prefixed with its enclosing scope path. So the id
  of everything beneath a knot is a function of the knot's name.
- The path-hash scheme is documented as **content-pure** — a fresh
  `IdAllocator` inside a per-chunk salsa memo must produce the same id
  as the whole-project walk (`context.rs`, FG-4d
  history-independence). #1504(b) was the one violation —
  `#root-terminus.{file_id}` keyed an address by `FileId` — and is now
  fixed: the terminus key is the bare `#root-terminus` under the
  allocator's per-file path prefix, which is derived from the owning
  file's project path (`brink-ir/src/hir/stamp.rs`'s
  `root_content_scope_path`). The same qualifier is what stops two files'
  root weaves from colliding, #1504(a).
- The scope-prefixed local scheme is *not* file-qualified: its own doc
  comment (`manifest.rs:435-440`) records that two files which
  (pathologically) declare the same scope-qualified local name still
  collide on one `DefinitionId` in the merged index. That is an
  in-tree, already-documented instance of the same **same name,
  different definitions → collision** direction §1 frames for #1504 —
  scoped down to a merged-index-only blast radius (per-file resolution
  does not go through the merged index for locals, so the collision
  cannot leak into resolution correctness).

### Who keys on it durably

| Surface | Key | Consults the alias table? |
|---------|-----|---------------------------|
| Save: visit/turn counts | scope `DefinitionId` (`brink-format/src/save.rs:36-38`) | **yes** — `brink-runtime/src/save.rs:254-267` → `rebind_address` (`:301-309`) → `Program::resolve_alias` |
| Save: globals | name, with the save-time id as the miss-path key (`brink-format/src/save.rs:45-59`) | **yes** — `brink-runtime/src/save.rs:289` |
| `lines.json` / XLIFF scope | scope `DefinitionId` hex (`brink-intl/src/export.rs:63`) | **no** |
| XLIFF `<unit id>` | `{scope_id}:{line_index}` (`brink-intl/src/xliff_convert.rs:189`, post-#1594) | **no** |
| Locale regeneration | scope id string equality (`brink-intl/src/regenerate.rs:19-28`) | **no** |
| Locale compile | scope id → base table, else `IntlError::ScopeNotInBase` (`brink-intl/src/compile.rs:37-43`) | **no** |

The alias table itself is real, shipped, and already in `.inkb`: one
`AliasEntry { old, new }` per `#@was` directive
(`brink-analyzer/src/manifest.rs:328-341`), sorted and binary-searched at
load (`brink-runtime/src/program.rs:180-186`; chains are explicitly not
followed).

## 3. Measured evidence

`crates/internal/brink-intl/tests/rename_identity.rs` compiles a knot
`hub` with a stitch `market`, then the same story with the knot renamed
to `plaza` and the rename **declared** (`#@was(hub)`). Observed:

| Scope | Before | After |
|-------|--------|-------|
| `hub` → `plaza` | `0x01dc2d3e4dc206af` | `0x01a3ccc344ad71ad` |
| `hub.market` → `plaza.market` | `0x01a409a019e32d22` | `0x015d074d1121ba51` |

Compiled alias table after the rename: **one** entry,
`0x01dc2d3e4dc206af → 0x01a3ccc344ad71ad`.

Three facts fall out, and the tests pin all three:

1. **Churn is transitive; the rename net is not.** The stitch was not
   renamed, but its id changed because its parent's name is part of its
   own qualified name — and no alias entry covers it. Its saved visit
   count and its whole translation set have *no* migration path, even
   though the author did declare the rename. (A stitch-level `#@was`
   *does* work — the lowering qualifies the old name at
   `brink-ir/src/hir/lower/structure/stitch.rs:135-146` — so the gap is
   specifically the ancestor-renamed case. The test file carries that
   positive control.)
2. **Translations orphan even when the migration edge exists.**
   Regeneration matched scopes by id string and never read
   `StoryData::alias_table`, so the knot's own translations were dropped
   too. For `compile-locale` it was worse than a drop: a stale locale
   file was a hard `ScopeNotInBase` error, not a partial merge.
   **CLOSED (#1442):** both surfaces now rebind through the alias table
   — see the "Alias rebinding" rules in `docs/intl-spec.md`. What
   remains is the residue of fact 1: a scope with no alias entry of its
   own still cannot rebind, which is #1671's transitive-`#@was` gap, not
   a matching-rule gap.
3. **The safety net is unwired at the authoring end.** `modules-spec`
   §5 says "IDE rename writes the directive automatically (module and
   knot renames both go through the #305/#306 rename machinery)".
   `brink-ide/src/rename.rs` contains no `#@was` emission at all
   (searched `crates/internal/brink-ide/src`, `crates/brink-lsp/src`,
   `crates/brink-cli/src` — no directive-writing site). So today the
   ruled net only catches renames the author hand-annotates.

## 4. What is already ruled

This is the part that most changes the shape of #1442's answer, and it
is easy to miss because it lives in the modules spec rather than the
intl spec.

`docs/modules-spec.md:108-132` (§5, marked **RULED**) says:

- identity **stays name-hashed** — `DefinitionId` = hash of
  `(module, name)`;
- `#@was` is *the* rename record, shipped as an old→new alias table in
  `.inkb`, consulted on the **miss path only**;
- the directive is deletable after a migration window;
- and it lists three **rejected** alternatives verbatim:
  "content-hashed identity (breaks on every edit), permanent GUIDs
  (hostile to text-first merging), fuzzy load-time rematching
  (silent-garbage risk)".

Two of the three shapes an unconstrained reading of #1442 would reach
for — minted GUIDs, and matching translations by content instead of by
scope — are already rejected in that list. Unless the maintainer
reopens that ruling, #1442's answer is **not** to invent a
rename-invariant identity: it is to complete the ruled lineage relation
and make the durable surfaces that ignore it consume it.

`docs/decision-log.md:2232-2240` is consistent with this: PR #1594's
entry was corrected in review to state plainly that `{scope_id}:{index}`
is *not* rename-stable, and that "Real rename stability is a
`DefinitionId`-level change that needs maintainer sign-off on #1442".

## 5. Options

### Option A — Complete the ruled model (lineage-based)

Keep name-hashed identity. Make the `#@was` relation **transitive** (a
rename record on a scope aliases every descendant whose qualified path
changed only because of it: stitches, labels, and — for save-side visit
counts — synthetic containers), wire the alias table into every durable
consumer that currently ignores it (intl regeneration, and by extension
XLIFF re-keying), and wire the IDE rename to write the directive as modules-spec §5
already ruled.

- **Fixes #1442?** Yes for *declared* renames, which is the ruled
  contract: a rename plus its `#@was` carries translations across, the
  same way it already carries visit counts.
- **Fixes #1504?** Not by itself. Qualifying root-content scope paths
  (the collision fix) is a separate change — and note it *is itself an
  identity break*, so it wants the same migration machinery.
- **Cost:** the alias table grows with declared renames (bounded, and
  the directive is deletable). Undeclared renames still churn.
  Multi-hop chains stay unsupported (`brink-runtime/src/program.rs:177-179`), so a rename
  of a rename inside one migration window needs the second `#@was` to
  name the *original* old id, not the intermediate one — worth stating
  explicitly if this option is taken.
- **Format impact:** none new — the alias section already exists.

### Option B — Minted durable ids

Allocate a per-definition id at creation (an `@[id("…")]` annotation, or
a project-level lock file mapping path → id) and hash nothing.

- Rename-invariant *and* collision-proof by construction; would close
  #1442 and #1504 together.
- **Rejected already** by modules-spec §5 ("permanent GUIDs (hostile to
  text-first merging)"). Beyond that ruling, it breaks the FG-4d
  content-purity invariant (`context.rs:360-369`): identity would become
  history-dependent, so a story could no longer be compiled from source
  alone deterministically, and a lost or merge-conflicted lock file is
  unrecoverable data loss rather than a diagnosable miss.
- Listed here because #1442's reopening comment gestures at it ("a
  stable per-definition id minted at creation"); taking it means
  **reopening a standing ruling**, which is exactly the sign-off this
  document exists to request.

### Option C — Content-matched translations (intl-only)

Leave identity alone; make intl stop depending on scope identity by
matching translation units on `source_hash` across the whole file, with
the scope id as a hint rather than a key.

- Closes #1442's *symptom* cheaply and needs no compiler change.
- Does nothing for #1504 or for saved visit counts, so it is explicitly
  **not** the "one answer" #1442 asks for.
- Collides with the third rejected alternative in modules-spec §5 ("fuzzy load-time
  rematching (silent-garbage risk)"): duplicate short lines ("Yes.",
  "No.") would rebind across scopes. A scope-hint tiebreaker narrows
  this but does not remove it.

### Option D — Split the key: content-hashed link id + lineage id

Keep `DefinitionId` as the pure, content-derived link/memo key, and give
*durable* surfaces (saves, translations) a second, lineage-carrying id
that the alias relation transports.

- Preserves FG-4d purity exactly, and lets the durable key be as stable
  as the lineage relation is complete.
- In practice this is Option A plus a rename of the concept: the
  lineage-carrying value is derived from the same alias edges. It is
  worth separating only if the maintainer wants durable keys to survive
  the *undeclared* case too, which no option here delivers without
  either Option B's minting or Option C's fuzziness.

### Decision matrix

| | #1442 (churn) | #1504 (collision) | Undeclared renames | Reopens a ruling? | FG-4d purity |
|---|---|---|---|---|---|
| A | fixed for declared renames | separate fix, same machinery | still lost | no | preserved |
| B | fixed | fixed | fixed | **yes** (GUIDs rejected) | **broken** |
| C | symptom only | untouched | partly, fuzzily | **yes** (fuzzy rematch rejected) | preserved |
| D | as A | as A | still lost | no | preserved |

## 6. The ruling required

**R1 — Does the standing modules-spec §5 ruling still hold?** That is:
is identity to stay name-hashed with `#@was` as the sole migration edge
(→ Option A/D), or is the project willing to reopen the rejected-GUID
line and take minted durable ids (→ Option B)?

Everything else follows from R1, and R1 is not an implementation choice.
Two subsidiary questions only need answering if R1 keeps the ruling:

- **R1a — Is a *declared* rename an acceptable precondition for durable
  survival?** Option A is only as good as the `#@was` habit. If
  undeclared renames must also survive, no option here suffices and B
  or C re-enters.
- **R1b — Does the transitive alias expansion belong in the compiled
  table, or is it derived at load?** Emitting one entry per descendant
  is simple and matches the existing miss-path lookup, but the table
  grows with subtree size; deriving it needs the old *path*, which the
  alias table does not carry today.

## 7. If R1 keeps the ruling — sequencing (not authorized by this doc)

Recorded so the follow-up work is legible, in dependency order. None of
it is implemented here.

1. **Transitive `#@was` expansion** — closes a *silent save-data loss*
   (§3 fact 1), independent of intl. Test 2 in the companion test file
   flips when this lands.
2. **Alias-aware locale regeneration** — `regenerate_lines` needs the
   alias edges, which means `LinesJson`/`regenerate_locale` must carry
   or be handed them; today `regenerate_lines` sees only two
   `LinesJson`. Test 4 flips when this lands.
3. **IDE rename writes `#@was`** — the ruled autopilot (§4) that makes
   1 and 2 reachable for ordinary authors rather than only for authors
   who know the directive exists.
4. **#1504 qualification fix** — **landed** (after R1 was ruled), by the
   owning *file*'s project path rather than by module, since an
   `INCLUDE`d file inherits its includer's module and would still have
   collided. It re-keys existing definitions, and the changeset calls
   that out as an identity break rather than slipping it in as a bug
   fix. Measured consequence: anonymous visit counts move; translation
   scope ids do **not** (root content's line table is keyed on the root
   scope id, which is not file-qualified).

Each of those is separately reviewable; none should land before R1.

## 8. What this writeup does not do

- It does not change any identity, id-minting, or matching code. The
  only production-code-adjacent change shipped with it is the
  characterization test file.
- It does not amend `docs/modules-spec.md` §5. If R1 reopens the ruling,
  that spec and the decision log are where the reversal is recorded —
  not here.
- It does not re-litigate PR #1594. That PR's canonical-id and
  collision-avoidance gains stand; only its withdrawn rename-stability
  claim is in scope, and the decision log already carries the
  correction (`docs/decision-log.md:2239`).
