---
"@brink-lang/web": patch
---

IDE: rename writes `#@was` automatically, and undeclared renames get an
authoring-time hint (issue #1672).

`docs/modules-spec.md` §5 rules that the IDE's rename refactor writes the
`#@was(old_name)` migration directive automatically — this was never
implemented. `rename`/`rename_safe` (the single chokepoint every rename
surface funnels through — CLI, LSP, and the studio/web editor) now stamps
`#@was` onto a renamed knot, stitch, `VAR`, `CONST`, or `LIST`'s declaration,
in the same edit set as the rename itself. Only under `dialect = brink`
(`#@was` is itself a brink extension; stamping it under strict ink would
introduce a fresh `E051` on every rename), and never over an existing
`#@was` (a second rename of an already-migrated declaration keeps its
original record).

New, separate: `brink_ide::rename_detection::detect_undeclared_renames`
diffs a file's current declared-symbol manifest against a previous one and
reports an unambiguous 1:1 rename shape (one name vanished, one same-kind
name appeared, unambiguously) as a `RenameSuspicion` — the authoring-time
detection for a rename that never went through the refactor (a hand edit, a
`sed`, a merge). Wired into `brink-lsp`: a `DiagnosticSeverity::HINT`
diagnostic asks the author directly ("`hub` disappeared and `plaza`
appeared — did you rename it?") rather than guessing.
