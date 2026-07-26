---
"@brink-lang/web": patch
---

IDE: fix UFCS rename/find-references corrupting `receiver.method(...)` call
sites (issue #1550, the mirror of #1539).

`resolve::resolve_function`'s UFCS-shaped-callee fallback records the
resolved reference for a `recv.verb(args)` call site's *receiver* spanning
the whole `recv.verb` path (this is intentional — the D2 UFCS pass keys off
that same whole-path range to type the receiver). `rename`'s and
`find_references`' plain-reference loops used that range directly, so
renaming just the receiver (e.g. `g` in `g.greet(3)`) rewrote the entire
path, silently dropping the method segment and producing a broken program
(`newname(3)` instead of `newname.greet(3)`) from what looked like a safe
rename.

`brink-ide::rename` and `brink-ide::navigation::find_references` now narrow
a UFCS receiver's reported reference/edit range down to the receiver's own
first segment (via a new `ufcs_hover::ufcs_receiver_head_range_at_path`,
mirroring the method-segment narrowing issue #1539 already added) before
emitting it.
