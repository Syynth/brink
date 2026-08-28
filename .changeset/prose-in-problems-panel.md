---
"@brink-lang/studio": patch
"@brink-lang/editor": patch
---

Spelling and grammar findings now appear in the Problems panel, behind a
filter toggle that is **off by default**.

This completes behaviour that was specified when prose checking was first
scoped — results "render as squiggles and are listable, but the Problems
panel filters them out by default; the author opts in to seeing them in the
list". Only the squiggles half had shipped, so a typo was visible in the
buffer and findable nowhere else.

- A fourth filter bucket, `prose`, sits beside error/warning/info. It is a
  SOURCE rather than a severity, which is what lets it default off while
  every severity defaults on — folding spelling into `info` would bury the
  E189 TODO notes an author actually reads.
- Prose findings are stored separately from compile diagnostics and joined
  for display. The two producers have different lifetimes — a compile
  replaces its whole set at once, prose lints arrive per view on their own
  debounce — so one list would mean each erasing the other's rows.
- A prose row's context menu offers **Prose settings…** rather than
  "Configure <code>…", which would have opened the Diagnostics section and
  offered nothing about it.

An existing author's stored preferences have no `prose` key, and it reads as
off: the severity rule ("only an explicit false hides it") is deliberately
inverted for this bucket, so upgrading never switches spelling rows on.

`@brink-lang/editor` gains an `onProseLints` document-session callback
reporting findings per file, fired from the same guarded point as the
squiggles so a host list can never hold rows the editor has cleared.
