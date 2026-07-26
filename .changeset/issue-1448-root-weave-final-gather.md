---
"@brink-lang/web": patch
---

Fix #1448: a story whose **root weave** ran out of content faulted with
"ran out of content. Do you need a '-> DONE' or '-> END'?" instead of
ending its turn. inklecate appends an implicit level-1 gather plus
`-> DONE` to the root weave (`FlowBase.cs:69-72`); brink only had the
root container's own trailing `Done`, which a gather can never reach —
a gather is entered by `goto`, which clears the container stack.

LIR lowering now synthesizes that terminus (a `g-final` gather holding a
single `-> DONE`) and diverts the root weave's outermost loose end into
it. Root scope only: a knot, stitch, tunnel, or function that runs out of
content is a genuine authoring error and keeps reporting one, matching
C# ink.

Playground/editor stories written without a trailing `-> DONE`/`-> END`
after a root-level weave now end cleanly. Oracle conformance: 5,577 →
5,598 passing episodes, 350 → 358 passing cases.
