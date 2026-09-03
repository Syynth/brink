# Auto-fix — fixers, tiers, batching, and the surfaces that call them

Status: **RULED 2026-09-01** (model, tiers, scope, policy layering,
surfaces, CLI) — with one item marked *tentative* (§6.2, the
app-setting ↔ `brink.toml` relationship). Rulings in
`docs/decision-log.md` under the same date ("Auto-fix: lazy per-code
fixers…", "Fix scope is the compilation…", "Auto-fix policy layering…").
Depends on `docs/observable-semantics-spec.md` (what *Safe* means) and
the oracle it describes (#3371). Epic: #3374.

## 1. Why this exists

`cargo fix` / `eslint --fix` for brink diagnostics: an author (or a CI
step) asks for every mechanical fix to be applied, and the compiler
does it. The repo already has three diagnostic-keyed quick-fixes
(`brink-ide`'s `import_fix`, `value_call_fix`, `creation_site_fix`) and
a code-actions menu; what is missing is one model that makes fixes
*batchable* with a trustworthy notion of *safe*, and the surfaces that
let an author say "fix everything" — in the studio, on the command
line, and from any LSP client.

## 2. The model — RULED

Diagnostics stay **data**. `Diagnostic { file, range, message, code }`
is unchanged: it crosses the wasm boundary, serializes into the
Problems panel, LSP and CLI JSON, and is constructed at ~200 sites; a
per-diagnostic trait would buy nothing the `DiagnosticCode` enum's
metadata methods (`severity`, `is_overridable`, `title`, …) do not
already give.

Fixes are **behaviour**, computed **lazily** by a per-code fixer —
never during analysis, only when a surface asks. (Computing edits for
every diagnostic on every keystroke is exactly the cost the live-typing
work has been fighting.)

```rust
// brink-ide::fix
pub enum Applicability { Safe, Suggested, Placeholder }

pub struct Fix {
    pub code: DiagnosticCode,              // which diagnostic this discharges
    pub title: String,
    pub applicability: Applicability,      // per instance; never above the fixer's declared max
    pub edits: Vec<FileEdit>,              // minimal TextEdits — the ONLY fix currency; may span files
    pub caret: Option<(FileId, TextSize)>, // Placeholder only: where the author fills the hole
}

pub struct FixCx<'a> { pub db: &'a ProjectDb /* the compilation */, /* … */ }

pub trait Fixer: Sync {
    fn code(&self) -> DiagnosticCode;
    /// Declared upper bound — lets surfaces count "N safe fixes" without computing an edit.
    fn max_applicability(&self) -> Applicability;
    /// On demand: cursor menu, a Problems row, a batch. Never during analysis.
    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix>;
}

pub static FIXERS: &[&dyn Fixer];                              // registry; one entry per code with fixes
pub fn fixes_for(cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix>;  // dispatch on d.code
```

Decisions folded into the shape (RULED as the defaults, 2026-09-01):

- **Trait-object registry** (`static FIXERS`) with a registry test
  (unique `code()`s; every `Safe` fixer has a §3 fixture) — not a
  `match` on the code.
- **`Vec<FileEdit>` is the only currency.** The three existing fixers
  migrate to it in the PR that introduces `Fix`; `resolve_code_action`'s
  whole-source return stays for structural refactors only.
- **Static `max_applicability`, per-instance actual** — some fixers are
  safe in some instances and withheld in others (the E080 precedent);
  the static bound is what makes counts cheap. Actual ≤ max is asserted.
- **No `data` payload up front.** A fixer reconstructs from `d.range`,
  the source, and db facts (`value_call_fix` already reads
  `BodyTypes::value_calls` rather than the message — the rule). If a
  specific code provably cannot, it gains an optional serde `data`
  payload *for that code*.
- **Placement**: fixers in `brink-ide` (home of the existing three;
  `brink-cli` already depends on it); the trace helper in
  `brink-test-harness`; `Safe` fixtures in an integration-test crate
  that sees both.

*As built (#3417, milestone 2):* the `Safe` helper is
`brink_test_harness::fix::assert_safe_fix`
(`crates/internal/brink-test-harness/src/fix.rs`), where the spec put it, and
the fixtures are on disk at `tests/fix/<code>/{before,expected}.{ink,brink}`
rather than in an integration-test crate. §3's `Safe` obligation is **split
across two crates and neither half is optional**: `brink-test-harness`
depends on `brink-ide`, so the registry test cannot call the oracle, and the
oracle's sweep cannot be the only enforcement without silently letting a
`Safe` fixer ship with no fixture at all. `brink_ide::fix`'s registry test
demands the fixture exists and is well-formed; the harness's own
`tests/fix_safe_obligations.rs` enumerates the same `FIXERS` registry and
runs each `Safe` fixer's fixture through `assert_safe_fix`.

*As built (#3377, milestone 1):* the model lives in `brink_ide::fix`
(`Applicability`, `Fix`, `FixCx`, `Fixer`, `FIXERS`, `fixes_for`) with one
addition the surfaces needed — `fixes_at(cx, file, offset)`, the
`Select::AtOffset` pull of §4, which collapses *identical* fixes: one site
can carry several diagnostics of the same code that a single edit discharges
(E080 reports one per unbound `ref` param), and the menu must show that entry
once. Both §3 helpers (`assert_fix_discharges` and the stub
`assert_safe_fix`) landed in `brink-ide`'s own `fix::obligations` rather than
`brink-test-harness`: the registry test that enforces them is `brink-ide`'s,
and the trace half has nothing to delegate to until #3371's oracle lands — at
which point it moves to the harness in place.

## 3. Tiers and their test obligations — RULED

A tier is not a label somebody typed; each names the test that backs it.

| Tier | Meaning | Batchable | Obligation (per fixer, enforced by the registry test) |
|---|---|---|---|
| **Safe** | observably equivalent (`docs/observable-semantics-spec.md` §2) **and** translation identity holds (§2.2) | yes — fix-all, `brink fix`, on-save, LSP `fixAll` | `assert_safe_fix(fixer, fixtures)`: compile → apply → recompile → `trace_diff` over explored runs is empty → line-table identity unchanged for untouched lines |
| **Suggested** | probably what the author meant, but changes meaning or loses text | only when a project promotes the code (§6.1); otherwise one instance per explicit click | `assert_fix_discharges`: the diagnostic is gone and **no new error** appears — the property `StructuralResult.safe` already computes |
| **Placeholder** | leaves a hole the author must fill (`caret` set) | never | discharge check only; the surface moves the caret into the hole |

Deleting a duplicate `LIST` item changes host-readable state and is
therefore Suggested, not Safe. Deleting unreachable code cannot pass
the trace check by construction — that is why it cannot be Safe
however sensible.

### 3.1 A Safe fixer discharges a non-blocking diagnostic — by construction

*Measured while building `assert_safe_fix` (#3417), and a consequence of
§2's definition rather than a new rule.* Observable equivalence compares
**two programs**. If the pre-fix source does not compile there is no
program on the left, so §2 says nothing about the transformation and no
amount of fixture work can make it say something. `assert_safe_fix`
reports that as its own verdict (`NoPreImage`) rather than as a passing
or failing comparison.

So a `Safe` fixer's diagnostic must be one compilation survives — in
practice `Warning` or `Info` severity, or a code a project has turned
down. Every code on §9's Safe list already is. The four **migrated**
fixers are the counterexample that makes the rule concrete: E025, E063,
E080 and E081 all block compilation (E063's base severity is
`types`-policy-dependent, and the policy under which it fires at all,
`types = "strict"`, is the one that makes it an error), so all four record
`NoPreImage` and could not declare `Safe` even if someone wanted them to.
They already declare `Suggested`; this is the mechanical confirmation.
The fixtures and their recorded verdicts live in `tests/fix/` — see that
directory's `README.md`.

Two further properties of the helper, both load-bearing:

- **Translation identity is allowance-based, not all-or-nothing.** §2.2
  asks for identity "for every line the transformation did not itself
  edit", so a fixture declares the units its fix necessarily rewrites in
  a `rewrites.txt`; every other reported change fails. The rewritten
  units are reported either way.
- **A vacuous comparison is not a pass.** Two programs that both run out
  of content immediately agree on everything. The helper counts the
  pre-fix program's *content* events (lines, choice presentations,
  external calls, probe results) across the whole explored run set and
  refuses to call an empty one equivalent.

## 4. Scope is the compilation, not the file — RULED

`FixCx` *is* the compilation (`ProjectDb`: for ink the entry's INCLUDE
tree, for native the module graph). A fixer produces whatever edits
the fix needs, wherever they land; there is no file gate in the fixer
contract — "if they need to be cross-file to work, they need to."

What varies between surfaces is the **selection of diagnostics**, not
the set of writable files:

```rust
pub enum Select { All, InFile(FileId), AtOffset(FileId, TextSize), One(DiagnosticId), Codes(Vec<DiagnosticCode>) }
```

The trace-equivalence check is compilation-wide already, so *Safe* and
the mechanism line up. Consequence, documented rather than
special-cased: **a per-file selection (fix-on-save, "Fix all in this
file") may edit other files.** In the studio the cross-file edits
apply to those buffers and mark them dirty (the road rename already
uses — nothing is written the author has not seen); the CLI and LSP
`fixAll` write every touched file, as `cargo fix` does.

*As built (#3418, milestone 3):* `Select` is a **filter**, not the enum
sketched above — `Select { codes: Option<Vec<DiagnosticCode>>, tiers:
Option<Vec<Applicability>>, range: Option<(FileId, Option<TextRange>)> }`, per
the milestone's own shape. The enum's arms are its constructors: `Select::all()`
is `All`, `.in_file(file)` is `InFile` (the *inner* `None` — no `&ProjectDb`
needed to construct it), `.at_offset(file, offset)` is `AtOffset` (an empty
range, matched inclusively — the same narrowing `fixes_at` does),
`.with_codes(…)` is `Codes`. `One(DiagnosticId)` has no equivalent: there is no
`DiagnosticId` in the tree to name one with. `tiers` is the new half the enum
had nowhere to put — it filters on the *offered fix's* tier, which is why it
cannot be decided before `fixes_for` runs.

`.in_file`'s range is deliberately not resolved to a byte span at construction
time: `files`/`matches` re-derive "the whole file" from the file's *current*
length on every call instead. A fix's own length is not stable across a
`fix_all` fixpoint — round one's insertion grows the file, shifting every
diagnostic after it — so a length frozen at `.in_file(file)`'s call site would
go stale after the very first round and silently strand any diagnostic that
shifted past it. `.at_offset`, by contrast, freezes an explicit `TextRange`
(an empty range at one offset): that selection is documented single-round —
the cursor-menu shape `fixes_at` already is — and is not carried across
`fix_all` rounds by any caller today.

`Select::files` is where "scope is the compilation" is cashed out:
`ProjectDb::compilation_closure` when an entry is set, and every loaded file
(id-ordered) when one is not — the shape an editor session without a
discovered `brink.toml` has.

## 5. Batching — one algorithm for every batch surface

```rust
pub fn apply_round(cx, select, policy: &FixPolicy) -> Round
// 1. for every selected diagnostic: fixes_for(); keep only fixes `policy` admits
//    (Safe always; Suggested only for codes the policy promotes; Placeholder never)
// 2. order edits by (file, start); an edit overlapping an already-kept edit is DROPPED, not merged
// 3. return the edit set + the dropped list ("N fixes deferred — run again")
pub fn fix_all(cx, select, policy, max_rounds: u8 /* 5 */) -> Report
// round → apply → re-analyze the compilation → repeat until a round applies nothing or the cap hits.
// The report names any diagnostic that reappeared: a fixer that does not discharge its own
// diagnostic is a bug, reported rather than looped on.
```

Dropping overlaps (never merging) is the load-bearing simplification:
no two fixes reason about each other; the fixpoint loop resolves
collisions a round later on fresh analysis, and the round cap is the
guard against unbounded growth (a standing repo rule).

*As built (#3418, milestone 3):* in `brink_ide::fix::batch`, with step 1 and
step 2 split into `collect` and `plan` so each is testable on its own;
`apply_round` is the two composed. Four points the sketch left open, decided
here:

- **Overlap is *touching*, not just intersecting.** Two edits of one file
  collide when their byte ranges meet at all — adjacent ranges included, and
  two pure insertions at the same offset included. That last case is not
  exotic: it is exactly what two `E025` auto-imports into one file produce.
- **The unit of dropping is the fix, not the edit.** A fix's edits are one
  atomic change and may span files (§4); applying half of one would leave the
  compilation in a state no fixer intended. A candidate any of whose edits
  touches an already-kept edit is deferred whole. "Earliest range wins" orders
  candidates by their earliest edit — `(file, start, end)`, then code and
  title as a stable tiebreak — so the same input always yields the same edit
  order.
- **`fix_all` takes the session, not `FixCx`.** `FixCx` is a read-only
  borrow of the `ProjectDb`, and re-analysis is a mutation, so the fixpoint
  loop takes the `IdeSession` that owns the db.
- **The cap is reported by re-running the selection.** After the loop,
  `fix_all` collects once more without applying: whatever the policy still
  admits is `Report::remaining`, and `Report::cap_hit` says the loop ran out
  of rounds instead of converging. A fixer that fails to discharge its own
  diagnostic therefore surfaces as a cap breach naming that diagnostic —
  reported, not looped on, exactly as this section requires.

`Report { applied: Vec<FixSite>, skipped_overlap: usize, remaining:
Vec<FixSite>, rounds: u8, cap_hit: bool }`, where `FixSite { code, file,
range }` is the *diagnostic's* site (the file an edit lands in may differ,
§4) and `skipped_overlap` counts deferrals summed across rounds. A batch
never fixes a suppressed diagnostic: `collect` runs
`brink_ir::suppressions::apply_suppressions` first, so it sees what the
Problems panel sees.

## 6. Policy — what is batchable, and when the editor acts

### 6.1 `brink.toml [fix]` — project-owned, RULED

```toml
[fix]
E033 = "auto"   # promote a Suggested fix to batch for this project
E014 = "off"    # never offer this fixer here ("it's annoying")
# absent ⇒ "ask": offered per click only (Suggested) / batchable (Safe)
```

Shaped like `[lints]` (`brink-project-config`), resolved by the same
kind of function (`effective_fix_policy`), and it **travels with the
project**: the CLI, the LSP, the studio and any host running
`brink compile` see one policy. Edited in the studio from the existing
lints table (`packages/studio-ui/src/LintSettings.tsx`) as a **Fix**
column beside severity — RULED "it can even go in the existing
diagnostics UI".

*As built (#3418, milestone 3):* the *type* the batch reads is
`brink_ide::fix::policy::{FixPolicy, FixMode}` — `FixMode { Auto, Ask, Off }`
plus per-code overrides over the tier defaults (`Safe → auto`,
`Suggested → ask`, `Placeholder → off`). Milestone 3 takes a `FixPolicy` as an
**input**; where it comes from — the `[fix]` table, `effective_fix_policy` in
`brink-project-config`, and the studio's Fix column — is #3419's, and nothing
about the source is decided here. One rule the type enforces rather than
leaving to callers: `FixPolicy::admits` refuses `Placeholder` unconditionally,
so promoting a Placeholder code to `"auto"` still does not batch it (§3,
"Batchable: never"). `"off"` withdraws a code from both batching and offering.

*As built (#3419, milestone 4):* the *source* side. `[fix]` parses into
`ProjectConfig::fix: BTreeMap<String, FixPolicy>` — note this is a
different, unrelated `FixPolicy`: a plain `Off < Ask < Auto` enum in
`brink-project-config`, not milestone 3's `brink_ide::fix::policy::FixPolicy`
struct; wiring one into the other (an override map keyed by the project's
per-code entries) is not built yet. Validated the same way `[lints]`'s value
is (`"off" | "ask" | "auto"`, a wrong TOML type or an unrecognized spelling
is a `ConfigError`, never a panic; an unrecognized *code* is accepted here
regardless — this crate stays dependency-free of the real `DiagnosticCode`
set, same split `validate_lint_code` uses). The diagnostic `[lints]` raises
for an unrecognized *code* (`validate_lint_code`, in `brink-analyzer`) is
still owed for `[fix]` — nothing consumes `ProjectConfig::fix` yet to hang
it off, so it's tracked as a follow-up rather than built here (#3447, to
land when a fix-policy engine first reads the table and reconciles it with
milestone 3's type). `ProjectConfig::effective_fix_policy(code,
app_ceiling: Option<FixPolicy>)` is the one function this section and
§6.2 both resolve through — `FixPolicy` is declared `Off < Ask < Auto`
so the intersection is just `project.min(ceiling)`. The studio's Fix
column writes `[fix]` through the exact same generic `setTomlString`
call `[lints]` already used (`packages/studio-store/src/toml-edit.ts`)
— a different table name, nothing else.

### 6.2 The app setting — personal, a ceiling — TENTATIVE

An app-scope setting, like format-on-save, saying *when* the editor
runs the batch and how far it may go:

    Fix on save:  Off | Safe only | Everything the project allows

Effective on-save policy = **app ceiling ∩ project policy**. The app
can only be *more conservative* than the project, never more
aggressive: a team promoting `E033` in `brink.toml` does not force it
onto an author who chose "Safe only"; an author cannot get `E033`
deletions on save in a project that never promoted it. No
project-scope duplicate of the toggle — the project's opinion already
lives in `brink.toml`.

Explicit actions may widen per run (`brink fix --suggested E033`, a
Problems-row click on a Suggested fix); the implicit action (save)
only narrows. *Marked tentative: the maintainer's one stated
uncertainty is exactly this relationship.*

## 7. Surfaces — RULED

All of them are callers of `fixes_for` / `fix_all`; none reconstructs
anything.

- **Code-actions menu** (`packages/ink-editor/src/code-actions.ts`):
  the fixes for the diagnostics under the cursor, every tier; one click
  applies one fix; Placeholder moves the caret. **Under the cursor means
  on the squiggle**: the selection is the diagnostics whose own range
  covers the offset, so a fix keyed to a diagnostic anchored at (say) a
  call's identifier is not offered from inside the argument list. That is
  a narrowing relative to the pre-#3377 quick-fixes, which searched from
  the cursor for a syntax node and then looked for a diagnostic anywhere
  inside it.
- **Editor context menu**: the same entries for the diagnostic under
  the pointer, plus "Fix all safe in this file".
- **Problems panel** (`ProblemsView.tsx`, `ProblemsContextMenu.tsx`):
  a per-row **Fix** (and the row's context menu lists every offered
  fix beside the existing suppress items); a header **Fix all safe
  (N)** for the compilation, `N` from `max_applicability` counts.
- **Command palette**: Fix all safe (file / project).
- **On save**: §6.2's policy, run on the save road before the write.
- **LSP** (`brink-lsp`): each `Fix` → `CodeAction { kind: quickfix,
  diagnostics: [d], edit: WorkspaceEdit }`; `source.fixAll.brink` →
  `fix_all(All, Safe ∩ project)` — which gives VS Code fix-on-save.
  *As built (#3377):* only the `Fix` → `CodeAction { kind: quickfix, edit }`
  half exists, because the currency change forced it — the three migrated
  fixers would otherwise have gone dark over LSP. `diagnostics: [d]` and
  `source.fixAll.brink` remain this surface's own milestone.
  *As built (#3422, milestone 7):* both remaining halves. `fix_code_actions`
  inlines the diagnostic-dispatch loop `fixes_at` runs (rather than calling
  it) so it can pair each collapsed `Fix` with the diagnostic that produced
  it, and attaches that as the action's single-element `diagnostics`. Two
  independent suppression paths both apply before a fix is ever offered,
  matching what the Problems panel shows: the diagnostic list is run
  through `brink_ir::suppressions::apply_suppressions` first (a
  `// brink-disable-file`/`@[allow(…)]`-suppressed diagnostic is dropped
  outright, same as `brink_ide::fix::batch::collect` and the publish path),
  and each survivor's action is skipped when `convert::diagnostic_to_lsp`
  (the same conversion the Problems-panel road uses) returns `None` for a
  `[lints] allow`-leveled code (#3173).
  `source.fixAll.brink` is `fix_all(Select{tiers: [Safe]}, FixPolicy::
  default())` — `[fix]`-table promotion does not reach it yet, since
  reconciling `brink.toml`'s table with `brink_ide::fix::policy::FixPolicy`
  is #3447's, not built here (§6.1) — run on a private scratch
  `IdeSession` that mirrors the live project's `AnalysisOptions` and every
  loaded file's current source — **but not its native/ink roots or compile
  entry**: `IdeSession` exposes no root or entry setter, so native module
  identity in the scratch mints from each file's full absolute path rather
  than the live project's mounted root, and with no entry configured, fix
  selection falls back to every loaded file (including the mounted stdlib)
  instead of the live compile closure. The fixes computed here can
  therefore diverge from what the live diagnostics show; closing that gap
  is tracked as issue #3458, which milestone 8 must land before the first
  `Safe` fixer makes this path live. **Never the live db** either way: a
  `codeAction` request fires continually to populate a client's lightbulb
  menu, not only right before an edit is accepted, so a batching pass that
  mutated the live project here would silently pre-fix files whose open
  buffers had not actually changed. The fixpoint's final state is reduced
  to one whole-file `TextEdit` per changed file (not a minimal per-line
  diff) — simple and always valid, at the cost of not preserving an
  unrelated concurrent edit to the same file made between the request and
  the client applying it, the same trade-off any whole-document formatter
  edit already accepts; a file whose path can't round-trip through
  `Url::from_file_path` or whose length overflows `u32` abandons the whole
  batch (`return None`) rather than shipping a `WorkspaceEdit` with the
  rest silently applied. The action itself is computed only when a
  `codeAction` request's `context.only` explicitly names
  `source.fixAll.brink` (or a shared prefix, `"source"`/`"source.fixAll"`),
  and even then only after a cheap check — no scratch session is built at
  all — that some registered fixer's `max_applicability` admits the
  selected tiers; the whole-compilation pass itself is too expensive to pay
  on every unfiltered lightbulb-menu request, and VS Code's own
  fix-on-save always sends that filter. No registered fixer declares
  `Safe` yet (§9's first-wave candidates are a later milestone), so
  `source.fixAll.brink` is a correct no-op today — it starts batching the
  moment the first one lands, once #3458 also closes.
- **wasm DTO** (`@brink-lang/web`): `FixJs { code, title,
  applicability, edits: FileEditJs[], caret? }`. *As built (#3377):*
  `fixes_at` / `fixes_at_doc` return it (offsets are UTF-16 file-absolute,
  the editor boundary convention), and `apply_fix` / `apply_fix_doc` take a
  chosen `FixJs` back and answer the `StructuralResult` shape the studio's
  existing cross-file apply seam already consumes — so a fix reaches the
  buffers by the same road a rename does. `resolve_code_action` stays for
  structural refactors.

*As built (#3420, milestone 5):* the studio half of this section — the
Problems panel's per-row **Fix** and header **Fix all safe (N)**, the row's
context-menu fix entries, the editor context menu's fix group plus "Fix all
safe in this file", the palette's two `fix.allSafe*` commands, and fix on
save. Six decisions the sketch left open, decided here:

- **The header's `N` is `collect().len()`, not a `max_applicability` tally.**
  The sketch's "from `max_applicability` counts" would count a diagnostic's
  *potential*; the button promises what pressing it does. `fix_count` runs
  the batch's own `collect` — the policy's `admits` gate applied, identical
  fixes collapsed — so the number and the action cannot disagree.
- **The Problems panel makes ONE fix query per compile, not one per row.**
  `fix_offers(select)` answers every OFFERED fix of the selection paired
  with its diagnostic's `(path, start, end, code)`, and each row looks
  itself up. A per-row query would be one wasm call per visible diagnostic
  on every render. "Offered" is `FixPolicy::offers` (everything except a
  code the project turned `"off"`), and each entry carries `batchable` —
  `FixPolicy::admits` — so a surface can tell "you may click this" from
  "the batch will take this" without a second query.
- **`fix_all` over wasm leaves the session unchanged.** The loop must
  rewrite sources to re-analyze between rounds (§5), but they are restored
  before it returns and the report carries `files` instead: every path whose
  text changed, with its full new source. That is not cosmetic symmetry with
  `apply_fix` — the studio's apply seam snapshots each file for undo *as it
  writes*, so a session left holding the fixed text would snapshot the fixed
  text and make Undo a no-op.
- **The report's sites carry no offsets.** `Report::applied`'s ranges are
  positions in the revision the round that took them saw, and later rounds
  rewrote that source; resolving them against the current text would report
  positions that never existed. `FixSiteJs` is `{ code, path }`.
- **`apply_fix_at_path` exists because a Problems row names its own file.**
  `apply_fix` reports its result against the *active* file, which for a row
  in an unopened file is the wrong primary.
- **§6.2's app setting is `off | safe | project`, default off, and it
  resolves as a CEILING rather than a tier filter.** "Safe only" is the
  ceiling `"ask"`: at that ceiling a Safe fix keeps its `auto` tier default
  and a Suggested fix — promoted by the project or not — resolves to `ask`
  and is not batched. Both roads go through
  `ProjectConfig::effective_fix_policy`, so the still-tentative ceiling
  relationship stays in one place. The setting lives with the other
  app-scope editor settings (`brink-studio.editor.v1`); an unrecognized
  persisted value lands on off. On-save runs after the editor's text is
  flushed into the session and before the write, and deliberately does NOT
  push an undo entry or a toast of its own — an implicit action on every
  Ctrl-S would make both noise.

## 8. `brink fix` — RULED

Its own subcommand (not a `--fix` flag on `compile`): it needs its own
flags and modes.

    brink fix [PROJECT|ENTRY]          apply the project policy (Safe + promoted) to a fixpoint
      --dry-run                        print the report, write nothing
      --diff [PATH|-]                  emit a `git apply`-able patch (the road `brink ide` already has)
      --suggested E033[,E054]          promote codes for this run
      --code E172                      restrict the selection to these codes
      --max-rounds N                   default 5

Exit status: 0 when the fixpoint is reached, non-zero when the round
cap hit or a fixer failed to discharge its diagnostic (the report names
it).

*As built (#3421, milestone 6):* `PROJECT|ENTRY` is one positional entry
file, exactly `brink compile`'s own addressing — `brink-cli`'s own
`crate::ide::project::Project::load` (already shared by `brink ide`;
`brink_ide` itself has no `Project` type) discovers `brink.toml` from the
entry's directory and follows `INCLUDE`s (or the native module graph); a
bare file with no discovered `brink.toml` is the same code path with an
empty `ProjectConfig`, not a second mode. `--suggested` takes an *optional*
value rather than the sketch's required list: bare, it promotes every
Suggested-max fixer in the registry for this run *except one the project's
`brink.toml` `[fix]` table explicitly sets to `"off"`* — `off` means never
offer or batch a fixer for this code in this project
(`docs/book/src/toolchain/project-config.md` §Fix policy), and a codeless
flag is not the "explicit action" that widens it; `--suggested E025,E080`
names codes explicitly, and naming a code *is* that explicit action, so it
wins over `[fix]` for those codes even over an `"off"` entry — the same
`CLI/API > file > default` precedence (#1005) `-D`/`--warn`/`--allow` follow
over `[lints]`, and exactly `--suggested E033`, §6.2's own sanctioned
widening example (a code-explicit form, not the bare one).
`ProjectConfig::effective_fix_policy`'s `Ask` (`docs/autofix-spec.md` §6.1's
neutral value, returned identically for an absent entry and an explicit
`= "ask"`) is deliberately **not** recorded as a `brink_ide::fix::policy::
FixPolicy` override — doing so would force a Safe-max fixer down to
non-batchable, which the TOML comment's own "absent ⇒ ask: … batchable
(Safe)" rules out; only `Off`/`Auto` become overrides.

One flag beyond the sketch: `--placeholder` lists every `Applicability::
Placeholder` fix available in the selection (code, location, title), on
**stderr** — never stdout, so it can never land inside a `--diff` patch
piped straight to `git apply` — alongside whichever of `--dry-run`/`--diff`/
the default write already ran; never applied, since `FixPolicy::admits`
refuses `Placeholder` unconditionally (§3) however a project's `[fix]` table
is written. It exists so an author (or a CI step driving `--dry-run`) can
see where a hole needs filling by hand without a second invocation. No
fixer registered today (milestone 6) declares `Applicability::Placeholder`,
so this listing has no positive-path test yet — tracked as issue #3456,
alongside the native (`.brink`) write-path fixture gap noted below.

The report itself names `applied`/`skipped_overlap` sites by file path only,
never a line:col: their `FixSite.range` was captured against whichever
round's source was current *then*, and a later round's own edits shift
every offset after it — resolving a stale range against the final source
would print a confidently wrong position. `remaining` (recomputed once,
after the loop, against the session's then-current source) is the only
bucket a line:col is safe to render from.

Out of scope for milestone 6: `resolve_fs_path`'s native (`.brink`)
write-path branch (`crate::ide::project::resolve_fs_path`, `ide/project.rs`)
rejoins a discovered key against `native_source_root(entry)` rather than
treating it as cwd-relative — the branch a nested `.brink` project (a
`brink.toml` above `entry`'s own directory) takes. Every fixture this
milestone ships (`tests/fix/E025`) is `.ink`, so `brink fix`'s own tests
only ever exercise the identity (cwd-relative) branch; there is no `.brink`
sibling fixture proving the write actually lands on the real file rather
than a phantom cwd-relative path. Tracked as issue #3456 alongside the
`--placeholder` coverage gap above.

## 9. First-wave membership

Sorted from the 31 Warning-default codes plus the compat-parity issues
(#3363–#3366); each is its own sub-issue under #3374.

- **Safe**: E014 bare `~` → delete the line; E092 redundant
  `#@public`/`#@private` → delete the directive line (issue #3424,
  `RedundantVisibilityFixer`,
  `crates/internal/brink-ide/src/redundant_visibility_fix.rs`; ink-only —
  a native file's module is always `declared` (defaults `Private`), so
  native's own `pub` mark can never be redundant in practice and this
  diagnostic cannot fire there, but the fixer still checks the dialect
  first since it only ever parses with the ink grammar; also offers
  nothing for a stacked pair of conflicting visibility directives, itself
  also `E093` and ambiguous about which line the diagnostic means);
  E095 self-alias `#@was` → delete;
  E110 `#@effects(…)` → `@[effects(…)]`; E172 tag-channel `#@…` →
  native annotation spelling; E031/E176 over-supplied args → trim
  (issue #3428 — the classic call/divert convention these two codes
  cover binds the **trailing** supplied argument, not the leading one
  "over-supplied args" suggests, so the fixer deletes the **leading**
  excess and keeps the trailing `expected` — see
  `crates/internal/brink-ide/src/arity_trim_fix.rs`'s module doc for the
  empirical proof; withheld outright — no fix offered — when a leading
  argument isn't provably pure, when the call's own return value isn't
  popped in isolation from a larger expression, or when the target
  declares a `ref` parameter); empty choice `* []` → `* ->` (#3365).
- **Suggested**: E026 duplicate list item → delete (changes host state);
  E033 unreachable after divert → delete; E035/E054/E188 shadowing or
  colliding name → rename (rides the rename machinery, cross-file);
  E038/E043 bad doc-comment tag → remove; E063 annotation disagrees
  with inference → rewrite to the inferred type; E165 undeclared markup
  attribute → remove.
- **Placeholder**: E173 required markup attribute missing → add with
  an empty value, caret inside.
- **No fixer** (judgment, not mechanics): E022/E023 duplicates (which
  one wins?), E030, E034, E106/E152, E131, E151 (parked on the
  implicit-`DONE` ruling), E164, E168/E170, E189, E190, E192.
- **Migrated, unchanged in meaning**: the existing E025 add-import,
  E080/E081 creation-site, and value-call trim fixers.

## 10. Not covered

- The **program generator** (#3370) is what will eventually run every
  Safe fixer over generated stories rather than hand fixtures; until it
  lands, §3's obligation is met by curated fixtures.
- **Fix composition** (one fix that applies several fixers' edits at
  once) — the round loop covers the need; no composite fix type.
- A `fix` entry in the **decision-log ↔ spec** sense for each code's
  wording — titles are the fixer's own; the message contract voice pass
  (#3263) applies to them when it lands.
