---
"@brink-lang/web": patch
---

Fix (#1358): editor analysis now judges native `.brink` source by the native
rule set, so two native modules can declare a same-named flow without one
disappearing from the editor's symbol index.

The editor session analyzes off the project database, through an analyzer
entry point that has no file paths and so cannot tell native source from ink
— the session has to declare it, and it never did. Everything downstream of
that ran the ink arm over native files. The consequence a wasm consumer can
observe: a native project's module is its path and is always declared, so
two modules declaring the same flow name were treated as a duplicate
definition, the later one was dropped from the index with a
`duplicate knot definition` warning, and every feature keyed off that index
— hover, go-to-definition, completion, the story graph — missed it. Both now
coexist, each resolving through the module its file imported.

The same declaration also selects the native arm of the diagnostic passes,
which is what the language server publishes as inline squiggles: a `.brink`
file no longer reports its ordinary syntax (struct declarations,
construction literals, type annotations, multi-line logic blocks) as
`E051` "brink extension" errors, no longer reports `E064` when a project
dials `types = strict` (the only policy native has), and now does report
`E137` when a project explicitly dials `types = gradual`, which native
source cannot compile under.
