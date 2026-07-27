---
"@brink-lang/web": patch
---

Native `.brink` files can now suppress a diagnostic at the site that
produces it: `@[allow(E151, E014)]` above a declaration silences those codes
for the whole span of that declaration (#1161). Previously `@[allow(…)]` was
an unknown annotation name and hard-failed the compile with E111, so the
only suppression available was the line-scoped `//brink-disable` comment or
the project-wide `[lints]` table.

Three rulings, all observable through `compileProject`:

- **Only warnings are suppressible.** A code whose default severity is
  `Error` is rejected with the new **E154** — matching the `[lints]` table's
  own hard-error exemption, so an annotation can never be used to ship a
  broken artifact. The admission-validator diagnostics are exempt for the
  same reason and because they never route through the suppression filter.
- **A source-level `allow` beats a project-level `deny`.** Suppression runs
  before severity resolution, so `@[allow(E151)]` removes the diagnostic
  even under `[lints] E151 = "deny"` or `deny-warnings = true`. The
  annotation names one declaration; `brink.toml` cannot.
- **A suppression that does nothing is loud.** An unknown or misspelled code
  is the new **E153**; a missing, empty, or non-identifier argument list is
  the new **E155**. One bad argument discards the whole directive.

Ink-dialect behavior and the oracle corpus are unaffected — the `allow`
tenant exists only on the native `@[…]` channel. The E111 and E112
diagnostic *messages* changed to name `allow` alongside the existing names.
