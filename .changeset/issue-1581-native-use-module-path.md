---
"@brink-lang/web": patch
---

Fix (#1581): a native `use story::market::barter::haggle;` now names a real
module, so qualified-import matching can succeed at all.

`use` lowering built `Import.module` by joining the path with `.` **and**
keeping the leaf segment — `story.market.barter.haggle` — while the module it
names is `story::market::barter` (`::`-joined, no leaf). Two independent
mismatches, so the string could never equal a real module name: every
`ImportScope`/`import_covers` match failed, cross-file references fell through
to the bare-name fallback (which picks a flat first-winner when two modules
export the same name), and a correctly imported public symbol was still
reported as needing an import (`E025`).

Now the leaf is the imported *item* and the prefix is its `::`-joined module,
matching the module names `native_module_path` mints. Editor-visible
consequences: `use a::b as c;` — previously rejected as an unrepresentable
module alias (`E129`) — is an ordinary aliased item import; a `use` of the
file's own module is now recognized as a self-import (`E090`); and a
reference imported from a declared module resolves to *that* module rather
than to whichever homonym happened to be indexed first. A single-segment
`use module;` still names the module itself (the qualified form), as does
`import a::b;`, whose path is now `::`-joined too.
