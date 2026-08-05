---
"@brink-lang/web": patch
---

Issue #2289 (`docs/decision-log.md` 2026-08-05 "Conventions are
PROJECT-WIDE by definition…"): corrects two defects in the §9.1 conventions
confinement ruling that had drifted apart since #1844.

- **A conventions module's `@[convention]` handlers now claim prose across
  the WHOLE PROJECT, not just their own declaring file.** Before this fix,
  a correctly-configured `[project] conventions` module claimed nothing
  outside the one file that declared its handlers — reach was silently
  file-local despite the confinement rule's whole purpose being to
  centralize conventions for the entire project.
- **A `@[convention]` handler declared with `[project] conventions` entirely
  unset is now `E169`, not a silent pass.** A claiming handler with no
  configured module names no module for the declaration to belong to, so it
  is a misconfiguration rather than an opt-out.

Both changes are observable through `@brink-lang/web`: a `.brink` project
compiled with a conventions module configured will now see prose in every
project file matched against that module's handlers (previously only the
declaring file's own prose was matched), and a project that declares
`@[convention]` handlers without configuring `[project] conventions` will
now fail to compile with `E169` where it previously compiled silently.
