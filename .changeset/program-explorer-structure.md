---
"@brink-lang/studio": patch
---

The Program Explorer's new shell and Structure view

The Program Explorer becomes one instrument with a view switch (Structure
· Line tables · Disassembly · Size — the last three disabled slots until
their phases land, each naming where its view is). The program reads as a
named thing: entry-file stem, status dot, checksum chip, and counts
replace the bare hex toolbar.

Structure: knot rows carry size at a glance — a bar of bytecode with a
lines fill inside it, on a shared scale, with per-row byte/line/container
counts rolled up from the knot's whole subtree. The definitions column
groups globals, lists, and externals, and each external states its
contract: a `fallback` body the story can run on, or `host` — a binding
the host must register. A footer totals the program; while paused, it
names the executing container the way a save file would.

The existing behavior contract is untouched: expansion, the
current-instruction and reveal-target highlights, and the stepi actions
all work exactly as before, pinned by the same tests.
