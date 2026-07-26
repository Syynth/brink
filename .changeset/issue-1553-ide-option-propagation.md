---
"@brink-lang/web": patch
---

Fix (#1553): the editor session's registered options now reach its own
project database, cross-file hover names the defining file, and module
stem-collision errors (`E085`) surface in the editor.

Three web-observable gaps, all the same class — the editor path silently
seeing different analysis inputs than a real compile:

- **Options never reached the database.** The editor session runs its live
  analysis off-database, but many features read database queries directly
  (per-file diagnostics, the symbol index, effect rows, inferred types), and
  those are gated on the options *input*. Only `compile()` ever wrote it, so
  a session that never compiled read every one of those under the defaults:
  the declared dialect, typed-mode policy, `[lints]` table and host manifest
  were silently absent. Cross-module duplicate coexistence (`brink`-only)
  and the native strict-only check (`E137`) were gated off outright. Every
  option setter now writes the session's options through.

- **Cross-file hover text could never render.** `hover`/`hover_doc` passed
  only the hovered file to the lookup set, so a definition in *another* file
  was never found and the ``*Defined in `path`*`` note was always dropped.
  The whole project is now in scope, matching the LSP.

- **Stem collisions were dropped in the editor.** `E085` (a file with no
  `#@module` whose stem is another file's declared module name) is produced
  by the project database's module resolution, not by the analyzer pass the
  editor runs, so a collision a compile catches never reached the editor.
  It is now folded back into the editor's analysis and the LSP's background
  pass.
