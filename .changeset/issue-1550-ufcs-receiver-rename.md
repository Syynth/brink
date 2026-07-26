---
"@brink-lang/web": patch
---

Analyzer: fix UFCS rename corrupting `receiver.method(...)` call sites
(issue #1550, the mirror of #1539).

`resolve::resolve_function`'s UFCS-shaped-callee fallback recorded the
resolved reference for a `recv.verb(args)` call site's *receiver* spanning
the whole `recv.verb` path. Renaming just the receiver (e.g. `g` in
`g.greet(3)`) therefore rewrote the entire path, silently dropping the
method segment and producing a broken program (`newname(3)` instead of
`newname.greet(3)`) from what looked like a safe rename.

The receiver's resolved reference now spans only its own segment.
`ufcs::UfcsVisitor::value_receiver_def` (the D2 UFCS pass, which keys off
the same reference to type the receiver) is updated to key its lookup on
that same narrowed range.
