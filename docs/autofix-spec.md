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
- **wasm DTO** (`@brink-lang/web`): `FixJs { code, title,
  applicability, edits: FileEditJs[], caret? }`. *As built (#3377):*
  `fixes_at` / `fixes_at_doc` return it (offsets are UTF-16 file-absolute,
  the editor boundary convention), and `apply_fix` / `apply_fix_doc` take a
  chosen `FixJs` back and answer the `StructuralResult` shape the studio's
  existing cross-file apply seam already consumes — so a fix reaches the
  buffers by the same road a rename does. `resolve_code_action` stays for
  structural refactors.

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

## 9. First-wave membership

Sorted from the 31 Warning-default codes plus the compat-parity issues
(#3363–#3366); each is its own sub-issue under #3374.

- **Safe**: E014 bare `~` → delete the line; E092 redundant
  `#@public`/`#@private` → delete; E095 self-alias `#@was` → delete;
  E110 `#@effects(…)` → `@[effects(…)]`; E172 tag-channel `#@…` →
  native annotation spelling; E031/E176 over-supplied args → trim
  (the discarded args were already being ignored); empty choice
  `* []` → `* ->` (#3365).
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
