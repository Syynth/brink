# CLI exposure of `brink-ide` — operations & queries inventory

Working design artifact for [epic #289](https://github.com/Syynth/brink/issues/289):
expose the `brink-ide` query/refactor surface as scriptable CLI commands. This
catalogs **everything `brink-ide` implements today** and proposes how each maps
to a command. Nothing here needs new analysis — it's a CLI/addressing/output
layer over existing functions.

## Conventions (apply to every command)

### Workspace
Most operations need the whole project, not one file. Every command builds an
`IdeSession` from an **entry `.ink`** and resolves `INCLUDE`s, exactly like
`brink compile`:

```
brink ide <cmd> [--entry main.ink] [--file extra.ink ...] ...
```
`--entry` defaults to a single `.ink` arg or an autodetected `main.ink`.

### Addressing — name-first, position-fallback
The motivating queries are name-based. A **qualified symbol name** resolves to a
definition; `--at` is the editor/disambiguation fallback.

- `<symbol>` — a qualified name (grammar below over the 10 `SymbolKind`s).
- `--at FILE:LINE:COL` — cursor address (1-based line/col), mapped to a byte
  offset via `LineIndex`. Drives the offset-based `brink-ide` fns directly.
- `--kind <k>` — disambiguate when a bare name matches multiple kinds.

**Symbol-name grammar** (kinds: Knot, Stitch, Variable, Constant, List,
ListItem, External, Label, Param, Temp):

| Form | Resolves to |
|------|-------------|
| `intro` | knot, or top-level var/const/list/external named `intro` (use `--kind` if ambiguous) |
| `intro.evidence` | stitch `evidence` in knot `intro` |
| `Colors.Red` | list item `Red` of list `Colors` |
| `intro.evidence.found` / `intro.found` | label `found` — ink's dotted path (`knot.stitch.label` or `knot.label`), matching inklecate |
| `damage(weapon)` | param `weapon` of knot/function `damage` |

The dotted label path is the same one ink itself uses, so it reads naturally. It
can collide with a stitch of the same dotted name (`knot.x` = stitch `x` or label
`x` under the knot); resolve those rare cases with `--kind label`. (Params use the
`fn(param)` form since they share their owner's dotted namespace.)

### Output
- `--format text` (default, human-readable) · `--format json` (stable shape, for `jq`).
- Locations render as `path:line:col` in text; JSON adds `{file, byteStart, byteEnd, line, col}`.
- **Exit codes**: lint-style queries set status (e.g. `--exists` → 0 if referenced, 1 if not; `unused` → 1 if any found) so they compose in CI.

### Help & discoverability
The CLI is `clap` derive-based; `brink ide` follows that pattern and raises the bar
(these commands are scripting-first, so help *is* the product):

- Every command and flag carries a `///` doc comment (clap help), as today.
- Each `brink ide <cmd>` gets an **`after_help` examples block** — the name-form,
  the `--at` form, a `--format json | jq` pipeline, and (for mutations) a `--patch`
  example. Discoverability of the addressing/output flags lives in the examples.
- Custom `value_name`s (`<SYMBOL>`, `FILE:LINE:COL`, `<NEW_NAME>`) so usage lines
  read clearly.
- A short shared `long_about` on the `ide` subcommand group explaining
  addressing + output + safety once, so per-command help stays terse.
- Stable exit-code contract documented in help: `0` ok, `1` query-false / unsafe /
  diagnostics-present, `2` usage error (clap default).

### Mutations — output modes
Every rename/move/refactor computes a set of cross-file edits; *what is done with
them* is selected by mutually-exclusive flags:

- **preview** *(default)* — print the edits for a human: a readable unified diff in
  text mode, `FileEdit[]` in `--format json`. Touches nothing.
- **`--patch [FILE]`** — emit a **`git apply`-able** unified diff (proper
  `diff --git a/… b/…` + `@@` hunk headers, project-relative paths) to stdout, or
  to `FILE` if given. The scriptable artifact: review it, attach it to a PR, or
  `git apply` / `patch -p1` it later. Disk-safe — it never writes the target files.
- **`--write`** — apply the edits to the project files in place.

Non-zero exit on hard conflicts the operation itself rejects (`NameCollision`,
`DestinationExists`, `IllegalNesting`, `InvalidReorder`, …), in any mode.

The safety check (below) couples to the mode by *what leaves the process*:
**preview** is always informational — it prints the edits **and** any
newly-introduced diagnostics and never aborts. **`--patch` and `--write`** produce
something actionable, so they are guarded: by default they refuse to emit a patch
or write to disk if the refactor introduces new diagnostics, and require
**`--unsafe`** to proceed anyway.

### Refactor safety — `--safe` / `--unsafe`
A refactor can be *structurally legal* yet still **break the project** — a rename
that shadows another symbol (E-code for shadowed names), a `move-stitch`/`promote`
that leaves a now-unresolvable bare reference, a `demote` that re-qualifies into a
collision the legality check didn't model. So every mutating command verifies the
result before committing:

1. Capture the **baseline diagnostics** (`Diagnostic{code,file,range}`) of the
   current project.
2. Apply the edits in-memory and **re-analyze** (cheap — `brink-db` re-lowers only
   changed files).
3. Diff the diagnostic sets → the **newly-introduced** diagnostics attributable to
   the refactor.

The guarantee is tied to **what the command produces**, not a global default:

- **preview** *(default mode)* — never gated. Always prints the edits **and** the
  newly-introduced diagnostics together, and exits 0. A "show me what would happen,
  including what would break" view.
- **`--patch` / `--write`** — **safe by default**: if the refactor introduces any
  new diagnostic, abort — print them, produce nothing, exit non-zero. Pass
  **`--unsafe`** (alias `--force`) to emit the patch / write anyway, still printing
  the new diagnostics as warnings so breakage is never silent.
- **`--check`** — a pure CI gate: run the safety analysis, print the would-be
  introduced diagnostics, exit 0/1 by safety, never produce a patch or write.

Pure-text refactors that cannot change resolution (`reorder-*`, `sort-*`,
`format-*`) introduce nothing, so `--patch`/`--write` never trip the gate.
`move-file` treats "project still analyzes clean after the INCLUDE rewrite" as its
safety condition.

JSON output always includes `{ edits, introducedDiagnostics, safe: bool }` so a
script can decide for itself regardless of exit code.

---

## 1. Navigation & reference queries (read)

| Command | `brink-ide` fn | Notes |
|---|---|---|
| `brink ide def <sym\|--at>` | `navigation::goto_definition` → `LocationResult{file,range}` | Prints the declaration location. |
| `brink ide references <sym\|--at> [--include-decl]` | `navigation::find_references` | All usage sites across files; `--include-decl` adds the definition. |
| `brink ide references <sym> --exists` | `find_references` (count) | **Exit 0 if referenced, 1 if not** — the "is this var/knot referenced?" check. `--count` prints the number. |
| `brink ide unused [--kind knot,var,list,…]` | `find_references` over the symbol index | Lists every declared symbol with **no references** (dead knots, unused vars/lists/externals). Exit 1 if any. The scriptable inverse of `--exists`. |

## 2. Symbols / outline (read)

| Command | `brink-ide` fn | Notes |
|---|---|---|
| `brink ide symbols [--file F]` | `document::document_symbols` → `DocumentSymbol{name,kind,detail,range,full_range,children}` | Outline of one file: knots with stitches nested; vars/lists/externals at top. |
| `brink ide symbols --search <q>` | `document::workspace_symbols` → `WorkspaceSymbol{name,kind,file,range}` | Project-wide substring search (the LSP workspace-symbol query). |

## 3. Information queries (read)

| Command | `brink-ide` fn | Notes |
|---|---|---|
| `brink ide hover <sym\|--at>` | `hover::hover` → `HoverInfo{content(markdown),range}` | Kind tag + signature + initializer + `///` docs + "Defined in `path`". Falls back to builtin docs. |
| `brink ide signature --at FILE:L:C` | `signature::signature_help` → `SignatureInfo{label,parameters[],active_parameter,documentation}` | Signature of the innermost active call; position-only (it's mid-call). |
| `brink ide values --at FILE:L:C` | `signature::argument_value_completions` | Pickable values for the argument's semantic type (manifest `--manifest`, host values N/A from CLI). |
| `brink ide complete --at FILE:L:C` | `completion::detect_completion_context` + `is_visible_in_context` | The symbols valid at a position (divert targets / expr / logic / args / general). Niche for CLI; useful for editor backends. |

## 4. Diagnostics (read)

| Command | source | Notes |
|---|---|---|
| `brink ide check [--severity …]` | analysis `Diagnostic{file,range,message,code}` (codes E001–E043) | Structured project diagnostics independent of a full compile. `--format json` for CI; exit 1 if any error. `--external-check off\|warn\|error` mirrors `ExternalCheckSeverity`. |

## 5. Symbol & file refactors (mutating)

| Command | `brink-ide` fn | Notes |
|---|---|---|
| `brink ide rename <sym\|--at> <new> [--write]` | `rename::prepare_rename` + `rename` → `RenameResult{edits[]}` | Renames a knot/stitch/var/const/list/list-item/label/param + all references cross-file. Rejects externals (built-ins). |
| `brink ide move-file <old> <new> [--write]` | `file_rename::rename_file` → `MoveResult{new_source,cross_file_edits[]}` | Move/rename a file and rewrite inbound + outbound `INCLUDE`s. Errors on `DestinationExists`/`NotFound`. |

## 6. Structural refactors (mutating)

All emit a `MoveResult`/new-source; dry-run by default. From `structural_move.rs`
+ `code_actions.rs` + `formatting.rs`.

| Command | `brink-ide` fn | Notes |
|---|---|---|
| `brink ide refactor sort-knots [--file F]` | `formatting::sort_knots_in_source` | Alphabetize knots (preamble preserved). |
| `brink ide refactor sort-stitches <knot>` | `formatting::sort_stitches_in_knot` | Alphabetize stitches in a knot. |
| `brink ide refactor format-knot <knot>` / `format-stitch <knot.stitch>` | `formatting::format_region` | Format just that region. (`brink fmt` already does whole files.) |
| `brink ide refactor reorder-knot <knot> up\|down` | `structural_move::reorder_knot` | Pure text move (no ref changes). |
| `brink ide refactor reorder-stitch <knot.stitch> up\|down` | `structural_move::reorder_stitch` | |
| `brink ide refactor reorder-knots <a,b,c>` / `reorder-stitches <knot> <a,b,c>` | `reorder_knots` / `reorder_stitches` | Permutation reorder (validates permutation). |
| `brink ide refactor move-stitch <src.stitch> <dest-knot> [--write]` | `structural_move::move_stitch` | Move a stitch between knots + re-qualify references. |
| `brink ide refactor promote-stitch <knot.stitch> [--write]` | `structural_move::promote_stitch_to_knot` | `= s` → `=== s ===`; references `knot.s` → bare `s`. |
| `brink ide refactor demote-knot <knot> <dest-knot> [--write]` | `structural_move::demote_knot_to_stitch` | `=== k ===` → `= k`; references `k` → `dest.k`. Rejects if `k` has stitches (no triple nesting). |
| `brink ide refactor convert-line --at FILE:L:C <narrative\|choice\|sticky-choice\|gather\|choice-body>` | `line_convert::convert_element` → `TextEdit` | Convert a line's structural type, preserving weave depth. |

> Note: `code_actions::code_actions` is the *cursor-driven* surface (what's
> applicable at a position) — useful for a `brink ide actions --at FILE:L:C` that
> lists available refactors at the cursor, each runnable by the verbs above.

## 7. Whole-project extraction / visualization (read)

| Command | `brink-ide` fn | Notes |
|---|---|---|
| `brink ide graph [--format json\|dot]` | `story_graph::story_graph` → `StoryGraph{nodes[],edges[]}` | The knot/stitch flow graph (Divert/Choice/Tunnel/Thread edges, END/DONE nodes). **High CLI value**: `--format dot` pipes to Graphviz; JSON for analysis. |
| `brink ide lines [--file F]` | `line_context::line_contexts` → `LineContext[]` per line | Per-line structural classification (element + weave depth + tags). Scriptable "list all choices/diverts/logic lines". |

## 8. Editor-rendering features (low CLI value — expose only as raw dumps if needed)

These exist for editors and have little standalone CLI use; expose behind a
`brink ide dump <kind>` escape hatch (JSON only) **only if a consumer asks**:

| Capability | `brink-ide` fn |
|---|---|
| Semantic tokens | `semantic_tokens::semantic_tokens` (+ legend) |
| Folding ranges | `folding::folding_ranges` |
| Inlay hints | `inlay_hints::inlay_hints` |
| Color hints | `color::color_hints` |
| Argument widgets | `argument_widgets::argument_widgets` |

(Out of scope for v1; listed for completeness.)

---

## What ships when

- **Phase 1 (foundation + headliners)**: session-from-entry, name resolver,
  `--at` addressing, `--format` framework → `def`, `references` (+`--exists`).
- **Phase 2**: `symbols`/`symbols --search`, `rename`, `unused`, `check`.
- **Phase 3**: `hover`, `signature`/`values`, `move-file`, all `refactor *`,
  `graph`, `lines`, `actions`.
- **Phase 4**: docs (book "The CLI" chapter) + JSON-shape stability statement.

## Cross-cutting decisions still open (epic #289)
1. Namespace: `brink ide <cmd>` (grouped) vs top-level verbs.
2. JSON shape: versioned/stable contract vs "unstable for now".
3. Name ambiguity: error+`--kind` hint vs print all matches.
4. Mutation default: dry-run + `--write` (proposed) vs write + `--dry-run`.
5. What counts as a "newly-introduced" diagnostic: a diagnostic *code+location* not
   in the baseline (precise, proposed) vs a raw count increase (cheaper but noisier
   under shifting offsets).
6. Structural refactors in v1 vs deferred to Phase 3 (proposed: defer).
7. Manifest input: how the CLI supplies a host manifest (`--manifest file.json`)
   for type-aware queries (hover types, value completions, external checks).

Resolved: labels addressed by ink's dotted path (`knot.stitch.label`); params by
`fn(param)`. Mutation safety is mode-coupled — **preview** always shows edits +
introduced diagnostics (ungated); **`--patch`/`--write`** are safe-by-default and
require **`--unsafe`** to bypass.
