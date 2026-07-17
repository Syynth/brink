# brink ide

`brink ide` exposes the same query-and-refactor engine the language server and
Studio use — navigation, outlines, hover, diagnostics, renames, and structural
refactors — as **scriptable, one-shot commands**. There is no server: each
invocation discovers the project from an entry `.ink` file (following
`INCLUDE`s, exactly like [`brink compile`](./compile.md)), answers one question
or applies one change, and exits.

This makes it the tool of choice for **scripts and coding agents**: "is this
variable referenced?", "where is this knot defined?", "rename this symbol across
the project and give me a patch", "list the dead code", "what's the story flow
graph" — each is a single command with a machine-readable `--format json` mode
and a stable exit-code contract.

```sh
brink ide --help
brink ide <COMMAND> --help    # per-command help with examples
```

## Anatomy of a command

```sh
brink ide <command> [TARGET] --entry <FILE> [--format text|json] [options]
```

- **`--entry <FILE>` / `-e`** — required on every command. The project's entry
  point; its `INCLUDE`s are followed to build the whole project. (Identical
  discovery to `brink compile`.)
- **`TARGET`** — what the command operates on: a qualified symbol name, or a
  cursor position via `--at` (see [Addressing](#addressing)). Read queries that
  operate on a whole file take `--file` instead; cursor-only commands
  (`signature`, `actions`, `refactor convert-line`) take `--at`.
- **`--format text|json`** — output mode (default `text`). See
  [Output & exit codes](#output--exit-codes).

## Addressing

Most commands address a **symbol**. You can name it, or point at it.

### By qualified name

Names use the same dotted paths ink itself uses:

| Form | Resolves to |
|------|-------------|
| `intro` | a knot, or a top-level `VAR`/`CONST`/`LIST`/`EXTERNAL` named `intro` |
| `intro.evidence` | the stitch `evidence` in knot `intro` |
| `Colors.Red` | the list item `Red` of list `Colors` |
| `intro.evidence.found` / `intro.found` | the label `found` (ink's `knot.stitch.label` / `knot.label` path) |
| `damage(weapon)` | the parameter `weapon` of knot/function `damage` |

When a bare name matches more than one kind, the command errors and asks you to
disambiguate with `--kind`:

| `--kind` value | Symbol kind |
|----------------|-------------|
| `knot`, `stitch`, `variable`, `constant`, `list`, `list-item`, `external`, `label`, `param`, `temp` | the corresponding declaration |

### By cursor — `--at FILE:LINE:COL`

`--at` takes a **1-based** line and column (the file may contain `:`; the two
numeric fields are split off the right). It resolves to the symbol under that
position — and if the position is a *use*, it resolves through to the
*definition*. This is the editor-integration / disambiguation fallback.

```sh
brink ide def intro -e main.ink                 # by name
brink ide def --at main.ink:7:5 -e main.ink     # by cursor
```

## Output & exit codes

- **`--format text`** (default) — concise, human-readable. Locations render as
  `path:line:col`.
- **`--format json`** — a stable shape for `jq` and programmatic use (see
  [JSON output & stability](#json-output--stability)). Locations become
  `{ "path", "line", "col", "byte_start", "byte_end" }`.

Exit codes are a contract — they compose in CI and agent loops:

| Code | Meaning |
|------|---------|
| `0` | success / query true |
| `1` | query false, lint hit, diagnostics present, or a mutation refused by the safety gate |
| `2` | usage error (unknown symbol, bad `--at`, ambiguous name, file not in project) |

Examples of the `1` contract: `references --exists` exits `1` if the symbol is
**not** referenced; `unused` exits `1` if it finds any dead symbols; `check`
exits `1` if the project has any error.

---

## Read queries

### `def` — where a symbol is defined

```sh
brink ide def intro -e main.ink
brink ide def intro.evidence -e main.ink --format json
brink ide def --at main.ink:7:5 -e main.ink
```

JSON: `{ "name", "kind", "location" }`.

### `references` — every use across the project

```sh
brink ide references gold -e main.ink
brink ide references gold --include-decl -e main.ink     # also count the declaration
brink ide references intro --exists -e main.ink          # exit 0 if used, 1 if not
brink ide references gold --count -e main.ink            # print just the number
```

| Flag | Effect |
|------|--------|
| `--include-decl` | include the declaration site in the results |
| `--exists` | print nothing; exit `0` if referenced, `1` if not |
| `--count` | print only the number of references |

JSON: `{ "name", "kind", "count", "references": [location, …] }`.

### `symbols` — file outline or project search

```sh
brink ide symbols -e main.ink                       # outline of the entry file
brink ide symbols --file scenes/intro.ink -e main.ink
brink ide symbols --search gold -e main.ink         # project-wide name search (flat)
brink ide symbols --kind knot -e main.ink           # filter the search by kind
```

Without `--search`, prints the file's hierarchical outline (knots with nested
stitches; globals at the top). With `--search`, prints a flat project-wide list
filtered by substring (and optionally `--kind`).

JSON: an array of `{ "name", "kind", "location", "detail"?, "children"? }`
(`detail` and `children` are omitted when empty).

### `unused` — declared but never referenced

The scriptable inverse of `references --exists`: lists every declared symbol
with no references (dead knots, unused vars/lists/externals). **Exits `1`** if
any are found.

```sh
brink ide unused -e main.ink
brink ide unused --kind variable -e main.ink
```

> Note: this is *reference*-based, not *reachability*-based. A knot reached
> implicitly by fall-through (no `->`) can appear here.

### `check` — project diagnostics

Reports all diagnostics (errors and warnings) with their E-codes. **Exits `1`**
if there is any *error* (warnings alone still exit `0`).

```sh
brink ide check -e main.ink
brink ide check -e main.ink --format json
```

JSON: an array of `{ "severity", "code", "message", "location" }`.

### `hover` — kind, signature, and docs

```sh
brink ide hover gold -e main.ink
brink ide hover --at main.ink:5:5 -e main.ink
```

JSON: `{ "content" (markdown), "location" }`.

### `signature` — the call at a cursor

Position-only (you are mid-call): pass `--at` inside the call's parentheses.

```sh
brink ide signature --at main.ink:9:10 -e main.ink
```

JSON: `{ "label", "documentation", "parameters": [string, …], "activeParameter" }`.

### `graph` — the story flow graph

Knots/stitches as nodes; diverts, choices, tunnels, and threads as edges.

```sh
brink ide graph -e main.ink
brink ide graph -e main.ink --format json
brink ide graph --dot -e main.ink | dot -Tsvg -o story.svg
```

`--dot` emits Graphviz DOT. JSON: `{ "nodes": [{ "id", "name", "kind", "parent" }],
"edges": [{ "from", "to", "kind" }] }`. Node kinds: `knot`, `stitch`, `end`,
`done`. Edge kinds: `divert`, `choice`, `tunnel`, `thread`.

### `lines` — per-line structural classification

```sh
brink ide lines -e main.ink
brink ide lines --file scenes/intro.ink -e main.ink --format json
```

JSON: an array of `{ "line", "element", "depth" }` (one per source line).

### `actions` — code actions at a cursor

Lists the refactors applicable at a position (each runnable via the
`refactor` verbs below).

```sh
brink ide actions --at main.ink:7:5 -e main.ink
brink ide actions --at main.ink:7:5 -e main.ink --format json
```

JSON: an array of `{ "title", "kind" }` (kind: `quickfix`, `refactor`, `source`).

### `effects-diff` — row drift between two revisions

Diffs every knot/stitch's inferred [effect row](../dialect/effects.md)
between two git revisions, or a revision and the working tree — the
drift-*visibility* tooling docs/effects-spec.md §10 names as the lockfile's
replacement: there is no drift policy to enforce, only a report of what
changed, CI-comment-friendly.

```sh
brink ide effects-diff -e main.ink --base HEAD~1              # working tree vs. last commit
brink ide effects-diff -e main.ink --base origin/main          # working tree vs. a branch
brink ide effects-diff -e main.ink --base main --head feature/foo   # rev vs. rev
brink ide effects-diff -e main.ink --base HEAD~1 --format json      # CI-comment-friendly
```

| Flag | Effect |
|------|--------|
| `--base REV` | required — the revision to diff against (any git commit-ish) |
| `--head REV` | diff against this revision instead of the working tree |

Unlike every other command here, `--entry` doesn't have to point at the
*current* working tree's file — `--base`/`--head` each re-resolve the whole
project (following `INCLUDE`s) as it existed at that revision, via `git
show`, so the diff is real even across renames-free structural changes on
either side. Output lines are `+`/`-`/`~` per drifted def, then indented
`+`/`- reads|writes|calls …` per changed atom set:

```text
~ spend
    + reads gold
```

This is the `1`-means-"found something" reading of the [exit-code
contract](#output--exit-codes) above (the same shape `unused`/`check` use):
`0` if nothing drifted, `1` if any def's row changed (added, removed, or a
changed atom set) — so a CI step can gate on "did anything change" without
parsing output — `2` on a usage error.

JSON: an object keyed by qualified def name, each value `{ "status":
"added"|"removed"|"changed", "row"? , "base"?, "head"? }` — `row` for
`added`/`removed`, `base`/`head` (each `{ "reads", "writes", "calls",
"opaque" }`) for `changed`. A def with no drift has no entry at all.

---

## Mutations

`rename`, `move-file`, and every `refactor` share one model. They **compute**
the edits, then *what is done with them* is chosen by mutually-exclusive mode
flags:

| Mode | Flag | Behavior |
|------|------|----------|
| **preview** | *(default)* | Print what would change — a unified diff (or `rename`'s per-edit list) plus any diagnostics the change would introduce. Touches nothing. |
| **patch** | `--patch [FILE]` | Emit a `git apply`-able unified diff to stdout, or to `FILE`. Disk-safe — never writes the target files. |
| **write** | `--write` | Apply the edits to the project files in place. |

### The safety gate

A refactor can be *structurally legal* yet still **break the project** — a
rename that shadows another symbol, a promote that leaves a now-unresolvable
reference. Every mutation re-analyzes the edited sources and diffs the
diagnostics to find what it would *introduce* (errors **and** warnings — a
collision surfaces as a warning).

- **preview** is never gated: it always prints the edits **and** the
  newly-introduced diagnostics, and exits `0`. A "show me what would happen,
  including what would break" view.
- **`--patch` / `--write`** are **safe by default**: if the change introduces
  any new diagnostic, they abort — print the diagnostics, produce nothing, exit
  `1`. Pass **`--unsafe`** (alias `--force`) to proceed anyway.

Pure-text refactors that cannot change resolution (`reorder-*`, `sort-*`,
`format`) never trip the gate. `move-file` treats "the project still analyzes
clean after the `INCLUDE` rewrite" as its safety condition.

In `--format json`, mutations always include `{ …, "introducedDiagnostics",
"safe": bool }`, so a script can decide for itself regardless of exit code.

### `rename` — a symbol and all its references

```sh
brink ide rename gold --to coins -e main.ink            # preview the edits
brink ide rename gold --to coins --patch -e main.ink    # git-applyable diff to stdout
brink ide rename gold --to coins --patch out.diff -e main.ink
brink ide rename gold --to coins --write -e main.ink
brink ide rename --at main.ink:5:5 --to newname --write -e main.ink
```

The new name is the `--to` flag. Preview JSON: `{ "edits": [{ "location", "old",
"new" }], "introducedDiagnostics", "safe" }`.

### `move-file` — relocate a file, rewriting `INCLUDE`s

Paths are **project-relative** (as they appear in `INCLUDE`s). Rewrites both
inbound `INCLUDE`s (other files that pointed at the old path) and the moved
file's own outbound relative `INCLUDE`s. On `--write`, missing destination
directories are created.

```sh
brink ide move-file scenes/intro.ink scenes/act1/intro.ink -e main.ink
brink ide move-file old.ink new.ink --patch -e main.ink
brink ide move-file old.ink new.ink --write -e main.ink
```

Errors (exit `2`) on a missing source or an occupied destination. Preview JSON:
`{ "diff", "files": [path, …], "introducedDiagnostics", "safe" }`.

### `refactor` — structural edits

```sh
brink ide refactor <operation> [args] -e main.ink [--patch|--write] [--unsafe]
```

| Operation | Synopsis | What it does |
|-----------|----------|--------------|
| `sort-knots` | `[--file F]` | Alphabetize top-level knots (preamble preserved). |
| `sort-stitches` | `<KNOT>` | Alphabetize a knot's stitches. |
| `format` | `<KNOT[.STITCH]>` | Reformat just that knot or stitch. |
| `reorder-knot` | `<KNOT> <up\|down>` | Move a knot up/down (pure text). |
| `reorder-stitch` | `<KNOT.STITCH> <up\|down>` | Move a stitch up/down within its knot. |
| `reorder-knots` | `<A,B,C> [--file F]` | Reorder all knots to an explicit permutation. |
| `reorder-stitches` | `<KNOT> <A,B,C>` | Reorder a knot's stitches to a permutation. |
| `move-stitch` | `<KNOT.STITCH> --to <DEST>` | Move a stitch into another knot, re-qualifying references. |
| `promote-stitch` | `<KNOT.STITCH>` | `= s` → `=== s ===`; references `knot.s` → bare `s`. |
| `demote-knot` | `<KNOT> --to <DEST>` | `=== k ===` → `= k` under `DEST`. Rejects if `k` has stitches. |
| `convert-line` | `--at FILE:L:C <TARGET>` | Convert a line's structural type, preserving weave depth. |

`convert-line` targets: `narrative`, `choice`, `sticky-choice`, `gather`,
`choice-body`.

Structural-op preview JSON: `{ "diff", "files", "introducedDiagnostics",
"safe" }` (a no-op reports `{ "changed": false, … }`).

```sh
# Alphabetize a file's knots, review the diff, then apply if it looks right.
brink ide refactor sort-knots -e main.ink                 # preview
brink ide refactor sort-knots --write -e main.ink         # apply

# Move a stitch between knots, capturing a patch for review.
brink ide refactor move-stitch intro.evidence --to clues --patch -e main.ink
```

> **Same-file references in `promote-stitch` / `demote-knot`.** These currently
> do not rewrite references *within the same file* to the moved symbol, so the
> promotion/demotion can leave a dangling divert. The safety gate catches this:
> `--write` refuses (and preview shows the would-be breakage). Use `--unsafe`
> only if you intend to fix the references yourself.

---

## JSON output & stability

Every command supports `--format json`. The shapes are intended to be **stable**:
fields will be **added** but not renamed or removed within a major version, so
scripts that read known keys keep working. Agents should depend on named keys,
not field order or absence.

Locations are always `{ "path", "line" (1-based), "col" (1-based), "byte_start",
"byte_end" }`.

| Command | JSON shape |
|---------|-----------|
| `def` | `{ name, kind, location }` |
| `references` | `{ name, kind, count, references: [location] }` |
| `symbols` | `[{ name, kind, location, detail?, children? }]` |
| `unused` | `[{ name, kind, location }]` |
| `check` | `[{ severity, code, message, location }]` |
| `hover` | `{ content, location }` |
| `signature` | `{ label, documentation, parameters: [string], activeParameter }` |
| `graph` | `{ nodes: [{ id, name, kind, parent }], edges: [{ from, to, kind }] }` |
| `lines` | `[{ line, element, depth }]` |
| `actions` | `[{ title, kind }]` |
| `rename` (preview) | `{ edits: [{ location, old, new }], introducedDiagnostics, safe }` |
| `move-file` / `refactor` (preview) | `{ diff, files: [path], introducedDiagnostics, safe }` |

`introducedDiagnostics` is an array of `{ severity, code, message, location }`;
`safe` is `true` when it is empty.

---

## Recipes for agents

```sh
# Is a symbol used anywhere? (exit code, no parsing)
brink ide references my_var --exists -e main.ink && echo "used" || echo "dead"

# Fail CI if there is any dead code.
brink ide unused -e main.ink

# Fail CI if a refactor would break the project (no write, just the gate).
brink ide rename old --to new --write -e main.ink   # exit 1 = unsafe

# Where is this knot defined? (just the path:line:col)
brink ide def my_knot -e main.ink

# Count references with jq.
brink ide references gold -e main.ink --format json | jq .count

# Produce a reviewable patch without touching the tree.
brink ide rename gold --to coins --patch rename.diff -e main.ink
git apply rename.diff
```
