# Auto-fix

The studio can fix a diagnostic for you instead of asking you to hand-edit
the source. This page is the author-facing tour; the full design (the
`Fixer` model, the batching algorithm, the policy layering) is
[`docs/autofix-spec.md`](https://github.com/Syynth/brink/blob/main/docs/autofix-spec.md),
and the command-line equivalent is [`brink fix`](../../toolchain/cli/fix.md).

## What a fix is

A **fix** is a small, targeted edit that discharges one diagnostic — the
same kind of change you'd make by hand, computed for you. Diagnostics stay
exactly what they always were (a squiggle, a Problems-panel row); a fix is
just an optional extra action attached to one.

Every fix is labeled with a **tier**, so you know how far it's allowed to go
before you click it:

- **Safe** — the result behaves exactly the same as before; only the
  redundant or already-inert text changes. Safe fixes are the ones "Fix all
  safe" batches for you without asking case by case.
- **Suggested** — probably what you meant, but it changes what the story
  does or removes text you wrote (for example, trimming an extra argument
  from a call). Each one is a deliberate, one-at-a-time click unless your
  project has explicitly promoted that diagnostic code to batch
  automatically (see [Policy](#policy-the-fix-table-in-brinktoml) below).
- **Placeholder** — the fix can't complete the thought for you; it removes
  the ambiguity and drops your cursor where you still need to type
  something. Placeholder fixes are never batched.

A fix never appears for a diagnostic it can't actually clear, and applying
one always re-runs analysis — if a fix doesn't make its own diagnostic go
away, that's treated as a bug in the fixer, not something the studio hides.

## Where fixes appear

All of these are different doors onto the same underlying fix — pressing
"Fix" on a Problems row and picking the identical entry from the editor's
context menu produce the same edit.

- **Problems panel** — a row with a fix available shows a **Fix** button
  (labeled with its tier) beside the diagnostic; right-clicking a row lists
  every fix offered for it alongside the existing suppress actions. The
  panel's header shows a **Fix all safe (N)** button once at least one Safe
  fix is available anywhere in the project — `N` is an exact count, not an
  estimate, so the button never promises more than clicking it will do.
- **Editor context menu** — right-click a squiggle to get the same fix
  entries (each still labeled with its tier) for the diagnostic under the
  pointer, plus a trailing **Fix all safe in this file** entry that only
  appears alongside at least one offered fix.
- **Code-actions menu** — the lightbulb-style menu at the cursor lists the
  fixes for diagnostics whose own range covers the cursor position (not just
  any diagnostic nearby); choosing one applies it immediately, and a
  Placeholder fix moves your cursor into the hole it left.
- **Command palette** — "Fix: Fix all safe in project" and "Fix: Fix all
  safe in this file" run the same Safe-tier batch as the Problems panel's
  header button, scoped to the whole project or to whichever file is
  focused.
- **Fix on save** — Settings ▸ Saving ▸ **Fix on save** applies fixes
  automatically each time you save a file: **Off**, **Safe fixes only**, or
  **Everything the project allows** (which also includes any Suggested-tier
  code your project's `brink.toml` has promoted to `"auto"`, see
  [Policy](#policy-the-fix-table-in-brinktoml)). This setting is a personal
  ceiling, not a project-wide switch — it can only make what happens on save
  *more* conservative than what the project allows, never less; a project
  promoting a code to `"auto"` doesn't force it onto an author who has
  chosen "Safe fixes only".
- **Other editors, via the LSP** — `brink-lsp` offers each fix as a
  standard quickfix code action tied to its diagnostic, plus the
  `source.fixAll.brink` action that VS Code (and any client supporting
  fix-on-save code actions) runs automatically on save — the same Safe-tier
  batch the studio's own on-save setting runs.

## Policy: the `[fix]` table in `brink.toml`

A project can adjust the default tier behavior per diagnostic code with a
`[fix]` table:

```toml
[fix]
E033 = "auto"   # promote a Suggested fix to batch automatically in this project
E014 = "off"    # never offer this fixer here
# a code left out of the table keeps its tier's default:
#   Safe -> batched automatically, Suggested -> offered per click, Placeholder -> never batched
```

The three values:

| Value | Meaning |
|-------|---------|
| `"auto"` | Batch this code's fixes automatically wherever a batch runs — this is how a Suggested-tier fixer gets included in "Fix all safe" and fix-on-save. (A Placeholder-tier code can never be batched, no matter what the table says.) |
| `"ask"` | The default: offered as a one-click fix, but not swept up by a batch unless it's already Safe-tier. |
| `"off"` | Withdraw the fix entirely — it stops being offered on any surface, even a single click. |

`[fix]` travels with the project like `[lints]` does, so the CLI, the LSP,
and the studio all read the same policy. In Settings, the same lint table
that shows each diagnostic's severity also carries a **Fix** column with
these three values — editing it writes straight into `brink.toml`'s `[fix]`
table, the same way changing a severity writes into `[lints]`. A code can
have a `[fix]` entry independently of whether it's configurable in
`[lints]`; the two tables are keyed by the same diagnostic code but are
otherwise unrelated.

## Today's Safe fixers

These are the diagnostic codes that currently ship a **Safe**-tier fixer —
the ones "Fix all safe", fix-on-save, and `source.fixAll.brink` will batch
without asking. (Suggested- and Placeholder-tier fixers exist too — E025,
E063, E080, and E081 today — but a one-at-a-time click is expected for
those, so they're left out of this list; see `docs/autofix-spec.md` §9 for
the full registry.)

| Code | What it fixes |
|------|----------------|
| `E014` | Removes an effect-free `~` logic line — one that does nothing at all. |
| `E031` | Trims a function call's extra arguments down to the number its declaration actually accepts. |
| `E092` | Removes a redundant `#@public`/`#@private` directive that only restates the module's own default. |
| `E095` | Removes a stale `#@was` tag that already names the definition's current name — there's nothing left to migrate. |
| `E110` | Rewrites the deprecated `#@effects(…)` directive spelling to the current `@[effects(…)]` annotation. |
| `E176` | Trims a divert-with-args site's (`-> knot(args)`, tunnel call, thread-start) extra arguments down to its resolved target's declared parameter count — `E031`'s sibling for the divert-call shape. |

This list will grow as more codes get Safe fixers; it's read out of the
`FIXERS` registry in `crates/internal/brink-ide/src/fix.rs` on `main`, so
check there (or `brink fix --dry-run` on your own project) for the current
truth rather than trusting a page that can drift.
